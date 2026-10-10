//! The display list: a page flattened into filled paths in document space,
//! back to front. It is what the renderer draws, on the canvas and for
//! export alike.

use omavec_geom::kurbo::{Affine, BezPath, Rect, Shape};
use omavec_geom::stroke::outline;

use crate::document::{Node, NodeKind};
use crate::paint::PaintKind;

/// One filled path, in document units.
pub struct Fill {
    pub path: BezPath,
    pub brush: peniko::Brush,
    /// From the brush's own coordinates to the document's. A gradient is
    /// laid out in its node's box, a unit square.
    pub transform: Affine,
    bounds: Rect,
}

impl Fill {
    pub fn bounds(&self) -> Rect {
        self.bounds
    }
}

/// One step of drawing a page.
// Nearly every item is a fill, so boxing them would save nothing.
#[allow(clippy::large_enum_variant)]
pub enum Item {
    Fill(Fill),
    /// Until the `Unclip` that matches it, only what is inside the path shows.
    Clip(BezPath),
    Unclip,
    /// Until the `Unfade` that matches it, what is drawn is drawn together
    /// and then let through at this opacity, so where two things in it
    /// overlap neither shows through the other, and mixed with what is
    /// under it in this way.
    Fade(f32, peniko::Mix),
    Unfade,
}

impl Item {
    pub fn new(path: BezPath, color: peniko::Color) -> Self {
        Item::Fill(Fill { bounds: path.bounding_box(), path, brush: color.into(), transform: Affine::IDENTITY })
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
        let path = node.shape();
        let stroked = node.stroke.weight > 0.0 && node.stroke.paints.iter().any(|paint| paint.visible);
        // A node's opacity is the node's as a whole. One paint and nothing
        // else can simply be that much fainter; anything more is drawn
        // together first.
        let paints = node.fills.iter().filter(|paint| paint.visible).count() + if stroked { node.stroke.paints.iter().filter(|paint| paint.visible).count() } else { 0 };
        let blended = node.blend != crate::paint::Blend::Normal;
        let together = blended || node.opacity < 1.0 && (paints > 1 || !node.children.is_empty());
        let opacity = if together { 1.0 } else { node.opacity };
        if together {
            self.items.push(Item::Fade(node.opacity.clamp(0.0, 1.0) as f32, node.blend.mix()));
        }
        let paint = |list: &mut Self, path: &BezPath, paints: &[crate::paint::Paint]| {
            let path = transform * path.clone();
            // A stroke with no weight has no area to paint.
            for paint in paints.iter().filter(|paint| paint.visible && !path.elements().is_empty()) {
                let strength = opacity * paint.opacity;
                let color = |color: crate::paint::Color, opacity: f64| peniko::Color::from_rgb8(color.r, color.g, color.b).with_alpha((strength * opacity).clamp(0.0, 1.0) as f32);
                let stops = |stops: Vec<crate::paint::Stop>| stops.into_iter().map(|stop| (stop.at.clamp(0.0, 1.0) as f32, color(stop.color, stop.opacity))).collect::<Vec<_>>();
                let brush: peniko::Brush = match (&paint.kind, paint.kind.stops()) {
                    (PaintKind::Linear { from, to, .. }, Some(along)) => peniko::Gradient::new_linear(*from, *to).with_stops(&stops(along)[..]).into(),
                    (PaintKind::Radial { from, to, .. }, Some(along)) => peniko::Gradient::new_radial(*from, (to.0 - from.0).hypot(to.1 - from.1) as f32).with_stops(&stops(along)[..]).into(),
                    (kind, _) => color(kind.color(), 1.0).into(),
                };
                let Item::Fill(mut fill) = Item::new(path.clone(), peniko::Color::TRANSPARENT) else { continue };
                (fill.brush, fill.transform) = (brush, transform * Affine::scale_non_uniform(node.size.width, node.size.height));
                list.items.push(Item::Fill(fill));
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
            paint(self, &outline(path, &node.stroke.style()), &node.stroke.paints);
        }
        if together {
            self.items.push(Item::Unfade);
        }
    }
}
