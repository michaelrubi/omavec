//! SVG export: writes a node and everything in it as an SVG document.

use std::fmt::Write;

use omavec_geom::kurbo::{Affine, BezPath, PathEl, Point};
use omavec_geom::stroke::{Align, Cap, Join, outline};

use crate::document::{Document, Error, Node, NodeId, NodeKind};
use crate::paint::{Blend, Paint, PaintKind};

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

/// The radius of all four corners of `node`, if they are the same, as SVG's
/// `rx` has it: no more than half of either side.
fn rounding(node: &Node) -> Option<f64> {
    let [radius, rest @ ..] = node.radii;
    rest.iter().all(|other| *other == radius).then(|| radius.clamp(0.0, node.size.width.min(node.size.height) / 2.0))
}

/// Whether `node` is written as a `<rect>` or an `<ellipse>`, not a path.
fn is_plain(node: &Node) -> bool {
    match node.kind {
        NodeKind::Frame { .. } | NodeKind::Rectangle => rounding(node).is_some(),
        NodeKind::Ellipse => true,
        _ => false,
    }
}

/// Whether `node`'s stroke is something SVG's own strokes can't say, and
/// is written as the area it covers instead, which any viewer draws the
/// same. SVG strokes are centred, with one kind of cap for both ends.
fn is_outlined(node: &Node) -> bool {
    let stroke = &node.stroke;
    match node.kind {
        NodeKind::Line => stroke.start_cap != stroke.end_cap || matches!(stroke.end_cap, Cap::Arrow | Cap::Triangle),
        _ => stroke.align != Align::Center,
    }
}

/// Writes one element for `piece` of `node`, with attributes in this order:
/// geometry, transform, fill or stroke, their opacity, opacity.
fn write_element(out: &mut String, indent: usize, node: &Node, transform: Option<&Affine>, piece: Piece<'_>, opacity: Option<f64>) {
    let translation = transform.filter(|t| is_translation(t)).map(|t| t.translation());
    let matrix = transform.filter(|t| !is_translation(t)).map(|t| t.as_coeffs());
    let (paint, outlined) = match piece {
        Piece::Stroke(paint) if is_outlined(node) => (paint, node.shape().map(|shape| outline(&shape, &node.stroke.style()))),
        Piece::Fill(paint) | Piece::Stroke(paint) => (paint, None),
    };
    let path = outlined.clone().or_else(|| node.shape().filter(|_| !is_plain(node)));
    // A gradient is defined just before what it paints, and named after it.
    let gradient = paint.kind.stops().map(|stops| {
        let (letter, paints) = if matches!(piece, Piece::Fill(_)) { ('f', &node.fills) } else { ('s', &node.stroke.paints) };
        let id = format!("paint{}{letter}{}", node.id.0, paints.iter().position(|other| std::ptr::eq(other, paint)).unwrap_or(0));
        // It is laid out in the node's box, a unit square: stretch that to
        // the box, and move it to where a plain element's x and y put it.
        let moved = translation.filter(|by| path.is_none() && (num(by.x) != "0" || num(by.y) != "0")).map(|by| format!("translate({} {}) ", num(by.x), num(by.y))).unwrap_or_default();
        let placed = format!("gradientUnits=\"userSpaceOnUse\" gradientTransform=\"{moved}scale({} {})\"", num(node.size.width), num(node.size.height));
        let (tag, line) = match paint.kind {
            PaintKind::Radial { from, to, .. } => ("radialGradient", format!("cx=\"{}\" cy=\"{}\" r=\"{}\"", num(from.0), num(from.1), num((to.0 - from.0).hypot(to.1 - from.1)))),
            PaintKind::Linear { from, to, .. } => ("linearGradient", format!("x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"", num(from.0), num(from.1), num(to.0), num(to.1))),
            PaintKind::Solid { .. } => ("linearGradient", String::new()),
        };
        let _ = writeln!(out, "{:indent$}<{tag} id=\"{id}\" {line} {placed}>", "");
        for stop in stops {
            let _ = write!(out, "{:indent$}  <stop offset=\"{}\" stop-color=\"{}\"", "", num(stop.at.clamp(0.0, 1.0)), String::from(stop.color));
            if stop.opacity != 1.0 {
                let _ = write!(out, " stop-opacity=\"{}\"", num(stop.opacity));
            }
            out.push_str("/>\n");
        }
        let _ = writeln!(out, "{:indent$}</{tag}>", "");
        format!("url(#{id})")
    });
    if let Some(path) = &path {
        let _ = write!(out, "{:indent$}<path d=\"{}\"", "", path_data(path));
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
        if let Some(radius) = rounding(node).map(num).filter(|radius| radius != "0") {
            let _ = write!(out, " rx=\"{radius}\"");
        }
    }
    if let Some([a, b, c, d, e, f]) = matrix {
        let _ = write!(out, " transform=\"matrix({} {} {} {} {} {})\"", num(a), num(b), num(c), num(d), num(e), num(f));
    }

    let color = gradient.unwrap_or_else(|| String::from(paint.kind.color()));
    let painted = match piece {
        Piece::Stroke(_) if outlined.is_none() => {
            let _ = write!(out, " fill=\"none\" stroke=\"{color}\" stroke-width=\"{}\"", num(node.stroke.weight));
            // Mitred corners and ends cut square are what SVG assumes.
            let cap = match node.stroke.end_cap {
                Cap::Round if node.kind == NodeKind::Line => Some("round"),
                Cap::Square if node.kind == NodeKind::Line => Some("square"),
                _ => None,
            };
            let join = match node.stroke.join {
                Join::Miter => None,
                Join::Bevel => Some("bevel"),
                Join::Round => Some("round"),
            };
            for (name, value) in [("stroke-linecap", cap), ("stroke-linejoin", join)] {
                if let Some(value) = value {
                    let _ = write!(out, " {name}=\"{value}\"");
                }
            }
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

/// The attribute that says how `node` mixes with what is under it, if it
/// isn't the usual way.
fn blend(node: &Node) -> Option<String> {
    (node.blend != Blend::Normal).then(|| format!(" style=\"mix-blend-mode:{}\"", node.blend.css()))
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
        let blend_attr = blend(node);
        let has_g = transform_attr.is_some() || opacity_attr.is_some() || blend_attr.is_some();
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
                out.push_str(&blend_attr.unwrap_or_default());
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
        // Several pieces under one opacity are grouped, so they fade as one;
        // so is whatever mixes with what is under it in its own way.
        let blended = blend(node).filter(|_| !painted.is_empty());
        let grouped = painted.len() > 1 && node.opacity != 1.0 || blended.is_some();
        if grouped {
            let opacity = if node.opacity != 1.0 { format!(" opacity=\"{}\"", num(node.opacity)) } else { String::new() };
            let _ = writeln!(out, "{:indent$}<g{opacity}{}>", "", blended.unwrap_or_default());
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
    // Rounded corners are not the picture's edge.
    let clip = matches!(node.kind, NodeKind::Frame { clip: true }) && !(clipped && node.radii == [0.0; 4]);
    let mut children = String::new();
    for child in &node.children {
        write_node(child, if clip { indent + 2 } else { indent }, &mut children);
    }
    if clip && !children.is_empty() {
        let id = node.id.0;
        let shape = match rounding(node).map(num) {
            Some(radius) if radius == "0" => format!("<rect width=\"{}\" height=\"{}\"/>", num(node.size.width), num(node.size.height)),
            Some(radius) => format!("<rect width=\"{}\" height=\"{}\" rx=\"{radius}\"/>", num(node.size.width), num(node.size.height)),
            None => format!("<path d=\"{}\"/>", node.shape().map(|shape| path_data(&shape)).unwrap_or_default()),
        };
        let _ = writeln!(out, "{:indent$}<clipPath id=\"clip{id}\">{shape}</clipPath>", "");
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
