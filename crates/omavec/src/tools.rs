//! The tools: pointer events in page coordinates in, document edits out. A
//! tool never sees egui, so each is tested by calling it, and every drag is
//! one undo step.

use omavec_engine::{Document, Error, History, NodeId, NodeKind};
use omavec_geom::kurbo::{Affine, Line, ParamCurveNearest, Point, Rect, Size, Vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Move,
    Frame,
    Rectangle,
    Ellipse,
}

impl Tool {
    /// What the tool draws, if it draws.
    fn draws(self) -> Option<NodeKind> {
        match self {
            Tool::Move => None,
            Tool::Frame => Some(NodeKind::Frame { clip: true }),
            Tool::Rectangle => Some(NodeKind::Rectangle),
            Tool::Ellipse => Some(NodeKind::Ellipse),
        }
    }
}

/// The modifier keys the tools read.
#[derive(Clone, Copy, Debug, Default)]
pub struct Keys {
    pub shift: bool,
    pub alt: bool,
}

/// What a click without a drag draws, as in Figma.
const CLICK_SIZE: f64 = 100.0;

enum Drag {
    /// Drawing a new node from `start`, in the coordinates of `parent`.
    Draw { kind: NodeKind, parent: NodeId, start: Point, to_parent: Affine, node: Option<NodeId> },
    /// Moving the selection: each node with the transform it started with
    /// and the way from the page's coordinates to its parent's.
    Move { start: Point, nodes: Vec<(NodeId, Affine, Affine)>, moved: bool },
    /// Resizing `node` by one of its handles: the transform and size it
    /// started with, and the way from the page to its own coordinates then.
    /// `offset` is from where the pointer took the handle to the handle
    /// itself, so the box doesn't jump to the pointer.
    Resize { node: NodeId, handle: Handle, began: Affine, size: Size, to_local: Affine, offset: Vec2, moved: bool },
}

/// Where a box is resized from, as fractions of its width and height: 0 or
/// 1 for a side that moves, 0.5 for a direction that stays as it is. The
/// four corners and the four edges.
type Handle = (f64, f64);

#[derive(Default)]
pub struct Tools {
    pub tool: Tool,
    pub selection: Vec<NodeId>,
    /// How near the pointer must be to a handle to take it, in page units:
    /// a few pixels at the canvas's zoom.
    pub grab: f64,
    drag: Option<Drag>,
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

impl Tools {
    /// The handle of `id`'s box within reach of `at`, corners before edges.
    fn handle_at(&self, document: &Document, id: NodeId, at: Point) -> Option<Handle> {
        let node = document.node(id).filter(|node| !node.locked)?;
        let to_page = document.to_page(id)?;
        let on_page = |handle: Handle| to_page * Point::new(handle.0 * node.size.width, handle.1 * node.size.height);
        let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let corner = corners.into_iter().find(|corner| (on_page(*corner) - at).hypot() <= self.grab);
        // An edge is taken anywhere along it, as in Figma.
        let edge = (0..4).map(|i| (corners[i], corners[(i + 1) % 4])).find_map(|(a, b)| {
            let near = Line::new(on_page(a), on_page(b)).nearest(at, 1e-9).distance_sq <= self.grab * self.grab;
            near.then_some(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0))
        });
        corner.or(edge)
    }

    /// The node a click at `at` selects: the front-most child of the page
    /// there, or, where that is a top-level frame, what is in the frame.
    fn pick(document: &Document, page: NodeId, at: Point) -> Option<NodeId> {
        let chain = document.node(page)?.hit(at);
        let top = document.node(*chain.first()?)?;
        match (&top.kind, chain.get(1)) {
            (NodeKind::Frame { .. }, Some(inside)) => Some(*inside),
            _ => Some(top.id),
        }
    }

    /// The pointer went down at `at` on `page`.
    pub fn press(&mut self, history: &History, page: NodeId, at: Point, keys: Keys) {
        let document = history.document();
        self.drag = match self.tool.draws() {
            Some(kind) => {
                // A new node goes into the deepest frame under the pointer.
                let chain = document.node(page).map(|page| page.hit(at)).unwrap_or_default();
                let frame = chain.iter().rev().copied().find(|id| document.node(*id).is_some_and(|node| matches!(node.kind, NodeKind::Frame { .. })));
                let parent = frame.unwrap_or(page);
                let to_parent = document.to_page(parent).unwrap_or_default().inverse();
                Some(Drag::Draw { kind, parent, start: to_parent * at, to_parent, node: None })
            }
            None => {
                // A handle of the one selected node comes before whatever is under it.
                if let [node] = self.selection[..]
                    && let Some(handle) = self.handle_at(document, node, at)
                    && let (Some(began), Some(to_page)) = (document.node(node), document.to_page(node))
                {
                    let to_local = to_page.inverse();
                    let offset = Point::new(handle.0 * began.size.width, handle.1 * began.size.height) - to_local * at;
                    self.drag = Some(Drag::Resize { node, handle, began: began.transform, size: began.size, to_local, offset, moved: false });
                    return;
                }
                let picked = Self::pick(document, page, at);
                match picked {
                    // Shift adds to the selection or takes away from it.
                    Some(node) if keys.shift => match self.selection.iter().position(|selected| *selected == node) {
                        Some(index) => drop(self.selection.remove(index)),
                        None => self.selection.push(node),
                    },
                    Some(node) if !self.selection.contains(&node) => self.selection = vec![node],
                    Some(_) => {}
                    None if !keys.shift => self.selection.clear(),
                    None => {}
                }
                picked.map(|_| {
                    let start_of = |id: &NodeId| {
                        let parent = document.parent(*id).and_then(|parent| document.to_page(parent)).unwrap_or_default();
                        Some((*id, document.node(*id)?.transform, parent.inverse()))
                    };
                    Drag::Move { start: at, nodes: self.selection.iter().filter_map(start_of).collect(), moved: false }
                })
            }
        };
    }

    /// The pointer moved to `at` with the button held.
    pub fn drag(&mut self, history: &mut History, at: Point, keys: Keys) -> Result<(), Error> {
        match &mut self.drag {
            Some(Drag::Draw { kind, parent, start, to_parent, node }) => {
                let area = drawn(*start, *to_parent * at, keys);
                let (kind, parent) = (kind.clone(), *parent);
                if node.is_none() {
                    history.begin("Draw");
                }
                let drawing = history.edit("Draw", |document| {
                    let id = match *node {
                        Some(id) => id,
                        None => {
                            let new = document.create(kind, Size::ZERO);
                            let id = new.id;
                            document.insert(parent, usize::MAX, new)?;
                            id
                        }
                    };
                    let shape = document.node_mut(id)?;
                    (shape.transform, shape.size) = (Affine::translate(area.origin().to_vec2()), area.size());
                    Ok(id)
                })?;
                *node = Some(drawing);
                self.selection = vec![drawing];
            }
            Some(Drag::Move { start, nodes, moved }) => {
                if !*moved {
                    history.begin("Move");
                    *moved = true;
                }
                let by = at - *start;
                history.edit("Move", |document| {
                    for (id, began, to_parent) in nodes.iter() {
                        // The drag is in page coordinates; a node moves in its parent's.
                        document.node_mut(*id)?.transform = Affine::translate(in_parent(*to_parent, by)) * *began;
                    }
                    Ok(())
                })?;
            }
            Some(Drag::Resize { node, handle, began, size, to_local, offset, moved }) => {
                if !*moved {
                    history.begin("Resize");
                    *moved = true;
                }
                let area = resized(*size, *handle, *to_local * at + *offset, keys);
                history.edit("Resize", |document| {
                    let shape = document.node_mut(*node)?;
                    (shape.transform, shape.size) = (*began * Affine::translate(area.origin().to_vec2()), area.size());
                    Ok(())
                })?;
            }
            None => {}
        }
        Ok(())
    }

    /// The button came up.
    pub fn release(&mut self, history: &mut History) -> Result<(), Error> {
        match self.drag.take() {
            // A click with a drawing tool: a shape of the usual size, there.
            Some(Drag::Draw { kind, parent, start, node: None, .. }) => {
                let id = history.edit("Draw", |document| {
                    let mut new = document.create(kind, Size::new(CLICK_SIZE, CLICK_SIZE));
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
            Some(Drag::Move { .. } | Drag::Resize { .. }) => history.commit(),
            None => {}
        }
        Ok(())
    }

    /// Esc: gives up the drag in progress, or else the drawing tool.
    pub fn cancel(&mut self, history: &mut History) {
        match self.drag.take() {
            Some(Drag::Draw { node, .. }) => {
                history.cancel();
                self.selection.retain(|selected| Some(*selected) != node);
            }
            Some(Drag::Move { .. } | Drag::Resize { .. }) => history.cancel(),
            None => self.tool = Tool::Move,
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
            document.to_page(id).unwrap().transform_rect_bbox(Rect::from_origin_size((0.0, 0.0), document.node(id).unwrap().size))
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
        let (shift, alt) = (Keys { shift: true, alt: false }, Keys { shift: false, alt: true });
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
        desk.stroke((100.0, 100.0), &[(130.0, 110.0)], Keys { shift: true, alt: true });
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
    }

    #[test]
    fn a_click_selects_and_shift_click_adds_or_takes_away() {
        let mut desk = Desk::new();
        let a = desk.draw(Tool::Rectangle, (0.0, 0.0), (100.0, 100.0));
        let b = desk.draw(Tool::Rectangle, (200.0, 0.0), (300.0, 100.0));
        let shift = Keys { shift: true, alt: false };
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
        desk.stroke((340.0, 240.0), &[], Keys::default());
        // The frame's child, not the child's child.
        assert_eq!(desk.tools.selection, [inner]);
        desk.stroke((120.0, 120.0), &[], Keys::default());
        assert_eq!(desk.tools.selection, [frame]);
        assert_eq!(desk.children(inner), [dot]);
    }

    #[test]
    fn dragging_moves_the_selection_as_one_step() {
        let mut desk = Desk::new();
        let a = desk.draw(Tool::Rectangle, (0.0, 0.0), (100.0, 100.0));
        let b = desk.draw(Tool::Rectangle, (200.0, 0.0), (300.0, 100.0));
        desk.stroke((50.0, 50.0), &[], Keys::default());
        desk.stroke((250.0, 50.0), &[], Keys { shift: true, alt: false });
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
        let (shift, alt) = (Keys { shift: true, alt: false }, Keys { shift: false, alt: true });
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
}
