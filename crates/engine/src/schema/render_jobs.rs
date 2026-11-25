//! Contracts and reply types for the render job commands in `cmd/render/jobs.rs` (#12836):
//! Render Progress, Cancel Render, Retain Displayed Render Result, Regrade Linear Master, Copy
//! Render Image and Export Render Image. They follow a job that `render.start` queued, and keep,
//! regrade, copy and export its finished image. None of them changes the design.
//!
//! The handlers parse their parameters with serde and give serde's message after the command's
//! prefix. Each key here reads its value as the same Rust type and passes serde's message on
//! whole ([`as_handler`]), so strict refuses those calls with today's message. Only an unknown
//! key and a missing required key are worded by the check. What the handlers check after
//! parsing (a presentation's ranges, the export path's extension, whether a job exists) stays
//! theirs, with their own messages.
//!
//! The render crate has no `JsonSchema`. A job's id, progress, error and frame are typed here;
//! its other sections (the request, what it resolved to, its options, the linear master) are
//! published as JSON objects, for reading.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use solidcraft_render::job::JobId;
use solidcraft_render::job::presentation::Presentation;

use super::check::whole;
use crate::render_jobs::RenderQueueState;

/// The value as a `T`, with serde's message passed on whole: the handler parses the same type
/// from the same JSON and gives that message after its prefix, so strict gives it too.
fn as_handler<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<T, D::Error> {
    serde_json::from_value(Value::deserialize(d)?).map_err(|e| D::Error::custom(whole(e)))
}

/// A render job's identity, as `render.start` returns it and the render crate's `JobId` reads
/// it: `{session, serial, generation}`, each a whole number, and no other key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Job(pub JobId);

impl JsonSchema for Job {
    fn schema_name() -> Cow<'static, str> {
        "RenderJob".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let whole_number = |description: &str| serde_json::json!({"description": description, "type": "integer", "minimum": 0});
        json_schema!({
            "description": "A render job, exactly as `render.start` returned it. It belongs to the open design: \
                a job of another design, or of this one before it was opened again, is refused.",
            "type": "object",
            "properties": {
                "session": whole_number("The open design's render identity."),
                "serial": whole_number("The job's number among that design's jobs."),
                "generation": whole_number("The job's render generation."),
            },
            "required": ["session", "serial", "generation"],
            "additionalProperties": false,
        })
    }
}

/// Render Progress (`render.status`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct StatusParams {
    /// The job to report. Without it, or with `null`, the reply has the shot queue only. Asking
    /// also moves a started shot queue (`render.batch`) on to its next shot when a job has
    /// finished; it never renders.
    #[serde(default, deserialize_with = "as_handler", skip_serializing_if = "Option::is_none")]
    pub job: Option<Job>,
}

/// Cancel Render (`render.cancel`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CancelParams {
    /// The job to cancel. Without it, or with `null`: the shot queue's job, and its waiting
    /// shots are dropped. A job that has already finished is left as it is.
    #[serde(default, deserialize_with = "as_handler", skip_serializing_if = "Option::is_none")]
    pub job: Option<Job>,
}

/// Retain Displayed Render Result (`render.result.pin`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PinParams {
    /// The finished job to select among the eight results kept (the oldest is dropped first).
    /// Without it: the result already selected. `null` clears the selection.
    #[serde(default, deserialize_with = "as_handler", skip_serializing_if = "Option::is_none")]
    pub job: Option<Job>,
}

/// Copy Render Image (`render.result.copy`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CopyParams {
    /// The finished job whose image to copy. Without it, or with `null`: the selected result.
    /// The image may be at most 4096 pixels a side and 4194304 pixels in all.
    #[serde(default, deserialize_with = "as_handler", skip_serializing_if = "Option::is_none")]
    pub job: Option<Job>,
}

/// Regrade Linear Master (`render.result.regrade`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RegradeParams {
    /// The finished job whose linear master to regrade: it must have kept one (a CPU or GPU job
    /// with `linear_exr`). Without it, or with `null`: the selected result.
    #[serde(default, deserialize_with = "as_handler", skip_serializing_if = "Option::is_none")]
    pub job: Option<Job>,
    #[serde(deserialize_with = "as_handler")]
    pub presentation: PresentationParam,
}

/// How a regraded image is presented: the render crate's `Presentation`, read as it reads it.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(transparent)]
pub struct PresentationParam(pub Presentation);

impl JsonSchema for PresentationParam {
    fn schema_name() -> Cow<'static, str> {
        "RenderPresentation".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let colour = |description: &str| {
            serde_json::json!({
                "description": description,
                "type": "array",
                "items": {"type": "integer", "minimum": 0, "maximum": 255},
                "minItems": 3,
                "maxItems": 3,
            })
        };
        json_schema!({
            "description": "How to present the image again from its linear master. Every key is optional, \
                and a key left out takes its default, not the first image's value. Transparent output can't \
                have a backdrop, bloom or photographic effects.",
            "type": "object",
            "properties": {
                "exposure_ev": {"description": "Exposure, in stops, from -32 to 32. Default: 0.", "type": "number", "default": 0.0},
                "tone_map": {
                    "description": "The tone response. Default: `reinhard`.",
                    "type": "string",
                    "enum": ["reinhard", "aces_fitted"],
                    "default": "reinhard",
                },
                "finishing": {
                    "description": "Creative finishing: `looks` (at most 8 layers), `photographic`, `bloom`, \
                        `rgb_gain` [r, g, b], `contrast`, `saturation` and `vignette`. `render.effects.describe` \
                        lists the looks and the photographic defaults. Default: none.",
                    "type": "object",
                    "properties": {
                        "looks": {"type": "array", "items": {"type": "object"}, "maxItems": 8},
                        "photographic": {"type": "object"},
                        "bloom": {"type": ["object", "null"]},
                        "rgb_gain": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
                        "contrast": {"type": "number"},
                        "saturation": {"type": "number"},
                        "vignette": {"type": "number"},
                    },
                    "additionalProperties": false,
                },
                "backdrop": {
                    "description": "A gradient behind the product, or `null` for none (the default): `mode` \
                        (`vertical`, `horizontal` or `radial`), `first_srgb` and `second_srgb` colours, and for \
                        `radial`, its `center` [x, y] as fractions of the image and its `radius`.",
                    "type": ["object", "null"],
                    "properties": {
                        "mode": {"type": "string", "enum": ["vertical", "horizontal", "radial"]},
                        "first_srgb": colour("The first colour, [r, g, b] from 0 to 255."),
                        "second_srgb": colour("The second colour, [r, g, b] from 0 to 255."),
                        "center": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2},
                        "radius": {"type": "number"},
                    },
                    "additionalProperties": false,
                },
                "background": colour("The background colour, [r, g, b] from 0 to 255. Default: [240, 243, 248]."),
                "transparent_background": {
                    "description": "Leave the background transparent. Default: false.",
                    "type": "boolean",
                    "default": false,
                },
            },
            "additionalProperties": false,
        })
    }
}

/// The file format Export Render Image writes: `png`, the finished image as shown; `exr`,
/// the raw linear FLOAT32 master, which the job must have kept (`linear_exr`); or
/// `denoise_mask`, the categorical PNG guide mask kept with `denoise_diagnostics`.
/// The handler reads it as this type.
// The variants have no doc comments on purpose: with them, schemars publishes `oneOf` and
// `const` instead of a plain `enum`, which the schema checks in the tests don't read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    #[default]
    Png,
    Exr,
    DenoiseMask,
}

impl ExportFormat {
    /// The file extension the path must end with.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png | Self::DenoiseMask => "png",
            Self::Exr => "exr",
        }
    }
}

/// Export Render Image (`render.export`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExportParams {
    /// The finished job whose image to write. Without it, or with `null`: the selected result.
    #[serde(default, deserialize_with = "as_handler", skip_serializing_if = "Option::is_none")]
    pub job: Option<Job>,
    /// The file to write. At most 4096 bytes, with no control characters, ending in the
    /// format's extension (`.png` or `.exr`, in any case), and within the paths this session
    /// may write.
    #[serde(deserialize_with = "as_handler")]
    pub path: String,
    /// The file format. Default: `png`.
    #[serde(default, deserialize_with = "as_handler")]
    pub format: ExportFormat,
    /// Replace a file already at `path`. Default: false, which refuses one. Either way the
    /// file is written whole or not at all.
    #[serde(default, deserialize_with = "as_handler")]
    pub overwrite: bool,
}

/// Where a job is. It has finished once `cancelled`, `completed` (its image is done) or
/// `failed` (see `error`).
// No doc comments on the variants, so the schema is a plain `enum` (see `ExportFormat`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobPhase {
    Queued,
    Preparing,
    Running,
    Encoding,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

/// A job's progress.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct JobProgress {
    pub phase: JobPhase,
    /// The work done, of `total`, in the job's own units (passes or tiles).
    pub done: u64,
    pub total: u64,
}

/// Why a job failed.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct JobFailure {
    /// The kind of failure, in snake_case: `invalid_request`, `limit_exceeded`, `cancelled`,
    /// `not_completed` and others.
    pub code: String,
    pub message: String,
}

/// A finished image.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Frame {
    /// The job that made it.
    pub job: Job,
    /// The design revision its scene was captured from.
    pub scene_generation: u64,
    /// The render backend and executor that made it (`builtin`, `cpu-raytrace`, …).
    pub backend: String,
    pub executor: String,
    /// Its size, in pixels.
    pub width: u32,
    pub height: u32,
    /// Its PNG's colour encoding: `srgb`.
    pub encoding: String,
    /// Its PNG's alpha: `straight`.
    pub alpha: String,
    /// The length of its PNG, in bytes.
    pub png_bytes: u64,
    /// The SHA-256 of its PNG, in lowercase hex.
    pub png_sha256: String,
}

/// A render job's status: what Render Progress gives for a job, and what Cancel Render and
/// Regrade Linear Master return.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct JobState {
    pub job: Job,
    /// The design revision its scene was captured from.
    pub scene_generation: u64,
    pub progress: JobProgress,
    /// Why it failed, or `null`.
    pub error: Option<JobFailure>,
    /// Its finished image, or `null` until it completes.
    pub frame: Option<Frame>,
    /// The image request it runs, as the render crate gives it.
    pub request: Map<String, Value>,
    /// The backend and executor the request resolved to, and their limits.
    pub resolved: Map<String, Value>,
    /// Its options, as the render crate gives them.
    pub options: Map<String, Value>,
    /// The textures it uploaded, or `null`.
    pub textures: Option<Map<String, Value>>,
    /// Preview passes left out because both preview images were still in use. The final image
    /// is complete all the same.
    pub preview_retention_skips: u32,
    /// The sections that apply to this job, each an object as the render crate gives it:
    /// `preview` and `preview_unavailable_reason`, `presentation` (for a regraded image),
    /// `linear_master`, `sampling`, `environment` and `punctual_diagnostics`.
    #[serde(flatten)]
    pub sections: Map<String, Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Render Progress for a job: its status and the shot queue.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JobProgressReply {
    #[serde(flatten)]
    pub state: JobState,
    pub queue: RenderQueueState,
}

/// Render Progress without a job: the shot queue only.
#[derive(Debug, Serialize, JsonSchema)]
pub struct QueueReply {
    pub queue: RenderQueueState,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.status` returns.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum RenderProgress {
    // Boxed: a job's status is far larger than the queue alone (clippy::large_enum_variant).
    Job(Box<JobProgressReply>),
    Queue(QueueReply),
}

/// Cancel Render while the shot queue is still preparing its first job, before it has one.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PreparationCancelled {
    /// Always true.
    pub preparing: bool,
    /// Always true: the preparation was asked to stop.
    pub cancel_requested: bool,
    pub queue: RenderQueueState,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.cancel` returns: the job's status once asked to stop (a job that had finished
/// is unchanged), or, while the shot queue prepares its first job, that preparation stopping.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum RenderCancelled {
    // Boxed, as in `RenderProgress`.
    Job(Box<JobState>),
    Preparation(PreparationCancelled),
}

/// What `render.result.pin` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ResultPinned {
    /// The selected result's image, or `null` when the selection was cleared.
    pub frame: Option<Frame>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.result.copy` returns. The app copies the image; a script gets its frame only.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImageCopied {
    pub frame: Frame,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.export` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImageExported {
    pub job: Job,
    /// The file written, as given.
    pub path: String,
    /// How many bytes were written.
    pub bytes: usize,
    pub format: ExportFormat,
    /// The file's colour encoding: `srgb` for a colour PNG, `scene_linear_srgb_d65` for an
    /// EXR, or `categorical_srgb` for a guide mask.
    pub encoding: &'static str,
    /// The file's alpha: `straight` for a colour PNG, `coverage_premultiplied` for an EXR,
    /// or `opaque` for a guide mask.
    pub alpha: &'static str,
    /// The parent finished colour image, including its colour PNG hash when a guide mask
    /// is exported. The separate mask metadata describes the exported mask bytes.
    pub frame: Frame,
    /// The linear master the job kept, as the render crate describes it, or `null`.
    pub linear_master: Option<Map<String, Value>>,
    /// How a regraded image was presented, as the render crate describes it, or `null` for an
    /// image rendered directly.
    pub presentation: Option<Map<String, Value>>,
    /// The guide mask the job kept, including its PNG byte count, dimensions and SHA, or `null`.
    pub denoise_mask: Option<Map<String, Value>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "render_jobs_tests.rs"]
mod tests;
