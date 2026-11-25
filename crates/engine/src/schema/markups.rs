//! Contracts and result types for the review markup commands in `cmd/markups.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{Point3, present};

/// A markup: its number in the set, or its guid (32 hexadecimal digits).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum MarkupRef {
    Id(u64),
    Guid(String),
}

/// The camera a markup is seen from: the orbit camera's target (mm), yaw and pitch (degrees),
/// distance (mm, above 0) and field of view (degrees, 0 for orthographic, below 180).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MarkupView {
    pub target: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    /// Default: 0 (orthographic).
    #[serde(default)]
    pub fov: f64,
}

/// A component path: occurrence ids from the top of the assembly down, 1 to 64 of them.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct OccurrencePath(pub Vec<u64>);

/// A feature of the timeline: its id, or its name.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FeatureRef {
    Id(u64),
    Name(String),
}

/// List a design's review markups, with where each one's face, edge, body, feature or
/// component stands in the model now.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MarkupListParams {
    /// The set's file (`…markups.json`). Default: the one beside the design's file.
    #[serde(default)]
    pub set: Option<String>,
    /// Only the markups with this status: `open`, `resolved` or `rejected`. Default: all.
    #[serde(default)]
    pub status: Option<String>,
}

/// Mark a place in the design with a note. Say what it is on with one of `face`, `edge`,
/// `feature`, `occurrence`, `body` or `at`; with none, the `view`, else the one thing selected.
/// Writes only the set beside the design: the design and its undo history stay as they are.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MarkupAddParams {
    /// The note (1 to 4,000 characters).
    pub text: String,
    /// A face by its lasting name, as `model.faces` lists it. With `at`, the point marked on it,
    /// which must lie on the face. Without `body`, the one body with a face of that name.
    #[serde(default)]
    pub face: Option<String>,
    /// An edge by its name, as `model.edges` lists it. With several edges between the same faces,
    /// `at` says which.
    #[serde(default)]
    pub edge: Option<String>,
    /// The body a `face` or `edge` is on; alone, the body itself; with `at`, the face of it the
    /// point lies on.
    #[serde(default)]
    pub body: Option<String>,
    /// A point (mm). Alone: the face of a body it lies on, found exactly, else the point in space.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point3")]
    pub at: Option<Point3>,
    /// A feature of the timeline, by id or name.
    #[serde(default)]
    pub feature: Option<FeatureRef>,
    /// A placed component, by its occurrence path.
    #[serde(default)]
    pub occurrence: Option<OccurrencePath>,
    /// The camera it was marked from, for Go there.
    #[serde(default)]
    pub view: Option<MarkupView>,
    /// Who marks it: the reviewer. Default: `agent`.
    #[serde(default)]
    pub by: Option<String>,
    /// The set's file (`…markups.json`). Default: the one beside the design's file.
    #[serde(default)]
    pub set: Option<String>,
}

/// Change a markup's note (its maker only), pin it again, or change its view. The thread,
/// pictures and status stay as they are.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MarkupEditParams {
    /// The markup.
    pub markup: MarkupRef,
    /// The note's new text (1 to 4,000 characters). Only the markup's maker (`by`) changes it.
    #[serde(default)]
    pub text: Option<String>,
    /// Pin it again on this face, as in `markup.add`.
    #[serde(default)]
    pub face: Option<String>,
    /// Pin it again on this edge, as in `markup.add`.
    #[serde(default)]
    pub edge: Option<String>,
    /// Pin it again on this body, as in `markup.add`.
    #[serde(default)]
    pub body: Option<String>,
    /// Pin it again at this point, as in `markup.add`.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point3")]
    pub at: Option<Point3>,
    /// Pin it again on this feature.
    #[serde(default)]
    pub feature: Option<FeatureRef>,
    /// Pin it again on this placed component.
    #[serde(default)]
    pub occurrence: Option<OccurrencePath>,
    /// The new camera.
    #[serde(default)]
    pub view: Option<MarkupView>,
    /// Who changes it. Default: `agent`.
    #[serde(default)]
    pub by: Option<String>,
    /// The set's file. Default: the one beside the design's file.
    #[serde(default)]
    pub set: Option<String>,
}

/// Add a message to a markup's thread without closing it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MarkupReplyParams {
    /// The markup.
    pub markup: MarkupRef,
    /// The message: at most 280 characters from anyone but the markup's maker (a longer one is
    /// refused whole), 4,000 from its maker.
    pub text: String,
    /// Who says it. Default: `agent`.
    #[serde(default)]
    pub by: Option<String>,
    /// The set's file. Default: the one beside the design's file.
    #[serde(default)]
    pub set: Option<String>,
}

/// Attach a picture to a markup, or take one off. Give exactly one of `file` and `remove`. A
/// picture is a PNG or a JPEG (by its first bytes), at most 16 MiB, at most 8 a markup. Its bytes
/// go in the design, an entry of their own; the set names the entry.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MarkupImageParams {
    /// The markup.
    pub markup: MarkupRef,
    /// A picture file to attach.
    #[serde(default)]
    pub file: Option<String>,
    /// The name the picture is shown with. Default: the file's name.
    #[serde(default)]
    pub name: Option<String>,
    /// A picture's entry (`<guid>.png` or `.jpg`, as `markup.list` gives it) to take off.
    #[serde(default)]
    pub remove: Option<String>,
    /// Who attaches it. Default: `agent`.
    #[serde(default)]
    pub by: Option<String>,
    /// The set's file. Default: the one beside the design's file.
    #[serde(default)]
    pub set: Option<String>,
}

/// Close an open markup as done (`markup.resolve`) or not done (`markup.reject`), with a reason.
/// The reason goes in the thread too, under the same length rule as a reply.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MarkupCloseParams {
    /// The markup.
    pub markup: MarkupRef,
    /// What was done, or why it was not.
    pub reason: String,
    /// Who closes it. Default: `agent`.
    #[serde(default)]
    pub by: Option<String>,
    /// The set's file. Default: the one beside the design's file.
    #[serde(default)]
    pub set: Option<String>,
}

/// Open a resolved or rejected markup again.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MarkupReopenParams {
    /// The markup.
    pub markup: MarkupRef,
    /// Why, added to the thread. Default: nothing added.
    #[serde(default)]
    pub reason: Option<String>,
    /// Who opens it. Default: `agent`.
    #[serde(default)]
    pub by: Option<String>,
    /// The set's file. Default: the one beside the design's file.
    #[serde(default)]
    pub set: Option<String>,
}

/// Delete a markup and its pictures.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MarkupDeleteParams {
    /// The markup.
    pub markup: MarkupRef,
    /// The set's file. Default: the one beside the design's file.
    #[serde(default)]
    pub set: Option<String>,
}

/// What a markup is on, as the set keeps it. `kind` says which: `face` (`body`, `maker`: the
/// feature that made the body, `face`: its lasting name, `at`, `config`?), `edge` (`body`,
/// `maker`, `edge`, `at`, `config`?), `body` (`body`, `maker`, `at`?, `config`?), `feature`
/// (`feature`: its id, `name`, `at`?), `occurrence` (`path`, `name`, `at`?), `point` (`at`),
/// `view` (nothing: the markup's view) or `unanchored` (`why`, `given`?).
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkupAnchor {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maker: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The point marked (mm), where it was then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 3]>,
    /// The configuration active when it was marked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given: Option<String>,
}

/// Where a markup's anchor stands in the model now. `state`: `anchored` (there, at `at`),
/// `moved` (there, now at `at`, `moved_by` mm from `was`), `hidden` (the history marker, a
/// suppression or another configuration leaves it out of the model; still in the design),
/// `gone` (deleted or replaced: `why` says what it was, in words) or `unchecked` (it could not be
/// measured: `why`).
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkupFound {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved_by: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// One message of a markup's thread.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkupMessage {
    pub by: String,
    /// Unix seconds.
    pub at: u64,
    pub text: String,
    /// Written by the markup's maker.
    pub reviewer: bool,
}

/// A picture on a markup.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkupImage {
    /// `<guid>.png` or `.jpg`: the picture's entry in the design.
    pub file: String,
    pub name: String,
    pub by: String,
    pub at: u64,
    pub bytes: u64,
    /// Whether the design holds the picture's bytes (not after an undo past `markup.image`).
    pub in_design: bool,
}

/// Who closed a markup, why and when (Unix seconds).
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkupOutcome {
    pub by: String,
    pub reason: String,
    pub at: u64,
}

/// One markup as `markup.list` gives it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkupRow {
    pub id: u64,
    pub guid: String,
    /// `open`, `resolved` or `rejected`.
    pub status: String,
    /// The thread's first message.
    pub note: String,
    pub thread: Vec<MarkupMessage>,
    #[schemars(with = "MarkupAnchor")]
    pub anchor: Value,
    pub found: MarkupFound,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<MarkupView>,
    pub images: Vec<MarkupImage>,
    /// Who made it: the reviewer.
    pub by: String,
    /// Unix seconds.
    pub made: u64,
    pub changed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<MarkupOutcome>,
}

/// How many markups have each status.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkupCounts {
    pub open: usize,
    pub resolved: usize,
    pub rejected: usize,
}

/// The result of `markup.list`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkupList {
    /// The set's file.
    pub set: String,
    pub format: u32,
    /// Over the whole set, whatever `status` asked for.
    pub counts: MarkupCounts,
    /// In the set's order (oldest first), only those of `status` when it was given.
    pub markups: Vec<MarkupRow>,
}

/// The result of `markup.add`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkupMade {
    /// The new markup's number.
    pub markup: u64,
    pub guid: String,
    /// The set's file.
    pub set: String,
    #[schemars(with = "MarkupAnchor")]
    pub anchor: Value,
    pub found: MarkupFound,
}

/// The result of a change to a markup.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkupChanged {
    pub markup: u64,
    /// Its status after the change.
    pub status: String,
    /// `markup.image`: the picture attached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// `markup.image`: the picture taken off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<String>,
}

/// The result of `markup.delete`: the number that was removed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkupDeleted {
    pub deleted: u64,
}
