//! Rulers and the pixel grid: overlays painted on top of the canvas frame.
//!
//! Rulers show a 20-point strip along the top and left edges with labelled ticks
//! at round document units. The pixel grid draws 1-point lines along integer
//! document units when zoomed in to 400% or more.

use egui::{Align2, FontId, Pos2, Rect, Stroke, vec2};

use crate::canvas::View;
use crate::theme::Theme;

/// Document units between labelled ruler ticks at `zoom` points per unit.
///
/// Returns the smallest of 1, 2, 5, 10, 20, 50, 100, … document units that puts
/// labels at least 60 points apart at the current zoom, never below 1.0 unit.
pub fn step(zoom: f64) -> f64 {
    if zoom <= 0.0 || !zoom.is_finite() {
        return 100.0;
    }
    let target = (60.0 / zoom).max(1.0);
    let exp = (target.log10().floor() as i32).max(0);
    let pow = 10.0_f64.powi(exp);
    for m in [1.0, 2.0, 5.0, 10.0] {
        let candidate = m * pow;
        if candidate >= 1.0 && candidate * zoom >= 60.0 - 1e-9 {
            return candidate;
        }
    }
    let next_pow = 10.0_f64.powi(exp + 1);
    for m in [1.0, 2.0, 5.0] {
        let candidate = m * next_pow;
        if candidate >= 1.0 && candidate * zoom >= 60.0 - 1e-9 {
            return candidate;
        }
    }
    next_pow * 10.0
}

/// How many parts minor ticks divide a step into (10, 5, or 1 for none).
///
/// Steps of 2 × 10ⁿ divide into 5 parts; all other steps divide into 10 parts.
/// If the spacing between minor ticks (step / parts * zoom) is less than 4 points,
/// minor ticks are dropped entirely and 1 is returned.
pub fn divisions(step: f64, zoom: f64) -> u32 {
    if step <= 0.0 || !step.is_finite() || zoom <= 0.0 || !zoom.is_finite() {
        return 1;
    }
    let exp = step.log10().floor();
    let base = (step / 10.0_f64.powf(exp)).round();
    let nominal = if (base - 2.0).abs() < 1e-6 { 5 } else { 10 };
    let minor_points = (step / f64::from(nominal)) * zoom;
    if minor_points < 4.0 {
        1
    } else {
        nominal
    }
}

/// The integer-multiple-of-`spacing` document coordinates visible along one axis of a
/// canvas `length` points long whose document origin is `origin` points from its start:
/// each as (points from the canvas's start, document coordinate), ascending.
pub fn marks(origin: f64, zoom: f64, length: f64, spacing: f64) -> Vec<(f64, f64)> {
    if length <= 0.0 || zoom <= 0.0 || spacing <= 0.0
        || !origin.is_finite() || !zoom.is_finite() || !length.is_finite() || !spacing.is_finite()
    {
        return Vec::new();
    }
    let step_points = spacing * zoom;
    if step_points <= 0.0 || !step_points.is_finite() {
        return Vec::new();
    }
    let eps = 1e-9;
    let f_k_min = ((-origin - eps) / step_points).ceil();
    let f_k_max = ((length - origin + eps) / step_points).floor();
    if f_k_min > f_k_max || f_k_min < i64::MIN as f64 || f_k_max > i64::MAX as f64 {
        return Vec::new();
    }
    let k_min = f_k_min as i64;
    let k_max = f_k_max as i64;
    let count = match (k_max - k_min).checked_add(1) {
        Some(c) if c >= 0 => c as usize,
        _ => return Vec::new(),
    };
    let mut result = Vec::with_capacity(count);
    for k in k_min..=k_max {
        let doc = k as f64 * spacing;
        let doc = if doc.abs() < 1e-12 { 0.0 } else { doc };
        let mut pt = origin + doc * zoom;
        if pt.abs() < 1e-12 {
            pt = 0.0;
        } else if (pt - length).abs() < 1e-12 {
            pt = length;
        }
        result.push((pt, doc));
    }
    result
}

/// Paints the rulers and pixel grid overlays on top of the rendered frame.
pub fn paint(
    painter: &egui::Painter,
    rect: Rect,
    view: View,
    theme: &Theme,
    rulers: bool,
    pixel_grid: bool,
) {
    if pixel_grid && view.zoom >= 4.0 {
        let stroke = Stroke::new(1.0, theme.foreground.gamma_multiply(0.1));
        for (pt, _) in marks(view.origin.x, view.zoom, f64::from(rect.width()), 1.0) {
            let x = rect.min.x + pt as f32;
            painter.line_segment([Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)], stroke);
        }
        for (pt, _) in marks(view.origin.y, view.zoom, f64::from(rect.height()), 1.0) {
            let y = rect.min.y + pt as f32;
            painter.line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], stroke);
        }
    }

    if rulers && rect.width() > 20.0 && rect.height() > 20.0 {
        let strip_size = 20.0;
        let top_strip = Rect::from_min_size(rect.min, vec2(rect.width(), strip_size));
        let left_strip = Rect::from_min_size(rect.min, vec2(strip_size, rect.height()));
        let corner = Rect::from_min_size(rect.min, vec2(strip_size, strip_size));

        painter.rect_filled(top_strip, 0.0, theme.dark_background);
        painter.rect_filled(left_strip, 0.0, theme.dark_background);
        painter.rect_filled(corner, 0.0, theme.dark_background);

        let tick_stroke = Stroke::new(1.0, theme.muted);
        let inner_edge_stroke = Stroke::new(1.0, theme.muted);

        painter.line_segment(
            [Pos2::new(rect.min.x + strip_size, rect.min.y + strip_size), Pos2::new(rect.max.x, rect.min.y + strip_size)],
            inner_edge_stroke,
        );
        painter.line_segment(
            [Pos2::new(rect.min.x + strip_size, rect.min.y + strip_size), Pos2::new(rect.min.x + strip_size, rect.max.y)],
            inner_edge_stroke,
        );

        let step_val = step(view.zoom);
        let div = divisions(step_val, view.zoom);
        let font_id = FontId::monospace(10.0);

        // Top ruler (horizontal)
        if div > 1 {
            let minor_spacing = step_val / f64::from(div);
            for (pt, _) in marks(view.origin.x, view.zoom, f64::from(rect.width()), minor_spacing) {
                if pt >= f64::from(strip_size) {
                    let x = rect.min.x + pt as f32;
                    painter.line_segment(
                        [Pos2::new(x, rect.min.y + 17.0), Pos2::new(x, rect.min.y + strip_size)],
                        tick_stroke,
                    );
                }
            }
        }
        for (pt, doc) in marks(view.origin.x, view.zoom, f64::from(rect.width()), step_val) {
            if pt >= f64::from(strip_size) {
                let x = rect.min.x + pt as f32;
                painter.line_segment(
                    [Pos2::new(x, rect.min.y + 14.0), Pos2::new(x, rect.min.y + strip_size)],
                    tick_stroke,
                );
                painter.text(
                    Pos2::new(x + 2.0, rect.min.y + 2.0),
                    Align2::LEFT_TOP,
                    format!("{}", doc.round() as i64),
                    font_id.clone(),
                    theme.dark_foreground,
                );
            }
        }

        // Left ruler (vertical)
        if div > 1 {
            let minor_spacing = step_val / f64::from(div);
            for (pt, _) in marks(view.origin.y, view.zoom, f64::from(rect.height()), minor_spacing) {
                if pt >= f64::from(strip_size) {
                    let y = rect.min.y + pt as f32;
                    painter.line_segment(
                        [Pos2::new(rect.min.x + 17.0, y), Pos2::new(rect.min.x + strip_size, y)],
                        tick_stroke,
                    );
                }
            }
        }
        for (pt, doc) in marks(view.origin.y, view.zoom, f64::from(rect.height()), step_val) {
            if pt >= f64::from(strip_size) {
                let y = rect.min.y + pt as f32;
                painter.line_segment(
                    [Pos2::new(rect.min.x + 14.0, y), Pos2::new(rect.min.x + strip_size, y)],
                    tick_stroke,
                );
                painter.text(
                    Pos2::new(rect.min.x + 2.0, y + 2.0),
                    Align2::LEFT_TOP,
                    format!("{}", doc.round() as i64),
                    font_id.clone(),
                    theme.dark_foreground,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_values_at_various_zooms() {
        assert_eq!(step(1.0), 100.0);
        assert_eq!(step(2.0), 50.0);
        assert_eq!(step(0.05), 2000.0);
        assert_eq!(step(64.0), 1.0);
        assert_eq!(step(256.0), 1.0);
        assert_eq!(step(0.7), 100.0);
    }

    #[test]
    fn division_counts() {
        assert_eq!(divisions(100.0, 1.0), 10);
        assert_eq!(divisions(2000.0, 0.05), 5);
        assert_eq!(divisions(50.0, 2.0), 10);
        assert_eq!(divisions(1.0, 5.0), 1);

        // Both sides of the 4-point limit:
        // Nominal 10 (step 1.0, minor spacing 0.1 * zoom):
        assert_eq!(divisions(1.0, 39.9), 1);
        assert_eq!(divisions(1.0, 40.0), 10);

        // Nominal 5 (step 20.0, minor spacing (20 / 5) * zoom = 4.0 * zoom):
        assert_eq!(divisions(20.0, 0.99), 1);
        assert_eq!(divisions(20.0, 1.0), 5);
    }

    #[test]
    fn marks_generation() {
        assert_eq!(
            marks(30.0, 2.0, 100.0, 10.0),
            vec![(10.0, -10.0), (30.0, 0.0), (50.0, 10.0), (70.0, 20.0), (90.0, 30.0)]
        );
        assert_eq!(
            marks(-1000.0, 4.0, 20.0, 1.0),
            vec![(0.0, 250.0), (4.0, 251.0), (8.0, 252.0), (12.0, 253.0), (16.0, 254.0), (20.0, 255.0)]
        );
        assert!(marks(0.0, 1.0, 0.0, 10.0).is_empty());
        assert!(marks(0.0, -1.0, 100.0, 10.0).is_empty());
        assert!(marks(0.0, f64::NAN, 100.0, 10.0).is_empty());
        assert!(marks(0.0, f64::INFINITY, 100.0, 10.0).is_empty());
        assert!(marks(0.0, 1.0, 100.0, 0.0).is_empty());
        assert!(marks(0.0, 1.0, 100.0, -5.0).is_empty());
        assert!(marks(0.0, 1.0, 100.0, f64::NAN).is_empty());
    }

    #[test]
    fn marks_entry_count_limit() {
        let zoom = 256.0;
        let origin = -2_000_000.0;
        let length = 1920.0;
        let spacing = 1.0;
        let entries = marks(origin, zoom, length, spacing);
        let max_allowed = (length / (spacing * zoom)) + 2.0;
        assert!(entries.len() as f64 <= max_allowed, "got {} entries, max allowed was {}", entries.len(), max_allowed);
        assert!(!entries.is_empty());
    }

    #[test]
    fn direction_check_panning_and_zooming() {
        let origin = 50.0;
        let zoom = 2.0;
        let length = 500.0;
        let spacing = 10.0;
        let before = marks(origin, zoom, length, spacing);
        assert!(!before.is_empty());

        // Panning right (larger origin) moves every mark right by the same amount and keeps doc coordinate
        let delta = 30.0;
        let after_pan = marks(origin + delta, zoom, length, spacing);
        for (pt, doc) in &before {
            if *pt + delta <= length {
                assert!(after_pan.iter().any(|(p, d)| (*p - (pt + delta)).abs() < 1e-9 && (*d - doc).abs() < 1e-9));
            }
        }

        // Zooming in about a mark keeps that mark's position
        let (anchor_pt, target_doc) = before[2];
        let zoom_factor = 3.0;
        let new_zoom = zoom * zoom_factor;
        let new_origin = anchor_pt + (origin - anchor_pt) * zoom_factor;
        let after_zoom = marks(new_origin, new_zoom, length, spacing);
        let found = after_zoom.iter().find(|(_, d)| (*d - target_doc).abs() < 1e-9).expect("mark should be present");
        assert!((found.0 - anchor_pt).abs() < 1e-9, "mark position should stay at anchor_pt");
    }
}
