//! Phase 0 spike: drop shadows and layer blurs on an arbitrary path.
//!
//! `vello_cpu` has blur and drop-shadow filter layers, but only on a
//! single-threaded context: a multithreaded one panics. So, as VectorCraft
//! does (`crates/render/src/fx.rs`, `with_filters`), each effect is drawn on
//! its own small single-threaded context, cropped to what the effect can
//! reach, and the result is drawn into the canvas as an image.
//!
//!     cargo run --release -p omavec-render --example effects
//!
//! Prints the cost of one effect per frame, and with `DUMP=dir` writes the
//! frame as `effects.png`.

use std::sync::Arc;
use std::time::Instant;

use kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use peniko::Color;
use vello_cpu::filter_effects::{EdgeMode, Filter, FilterPrimitive};
use vello_cpu::{Image, ImageSource, Pixmap, RenderContext, RenderSettings, Resources};

const WIDTH: u16 = 2560;
const HEIGHT: u16 = 1440;

/// A five-pointed star with curved sides: nothing a rounded rectangle's
/// shortcut could draw.
fn star(centre: Point, radius: f64) -> BezPath {
    let point = |i: usize, r: f64| centre + Vec2::from_angle(std::f64::consts::TAU * i as f64 / 10.0 - std::f64::consts::FRAC_PI_2) * r;
    let mut path = BezPath::new();
    path.move_to(point(0, radius));
    for i in (0..10).step_by(2) {
        path.quad_to(point(i + 1, radius * 0.3), point(i + 2, radius));
    }
    path.close_path();
    path
}

/// Draws `draw` inside `filter` on a single-threaded context just big enough
/// for `reach` grown by `spread`, and returns the pixels and where they go.
fn filtered(resources: &mut Resources, reach: Rect, spread: f64, filter: Filter, draw: impl FnOnce(&mut RenderContext)) -> (Arc<Pixmap>, Point) {
    let screen = Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT));
    let area = reach.inflate(spread, spread).intersect(screen).expand();
    let (width, height) = (area.width() as u16, area.height() as u16);
    let mut context = RenderContext::new_with(width, height, RenderSettings { num_threads: 0, ..Default::default() });
    context.set_transform(Affine::translate(-area.origin().to_vec2()));
    context.push_filter_layer(filter);
    draw(&mut context);
    context.pop_layer();
    context.flush();
    let mut pixmap = Pixmap::new(width, height);
    context.render(&mut pixmap, resources);
    (Arc::new(pixmap), area.origin())
}

fn draw_pixmap(context: &mut RenderContext, pixmap: Arc<Pixmap>, at: Point) {
    let size = Rect::new(0.0, 0.0, f64::from(pixmap.width()), f64::from(pixmap.height()));
    context.set_transform(Affine::translate(at.to_vec2()));
    context.set_paint(Image { image: ImageSource::Pixmap(pixmap), sampler: Default::default() });
    context.fill_rect(&size);
    context.set_transform(Affine::IDENTITY);
}

fn median_ms(mut run: impl FnMut()) -> f64 {
    run();
    let mut times: Vec<_> = (0..20).map(|_| {
        let start = Instant::now();
        run();
        start.elapsed()
    }).collect();
    times.sort();
    times[times.len() / 2].as_secs_f64() * 1000.0
}

fn main() {
    let mut resources = Resources::new();
    let mut canvas = RenderContext::new(WIDTH, HEIGHT);
    canvas.set_paint(Color::from_rgb8(0xf2, 0xf2, 0xf2));
    canvas.fill_rect(&Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT)));

    let fill = Color::from_rgb8(0x89, 0xb4, 0xfa);
    let shadow_color = Color::from_rgba8(0, 0, 0, 110);
    println!("effect on a star            | shape px | sigma | offscreen px | ms");
    // The second row is only timed: stars that size would cover the frame.
    for (row, radius) in [150.0, 600.0].into_iter().enumerate() {
        let y = 720.0;
        for (column, sigma) in [4.0f32, 16.0, 64.0].into_iter().enumerate() {
            // The blur reaches about three standard deviations.
            let spread = f64::from(sigma) * 3.0 + 2.0;

            let path = star(Point::new(250.0 + 420.0 * column as f64, y), radius);
            let shadow = FilterPrimitive::DropShadowOnly { dx: 0.0, dy: sigma / 2.0, std_deviation: sigma, color: shadow_color, edge_mode: EdgeMode::None };
            let reach = path.bounding_box() + Vec2::new(0.0, f64::from(sigma) / 2.0);
            let mut drawn = None;
            let ms = median_ms(|| {
                drawn = Some(filtered(&mut resources, reach, spread, Filter::from_primitive(shadow.clone()), |context| {
                    context.set_paint(Color::BLACK);
                    context.fill_path(&path);
                }));
            });
            let (pixmap, at) = drawn.take().expect("ran");
            println!("drop shadow                 | {:>8} | {sigma:>5} | {:>5}x{:<6} | {ms:.2}", radius * 2.0, pixmap.width(), pixmap.height());
            if row == 0 {
                draw_pixmap(&mut canvas, pixmap, at);
                canvas.set_paint(fill);
                canvas.fill_path(&path);
            }

            let path = star(Point::new(1530.0 + 420.0 * column as f64, y), radius);
            let blur = FilterPrimitive::GaussianBlur { std_deviation: sigma, edge_mode: EdgeMode::None };
            let ms = median_ms(|| {
                drawn = Some(filtered(&mut resources, path.bounding_box(), spread, Filter::from_primitive(blur.clone()), |context| {
                    context.set_paint(fill);
                    context.fill_path(&path);
                }));
            });
            let (pixmap, at) = drawn.take().expect("ran");
            println!("layer blur                  | {:>8} | {sigma:>5} | {:>5}x{:<6} | {ms:.2}", radius * 2.0, pixmap.width(), pixmap.height());
            if row == 0 {
                draw_pixmap(&mut canvas, pixmap, at);
            }
        }
    }

    // The same shadow drawn the cheap way, for rounded rectangles only: no
    // filter layer, and it works on the multithreaded canvas.
    let card = Rect::from_center_size((1280.0, 1200.0), (600.0, 160.0));
    canvas.set_paint(shadow_color);
    canvas.fill_blurred_rounded_rect(&(card + Vec2::new(0.0, 8.0)), 16.0, 16.0, false);
    canvas.set_paint(fill);
    canvas.fill_path(&card.to_rounded_rect(16.0).to_path(0.1));

    canvas.flush();
    let mut frame = Pixmap::new(WIDTH, HEIGHT);
    let start = Instant::now();
    canvas.render(&mut frame, &mut resources);
    println!("\nframe with six effects as images and one blurred rounded rectangle: {:.2} ms", start.elapsed().as_secs_f64() * 1000.0);
    if let Some(dir) = std::env::var_os("DUMP") {
        let path = std::path::Path::new(&dir).join("effects.png");
        std::fs::write(path, frame.into_png().expect("encode png")).expect("write png");
    }
}
