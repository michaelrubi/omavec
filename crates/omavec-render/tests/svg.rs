//! Tests comparing omavec-render output against resvg rasterisation.

use kurbo::{Affine, Size};
use omavec_engine::display::DisplayList;
use omavec_engine::{Color, Document, NodeId, NodeKind, Paint};
use omavec_render::Renderer;

fn assert_svg_matches_render(label: &str, document: &Document, frame_id: NodeId) {
    let frame_node = document.node(frame_id).unwrap();
    let width = frame_node.size.width.round() as u16;
    let height = frame_node.size.height.round() as u16;

    let svg_text = omavec_engine::svg::write(document, frame_id).unwrap();
    if let Ok(dir) = std::env::var("DUMP_SVG") { std::fs::write(format!("{dir}/{label}.svg"), &svg_text).unwrap(); }

    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(&svg_text, &opt).unwrap();
    let mut resvg_pixmap = resvg::tiny_skia::Pixmap::new(u32::from(width), u32::from(height)).unwrap();
    resvg_pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut resvg_pixmap.as_mut());

    let list = DisplayList::of(&document.pages[0]);
    let to_page = document.to_page(frame_id).unwrap();
    let view = to_page.inverse();
    let mut renderer = Renderer::default();
    let rendered = renderer.render(&list, view, width, height, peniko::Color::WHITE);

    let total_pixels = usize::from(width) * usize::from(height);
    let omavec_pixels = &rendered.pixels;
    let resvg_pixels = resvg_pixmap.data();
    assert_eq!(omavec_pixels.len(), total_pixels * 4);
    assert_eq!(resvg_pixels.len(), total_pixels * 4);

    let mut differing = 0;
    for i in 0..total_pixels {
        let idx = i * 4;
        let r_diff = (i32::from(omavec_pixels[idx]) - i32::from(resvg_pixels[idx])).abs();
        let g_diff = (i32::from(omavec_pixels[idx + 1]) - i32::from(resvg_pixels[idx + 1])).abs();
        let b_diff = (i32::from(omavec_pixels[idx + 2]) - i32::from(resvg_pixels[idx + 2])).abs();
        let a_diff = (i32::from(omavec_pixels[idx + 3]) - i32::from(resvg_pixels[idx + 3])).abs();

        if r_diff > 32 || g_diff > 32 || b_diff > 32 || a_diff > 32 {
            differing += 1;
        }
    }

    let percentage = (differing as f64 / total_pixels as f64) * 100.0;
    println!("{label}: {percentage:.4}% pixels differ ({differing}/{total_pixels})");
    assert!(percentage < 1.0, "{label}: {percentage:.4}% pixels differ by > 32, which exceeds 1%");
}

#[test]
fn spec_document_renders_identically() {
    let mut document = Document::default();
    let page = document.pages[0].id;

    let mut frame = document.create(NodeKind::Frame { clip: false }, Size::new(400.0, 300.0));
    frame.fills = vec![Paint::solid(Color::rgb(0xff, 0xff, 0xff))];
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();

    let mut r1 = document.create(NodeKind::Rectangle, Size::new(100.0, 50.0));
    r1.transform = Affine::translate((20.0, 40.0));
    r1.fills = vec![Paint::solid(Color::rgb(0xd9, 0xd9, 0xd9))];
    document.insert(frame_id, 0, r1).unwrap();

    let mut e1 = document.create(NodeKind::Ellipse, Size::new(100.0, 60.0));
    e1.transform = Affine::translate((200.0, 120.0));
    let mut p1 = Paint::solid(Color::rgb(0xff, 0x00, 0x00));
    p1.opacity = 0.5;
    e1.fills = vec![p1];
    document.insert(frame_id, 1, e1).unwrap();

    let mut group = document.create(NodeKind::Group, Size::new(60.0, 40.0));
    group.transform = Affine::translate((10.0, 10.0));
    group.opacity = 0.5;
    let group_id = group.id;
    document.insert(frame_id, 2, group).unwrap();

    let mut r2 = document.create(NodeKind::Rectangle, Size::new(20.0, 20.0));
    r2.transform = Affine::IDENTITY;
    r2.fills = vec![Paint::solid(Color::rgb(0x00, 0x00, 0xff))];
    document.insert(group_id, 0, r2).unwrap();

    let mut r3 = document.create(NodeKind::Rectangle, Size::new(20.0, 20.25));
    r3.transform = Affine::translate((30.5, 0.0));
    r3.visible = false;
    document.insert(group_id, 1, r3).unwrap();

    let mut r4 = document.create(NodeKind::Rectangle, Size::new(50.0, 50.0));
    r4.transform = Affine::translate((300.0, 200.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2);
    let mut p2 = Paint::solid(Color::rgb(0x00, 0x00, 0x00));
    p2.opacity = 0.25;
    r4.fills = vec![Paint::solid(Color::rgb(0x00, 0xff, 0x00)), p2];
    r4.opacity = 0.8;
    document.insert(frame_id, 3, r4).unwrap();

    assert_svg_matches_render("spec_document", &document, frame_id);
}

#[test]
fn translated_frame_with_rotated_ellipse_renders_identically() {
    let mut document = Document::default();
    let page = document.pages[0].id;

    // Frame placed away from page origin at (37, 55)
    let mut frame = document.create(NodeKind::Frame { clip: false }, Size::new(320.0, 240.0));
    frame.transform = Affine::translate((37.0, 55.0));
    frame.fills = vec![Paint::solid(Color::rgb(0xff, 0xff, 0xff))];
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();

    // Rotated ellipse with translucent fill
    let mut e1 = document.create(NodeKind::Ellipse, Size::new(100.0, 60.0));
    e1.transform = Affine::translate((160.0, 110.0)) * Affine::rotate(std::f64::consts::FRAC_PI_4);
    let mut p1 = Paint::solid(Color::rgb(0xe6, 0x39, 0x46));
    p1.opacity = 0.8;
    e1.fills = vec![p1];
    document.insert(frame_id, 0, e1).unwrap();

    // Translucent leaf rectangle
    let mut r1 = document.create(NodeKind::Rectangle, Size::new(80.0, 50.0));
    r1.transform = Affine::translate((20.0, 20.0));
    r1.fills = vec![Paint::solid(Color::rgb(0x1d, 0x35, 0x57))];
    r1.opacity = 0.7;
    document.insert(frame_id, 1, r1).unwrap();

    // Solid ellipse with translation only
    let mut e2 = document.create(NodeKind::Ellipse, Size::new(60.0, 60.0));
    e2.transform = Affine::translate((30.0, 130.0));
    e2.fills = vec![Paint::solid(Color::rgb(0x2a, 0x9d, 0x8f))];
    document.insert(frame_id, 2, e2).unwrap();

    assert_svg_matches_render("translated_frame_with_rotated_ellipse", &document, frame_id);
}

#[test]
fn nested_frame_inside_rotated_group_renders_identically() {
    let mut document = Document::default();
    let page = document.pages[0].id;

    // Frame placed at (42, 18)
    let mut frame = document.create(NodeKind::Frame { clip: false }, Size::new(360.0, 280.0));
    frame.transform = Affine::translate((42.0, 18.0));
    frame.fills = vec![Paint::solid(Color::rgb(0xff, 0xff, 0xff))];
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();

    // Base rectangle on the frame
    let mut r_bg = document.create(NodeKind::Rectangle, Size::new(70.0, 50.0));
    r_bg.transform = Affine::translate((20.0, 20.0));
    r_bg.fills = vec![Paint::solid(Color::rgb(0x10, 0xb9, 0x81))];
    document.insert(frame_id, 0, r_bg).unwrap();

    // Rotated group
    let mut group = document.create(NodeKind::Group, Size::new(120.0, 100.0));
    group.transform = Affine::translate((180.0, 130.0)) * Affine::rotate(std::f64::consts::FRAC_PI_6);
    let group_id = group.id;
    document.insert(frame_id, 1, group).unwrap();

    // Nested frame inside the rotated group
    let mut nested_frame = document.create(NodeKind::Frame { clip: false }, Size::new(100.0, 80.0));
    nested_frame.transform = Affine::translate((15.0, 10.0));
    nested_frame.fills = vec![Paint::solid(Color::rgb(0xe0, 0xe7, 0xff))];
    let nested_id = nested_frame.id;
    document.insert(group_id, 0, nested_frame).unwrap();

    // Rectangle inside nested frame
    let mut r1 = document.create(NodeKind::Rectangle, Size::new(40.0, 30.0));
    r1.transform = Affine::translate((10.0, 10.0));
    r1.fills = vec![Paint::solid(Color::rgb(0x43, 0x38, 0xca))];
    document.insert(nested_id, 0, r1).unwrap();

    // Ellipse inside nested frame
    let mut e1 = document.create(NodeKind::Ellipse, Size::new(35.0, 35.0));
    e1.transform = Affine::translate((50.0, 25.0));
    let mut p_e = Paint::solid(Color::rgb(0x06, 0xb6, 0xd4));
    p_e.opacity = 0.6;
    e1.fills = vec![p_e];
    document.insert(nested_id, 1, e1).unwrap();

    assert_svg_matches_render("nested_frame_inside_rotated_group", &document, frame_id);
}

#[test]
fn strokes_render_identically_on_every_side_of_the_edge() {
    use omavec_engine::{Align, Stroke};
    let mut document = Document::default();
    let page = document.pages[0].id;
    let stroke = |color: Color, weight: f64, align: Align| Stroke { paints: vec![Paint::solid(color)], weight, align, ..Default::default() };

    // A frame with a thick inside stroke of its own, drawn over what is in it.
    let mut frame = document.create(NodeKind::Frame { clip: false }, Size::new(360.0, 240.0));
    frame.transform = Affine::translate((21.0, 34.0));
    frame.stroke = stroke(Color::rgb(0x33, 0x33, 0x33), 12.0, Align::Inside);
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();

    // One of each alignment, far enough apart that the sides are plain to see,
    // and one overlapping the frame's own stroke.
    for (i, align) in [Align::Inside, Align::Center, Align::Outside].into_iter().enumerate() {
        let mut ellipse = document.create(NodeKind::Ellipse, Size::new(80.0, 50.0));
        ellipse.transform = Affine::translate((30.0 + 110.0 * i as f64, 30.0));
        ellipse.fills = vec![Paint::solid(Color::rgb(0xf9, 0xe2, 0xaf))];
        ellipse.stroke = stroke(Color::rgb(0xd2, 0x0f, 0x39), 14.0, align);
        document.insert(frame_id, usize::MAX, ellipse).unwrap();

        let mut rectangle = document.create(NodeKind::Rectangle, Size::new(70.0, 40.0));
        rectangle.transform = Affine::translate((50.0 + 110.0 * i as f64, 140.0)) * Affine::rotate(0.4);
        rectangle.fills = vec![Paint::solid(Color::rgb(0x89, 0xb4, 0xfa))];
        rectangle.stroke = stroke(Color::rgb(0x40, 0xa0, 0x2b), 10.0, align);
        rectangle.stroke.paints[0].opacity = 0.6;
        document.insert(frame_id, usize::MAX, rectangle).unwrap();
    }
    let mut edge = document.create(NodeKind::Rectangle, Size::new(60.0, 60.0));
    edge.transform = Affine::translate((-20.0, 90.0));
    document.insert(frame_id, usize::MAX, edge).unwrap();

    assert_svg_matches_render("strokes", &document, frame_id);
}

#[test]
fn a_clipping_frame_and_a_faded_group_render_identically() {
    let mut document = Document::default();
    let page = document.pages[0].id;
    let mut add = |parent: NodeId, kind: NodeKind, transform: Affine, size: (f64, f64), fill: Option<Color>| {
        let mut node = document.create(kind, Size::new(size.0, size.1));
        node.transform = transform;
        node.fills = fill.into_iter().map(Paint::solid).collect();
        let id = node.id;
        document.insert(parent, usize::MAX, node).unwrap();
        id
    };
    let frame_id = add(page, NodeKind::Frame { clip: true }, Affine::translate((25.0, 15.0)), (320.0, 240.0), Some(Color::rgb(0xff, 0xff, 0xff)));
    // A turned frame that clips, with an ellipse and a bar sticking out of it.
    let inner = add(frame_id, NodeKind::Frame { clip: true }, Affine::translate((60.0, 30.0)) * Affine::rotate(0.3), (140.0, 90.0), Some(Color::rgb(0xf1, 0xfa, 0xee)));
    add(inner, NodeKind::Ellipse, Affine::translate((90.0, 40.0)), (100.0, 100.0), Some(Color::rgb(0xe6, 0x39, 0x46)));
    add(inner, NodeKind::Rectangle, Affine::translate((-30.0, 20.0)), (80.0, 20.0), Some(Color::rgb(0x1d, 0x35, 0x57)));
    // A group at 60%, with two shapes that overlap, half off the outer frame.
    let group = add(frame_id, NodeKind::Group, Affine::translate((200.0, 150.0)), (0.0, 0.0), None);
    add(group, NodeKind::Rectangle, Affine::IDENTITY, (90.0, 60.0), Some(Color::rgb(0x2a, 0x9d, 0x8f)));
    add(group, NodeKind::Ellipse, Affine::translate((50.0, 20.0)), (120.0, 90.0), Some(Color::rgb(0xe9, 0xc4, 0x6a)));
    document.node_mut(group).unwrap().opacity = 0.6;
    // And the turned frame's stroke, outside it and so outside its clip.
    document.node_mut(inner).unwrap().stroke = omavec_engine::Stroke { paints: vec![Paint::solid(Color::rgb(0, 0, 0))], weight: 4.0, align: omavec_engine::Align::Outside, ..Default::default() };

    assert_svg_matches_render("a_clipping_frame_and_a_faded_group", &document, frame_id);
}

#[test]
fn every_kind_of_shape_renders_identically() {
    use omavec_engine::{Align, Cap, Join, Stroke};
    let mut document = Document::default();
    let page = document.pages[0].id;
    let mut frame = document.create(NodeKind::Frame { clip: true }, Size::new(360.0, 240.0));
    frame.radii = [30.0; 4];
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();
    let mut add = |kind: NodeKind, transform: Affine, size: (f64, f64), color: Color| {
        let mut node = document.create(kind, Size::new(size.0, size.1));
        node.transform = transform;
        if node.kind != NodeKind::Line {
            node.fills = vec![Paint::solid(color)];
        }
        let id = node.id;
        document.insert(frame_id, usize::MAX, node).unwrap();
        id
    };
    let at = |x: f64, y: f64| Affine::translate((x, y));
    // In the frame's rounded corner, to be cut off by it.
    add(NodeKind::Rectangle, at(-10.0, -10.0), (60.0, 60.0), Color::rgb(0x1d, 0x35, 0x57));
    let rounded = add(NodeKind::Rectangle, at(70.0, 20.0), (90.0, 60.0), Color::rgb(0xe6, 0x39, 0x46));
    let lopsided = add(NodeKind::Rectangle, at(180.0, 20.0) * Affine::rotate(0.2), (90.0, 60.0), Color::rgb(0x2a, 0x9d, 0x8f));
    add(NodeKind::Arc { start: 0.5, sweep: 4.0, ratio: 0.5 }, at(20.0, 100.0), (90.0, 70.0), Color::rgb(0xe9, 0xc4, 0x6a));
    let polygon = add(NodeKind::Polygon { sides: 6 }, at(130.0, 100.0), (80.0, 80.0), Color::rgb(0x45, 0x7b, 0x9d));
    add(NodeKind::Star { points: 5, ratio: 0.382 }, at(230.0, 100.0), (100.0, 90.0), Color::rgb(0xf4, 0xa2, 0x61));
    let line = add(NodeKind::Line, at(30.0, 200.0), (120.0, 0.0), Color::rgb(0, 0, 0));
    let arrow = add(NodeKind::Line, at(190.0, 215.0) * Affine::rotate(-0.3), (120.0, 0.0), Color::rgb(0, 0, 0));
    document.node_mut(rounded).unwrap().radii = [12.0; 4];
    document.node_mut(lopsided).unwrap().radii = [30.0, 0.0, 15.0, 5.0];
    let black = |weight: f64, align: Align| Stroke { paints: vec![Paint::solid(Color::rgb(0, 0, 0))], weight, align, ..Default::default() };
    document.node_mut(rounded).unwrap().stroke = black(4.0, Align::Inside);
    document.node_mut(polygon).unwrap().stroke = Stroke { join: Join::Round, ..black(8.0, Align::Center) };
    let stroke = &mut document.node_mut(line).unwrap().stroke;
    (stroke.weight, stroke.start_cap, stroke.end_cap) = (10.0, Cap::Round, Cap::Round);
    let stroke = &mut document.node_mut(arrow).unwrap().stroke;
    (stroke.weight, stroke.start_cap, stroke.end_cap) = (3.0, Cap::Triangle, Cap::Arrow);

    assert_svg_matches_render("every_kind_of_shape", &document, frame_id);
}

#[test]
fn gradients_render_identically() {
    use omavec_engine::{Align, PaintKind, Stop, Stroke};
    let mut document = Document::default();
    let page = document.pages[0].id;
    let frame = document.create(NodeKind::Frame { clip: true }, Size::new(360.0, 240.0));
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();
    let stop = |at: f64, color: Color, opacity: f64| Stop { at, color, opacity };
    let (red, blue, gold) = (Color::rgb(0xe6, 0x39, 0x46), Color::rgb(0x1d, 0x35, 0x57), Color::rgb(0xe9, 0xc4, 0x6a));
    let mut add = |kind: NodeKind, transform: Affine, size: (f64, f64), paint: PaintKind| {
        let mut node = document.create(kind, Size::new(size.0, size.1));
        node.transform = transform;
        node.fills = vec![Paint { kind: paint, opacity: 1.0, visible: true }];
        let id = node.id;
        document.insert(frame_id, usize::MAX, node).unwrap();
        id
    };
    let at = |x: f64, y: f64| Affine::translate((x, y));
    // Corner to corner on a plain rectangle, with a stop in between.
    add(NodeKind::Rectangle, at(20.0, 20.0), (140.0, 80.0), PaintKind::Linear { from: (0.0, 0.0), to: (1.0, 1.0), stops: vec![stop(0.0, red, 1.0), stop(0.4, gold, 1.0), stop(1.0, blue, 1.0)] });
    // From the middle of a wide ellipse: as wide and as high as its box.
    add(NodeKind::Ellipse, at(190.0, 20.0), (150.0, 80.0), PaintKind::Radial { from: (0.5, 0.5), to: (1.0, 0.5), stops: vec![stop(0.0, gold, 1.0), stop(1.0, red, 1.0)] });
    // On a turned star, which is a path: fading out, over the frame.
    add(NodeKind::Star { points: 5, ratio: 0.5 }, at(60.0, 120.0) * Affine::rotate(0.3), (110.0, 100.0), PaintKind::Linear { from: (0.5, 0.0), to: (0.5, 1.0), stops: vec![stop(0.0, blue, 1.0), stop(1.0, blue, 0.0)] });
    // Off-centre in a rounded rectangle, with a gradient for a stroke too.
    let card = add(NodeKind::Rectangle, at(200.0, 125.0), (140.0, 95.0), PaintKind::Radial { from: (0.25, 0.3), to: (0.9, 0.3), stops: vec![stop(0.0, red, 1.0), stop(0.5, gold, 0.5), stop(1.0, blue, 1.0)] });
    let node = document.node_mut(card).unwrap();
    node.radii = [18.0; 4];
    let across = PaintKind::Linear { from: (0.0, 0.5), to: (1.0, 0.5), stops: vec![stop(0.0, blue, 1.0), stop(1.0, red, 1.0)] };
    node.stroke = Stroke { paints: vec![Paint { kind: across, opacity: 1.0, visible: true }], weight: 8.0, align: Align::Center, ..Default::default() };

    assert_svg_matches_render("gradients", &document, frame_id);
}

#[test]
fn blend_modes_render_identically() {
    use omavec_engine::Blend;
    let mut document = Document::default();
    let page = document.pages[0].id;
    let mut frame = document.create(NodeKind::Frame { clip: true }, Size::new(360.0, 200.0));
    frame.fills = vec![Paint::solid(Color::rgb(0xe9, 0xc4, 0x6a))];
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();
    let mut add = |parent: NodeId, kind: NodeKind, at: (f64, f64), size: (f64, f64), color: Option<Color>, blend: Blend| {
        let mut node = document.create(kind, Size::new(size.0, size.1));
        (node.transform, node.blend) = (Affine::translate(at), blend);
        node.fills = color.into_iter().map(Paint::solid).collect();
        let id = node.id;
        document.insert(parent, usize::MAX, node).unwrap();
        id
    };
    // A dark bar for the modes to show against, half the height of each.
    add(frame_id, NodeKind::Rectangle, (0.0, 100.0), (360.0, 100.0), Some(Color::rgb(0x1d, 0x35, 0x57)), Blend::Normal);
    let modes = [Blend::Multiply, Blend::Screen, Blend::Overlay, Blend::Darken, Blend::Lighten, Blend::Difference, Blend::Exclusion, Blend::HardLight];
    for (index, blend) in modes.into_iter().enumerate() {
        add(frame_id, NodeKind::Ellipse, (10.0 + 43.0 * index as f64, 60.0), (40.0, 80.0), Some(Color::rgb(0xe6, 0x39, 0x46)), blend);
    }
    // A group that multiplies as one, half faded, holding two that overlap.
    let group = add(frame_id, NodeKind::Group, (40.0, 10.0), (0.0, 0.0), None, Blend::Multiply);
    add(group, NodeKind::Rectangle, (0.0, 0.0), (120.0, 40.0), Some(Color::rgb(0x2a, 0x9d, 0x8f)), Blend::Normal);
    add(group, NodeKind::Rectangle, (80.0, 10.0), (120.0, 40.0), Some(Color::rgb(0x45, 0x7b, 0x9d)), Blend::Normal);
    document.node_mut(group).unwrap().opacity = 0.5;

    assert_svg_matches_render("blend_modes", &document, frame_id);
}
