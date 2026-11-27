//! Contracts and result types for `drawing.export` (`cmd/drawing_export.rs`, #4890 A7).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::drawing_scene::{
    SheetAnnotationFailure, SheetSceneBalloon, SheetSceneDimension, SheetSceneFill, SheetSceneNote, SheetSceneSketch, SheetSceneTable, SheetSceneView,
};
use super::drawing_sheet::sheet_ref;

/// Write one sheet of the open drawing to a file, or return it; a PDF can hold several sheets.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ExportParams {
    /// The sheet, by id or name. A PDF given neither this nor `sheets` holds every sheet.
    #[serde(default, deserialize_with = "some_sheet", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub sheet: Option<String>,
    /// PDF only: the sheets, by id or name, one page each in this order.
    #[serde(default, deserialize_with = "some_sheets", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<String>")]
    pub sheets: Option<Vec<String>>,
    /// The file format (svg when not given).
    #[serde(default)]
    pub format: ExportFormat,
    /// The file to write, ending in the format's extension. Required for a PNG or PDF; an SVG's or
    /// DXF's text is in the reply when not given.
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
    /// The paper (white when not given), which the lines, dimensions and text suit. A blue paper's
    /// PNG is rgb.
    #[serde(default)]
    pub paper: ExportPaper,
    /// PDF only: how its text is written (text when not given).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ExportText")]
    pub text: Option<ExportText>,
}

/// Print the whole open drawing: its PDF, written to a file or returned.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct PrintParams {
    /// The .pdf file to write. The app's own print takes the PDF's bytes in-process instead.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub path: Option<String>,
}

/// [`sheet_ref`], for a key that may be absent (`null` is refused).
fn some_sheet<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    sheet_ref(d).map(Some)
}

/// A list of [`sheet_ref`]s, for a key that may be absent (`null` is refused).
fn some_sheets<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                serde_json::Value::String(s) => Ok(s),
                _ => Err(serde::de::Error::custom("each must be a sheet's id or name")),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        _ => Err(serde::de::Error::custom("must be a list of sheets' ids or names")),
    }
}

/// A sheet export's file format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    #[default]
    Svg,
    Png,
    /// Vector, at the sheet's paper size, one page a sheet.
    Pdf,
    /// Lines in millimetres, a layer for each kind of line; pictures left out.
    Dxf,
}

impl ExportFormat {
    /// The extension a file of this format ends in, which a path must have.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Svg => "svg",
            ExportFormat::Png => "png",
            ExportFormat::Pdf => "pdf",
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

/// How a sheet PDF carries its text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportText {
    /// Real text in an embedded font, with a map back to Unicode: it looks as outlines do, and a
    /// viewer selects, searches and copies it.
    #[default]
    Text,
    /// Glyph outlines only, as earlier versions wrote: the file holds no text.
    Outlines,
}

impl From<ExportText> for solidcraft_io::paper::PdfText {
    fn from(text: ExportText) -> Self {
        match text {
            ExportText::Text => Self::Text,
            ExportText::Outlines => Self::Outlines,
        }
    }
}

/// The paper a sheet is exported on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportPaper {
    /// White, or in an SVG no background at all.
    #[default]
    White,
    /// Blue paper, `#123A76`, with `#E2EEFC` lines.
    Blueprint,
    /// Prussian blue paper, `#003153`, with `#E2EEFC` lines.
    PrussianBlue,
}

impl From<ExportPaper> for Option<solidcraft_io::paper::PaperColour> {
    fn from(paper: ExportPaper) -> Self {
        match paper {
            ExportPaper::White => None,
            ExportPaper::Blueprint => Some(solidcraft_io::paper::PaperColour::Blueprint),
            ExportPaper::PrussianBlue => Some(solidcraft_io::paper::PaperColour::PrussianBlue),
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
    /// A DXF's pictures (shaded views, images), left out: it holds only lines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pictures_left_out: Option<usize>,
}

/// What `drawing.export` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportResult {
    /// The sheet, or a PDF's first page's.
    pub sheet: String,
    /// A PDF's sheets, one page each, in page order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<String>,
    pub format: ExportFormat,
    /// The document's size in bytes.
    pub bytes: usize,
    /// The file written, when a path was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The SVG's text when no path was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub svg: Option<String>,
    /// The DXF's text when no path was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dxf: Option<String>,
    pub report: ExportReport,
    /// Each view on the sheet, in id order, as `drawing.scene` reports it.
    pub views: Vec<SheetSceneView>,
    /// Each dimension of a view on the sheet, as `drawing.scene` reports it: one that isn't drawn
    /// leaves the file written, and says why here.
    pub dimensions: Vec<SheetSceneDimension>,
    /// Each note on the sheet or its views, as `drawing.scene` reports it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<SheetSceneNote>,
    /// Centre annotations omitted and why, as `drawing.scene` reports them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotation_failures: Vec<SheetAnnotationFailure>,
    /// Each balloon of a view on the sheet, as `drawing.scene` reports it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub balloons: Vec<SheetSceneBalloon>,
    /// Each table on the sheet, as `drawing.scene` reports it: one that isn't drawn leaves the file
    /// written, and says why here.
    pub tables: Vec<SheetSceneTable>,
    /// What a deferred drawing wrote as it last was, as `drawing.scene` reports it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub out_of_date: Vec<String>,
    /// Each sheet sketch on the sheet, in id order, as `drawing.scene` reports it.
    pub sketches: Vec<SheetSceneSketch>,
    /// Each fill and its boundary/expansion status, as drawing.scene reports it.
    pub fills: Vec<SheetSceneFill>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
