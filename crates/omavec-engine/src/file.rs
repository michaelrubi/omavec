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
//!
//! `logo.omavecz` is the same files in a zip, for sending to someone or
//! opening from a file manager.

use std::collections::HashSet;
use std::io::{Read, Write};
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
    #[error("{}: {source}", path.display())]
    Zip { path: PathBuf, source: zip::result::ZipError },
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
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    std::fs::write(&partial, bytes).map_err(io)?;
    std::fs::rename(&partial, path).map_err(io)
}

/// The document's files, by their place in its folder. The manifest is
/// last: a folder is still the old document until it is written.
fn files(document: &Document) -> Result<Vec<(String, Vec<u8>)>, FileError> {
    let (mut files, mut names) = (Vec::new(), Vec::new());
    for (index, page) in document.pages.iter().enumerate() {
        let name = format!("{:02}-{}.json", index + 1, slug(&page.name));
        let place = format!("pages/{name}");
        files.push((place.clone(), json(&**page, Path::new(&place))?));
        names.push(name);
    }
    let manifest = Manifest { format: FORMAT, next_id: document.next_id(), pages: names };
    files.push(("document.json".into(), json(&manifest, Path::new("document.json"))?));
    Ok(files)
}

/// Whether `path` names the zipped form of a document.
fn zipped(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "omavecz")
}

/// Saves `document` to `path`: a `.omavec` folder, or, if that is how the
/// path ends, a `.omavecz` file, which is the same folder zipped.
pub fn save(document: &Document, path: &Path) -> Result<(), FileError> {
    let files = files(document)?;
    if zipped(path) {
        return write(path, &zip(&files).map_err(|source| FileError::Zip { path: path.into(), source })?);
    }
    let pages = path.join("pages");
    std::fs::create_dir_all(&pages).map_err(|source| FileError::Io { path: pages.clone(), source })?;
    for (place, bytes) in &files {
        write(&path.join(place), bytes)?;
    }
    // Pages that were deleted or renamed leave files behind.
    let io = |source| FileError::Io { path: pages.clone(), source };
    for entry in std::fs::read_dir(&pages).map_err(io)? {
        let entry = entry.map_err(io)?;
        let name = entry.file_name();
        let stale = name.to_str().is_some_and(|name| name.ends_with(".json") && !files.iter().any(|(place, _)| place.strip_prefix("pages/") == Some(name)));
        if stale {
            std::fs::remove_file(entry.path()).map_err(|source| FileError::Io { path: entry.path(), source })?;
        }
    }
    Ok(())
}

/// `files` as a zip archive. The same files give the same bytes: nothing in
/// it depends on when it was made.
fn zip(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, zip::result::ZipError> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).last_modified_time(zip::DateTime::default());
    for (place, bytes) in files {
        archive.start_file(place.as_str(), options)?;
        archive.write_all(bytes)?;
    }
    Ok(archive.finish()?.into_inner())
}

/// Opens the document at `path`: a `.omavec` folder or a `.omavecz` file.
pub fn open(path: &Path) -> Result<Document, FileError> {
    if path.is_dir() {
        return read_document(path, &|place| std::fs::read(path.join(place)).map_err(|source| FileError::Io { path: path.join(place), source }));
    }
    let bytes = std::fs::read(path).map_err(|source| FileError::Io { path: path.into(), source })?;
    let archive = std::cell::RefCell::new(zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|source| FileError::Zip { path: path.into(), source })?);
    read_document(path, &|place| {
        let mut bytes = Vec::new();
        let zip = |source| FileError::Zip { path: path.join(place), source };
        archive.borrow_mut().by_name(place).map_err(zip)?.read_to_end(&mut bytes).map_err(|source| zip(source.into()))?;
        Ok(bytes)
    })
}

/// Reads a document whose files `read` fetches by their place in it. `root`
/// is only for saying where a problem is.
fn read_document(root: &Path, read: &dyn Fn(&str) -> Result<Vec<u8>, FileError>) -> Result<Document, FileError> {
    fn parse<T: for<'de> Deserialize<'de>>(path: &Path, bytes: &[u8]) -> Result<T, FileError> {
        serde_json::from_slice(bytes).map_err(|source| FileError::Json { path: path.into(), source })
    }
    let path = root.join("document.json");
    let manifest = read("document.json")?;
    // Only the version at first: a newer format's manifest may not read as ours.
    #[derive(Deserialize)]
    struct Version {
        format: u32,
    }
    let Version { format } = parse(&path, &manifest)?;
    if format > FORMAT {
        return Err(FileError::Newer { path, found: format });
    }
    let manifest: Manifest = parse(&path, &manifest)?;
    let mut pages = Vec::new();
    let mut seen = HashSet::new();
    for name in &manifest.pages {
        // A page is a file in `pages/`, not a path to anywhere else.
        if name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(FileError::Malformed { path, problem: format!("\"{name}\" is not a page file name") });
        }
        let place = format!("pages/{name}");
        let path = root.join(&place);
        let page: Node = parse(&path, &read(&place)?)?;
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
