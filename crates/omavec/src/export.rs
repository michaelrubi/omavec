//! Writing nodes out as SVG and PNG files, for the Export command and for
//! `omavec export` alike. The PNG is drawn by the canvas's own renderer.

use std::path::{Path, PathBuf};

use omavec_engine::display::DisplayList;
use omavec_engine::{Document, Export, NodeId, svg};
use omavec_geom::kurbo::Affine;
use omavec_render::{Renderer, peniko};

/// `id` as the bytes of a file of the kind `export` says: the node alone,
/// upright, with the corner of its box in the picture's.
pub fn render(document: &Document, id: NodeId, export: Export) -> Result<Vec<u8>, String> {
    let node = document.node(id).ok_or_else(|| format!("there is no node {id:?}"))?;
    match export {
        Export::Svg => svg::write(document, id).map(String::into_bytes).map_err(|error| error.to_string()),
        Export::Png { scale } => {
            let area = node.bounds();
            let pixels = |length: f64| {
                let pixels = (length * scale).ceil();
                (1.0..=f64::from(u16::MAX)).contains(&pixels).then_some(pixels as u16).ok_or(format!("{} at {scale}x is {pixels} pixels: too big, or nothing", node.name))
            };
            let (width, height) = (pixels(area.width())?, pixels(area.height())?);
            // The list has the node where its parent puts it: take that away.
            let view = Affine::scale(scale) * Affine::translate(-area.origin().to_vec2()) * node.transform.inverse();
            let drawn = Renderer::default().render(&DisplayList::of(node), view, width, height, peniko::Color::TRANSPARENT);
            drawn.into_png().map_err(|error| error.to_string())
        }
    }
}

/// Writes each of `ids` into `folder`, once for each of `formats`, or if
/// there are none, for each of the node's own export settings, or failing
/// those as a PNG. Returns the files, in order. Everything is drawn before
/// anything is written, so an export that can't be done leaves no files.
pub fn write(document: &Document, ids: &[NodeId], formats: &[Export], folder: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for id in ids {
        let node = document.node(*id).ok_or_else(|| format!("there is no node {id:?}"))?;
        let own = if node.exports.is_empty() { &[Export::PNG][..] } else { &node.exports };
        for export in if formats.is_empty() { own } else { formats } {
            let path = folder.join(export.file_name(&node.name));
            // Two nodes of one name don't write over each other.
            if !files.iter().any(|(written, _)| *written == path) {
                files.push((path, render(document, *id, *export)?));
            }
        }
    }
    let io = |path: &Path, error: std::io::Error| format!("{}: {error}", path.display());
    std::fs::create_dir_all(folder).map_err(|error| io(folder, error))?;
    for (path, bytes) in &files {
        std::fs::write(path, bytes).map_err(|error| io(path, error))?;
    }
    Ok(files.into_iter().map(|(path, _)| path).collect())
}
