//! Draws a display list with `vello_cpu`: on a worker thread for the canvas,
//! and directly for headless export and golden-image tests. It is one
//! renderer for all three, so an export is what the canvas showed.

pub mod spike;

pub use peniko;

use kurbo::{Affine, BezPath, Rect, Shape};
use peniko::{Color, ImageAlphaType};
use vello_cpu::{Pixmap, RenderContext, Resources};

/// One filled path, in document units.
pub struct Item {
    pub path: BezPath,
    pub color: Color,
    bounds: Rect,
}

impl Item {
    pub fn new(path: BezPath, color: Color) -> Self {
        Self { bounds: path.bounding_box(), path, color }
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }
}

/// What to draw, back to front.
#[derive(Default)]
pub struct DisplayList {
    pub items: Vec<Item>,
}

/// A drawn frame: premultiplied RGBA8, top row first.
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
}

/// Keeps `vello_cpu`'s buffers and worker threads between frames.
pub struct Renderer {
    context: RenderContext,
    resources: Resources,
}

impl Default for Renderer {
    fn default() -> Self {
        Self { context: RenderContext::new(1, 1), resources: Resources::new() }
    }
}

impl Renderer {
    /// Draws what `view` (document units to pixels) puts of `list` inside a
    /// `width` × `height` frame, over `background`.
    pub fn render(&mut self, list: &DisplayList, view: Affine, width: u16, height: u16, background: Color) -> Frame {
        let screen = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
        self.context.reset_and_resize(width, height);
        self.context.set_paint(background);
        self.context.fill_rect(&screen);
        self.context.set_transform(view);
        for item in &list.items {
            // vello_cpu keeps no scene between frames, so every path it's
            // given is processed again: skip the ones off screen.
            if view.transform_rect_bbox(item.bounds).intersect(screen).is_zero_area() {
                continue;
            }
            self.context.set_paint(item.color);
            self.context.fill_path(&item.path);
        }
        self.context.flush();
        let mut pixmap = Pixmap::new(width, height);
        self.context.render(&mut pixmap, &mut self.resources);
        Frame { width, height, pixels: pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(frame: &Frame, x: usize, y: usize) -> [u8; 4] {
        let at = (y * usize::from(frame.width) + x) * 4;
        [frame.pixels[at], frame.pixels[at + 1], frame.pixels[at + 2], frame.pixels[at + 3]]
    }

    #[test]
    fn draws_a_path_where_the_view_puts_it() {
        let red = Color::from_rgb8(255, 0, 0);
        let grey = Color::from_rgb8(40, 40, 40);
        // A 10-unit square at (10, 10), and one far outside the frame.
        let list = DisplayList {
            items: vec![
                Item::new(Rect::new(10.0, 10.0, 20.0, 20.0).to_path(0.1), red),
                Item::new(Rect::new(5000.0, 5000.0, 5010.0, 5010.0).to_path(0.1), red),
            ],
        };
        // Twice the size, then 4 pixels right and 6 down: pixels 24..44 by 26..46.
        let view = Affine::translate((4.0, 6.0)) * Affine::scale(2.0);
        let frame = Renderer::default().render(&list, view, 64, 64, grey);
        assert_eq!(frame.pixels.len(), 64 * 64 * 4);
        assert_eq!(pixel(&frame, 34, 36), [255, 0, 0, 255]);
        assert_eq!(pixel(&frame, 25, 27), [255, 0, 0, 255]);
        assert_eq!(pixel(&frame, 43, 45), [255, 0, 0, 255]);
        assert_eq!(pixel(&frame, 22, 36), [40, 40, 40, 255]);
        assert_eq!(pixel(&frame, 34, 24), [40, 40, 40, 255]);
        assert_eq!(pixel(&frame, 45, 47), [40, 40, 40, 255]);
    }

    #[test]
    fn a_path_half_off_the_frame_is_still_drawn() {
        let red = Color::from_rgb8(255, 0, 0);
        let list = DisplayList { items: vec![Item::new(Rect::new(-50.0, -50.0, 8.0, 8.0).to_path(0.1), red)] };
        let frame = Renderer::default().render(&list, Affine::IDENTITY, 16, 16, Color::BLACK);
        assert_eq!(pixel(&frame, 3, 3), [255, 0, 0, 255]);
        assert_eq!(pixel(&frame, 12, 12), [0, 0, 0, 255]);
    }
}
