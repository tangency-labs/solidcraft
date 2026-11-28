//! Why a query fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the query subsystem (`crate::query`: edge
//! and face queries, point classification on a face, contact and distance bounds,
//! continuity, normals, the face-surface intersection, the poly B-rep and its copy,
//! properties and tessellation, and ray casting on a B-rep). Each also records, for
//! difftest only, the status the C++ reports for it (crate-private `comparison_status`,
//! read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                           | Status difftest compares |
//! |--------------------------------|--------------------------|
//! | [`QueryFailure::Defect`]       | `SM_ERR` (or the site's, through `with_status`) |
//! | [`QueryFailure::Failed`]       | `SM_ERR`                 |
//! | [`QueryFailure::InvalidValue`] | `SM_ERR_INVALID_INPUT`   |
//! | [`QueryFailure::Missing`]      | `SM_ERR_NULL_POINTER`    |
//! | the named causes, from [`QueryFailure::BoundOverflows`] on | the status of the phrase each replaced (`comparison_status`), or the site's through `with_status` |
//!
//! [`QueryFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes
//! ([`QueryFailure::Failed`], [`QueryFailure::Missing`], [`QueryFailure::InvalidValue`]) have
//! no library sites left, and `the_query_phrase_cause_sites_only_shrink` keeps it so (the
//! conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. A failure names what went wrong (a named cause, or a phrase
//! where a site is not yet converted); the failure's `operation` path says which routine.
//! A failure names an entity only where the site it replaces did (the point
//! classification's seam failure, at its face). A check of a caller's argument or body
//! that upstream answers with a status is a named cause of kind
//! [`ErrorKind::InvalidInput`] that keeps the status (for example
//! [`QueryFailure::FaceWithoutSurface`]); the checks that report no status stay
//! [`crate::SolidError::InvalidInput`].

use super::{ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT, SM_ERR_NULL_POINTER};

/// Why a query fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum QueryFailure {
    /// A bound or size the query computes overflows the floating range, though every number
    /// given is finite: the inputs' size, or how far apart they are, reaches past it. Class
    /// `failed`.
    BoundOverflows,
    /// The classification curve's intersection with the face's trim boundary failed (its
    /// error is the cause). Class `failed`; whether it is a defect or a refusal is its
    /// cause's ([`crate::SolidError::is_defect`] follows the chain).
    BoundaryNotIntersected,
    /// A bounding box asked of the edge's curve could not be found (the first box's error is
    /// the cause). Class `failed`; whether it is a defect or a refusal is its cause's
    /// ([`crate::SolidError::is_defect`] follows the chain).
    BoundingBoxNotFound,
    /// The B-rep has no edges to list. Class `failed`.
    BrepWithoutEdges,
    /// The B-rep has no faces to list. Class `failed`.
    BrepWithoutFaces,
    /// A closest point the solve found does not evaluate to a finite point (or its distance
    /// is not a finite number at least zero). Class `failed`.
    ClosestPointNotEvaluated,
    /// The curve crosses the face's surface outside the tube of the edge's tolerance, so the
    /// crossing found is another one, not this edge's. Class `failed`.
    CrossingOffEdge,
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// An edge of the caller's body has no curve. [`ErrorKind::InvalidInput`].
    EdgeWithoutCurve,
    /// An edge use of the caller's body is oriented neither the same as its edge nor
    /// opposite it. [`ErrorKind::InvalidInput`].
    EdgeuseOrientationUnset,
    /// An edge use of the caller's body has no edge. [`ErrorKind::InvalidInput`].
    EdgeuseWithoutEdge,
    /// A face of the caller's body has no (upward) face use. [`ErrorKind::InvalidInput`].
    FaceWithoutFaceuse,
    /// The face has no loops: no upward face use, or none with a loop use. Class `failed`.
    FaceWithoutLoops,
    /// A face of the caller's body has no surface. [`ErrorKind::InvalidInput`].
    FaceWithoutSurface,
    /// The query could not continue, as the phrase says (for example "the loop does not
    /// close").
    Failed(&'static str),
    /// The reach given is not finite and at least zero. [`ErrorKind::InvalidInput`].
    InvalidReach,
    /// A value the query computes with has no defined result, as the phrase says (for
    /// example "a tangent of zero length has no direction"). Upstream returns
    /// `SM_ERR_INVALID_INPUT` as a status here, so its kind is
    /// [`ErrorKind::InvalidInput`].
    InvalidValue(&'static str),
    /// A loop use of the caller's body lists a vertex use slot that holds none.
    /// [`ErrorKind::InvalidInput`].
    LoopuseWithoutVertexuse,
    /// Something the query needs is missing, as the phrase says (for example "the edge's
    /// curve").
    Missing(&'static str),
    /// The closest-point search found no point: the solver returned no solution, or the drop
    /// onto the curve, surface or edge found no foot. Class `failed`.
    NoClosestPoint,
    /// No edge of the B-rep is at the point. Class `failed`.
    NoEdgeAtPoint,
    /// No edge was given where the routine needs one. [`ErrorKind::InvalidInput`].
    NoEdgeGiven,
    /// No face of the B-rep is at the point. Class `failed`.
    NoFaceAtPoint,
    /// A drop of a point onto a curve or a surface found no foot point. Class `failed`.
    NoFoot,
    /// No loop of the face classifies the point (a loop that crosses the face's seam does
    /// not). The failure is at the face. Class `failed`.
    NoLoopClassifies,
    /// The classification curve's touch list holds no entry to start the loop walk from:
    /// no touch, or in a cyclic walk no exit followed by an entry. Class `failed`.
    NoLoopTouches,
    /// No patch of the surface overlaps the UV box given: the box is within the rounding of
    /// the patches' knot range, but past every patch. [`ErrorKind::InvalidInput`].
    NoPatchInBox,
    /// The surface cache has no poly B-rep: phase 1 has not run on its face.
    /// [`ErrorKind::InvalidInput`].
    NoPolyBrep,
    /// No station of the first curve dropped onto the other curve or the surface, so no
    /// deviation was measured. Class `failed`.
    NoStationDropped,
    /// No tessellation driver was given where the test needs one.
    /// [`ErrorKind::InvalidInput`].
    NoTessDriver,
    /// The face's surface cache is not a trimmed one: its trim curves could not be cached (the
    /// trimmed build's error is not kept by the cache lease, so there is no cause to chain).
    /// Status `SM_ERR_NULL_POINTER`, class `missing`.
    NoTrimmedCache,
    /// The cache's poly B-rep has no face (the caller's poly B-rep: phase 1 gives it one).
    /// [`ErrorKind::InvalidInput`].
    PolyBrepWithoutFace,
    /// The poly edge to remove from the vertex does not start at it.
    /// [`ErrorKind::InvalidInput`].
    PolyEdgeNotAtVertex,
    /// The poly edge added does not start where the loop's last edge ends (farther than a
    /// hundred times the zone tolerance). [`ErrorKind::InvalidInput`].
    PolyEdgeNotContinuing,
    /// A poly edge of the caller's poly B-rep has no start or no end vertex.
    /// [`ErrorKind::InvalidInput`].
    PolyEdgeWithoutVertex,
    /// The poly loop to finish has no edge. [`ErrorKind::InvalidInput`].
    PolyLoopEmpty,
    /// The poly loop's last edge does not end at its first vertex (farther than the vertex's
    /// tolerance), so the loop stays open. [`ErrorKind::InvalidInput`].
    PolyLoopNotClosed,
    /// The poly edge's radial ring does not lead back to it: an edge of the ring was linked
    /// into another ring ([`crate::query::poly::PolyBrep::link_radials`] given an edge
    /// that already had radials). [`ErrorKind::InvalidInput`].
    PolyRadialRingNotClosed,
    /// No ray fired from the point found the region it lies in (every ray, both ways, failed
    /// or found none). Class `failed`.
    RegionNotFound,
    /// The region has no shells, so it has no box. Class `failed`.
    RegionWithoutShells,
    /// The poly edge's segment meets no node of the cache's subdivision tree. Class `failed`.
    SegmentOutsideTree,
    /// The sector classification stepped off a face on its first step, where it has no
    /// earlier step to fall back to. Class `failed`.
    SteppedOffOnFirstStep,
    /// The surface the query reads (a B-spline surface, or the B-spline base of an analytic
    /// one) has no NURBS net: none was made for it, or it could not be made (a NURB that
    /// cannot be made stays missing), so the query has no net to read.
    /// [`ErrorKind::Unsupported`].
    SurfaceWithoutNurbs,
    /// The curve classified against the edge's radial sector runs along the edge there: its
    /// tangent is parallel to the edge's, so it has no direction across the sector. Class
    /// `failed`.
    TangentAlongEdge,
    /// The scale given is undefined. Upstream returns `SM_ERR_INVALID_INPUT` as a status
    /// here. [`ErrorKind::InvalidInput`].
    UndefinedScale,
    /// The solution is on an object of a kind the routine does not take (not a vertex, curve,
    /// edge, surface or face). [`ErrorKind::Unsupported`].
    UnsupportedObjectKind,
    /// The UV line of the classification could not be made (its error is the cause: the
    /// line's one error is ends that coincide, the caller's points). Status
    /// `SM_ERR_NULL_POINTER`, [`ErrorKind::InvalidInput`].
    UvLineNotMade,
    /// A vertex use of the caller's body has no vertex. [`ErrorKind::InvalidInput`].
    VertexuseWithoutVertex,
    /// A vector the query unitizes or measures an angle with has zero length, so it has no
    /// direction. Upstream returns `SM_ERR_INVALID_INPUT` as a status here, so its kind is
    /// [`ErrorKind::InvalidInput`].
    ZeroLengthVector,
}

impl QueryFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::BoundOverflows
            | Self::BoundaryNotIntersected
            | Self::BoundingBoxNotFound
            | Self::BrepWithoutEdges
            | Self::BrepWithoutFaces
            | Self::ClosestPointNotEvaluated
            | Self::CrossingOffEdge
            | Self::Defect { .. }
            | Self::EdgeuseOrientationUnset
            | Self::EdgeuseWithoutEdge
            | Self::FaceWithoutLoops
            | Self::Failed(_)
            | Self::InvalidReach
            | Self::NoClosestPoint
            | Self::NoEdgeAtPoint
            | Self::NoFaceAtPoint
            | Self::NoFoot
            | Self::NoLoopClassifies
            | Self::NoLoopTouches
            | Self::NoPatchInBox
            | Self::NoStationDropped
            | Self::PolyEdgeNotAtVertex
            | Self::PolyEdgeNotContinuing
            | Self::PolyLoopEmpty
            | Self::PolyLoopNotClosed
            | Self::PolyRadialRingNotClosed
            | Self::RegionNotFound
            | Self::RegionWithoutShells
            | Self::SegmentOutsideTree
            | Self::SteppedOffOnFirstStep
            | Self::SurfaceWithoutNurbs
            | Self::TangentAlongEdge
            | Self::UnsupportedObjectKind => SM_ERR,
            Self::EdgeWithoutCurve
            | Self::FaceWithoutFaceuse
            | Self::FaceWithoutSurface
            | Self::LoopuseWithoutVertexuse
            | Self::Missing(_)
            | Self::NoEdgeGiven
            | Self::NoPolyBrep
            | Self::NoTessDriver
            | Self::NoTrimmedCache
            | Self::PolyBrepWithoutFace
            | Self::PolyEdgeWithoutVertex
            | Self::UvLineNotMade
            | Self::VertexuseWithoutVertex => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_) | Self::UndefinedScale | Self::ZeroLengthVector => {
                SM_ERR_INVALID_INPUT
            }
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::InvalidInput`] for
    /// [`QueryFailure::InvalidValue`] and for a check of the caller's argument or body,
    /// [`ErrorKind::Unsupported`] for a surface or object the routine does not take,
    /// [`ErrorKind::Missing`] for [`QueryFailure::Missing`], otherwise the kind of the status
    /// it replaced.
    /// [`QueryFailure::Defect`] is [`ErrorKind::StageFailed`], a defect.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::EdgeWithoutCurve
            | Self::EdgeuseOrientationUnset
            | Self::EdgeuseWithoutEdge
            | Self::FaceWithoutFaceuse
            | Self::FaceWithoutSurface
            | Self::InvalidReach
            | Self::LoopuseWithoutVertexuse
            | Self::NoEdgeGiven
            | Self::NoPatchInBox
            | Self::NoPolyBrep
            | Self::NoTessDriver
            | Self::PolyBrepWithoutFace
            | Self::PolyEdgeNotAtVertex
            | Self::PolyEdgeNotContinuing
            | Self::PolyEdgeWithoutVertex
            | Self::PolyLoopEmpty
            | Self::PolyLoopNotClosed
            | Self::PolyRadialRingNotClosed
            | Self::UndefinedScale
            | Self::UvLineNotMade
            | Self::VertexuseWithoutVertex => ErrorKind::InvalidInput,
            Self::SurfaceWithoutNurbs | Self::UnsupportedObjectKind => ErrorKind::Unsupported,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for QueryFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BoundOverflows => f.write_str("the bound overflows the floating range"),
            Self::BoundaryNotIntersected => f.write_str(
                "the classification curve's intersection with the face's boundary failed",
            ),
            Self::BoundingBoxNotFound => f.write_str("the curve's bounding box could not be found"),
            Self::BrepWithoutEdges => f.write_str("the B-rep has no edges"),
            Self::BrepWithoutFaces => f.write_str("the B-rep has no faces"),
            Self::ClosestPointNotEvaluated => f.write_str("a closest point does not evaluate"),
            Self::CrossingOffEdge => {
                f.write_str("the crossing on the face's surface is off the edge")
            }
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::EdgeWithoutCurve => f.write_str("the edge has no curve"),
            Self::EdgeuseOrientationUnset => {
                f.write_str("the edge use's orientation is neither same nor opposite")
            }
            Self::EdgeuseWithoutEdge => f.write_str("the edge use has no edge"),
            Self::FaceWithoutFaceuse => f.write_str("the face has no face use"),
            Self::FaceWithoutLoops => f.write_str("the face has no loops"),
            Self::FaceWithoutSurface => f.write_str("the face has no surface"),
            Self::Failed(what) | Self::InvalidValue(what) => f.write_str(what),
            Self::InvalidReach => f.write_str("the reach must be finite and at least zero"),
            Self::LoopuseWithoutVertexuse => f.write_str("a loop use's vertex use is missing"),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::NoClosestPoint => f.write_str("no closest point was found"),
            Self::NoEdgeAtPoint => f.write_str("no edge is at the point"),
            Self::NoEdgeGiven => f.write_str("no edge was given"),
            Self::NoFaceAtPoint => f.write_str("no face is at the point"),
            Self::NoFoot => f.write_str("the drop found no foot point"),
            Self::NoLoopClassifies => {
                f.write_str("the point is in no loop of the face (a loop crosses its seam)")
            }
            Self::NoLoopTouches => {
                f.write_str("the touch-direction list is empty or its index is unset")
            }
            Self::NoPatchInBox => f.write_str("no patch of the surface overlaps the UV box"),
            Self::NoPolyBrep => f.write_str("the surface cache has no poly B-rep"),
            Self::NoStationDropped => f.write_str("no station dropped onto the other"),
            Self::NoTessDriver => f.write_str("no tessellation driver was given"),
            Self::NoTrimmedCache => f.write_str("the face's surface cache has no trim curves"),
            Self::PolyBrepWithoutFace => f.write_str("the poly B-rep has no face"),
            Self::PolyEdgeNotAtVertex => f.write_str("the poly edge does not start at the vertex"),
            Self::PolyEdgeNotContinuing => {
                f.write_str("the poly edge does not start where the loop's last edge ends")
            }
            Self::PolyEdgeWithoutVertex => f.write_str("the poly edge has no start or end vertex"),
            Self::PolyLoopEmpty => f.write_str("the loop has no first edge"),
            Self::PolyLoopNotClosed => {
                f.write_str("the loop's last edge does not end at its first vertex")
            }
            Self::PolyRadialRingNotClosed => {
                f.write_str("the poly edge's radial ring does not lead back to it")
            }
            Self::RegionNotFound => f.write_str("no ray fired from the point found its region"),
            Self::RegionWithoutShells => f.write_str("the region has no shells"),
            Self::SegmentOutsideTree => f.write_str("the segment meets no subdivision tree node"),
            Self::SteppedOffOnFirstStep => f.write_str("the first step stepped off the surface"),
            Self::SurfaceWithoutNurbs => f.write_str("the surface has no NURBS net"),
            Self::TangentAlongEdge => {
                f.write_str("the curve's tangent is parallel to the edge's tangent")
            }
            Self::UndefinedScale => f.write_str("the scale is undefined"),
            Self::UnsupportedObjectKind => {
                f.write_str("the solution's object is not a vertex, curve, edge, surface or face")
            }
            Self::UvLineNotMade => f.write_str("the UV line could not be made"),
            Self::VertexuseWithoutVertex => f.write_str("the vertex use has no vertex"),
            Self::ZeroLengthVector => f.write_str("a vector of zero length has no direction"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a query that fails with `failure`.
    #[must_use]
    pub(crate) fn query(operation: &'static str, failure: QueryFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// The query failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Query`].
    #[must_use]
    pub fn query_failure(&self) -> Option<&QueryFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Query(q) => Some(q),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::SolidError;

    #[test]
    fn query_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (QueryFailure::Failed("x"), SM_ERR),
            (QueryFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (QueryFailure::InvalidValue("x"), SM_ERR_INVALID_INPUT),
        ];
        for (failure, code) in cases {
            let typed = SolidError::query("query::op", failure);
            let before = SolidError::Upstream {
                operation: "query::op",
                code,
                entity: None,
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
            assert_eq!(typed.query_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::query("query::op", QueryFailure::Missing("the edge's curve")).to_string(),
            "query::op: the edge's curve is missing"
        );
        assert_eq!(
            SolidError::query("query::op", QueryFailure::Failed("the loop does not close"))
                .to_string(),
            "query::op: the loop does not close"
        );
        assert_eq!(
            SolidError::query("query::op", QueryFailure::InvalidValue("x")).kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(SolidError::sm_err("query::op").query_failure(), None);
    }

    /// Each named cause reports the status of the phrase it replaced, and its kind.
    #[test]
    fn the_named_query_causes_keep_the_status_they_replaced() {
        use ErrorKind as K;
        use QueryFailure as F;
        let cases = [
            (F::BoundOverflows, SM_ERR, K::Failed),
            (F::NoStationDropped, SM_ERR, K::Failed),
            (F::NoPatchInBox, SM_ERR, K::InvalidInput),
            (F::FaceWithoutSurface, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::SurfaceWithoutNurbs, SM_ERR, K::Unsupported),
            (F::NoClosestPoint, SM_ERR, K::Failed),
            (F::EdgeWithoutCurve, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::UnsupportedObjectKind, SM_ERR, K::Unsupported),
            (F::BrepWithoutEdges, SM_ERR, K::Failed),
            (F::BrepWithoutFaces, SM_ERR, K::Failed),
            (F::FaceWithoutLoops, SM_ERR, K::Failed),
            (F::EdgeuseWithoutEdge, SM_ERR, K::InvalidInput),
            (F::EdgeuseOrientationUnset, SM_ERR, K::InvalidInput),
            (F::NoEdgeAtPoint, SM_ERR, K::Failed),
            (F::NoFaceAtPoint, SM_ERR, K::Failed),
            (F::ClosestPointNotEvaluated, SM_ERR, K::Failed),
            (F::InvalidReach, SM_ERR, K::InvalidInput),
            (F::CrossingOffEdge, SM_ERR, K::Failed),
            (F::RegionNotFound, SM_ERR, K::Failed),
            (F::BoundingBoxNotFound, SM_ERR, K::Failed),
            (F::ZeroLengthVector, SM_ERR_INVALID_INPUT, K::InvalidInput),
            (F::NoFoot, SM_ERR, K::Failed),
            (F::SteppedOffOnFirstStep, SM_ERR, K::Failed),
            (F::TangentAlongEdge, SM_ERR, K::Failed),
            (F::RegionWithoutShells, SM_ERR, K::Failed),
            (F::NoEdgeGiven, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::UvLineNotMade, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::NoLoopTouches, SM_ERR, K::Failed),
            (F::NoPolyBrep, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::SegmentOutsideTree, SM_ERR, K::Failed),
            (F::NoTessDriver, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::PolyBrepWithoutFace, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::FaceWithoutFaceuse, SM_ERR_NULL_POINTER, K::InvalidInput),
            (
                F::LoopuseWithoutVertexuse,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                F::VertexuseWithoutVertex,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (F::BoundaryNotIntersected, SM_ERR, K::Failed),
            (F::NoLoopClassifies, SM_ERR, K::Failed),
            (F::NoTrimmedCache, SM_ERR_NULL_POINTER, K::Missing),
            (F::UndefinedScale, SM_ERR_INVALID_INPUT, K::InvalidInput),
            (F::PolyEdgeNotContinuing, SM_ERR, K::InvalidInput),
            (F::PolyLoopEmpty, SM_ERR, K::InvalidInput),
            (F::PolyLoopNotClosed, SM_ERR, K::InvalidInput),
            (
                F::PolyEdgeWithoutVertex,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (F::PolyEdgeNotAtVertex, SM_ERR, K::InvalidInput),
            (F::PolyRadialRingNotClosed, SM_ERR, K::InvalidInput),
        ];
        for (failure, code, kind) in cases {
            let e = SolidError::query("query::op", failure);
            assert_eq!(e.sm_status(), Some(code), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(code), "{failure:?}");
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.query_failure(), Some(&failure));
            assert!(!e.is_defect(), "{failure:?}");
        }
        assert_eq!(
            SolidError::query("query::op", F::FaceWithoutSurface).to_string(),
            "query::op: the face has no surface"
        );
    }

    /// The query routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the files listed (with the bare sites each may still hold)
    /// whose conversion waits on other open work.
    #[test]
    fn the_query_routines_build_no_bare_statuses() {
        assert_eq!(bare_statuses_in("query", 20), Vec::<(String, usize)>::new());
    }

    /// The files under `src/<area>` (and `src/<area>.rs`) whose library code builds bare
    /// statuses, with the number each builds, sorted by path; at least `least` files are
    /// read. Kept for the ratchets that call it; the scan is [`super::super::bare_scan`].
    pub(in crate::error) fn bare_statuses_in(area: &str, least: usize) -> Vec<(String, usize)> {
        super::super::bare_scan::bare_statuses_in(area, least)
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the face's cache was built just before and has no loops, which a built cache always has";
        let defect = QueryFailure::Defect { why };
        let op = "query::op";
        super::super::tests::assert_defect(
            &SolidError::query(op, defect),
            &SolidError::query(op, QueryFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::query(op, QueryFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.query_failure(), Some(&defect));
    }

    /// The sites that build a query phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_query_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("QueryFailure", "Failed"),
                Builder::Variant("QueryFailure", "Missing"),
                Builder::Variant("QueryFailure", "InvalidValue"),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
        ];
        assert_pinned("query phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
