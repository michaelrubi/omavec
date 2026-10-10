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
    // A group is as big as what is in it, hidden or not, whatever size it
    // was made with: 0..50.5 by 0..20.25, out to whole units for a picture.
    let expected = "\
<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"51\" height=\"21\" viewBox=\"0 0 51 21\">
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

#[test]
fn a_centred_stroke_is_a_stroke_and_the_others_are_the_area_they_cover() {
    use omavec_engine::{Align, Stroke};
    let mut document = Document::default();
    let page = document.pages[0].id;
    let black = |weight: f64, align: Align| Stroke { paints: vec![Paint::solid(Color::rgb(0, 0, 0))], weight, align, ..Default::default() };

    let mut frame = document.create(NodeKind::Frame { clip: false }, Size::new(100.0, 60.0));
    frame.stroke = black(1.0, Align::Center);
    frame.stroke.paints[0].opacity = 0.5;
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();

    let mut centred = document.create(NodeKind::Ellipse, Size::new(20.0, 10.0));
    centred.transform = Affine::translate((5.0, 5.0));
    centred.stroke = black(2.5, Align::Center);
    document.insert(frame_id, usize::MAX, centred).unwrap();

    let mut inside = document.create(NodeKind::Rectangle, Size::new(20.0, 10.0));
    inside.transform = Affine::translate((40.0, 5.0));
    inside.fills.clear();
    inside.stroke = black(2.0, Align::Inside);
    document.insert(frame_id, usize::MAX, inside).unwrap();

    let mut hidden = document.create(NodeKind::Rectangle, Size::new(20.0, 10.0));
    hidden.fills.clear();
    hidden.stroke = black(2.0, Align::Outside);
    hidden.stroke.paints[0].visible = false;
    document.insert(frame_id, usize::MAX, hidden).unwrap();

    let svg = omavec_engine::svg::write(&document, frame_id).unwrap();
    let lines: Vec<&str> = svg.lines().collect();
    assert_eq!(lines[1], r##"  <rect width="100" height="60" fill="#ffffff"/>"##);
    assert_eq!(lines[2], r##"  <ellipse cx="15" cy="10" rx="10" ry="5" fill="#d9d9d9"/>"##);
    assert_eq!(lines[3], r##"  <ellipse cx="15" cy="10" rx="10" ry="5" fill="none" stroke="#000000" stroke-width="2.5"/>"##);
    // An inside stroke on a 20 × 10 box: the ring between it and 16 × 6.
    assert!(lines[4].starts_with(r#"  <path d="M"#) && lines[4].ends_with(r##"Z" transform="translate(40 5)" fill="#000000"/>"##), "{}", lines[4]);
    for number in lines[4].split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|part| !part.is_empty()) {
        assert!(number.split_once('.').is_none_or(|(_, decimals)| decimals.len() <= 3), "{number} in {}", lines[4]);
    }
    // The frame's own stroke last, over what is in it; the hidden one nowhere.
    assert_eq!(lines[5], r##"  <rect width="100" height="60" fill="none" stroke="#000000" stroke-width="1" stroke-opacity="0.5"/>"##);
    assert_eq!(lines[6], "</svg>");
}

#[test]
fn a_frame_that_clips_is_a_clip_path_round_its_children() {
    let mut document = Document::default();
    let page = document.pages[0].id;
    let mut add = |parent: NodeId, kind: NodeKind, at: (f64, f64), size: (f64, f64)| {
        let mut node = document.create(kind, Size::new(size.0, size.1));
        node.transform = Affine::translate(at);
        let id = node.id;
        document.insert(parent, usize::MAX, node).unwrap();
        id
    };
    let outer = add(page, NodeKind::Frame { clip: true }, (500.0, 500.0), (200.0, 100.0));
    let inner = add(outer, NodeKind::Frame { clip: true }, (20.0, 10.0), (60.0, 40.0));
    add(inner, NodeKind::Ellipse, (40.0, 20.0), (40.0, 40.0));
    add(outer, NodeKind::Frame { clip: true }, (120.0, 10.0), (60.0, 40.0));
    // The picture's own edge clips the frame that is exported; the frame
    // in it gets a clip path, named after its id; one with nothing in it
    // has nothing to clip.
    let expected = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
  <rect width="200" height="100" fill="#ffffff"/>
  <g transform="translate(20 10)">
    <rect width="60" height="40" fill="#ffffff"/>
    <clipPath id="clip3"><rect width="60" height="40"/></clipPath>
    <g clip-path="url(#clip3)">
      <ellipse cx="60" cy="40" rx="20" ry="20" fill="#d9d9d9"/>
    </g>
  </g>
  <g transform="translate(120 10)">
    <rect width="60" height="40" fill="#ffffff"/>
  </g>
</svg>
"##;
    assert_eq!(svg::write(&document, outer).unwrap(), expected);
}

#[test]
fn shapes_are_the_plainest_element_that_says_them() {
    use omavec_engine::Cap;
    let mut document = Document::default();
    let page = document.pages[0].id;
    let frame = document.create(NodeKind::Frame { clip: false }, Size::new(300.0, 200.0));
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();
    let mut add = |kind: NodeKind, at: (f64, f64), size: (f64, f64)| {
        let mut node = document.create(kind, Size::new(size.0, size.1));
        node.transform = Affine::translate(at);
        let id = node.id;
        document.insert(frame_id, usize::MAX, node).unwrap();
        id
    };
    let rounded = add(NodeKind::Rectangle, (10.0, 10.0), (80.0, 40.0));
    let lopsided = add(NodeKind::Rectangle, (100.0, 10.0), (80.0, 40.0));
    add(NodeKind::Polygon { sides: 4 }, (10.0, 60.0), (40.0, 40.0));
    let line = add(NodeKind::Line, (100.0, 80.0), (60.0, 0.0));
    let arrow = add(NodeKind::Line, (100.0, 120.0), (60.0, 0.0));
    document.node_mut(rounded).unwrap().radii = [8.0; 4];
    // More than fits: half the shorter side is as round as it gets.
    document.node_mut(frame_id).unwrap().radii = [500.0; 4];
    document.node_mut(lopsided).unwrap().radii = [20.0, 0.0, 0.0, 0.0];
    let stroke = &mut document.node_mut(line).unwrap().stroke;
    (stroke.weight, stroke.start_cap, stroke.end_cap) = (4.0, Cap::Round, Cap::Round);
    document.node_mut(arrow).unwrap().stroke.end_cap = Cap::Triangle;

    let svg = svg::write(&document, frame_id).unwrap();
    let lines: Vec<&str> = svg.lines().map(str::trim).collect();
    assert_eq!(lines[1], r##"<rect width="300" height="200" rx="100" fill="#ffffff"/>"##);
    assert_eq!(lines[2], r##"<rect x="10" y="10" width="80" height="40" rx="8" fill="#d9d9d9"/>"##);
    // One round corner: a path, starting where that corner's arc does.
    assert!(lines[3].starts_with(r#"<path d="M0 20C"#) && lines[3].ends_with(r##"L0 40Z" transform="translate(100 10)" fill="#d9d9d9"/>"##), "{}", lines[3]);
    assert_eq!(lines[4], r##"<path d="M20 0L40 20L20 40L0 20Z" transform="translate(10 60)" fill="#d9d9d9"/>"##);
    // A line is a stroke; one with an arrowhead is the area it covers.
    assert_eq!(lines[5], r##"<path d="M0 0L60 0" transform="translate(100 80)" fill="none" stroke="#000000" stroke-width="4" stroke-linecap="round"/>"##);
    assert!(lines[6].starts_with(r#"<path d="M"#) && lines[6].ends_with(r##"Z" transform="translate(100 120)" fill="#000000"/>"##), "{}", lines[6]);
    assert_eq!(lines.len(), 8);
}

#[test]
fn a_picture_is_as_big_as_what_the_node_paints() {
    use omavec_engine::{Align, Cap, Stroke};
    let mut document = Document::default();
    let page = document.pages[0].id;
    let mut line = document.create(NodeKind::Line, Size::new(100.0, 0.0));
    line.transform = Affine::translate((300.0, 300.0)) * Affine::rotate(1.0);
    line.stroke.weight = 6.0;
    let line_id = line.id;
    document.insert(page, 0, line).unwrap();
    // A line's box has no height, but its stroke has: three either side.
    // It is exported upright, wherever and however it lies on the page.
    let svg = svg::write(&document, line_id).unwrap();
    assert!(svg.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="6" viewBox="0 -3 100 6">"#), "{svg}");
    // Round ends reach three past each end as well.
    let stroke = &mut document.node_mut(line_id).unwrap().stroke;
    (stroke.start_cap, stroke.end_cap) = (Cap::Round, Cap::Round);
    assert!(svg::write(&document, line_id).unwrap().contains(r#"viewBox="-3 -3 106 6""#));
    // A rectangle with a stroke outside it is that much bigger all round;
    // a frame is its box whatever it paints, as in Figma.
    let mut rectangle = document.create(NodeKind::Rectangle, Size::new(40.0, 20.0));
    rectangle.stroke = Stroke { paints: vec![Paint::solid(Color::rgb(0, 0, 0))], weight: 2.5, align: Align::Outside, ..Default::default() };
    let rectangle_id = rectangle.id;
    document.insert(page, 1, rectangle).unwrap();
    assert!(svg::write(&document, rectangle_id).unwrap().contains(r#"width="46" height="26" viewBox="-3 -3 46 26""#));
    document.node_mut(rectangle_id).unwrap().kind = NodeKind::Frame { clip: false };
    assert!(svg::write(&document, rectangle_id).unwrap().contains(r#"width="40" height="20" viewBox="0 0 40 20""#));
}
