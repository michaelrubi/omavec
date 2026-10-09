//! The canvas: the document drawn by `omavec-render` on a worker thread, and
//! panned and zoomed as in Figma.
//!
//! Drawing happens off the UI thread, so a heavy document never stalls the
//! interface. While a new frame is on its way the last one is shown moved and
//! scaled to where it now belongs, as VectorCraft's canvas does.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use egui::{Color32, ColorImage, Key, PointerButton, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui};
use omavec_geom::kurbo::{Affine, Point, Vec2};
use omavec_render::{DisplayList, Renderer, peniko};

use crate::rulers;
use crate::theme::Theme;

/// Figma's zoom range: 2% to 25,600%.
const ZOOM_RANGE: (f64, f64) = (0.02, 256.0);

/// Where the document sits in the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The document's origin, in points from the canvas's top-left corner.
    pub origin: Vec2,
    /// Points per document unit.
    pub zoom: f64,
}

impl Default for View {
    fn default() -> Self {
        Self { origin: Vec2::ZERO, zoom: 1.0 }
    }
}

impl View {
    /// Document units to pixels of a frame drawn at `pixels_per_point`.
    fn to_pixels(self, pixels_per_point: f32) -> Affine {
        Affine::scale(f64::from(pixels_per_point)) * Affine::translate(self.origin) * Affine::scale(self.zoom)
    }

    /// Zooms by `factor`, keeping the document point under `anchor` (points
    /// from the canvas's corner) where it is.
    pub fn zoom_about(&mut self, anchor: Vec2, factor: f64) {
        let zoom = (self.zoom * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        self.origin = anchor + (self.origin - anchor) * (zoom / self.zoom);
        self.zoom = zoom;
    }

    /// Where a point of a frame drawn for this view belongs under `now`.
    fn reproject(self, now: View, point: Vec2) -> Vec2 {
        now.origin + (point - self.origin) * (now.zoom / self.zoom)
    }
}

struct Request {
    ctx: egui::Context,
    list: Arc<DisplayList>,
    view: View,
    size: [u16; 2],
    pixels_per_point: f32,
    backdrop: Color32,
}

struct Drawn {
    image: ColorImage,
    view: View,
    took: Duration,
}

/// Starts the thread that draws frames. It ends when the canvas is dropped.
fn worker() -> (Sender<Request>, Receiver<Drawn>) {
    let (requests, queue) = channel::<Request>();
    let (frames, drawn) = channel();
    let spawned = std::thread::Builder::new()
        .name("canvas".into())
        .spawn(move || {
            let mut renderer = Renderer::default();
            while let Ok(mut request) = queue.recv() {
                // Only the newest view is worth drawing.
                while let Ok(newer) = queue.try_recv() {
                    request = newer;
                }
                let start = Instant::now();
                let [width, height] = request.size;
                let [r, g, b, _] = request.backdrop.to_array();
                let view = request.view.to_pixels(request.pixels_per_point);
                let frame = renderer.render(&request.list, view, width, height, peniko::Color::from_rgb8(r, g, b));
                let image = ColorImage::from_rgba_premultiplied([width.into(), height.into()], &frame.pixels);
                if frames.send(Drawn { image, view: request.view, took: start.elapsed() }).is_err() {
                    return;
                }
                request.ctx.request_repaint();
            }
        });
    // Without the thread nothing is drawn, but nothing is lost either.
    if let Err(error) = spawned {
        log::error!("the canvas can't draw: {error}");
    }
    (requests, drawn)
}

/// What the pointer did on the canvas this frame, in document coordinates,
/// for the tools.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pointer {
    Press(Point),
    Drag(Point),
    Release,
}

pub struct Canvas {
    pub view: View,
    pub rulers: bool,
    pub pixel_grid: bool,
    /// Whether to show the zoom and the last frame's time in the corner.
    pub readout: bool,
    list: Arc<DisplayList>,
    /// Counts the lists drawn, so a new one is asked for even in the same view.
    generation: u64,
    requests: Sender<Request>,
    frames: Receiver<Drawn>,
    /// What the worker was last asked for, so a still canvas asks for nothing.
    asked: Option<(View, [u16; 2], Color32, u64)>,
    /// The newest frame, the view it was drawn for and how long it took.
    shown: Option<(TextureHandle, View, Duration)>,
    /// The canvas's size in points, as last laid out.
    size: Vec2,
    /// A document point to put in the middle once the size is known.
    centre: Option<Point>,
}

impl Canvas {
    pub fn new(list: DisplayList, centre: Option<Point>) -> Self {
        let (requests, frames) = worker();
        Self { view: View::default(), rulers: false, pixel_grid: true, readout: false, list: Arc::new(list), generation: 0, requests, frames, asked: None, shown: None, size: Vec2::ZERO, centre }
    }

    /// Draws `list` from now on.
    pub fn set_list(&mut self, list: DisplayList) {
        self.list = Arc::new(list);
        self.generation += 1;
    }

    /// Zooms by `factor` about the middle of the canvas.
    pub fn zoom_by(&mut self, factor: f64) {
        self.view.zoom_about(self.size / 2.0, factor);
    }

    /// Lays the canvas out in what's left of `ui`, handles panning and
    /// zooming, and outlines each of `selected` (the corners of a node's box,
    /// in document coordinates). Returns the rectangle it got and what the
    /// pointer did for the tools.
    pub fn show(&mut self, ui: &mut Ui, theme: &Theme, selected: &[[Point; 4]]) -> (Rect, Vec<Pointer>) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let from_corner = |p: Pos2| Vec2::new(f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y));
        let to_screen = |v: Vec2| Pos2::new(rect.min.x + v.x as f32, rect.min.y + v.y as f32);
        let moved = |v: egui::Vec2| Vec2::new(f64::from(v.x), f64::from(v.y));
        self.size = Vec2::new(f64::from(rect.width()), f64::from(rect.height()));
        if let Some(centre) = self.centre.take() {
            self.view.origin = self.size / 2.0 - centre.to_vec2() * self.view.zoom;
        }

        // Middle drag or Space+drag pans; the wheel pans, and with Ctrl (or a
        // pinch) zooms about the pointer.
        let space = ui.input(|i| i.key_down(Key::Space));
        if response.dragged_by(PointerButton::Middle) || (space && response.dragged_by(PointerButton::Primary)) {
            self.view.origin += moved(response.drag_delta());
        }
        // Everything else the primary button does is the tools' business.
        let mut pointer = Vec::new();
        let view = self.view;
        let document = |p: Pos2| ((from_corner(p) - view.origin) / view.zoom).to_point();
        let at = response.interact_pointer_pos();
        if !space {
            if response.drag_started_by(PointerButton::Primary)
                && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            {
                pointer.push(Pointer::Press(document(origin)));
            }
            if response.dragged_by(PointerButton::Primary)
                && let Some(at) = at
            {
                pointer.push(Pointer::Drag(document(at)));
            }
            if response.clicked_by(PointerButton::Primary)
                && let Some(at) = at
            {
                pointer.extend([Pointer::Press(document(at)), Pointer::Release]);
            }
        }
        // Released even if Space went down on the way, so no drag is left open.
        if response.drag_stopped_by(PointerButton::Primary) {
            pointer.push(Pointer::Release);
        }
        if let Some(pointer) = response.hover_pos() {
            let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta()));
            if zoom != 1.0 {
                self.view.zoom_about(from_corner(pointer), f64::from(zoom));
            } else {
                self.view.origin += moved(scroll);
            }
        }

        if let Some(drawn) = self.frames.try_iter().last() {
            let options = TextureOptions::LINEAR;
            let texture = match self.shown.take() {
                Some((mut texture, ..)) => {
                    texture.set(drawn.image, options);
                    texture
                }
                None => ui.ctx().load_texture("canvas", drawn.image, options),
            };
            self.shown = Some((texture, drawn.view, drawn.took));
        }

        let pixels_per_point = ui.ctx().pixels_per_point();
        let pixels = |points: f32| (points * pixels_per_point).round().clamp(0.0, f32::from(u16::MAX)) as u16;
        let size = [pixels(rect.width()), pixels(rect.height())];
        let backdrop = theme.backdrop();
        let want = (self.view, size, backdrop, self.generation);
        if self.asked != Some(want) && size[0] > 0 && size[1] > 0 {
            let request = Request { ctx: ui.ctx().clone(), list: self.list.clone(), view: self.view, size, pixels_per_point, backdrop };
            if self.requests.send(request).is_ok() {
                self.asked = Some(want);
            }
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme.backdrop());
        if let Some((texture, drawn_for, _)) = &self.shown {
            // Until the frame for this view arrives, the last one stands in,
            // moved and scaled to where its pixels now belong.
            let [width, height] = texture.size();
            let frame = Vec2::new(width as f64, height as f64) / f64::from(pixels_per_point);
            let place = Rect::from_min_max(
                to_screen(drawn_for.reproject(self.view, Vec2::ZERO)),
                to_screen(drawn_for.reproject(self.view, frame)),
            );
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(texture.id(), place, uv, Color32::WHITE);
        }
        let outline = egui::Stroke::new(1.5, theme.accent);
        for corners in selected {
            let corners = corners.map(|corner| to_screen(self.view.origin + corner.to_vec2() * self.view.zoom));
            painter.add(egui::Shape::closed_line(corners.to_vec(), outline));
            // One node alone shows the corners it can be resized by.
            if selected.len() == 1 {
                for corner in corners {
                    painter.rect(Rect::from_center_size(corner, egui::vec2(7.0, 7.0)), 0.0, Color32::WHITE, outline, egui::StrokeKind::Inside);
                }
            }
        }
        rulers::paint(&painter, rect, self.view, theme, self.rulers, self.pixel_grid);
        if let Some((_, _, took)) = &self.shown
            && self.readout
        {
            // Phase 0's readout, while the only thing to draw is the test scene.
            let text = format!("{:.0}%  {:.1} ms  {} paths", self.view.zoom * 100.0, took.as_secs_f64() * 1000.0, self.list.items.len());
            let at = rect.left_bottom() + egui::vec2(8.0, -8.0);
            painter.text(at, egui::Align2::LEFT_BOTTOM, text, egui::FontId::monospace(12.0), Color32::WHITE);
        }
        (rect, pointer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, MouseWheelUnit, TouchPhase, pos2, vec2};
    use omavec_geom::kurbo::Shape;
    use omavec_render::Item;

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let mut view = View { origin: Vec2::new(30.0, -12.0), zoom: 1.5 };
        let anchor = Vec2::new(400.0, 250.0);
        let under = |view: View| (anchor - view.origin) / view.zoom;
        let before = under(view);
        view.zoom_about(anchor, 3.0);
        assert_eq!(view.zoom, 4.5);
        assert!((under(view) - before).hypot() < 1e-9);
        // Stops at Figma's limits, still about the same point.
        view.zoom_about(anchor, 1e6);
        assert_eq!(view.zoom, 256.0);
        assert!((under(view) - before).hypot() < 1e-9);
        view.zoom_about(anchor, 1e-9);
        assert_eq!(view.zoom, 0.02);
        assert!((under(view) - before).hypot() < 1e-9);
    }

    #[test]
    fn a_stale_frame_is_placed_where_its_pixels_now_belong() {
        let drawn = View { origin: Vec2::new(100.0, 50.0), zoom: 2.0 };
        // Panned 10 right and 5 up: the frame moves with it.
        let panned = View { origin: Vec2::new(110.0, 45.0), zoom: 2.0 };
        assert_eq!(drawn.reproject(panned, Vec2::ZERO), Vec2::new(10.0, -5.0));
        assert_eq!(drawn.reproject(panned, Vec2::new(800.0, 600.0)), Vec2::new(810.0, 595.0));
        // Zoomed to twice the size about (400, 300): the frame doubles around it.
        let mut zoomed = drawn;
        zoomed.zoom_about(Vec2::new(400.0, 300.0), 2.0);
        assert_eq!(drawn.reproject(zoomed, Vec2::new(400.0, 300.0)), Vec2::new(400.0, 300.0));
        assert_eq!(drawn.reproject(zoomed, Vec2::ZERO), Vec2::new(-400.0, -300.0));
        assert_eq!(drawn.reproject(zoomed, Vec2::new(800.0, 600.0)), Vec2::new(1200.0, 900.0));
    }

    /// The canvas filling an 800 × 600 window, driven by made-up events.
    struct Harness {
        ctx: egui::Context,
        canvas: Canvas,
        time: f64,
        /// Everything the canvas has passed on for the tools.
        pointer: Vec<Pointer>,
    }

    impl Harness {
        fn new() -> Self {
            let square = omavec_geom::kurbo::Rect::new(0.0, 0.0, 100.0, 100.0).to_path(0.1);
            let list = DisplayList { items: vec![Item::new(square, peniko::Color::from_rgb8(255, 0, 0))] };
            let mut harness = Self { ctx: egui::Context::default(), canvas: Canvas::new(list, None), time: 0.0, pointer: Vec::new() };
            harness.frame(vec![Event::PointerMoved(pos2(400.0, 300.0))]);
            harness
        }

        fn frame(&mut self, events: Vec<Event>) -> Rect {
            self.time += 0.05;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let (canvas, pointer) = (&mut self.canvas, &mut self.pointer);
            let mut rect = Rect::NOTHING;
            let mut output = self.ctx.run_ui(input, |ui| {
                let shown = egui::CentralPanel::no_frame().show(ui, |ui| canvas.show(ui, &Theme::default(), &[])).inner;
                rect = shown.0;
                pointer.extend(shown.1);
            });
            // There's no renderer to upload textures to.
            output.textures_delta.clear();
            rect
        }

        /// A wheel turn, and the frames egui spreads its scroll over.
        fn wheel(&mut self, delta: egui::Vec2, modifiers: Modifiers) {
            let wheel = Event::MouseWheel { unit: MouseWheelUnit::Point, delta, phase: TouchPhase::Move, modifiers };
            self.frame(vec![Event::ModifiersChanged(modifiers), wheel]);
            for _ in 0..40 {
                self.frame(vec![]);
            }
            self.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
        }
    }

    #[test]
    fn the_wheel_pans() {
        let mut harness = Harness::new();
        harness.wheel(vec2(30.0, -80.0), Modifiers::NONE);
        let view = harness.canvas.view;
        assert_eq!(view.zoom, 1.0);
        assert!((view.origin - Vec2::new(30.0, -80.0)).hypot() < 1.0, "{view:?}");
    }

    #[test]
    fn ctrl_wheel_zooms_about_the_pointer() {
        let mut harness = Harness::new();
        harness.canvas.view.origin = Vec2::new(40.0, 20.0);
        let pointer = Vec2::new(400.0, 300.0);
        let under = |view: View| (pointer - view.origin) / view.zoom;
        let before = under(harness.canvas.view);
        harness.wheel(vec2(0.0, 100.0), Modifiers::COMMAND);
        let view = harness.canvas.view;
        assert!(view.zoom > 1.2, "wheel up zooms in: {view:?}");
        assert!((under(view) - before).hypot() < 1e-6, "{view:?}");
        harness.wheel(vec2(0.0, -300.0), Modifiers::COMMAND);
        assert!(harness.canvas.view.zoom < 1.0, "wheel down zooms out");
    }

    #[test]
    fn middle_drag_and_space_drag_pan() {
        let button = |button, pressed, x, y| Event::PointerButton { pos: pos2(x, y), button, pressed, modifiers: Modifiers::NONE };
        let mut harness = Harness::new();
        harness.frame(vec![button(PointerButton::Middle, true, 400.0, 300.0)]);
        harness.frame(vec![Event::PointerMoved(pos2(460.0, 280.0))]);
        harness.frame(vec![button(PointerButton::Middle, false, 460.0, 280.0)]);
        assert_eq!(harness.canvas.view.origin, Vec2::new(60.0, -20.0));

        // A plain drag is for the tools, not the view.
        harness.frame(vec![button(PointerButton::Primary, true, 460.0, 280.0)]);
        harness.frame(vec![Event::PointerMoved(pos2(500.0, 300.0))]);
        harness.frame(vec![button(PointerButton::Primary, false, 500.0, 300.0)]);
        assert_eq!(harness.canvas.view.origin, Vec2::new(60.0, -20.0));

        let space = |pressed| Event::Key { key: Key::Space, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        harness.frame(vec![space(true)]);
        harness.frame(vec![button(PointerButton::Primary, true, 500.0, 300.0)]);
        harness.frame(vec![Event::PointerMoved(pos2(490.0, 330.0))]);
        harness.frame(vec![button(PointerButton::Primary, false, 490.0, 330.0), space(false)]);
        assert_eq!(harness.canvas.view.origin, Vec2::new(50.0, 10.0));
    }

    #[test]
    fn the_primary_button_goes_to_the_tools_in_document_coordinates() {
        let button = |button, pressed, x, y| Event::PointerButton { pos: pos2(x, y), button, pressed, modifiers: Modifiers::NONE };
        let mut harness = Harness::new();
        harness.canvas.view = View { origin: Vec2::new(40.0, 20.0), zoom: 2.0 };
        // A drag: pressed where it began, wherever it has got to since.
        harness.frame(vec![button(PointerButton::Primary, true, 400.0, 300.0)]);
        harness.frame(vec![Event::PointerMoved(pos2(430.0, 320.0))]);
        harness.frame(vec![Event::PointerMoved(pos2(460.0, 340.0))]);
        harness.frame(vec![button(PointerButton::Primary, false, 460.0, 340.0)]);
        assert_eq!(harness.pointer.first(), Some(&Pointer::Press((180.0, 140.0).into())));
        assert_eq!(harness.pointer.last(), Some(&Pointer::Release));
        let drags: Vec<&Pointer> = harness.pointer.iter().filter(|event| matches!(event, Pointer::Drag(_))).collect();
        assert_eq!(drags.last(), Some(&&Pointer::Drag((210.0, 160.0).into())));
        assert_eq!(harness.pointer.len(), drags.len() + 2);
        // The view didn't move.
        assert_eq!(harness.canvas.view.origin, Vec2::new(40.0, 20.0));

        // A click: a press and a release where it was.
        harness.pointer.clear();
        harness.frame(vec![Event::PointerMoved(pos2(100.0, 100.0))]);
        harness.frame(vec![button(PointerButton::Primary, true, 100.0, 100.0)]);
        harness.frame(vec![button(PointerButton::Primary, false, 100.0, 100.0)]);
        assert_eq!(harness.pointer, [Pointer::Press((30.0, 40.0).into()), Pointer::Release]);

        // Panning is not for the tools: the middle button, or Space held.
        harness.pointer.clear();
        harness.frame(vec![button(PointerButton::Middle, true, 100.0, 100.0)]);
        harness.frame(vec![Event::PointerMoved(pos2(150.0, 150.0))]);
        harness.frame(vec![button(PointerButton::Middle, false, 150.0, 150.0)]);
        let space = |pressed| Event::Key { key: Key::Space, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        harness.frame(vec![space(true)]);
        harness.frame(vec![button(PointerButton::Primary, true, 150.0, 150.0)]);
        harness.frame(vec![Event::PointerMoved(pos2(200.0, 200.0))]);
        harness.frame(vec![button(PointerButton::Primary, false, 200.0, 200.0), space(false)]);
        assert!(harness.pointer.iter().all(|event| *event == Pointer::Release), "{:?}", harness.pointer);
    }

    #[test]
    fn the_worker_draws_the_view_it_was_asked_for() {
        let mut harness = Harness::new();
        harness.canvas.view = View { origin: Vec2::new(200.0, 100.0), zoom: 2.0 };
        let deadline = Instant::now() + Duration::from_secs(10);
        while harness.canvas.shown.as_ref().is_none_or(|(_, view, _)| *view != harness.canvas.view) {
            assert!(Instant::now() < deadline, "no frame for the new view");
            harness.frame(vec![]);
            std::thread::sleep(Duration::from_millis(2));
        }
        let (texture, ..) = harness.canvas.shown.as_ref().unwrap();
        assert_eq!(texture.size(), [800, 600]);
        // A still canvas asks for nothing more.
        harness.frame(vec![]);
        assert!(harness.canvas.frames.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn rulers_and_pixel_grid_do_not_change_canvas_rect_or_panic() {
        let mut harness_off = Harness::new();
        harness_off.canvas.rulers = false;
        harness_off.canvas.pixel_grid = false;
        harness_off.canvas.view.zoom = 8.0;
        let rect_off = harness_off.frame(vec![]);

        let mut harness_on = Harness::new();
        harness_on.canvas.rulers = true;
        harness_on.canvas.pixel_grid = true;
        harness_on.canvas.view.zoom = 8.0;
        let rect_on = harness_on.frame(vec![]);

        assert_eq!(rect_off, rect_on);
    }
}
