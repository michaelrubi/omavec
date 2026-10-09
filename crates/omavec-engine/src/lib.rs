//! The document: node tree, paints, layout, text, components, variables,
//! undo, the `.omavec` format and SVG. No UI or GPU code, so all of it is
//! tested headless.
// Shipped code returns errors; only tests may panic.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod document;
mod history;

pub use document::{Document, Error, Node, NodeId, NodeKind};
pub use history::History;
