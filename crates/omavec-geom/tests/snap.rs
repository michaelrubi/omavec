use kurbo::{Line, Rect, Vec2};
use omavec_geom::snap::{Axis, Snap, snap_edge, snap_move};
use proptest::prelude::*;

fn is_finite_rect(r: Rect) -> bool {
    r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()
}

fn oracle_axis(moving: Rect, others: &[Rect], axis: Axis, reach: f64) -> Option<f64> {
    if !is_finite_rect(moving) || reach.is_nan() || reach.is_sign_negative() {
        return None;
    }
    let moving_lines = match axis {
        Axis::X => [moving.x0, (moving.x0 + moving.x1) / 2.0, moving.x1],
        Axis::Y => [moving.y0, (moving.y0 + moving.y1) / 2.0, moving.y1],
    };
    let mut min_dist: Option<f64> = None;
    for &o in others {
        if !is_finite_rect(o) {
            continue;
        }
        let other_lines = match axis {
            Axis::X => [o.x0, (o.x0 + o.x1) / 2.0, o.x1],
            Axis::Y => [o.y0, (o.y0 + o.y1) / 2.0, o.y1],
        };
        for m in moving_lines {
            for ol in other_lines {
                let dist = (ol - m).abs();
                if dist <= reach {
                    match min_dist {
                        None => min_dist = Some(dist),
                        Some(d) if dist < d => min_dist = Some(dist),
                        _ => {}
                    }
                }
            }
        }
    }
    min_dist
}

#[test]
fn hand_case_1_x_edge_snap() {
    let moving = Rect::new(103.0, 50.0, 153.0, 80.0);
    let others = [Rect::new(100.0, 200.0, 180.0, 260.0)];
    let reach = 5.0;
    assert_eq!(
        snap_move(moving, &others, reach),
        Snap {
            by: Vec2::new(-3.0, 0.0),
            guides: vec![Line::new((100.0, 50.0), (100.0, 260.0))],
        }
    );
}

#[test]
fn hand_case_2_middle_to_middle() {
    let others = [Rect::new(100.0, 200.0, 180.0, 260.0)];
    let moving = Rect::new(112.0, 50.0, 162.0, 80.0);
    let reach = 5.0;
    assert_eq!(
        snap_move(moving, &others, reach),
        Snap {
            by: Vec2::new(3.0, 0.0),
            guides: vec![Line::new((140.0, 50.0), (140.0, 260.0))],
        }
    );
}

#[test]
fn hand_case_3_out_of_reach() {
    let others = [Rect::new(100.0, 200.0, 180.0, 260.0)];
    let moving = Rect::new(0.0, 0.0, 10.0, 10.0);
    let reach = 5.0;
    assert_eq!(snap_move(moving, &others, reach), Snap::default());
}

#[test]
fn hand_case_4_both_axes() {
    let others = [Rect::new(100.0, 200.0, 180.0, 260.0)];
    let moving = Rect::new(98.0, 262.0, 120.0, 290.0);
    let reach = 5.0;
    assert_eq!(
        snap_move(moving, &others, reach),
        Snap {
            by: Vec2::new(2.0, -2.0),
            guides: vec![
                Line::new((100.0, 200.0), (100.0, 288.0)),
                Line::new((100.0, 260.0), (180.0, 260.0)),
            ],
        }
    );
}

#[test]
fn hand_case_5_column_aligned_others_one_vertical_guide() {
    let others = [
        Rect::new(100.0, 0.0, 120.0, 50.0),
        Rect::new(100.0, 100.0, 130.0, 150.0),
    ];
    let moving = Rect::new(102.0, 200.0, 172.0, 250.0);
    let reach = 5.0;
    assert_eq!(
        snap_move(moving, &others, reach),
        Snap {
            by: Vec2::new(-2.0, 0.0),
            guides: vec![Line::new((100.0, 0.0), (100.0, 250.0))],
        }
    );
}

#[test]
fn hand_case_6_snap_edge() {
    let others = [Rect::new(100.0, 200.0, 180.0, 260.0)];
    assert_eq!(
        snap_edge(178.5, Axis::X, (0.0, 10.0), &others, 2.0),
        Some((180.0, Line::new((180.0, 0.0), (180.0, 260.0))))
    );
    assert_eq!(
        snap_edge(178.5, Axis::X, (0.0, 10.0), &others, 1.0),
        None
    );
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn proptest_snap_properties_against_oracle(
        (mx0, my0, mx1, my1) in (-1000.0..1000.0, -1000.0..1000.0, -1000.0..1000.0, -1000.0..1000.0),
        others_data in prop::collection::vec(
            (-1000.0..1000.0, -1000.0..1000.0, -1000.0..1000.0, -1000.0..1000.0),
            0..=6
        ),
        reach in -10.0..100.0f64,
    ) {
        let moving = Rect::new(mx0, my0, mx1, my1);
        let others: Vec<Rect> = others_data
            .into_iter()
            .map(|(x0, y0, x1, y1)| Rect::new(x0, y0, x1, y1))
            .collect();
        let snap = snap_move(moving, &others, reach);

        let opt_x = oracle_axis(moving, &others, Axis::X, reach);
        let opt_y = oracle_axis(moving, &others, Axis::Y, reach);

        // 1. by.x.abs() equals oracle smallest distance if within reach, else 0.0
        match opt_x {
            Some(d) => {
                let expected = if d <= 1e-9 { 0.0 } else { d };
                assert!((snap.by.x.abs() - expected).abs() <= 1e-12, "x distance mismatch: {} vs {}", snap.by.x.abs(), d);
            }
            None => assert_eq!(snap.by.x, 0.0),
        }
        match opt_y {
            Some(d) => {
                let expected = if d <= 1e-9 { 0.0 } else { d };
                assert!((snap.by.y.abs() - expected).abs() <= 1e-12, "y distance mismatch: {} vs {}", snap.by.y.abs(), d);
            }
            None => assert_eq!(snap.by.y, 0.0),
        }

        let shifted = Rect::new(
            moving.x0 + snap.by.x,
            moving.y0 + snap.by.y,
            moving.x1 + snap.by.x,
            moving.y1 + snap.by.y,
        );
        let shifted_x = [shifted.x0, (shifted.x0 + shifted.x1) / 2.0, shifted.x1];
        let shifted_y = [shifted.y0, (shifted.y0 + shifted.y1) / 2.0, shifted.y1];

        let vert_guides = snap.guides.iter().filter(|g| g.p0.x == g.p1.x).count();
        let horiz_guides = snap.guides.iter().filter(|g| g.p0.y == g.p1.y).count();

        // 2. After shifting, if axis snapped, at least one line equals a line of some other
        // to within 1e-9, and >= 1 guide on that axis; if not snapped, none on that axis.
        if opt_x.is_some() {
            let aligned = others.iter().filter(|o| is_finite_rect(**o)).any(|o| {
                let lines = [o.x0, (o.x0 + o.x1) / 2.0, o.x1];
                lines.iter().any(|&ol| shifted_x.iter().any(|&sl| (ol - sl).abs() <= 1e-9))
            });
            assert!(aligned, "shifted box has no x line matching any other within 1e-9");
            assert!(vert_guides >= 1, "expected at least one vertical guide");
        } else {
            assert_eq!(vert_guides, 0, "expected no vertical guides when x did not snap");
        }

        if opt_y.is_some() {
            let aligned = others.iter().filter(|o| is_finite_rect(**o)).any(|o| {
                let lines = [o.y0, (o.y0 + o.y1) / 2.0, o.y1];
                lines.iter().any(|&ol| shifted_y.iter().any(|&sl| (ol - sl).abs() <= 1e-9))
            });
            assert!(aligned, "shifted box has no y line matching any other within 1e-9");
            assert!(horiz_guides >= 1, "expected at least one horizontal guide");
        } else {
            assert_eq!(horiz_guides, 0, "expected no horizontal guides when y did not snap");
        }

        // 3. Every guide is exactly vertical or horizontal, p0 <= p1 along length, and contains shifted box's extent.
        for g in &snap.guides {
            let is_vert = g.p0.x == g.p1.x;
            let is_horiz = g.p0.y == g.p1.y;
            assert!(is_vert || is_horiz, "guide is neither vertical nor horizontal: {:?}", g);
            if is_vert {
                assert!(g.p0.y <= g.p1.y, "vertical guide p0.y > p1.y: {:?}", g);
                let shift_min_y = shifted.y0.min(shifted.y1);
                let shift_max_y = shifted.y0.max(shifted.y1);
                assert!(g.p0.y <= shift_min_y + 1e-9, "guide does not cover shifted min y");
                assert!(g.p1.y >= shift_max_y - 1e-9, "guide does not cover shifted max y");
            } else {
                assert!(g.p0.x <= g.p1.x, "horizontal guide p0.x > p1.x: {:?}", g);
                let shift_min_x = shifted.x0.min(shifted.x1);
                let shift_max_x = shifted.x0.max(shifted.x1);
                assert!(g.p0.x <= shift_min_x + 1e-9, "guide does not cover shifted min x");
                assert!(g.p1.x >= shift_max_x - 1e-9, "guide does not cover shifted max x");
            }
        }

        // 4. Snapping the already-snapped box again gives by == Vec2::ZERO on each axis that snapped.
        let snap2 = snap_move(shifted, &others, reach);
        if opt_x.is_some() {
            assert_eq!(snap2.by.x, 0.0, "second snap by.x != 0 on snapped axis");
        }
        if opt_y.is_some() {
            assert_eq!(snap2.by.y, 0.0, "second snap by.y != 0 on snapped axis");
        }
    }

    #[test]
    fn proptest_any_floats_never_panic_and_results_are_finite(
        moving_coords in (any::<f64>(), any::<f64>(), any::<f64>(), any::<f64>()),
        others_coords in prop::collection::vec(
            (any::<f64>(), any::<f64>(), any::<f64>(), any::<f64>()),
            0..=6
        ),
        reach in any::<f64>(),
        edge_at in any::<f64>(),
        span in (any::<f64>(), any::<f64>()),
        axis_x in any::<bool>(),
    ) {
        let moving = Rect::new(moving_coords.0, moving_coords.1, moving_coords.2, moving_coords.3);
        let others: Vec<Rect> = others_coords
            .into_iter()
            .map(|(x0, y0, x1, y1)| Rect::new(x0, y0, x1, y1))
            .collect();
        let snap = snap_move(moving, &others, reach);
        assert!(snap.by.x.is_finite());
        assert!(snap.by.y.is_finite());
        for g in &snap.guides {
            assert!(g.p0.x.is_finite());
            assert!(g.p0.y.is_finite());
            assert!(g.p1.x.is_finite());
            assert!(g.p1.y.is_finite());
        }

        let axis = if axis_x { Axis::X } else { Axis::Y };
        if let Some((coord, guide)) = snap_edge(edge_at, axis, span, &others, reach) {
            assert!(coord.is_finite());
            assert!(guide.p0.x.is_finite());
            assert!(guide.p0.y.is_finite());
            assert!(guide.p1.x.is_finite());
            assert!(guide.p1.y.is_finite());
        }
    }
}
