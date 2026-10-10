//! Undo and redo. A step keeps the document as it was before it, which costs
//! little: documents share every node an edit didn't touch.

use std::sync::Arc;

use crate::document::{Document, Error};

struct Step {
    name: String,
    document: Document,
    revision: u64,
}

pub struct History {
    document: Document,
    /// Identifies this state of the document. An undo brings the old number
    /// back with the old state.
    revision: u64,
    /// The last revision number handed out.
    revisions: u64,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// The revision on disk, or the one a new document started at: either
    /// way, the state with nothing to lose.
    saved: u64,
    /// The document before the drag in progress, while there is one.
    gesture: Option<Step>,
}

/// Whether nothing in `a` was touched to make `b`: the same pages, by
/// pointer. An edit always copies the page it changes.
fn untouched(a: &Document, b: &Document) -> bool {
    a.pages.len() == b.pages.len() && a.pages.iter().zip(&b.pages).all(|(a, b)| Arc::ptr_eq(a, b))
}

impl History {
    pub fn new(document: Document) -> Self {
        Self { document, revision: 0, revisions: 0, undo: Vec::new(), redo: Vec::new(), saved: 0, gesture: None }
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Changes with every change to the document, and goes back with an
    /// undo: whoever draws or saves the document compares it to know.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Makes one change, as one undo step named `name`. If `change` fails
    /// the document is left as it was and nothing is recorded. During a
    /// gesture (see [`Self::begin`]) the change joins the gesture's step.
    pub fn edit<T>(&mut self, name: &str, change: impl FnOnce(&mut Document) -> Result<T, Error>) -> Result<T, Error> {
        let before = self.document.clone();
        let revision = self.revision;
        match change(&mut self.document) {
            Ok(value) => {
                if !untouched(&before, &self.document) {
                    self.revisions += 1;
                    self.revision = self.revisions;
                    if self.gesture.is_none() {
                        self.undo.push(Step { name: name.into(), document: before, revision });
                        self.redo.clear();
                    }
                }
                Ok(value)
            }
            Err(error) => {
                self.document = before;
                Err(error)
            }
        }
    }

    /// Starts a gesture, such as a drag: every edit until [`Self::commit`]
    /// is one undo step named `name`. A gesture already open is committed.
    pub fn begin(&mut self, name: &str) {
        self.commit();
        self.gesture = Some(Step { name: name.into(), document: self.document.clone(), revision: self.revision });
    }

    /// Ends the gesture, recording it if it changed anything.
    pub fn commit(&mut self) {
        if let Some(step) = self.gesture.take()
            && step.revision != self.revision
        {
            self.undo.push(step);
            self.redo.clear();
        }
    }

    /// Ends the gesture and puts the document back as it was before it.
    pub fn cancel(&mut self) {
        if let Some(step) = self.gesture.take() {
            (self.document, self.revision) = (step.document, step.revision);
        }
    }

    /// The name of the step Undo would undo.
    pub fn undo_name(&self) -> Option<&str> {
        self.undo.last().map(|step| step.name.as_str())
    }

    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|step| step.name.as_str())
    }

    /// Undoes the last step; `false` if there was none. A gesture in
    /// progress is committed first, so it is what gets undone.
    pub fn undo(&mut self) -> bool {
        self.commit();
        Self::step(&mut self.undo, &mut self.redo, &mut self.document, &mut self.revision)
    }

    pub fn redo(&mut self) -> bool {
        self.commit();
        Self::step(&mut self.redo, &mut self.undo, &mut self.document, &mut self.revision)
    }

    /// Swaps the document with the top of `from`, leaving the way back on `to`.
    fn step(from: &mut Vec<Step>, to: &mut Vec<Step>, document: &mut Document, revision: &mut u64) -> bool {
        let Some(mut step) = from.pop() else { return false };
        std::mem::swap(document, &mut step.document);
        std::mem::swap(revision, &mut step.revision);
        to.push(step);
        true
    }

    /// Notes that the document as it is now is what's on disk.
    pub fn mark_saved(&mut self) {
        self.saved = self.revision;
    }

    /// Whether the document has changes that aren't on disk. A new document
    /// has none until it's edited, and undoing back to the saved state makes
    /// it clean again.
    pub fn is_dirty(&self) -> bool {
        self.saved != self.revision
    }
}
