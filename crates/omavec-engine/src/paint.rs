//! What a node is filled with.

use omavec_geom::stroke::Align;
use serde::{Deserialize, Serialize};

/// An sRGB colour. In a file it is `"#rrggbb"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

impl From<Color> for String {
    fn from(color: Color) -> Self {
        format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
    }
}

impl TryFrom<String> for Color {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        let channel = |at: usize| text.get(at..at + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (text.len(), text.starts_with('#'), channel(1), channel(3), channel(5)) {
            (7, true, Some(r), Some(g), Some(b)) => Ok(Self { r, g, b }),
            _ => Err(format!("\"{text}\" is not a colour like #1e1e2e")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaintKind {
    Solid { color: Color },
}

/// One layer of a node's fill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paint {
    #[serde(flatten)]
    pub kind: PaintKind,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f64,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub visible: bool,
}

impl Paint {
    pub fn solid(color: Color) -> Self {
        Self { kind: PaintKind::Solid { color }, opacity: 1.0, visible: true }
    }
}

/// A node's stroke: what it is painted with, and how wide and on which
/// side of the edge. With no paints there is no stroke.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    /// Bottom to top, like fills.
    pub paints: Vec<Paint>,
    pub weight: f64,
    pub align: Align,
}

impl Default for Stroke {
    /// What Figma starts a stroke with, less the paint.
    fn default() -> Self {
        Self { paints: Vec::new(), weight: 1.0, align: Align::Inside }
    }
}

impl Stroke {
    pub(crate) fn is_none(&self) -> bool {
        self.paints.is_empty()
    }
}

// What serde leaves out of a file and fills back in: see `Node`.

pub(crate) fn yes() -> bool {
    true
}

pub(crate) fn is_yes(value: &bool) -> bool {
    *value
}

pub(crate) fn is_no(value: &bool) -> bool {
    !*value
}

pub(crate) fn one() -> f64 {
    1.0
}

pub(crate) fn is_one(value: &f64) -> bool {
    *value == 1.0
}
