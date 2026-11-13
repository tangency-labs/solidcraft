//! Contracts for the commands in `cmd/mesh.rs`. Convert Mesh replies with the plain feature
//! reply, [`super::features::FeatureResult`].

use schemars::JsonSchema;
use serde::Deserialize;

use super::types::{LengthExpr, StringList};

/// Rebuild a closed mesh body as an editable solid, with a parametric feature in the timeline.
/// The solid keeps the mesh body's name and colour.
///
/// Whether the body exists and is a mesh, and whether the active component may edit it, stay the
/// handler's to check. The kernel checks the tolerance's range when the feature evaluates, so an
/// expression is taken here whatever its value.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MeshConvertParams {
    /// The mesh body, by name. Default: the one selected body. A blank name also means the
    /// selected body, as the handler reads it.
    #[serde(default)]
    pub body: Option<String>,
    /// How far the rebuilt faces may stray from the mesh's triangles: a run of facets within it of
    /// an arc becomes a cylinder. From 0.000001 mm to 1 mm. Default: 0.05 mm.
    #[serde(default)]
    pub tolerance: Option<LengthExpr>,
    // Not `FeatureCommon`: the solid keeps the mesh body's name, so `body_name` and `body_names`
    // would do nothing. `participants` stays because `timeline.redefine` passes a feature's own
    // back.
    /// The feature's name in the timeline.
    #[serde(default)]
    pub name: Option<String>,
    /// The bodies taking part, which every feature records; each an existing body or `*`. A
    /// conversion changes only `body`, whatever this lists.
    #[serde(default)]
    pub participants: Option<StringList>,
}

#[cfg(test)]
#[path = "mesh_tests.rs"]
mod tests;
