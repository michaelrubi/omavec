//! Phase 0 spike: what `vectorcraft-pathops` does with curved shapes.
//!
//! Times its booleans and Shape Builder regions, counts the anchors that
//! come back, and checks each result against its inputs on a grid of points
//! without using the library to do it.
//!
//!     cargo run --release -p omavec-geom --example pathops

use std::f64::consts::PI;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use kurbo::{BezPath, Circle, Ellipse, Point, Rect, RoundedRect, Shape as KurboShape, Vec2};
use vectorcraft_geom::{FillRule, PathData};
use vectorcraft_pathops::{
    BoolOp, DEFAULT_PRECISION, Shape, area, boolean, boolean_n, merge_regions,
    region_at, regions, shape_builder, try_boolean, unite_all,
};

type PredFn = fn(&[bool]) -> bool;
type Predicate<'a> = &'a dyn Fn(&[bool]) -> bool;
type OpDef = (&'static str, BoolOp, PredFn);

const NZ: FillRule = FillRule::NonZero;

const OPS: [OpDef; 4] = [
    ("Union", BoolOp::Union, |m| m[0] || m[1]),
    ("Intersect", BoolOp::Intersect, |m| m[0] && m[1]),
    ("Difference", BoolOp::Difference, |m| m[0] && !m[1]),
    ("Xor", BoolOp::Xor, |m| m[0] != m[1]),
];

struct Rng(u64);

impl Rng {
    fn next_f64(&mut self, min: f64, max: f64) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        min + ((self.0 >> 11) as f64 / ((1u64 << 53) as f64)) * (max - min)
    }
}

fn to_path(s: &impl KurboShape) -> (PathData, BezPath) {
    let bp = s.to_path(1e-3);
    (PathData::from_bezpath(&bp), bp)
}

fn two_circles_expected_areas(r1: f64, r2: f64, d: f64) -> [f64; 4] {
    let (a1, a2) = (PI * r1 * r1, PI * r2 * r2);
    let i = if d >= r1 + r2 {
        0.0
    } else if d <= (r1 - r2).abs() {
        PI * r1.min(r2).powi(2)
    } else {
        let (d1, d2) = ((d * d + r1 * r1 - r2 * r2) / (2.0 * d * r1), (d * d + r2 * r2 - r1 * r1) / (2.0 * d * r2));
        let p3 = 0.5 * ((-d + r1 + r2) * (d + r1 - r2) * (d - r1 + r2) * (d + r1 + r2)).max(0.0).sqrt();
        r1 * r1 * d1.clamp(-1.0, 1.0).acos() + r2 * r2 * d2.clamp(-1.0, 1.0).acos() - p3
    };
    [a1 + a2 - i, i, a1 - i, a1 + a2 - 2.0 * i]
}

fn two_rects_expected_areas(r1: Rect, r2: Rect) -> [f64; 4] {
    let (a1, a2) = (r1.width() * r1.height(), r2.width() * r2.height());
    let (ix, iy) = (r1.x1.min(r2.x1) - r1.x0.max(r2.x0), r1.y1.min(r2.y1) - r1.y0.max(r2.y0));
    let i = if ix > 0.0 && iy > 0.0 { ix * iy } else { 0.0 };
    [a1 + a2 - i, i, a1 - i, a1 + a2 - 2.0 * i]
}

fn mismatch_percent(inputs: &[BezPath], pred: Predicate<'_>, result: &BezPath) -> f64 {
    if inputs.is_empty() {
        return 0.0;
    }
    let mut bbox = inputs[0].bounding_box();
    for bp in &inputs[1..] {
        bbox = bbox.union(bp.bounding_box());
    }
    let (w, h) = (bbox.width(), bbox.height());
    let mut inside_in = vec![false; inputs.len()];
    let mut mismatches = 0usize;
    const GRID: usize = 300;
    for iy in 0..GRID {
        let y = bbox.y0 + (iy as f64 + 0.5) * h / GRID as f64;
        for ix in 0..GRID {
            let x = bbox.x0 + (ix as f64 + 0.5) * w / GRID as f64;
            let pt = Point::new(x, y);
            for (k, bp) in inputs.iter().enumerate() {
                inside_in[k] = bp.winding(pt) != 0;
            }
            if pred(&inside_in) != (result.winding(pt) != 0) {
                mismatches += 1;
            }
        }
    }
    (mismatches as f64 / (GRID * GRID) as f64) * 100.0
}

struct Row<'a> {
    case: &'a str,
    op: &'a str,
    time: &'a str,
    in_anch: usize,
    out_anch: &'a str,
    subpaths: &'a str,
    res_area: &'a str,
    exp_area: Option<f64>,
    mismatch: &'a str,
}

impl Row<'_> {
    fn print(&self) {
        let exp_str = self.exp_area.map(|a| format!("{:.2}", a)).unwrap_or_else(|| "-".to_string());
        println!(
            "{:<20} | {:<22} | {:>9} | {:>7} | {:>10} | {:>8} | {:>12} | {:>13} | {:>10}",
            self.case, self.op, self.time, self.in_anch, self.out_anch, self.subpaths, self.res_area, exp_str, self.mismatch
        );
    }
}

fn run_row<F>(
    case: &str,
    op: &str,
    inputs: &[BezPath],
    in_anch: usize,
    exp: Option<f64>,
    pred: Option<Predicate<'_>>,
    mut f: F,
) where
    F: FnMut() -> Result<PathData, String>,
{
    let warm = catch_unwind(AssertUnwindSafe(&mut f));
    let (out_anch, subpaths, res_area, mismatch, time) = match warm {
        Err(_) => ("PANIC".into(), "-".into(), "-".into(), "-".into(), "-".into()),
        Ok(Err(e)) => (format!("ERR: {e}"), "-".into(), "-".into(), "-".into(), "-".into()),
        Ok(Ok(mut last)) => {
            let mut times = Vec::with_capacity(50);
            for _ in 0..50 {
                let t0 = Instant::now();
                match catch_unwind(AssertUnwindSafe(&mut f)) {
                    Ok(Ok(res)) => { times.push(t0.elapsed()); last = res; }
                    Ok(Err(e)) => return Row { case, op, time: "-", in_anch, out_anch: &format!("ERR: {e}"), subpaths: "-", res_area: "-", exp_area: exp, mismatch: "-" }.print(),
                    Err(_) => return Row { case, op, time: "-", in_anch, out_anch: "PANIC", subpaths: "-", res_area: "-", exp_area: exp, mismatch: "-" }.print(),
                }
            }
            times.sort();
            let mis = pred.map(|p| format!("{:.2}%", mismatch_percent(inputs, p, &last.to_bezpath()))).unwrap_or_else(|| "-".into());
            (last.anchor_count().to_string(), last.subpaths.len().to_string(), format!("{:.2}", area(&last, NZ)), mis, format!("{:.1}", times[25].as_secs_f64() * 1e6))
        }
    };
    Row { case, op, time: &time, in_anch, out_anch: &out_anch, subpaths: &subpaths, res_area: &res_area, exp_area: exp, mismatch: &mismatch }.print();
}

fn run_binary_ops(
    case: &str,
    a: &(PathData, BezPath),
    b: &(PathData, BezPath),
    expected_areas: [f64; 4],
    use_try: bool,
) {
    let inputs = [a.1.clone(), b.1.clone()];
    let in_anch = a.0.anchor_count() + b.0.anchor_count();
    for (i, &(op_name, op, pred)) in OPS.iter().enumerate() {
        run_row(case, op_name, &inputs, in_anch, Some(expected_areas[i]), Some(&pred), || {
            if use_try {
                try_boolean(&a.0, NZ, &b.0, NZ, op, DEFAULT_PRECISION).map_err(|e| format!("{e}"))
            } else {
                Ok(boolean(&a.0, NZ, &b.0, NZ, op))
            }
        });
    }
}

fn generate_twenty_shapes() -> Vec<(PathData, BezPath)> {
    let mut rng = Rng(0x4d69_6368_6165_6c21);
    let mut shapes = Vec::with_capacity(20);
    for i in 0..20 {
        let (cx, cy) = if i == 0 { (60.0, 100.0) } else { (rng.next_f64(80.0, 120.0), rng.next_f64(80.0, 120.0)) };
        match i % 3 {
            0 => shapes.push(to_path(&Circle::new(Point::new(cx, cy), rng.next_f64(40.0, 60.0)))),
            1 => shapes.push(to_path(&Ellipse::new(Point::new(cx, cy), Vec2::new(rng.next_f64(40.0, 60.0), rng.next_f64(30.0, 50.0)), rng.next_f64(0.0, PI)))),
            _ => {
                let (w, h) = (rng.next_f64(60.0, 100.0), rng.next_f64(60.0, 100.0));
                shapes.push(to_path(&RoundedRect::new(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0, rng.next_f64(10.0, 20.0))));
            }
        }
    }
    shapes
}

fn main() {
    println!(
        "{:<20} | {:<22} | {:>9} | {:>7} | {:>10} | {:>8} | {:>12} | {:>13} | {:>10}",
        "Case", "Operation", "Time (µs)", "In Anch", "Out Anch", "Subpaths", "Result Area", "Expected Area", "Mismatch %"
    );
    println!("{:-<20}-+-{:-<22}-+-{:-<9}-+-{:-<7}-+-{:-<10}-+-{:-<8}-+-{:-<12}-+-{:-<13}-+-{:-<10}", "", "", "", "", "", "", "", "", "");

    // A. Two overlapping circles of different radii
    let c1 = to_path(&Circle::new(Point::new(0.0, 0.0), 100.0));
    let c2 = to_path(&Circle::new(Point::new(90.0, 0.0), 60.0));
    run_binary_ops("A: Two circles", &c1, &c2, two_circles_expected_areas(100.0, 60.0, 90.0), false);

    // B. Twenty overlapping curved shapes
    let shapes20 = generate_twenty_shapes();
    let b_inputs: Vec<BezPath> = shapes20.iter().map(|(_, bp)| bp.clone()).collect();
    let b_in_anch: usize = shapes20.iter().map(|(p, _)| p.anchor_count()).sum();
    let b_refs: Vec<(&PathData, FillRule)> = shapes20.iter().map(|(p, _)| (p, NZ)).collect();

    run_row("B: 20 curved shapes", "unite_all", &b_inputs, b_in_anch, None, Some(&|m| m.iter().any(|&b| b)), || Ok(unite_all(&b_refs)));
    let p_all3 = |m: &[bool]| m.len() >= 3 && m[0] && m[1] && m[2];
    run_row("B: 20 curved shapes", "inside all first 3", &b_inputs, b_in_anch, None, Some(&p_all3), || Ok(boolean_n(&b_refs, p_all3)));
    let p_odd = |m: &[bool]| m.iter().filter(|&&b| b).count() % 2 == 1;
    run_row("B: 20 curved shapes", "inside odd (exclude)", &b_inputs, b_in_anch, None, Some(&p_odd), || Ok(boolean_n(&b_refs, p_odd)));
    let p_sub = |m: &[bool]| !m.is_empty() && m[0] && !m[1..].iter().any(|&b| b);
    run_row("B: 20 curved shapes", "first only (subtract)", &b_inputs, b_in_anch, None, Some(&p_sub), || Ok(boolean_n(&b_refs, p_sub)));

    // C. Shape Builder regions
    let v0 = to_path(&Circle::new(Point::new(100.0, 70.0), 60.0));
    let v1 = to_path(&Circle::new(Point::new(70.0, 120.0), 60.0));
    let v2 = to_path(&Circle::new(Point::new(130.0, 120.0), 60.0));
    let venn_inputs = [v0.1.clone(), v1.1.clone(), v2.1.clone()];
    let venn_in_anch = v0.0.anchor_count() + v1.0.anchor_count() + v2.0.anchor_count();
    let venn_shapes = vec![Shape::new(v0.0.clone(), NZ, 0), Shape::new(v1.0.clone(), NZ, 1), Shape::new(v2.0.clone(), NZ, 2)];
    let venn_union_area = area(&unite_all(&[(&v0.0, NZ), (&v1.0, NZ), (&v2.0, NZ)]), NZ);

    run_row("C: Venn 3 circles", "regions", &venn_inputs, venn_in_anch, Some(venn_union_area), Some(&|m| m[0] || m[1] || m[2]), || {
        let r = regions(&venn_shapes);
        if r.len() != 7 { return Err(format!("expected 7 regions, got {}", r.len())); }
        Ok(PathData::new(r.into_iter().flat_map(|reg| reg.path.subpaths).collect()))
    });
    run_row("C: Venn 3 circles", "shape_builder", &venn_inputs, venn_in_anch, Some(venn_union_area), Some(&|m| m[0] || m[1] || m[2]), || {
        let sb = shape_builder(&venn_shapes, true);
        Ok(PathData::new(sb.regions.into_iter().flat_map(|reg| reg.path.subpaths).collect()))
    });
    run_row("C: Venn 3 circles", "region_at center", &venn_inputs, venn_in_anch, None, Some(&|m| m[0] && m[1] && m[2]), || {
        let r = region_at(&venn_shapes, Point::new(100.0, 105.0)).ok_or("no region at center")?;
        if r.sources != [0, 1, 2] { return Err(format!("sources {:?}", r.sources)); }
        Ok(r.path)
    });
    let all_regs = regions(&venn_shapes);
    if let (Some(rc), Some(ra)) = (all_regs.iter().find(|r| r.sources == [0, 1, 2]), all_regs.iter().find(|r| r.sources == [0, 1])) {
        let expected_merge = area(&rc.path, NZ) + area(&ra.path, NZ);
        run_row("C: Venn 3 circles", "merge_regions (2 adj)", &venn_inputs, venn_in_anch, Some(expected_merge), Some(&|m| m[0] && m[1]), || {
            Ok(merge_regions(&[rc, ra]))
        });
    }

    let shapes20_vec: Vec<Shape> = shapes20.iter().enumerate().map(|(i, (p, _))| Shape::new(p.clone(), NZ, i as u64)).collect();
    run_row("C: 20 curved shapes", "regions", &b_inputs, b_in_anch, None, None, || {
        Ok(PathData::new(regions(&shapes20_vec).into_iter().flat_map(|reg| reg.path.subpaths).collect()))
    });
    run_row("C: 20 curved shapes", "shape_builder", &b_inputs, b_in_anch, None, None, || {
        Ok(PathData::new(shape_builder(&shapes20_vec, true).regions.into_iter().flat_map(|reg| reg.path.subpaths).collect()))
    });

    // D. Degenerate inputs
    let (r1, r2) = (Rect::new(0.0, 0.0, 100.0, 100.0), Rect::new(100.0, 0.0, 200.0, 100.0));
    run_binary_ops("D1: Shared full edge", &to_path(&r1), &to_path(&r2), two_rects_expected_areas(r1, r2), true);

    let (r3, r4) = (Rect::new(0.0, 0.0, 100.0, 100.0), Rect::new(100.0, 30.0, 200.0, 130.0));
    run_binary_ops("D2: Shared part edge", &to_path(&r3), &to_path(&r4), two_rects_expected_areas(r3, r4), true);

    let (c_id1, c_id2) = (to_path(&Circle::new(Point::new(100.0, 100.0), 60.0)), to_path(&Circle::new(Point::new(100.0, 100.0), 60.0)));
    run_binary_ops("D3: Identical circles", &c_id1, &c_id2, two_circles_expected_areas(60.0, 60.0, 0.0), true);

    let (c_te1, c_te2) = (to_path(&Circle::new(Point::new(100.0, 100.0), 60.0)), to_path(&Circle::new(Point::new(200.0, 100.0), 40.0)));
    run_binary_ops("D4: Tangent external", &c_te1, &c_te2, two_circles_expected_areas(60.0, 40.0, 100.0), true);

    let (c_ti1, c_ti2) = (to_path(&Circle::new(Point::new(100.0, 100.0), 80.0)), to_path(&Circle::new(Point::new(140.0, 100.0), 40.0)));
    run_binary_ops("D5: Tangent internal", &c_ti1, &c_ti2, two_circles_expected_areas(80.0, 40.0, 40.0), true);

    let (rr, arc_c) = (to_path(&RoundedRect::new(0.0, 0.0, 100.0, 100.0, 20.0)), to_path(&Circle::new(Point::new(80.0, 20.0), 20.0)));
    let (rr_a, c_a) = (10000.0 - (4.0 - PI) * 400.0, PI * 400.0);
    run_binary_ops("D6: Coincident corner", &rr, &arc_c, [rr_a, c_a, rr_a - c_a, rr_a - c_a], true);
}
