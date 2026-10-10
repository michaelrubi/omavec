//! Draws a display list with `vello_cpu`: on a worker thread for the canvas,
//! and directly for headless export and golden-image tests. It is one
//! renderer for all three, so an export is what the canvas showed.
// Shipped code returns errors; only tests may panic.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod spike;

pub use omavec_engine::display::{DisplayList, Fill, Item};
pub use peniko;

use kurbo::{Affine, Rect};
use peniko::{Color, ImageAlphaType};
use vello_cpu::{Pixmap, RenderContext, Resources};

/// A drawn frame: premultiplied RGBA8, top row first.
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
}

impl Frame {
    /// The frame as a PNG file's bytes.
    pub fn into_png(self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let pixmap = Pixmap::from_parts(self.pixels, self.width, self.height, vello_cpu::PixelMetadata::new(ImageAlphaType::AlphaPremultiplied, true));
        Ok(pixmap.into_png()?)
    }
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
            match item {
                Item::Fill(fill) => {
                    // vello_cpu keeps no scene between frames, so every path
                    // it's given is processed again: skip the ones off screen.
                    if !view.transform_rect_bbox(fill.bounds()).intersect(screen).is_zero_area() {
                        match &fill.brush {
                            peniko::Brush::Solid(color) => self.context.set_paint(*color),
                            peniko::Brush::Gradient(gradient) => self.context.set_paint(gradient.clone()),
                            // Nothing makes these yet.
                            peniko::Brush::Image(_) => continue,
                        }
                        self.context.set_paint_transform(fill.transform);
                        self.context.fill_path(&fill.path);
                    }
                }
                Item::Clip(path) => self.context.push_clip_path(path),
                Item::Unclip => self.context.pop_clip(),
                Item::Fade(opacity, mix) => self.context.push_layer(None, Some(peniko::BlendMode::new(*mix, peniko::Compose::SrcOver)), Some(*opacity), None, None),
                Item::Unfade => self.context.pop_layer(),
            }
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
    use kurbo::Shape;

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
    fn a_document_is_drawn_as_its_nodes_are_laid_out() {
        use omavec_engine::{Document, NodeKind};
        // A white frame at (10, 10), with Figma's grey ellipse filling its
        // right half.
        let mut document = Document::default();
        let page = document.pages[0].id;
        let mut frame = document.create(NodeKind::Frame { clip: true }, (40.0, 20.0).into());
        frame.transform = Affine::translate((10.0, 10.0));
        let frame_id = frame.id;
        document.insert(page, 0, frame).unwrap();
        let mut ellipse = document.create(NodeKind::Ellipse, (20.0, 20.0).into());
        ellipse.transform = Affine::translate((20.0, 0.0));
        document.insert(frame_id, 0, ellipse).unwrap();

        let list = DisplayList::of(&document.pages[0]);
        let frame = Renderer::default().render(&list, Affine::IDENTITY, 64, 40, Color::from_rgb8(40, 40, 40));
        assert_eq!(pixel(&frame, 5, 20), [40, 40, 40, 255], "the backdrop");
        assert_eq!(pixel(&frame, 20, 20), [255, 255, 255, 255], "the frame");
        assert_eq!(pixel(&frame, 40, 20), [0xd9, 0xd9, 0xd9, 255], "the middle of the ellipse");
        assert_eq!(pixel(&frame, 31, 11), [255, 255, 255, 255], "the frame, in the corner of the ellipse's box");
        assert_eq!(pixel(&frame, 55, 20), [40, 40, 40, 255], "past the frame");
    }

    #[test]
    fn a_frame_clips_its_children_and_a_group_fades_as_one() {
        use omavec_engine::{Document, NodeId, NodeKind, Paint};
        let mut document = Document::default();
        let page = document.pages[0].id;
        let mut add = |parent: NodeId, kind: NodeKind, at: (f64, f64), size: (f64, f64), fill: Option<[u8; 3]>| {
            let mut node = document.create(kind, size.into());
            node.transform = Affine::translate(at);
            node.fills = fill.into_iter().map(|[r, g, b]| Paint::solid(omavec_engine::Color::rgb(r, g, b))).collect();
            let id = node.id;
            document.insert(parent, usize::MAX, node).unwrap();
            id
        };
        // A white frame at 10..50 by 10..30 with a red bar through it, 40..60
        // by 0..40.
        let frame = add(page, NodeKind::Frame { clip: true }, (10.0, 10.0), (40.0, 20.0), Some([255, 255, 255]));
        add(frame, NodeKind::Rectangle, (30.0, -10.0), (20.0, 40.0), Some([255, 0, 0]));
        // A group with red at 0..20 and blue over it at 10..30, down at 50..60.
        let group = add(page, NodeKind::Group, (0.0, 50.0), (0.0, 0.0), None);
        add(group, NodeKind::Rectangle, (0.0, 0.0), (20.0, 10.0), Some([255, 0, 0]));
        add(group, NodeKind::Rectangle, (10.0, 0.0), (20.0, 10.0), Some([0, 0, 255]));

        let draw = |document: &Document| Renderer::default().render(&DisplayList::of(&document.pages[0]), Affine::IDENTITY, 64, 64, Color::WHITE);
        let drawn = draw(&document);
        assert_eq!(pixel(&drawn, 45, 20), [255, 0, 0, 255], "the bar, in the frame");
        assert_eq!(pixel(&drawn, 45, 5), [255, 255, 255, 255], "the bar, cut off above the frame");
        assert_eq!(pixel(&drawn, 55, 20), [255, 255, 255, 255], "and to its right");
        assert_eq!(pixel(&drawn, 15, 55), [0, 0, 255, 255], "blue over red");

        document.node_mut(frame).unwrap().kind = NodeKind::Frame { clip: false };
        document.node_mut(group).unwrap().opacity = 0.5;
        let drawn = draw(&document);
        assert_eq!(pixel(&drawn, 45, 5), [255, 0, 0, 255], "the bar, no longer cut off");
        assert_eq!(pixel(&drawn, 55, 20), [255, 0, 0, 255]);
        // Half of what the group looked like: red alone, then blue with no
        // red showing through it.
        let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1);
        assert!(near(pixel(&drawn, 5, 55), [255, 128, 128, 255]), "{:?}", pixel(&drawn, 5, 55));
        assert!(near(pixel(&drawn, 15, 55), [128, 128, 255, 255]), "{:?}", pixel(&drawn, 15, 55));
        assert!(near(pixel(&drawn, 25, 55), [128, 128, 255, 255]), "{:?}", pixel(&drawn, 25, 55));
    }

    #[test]
    fn a_frame_becomes_a_png_with_its_transparency() {
        let list = DisplayList { items: vec![Item::new(Rect::new(0.0, 0.0, 4.0, 4.0).to_path(0.1), Color::from_rgba8(255, 0, 0, 128))] };
        let frame = Renderer::default().render(&list, Affine::IDENTITY, 8, 6, Color::TRANSPARENT);
        let png = frame.into_png().unwrap();
        let decoded = Pixmap::from_png(std::io::Cursor::new(png)).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (8, 6));
        let pixel = |x: u16, y: u16| decoded.sample(x, y);
        // Half-transparent red where the square is, nothing beside it.
        assert_eq!((pixel(1, 1).a, pixel(6, 4).a), (128, 0));
        assert!(pixel(1, 1).r >= 127 && pixel(1, 1).g == 0);
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
