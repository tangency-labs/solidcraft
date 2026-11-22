//! Why a surface routine fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the surface routines (`crate::surface`: the
//! surface kinds' evaluation, inversion, solvers, trimming, conversion and approximation,
//! their caches, and the volume and deformation surfaces). Each also records, for difftest
//! only, the status the C++ reports for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                               | Status difftest compares       |
//! |------------------------------------|--------------------------------|
//! | [`SurfaceFailure::Failed`]         | `SM_ERR`                       |
//! | [`SurfaceFailure::Missing`]        | `SM_ERR_NULL_POINTER`          |
//! | [`SurfaceFailure::InvalidValue`]   | `SM_ERR_INVALID_INPUT`         |
//! | [`SurfaceFailure::OutsideDomain`]  | `SM_ERR_OUTSIDE_OF_DOMAIN`     |
//! | [`SurfaceFailure::OffSurface`]     | `SM_ERR_NOT_WITHIN_TOLERANCE`  |
//! | [`SurfaceFailure::Defect`]         | `SM_ERR` (or the site's, through `with_status`) |
//! | [`SurfaceFailure::NoNurb`], [`SurfaceFailure::InputNotGiven`], [`SurfaceFailure::TrimmedCacheHasNoFace`] | `SM_ERR_NULL_POINTER` (class [`ErrorKind::InvalidInput`]) |
//! | [`SurfaceFailure::SurfaceCacheNotBuilt`], [`SurfaceFailure::SurfaceCacheTreeNotBuilt`], [`SurfaceFailure::CurveCacheNotBuilt`], [`SurfaceFailure::CurveCacheTreeNotBuilt`] | `SM_ERR_NULL_POINTER` (class [`ErrorKind::Failed`]; defect-ness follows the cause) |
//! | [`SurfaceFailure::NormalsCancel`]  | `SM_ERR_INVALID_INPUT` (class [`ErrorKind::Degenerate`]) |
//! | every other named cause            | `SM_ERR`, with the class its doc names |
//!
//! [`SurfaceFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes are sites
//! not yet converted (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. A failure says in a phrase what went wrong; the failure's
//! `operation` path says which routine. A failure names an entity only where the site it
//! replaces named one (a blend face whose net fails its re-certification). The failures that are checks of a caller's arguments stay
//! [`crate::SolidError::InvalidInput`]. Only part of `crate::surface` uses these yet; the
//! files that still build bare statuses are listed in this module's ratchet test.

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT,
    SM_ERR_NOT_WITHIN_TOLERANCE, SM_ERR_NULL_POINTER, SM_ERR_OUTSIDE_OF_DOMAIN,
};

/// Why a surface routine fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum SurfaceFailure {
    /// The routine could not continue, as the phrase says (for example "the generator
    /// curve has no NURB form").
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// surface's cache").
    Missing(&'static str),
    /// A value the routine computes with has no defined result, as the phrase says (for
    /// example "the line direction is the zero vector"). Upstream returns
    /// `SM_ERR_INVALID_INPUT` as a status here, so its kind is
    /// [`ErrorKind::InvalidInput`].
    InvalidValue(&'static str),
    /// A parameter or domain lies outside the surface's domain, as the phrase says (for
    /// example "the point is outside the sweep of the surface of revolution").
    OutsideDomain(&'static str),
    /// A point is farther from the surface than the tolerance, as the phrase says (for
    /// example "the point is off the surface"): an inversion's answer.
    OffSurface(&'static str),
    /// A derivative order above the highest the routine evaluates was asked for: the
    /// caller's argument ([`ErrorKind::InvalidInput`], `SM_ERR`).
    DerivativeOrderTooHigh {
        /// The highest order the routine evaluates.
        max: usize,
    },
    /// The caller's surface has no NURB form for the routine to work on
    /// ([`ErrorKind::InvalidInput`], `SM_ERR_NULL_POINTER`).
    NoNurb,
    /// The surface cache is of a kind that does not do this: the base cache answers the
    /// trimmed cache's queries with a refusal ([`ErrorKind::Unsupported`], `SM_ERR`).
    UnsupportedCacheKind,
    /// The surface's cache could not be made; the failure that stopped it is the cause
    /// (`SM_ERR_NULL_POINTER`, class [`ErrorKind::Failed`]: its defect-ness follows the
    /// cause).
    SurfaceCacheNotBuilt,
    /// The surface cache's tree could not be built; the failure that stopped it, where
    /// there was one, is the cause (`SM_ERR_NULL_POINTER`, class [`ErrorKind::Failed`]).
    SurfaceCacheTreeNotBuilt,
    /// The curve's cache could not be made; the failure that stopped it is the cause
    /// (`SM_ERR_NULL_POINTER`, class [`ErrorKind::Failed`]).
    CurveCacheNotBuilt,
    /// The curve cache has no tree (`SM_ERR_NULL_POINTER`, class [`ErrorKind::Failed`]).
    CurveCacheTreeNotBuilt,
    /// The cubic approximation of a surface was still farther from it than the tolerance
    /// after the last subdivision pass ([`ErrorKind::NotConverged`], `SM_ERR`).
    ApproximationNotConverged {
        /// The largest distance of the approximation from the surface it sampled.
        achieved: f64,
        /// The distance asked for.
        tolerance: f64,
    },
    /// The caller's parameter lies in none of the curve's segments, which cover its
    /// interval ([`ErrorKind::InvalidInput`], `SM_ERR`).
    ParameterOutsideInterval {
        /// The parameter.
        param: f64,
    },
    /// A first derivative (tangent) of the curve or surface is of zero length where the
    /// routine needs its direction ([`ErrorKind::Degenerate`], `SM_ERR`).
    ZeroLengthTangent,
    /// The surface has no normal at the point ([`ErrorKind::Degenerate`], `SM_ERR`).
    NoNormal,
    /// The two surfaces' normals are parallel, so they span no plane
    /// ([`ErrorKind::Degenerate`], `SM_ERR`).
    ParallelNormals,
    /// The normals around a singular point sum to zero, so they give no normal there
    /// ([`ErrorKind::Degenerate`]; upstream's status `SM_ERR_INVALID_INPUT`).
    NormalsCancel,
    /// The curve's tangent is along the surface normal, so there is no binormal
    /// ([`ErrorKind::Degenerate`], `SM_ERR`).
    TangentAlongNormal,
    /// The solver's caller gave fewer variables than its equations take
    /// ([`ErrorKind::InvalidInput`], `SM_ERR`).
    TooFewSolverVariables {
        /// The number of variables the equations take.
        needed: usize,
    },
    /// An input the routine needs was not given by its caller
    /// ([`ErrorKind::InvalidInput`], `SM_ERR_NULL_POINTER`).
    InputNotGiven {
        /// Which input.
        what: SolveInput,
    },
    /// The trimmed surface cache that point testing reads has no face to test against
    /// ([`ErrorKind::InvalidInput`], `SM_ERR_NULL_POINTER`).
    TrimmedCacheHasNoFace,
    /// An end point of the caller's UV line lies outside the surface's UV domain
    /// ([`ErrorKind::InvalidInput`], `SM_ERR`).
    EndPointOutsideDomain,
    /// The surface has fewer than two distinct knots in a direction, so it has no span
    /// there ([`ErrorKind::Degenerate`], `SM_ERR`).
    TooFewUniqueKnots,
    /// The derivative surface definition's tangency type is not one a derivative surface
    /// is made for ([`ErrorKind::Unsupported`], `SM_ERR`).
    UnsupportedTangencyType,
    /// The caller's derivative surface definition fails its validity check
    /// ([`ErrorKind::InvalidInput`], `SM_ERR`).
    InvalidDerivativeSurface,
    /// A point of the boundary curve does not drop onto the surface
    /// ([`ErrorKind::Failed`], `SM_ERR`).
    PointDoesNotDrop,
    /// A 3d direction has no direction in the surface's parameters at the point; the
    /// failure is the cause ([`ErrorKind::Failed`], `SM_ERR`).
    NoParameterDirection,
    /// The tangent reference curve's point lies on the boundary curve, so it gives no
    /// direction ([`ErrorKind::Degenerate`], `SM_ERR`).
    ReferencePointOnBoundary,
    /// No sampled tangent of the singular direction points along the given direction
    /// ([`ErrorKind::Failed`], `SM_ERR`).
    NoTangentTowardDirection,
    /// The point is at a pole of the surface, where the seam test does not apply
    /// ([`ErrorKind::Unsupported`], `SM_ERR`).
    AtPole,
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
}

/// An input of a surface routine that its caller did not give
/// ([`SurfaceFailure::InputNotGiven`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SolveInput {
    /// The surface.
    Surface,
    /// The first of two surfaces.
    FirstSurface,
    /// The second of two surfaces.
    SecondSurface,
    /// The plane of the solve.
    Plane,
    /// The surface cache of the solve.
    SurfaceCache,
    /// The target distance of a solve at a distance.
    TargetDistance,
    /// The direction vector of a directed minimize.
    DirectionVector,
    /// The tangent reference curve of a derivative surface.
    TangentReference,
}

impl core::fmt::Display for SolveInput {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Surface => "the surface",
            Self::FirstSurface => "the first surface",
            Self::SecondSurface => "the second surface",
            Self::Plane => "the plane",
            Self::SurfaceCache => "the surface cache",
            Self::TargetDistance => "the target distance",
            Self::DirectionVector => "the direction vector",
            Self::TangentReference => "the tangent reference curve",
        })
    }
}

impl SurfaceFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_)
            | Self::Defect { .. }
            | Self::DerivativeOrderTooHigh { .. }
            | Self::UnsupportedCacheKind
            | Self::ApproximationNotConverged { .. }
            | Self::ParameterOutsideInterval { .. }
            | Self::ZeroLengthTangent
            | Self::NoNormal
            | Self::ParallelNormals
            | Self::TangentAlongNormal
            | Self::TooFewSolverVariables { .. }
            | Self::EndPointOutsideDomain
            | Self::TooFewUniqueKnots
            | Self::UnsupportedTangencyType
            | Self::InvalidDerivativeSurface
            | Self::PointDoesNotDrop
            | Self::NoParameterDirection
            | Self::ReferencePointOnBoundary
            | Self::NoTangentTowardDirection
            | Self::AtPole => SM_ERR,
            Self::Missing(_)
            | Self::NoNurb
            | Self::SurfaceCacheNotBuilt
            | Self::SurfaceCacheTreeNotBuilt
            | Self::CurveCacheNotBuilt
            | Self::CurveCacheTreeNotBuilt
            | Self::InputNotGiven { .. }
            | Self::TrimmedCacheHasNoFace => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_) | Self::NormalsCancel => SM_ERR_INVALID_INPUT,
            Self::OutsideDomain(_) => SM_ERR_OUTSIDE_OF_DOMAIN,
            Self::OffSurface(_) => SM_ERR_NOT_WITHIN_TOLERANCE,
        }
    }

    /// The failure's stable [`ErrorKind`]: the kind of the status it replaced
    /// ([`ErrorKind::Failed`], [`ErrorKind::Missing`], [`ErrorKind::InvalidInput`],
    /// [`ErrorKind::OutsideDomain`], [`ErrorKind::NotWithinTolerance`]).
    /// [`SurfaceFailure::Defect`] is [`ErrorKind::StageFailed`], a defect.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::DerivativeOrderTooHigh { .. }
            | Self::NoNurb
            | Self::ParameterOutsideInterval { .. }
            | Self::TooFewSolverVariables { .. }
            | Self::InputNotGiven { .. }
            | Self::TrimmedCacheHasNoFace
            | Self::EndPointOutsideDomain
            | Self::InvalidDerivativeSurface => ErrorKind::InvalidInput,
            Self::UnsupportedCacheKind | Self::UnsupportedTangencyType | Self::AtPole => {
                ErrorKind::Unsupported
            }
            Self::SurfaceCacheNotBuilt
            | Self::SurfaceCacheTreeNotBuilt
            | Self::CurveCacheNotBuilt
            | Self::CurveCacheTreeNotBuilt
            | Self::PointDoesNotDrop
            | Self::NoParameterDirection
            | Self::NoTangentTowardDirection => ErrorKind::Failed,
            Self::ApproximationNotConverged { .. } => ErrorKind::NotConverged,
            Self::ZeroLengthTangent
            | Self::NoNormal
            | Self::ParallelNormals
            | Self::NormalsCancel
            | Self::TangentAlongNormal
            | Self::TooFewUniqueKnots
            | Self::ReferencePointOnBoundary => ErrorKind::Degenerate,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for SurfaceFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what)
            | Self::InvalidValue(what)
            | Self::OutsideDomain(what)
            | Self::OffSurface(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::DerivativeOrderTooHigh { max } => {
                write!(f, "the derivative order is above {max}")
            }
            Self::NoNurb => f.write_str("the surface has no NURB form"),
            Self::UnsupportedCacheKind => {
                f.write_str("the surface cache's kind does not support the operation")
            }
            Self::SurfaceCacheNotBuilt => f.write_str("the surface's cache could not be made"),
            Self::SurfaceCacheTreeNotBuilt => {
                f.write_str("the surface cache's tree could not be built")
            }
            Self::CurveCacheNotBuilt => f.write_str("the curve's cache could not be made"),
            Self::CurveCacheTreeNotBuilt => f.write_str("the curve cache has no tree"),
            Self::ApproximationNotConverged {
                achieved,
                tolerance,
            } => write!(
                f,
                "the approximation is {achieved:e} from the surface after its last pass, above the tolerance {tolerance:e}"
            ),
            Self::ParameterOutsideInterval { param } => {
                write!(f, "the parameter {param} lies in none of the curve's segments")
            }
            Self::ZeroLengthTangent => f.write_str("a tangent has zero length"),
            Self::NoNormal => f.write_str("the surface has no normal at the point"),
            Self::ParallelNormals => f.write_str("the surface normals are parallel"),
            Self::NormalsCancel => f.write_str("the normals around the singularity sum to zero"),
            Self::TangentAlongNormal => {
                f.write_str("the curve tangent is along the surface normal")
            }
            Self::TooFewSolverVariables { needed } => {
                write!(f, "the solver was given fewer than {needed} variables")
            }
            Self::InputNotGiven { what } => write!(f, "{what} was not given"),
            Self::TrimmedCacheHasNoFace => f.write_str("the trimmed surface cache has no face"),
            Self::EndPointOutsideDomain => {
                f.write_str("an end point is outside the surface's UV domain")
            }
            Self::TooFewUniqueKnots => {
                f.write_str("the surface has fewer than two distinct knots in the direction")
            }
            Self::UnsupportedTangencyType => {
                f.write_str("the tangency type has no derivative surface")
            }
            Self::InvalidDerivativeSurface => {
                f.write_str("the derivative surface definition is not valid")
            }
            Self::PointDoesNotDrop => {
                f.write_str("the curve point does not drop onto the surface")
            }
            Self::NoParameterDirection => {
                f.write_str("the direction has no direction in the surface's parameters")
            }
            Self::ReferencePointOnBoundary => {
                f.write_str("the reference curve point lies on the boundary curve")
            }
            Self::NoTangentTowardDirection => {
                f.write_str("every sample tangent points away from the direction")
            }
            Self::AtPole => f.write_str("the point is at a pole"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a surface routine that fails with `failure`.
    #[must_use]
    pub(crate) fn surface(operation: &'static str, failure: SurfaceFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a surface routine that fails with `failure` about
    /// `entity`.
    #[must_use]
    pub(crate) fn surface_at(
        operation: &'static str,
        failure: SurfaceFailure,
        entity: EntityRef,
    ) -> Self {
        Failure::new(operation, failure).at(entity).into_error()
    }

    /// [`crate::SolidError::Failed`] for a surface routine that could not continue, as the
    /// phrase says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn surface_failed(operation: &'static str, what: &'static str) -> Self {
        Self::surface(operation, SurfaceFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a surface routine needs that is not
    /// there, as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)`
    /// on a `NULL` pointer).
    #[must_use]
    pub(crate) fn surface_missing(operation: &'static str, what: &'static str) -> Self {
        Self::surface(operation, SurfaceFailure::Missing(what))
    }

    /// The surface failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Surface`].
    #[must_use]
    pub fn surface_failure(&self) -> Option<&SurfaceFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Surface(s) => Some(s),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SolidError;

    #[test]
    fn surface_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (SurfaceFailure::Failed("x"), SM_ERR),
            (SurfaceFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (SurfaceFailure::InvalidValue("x"), SM_ERR_INVALID_INPUT),
            (SurfaceFailure::OutsideDomain("x"), SM_ERR_OUTSIDE_OF_DOMAIN),
            (SurfaceFailure::OffSurface("x"), SM_ERR_NOT_WITHIN_TOLERANCE),
        ];
        for (failure, code) in cases {
            let typed = SolidError::surface("surface::op", failure);
            let before = SolidError::Upstream {
                operation: "surface::op",
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
            assert_eq!(typed.surface_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::surface_missing("surface::op", "the surface's cache").to_string(),
            "surface::op: the surface's cache is missing"
        );
        assert_eq!(
            SolidError::surface_failed("surface::op", "the generator has no NURB form").to_string(),
            "surface::op: the generator has no NURB form"
        );
        assert_eq!(
            SolidError::surface(
                "surface::op",
                SurfaceFailure::OffSurface("the point is off")
            )
            .to_string(),
            "surface::op: the point is off"
        );
        assert_eq!(SolidError::sm_err("surface::op").surface_failure(), None);
    }

    /// The named causes of the general surface evaluation (TE-O-07) keep the status of the
    /// phrase cause each replaced, with their own class.
    #[test]
    fn the_surface_evaluation_causes_keep_their_status_with_their_class() {
        use ErrorKind as K;
        let cases = [
            (
                SurfaceFailure::DerivativeOrderTooHigh { max: 3 },
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::NoNurb, SM_ERR_NULL_POINTER, K::InvalidInput),
            (SurfaceFailure::UnsupportedCacheKind, SM_ERR, K::Unsupported),
            (
                SurfaceFailure::SurfaceCacheNotBuilt,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                SurfaceFailure::SurfaceCacheTreeNotBuilt,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                SurfaceFailure::CurveCacheNotBuilt,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                SurfaceFailure::CurveCacheTreeNotBuilt,
                SM_ERR_NULL_POINTER,
                K::Failed,
            ),
            (
                SurfaceFailure::ApproximationNotConverged {
                    achieved: 2e-3,
                    tolerance: 1e-3,
                },
                SM_ERR,
                K::NotConverged,
            ),
            (
                SurfaceFailure::ParameterOutsideInterval { param: 2.0 },
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::ZeroLengthTangent, SM_ERR, K::Degenerate),
            (SurfaceFailure::NoNormal, SM_ERR, K::Degenerate),
            (SurfaceFailure::ParallelNormals, SM_ERR, K::Degenerate),
            (
                SurfaceFailure::NormalsCancel,
                SM_ERR_INVALID_INPUT,
                K::Degenerate,
            ),
            (SurfaceFailure::TangentAlongNormal, SM_ERR, K::Degenerate),
            (
                SurfaceFailure::TooFewSolverVariables { needed: 2 },
                SM_ERR,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::InputNotGiven {
                    what: SolveInput::Plane,
                },
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::TrimmedCacheHasNoFace,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                SurfaceFailure::EndPointOutsideDomain,
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::TooFewUniqueKnots, SM_ERR, K::Degenerate),
            (
                SurfaceFailure::UnsupportedTangencyType,
                SM_ERR,
                K::Unsupported,
            ),
            (
                SurfaceFailure::InvalidDerivativeSurface,
                SM_ERR,
                K::InvalidInput,
            ),
            (SurfaceFailure::PointDoesNotDrop, SM_ERR, K::Failed),
            (SurfaceFailure::NoParameterDirection, SM_ERR, K::Failed),
            (
                SurfaceFailure::ReferencePointOnBoundary,
                SM_ERR,
                K::Degenerate,
            ),
            (SurfaceFailure::NoTangentTowardDirection, SM_ERR, K::Failed),
            (SurfaceFailure::AtPole, SM_ERR, K::Unsupported),
        ];
        for (failure, code, kind) in cases {
            let typed = SolidError::surface("surface::op", failure);
            assert_eq!(typed.sm_status(), Some(code), "{failure:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{failure:?}");
            assert_eq!(typed.kind(), kind, "{failure:?}");
            assert!(typed.is_status_answer(), "{failure:?}");
            assert!(!typed.is_defect(), "{failure:?}");
            assert_eq!(typed.surface_failure(), Some(&failure));
            assert!(!typed.to_string().is_empty());
        }
        assert_eq!(
            SolidError::surface(
                "surface::op",
                SurfaceFailure::InputNotGiven {
                    what: SolveInput::TargetDistance
                }
            )
            .to_string(),
            "surface::op: the target distance was not given"
        );
        // a cache failure's defect-ness follows its cause
        let defect =
            SolidError::surface("surface::inner", SurfaceFailure::Defect { why: "the step" });
        let chained = Failure::new("surface::op", SurfaceFailure::SurfaceCacheNotBuilt)
            .caused_by(defect)
            .into_error();
        assert!(chained.is_defect());
        assert_eq!(chained.sm_status(), Some(SM_ERR_NULL_POINTER));
    }

    /// A sample of the second set of converted files (`extend` and its sides, `side`,
    /// `implicit`, the volumes, `srf_in_volume`, `procedural_blend` and its offset,
    /// `trim_srf_cache`): each keeps the operation, status, kind and entity of its
    /// `Upstream`.
    #[test]
    fn the_extend_volume_and_blend_samples_keep_their_upstream_answers() {
        let samples = [
            (
                "surface::extend::analytic::analytic_extended_sides",
                SurfaceFailure::Failed("the joint is not an interior knot"),
                SM_ERR,
            ),
            (
                "surface::extend::ball",
                SurfaceFailure::Failed("the B-spline surface has no NURB"),
                SM_ERR,
            ),
            (
                "surface::volume::p1::Volume::copy",
                SurfaceFailure::Failed("a plain volume cannot be copied"),
                SM_ERR,
            ),
            (
                "surface::srf_in_volume",
                SurfaceFailure::Failed("the volume's inversion failed"),
                SM_ERR,
            ),
            (
                "surface::procedural_blend::ProceduralBlend::read_from_db",
                SurfaceFailure::Missing("the input stream"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "surface::trim_srf_cache::TrimSrfCache::build_tree_with_face",
                SurfaceFailure::InvalidValue("the face's domain and the surface's are disjoint"),
                SM_ERR_INVALID_INPUT,
            ),
            (
                "surface::side",
                SurfaceFailure::Failed("the iso curve has no NURB"),
                SM_ERR,
            ),
        ];
        for (operation, failure, code) in samples {
            let typed = SolidError::surface(operation, failure);
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
        // the re-certification refusal keeps the face the bare status named
        let face = EntityRef::Face(crate::topology::FaceId::from_index(3));
        let typed = SolidError::surface_at(
            "surface::procedural_blend::recertify_faces",
            SurfaceFailure::Failed(
                "the face's net is not proved within its bound of its definition",
            ),
            face,
        );
        let before = SolidError::Upstream {
            operation: "surface::procedural_blend::recertify_faces",
            code: SM_ERR,
            entity: Some(face),
        };
        assert_eq!(typed.sm_status(), before.sm_status());
        assert_eq!(typed.upstream_status(), Some(SM_ERR));
        assert_eq!(typed.kind(), before.kind());
        assert_eq!(typed.operation(), before.operation());
        assert_eq!(typed.entity(), Some(face));
        assert_eq!(typed.is_status_answer(), before.is_status_answer());
    }

    /// The surface routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the files listed (with the bare sites each may still hold)
    /// whose conversion waits on other open work or a later part of the slice.
    #[test]
    fn the_surface_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("surface", 51, &[]);
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the B-spline form the surface just converted to has no control net, which conversion always gives";
        let defect = SurfaceFailure::Defect { why };
        let op = "surface::op";
        super::super::tests::assert_defect(
            &SolidError::surface(op, defect),
            &SolidError::surface(op, SurfaceFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::surface(op, SurfaceFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.surface_failure(), Some(&defect));
    }

    /// The sites that build a surface phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_surface_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("SurfaceFailure", "Failed"),
                Builder::Variant("SurfaceFailure", "Missing"),
                Builder::Variant("SurfaceFailure", "InvalidValue"),
                Builder::Variant("SurfaceFailure", "OutsideDomain"),
                Builder::Variant("SurfaceFailure", "OffSurface"),
                Builder::Method("surface_failed", Some(1)),
                // the WR-57 replay seam's own constructor of a taped answer (feature
                // `wr57-replay`), which only the goldens call: not a kernel site
                Builder::Shared(
                    "solidcore/src/surface/surface/replay",
                    "surface_failed",
                    Some(1),
                ),
                Builder::Method("surface_missing", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/surface/b_spline_volume.rs", 4),
            ("solidcore/src/surface/b_spline_volume_n_lib.rs", 1),
            ("solidcore/src/surface/ball_patch.rs", 8),
            ("solidcore/src/surface/bend_volume.rs", 2),
            ("solidcore/src/surface/cone/p1.rs", 12),
            ("solidcore/src/surface/cone/p2.rs", 14),
            ("solidcore/src/surface/cone/p3.rs", 3),
            ("solidcore/src/surface/extend.rs", 2),
            ("solidcore/src/surface/extend/analytic.rs", 25),
            ("solidcore/src/surface/extend/ball.rs", 16),
            ("solidcore/src/surface/extend/blend.rs", 6),
            ("solidcore/src/surface/extend/sides.rs", 2),
            ("solidcore/src/surface/from_parameters.rs", 1),
            ("solidcore/src/surface/implicit.rs", 9),
            ("solidcore/src/surface/offset_surface.rs", 27),
            ("solidcore/src/surface/plane/p1.rs", 3),
            ("solidcore/src/surface/plane/p2.rs", 9),
            ("solidcore/src/surface/procedural_blend.rs", 9),
            ("solidcore/src/surface/procedural_blend/convert.rs", 13),
            ("solidcore/src/surface/procedural_blend/offset.rs", 5),
            ("solidcore/src/surface/side.rs", 7),
            ("solidcore/src/surface/sphere.rs", 16),
            ("solidcore/src/surface/srf_in_volume.rs", 4),
            ("solidcore/src/surface/surf_of_extrusion/p1.rs", 16),
            ("solidcore/src/surface/surf_of_extrusion/p2.rs", 13),
            ("solidcore/src/surface/surf_of_revolution/p1.rs", 10),
            ("solidcore/src/surface/surf_of_revolution/p2.rs", 6),
            ("solidcore/src/surface/surf_of_revolution/p3.rs", 21),
            ("solidcore/src/surface/surface/p10.rs", 7),
            ("solidcore/src/surface/surface/p11.rs", 20),
            ("solidcore/src/surface/surface/p2.rs", 21),
            ("solidcore/src/surface/surface/p8.rs", 33),
            ("solidcore/src/surface/surface/p9.rs", 33),
            ("solidcore/src/surface/surface_cache/face_context.rs", 1),
            ("solidcore/src/surface/surface_cache/p1.rs", 17),
            ("solidcore/src/surface/surface_cache/p2.rs", 5),
            ("solidcore/src/surface/surface_cache/p3.rs", 1),
            ("solidcore/src/surface/surface_point_solve/p1.rs", 5),
            ("solidcore/src/surface/surface_point_solve/p2.rs", 8),
            ("solidcore/src/surface/torus.rs", 15),
            ("solidcore/src/surface/trim_srf_cache.rs", 4),
            ("solidcore/src/surface/unbend_volume.rs", 2),
            ("solidcore/src/surface/volume/p1.rs", 8),
            ("solidcore/src/surface/volume/p2.rs", 6),
        ];
        assert_pinned("surface phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
