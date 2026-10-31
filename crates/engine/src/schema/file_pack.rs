//! Contracts and result types for `file.pack` (`cmd/file_pack.rs`, #3890 K12).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Pack the saved design and the designs its links name into one .zip to hand on.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct PackParams {
    /// The .zip file to write.
    pub path: String,
}

/// A linked design in the pack.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PackedDesign {
    /// The file it was copied from.
    pub file: String,
    /// Its name in the pack, under `designs/`.
    pub entry: String,
}

/// A link a linked design makes, left out of the pack: packs go one level deep.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DeeperLink {
    /// The linked design that makes it, by its name in the pack.
    pub design: String,
    /// The link's path as that design keeps it.
    pub path: String,
}

/// What `file.pack` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PackResult {
    /// The .zip file written.
    pub path: String,
    /// Its size in bytes.
    pub bytes: usize,
    /// The design's name in the pack, at the top level.
    pub design: String,
    /// Each design its links name, once each, in link order.
    pub designs: Vec<PackedDesign>,
    /// The links those designs make, not followed.
    pub deeper: Vec<DeeperLink>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "file_pack_tests.rs"]
mod tests;
