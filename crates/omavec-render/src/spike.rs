//! Phase 0's test scene: random cubic blobs, for `examples/canvas_bench.rs`
//! and for trying the canvas by hand (`OMAVEC_BLOBS=10000 omavec`). It goes
//! when there are documents to draw.

use kurbo::{BezPath, Point, Vec2};
use peniko::Color;

use crate::{DisplayList, Item};

/// The side of the square the blobs are scattered over, in document units.
pub const DOCUMENT: f64 = 8000.0;

/// xorshift64*: the same blobs on every run, with no `rand`.
struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }
}

/// A closed loop of four to six smooth cubic segments around `centre`.
fn blob(rng: &mut Rng, centre: Point) -> Item {
    let corners = 4 + (rng.unit() * 3.0) as usize;
    let radius = rng.range(20.0, 120.0);
    let step = std::f64::consts::TAU / corners as f64;
    let points: Vec<(Point, Vec2)> = (0..corners)
        .map(|i| {
            let r = radius * rng.range(0.6, 1.0);
            let out = Vec2::from_angle(step * i as f64);
            // The tangent that would make a circle, so the loop stays smooth.
            let tangent = Vec2::new(-out.y, out.x) * (r * 4.0 / 3.0 * (step / 4.0).tan());
            (centre + out * r, tangent)
        })
        .collect();
    let mut path = BezPath::new();
    path.move_to(points[0].0);
    for i in 0..corners {
        let (from, from_tangent) = points[i];
        let (to, to_tangent) = points[(i + 1) % corners];
        path.curve_to(from + from_tangent, to - to_tangent, to);
    }
    path.close_path();
    let channel = |rng: &mut Rng| rng.range(40.0, 255.0) as u8;
    Item::new(path, Color::from_rgba8(channel(rng), channel(rng), channel(rng), 200))
}

pub fn blobs(count: usize) -> DisplayList {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let items = (0..count)
        .map(|_| {
            let centre = Point::new(rng.range(0.0, DOCUMENT), rng.range(0.0, DOCUMENT));
            blob(&mut rng, centre)
        })
        .collect();
    DisplayList { items }
}
