//! Tests for SVG export.

use omavec_engine::svg;
use omavec_engine::{Color, Document, Error, NodeId, NodeKind, Paint};
use omavec_geom::kurbo::{Affine, Size};

fn build_spec_document() -> (Document, NodeId, NodeId) {
    let mut document = Document::default();
    let page = document.pages[0].id;

    // Frame 400×300, fill #ffffff
    let mut frame = document.create(NodeKind::Frame { clip: false }, Size::new(400.0, 300.0));
    frame.fills = vec![Paint::solid(Color::rgb(0xff, 0xff, 0xff))];
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();

    // Rectangle at (20, 40), 100×50, fill #d9d9d9
    let mut r1 = document.create(NodeKind::Rectangle, Size::new(100.0, 50.0));
    r1.transform = Affine::translate((20.0, 40.0));
    r1.fills = vec![Paint::solid(Color::rgb(0xd9, 0xd9, 0xd9))];
    document.insert(frame_id, 0, r1).unwrap();

    // Ellipse at (200, 120), 100×60, fill #ff0000 with fill opacity 0.5
    let mut e1 = document.create(NodeKind::Ellipse, Size::new(100.0, 60.0));
    e1.transform = Affine::translate((200.0, 120.0));
    let mut p1 = Paint::solid(Color::rgb(0xff, 0x00, 0x00));
    p1.opacity = 0.5;
    e1.fills = vec![p1];
    document.insert(frame_id, 1, e1).unwrap();

    // Group at (10, 10), opacity 0.5
    let mut group = document.create(NodeKind::Group, Size::new(60.0, 40.0));
    group.transform = Affine::translate((10.0, 10.0));
    group.opacity = 0.5;
    let group_id = group.id;
    document.insert(frame_id, 2, group).unwrap();

    // Rectangle at (0, 0), 20×20, fill #0000ff
    let mut r2 = document.create(NodeKind::Rectangle, Size::new(20.0, 20.0));
    r2.transform = Affine::IDENTITY;
    r2.fills = vec![Paint::solid(Color::rgb(0x00, 0x00, 0xff))];
    document.insert(group_id, 0, r2).unwrap();

    // Rectangle at (30.5, 0), 20×20.25, hidden
    let mut r3 = document.create(NodeKind::Rectangle, Size::new(20.0, 20.25));
    r3.transform = Affine::translate((30.5, 0.0));
    r3.visible = false;
    document.insert(group_id, 1, r3).unwrap();

    // Rectangle, 50×50, transform = Affine::translate((300, 200)) * Affine::rotate(90°), two fills (#00ff00, then #000000 at fill opacity 0.25), node opacity 0.8
    let mut r4 = document.create(NodeKind::Rectangle, Size::new(50.0, 50.0));
    r4.transform = Affine::translate((300.0, 200.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2);
    let mut p2 = Paint::solid(Color::rgb(0x00, 0x00, 0x00));
    p2.opacity = 0.25;
    r4.fills = vec![Paint::solid(Color::rgb(0x00, 0xff, 0x00)), p2];
    r4.opacity = 0.8;
    document.insert(frame_id, 3, r4).unwrap();

    (document, frame_id, group_id)
}

#[test]
fn spec_document_matches_exact_svg() {
    let (document, frame_id, _) = build_spec_document();
    let expected = "\
<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"400\" height=\"300\" viewBox=\"0 0 400 300\">
  <rect width=\"400\" height=\"300\" fill=\"#ffffff\"/>
  <rect x=\"20\" y=\"40\" width=\"100\" height=\"50\" fill=\"#d9d9d9\"/>
  <ellipse cx=\"250\" cy=\"150\" rx=\"50\" ry=\"30\" fill=\"#ff0000\" fill-opacity=\"0.5\"/>
  <g transform=\"translate(10 10)\" opacity=\"0.5\">
    <rect width=\"20\" height=\"20\" fill=\"#0000ff\"/>
  </g>
  <g opacity=\"0.8\">
    <rect width=\"50\" height=\"50\" transform=\"matrix(0 1 -1 0 300 200)\" fill=\"#00ff00\"/>
    <rect width=\"50\" height=\"50\" transform=\"matrix(0 1 -1 0 300 200)\" fill=\"#000000\" fill-opacity=\"0.25\"/>
  </g>
</svg>
";
    let svg_text = svg::write(&document, frame_id).unwrap();
    assert_eq!(svg_text, expected);
}

#[test]
fn export_group_by_id_sizes_to_group_without_parent_transform_or_opacity() {
    let (document, _, group_id) = build_spec_document();
    let expected = "\
<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"60\" height=\"40\" viewBox=\"0 0 60 40\">
  <rect width=\"20\" height=\"20\" fill=\"#0000ff\"/>
</svg>
";
    let svg_text = svg::write(&document, group_id).unwrap();
    assert_eq!(svg_text, expected);
}

#[test]
fn nonexistent_node_returns_error() {
    let document = Document::default();
    let missing_id = NodeId(123456);
    assert_eq!(svg::write(&document, missing_id), Err(Error::NoSuchNode(missing_id)));
}

#[test]
fn number_formatting_through_documents() {
    let mut document = Document::default();
    let page = document.pages[0].id;

    // Frame with size 100.0 × 12.3456
    let mut frame = document.create(NodeKind::Frame { clip: false }, Size::new(100.0, 12.3456));
    frame.fills = vec![Paint::solid(Color::rgb(0xff, 0xff, 0xff))];
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();

    // Child rectangle with size (0.1 + 0.2) × 10.0 at (0.1 + 0.2, 0)
    let mut r1 = document.create(NodeKind::Rectangle, Size::new(0.1 + 0.2, 10.0));
    r1.transform = Affine::translate((0.1 + 0.2, 0.0));
    r1.fills = vec![Paint::solid(Color::rgb(0xff, 0x00, 0x00))];
    document.insert(frame_id, 0, r1).unwrap();

    // Rotated rectangle placing -0.0004 and 1e-9 into matrix positions (e, f)
    let mut r2 = document.create(NodeKind::Rectangle, Size::new(10.0, 10.0));
    r2.transform = Affine::translate((-0.0004, 1e-9)) * Affine::rotate(std::f64::consts::FRAC_PI_2);
    r2.fills = vec![Paint::solid(Color::rgb(0x00, 0xff, 0x00))];
    document.insert(frame_id, 1, r2).unwrap();

    let text = svg::write(&document, frame_id).unwrap();
    assert!(text.contains("width=\"100\" height=\"12.346\" viewBox=\"0 0 100 12.346\""));
    assert!(text.contains("<rect width=\"100\" height=\"12.346\" fill=\"#ffffff\"/>"));
    assert!(text.contains("<rect x=\"0.3\" width=\"0.3\" height=\"10\" fill=\"#ff0000\"/>"));
    assert!(text.contains("transform=\"matrix(0 1 -1 0 0 0)\""));
    assert!(!text.contains("-0"));
}

#[test]
fn hidden_root_exports_empty_svg() {
    let (mut document, frame_id, _) = build_spec_document();
    document.node_mut(frame_id).unwrap().visible = false;
    let expected = "\
<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"400\" height=\"300\" viewBox=\"0 0 400 300\">
</svg>
";
    let svg_text = svg::write(&document, frame_id).unwrap();
    assert_eq!(svg_text, expected);
}
