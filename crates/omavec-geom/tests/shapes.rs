use core::f64::consts::{FRAC_PI_2, PI, TAU};
use kurbo::{BezPath, PathEl, Point, Shape, Size};
use omavec_geom::shapes::{arc, polygon, rounded_rect, star};
use proptest::prelude::*;

fn covers_rounded_rect(pt: Point, size: Size, radii: [f64; 4]) -> bool {
    let w = size.width;
    let h = size.height;
    if pt.x < 0.0 || pt.x > w || pt.y < 0.0 || pt.y > h {
        return false;
    }
    let mut r = [
        radii[0].max(0.0),
        radii[1].max(0.0),
        radii[2].max(0.0),
        radii[3].max(0.0),
    ];
    let mut factor = 1.0f64;
    let s_top = r[0] + r[1];
    if s_top > w {
        factor = factor.min(w / s_top);
    }
    let s_right = r[1] + r[2];
    if s_right > h {
        factor = factor.min(h / s_right);
    }
    let s_bottom = r[2] + r[3];
    if s_bottom > w {
        factor = factor.min(w / s_bottom);
    }
    let s_left = r[3] + r[0];
    if s_left > h {
        factor = factor.min(h / s_left);
    }
    if factor < 1.0 {
        r[0] *= factor;
        r[1] *= factor;
        r[2] *= factor;
        r[3] *= factor;
    }

    if pt.x < r[0] && pt.y < r[0] {
        let dx = pt.x - r[0];
        let dy = pt.y - r[0];
        if dx * dx + dy * dy > r[0] * r[0] {
            return false;
        }
    }
    if pt.x > w - r[1] && pt.y < r[1] {
        let dx = pt.x - (w - r[1]);
        let dy = pt.y - r[1];
        if dx * dx + dy * dy > r[1] * r[1] {
            return false;
        }
    }
    if pt.x > w - r[2] && pt.y > h - r[2] {
        let dx = pt.x - (w - r[2]);
        let dy = pt.y - (h - r[2]);
        if dx * dx + dy * dy > r[2] * r[2] {
            return false;
        }
    }
    if pt.x < r[3] && pt.y > h - r[3] {
        let dx = pt.x - r[3];
        let dy = pt.y - (h - r[3]);
        if dx * dx + dy * dy > r[3] * r[3] {
            return false;
        }
    }
    true
}

fn covers_arc(pt: Point, size: Size, start: f64, sweep: f64, ratio: f64) -> bool {
    let a = size.width / 2.0;
    let b = size.height / 2.0;
    let cx = a;
    let cy = b;

    let u = (pt.x - cx) / a;
    let v = (pt.y - cy) / b;
    let r = (u * u + v * v).sqrt();

    let ratio = ratio.clamp(0.0, 1.0);
    if r < ratio || r > 1.0 {
        return false;
    }

    let sweep = sweep.clamp(-TAU, TAU);
    if sweep == 0.0 {
        return false;
    }
    if sweep.abs() >= TAU - 1e-12 {
        return true;
    }

    let theta = v.atan2(u);

    if sweep > 0.0 {
        let d = (theta - start).rem_euclid(TAU);
        d <= sweep
    } else {
        let d = (start - theta).rem_euclid(TAU);
        d <= -sweep
    }
}

fn point_in_polygon(pt: Point, corners: &[Point]) -> bool {
    let mut inside = false;
    let n = corners.len();
    for i in 0..n {
        let p1 = corners[i];
        let p2 = corners[(i + 1) % n];
        if (p1.y > pt.y) != (p2.y > pt.y) {
            let x_inters = p1.x + (pt.y - p1.y) * (p2.x - p1.x) / (p2.y - p1.y);
            if pt.x < x_inters {
                inside = !inside;
            }
        }
    }
    inside
}

fn polygon_corners(size: Size, sides: u32) -> Vec<Point> {
    let sides = sides.clamp(3, 100) as usize;
    let a = size.width / 2.0;
    let b = size.height / 2.0;
    let cx = a;
    let cy = b;
    let step = TAU / (sides as f64);
    let start_angle = -FRAC_PI_2;
    (0..sides)
        .map(|i| {
            let angle = start_angle + (i as f64) * step;
            Point::new(cx + a * angle.cos(), cy + b * angle.sin())
        })
        .collect()
}

fn covers_polygon(pt: Point, size: Size, sides: u32) -> bool {
    let corners = polygon_corners(size, sides);
    point_in_polygon(pt, &corners)
}

fn star_corners(size: Size, points: u32, ratio: f64) -> Vec<Point> {
    let points = points.clamp(3, 100) as usize;
    let ratio = ratio.clamp(0.0, 1.0);
    let a = size.width / 2.0;
    let b = size.height / 2.0;
    let cx = a;
    let cy = b;
    let total = points * 2;
    let step = PI / (points as f64);
    let start_angle = -FRAC_PI_2;
    (0..total)
        .map(|i| {
            let angle = start_angle + (i as f64) * step;
            let (ra, rb) = if i % 2 == 0 {
                (a, b)
            } else {
                (a * ratio, b * ratio)
            };
            Point::new(cx + ra * angle.cos(), cy + rb * angle.sin())
        })
        .collect()
}

fn covers_star(pt: Point, size: Size, points: u32, ratio: f64) -> bool {
    let corners = star_corners(size, points, ratio);
    point_in_polygon(pt, &corners)
}

fn check_point_grid<F: Fn(Point) -> bool>(path: &BezPath, size: Size, predicate: F) {
    let margin_x = size.width * 0.1;
    let margin_y = size.height * 0.1;
    let x_min = -margin_x;
    let x_max = size.width + margin_x;
    let y_min = -margin_y;
    let y_max = size.height + margin_y;

    let delta = 0.02;

    for i in 0..=60 {
        let x = x_min + (i as f64 / 60.0) * (x_max - x_min);
        for j in 0..=60 {
            let y = y_min + (j as f64 / 60.0) * (y_max - y_min);
            let pt = Point::new(x, y);

            let c = predicate(pt);
            let near_edge = predicate(Point::new(x + delta, y)) != c
                || predicate(Point::new(x - delta, y)) != c
                || predicate(Point::new(x, y + delta)) != c
                || predicate(Point::new(x, y - delta)) != c;

            if near_edge {
                continue;
            }

            let path_contains = path.winding(pt) != 0;
            assert_eq!(
                path_contains,
                c,
                "Point grid mismatch at ({}, {}): path winding is {}, predicate is {}",
                x,
                y,
                path.winding(pt),
                c
            );
        }
    }
}

#[test]
fn point_grid_tests() {
    let size = Size::new(200.0, 100.0);

    // Rounded rect
    for radii in [
        [0.0, 0.0, 0.0, 0.0],
        [10.0, 20.0, 30.0, 40.0],
        [500.0, 0.0, 0.0, 0.0],
        [80.0, 80.0, 80.0, 80.0],
    ] {
        let path = rounded_rect(size, radii);
        check_point_grid(&path, size, |pt| covers_rounded_rect(pt, size, radii));
    }

    // Arc
    let arc_cases: &[(f64, f64, f64)] = &[
        (0.0, TAU, 0.0),
        (0.0, TAU, 0.5),
        (0.3, 2.0, 0.0),
        (-1.0, -4.0, 0.4),
        (5.0, 3.0, 0.9),
    ];
    for &(start, sweep, ratio) in arc_cases {
        let path = arc(size, start, sweep, ratio);
        check_point_grid(&path, size, |pt| covers_arc(pt, size, start, sweep, ratio));
    }

    // Polygon
    for sides in [3, 4, 5, 12] {
        let path = polygon(size, sides);
        check_point_grid(&path, size, |pt| covers_polygon(pt, size, sides));
    }

    // Star
    let star_cases: &[(u32, f64)] = &[(5, 0.382), (3, 0.5), (12, 0.9), (5, 0.0), (5, 1.0)];
    for &(points, ratio) in star_cases {
        let path = star(size, points, ratio);
        check_point_grid(&path, size, |pt| covers_star(pt, size, points, ratio));
    }
}

fn assert_area_approx(actual: f64, expected: f64, desc: &str) {
    let diff = (actual - expected).abs();
    let tol = 0.001 * expected.abs().max(1e-4);
    assert!(
        diff <= tol,
        "{}: actual area {} differs from expected {} (diff {}, tol {})",
        desc,
        actual,
        expected,
        diff,
        tol
    );
}

#[test]
fn area_tests() {
    let size = Size::new(200.0, 100.0);
    let (w, h) = (size.width, size.height);
    let (a, b) = (w / 2.0, h / 2.0);

    // Rounded rect
    for radii in [
        [0.0, 0.0, 0.0, 0.0],
        [10.0, 20.0, 30.0, 40.0],
        [500.0, 0.0, 0.0, 0.0],
        [80.0, 80.0, 80.0, 80.0],
    ] {
        let mut r = radii;
        let mut factor = 1.0f64;
        let s_top = r[0] + r[1];
        if s_top > w {
            factor = factor.min(w / s_top);
        }
        let s_right = r[1] + r[2];
        if s_right > h {
            factor = factor.min(h / s_right);
        }
        let s_bottom = r[2] + r[3];
        if s_bottom > w {
            factor = factor.min(w / s_bottom);
        }
        let s_left = r[3] + r[0];
        if s_left > h {
            factor = factor.min(h / s_left);
        }
        if factor < 1.0 {
            r[0] *= factor;
            r[1] *= factor;
            r[2] *= factor;
            r[3] *= factor;
        }

        let expected =
            w * h - (1.0 - PI / 4.0) * (r[0] * r[0] + r[1] * r[1] + r[2] * r[2] + r[3] * r[3]);
        let path = rounded_rect(size, radii);
        let actual = Shape::area(&path).abs();
        assert_area_approx(actual, expected, "rounded_rect area");
    }

    // Arc
    let arc_cases: &[(f64, f64, f64)] = &[
        (0.0, TAU, 0.0),
        (0.0, TAU, 0.5),
        (0.3, 2.0, 0.0),
        (-1.0, -4.0, 0.4),
        (5.0, 3.0, 0.9),
    ];
    for &(start, sweep, ratio) in arc_cases {
        let clamped_sweep = sweep.clamp(-TAU, TAU);
        let clamped_ratio = ratio.clamp(0.0, 1.0);
        let expected = clamped_sweep.abs() / 2.0 * a * b * (1.0 - clamped_ratio * clamped_ratio);
        let path = arc(size, start, sweep, ratio);
        let actual = Shape::area(&path).abs();
        assert_area_approx(actual, expected, "arc area");
    }

    // Polygon
    for sides in [3, 4, 5, 12] {
        let n = sides as f64;
        let expected = n / 2.0 * (TAU / n).sin() * a * b;
        let path = polygon(size, sides);
        let actual = Shape::area(&path).abs();
        assert_area_approx(actual, expected, "polygon area");
    }

    // Star
    let star_cases: &[(u32, f64)] = &[(5, 0.382), (3, 0.5), (12, 0.9), (5, 0.0), (5, 1.0)];
    for &(points, ratio) in star_cases {
        let n = points as f64;
        let expected = n * a * b * ratio * (PI / n).sin();
        let path = star(size, points, ratio);
        let actual = Shape::area(&path).abs();
        assert_area_approx(actual, expected, "star area");
    }
}

#[test]
fn exact_outputs() {
    let p_rect = rounded_rect(Size::new(200.0, 100.0), [0.0; 4]);
    let els = p_rect.elements();
    assert_eq!(els.len(), 5);
    assert_eq!(els[0], PathEl::MoveTo(Point::new(0.0, 0.0)));
    assert_eq!(els[1], PathEl::LineTo(Point::new(200.0, 0.0)));
    assert_eq!(els[2], PathEl::LineTo(Point::new(200.0, 100.0)));
    assert_eq!(els[3], PathEl::LineTo(Point::new(0.0, 100.0)));
    assert_eq!(els[4], PathEl::ClosePath);

    let p_poly = polygon(Size::new(100.0, 100.0), 4);
    let poly_corners: Vec<Point> = p_poly
        .elements()
        .iter()
        .filter_map(|el| match el {
            PathEl::MoveTo(pt) | PathEl::LineTo(pt) => Some(*pt),
            _ => None,
        })
        .collect();
    assert_eq!(poly_corners.len(), 4);
    let expected_poly = [
        Point::new(50.0, 0.0),
        Point::new(100.0, 50.0),
        Point::new(50.0, 100.0),
        Point::new(0.0, 50.0),
    ];
    for (i, (c, e)) in poly_corners.iter().zip(expected_poly.iter()).enumerate() {
        assert!(
            (c.x - e.x).abs() < 1e-9 && (c.y - e.y).abs() < 1e-9,
            "Polygon corner {} mismatch: got {:?}, expected {:?}",
            i,
            c,
            e
        );
    }

    let p_star = star(Size::new(100.0, 100.0), 5, 0.5);
    let star_corners: Vec<Point> = p_star
        .elements()
        .iter()
        .filter_map(|el| match el {
            PathEl::MoveTo(pt) | PathEl::LineTo(pt) => Some(*pt),
            _ => None,
        })
        .collect();
    assert_eq!(star_corners.len(), 10);
    assert!(
        (star_corners[0].x - 50.0).abs() < 1e-9 && (star_corners[0].y - 0.0).abs() < 1e-9,
        "Star first corner mismatch: got {:?}",
        star_corners[0]
    );
}

fn check_path_in_box(path: &BezPath, size: Size) {
    let valid_box =
        size.width > 0.0 && size.height > 0.0 && size.width.is_finite() && size.height.is_finite();

    if !valid_box {
        assert!(path.is_empty(), "path should be empty for invalid size");
        return;
    }

    let min_x = -1e-6;
    let max_x = size.width + 1e-6;
    let min_y = -1e-6;
    let max_y = size.height + 1e-6;

    for el in path.elements() {
        let pts: &[Point] = match el {
            PathEl::MoveTo(p) => core::slice::from_ref(p),
            PathEl::LineTo(p) => core::slice::from_ref(p),
            PathEl::QuadTo(p1, p2) => &[*p1, *p2],
            PathEl::CurveTo(p1, p2, p3) => &[*p1, *p2, *p3],
            PathEl::ClosePath => &[],
        };
        for pt in pts {
            assert!(pt.x.is_finite(), "coord x is not finite: {}", pt.x);
            assert!(pt.y.is_finite(), "coord y is not finite: {}", pt.y);
            assert!(
                pt.x >= min_x && pt.x <= max_x,
                "coord x out of bounds: {} (bounds: [{}, {}], size: {:?})",
                pt.x,
                min_x,
                max_x,
                size
            );
            assert!(
                pt.y >= min_y && pt.y <= max_y,
                "coord y out of bounds: {} (bounds: [{}, {}], size: {:?})",
                pt.y,
                min_y,
                max_y,
                size
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn proptest_rounded_rect(
        w in any::<f64>(),
        h in any::<f64>(),
        r0 in any::<f64>(),
        r1 in any::<f64>(),
        r2 in any::<f64>(),
        r3 in any::<f64>(),
    ) {
        let size = Size::new(w, h);
        let path = rounded_rect(size, [r0, r1, r2, r3]);
        check_path_in_box(&path, size);
    }

    #[test]
    fn proptest_arc(
        w in any::<f64>(),
        h in any::<f64>(),
        start in any::<f64>(),
        sweep in any::<f64>(),
        ratio in any::<f64>(),
    ) {
        let size = Size::new(w, h);
        let path = arc(size, start, sweep, ratio);
        check_path_in_box(&path, size);
    }

    #[test]
    fn proptest_polygon(
        w in any::<f64>(),
        h in any::<f64>(),
        sides in any::<u32>(),
    ) {
        let size = Size::new(w, h);
        let path = polygon(size, sides);
        check_path_in_box(&path, size);
    }

    #[test]
    fn proptest_star(
        w in any::<f64>(),
        h in any::<f64>(),
        points in any::<u32>(),
        ratio in any::<f64>(),
    ) {
        let size = Size::new(w, h);
        let path = star(size, points, ratio);
        check_path_in_box(&path, size);
    }
}
