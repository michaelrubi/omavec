//! The node tree and its undo history.

use std::sync::Arc;

use omavec_engine::{Document, Error, History, Node, NodeId, NodeKind};
use omavec_geom::kurbo::Size;
use proptest::prelude::*;

fn page(document: &Document) -> NodeId {
    document.pages[0].id
}

/// Adds a node of `kind` on top of `parent`'s children and returns its id.
fn add(document: &mut Document, parent: NodeId, kind: NodeKind) -> NodeId {
    let node = document.create(kind, Size::new(100.0, 50.0));
    let id = node.id;
    document.insert(parent, usize::MAX, node).unwrap();
    id
}

fn frame() -> NodeKind {
    NodeKind::Frame { clip: true }
}

fn children(document: &Document, parent: NodeId) -> Vec<NodeId> {
    document.node(parent).unwrap().children.iter().map(|node| node.id).collect()
}

/// A page holding frame A (rectangles 1 and 2) and frame B (a group holding
/// an ellipse), and an empty second page.
struct Sample {
    document: Document,
    page: NodeId,
    a: NodeId,
    one: NodeId,
    two: NodeId,
    b: NodeId,
    group: NodeId,
    ellipse: NodeId,
}

fn sample() -> Sample {
    let mut document = Document::default();
    let page = page(&document);
    let a = add(&mut document, page, frame());
    let one = add(&mut document, a, NodeKind::Rectangle);
    let two = add(&mut document, a, NodeKind::Rectangle);
    let b = add(&mut document, page, frame());
    let group = add(&mut document, b, NodeKind::Group);
    let ellipse = add(&mut document, group, NodeKind::Ellipse);
    document.add_page("Page 2");
    Sample { document, page, a, one, two, b, group, ellipse }
}

#[test]
fn a_new_document_has_one_empty_page() {
    let document = Document::default();
    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].name, "Page 1");
    assert_eq!(document.pages[0].kind, NodeKind::Page);
    assert!(document.pages[0].children.is_empty());
}

#[test]
fn nodes_are_found_by_id_with_their_parents() {
    let s = sample();
    assert_eq!(s.document.node(s.ellipse).unwrap().kind, NodeKind::Ellipse);
    assert_eq!(s.document.node(s.ellipse).unwrap().name, "Ellipse");
    assert_eq!(s.document.parent(s.ellipse), Some(s.group));
    assert_eq!(s.document.parent(s.group), Some(s.b));
    assert_eq!(s.document.parent(s.a), Some(s.page));
    assert_eq!(s.document.parent(s.page), None);
    assert_eq!(s.document.node(NodeId(999)), None);
    assert_eq!(s.document.parent(NodeId(999)), None);
    // Back to front, in the order they were added.
    assert_eq!(children(&s.document, s.page), [s.a, s.b]);
    assert_eq!(children(&s.document, s.a), [s.one, s.two]);
}

#[test]
fn ids_are_never_handed_out_twice() {
    let mut s = sample();
    s.document.remove(s.two).unwrap();
    let new = add(&mut s.document, s.a, NodeKind::Rectangle);
    assert!(new.0 > s.ellipse.0 && new != s.two);
}

#[test]
fn insert_takes_an_index_from_the_back() {
    let mut s = sample();
    let bottom = s.document.create(NodeKind::Ellipse, Size::ZERO);
    let middle = s.document.create(NodeKind::Ellipse, Size::ZERO);
    let (bottom_id, middle_id) = (bottom.id, middle.id);
    s.document.insert(s.a, 0, bottom).unwrap();
    s.document.insert(s.a, 2, middle).unwrap();
    assert_eq!(children(&s.document, s.a), [bottom_id, s.one, middle_id, s.two]);
}

#[test]
fn removing_a_node_takes_what_is_inside_it() {
    let mut s = sample();
    let removed = s.document.remove(s.b).unwrap();
    assert_eq!(removed.id, s.b);
    assert_eq!(removed.children[0].children[0].id, s.ellipse);
    assert_eq!(s.document.node(s.ellipse), None);
    assert_eq!(children(&s.document, s.page), [s.a]);
}

#[test]
fn a_node_moves_within_its_parent_and_to_another() {
    let mut s = sample();
    // To the back of its own parent.
    s.document.move_to(s.two, s.a, 0).unwrap();
    assert_eq!(children(&s.document, s.a), [s.two, s.one]);
    // Into the group in the other frame, on top.
    s.document.move_to(s.two, s.group, usize::MAX).unwrap();
    assert_eq!(children(&s.document, s.a), [s.one]);
    assert_eq!(children(&s.document, s.group), [s.ellipse, s.two]);
    assert_eq!(s.document.parent(s.two), Some(s.group));
    // A whole frame into the other frame.
    s.document.move_to(s.b, s.a, 0).unwrap();
    assert_eq!(children(&s.document, s.page), [s.a]);
    assert_eq!(s.document.parent(s.ellipse), Some(s.group));
}

#[test]
fn what_cannot_be_done_is_an_error_and_changes_nothing() {
    let mut s = sample();
    let before = s.document.clone();
    let missing = NodeId(999);
    let spare = s.document.create(NodeKind::Rectangle, Size::ZERO);
    let before_with_spare = s.document.clone();
    assert_eq!(s.document.insert(missing, 0, spare.clone()), Err(Error::NoSuchNode(missing)));
    assert_eq!(s.document.insert(s.one, 0, spare), Err(Error::NotAContainer(s.one)));
    assert_eq!(s.document.remove(missing).unwrap_err(), Error::NoSuchNode(missing));
    assert_eq!(s.document.node_mut(missing).unwrap_err(), Error::NoSuchNode(missing));
    assert_eq!(s.document.move_to(missing, s.a, 0), Err(Error::NoSuchNode(missing)));
    assert_eq!(s.document.move_to(s.one, missing, 0), Err(Error::NoSuchNode(missing)));
    assert_eq!(s.document.move_to(s.one, s.two, 0), Err(Error::NotAContainer(s.two)));
    // Not into itself, nor into something inside itself.
    assert_eq!(s.document.move_to(s.b, s.b, 0), Err(Error::IntoItself(s.b)));
    assert_eq!(s.document.move_to(s.b, s.group, 0), Err(Error::IntoItself(s.b)));
    // Pages stay at the top.
    assert_eq!(s.document.move_to(s.page, s.a, 0), Err(Error::PageInsideNode));
    let page = Node { kind: NodeKind::Page, ..s.document.node(s.one).unwrap().clone() };
    assert_eq!(s.document.insert(s.a, 0, page), Err(Error::PageInsideNode));
    assert_eq!(s.document, before_with_spare);
    assert_eq!(s.document.pages, before.pages);
}

#[test]
fn an_edit_copies_only_the_path_to_the_node() {
    let mut s = sample();
    let snapshot = s.document.clone();
    s.document.node_mut(s.ellipse).unwrap().name = "Dot".into();

    // The snapshot still has the old name.
    assert_eq!(snapshot.node(s.ellipse).unwrap().name, "Ellipse");
    assert_eq!(s.document.node(s.ellipse).unwrap().name, "Dot");
    let shared = |id: NodeId| std::ptr::eq(snapshot.node(id).unwrap(), s.document.node(id).unwrap());
    // Copied: the page, frame B, the group and the ellipse.
    assert!(!shared(s.page) && !shared(s.b) && !shared(s.group) && !shared(s.ellipse));
    // Still shared: frame A with everything in it, and the other page.
    assert!(shared(s.a) && shared(s.one) && shared(s.two));
    assert!(Arc::ptr_eq(&snapshot.pages[1], &s.document.pages[1]));
}

fn rename(history: &mut History, id: NodeId, name: &str) {
    history
        .edit("Rename", |document| {
            document.node_mut(id)?.name = name.into();
            Ok(())
        })
        .unwrap();
}

fn widen(history: &mut History, id: NodeId, width: f64) {
    history
        .edit("Move", |document| {
            document.node_mut(id)?.size.width = width;
            Ok(())
        })
        .unwrap();
}

fn name(history: &History, id: NodeId) -> &str {
    &history.document().node(id).unwrap().name
}

#[test]
fn edits_undo_and_redo_one_step_at_a_time() {
    let s = sample();
    let mut history = History::new(s.document);
    assert_eq!((history.undo_name(), history.redo_name()), (None, None));
    assert!(!history.undo() && !history.redo());

    rename(&mut history, s.one, "First");
    history.edit("Delete", |document| document.remove(s.two)).unwrap();
    assert_eq!(history.undo_name(), Some("Delete"));

    assert!(history.undo());
    assert!(history.document().node(s.two).is_some());
    assert_eq!(name(&history, s.one), "First");
    assert_eq!((history.undo_name(), history.redo_name()), (Some("Rename"), Some("Delete")));

    assert!(history.undo());
    assert_eq!(name(&history, s.one), "Rectangle");
    assert!(!history.undo());

    assert!(history.redo() && history.redo());
    assert!(history.document().node(s.two).is_none());
    assert_eq!(name(&history, s.one), "First");
    assert!(!history.redo());

    // A new edit after an undo drops what could have been redone.
    history.undo();
    rename(&mut history, s.one, "Other");
    assert_eq!(history.redo_name(), None);
    assert!(history.document().node(s.two).is_some());
}

#[test]
fn a_failed_edit_leaves_no_trace() {
    let s = sample();
    let mut history = History::new(s.document.clone());
    let revision = history.revision();
    let result = history.edit("Broken", |document| {
        document.node_mut(s.one)?.name = "Half done".into();
        document.remove(NodeId(999))
    });
    assert_eq!(result.unwrap_err(), Error::NoSuchNode(NodeId(999)));
    assert_eq!(history.document(), &s.document);
    assert_eq!(history.revision(), revision);
    assert_eq!(history.undo_name(), None);
    assert!(!history.is_dirty());
    // Nor does an edit that changes nothing.
    history.edit("Look", |document| Ok(document.node(s.one).is_some())).unwrap();
    assert_eq!(history.undo_name(), None);
}

#[test]
fn a_gesture_is_one_step_however_many_edits_it_takes() {
    let s = sample();
    let mut history = History::new(s.document);
    rename(&mut history, s.one, "Before");

    history.begin("Move");
    let mut seen = vec![history.revision()];
    for x in 1..=20 {
        widen(&mut history, s.one, f64::from(x));
        // Each change is a new state to draw, though not a new step.
        assert!(!seen.contains(&history.revision()));
        seen.push(history.revision());
    }
    history.commit();
    assert_eq!(history.document().node(s.one).unwrap().size.width, 20.0);
    assert_eq!(history.undo_name(), Some("Move"));

    assert!(history.undo());
    assert_eq!(history.document().node(s.one).unwrap().size.width, 100.0);
    assert_eq!(history.undo_name(), Some("Rename"));
    assert!(history.redo());
    assert_eq!(history.document().node(s.one).unwrap().size.width, 20.0);
}

#[test]
fn a_gesture_can_be_cancelled_or_come_to_nothing() {
    let s = sample();
    let mut history = History::new(s.document.clone());
    // Esc during a drag: back to where it started, nothing to undo.
    history.begin("Move");
    widen(&mut history, s.one, 7.0);
    history.cancel();
    assert_eq!(history.document(), &s.document);
    assert_eq!((history.undo_name(), history.is_dirty()), (None, false));
    // A click that never became a drag.
    history.begin("Move");
    history.commit();
    assert_eq!(history.undo_name(), None);
    // Undo in the middle of a drag undoes the drag so far.
    history.begin("Move");
    widen(&mut history, s.one, 7.0);
    assert!(history.undo());
    assert_eq!(history.document(), &s.document);
    assert_eq!(history.redo_name(), Some("Move"));
}

#[test]
fn dirty_means_different_from_what_was_saved() {
    let s = sample();
    let mut history = History::new(s.document);
    assert!(!history.is_dirty(), "a new document has nothing to lose");
    rename(&mut history, s.one, "A");
    assert!(history.is_dirty());
    history.mark_saved();
    assert!(!history.is_dirty());
    rename(&mut history, s.one, "B");
    assert!(history.is_dirty());
    // Back to the saved state by undo: clean. Past it: dirty again.
    history.undo();
    assert!(!history.is_dirty());
    history.undo();
    assert!(history.is_dirty());
    history.redo();
    assert!(!history.is_dirty());
}

/// One step of a random session. Numbers pick among the nodes there are.
#[derive(Clone, Debug)]
enum Action {
    Add(usize, u8),
    Remove(usize),
    Move(usize, usize, usize),
    Rename(usize, String),
    Undo,
    Redo,
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        3 => (any::<usize>(), 0..4u8).prop_map(|(parent, kind)| Action::Add(parent, kind)),
        1 => any::<usize>().prop_map(Action::Remove),
        2 => (any::<usize>(), any::<usize>(), 0..4usize).prop_map(|(node, parent, index)| Action::Move(node, parent, index)),
        1 => (any::<usize>(), "[a-z]{1,4}").prop_map(|(node, name)| Action::Rename(node, name)),
        2 => Just(Action::Undo),
        1 => Just(Action::Redo),
    ]
}

/// Every node's id with its parent's, walking the tree.
fn walk(nodes: &[Arc<Node>], parent: Option<NodeId>, out: &mut Vec<(NodeId, Option<NodeId>)>) {
    for node in nodes {
        out.push((node.id, parent));
        walk(&node.children, Some(node.id), out);
    }
}

proptest! {
    /// Whatever a session does, the history behaves like a plain list of
    /// whole documents, and the tree stays a tree: unique ids, every node
    /// found by its id under the parent it is really in.
    #[test]
    fn a_session_matches_a_list_of_whole_documents(actions in prop::collection::vec(action(), 0..60)) {
        let mut history = History::new(Document::default());
        // The model: every state so far, and where in it we are.
        let mut states = vec![history.document().clone()];
        let mut at = 0;
        for action in actions {
            let mut all = Vec::new();
            walk(&history.document().pages, None, &mut all);
            let pick = |n: usize| all[n % all.len()].0;
            let before = history.revision();
            match action.clone() {
                Action::Undo => prop_assert_eq!(history.undo(), at > 0),
                Action::Redo => prop_assert_eq!(history.redo(), at + 1 < states.len()),
                Action::Add(parent, kind) => {
                    let kind = [frame(), NodeKind::Group, NodeKind::Rectangle, NodeKind::Ellipse][usize::from(kind)].clone();
                    let _ = history.edit("Add", |document| {
                        let node = document.create(kind, Size::ZERO);
                        document.insert(pick(parent), 0, node)
                    });
                }
                Action::Remove(node) => {
                    // Not the only page.
                    if all.len() > 1 && pick(node) != all[0].0 {
                        history.edit("Remove", |document| document.remove(pick(node))).unwrap();
                    }
                }
                Action::Move(node, parent, index) => {
                    let _ = history.edit("Move", |document| document.move_to(pick(node), pick(parent), index));
                }
                Action::Rename(node, name) => {
                    rename(&mut history, pick(node), &name);
                }
            }
            match action {
                Action::Undo => at = at.saturating_sub(1),
                Action::Redo => at = (at + 1).min(states.len() - 1),
                // An edit that did something drops the redo states and adds one.
                _ if history.revision() != before => {
                    states.truncate(at + 1);
                    states.push(history.document().clone());
                    at += 1;
                }
                _ => {}
            }
            prop_assert_eq!(history.document(), &states[at]);
            prop_assert_eq!(history.undo_name().is_some(), at > 0);
            prop_assert_eq!(history.redo_name().is_some(), at + 1 < states.len());

            let mut all = Vec::new();
            walk(&history.document().pages, None, &mut all);
            let mut ids: Vec<NodeId> = all.iter().map(|(id, _)| *id).collect();
            ids.sort();
            ids.dedup();
            prop_assert_eq!(ids.len(), all.len(), "an id appears twice");
            for (id, parent) in all {
                prop_assert_eq!(history.document().node(id).map(|node| node.id), Some(id));
                prop_assert_eq!(history.document().parent(id), parent);
            }
        }
    }
}

/// A page with, back to front: a clipping frame at (100, 100) holding a
/// rectangle that sticks out of its right edge, then a group holding an
/// ellipse, then a rectangle turned 45°.
struct Scene {
    document: Document,
    frame: NodeId,
    inside: NodeId,
    group: NodeId,
    ellipse: NodeId,
    turned: NodeId,
}

fn scene() -> Scene {
    use omavec_geom::kurbo::Affine;
    let mut document = Document::default();
    let page = page(&document);
    let place = |document: &mut Document, parent: NodeId, kind: NodeKind, transform: Affine, size: (f64, f64)| {
        let id = add(document, parent, kind);
        let node = document.node_mut(id).unwrap();
        (node.transform, node.size) = (transform, Size::new(size.0, size.1));
        id
    };
    let frame = place(&mut document, page, frame(), Affine::translate((100.0, 100.0)), (200.0, 200.0));
    let inside = place(&mut document, frame, NodeKind::Rectangle, Affine::translate((150.0, 50.0)), (100.0, 50.0));
    let group = place(&mut document, page, NodeKind::Group, Affine::translate((400.0, 100.0)), (0.0, 0.0));
    let ellipse = place(&mut document, group, NodeKind::Ellipse, Affine::translate((10.0, 10.0)), (100.0, 60.0));
    let turned = place(&mut document, page, NodeKind::Rectangle, Affine::translate((600.0, 100.0)) * Affine::rotate(std::f64::consts::FRAC_PI_4), (100.0, 100.0));
    Scene { document, frame, inside, group, ellipse, turned }
}

fn hit(scene: &Scene, x: f64, y: f64) -> Vec<NodeId> {
    scene.document.pages[0].hit((x, y).into())
}

#[test]
fn a_point_hits_the_front_most_node_and_names_what_holds_it() {
    let s = scene();
    assert_eq!(hit(&s, 50.0, 50.0), []);
    assert_eq!(hit(&s, 110.0, 110.0), [s.frame]);
    // The rectangle in the frame is at 250..350 across, 150..200 down.
    assert_eq!(hit(&s, 260.0, 160.0), [s.frame, s.inside]);
    // Where it sticks out of the clipping frame there is nothing to hit...
    assert_eq!(hit(&s, 320.0, 160.0), []);
    // ...unless the frame doesn't clip.
    let mut open = scene();
    open.document.node_mut(open.frame).unwrap().kind = NodeKind::Frame { clip: false };
    assert_eq!(hit(&open, 320.0, 160.0), [open.frame, open.inside]);
}

#[test]
fn shapes_are_hit_on_their_shape_not_their_box() {
    let s = scene();
    // The ellipse's box is 410..510 across, 110..170 down. A group is hit
    // only through what's in it.
    assert_eq!(hit(&s, 460.0, 140.0), [s.group, s.ellipse]);
    assert_eq!(hit(&s, 412.0, 112.0), []);
    assert_eq!(hit(&s, 405.0, 105.0), []);
    // Turned 45° about (600, 100), the square is a diamond from 100 to 241 down.
    assert_eq!(hit(&s, 600.0, 170.0), [s.turned]);
    assert_eq!(hit(&s, 600.0, 235.0), [s.turned]);
    assert_eq!(hit(&s, 650.0, 110.0), []);
    assert_eq!(hit(&s, 560.0, 110.0), []);
}

#[test]
fn hidden_and_locked_nodes_let_the_point_through() {
    let mut s = scene();
    // A rectangle over the frame's rectangle.
    let page = page(&s.document);
    let cover = add(&mut s.document, page, NodeKind::Rectangle);
    let node = s.document.node_mut(cover).unwrap();
    (node.transform, node.size) = (omavec_geom::kurbo::Affine::translate((240.0, 140.0)), Size::new(50.0, 50.0));
    assert_eq!(hit(&s, 260.0, 160.0), [cover]);
    s.document.node_mut(cover).unwrap().locked = true;
    assert_eq!(hit(&s, 260.0, 160.0), [s.frame, s.inside]);
    s.document.node_mut(cover).unwrap().locked = false;
    s.document.node_mut(cover).unwrap().visible = false;
    assert_eq!(hit(&s, 260.0, 160.0), [s.frame, s.inside]);
    // A node squashed to nothing can't be hit, and nothing panics.
    s.document.node_mut(s.turned).unwrap().transform = omavec_geom::kurbo::Affine::scale(0.0);
    assert_eq!(hit(&s, 0.0, 0.0), []);
}

#[test]
fn a_node_knows_where_it_is_on_the_page() {
    let s = scene();
    let to_page = s.document.to_page(s.inside).unwrap();
    assert_eq!(to_page * omavec_geom::kurbo::Point::new(0.0, 0.0), (250.0, 150.0).into());
    assert_eq!(s.document.to_page(page(&s.document)), Some(omavec_geom::kurbo::Affine::IDENTITY));
    assert_eq!(s.document.to_page(NodeId(999)), None);
}
