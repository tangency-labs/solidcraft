//! Display normals at a pole of higher order: a boundary node where the kernel's evaluator has
//! no normal ([`EvalError::Degenerate`]: `S_u × S_v` and its first-order limit vanish there).
//!
//! The planner's layout is kept as it is. The plan names each such node ([`PoleNode`]) with the
//! normal it gets:
//!
//! - [`PoleRule::Limit`]: the node is emitted once per incident triangle, and each copy gets the
//!   kernel's exact limit normal ([`EvalSurface::normal_along`]) at the triangle's own point of
//!   the pole, along the triangle's direction into it ([`foot`]). The mesh is then watertight
//!   by position there, not by index.
//! - [`PoleRule::Averaged`], where the kernel refuses that limit for some triangle (a pole of
//!   order four or more, or any other refusal): the node stays one vertex, and its normal is the
//!   area-weighted mean of the normals of the face's triangles at the pole point. Each such
//!   node is named in [`crate::PlanStats::averaged_poles`] with the kernel's reason.
//!
//! Either is a display normal and decides nothing a model depends on.

use solidcore_record::Vec3;

/// A boundary node of a face where its surface has no normal, with the normal it gets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoleNode {
    /// The node's index in [`FacePlan::uv`].
    pub node: u32,
    /// What its triangles get there.
    pub rule: PoleRule,
}

/// The normal a [`PoleNode`] gets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PoleRule {
    /// One copy of the node per incident triangle, with the surface's limit normal along that
    /// triangle's direction into the pole.
    Limit {
        /// The parameter the pole is collapsed along (0: u, 1: v): along it every value is the
        /// pole point.
        along: usize,
        /// How near a pole a point is taken as one: the face's, as the planner classified it.
        reach: f64,
    },
    /// One vertex, with the area-weighted mean of the normals of the face's triangles at the
    /// pole point: the kernel refused the limit ([`AveragedPole`]).
    Averaged,
}

/// A pole whose normal is the mean of its triangles' normals, because the kernel refused its
/// limit normal (see [`crate::PlanStats::averaged_poles`]).
#[derive(Clone, Debug, PartialEq)]
pub struct AveragedPole {
    /// The face's index in the record.
    pub face: usize,
    /// The parameters of the face's nodes at the pole point (the planner puts a pole at each
    /// end of its collapsed side).
    pub uv: Vec<[f64; 2]>,
    /// The pole point.
    pub point: Vec3,
    /// Why the kernel has no limit normal there.
    pub reason: String,
}
