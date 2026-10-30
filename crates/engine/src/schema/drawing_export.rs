//! Contracts and result types for `drawing.export` (`cmd/drawing_export.rs`, #4890 A7).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::drawing_scene::SheetSceneView;
use super::drawing_sheet::sheet_ref;

/// Write one sheet of the open drawing to a file, or return it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ExportParams {
    /// The sheet, by id or name.
    #[serde(deserialize_with = "sheet_ref")]
    pub sheet: String,
    /// The file format (svg when not given). DXF is not supported yet.
    #[serde(default)]
    pub format: ExportFormat,
    /// The file to write, ending in the format's extension (the reply holds the document when
    /// not given).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub path: Option<String>,
    /// PNG only: its resolution in dots per inch, 25 to 2400 (the sheet's own when not given).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub dpi: Option<f64>,
    /// PNG only: how it stores its pixels (grey when not given).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ExportColour")]
    pub colour: Option<ExportColour>,
}

/// A sheet export's file format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    #[default]
    Svg,
    Png,
    Dxf,
}

impl ExportFormat {
    /// The extension a file of this format ends in, which a path must have.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Svg => "svg",
            ExportFormat::Png => "png",
            ExportFormat::Dxf => "dxf",
        }
    }
}

/// How a sheet PNG stores its pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportColour {
    /// 8-bit greyscale on white paper.
    #[default]
    Grey,
    /// One bit a pixel, black or white, every line at least a pixel wide.
    BlackWhite,
    /// 24-bit colour on white paper.
    Rgb,
    /// 32-bit colour, the paper transparent.
    Transparent,
}

impl From<ExportColour> for solidcraft_io::paper::PngColour {
    fn from(colour: ExportColour) -> Self {
        match colour {
            ExportColour::Grey => Self::Grey,
            ExportColour::BlackWhite => Self::BlackWhite,
            ExportColour::Rgb => Self::Rgb,
            ExportColour::Transparent => Self::Transparent,
        }
    }
}

/// What a writer did that the scene didn't say.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportReport {
    /// Stroke widths under half a nanometre, written as one nanometre.
    pub widths_snapped: usize,
    /// Pieces too short for the nanometre grid, merged into the next.
    pub pieces_merged: usize,
    /// A PNG's width and height in pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pixels: Option<[u32; 2]>,
}

/// What `drawing.export` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportResult {
    pub sheet: String,
    pub format: ExportFormat,
    /// The document's size in bytes.
    pub bytes: usize,
    /// The file written, when a path was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The document when no path was given and it is text (SVG).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The document as base64 when no path was given and it is binary (PNG).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    pub report: ExportReport,
    /// Each view on the sheet, in id order, as `drawing.scene` reports it.
    pub views: Vec<SheetSceneView>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
