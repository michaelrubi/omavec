//! Strokes as areas: what a stroke covers, as a path to fill. It is how
//! strokes are drawn, and what Outline Stroke will bake.

use kurbo::{BezPath, Cap, Join, Stroke, StrokeOpts};
use serde::{Deserialize, Serialize};
use vectorcraft_geom::{FillRule, PathData};
use vectorcraft_pathops::{BoolOp, boolean};

/// How close the outline keeps to the true offset curve, in document units:
/// a quarter of a pixel at the deepest zoom.
const TOLERANCE: f64 = 1e-3;

/// Which side of a shape's edge its stroke is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Inside,
    Center,
    Outside,
}

/// The area a stroke of `weight` covers on the closed `shape`, to be filled
/// non-zero. Empty for a weight of nothing.
pub fn outline(shape: &BezPath, weight: f64, align: Align) -> BezPath {
    if weight.is_nan() || weight <= 0.0 {
        return BezPath::new();
    }
    // Inside and outside are half of a centred stroke twice as wide: the
    // half in the shape, or the half out of it.
    let width = if align == Align::Center { weight } else { weight * 2.0 };
    let style = Stroke::new(width).with_join(Join::Miter).with_caps(Cap::Butt);
    let band = kurbo::stroke(shape.elements().iter().copied(), &style, &StrokeOpts::default(), TOLERANCE);
    let operation = match align {
        Align::Center => return band,
        Align::Inside => BoolOp::Intersect,
        Align::Outside => BoolOp::Difference,
    };
    boolean(&PathData::from_bezpath(&band), FillRule::NonZero, &PathData::from_bezpath(shape), FillRule::NonZero, operation).to_bezpath()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Circle, Point, Rect, Shape};

    /// Whether the outline covers the point (`x`, `y`).
    fn covers(outline: &BezPath, x: f64, y: f64) -> bool {
        outline.winding(Point::new(x, y)) != 0
    }

    /// The area an outline covers when filled non-zero.
    fn area(outline: &BezPath) -> f64 {
        vectorcraft_pathops::area(&PathData::from_bezpath(outline), FillRule::NonZero)
    }

    #[test]
    fn a_rectangles_stroke_sits_on_the_side_it_is_aligned_to() {
        let shape = Rect::new(0.0, 0.0, 100.0, 50.0).to_path(0.1);
        // Just inside the left edge, just outside it, and well away from it.
        let points = [(3.0, 25.0), (-3.0, 25.0), (50.0, 25.0), (-8.0, 25.0), (8.0, 25.0)];
        let at = |align| points.map(|(x, y)| covers(&outline(&shape, 10.0, align), x, y));
        assert_eq!(at(Align::Inside), [true, false, false, false, true]);
        assert_eq!(at(Align::Center), [true, true, false, false, false]);
        assert_eq!(at(Align::Outside), [false, true, false, true, false]);
        // With mitred corners: 100 × 50 less 80 × 30, 110 × 60 less 90 × 40,
        // and 120 × 70 less 100 × 50.
        for (align, expected) in [(Align::Inside, 2600.0), (Align::Center, 3000.0), (Align::Outside, 3400.0)] {
            let covered = area(&outline(&shape, 10.0, align));
            assert!((covered - expected).abs() < 1.0, "{align:?}: {covered}");
        }
        // A mitred corner is square: the outside stroke reaches its tip.
        assert!(covers(&outline(&shape, 10.0, Align::Outside), -9.0, -9.0));
    }

    #[test]
    fn a_circles_stroke_is_a_ring() {
        let shape = Circle::new((0.0, 0.0), 50.0).to_path(1e-4);
        let ring = |outer: f64, inner: f64| std::f64::consts::PI * (outer * outer - inner * inner);
        for (align, expected) in [(Align::Inside, ring(50.0, 40.0)), (Align::Center, ring(55.0, 45.0)), (Align::Outside, ring(60.0, 50.0))] {
            let covered = area(&outline(&shape, 10.0, align));
            assert!((covered - expected).abs() < expected * 0.002, "{align:?}: {covered} against {expected}");
        }
        assert!(!covers(&outline(&shape, 10.0, Align::Inside), 0.0, 0.0));
    }

    #[test]
    fn no_weight_is_no_stroke() {
        let shape = Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1);
        for weight in [0.0, -1.0, f64::NAN] {
            assert!(outline(&shape, weight, Align::Center).elements().is_empty());
        }
    }
}
