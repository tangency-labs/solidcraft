//! Typed contracts for the note commands (#1631).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::{Point3, present};

/// Anchor a new note to a point in model space.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct NoteAddParams {
    /// The point the note is anchored to: `[x, y, z]` (mm). Without it the note lands where
    /// the selection points (the picked point, a body, a construction point or a sketch point),
    /// or at the origin when nothing is selected.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "Point3")]
    pub anchor: Option<Point3>,
    /// The note's text (may span lines).
    pub text: String,
    /// Save the note hidden: kept with the design but not drawn.
    #[serde(default)]
    pub hidden: bool,
}

/// Change a note's text, anchor, hidden or done flag.
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
    /// Mark the note done (worked off) or open again.
    #[serde(default)]
    pub done: Option<bool>,
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
    /// The comment has been worked off.
    pub done: bool,
    /// What was selected when the note was made, when it was a body or a face, edge or vertex.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<NoteTargetItem>,
}

/// The element a note refers to.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoteTargetItem {
    /// `body`, `face`, `edge` or `vertex`.
    pub kind: String,
    /// The body's name.
    pub body: String,
    /// The face or edge index within the body, as `body_topology` lists them; it can change
    /// when the design rebuilds, so check it against the anchor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

/// The result of `note.list`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NotesList {
    pub notes: Vec<NoteItem>,
}

/// Show or hide the Notes panel in the app (takes nothing).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct NotesPanelParams {}

/// Headless sessions refuse Notes panel requests and produce no result.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NotesPanelResult {}
