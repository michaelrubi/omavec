//! Phase 0 spike: how long a vector network takes to find its faces.
//!
//!     cargo run --release -p omavec-geom --example network

use std::time::Instant;

use omavec_geom::kurbo::Vec2;
use omavec_geom::network::{Segment, VectorNetwork, Vertex};

fn line(start: usize, end: usize) -> Segment {
    Segment { start, end, tangent_start: Vec2::ZERO, tangent_end: Vec2::ZERO }
}

/// A full grid `side` points to a side: every cell is a face.
fn mesh(side: usize) -> VectorNetwork {
    let mut network = VectorNetwork::default();
    for i in 0..side * side {
        network.vertices.push(Vertex { point: ((i % side) as f64, (i / side) as f64).into() });
        if i % side + 1 < side {
            network.segments.push(line(i, i + 1));
        }
        if i / side + 1 < side {
            network.segments.push(line(i, i + side));
        }
    }
    network
}

/// `side` × `side` separate small squares inside one big one: every small
/// square is a hole in the big square's face.
fn islands(side: usize) -> VectorNetwork {
    let mut network = VectorNetwork::default();
    let mut square = |x: f64, y: f64, size: f64| {
        let first = network.vertices.len();
        for (dx, dy) in [(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)] {
            network.vertices.push(Vertex { point: (x + dx, y + dy).into() });
        }
        network.segments.extend((0..4).map(|i| line(first + i, first + (i + 1) % 4)));
    };
    square(-1.0, -1.0, side as f64 * 2.0 + 1.0);
    for i in 0..side * side {
        square((i % side) as f64 * 2.0, (i / side) as f64 * 2.0, 1.0);
    }
    network
}

fn main() {
    println!("network                         | vertices | segments |  faces | faces() ms | to_bezpath ms | from_bezpath ms");
    let cases = [("mesh 10 x 10", mesh(10)), ("mesh 50 x 50", mesh(50)), ("mesh 200 x 200", mesh(200)), ("100 islands in a square", islands(10)), ("2,500 islands in a square", islands(50))];
    for (name, mut network) in cases {
        let start = Instant::now();
        network.regions = network.faces();
        let faces = start.elapsed();
        let start = Instant::now();
        let path = network.to_bezpath();
        let out = start.elapsed();
        let start = Instant::now();
        let back = VectorNetwork::from_bezpath(&path, omavec_geom::FillRule::NonZero, 1e-9);
        let back_in = start.elapsed();
        assert_eq!(back.segments.len(), network.segments.len());
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        println!("{name:<31} | {:>8} | {:>8} | {:>6} | {:>10.2} | {:>13.2} | {:>15.2}", network.vertices.len(), network.segments.len(), network.regions.len(), ms(faces), ms(out), ms(back_in));
    }
}
