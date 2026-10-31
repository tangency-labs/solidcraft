//! Typed contracts for the note commands (#1631).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::Point3;

/// Anchor a new note to a point in model space.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct NoteAddParams {
    /// The point the note is anchored to: `[x, y, z]` (mm).
    pub anchor: Point3,
    /// The note's text (may span lines).
    pub text: String,
    /// Save the note hidden: kept with the design but not drawn.
    #[serde(default)]
    pub hidden: bool,
}

/// Change a note's text, anchor, or hidden flag.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct NoteUpdateParams {
    /// The note's id.
    pub note: u64,
    /// The new text (may span lines).
    #[serde(default)]
    pub text: Option<String>,
    /// The new anchor point: `[x, y, z]` (mm).
    #[serde(default)]
    pub anchor: Option<Point3>,
    /// Show or hide the note.
    #[serde(default)]
    pub hidden: Option<bool>,
}

/// Delete a note.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct NoteDeleteParams {
    /// The note's id.
    pub note: u64,
}

/// List the design's notes (takes nothing).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct NoteListParams {}

/// The result of `note.add` and `note.update`: the note's id.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoteId {
    pub note: u64,
}

/// The result of `note.delete`: the id that was removed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoteDeleted {
    pub deleted: u64,
}

/// One note as `note.list` returns it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoteItem {
    pub id: u64,
    /// The anchor point `[x, y, z]` (mm).
    pub anchor: [f64; 3],
    pub text: String,
    pub hidden: bool,
}

/// The result of `note.list`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NotesList {
    pub notes: Vec<NoteItem>,
}
