//! Crash recovery: while a document has unsaved changes, a copy of it is
//! written every so often to `~/.local/state/omavec/recovery/`, and taken
//! away again when the changes are saved or given up. A copy still there
//! when Omavec starts was left by a session that never got that far.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use omavec_engine::{Document, History, file};

/// How long changes can go without a copy being made.
pub const EVERY: Duration = Duration::from_secs(30);

pub struct Recovery {
    /// Where copies are kept; `None` keeps none (in tests).
    folder: Option<PathBuf>,
    /// This session's process id, which names its copy.
    session: u32,
    /// The revision of the document last copied.
    copied: Option<u64>,
    /// When the next copy may be made.
    due: Instant,
}

/// A copy left behind by a session that is no longer running.
#[derive(Clone, Debug, PartialEq)]
pub struct Left {
    /// The copy itself: a `.omavecz`.
    pub copy: PathBuf,
    /// Where the document it is a copy of was saved, if it ever was.
    pub path: Option<PathBuf>,
}

impl Left {
    pub fn name(&self) -> String {
        self.path.as_deref().and_then(Path::file_stem).map_or_else(|| "Untitled".into(), |name| name.to_string_lossy().into_owned())
    }

    /// The document as it was when the copy was made.
    pub fn open(&self) -> Result<Document, file::FileError> {
        file::open(&self.copy)
    }

    /// Throws the copy away.
    pub fn discard(&self) {
        let _ = std::fs::remove_file(&self.copy);
        let _ = std::fs::remove_file(self.copy.with_extension("path"));
    }
}

/// Whether a session of Omavec with this process id is running.
fn running(session: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{session}/comm")).is_ok_and(|name| name.trim() == "omavec")
}

impl Recovery {
    pub fn new(folder: Option<PathBuf>, session: u32) -> Self {
        Self { folder, session, copied: None, due: Instant::now() + EVERY }
    }

    fn copy(&self) -> Option<PathBuf> {
        Some(self.folder.as_ref()?.join(format!("{}.omavecz", self.session)))
    }

    /// Makes a copy of the document if it has changes that aren't saved,
    /// none of which the last copy has, and it is time. `path` is where
    /// the document is saved, to be put back with it.
    pub fn keep(&mut self, history: &History, path: Option<&Path>, now: Instant) {
        let Some(copy) = self.copy() else { return };
        if !history.is_dirty() || self.copied == Some(history.revision()) || now < self.due {
            return;
        }
        (self.copied, self.due) = (Some(history.revision()), now + EVERY);
        // Off the UI thread: the snapshot shares the document's nodes, and
        // a big document takes a moment to write.
        let (document, path) = (history.document().clone(), path.map(Path::to_path_buf));
        std::thread::spawn(move || {
            let write = || -> Result<(), String> {
                if let Some(folder) = copy.parent() {
                    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
                }
                file::save(&document, &copy).map_err(|error| error.to_string())?;
                let Some(path) = path else { return Ok(()) };
                // Whole or not at all, for whoever comes looking.
                let (note, partial) = (copy.with_extension("path"), copy.with_extension("path.partial"));
                std::fs::write(&partial, path.to_string_lossy().as_bytes()).and_then(|()| std::fs::rename(&partial, &note)).map_err(|error| error.to_string())
            };
            if let Err(error) = write() {
                log::warn!("can't keep a recovery copy in {}: {error}", copy.display());
            }
        });
    }

    /// Takes this session's copy away: its changes are saved, or given up.
    pub fn clear(&mut self) {
        if let (Some(copy), Some(_)) = (self.copy(), self.copied.take()) {
            Left { copy, path: None }.discard();
        }
    }

    /// The copies left by sessions that are gone, newest first.
    pub fn left(&self) -> Vec<Left> {
        let Some(folder) = &self.folder else { return Vec::new() };
        let mut found: Vec<(std::time::SystemTime, Left)> = Vec::new();
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let copy = entry.path();
            let session = copy.file_stem().and_then(|stem| stem.to_str()).and_then(|stem| stem.parse::<u32>().ok());
            let gone = session.is_some_and(|session| session != self.session && !running(session));
            if gone && copy.extension().is_some_and(|extension| extension == "omavecz") {
                let path = std::fs::read_to_string(copy.with_extension("path")).ok().map(PathBuf::from);
                let written = entry.metadata().and_then(|metadata| metadata.modified()).unwrap_or(std::time::UNIX_EPOCH);
                found.push((written, Left { copy, path }));
            }
        }
        found.sort_by_key(|(written, _)| std::cmp::Reverse(*written));
        found.into_iter().map(|(_, left)| left).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omavec_engine::NodeKind;
    use omavec_geom::kurbo::Size;

    /// Waits for `path` to be there, as the copy is written on a thread.
    fn wait_for(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            assert!(Instant::now() < deadline, "{} was never written", path.display());
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn unsaved_changes_are_copied_and_the_copy_goes_when_they_are_saved() {
        let folder = std::env::temp_dir().join(format!("omavec-recovery-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        // No session has this id, so what it leaves is taken for a crash's.
        let dead = u32::MAX - 7;
        let mut recovery = Recovery::new(Some(folder.clone()), dead);
        let mut history = History::new(Document::default());
        let start = Instant::now();
        let later = start + EVERY * 2;
        let copy = folder.join(format!("{dead}.omavecz"));

        // Nothing to lose: no copy, however long it has been.
        recovery.keep(&history, None, later);
        std::thread::sleep(Duration::from_millis(20));
        assert!(!copy.exists());

        let draw = |history: &mut History| {
            history.edit("Draw", |document| {
                let page = document.pages[0].id;
                let node = document.create(NodeKind::Rectangle, Size::new(10.0, 10.0));
                document.insert(page, usize::MAX, node)
            })
        };
        draw(&mut history).unwrap();
        // Too soon after starting; then it is time.
        recovery.keep(&history, None, start);
        assert!(recovery.copied.is_none());
        recovery.keep(&history, Some(Path::new("/somewhere/Logo.omavec")), later);
        wait_for(&copy.with_extension("path"));
        // Another session finds it, with where it belongs, and can open it.
        let other = Recovery::new(Some(folder.clone()), dead - 1);
        let [left] = &other.left()[..] else { panic!("{:?}", other.left()) };
        assert_eq!((left.name().as_str(), left.path.as_deref()), ("Logo", Some(Path::new("/somewhere/Logo.omavec"))));
        assert_eq!(&left.open().unwrap(), history.document());
        // The session that made it doesn't take its own for a crash's.
        assert!(recovery.left().is_empty());

        // The same changes aren't copied twice; new ones wait their turn.
        recovery.keep(&history, None, later + EVERY * 2);
        assert_eq!(recovery.due, later + EVERY);
        draw(&mut history).unwrap();
        recovery.keep(&history, None, later + EVERY / 2);
        assert_eq!(recovery.copied, Some(1));

        // Saved, or given up: the copy goes.
        recovery.clear();
        assert!(!copy.exists() && !copy.with_extension("path").exists() && other.left().is_empty());
        let _ = std::fs::remove_dir_all(folder);
    }
}
