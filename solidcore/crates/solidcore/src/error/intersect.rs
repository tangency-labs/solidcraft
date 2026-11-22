//! Why an intersection or classification fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the intersection subsystem
//! (`crate::intersect`: curve and point classification, ray tracing, dropping points and
//! curves onto surfaces, surface tracing, silhouettes, the advanced surface intersector
//! and the intersection API). Each also records, for difftest only, the status the C++
//! reports for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                                | Status difftest compares   |
//! |-------------------------------------|----------------------------|
//! | [`IntersectFailure::Failed`]        | `SM_ERR`                   |
//! | [`IntersectFailure::Missing`]       | `SM_ERR_NULL_POINTER`      |
//! | [`IntersectFailure::OutsideDomain`] | `SM_ERR_OUTSIDE_OF_DOMAIN` |
//! | [`IntersectFailure::Inconsistent`]  | `SM_ERR_ASSERT_FAILURE`    |
//! | [`IntersectFailure::StepTangentTooShort`] | `SM_ERR`             |
//! | [`IntersectFailure::PointsWhereAPlaneCrosses`] | `SM_ERR`        |
//! | [`IntersectFailure::StageFailed`]   | the site's (`SM_ERR`, or `SM_ERR_NULL_POINTER` for an `NER`) |
//! | [`IntersectFailure::Defect`]        | the site's (`SM_ERR`, or another through `with_status`) |
//! | the named causes, from [`IntersectFailure::NoIntersection`] on | the status of the phrase each replaced (`comparison_status`), or the site's through `with_status` |
//!
//! **The record** (#15034). A check that refuses builds a typed cause; a stage that fails
//! with no reason the kernel names is an [`IntersectFailure::StageFailed`], a defect
//! ([`ErrorKind::StageFailed`]), keeping the error of the call it made as its cause where it
//! has one. The phrase causes ([`IntersectFailure::Failed`] to
//! [`IntersectFailure::Inconsistent`]) are sites not yet converted, and so is an
//! [`IntersectFailure::StageFailed`] not yet reviewed: a reviewed one is an
//! [`IntersectFailure::Defect`] with its reason (the conversion guide:
//! `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. The failure's `operation` path says which routine. The handles these routines work on are the
//! classification's or intersector's own, not ids of the caller's B-rep, so no failure
//! names an entity. A check of a caller's handles that reports no status stays
//! [`crate::SolidError::InvalidInput`]; a check of the caller's argument, state or body that
//! reports a status is a named cause of class `invalid_input`, keeping that status.

use super::{
    ErrorKind, Failure, Reason, SM_ERR, SM_ERR_ASSERT_FAILURE, SM_ERR_NULL_POINTER,
    SM_ERR_OUTSIDE_OF_DOMAIN,
};

/// Why an intersection or classification fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum IntersectFailure {
    /// The routine could not continue, as the phrase says (for example "the ray hits no
    /// face").
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// edge's curve").
    Missing(&'static str),
    /// A point or parameter falls outside the domain it must lie in, as the phrase says
    /// (for example "the span's end does not snap onto the seam").
    OutsideDomain(&'static str),
    /// A consistency check of the routine's own data did not hold, as the phrase says.
    Inconsistent(&'static str),
    /// The traced intersection curve's tangent at the next point is shorter than the
    /// stepping's parallel tolerance: the surfaces are (nearly) tangent there, so the step
    /// has no direction.
    StepTangentTooShort,
    /// A surface/surface intersection found touching points only, farther from one of the
    /// surfaces than their default tolerance (and than the caller's tolerance, when it
    /// gave a larger one), but the other surface has points
    /// strictly on both sides of the plane it was intersected with, over the plane's
    /// domain: the plane crosses it along curves the intersection missed. `tolerance` is the
    /// approximation tolerance the intersection ran at; `surface_tolerance` is the
    /// surfaces' default tolerance (the intersector's, the same in either order), the
    /// tolerance the touching points had to be within.
    PointsWhereAPlaneCrosses {
        /// The approximation tolerance the intersection ran at.
        tolerance: f64,
        /// The surfaces' default approximation tolerance.
        surface_tolerance: f64,
    },
    /// A stage of an intersection routine failed with no reason the kernel names: a defect
    /// ([`ErrorKind::StageFailed`]). The failure holds the operation, the status the C++
    /// reports (for the goldens only), where it was built, and the error of the call it made
    /// where it has one. The form not yet reviewed: new code builds
    /// [`IntersectFailure::Defect`], a named cause or a refusal instead.
    StageFailed,
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// The intersection searched for is empty: the search found nothing of what the caller
    /// asked for (no curve, overlap or point; points count only where the caller asked for
    /// them), so the curves, surfaces, B-reps or views given do not meet in it. Class
    /// `failed`.
    NoIntersection,
    /// A drop of a point onto a curve, a surface or a volume (a point solve) found no foot
    /// point. Class `failed`.
    NoFoot,
    /// The point solve found `found` nearest points on the face's surface, also within 100
    /// times the zone tolerance, where the classification needs exactly one. Class `failed`.
    NotOneFoot {
        /// How many nearest points the second solve found.
        found: usize,
    },
    /// The boundary fix-up refused the dropped UV curve (an empty domain, a number not
    /// finite, or a span's move not proved within the tolerance left: the fix-up's own
    /// cause is chained), and the drop of the curve stops. Class `failed`.
    BoundaryFixRefused,
    /// The cache of the curve to drop could not be built (the build's error is the cause).
    /// Class `failed`; whether it is a defect or a refusal is its cause's
    /// ([`crate::SolidError::is_defect`] follows the chain).
    CurveCacheNotBuilt,
    /// The object point of a point classification was asked for before its parameters were
    /// set. The caller's state: [`ErrorKind::InvalidInput`].
    ParametersNotSet,
    /// A face of the caller's body has no surface. [`ErrorKind::InvalidInput`].
    FaceWithoutSurface,
    /// An edge of the caller's body has no curve. [`ErrorKind::InvalidInput`].
    EdgeWithoutCurve,
    /// The point classification has no object, or none of the kind its class names. The
    /// caller's classification: [`ErrorKind::InvalidInput`].
    NoClassObject,
    /// The routine reads what an earlier step of the same object sets up (a drop's start
    /// points, a cache's tree), and the caller has not run that step.
    /// [`ErrorKind::InvalidInput`].
    NotSetUp,
    /// The span stack holds no point and tangent to read. [`ErrorKind::InvalidInput`].
    EmptyStack,
    /// The UV domain given does not fit within the surface's natural domain.
    /// [`ErrorKind::InvalidInput`].
    DomainOutsideSurface,
    /// The UV domain given is empty or its bounds are not numbers.
    /// [`ErrorKind::InvalidInput`].
    EmptyDomain,
    /// A control point of the UV polygon given is not a finite number.
    /// [`ErrorKind::InvalidInput`].
    NotFinite,
    /// A face of the caller's poly B-rep has no outer loop, or its outer loop no edge, so
    /// its plane has no point to pass through. [`ErrorKind::InvalidInput`].
    PolyFaceWithoutEdges,
    /// The classification or solution is on an object of a kind the routine does not take
    /// (for example a region where a point is needed, or a loop where a vertex, edge, face or
    /// region is). [`ErrorKind::Unsupported`].
    UnsupportedObjectKind,
    /// The edge was not split at the classified point: editing is not enabled on the
    /// caller's B-rep, or the classification's parameter is not within the edge's interval.
    /// [`ErrorKind::InvalidInput`].
    EdgeNotSplit,
    /// The dropped UV curve is of a kind with no B-spline form. [`ErrorKind::Unsupported`].
    CurveNotBSpline,
    /// A singular point of the surface lies inside its domain, not on its boundary, so the
    /// drop has no iso direction to start along. [`ErrorKind::Unsupported`].
    SingularPointInside,
    /// The dropped UV curve reached `limit` control points, more than the drop builds.
    /// [`ErrorKind::Unsupported`].
    TooManyControlPoints {
        /// The number of control points the drop stops at.
        limit: usize,
    },
    /// The surface has no normal at the dropped point (the cross product of its tangents and
    /// the evaluated normal are both shorter than the kernel's zero). [`ErrorKind::Degenerate`].
    NoSurfaceNormal,
    /// The curve to drop has no tangent to step along: its first derivative is zero where
    /// the curve is not marked degenerate, or its tangent projected onto the surface's tangent
    /// plane is zero at the first point. [`ErrorKind::Degenerate`].
    DegenerateTangent,
    /// The curve's tangent is `degrees` from the surface normal, within the 3 degrees under
    /// which its drop onto the tangent plane has no direction. [`ErrorKind::Degenerate`].
    TangentAlongNormal {
        /// The angle between the curve's tangent and the surface normal, in degrees.
        degrees: f64,
    },
    /// The surface's UV domain is degenerate in `u` or `v`. [`ErrorKind::Degenerate`].
    DegenerateDomain,
    /// The span's end crosses the seam of a closed surface and does not snap onto it within
    /// the tolerance. [`ErrorKind::OutsideDomain`].
    SeamJumpNotSnapped,
    /// The boundary fix-up moved a span of the UV polygon by an amount it cannot prove within
    /// `budget`, what is left of the drop's tolerance (negative where the trace already
    /// accepted a span over it). [`ErrorKind::NotWithinTolerance`].
    MoveNotProved {
        /// What is left of the drop's tolerance for the move.
        budget: f64,
    },
}

impl IntersectFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_)
            | Self::StepTangentTooShort
            | Self::PointsWhereAPlaneCrosses { .. }
            | Self::StageFailed
            | Self::Defect { .. }
            | Self::NoIntersection
            | Self::NoFoot
            | Self::NotOneFoot { .. }
            | Self::BoundaryFixRefused
            | Self::ParametersNotSet
            | Self::EmptyStack
            | Self::DomainOutsideSurface
            | Self::EmptyDomain
            | Self::NotFinite
            | Self::UnsupportedObjectKind
            | Self::EdgeNotSplit
            | Self::SingularPointInside
            | Self::TooManyControlPoints { .. }
            | Self::NoSurfaceNormal
            | Self::DegenerateTangent
            | Self::TangentAlongNormal { .. }
            | Self::DegenerateDomain
            | Self::MoveNotProved { .. } => SM_ERR,
            Self::Missing(_)
            | Self::CurveCacheNotBuilt
            | Self::FaceWithoutSurface
            | Self::EdgeWithoutCurve
            | Self::NoClassObject
            | Self::NotSetUp
            | Self::PolyFaceWithoutEdges
            | Self::CurveNotBSpline => SM_ERR_NULL_POINTER,
            Self::OutsideDomain(_) | Self::SeamJumpNotSnapped => SM_ERR_OUTSIDE_OF_DOMAIN,
            Self::Inconsistent(_) => SM_ERR_ASSERT_FAILURE,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::StageFailed`] for
    /// [`IntersectFailure::StageFailed`] and [`IntersectFailure::Defect`], a defect;
    /// [`ErrorKind::Missing`] for
    /// [`IntersectFailure::Missing`], [`ErrorKind::OutsideDomain`] for
    /// [`IntersectFailure::OutsideDomain`] and [`IntersectFailure::SeamJumpNotSnapped`],
    /// [`ErrorKind::NotWithinTolerance`] for [`IntersectFailure::PointsWhereAPlaneCrosses`]
    /// and [`IntersectFailure::MoveNotProved`], [`ErrorKind::InvalidInput`] for a check of
    /// the caller's argument, state or body, [`ErrorKind::Unsupported`] for a kind or size
    /// the routine does not take, [`ErrorKind::Degenerate`] for a degenerate normal, tangent
    /// or domain, [`ErrorKind::Failed`] otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::StageFailed | Self::Defect { .. } => ErrorKind::StageFailed,
            Self::PointsWhereAPlaneCrosses { .. } | Self::MoveNotProved { .. } => {
                ErrorKind::NotWithinTolerance
            }
            Self::ParametersNotSet
            | Self::FaceWithoutSurface
            | Self::EdgeWithoutCurve
            | Self::NoClassObject
            | Self::NotSetUp
            | Self::EmptyStack
            | Self::DomainOutsideSurface
            | Self::EmptyDomain
            | Self::NotFinite
            | Self::PolyFaceWithoutEdges
            | Self::EdgeNotSplit => ErrorKind::InvalidInput,
            Self::UnsupportedObjectKind
            | Self::CurveNotBSpline
            | Self::SingularPointInside
            | Self::TooManyControlPoints { .. } => ErrorKind::Unsupported,
            Self::NoSurfaceNormal
            | Self::DegenerateTangent
            | Self::TangentAlongNormal { .. }
            | Self::DegenerateDomain => ErrorKind::Degenerate,
            Self::CurveCacheNotBuilt => ErrorKind::Failed,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for IntersectFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) | Self::OutsideDomain(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
            Self::StepTangentTooShort => f.write_str(
                "the intersection curve's tangent at the next point is shorter than the parallel tolerance",
            ),
            Self::PointsWhereAPlaneCrosses {
                tolerance,
                surface_tolerance,
            } => write!(
                f,
                "the intersection found only touching points, but the plane crosses the other \
                 surface: at the tolerance {tolerance:e} they lie farther from the surfaces than \
                 their default tolerance {surface_tolerance:e}"
            ),
            Self::StageFailed => write!(f, "{}", ErrorKind::StageFailed),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::NoIntersection => f.write_str("no intersection was found"),
            Self::NoFoot => f.write_str("the drop found no foot point"),
            Self::NotOneFoot { found } => write!(
                f,
                "no single nearest point on the face's surface was found ({found} found)"
            ),
            Self::BoundaryFixRefused => {
                f.write_str("the boundary fix-up refused the dropped curve")
            }
            Self::CurveCacheNotBuilt => f.write_str("the curve cache could not be built"),
            Self::ParametersNotSet => {
                f.write_str("the point's classification parameters are not set")
            }
            Self::FaceWithoutSurface => f.write_str("the face has no surface"),
            Self::EdgeWithoutCurve => f.write_str("the edge has no curve"),
            Self::NoClassObject => {
                f.write_str("the classification has no object of the kind its class names")
            }
            Self::NotSetUp => f.write_str("a step this one reads from has not run"),
            Self::EmptyStack => f.write_str("the stack holds no point and tangent"),
            Self::DomainOutsideSurface => {
                f.write_str("the uv domain does not fit within the surface's natural domain")
            }
            Self::EmptyDomain => f.write_str("the uv domain is empty or not a number"),
            Self::NotFinite => f.write_str("a control point is not a finite number"),
            Self::PolyFaceWithoutEdges => f.write_str("a poly face has no outer loop edge"),
            Self::UnsupportedObjectKind => {
                f.write_str("the object is of a kind the routine does not take")
            }
            Self::EdgeNotSplit => f.write_str("the edge was not split at the point"),
            Self::CurveNotBSpline => f.write_str("the dropped uv curve has no B-spline form"),
            Self::SingularPointInside => {
                f.write_str("the singular surface point is not on the domain's boundary")
            }
            Self::TooManyControlPoints { limit } => write!(
                f,
                "the dropped curve reached {limit} control points, more than the drop builds"
            ),
            Self::NoSurfaceNormal => f.write_str("the surface has no normal at the dropped point"),
            Self::DegenerateTangent => f.write_str("the curve to drop has no tangent there"),
            Self::TangentAlongNormal { degrees } => write!(
                f,
                "the curve's tangent is {degrees} degrees from the surface normal, within 3"
            ),
            Self::DegenerateDomain => f.write_str("the surface's uv domain is degenerate"),
            Self::SeamJumpNotSnapped => f.write_str(
                "the span's end crosses the seam and does not snap onto it within the tolerance",
            ),
            Self::MoveNotProved { budget } => write!(
                f,
                "a span's move is not proved within the tolerance left ({budget:e})"
            ),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for an intersection or classification routine that
    /// fails with `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn intersect(operation: &'static str, failure: IntersectFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for an intersection or classification routine that
    /// could not continue, as the phrase says, with status [`SM_ERR`] (upstream
    /// `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn intersect_failed(operation: &'static str, what: &'static str) -> Self {
        Self::intersect(operation, IntersectFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something an intersection or classification
    /// routine needs that is not there, as the phrase says, with status
    /// [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)` on a `NULL` pointer).
    #[must_use]
    pub(crate) fn intersect_missing(operation: &'static str, what: &'static str) -> Self {
        Self::intersect(operation, IntersectFailure::Missing(what))
    }

    /// [`crate::SolidError::Failed`] for a stage of an intersection routine that failed with
    /// no reason the kernel names ([`IntersectFailure::StageFailed`], a defect), with
    /// `status`, the one the C++ reports (`SM_ERR`, or `SM_ERR_NULL_POINTER` for an `NER`).
    #[must_use]
    #[track_caller]
    pub(crate) fn intersect_stage_failed(operation: &'static str, status: i32) -> Self {
        Failure::new(operation, IntersectFailure::StageFailed)
            .with_status(status)
            .into_error()
    }

    /// The intersection failure behind this error, for a [`crate::SolidError::Failed`]
    /// with a [`Reason::Intersect`].
    #[must_use]
    pub fn intersect_failure(&self) -> Option<&IntersectFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Intersect(i) => Some(i),
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
    fn intersect_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (IntersectFailure::Failed("x"), SM_ERR),
            (IntersectFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (
                IntersectFailure::OutsideDomain("x"),
                SM_ERR_OUTSIDE_OF_DOMAIN,
            ),
            (IntersectFailure::Inconsistent("x"), SM_ERR_ASSERT_FAILURE),
        ];
        for (failure, code) in cases {
            let typed = SolidError::intersect("intersect::op", failure);
            let before = SolidError::Upstream {
                operation: "intersect::op",
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{failure:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{failure:?}");
            assert_eq!(typed.kind(), before.kind(), "{failure:?}");
            assert_eq!(typed.operation(), before.operation(), "{failure:?}");
            assert_eq!(typed.entity(), None, "{failure:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert_eq!(typed.intersect_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::intersect_missing("intersect::op", "the edge's curve").to_string(),
            "intersect::op: the edge's curve is missing"
        );
        assert_eq!(
            SolidError::intersect_failed("intersect::op", "the ray hits no face").kind(),
            ErrorKind::Failed
        );
    }

    /// A sample of the second set of converted files (`surface_intersector/p2` to `p4`,
    /// `topology_solver/p1` and `p2`): each keeps the operation, status, kind and entity of
    /// its `Upstream`.
    #[test]
    fn the_surface_intersector_and_topology_solver_samples_keep_their_upstream_answers() {
        let samples = [
            (
                "intersect::surface_intersector::p2::SurfaceIntersector::compute_point_values",
                IntersectFailure::Failed("the surface point has no normal"),
                SM_ERR,
            ),
            (
                "intersect::surface_intersector::p3::SurfaceIntersector::find_boundary_start_points",
                IntersectFailure::Missing("the surface's cache"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "intersect::surface_intersector::p4::SurfaceIntersector::do_law_intersection",
                IntersectFailure::Missing("the first surface's offset surface"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "intersect::topology_solver::p1::LocalSolver::solve_it",
                IntersectFailure::Failed("the base, point and curve local solvers solve nothing"),
                SM_ERR,
            ),
            (
                "intersect::topology_solver::p2::LocalSolver::solve_face_face",
                IntersectFailure::Missing("the first surface to solve against"),
                SM_ERR_NULL_POINTER,
            ),
        ];
        for (operation, failure, code) in samples {
            let typed = SolidError::intersect(operation, failure);
            let before = SolidError::Upstream {
                operation,
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{typed:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{typed:?}");
            assert_eq!(super::super::upstream_status(&typed), Some(code));
            assert_eq!(typed.kind(), before.kind(), "{typed:?}");
            assert_eq!(typed.operation(), before.operation(), "{typed:?}");
            assert_eq!(typed.entity(), None, "{typed:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert_eq!(
                typed.is_unported_or_cancelled(),
                before.is_unported_or_cancelled()
            );
        }
    }

    #[test]
    fn a_stage_failure_is_a_defect_and_a_typed_cause_keeps_its_status() {
        for status in [SM_ERR, SM_ERR_NULL_POINTER] {
            super::super::tests::assert_stage_failure(
                &SolidError::intersect_stage_failed("intersect::op", status),
                "intersect::op",
                status,
            );
        }
        let e = SolidError::intersect("intersect::op", IntersectFailure::StepTangentTooShort);
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert_eq!(e.kind(), ErrorKind::Failed);
        assert!(!e.is_defect() && e.is_status_answer());
    }

    /// A reviewed defect is of class 127 with the stage failure's status, and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the trace lost the branch it was started on, which its own step control keeps";
        let defect = IntersectFailure::Defect { why };
        for status in [SM_ERR, SM_ERR_NULL_POINTER] {
            let e = Failure::new("intersect::op", defect)
                .with_status(status)
                .into_error();
            super::super::tests::assert_defect(
                &e,
                &SolidError::intersect_stage_failed("intersect::op", status),
                "intersect::op",
                status,
                &format!("intersect::op: stage_failed: {why}"),
            );
            assert_eq!(e.intersect_failure(), Some(&defect));
        }
        super::super::tests::assert_defect(
            &SolidError::intersect("intersect::op", defect),
            &SolidError::intersect_stage_failed("intersect::op", SM_ERR),
            "intersect::op",
            SM_ERR,
            &format!("intersect::op: stage_failed: {why}"),
        );
    }

    /// The named causes of the drops, the point classification, the ray tracer and the
    /// intersection entries keep the status of the phrase each replaced, so
    /// `upstream_status` reads what it did, and have their class.
    #[test]
    fn the_drop_classification_and_ray_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        use IntersectFailure as F;
        let op = "intersect::op";
        let cases = [
            (F::NoIntersection, SM_ERR, K::Failed),
            (F::NoFoot, SM_ERR, K::Failed),
            (F::NotOneFoot { found: 2 }, SM_ERR, K::Failed),
            (F::BoundaryFixRefused, SM_ERR, K::Failed),
            (F::CurveCacheNotBuilt, SM_ERR_NULL_POINTER, K::Failed),
            (F::ParametersNotSet, SM_ERR, K::InvalidInput),
            (F::FaceWithoutSurface, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::EdgeWithoutCurve, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::NoClassObject, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::NotSetUp, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::EmptyStack, SM_ERR, K::InvalidInput),
            (F::DomainOutsideSurface, SM_ERR, K::InvalidInput),
            (F::EmptyDomain, SM_ERR, K::InvalidInput),
            (F::NotFinite, SM_ERR, K::InvalidInput),
            (
                F::PolyFaceWithoutEdges,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (F::UnsupportedObjectKind, SM_ERR, K::Unsupported),
            (F::EdgeNotSplit, SM_ERR, K::InvalidInput),
            (F::CurveNotBSpline, SM_ERR_NULL_POINTER, K::Unsupported),
            (F::SingularPointInside, SM_ERR, K::Unsupported),
            (F::TooManyControlPoints { limit: 9 }, SM_ERR, K::Unsupported),
            (F::NoSurfaceNormal, SM_ERR, K::Degenerate),
            (F::DegenerateTangent, SM_ERR, K::Degenerate),
            (
                F::TangentAlongNormal { degrees: 1.5 },
                SM_ERR,
                K::Degenerate,
            ),
            (F::DegenerateDomain, SM_ERR, K::Degenerate),
            (
                F::SeamJumpNotSnapped,
                SM_ERR_OUTSIDE_OF_DOMAIN,
                K::OutsideDomain,
            ),
            (
                F::MoveNotProved { budget: 1e-6 },
                SM_ERR,
                K::NotWithinTolerance,
            ),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::intersect(op, failure);
            assert_eq!(e.sm_status(), Some(status), "{failure:?}");
            assert_eq!(
                super::super::upstream_status(&e),
                Some(status),
                "{failure:?}"
            );
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.intersect_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
            assert!(e.is_status_answer(), "{failure:?}");
        }
        // a site whose status is not the cause's default keeps its own
        let e = Failure::new(op, F::FaceWithoutSurface)
            .with_status(SM_ERR)
            .into_error();
        assert_eq!(super::super::upstream_status(&e), Some(SM_ERR));
        assert_eq!(e.kind(), K::InvalidInput);
        // the cache build's error stays the cause, and decides defect or refusal
        let why = "a step the kernel got wrong";
        let cause = SolidError::intersect(op, F::Defect { why });
        let e = Failure::new(op, F::CurveCacheNotBuilt)
            .caused_by(cause.clone())
            .into_error();
        assert_eq!(e.cause(), Some(&cause));
        assert!(e.is_defect());
        assert_eq!(super::super::upstream_status(&e), Some(SM_ERR_NULL_POINTER));
    }

    /// The intersection routines describe their failures: their library code builds no
    /// bare `Upstream` status.
    #[test]
    fn the_intersect_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("intersect", 40, &[]);
    }

    /// The sites that build a intersection phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_intersect_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("IntersectFailure", "Failed"),
                Builder::Variant("IntersectFailure", "Missing"),
                Builder::Variant("IntersectFailure", "OutsideDomain"),
                Builder::Variant("IntersectFailure", "Inconsistent"),
                Builder::Method("intersect_failed", Some(1)),
                Builder::Method("intersect_missing", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/intersect/adv_surface_intersector.rs", 10),
            ("solidcore/src/intersect/curve_class/p1.rs", 9),
            ("solidcore/src/intersect/curve_class/p2.rs", 7),
            ("solidcore/src/intersect/curve_class/p3.rs", 19),
            ("solidcore/src/intersect/curve_class/p4.rs", 26),
            ("solidcore/src/intersect/curve_class/p5.rs", 8),
            ("solidcore/src/intersect/curve_class/p6.rs", 21),
            ("solidcore/src/intersect/curve_class/p7.rs", 17),
            ("solidcore/src/intersect/curve_class/p8.rs", 17),
            ("solidcore/src/intersect/curve_class/p9.rs", 17),
            ("solidcore/src/intersect/surface_intersector/p2.rs", 9),
            ("solidcore/src/intersect/surface_intersector/p3.rs", 5),
            ("solidcore/src/intersect/surface_intersector/p4.rs", 17),
            ("solidcore/src/intersect/surface_silhouette.rs", 7),
            ("solidcore/src/intersect/surface_tracer.rs", 6),
            ("solidcore/src/intersect/topology_solver/p1.rs", 12),
            ("solidcore/src/intersect/topology_solver/p2.rs", 24),
        ];
        assert_pinned(
            "intersection phrase-cause sites",
            &crate_sites(&PHRASES),
            PINNED,
        );
    }
}
