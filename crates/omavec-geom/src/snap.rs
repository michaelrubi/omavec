//! Arithmetic of smart guides: snapping a moving or resized box to edges and
//! middles of other boxes.

use kurbo::{Line, Rect, Vec2};

/// What snapping found: how far to shift, and the lines to draw to show why.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snap {
    pub by: Vec2,
    pub guides: Vec<Line>,
}

/// Which way an edge runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis { X, Y }

/// Snaps the box `moving` to `others`.
pub fn snap_move(moving: Rect, others: &[Rect], reach: f64) -> Snap {
    if !is_finite(moving) || reach.is_nan() || reach.is_sign_negative() {
        return Snap::default();
    }
    let snap_x = best_axis_shift([moving.x0, (moving.x0 + moving.x1) / 2.0, moving.x1], others, Axis::X, reach);
    let snap_y = best_axis_shift([moving.y0, (moving.y0 + moving.y1) / 2.0, moving.y1], others, Axis::Y, reach);
    let by = Vec2::new(snap_x.unwrap_or(0.0), snap_y.unwrap_or(0.0));
    let shifted = Rect::new(moving.x0 + by.x, moving.y0 + by.y, moving.x1 + by.x, moving.y1 + by.y);
    let mut guides = Vec::new();
    if snap_x.is_some() {
        guides.extend(axis_guides(Axis::X, shifted, others));
    }
    if snap_y.is_some() {
        guides.extend(axis_guides(Axis::Y, shifted, others));
    }
    Snap { by, guides }
}

/// Snaps one coordinate, as when an edge is dragged to resize a box: `at` is the edge's x
/// (for `Axis::X`) or y (for `Axis::Y`); `span` is the box's extent along the other axis, as
/// (low, high), used only for the guide. Returns the coordinate to use instead and the guide,
/// or `None` if nothing is within reach.
pub fn snap_edge(at: f64, axis: Axis, span: (f64, f64), others: &[Rect], reach: f64) -> Option<(f64, Line)> {
    if !at.is_finite() || !span.0.is_finite() || !span.1.is_finite() || reach.is_nan() || reach.is_sign_negative() {
        return None;
    }
    let mut best: Option<(f64, f64)> = None;
    for &b in others {
        if !is_finite(b) { continue; }
        for o in axis_lines(b, axis) {
            let dist = (o - at).abs();
            if dist <= reach && best.is_none_or(|(d, _)| dist < d) {
                best = Some((dist, o));
            }
        }
    }
    let (_, coord) = best?;
    Some((coord, make_guide(axis, coord, (span.0.min(span.1), span.0.max(span.1)), others)))
}

fn is_finite(r: Rect) -> bool {
    r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()
}

fn axis_lines(r: Rect, axis: Axis) -> [f64; 3] {
    match axis {
        Axis::X => [r.x0, (r.x0 + r.x1) / 2.0, r.x1],
        Axis::Y => [r.y0, (r.y0 + r.y1) / 2.0, r.y1],
    }
}

fn best_axis_shift(moving_lines: [f64; 3], others: &[Rect], axis: Axis, reach: f64) -> Option<f64> {
    let mut best: Option<(f64, f64)> = None;
    for &b in others {
        if !is_finite(b) { continue; }
        for o in axis_lines(b, axis) {
            for m in moving_lines {
                let diff = o - m;
                let dist = diff.abs();
                if dist <= reach && best.is_none_or(|(d, _)| dist < d) {
                    best = Some((dist, diff));
                }
            }
        }
    }
    best.map(|(_, diff)| if diff.abs() <= 1e-9 { 0.0 } else { diff })
}

fn axis_guides(axis: Axis, shifted: Rect, others: &[Rect]) -> Vec<Line> {
    let shifted_lines = axis_lines(shifted, axis);
    let mut qualifying: Vec<f64> = Vec::new();
    for &b in others {
        if !is_finite(b) { continue; }
        for o in axis_lines(b, axis) {
            if shifted_lines.iter().any(|&sl| (o - sl).abs() <= 1e-9) {
                qualifying.push(o);
            }
        }
    }
    qualifying.sort_by(|a, b| a.total_cmp(b));
    let mut distinct: Vec<f64> = Vec::new();
    for o in qualifying {
        if distinct.last().is_none_or(|&last| (o - last).abs() > 1e-9) {
            distinct.push(o);
        }
    }
    let shifted_other_span = match axis {
        Axis::X => (shifted.y0.min(shifted.y1), shifted.y0.max(shifted.y1)),
        Axis::Y => (shifted.x0.min(shifted.x1), shifted.x0.max(shifted.x1)),
    };
    distinct.into_iter().map(|o| make_guide(axis, o, shifted_other_span, others)).collect()
}

fn make_guide(axis: Axis, coord: f64, mut span: (f64, f64), others: &[Rect]) -> Line {
    for &b in others {
        if !is_finite(b) { continue; }
        if axis_lines(b, axis).iter().any(|&l| (l - coord).abs() <= 1e-9) {
            let (low, high) = match axis {
                Axis::X => (b.y0.min(b.y1), b.y0.max(b.y1)),
                Axis::Y => (b.x0.min(b.x1), b.x0.max(b.x1)),
            };
            span.0 = span.0.min(low);
            span.1 = span.1.max(high);
        }
    }
    match axis {
        Axis::X => Line::new((coord, span.0), (coord, span.1)),
        Axis::Y => Line::new((span.0, coord), (span.1, coord)),
    }
}
