//! The contract of `drawing.template.import` (#3875).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Make a drawing template from another program's sheet format, saved as SVG.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ImportParams {
    /// The SVG file to read, at most 4096 bytes long.
    pub file: String,
    /// The .json template file to write (none with `dry_run`).
    #[serde(default)]
    pub path: Option<String>,
    /// Answer what the template would hold without writing it.
    #[serde(default)]
    pub dry_run: bool,
}

/// What `drawing.template.import` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportResult {
    /// The file written; none for a dry run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The paper the template's sheet takes.
    pub sheet: ImportedSheet,
    /// How many strokes of line work its sheet format draws.
    pub strokes: usize,
    /// How many fixed texts it draws.
    pub texts: usize,
    /// The texts the file marks as fields, each by its name and the text it shows now. They are
    /// kept as fixed text: filling them in is not supported yet.
    pub fields: Vec<ImportedField>,
    /// What the file holds that the template leaves out, by kind and count.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<SkippedCount>,
    /// Pieces drawn close to the file's own, not exactly: splines become lines through their points.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub approximated: Vec<SkippedCount>,
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The paper an imported sheet takes.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportedSheet {
    /// Its width and height as it lies, in millimetres.
    pub size_mm: [f64; 2],
    /// The standard size it matches within 2 mm, such as `a3`; none for a size of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standard: Option<String>,
    /// `landscape` or `portrait`.
    pub orientation: String,
}

/// A text the file marks as a field.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportedField {
    pub name: String,
    pub text: String,
}

/// A kind of thing and how many there were.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SkippedCount {
    pub kind: String,
    pub count: usize,
}
