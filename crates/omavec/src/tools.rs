//! The tools: pointer events in page coordinates in, document edits out. A
//! tool never sees egui, so each is tested by calling it, and every drag is
//! one undo step.

use omavec_engine::{Cap, Document, Error, History, Node, NodeId, NodeKind};
use omavec_geom::kurbo::{Affine, Line, ParamCurveNearest, Point, Rect, Size, Vec2};
use omavec_geom::snap::{Axis, snap_edge, snap_move};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Move,
    /// Drags pan the canvas; the canvas does it and the tools see nothing.
    Hand,
    Frame,
    Rectangle,
    Ellipse,
    Polygon,
    Star,
    Line,
    Arrow,
    /// A click takes the colour under it for the selection's fill. The app
    /// does it, as it has the pixels; the tools see nothing.
    Eyedropper,
}

impl Tool {
    /// What the tool draws, if it draws, as Figma starts each off.
    fn draws(self) -> Option<NodeKind> {
        match self {
            Tool::Move | Tool::Hand | Tool::Eyedropper => None,
            Tool::Frame => Some(NodeKind::Frame { clip: true }),
            Tool::Rectangle => Some(NodeKind::Rectangle),
            Tool::Ellipse => Some(NodeKind::Ellipse),
            Tool::Polygon => Some(NodeKind::Polygon { sides: 3 }),
            Tool::Star => Some(NodeKind::Star { points: 5, ratio: 0.382 }),
            Tool::Line | Tool::Arrow => Some(NodeKind::Line),
        }
    }

    /// A new node of the tool's kind, not yet in the tree or of any size.
    fn node(self, document: &mut Document) -> Option<Node> {
        let mut node = document.create(self.draws()?, Size::ZERO);
        if self == Tool::Arrow {
            node.stroke.end_cap = Cap::Arrow;
        }
        Some(node)
    }
}

/// The modifier keys the tools read.
#[derive(Clone, Copy, Debug, Default)]
pub struct Keys {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// What a click without a drag draws, as in Figma.
const CLICK_SIZE: f64 = 100.0;

enum Drag {
    /// Drawing a new node with `tool` from `start`, in the coordinates of
    /// `parent`.
    /// `snap` is what its corners can line up with, if they can.
    Draw { tool: Tool, parent: NodeId, start: Point, to_parent: Affine, node: Option<NodeId>, snap: Option<Vec<Rect>> },
    /// Moving the selection: each node with the transform it started with
    /// and the way from the page's coordinates to its parent's.
    /// `deselect` is the selected node that Shift went down on: a click
    /// takes it out of the selection, a drag moves it with the rest.
    /// `snap` is the box round them as the drag began and what it can line
    /// up with, if it can.
    Move { start: Point, nodes: Vec<(NodeId, Affine, Affine)>, moved: bool, deselect: Option<NodeId>, snap: Option<(Rect, Vec<Rect>)> },
    /// Resizing `node` by one of its handles: the transform and size it
    /// started with, and the way from the page to its own coordinates then.
    /// `offset` is from where the pointer took the handle to the handle
    /// itself, so the box doesn't jump to the pointer.
    Resize { node: NodeId, handle: Handle, began: Affine, size: Size, to_local: Affine, offset: Vec2, moved: bool, snap: Option<Vec<Rect>> },
    /// Resizing several nodes, or a group, by a handle of the box round
    /// them: the document and the box (`area`, in coordinates that
    /// `to_page` takes to the page) as they were, to start again from at
    /// each move of the pointer.
    Stretch { began: Document, handle: Handle, to_page: Affine, area: Rect, offset: Vec2, moved: bool },
    /// Turning the selection about `centre`, from the angle the pointer was
    /// at when it took hold.
    Rotate { began: Document, centre: Point, from: f64, moved: bool },
    /// Dragging out a box that selects what it touches. `kept` is what
    /// Shift keeps of the selection before it; `click` is the frame whose
    /// background the pointer went down on, which a click there selects.
    Marquee { page: NodeId, start: Point, to: Point, kept: Vec<NodeId>, click: Option<NodeId>, moved: bool },
    /// Moving one end of the line `node`. The other end stays at `fixed`
    /// (in the coordinates of the line's parent, which `to_parent` gives
    /// from the page's); `start` says whether it is the line's start that
    /// moves.
    End { node: NodeId, start: bool, fixed: Point, to_parent: Affine, moved: bool, snap: Option<Vec<Rect>> },
}

/// What the pointer would take hold of on the box round the selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grab {
    Resize(Handle),
    /// Just outside a corner.
    Rotate,
}

/// Where a box is resized from, as fractions of its width and height: 0 or
/// 1 for a side that moves, 0.5 for a direction that stays as it is. The
/// four corners and the four edges.
pub type Handle = (f64, f64);

pub struct Tools {
    pub tool: Tool,
    pub selection: Vec<NodeId>,
    /// How near the pointer must be to a handle to take it, in page units:
    /// a few pixels at the canvas's zoom. It is also how near a dragged
    /// edge must come to another node's to line up with it.
    pub grab: f64,
    /// Whether what is dragged lines up with its neighbours, and otherwise
    /// keeps to whole units.
    pub snap: bool,
    /// The lines that show what the drag in progress has lined up with.
    pub guides: Vec<Line>,
    drag: Option<Drag>,
}

impl Default for Tools {
    fn default() -> Self {
        Self { tool: Tool::Move, selection: Vec::new(), grab: 0.0, snap: true, guides: Vec::new(), drag: None }
    }
}

/// The boxes on the page that something in `parent` can line up with: the
/// other things in `parent`, and `parent` itself if it is a frame.
fn neighbours(document: &Document, parent: NodeId, except: &[NodeId]) -> Vec<Rect> {
    let Some(node) = document.node(parent) else { return Vec::new() };
    let mut boxes: Vec<Rect> = node.children.iter().filter(|child| child.visible && !except.contains(&child.id)).filter_map(|child| document.area(&[child.id])).collect();
    if matches!(node.kind, NodeKind::Frame { .. }) {
        boxes.extend(document.area(&[parent]));
    }
    boxes
}

/// Whether `transform` only moves things: boxes that are turned or scaled
/// have no edges that a whole number or a guide would suit.
fn only_moves(transform: Affine) -> bool {
    transform.as_coeffs()[..4] == [1.0, 0.0, 0.0, 1.0]
}

/// Where a corner or an edge dragged to `at` (on the page) should go: on
/// each of `axes` that it moves along, onto a line of one of `others` within
/// `reach`, or else onto a whole number. With the guides that show why.
fn snapped(at: Point, axes: (bool, bool), others: &[Rect], reach: f64) -> (Point, Vec<Line>) {
    let mut guides = Vec::new();
    let mut along = |value: f64, axis: Axis, across: f64| match snap_edge(value, axis, (across, across), others, reach) {
        Some((to, guide)) => {
            guides.push(guide);
            to
        }
        None => value.round(),
    };
    let x = if axes.0 { along(at.x, Axis::X, at.y) } else { at.x };
    let y = if axes.1 { along(at.y, Axis::Y, at.x) } else { at.y };
    (Point::new(x, y), guides)
}

/// The box from `start` to `to`: a square with Shift, and with Alt grown
/// from `start` as its middle instead of its corner.
fn drawn(start: Point, to: Point, keys: Keys) -> Rect {
    let mut reach = to - start;
    if keys.shift {
        let side = reach.x.abs().max(reach.y.abs());
        // A drag straight along an axis still makes a square, down and right.
        let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
        reach = Vec2::new(side * sign(reach.x), side * sign(reach.y));
    }
    let from = if keys.alt { start - reach } else { start };
    Rect::from_points(from, start + reach)
}

/// Where a line from `from` to `to` sits and how long it is: Shift keeps it
/// to the nearest eighth of a turn. A line runs along its own x axis.
fn line(from: Point, to: Point, keys: Keys) -> (Affine, Size) {
    let (mut angle, length) = ((to - from).atan2(), (to - from).hypot());
    if keys.shift {
        angle = (angle / std::f64::consts::FRAC_PI_4).round() * std::f64::consts::FRAC_PI_4;
    }
    (Affine::translate(from.to_vec2()) * Affine::rotate(angle), Size::new(length, 0.0))
}

/// Where a node drawn with `tool` from `start` to `to` sits, and its size.
fn placed(tool: Tool, start: Point, to: Point, keys: Keys) -> (Affine, Size) {
    if matches!(tool, Tool::Line | Tool::Arrow) {
        // Alt draws it from its middle.
        let from = if keys.alt { start - (to - start) } else { start };
        return line(from, to, keys);
    }
    let area = drawn(start, to, keys);
    (Affine::translate(area.origin().to_vec2()), area.size())
}

/// A box of `size` after the pointer has taken `handle` to `to` (in the
/// box's own coordinates): Shift keeps its proportions, and Alt moves the
/// far side as much the other way, so its middle stays put. Dragged past
/// its far side, the box comes out the other way round rather than inside
/// out.
fn resized(size: Size, handle: Handle, to: Point, keys: Keys) -> Rect {
    // The near and far edge along one axis: the side with the handle follows
    // the pointer.
    let span = |handle: f64, extent: f64, to: f64| match handle {
        0.0 => (to, extent),
        1.0 => (0.0, to),
        _ => (0.0, extent),
    };
    let (mut x, mut y) = (span(handle.0, size.width, to.x), span(handle.1, size.height, to.y));
    if keys.shift && size.width > 0.0 && size.height > 0.0 {
        let (grown_x, grown_y) = ((x.1 - x.0) / size.width, (y.1 - y.0) / size.height);
        // A corner takes the larger change for both; an edge passes its own
        // change to the other direction, about the middle.
        let scale = match handle {
            (0.5, _) => grown_y.abs(),
            (_, 0.5) => grown_x.abs(),
            _ => grown_x.abs().max(grown_y.abs()),
        };
        let keep = |handle: f64, span: (f64, f64), grown: f64, extent: f64| {
            let length = extent * scale * if grown < 0.0 { -1.0 } else { 1.0 };
            match handle {
                0.0 => (span.1 - length, span.1),
                1.0 => (span.0, span.0 + length),
                _ => ((extent - extent * scale) / 2.0, (extent + extent * scale) / 2.0),
            }
        };
        (x, y) = (keep(handle.0, x, grown_x, size.width), keep(handle.1, y, grown_y, size.height));
    }
    if keys.alt {
        let mirror = |handle: f64, span: (f64, f64), extent: f64| match handle {
            0.0 => (span.0, extent - span.0),
            1.0 => (extent - span.1, span.1),
            _ => span,
        };
        (x, y) = (mirror(handle.0, x, size.width), mirror(handle.1, y, size.height));
    }
    Rect::from_points((x.0, y.0), (x.1, y.1))
}

/// A move of `by` on the page, as a node whose parent is reached by
/// `to_parent` sees it.
fn in_parent(to_parent: Affine, by: Vec2) -> Vec2 {
    to_parent * by.to_point() - to_parent * Point::ZERO
}

/// Each of `ids` with the transform it has now and the way from the page's
/// coordinates to its parent's: what a move starts from. A node inside
/// another of them moves with that one, and isn't moved twice.
fn starts(document: &Document, ids: &[NodeId]) -> Vec<(NodeId, Affine, Affine)> {
    let start_of = |id: NodeId| {
        let parent = document.parent(id).and_then(|parent| document.to_page(parent)).unwrap_or_default();
        Some((id, document.node(id)?.transform, parent.inverse()))
    };
    document.roots(ids).into_iter().filter_map(start_of).collect()
}

/// What a marquee over `area` of `page` selects: whatever it touches of
/// the page's children, and of what is in a top-level frame unless the
/// whole frame is inside it.
fn touched(document: &Document, page: NodeId, area: Rect) -> Vec<NodeId> {
    let within = |parent: NodeId| document.node(parent).into_iter().flat_map(|node| &node.children).filter(|child| child.visible && !child.locked);
    let on_page = |id: NodeId| document.area(&[id]).unwrap_or_default();
    let mut found = Vec::new();
    for child in within(page) {
        let whole = on_page(child.id);
        if matches!(child.kind, NodeKind::Frame { .. }) && !child.children.is_empty() && !area.contains_rect(whole) {
            found.extend(within(child.id).filter(|inner| on_page(inner.id).overlaps(area)).map(|inner| inner.id));
        } else if whole.overlaps(area) {
            found.push(child.id);
        }
    }
    found
}

impl Tools {
    /// The box the selection is resized and turned by: one node's own, at
    /// whatever angle it sits, or an upright one round several. As the way
    /// from the box's coordinates to the page's, and the box in them.
    pub fn frame(&self, document: &Document) -> Option<(Affine, Rect)> {
        match self.selection[..] {
            [] => None,
            [one] => Some((document.to_page(one)?, document.node(one)?.bounds())),
            _ => Some((Affine::IDENTITY, document.area(&self.selection)?)),
        }
    }

    /// The selection, if it is one line.
    fn line<'a>(&self, document: &'a Document) -> Option<&'a Node> {
        match self.selection[..] {
            [one] => document.node(one).filter(|node| node.kind == NodeKind::Line),
            _ => None,
        }
    }

    /// What of the selection's box is within reach of `at`: a corner, then
    /// an edge, then the space just outside a corner that turns it.
    pub fn grab_at(&self, document: &Document, at: Point) -> Option<Grab> {
        if self.selection.iter().any(|id| document.node(*id).is_none_or(|node| node.locked)) {
            return None;
        }
        let (to_page, area) = self.frame(document)?;
        let on_page = |handle: Handle| to_page * Point::new(area.x0 + handle.0 * area.width(), area.y0 + handle.1 * area.height());
        let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let nearest = corners.into_iter().map(|corner| (on_page(corner) - at).hypot()).fold(f64::INFINITY, f64::min);
        let corner = corners.into_iter().find(|corner| (on_page(*corner) - at).hypot() <= self.grab);
        // An edge is taken anywhere along it, as in Figma.
        let edge = (0..4).map(|i| (corners[i], corners[(i + 1) % 4])).find_map(|(a, b)| {
            let near = Line::new(on_page(a), on_page(b)).nearest(at, 1e-9).distance_sq <= self.grab * self.grab;
            near.then_some(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0))
        });
        // A line is taken by its ends alone: the rest of it moves it.
        if self.line(document).is_some() {
            return corner.map(Grab::Resize);
        }
        let outside = !area.contains(to_page.inverse() * at);
        corner.or(edge).map(Grab::Resize).or((outside && nearest <= self.grab * 3.0).then_some(Grab::Rotate))
    }

    /// The node a click at `at` selects: the deepest one there that a click
    /// can reach, which is a child of the page, of a top-level frame, or of
    /// whatever holds something that is selected already. With `deep`
    /// (Ctrl), the deepest there is.
    fn pick(&self, document: &Document, page: NodeId, at: Point, deep: bool) -> Option<NodeId> {
        let chain = document.node(page)?.hit(at);
        if deep {
            return chain.last().copied();
        }
        let holds_selection = |id: NodeId| self.selection.iter().any(|selected| std::iter::successors(document.parent(*selected), |above| document.parent(*above)).any(|above| above == id));
        let open = |index: usize| match index.checked_sub(1).map(|above| chain[above]) {
            None => true,
            Some(parent) => (index == 1 && document.node(parent).is_some_and(|node| matches!(node.kind, NodeKind::Frame { .. }))) || holds_selection(parent),
        };
        (0..chain.len()).rev().find(|index| open(*index)).map(|index| chain[index])
    }

    /// The box being dragged out to select with, on the page.
    pub fn marquee(&self) -> Option<Rect> {
        match &self.drag {
            Some(Drag::Marquee { start, to, moved: true, .. }) => Some(Rect::from_points(*start, *to)),
            _ => None,
        }
    }

    /// A double click at `at`: goes one deeper into what is selected there.
    pub fn enter(&mut self, history: &History, page: NodeId, at: Point) {
        let chain = history.document().node(page).map(|page| page.hit(at)).unwrap_or_default();
        let selected = chain.iter().position(|id| self.selection.contains(id));
        if let Some(inside) = selected.and_then(|selected| chain.get(selected + 1)) {
            self.selection = vec![*inside];
        }
    }

    /// The pointer went down at `at` on `page`.
    pub fn press(&mut self, history: &History, page: NodeId, at: Point, keys: Keys) {
        let document = history.document();
        self.drag = match self.tool.draws() {
            Some(_) => {
                // A new node goes into the deepest frame under the pointer.
                let chain = document.node(page).map(|page| page.hit(at)).unwrap_or_default();
                let frame = chain.iter().rev().copied().find(|id| document.node(*id).is_some_and(|node| matches!(node.kind, NodeKind::Frame { .. })));
                let parent = frame.unwrap_or(page);
                let to_parent = document.to_page(parent).unwrap_or_default().inverse();
                let snap = (self.snap && only_moves(to_parent)).then(|| neighbours(document, parent, &[]));
                let at = snap.as_ref().map_or(at, |others| snapped(at, (true, true), others, self.grab).0);
                Some(Drag::Draw { tool: self.tool, parent, start: to_parent * at, to_parent, node: None, snap })
            }
            None => {
                // A handle of the selection's box comes before whatever is under it.
                if let (Some(grab), Some((to_page, area))) = (self.grab_at(document, at), self.frame(document)) {
                    let held = |handle: Handle| Point::new(area.x0 + handle.0 * area.width(), area.y0 + handle.1 * area.height()) - to_page.inverse() * at;
                    let one = match self.selection[..] {
                        [one] => document.node(one).filter(|node| node.kind != NodeKind::Group),
                        _ => None,
                    };
                    let parent = document.parent(self.selection[0]).unwrap_or(page);
                    let to_parent = document.to_page(parent).unwrap_or_default().inverse();
                    let snap = (self.snap && only_moves(to_page)).then(|| neighbours(document, parent, &self.selection));
                    self.drag = Some(match (grab, one) {
                        (Grab::Resize(handle), Some(node)) if node.kind == NodeKind::Line => {
                            // The end that stays is the one the handle isn't on.
                            let fixed = node.transform * Point::new((1.0 - handle.0) * node.size.width, 0.0);
                            Drag::End { node: node.id, start: handle.0 == 0.0, fixed, to_parent, moved: false, snap: snap.filter(|_| only_moves(to_parent)) }
                        }
                        (Grab::Resize(handle), Some(node)) => Drag::Resize { node: node.id, handle, began: node.transform, size: node.size, to_local: to_page.inverse(), offset: held(handle), moved: false, snap },
                        (Grab::Resize(handle), None) => Drag::Stretch { began: document.clone(), handle, to_page, area, offset: held(handle), moved: false },
                        (Grab::Rotate, _) => {
                            let centre = to_page * area.center();
                            Drag::Rotate { began: document.clone(), centre, from: (at - centre).atan2(), moved: false }
                        }
                    });
                    return;
                }
                let picked = self.pick(document, page, at, keys.ctrl);
                // A top-level frame with things in it is selected by a click
                // on its background, but a drag from there selects in it.
                let mut deselect = None;
                let background = picked.filter(|id| !self.selection.contains(id) && document.parent(*id) == Some(page) && document.node(*id).is_some_and(|node| matches!(node.kind, NodeKind::Frame { .. }) && !node.children.is_empty()));
                match picked {
                    None => {}
                    Some(_) if background.is_some() => {}
                    // Shift adds to the selection, or (on release) takes away.
                    Some(node) if keys.shift && !self.selection.contains(&node) => self.selection.push(node),
                    Some(node) if keys.shift => deselect = Some(node),
                    Some(node) if !self.selection.contains(&node) => self.selection = vec![node],
                    Some(_) => {}
                }
                if picked.is_none() || background.is_some() {
                    let kept = if keys.shift { self.selection.clone() } else { Vec::new() };
                    self.selection.clone_from(&kept);
                    Some(Drag::Marquee { page, start: at, to: at, kept, click: background, moved: false })
                } else {
                    let nodes = starts(document, &self.selection);
                    // Lining up is with what is beside the first of them.
                    let beside = nodes.first().filter(|(_, _, to_parent)| self.snap && only_moves(*to_parent)).and_then(|(first, ..)| document.parent(*first));
                    let snap = beside.and_then(|parent| Some((document.area(&self.selection)?, neighbours(document, parent, &self.selection))));
                    Some(Drag::Move { start: at, nodes, moved: false, deselect, snap })
                }
            }
        };
    }

    /// The pointer moved to `at` with the button held.
    pub fn drag(&mut self, history: &mut History, at: Point, keys: Keys) -> Result<(), Error> {
        match &mut self.drag {
            Some(Drag::Draw { tool, parent, start, to_parent, node, snap }) => {
                let at = match snap {
                    Some(others) => {
                        let (at, guides) = snapped(at, (true, true), others, self.grab);
                        self.guides = guides;
                        at
                    }
                    None => at,
                };
                let (transform, size) = placed(*tool, *start, *to_parent * at, keys);
                let (tool, parent) = (*tool, *parent);
                if node.is_none() {
                    history.begin("Draw");
                }
                let drawing = history.edit("Draw", |document| {
                    let id = match *node {
                        Some(id) => id,
                        None => {
                            let new = tool.node(document).ok_or(Error::Nothing)?;
                            let id = new.id;
                            document.insert(parent, usize::MAX, new)?;
                            id
                        }
                    };
                    let shape = document.node_mut(id)?;
                    (shape.transform, shape.size) = (transform, size);
                    Ok(id)
                })?;
                *node = Some(drawing);
                self.selection = vec![drawing];
            }
            Some(Drag::End { node, start, fixed, to_parent, moved, snap }) => {
                if !*moved {
                    history.begin("Resize");
                    *moved = true;
                }
                let at = match snap {
                    Some(others) => {
                        let (at, guides) = snapped(at, (true, true), others, self.grab);
                        self.guides = guides;
                        at
                    }
                    None => at,
                };
                let end = *to_parent * at;
                // A line starts at its origin: Shift's angle is about the end that stays.
                let (transform, size) = if *start {
                    let (turned, size) = line(*fixed, end, keys);
                    let end = turned * Point::new(size.width, 0.0);
                    (Affine::translate(end.to_vec2()) * Affine::rotate((*fixed - end).atan2()), size)
                } else {
                    line(*fixed, end, keys)
                };
                history.edit("Resize", |document| {
                    let shape = document.node_mut(*node)?;
                    (shape.transform, shape.size) = (transform, size);
                    Ok(())
                })?;
            }
            Some(Drag::Move { start, nodes, moved, snap, .. }) => {
                if !*moved {
                    history.begin(if keys.alt { "Duplicate" } else { "Move" });
                    *moved = true;
                    if keys.alt {
                        // Alt leaves the selection where it is and takes a copy.
                        let originals: Vec<NodeId> = nodes.iter().map(|(id, ..)| *id).collect();
                        self.selection = history.edit("Duplicate", |document| document.duplicate(&originals))?;
                        *nodes = starts(history.document(), &self.selection);
                    }
                }
                let mut by = at - *start;
                // Shift keeps the move along whichever axis it is mostly on.
                let mut free = (true, true);
                if keys.shift {
                    free = (by.x.abs() >= by.y.abs(), by.x.abs() < by.y.abs());
                    by = if free.0 { Vec2::new(by.x, 0.0) } else { Vec2::new(0.0, by.y) };
                }
                if let Some((area, others)) = snap {
                    let lined = snap_move(*area + by, others, self.grab);
                    // Along an axis with nothing to line up with, the box's
                    // corner keeps to whole units.
                    let upright = |guide: &Line| guide.p0.x == guide.p1.x;
                    let whole = |corner: f64| corner.round() - corner;
                    let moved = *area + by;
                    let x = if lined.guides.iter().any(upright) { lined.by.x } else { whole(moved.x0) };
                    let y = if lined.guides.iter().any(|guide| !upright(guide)) { lined.by.y } else { whole(moved.y0) };
                    by += Vec2::new(if free.0 { x } else { 0.0 }, if free.1 { y } else { 0.0 });
                    // The lines for where it has ended up, whole units and all.
                    self.guides = snap_move(*area + by, others, 0.0).guides.into_iter().filter(|guide| if upright(guide) { free.0 } else { free.1 }).collect();
                }
                history.edit("Move", |document| {
                    for (id, began, to_parent) in nodes.iter() {
                        // The drag is in page coordinates; a node moves in its parent's.
                        document.node_mut(*id)?.transform = Affine::translate(in_parent(*to_parent, by)) * *began;
                    }
                    Ok(())
                })?;
            }
            Some(Drag::Resize { node, handle, began, size, to_local, offset, moved, snap }) => {
                if !*moved {
                    history.begin("Resize");
                    *moved = true;
                }
                // The side of the box that the handle is on goes to the
                // pointer, less the bit it was taken off by.
                let to = match snap {
                    Some(others) => {
                        let (to, guides) = snapped(at + *offset, (handle.0 != 0.5, handle.1 != 0.5), others, self.grab);
                        self.guides = guides;
                        *to_local * to
                    }
                    None => *to_local * at + *offset,
                };
                let area = resized(*size, *handle, to, keys);
                history.edit("Resize", |document| {
                    let shape = document.node_mut(*node)?;
                    (shape.transform, shape.size) = (*began * Affine::translate(area.origin().to_vec2()), area.size());
                    Ok(())
                })?;
            }
            Some(Drag::Stretch { began, handle, to_page, area, offset, moved }) => {
                if !*moved {
                    history.begin("Resize");
                    *moved = true;
                }
                let corner = area.origin().to_vec2();
                let now = resized(area.size(), *handle, to_page.inverse() * at + *offset - corner, keys) + corner;
                // A box with no width can't be scaled to one that has some.
                let factor = |now: f64, was: f64| if was == 0.0 { 1.0 } else { now / was };
                let scale = Affine::translate(now.origin().to_vec2()) * Affine::scale_non_uniform(factor(now.width(), area.width()), factor(now.height(), area.height())) * Affine::translate(-corner);
                let on_page = *to_page * scale * to_page.inverse();
                let selection = &self.selection;
                history.edit("Resize", |document| {
                    document.clone_from(began);
                    for id in document.roots(selection) {
                        let parent = document.parent(id).and_then(|parent| document.to_page(parent)).unwrap_or_default();
                        document.node_mut(id)?.stretch(parent.inverse() * on_page * parent);
                    }
                    Ok(())
                })?;
            }
            Some(Drag::Rotate { began, centre, from, moved }) => {
                if !*moved {
                    history.begin("Rotate");
                    *moved = true;
                }
                let mut angle = (at - *centre).atan2() - *from;
                // Shift turns by fifteen degrees at a time.
                if keys.shift {
                    let step = 15f64.to_radians();
                    angle = (angle / step).round() * step;
                }
                let turn = Affine::rotate_about(angle, *centre);
                let selection = &self.selection;
                history.edit("Rotate", |document| {
                    document.clone_from(began);
                    for id in document.roots(selection) {
                        let parent = document.parent(id).and_then(|parent| document.to_page(parent)).unwrap_or_default();
                        let node = document.node_mut(id)?;
                        node.transform = parent.inverse() * turn * parent * node.transform;
                    }
                    Ok(())
                })?;
            }
            Some(Drag::Marquee { page, start, to, kept, moved, .. }) => {
                (*to, *moved) = (at, true);
                let mut selection = kept.clone();
                for id in touched(history.document(), *page, Rect::from_points(*start, at)) {
                    // With Shift, what was selected is taken away, as by a click.
                    match selection.iter().position(|selected| *selected == id) {
                        Some(index) => drop(selection.remove(index)),
                        None => selection.push(id),
                    }
                }
                self.selection = selection;
            }
            None => {}
        }
        Ok(())
    }

    /// The button came up.
    pub fn release(&mut self, history: &mut History) -> Result<(), Error> {
        self.guides.clear();
        match self.drag.take() {
            // A click with a drawing tool: a shape of the usual size, there.
            Some(Drag::Draw { tool, parent, start, node: None, .. }) => {
                let id = history.edit("Draw", |document| {
                    let mut new = tool.node(document).ok_or(Error::Nothing)?;
                    new.size = if new.kind == NodeKind::Line { Size::new(CLICK_SIZE, 0.0) } else { Size::new(CLICK_SIZE, CLICK_SIZE) };
                    new.transform = Affine::translate(start.to_vec2());
                    let id = new.id;
                    document.insert(parent, usize::MAX, new)?;
                    Ok(id)
                })?;
                self.selection = vec![id];
                self.tool = Tool::Move;
            }
            Some(Drag::Draw { .. }) => {
                history.commit();
                // As in Figma, a shape drawn hands back to the Move tool.
                self.tool = Tool::Move;
            }
            Some(Drag::Move { moved: false, deselect: Some(node), .. }) => self.selection.retain(|selected| *selected != node),
            Some(Drag::Move { .. } | Drag::Resize { .. } | Drag::Stretch { .. } | Drag::Rotate { .. } | Drag::End { .. }) => history.commit(),
            // A click on a frame's background selects the frame.
            Some(Drag::Marquee { click: Some(frame), moved: false, .. }) => match self.selection.iter().position(|selected| *selected == frame) {
                Some(index) => drop(self.selection.remove(index)),
                None => self.selection.push(frame),
            },
            Some(Drag::Marquee { .. }) | None => {}
        }
        Ok(())
    }

    /// Esc: gives up the drag in progress, or else the drawing tool, or
    /// else the selection.
    pub fn cancel(&mut self, history: &mut History) {
        self.guides.clear();
        match self.drag.take() {
            Some(Drag::Draw { node, .. }) => {
                history.cancel();
                self.selection.retain(|selected| Some(*selected) != node);
            }
            Some(Drag::Move { .. } | Drag::Resize { .. } | Drag::Stretch { .. } | Drag::Rotate { .. } | Drag::End { .. }) => history.cancel(),
            Some(Drag::Marquee { kept, .. }) => self.selection = kept,
            None if self.tool != Tool::Move => self.tool = Tool::Move,
            None => self.selection.clear(),
        }
    }

    /// The arrow keys: moves the selection `by` on the page, as one step.
    pub fn nudge(&mut self, history: &mut History, by: Vec2) -> Result<(), Error> {
        let selection = &self.selection;
        history.edit("Nudge", |document| {
            for id in selection {
                let to_parent = document.parent(*id).and_then(|parent| document.to_page(parent)).unwrap_or_default().inverse();
                let node = document.node_mut(*id)?;
                node.transform = Affine::translate(in_parent(to_parent, by)) * node.transform;
            }
            Ok(())
        })
    }

    /// Delete: removes the selected nodes, as one step.
    pub fn delete(&mut self, history: &mut History) -> Result<(), Error> {
        let selection = std::mem::take(&mut self.selection);
        history.edit("Delete", |document| {
            for id in &selection {
                // One of them may have been inside another that's already gone.
                if document.node(*id).is_some() {
                    document.remove(*id)?;
                }
            }
            Ok(())
        })
    }

    /// Drops from the selection whatever is no longer in the document, as
    /// after an undo.
    pub fn forget_missing(&mut self, document: &Document) {
        self.selection.retain(|id| document.node(*id).is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document, its history and the tools, driven like a pointer.
    struct Desk {
        history: History,
        tools: Tools,
        page: NodeId,
    }

    impl Desk {
        fn new() -> Self {
            let document = Document::default();
            let page = document.pages[0].id;
            Self { history: History::new(document), tools: Tools::default(), page }
        }

        /// Presses at `from`, drags through `via`, and releases.
        fn stroke(&mut self, from: (f64, f64), via: &[(f64, f64)], keys: Keys) {
            self.tools.press(&self.history, self.page, from.into(), keys);
            for at in via {
                self.tools.drag(&mut self.history, (*at).into(), keys).unwrap();
            }
            self.tools.release(&mut self.history).unwrap();
        }

        fn draw(&mut self, tool: Tool, from: (f64, f64), to: (f64, f64)) -> NodeId {
            self.tools.tool = tool;
            self.stroke(from, &[to], Keys::default());
            self.tools.selection[0]
        }

        /// Where a node's box is on the page.
        fn bounds(&self, id: NodeId) -> Rect {
            let document = self.history.document();
            document.to_page(id).unwrap().transform_rect_bbox(document.node(id).unwrap().bounds())
        }

        fn children(&self, id: NodeId) -> Vec<NodeId> {
            self.history.document().node(id).unwrap().children.iter().map(|node| node.id).collect()
        }
    }

    #[test]
    fn dragging_with_the_rectangle_tool_draws_one() {
        let mut desk = Desk::new();
        desk.tools.tool = Tool::Rectangle;
        // Up and to the left, through a few points on the way.
        desk.stroke((300.0, 200.0), &[(280.0, 190.0), (250.0, 150.0), (100.0, 80.0)], Keys::default());
        let id = desk.tools.selection[0];
        let node = desk.history.document().node(id).unwrap();
        assert_eq!(node.kind, NodeKind::Rectangle);
        assert_eq!(desk.bounds(id), Rect::new(100.0, 80.0, 300.0, 200.0));
        assert_eq!(desk.children(desk.page), [id]);
        // The tool hands back to Move, and the whole drag is one step.
        assert_eq!(desk.tools.tool, Tool::Move);
        assert_eq!(desk.history.undo_name(), Some("Draw"));
        assert!(desk.history.undo());
        assert!(desk.children(desk.page).is_empty());
        assert!(!desk.history.undo());
    }

    #[test]
    fn shift_draws_a_square_and_alt_draws_from_the_middle() {
        let mut desk = Desk::new();
        let (shift, alt) = (Keys { shift: true, alt: false, ctrl: false }, Keys { shift: false, alt: true, ctrl: false });
        desk.tools.tool = Tool::Ellipse;
        desk.stroke((100.0, 100.0), &[(160.0, 130.0)], shift);
        assert_eq!(desk.bounds(desk.tools.selection[0]), Rect::new(100.0, 100.0, 160.0, 160.0));
        // Up and left: the square grows that way.
        desk.tools.tool = Tool::Ellipse;
        desk.stroke((100.0, 100.0), &[(80.0, 40.0)], shift);
        assert_eq!(desk.bounds(desk.tools.selection[0]), Rect::new(40.0, 40.0, 100.0, 100.0));
        desk.tools.tool = Tool::Rectangle;
        desk.stroke((100.0, 100.0), &[(130.0, 110.0)], alt);
        assert_eq!(desk.bounds(desk.tools.selection[0]), Rect::new(70.0, 90.0, 130.0, 110.0));
        desk.tools.tool = Tool::Rectangle;
        desk.stroke((100.0, 100.0), &[(130.0, 110.0)], Keys { shift: true, alt: true, ctrl: false });
        assert_eq!(desk.bounds(desk.tools.selection[0]), Rect::new(70.0, 70.0, 130.0, 130.0));
    }

    #[test]
    fn a_click_with_a_drawing_tool_draws_the_usual_size() {
        let mut desk = Desk::new();
        desk.tools.tool = Tool::Frame;
        desk.stroke((50.0, 60.0), &[], Keys::default());
        let id = desk.tools.selection[0];
        assert_eq!(desk.history.document().node(id).unwrap().kind, NodeKind::Frame { clip: true });
        assert_eq!(desk.bounds(id), Rect::new(50.0, 60.0, 150.0, 160.0));
        assert_eq!(desk.tools.tool, Tool::Move);
        assert!(desk.history.undo());
        assert!(desk.children(desk.page).is_empty());
    }

    #[test]
    fn a_shape_drawn_over_a_frame_goes_into_it() {
        let mut desk = Desk::new();
        let frame = desk.draw(Tool::Frame, (100.0, 100.0), (500.0, 400.0));
        let inner = desk.draw(Tool::Frame, (300.0, 200.0), (450.0, 350.0));
        assert_eq!(desk.children(frame), [inner]);
        // Started over the inner frame: its child, in its coordinates.
        let dot = desk.draw(Tool::Ellipse, (320.0, 220.0), (340.0, 250.0));
        assert_eq!(desk.children(inner), [dot]);
        assert_eq!(desk.history.document().node(dot).unwrap().transform, Affine::translate((20.0, 20.0)));
        assert_eq!(desk.bounds(dot), Rect::new(320.0, 220.0, 340.0, 250.0));
        // Started on the bare page: the page's child, even if it ends over a frame.
        let outside = desk.draw(Tool::Rectangle, (10.0, 10.0), (200.0, 200.0));
        assert_eq!(desk.children(desk.page), [frame, outside]);
    }

    #[test]
    fn esc_gives_up_a_shape_being_drawn_then_the_tool() {
        let mut desk = Desk::new();
        desk.tools.tool = Tool::Rectangle;
        desk.tools.press(&desk.history, desk.page, (10.0, 10.0).into(), Keys::default());
        desk.tools.drag(&mut desk.history, (50.0, 50.0).into(), Keys::default()).unwrap();
        assert_eq!(desk.children(desk.page).len(), 1);
        desk.tools.cancel(&mut desk.history);
        assert!(desk.children(desk.page).is_empty());
        assert!(desk.tools.selection.is_empty());
        assert_eq!((desk.history.undo_name(), desk.history.is_dirty()), (None, false));
        // Still the Rectangle tool, until Esc again.
        assert_eq!(desk.tools.tool, Tool::Rectangle);
        desk.tools.cancel(&mut desk.history);
        assert_eq!(desk.tools.tool, Tool::Move);
        // And once more lets go of the selection.
        desk.draw(Tool::Rectangle, (10.0, 10.0), (50.0, 50.0));
        assert_eq!(desk.tools.selection.len(), 1);
        desk.tools.cancel(&mut desk.history);
        assert!(desk.tools.selection.is_empty());
    }

    #[test]
    fn a_click_selects_and_shift_click_adds_or_takes_away() {
        let mut desk = Desk::new();
        let a = desk.draw(Tool::Rectangle, (0.0, 0.0), (100.0, 100.0));
        let b = desk.draw(Tool::Rectangle, (200.0, 0.0), (300.0, 100.0));
        let shift = Keys { shift: true, alt: false, ctrl: false };
        desk.stroke((50.0, 50.0), &[], Keys::default());
        assert_eq!(desk.tools.selection, [a]);
        desk.stroke((250.0, 50.0), &[], shift);
        assert_eq!(desk.tools.selection, [a, b]);
        desk.stroke((50.0, 50.0), &[], shift);
        assert_eq!(desk.tools.selection, [b]);
        // Shift-click on nothing keeps the selection; a plain click clears it.
        desk.stroke((500.0, 500.0), &[], shift);
        assert_eq!(desk.tools.selection, [b]);
        desk.stroke((500.0, 500.0), &[], Keys::default());
        assert!(desk.tools.selection.is_empty());
        // None of that is an edit.
        assert_eq!(desk.history.undo_name(), Some("Draw"));
    }

    #[test]
    fn a_click_in_a_top_level_frame_selects_what_is_in_it() {
        let mut desk = Desk::new();
        let frame = desk.draw(Tool::Frame, (100.0, 100.0), (500.0, 400.0));
        let inner = desk.draw(Tool::Frame, (300.0, 200.0), (450.0, 350.0));
        let dot = desk.draw(Tool::Ellipse, (320.0, 220.0), (360.0, 260.0));
        desk.stroke((900.0, 900.0), &[], Keys::default());
        desk.stroke((340.0, 240.0), &[], Keys::default());
        // The frame's child, not the child's child.
        assert_eq!(desk.tools.selection, [inner]);
        desk.stroke((120.0, 120.0), &[], Keys::default());
        assert_eq!(desk.tools.selection, [frame]);
        assert_eq!(desk.children(inner), [dot]);
    }

    #[test]
    fn ctrl_click_and_a_double_click_reach_further_in() {
        let mut desk = Desk::new();
        let frame = desk.draw(Tool::Frame, (100.0, 100.0), (500.0, 400.0));
        let inner = desk.draw(Tool::Frame, (300.0, 200.0), (450.0, 350.0));
        let dot = desk.draw(Tool::Ellipse, (320.0, 220.0), (360.0, 260.0));
        let other = desk.draw(Tool::Ellipse, (400.0, 300.0), (420.0, 320.0));
        assert_eq!(desk.children(inner), [dot, other]);
        desk.stroke((900.0, 900.0), &[], Keys::default());
        // Ctrl goes straight to the deepest thing there.
        desk.stroke((340.0, 240.0), &[], Keys { ctrl: true, ..Keys::default() });
        assert_eq!(desk.tools.selection, [dot]);
        // With something in `inner` selected, a click reaches what is beside it.
        desk.stroke((410.0, 310.0), &[], Keys::default());
        assert_eq!(desk.tools.selection, [other]);
        // But a click elsewhere in the frame is back to the frame's children.
        let beside = desk.draw(Tool::Rectangle, (120.0, 300.0), (160.0, 340.0));
        assert_eq!(desk.children(frame), [inner, beside]);
        desk.stroke((340.0, 240.0), &[], Keys::default());
        assert_eq!(desk.tools.selection, [inner]);
        // A double click goes one further in each time.
        desk.tools.enter(&desk.history, desk.page, (340.0, 240.0).into());
        assert_eq!(desk.tools.selection, [dot]);
        desk.tools.enter(&desk.history, desk.page, (340.0, 240.0).into());
        assert_eq!(desk.tools.selection, [dot], "nothing further in");
        // On nothing that is selected, it does nothing.
        desk.tools.enter(&desk.history, desk.page, (140.0, 320.0).into());
        assert_eq!(desk.tools.selection, [dot]);
    }

    #[test]
    fn a_drag_from_nothing_selects_what_its_box_touches() {
        let mut desk = Desk::new();
        let a = desk.draw(Tool::Rectangle, (0.0, 0.0), (100.0, 100.0));
        let b = desk.draw(Tool::Rectangle, (200.0, 0.0), (300.0, 100.0));
        let frame = desk.draw(Tool::Frame, (0.0, 200.0), (300.0, 400.0));
        let in_frame = desk.draw(Tool::Ellipse, (20.0, 220.0), (60.0, 260.0));
        let far = desk.draw(Tool::Ellipse, (240.0, 340.0), (280.0, 380.0));
        let none = Keys::default();
        let drag = |desk: &mut Desk, from: (f64, f64), to: (f64, f64), keys| {
            desk.tools.press(&desk.history, desk.page, from.into(), keys);
            desk.tools.drag(&mut desk.history, to.into(), keys).unwrap();
        };
        // From the bare page, over a corner of each rectangle.
        drag(&mut desk, (150.0, 150.0), (90.0, 90.0), none);
        assert_eq!(desk.tools.selection, [a]);
        assert_eq!(desk.tools.marquee(), Some(Rect::new(90.0, 90.0, 150.0, 150.0)));
        desk.tools.drag(&mut desk.history, (210.0, 50.0).into(), none).unwrap();
        assert_eq!(desk.tools.selection, [b], "the box follows the pointer, and lets go of what it leaves");
        desk.tools.release(&mut desk.history).unwrap();
        assert_eq!((desk.tools.selection.clone(), desk.tools.marquee()), (vec![b], None));
        // Part of a frame with things in it: what it touches in the frame.
        drag(&mut desk, (150.0, 150.0), (30.0, 230.0), none);
        desk.tools.release(&mut desk.history).unwrap();
        assert_eq!(desk.tools.selection, [in_frame]);
        // From the frame's own background too, where a click selects the frame.
        drag(&mut desk, (150.0, 300.0), (250.0, 350.0), none);
        desk.tools.release(&mut desk.history).unwrap();
        assert_eq!(desk.tools.selection, [far]);
        // Round the whole frame: the frame.
        drag(&mut desk, (-10.0, 150.0), (310.0, 410.0), none);
        desk.tools.release(&mut desk.history).unwrap();
        assert_eq!(desk.tools.selection, [frame]);
        // Shift adds what wasn't selected and takes away what was.
        let shift = Keys { shift: true, ..none };
        drag(&mut desk, (-10.0, -10.0), (310.0, 110.0), shift);
        desk.tools.release(&mut desk.history).unwrap();
        assert_eq!(desk.tools.selection, [frame, a, b]);
        drag(&mut desk, (150.0, -20.0), (250.0, 120.0), shift);
        assert_eq!(desk.tools.selection, [frame, a]);
        // Esc puts the selection back as it was.
        desk.tools.cancel(&mut desk.history);
        assert_eq!(desk.tools.selection, [frame, a, b]);
        // Locked and hidden things aren't selected, and none of it is an edit.
        desk.history.edit("Lock", |document| document.node_mut(a).map(|node| node.locked = true)).unwrap();
        drag(&mut desk, (150.0, 150.0), (-10.0, -10.0), none);
        desk.tools.release(&mut desk.history).unwrap();
        assert!(desk.tools.selection.is_empty());
        assert_eq!(desk.history.undo_name(), Some("Lock"));
    }

    #[test]
    fn dragging_moves_the_selection_as_one_step() {
        let mut desk = Desk::new();
        let a = desk.draw(Tool::Rectangle, (0.0, 0.0), (100.0, 100.0));
        let b = desk.draw(Tool::Rectangle, (200.0, 0.0), (300.0, 100.0));
        desk.stroke((50.0, 50.0), &[], Keys::default());
        desk.stroke((250.0, 50.0), &[], Keys { shift: true, alt: false, ctrl: false });
        // Drag by (30, -10), by way of somewhere else.
        desk.stroke((250.0, 50.0), &[(400.0, 400.0), (280.0, 40.0)], Keys::default());
        assert_eq!(desk.bounds(a), Rect::new(30.0, -10.0, 130.0, 90.0));
        assert_eq!(desk.bounds(b), Rect::new(230.0, -10.0, 330.0, 90.0));
        assert_eq!(desk.history.undo_name(), Some("Move"));
        assert!(desk.history.undo());
        assert_eq!(desk.bounds(a), Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(desk.bounds(b), Rect::new(200.0, 0.0, 300.0, 100.0));
        // Dragging something that isn't selected selects it and moves it alone.
        desk.stroke((500.0, 500.0), &[], Keys::default());
        desk.stroke((50.0, 50.0), &[(60.0, 70.0)], Keys::default());
        assert_eq!(desk.tools.selection, [a]);
        assert_eq!(desk.bounds(a), Rect::new(10.0, 20.0, 110.0, 120.0));
        assert_eq!(desk.bounds(b), Rect::new(200.0, 0.0, 300.0, 100.0));
    }

    #[test]
    fn a_node_in_a_turned_frame_follows_the_pointer_on_the_page() {
        let mut desk = Desk::new();
        let frame = desk.draw(Tool::Frame, (100.0, 100.0), (500.0, 400.0));
        let dot = desk.draw(Tool::Ellipse, (200.0, 200.0), (240.0, 240.0));
        // Turn the frame a quarter turn about its corner, and double it.
        desk.history
            .edit("Turn", |document| {
                document.node_mut(frame)?.transform = Affine::translate((100.0, 100.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2) * Affine::scale(2.0);
                Ok(())
            })
            .unwrap();
        let before = desk.bounds(dot).center();
        desk.tools.selection = vec![dot];
        desk.stroke((before.x, before.y), &[(before.x + 50.0, before.y - 20.0)], Keys::default());
        let after = desk.bounds(dot).center();
        assert!((after - before - Vec2::new(50.0, -20.0)).hypot() < 1e-9, "{before:?} to {after:?}");
    }

    /// Draws a 200 × 100 rectangle at (100, 100), selected, with handles
    /// that take anything within 4 units.
    fn with_rectangle() -> (Desk, NodeId) {
        let mut desk = Desk::new();
        let id = desk.draw(Tool::Rectangle, (100.0, 100.0), (300.0, 200.0));
        desk.tools.grab = 4.0;
        (desk, id)
    }

    #[test]
    fn handles_resize_from_the_side_they_are_on() {
        let none = Keys::default();
        let (mut desk, id) = with_rectangle();
        // The bottom-right corner, taken a little off it: it moves as far as
        // the pointer does, without jumping to it first.
        desk.stroke((302.0, 201.0), &[(352.0, 261.0)], none);
        assert_eq!(desk.bounds(id), Rect::new(100.0, 100.0, 350.0, 260.0));
        assert_eq!(desk.history.undo_name(), Some("Resize"));
        // The top-left corner: the far corner stays.
        desk.stroke((100.0, 100.0), &[(90.0, 120.0)], none);
        assert_eq!(desk.bounds(id), Rect::new(90.0, 120.0, 350.0, 260.0));
        // The left edge, anywhere along it: only the width changes.
        desk.stroke((90.0, 150.0), &[(40.0, 500.0)], none);
        assert_eq!(desk.bounds(id), Rect::new(40.0, 120.0, 350.0, 260.0));
        // The bottom edge.
        desk.stroke((200.0, 260.0), &[(0.0, 300.0)], none);
        assert_eq!(desk.bounds(id), Rect::new(40.0, 120.0, 350.0, 300.0));
        // Each was one step, and the node is still the only thing there.
        for _ in 0..4 {
            assert!(desk.history.undo());
        }
        assert_eq!(desk.bounds(id), Rect::new(100.0, 100.0, 300.0, 200.0));
        assert_eq!(desk.children(desk.page), [id]);
    }

    #[test]
    fn shift_keeps_proportions_and_alt_resizes_about_the_middle() {
        let (shift, alt) = (Keys { shift: true, alt: false, ctrl: false }, Keys { shift: false, alt: true, ctrl: false });
        // 200 × 100, corner dragged to make it 300 wide: 150 high to match.
        let (mut desk, id) = with_rectangle();
        desk.stroke((300.0, 200.0), &[(400.0, 210.0)], shift);
        assert_eq!(desk.bounds(id), Rect::new(100.0, 100.0, 400.0, 250.0));
        // An edge with Shift grows the other direction about the middle.
        let (mut desk, id) = with_rectangle();
        desk.stroke((300.0, 150.0), &[(500.0, 150.0)], shift);
        assert_eq!(desk.bounds(id), Rect::new(100.0, 50.0, 500.0, 250.0));
        // Alt: the far side moves as much the other way.
        let (mut desk, id) = with_rectangle();
        desk.stroke((300.0, 200.0), &[(310.0, 220.0)], alt);
        assert_eq!(desk.bounds(id), Rect::new(90.0, 80.0, 310.0, 220.0));
        let (mut desk, id) = with_rectangle();
        desk.stroke((100.0, 150.0), &[(120.0, 150.0)], alt);
        assert_eq!(desk.bounds(id), Rect::new(120.0, 100.0, 280.0, 200.0));
    }

    #[test]
    fn a_handle_dragged_past_the_far_side_turns_the_box_round() {
        let (mut desk, id) = with_rectangle();
        desk.stroke((300.0, 200.0), &[(60.0, 70.0)], Keys::default());
        assert_eq!(desk.bounds(id), Rect::new(60.0, 70.0, 100.0, 100.0));
        let size = desk.history.document().node(id).unwrap().size;
        assert!(size.width > 0.0 && size.height > 0.0);
    }

    #[test]
    fn a_turned_node_resizes_along_its_own_sides() {
        let (mut desk, id) = with_rectangle();
        // A quarter turn about its top-left corner: its width now runs down the page.
        desk.history
            .edit("Turn", |document| {
                document.node_mut(id)?.transform = Affine::translate((100.0, 100.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2);
                Ok(())
            })
            .unwrap();
        // Its "right" edge is the bottom one on the page, from (0, 300) to (100, 300).
        let turned = desk.bounds(id);
        assert!((turned.x1 - 100.0).abs() < 1e-9 && turned.y1 == 300.0, "{turned:?}");
        desk.stroke((50.0, 300.0), &[(50.0, 350.0)], Keys::default());
        let node = desk.history.document().node(id).unwrap();
        assert!((node.size.width - 250.0).abs() < 1e-9 && (node.size.height - 100.0).abs() < 1e-9, "{:?}", node.size);
        let bounds = desk.bounds(id);
        assert!((bounds.y1 - 350.0).abs() < 1e-9 && (bounds.x0 - 0.0).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn handles_need_one_unlocked_selected_node_and_esc_puts_a_resize_back() {
        let (mut desk, id) = with_rectangle();
        // Out of reach of the corner: a plain click on nothing.
        desk.stroke((310.0, 210.0), &[(350.0, 260.0)], Keys::default());
        assert!(desk.tools.selection.is_empty());
        assert_eq!(desk.bounds(id), Rect::new(100.0, 100.0, 300.0, 200.0));
        // Not selected: the same drag from the corner moves it instead.
        desk.stroke((299.0, 199.0), &[(309.0, 209.0)], Keys::default());
        assert_eq!(desk.bounds(id), Rect::new(110.0, 110.0, 310.0, 210.0));
        // Selected now; Esc in the middle of a resize.
        desk.tools.press(&desk.history, desk.page, (310.0, 210.0).into(), Keys::default());
        desk.tools.drag(&mut desk.history, (400.0, 400.0).into(), Keys::default()).unwrap();
        assert_eq!(desk.bounds(id).x1, 400.0);
        desk.tools.cancel(&mut desk.history);
        assert_eq!(desk.bounds(id), Rect::new(110.0, 110.0, 310.0, 210.0));
        assert_eq!(desk.history.undo_name(), Some("Move"));
    }

    #[test]
    fn arrows_nudge_the_selection_on_the_page() {
        let mut desk = Desk::new();
        let frame = desk.draw(Tool::Frame, (100.0, 100.0), (500.0, 400.0));
        let dot = desk.draw(Tool::Ellipse, (200.0, 200.0), (240.0, 240.0));
        // The frame turned upside down: right on the page is still right.
        desk.history
            .edit("Turn", |document| {
                document.node_mut(frame)?.transform = Affine::translate((500.0, 400.0)) * Affine::rotate(std::f64::consts::PI);
                Ok(())
            })
            .unwrap();
        let before = desk.bounds(dot).center();
        desk.tools.selection = vec![dot];
        desk.tools.nudge(&mut desk.history, Vec2::new(10.0, 0.0)).unwrap();
        desk.tools.nudge(&mut desk.history, Vec2::new(0.0, -1.0)).unwrap();
        assert!((desk.bounds(dot).center() - before - Vec2::new(10.0, -1.0)).hypot() < 1e-9);
        assert_eq!(desk.history.undo_name(), Some("Nudge"));
        // With nothing selected there is nothing to undo.
        let mut empty = Desk::new();
        empty.tools.nudge(&mut empty.history, Vec2::new(1.0, 0.0)).unwrap();
        assert_eq!(empty.history.undo_name(), None);
    }

    #[test]
    fn esc_puts_a_move_back_and_delete_removes_the_selection() {
        let mut desk = Desk::new();
        let frame = desk.draw(Tool::Frame, (100.0, 100.0), (500.0, 400.0));
        let dot = desk.draw(Tool::Ellipse, (200.0, 200.0), (240.0, 240.0));
        desk.tools.press(&desk.history, desk.page, (220.0, 220.0).into(), Keys::default());
        desk.tools.drag(&mut desk.history, (300.0, 300.0).into(), Keys::default()).unwrap();
        assert_eq!(desk.bounds(dot).origin(), (280.0, 280.0).into());
        desk.tools.cancel(&mut desk.history);
        assert_eq!(desk.bounds(dot).origin(), (200.0, 200.0).into());
        assert_eq!(desk.history.undo_name(), Some("Draw"));

        // A frame and something inside it, both selected: one step, no error.
        desk.tools.selection = vec![frame, dot];
        desk.tools.delete(&mut desk.history).unwrap();
        assert!(desk.children(desk.page).is_empty());
        assert!(desk.tools.selection.is_empty());
        assert!(desk.history.undo());
        assert_eq!(desk.children(frame), [dot]);
        // After an undo the selection only names what exists.
        desk.tools.selection = vec![dot, NodeId(999)];
        desk.tools.forget_missing(desk.history.document());
        assert_eq!(desk.tools.selection, [dot]);
    }

    #[test]
    fn shift_keeps_a_move_on_one_axis_and_alt_moves_a_copy() {
        let mut desk = Desk::new();
        let a = desk.draw(Tool::Rectangle, (0.0, 0.0), (100.0, 100.0));
        desk.stroke((50.0, 50.0), &[(90.0, 60.0)], Keys { shift: true, ..Keys::default() });
        assert_eq!(desk.bounds(a), Rect::new(40.0, 0.0, 140.0, 100.0));
        desk.stroke((90.0, 50.0), &[(95.0, 150.0)], Keys { shift: true, ..Keys::default() });
        assert_eq!(desk.bounds(a), Rect::new(40.0, 100.0, 140.0, 200.0));

        desk.stroke((90.0, 150.0), &[(100.0, 150.0), (290.0, 150.0)], Keys { alt: true, ..Keys::default() });
        let [original, copy] = desk.children(desk.page)[..] else { panic!() };
        assert_eq!((original, desk.tools.selection.clone()), (a, vec![copy]));
        assert_eq!(desk.bounds(a), Rect::new(40.0, 100.0, 140.0, 200.0));
        assert_eq!(desk.bounds(copy), Rect::new(240.0, 100.0, 340.0, 200.0));
        // The copy and its move are one step.
        assert_eq!(desk.history.undo_name(), Some("Duplicate"));
        assert!(desk.history.undo());
        assert_eq!(desk.children(desk.page), [a]);
    }

    #[test]
    fn a_node_and_what_is_in_it_move_once() {
        let mut desk = Desk::new();
        let frame = desk.draw(Tool::Frame, (100.0, 100.0), (500.0, 400.0));
        let dot = desk.draw(Tool::Ellipse, (200.0, 200.0), (240.0, 240.0));
        desk.tools.selection = vec![dot, frame];
        desk.stroke((220.0, 220.0), &[(230.0, 225.0)], Keys::default());
        assert_eq!(desk.bounds(dot), Rect::new(210.0, 205.0, 250.0, 245.0));
    }

    #[test]
    fn the_space_outside_a_corner_turns_the_selection_about_its_middle() {
        let (mut desk, id) = with_rectangle();
        // The box is 100..300 by 100..200, so its middle is (200, 150).
        assert_eq!(desk.tools.grab_at(desk.history.document(), (308.0, 208.0).into()), Some(Grab::Rotate));
        assert_eq!(desk.tools.grab_at(desk.history.document(), (302.0, 202.0).into()), Some(Grab::Resize((1.0, 1.0))));
        assert_eq!(desk.tools.grab_at(desk.history.document(), (292.0, 192.0).into()), None, "inside the box is the node itself");
        assert_eq!(desk.tools.grab_at(desk.history.document(), (330.0, 230.0).into()), None);
        // From straight below-right of the middle round to straight below it:
        // the pointer goes a quarter turn less the corner's own angle.
        let corner = (108.0f64 - 150.0 + 100.0).atan2(308.0 - 200.0);
        let turn = std::f64::consts::FRAC_PI_2 - corner;
        desk.stroke((308.0, 208.0), &[(250.0, 400.0), (200.0, 300.0)], Keys::default());
        let node = desk.history.document().node(id).unwrap().clone();
        let [a, b, ..] = node.transform.as_coeffs();
        assert!((b.atan2(a) - turn).abs() < 1e-9, "{}", b.atan2(a));
        assert!((node.transform * Point::new(100.0, 50.0) - Point::new(200.0, 150.0)).hypot() < 1e-9, "the middle stays");
        assert_eq!((node.size, desk.history.undo_name()), (Size::new(200.0, 100.0), Some("Rotate")));
        assert!(desk.history.undo());

        // With Shift, by fifteen degrees at a time; Esc puts it back.
        desk.tools.press(&desk.history, desk.page, (308.0, 208.0).into(), Keys::default());
        desk.tools.drag(&mut desk.history, (200.0, 300.0).into(), Keys { shift: true, ..Keys::default() }).unwrap();
        let [a, b, ..] = desk.history.document().node(id).unwrap().transform.as_coeffs();
        assert!((b.atan2(a).to_degrees() - 60.0).abs() < 1e-9, "{}", b.atan2(a).to_degrees());
        desk.tools.cancel(&mut desk.history);
        assert_eq!(desk.bounds(id), Rect::new(100.0, 100.0, 300.0, 200.0));
        assert_eq!(desk.history.undo_name(), Some("Draw"));
    }

    #[test]
    fn several_nodes_resize_together_by_the_box_round_them() {
        let mut desk = Desk::new();
        let a = desk.draw(Tool::Rectangle, (0.0, 0.0), (100.0, 100.0));
        let b = desk.draw(Tool::Ellipse, (200.0, 100.0), (300.0, 200.0));
        desk.tools.grab = 4.0;
        desk.tools.selection = vec![a, b];
        assert_eq!(desk.tools.frame(desk.history.document()), Some((Affine::IDENTITY, Rect::new(0.0, 0.0, 300.0, 200.0))));
        // The bottom-right corner out to twice the width and half as high again.
        desk.stroke((300.0, 200.0), &[(450.0, 250.0), (600.0, 300.0)], Keys::default());
        assert_eq!(desk.bounds(a), Rect::new(0.0, 0.0, 200.0, 150.0));
        assert_eq!(desk.bounds(b), Rect::new(400.0, 150.0, 600.0, 300.0));
        assert_eq!(desk.history.undo_name(), Some("Resize"));
        // The left edge in by half: everything half as wide, the right side staying.
        desk.stroke((0.0, 100.0), &[(300.0, 100.0)], Keys::default());
        assert_eq!(desk.bounds(a), Rect::new(300.0, 0.0, 400.0, 150.0));
        assert_eq!(desk.bounds(b), Rect::new(500.0, 150.0, 600.0, 300.0));
        // Each was one step.
        assert!(desk.history.undo() && desk.history.undo());
        assert_eq!((desk.bounds(a), desk.bounds(b)), (Rect::new(0.0, 0.0, 100.0, 100.0), Rect::new(200.0, 100.0, 300.0, 200.0)));
    }

    #[test]
    fn a_group_resizes_what_is_in_it_along_its_own_sides() {
        let mut desk = Desk::new();
        let a = desk.draw(Tool::Rectangle, (0.0, 0.0), (100.0, 100.0));
        let b = desk.draw(Tool::Rectangle, (200.0, 100.0), (300.0, 200.0));
        let group = desk.history.edit("Group", |document| document.group(&[a, b], NodeKind::Group)).unwrap();
        // A quarter turn about the page's origin: the box is now -200..0 by 0..300.
        desk.history.edit("Turn", |document| document.node_mut(group).map(|node| node.transform = Affine::rotate(std::f64::consts::FRAC_PI_2))).unwrap();
        desk.tools.grab = 4.0;
        desk.tools.selection = vec![group];
        let near = |a: Rect, b: Rect| (a.x0 - b.x0).abs() + (a.y0 - b.y0).abs() + (a.x1 - b.x1).abs() + (a.y1 - b.y1).abs() < 1e-9;
        assert!(near(desk.bounds(group), Rect::new(-200.0, 0.0, 0.0, 300.0)), "{:?}", desk.bounds(group));
        // The group's own right edge is the bottom one on the page: down by 300.
        desk.stroke((-100.0, 300.0), &[(-100.0, 600.0)], Keys::default());
        assert!(near(desk.bounds(group), Rect::new(-200.0, 0.0, 0.0, 600.0)), "{:?}", desk.bounds(group));
        // What is in it is twice as wide in the group's coordinates, and no higher.
        let document = desk.history.document();
        assert_eq!(document.node(a).unwrap().size, Size::new(200.0, 100.0));
        assert_eq!((document.node(b).unwrap().size, document.node(b).unwrap().transform), (Size::new(200.0, 100.0), Affine::translate((400.0, 100.0))));
    }

    #[test]
    fn the_other_tools_draw_figmas_shapes() {
        let mut desk = Desk::new();
        let polygon = desk.draw(Tool::Polygon, (0.0, 0.0), (100.0, 80.0));
        let star = desk.draw(Tool::Star, (200.0, 0.0), (300.0, 100.0));
        let kind = |desk: &Desk, id| desk.history.document().node(id).unwrap().kind.clone();
        assert_eq!((kind(&desk, polygon), desk.bounds(polygon)), (NodeKind::Polygon { sides: 3 }, Rect::new(0.0, 0.0, 100.0, 80.0)));
        assert_eq!((kind(&desk, star), desk.bounds(star)), (NodeKind::Star { points: 5, ratio: 0.382 }, Rect::new(200.0, 0.0, 300.0, 100.0)));
        assert_eq!(desk.tools.tool, Tool::Move);
    }

    /// Where a line starts and ends on the page.
    fn ends(desk: &Desk, id: NodeId) -> (Point, Point) {
        let document = desk.history.document();
        let (to_page, node) = (document.to_page(id).unwrap(), document.node(id).unwrap());
        assert_eq!(node.size.height, 0.0);
        (to_page * Point::ZERO, to_page * Point::new(node.size.width, 0.0))
    }

    fn near(a: (Point, Point), b: ((f64, f64), (f64, f64))) -> bool {
        (a.0 - Point::from(b.0)).hypot() < 1e-9 && (a.1 - Point::from(b.1)).hypot() < 1e-9
    }

    #[test]
    fn a_line_runs_from_where_the_drag_began_to_where_it_ended() {
        let mut desk = Desk::new();
        let line = desk.draw(Tool::Line, (100.0, 100.0), (130.0, 60.0));
        let node = desk.history.document().node(line).unwrap().clone();
        assert_eq!((node.kind.clone(), node.size), (NodeKind::Line, Size::new(50.0, 0.0)));
        assert!(near(ends(&desk, line), ((100.0, 100.0), (130.0, 60.0))), "{:?}", ends(&desk, line));
        // Black, one unit wide, with nothing on its ends; an arrow has a head.
        assert_eq!((node.stroke.paints.len(), node.stroke.weight, node.stroke.end_cap, node.fills.len()), (1, 1.0, Cap::None, 0));
        let arrow = desk.draw(Tool::Arrow, (0.0, 0.0), (0.0, 40.0));
        assert_eq!(desk.history.document().node(arrow).unwrap().stroke.end_cap, Cap::Arrow);
        assert!(near(ends(&desk, arrow), ((0.0, 0.0), (0.0, 40.0))));

        // Shift keeps it to an eighth of a turn; Alt draws it from its middle.
        desk.tools.tool = Tool::Line;
        desk.stroke((0.0, 0.0), &[(100.0, 8.0)], Keys { shift: true, ..Keys::default() });
        let flat = desk.tools.selection[0];
        assert!(near(ends(&desk, flat), ((0.0, 0.0), (100.0f64.hypot(8.0), 0.0))), "{:?}", ends(&desk, flat));
        desk.tools.tool = Tool::Line;
        desk.stroke((50.0, 50.0), &[(80.0, 50.0)], Keys { alt: true, ..Keys::default() });
        assert!(near(ends(&desk, desk.tools.selection[0]), ((20.0, 50.0), (80.0, 50.0))));
        // A click draws one of the usual length.
        desk.tools.tool = Tool::Line;
        desk.stroke((300.0, 300.0), &[], Keys::default());
        assert!(near(ends(&desk, desk.tools.selection[0]), ((300.0, 300.0), (400.0, 300.0))));
    }

    #[test]
    fn a_line_is_reshaped_by_its_ends_and_moved_by_the_rest_of_it() {
        let mut desk = Desk::new();
        let line = desk.draw(Tool::Line, (100.0, 100.0), (200.0, 100.0));
        desk.tools.grab = 4.0;
        let document = desk.history.document();
        assert_eq!(desk.tools.grab_at(document, (201.0, 101.0).into()), Some(Grab::Resize((1.0, 0.0))));
        assert_eq!(desk.tools.grab_at(document, (150.0, 100.0).into()), None, "along it is the line itself");
        // Its end, to somewhere else: the start stays.
        desk.stroke((200.0, 100.0), &[(180.0, 160.0)], Keys::default());
        assert!(near(ends(&desk, line), ((100.0, 100.0), (180.0, 160.0))), "{:?}", ends(&desk, line));
        assert_eq!(desk.history.undo_name(), Some("Resize"));
        // Its start: the end stays, and it still runs from start to end.
        desk.stroke((100.0, 100.0), &[(120.0, 40.0)], Keys::default());
        assert!(near(ends(&desk, line), ((120.0, 40.0), (180.0, 160.0))), "{:?}", ends(&desk, line));
        // With Shift, straight up from the end that stays.
        desk.stroke((120.0, 40.0), &[(184.0, 60.0)], Keys { shift: true, ..Keys::default() });
        let (start, end) = ends(&desk, line);
        assert!((start.x - 180.0).abs() < 1e-9 && start.y < 160.0 && (end - Point::new(180.0, 160.0)).hypot() < 1e-9, "{start:?} {end:?}");
        // By its middle it moves, both ends together.
        desk.history.edit("Flat", |document| document.node_mut(line).map(|node| (node.transform, node.size) = (Affine::translate((0.0, 0.0)), Size::new(100.0, 0.0)))).unwrap();
        desk.stroke((50.0, 1.0), &[(60.0, 21.0)], Keys::default());
        assert!(near(ends(&desk, line), ((10.0, 20.0), (110.0, 20.0))), "{:?}", ends(&desk, line));
    }

    #[test]
    fn what_is_dragged_lines_up_with_its_neighbours_or_keeps_to_whole_units() {
        let mut desk = Desk::new();
        let still = desk.draw(Tool::Rectangle, (100.0, 200.0), (180.0, 260.0));
        let moving = desk.draw(Tool::Rectangle, (300.0, 40.0), (350.0, 70.0));
        desk.tools.grab = 5.0;
        let none = Keys::default();
        // Dragged to within reach of the other's left edge: onto it, with a
        // line down both to show it. Up and down there is nothing near, so
        // it keeps to whole units.
        desk.tools.press(&desk.history, desk.page, (320.0, 50.0).into(), none);
        desk.tools.drag(&mut desk.history, (123.0, 60.4).into(), none).unwrap();
        assert_eq!(desk.bounds(moving), Rect::new(100.0, 50.0, 150.0, 80.0));
        assert_eq!(desk.tools.guides, [Line::new((100.0, 50.0), (100.0, 260.0))]);
        // Further on, out of reach: where the pointer says, to the unit.
        desk.tools.drag(&mut desk.history, (140.3, 60.0).into(), none).unwrap();
        assert_eq!((desk.bounds(moving), desk.tools.guides.len()), (Rect::new(120.0, 50.0, 170.0, 80.0), 0));
        // Its middle onto the other's, and its top onto the other's bottom.
        desk.tools.drag(&mut desk.history, (137.0, 272.0).into(), none).unwrap();
        assert_eq!(desk.bounds(moving), Rect::new(115.0, 260.0, 165.0, 290.0));
        assert_eq!(desk.tools.guides.len(), 2);
        // The guides go when the button does.
        desk.tools.release(&mut desk.history).unwrap();
        assert!(desk.tools.guides.is_empty());

        // An edge dragged near the other's right edge takes to it.
        desk.tools.selection = vec![moving];
        desk.stroke((165.0, 275.0), &[(177.6, 275.0)], none);
        assert_eq!(desk.bounds(moving), Rect::new(115.0, 260.0, 180.0, 290.0));
        // A new shape starts and ends on whole units, or on a neighbour.
        desk.tools.tool = Tool::Ellipse;
        desk.stroke((10.4, 10.6), &[(98.0, 55.5)], none);
        assert_eq!(desk.bounds(desk.tools.selection[0]), Rect::new(10.0, 11.0, 100.0, 56.0));

        // With snapping off, everything goes exactly where the pointer does.
        desk.tools.snap = false;
        desk.tools.selection = vec![moving];
        desk.stroke((140.0, 275.0), &[(142.5, 275.25)], none);
        assert_eq!((desk.bounds(moving), desk.bounds(still)), (Rect::new(117.5, 260.25, 182.5, 290.25), Rect::new(100.0, 200.0, 180.0, 260.0)));
    }

    #[test]
    fn a_move_kept_to_one_axis_lines_up_along_that_axis_only() {
        let mut desk = Desk::new();
        desk.draw(Tool::Rectangle, (100.0, 200.0), (180.0, 260.0));
        let moving = desk.draw(Tool::Rectangle, (300.0, 197.0), (350.0, 230.0));
        desk.tools.grab = 5.0;
        // Sideways with Shift: its top is 3 from the other's, and stays so.
        desk.tools.press(&desk.history, desk.page, (320.0, 210.0).into(), Keys::default());
        desk.tools.drag(&mut desk.history, (201.0, 212.0).into(), Keys { shift: true, ..Keys::default() }).unwrap();
        assert_eq!(desk.bounds(moving), Rect::new(180.0, 197.0, 230.0, 230.0));
        assert_eq!(desk.tools.guides, [Line::new((180.0, 197.0), (180.0, 260.0))]);
    }
}
