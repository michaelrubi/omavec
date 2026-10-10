//! Best-effort `.fig` importer: kiwi decoding into engine nodes, with a
//! report of what could not be converted.
// Shipped code returns errors; only tests may panic.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod decode;
pub use decode::{FigError, decode, tree};
