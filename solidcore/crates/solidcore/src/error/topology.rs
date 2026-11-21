//! Why a topology routine fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the topology subsystem (`crate::topology`:
//! the B-rep constructor, cache and data streams, faces, edges, edge uses, loop uses,
//! vertex uses, shells, trimming tools, cutting, mirroring, planar sections, vertex fans,
//! projection, traversal and the file wrappers). Each also records, for difftest only,
//! the status the C++ reports for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                                 | Status difftest compares      |
//! |--------------------------------------|-------------------------------|
//! | [`TopologyFailure::Failed`]          | `SM_ERR`                      |
//! | [`TopologyFailure::Missing`]         | `SM_ERR_NULL_POINTER`         |
//! | [`TopologyFailure::InvalidValue`]    | `SM_ERR_INVALID_INPUT`        |
//! | [`TopologyFailure::Inconsistent`]    | `SM_ERR_ASSERT_FAILURE`       |
//! | [`TopologyFailure::OutOfMemory`]     | `SM_ERR_OUT_OF_MEMORY`        |
//! | [`TopologyFailure::DropFailed`]      | the drop's own recorded status |
//! | [`TopologyFailure::JoinedVertexReaches`] | `SM_ERR` (not upstream, #15718) |
//! | [`TopologyFailure::Defect`]              | `SM_ERR` (or the site's, through `with_status`) |
//! | the named causes, from [`TopologyFailure::EditingDisabled`] on | the status of the phrase each replaced (`comparison_status`), or the site's through `with_status` |
//!
//! [`TopologyFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes are sites
//! not yet converted (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. A failure says in a phrase (or by its variant) what went
//! wrong; the failure's `operation` path says which routine, and its entity is the one
//! the site it replaces named (a vertex use for the vertex-use routines, none
//! elsewhere). A check of a caller's argument that reports no status (upstream
//! dereferences null) stays [`crate::SolidError::InvalidInput`]; one that reports a status
//! is a named cause of class `invalid_input`, keeping that status.
//! `brep/p9`'s face-orientation repair keeps its bare final status 1 (upstream `TRUE`),
//! which no failure here stands for.

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_ASSERT_FAILURE, SM_ERR_INVALID_INPUT,
    SM_ERR_NULL_POINTER, SM_ERR_OUT_OF_MEMORY,
};

/// Why a topology routine fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum TopologyFailure {
    /// The routine could not continue, as the phrase says (for example "the loop does
    /// not close").
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// edge's curve").
    Missing(&'static str),
    /// A value the routine is given or computes with is not usable, as the phrase says
    /// (for example "the plane normal has zero length"). Upstream returns
    /// `SM_ERR_INVALID_INPUT` as a status here, so its kind is
    /// [`ErrorKind::InvalidInput`].
    InvalidValue(&'static str),
    /// A consistency check of the routine's own data did not hold, as the phrase says.
    Inconsistent(&'static str),
    /// Memory for the data could not be had, as the phrase says.
    OutOfMemory(&'static str),
    /// Dropping the edge use's curve onto its face failed earlier, with `status`, and the
    /// edge use keeps that failure.
    DropFailed {
        /// The status the drop recorded (a comparison detail).
        status: i32,
    },
    /// A vertex at a joined edge needs tolerance `needed` to cover the gap the join left
    /// between it and the edge's curve end (twice the gap), and that ball would reach the far
    /// vertex of one of its edges, which it does not bound. A tolerance below `fits_below`
    /// fits (the distance less that vertex's tolerance, the smallest over its edges); `None`
    /// when none does.
    JoinedVertexReaches {
        /// The tolerance the vertex needs.
        needed: f64,
        /// The tolerance that fits, if any.
        fits_below: Option<f64>,
    },
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// The routine edits the B-rep, and the caller has not enabled editing on it
    /// (`Brep::set_editing_enabled`). The caller's state: [`ErrorKind::InvalidInput`].
    EditingDisabled,
    /// The caller gave no B-rep. [`ErrorKind::InvalidInput`].
    NoBrepGiven,
    /// The caller gave no region. [`ErrorKind::InvalidInput`].
    NoRegionGiven,
    /// The caller gave no edge use. [`ErrorKind::InvalidInput`].
    NoEdgeuseGiven,
    /// An entity the caller named (a region, a face) is not one of the B-rep's.
    /// [`ErrorKind::InvalidInput`].
    NotInBrep,
    /// A vertex the caller named for an edge has no shell in the region the caller named
    /// (the infinite region when the caller names none). [`ErrorKind::InvalidInput`].
    VertexNotInRegion {
        /// Whether it is the edge's end vertex (else its start vertex).
        at_end: bool,
    },
    /// The B-rep the caller gave has no infinite region. [`ErrorKind::InvalidInput`].
    NoInfiniteRegion,
    /// The infinite region of the B-rep the caller gave is not first on its region list.
    /// [`ErrorKind::InvalidInput`].
    InfiniteRegionNotFirst,
    /// The vertex the caller named to survive is not a vertex of the edge.
    /// [`ErrorKind::InvalidInput`].
    VertexNotOnEdge,
    /// The edge starts and ends at one vertex, which the routine does not take.
    /// [`ErrorKind::InvalidInput`].
    ClosedEdge,
    /// The edge is not lamina (bounding one face only), which the routine needs.
    /// [`ErrorKind::InvalidInput`].
    NotLamina,
    /// A neighbour of the lamina edge in its loop is neither lamina nor a seam of the
    /// face's surface, so the face cannot be stretched to the mating surface there.
    /// [`ErrorKind::InvalidInput`].
    LaminaNeighbourBound,
    /// A corner blend was given `given` edge uses, fewer than the two it needs.
    /// [`ErrorKind::InvalidInput`].
    TooFewEdgeuses {
        /// The number of edge uses given.
        given: usize,
    },
    /// The edge uses given for a corner blend do not join end to end, even once the match
    /// tolerance has grown past a hundredfold. [`ErrorKind::InvalidInput`].
    EdgeuseChainGap,
    /// The hit the caller gave names no face, edge or vertex. [`ErrorKind::InvalidInput`].
    HitNotOnBoundary,
    /// A face of the caller's body has no surface. [`ErrorKind::InvalidInput`].
    FaceWithoutSurface,
    /// A loop the marking crosses has an edge that is a wire, lamina or non-manifold,
    /// across which the marking cannot follow the face's neighbours.
    /// [`ErrorKind::Unsupported`].
    EdgeNotManifold,
    /// A curve the routine needs as a B-spline is of a kind with no B-spline form here.
    /// [`ErrorKind::Unsupported`].
    CurveNotBSpline {
        /// The curve's owner, in the B-rep the routine read it from, where it has one.
        entity: Option<EntityRef>,
    },
    /// An edge gave no B-spline copy of its curve trimmed to its interval: it has no curve,
    /// its curve has no B-spline form, or the trim failed. Class `failed`.
    NoTrimmedBSpline,
    /// A surface has no NURBS form. [`ErrorKind::Unsupported`].
    SurfaceWithoutNurbs {
        /// The face the surface belongs to, where it has one.
        entity: Option<EntityRef>,
    },
    /// The face's upward face use has no edge use (its loops are vertex loops, or it has
    /// none), so no edge gives a direction to vote with. [`ErrorKind::Unsupported`].
    FaceWithoutEdges,
    /// The edge to squeeze ends a wire at the vertex to delete, so no other edge there can
    /// take the head of the wire's shell. [`ErrorKind::Unsupported`].
    WireEndSqueezed,
    /// An end of the edge use's UV trim curve, lifted by its face's surface, lies
    /// `distance` from the edge's vertex or curve end there, past `tolerance` (the face's
    /// tolerance plus the vertex's or the edge's). [`ErrorKind::NotWithinTolerance`].
    TrimCurveOffEnd {
        /// Whether it is the trim curve's end (else its start).
        at_end: bool,
        /// Whether it is measured to the vertex (else to the edge curve's end).
        to_vertex: bool,
        /// The distance between the lifted end and the vertex or curve end.
        distance: f64,
        /// The tolerance it exceeds.
        tolerance: f64,
    },
    /// The side of the curve the point lies on is left open, for the reason given.
    /// Class `failed`.
    SideUndecided {
        /// Why the side is open.
        why: SideOpen,
    },
    /// A search for the point's foot on the face's surface found none.
    /// Class `failed`.
    NoFootOnSurface,
    /// The test vector has zero length: the point to classify lies on the face's surface,
    /// or the test direction given is zero, so there is no direction to classify along.
    /// Class `failed`.
    TestVectorZero,
    /// The cache of the face's surface (with the face's trim) could not be made; the error
    /// that stopped it is the cause. Class `failed`; whether it is a defect or a refusal is
    /// its cause's.
    SurfaceCacheNotMade,
    /// The curve's crossing of an edge falls in none of the edge's radial sectors.
    /// Class `failed`.
    NoRadialSector,
    /// The nearest hit of the ray is `distance` along it, below `tolerance`: at the ray's
    /// start or behind it. Class `failed`.
    HitAtRayStart {
        /// The ray parameter of the nearest hit.
        distance: f64,
        /// The distance below which a hit counts as at the start.
        tolerance: f64,
    },
    /// The classification could not decide whether a maybe-coincident interval is
    /// coincident. Class `failed`.
    CoincidenceUndecided,
    /// No curve of the intersection of the extended surfaces meets both of the lamina
    /// edge's extended neighbours. Class `failed`.
    NoMatingCurve,
    /// The edge's curve could be neither lifted nor approximated as a B-spline (the
    /// approximation's failure is the cause). Class `failed`; whether it is a defect or a
    /// refusal is its cause's ([`crate::SolidError::is_defect`] follows the chain).
    CurveNotApproximated,
    /// Gathering the face uses connected to one side of a face reached more face uses than
    /// faces remain to be oriented (upstream's "Nonorientable Brep" assert): the body is not
    /// orientable, or it is an open sheet the closed-solid orientation does not take. Class
    /// `failed`.
    GatheredBothSides,
    /// The edge to delete is neither a wire, a lamina nor a manifold edge (for example a
    /// spine edge, which more than two faces share): the deletion takes only those.
    /// [`ErrorKind::Unsupported`].
    EdgeNotDeletable,
    /// Both sides of the manifold edge to delete are two different inner loops: the deletion
    /// merges faces only through an outer loop. As upstream, it finds this after the edge
    /// is deleted, so the B-rep is left edited. [`ErrorKind::Unsupported`].
    InnerLoopsOnBothSides,
    /// The lamina edge to delete is a strut at an end (its two edge uses follow each other
    /// in the loop there), which the lamina deletion does not take.
    /// [`ErrorKind::Unsupported`].
    LaminaEdgeIsStrut,
    /// The vertex to delete still has edges; the caller deletes them first.
    /// [`ErrorKind::InvalidInput`].
    VertexHasEdges,
}

/// Why [`TopologyFailure::SideUndecided`] leaves the side of a curve open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SideOpen {
    /// The point is on the curve, within the tolerance.
    OnCurve,
    /// The curve's tangent, seen along the projection direction, is zero.
    TangentAlongProjection,
    /// The point lies along the curve's tangent at its foot.
    AlongTangent,
    /// The point does not project onto the curve exactly once: the projection found no
    /// foot (the curve is nearest at an end) or more than one.
    NotProjectedOnce,
    /// The point's one foot is an end of the open curve, and the point lies beyond that
    /// end, on neither side of the curve.
    BeyondEnd,
}

impl core::fmt::Display for SideOpen {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::OnCurve => "the point is on the curve",
            Self::TangentAlongProjection => {
                "the curve tangent is zero or along the projection direction"
            }
            Self::AlongTangent => "the point lies along the curve tangent",
            Self::NotProjectedOnce => "the point does not project onto the curve exactly once",
            Self::BeyondEnd => "the point lies beyond the end of the curve",
        })
    }
}

impl TopologyFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_)
            | Self::JoinedVertexReaches { .. }
            | Self::Defect { .. }
            | Self::EditingDisabled
            | Self::NotInBrep
            | Self::VertexNotInRegion { .. }
            | Self::NoInfiniteRegion
            | Self::InfiniteRegionNotFirst
            | Self::VertexNotOnEdge
            | Self::ClosedEdge
            | Self::NotLamina
            | Self::LaminaNeighbourBound
            | Self::TooFewEdgeuses { .. }
            | Self::EdgeuseChainGap
            | Self::HitNotOnBoundary
            | Self::EdgeNotManifold
            | Self::FaceWithoutEdges
            | Self::WireEndSqueezed
            | Self::TrimCurveOffEnd { .. }
            | Self::SideUndecided { .. }
            | Self::NoFootOnSurface
            | Self::TestVectorZero
            | Self::NoRadialSector
            | Self::HitAtRayStart { .. }
            | Self::CoincidenceUndecided
            | Self::NoMatingCurve
            | Self::EdgeNotDeletable
            | Self::InnerLoopsOnBothSides
            | Self::LaminaEdgeIsStrut
            | Self::VertexHasEdges => SM_ERR,
            Self::Missing(_)
            | Self::NoEdgeuseGiven
            | Self::FaceWithoutSurface
            | Self::CurveNotBSpline { .. }
            | Self::SurfaceWithoutNurbs { .. }
            | Self::CurveNotApproximated
            | Self::SurfaceCacheNotMade => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_)
            | Self::NoBrepGiven
            | Self::NoRegionGiven
            | Self::NoTrimmedBSpline => SM_ERR_INVALID_INPUT,
            Self::Inconsistent(_) | Self::GatheredBothSides => SM_ERR_ASSERT_FAILURE,
            Self::OutOfMemory(_) => SM_ERR_OUT_OF_MEMORY,
            Self::DropFailed { status } => *status,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`TopologyFailure::Missing`], [`ErrorKind::InvalidInput`] for
    /// [`TopologyFailure::InvalidValue`] and for a check of the caller's arguments, state or
    /// body, [`ErrorKind::Unsupported`] for a kind or configuration the routine does not
    /// handle, [`ErrorKind::NotWithinTolerance`] for a trim curve end off its edge, the
    /// drop status's kind for [`TopologyFailure::DropFailed`], [`ErrorKind::Failed`]
    /// otherwise. [`TopologyFailure::Defect`] is [`ErrorKind::StageFailed`], a defect.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::EditingDisabled
            | Self::NoBrepGiven
            | Self::NoRegionGiven
            | Self::NoEdgeuseGiven
            | Self::NotInBrep
            | Self::VertexNotInRegion { .. }
            | Self::NoInfiniteRegion
            | Self::InfiniteRegionNotFirst
            | Self::VertexNotOnEdge
            | Self::ClosedEdge
            | Self::NotLamina
            | Self::LaminaNeighbourBound
            | Self::TooFewEdgeuses { .. }
            | Self::EdgeuseChainGap
            | Self::HitNotOnBoundary
            | Self::FaceWithoutSurface
            | Self::VertexHasEdges => ErrorKind::InvalidInput,
            Self::EdgeNotManifold
            | Self::CurveNotBSpline { .. }
            | Self::SurfaceWithoutNurbs { .. }
            | Self::FaceWithoutEdges
            | Self::WireEndSqueezed
            | Self::EdgeNotDeletable
            | Self::InnerLoopsOnBothSides
            | Self::LaminaEdgeIsStrut => ErrorKind::Unsupported,
            Self::TrimCurveOffEnd { .. } => ErrorKind::NotWithinTolerance,
            Self::CurveNotApproximated | Self::SurfaceCacheNotMade | Self::NoTrimmedBSpline => {
                ErrorKind::Failed
            }
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for TopologyFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) | Self::InvalidValue(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
            Self::OutOfMemory(what) => write!(f, "out of memory for {what}"),
            Self::DropFailed { .. } => {
                f.write_str("the edge use's curve could not be dropped onto its face")
            }
            Self::JoinedVertexReaches {
                needed,
                fits_below: Some(fits),
            } => write!(
                f,
                "a joined edge's vertex needs tolerance {needed} to cover the join's gap, which \
                 reaches the far vertex of one of its edges; a tolerance below {fits} fits"
            ),
            Self::JoinedVertexReaches {
                needed,
                fits_below: None,
            } => write!(
                f,
                "a joined edge's vertex needs tolerance {needed} to cover the join's gap, which \
                 reaches the far vertex of one of its edges; no tolerance fits"
            ),
            Self::EditingDisabled => f.write_str("editing is not enabled on the B-rep"),
            Self::NoBrepGiven => f.write_str("no B-rep was given"),
            Self::NoRegionGiven => f.write_str("no region was given"),
            Self::NoEdgeuseGiven => f.write_str("no edge use was given"),
            Self::NotInBrep => f.write_str("the entity given is not one of the B-rep's"),
            Self::VertexNotInRegion { at_end } => write!(
                f,
                "the {} vertex given has no shell in the region given",
                if *at_end { "end" } else { "start" }
            ),
            Self::NoInfiniteRegion => f.write_str("the B-rep has no infinite region"),
            Self::InfiniteRegionNotFirst => {
                f.write_str("the infinite region is not first on the B-rep's region list")
            }
            Self::VertexNotOnEdge => f.write_str("the surviving vertex is not on the edge"),
            Self::ClosedEdge => f.write_str("the edge starts and ends at one vertex"),
            Self::NotLamina => f.write_str("the edge is not a lamina edge"),
            Self::LaminaNeighbourBound => {
                f.write_str("a neighbour of the lamina edge is neither lamina nor a seam")
            }
            Self::TooFewEdgeuses { given } => {
                write!(
                    f,
                    "a corner blend needs two edge uses or more; {given} given"
                )
            }
            Self::EdgeuseChainGap => {
                f.write_str("the corner blend's edge uses do not join end to end")
            }
            Self::HitNotOnBoundary => f.write_str("the hit names no face, edge or vertex"),
            Self::FaceWithoutSurface => f.write_str("a face has no surface"),
            Self::EdgeNotManifold => {
                f.write_str("a loop to delete has a wire, lamina or non-manifold edge")
            }
            Self::CurveNotBSpline { entity: None } => f.write_str("a curve has no B-spline form"),
            Self::CurveNotBSpline { entity: Some(e) } => {
                write!(f, "a curve has no B-spline form ({e})")
            }
            Self::NoTrimmedBSpline => {
                f.write_str("an edge gave no B-spline of its curve trimmed to its interval")
            }
            Self::SurfaceWithoutNurbs { entity: None } => {
                f.write_str("a surface has no NURBS form")
            }
            Self::SurfaceWithoutNurbs { entity: Some(e) } => {
                write!(f, "a surface has no NURBS form ({e})")
            }
            Self::FaceWithoutEdges => f.write_str("the face has no edges"),
            Self::WireEndSqueezed => {
                f.write_str("the edge to squeeze ends its wire at the vertex to delete")
            }
            Self::TrimCurveOffEnd {
                at_end,
                to_vertex,
                distance,
                tolerance,
            } => write!(
                f,
                "the UV trim curve's {} lies {distance} from {}, past the tolerance {tolerance}",
                if *at_end { "end" } else { "start" },
                if *to_vertex {
                    "its vertex"
                } else {
                    "the edge curve's end"
                }
            ),
            Self::SideUndecided { why } => write!(f, "{why}"),
            Self::NoFootOnSurface => {
                f.write_str("the point does not project onto the face's surface")
            }
            Self::TestVectorZero => f.write_str(
                "the test point is on the face's surface, or the test direction is zero",
            ),
            Self::SurfaceCacheNotMade => {
                f.write_str("the cache of the face's surface could not be made")
            }
            Self::NoRadialSector => f.write_str("the curve's radial sector has no edge use"),
            Self::HitAtRayStart {
                distance,
                tolerance,
            } => {
                write!(
                    f,
                    "the nearest hit is at the ray's start ({distance} along it, below \
                     {tolerance})"
                )
            }
            Self::CoincidenceUndecided => {
                f.write_str("a coincident interval could not be resolved")
            }
            Self::NoMatingCurve => {
                f.write_str("no intersection curve meets both neighbouring edges")
            }
            Self::CurveNotApproximated => {
                f.write_str("the edge's curve could not be made a B-spline")
            }
            Self::GatheredBothSides => {
                f.write_str("more face uses are connected than faces remain to be oriented")
            }
            Self::EdgeNotDeletable => {
                f.write_str("only a wire, lamina or manifold edge can be deleted")
            }
            Self::InnerLoopsOnBothSides => {
                f.write_str("both sides of the edge are inner loops of different faces")
            }
            Self::LaminaEdgeIsStrut => f.write_str("the lamina edge to delete is a strut"),
            Self::VertexHasEdges => f.write_str("the vertex still has edges"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a topology routine that fails with `failure`.
    #[must_use]
    pub(crate) fn topology(operation: &'static str, failure: TopologyFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a topology routine that fails with `failure`
    /// about `entity`.
    #[must_use]
    pub(crate) fn topology_at(
        operation: &'static str,
        failure: TopologyFailure,
        entity: EntityRef,
    ) -> Self {
        Failure::new(operation, failure).at(entity).into_error()
    }

    /// [`crate::SolidError::Failed`] for a topology routine that could not continue, as
    /// the phrase says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn topology_failed(operation: &'static str, what: &'static str) -> Self {
        Self::topology(operation, TopologyFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a topology routine needs that is not
    /// there, as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream
    /// `NER(ptr)` on a `NULL` pointer).
    #[must_use]
    pub(crate) fn topology_missing(operation: &'static str, what: &'static str) -> Self {
        Self::topology(operation, TopologyFailure::Missing(what))
    }

    /// [`crate::SolidError::Failed`] for a value a topology routine computes with that has
    /// no defined result, as the phrase says, with status [`SM_ERR_INVALID_INPUT`]
    /// (upstream `return(SM_ERR_INVALID_INPUT)`, or `SER` of a vector operation that
    /// returns it).
    #[must_use]
    pub(crate) fn topology_invalid_value(operation: &'static str, what: &'static str) -> Self {
        Self::topology(operation, TopologyFailure::InvalidValue(what))
    }

    /// The topology failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Topology`].
    #[must_use]
    pub fn topology_failure(&self) -> Option<&TopologyFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Topology(t) => Some(t),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SM_ERR_OUTSIDE_OF_DOMAIN;
    use crate::topology::VertexuseId;
    use crate::SolidError;

    #[test]
    fn topology_failures_keep_the_status_kind_and_entity_of_the_upstream_they_replace() {
        let vu = Some(EntityRef::Vertexuse(VertexuseId::from_index(3)));
        let cases = [
            (TopologyFailure::Failed("x"), SM_ERR),
            (TopologyFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (TopologyFailure::InvalidValue("x"), SM_ERR_INVALID_INPUT),
            (TopologyFailure::Inconsistent("x"), SM_ERR_ASSERT_FAILURE),
            (TopologyFailure::OutOfMemory("x"), SM_ERR_OUT_OF_MEMORY),
            (
                TopologyFailure::DropFailed {
                    status: SM_ERR_OUTSIDE_OF_DOMAIN,
                },
                SM_ERR_OUTSIDE_OF_DOMAIN,
            ),
        ];
        for (failure, code) in cases {
            for entity in [None, vu] {
                let typed = match entity {
                    Some(e) => SolidError::topology_at("topology::op", failure, e),
                    None => SolidError::topology("topology::op", failure),
                };
                let before = SolidError::Upstream {
                    operation: "topology::op",
                    code,
                    entity,
                };
                assert_eq!(typed.sm_status(), before.sm_status(), "{failure:?}");
                assert_eq!(typed.upstream_status(), Some(code), "{failure:?}");
                assert_eq!(super::super::upstream_status(&typed), Some(code));
                assert_eq!(typed.kind(), before.kind(), "{failure:?}");
                assert_eq!(typed.operation(), before.operation(), "{failure:?}");
                assert_eq!(typed.entity(), before.entity(), "{failure:?}");
                assert_eq!(typed.is_port_failure(), before.is_port_failure());
                assert_eq!(typed.is_status_answer(), before.is_status_answer());
                assert_eq!(
                    typed.is_unported_or_cancelled(),
                    before.is_unported_or_cancelled()
                );
                assert_eq!(typed.topology_failure(), Some(&failure));
            }
        }
        assert_eq!(
            SolidError::topology_missing("topology::op", "the edge's curve").to_string(),
            "topology::op: the edge's curve is missing"
        );
        assert_eq!(
            SolidError::topology_at(
                "topology::op",
                TopologyFailure::Failed("the vertex use has no point"),
                EntityRef::Vertexuse(VertexuseId::from_index(3)),
            )
            .to_string(),
            format!(
                "topology::op: the vertex use has no point ({})",
                EntityRef::Vertexuse(VertexuseId::from_index(3))
            )
        );
        assert_eq!(
            SolidError::topology_invalid_value("topology::op", "x").kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            SolidError::topology_failed("topology::op", "x").kind(),
            ErrorKind::Failed
        );
        assert_eq!(SolidError::sm_err("topology::op").topology_failure(), None);
    }

    /// A sample of the second set of converted files (`brep/p1`, `p5` to `p10`,
    /// `face/p2`): each keeps the operation, status, kind and entity of its `Upstream`.
    #[test]
    fn the_brep_edit_and_face_samples_keep_their_upstream_answers() {
        let samples = [
            (
                "topology::brep::p1",
                TopologyFailure::NoRegionGiven,
                SM_ERR_INVALID_INPUT,
            ),
            (
                "topology::brep::p5",
                TopologyFailure::Failed("editing is not enabled"),
                SM_ERR,
            ),
            (
                crate::topology::brep::p5::NO_VERTEX_SECTOR_OP,
                TopologyFailure::Missing("a vertex sector at the curve's start"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "topology::brep::p6",
                TopologyFailure::Missing("the face's upward face use"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "topology::brep::p7::unstitch",
                TopologyFailure::Failed("not every face could be copied"),
                SM_ERR,
            ),
            (
                "topology::brep::p8",
                TopologyFailure::InvalidValue("the curve loops are not valid"),
                SM_ERR_INVALID_INPUT,
            ),
            (
                "topology::brep::p9",
                TopologyFailure::Missing("the face use's mate"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "topology::face::trim_curve_breaks",
                TopologyFailure::Failed("the trim curve has fewer than two knots"),
                SM_ERR,
            ),
        ];
        for (operation, failure, code) in samples {
            let typed = SolidError::topology(operation, failure);
            let before = SolidError::Upstream {
                operation,
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{typed:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{typed:?}");
            assert_eq!(typed.kind(), before.kind(), "{typed:?}");
            assert_eq!(typed.operation(), before.operation(), "{typed:?}");
            assert_eq!(typed.entity(), None, "{typed:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
        }
    }

    /// The named causes of the B-rep edits keep the status of the phrase each replaced, and
    /// have their class.
    #[test]
    fn the_brep_edit_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        let op = "topology::op";
        let cases = [
            (TopologyFailure::EditingDisabled, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::NoBrepGiven,
                SM_ERR_INVALID_INPUT,
                K::InvalidInput,
            ),
            (
                TopologyFailure::NoRegionGiven,
                SM_ERR_INVALID_INPUT,
                K::InvalidInput,
            ),
            (
                TopologyFailure::NoEdgeuseGiven,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (TopologyFailure::NotInBrep, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::VertexNotInRegion { at_end: true },
                SM_ERR,
                K::InvalidInput,
            ),
            (TopologyFailure::NoInfiniteRegion, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::InfiniteRegionNotFirst,
                SM_ERR,
                K::InvalidInput,
            ),
            (TopologyFailure::VertexNotOnEdge, SM_ERR, K::InvalidInput),
            (TopologyFailure::ClosedEdge, SM_ERR, K::InvalidInput),
            (TopologyFailure::NotLamina, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::LaminaNeighbourBound,
                SM_ERR,
                K::InvalidInput,
            ),
            (
                TopologyFailure::TooFewEdgeuses { given: 1 },
                SM_ERR,
                K::InvalidInput,
            ),
            (TopologyFailure::EdgeuseChainGap, SM_ERR, K::InvalidInput),
            (TopologyFailure::HitNotOnBoundary, SM_ERR, K::InvalidInput),
            (
                TopologyFailure::FaceWithoutSurface,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (TopologyFailure::EdgeNotManifold, SM_ERR, K::Unsupported),
            (
                TopologyFailure::CurveNotBSpline { entity: None },
                SM_ERR_NULL_POINTER,
                K::Unsupported,
            ),
            (
                TopologyFailure::NoTrimmedBSpline,
                SM_ERR_INVALID_INPUT,
                K::Failed,
            ),
            (
                TopologyFailure::SurfaceWithoutNurbs { entity: None },
                SM_ERR_NULL_POINTER,
                K::Unsupported,
            ),
            (TopologyFailure::FaceWithoutEdges, SM_ERR, K::Unsupported),
            (TopologyFailure::WireEndSqueezed, SM_ERR, K::Unsupported),
            (
                TopologyFailure::TrimCurveOffEnd {
                    at_end: true,
                    to_vertex: false,
                    distance: 1.0,
                    tolerance: 0.5,
                },
                SM_ERR,
                K::NotWithinTolerance,
            ),
            (
                TopologyFailure::SideUndecided {
                    why: SideOpen::NotProjectedOnce,
                },
                SM_ERR,
                K::Failed,
            ),
            (TopologyFailure::NoFootOnSurface, SM_ERR, K::Failed),
            (TopologyFailure::TestVectorZero, SM_ERR, K::Failed),
            (
                TopologyFailure::SurfaceCacheNotMade,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (TopologyFailure::NoRadialSector, SM_ERR, K::Failed),
            (
                TopologyFailure::HitAtRayStart {
                    distance: 0.0,
                    tolerance: 1e-12,
                },
                SM_ERR,
                K::Failed,
            ),
            (TopologyFailure::CoincidenceUndecided, SM_ERR, K::Failed),
            (TopologyFailure::NoMatingCurve, SM_ERR, K::Failed),
            (
                TopologyFailure::CurveNotApproximated,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                TopologyFailure::GatheredBothSides,
                SM_ERR_ASSERT_FAILURE,
                K::Failed,
            ),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::topology(op, failure);
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.topology_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
        // the approximation's failure stays its cause, and decides defect or refusal
        let why = "a step the kernel got wrong";
        let cause = SolidError::topology(op, TopologyFailure::Defect { why });
        let e = Failure::new(op, TopologyFailure::CurveNotApproximated)
            .caused_by(cause.clone())
            .into_error();
        assert_eq!(e.cause(), Some(&cause));
        assert!(e.is_defect());
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
    }

    /// The named causes of the B-rep edits of `brep/p4` keep the status of the phrase each
    /// replaced, and have their class.
    #[test]
    fn the_brep_p4_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        let cases = [
            (TopologyFailure::EdgeNotDeletable, SM_ERR, K::Unsupported),
            (
                TopologyFailure::InnerLoopsOnBothSides,
                SM_ERR,
                K::Unsupported,
            ),
            (TopologyFailure::LaminaEdgeIsStrut, SM_ERR, K::Unsupported),
            (TopologyFailure::VertexHasEdges, SM_ERR, K::InvalidInput),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::topology("topology::op", failure);
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status));
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.topology_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
    }

    /// The topology routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the file listed (with the bare sites it may still hold).
    #[test]
    fn the_topology_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("topology", 53, &[]);
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the edge use the loop walk just reached has no loop use, which the B-rep's links always give";
        let defect = TopologyFailure::Defect { why };
        let op = "topology::op";
        super::super::tests::assert_defect(
            &SolidError::topology(op, defect),
            &SolidError::topology(op, TopologyFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::topology(op, TopologyFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.topology_failure(), Some(&defect));
    }

    /// The sites that build a topology phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_topology_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("TopologyFailure", "Failed"),
                Builder::Variant("TopologyFailure", "Missing"),
                Builder::Variant("TopologyFailure", "InvalidValue"),
                Builder::Variant("TopologyFailure", "Inconsistent"),
                Builder::Variant("TopologyFailure", "OutOfMemory"),
                Builder::Method("topology_failed", Some(1)),
                Builder::Method("topology_missing", Some(1)),
                Builder::Method("topology_invalid_value", Some(1)),
                Builder::Shared(
                    "solidcore/src/topology/trimming_tools",
                    "invalid_value",
                    Some(0),
                ),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/topology/api_files.rs", 9),
            ("solidcore/src/topology/api_projection.rs", 7),
            ("solidcore/src/topology/brep/cutting.rs", 11),
            ("solidcore/src/topology/brep/local_operation.rs", 5),
            ("solidcore/src/topology/brep/mirror.rs", 2),
            ("solidcore/src/topology/brep/p10.rs", 10),
            ("solidcore/src/topology/brep/p5.rs", 61),
            ("solidcore/src/topology/brep/p6.rs", 29),
            ("solidcore/src/topology/brep/p7.rs", 13),
            ("solidcore/src/topology/brep/p8.rs", 15),
            ("solidcore/src/topology/brep/p9.rs", 18),
            ("solidcore/src/topology/brep/planar_section.rs", 3),
            ("solidcore/src/topology/brep/sheet.rs", 4),
            ("solidcore/src/topology/brep/validation/assert_valid.rs", 1),
            ("solidcore/src/topology/brep/validation/assert_valid/seams.rs", 1),
            ("solidcore/src/topology/brep/validation/pointers.rs", 2),
            ("solidcore/src/topology/brep/vertex_fans.rs", 6),
            ("solidcore/src/topology/brep_cache.rs", 17),
            ("solidcore/src/topology/brep_constructor.rs", 24),
            ("solidcore/src/topology/brep_data.rs", 5),
            ("solidcore/src/topology/brep_data_write.rs", 5),
            ("solidcore/src/topology/database_io.rs", 3),
            ("solidcore/src/topology/edge.rs", 7),
            ("solidcore/src/topology/edgeuse/p1.rs", 2),
            ("solidcore/src/topology/edgeuse/p2.rs", 4),
            ("solidcore/src/topology/edgeuse/p3.rs", 12),
            ("solidcore/src/topology/face/p1.rs", 24),
            ("solidcore/src/topology/face/p2.rs", 10),
            ("solidcore/src/topology/geometry_write.rs", 2),
            ("solidcore/src/topology/loopuse.rs", 9),
            ("solidcore/src/topology/part_file.rs", 1),
            ("solidcore/src/topology/proto_topology/p1/owning_topology.rs", 6),
            ("solidcore/src/topology/proto_topology/p1/proto.rs", 16),
            ("solidcore/src/topology/proto_topology/p2.rs", 8),
            ("solidcore/src/topology/proto_topology/p3.rs", 6),
            ("solidcore/src/topology/shell.rs", 15),
            ("solidcore/src/topology/topology_traverser.rs", 3),
            ("solidcore/src/topology/trimming_tools.rs", 22),
            ("solidcore/src/topology/trimming_tools/curve_loops.rs", 12),
            ("solidcore/src/topology/vertex/corner.rs", 1),
            ("solidcore/src/topology/vertexuse.rs", 10),
        ];
        assert_pinned(
            "topology phrase-cause sites",
            &crate_sites(&PHRASES),
            PINNED,
        );
    }
}
