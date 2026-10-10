//! The clipboard: the last cut or copy as nodes, for pasting in Omavec, and
//! as SVG on the Wayland clipboard (through `wl-copy`) for other apps.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;

use omavec_engine::{Document, Node, NodeId, NodeKind, svg};

#[derive(Default)]
pub struct Clipboard {
    /// What [`Document::copy`] gave: nodes placed on their page.
    nodes: Vec<Arc<Node>>,
    /// Whether other apps get a copy too; off in tests.
    pub system: bool,
}

impl Clipboard {
    /// Puts `ids` on the clipboard; `false` if that is nothing.
    pub fn copy(&mut self, document: &Document, ids: &[NodeId]) -> bool {
        let nodes = document.copy(ids);
        if nodes.is_empty() {
            return false;
        }
        self.nodes = nodes;
        if self.system
            && let Some(svg) = svg_of(document, ids)
        {
            // wl-copy can take a moment, and may not be there at all.
            std::thread::spawn(move || {
                if let Err(error) = offer(&svg) {
                    log::warn!("can't copy to the system clipboard: {error}");
                }
            });
        }
        true
    }

    pub fn nodes(&self) -> &[Arc<Node>] {
        &self.nodes
    }
}

/// `ids` as one SVG document the size of the box round them.
pub fn svg_of(document: &Document, ids: &[NodeId]) -> Option<String> {
    // A group round them has that box, and keeps each where it is in it.
    let mut scratch = document.clone();
    let group = scratch.group(ids, NodeKind::Group).ok()?;
    svg::write(&scratch, group).ok()
}

/// Hands `svg` to wl-copy, which goes on serving it after this returns.
fn offer(svg: &str) -> std::io::Result<()> {
    let mut child = Command::new("wl-copy").args(["--type", "image/svg+xml"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    // Dropping stdin closes it, so wl-copy knows it has everything.
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(svg.as_bytes())?;
    }
    child.wait().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use omavec_geom::kurbo::{Affine, Size};

    #[test]
    fn a_copy_is_one_svg_the_size_of_what_was_copied() {
        let mut document = Document::default();
        let page = document.pages[0].id;
        let mut add = |kind, x: f64, y: f64| {
            let mut node = document.create(kind, Size::new(40.0, 20.0));
            node.transform = Affine::translate((x, y));
            let id = node.id;
            document.insert(page, usize::MAX, node).unwrap();
            id
        };
        let (rectangle, ellipse) = (add(NodeKind::Rectangle, 100.0, 100.0), add(NodeKind::Ellipse, 160.0, 130.0));
        let svg = svg_of(&document, &[rectangle, ellipse]).unwrap();
        assert_eq!(
            svg,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50">
  <rect width="40" height="20" fill="#d9d9d9"/>
  <ellipse cx="80" cy="40" rx="20" ry="10" fill="#d9d9d9"/>
</svg>
"##
        );
        // The document itself is as it was.
        assert_eq!(document.node(page).unwrap().children.len(), 2);
        assert_eq!(svg_of(&document, &[]), None);

        let mut clipboard = Clipboard::default();
        assert!(!clipboard.copy(&document, &[]));
        assert!(clipboard.copy(&document, &[ellipse]));
        assert_eq!(clipboard.nodes().len(), 1);
        // Nothing to copy leaves the last copy there.
        assert!(!clipboard.copy(&document, &[]));
        assert_eq!(clipboard.nodes().len(), 1);
    }
}
