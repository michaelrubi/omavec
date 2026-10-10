//! Strokes as areas: what a stroke covers, as a path to fill. It is how
//! strokes are drawn, and what Outline Stroke will bake.

use kurbo::{BezPath, PathEl, PathSeg, Point, Stroke, StrokeOpts, Vec2};
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

/// How a stroke ends, where the path it follows does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cap {
    /// Cut square at the end of the path.
    #[default]
    None,
    Round,
    /// Cut square half the weight beyond it.
    Square,
    /// Two lines back from the end: an open arrowhead.
    Arrow,
    /// A filled arrowhead.
    Triangle,
}

/// How a stroke turns a corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Join {
    #[default]
    Miter,
    Bevel,
    Round,
}

/// Everything about a stroke but what it is painted with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub weight: f64,
    pub align: Align,
    pub join: Join,
    pub start: Cap,
    pub end: Cap,
}

impl Style {
    /// A stroke with mitred corners and plain ends.
    pub fn new(weight: f64, align: Align) -> Self {
        Self { weight, align, join: Join::Miter, start: Cap::None, end: Cap::None }
    }
}

/// How long an arrowhead is for each unit of the stroke's weight.
const HEAD: f64 = 4.0;

/// The area of the arrowhead `cap` on a stroke of `weight` that ends at
/// `tip` heading `towards` (a unit vector). It reaches a little past the
/// tip, so the square end of the stroke is inside it.
fn head(cap: Cap, tip: Point, towards: Vec2, weight: f64) -> Option<BezPath> {
    let (back, across) = (-towards, Vec2::new(-towards.y, towards.x));
    let mut path = BezPath::new();
    match cap {
        Cap::Arrow => {
            // Two arms at 45° either side of the way back, stroked.
            let arm = |side: f64| tip + (back + across * side) * (HEAD * weight * std::f64::consts::FRAC_1_SQRT_2);
            path.move_to(arm(1.0));
            path.line_to(tip);
            path.line_to(arm(-1.0));
            Some(kurbo::stroke(path, &Stroke::new(weight), &StrokeOpts::default(), TOLERANCE))
        }
        Cap::Triangle => {
            // Equilateral, its point one weight past the tip.
            let (side, point) = (HEAD * weight, tip + towards * weight);
            let base = point + back * (side * 3f64.sqrt() / 2.0);
            path.move_to(point);
            path.line_to(base + across * (side / 2.0));
            path.line_to(base - across * (side / 2.0));
            path.close_path();
            Some(path)
        }
        Cap::None | Cap::Round | Cap::Square => None,
    }
}

/// Each open run of `shape`: where it starts and ends, and the unit vector
/// it is heading along as it leaves each (outwards, away from the path).
fn ends(shape: &BezPath) -> Vec<[(Point, Vec2); 2]> {
    // The first of `points` that isn't `from` says which way the curve goes.
    let heading = |from: Point, points: [Point; 3]| points.into_iter().map(|point| from - point).find(|way| way.hypot() > 0.0).map(|way| way.normalize());
    let mut found = Vec::new();
    let mut run: Vec<PathSeg> = Vec::new();
    let mut close = |run: &mut Vec<PathSeg>, closed: bool| {
        if let (false, Some(first), Some(last)) = (closed, run.first(), run.last()) {
            let (first, last) = (first.to_cubic(), last.to_cubic());
            if let (Some(out), Some(on)) = (heading(first.p0, [first.p1, first.p2, first.p3]), heading(last.p3, [last.p2, last.p1, last.p0])) {
                found.push([(first.p0, out), (last.p3, on)]);
            }
        }
        run.clear();
    };
    let mut at = Point::ZERO;
    for element in shape.elements() {
        match *element {
            PathEl::MoveTo(to) => {
                close(&mut run, false);
                at = to;
            }
            PathEl::LineTo(to) => run.push(PathSeg::Line(kurbo::Line::new(at, to))),
            PathEl::QuadTo(a, to) => run.push(PathSeg::Quad(kurbo::QuadBez::new(at, a, to))),
            PathEl::CurveTo(a, b, to) => run.push(PathSeg::Cubic(kurbo::CubicBez::new(at, a, b, to))),
            PathEl::ClosePath => close(&mut run, true),
        }
        at = element.end_point().unwrap_or(at);
    }
    close(&mut run, false);
    found
}

/// The area a stroke of `style` covers on `shape`, to be filled non-zero.
/// Empty for a weight of nothing. A path that isn't closed has no inside
/// or outside, so its stroke is centred, and has the style's caps.
pub fn outline(shape: &BezPath, style: &Style) -> BezPath {
    let weight = style.weight;
    if weight.is_nan() || weight <= 0.0 {
        return BezPath::new();
    }
    let open = ends(shape);
    let align = if open.is_empty() { style.align } else { Align::Center };
    // Inside and outside are half of a centred stroke twice as wide: the
    // half in the shape, or the half out of it.
    let width = if align == Align::Center { weight } else { weight * 2.0 };
    let cap = |cap: Cap| match cap {
        Cap::Round => kurbo::Cap::Round,
        Cap::Square => kurbo::Cap::Square,
        Cap::None | Cap::Arrow | Cap::Triangle => kurbo::Cap::Butt,
    };
    let join = match style.join {
        Join::Miter => kurbo::Join::Miter,
        Join::Bevel => kurbo::Join::Bevel,
        Join::Round => kurbo::Join::Round,
    };
    let pen = Stroke::new(width).with_join(join).with_start_cap(cap(style.start)).with_end_cap(cap(style.end));
    let band = kurbo::stroke(shape.elements().iter().copied(), &pen, &StrokeOpts::default(), TOLERANCE);
    let combine = |a: &BezPath, b: &BezPath, operation| boolean(&PathData::from_bezpath(a), FillRule::NonZero, &PathData::from_bezpath(b), FillRule::NonZero, operation).to_bezpath();
    let mut band = match align {
        Align::Center => band,
        Align::Inside => combine(&band, shape, BoolOp::Intersect),
        Align::Outside => combine(&band, shape, BoolOp::Difference),
    };
    for [(start, out), (end, on)] in open {
        for head in [head(style.start, start, out, weight), head(style.end, end, on, weight)].into_iter().flatten() {
            band = combine(&band, &head, BoolOp::Union);
        }
    }
    band
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Circle, Rect, Shape};

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
        let at = |align| points.map(|(x, y)| covers(&outline(&shape, &Style::new(10.0, align)), x, y));
        assert_eq!(at(Align::Inside), [true, false, false, false, true]);
        assert_eq!(at(Align::Center), [true, true, false, false, false]);
        assert_eq!(at(Align::Outside), [false, true, false, true, false]);
        // With mitred corners: 100 × 50 less 80 × 30, 110 × 60 less 90 × 40,
        // and 120 × 70 less 100 × 50.
        for (align, expected) in [(Align::Inside, 2600.0), (Align::Center, 3000.0), (Align::Outside, 3400.0)] {
            let covered = area(&outline(&shape, &Style::new(10.0, align)));
            assert!((covered - expected).abs() < 1.0, "{align:?}: {covered}");
        }
        // A mitred corner is square: the outside stroke reaches its tip.
        assert!(covers(&outline(&shape, &Style::new(10.0, Align::Outside)), -9.0, -9.0));
    }

    #[test]
    fn a_circles_stroke_is_a_ring() {
        let shape = Circle::new((0.0, 0.0), 50.0).to_path(1e-4);
        let ring = |outer: f64, inner: f64| std::f64::consts::PI * (outer * outer - inner * inner);
        for (align, expected) in [(Align::Inside, ring(50.0, 40.0)), (Align::Center, ring(55.0, 45.0)), (Align::Outside, ring(60.0, 50.0))] {
            let covered = area(&outline(&shape, &Style::new(10.0, align)));
            assert!((covered - expected).abs() < expected * 0.002, "{align:?}: {covered} against {expected}");
        }
        assert!(!covers(&outline(&shape, &Style::new(10.0, Align::Inside)), 0.0, 0.0));
    }

    #[test]
    fn a_path_that_ends_where_it_began_without_closing_is_still_open() {
        // As kurbo's own ellipse does. Its stroke is centred, with a seam.
        let unclosed = kurbo::Ellipse::new((0.0, 0.0), (50.0, 50.0), 0.0).to_path(1e-4);
        assert!(!unclosed.elements().iter().any(|element| matches!(element, PathEl::ClosePath)));
        let ring = |outer: f64, inner: f64| std::f64::consts::PI * (outer * outer - inner * inner);
        let covered = area(&outline(&unclosed, &Style::new(10.0, Align::Inside)));
        assert!((covered - ring(55.0, 45.0)).abs() < ring(55.0, 45.0) * 0.002, "{covered}");
    }

    #[test]
    fn no_weight_is_no_stroke() {
        let shape = Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1);
        for weight in [0.0, -1.0, f64::NAN] {
            assert!(outline(&shape, &Style::new(weight, Align::Center)).elements().is_empty());
        }
    }

    /// A line from (0, 0) to (100, 0).
    fn line() -> BezPath {
        let mut line = BezPath::new();
        line.move_to((0.0, 0.0));
        line.line_to((100.0, 0.0));
        line
    }

    #[test]
    fn an_open_path_is_stroked_down_its_middle_whatever_side_is_asked_for() {
        for align in [Align::Inside, Align::Center, Align::Outside] {
            let band = outline(&line(), &Style::new(10.0, align));
            assert!((area(&band) - 1000.0).abs() < 1e-6, "{align:?}: {}", area(&band));
            assert!(covers(&band, 50.0, 4.0) && covers(&band, 50.0, -4.0) && !covers(&band, 50.0, 6.0));
            assert!(!covers(&band, -1.0, 0.0) && !covers(&band, 101.0, 0.0));
        }
    }

    #[test]
    fn caps_end_an_open_path_each_in_its_own_way() {
        let with = |start, end| outline(&line(), &Style { start, end, ..Style::new(10.0, Align::Center) });
        // Half a circle of radius 5 at the end; nothing more at the start.
        let round = with(Cap::None, Cap::Round);
        assert!((area(&round) - (1000.0 + std::f64::consts::PI * 12.5)).abs() < 0.05, "{}", area(&round));
        assert!(covers(&round, 104.0, 0.0) && !covers(&round, 104.0, 4.0) && !covers(&round, -1.0, 0.0));
        // Five more units, square, at each end.
        let square = with(Cap::Square, Cap::Square);
        assert!((area(&square) - 1100.0).abs() < 1e-6);
        assert!(covers(&square, 104.0, 4.0) && covers(&square, -4.0, -4.0));
        // An open arrowhead: arms back at 45° from the tip, 40 long.
        let arrow = with(Cap::None, Cap::Arrow);
        assert!(covers(&arrow, 80.0, 20.0) && covers(&arrow, 80.0, -20.0), "on the arms");
        assert!(!covers(&arrow, 80.0, 10.0), "between an arm and the line");
        assert!(!covers(&arrow, 20.0, 20.0) && !covers(&arrow, -1.0, 0.0), "nothing at the start");
        // A filled one: 40 a side, its point 10 past the end.
        let triangle = with(Cap::Triangle, Cap::None);
        assert!(covers(&triangle, -9.0, 0.0) && covers(&triangle, 20.0, 15.0) && covers(&triangle, 20.0, -15.0));
        assert!(!covers(&triangle, -11.0, 0.0) && !covers(&triangle, 30.0, 15.0));
        let expected = 1000.0 + 3f64.sqrt() / 4.0 * 1600.0 - (40.0 * 3f64.sqrt() / 2.0 - 10.0) * 10.0;
        // Less the piece of the line under it, which is 10 wide but
        // narrower than that right at the point.
        assert!((area(&triangle) - expected).abs() < 30.0, "{} against {expected}", area(&triangle));
        // A closed shape has no ends to cap.
        let shape = Rect::new(0.0, 0.0, 100.0, 50.0).to_path(0.1);
        let capped = outline(&shape, &Style { start: Cap::Triangle, end: Cap::Round, ..Style::new(10.0, Align::Center) });
        assert!((area(&capped) - 3000.0).abs() < 1.0);
    }

    #[test]
    fn joins_turn_a_corner_square_cut_off_or_round() {
        let shape = Rect::new(0.0, 0.0, 100.0, 50.0).to_path(0.1);
        let with = |join| outline(&shape, &Style { join, ..Style::new(10.0, Align::Outside) });
        // Just inside the tip of the corner, and well inside its quarter circle.
        let at = |join| (covers(&with(join), -9.0, -9.0), covers(&with(join), -6.0, -6.0), covers(&with(join), -3.0, -3.0));
        assert_eq!(at(Join::Miter), (true, true, true));
        assert_eq!(at(Join::Round), (false, true, true));
        assert_eq!(at(Join::Bevel), (false, false, true));
    }
}
