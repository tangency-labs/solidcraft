//! Contracts and result types for the commands in `cmd/drawing_image.rs` (#5426, W38e).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::drawing_sheet::sheet_ref;

/// What `drawing.image.add` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct AddParams {
    /// The sheet it goes on, by id or name.
    #[serde(deserialize_with = "sheet_ref")]
    pub sheet: String,
    /// The PNG or JPEG file to read. Its bytes are saved in the design and its path isn't.
    pub file: String,
    /// Where it sits, `[x0, y0, x1, y1]` in millimetres from the sheet's lower-left corner.
    pub rect_mm: [f64; 4],
}

/// What `drawing.image.add` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AddResult {
    /// The placement's id.
    pub image: String,
    /// `png` or `jpeg`.
    pub format: String,
    /// Its width and height in pixels.
    pub pixels: [u32; 2],
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `drawing.title_block.logo` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct LogoParams {
    /// A text cell's name in a sheet's title block.
    pub cell: String,
    /// The PNG or JPEG file to show in place of the cell's text, or null for its text again. Its
    /// bytes are saved in the design and its path isn't.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(required)]
    pub file: Option<String>,
    /// The sheets, by id or name: every sheet whose block has the cell when not given.
    #[serde(default)]
    pub sheets: Option<Vec<String>>,
}

/// What `drawing.title_block.logo` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LogoResult {
    /// The sheets it set.
    pub sheets: Vec<String>,
    /// The image file the cell shows; null when it shows its text.
    pub image: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
