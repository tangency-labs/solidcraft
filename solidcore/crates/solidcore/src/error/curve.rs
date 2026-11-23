//! Why a curve query refuses, and why a curve routine fails, in solidcore's terms (#3184).
//!
//! Upstream: none. A curve's certified length ([`crate::curve::curve::Curve::length_bound`])
//! is a bracket whose width cannot fall below the rounding of the pieces it is summed over:
//! asked for a narrower one, it refuses and names the narrowest it proves. Asked for one
//! the curve would need more than the cap of pieces for, it refuses and names the width
//! reached at the cap. A piece found outside the knot span it is blossomed in (which the
//! halving never makes, and the rounding bound assumes) is refused, named (#14434). The other
//! failures describe the curve routines (`crate::curve`: the
//! curve kinds' evaluation, editing, reading and conversion, their caches, solvers and
//! property functions), each recording, for difftest only, the status the C++ reports for it
//! (crate-private `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                                 | Status difftest compares |
//! |--------------------------------------|--------------------------|
//! | [`CurveFailure::LengthBelowFloor`]   | `SM_ERR`                 |
//! | [`CurveFailure::LengthPastPieceCap`] | `SM_ERR`                 |
//! | [`CurveFailure::PieceOutsideSpan`]   | `SM_ERR`                 |
//! | [`CurveFailure::Failed`]             | `SM_ERR`                 |
//! | [`CurveFailure::Missing`]            | `SM_ERR_NULL_POINTER`    |
//! | [`CurveFailure::ZeroDirection`]      | `SM_ERR_INVALID_INPUT`   |
//! | [`CurveFailure::SingleLinearCurve`]  | `SM_ERR`                 |
//! | [`CurveFailure::InfiniteTangent`], [`CurveFailure::ZeroTangent`], [`CurveFailure::PointOnCurveWhenMaximising`] | `SM_ERR` |
//! | [`CurveFailure::StageFailed`]        | the site's (`SM_ERR`, or `SM_ERR_NULL_POINTER` for an `NER`) |
//! | [`CurveFailure::PointSolveOptionMissing`] | `SM_ERR_NULL_POINTER` |
//! | [`CurveFailure::Defect`]             | the site's (`SM_ERR`, or another through `with_status`) |
//! | [`CurveFailure::AuxiliaryAlongProjection`] | `SM_ERR_INVALID_INPUT`  |
//! | the other named causes ([`CurveFailure::TooManyDerivatives`], [`CurveFailure::BaseCurveNotOwned`], ...) | `SM_ERR` |
//!
//! **The record** (#15034). A check that refuses builds a typed cause with its numbers
//! ([`CurveFailure::SingleLinearCurve`], [`CurveFailure::InfiniteTangent`], ...); a stage that
//! fails with no reason the kernel names is a [`CurveFailure::StageFailed`], a defect
//! ([`ErrorKind::StageFailed`]), keeping the error of the call it made as its cause where it
//! has one. The phrase causes ([`CurveFailure::Failed`], [`CurveFailure::Missing`],
//! [`CurveFailure::ZeroDirection`]) are sites not yet converted, and so is a
//! [`CurveFailure::StageFailed`] not yet reviewed: a reviewed one is a
//! [`CurveFailure::Defect`] with its reason (the conversion guide:
//! `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. The failure's `operation` path says which routine. No failure names an entity: the sites they
//! replace named none. A check of a caller's argument that reported a status is a named
//! cause of kind [`ErrorKind::InvalidInput`] keeping that status; the checks that reported
//! none stay [`crate::SolidError::InvalidInput`].

use super::{ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT, SM_ERR_NULL_POINTER};

/// Why a curve query refuses.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum CurveFailure {
    /// A length bracket was asked `asked` wide, below `floor`, the sum over the `pieces` the
    /// curve was halved into of each piece's rounding bound: halving further only widens it.
    LengthBelowFloor {
        /// The width asked.
        asked: f64,
        /// The narrowest width the bracket proves: the pieces' rounding, summed.
        floor: f64,
        /// The pieces the halving used.
        pieces: usize,
    },
    /// A length bracket was asked `asked` wide, and the curve was halved into its most
    /// pieces, `pieces`, with its bracket still `reached` wide: it could narrow further but
    /// is not halved past the cap.
    LengthPastPieceCap {
        /// The width asked.
        asked: f64,
        /// The bracket's width at the cap.
        reached: f64,
        /// The pieces at the cap.
        pieces: usize,
    },
    /// A curve routine could not continue, as the phrase says (for example "the split
    /// parameter is outside the curve's interval").
    Failed(&'static str),
    /// Something a curve routine needs is missing, as the phrase says (for example "the
    /// curve's cache").
    Missing(&'static str),
    /// A direction a curve routine needs is the zero vector, as the phrase says. No site
    /// builds it any more (the projected curve's was the last); it goes with the other phrase
    /// forms once every count is zero.
    ZeroDirection(&'static str),
    /// The only curve whose area normal is asked is a line: one line spans no plane.
    SingleLinearCurve,
    /// The curve's tangent at `t` is infinite, so the closest-point function cannot be
    /// scaled down to evaluate there.
    InfiniteTangent {
        /// The curve parameter.
        t: f64,
    },
    /// The curve's tangent at `t` is zero where the curve is not degenerate: a
    /// singularity, where the closest-point function has no direction to step.
    ZeroTangent {
        /// The curve parameter.
        t: f64,
    },
    /// The point lies on the curve at `t`, where a solve for the farthest point finds no
    /// extremum (the gap is zero there).
    PointOnCurveWhenMaximising {
        /// The curve parameter.
        t: f64,
    },
    /// A stage of a curve routine failed with no reason the kernel names: a defect
    /// ([`ErrorKind::StageFailed`]). The failure holds the operation, the status the C++
    /// reports (for the goldens only), where it was built, and the error of the call it made
    /// where it has one. The form not yet reviewed: new code builds
    /// [`CurveFailure::Defect`], a named cause or a refusal instead.
    StageFailed,
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// A ray solve ([`crate::curve::curve::SolverOperation::Rayfire`] or
    /// `Signed3dDirectedMinimize`) was called without `option`, which it requires: the
    /// caller's argument, so of kind [`ErrorKind::InvalidInput`], not a defect.
    PointSolveOptionMissing {
        /// The option left out.
        option: PointSolveOption,
    },
    /// A piece `[a, b]` of a length bracket lies outside the knot span
    /// `[span_start, span_end]` it is blossomed in: the bracket's rounding bound holds only
    /// for a piece within its span (a port invariant: the halving keeps every piece within).
    PieceOutsideSpan {
        /// The piece's start.
        a: f64,
        /// The piece's end.
        b: f64,
        /// The knot span's start.
        span_start: f64,
        /// The knot span's end.
        span_end: f64,
    },
    /// The routine evaluates at most `most` derivatives, and the caller asked for
    /// `asked`. The caller's argument: [`ErrorKind::InvalidInput`].
    TooManyDerivatives {
        /// The derivatives asked for.
        asked: usize,
        /// The most the routine evaluates.
        most: usize,
    },
    /// An analytic curve (a conic) is asked to scale by `scale`, which is not the same
    /// along every axis: its kind keeps its shape only under a uniform scale.
    /// [`ErrorKind::Unsupported`].
    NonUniformScale {
        /// The scale asked for, per axis.
        scale: crate::geom::vector3d::Vec3,
    },
    /// The curve refers to a base curve it does not own, so the routine may not `edit`
    /// it. The caller's curve as supplied: [`ErrorKind::InvalidInput`].
    BaseCurveNotOwned {
        /// The edit refused.
        edit: CurveEdit,
    },
    /// A composite curve is evaluated through its segments; it has no evaluation of its own
    /// at a parameter of the whole. [`ErrorKind::Unsupported`].
    NoEvaluationOfItsOwn,
    /// An offset corner by curve extension is asked of the corner builder, which builds
    /// line, fillet and chamfer corners only (the taper extrude continues each wall
    /// itself). [`ErrorKind::Unsupported`].
    CornerByCurveExtension,
    /// The same-point tolerance given, `tolerance`, is not positive.
    /// [`ErrorKind::InvalidInput`].
    SamePointToleranceNotPositive {
        /// The tolerance given.
        tolerance: f64,
    },
    /// The distance under which a gap gets a line, `distance`, is neither zero nor above the
    /// same-point tolerance `tolerance`. [`ErrorKind::InvalidInput`].
    LineDistanceNotAboveTolerance {
        /// The distance given.
        distance: f64,
        /// The same-point tolerance given.
        tolerance: f64,
    },
    /// A hyperbola's arc is numerically degenerate for its rational quadratic form: the
    /// line from its middle point toward the end tangents' meeting point cuts the chord at
    /// `at` of the chord's length, within the kernel's zero of an end (exactly, it cuts it
    /// at the middle; only rounding moves it there). [`ErrorKind::Degenerate`].
    MidTangentOffChord {
        /// Where the line cuts the chord, as a fraction of the chord from its start.
        at: f64,
    },
    /// The curve's cached breaks and continuities are not current: the caller asked before
    /// the breaks were computed (a new, trimmed or reparametrised curve), or loaded a cache
    /// whose counts differ. [`ErrorKind::InvalidInput`].
    BreaksNotCurrent,
    /// The knot `knot` the caller gave lies past the curve's last cached break.
    /// [`ErrorKind::InvalidInput`].
    KnotPastBreaks {
        /// The knot given.
        knot: f64,
    },
    /// The cached continuity at the break `at` is undefined: the kernel never caches one,
    /// so it came with the curve the caller loaded. [`ErrorKind::InvalidInput`].
    ContinuityUndefined {
        /// The break.
        at: f64,
    },
    /// A projected curve's projection vector is unset or zero. [`ErrorKind::InvalidInput`].
    ProjectionVectorZero,
    /// A rotation's auxiliary vector is unset or zero. [`ErrorKind::InvalidInput`].
    AuxiliaryVectorZero,
    /// A rotation's auxiliary vector is parallel to the projection vector, so it has no
    /// direction normal to it. Status `SM_ERR_INVALID_INPUT`. [`ErrorKind::InvalidInput`].
    AuxiliaryAlongProjection,
    /// The point of the tangent field's start curve at its mated parameter lies `gap` from
    /// the start of the base curve, farther than the tolerance `tolerance`.
    /// [`ErrorKind::NotWithinTolerance`].
    StartsApart {
        /// The distance between the two starts.
        gap: f64,
        /// The tolerance given.
        tolerance: f64,
    },
    /// The point of the tangent field's end curve at its mated parameter lies `gap` from the
    /// end of the base curve, farther than the tolerance `tolerance`.
    /// [`ErrorKind::NotWithinTolerance`].
    EndsApart {
        /// The distance between the two ends.
        gap: f64,
        /// The tolerance given.
        tolerance: f64,
    },
}

/// An edit of a curve that [`CurveFailure::BaseCurveNotOwned`] refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CurveEdit {
    /// A change or reversal of its parameterisation.
    Reparametrise,
    /// A trim.
    Trim,
    /// A transform.
    Transform,
}

impl core::fmt::Display for CurveEdit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Reparametrise => "reparametrised",
            Self::Trim => "trimmed",
            Self::Transform => "transformed",
        })
    }
}

/// A required option of [`crate::curve::curve::p3::PointSolveOptions`] that a ray solve
/// was called without ([`CurveFailure::PointSolveOptionMissing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PointSolveOption {
    /// `distance_tolerance`, the largest ray / curve gap.
    DistanceTolerance,
    /// `vectors`, whose first entry is the ray's direction.
    Vectors,
}

impl core::fmt::Display for PointSolveOption {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::DistanceTolerance => "distance tolerance",
            Self::Vectors => "vectors",
        })
    }
}

impl CurveFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`, except
    /// `SM_ERR_NULL_POINTER` for [`CurveFailure::Missing`] and
    /// [`CurveFailure::PointSolveOptionMissing`], and `SM_ERR_INVALID_INPUT` for
    /// [`CurveFailure::ZeroDirection`] and [`CurveFailure::AuxiliaryAlongProjection`].
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::LengthBelowFloor { .. }
            | Self::LengthPastPieceCap { .. }
            | Self::Failed(_)
            | Self::SingleLinearCurve
            | Self::InfiniteTangent { .. }
            | Self::ZeroTangent { .. }
            | Self::PointOnCurveWhenMaximising { .. }
            | Self::StageFailed
            | Self::Defect { .. }
            | Self::PieceOutsideSpan { .. }
            | Self::TooManyDerivatives { .. }
            | Self::NonUniformScale { .. }
            | Self::BaseCurveNotOwned { .. }
            | Self::NoEvaluationOfItsOwn
            | Self::CornerByCurveExtension
            | Self::SamePointToleranceNotPositive { .. }
            | Self::LineDistanceNotAboveTolerance { .. }
            | Self::MidTangentOffChord { .. }
            | Self::BreaksNotCurrent
            | Self::KnotPastBreaks { .. }
            | Self::ContinuityUndefined { .. }
            | Self::ProjectionVectorZero
            | Self::AuxiliaryVectorZero
            | Self::StartsApart { .. }
            | Self::EndsApart { .. } => SM_ERR,
            Self::Missing(_) | Self::PointSolveOptionMissing { .. } => SM_ERR_NULL_POINTER,
            Self::ZeroDirection(_) | Self::AuxiliaryAlongProjection => SM_ERR_INVALID_INPUT,
        }
    }

    /// Its stable [`ErrorKind`]: [`ErrorKind::NotWithinTolerance`] for a length bracket's
    /// refusal (the accuracy asked is finer than the computation resolves); for a routine's
    /// failure, the kind of the status it replaced ([`ErrorKind::Failed`],
    /// [`ErrorKind::Missing`], [`ErrorKind::InvalidInput`]) and that of the status of a typed
    /// cause that replaced one; [`ErrorKind::StageFailed`] for a stage that failed, reviewed
    /// or not, a defect;
    /// [`ErrorKind::PortInvariant`] for a piece outside its span, a port invariant broken;
    /// [`ErrorKind::InvalidInput`] for a check of the caller's argument or curve,
    /// [`ErrorKind::Unsupported`] for what the routine does not do,
    /// [`ErrorKind::Degenerate`] for an arc too flat for its form and
    /// [`ErrorKind::NotWithinTolerance`] for curves whose ends lie apart.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::LengthBelowFloor { .. }
            | Self::LengthPastPieceCap { .. }
            | Self::StartsApart { .. }
            | Self::EndsApart { .. } => ErrorKind::NotWithinTolerance,
            Self::PieceOutsideSpan { .. } => ErrorKind::PortInvariant,
            Self::StageFailed | Self::Defect { .. } => ErrorKind::StageFailed,
            Self::PointSolveOptionMissing { .. }
            | Self::TooManyDerivatives { .. }
            | Self::BaseCurveNotOwned { .. }
            | Self::SamePointToleranceNotPositive { .. }
            | Self::LineDistanceNotAboveTolerance { .. }
            | Self::BreaksNotCurrent
            | Self::KnotPastBreaks { .. }
            | Self::ContinuityUndefined { .. }
            | Self::ProjectionVectorZero
            | Self::AuxiliaryVectorZero
            | Self::AuxiliaryAlongProjection => ErrorKind::InvalidInput,
            Self::NonUniformScale { .. }
            | Self::NoEvaluationOfItsOwn
            | Self::CornerByCurveExtension => ErrorKind::Unsupported,
            Self::MidTangentOffChord { .. } => ErrorKind::Degenerate,
            Self::Failed(_)
            | Self::Missing(_)
            | Self::ZeroDirection(_)
            | Self::SingleLinearCurve
            | Self::InfiniteTangent { .. }
            | Self::ZeroTangent { .. }
            | Self::PointOnCurveWhenMaximising { .. } => {
                ErrorKind::of_status(self.comparison_status())
            }
        }
    }

    /// True for a failure that replaced a C++ status the routine returns (every cause but
    /// the length bracket's refusals and a piece outside its span):
    /// it answers with that status the way the `Upstream` it replaced did
    /// ([`crate::error::upstream_status`]). A length bracket's refusal, and a piece outside its
    /// span, are solidcore's own and replaced none.
    #[must_use]
    pub const fn replaces_upstream_status(&self) -> bool {
        match self {
            Self::LengthBelowFloor { .. }
            | Self::LengthPastPieceCap { .. }
            | Self::PieceOutsideSpan { .. } => false,
            Self::Failed(_)
            | Self::Missing(_)
            | Self::ZeroDirection(_)
            | Self::SingleLinearCurve
            | Self::InfiniteTangent { .. }
            | Self::ZeroTangent { .. }
            | Self::PointOnCurveWhenMaximising { .. }
            | Self::StageFailed
            | Self::Defect { .. }
            | Self::PointSolveOptionMissing { .. }
            | Self::TooManyDerivatives { .. }
            | Self::NonUniformScale { .. }
            | Self::BaseCurveNotOwned { .. }
            | Self::NoEvaluationOfItsOwn
            | Self::CornerByCurveExtension
            | Self::SamePointToleranceNotPositive { .. }
            | Self::LineDistanceNotAboveTolerance { .. }
            | Self::MidTangentOffChord { .. }
            | Self::BreaksNotCurrent
            | Self::KnotPastBreaks { .. }
            | Self::ContinuityUndefined { .. }
            | Self::ProjectionVectorZero
            | Self::AuxiliaryVectorZero
            | Self::AuxiliaryAlongProjection
            | Self::StartsApart { .. }
            | Self::EndsApart { .. } => true,
        }
    }
}

impl core::fmt::Display for CurveFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::LengthBelowFloor {
                asked,
                floor,
                pieces,
            } => write!(
                f,
                "a length bracket {asked:e} wide was asked, below the {floor:e} its rounding over {pieces} pieces allows"
            ),
            Self::LengthPastPieceCap {
                asked,
                reached,
                pieces,
            } => write!(
                f,
                "a length bracket {asked:e} wide was asked; at its cap of {pieces} pieces it is {reached:e} wide"
            ),
            Self::PieceOutsideSpan {
                a,
                b,
                span_start,
                span_end,
            } => write!(
                f,
                "a piece [{a:e}, {b:e}] of a length bracket lies outside its knot span [{span_start:e}, {span_end:e}]"
            ),
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::ZeroDirection(what) => write!(f, "{what} is the zero vector"),
            Self::SingleLinearCurve => f.write_str("the only curve is a line, which spans no plane"),
            Self::InfiniteTangent { t } => write!(f, "the curve's tangent at t = {t} is infinite"),
            Self::ZeroTangent { t } => write!(
                f,
                "the curve's tangent at t = {t} is zero where the curve is not degenerate"
            ),
            Self::PointOnCurveWhenMaximising { t } => write!(
                f,
                "the point lies on the curve at t = {t}, where a farthest-point solve finds no extremum"
            ),
            Self::StageFailed => write!(f, "{}", ErrorKind::StageFailed),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::PointSolveOptionMissing { option } => {
                write!(f, "the ray solve was called without its {option}")
            }
            Self::TooManyDerivatives { asked, most } => write!(
                f,
                "{asked} derivatives were asked, more than the {most} the routine evaluates"
            ),
            Self::NonUniformScale { scale } => write!(
                f,
                "the scale ({}, {}, {}) is not uniform, and an analytic curve scales only uniformly",
                scale.x, scale.y, scale.z
            ),
            Self::BaseCurveNotOwned { edit } => write!(
                f,
                "the curve does not own its base curve, so it cannot be {edit}"
            ),
            Self::NoEvaluationOfItsOwn => f.write_str(
                "a composite curve is evaluated through its segments and has no evaluation of its own",
            ),
            Self::CornerByCurveExtension => f.write_str(
                "an offset corner by curve extension is not built here, only line, fillet and chamfer corners",
            ),
            Self::SamePointToleranceNotPositive { tolerance } => {
                write!(f, "the same-point tolerance {tolerance:e} is not positive")
            }
            Self::LineDistanceNotAboveTolerance {
                distance,
                tolerance,
            } => write!(
                f,
                "the distance to create a line {distance:e} is neither zero nor above the same-point tolerance {tolerance:e}"
            ),
            Self::MidTangentOffChord { at } => write!(
                f,
                "the hyperbola's arc is numerically degenerate for its rational form: its middle tangent line cuts the chord at {at} of its length, at an end"
            ),
            Self::BreaksNotCurrent => {
                f.write_str("the curve's cached breaks and continuities are not current")
            }
            Self::KnotPastBreaks { knot } => {
                write!(f, "the knot {knot} lies past the curve's last cached break")
            }
            Self::ContinuityUndefined { at } => {
                write!(f, "the cached continuity at the break {at} is undefined")
            }
            Self::ProjectionVectorZero => {
                f.write_str("the projected curve's projection vector is unset or zero")
            }
            Self::AuxiliaryVectorZero => {
                f.write_str("the rotation's auxiliary vector is unset or zero")
            }
            Self::AuxiliaryAlongProjection => f.write_str(
                "the rotation's auxiliary vector is parallel to the projection vector",
            ),
            Self::StartsApart { gap, tolerance } => write!(
                f,
                "the start curve's mated point lies {gap:e} from the base curve's start, more than the tolerance {tolerance:e}"
            ),
            Self::EndsApart { gap, tolerance } => write!(
                f,
                "the end curve's mated point lies {gap:e} from the base curve's end, more than the tolerance {tolerance:e}"
            ),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a curve routine that fails with `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn curve(operation: &'static str, failure: CurveFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a curve routine that could not continue, as the phrase
    /// says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn curve_failed(operation: &'static str, what: &'static str) -> Self {
        Self::curve(operation, CurveFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a curve routine needs that is not there, as
    /// the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)` on a `NULL`
    /// pointer).
    #[must_use]
    pub(crate) fn curve_missing(operation: &'static str, what: &'static str) -> Self {
        Self::curve(operation, CurveFailure::Missing(what))
    }

    /// [`crate::SolidError::Failed`] for a stage of a curve routine that failed with no
    /// reason the kernel names ([`CurveFailure::StageFailed`], a defect), with `status`, the
    /// one the C++ reports (`SM_ERR`, or `SM_ERR_NULL_POINTER` for an `NER`).
    #[must_use]
    #[track_caller]
    pub(crate) fn curve_stage_failed(operation: &'static str, status: i32) -> Self {
        Failure::new(operation, CurveFailure::StageFailed)
            .with_status(status)
            .into_error()
    }

    /// The curve failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Curve`].
    #[must_use]
    pub fn curve_failure(&self) -> Option<&CurveFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Curve(c) => Some(c),
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
    fn curve_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (CurveFailure::Failed("x"), SM_ERR),
            (CurveFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (CurveFailure::ZeroDirection("x"), SM_ERR_INVALID_INPUT),
        ];
        for (failure, code) in cases {
            let typed = SolidError::curve("curve::op", failure);
            let before = SolidError::Upstream {
                operation: "curve::op",
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
            assert!(failure.replaces_upstream_status());
            assert_eq!(typed.curve_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::curve_missing("curve::op", "the curve's cache").to_string(),
            "curve::op: the curve's cache is missing"
        );
        assert_eq!(
            SolidError::curve_failed("curve::op", "the scale is not uniform").to_string(),
            "curve::op: the scale is not uniform"
        );
        assert_eq!(
            SolidError::curve("curve::op", CurveFailure::ZeroDirection("the direction"))
                .to_string(),
            "curve::op: the direction is the zero vector"
        );
        assert_eq!(SolidError::sm_err("curve::op").curve_failure(), None);
    }

    /// A stage that failed is a defect that keeps its site's status; a typed cause keeps
    /// the status and kind of the bare status it replaced.
    #[test]
    fn a_stage_failure_is_a_defect_and_a_typed_cause_keeps_its_status() {
        super::super::tests::assert_stage_failure(
            &SolidError::curve_stage_failed("curve::op", SM_ERR_NULL_POINTER),
            "curve::op",
            SM_ERR_NULL_POINTER,
        );
        super::super::tests::assert_stage_failure(
            &SolidError::curve_stage_failed("curve::op", SM_ERR),
            "curve::op",
            SM_ERR,
        );
        for cause in [
            CurveFailure::SingleLinearCurve,
            CurveFailure::InfiniteTangent { t: 0.25 },
            CurveFailure::ZeroTangent { t: 0.5 },
            CurveFailure::PointOnCurveWhenMaximising { t: 0.75 },
        ] {
            let e = SolidError::curve("curve::op", cause);
            assert_eq!(e.upstream_status(), Some(SM_ERR), "{cause:?}");
            assert_eq!(e.kind(), ErrorKind::Failed, "{cause:?}");
            assert!(!e.is_defect(), "{cause:?}");
            assert!(e.is_status_answer(), "{cause:?}");
        }
        assert_eq!(
            SolidError::curve("curve::op", CurveFailure::InfiniteTangent { t: 0.25 }).to_string(),
            "curve::op: the curve's tangent at t = 0.25 is infinite"
        );
    }

    /// A reviewed defect is of class 127 with the stage failure's status, and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the cache rebuilt a moment ago has no span at a parameter inside the curve";
        let defect = CurveFailure::Defect { why };
        super::super::tests::assert_defect(
            &SolidError::curve("curve::op", defect),
            &SolidError::curve_stage_failed("curve::op", SM_ERR),
            "curve::op",
            SM_ERR,
            &format!("curve::op: stage_failed: {why}"),
        );
        let missing = Failure::new("curve::op", defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::curve_stage_failed("curve::op", SM_ERR_NULL_POINTER),
            "curve::op",
            SM_ERR_NULL_POINTER,
            &format!("curve::op: stage_failed: {why}"),
        );
        assert_eq!(missing.curve_failure(), Some(&defect));
        assert!(defect.replaces_upstream_status());
    }

    /// A length bracket's refusals report what they reported before the curve routines'
    /// failures joined them: status `SM_ERR`, kind `not_within_tolerance`, no upstream status
    /// (they replaced none), and the same text.
    #[test]
    fn a_length_bracket_refusal_reports_as_before() {
        let refusals = [
            (
                CurveFailure::LengthBelowFloor {
                    asked: 1e-12,
                    floor: 1e-10,
                    pieces: 8,
                },
                "curve::length_bound: a length bracket 1e-12 wide was asked, below the 1e-10 its rounding over 8 pieces allows",
            ),
            (
                CurveFailure::LengthPastPieceCap {
                    asked: 1e-9,
                    reached: 1e-6,
                    pieces: 4096,
                },
                "curve::length_bound: a length bracket 1e-9 wide was asked; at its cap of 4096 pieces it is 1e-6 wide",
            ),
        ];
        for (refusal, text) in refusals {
            let e = Failure::new("curve::length_bound", refusal).into_error();
            assert_eq!(e.sm_status(), Some(SM_ERR));
            assert_eq!(e.kind(), ErrorKind::NotWithinTolerance);
            assert_eq!(e.upstream_status(), None);
            assert_eq!(super::super::upstream_status(&e), None);
            assert_eq!(e.operation(), "curve::length_bound");
            assert_eq!(e.entity(), None);
            assert!(!refusal.replaces_upstream_status());
            assert!(e.is_status_answer());
            assert!(!e.is_port_failure());
            assert_eq!(e.to_string(), text);
            assert_eq!(e.curve_failure(), Some(&refusal));
        }
    }

    /// The named causes keep the status of the phrase they replace, and `upstream_status`
    /// reads it, with the kind of what they check.
    #[test]
    fn the_named_causes_keep_their_sites_statuses() {
        use crate::geom::vector3d::Vec3;
        let cases = [
            (
                CurveFailure::TooManyDerivatives { asked: 4, most: 3 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::NonUniformScale {
                    scale: Vec3::new(1.0, 2.0, 1.0),
                },
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                CurveFailure::BaseCurveNotOwned {
                    edit: CurveEdit::Trim,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::NoEvaluationOfItsOwn,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                CurveFailure::CornerByCurveExtension,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
            (
                CurveFailure::SamePointToleranceNotPositive { tolerance: 0.0 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::LineDistanceNotAboveTolerance {
                    distance: 0.5,
                    tolerance: 1.0,
                },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::MidTangentOffChord { at: 0.0 },
                SM_ERR,
                ErrorKind::Degenerate,
            ),
            (
                CurveFailure::BreaksNotCurrent,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::KnotPastBreaks { knot: 2.0 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::ContinuityUndefined { at: 0.5 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::ProjectionVectorZero,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::AuxiliaryVectorZero,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::AuxiliaryAlongProjection,
                SM_ERR_INVALID_INPUT,
                ErrorKind::InvalidInput,
            ),
            (
                CurveFailure::StartsApart {
                    gap: 1.0,
                    tolerance: 0.1,
                },
                SM_ERR,
                ErrorKind::NotWithinTolerance,
            ),
            (
                CurveFailure::EndsApart {
                    gap: 1.0,
                    tolerance: 0.1,
                },
                SM_ERR,
                ErrorKind::NotWithinTolerance,
            ),
        ];
        for (cause, status, kind) in cases {
            let e = SolidError::curve("curve::op", cause);
            assert_eq!(e.sm_status(), Some(status), "{cause:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status), "{cause:?}");
            assert_eq!(e.kind(), kind, "{cause:?}");
            assert!(cause.replaces_upstream_status(), "{cause:?}");
            assert!(!e.is_defect(), "{cause:?}");
            assert!(e.is_status_answer(), "{cause:?}");
            assert_eq!(e.curve_failure(), Some(&cause));
        }
        assert_eq!(
            SolidError::curve(
                "curve::op",
                CurveFailure::BaseCurveNotOwned {
                    edit: CurveEdit::Reparametrise
                }
            )
            .to_string(),
            "curve::op: the curve does not own its base curve, so it cannot be reparametrised"
        );
        assert_eq!(
            SolidError::curve(
                "curve::op",
                CurveFailure::TooManyDerivatives { asked: 4, most: 3 }
            )
            .to_string(),
            "curve::op: 4 derivatives were asked, more than the 3 the routine evaluates"
        );
    }

    /// The curve routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the files listed (with the bare sites each may still hold)
    /// whose conversion waits on other open work.
    #[test]
    fn the_curve_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("curve", 31, &[("curve/ellipse/p1.rs", 13)]);
    }

    /// The sites that build a curve phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_curve_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("CurveFailure", "Failed"),
                Builder::Variant("CurveFailure", "Missing"),
                Builder::Variant("CurveFailure", "ZeroDirection"),
                Builder::Method("curve_failed", Some(1)),
                Builder::Method("curve_missing", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/curve/composite_curve/p2.rs", 2),
            ("solidcore/src/curve/crv_on_surf.rs", 2),
            ("solidcore/src/curve/curve/p1.rs", 13),
            ("solidcore/src/curve/curve/p2.rs", 9),
            ("solidcore/src/curve/curve/p3.rs", 1),
            ("solidcore/src/curve/curve/p4.rs", 7),
            ("solidcore/src/curve/curve/p5.rs", 11),
            ("solidcore/src/curve/curve/p6.rs", 2),
            ("solidcore/src/curve/curve/p8.rs", 8),
            ("solidcore/src/curve/curve/p9.rs", 5),
            ("solidcore/src/curve/curve_cache.rs", 7),
            ("solidcore/src/curve/ellipse/p2.rs", 15),
            ("solidcore/src/curve/line.rs", 4),
            ("solidcore/src/curve/offset_curve.rs", 8),
        ];
        assert_pinned("curve phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
