//! solidcore's services that the application uses beside the body-making operations: 2D and
//! curve helpers, projection, line-family clipping, affine maps, sheet development and the
//! display plan. They read records and data; none
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
pub use solidcore_ops::conic_projection;
pub use solidcore_ops::constraint;
pub use solidcore_ops::curve;
pub use solidcore_ops::curve_fit;
pub use solidcore_ops::develop;
pub use solidcore_ops::dxf;
pub use solidcore_ops::face_pair;
pub use solidcore_ops::face_surface;
pub use solidcore_ops::fit_projection;
pub use solidcore_ops::hole_stacks;
pub use solidcore_ops::joint_turn;
pub use solidcore_ops::mid_joint;
pub use solidcore_ops::mid_sheet;
pub use solidcore_ops::outline;
pub use solidcore_ops::paper_gaps;
pub use solidcore_ops::pcurve;
pub use solidcore_ops::point_identity;
pub use solidcore_ops::prism;
pub use solidcore_ops::profile;
pub use solidcore_ops::query;
pub use solidcore_ops::section_corners;
pub use solidcore_ops::session;
pub use solidcore_ops::sketch_arc;
pub use solidcore_ops::spline_projection;
pub use solidcore_ops::sweep;
pub use solidcore_ops::unit_direction;
pub use solidcore_ops::{Body as ExactBody, CheckLevel, Order, Result, boolean, make_box, make_sphere, measure};

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
