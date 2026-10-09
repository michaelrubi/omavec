//! Best-effort `.fig` importer: kiwi decoding into engine nodes, with a
//! report of what could not be converted.

mod decode;
pub use decode::{FigError, decode, tree};
