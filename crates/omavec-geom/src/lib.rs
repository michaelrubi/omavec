//! Curve maths: vector networks, booleans, offsets, stroke expansion, width
//! profiles and warps. Knows nothing about documents, and has no UI or GPU
//! code.
// Shipped code returns errors; only tests may panic.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod network;

pub use kurbo;
pub use vectorcraft_geom::FillRule;
