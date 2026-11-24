//! Contracts for the commands in `cmd/mesh.rs`. Convert Mesh replies with the plain feature
//! reply, [`super::features::FeatureResult`]; Section Stack with [`MeshSectionStackResult`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::sketch_session::SketchPlane;
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

/// Cut a mesh body with a row of parallel planes and put one sketch on each plane, holding the
/// lines, arcs and circles fitted to the mesh's section there (as Fit Curves to Mesh Section
/// fits one). Optionally extrude each section to the next plane, each slab a body of its own,
/// so the stack of slabs stands in for the mesh. One undo step; the sketches and extrudes go into
/// one timeline folder. A plane that misses the mesh makes no sketch.
///
/// The planes run along the normal of `plane`, from `from` to `to` every `spacing`, always
/// including `to`; a last plane closer than 0.01 mm (or a tenth of the spacing) to `to` moves
/// onto it. Both ends are kept 0.001 mm inside the body's reach along the normal, so no plane
/// lies exactly on a flat end face. Whether the body exists and is a mesh, and the numbers'
/// ranges, stay the handler's to check.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MeshSectionStackParams {
    /// The mesh body, by name. Default: the one selected body. A blank name also means the
    /// selected body. It is a body of the active component, or, with the design's top level
    /// active, of any component in it placed once (where Insert Mesh puts a scan): that body is
    /// cut where it is placed.
    #[serde(default)]
    pub body: Option<String>,
    /// The first plane, as `sketch.create` takes it: XY, XZ or YZ (also Top, Front, Right), a
    /// construction plane's name or id, a picked planar face `{face: [x, y, z], body?, index?,
    /// path?, fixed?}`, or an explicit plane `{origin?, x_dir, y_dir}` in mm. The stack runs
    /// along its normal.
    pub plane: SketchPlane,
    /// The first section's distance from `plane` along its normal. Default: where the body
    /// starts along the normal.
    #[serde(default)]
    pub from: Option<LengthExpr>,
    /// The last section's distance from `plane` along its normal. Default: where the body ends
    /// along the normal. `from` and `to` may be given either way round; a range wholly outside
    /// the body is refused.
    #[serde(default)]
    pub to: Option<LengthExpr>,
    /// The distance between sections, more than 0.
    pub spacing: LengthExpr,
    /// The most sections the stack may make, from 1 to 1000. Default: 100. A stack that needs
    /// more is refused, naming the spacing that fits.
    #[serde(default)]
    pub max_sections: Option<u32>,
    /// Extrude each section's material (its outlines, not the holes in them) to the next
    /// plane, whether or not that plane met the mesh, the last one a spacing on but never past
    /// the body, each slab a new body named after the mesh body (`<body> slab 1`, …). They are
    /// not joined: neighbouring slabs meet on outlines fitted section by section, which a boolean
    /// would have to merge. A slab that cannot be built is listed in `skipped`, with why, and
    /// the rest stand. Default: false.
    #[serde(default)]
    pub extrude: Option<bool>,
    /// The start of the sketches' names, at most 100 characters. Default: `Section`. When a
    /// sketch of the stack would take a name already in the design, the stack starts its names
    /// with the first of `<name> 2`, `<name> 3`… that is free.
    #[serde(default)]
    pub name: Option<String>,
}

/// A section that made a sketch but no slab, and why.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SkippedSection {
    /// The sketch's id.
    pub sketch: u64,
    /// Why it made no slab.
    pub reason: String,
}

/// What `mesh.section_stack` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshSectionStackResult {
    /// The sketches made, one for each plane that met the mesh, in order along the normal.
    pub sketches: Vec<u64>,
    /// How many planes the stack cut.
    pub planes: usize,
    /// How many planes missed the mesh, so made no sketch.
    pub empty: usize,
    /// The extrude features made, in order.
    pub slabs: Vec<u64>,
    /// The bodies the slabs make, one each (empty without `extrude`).
    pub bodies: Vec<String>,
    /// Sections that made no slab, and why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<SkippedSection>,
    /// The timeline folder holding the stack's features.
    pub group: Option<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Create a reduced mesh in the source body's component, leaving the source unchanged.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MeshReduceParams {
    /// Mesh body name; omitted or blank uses the selected body.
    #[serde(default)]
    pub body: Option<String>,
    /// Desired positive triangle count. Specify this or `percentage`, not both.
    #[serde(default)]
    pub triangle_count: Option<usize>,
    /// Percentage of triangles to retain, strictly between 0 and 100.
    #[serde(default)]
    pub percentage: Option<f64>,
    /// Timeline feature name.
    #[serde(default)]
    pub name: Option<String>,
    /// Existing participating bodies.
    #[serde(default)]
    pub participants: Option<StringList>,
}

/// Feature result with the requested and actual triangle counts.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshReduceResult {
    #[serde(flatten)]
    pub feature: super::features::FeatureResult,
    pub original_triangles: usize,
    pub target_triangles: usize,
    pub achieved_triangles: usize,
}

#[cfg(test)]
#[path = "mesh_tests.rs"]
mod tests;
