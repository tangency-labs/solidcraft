//! Contracts and result types for the commands in `cmd/live.rs`.
//!
//! The handler reads its parameters through [`LiveMeshesParams`] itself, so the contract and what
//! the handler accepts cannot drift apart.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How finely a body is tessellated: one of the display qualities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LiveQuality {
    Coarse,
    #[default]
    Normal,
    Fine,
    Ultra,
}

/// What polygons a face may become.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LiveTopology {
    /// Quads where triangles form good ones, and a planar face with one boundary loop as one
    /// polygon.
    #[default]
    Ngons,
    /// Quads where triangles form good ones; no n-gons.
    Quads,
    /// Triangles only.
    Triangles,
}

/// How one body is tessellated, for a body named in `overrides` or for all of them.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
pub struct LiveTessellation {
    /// One of the display qualities. Default: normal. Ignored when `chord_mm` or
    /// `chord_relative` is given.
    #[serde(default)]
    pub quality: Option<LiveQuality>,
    /// Most a facet may stand off the surface, in mm (more than 0). Default: from `quality`.
    #[serde(default)]
    pub chord_mm: Option<f64>,
    /// Most a facet may stand off the surface, as a fraction of the body's size (more than 0, at
    /// most 0.1). Ignored when `chord_mm` is given.
    #[serde(default)]
    pub chord_relative: Option<f64>,
    /// Most a facet of a curved face may turn, in degrees (1 to 90). Default: from `quality`.
    #[serde(default)]
    pub angle_deg: Option<f64>,
    /// What polygons faces become. Default: ngons.
    #[serde(default)]
    pub topology: Option<LiveTopology>,
}

/// A body tessellated in its own way (a refacet asked for by the receiving program).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct LiveOverride {
    /// The body's name, as in `objects[].key`.
    pub body: String,
    #[serde(flatten)]
    pub tessellation: LiveTessellation,
}

/// The visible bodies as welded polygon meshes, for a program that follows the open design (a
/// live link to a modelling or rendering program, `docs/formats/live-mesh.md`). Every body is
/// listed with its placement and colours; a mesh is sent only when its key is not in `have`, and
/// placements of one part share one mesh. Read-only. Replies can be many megabytes: meant for a
/// live link, not for an agent's context.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct LiveMeshesParams {
    #[serde(flatten)]
    pub tessellation: LiveTessellation,
    /// Send corner normals (the surfaces' own, so curved faces shade smoothly). Default: true.
    #[serde(default)]
    pub normals: Option<bool>,
    /// Only these bodies (at most 65536 names). Default: every visible body.
    #[serde(default)]
    pub bodies: Option<Vec<String>>,
    /// Bodies tessellated another way than the rest (at most 4096).
    #[serde(default)]
    pub overrides: Option<Vec<LiveOverride>>,
    /// Keys of meshes the caller holds already: those are not sent again (at most 65536).
    #[serde(default)]
    pub have: Option<Vec<String>>,
    /// List hidden bodies too, with `visible: false`. Default: false.
    #[serde(default)]
    pub include_hidden: Option<bool>,
    /// Most mesh data to send in one reply, in MiB (1 to 512). Meshes past it are listed in
    /// `pending`: ask again with the received keys in `have`. At least one mesh is always sent.
    /// Default: 64.
    #[serde(default)]
    pub max_mb: Option<f64>,
}

/// A component, for grouping objects.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct LiveComponent {
    pub id: u64,
    pub name: String,
    /// The parent component (0: the design itself).
    pub parent: u64,
}

/// A face whose look differs from its body's.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct LiveFaceLook {
    /// B-rep face index (as in a mesh's `poly_face`).
    pub face: u32,
    /// sRGB, 0–255.
    pub color: [u8; 3],
    pub opacity: f64,
}

/// One body where it stands.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct LiveObject {
    /// The body's name, unique in the design: what follows it from one reply to the next.
    pub key: String,
    /// The part it is a placement of (its own name for a body that is not a placement).
    pub definition: String,
    /// The component it belongs to (0: the design itself).
    pub component: u64,
    /// Column-major 4×4 matrix (`m[col][row]`, mm) that places the mesh.
    pub matrix: [f64; 16],
    /// Key of its mesh in `meshes` or one the caller holds; `null` when it could not be meshed
    /// (`error` says why).
    pub mesh: Option<String>,
    pub visible: bool,
    /// sRGB, 0–255.
    pub color: [u8; 3],
    pub opacity: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<LiveFaceLook>,
    /// The body had no display plan and was meshed from its triangles (fewer quads).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fallback: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `live.meshes` returns.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct LiveMeshesResult {
    /// The design these meshes are of (as `document.revision` names it).
    pub document_id: String,
    /// Always "mm".
    pub unit: String,
    /// Always "Z".
    pub up: String,
    pub components: Vec<LiveComponent>,
    pub objects: Vec<LiveObject>,
    /// Live-link meshes (`docs/formats/live-mesh.md`) the caller does not hold yet.
    pub meshes: Vec<serde_json::Value>,
    /// Keys of meshes left out to keep the reply within `max_mb`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
