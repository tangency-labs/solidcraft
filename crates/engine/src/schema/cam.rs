//! Contract and result types for the commands in `cmd/cam.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Post a setup's operations to machine code.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PostParams {
    /// The setup whose operations are posted.
    pub setup: String,
    /// A built-in post (`rs274`, `LinuxCNC`, …) or the name of a post saved
    /// on the design. Default: `rs274`.
    #[serde(default)]
    pub post: Option<String>,
    /// The program's name. Default: the setup's name.
    #[serde(default)]
    pub program: Option<String>,
}

/// The machine code block one posted operation produced, checked against
/// its toolpaths.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PostReadback {
    /// Largest distance between a cutting move as written and as planned (mm).
    pub max_deviation: f64,
    /// How far the post's own precision lets the two differ (mm).
    pub allowed: f64,
    /// Distance at feed as written (mm).
    pub cut_length: f64,
    /// Time at feed as written, dwells included (minutes).
    pub cut_minutes: f64,
    /// Whether the written program stays within the post's precision.
    pub within: bool,
}

/// What `cam.post` returns: the machine code, how it is tagged, and the
/// read-back check that gates it. Always verified: a reply means the code
/// stayed within the post's own precision of the toolpaths.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PostResult {
    /// The machine code language, lower-case (`"gcode"` today).
    pub format: String,
    /// File extension for the code, without the dot.
    pub extension: String,
    /// The machine code text.
    pub code: String,
    /// The post processor that wrote it.
    pub post: String,
    /// How many operations were posted.
    pub operations: u64,
    /// The work coordinate system the program runs in, 1 to 6.
    pub work_offset: u8,
    /// The written code held against the toolpaths it was written from.
    pub readback: PostReadback,
}

#[cfg(test)]
#[path = "cam_tests.rs"]
mod tests;
