//! Vector networks: Figma's planar-graph paths. A vertex can have any number
//! of segments, and the fillable areas are found from the graph instead of
//! being drawn as closed subpaths.
//!
//! Phase 0's spike: the data structure, its faces, and conversion to and from
//! `BezPath` (and so `vectorcraft_geom::PathData`). Segments are assumed to
//! meet only at vertices; crossings that aren't vertices are Phase 2's job.

use std::collections::HashMap;

use kurbo::{BezPath, CubicBez, ParamCurve, PathEl, Point, Rect, Shape, Vec2};
use vectorcraft_geom::{FillRule, PathData};

/// Faces smaller than this (in square units) are slivers between coincident
/// segments, not regions.
const MIN_AREA: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub point: Point,
}

/// A cubic from one vertex to another. The tangents are relative to their
/// vertices, as in Figma; both zero makes a straight line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub tangent_start: Vec2,
    pub tangent_end: Vec2,
}

/// A segment walked forwards (start to end) or backwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HalfEdge {
    pub segment: usize,
    pub reversed: bool,
}

impl HalfEdge {
    fn index(self) -> usize {
        self.segment * 2 + usize::from(self.reversed)
    }

    fn from_index(index: usize) -> Self {
        Self { segment: index / 2, reversed: index % 2 == 1 }
    }

    fn twin(self) -> Self {
        Self { reversed: !self.reversed, ..self }
    }
}

/// A fillable area: closed loops of segments and how they combine. For a
/// face found by [`VectorNetwork::faces`] the first loop is its outline and
/// the rest are holes, wound the other way.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub loops: Vec<Vec<HalfEdge>>,
    pub rule: FillRule,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorNetwork {
    pub vertices: Vec<Vertex>,
    pub segments: Vec<Segment>,
    pub regions: Vec<Region>,
}

impl VectorNetwork {
    /// The network of a path: subpaths become chains of segments, points
    /// closer than `tolerance` on each axis become one vertex, an edge two
    /// subpaths share becomes one segment, and the closed subpaths become the
    /// loops of one region filled with `rule`.
    pub fn from_bezpath(path: &BezPath, rule: FillRule, tolerance: f64) -> Self {
        let mut network = Self::default();
        let mut between: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        let mut segment = |network: &mut Self, start: usize, end: usize, tangent_start: Vec2, tangent_end: Vec2| {
            let same = |a: Vec2, b: Vec2| (a - b).hypot() <= tolerance;
            let known = between.entry((start.min(end), start.max(end))).or_default();
            for &index in known.iter() {
                let other = network.segments[index];
                if other.start == start && same(other.tangent_start, tangent_start) && same(other.tangent_end, tangent_end) {
                    return HalfEdge { segment: index, reversed: false };
                }
                if other.start == end && same(other.tangent_start, tangent_end) && same(other.tangent_end, tangent_start) {
                    return HalfEdge { segment: index, reversed: true };
                }
            }
            network.segments.push(Segment { start, end, tangent_start, tangent_end });
            known.push(network.segments.len() - 1);
            HalfEdge { segment: network.segments.len() - 1, reversed: false }
        };
        let mut found: HashMap<(i64, i64), usize> = HashMap::new();
        let mut vertex = |network: &mut Self, point: Point| {
            let key = ((point.x / tolerance).round() as i64, (point.y / tolerance).round() as i64);
            *found.entry(key).or_insert_with(|| {
                network.vertices.push(Vertex { point });
                network.vertices.len() - 1
            })
        };
        // The subpath being read: its first vertex, its last, and its segments.
        let mut subpath: Option<(usize, usize, Vec<HalfEdge>)> = None;
        let mut loops = Vec::new();
        for element in path.elements() {
            let (control_1, control_2, to) = match *element {
                PathEl::MoveTo(point) => {
                    let start = vertex(&mut network, point);
                    subpath = Some((start, start, Vec::new()));
                    continue;
                }
                PathEl::ClosePath => {
                    let Some((start, last, mut edges)) = subpath.take() else { continue };
                    if last != start {
                        edges.push(segment(&mut network, last, start, Vec2::ZERO, Vec2::ZERO));
                    }
                    if !edges.is_empty() {
                        loops.push(edges);
                    }
                    subpath = Some((start, start, Vec::new()));
                    continue;
                }
                PathEl::LineTo(to) => (None, None, to),
                PathEl::QuadTo(control, to) => (Some(control), Some(control), to),
                PathEl::CurveTo(control_1, control_2, to) => (Some(control_1), Some(control_2), to),
            };
            let Some((_, last, edges)) = &mut subpath else { continue };
            let from = network.vertices[*last].point;
            let end = vertex(&mut network, to);
            let to = network.vertices[end].point;
            let (tangent_start, tangent_end) = match (control_1, control_2, element) {
                // A quadratic raised to a cubic: its controls are two thirds
                // of the way to the quadratic's.
                (Some(control), _, PathEl::QuadTo(..)) => ((control - from) * (2.0 / 3.0), (control - to) * (2.0 / 3.0)),
                (Some(control_1), Some(control_2), _) => (control_1 - from, control_2 - to),
                _ => (Vec2::ZERO, Vec2::ZERO),
            };
            // A segment from a vertex to itself with no handles draws nothing.
            if end != *last || tangent_start != Vec2::ZERO || tangent_end != Vec2::ZERO {
                edges.push(segment(&mut network, *last, end, tangent_start, tangent_end));
            }
            *last = end;
        }
        if !loops.is_empty() {
            network.regions.push(Region { loops, rule });
        }
        network
    }

    /// As [`Self::from_bezpath`], from VectorCraft's path type.
    pub fn from_path_data(path: &PathData, rule: FillRule, tolerance: f64) -> Self {
        Self::from_bezpath(&path.to_bezpath(), rule, tolerance)
    }

    /// The curve a half-edge follows, in the direction it's walked. `None`
    /// if it names a segment or vertex that doesn't exist.
    pub fn curve(&self, edge: HalfEdge) -> Option<CubicBez> {
        let segment = self.segments.get(edge.segment)?;
        let (start, end) = (self.vertices.get(segment.start)?.point, self.vertices.get(segment.end)?.point);
        let curve = CubicBez::new(start, start + segment.tangent_start, end + segment.tangent_end, end);
        Some(if edge.reversed { CubicBez::new(curve.p3, curve.p2, curve.p1, curve.p0) } else { curve })
    }

    fn origin(&self, edge: HalfEdge) -> usize {
        let segment = &self.segments[edge.segment];
        if edge.reversed { segment.end } else { segment.start }
    }

    /// The half-edges of the segments that exist and have some length.
    fn drawable(&self) -> impl Iterator<Item = HalfEdge> + '_ {
        (0..self.segments.len() * 2).map(HalfEdge::from_index).filter(|&edge| {
            self.curve(edge).is_some_and(|c| c.p0 != c.p3 || c.p1 != c.p0 || c.p2 != c.p0)
        })
    }

    /// `edges` end to end as one subpath of `path`, closed if asked.
    fn append(&self, path: &mut BezPath, edges: &[HalfEdge], close: bool) {
        let curves: Vec<CubicBez> = edges.iter().filter_map(|&edge| self.curve(edge)).collect();
        let Some(first) = curves.first() else { return };
        path.move_to(first.p0);
        for curve in &curves {
            if curve.p1 == curve.p0 && curve.p2 == curve.p3 {
                path.line_to(curve.p3);
            } else {
                path.curve_to(curve.p1, curve.p2, curve.p3);
            }
        }
        if close {
            path.close_path();
        }
    }

    /// A region's outline, one closed subpath per loop.
    pub fn region_path(&self, region: &Region) -> BezPath {
        let mut path = BezPath::new();
        for edges in &region.loops {
            self.append(&mut path, edges, true);
        }
        path
    }

    /// The fillable areas of the graph: every smallest closed cell the
    /// segments enclose, as the paint bucket sees them. A network drawn
    /// inside another's face becomes a hole in it.
    pub fn faces(&self) -> Vec<Region> {
        // Around each vertex, the half-edges leaving it in order of angle.
        let mut leaving: Vec<Vec<HalfEdge>> = vec![Vec::new(); self.vertices.len()];
        for edge in self.drawable() {
            leaving[self.origin(edge)].push(edge);
        }
        let mut place = vec![usize::MAX; self.segments.len() * 2];
        for edges in &mut leaving {
            edges.sort_by(|&a, &b| {
                let (a_angles, b_angles) = (self.angles(a), self.angles(b));
                a_angles.0.total_cmp(&b_angles.0).then(a_angles.1.total_cmp(&b_angles.1)).then(a.index().cmp(&b.index()))
            });
            for (i, edge) in edges.iter().enumerate() {
                place[edge.index()] = i;
            }
        }

        // Walking a half-edge and then always taking the next one clockwise
        // from the way back traces the face on its left. Every half-edge is
        // on exactly one such walk.
        let mut walked = vec![false; self.segments.len() * 2];
        let mut components = Components::new(self.vertices.len());
        let mut outlines: Vec<(Vec<HalfEdge>, f64)> = Vec::new();
        for start in self.drawable() {
            components.join(self.origin(start), self.origin(start.twin()));
            if walked[start.index()] {
                continue;
            }
            let (mut edge, mut walk) = (start, Vec::new());
            while !walked[edge.index()] {
                walked[edge.index()] = true;
                walk.push(edge);
                let around = &leaving[self.origin(edge.twin())];
                let back = place[edge.twin().index()];
                edge = around[(back + around.len() - 1) % around.len()];
            }
            let walk = without_spurs(walk);
            let mut path = BezPath::new();
            self.append(&mut path, &walk, true);
            outlines.push((walk, path.area()));
        }

        // A face walked this way has positive area; the one walk around the
        // outside of each connected piece has negative area.
        let mut faces: Vec<(Region, f64, usize)> = Vec::new();
        let mut outsides = Vec::new();
        for (walk, area) in outlines {
            let Some(first) = walk.first() else { continue };
            let component = components.find(self.origin(*first));
            if area > MIN_AREA {
                faces.push((Region { loops: vec![walk], rule: FillRule::NonZero }, area, component));
            } else if area < -MIN_AREA {
                outsides.push((walk, component));
            }
        }
        // A piece inside another piece's face is a hole in the smallest face
        // that holds it.
        let outlines: Vec<(BezPath, Rect)> = faces
            .iter()
            .map(|(face, ..)| {
                let path = self.loop_path(&face.loops[0]);
                let bounds = path.bounding_box();
                (path, bounds)
            })
            .collect();
        for (outside, component) in outsides {
            let inside = self.vertices[self.origin(outside[0])].point;
            let holder = (0..faces.len())
                .filter(|&i| faces[i].2 != component && outlines[i].1.contains(inside) && outlines[i].0.winding(inside) != 0)
                .min_by(|&a, &b| faces[a].1.total_cmp(&faces[b].1));
            if let Some(holder) = holder {
                faces[holder].0.loops.push(outside);
            }
        }
        faces.into_iter().map(|(face, ..)| face).collect()
    }

    fn loop_path(&self, edges: &[HalfEdge]) -> BezPath {
        let mut path = BezPath::new();
        self.append(&mut path, edges, true);
        path
    }

    /// The direction a half-edge leaves its vertex in, and where it has got
    /// to a little later, to tell apart curves that leave together.
    fn angles(&self, edge: HalfEdge) -> (f64, f64) {
        let Some(curve) = self.curve(edge) else { return (0.0, 0.0) };
        let leaves = [curve.p1, curve.p2, curve.p3].into_iter().map(|p| p - curve.p0).find(|v| *v != Vec2::ZERO).unwrap_or(Vec2::ZERO);
        let later = curve.eval(0.25) - curve.p0;
        (leaves.y.atan2(leaves.x), later.y.atan2(later.x))
    }

    /// Every segment once, joined into the longest runs: through vertices
    /// with two segments, stopping at ends and branches. A run that comes
    /// back to where it began is closed. This is what a stroke follows.
    pub fn stroke_path(&self) -> BezPath {
        self.runs(|_| true)
    }

    /// The whole network as a path: each region's loops as closed subpaths,
    /// then the segments no region uses as open runs. [`Self::from_bezpath`]
    /// reads it back.
    pub fn to_bezpath(&self) -> BezPath {
        let mut in_region = vec![false; self.segments.len()];
        let mut path = BezPath::new();
        for region in &self.regions {
            for edge in region.loops.iter().flatten() {
                if let Some(used) = in_region.get_mut(edge.segment) {
                    *used = true;
                }
            }
            path.extend(self.region_path(region));
        }
        path.extend(self.runs(|segment| !in_region[segment]));
        path
    }

    /// As [`Self::to_bezpath`], to VectorCraft's path type, for its path
    /// operations.
    pub fn to_path_data(&self) -> PathData {
        PathData::from_bezpath(&self.to_bezpath())
    }

    fn runs(&self, include: impl Fn(usize) -> bool) -> BezPath {
        let mut at: Vec<Vec<HalfEdge>> = vec![Vec::new(); self.vertices.len()];
        for edge in self.drawable().filter(|edge| include(edge.segment)) {
            at[self.origin(edge)].push(edge);
        }
        let mut used = vec![false; self.segments.len()];
        let mut path = BezPath::new();
        let run_from = |first: HalfEdge, used: &mut Vec<bool>, path: &mut BezPath| {
            let (mut edge, mut run) = (first, Vec::new());
            loop {
                used[edge.segment] = true;
                run.push(edge);
                let head = self.origin(edge.twin());
                // Carry on only through a vertex with exactly one other way out.
                let onward: Vec<HalfEdge> = at[head].iter().copied().filter(|next| *next != edge.twin()).collect();
                match onward[..] {
                    [next] if !used[next.segment] => edge = next,
                    _ => break,
                }
            }
            let closed = self.origin(first) == self.origin(run[run.len() - 1].twin()) && at[self.origin(first)].len() == 2;
            self.append(path, &run, closed);
        };
        // Runs that start at an end or a branch first; what's left is rings.
        for ends_first in [true, false] {
            for edges in at.iter().filter(|edges| !ends_first || edges.len() != 2) {
                for &edge in edges {
                    if !used[edge.segment] {
                        run_from(edge, &mut used, &mut path);
                    }
                }
            }
        }
        path
    }
}

/// `walk` without the dead ends it went down and came back from: a segment
/// walked one way and straight back the other bounds nothing.
fn without_spurs(walk: Vec<HalfEdge>) -> Vec<HalfEdge> {
    let mut kept: Vec<HalfEdge> = Vec::with_capacity(walk.len());
    for edge in walk {
        if kept.last() == Some(&edge.twin()) {
            kept.pop();
        } else {
            kept.push(edge);
        }
    }
    // The walk is a ring, so a dead end can straddle its two ends.
    let mut start = 0;
    while kept.len() - start >= 2 && kept[start] == kept[kept.len() - 1].twin() {
        kept.pop();
        start += 1;
    }
    kept.split_off(start)
}

/// Which vertices are joined by segments (union-find).
struct Components(Vec<usize>);

impl Components {
    fn new(count: usize) -> Self {
        Self((0..count).collect())
    }

    fn find(&mut self, mut vertex: usize) -> usize {
        while self.0[vertex] != vertex {
            self.0[vertex] = self.0[self.0[vertex]];
            vertex = self.0[vertex];
        }
        vertex
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        self.0[a] = b;
    }
}
