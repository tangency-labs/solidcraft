//! Contracts and result types for the commands in `cmd/file.rs` that start a design: `file.new`
//! (#13305).
//!
//! The handler reads its parameters from the raw JSON, so the key here reads as the handler reads
//! it. `file.new` refuses nothing: a `name` that is not text is read as left out, so the
//! handler has no message of its own for the check to keep.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Start a new, empty design and make it the current one, replacing the design that is open
/// (save it first: this does not ask). A name you give is kept by Save As. Not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct NewDesignParams {
    /// The design's name. Default: "Untitled". A name other than "Untitled" (and not blank) is
    /// the user's own, so Save As offers it as the file name.
    #[serde(default)]
    pub name: Option<String>,
}

/// What `file.new` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NewDesignResult {
    /// The new design's name: the `name` given, else "Untitled".
    pub name: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "new_design_tests.rs"]
mod tests;
