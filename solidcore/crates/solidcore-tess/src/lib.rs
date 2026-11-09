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
//! - [`TessPlan`], [`FacePlan`]: the plan, and [`TessPlan::transformed`] for a placed copy;
//! - [`Mesh`]: an indexed triangle mesh with its edge polylines and their display flags;
//! - [`simplify`]: display levels of detail of a large mesh;
//! - [`preview`]: a bounded coarse mesh of a decoded body.
//!
//! A display mesh decides nothing a model depends on: it is drawn, never measured.
//!
//! Library code never panics, unwraps or indexes on input-derived data: a face the planner
//! cannot mesh is named in its [`PlanStats`], and the rest of the body is meshed.

mod mesh;
mod par;
mod plan;
mod planner;
mod pole;
pub mod preview;
mod ring;
pub mod simplify;
mod tolerance;
mod vec;

pub use mesh::Mesh;
pub use par::guarded;
pub use plan::{FacePlan, TessPlan};
pub use planner::{
    Cap, CappedEdge, EdgeCap, MAX_ANGLE_BOUNDARY, OverTolerance, PlanStats, RefusedEdge,
    RefusedFace, plan,
};
pub use pole::{AveragedPole, PoleNode, PoleRule};
pub use tolerance::{DISPLAY_ANGLE, MeshTolerance};

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
        | solidcore::eval::EvalError::Degenerate(what) => what,
    }
}

/// An empty `Vec` with room for `n` items, or `None` where that much cannot be reserved (where
/// `Vec::with_capacity` would panic).
pub(crate) fn with_room<T>(n: usize) -> Option<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve(n).ok()?;
    Some(v)
}
