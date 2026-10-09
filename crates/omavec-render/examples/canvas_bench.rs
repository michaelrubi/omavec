//! Phase 0 spike: which renderer draws the canvas?
//!
//! Draws the same 10,000 random cubic blobs with vello on the GPU (into a
//! texture, as it would for egui) and with `vello_cpu`, at three zooms, and
//! prints the time per frame. No window is opened.
//!
//!     cargo run --release -p omavec-render --example canvas_bench
//!
//! With `DUMP=dir` it also writes each renderer's frame as a PNG, to check by
//! eye that they draw the same thing.

use std::future::Future;
use std::time::{Duration, Instant};

use kurbo::{Affine, Rect};
use omavec_render::Item;
use omavec_render::spike::{DOCUMENT, blobs};
use peniko::{Color, Fill};
use vello::wgpu;

const WIDTH: u16 = 2560;
const HEIGHT: u16 = 1440;
const ZOOMS: [f64; 3] = [1.0, 64.0, 0.05];
const FRAMES: usize = 30;

/// The middle of the document in the middle of the view.
fn view(zoom: f64) -> Affine {
    Affine::translate((f64::from(WIDTH) / 2.0, f64::from(HEIGHT) / 2.0))
        * Affine::scale(zoom)
        * Affine::translate((-DOCUMENT / 2.0, -DOCUMENT / 2.0))
}

fn visible(blob: &Item, view: Affine, screen: Rect) -> bool {
    !view.transform_rect_bbox(blob.bounds()).intersect(screen).is_zero_area()
}

fn median(mut times: Vec<Duration>) -> f64 {
    times.sort();
    times[times.len() / 2].as_secs_f64() * 1000.0
}

/// The median time of `frame` over [`FRAMES`] runs, after three to warm up.
fn time(mut frame: impl FnMut()) -> f64 {
    for _ in 0..3 {
        frame();
    }
    median((0..FRAMES).map(|_| {
        let start = Instant::now();
        frame();
        start.elapsed()
    }).collect())
}

/// With `DUMP=dir`, writes a frame there as a PNG. `vello_cpu`'s pixels are
/// premultiplied; vello's fine shader writes straight alpha.
fn dump(name: &str, rgba: Vec<u8>, alpha: vello_cpu::peniko::ImageAlphaType) {
    let Some(dir) = std::env::var_os("DUMP") else { return };
    let metadata = vello_cpu::PixelMetadata::new(alpha, true);
    let pixmap = vello_cpu::Pixmap::from_parts(rgba, WIDTH, HEIGHT, metadata);
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    std::fs::write(&path, pixmap.into_png().expect("encode png")).expect("write png");
}

/// `vello_cpu` has no retained scene: every frame feeds it every path again.
/// `cull` skips the paths outside the view first, as a canvas would.
fn cpu(blobs: &[Item], threads: u16, cull: bool) {
    let settings = vello_cpu::RenderSettings { num_threads: threads, ..Default::default() };
    let mut context = vello_cpu::RenderContext::new_with(WIDTH, HEIGHT, settings);
    let mut resources = vello_cpu::Resources::new();
    let mut pixmap = vello_cpu::Pixmap::new(WIDTH, HEIGHT);
    let screen = Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT));
    for zoom in ZOOMS {
        let transform = view(zoom);
        let mut drawn = 0;
        let ms = time(|| {
            drawn = 0;
            context.reset();
            context.set_transform(transform);
            for blob in blobs {
                if cull && !visible(blob, transform, screen) {
                    continue;
                }
                drawn += 1;
                context.set_paint(blob.color);
                context.fill_path(&blob.path);
            }
            context.flush();
            pixmap.data_as_u8_slice_mut().fill(0);
            context.render(&mut pixmap, &mut resources);
        });
        let cull = if cull { "culled" } else { "all paths" };
        println!("vello_cpu, {threads:>2} threads, {cull:<9} | {zoom:>5}x | {drawn:>6} drawn | {ms:>7.2} ms");
        dump(&format!("cpu-{threads}-{}-{zoom}x", &cull[..3]), pixmap.data_as_u8_slice().to_vec(), vello_cpu::peniko::ImageAlphaType::AlphaPremultiplied);
    }
}

/// vello keeps the encoded scene: a frame appends it under the view
/// transform and the GPU does the rest, off-screen paths included.
async fn gpu(blobs: &[Item], power: wgpu::PowerPreference) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let options = wgpu::RequestAdapterOptions { power_preference: power, ..Default::default() };
    let Ok(adapter) = instance.request_adapter(&options).await else {
        println!("vello: no {power:?} adapter");
        return;
    };
    let name = adapter.get_info().name;
    let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default()).await.expect("device");
    let started = Instant::now();
    let mut renderer = vello::Renderer::new(&device, vello::RendererOptions {
        antialiasing_support: vello::AaSupport::area_only(),
        ..Default::default()
    })
    .expect("vello renderer");
    println!("vello on {name}: shaders ready in {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);

    let size = wgpu::Extent3d { width: WIDTH.into(), height: HEIGHT.into(), depth_or_array_layers: 1 };
    upload(&device, &queue, &name, size);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("canvas"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let params = vello::RenderParams {
        base_color: Color::TRANSPARENT,
        width: WIDTH.into(),
        height: HEIGHT.into(),
        antialiasing_method: vello::AaConfig::Area,
    };

    let started = Instant::now();
    let mut document = vello::Scene::new();
    for blob in blobs {
        document.fill(Fill::NonZero, Affine::IDENTITY, blob.color, None, &blob.path);
    }
    println!("vello on {name}: scene encoded once in {:.2} ms", started.elapsed().as_secs_f64() * 1000.0);

    let screen = Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT));
    let mut scene = vello::Scene::new();
    for cull in [false, true] {
        for zoom in ZOOMS {
            let transform = view(zoom);
            let mut drawn = blobs.len();
            let ms = time(|| {
                scene.reset();
                if cull {
                    // Encoding again is cheap; the GPU never sees the rest.
                    drawn = 0;
                    for blob in blobs.iter().filter(|blob| visible(blob, transform, screen)) {
                        drawn += 1;
                        scene.fill(Fill::NonZero, transform, blob.color, None, &blob.path);
                    }
                } else {
                    scene.append(&document, Some(transform));
                }
                renderer.render_to_texture(&device, &queue, &scene, &target, &params).expect("render");
                // Until the GPU has finished, the frame isn't on screen.
                device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
            });
            let label = if cull { "culled" } else { "all paths" };
            let overflow = overflow(&mut renderer, &device, &queue, &scene, &target, &params);
            println!("vello on {name}, {label:<9} | {zoom:>5}x | {drawn:>6} drawn | {ms:>7.2} ms{overflow}");
            if std::env::var_os("DUMP").is_some() {
                dump(&format!("gpu-{power:?}-{}-{zoom}x", &label[..3]), read_back(&device, &queue, &texture, size), vello_cpu::peniko::ImageAlphaType::Alpha);
            }
        }
    }
}

/// What the `vello_cpu` canvas pays on top of drawing: its frame goes to the
/// GPU as a texture every time it changes.
fn upload(device: &wgpu::Device, queue: &wgpu::Queue, name: &str, size: wgpu::Extent3d) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cpu frame"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let pixels = vec![127u8; size.width as usize * size.height as usize * 4];
    let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.width * 4), rows_per_image: None };
    let ms = time(|| {
        queue.write_texture(texture.as_image_copy(), &pixels, layout, size);
        queue.submit([]);
        device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
    });
    println!("upload of a {}x{} frame to {name}: {ms:.2} ms", size.width, size.height);
}

/// vello's GPU buffers have fixed sizes (`vello_encoding::BufferSizes`). A
/// scene that needs more is drawn wrong or not at all, and
/// `render_to_texture` doesn't say so; only the deprecated async call
/// reports what the frame needed.
fn overflow(
    renderer: &mut vello::Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &vello::Scene,
    target: &wgpu::TextureView,
    params: &vello::RenderParams,
) -> String {
    // The future waits on a buffer mapping, which only a device poll completes.
    let render = || {
        #[expect(deprecated, reason = "the only call that returns the bump allocators")]
        let mut render = std::pin::pin!(renderer.render_to_texture_async(device, queue, scene, target, params, vello::low_level::DebugLayers::none()));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(bump) = render.as_mut().poll(&mut context) {
                break bump.expect("render");
            }
            device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        }
    };
    // When the line buffer overflows, vello's own debug download slices past
    // its end and panics.
    let Ok(bump) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(render)) else {
        return " | WRONG FRAME, vello panicked reading its buffers back".into();
    };
    let bump = bump.expect("vello's debug_layers feature");
    if bump.failed == 0 {
        return String::new();
    }
    let needed = [
        ("binning", bump.binning, 1u32 << 18),
        ("ptcl", bump.ptcl, 1 << 23),
        ("tiles", bump.tile, 1 << 21),
        ("seg counts", bump.seg_counts, 1 << 21),
        ("segments", bump.segments, 1 << 21),
        ("lines", bump.lines, 1 << 21),
    ];
    let over: Vec<String> = needed
        .iter()
        .filter(|(_, needed, size)| needed > size)
        .map(|(name, needed, size)| format!("{name} {:.1}x", f64::from(*needed) / f64::from(*size)))
        .collect();
    format!(" | WRONG FRAME, buffers too small: {}", over.join(", "))
}

fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture, size: wgpu::Extent3d) -> Vec<u8> {
    let bytes = u64::from(size.width) * u64::from(size.height) * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            // 2560 × 4 is already a multiple of the 256-byte row alignment.
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.width * 4), rows_per_image: None },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |result| result.expect("map"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
    buffer.slice(..).get_mapped_range().expect("mapped").to_vec()
}

fn main() {
    let count = std::env::args().nth(1).and_then(|n| n.parse().ok()).unwrap_or(10_000);
    let blobs = blobs(count).items;
    println!("{count} cubic blobs over {DOCUMENT} units, drawn at {WIDTH}x{HEIGHT}\n");
    let threads = vello_cpu::RenderSettings::default().num_threads;
    cpu(&blobs, threads, true);
    cpu(&blobs, threads, false);
    cpu(&blobs, 0, true);
    println!();
    pollster::block_on(gpu(&blobs, wgpu::PowerPreference::LowPower));
    println!();
    pollster::block_on(gpu(&blobs, wgpu::PowerPreference::HighPerformance));
}
