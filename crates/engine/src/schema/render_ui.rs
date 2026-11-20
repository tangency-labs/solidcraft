//! Contracts for transient Render workspace clipboard and retained-result requests.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::render_jobs::Job;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckReport {
    /// Report text, bounded to 65536 UTF-8 bytes by the command handler.
    #[schemars(length(min = 1, max = 65536))]
    pub text: String,
}

/// Select or discard a retained result without editing a design.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResultIndex {
    /// Zero-based index, oldest first. The result must still be retained.
    #[schemars(range(min = 0, max = 7))]
    pub index: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiveSide {
    Live,
}

/// A live product view or a retained result's zero-based index (oldest first).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(untagged)]
pub enum CompareSide {
    Live(LiveSide),
    Result(#[schemars(range(min = 0, max = 7))] u32),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComparePair {
    pub left: CompareSide,
    pub right: CompareSide,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareOff {
    /// Must be true. Leave comparison without changing retained results.
    pub off: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CompareParams {
    Off(CompareOff),
    Pair(ComparePair),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareDivider {
    /// Finite fraction of the canvas width, clamped to 0.05..0.95.
    pub position: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchParams {
    /// Optional render.start request. Its setup is shared, with each saved shot's camera.
    /// Without a request, use the first saved shot's setup (name order).
    pub request: Option<std::collections::BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BatchResult {
    /// Total accepted shots, including the first active job. At most 32.
    pub queued: u32,
}

/// Which view of a retained render result the Render workspace shows. `render.result.view` names
/// one; `render.result.close` is the Escape key's way back to [`ResultViewName::Model`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResultViewName {
    /// The model preview: no render result is shown, and any comparison is left.
    Model,
    /// The selected result's image, as the canvas already shows it.
    Image,
    /// The selected result's image, scaled to fit the canvas.
    Fit,
    /// The selected result's image, one image pixel to one screen pixel.
    Actual,
    /// The denoising guide mask of the selected result, which only a result captured with
    /// denoising diagnostics has.
    DenoiseMask,
}

/// The name of a view the handler refuses, in its own words: the five names are read exactly as
/// they are spelled, so another name or another case gets this. The contract leaves the name to
/// [`ResultViewName::parse`], so the call is refused this way with the parameter check off and in
/// strict alike. A value that is not text is serde's to refuse.
pub const RESULT_VIEW_FORMS: &str = "view must be model, image, fit, actual or denoise_mask";

impl ResultViewName {
    /// The view named by `name`, exactly as the handler reads it: the names below, in lower case,
    /// and nothing else.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "model" => Some(Self::Model),
            "image" => Some(Self::Image),
            "fit" => Some(Self::Fit),
            "actual" => Some(Self::Actual),
            "denoise_mask" => Some(Self::DenoiseMask),
            _ => None,
        }
    }
}

/// `render.result.view`: which view of the render result to show. The name is read as text and
/// looked up by [`ResultViewName::parse`] in the handler, so a name that is not one of the five is
/// refused the same way with the parameter check off and in strict. A value that is not text is
/// serde's to refuse: the handler gives its message, the check its own (`view` must be text).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResultViewParams {
    /// One of `model`, `image`, `fit`, `actual`, `denoise_mask`. `denoise_mask` needs a selected
    /// completed result that was captured with denoising diagnostics.
    #[schemars(with = "ResultViewName")]
    pub view: String,
}

/// No parameters: `render.result.close` returns to the model preview. A call that gives a key is
/// refused by the check as an unknown key, or, with the check off, by the handler's own
/// `expected no parameters`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CloseResultParams {}

/// What `render.result.view` and `render.result.close` answer with: the view the workspace shows
/// now. The same JSON the handlers built by hand before they built their reply from this type.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ResultViewResult {
    /// The view the workspace shows now.
    pub view: ResultViewName,
    /// The retained result whose guide mask is shown. Only with `view: denoise_mask`, which is
    /// refused when the selected result has no guide mask. The key is absent, never null, without it.
    #[schemars(with = "Job")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<Job>,
}

/// `render.output.size`: the Render workspace's output-size draft. The command answers with what it
/// was given, so this one type describes the call and the answer, as [`ResultIndex`] does for the
/// retained-result commands. The range is what the command accepts and refuses; it is stated here for
/// the caller, and the handler keeps its own message for a size outside it, in both parameter-check
/// modes, because the check does not read ranges.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputSize {
    /// Output width in pixels, 1..16384. The UI separately limits new choices to 8192 per side and
    /// 33554432 output pixels; the command itself accepts the wider range.
    #[schemars(range(min = 1, max = 16384))]
    pub width: u32,
    /// Output height in pixels, 1..16384, with the same limits as the width.
    #[schemars(range(min = 1, max = 16384))]
    pub height: u32,
    /// The size was typed rather than chosen from the listed sizes. Default: false; an answer leaves
    /// the key out unless it is true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub custom: bool,
}

#[cfg(test)]
#[path = "render_ui_tests.rs"]
mod tests;
