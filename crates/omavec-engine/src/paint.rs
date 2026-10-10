//! What a node is filled with.

use omavec_geom::stroke::{Align, Cap, Join, Style};
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
    /// Colours along a line from `from` to `to`, which are places in the
    /// node's box: (0, 0) is its top-left corner and (1, 1) its bottom-right.
    Linear { from: (f64, f64), to: (f64, f64), stops: Vec<Stop> },
    /// Colours outwards from `from` to the ellipse through `to`: a circle
    /// in the box's own terms, so as wide and as high as the box makes it.
    Radial { from: (f64, f64), to: (f64, f64), stops: Vec<Stop> },
}

/// A colour at a place along a gradient, from 0 at its start to 1 at its end.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    pub at: f64,
    pub color: Color,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f64,
}

impl PaintKind {
    /// The paint's one colour, or a gradient's first.
    pub fn color(&self) -> Color {
        match self {
            PaintKind::Solid { color } => *color,
            PaintKind::Linear { stops, .. } | PaintKind::Radial { stops, .. } => stops.first().map_or(Color::rgb(0, 0, 0), |stop| stop.color),
        }
    }

    /// A gradient's stops, in order along it.
    pub fn stops(&self) -> Option<Vec<Stop>> {
        let (PaintKind::Linear { stops, .. } | PaintKind::Radial { stops, .. }) = self else { return None };
        let mut stops = stops.clone();
        stops.sort_by(|a, b| a.at.total_cmp(&b.at));
        Some(stops)
    }
}

/// How a node's colours mix with what is under it. Figma's list, in its
/// order; the names in a file are CSS's for `mix-blend-mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Blend {
    #[default]
    Normal,
    Darken,
    Multiply,
    ColorBurn,
    Lighten,
    Screen,
    ColorDodge,
    Overlay,
    SoftLight,
    HardLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl Blend {
    pub const ALL: [Blend; 16] = [
        Blend::Normal,
        Blend::Darken,
        Blend::Multiply,
        Blend::ColorBurn,
        Blend::Lighten,
        Blend::Screen,
        Blend::ColorDodge,
        Blend::Overlay,
        Blend::SoftLight,
        Blend::HardLight,
        Blend::Difference,
        Blend::Exclusion,
        Blend::Hue,
        Blend::Saturation,
        Blend::Color,
        Blend::Luminosity,
    ];

    /// Its name in CSS and in a file.
    pub fn css(self) -> &'static str {
        match self {
            Blend::Normal => "normal",
            Blend::Darken => "darken",
            Blend::Multiply => "multiply",
            Blend::ColorBurn => "color-burn",
            Blend::Lighten => "lighten",
            Blend::Screen => "screen",
            Blend::ColorDodge => "color-dodge",
            Blend::Overlay => "overlay",
            Blend::SoftLight => "soft-light",
            Blend::HardLight => "hard-light",
            Blend::Difference => "difference",
            Blend::Exclusion => "exclusion",
            Blend::Hue => "hue",
            Blend::Saturation => "saturation",
            Blend::Color => "color",
            Blend::Luminosity => "luminosity",
        }
    }

    pub(crate) fn mix(self) -> peniko::Mix {
        match self {
            Blend::Normal => peniko::Mix::Normal,
            Blend::Darken => peniko::Mix::Darken,
            Blend::Multiply => peniko::Mix::Multiply,
            Blend::ColorBurn => peniko::Mix::ColorBurn,
            Blend::Lighten => peniko::Mix::Lighten,
            Blend::Screen => peniko::Mix::Screen,
            Blend::ColorDodge => peniko::Mix::ColorDodge,
            Blend::Overlay => peniko::Mix::Overlay,
            Blend::SoftLight => peniko::Mix::SoftLight,
            Blend::HardLight => peniko::Mix::HardLight,
            Blend::Difference => peniko::Mix::Difference,
            Blend::Exclusion => peniko::Mix::Exclusion,
            Blend::Hue => peniko::Mix::Hue,
            Blend::Saturation => peniko::Mix::Saturation,
            Blend::Color => peniko::Mix::Color,
            Blend::Luminosity => peniko::Mix::Luminosity,
        }
    }
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
    #[serde(default, skip_serializing_if = "is_default")]
    pub join: Join,
    /// How each end of an open path is finished.
    #[serde(default, skip_serializing_if = "is_default")]
    pub start_cap: Cap,
    #[serde(default, skip_serializing_if = "is_default")]
    pub end_cap: Cap,
}

pub(crate) fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

impl Default for Stroke {
    /// What Figma starts a stroke with, less the paint.
    fn default() -> Self {
        Self { paints: Vec::new(), weight: 1.0, align: Align::Inside, join: Join::Miter, start_cap: Cap::None, end_cap: Cap::None }
    }
}

impl Stroke {
    pub(crate) fn is_none(&self) -> bool {
        self.paints.is_empty()
    }

    /// The stroke's shape, without its paints.
    pub fn style(&self) -> Style {
        Style { weight: self.weight, align: self.align, join: self.join, start: self.start_cap, end: self.end_cap }
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
