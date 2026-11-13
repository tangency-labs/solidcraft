//! # solidcore-tess
//!
//! Geometry made drawable (docs/layers.md §1): the one mesher of solidcore and the applications
//! built on it. Each face of a B-rep record ([`solidcore_record::brep::Brep`]) is triangulated in
//! its own (u, v), every edge sampled once so neighbouring faces share their boundary points
//! exactly, and handed over as a [`TessPlan`]: the parameters and triangles of every face with
//! the surfaces to evaluate them on, in the kernel evaluator's form ([`solidcore::eval`]). The
//! CPU evaluates a plan into a [`Mesh`] ([`TessPlan::evaluate`]); a GPU evaluates the same plan
//! in a compute shader.
//!
//! - [`plan`]: the face planner and its display entry point, at a [`MeshTolerance`];
//! - [`mesh()`]: the mesh path, not for display: every face by constrained Delaunay with lattice
//!   seeds and Ruppert shaping to a [`MeshSpec`], for mesh bodies, mesh export and analysis
//!   meshers, slow by design; [`TessPlan::welded`] joins its faces at their shared vertices;
//! - [`TessPlan`], [`FacePlan`]: the plan, and [`TessPlan::transformed`] for a placed copy;
//! - [`Mesh`]: an indexed triangle mesh with its edge polylines and their display flags;
//! - [`curve_points`]: a free curve's points at a [`MeshTolerance`], by the edge rules;
//! - [`simplify`]: display levels of detail of a large mesh;
//! - [`preview`]: a bounded coarse mesh of a decoded body.
//!
//! A display mesh decides nothing a model depends on: it is drawn, never measured.
//!
//! Library code never panics, unwraps or indexes on input-derived data: a face the planner
//! cannot mesh is named in its [`PlanStats`], and the rest of the body is meshed.

mod curve;
#[cfg(test)]
mod fixtures;
mod hash;
mod iso;
mod mesh;
// SolidCraft's mesher, moved here (RFC 0003, item 1.1) and kept in SolidCraft's formatting so it
// reads the same as its history.
#[rustfmt::skip]
mod mesher;
mod par;
mod plan;
mod pole;
pub mod preview;
mod ring;
pub mod simplify;
mod spec;
mod stats;
mod tolerance;
mod vec;

pub use curve::curve_points;
pub use iso::{MAX_ISOPARMS, isoparms};
pub use mesh::Mesh;
pub use mesher::guided::{DisplayGuide, DisplayGuidePoint, GuidedMesh, mesh_with_guides};
pub use mesher::{MAX_ANGLE_BOUNDARY, plan};
pub use par::guarded;
pub use plan::{FacePlan, TessPlan};
pub use pole::{AveragedPole, PoleNode, PoleRule};
pub use spec::{MAX_MESH_MIN_ANGLE, MESH_MIN_ANGLE, MeshSpec, WeldedMesh, mesh};
pub use stats::{
    Cap, CappedEdge, EdgeCap, OverTolerance, PlanStats, RefusedEdge, RefusedFace, ShapeLimit,
    Unshaped,
};
pub use tolerance::{DISPLAY_ANGLE, MeshTolerance};

/// The plan evaluator's curve and surface forms a [`TessPlan`] holds (its `surfaces`), so a
/// client that builds or reads a plan names them through this crate, not the kernel's.
pub use solidcore::eval::plan::{EvalCurve, EvalSurface, SurfaceJet, nurbs_derivs};
/// The vector conversions between the record and the evaluator that plan surfaces are built with.
pub use solidcore_convert::{record_vec3, vec3};

/// Why a body could not be planned. The messages are the ones the mesher has always reported.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum TessError {
    /// The record is malformed where the mesher reads it.
    #[error("invalid input: {0}")]
    Invalid(String),
    /// The mesher could not plan it (a face, an edge, a kind not supported yet).
    #[error("the operation failed: {0}")]
    Failed(String),
    /// A panic inside the mesher, caught there.
    #[error("kernel internal error: {0}")]
    Internal(String),
}

/// The mesher's result.
pub type Result<T> = std::result::Result<T, TessError>;

/// Why the kernel's evaluator refused a curve or surface, in the words the mesher reports.
pub(crate) fn eval_message(e: solidcore::eval::EvalError) -> String {
    match e {
        solidcore::eval::EvalError::Unsupported(what) => format!("not supported yet: {what}"),
        solidcore::eval::EvalError::Invalid(what)
        | solidcore::eval::EvalError::TooLarge(what)
        | solidcore::eval::EvalError::Unevaluable(what)
        | solidcore::eval::EvalError::Degenerate(what)
        | solidcore::eval::EvalError::OffRange(what) => what,
    }
}

/// An empty `Vec` with room for `n` items, or `None` where that much cannot be reserved (where
/// `Vec::with_capacity` would panic).
pub(crate) fn with_room<T>(n: usize) -> Option<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve(n).ok()?;
    Some(v)
}

#[cfg(test)]
mod reexport_tests {
    use solidcore_record::Vec3;

    /// A plan's surfaces, read through this crate's names alone.
    fn surfaces(plan: &crate::TessPlan) -> &[crate::EvalSurface] {
        &plan.surfaces
    }

    #[test]
    fn a_plans_surfaces_are_built_and_read_through_this_crate() {
        let plane = crate::EvalSurface::Plane {
            origin: crate::vec3(Vec3::new(1.0, 2.0, 3.0)),
            du: crate::vec3(Vec3::new(1.0, 0.0, 0.0)),
            dv: crate::vec3(Vec3::new(0.0, 1.0, 0.0)),
        };
        let crate::EvalSurface::Plane { origin, .. } = &plane else {
            panic!("{plane:?}")
        };
        assert_eq!(crate::record_vec3(*origin), Vec3::new(1.0, 2.0, 3.0));
        let plan = crate::TessPlan {
            surfaces: vec![plane.clone()],
            ..Default::default()
        };
        assert_eq!(surfaces(&plan), [plane]);
    }
}
