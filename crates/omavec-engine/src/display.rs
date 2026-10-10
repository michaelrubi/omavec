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
pub struct Fill {
    pub path: BezPath,
    pub color: peniko::Color,
    bounds: Rect,
}

impl Fill {
    pub fn bounds(&self) -> Rect {
        self.bounds
    }
}

/// One step of drawing a page.
pub enum Item {
    Fill(Fill),
    /// Until the `Unclip` that matches it, only what is inside the path shows.
    Clip(BezPath),
    Unclip,
    /// Until the `Unfade` that matches it, what is drawn is drawn together
    /// and then let through at this opacity, so where two things in it
    /// overlap neither shows through the other.
    Fade(f32),
    Unfade,
}

impl Item {
    pub fn new(path: BezPath, color: peniko::Color) -> Self {
        Item::Fill(Fill { bounds: path.bounding_box(), path, color })
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
        list.add(page, Affine::IDENTITY);
        list
    }

    fn add(&mut self, node: &Node, parent: Affine) {
        if !node.visible {
            return;
        }
        let transform = parent * node.transform;
        let path = shape(node);
        let stroked = node.stroke.weight > 0.0 && node.stroke.paints.iter().any(|paint| paint.visible);
        // A node's opacity is the node's as a whole. One paint and nothing
        // else can simply be that much fainter; anything more is drawn
        // together first.
        let paints = node.fills.iter().filter(|paint| paint.visible).count() + if stroked { node.stroke.paints.iter().filter(|paint| paint.visible).count() } else { 0 };
        let together = node.opacity < 1.0 && (paints > 1 || !node.children.is_empty());
        let opacity = if together { 1.0 } else { node.opacity };
        if together {
            self.items.push(Item::Fade(node.opacity.clamp(0.0, 1.0) as f32));
        }
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
        // A frame that clips shows nothing of its children outside itself.
        let clip = path.as_ref().filter(|_| matches!(node.kind, NodeKind::Frame { clip: true }) && !node.children.is_empty());
        if let Some(path) = clip {
            self.items.push(Item::Clip(transform * path.clone()));
        }
        for child in &node.children {
            self.add(child, transform);
        }
        if clip.is_some() {
            self.items.push(Item::Unclip);
        }
        // The stroke goes over the fill, and a frame's over what is in it.
        if let Some(path) = path.as_ref().filter(|_| stroked) {
            paint(self, &outline(path, node.stroke.weight, node.stroke.align), &node.stroke.paints);
        }
        if together {
            self.items.push(Item::Unfade);
        }
    }
}
