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
