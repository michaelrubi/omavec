//! The outlines of Figma's parametric shapes, in the shape's own
//! coordinates: its box has its corner at the origin, and y points down.
//! A size that isn't one (nothing, negative, or not a number) is an empty
//! path, and no other input can make one panic.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use kurbo::{Arc, BezPath, PathEl, Point, Rect, Size, Vec2};

/// How far a curve may stray from the true one, in document units: a
/// quarter of a pixel at the deepest zoom.
const TOLERANCE: f64 = 1e-3;

/// The box of a shape of `size`, if `size` is one.
fn frame(size: Size) -> Option<Rect> {
    (size.width > 0.0 && size.height > 0.0 && size.is_finite()).then(|| size.to_rect())
}

/// `point`, kept in `area`: at an astronomical size rounding alone can put
/// one a hair outside.
fn within(area: Rect, point: Point) -> Point {
    Point::new(point.x.clamp(area.x0, area.x1), point.y.clamp(area.y0, area.y1))
}

/// Zero for what isn't a number.
fn number(value: f64) -> f64 {
    if value.is_nan() { 0.0 } else { value }
}

/// Carries `path` on round the ellipse about `centre` from the angle
/// `start` (0 at three o'clock, growing clockwise on screen) by `sweep`, at
/// most a whole turn. The pieces stop at each quarter, so no control point
/// lies outside the ellipse's box.
fn arc_to(path: &mut BezPath, area: Rect, centre: Point, radii: Vec2, start: f64, sweep: f64) {
    if radii.x <= 0.0 || radii.y <= 0.0 {
        return;
    }
    // kurbo counts its segments from the tolerance, and at an astronomical
    // radius would go on counting.
    let tolerance = TOLERANCE.max(radii.x.max(radii.y) / 1e12);
    let (low, high) = (start.min(start + sweep), start.max(start + sweep));
    let quarter = |angle: f64| (angle / FRAC_PI_2).floor() as i64;
    let mut stops: Vec<f64> = (quarter(low)..=quarter(high) + 1).map(|k| k as f64 * FRAC_PI_2).filter(|stop| *stop > low + 1e-12 && *stop < high - 1e-12).collect();
    if sweep < 0.0 {
        stops.reverse();
    }
    let mut at = start;
    for next in stops.into_iter().chain([start + sweep]) {
        for element in Arc::new(centre, radii, at, next - at, 0.0).append_iter(tolerance) {
            if let PathEl::CurveTo(a, b, to) = element {
                path.curve_to(within(area, a), within(area, b), within(area, to));
            }
        }
        at = next;
    }
}

/// A rectangle with a radius for each corner: top-left, top-right,
/// bottom-right, bottom-left. As in CSS, if the radii along any side come
/// to more than the side, all four shrink together until they fit.
pub fn rounded_rect(size: Size, radii: [f64; 4]) -> BezPath {
    let Some(area) = frame(size) else { return BezPath::new() };
    let (w, h) = (size.width, size.height);
    let r = radii.map(|radius| if radius > 0.0 { radius.min(1e300) } else { 0.0 });
    let sides = [(w, r[0] + r[1]), (h, r[1] + r[2]), (w, r[2] + r[3]), (h, r[3] + r[0])];
    let fit = sides.into_iter().filter(|(_, along)| *along > 0.0).map(|(side, along)| side / along).fold(1.0, f64::min);
    let r = r.map(|radius| (radius * fit).min(w.min(h)));
    // Each corner clockwise from the top-left: where its arc begins, the
    // middle of its circle, and the angle the arc begins at.
    let corners = [((0.0, r[0]), (r[0], r[0]), PI), ((w - r[1], 0.0), (w - r[1], r[1]), 1.5 * PI), ((w, h - r[2]), (w - r[2], h - r[2]), 0.0), ((r[3], h), (r[3], h - r[3]), FRAC_PI_2)];
    let mut path = BezPath::new();
    let mut last = None;
    for (index, (from, centre, start)) in corners.into_iter().enumerate() {
        let from = within(area, from.into());
        match last {
            None => path.move_to(from),
            // Two corners that meet leave no side between them.
            Some(last) if last == from => {}
            Some(_) => path.line_to(from),
        }
        arc_to(&mut path, area, centre.into(), Vec2::new(r[index], r[index]), start, FRAC_PI_2);
        last = path.current_position();
    }
    path.close_path();
    path
}

/// Figma's ellipse with its arc settings. `start` is the angle the arc
/// begins at, in radians, 0 at three o'clock and growing clockwise on
/// screen; `sweep` is how far it goes from there, up to a whole turn
/// either way; `ratio` is the hole's radius as a fraction of the
/// ellipse's. A whole turn is the ellipse, or a ring (two loops, one each
/// way, so it fills as a ring by either rule); less is a pie slice, or a
/// piece of the ring. No sweep at all is an empty path.
pub fn arc(size: Size, start: f64, sweep: f64, ratio: f64) -> BezPath {
    let Some(area) = frame(size) else { return BezPath::new() };
    let start = if start.is_finite() { start.rem_euclid(TAU) } else { 0.0 };
    let (sweep, ratio) = (number(sweep).clamp(-TAU, TAU), number(ratio).clamp(0.0, 1.0));
    let mut path = BezPath::new();
    if sweep == 0.0 {
        return path;
    }
    let (centre, outer) = (area.center(), Vec2::new(size.width / 2.0, size.height / 2.0));
    let (whole, inner) = (sweep.abs() >= TAU - 1e-12, outer * ratio);
    let on = |radii: Vec2, angle: f64| within(area, centre + Vec2::new(radii.x * angle.cos(), radii.y * angle.sin()));
    if !whole && ratio == 0.0 {
        path.move_to(centre);
        path.line_to(on(outer, start));
    } else {
        path.move_to(on(outer, start));
    }
    arc_to(&mut path, area, centre, outer, start, sweep);
    if ratio > 0.0 {
        // Back along the hole's edge: a loop of its own if it goes all
        // the way round.
        if whole {
            path.close_path();
            path.move_to(on(inner, start + sweep));
        } else {
            path.line_to(on(inner, start + sweep));
        }
        arc_to(&mut path, area, centre, inner, start + sweep, -sweep);
    }
    path.close_path();
    path
}

/// `count` corners spaced evenly round the ellipse in `area`, the first at
/// the top, going clockwise on screen. Every other one is pulled in to
/// `inner` of the way from the middle.
fn corners(area: Rect, count: u32, inner: f64) -> BezPath {
    let (centre, radii) = (area.center(), Vec2::new(area.width() / 2.0, area.height() / 2.0));
    let mut path = BezPath::new();
    for index in 0..count {
        let angle = TAU * f64::from(index) / f64::from(count) - FRAC_PI_2;
        let reach = if index % 2 == 1 { inner } else { 1.0 };
        let corner = within(area, centre + Vec2::new(radii.x * angle.cos(), radii.y * angle.sin()) * reach);
        if index == 0 {
            path.move_to(corner);
        } else {
            path.line_to(corner);
        }
    }
    path.close_path();
    path
}

/// A regular polygon as Figma draws it: 3 to 100 corners on the ellipse
/// that fits the box, the first at the top.
pub fn polygon(size: Size, sides: u32) -> BezPath {
    frame(size).map(|area| corners(area, sides.clamp(3, 100), 1.0)).unwrap_or_default()
}

/// A star: 3 to 100 points on the ellipse that fits the box, the first at
/// the top, and between each pair a corner `ratio` of the way out from the
/// middle (Figma starts at 0.382).
pub fn star(size: Size, points: u32, ratio: f64) -> BezPath {
    frame(size).map(|area| corners(area, points.clamp(3, 100) * 2, number(ratio).clamp(0.0, 1.0))).unwrap_or_default()
}
