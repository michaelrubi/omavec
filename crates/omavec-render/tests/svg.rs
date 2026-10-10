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
    let stroke = |color: Color, weight: f64, align: Align| Stroke { paints: vec![Paint::solid(color)], weight, align };

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
