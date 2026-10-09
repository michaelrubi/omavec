//! Curve maths: vector networks, booleans, offsets, stroke expansion, width
//! profiles and warps. Knows nothing about documents, and has no UI or GPU
//! code.

pub mod network;

pub use kurbo;
pub use vectorcraft_geom::FillRule;
