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
//!
//! **The record** (#15034). A check that refuses builds a typed cause with its numbers
//! ([`CurveFailure::SingleLinearCurve`], [`CurveFailure::InfiniteTangent`], ...); a stage that
//! fails with no reason the kernel names is a [`CurveFailure::StageFailed`], a defect
//! ([`ErrorKind::StageFailed`]), keeping the error of the call it made as its cause where it
//! has one. The phrase causes ([`CurveFailure::Failed`], [`CurveFailure::Missing`],
//! [`CurveFailure::ZeroDirection`]) are sites not yet converted.
//!
//! Deviations: not upstream. The failure's `operation` path says which routine. No failure names an entity: the sites they
//! replace named none. The failures that are checks of a caller's arguments stay
//! [`crate::SolidError::InvalidInput`].

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
    /// A direction a curve routine needs is the zero vector, as the phrase says (for example
    /// "the projected curve's auxiliary direction").
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
    /// where it has one.
    StageFailed,
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
    /// `SM_ERR_NULL_POINTER` for [`CurveFailure::Missing`] and `SM_ERR_INVALID_INPUT` for
    /// [`CurveFailure::ZeroDirection`].
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
            | Self::PieceOutsideSpan { .. } => SM_ERR,
            Self::Missing(_) | Self::PointSolveOptionMissing { .. } => SM_ERR_NULL_POINTER,
            Self::ZeroDirection(_) => SM_ERR_INVALID_INPUT,
        }
    }

    /// Its stable [`ErrorKind`]: [`ErrorKind::NotWithinTolerance`] for a length bracket's
    /// refusal (the accuracy asked is finer than the computation resolves); for a routine's
    /// failure, the kind of the status it replaced ([`ErrorKind::Failed`],
    /// [`ErrorKind::Missing`], [`ErrorKind::InvalidInput`]) and that of the status of a typed
    /// cause that replaced one; [`ErrorKind::StageFailed`] for a stage that failed, a defect;
    /// [`ErrorKind::PortInvariant`] for a piece outside its span, a port invariant broken.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::LengthBelowFloor { .. } | Self::LengthPastPieceCap { .. } => {
                ErrorKind::NotWithinTolerance
            }
            Self::PieceOutsideSpan { .. } => ErrorKind::PortInvariant,
            Self::StageFailed => ErrorKind::StageFailed,
            Self::PointSolveOptionMissing { .. } => ErrorKind::InvalidInput,
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
            | Self::PointSolveOptionMissing { .. } => true,
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
            Self::PointSolveOptionMissing { option } => {
                write!(f, "the ray solve was called without its {option}")
            }
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

    /// The curve routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the files listed (with the bare sites each may still hold)
    /// whose conversion waits on other open work.
    #[test]
    fn the_curve_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("curve", 31, &[("curve/ellipse/p1.rs", 6)]);
    }
}
