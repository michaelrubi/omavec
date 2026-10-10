//! The document: node tree, paints, layout, text, components, variables,
//! undo, the `.omavec` format and SVG. No UI or GPU code, so all of it is
//! tested headless.
// Shipped code returns errors; only tests may panic.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod arrange;
pub mod display;
mod document;
pub mod file;
mod history;
mod paint;
pub mod svg;

pub use arrange::Stack;
pub use document::{Document, Error, Node, NodeId, NodeKind};
pub use history::History;
pub use omavec_geom::stroke::{Align, Cap, Join};
pub use paint::{Color, Paint, PaintKind, Stroke};
