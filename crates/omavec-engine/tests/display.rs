//! A page flattened into what the renderer draws.

use omavec_engine::display::{DisplayList, Item};
use omavec_engine::{Color, Document, NodeId, NodeKind, Paint};
use omavec_geom::kurbo::{Affine, Rect, Shape, Size};

const RED: Color = Color::rgb(255, 0, 0);
const BLUE: Color = Color::rgb(0, 0, 255);

/// Adds a node of `kind`, `size` at (`x`, `y`) in `parent`, filled with `fill`.
fn add(document: &mut Document, parent: NodeId, kind: NodeKind, at: (f64, f64), size: (f64, f64), fill: Color) -> NodeId {
    let mut node = document.create(kind, Size::new(size.0, size.1));
    node.transform = Affine::translate(at);
    node.fills = vec![Paint::solid(fill)];
    let id = node.id;
    document.insert(parent, usize::MAX, node).unwrap();
    id
}

fn rgba(color: Color, alpha: u8) -> [u8; 4] {
    [color.r, color.g, color.b, alpha]
}

/// Each fill's colour and bounds, back to front.
fn drawn(document: &Document) -> Vec<([u8; 4], Rect)> {
    let fill = |item: &Item| match item {
        Item::Fill(fill) => {
            let c = fill.color.to_rgba8();
            Some(([c.r, c.g, c.b, c.a], fill.bounds()))
        }
        _ => None,
    };
    DisplayList::of(&document.pages[0]).items.iter().filter_map(fill).collect()
}

/// The list as letters: `f` a fill, `[` and `]` round what is clipped, `(`
/// and `)` round what fades together.
fn steps(document: &Document) -> String {
    let letter = |item: &Item| match item {
        Item::Fill(_) => 'f',
        Item::Clip(_) => '[',
        Item::Unclip => ']',
        Item::Fade(_) => '(',
        Item::Unfade => ')',
    };
    DisplayList::of(&document.pages[0]).items.iter().map(letter).collect()
}

#[test]
fn nodes_are_drawn_back_to_front_where_their_parents_put_them() {
    let mut document = Document::default();
    let page = document.pages[0].id;
    let frame = add(&mut document, page, NodeKind::Frame { clip: false }, (100.0, 200.0), (400.0, 300.0), BLUE);
    let group = document.create(NodeKind::Group, Size::ZERO);
    let group_id = group.id;
    document.insert(frame, 0, group).unwrap();
    document.node_mut(group_id).unwrap().transform = Affine::translate((10.0, 10.0));
    add(&mut document, group_id, NodeKind::Rectangle, (5.0, 6.0), (20.0, 30.0), RED);

    assert_eq!(
        drawn(&document),
        [
            // The frame's own fill, then what's inside it: 100 + 10 + 5 across, 200 + 10 + 6 down.
            (rgba(BLUE, 255), Rect::new(100.0, 200.0, 500.0, 500.0)),
            (rgba(RED, 255), Rect::new(115.0, 216.0, 135.0, 246.0)),
        ]
    );
}

#[test]
fn a_new_shape_has_figmas_grey_and_a_new_frame_is_white() {
    let mut document = Document::default();
    let page = document.pages[0].id;
    for kind in [NodeKind::Frame { clip: true }, NodeKind::Rectangle, NodeKind::Ellipse, NodeKind::Group] {
        let node = document.create(kind, Size::new(10.0, 10.0));
        document.insert(page, usize::MAX, node).unwrap();
    }
    let colours: Vec<[u8; 4]> = drawn(&document).into_iter().map(|(colour, _)| colour).collect();
    // The group draws nothing itself.
    assert_eq!(colours, [[255, 255, 255, 255], [0xd9, 0xd9, 0xd9, 255], [0xd9, 0xd9, 0xd9, 255]]);
}

#[test]
fn an_ellipse_is_an_ellipse() {
    let mut document = Document::default();
    let page = document.pages[0].id;
    add(&mut document, page, NodeKind::Ellipse, (10.0, 20.0), (200.0, 100.0), RED);
    let list = DisplayList::of(&document.pages[0]);
    let Item::Fill(fill) = &list.items[0] else { panic!() };
    let path = &fill.path;
    assert!((path.area().abs() - std::f64::consts::PI * 100.0 * 50.0).abs() < 1.0, "{}", path.area());
    assert!((fill.bounds().x0 - 10.0).abs() < 1e-3 && (fill.bounds().y1 - 120.0).abs() < 1e-3);
    // The corners of its box are outside it; the middle is inside.
    assert_ne!(path.winding((110.0, 70.0).into()), 0);
    assert_eq!(path.winding((12.0, 22.0).into()), 0);
}

#[test]
fn hidden_nodes_and_hidden_fills_are_left_out() {
    let mut document = Document::default();
    let page = document.pages[0].id;
    let frame = add(&mut document, page, NodeKind::Frame { clip: false }, (0.0, 0.0), (100.0, 100.0), BLUE);
    let rectangle = add(&mut document, frame, NodeKind::Rectangle, (0.0, 0.0), (10.0, 10.0), RED);
    assert_eq!(drawn(&document).len(), 2);

    // A hidden fill hides that fill only; fills are painted bottom to top.
    let mut hidden = Paint::solid(BLUE);
    hidden.visible = false;
    let mut half = Paint::solid(BLUE);
    half.opacity = 0.5;
    document.node_mut(rectangle).unwrap().fills = vec![Paint::solid(RED), hidden, half];
    let colours: Vec<[u8; 4]> = drawn(&document).into_iter().map(|(colour, _)| colour).collect();
    assert_eq!(colours, [rgba(BLUE, 255), rgba(RED, 255), rgba(BLUE, 128)]);

    // A hidden node hides everything inside it.
    document.node_mut(frame).unwrap().visible = false;
    assert!(drawn(&document).is_empty());
}

#[test]
fn a_nodes_opacity_fades_all_of_it_together() {
    let mut document = Document::default();
    let page = document.pages[0].id;
    let frame = add(&mut document, page, NodeKind::Frame { clip: false }, (0.0, 0.0), (100.0, 100.0), BLUE);
    let rectangle = add(&mut document, frame, NodeKind::Rectangle, (0.0, 0.0), (10.0, 10.0), RED);
    document.node_mut(frame).unwrap().opacity = 0.5;
    document.node_mut(rectangle).unwrap().opacity = 0.5;
    // The frame and what is in it are drawn as they are and faded as one.
    // The rectangle is one fill, so it is simply half as strong.
    assert_eq!(steps(&document), "(ff)");
    let colours: Vec<[u8; 4]> = drawn(&document).into_iter().map(|(colour, _)| colour).collect();
    assert_eq!(colours, [rgba(BLUE, 255), rgba(RED, 128)]);
    let Item::Fade(opacity) = DisplayList::of(&document.pages[0]).items[0] else { panic!() };
    assert_eq!(opacity, 0.5);
    // Two fills on one node fade together too.
    document.node_mut(rectangle).unwrap().fills.push(Paint::solid(BLUE));
    assert_eq!(steps(&document), "(f(ff))");
    // At full strength nothing needs doing.
    document.node_mut(frame).unwrap().opacity = 1.0;
    document.node_mut(rectangle).unwrap().opacity = 1.0;
    assert_eq!(steps(&document), "fff");
}

#[test]
fn a_frame_that_clips_does_so_round_its_children_only() {
    let mut document = Document::default();
    let page = document.pages[0].id;
    let frame = add(&mut document, page, NodeKind::Frame { clip: true }, (10.0, 20.0), (100.0, 50.0), BLUE);
    // Nothing in it: nothing to clip.
    assert_eq!(steps(&document), "f");
    add(&mut document, frame, NodeKind::Rectangle, (90.0, 40.0), (30.0, 30.0), RED);
    document.node_mut(frame).unwrap().stroke = omavec_engine::Stroke { paints: vec![Paint::solid(RED)], weight: 4.0, align: omavec_engine::Align::Outside };
    // Its fill, its children inside the clip, then its stroke outside it.
    assert_eq!(steps(&document), "f[f]f");
    let list = DisplayList::of(&document.pages[0]);
    let Item::Clip(clip) = &list.items[1] else { panic!() };
    assert_eq!(clip.bounding_box(), Rect::new(10.0, 20.0, 110.0, 70.0));
    // One that doesn't clip, and a group, are just what is in them.
    document.node_mut(frame).unwrap().kind = NodeKind::Frame { clip: false };
    assert_eq!(steps(&document), "fff");
}

#[test]
fn colours_are_hex_in_a_file_and_nothing_else_is_taken() {
    let paint = Paint::solid(Color::rgb(0x1e, 0x1e, 0x2e));
    let text = serde_json::to_string(&paint).unwrap();
    assert_eq!(text, r##"{"type":"solid","color":"#1e1e2e"}"##);
    assert_eq!(serde_json::from_str::<Paint>(&text).unwrap(), paint);
    assert_eq!(serde_json::from_str::<Color>("\"#FFaa00\"").unwrap(), Color::rgb(255, 170, 0));
    for bad in ["\"1e1e2e\"", "\"#1e1e2\"", "\"#1e1e2e0\"", "\"#gg0000\"", "\"#é0000\"", "\"red\"", "12"] {
        assert!(serde_json::from_str::<Color>(bad).is_err(), "{bad} was taken as a colour");
    }
}

#[test]
fn a_stroke_is_drawn_over_the_fill_and_a_frames_over_its_children() {
    use omavec_engine::{Align, Stroke};
    let black = Color::rgb(0, 0, 0);
    let mut document = Document::default();
    let page = document.pages[0].id;
    let frame = add(&mut document, page, NodeKind::Frame { clip: false }, (0.0, 0.0), (100.0, 100.0), BLUE);
    let rectangle = add(&mut document, frame, NodeKind::Rectangle, (10.0, 10.0), (50.0, 20.0), RED);
    document.node_mut(frame).unwrap().stroke = Stroke { paints: vec![Paint::solid(black)], weight: 4.0, align: Align::Outside };
    document.node_mut(rectangle).unwrap().stroke = Stroke { paints: vec![Paint::solid(black)], weight: 2.0, align: Align::Inside };

    let drawn = drawn(&document);
    let colours: Vec<[u8; 4]> = drawn.iter().map(|(colour, _)| *colour).collect();
    assert_eq!(colours, [rgba(BLUE, 255), rgba(RED, 255), rgba(black, 255), rgba(black, 255)]);
    // The rectangle's inside stroke stays in its box; the frame's outside one
    // reaches four units past its own.
    let close = |a: Rect, b: Rect| (a.x0 - b.x0).abs() + (a.y0 - b.y0).abs() + (a.x1 - b.x1).abs() + (a.y1 - b.y1).abs() < 1e-6;
    assert!(close(drawn[2].1, Rect::new(10.0, 10.0, 60.0, 30.0)), "{:?}", drawn[2].1);
    assert!(close(drawn[3].1, Rect::new(-4.0, -4.0, 104.0, 104.0)), "{:?}", drawn[3].1);
    // No weight, or no visible paint: no stroke.
    document.node_mut(frame).unwrap().stroke.weight = 0.0;
    document.node_mut(rectangle).unwrap().stroke.paints[0].visible = false;
    assert_eq!(self::drawn(&document).len(), 2);
}

#[test]
fn a_stroke_is_saved_only_when_there_is_one() {
    use omavec_engine::{Align, Stroke};
    let mut document = Document::default();
    let mut node = document.create(NodeKind::Rectangle, Size::new(10.0, 10.0));
    assert!(!serde_json::to_string(&node).unwrap().contains("stroke"));
    node.stroke = Stroke { paints: vec![Paint::solid(RED)], weight: 2.5, align: Align::Center };
    let text = serde_json::to_string(&node).unwrap();
    assert!(text.contains(r##""stroke":{"paints":[{"type":"solid","color":"#ff0000"}],"weight":2.5,"align":"center"}"##), "{text}");
    assert_eq!(serde_json::from_str::<omavec_engine::Node>(&text).unwrap(), node);
}
