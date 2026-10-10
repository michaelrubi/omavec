//! SVG export: writes a node and everything in it as an SVG document.

use std::fmt::Write;

use omavec_geom::kurbo::{Affine, BezPath, PathEl, Point};
use omavec_geom::stroke::{Align, outline};

use crate::display::shape;
use crate::document::{Document, Error, Node, NodeId, NodeKind};
use crate::paint::{Paint, PaintKind};

/// One thing a node paints: a fill of its shape, or a layer of its stroke.
#[derive(Clone, Copy)]
enum Piece<'a> {
    Fill(&'a Paint),
    Stroke(&'a Paint),
}

/// A node's visible fills, or the visible layers of its stroke.
fn pieces(node: &Node, strokes: bool) -> impl Iterator<Item = Piece<'_>> {
    let paints = if strokes { &node.stroke.paints } else { &node.fills };
    // A stroke with no weight draws nothing.
    let none = strokes && node.stroke.weight.is_nan() || strokes && node.stroke.weight <= 0.0;
    paints.iter().filter(move |paint| paint.visible && !none).map(move |paint| if strokes { Piece::Stroke(paint) } else { Piece::Fill(paint) })
}

/// Path data with the same short numbers as everything else.
fn path_data(path: &BezPath) -> String {
    let mut data = String::new();
    for element in path.elements() {
        let (command, points) = match element {
            PathEl::MoveTo(p) => ('M', vec![p]),
            PathEl::LineTo(p) => ('L', vec![p]),
            PathEl::QuadTo(a, p) => ('Q', vec![a, p]),
            PathEl::CurveTo(a, b, p) => ('C', vec![a, b, p]),
            PathEl::ClosePath => ('Z', Vec::new()),
        };
        data.push(command);
        let numbers: Vec<String> = points.iter().flat_map(|p| [num(p.x), num(p.y)]).collect();
        data.push_str(&numbers.join(" "));
    }
    data
}

/// Formats a number to at most 3 decimal places without trailing zeros or a
/// trailing '.', clamping -0 to 0.
fn num(n: f64) -> String {
    let rounded = (n * 1000.0).round() / 1000.0;
    let n = if rounded.abs() < 1e-9 { 0.0 } else { rounded };
    let mut s = format!("{:.3}", n);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s
    }
}

fn is_translation(t: &Affine) -> bool {
    let [a, b, c, d, _, _] = t.as_coeffs();
    num(a) == "1" && num(b) == "0" && num(c) == "0" && num(d) == "1"
}

fn is_identity(t: &Affine) -> bool {
    let [_, _, _, _, e, f] = t.as_coeffs();
    is_translation(t) && num(e) == "0" && num(f) == "0"
}

/// Writes one element for `piece` of `node`, with attributes in this order:
/// geometry, transform, fill or stroke, their opacity, opacity.
fn write_element(out: &mut String, indent: usize, node: &Node, transform: Option<&Affine>, piece: Piece<'_>, opacity: Option<f64>) {
    let translation = transform.filter(|t| is_translation(t)).map(|t| t.translation());
    let matrix = transform.filter(|t| !is_translation(t)).map(|t| t.as_coeffs());
    // SVG strokes are centred. One on the inside or the outside is written
    // as the area it covers, which any viewer draws the same.
    let (paint, outlined) = match piece {
        Piece::Stroke(paint) if node.stroke.align != Align::Center => (paint, shape(node).map(|shape| outline(&shape, node.stroke.weight, node.stroke.align))),
        Piece::Fill(paint) | Piece::Stroke(paint) => (paint, None),
    };
    if let Some(outline) = &outlined {
        let _ = write!(out, "{:indent$}<path d=\"{}\"", "", path_data(outline));
        if let Some(by) = translation.filter(|by| num(by.x) != "0" || num(by.y) != "0") {
            let _ = write!(out, " transform=\"translate({} {})\"", num(by.x), num(by.y));
        }
    } else if node.kind == NodeKind::Ellipse {
        let (rx, ry) = (node.size.width / 2.0, node.size.height / 2.0);
        let by = translation.unwrap_or_default();
        let _ = write!(out, "{:indent$}<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"", "", num(by.x + rx), num(by.y + ry), num(rx), num(ry));
    } else {
        let _ = write!(out, "{:indent$}<rect", "");
        for (name, value) in [("x", translation.map(|by| by.x)), ("y", translation.map(|by| by.y))] {
            if let Some(value) = value.map(num).filter(|value| value != "0") {
                let _ = write!(out, " {name}=\"{value}\"");
            }
        }
        let _ = write!(out, " width=\"{}\" height=\"{}\"", num(node.size.width), num(node.size.height));
    }
    if let Some([a, b, c, d, e, f]) = matrix {
        let _ = write!(out, " transform=\"matrix({} {} {} {} {} {})\"", num(a), num(b), num(c), num(d), num(e), num(f));
    }

    let PaintKind::Solid { color } = paint.kind;
    let color = String::from(color);
    let painted = match piece {
        Piece::Stroke(_) if outlined.is_none() => {
            let _ = write!(out, " fill=\"none\" stroke=\"{color}\" stroke-width=\"{}\"", num(node.stroke.weight));
            "stroke-opacity"
        }
        _ => {
            let _ = write!(out, " fill=\"{color}\"");
            "fill-opacity"
        }
    };
    if paint.opacity != 1.0 {
        let _ = write!(out, " {painted}=\"{}\"", num(paint.opacity));
    }
    if let Some(op) = opacity.filter(|&o| o != 1.0) {
        let _ = write!(out, " opacity=\"{}\"", num(op));
    }
    out.push_str("/>\n");
}

fn write_node(node: &Node, indent: usize, out: &mut String) {
    if !node.visible {
        return;
    }
    if node.kind.is_container() {
        // Collect children and fills into a buffer so empty containers can be dropped.
        let [a, b, c, d, e, f] = node.transform.as_coeffs();
        let transform_attr = if is_identity(&node.transform) {
            None
        } else if is_translation(&node.transform) {
            Some(format!("translate({} {})", num(e), num(f)))
        } else {
            Some(format!("matrix({} {} {} {} {} {})", num(a), num(b), num(c), num(d), num(e), num(f)))
        };
        let opacity_attr = (node.opacity != 1.0).then(|| num(node.opacity));
        let has_g = transform_attr.is_some() || opacity_attr.is_some();
        let child_indent = if has_g { indent + 2 } else { indent };

        let mut body = String::new();
        write_inside(node, child_indent, false, &mut body);

        if !body.is_empty() {
            if has_g {
                let _ = write!(out, "{:indent$}<g", "");
                if let Some(t) = transform_attr {
                    let _ = write!(out, " transform=\"{t}\"");
                }
                if let Some(op) = opacity_attr {
                    let _ = write!(out, " opacity=\"{op}\"");
                }
                out.push_str(">\n");
                out.push_str(&body);
                let _ = writeln!(out, "{:indent$}</g>", "");
            } else {
                out.push_str(&body);
            }
        }
    } else {
        // Fills, then the stroke over them.
        let painted: Vec<Piece<'_>> = pieces(node, false).chain(pieces(node, true)).collect();
        // Several pieces under one opacity are grouped, so they fade as one.
        let grouped = painted.len() > 1 && node.opacity != 1.0;
        if grouped {
            let _ = writeln!(out, "{:indent$}<g opacity=\"{}\">", "", num(node.opacity));
        }
        let inner = if grouped { indent + 2 } else { indent };
        for piece in painted {
            write_element(out, inner, node, Some(&node.transform), piece, (!grouped).then_some(node.opacity));
        }
        if grouped {
            let _ = writeln!(out, "{:indent$}</g>", "");
        }
    }
}

/// What is inside a frame, a group or the root, at its own origin: its
/// fills, its children, then its stroke over them. A frame that clips does
/// so round its children, unless `clipped` already (the root, which the
/// picture's own edge clips).
fn write_inside(node: &Node, indent: usize, clipped: bool, out: &mut String) {
    for piece in pieces(node, false) {
        write_element(out, indent, node, None, piece, None);
    }
    let clip = !clipped && matches!(node.kind, NodeKind::Frame { clip: true });
    let mut children = String::new();
    for child in &node.children {
        write_node(child, if clip { indent + 2 } else { indent }, &mut children);
    }
    if clip && !children.is_empty() {
        let id = node.id.0;
        let _ = writeln!(out, "{:indent$}<clipPath id=\"clip{id}\"><rect width=\"{}\" height=\"{}\"/></clipPath>", "", num(node.size.width), num(node.size.height));
        let _ = writeln!(out, "{:indent$}<g clip-path=\"url(#clip{id})\">", "");
        out.push_str(&children);
        let _ = writeln!(out, "{:indent$}</g>", "");
    } else {
        out.push_str(&children);
    }
    for piece in pieces(node, true) {
        write_element(out, indent, node, None, piece, None);
    }
}

/// `id` and everything in it as an SVG document, sized to the node's box.
pub fn write(document: &Document, id: NodeId) -> Result<String, Error> {
    let node = document.node(id).ok_or(Error::NoSuchNode(id))?;
    // A group's box needn't start at its origin.
    let area = node.bounds();
    let corner = if area.origin() == Point::ZERO { "0 0".into() } else { format!("{} {}", num(area.x0), num(area.y0)) };
    let (width, height) = (num(area.width()), num(area.height()));
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{corner} {width} {height}\">\n");
    if node.visible {
        write_inside(node, 2, true, &mut out);
    }
    out.push_str("</svg>\n");
    Ok(out)
}
