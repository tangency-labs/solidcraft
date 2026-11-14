//! Contracts and result types for the commands in `cmd/mesh.rs` that have them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::LengthExpr;

/// Read a mesh body as the surfaces it stands for: planes, cylinders, cones, spheres and tori,
/// each certified against the mesh, and the directions they share. Nothing is changed; the reply
/// is what a design is rebuilt from.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MeshRecognizeParams {
    /// The mesh body, by name. The selected mesh body when left out.
    pub body: Option<String>,
    /// How far the mesh may stray from a surface and still be certified as it (0.05 mm by
    /// default, between 0.000001 and 1 mm). It has to cover the mesh's own faceting: a surface
    /// is measured inside each triangle, not only at its corners, so a coarse mesh needs a
    /// larger tolerance than a fine one to come back round rather than flat.
    pub tolerance: Option<LengthExpr>,
    /// One group number per triangle, in the body's own triangle order: a proposal about which
    /// triangles belong to one face, as a neural segmenter produces. It only decides which
    /// triangles are tried together; every surface is still fitted and certified, and a group
    /// that will not certify is split further, so a wrong proposal costs recognition and never
    /// correctness.
    pub groups: Option<Vec<u32>>,
    /// Most patches to report, largest area first (256 by default, at most 4096).
    pub max_patches: Option<usize>,
}

/// What `mesh.recognize` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshRecognizeResult {
    /// The mesh body that was read.
    pub body: String,
    /// Triangles after welding the mesh's duplicate vertices together.
    pub triangles: usize,
    /// Vertices after welding.
    pub vertices: usize,
    /// Whether the mesh is closed and consistently oriented. An open mesh is still recognized;
    /// only a closed one can become a solid without repair.
    pub closed: bool,
    /// The mesh's area (mm^2).
    pub area: f64,
    /// The tolerance every patch was certified within (mm).
    pub tolerance: f64,
    /// How many patches the mesh broke into, which can be more than are reported.
    pub found: usize,
    /// The share of the area (0 to 1) that no analytic surface certified over: how much of this
    /// part is sculpted, or too noisy to name.
    pub freeform: f64,
    /// The patches, largest area first.
    pub patches: Vec<MeshPatch>,
    /// The directions the patches share, most area first: where an extrusion or a revolve would
    /// be.
    pub axes: Vec<MeshAxis>,
}

/// One patch of the mesh and the surface it was certified as.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshPatch {
    /// `plane`, `cylinder`, `cone`, `sphere`, `torus`, or `freeform` where nothing analytic fit.
    pub surface: String,
    /// How many triangles the patch holds.
    pub triangles: usize,
    /// The patch's area (mm^2).
    pub area: f64,
    /// The greatest distance from the fitted surface over the patch, measured at its vertices and
    /// inside each of its triangles (mm). Zero for a freeform patch, which was not fitted.
    pub deviation: f64,
    /// The surface's own point: a plane's or a cylinder's origin, a cone's apex, a sphere's or a
    /// torus's centre. Absent for a freeform patch.
    pub origin: Option<[f64; 3]>,
    /// The surface's axis as a unit direction: a plane's normal, a cylinder's, cone's or torus's
    /// axis. A cone's points away from its apex, the way the radius widens. Absent for a sphere
    /// and a freeform patch.
    pub axis: Option<[f64; 3]>,
    /// A cylinder's or a sphere's radius, or a torus's major radius (mm).
    pub radius: Option<f64>,
    /// A torus's tube radius (mm): the fillet radius.
    pub minor: Option<f64>,
    /// A cone's half angle (degrees).
    pub half_angle: Option<f64>,
    /// Where the patch sits along its axis, from the surface's own point (mm): how long a
    /// cylinder's run is, or which part of a cone it covers.
    pub along: Option<[f64; 2]>,
    /// Whether the mesh faces away from the surface's own middle: true for a shaft or a boss,
    /// false for a bore. Absent for a plane, whose `axis` already points out of the part, and for
    /// a freeform patch.
    pub outward: Option<bool>,
}

/// A direction several patches share.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshAxis {
    /// The direction, as a unit vector.
    pub direction: [f64; 3],
    /// Area of the round patches turning about it, and of the planes parallel to it: an
    /// extrusion's walls (mm^2).
    pub along: f64,
    /// Area of the planes square to it: an extrusion's end caps (mm^2).
    pub across: f64,
}

/// Rebuild a prismatic part from what its mesh was recognized as: a sketch on one end face, an
/// extrusion to the other, and the holes and bosses the cylinders stand for.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MeshRebuildParams {
    /// The mesh body, by name. The selected mesh body when left out.
    pub body: Option<String>,
    /// How far the mesh may stray from a surface and still be certified as it (0.05 mm by
    /// default, between 0.000001 and 1 mm), as `mesh.recognize` takes it.
    pub tolerance: Option<LengthExpr>,
    /// One group number per triangle, as `mesh.recognize` takes it: a proposal about which
    /// triangles make one face, which steers the search and never decides the geometry.
    pub groups: Option<Vec<u32>>,
    /// The direction to extrude along, as `[x, y, z]`. Left out, the rebuild takes the direction
    /// with the most recognized area square to it, which is the one with end faces to extrude
    /// between.
    pub axis: Option<[f64; 3]>,
    /// How far a surface's own axis may lean from the build axis and still count as along it or
    /// square to it (degrees, 2 by default, between 0.0573 and 17.19). A scan of a part that is
    /// not quite square to itself needs more than a mesh made by CAD.
    pub align: Option<f64>,
    /// Whether to name the sizes as user parameters the features then follow (true by default).
    pub parameters: Option<bool>,
    /// What to call those parameters, before `_thickness` and the rest. Taken from the body's
    /// name when left out.
    pub prefix: Option<String>,
    /// Report the plan and the commands it would run, and change nothing (false by default).
    pub plan_only: Option<bool>,
    /// A name for the extrusion feature.
    pub name: Option<String>,
}

/// What `mesh.rebuild` returns: what the part was taken to be, what was built, and what was left
/// for the caller to model.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshRebuildResult {
    /// The mesh body that was read.
    pub body: String,
    /// The direction the part was extruded along.
    pub axis: [f64; 3],
    /// Where the sketch was made, in world coordinates.
    pub origin: [f64; 3],
    /// The sketch's own x and y directions.
    pub x_dir: [f64; 3],
    pub y_dir: [f64; 3],
    /// The distance between the two end faces (mm).
    pub thickness: f64,
    /// The outline: `circle` or `polygon`.
    pub outline: String,
    /// A round part's radius (mm).
    pub radius: Option<f64>,
    /// A polygon outline's corners, in order, in the sketch's own coordinates.
    pub corners: Vec<MeshCorner>,
    /// The holes through or into the part.
    pub bores: Vec<MeshBore>,
    /// The cylinders standing on the far end face.
    pub bosses: Vec<MeshBoss>,
    /// The share of the mesh's area (0 to 1) the rebuilt features cover. What is missing is in
    /// `unused`.
    pub accounted: f64,
    /// The recognized surfaces the rebuild does not account for, largest first, and why.
    pub unused: Vec<MeshUnused>,
    /// The parameters that were added, and what each was set to.
    pub parameters: Vec<MeshParameter>,
    /// The commands the rebuild runs, in order: what `plan_only` reports instead of building, and
    /// what to edit and send again as a batch to build it differently.
    pub steps: Vec<MeshStep>,
    /// The features that were built, in timeline order. Empty for `plan_only`.
    pub features: Vec<String>,
}

/// One corner of a rebuilt outline.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshCorner {
    /// The corner in the sketch's own coordinates (mm).
    pub at: [f64; 2],
    /// The radius it is rounded by, or zero where it is sharp (mm).
    pub radius: f64,
}

/// One hole through or into the rebuilt part.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshBore {
    /// Its centre in the sketch's own coordinates (mm).
    pub centre: [f64; 2],
    pub diameter: f64,
    /// Whether it goes all the way through.
    pub through: bool,
    /// How deep it goes, for one that does not (mm).
    pub depth: f64,
    /// Whether it is drilled from the far end face rather than the near one.
    pub from_top: bool,
    /// How far along the build axis the face it is drilled from sits (mm): the far end face,
    /// unless a boss stands over it, and then the top of that boss. `depth` is measured from
    /// there.
    pub at: f64,
}

/// One cylinder standing on the far end face.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshBoss {
    pub centre: [f64; 2],
    pub diameter: f64,
    pub height: f64,
}

/// A recognized surface the rebuild does not account for.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshUnused {
    /// `plane`, `cylinder`, `cone`, `sphere`, `torus` or `freeform`.
    pub surface: String,
    /// The area it covers (mm^2).
    pub area: f64,
    /// Why the rebuild left it out, and what to do about it by hand.
    pub reason: String,
}

/// A parameter the rebuild added.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshParameter {
    pub name: String,
    pub expression: String,
}

/// One command of the rebuild.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshStep {
    pub command: String,
    pub params: serde_json::Value,
}

#[cfg(test)]
#[path = "mesh_tests.rs"]
mod tests;

/// Rebuild a scanned mesh as the stack of extruded layers it was modelled from: a sketch per
/// level, each outline fitted with lines, arcs and circles, squared up and rounded to a grid.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MeshReconstructParams {
    /// The mesh body, by name. The selected mesh body when left out.
    pub body: Option<String>,
    /// The scan's own noise: how far a measured point may sit off the surface it belongs to, and
    /// so how close a line or an arc has to come before it counts as fitting (0.25 mm by default,
    /// between 0.001 and 10 mm). A clean export from another CAD program wants a small one; a
    /// photogrammetry scan wants a large one.
    pub tolerance: Option<LengthExpr>,
    /// Round the finished dimensions to this step (0.1 mm by default, at most 10 mm). Zero leaves
    /// them exactly as measured. This is what turns a 59.94 mm reading back into a 60 mm part.
    pub grid: Option<LengthExpr>,
    /// Which mirror planes to look for and average the part across: `auto` (both, the default),
    /// `x`, `y`, or `off`.
    pub symmetry: Option<String>,
    /// Snap an edge's direction onto a multiple of this many degrees where it is already within
    /// three degrees of one (15 by default; 0 leaves directions as measured).
    pub snap_angle: Option<f64>,
    /// The thinnest layer kept (0.6 mm by default). Two levels closer together than this are read
    /// as one: a scan's face is not flat enough to tell a shallower step from its own noise.
    pub min_thickness: Option<LengthExpr>,
    /// The direction to build along, as `[x, y, z]`. Left out, the stack takes the direction the
    /// most facet area lies square to, which on a plate is the one between its two faces.
    pub axis: Option<[f64; 3]>,
    /// Name each layer's thickness and each circle's diameter as a user parameter the features
    /// follow (true by default). An outline's own corners stay as numbers: they came off the scan
    /// and carry no dimension to hold them.
    pub parameters: Option<bool>,
    /// Those parameters' name before `_layer_1_thickness` and the rest. From the body name when
    /// left out.
    pub prefix: Option<String>,
    /// Report the plan and the commands it would run, and change nothing (false by default).
    pub plan_only: Option<bool>,
    /// A name for the first extrusion.
    pub name: Option<String>,
}

/// What `mesh.reconstruct` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshReconstructResult {
    /// The mesh body that was read.
    pub body: String,
    /// Where the sketches sit: the part's lowest level, at the datum its outlines were measured
    /// from.
    pub origin: [f64; 3],
    /// The direction the layers stack along, and the sketches' own axes.
    pub axis: [f64; 3],
    pub x_dir: [f64; 3],
    pub y_dir: [f64; 3],
    /// The heights the part steps between, from the sketch plane (mm).
    pub levels: Vec<f64>,
    /// The rebuilt part's size along `x_dir`, `y_dir` and `axis` (mm), after rounding to the
    /// grid: the part's size, not the scan's.
    pub size: [f64; 3],
    pub slabs: Vec<MeshSlab>,
    /// The mirror planes the part was found symmetric about, and made exactly symmetric across.
    pub mirrors: Vec<MeshMirror>,
    /// Triangles the scan was read from, after welding.
    pub triangles: usize,
    /// The scan's area (mm^2).
    pub area: f64,
    /// How far the finished plan sits from the scan it came from (mm). This is the number that
    /// says whether the part really was a stack of layers: a plate comes back within its own
    /// scanning noise, and a curved shell forced into slabs comes back millimetres out.
    pub deviation: Option<MeshDeviation>,
    /// How many outlines would not fit lines and arcs and were kept as traced polygons instead.
    /// Nothing about those is a dimension to trust.
    pub traced: usize,
    pub parameters: Vec<MeshParameter>,
    /// The commands the rebuild runs, in order.
    pub steps: Vec<MeshStep>,
    /// The features built, unless `plan_only`.
    pub features: Vec<String>,
}

/// One extruded layer of a rebuilt part.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshSlab {
    /// Where the layer starts and ends along the build axis, from the sketch plane (mm).
    pub from: f64,
    pub to: f64,
    pub loops: Vec<MeshLoop>,
}

/// One closed outline of a layer.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshLoop {
    /// How many lines the outline was fitted with, and how many arcs.
    pub lines: usize,
    pub arcs: usize,
    /// The radius, where the whole outline is one circle (mm).
    pub circle: Option<f64>,
    /// The area the outline encloses (mm^2), signed: negative where it is a hole in the outline
    /// around it.
    pub area: f64,
    /// How far the fitted outline sits from the points it was measured from (mm), at the 95th
    /// percentile.
    pub deviation: f64,
    /// Whether lines and arcs fitted at all. False where the outline was kept as a traced
    /// polygon because nothing else would close.
    pub fitted: bool,
}

/// A mirror plane the part was found to be symmetric about.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshMirror {
    /// Which sketch axis the plane is square to: `x` or `y`.
    pub across: String,
    /// Where the plane sits along that axis, in sketch coordinates (mm).
    pub at: f64,
    /// The median distance between the scan and its own reflection (mm): how true the symmetry
    /// is.
    pub mismatch: f64,
    /// How many measured features were averaged with their partner across the plane.
    pub paired: usize,
}

/// How far a rebuilt part sits from the scan it was read from (mm).
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshDeviation {
    pub mean: f64,
    pub p95: f64,
    pub max: f64,
    /// How many scan points were measured.
    pub probes: usize,
}
