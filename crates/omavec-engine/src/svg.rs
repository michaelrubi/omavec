//! SVG export: writes a node and everything in it as an SVG document.

use std::fmt::Write;

use omavec_geom::kurbo::{Affine, Size};

use crate::document::{Document, Error, Node, NodeId, NodeKind};
use crate::paint::{Paint, PaintKind};

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

/// Writes one rect or ellipse element, with attributes in specification order:
/// geometry, transform, fill, fill-opacity, opacity.
fn write_element(
    out: &mut String,
    indent: usize,
    kind: &NodeKind,
    size: Size,
    transform: Option<&Affine>,
    paint: &Paint,
    opacity: Option<f64>,
) {
    let (tag, is_rect) = match kind {
        NodeKind::Ellipse => ("ellipse", false),
        _ => ("rect", true),
    };
    let _ = write!(out, "{:indent$}<{tag}", "");

    let matrix = transform.and_then(|t| (!is_translation(t)).then(|| t.as_coeffs()));
    if is_rect {
        if let Some(t) = transform.filter(|t| is_translation(t)) {
            let [_, _, _, _, e, f] = t.as_coeffs();
            let x = num(e);
            let y = num(f);
            if x != "0" {
                let _ = write!(out, " x=\"{x}\"");
            }
            if y != "0" {
                let _ = write!(out, " y=\"{y}\"");
            }
        }
        let _ = write!(out, " width=\"{}\" height=\"{}\"", num(size.width), num(size.height));
    } else {
        let rx = size.width / 2.0;
        let ry = size.height / 2.0;
        let (cx, cy) = match transform {
            Some(t) if is_translation(t) => {
                let [_, _, _, _, e, f] = t.as_coeffs();
                (e + rx, f + ry)
            }
            _ => (rx, ry),
        };
        let _ = write!(out, " cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"", num(cx), num(cy), num(rx), num(ry));
    }

    if let Some([a, b, c, d, e, f]) = matrix {
        let _ = write!(
            out,
            " transform=\"matrix({} {} {} {} {} {})\"",
            num(a), num(b), num(c), num(d), num(e), num(f)
        );
    }

    let PaintKind::Solid { color } = paint.kind;
    let _ = write!(out, " fill=\"{}\"", String::from(color));
    if paint.opacity != 1.0 {
        let _ = write!(out, " fill-opacity=\"{}\"", num(paint.opacity));
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
    let fills: Vec<&Paint> = node.fills.iter().filter(|p| p.visible).collect();

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
        // A frame's own fills sit at its origin before any children.
        for paint in fills {
            write_element(&mut body, child_indent, &node.kind, node.size, None, paint, None);
        }
        for child in &node.children {
            write_node(child, child_indent, &mut body);
        }

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
        if fills.is_empty() {
            return;
        }
        if fills.len() == 1 {
            write_element(out, indent, &node.kind, node.size, Some(&node.transform), fills[0], Some(node.opacity));
        } else if node.opacity != 1.0 {
            let _ = writeln!(out, "{:indent$}<g opacity=\"{}\">", "", num(node.opacity));
            for paint in fills {
                write_element(out, indent + 2, &node.kind, node.size, Some(&node.transform), paint, None);
            }
            let _ = writeln!(out, "{:indent$}</g>", "");
        } else {
            for paint in fills {
                write_element(out, indent, &node.kind, node.size, Some(&node.transform), paint, None);
            }
        }
    }
}

/// `id` and everything in it as an SVG document, sized to the node.
pub fn write(document: &Document, id: NodeId) -> Result<String, Error> {
    let node = document.node(id).ok_or(Error::NoSuchNode(id))?;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
        num(node.size.width),
        num(node.size.height),
        num(node.size.width),
        num(node.size.height)
    );
    if node.visible {
        // The root's own visible fills sit at origin before its children.
        for paint in node.fills.iter().filter(|p| p.visible) {
            write_element(&mut out, 2, &node.kind, node.size, None, paint, None);
        }
        for child in &node.children {
            write_node(child, 2, &mut out);
        }
    }
    out.push_str("</svg>\n");
    Ok(out)
}
