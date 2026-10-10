//! The display list: a page flattened into filled paths in document space,
//! back to front. It is what the renderer draws, on the canvas and for
//! export alike.

use omavec_geom::kurbo::{Affine, BezPath, Ellipse, Rect, Shape};
use omavec_geom::stroke::outline;

use crate::document::{Node, NodeKind};
use crate::paint::PaintKind;

/// How far an ellipse's path may stray from the true curve, in document
/// units: a quarter of a pixel at the deepest zoom.
const TOLERANCE: f64 = 1e-3;

/// A node's own shape as a path, in its own coordinates. Pages and groups
/// have none.
pub(crate) fn shape(node: &Node) -> Option<BezPath> {
    let bounds = Rect::from_origin_size((0.0, 0.0), node.size);
    match node.kind {
        NodeKind::Page | NodeKind::Group => None,
        NodeKind::Frame { .. } | NodeKind::Rectangle => Some(bounds.to_path(TOLERANCE)),
        NodeKind::Ellipse => Some(Ellipse::from_rect(bounds).to_path(TOLERANCE)),
    }
}

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
        let path = shape(node);
        let paint = |list: &mut Self, path: &BezPath, paints: &[crate::paint::Paint]| {
            let path = transform * path.clone();
            // A stroke with no weight has no area to paint.
            for paint in paints.iter().filter(|paint| paint.visible && !path.elements().is_empty()) {
                let PaintKind::Solid { color } = paint.kind;
                let alpha = (opacity * paint.opacity).clamp(0.0, 1.0) as f32;
                list.items.push(Item::new(path.clone(), peniko::Color::from_rgb8(color.r, color.g, color.b).with_alpha(alpha)));
            }
        };
        if let Some(path) = &path {
            paint(self, path, &node.fills);
        }
        // Later: a frame with `clip` on hides what its children draw outside it.
        for child in &node.children {
            self.add(child, transform, opacity);
        }
        // The stroke goes over the fill, and a frame's over what is in it.
        if let Some(path) = &path
            && node.stroke.paints.iter().any(|paint| paint.visible)
        {
            paint(self, &outline(path, node.stroke.weight, node.stroke.align), &node.stroke.paints);
        }
    }
}
