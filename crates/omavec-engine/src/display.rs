//! The display list: a page flattened into filled paths in document space,
//! back to front. It is what the renderer draws, on the canvas and for
//! export alike.

use omavec_geom::kurbo::{Affine, BezPath, Ellipse, Rect, Shape};

use crate::document::{Node, NodeKind};
use crate::paint::PaintKind;

/// How far an ellipse's path may stray from the true curve, in document
/// units: a quarter of a pixel at the deepest zoom.
const TOLERANCE: f64 = 1e-3;

/// One filled path, in document units.
pub struct Item {
    pub path: BezPath,
    pub color: peniko::Color,
    bounds: Rect,
}

impl Item {
    pub fn new(path: BezPath, color: peniko::Color) -> Self {
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

impl DisplayList {
    /// Everything visible on `page`.
    pub fn of(page: &Node) -> Self {
        let mut list = Self::default();
        list.add(page, Affine::IDENTITY, 1.0);
        list
    }

    fn add(&mut self, node: &Node, parent: Affine, opacity: f64) {
        if !node.visible {
            return;
        }
        let transform = parent * node.transform;
        // Later: a node's opacity belongs to the node as a whole, so where
        // its children overlap this shows through and a layer wouldn't.
        let opacity = opacity * node.opacity;
        let outline = Rect::from_origin_size((0.0, 0.0), node.size);
        let path = match node.kind {
            NodeKind::Page | NodeKind::Group => None,
            NodeKind::Frame { .. } | NodeKind::Rectangle => Some(outline.to_path(TOLERANCE)),
            NodeKind::Ellipse => Some(Ellipse::from_rect(outline).to_path(TOLERANCE)),
        };
        if let Some(path) = path {
            let path = transform * path;
            for paint in node.fills.iter().filter(|paint| paint.visible) {
                let PaintKind::Solid { color } = paint.kind;
                let alpha = (opacity * paint.opacity).clamp(0.0, 1.0) as f32;
                self.items.push(Item::new(path.clone(), peniko::Color::from_rgb8(color.r, color.g, color.b).with_alpha(alpha)));
            }
        }
        // Later: a frame with `clip` on hides what its children draw outside it.
        for child in &node.children {
            self.add(child, transform, opacity);
        }
    }
}
