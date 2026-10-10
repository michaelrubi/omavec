//! Grouping, duplicating, restacking, copying and pasting.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use omavec_engine::{Document, Error, Node, NodeId, NodeKind, Stack};
use omavec_geom::kurbo::{Affine, Rect, Size};
use proptest::prelude::*;

/// Adds a `width` × `height` node of `kind` on top of `parent`, placed by
/// `transform`.
fn add(document: &mut Document, parent: NodeId, kind: NodeKind, transform: Affine, (width, height): (f64, f64)) -> NodeId {
    let mut node = document.create(kind, Size::new(width, height));
    node.transform = transform;
    let id = node.id;
    document.insert(parent, usize::MAX, node).unwrap();
    id
}

fn children(document: &Document, parent: NodeId) -> Vec<NodeId> {
    document.node(parent).unwrap().children.iter().map(|node| node.id).collect()
}

/// Where every node that isn't a container sits on its page.
fn places(document: &Document) -> BTreeMap<NodeId, [f64; 6]> {
    fn walk(document: &Document, nodes: &[Arc<Node>], out: &mut BTreeMap<NodeId, [f64; 6]>) {
        for node in nodes {
            if !node.kind.is_container() {
                out.insert(node.id, document.to_page(node.id).unwrap().as_coeffs());
            }
            walk(document, &node.children, out);
        }
    }
    let mut out = BTreeMap::new();
    walk(document, &document.pages, &mut out);
    out
}

fn assert_same_places(before: &BTreeMap<NodeId, [f64; 6]>, after: &BTreeMap<NodeId, [f64; 6]>) {
    for (id, was) in before {
        let Some(is) = after.get(id) else { continue };
        for (was, is) in was.iter().zip(is) {
            assert!((was - is).abs() < 1e-6, "{id:?} moved: {was} to {is}");
        }
    }
}

/// A page with a rotated frame holding two rectangles, and an ellipse
/// beside it.
struct Scene {
    document: Document,
    page: NodeId,
    frame: NodeId,
    one: NodeId,
    two: NodeId,
    ellipse: NodeId,
}

fn scene() -> Scene {
    let mut document = Document::default();
    let page = document.pages[0].id;
    let frame = add(&mut document, page, NodeKind::Frame { clip: true }, Affine::translate((100.0, 100.0)) * Affine::rotate(0.5), (300.0, 200.0));
    let one = add(&mut document, frame, NodeKind::Rectangle, Affine::translate((10.0, 20.0)), (50.0, 40.0));
    let two = add(&mut document, frame, NodeKind::Rectangle, Affine::translate((120.0, 90.0)), (80.0, 30.0));
    let ellipse = add(&mut document, page, NodeKind::Ellipse, Affine::translate((500.0, 50.0)), (60.0, 60.0));
    Scene { document, page, frame, one, two, ellipse }
}

#[test]
fn a_group_takes_the_place_of_its_front_most_member_and_fits_them() {
    let Scene { mut document, frame, one, two, .. } = scene();
    let between = add(&mut document, frame, NodeKind::Ellipse, Affine::IDENTITY, (5.0, 5.0));
    document.move_to(between, frame, 1).unwrap();
    let before = places(&document);
    // Named front first: the document's order is kept all the same.
    let group = document.group(&[two, one], NodeKind::Group).unwrap();
    assert_eq!(children(&document, frame), [between, group]);
    assert_eq!(children(&document, group), [one, two]);
    assert_same_places(&before, &places(&document));
    // It sits at its members' top-left corner and is as big as they are.
    let node = document.node(group).unwrap();
    assert_eq!(node.transform, Affine::translate((10.0, 20.0)));
    assert_eq!(node.bounds(), Rect::new(0.0, 0.0, 190.0, 100.0));
    assert!(node.fills.is_empty());
}

#[test]
fn nodes_from_different_parents_group_where_the_front_most_was() {
    let Scene { mut document, page, frame, one, ellipse, .. } = scene();
    let before = places(&document);
    let group = document.group(&[one, ellipse], NodeKind::Frame { clip: false }).unwrap();
    assert_eq!(children(&document, page), [frame, group]);
    assert_eq!(children(&document, group), [one, ellipse]);
    assert_same_places(&before, &places(&document));
    assert_eq!(document.node(group).unwrap().kind, NodeKind::Frame { clip: false });
}

#[test]
fn a_node_and_something_inside_it_group_as_the_node() {
    let Scene { mut document, page, frame, one, two, ellipse } = scene();
    let group = document.group(&[one, frame, frame], NodeKind::Group).unwrap();
    assert_eq!(children(&document, page), [group, ellipse]);
    assert_eq!(children(&document, group), [frame]);
    assert_eq!(children(&document, frame), [one, two]);
}

#[test]
fn grouping_nothing_or_as_a_shape_is_refused() {
    let Scene { mut document, page, one, .. } = scene();
    let before = document.clone();
    assert_eq!(document.group(&[], NodeKind::Group), Err(Error::Nothing));
    assert_eq!(document.group(&[page], NodeKind::Group), Err(Error::Nothing));
    assert_eq!(document.group(&[NodeId(999)], NodeKind::Group), Err(Error::Nothing));
    assert_eq!(document.group(&[one], NodeKind::Rectangle), Err(Error::NotAContainer(one)));
    assert_eq!(document.group(&[one], NodeKind::Page), Err(Error::NotAContainer(one)));
    assert_eq!(document, before);
}

#[test]
fn ungrouping_leaves_the_members_where_the_group_was() {
    let Scene { mut document, page, frame, one, two, ellipse } = scene();
    let before = places(&document);
    assert_eq!(document.ungroup(frame).unwrap(), [one, two]);
    assert_eq!(children(&document, page), [one, two, ellipse]);
    assert_same_places(&before, &places(&document));
    assert_eq!(document.ungroup(one), Err(Error::NotAContainer(one)));
    assert_eq!(document.ungroup(page), Err(Error::PageInsideNode));
    assert_eq!(document.ungroup(NodeId(999)), Err(Error::NoSuchNode(NodeId(999))));
}

#[test]
fn group_then_ungroup_changes_nothing() {
    let Scene { mut document, one, two, .. } = scene();
    let before = document.clone();
    let group = document.group(&[one, two], NodeKind::Group).unwrap();
    document.ungroup(group).unwrap();
    assert_eq!(document.pages, before.pages);
}

#[test]
fn a_duplicate_sits_just_in_front_with_ids_of_its_own() {
    let Scene { mut document, page, frame, one, two, ellipse } = scene();
    let copies = document.duplicate(&[frame, one]).unwrap();
    // `one` is inside `frame`, so it comes along with it and isn't copied twice.
    let [copy] = copies[..] else { panic!("{copies:?}") };
    assert_eq!(children(&document, page), [frame, copy, ellipse]);
    assert_eq!(children(&document, frame), [one, two]);
    let (original, copied) = (document.node(frame).unwrap(), document.node(copy).unwrap());
    assert_eq!(copied.transform, original.transform);
    assert_eq!(copied.children.len(), 2);
    let ids: BTreeSet<NodeId> = [frame, one, two, ellipse, copy].into_iter().chain(copied.children.iter().map(|node| node.id)).collect();
    assert_eq!(ids.len(), 7);
}

#[test]
fn restacking_moves_past_what_is_not_moving() {
    let mut document = Document::default();
    let page = document.pages[0].id;
    let [a, b, c, d, e] = [0; 5].map(|_| add(&mut document, page, NodeKind::Rectangle, Affine::IDENTITY, (1.0, 1.0)));
    let mut order = |ids: &[NodeId], to| {
        document.restack(ids, to).unwrap();
        children(&document, page)
    };
    assert_eq!(order(&[a, c], Stack::Forward), [b, a, d, c, e]);
    assert_eq!(order(&[c, e], Stack::Forward), [b, a, d, c, e], "already at the front, together");
    assert_eq!(order(&[c, e], Stack::Backward), [b, a, c, e, d]);
    assert_eq!(order(&[b, d], Stack::Front), [a, c, e, b, d]);
    assert_eq!(order(&[e, b], Stack::Back), [e, b, a, c, d]);
    assert_eq!(order(&[], Stack::Front), [e, b, a, c, d]);
}

#[test]
fn a_copy_pastes_where_it_was_on_the_page_whatever_it_goes_into() {
    let Scene { mut document, page, frame, one, ellipse, .. } = scene();
    let was = document.to_page(one).unwrap();
    let clip = document.copy(&[one, ellipse]);
    // Into the page, out of the rotated frame it was copied from.
    let pasted = document.paste(page, &clip).unwrap();
    assert_eq!(children(&document, page)[2..], pasted[..]);
    assert!((document.to_page(pasted[0]).unwrap().as_coeffs().iter().zip(was.as_coeffs()).all(|(a, b)| (a - b).abs() < 1e-9)));
    // And into the frame, from the page.
    let pasted = document.paste(frame, &clip).unwrap();
    let is = document.to_page(pasted[1]).unwrap().as_coeffs();
    assert!(is.iter().zip(document.to_page(ellipse).unwrap().as_coeffs()).all(|(a, b)| (a - b).abs() < 1e-9));
    assert_eq!(document.paste(one, &clip), Err(Error::NotAContainer(one)));
}

#[test]
fn the_area_of_some_nodes_is_the_box_round_them_on_the_page() {
    let Scene { mut document, one, two, ellipse, frame, .. } = scene();
    document.node_mut(frame).unwrap().transform = Affine::translate((100.0, 100.0));
    assert_eq!(document.area(&[one, two]), Some(Rect::new(110.0, 120.0, 300.0, 220.0)));
    assert_eq!(document.area(&[one, ellipse]), Some(Rect::new(110.0, 50.0, 560.0, 160.0)));
    assert_eq!(document.area(&[]), None);
    // A group is as big as what is in it, wherever its own corner is.
    let group = document.group(&[one, two], NodeKind::Group).unwrap();
    document.node_mut(one).unwrap().transform = Affine::translate((-50.0, 0.0));
    assert_eq!(document.area(&[group]), Some(Rect::new(60.0, 120.0, 300.0, 220.0)));
}

#[test]
fn stretching_resizes_nodes_and_never_skews_them() {
    let Scene { mut document, frame, one, two, ellipse, .. } = scene();
    // Twice as wide about x = 500: the ellipse at 500..560 becomes 500..620.
    let wider = Affine::translate((500.0, 0.0)) * Affine::scale_non_uniform(2.0, 1.0) * Affine::translate((-500.0, 0.0));
    document.node_mut(ellipse).unwrap().stretch(wider);
    let node = document.node(ellipse).unwrap();
    assert_eq!((node.transform, node.size), (Affine::translate((500.0, 50.0)), Size::new(120.0, 60.0)));

    // A group passes it on to what is in it; a frame's children stay.
    let group = document.group(&[one, two], NodeKind::Group).unwrap();
    assert_eq!(document.node(group).unwrap().bounds(), Rect::new(0.0, 0.0, 190.0, 100.0));
    let half = Affine::translate((10.0, 20.0)) * Affine::scale(0.5) * Affine::translate((-10.0, -20.0));
    document.node_mut(group).unwrap().stretch(half);
    assert_eq!(document.node(group).unwrap().bounds(), Rect::new(0.0, 0.0, 95.0, 50.0));
    assert_eq!(document.node(one).unwrap().size, Size::new(25.0, 20.0));
    assert_eq!(document.node(two).unwrap().transform, Affine::translate((55.0, 35.0)));
    let before = document.node(two).unwrap().clone();
    document.node_mut(frame).unwrap().stretch(Affine::scale(3.0));
    assert_eq!(document.node(two).unwrap(), &before);

    // The rotated frame, stretched sideways on the page, is still a
    // rectangle at the same angle, with its middle where the stretch put it.
    let was = document.node(frame).unwrap().clone();
    let middle = was.transform * omavec_geom::kurbo::Point::new(450.0, 300.0);
    document.node_mut(frame).unwrap().stretch(wider);
    let is = document.node(frame).unwrap();
    assert_eq!(is.transform.as_coeffs()[..4], was.transform.as_coeffs()[..4]);
    let moved = is.transform * (is.size.to_vec2() / 2.0).to_point();
    assert!((moved - wider * middle).hypot() < 1e-9);
    // A side at 0.5 radians to a doubling in x: longer by hypot(2 cos, sin).
    let grown = (2.0 * 0.5f64.cos()).hypot(0.5f64.sin());
    assert!((is.size.width - 900.0 * grown).abs() < 1e-9, "{:?}", is.size);
}

#[derive(Clone, Debug)]
enum Action {
    Group(Vec<usize>, bool),
    Ungroup(usize),
    Duplicate(Vec<usize>),
    Restack(Vec<usize>, u8),
    CopyPaste(Vec<usize>, usize),
}

fn action() -> impl Strategy<Value = Action> {
    let some = || prop::collection::vec(0usize..64, 0..4);
    prop_oneof![
        (some(), any::<bool>()).prop_map(|(ids, frame)| Action::Group(ids, frame)),
        (0usize..64).prop_map(Action::Ungroup),
        some().prop_map(Action::Duplicate),
        (some(), 0u8..4).prop_map(|(ids, to)| Action::Restack(ids, to)),
        (some(), 0usize..64).prop_map(|(ids, into)| Action::CopyPaste(ids, into)),
    ]
}

fn all(nodes: &[Arc<Node>], out: &mut Vec<NodeId>) {
    for node in nodes {
        out.push(node.id);
        all(&node.children, out);
    }
}

proptest! {
    /// Whatever is done, in whatever order: nothing that isn't a container
    /// moves on the page, no id appears twice, and a refusal changes nothing.
    #[test]
    fn rearranging_never_moves_anything(actions in prop::collection::vec(action(), 1..24)) {
        let Scene { mut document, .. } = scene();
        for action in actions {
            let mut ids = Vec::new();
            all(&document.pages, &mut ids);
            let pick = |indices: &[usize]| indices.iter().map(|i| ids[i % ids.len()]).collect::<Vec<_>>();
            let (before, places_before) = (document.clone(), places(&document));
            let done = match &action {
                Action::Group(members, frame) => document.group(&pick(members), if *frame { NodeKind::Frame { clip: true } } else { NodeKind::Group }).map(drop),
                Action::Ungroup(index) => document.ungroup(pick(&[*index])[0]).map(drop),
                Action::Duplicate(members) => document.duplicate(&pick(members)).map(drop),
                Action::Restack(members, to) => document.restack(&pick(members), [Stack::Front, Stack::Forward, Stack::Backward, Stack::Back][usize::from(*to)]),
                Action::CopyPaste(members, into) => {
                    let clip = before.copy(&pick(members));
                    document.paste(pick(&[*into])[0], &clip).map(drop)
                }
            };
            if done.is_err() {
                prop_assert_eq!(&document.pages, &before.pages, "{:?} was refused but changed something", action);
            }
            assert_same_places(&places_before, &places(&document));
            let mut after = Vec::new();
            all(&document.pages, &mut after);
            let unique: BTreeSet<NodeId> = after.iter().copied().collect();
            prop_assert_eq!(unique.len(), after.len());
            // Only a duplicate or a paste makes more of anything.
            if !matches!(action, Action::Duplicate(_) | Action::CopyPaste(..)) {
                let leaves = |ids: &[NodeId], document: &Document| ids.iter().filter(|id| !document.node(**id).unwrap().kind.is_container()).count();
                prop_assert_eq!(leaves(&ids, &before), leaves(&after, &document));
            }
        }
    }
}
