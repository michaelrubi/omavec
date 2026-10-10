//! The `.omavec` format: a folder of pretty-printed JSON, one file per page,
//! made to live in git.
//!
//! ```text
//! logo.omavec/
//!   document.json        format version, the pages in order, the next id
//!   pages/
//!     01-cover.json      one page's node tree
//! ```
//!
//! Saving writes the same bytes for the same document, touches only the
//! files that changed, and leaves alone whatever else is in the folder.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::document::{Document, Node, NodeId, NodeKind};

/// The format this build writes. It goes up whenever a file could hold
/// something an older build would drop.
pub const FORMAT: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{}: {source}", path.display())]
    Json { path: PathBuf, source: serde_json::Error },
    #[error("{} was saved by a newer Omavec (format {found}; this one reads up to {FORMAT})", path.display())]
    Newer { path: PathBuf, found: u32 },
    #[error("{}: {problem}", path.display())]
    Malformed { path: PathBuf, problem: String },
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: u32,
    next_id: u64,
    /// File names in `pages/`, in page order.
    pages: Vec<String>,
}

/// `name` as part of a file name: lower case, with a hyphen for each run of
/// anything that isn't a letter or digit.
fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() { "page".into() } else { slug.into() }
}

fn json<T: Serialize>(value: &T, path: &Path) -> Result<Vec<u8>, FileError> {
    let mut text = serde_json::to_vec_pretty(value).map_err(|source| FileError::Json { path: path.into(), source })?;
    text.push(b'\n');
    Ok(text)
}

/// Writes `bytes` to `path` unless that's what it holds already. The file
/// is replaced in one step, so a crash leaves the old one or the new one.
fn write(path: &Path, bytes: &[u8]) -> Result<(), FileError> {
    let io = |source| FileError::Io { path: path.into(), source };
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, bytes).map_err(io)?;
    std::fs::rename(&partial, path).map_err(io)
}

pub fn save(document: &Document, folder: &Path) -> Result<(), FileError> {
    let pages = folder.join("pages");
    std::fs::create_dir_all(&pages).map_err(|source| FileError::Io { path: pages.clone(), source })?;
    let mut names = Vec::new();
    for (index, page) in document.pages.iter().enumerate() {
        let name = format!("{:02}-{}.json", index + 1, slug(&page.name));
        let path = pages.join(&name);
        write(&path, &json(&**page, &path)?)?;
        names.push(name);
    }
    // The manifest goes last: until it's written, the folder still opens as
    // the document it was.
    let manifest = folder.join("document.json");
    write(&manifest, &json(&Manifest { format: FORMAT, next_id: document.next_id(), pages: names.clone() }, &manifest)?)?;
    // Pages that were deleted or renamed leave files behind.
    let io = |source| FileError::Io { path: pages.clone(), source };
    for entry in std::fs::read_dir(&pages).map_err(io)? {
        let entry = entry.map_err(io)?;
        let name = entry.file_name();
        let stale = name.to_str().is_some_and(|name| name.ends_with(".json") && !names.iter().any(|kept| kept == name));
        if stale {
            std::fs::remove_file(entry.path()).map_err(|source| FileError::Io { path: entry.path(), source })?;
        }
    }
    Ok(())
}

fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, FileError> {
    let bytes = std::fs::read(path).map_err(|source| FileError::Io { path: path.into(), source })?;
    serde_json::from_slice(&bytes).map_err(|source| FileError::Json { path: path.into(), source })
}

pub fn open(folder: &Path) -> Result<Document, FileError> {
    let path = folder.join("document.json");
    // Only the version at first: a newer format's manifest may not read as ours.
    #[derive(Deserialize)]
    struct Version {
        format: u32,
    }
    let Version { format } = read(&path)?;
    if format > FORMAT {
        return Err(FileError::Newer { path, found: format });
    }
    let manifest: Manifest = read(&path)?;
    let mut pages = Vec::new();
    let mut seen = HashSet::new();
    for name in &manifest.pages {
        // A page is a file in `pages/`, not a path to anywhere else.
        if name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(FileError::Malformed { path, problem: format!("\"{name}\" is not a page file name") });
        }
        let path = folder.join("pages").join(name);
        let page: Node = read(&path)?;
        if page.kind != NodeKind::Page {
            return Err(FileError::Malformed { path, problem: "the top node is not a page".into() });
        }
        if let Err(problem) = check(&page, &mut seen) {
            return Err(FileError::Malformed { path, problem });
        }
        pages.push(Arc::new(page));
    }
    if pages.is_empty() {
        return Err(FileError::Malformed { path, problem: "the document has no pages".into() });
    }
    Ok(Document::from_parts(pages, manifest.next_id))
}

/// What a merge in git can leave behind: two nodes with one id, or a page
/// pasted inside another.
fn check(node: &Node, seen: &mut HashSet<NodeId>) -> Result<(), String> {
    if !seen.insert(node.id) {
        return Err(format!("id {} is used twice in the document", node.id.0));
    }
    for child in &node.children {
        if child.kind == NodeKind::Page {
            return Err(format!("node {} is a page inside node {}", child.id.0, node.id.0));
        }
        check(child, seen)?;
    }
    Ok(())
}
