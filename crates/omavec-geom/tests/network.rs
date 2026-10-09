//! Vector networks: faces of small graphs by hand, then properties of random
//! ones.

use omavec_geom::FillRule;
use omavec_geom::kurbo::{BezPath, Circle, PathEl, Point, Rect, Shape, Vec2};
use omavec_geom::network::{HalfEdge, Region, Segment, VectorNetwork, Vertex};
use proptest::prelude::*;

/// A network of straight segments.
fn lines(points: &[(f64, f64)], segments: &[(usize, usize)]) -> VectorNetwork {
    VectorNetwork {
        vertices: points.iter().map(|&point| Vertex { point: point.into() }).collect(),
        segments: segments.iter().map(|&(start, end)| Segment { start, end, tangent_start: Vec2::ZERO, tangent_end: Vec2::ZERO }).collect(),
        regions: Vec::new(),
    }
}

/// Each face's area with its holes taken out, smallest first.
fn areas(network: &VectorNetwork) -> Vec<f64> {
    let mut areas: Vec<f64> = network.faces().iter().map(|face| network.region_path(face).area()).collect();
    areas.sort_by(f64::total_cmp);
    areas
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6)
}

const SQUARE: [(f64, f64); 4] = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
const RING: [(usize, usize); 4] = [(0, 1), (1, 2), (2, 3), (3, 0)];

#[test]
fn a_square_is_one_face_wound_the_way_kurbo_fills() {
    let square = lines(&SQUARE, &RING);
    let faces = square.faces();
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].loops.len(), 1);
    assert_eq!(faces[0].loops[0].len(), 4);
    let path = square.region_path(&faces[0]);
    // Positive, like the path of a kurbo `Rect`: not the walk round the outside.
    assert_eq!(path.area(), 100.0);
    assert_eq!(Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1).area(), 100.0);
    assert_ne!(path.winding(Point::new(5.0, 5.0)), 0);
    assert_eq!(path.winding(Point::new(15.0, 5.0)), 0);
}

#[test]
fn the_same_square_drawn_the_other_way_round_is_the_same_face() {
    let backwards = lines(&SQUARE, &[(1, 0), (2, 1), (3, 2), (0, 3)]);
    assert_eq!(areas(&backwards), [100.0]);
}

#[test]
fn a_chord_splits_a_face_and_a_branch_is_allowed() {
    // A diagonal from corner to corner: both of its ends have three segments.
    let mut segments = RING.to_vec();
    segments.push((0, 2));
    assert_eq!(areas(&lines(&SQUARE, &segments)), [50.0, 50.0]);
}

#[test]
fn two_loops_sharing_one_vertex_are_two_faces() {
    let bow_tie = lines(&[(0.0, 0.0), (-10.0, -5.0), (-10.0, 5.0), (10.0, -5.0), (10.0, 5.0)], &[(0, 1), (1, 2), (2, 0), (0, 3), (3, 4), (4, 0)]);
    assert_eq!(areas(&bow_tie), [50.0, 50.0]);
}

#[test]
fn dead_ends_bound_nothing() {
    // A tree has no faces.
    assert!(lines(&[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (9.0, 0.0)], &[(0, 1), (1, 2), (1, 3)]).faces().is_empty());
    // A spur into a square, and one out of it, leave its outline alone.
    let mut points = SQUARE.to_vec();
    points.extend([(5.0, 5.0), (20.0, 0.0)]);
    let mut segments = RING.to_vec();
    segments.extend([(0, 4), (1, 5)]);
    let spurred = lines(&points, &segments);
    let faces = spurred.faces();
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].loops, [vec![0, 1, 2, 3].into_iter().map(|segment| HalfEdge { segment, reversed: false }).collect::<Vec<_>>()]);
}

#[test]
fn a_network_inside_a_face_is_a_hole_in_it() {
    let inner = [(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0)];
    // A third square inside the second: a hole in the hole's own face.
    let innermost = [(4.5, 4.5), (5.5, 4.5), (5.5, 5.5), (4.5, 5.5)];
    let points: Vec<_> = SQUARE.into_iter().chain(inner).chain(innermost).collect();
    let segments: Vec<_> = (0..3).flat_map(|square| RING.map(|(a, b)| (a + square * 4, b + square * 4))).collect();
    let nested = lines(&points, &segments);
    assert_eq!(areas(&nested), [1.0, 3.0, 96.0]);
    let faces = nested.faces();
    let outer = faces.iter().find(|face| nested.region_path(face).area() == 96.0).unwrap();
    assert_eq!(outer.loops.len(), 2);
    // The hole is wound against the outline, so either fill rule leaves it empty.
    let path = nested.region_path(outer);
    assert_eq!(path.winding(Point::new(5.0, 4.2)), 0);
    assert_ne!(path.winding(Point::new(2.0, 2.0)), 0);
    // The outline is a move, four lines and a close; the hole is the rest.
    let hole = BezPath::from_vec(path.elements()[6..].to_vec());
    assert_eq!(hole.area(), -4.0);
}

#[test]
fn curved_segments_bound_faces_too() {
    // A circle as four arcs, cut by a straight chord between opposite vertices.
    let handle = 10.0 * 0.5522847498;
    let mut path = BezPath::new();
    path.move_to((10.0, 0.0));
    path.curve_to((10.0, handle), (handle, 10.0), (0.0, 10.0));
    path.curve_to((-handle, 10.0), (-10.0, handle), (-10.0, 0.0));
    path.curve_to((-10.0, -handle), (-handle, -10.0), (0.0, -10.0));
    path.curve_to((handle, -10.0), (10.0, -handle), (10.0, 0.0));
    path.close_path();
    let mut network = VectorNetwork::from_bezpath(&path, FillRule::NonZero, 1e-9);
    assert_eq!((network.vertices.len(), network.segments.len()), (4, 4));
    assert_eq!(network.regions.len(), 1);
    let whole = path.area().abs();
    assert!((whole - Circle::new((0.0, 0.0), 10.0).area()).abs() < 0.1);
    assert!(close(&areas(&network), &[whole]));
    network.segments.push(Segment { start: 0, end: 2, tangent_start: Vec2::ZERO, tangent_end: Vec2::ZERO });
    assert!(close(&areas(&network), &[whole / 2.0, whole / 2.0]));
}

#[test]
fn a_stroke_follows_runs_through_plain_vertices_and_stops_at_branches() {
    let subpaths = |path: &BezPath| path.elements().iter().filter(|el| matches!(el, PathEl::MoveTo(_))).count();
    let closed = |path: &BezPath| path.elements().iter().filter(|el| matches!(el, PathEl::ClosePath)).count();
    // Three segments in a row: one open run, whichever way each was drawn.
    let row = lines(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)], &[(1, 0), (1, 2), (3, 2)]).stroke_path();
    assert_eq!((subpaths(&row), closed(&row), row.elements().len()), (1, 0, 4));
    // A Y: three runs meeting at the branch.
    let y = lines(&[(0.0, 0.0), (0.0, 5.0), (-3.0, -3.0), (3.0, -3.0)], &[(0, 1), (0, 2), (0, 3)]).stroke_path();
    assert_eq!((subpaths(&y), closed(&y)), (3, 0));
    // A ring: one closed run.
    let ring = lines(&SQUARE, &RING).stroke_path();
    assert_eq!((subpaths(&ring), closed(&ring)), (1, 1));
    assert_eq!(ring.area().abs(), 100.0);
}

#[test]
fn two_subpaths_sharing_an_edge_share_its_vertices() {
    // Two squares side by side, as an SVG would have them.
    let mut path = Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1);
    path.extend(Rect::new(10.0, 0.0, 20.0, 10.0).to_path(0.1));
    let network = VectorNetwork::from_bezpath(&path, FillRule::NonZero, 1e-9);
    // Six corners, and seven segments: the edge between them is there once.
    assert_eq!((network.vertices.len(), network.segments.len()), (6, 7));
    assert_eq!(network.regions.len(), 1);
    assert_eq!(network.regions[0].loops.len(), 2);
    assert_eq!(network.region_path(&network.regions[0]).area(), 200.0);
    assert_eq!(areas(&network), [100.0, 100.0]);
}

#[test]
fn a_network_survives_a_trip_through_vectorcrafts_paths() {
    let mut segments = RING.to_vec();
    segments.push((0, 2));
    let mut network = lines(&SQUARE, &segments);
    network.regions = network.faces();
    let back = VectorNetwork::from_path_data(&network.to_path_data(), FillRule::NonZero, 1e-9);
    // The chord is in both faces' loops and comes back as one segment.
    assert_eq!((back.vertices.len(), back.segments.len()), (4, 5));
    assert_eq!(areas(&back), [50.0, 50.0]);
}

/// Grid points `SIDE` to a side, and unit cells between them.
const SIDE: usize = 5;
const CELLS: usize = SIDE - 1;

fn at(column: usize, row: usize) -> usize {
    row * SIDE + column
}

/// The straight network with the chosen horizontal and vertical unit edges
/// of the grid. `across[row][column]` is the edge along the top of cell
/// (column, row); `down[row][column]` the edge down its left side.
fn grid(across: &[[bool; CELLS]; SIDE], down: &[[bool; SIDE]; CELLS]) -> VectorNetwork {
    let points: Vec<_> = (0..SIDE * SIDE).map(|i| ((i % SIDE) as f64, (i / SIDE) as f64)).collect();
    let mut segments = Vec::new();
    for (row, edges) in across.iter().enumerate() {
        segments.extend(edges.iter().enumerate().filter(|(_, on)| **on).map(|(column, _)| (at(column, row), at(column + 1, row))));
    }
    for (row, edges) in down.iter().enumerate() {
        segments.extend(edges.iter().enumerate().filter(|(_, on)| **on).map(|(column, _)| (at(column, row), at(column, row + 1))));
    }
    lines(&points, &segments)
}

/// What a flood fill makes of the same grid, without any geometry: for each
/// cell, the number of the enclosed area it belongs to, or `None` if the
/// fill reaches it from outside. Areas are numbered from 0.
fn flood(across: &[[bool; CELLS]; SIDE], down: &[[bool; SIDE]; CELLS]) -> (Vec<Option<usize>>, usize) {
    let cell = |column: usize, row: usize| row * CELLS + column;
    // The cells next to (column, row) with no edge in between; `None` is outside.
    let open = |column: usize, row: usize| {
        let mut next = Vec::new();
        if !across[row][column] {
            next.push((row > 0).then(|| (column, row - 1)));
        }
        if !across[row + 1][column] {
            next.push((row + 1 < CELLS).then(|| (column, row + 1)));
        }
        if !down[row][column] {
            next.push((column > 0).then(|| (column - 1, row)));
        }
        if !down[row][column + 1] {
            next.push((column + 1 < CELLS).then(|| (column + 1, row)));
        }
        next
    };
    // Group cells that reach each other, and note the groups that reach outside.
    let mut group = vec![usize::MAX; CELLS * CELLS];
    let mut leaks = Vec::new();
    for start in 0..CELLS * CELLS {
        if group[start] != usize::MAX {
            continue;
        }
        let id = leaks.len();
        leaks.push(false);
        group[start] = id;
        let mut stack = vec![(start % CELLS, start / CELLS)];
        while let Some((column, row)) = stack.pop() {
            for next in open(column, row) {
                match next {
                    None => leaks[id] = true,
                    Some((c, r)) if group[cell(c, r)] == usize::MAX => {
                        group[cell(c, r)] = id;
                        stack.push((c, r));
                    }
                    Some(_) => {}
                }
            }
        }
    }
    let mut number = vec![None; leaks.len()];
    let mut enclosed = 0;
    for (id, leaks) in leaks.iter().enumerate() {
        if !leaks {
            number[id] = Some(enclosed);
            enclosed += 1;
        }
    }
    (group.into_iter().map(|id| number[id]).collect(), enclosed)
}

/// A grid whose points are nudged and whose edges bulge, with one diagonal
/// in some cells: still planar, no longer straight.
fn bent_grid(edges: &[bool], diagonals: &[Option<bool>], nudges: &[(f64, f64)], bulges: &[f64]) -> VectorNetwork {
    let points: Vec<_> = (0..SIDE * SIDE).map(|i| ((i % SIDE) as f64 + nudges[i].0, (i / SIDE) as f64 + nudges[i].1)).collect();
    let mut pairs = Vec::new();
    for row in 0..SIDE {
        for column in 0..SIDE {
            if column < CELLS {
                pairs.push((at(column, row), at(column + 1, row)));
            }
            if row < CELLS {
                pairs.push((at(column, row), at(column, row + 1)));
            }
        }
    }
    let mut chosen: Vec<_> = pairs.into_iter().zip(edges).filter(|(_, on)| **on).map(|(pair, _)| pair).collect();
    for (i, diagonal) in diagonals.iter().enumerate() {
        let (column, row) = (i % CELLS, i / CELLS);
        match diagonal {
            Some(true) => chosen.push((at(column, row), at(column + 1, row + 1))),
            Some(false) => chosen.push((at(column + 1, row), at(column, row + 1))),
            None => {}
        }
    }
    let mut network = lines(&points, &chosen);
    for (segment, bulge) in network.segments.iter_mut().zip(bulges.iter().cycle()) {
        let along = network.vertices[segment.end].point - network.vertices[segment.start].point;
        let sideways = Vec2::new(-along.y, along.x) * *bulge;
        segment.tangent_start = along * 0.3 + sideways;
        segment.tangent_end = along * -0.3 + sideways;
    }
    network
}

/// How many connected pieces the graph is in, lone vertices included.
fn pieces(network: &VectorNetwork) -> usize {
    let mut piece: Vec<usize> = (0..network.vertices.len()).collect();
    loop {
        let mut changed = false;
        for segment in &network.segments {
            let low = piece[segment.start].min(piece[segment.end]);
            changed |= piece[segment.start] != low || piece[segment.end] != low;
            (piece[segment.start], piece[segment.end]) = (low, low);
        }
        if !changed {
            break;
        }
    }
    piece.sort();
    piece.dedup();
    piece.len()
}

fn bent() -> impl Strategy<Value = VectorNetwork> {
    (
        prop::collection::vec(any::<bool>(), 2 * SIDE * CELLS),
        prop::collection::vec(prop::option::of(any::<bool>()), CELLS * CELLS),
        prop::collection::vec((-0.15..0.15f64, -0.15..0.15f64), SIDE * SIDE),
        prop::collection::vec(-0.04..0.04f64, 7),
    )
        .prop_map(|(edges, diagonals, nudges, bulges)| bent_grid(&edges, &diagonals, &nudges, &bulges))
}

proptest! {
    /// On a straight grid a flood fill knows which cells are enclosed and
    /// which belong together. The faces must be exactly those.
    #[test]
    fn faces_are_what_a_flood_fill_encloses(across in any::<[[bool; CELLS]; SIDE]>(), down in any::<[[bool; SIDE]; CELLS]>()) {
        let network = grid(&across, &down);
        let (cells, enclosed) = flood(&across, &down);
        let faces = network.faces();
        prop_assert_eq!(faces.len(), enclosed);
        let paths: Vec<BezPath> = faces.iter().map(|face| network.region_path(face)).collect();
        // Each enclosed area is one face: the same cells, and so the same size.
        let mut face_of_area = vec![None; enclosed];
        for (i, area) in cells.iter().enumerate() {
            let centre = Point::new((i % CELLS) as f64 + 0.5, (i / CELLS) as f64 + 0.5);
            let holding: Vec<usize> = (0..paths.len()).filter(|&face| paths[face].winding(centre) != 0).collect();
            match area {
                None => prop_assert!(holding.is_empty(), "cell {} is open but in face {:?}", i, holding),
                Some(area) => {
                    prop_assert_eq!(holding.len(), 1, "cell {} is in faces {:?}", i, &holding);
                    prop_assert_eq!(*face_of_area[*area].get_or_insert(holding[0]), holding[0]);
                }
            }
        }
        for (area, face) in face_of_area.iter().enumerate() {
            let size = cells.iter().filter(|cell| **cell == Some(area)).count();
            prop_assert_eq!(paths[face.unwrap()].area(), size as f64);
        }
    }

    /// Euler's formula for a planar graph: faces = segments − vertices + pieces.
    #[test]
    fn curved_faces_obey_eulers_formula_and_never_overlap(network in bent(), samples in prop::collection::vec((0.0..4.0f64, 0.0..4.0f64), 40)) {
        let faces = network.faces();
        prop_assert_eq!(faces.len() + network.vertices.len(), network.segments.len() + pieces(&network));
        let paths: Vec<BezPath> = faces.iter().map(|face| network.region_path(face)).collect();
        for path in &paths {
            prop_assert!(path.area() > 0.0);
        }
        for sample in samples {
            let holding = paths.iter().filter(|path| path.winding(sample.into()) != 0).count();
            prop_assert!(holding <= 1, "{:?} is in {} faces", sample, holding);
        }
    }

    /// Faces as regions, out to a path (and to VectorCraft's), and back:
    /// the same graph and the same faces.
    #[test]
    fn faces_survive_a_round_trip_through_a_path(network in bent()) {
        let mut network = network;
        network.regions = network.faces();
        let expected = areas(&network);
        let back = VectorNetwork::from_bezpath(&network.to_bezpath(), FillRule::NonZero, 1e-9);
        prop_assert!(close(&areas(&back), &expected), "{:?} != {:?}", areas(&back), expected);
        let through_vectorcraft = VectorNetwork::from_path_data(&network.to_path_data(), FillRule::NonZero, 1e-9);
        prop_assert!(close(&areas(&through_vectorcraft), &expected));
        // Every segment is in a loop or a leftover run, and none comes back twice.
        prop_assert_eq!(back.segments.len(), network.segments.len());
        prop_assert_eq!(through_vectorcraft.segments.len(), network.segments.len());
        let used = network.vertices.iter().enumerate().filter(|(i, _)| network.segments.iter().any(|s| s.start == *i || s.end == *i)).count();
        prop_assert_eq!(back.vertices.len(), used);
    }

    /// Whatever it is handed (segments and regions naming things that don't
    /// exist, loops on one vertex, infinities, NaN), nothing panics.
    #[test]
    fn nonsense_never_panics(
        points in prop::collection::vec((any::<f64>(), any::<f64>()), 0..8),
        tame in prop::collection::vec((-5.0..5.0f64, -5.0..5.0f64), 0..8),
        segments in prop::collection::vec((0..10usize, 0..10usize, -3.0..3.0f64, -3.0..3.0f64), 0..16),
        loops in prop::collection::vec(prop::collection::vec((0..20usize, any::<bool>()), 0..5), 0..3),
    ) {
        let network = VectorNetwork {
            vertices: points.into_iter().chain(tame).map(|point| Vertex { point: point.into() }).collect(),
            segments: segments.into_iter().map(|(start, end, x, y)| Segment { start, end, tangent_start: Vec2::new(x, y), tangent_end: Vec2::new(y, -x) }).collect(),
            regions: vec![Region { loops: loops.into_iter().map(|edges| edges.into_iter().map(|(segment, reversed)| HalfEdge { segment, reversed }).collect()).collect(), rule: FillRule::EvenOdd }],
        };
        let faces = network.faces();
        for face in &faces {
            network.region_path(face);
        }
        network.stroke_path();
        let path = network.to_bezpath();
        network.to_path_data();
        VectorNetwork::from_bezpath(&path, FillRule::NonZero, 1e-9).faces();
    }
}
