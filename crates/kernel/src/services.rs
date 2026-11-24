//! solidcore's services that the application uses beside the body-making operations: 2D and
//! curve helpers, projection, hatching, affine maps, sheet development, frame members, the
//! display plan, and STEP's string and parser helpers. They read records and data; none
//! makes a body, so none is routed (`docs/design/kernel-layer.md` §2).
//!
//! The rest of SolidCraft takes them from here, never from solidcore's crates directly: this
//! crate is solidcore's adapter, and `cargo xtask layers` keeps the other crates off solidcore.
//! Grouped as `solidcore-ops` groups them, under the same module names, so a call keeps its
//! path below `services`.

use solidcore_record::Vec3;

use crate::body::Body;

pub use solidcore_ops::ClassifiedPoint;
pub use solidcore_ops::Error;
pub use solidcore_ops::FailureCode;
pub use solidcore_ops::PointNormalFace;
pub use solidcore_ops::SquareFaceLevel;
pub use solidcore_ops::affine;
pub use solidcore_ops::cancel;
pub use solidcore_ops::cell_grid;
pub use solidcore_ops::centroid_and_mean_radius;
pub use solidcore_ops::conic_projection;
pub use solidcore_ops::curve;
pub use solidcore_ops::curve_fit;
pub use solidcore_ops::develop;
pub use solidcore_ops::export_geometry;
pub use solidcore_ops::face_surface;
pub use solidcore_ops::fit_projection;
pub use solidcore_ops::frame_gusset;
pub use solidcore_ops::frame_member;
pub use solidcore_ops::frame_trim;
pub use solidcore_ops::hatch;
pub use solidcore_ops::hole_stacks;
pub use solidcore_ops::import_geometry;
pub use solidcore_ops::outline;
pub use solidcore_ops::path_export;
pub use solidcore_ops::pcurve;
pub use solidcore_ops::point_displacement;
pub use solidcore_ops::point_distance;
pub use solidcore_ops::point_identity;
pub use solidcore_ops::prism;
pub use solidcore_ops::query;
pub use solidcore_ops::session;
pub use solidcore_ops::spline_projection;
pub use solidcore_ops::sweep;

/// Centroids of `tool`'s planar faces that lie back to back on a planar face of `target`
/// (`solidcore_ops::flush_contact_points`). `None` where either body's record doesn't convert or
/// the kernel fails.
pub fn flush_contact_points(target: &Body, tool: &Body) -> Option<Vec<[f64; 3]>> {
    let (target, _) = solidcore_ops::Body::from_record(target.record(), true).ok()?;
    let (tool, _) = solidcore_ops::Body::from_record(tool.record(), true).ok()?;
    solidcore_ops::flush_contact_points(&target, &tool).ok()
}

/// How far record edge `edge` of `body`, on its exact trimmed curve, passes from `point`. `None`
/// where the record doesn't convert, the index is past the edges, or the kernel finds no point.
pub fn exact_edge_distance(body: &Body, edge: usize, point: Vec3) -> Option<f64> {
    let (exact, order) = solidcore_ops::Body::from_record(body.record(), true).ok()?;
    exact.edge_closest_point(&order, edge, [point.x, point.y, point.z]).ok()?.map(|p| p.distance)
}

/// The display plan and meshes (`solidcore-tess`).
pub mod tess {
    pub use solidcore_tess::EvalCurve;
    pub use solidcore_tess::EvalSurface;
    pub use solidcore_tess::FacePlan;
    pub use solidcore_tess::Mesh;
    pub use solidcore_tess::PoleNode;
    pub use solidcore_tess::PoleRule;
    pub use solidcore_tess::ShapeLimit;
    pub use solidcore_tess::TessPlan;
    pub use solidcore_tess::Unshaped;
    pub use solidcore_tess::mesh;
    pub use solidcore_tess::plan;
    pub use solidcore_tess::simplify;
    pub use solidcore_tess::triangulate_planar;
    pub use solidcore_tess::triangulate_union;
}

/// STEP's helpers that the application's own exchange code uses: the part 21 parser (a header's
/// producer, the memory a file needs) and the string escape a product name is written with.
pub mod step {
    pub use solidcore_record::step::p21;
    pub use solidcore_step::write::string;
}
