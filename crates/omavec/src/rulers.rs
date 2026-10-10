//! Rulers and the pixel grid: overlays painted on top of the canvas's frame,
//! as in Figma (Shift+R and Shift+').

use egui::{Align2, FontId, Pos2, Rect, Stroke};

use crate::canvas::View;
use crate::theme::Theme;

/// How wide the ruler strips are, in points.
const STRIP: f32 = 20.0;

/// Document units between labelled ruler ticks at `zoom` points per unit:
/// the smallest of 1, 2, 5, 10, 20, 50, … that puts labels at least 60 points
/// apart.
pub fn step(zoom: f64) -> f64 {
    let mut decade = 1.0;
    loop {
        for multiple in [1.0, 2.0, 5.0] {
            let step = multiple * decade;
            // The cap is the way out for a zoom of zero.
            if step * zoom >= 60.0 || step >= 1e12 || zoom.is_nan() {
                return step;
            }
        }
        decade *= 10.0;
    }
}

/// How many parts minor ticks divide a step into: 5 for a step of 2 × 10ⁿ,
/// otherwise 10, and 1 (no minor ticks) if they'd be under 4 points apart.
pub fn divisions(step: f64, zoom: f64) -> u32 {
    let leading = step / 10.0_f64.powf(step.log10().floor());
    let parts = if (leading - 2.0).abs() < 0.5 { 5 } else { 10 };
    if step / f64::from(parts) * zoom >= 4.0 { parts } else { 1 }
}

/// The multiples of `spacing` document units visible along one axis of a
/// canvas `length` points long whose document origin is `origin` points from
/// its start: each as (points from the canvas's start, document coordinate),
/// ascending.
pub fn marks(origin: f64, zoom: f64, length: f64, spacing: f64) -> Vec<(f64, f64)> {
    let gap = spacing * zoom;
    let first = (-origin / gap).ceil();
    let last = ((length - origin) / gap).floor();
    // Also turns away a zoom, spacing or origin that isn't a usable number.
    if !(gap > 0.0 && gap.is_finite() && length > 0.0 && last >= first && last - first < 1e6) {
        return Vec::new();
    }
    (0..=(last - first) as usize)
        .map(|i| {
            // Adding zero turns the -0.0 a ceiling can give into 0.0.
            let coordinate = (first + i as f64) * spacing + 0.0;
            (origin + coordinate * zoom, coordinate)
        })
        .collect()
}

/// Paints the pixel grid and the rulers over the canvas at `rect`.
pub fn paint(painter: &egui::Painter, rect: Rect, view: View, theme: &Theme, rulers: bool, pixel_grid: bool) {
    // A point `along` an axis and `across` it, for the top ruler and columns
    // (`down` false) or the left ruler and rows (`down` true).
    let at = |down: bool, along: f64, across: f32| {
        if down { Pos2::new(rect.min.x + across, rect.min.y + along as f32) } else { Pos2::new(rect.min.x + along as f32, rect.min.y + across) }
    };
    let axes = [(false, view.origin.x, rect.width(), rect.height()), (true, view.origin.y, rect.height(), rect.width())];

    // One line per document unit, from 400% up.
    if pixel_grid && view.zoom >= 4.0 {
        let stroke = Stroke::new(1.0, theme.foreground.gamma_multiply(0.1));
        for (down, origin, length, breadth) in axes {
            for (along, _) in marks(origin, view.zoom, f64::from(length), 1.0) {
                painter.line_segment([at(down, along, 0.0), at(down, along, breadth)], stroke);
            }
        }
    }

    if !rulers || rect.width() <= STRIP || rect.height() <= STRIP {
        return;
    }
    let stroke = Stroke::new(1.0, theme.muted);
    let step = step(view.zoom);
    let minor = step / f64::from(divisions(step, view.zoom));
    for (down, _, length, _) in axes {
        painter.rect_filled(Rect::from_two_pos(at(down, 0.0, 0.0), at(down, f64::from(length), STRIP)), 0.0, theme.dark_background);
    }
    for (down, origin, length, _) in axes {
        painter.line_segment([at(down, f64::from(STRIP), STRIP), at(down, f64::from(length), STRIP)], stroke);
        // Ticks start past the corner where the two strips meet.
        let past_corner = |marks: Vec<(f64, f64)>| marks.into_iter().filter(|(along, _)| *along >= f64::from(STRIP));
        for (along, _) in past_corner(marks(origin, view.zoom, f64::from(length), minor)) {
            painter.line_segment([at(down, along, STRIP - 3.0), at(down, along, STRIP)], stroke);
        }
        for (along, coordinate) in past_corner(marks(origin, view.zoom, f64::from(length), step)) {
            painter.line_segment([at(down, along, STRIP - 6.0), at(down, along, STRIP)], stroke);
            painter.text(at(down, along + 2.0, 2.0), Align2::LEFT_TOP, format!("{coordinate:.0}"), FontId::monospace(10.0), theme.dark_foreground);
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
