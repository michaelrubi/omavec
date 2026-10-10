//! Export settings: what a node is exported as. They are kept on the node,
//! as in Figma, so a document says for itself what it produces.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum Export {
    Svg,
    /// PNG at this many pixels per document unit.
    Png { scale: f64 },
}

impl Export {
    /// What is exported when nothing says otherwise.
    pub const PNG: Export = Export::Png { scale: 1.0 };

    /// `svg`, `png`, or `png@2x`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let scale = |text: &str| text.strip_suffix('x').and_then(|scale| scale.parse::<f64>().ok()).filter(|scale| *scale > 0.0 && scale.is_finite());
        match text.split_once('@') {
            None if text == "svg" => Ok(Export::Svg),
            None if text == "png" => Ok(Export::PNG),
            Some(("png", size)) => scale(size).map(|scale| Export::Png { scale }).ok_or(format!("\"{text}\" is not a size like png@2x")),
            _ => Err(format!("\"{text}\" is not a format: use svg, png or png@2x")),
        }
    }

    /// The file a node called `name` goes to, as Figma names its exports.
    pub fn file_name(self, name: &str) -> String {
        // A name is not a path.
        let name = name.replace(['/', '\\'], "-");
        match self {
            Export::Svg => format!("{name}.svg"),
            Export::Png { scale: 1.0 } => format!("{name}.png"),
            Export::Png { scale } => format!("{name}@{scale}x.png"),
        }
    }
}
