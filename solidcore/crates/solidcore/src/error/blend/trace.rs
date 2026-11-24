//! Why the fillet trace fails: the standard and custom fillet solvers
//! (`crate::blend::fillet_standard_solver`, `crate::blend::fillet_custom_solver`), the fillet
//! intersector (`crate::blend::fillet_intersector`) and the law corners
//! (`crate::blend::law_corners`), as [`super::BlendFailure::Trace`].
//!
//! Upstream: none. Each cause names its class ([`TraceFailure::class`]) and keeps the
//! status the site it replaces returned. None is a defect: the trace's defects are
//! [`super::BlendFailure::Defect`], with their reason, built by the crate's `defect` here
//! (status `SM_ERR`) and `SolidError::blend_null_defect` (status `SM_ERR_NULL_POINTER`).
//!
//! Deviations: not upstream.

use super::super::{ErrorKind, Failure, SM_ERR, SM_ERR_NULL_POINTER};
use super::BlendFailure;
use crate::SolidError;

/// Why the fillet trace fails ([`super::BlendFailure::Trace`]).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum TraceFailure {
    /// A surface's first partial derivative has no length at the solver's current
    /// solution (a base surface's in the custom solver, an offset surface's in the
    /// standard solvers), so its equations have no Jacobian there
    /// ([`ErrorKind::Degenerate`]).
    BaseDerivativeZero,
    /// A base surface's normal has no length at the custom solver's current solution:
    /// base surface `side` where the check names one, either where it tests both
    /// ([`ErrorKind::Degenerate`]).
    BaseNormalZero {
        /// The base surface, 0 or 1, where the check names one.
        side: Option<usize>,
    },
    /// The surface/surface solver's base surface `side` is not a face's surface, and the
    /// trace in a live executive puts each rail on its base surface's face. The caller's
    /// input ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    BaseSurfaceNotOnFace {
        /// The base surface, 0 or 1.
        side: usize,
    },
    /// The Eiffel tower cross section's bisector meets the segment it must cross in
    /// `solutions` points, not one, so the second arc's centre is not found
    /// ([`ErrorKind::NotFound`]).
    BisectorMissesSegment {
        /// The intersection points found.
        solutions: usize,
    },
    /// The chord between a section and its neighbour along the trace, which stands for the
    /// section's tangent, is `length` long, under the effective zero
    /// ([`ErrorKind::Degenerate`]).
    ChordTooShort {
        /// The chord's length.
        length: f64,
    },
    /// The Eiffel tower surface generator was given rails with `rail1_knots` and
    /// `rail2_knots` distinct knots and `sections` fillet points, which must all be equal
    /// (one cross section at each knot). The caller's input ([`ErrorKind::InvalidInput`]);
    /// status `SM_ERR`.
    CrossSectionsIncompatible {
        /// The first rail's distinct knots.
        rail1_knots: usize,
        /// The second rail's distinct knots.
        rail2_knots: usize,
        /// The fillet points given.
        sections: usize,
    },
    /// The curve-based solver's base curve is not B-spline based, and only a B-spline
    /// based curve is extended past its interval ([`ErrorKind::Unsupported`]).
    CurveNotExtendable,
    /// The faces of the edge the caller asked to fillet meet tangentially (the cosine
    /// `dot` between one face's binormal and the other's normal is within the square root
    /// of the effective zero, where the check measures one), or the edge has no sector to
    /// fillet: the constant radius solver has no side to offset each surface to. The
    /// caller's input ([`ErrorKind::InvalidInput`]); status `SM_ERR`.
    EdgeFacesTangent {
        /// The cosine measured, where the check measures one.
        dot: Option<f64>,
    },
    /// At a joint of a law-cornered round's pieces, the joined surface's weight `joined`
    /// and the next piece's `piece` at an end of the joint are not both positive with a
    /// finite ratio, so the piece cannot be put on the joined surface's homogeneous scale
    /// ([`ErrorKind::Unsupported`]).
    JointWeightNotPositive {
        /// The joined surface's weight at the joint's end.
        joined: f64,
        /// The piece's weight at the joint's end.
        piece: f64,
    },
    /// The law corners' classification was given no end parameter for the corner after
    /// piece `piece`: its `ends` hold fewer parameters than there are pieces. The
    /// caller's input ([`ErrorKind::InvalidInput`]); status `SM_ERR`.
    LawCornerEndMissing {
        /// The piece before the corner.
        piece: usize,
    },
    /// The nearest-point search on a surface found `count` nearest points where one is
    /// needed: an empty or ambiguous search ([`ErrorKind::NotFound`]). The curve's case is
    /// `SolverFailure::NoSingleCurveFoot`.
    NoSingleSurfaceFoot {
        /// The number of nearest points found.
        count: usize,
    },
    /// A knot inserted in the middle of the largest span of the rails, which have
    /// `knots` distinct knots, added none, so the rails cannot be refined to four cross
    /// sections. The caller's rails ([`ErrorKind::InvalidInput`]); status `SM_ERR`.
    RailKnotsNotRefinable {
        /// The rails' distinct knots.
        knots: usize,
    },
    /// The chamfer solver's rail points are `gap` apart, more than ten times its distance
    /// `distance` (the check measures `sqrt(|gap^2 - distance^2|)` against `10 distance`):
    /// the Newton step has run away from the solution. A solve that stops with its numbers,
    /// a defect ([`ErrorKind::StationDidNotConverge`]).
    RailPointsDiverged {
        /// The distance between the rail points.
        gap: f64,
        /// The chamfer's distance.
        distance: f64,
    },
    /// The next section of the trace, at curve parameter `t`, has no tangent, so the span
    /// to it cannot be tested ([`ErrorKind::Degenerate`]).
    SectionTangentZero {
        /// The section's curve parameter.
        t: f64,
    },
    /// The solver could not compute a section's values: the solver's error is the
    /// failure's cause, and decides whether it is a defect or a refusal
    /// ([`ErrorKind::Failed`]).
    SectionValuesFailed,
    /// The fillet surface could not be skinned from the traced curves: the surface
    /// generator's error is the failure's cause ([`ErrorKind::Failed`]).
    SkinningFailed,
    /// The solver's smoothing of a self-intersecting fillet
    /// (`FilletSolverOps::smooth_out_intersection`) handed back fewer curves than the
    /// fillet needs (its centre line, both rails and a uv curve on each side). The kernel's
    /// handler writes back every curve it takes, so this is a caller's own solver breaking
    /// the trait's contract ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    SmoothedCurveMissing,
    /// The self-intersecting fillet's curves could not be smoothed: the smoothing
    /// handler's error is the failure's cause ([`ErrorKind::Failed`]).
    SmoothingFailed,
    /// The constant-radius solver was asked for the fillet surface but holds no fillet
    /// surface generator (one built with `ConstantRadiusAssistedFs::from_parts` has none):
    /// the caller's solver ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    SurfaceGeneratorMissing,
    /// The circular cross section surface generator kept `sections` cross sections (it
    /// skips those whose rails are at radii more than a tenth apart), fewer than the
    /// surface it builds needs ([`ErrorKind::NotFound`]).
    TooFewCrossSections {
        /// The cross sections kept.
        sections: usize,
    },
    /// The surface/surface solver's trace of its offset surfaces' intersection succeeded
    /// but left its fillet geometry with no fillet surface: the trace found no curve to skin
    /// ([`ErrorKind::NotFound`]); status `SM_ERR_NULL_POINTER`.
    TraceBuiltNoSurface,
    /// The trace's curve has no points to flush into a curve: the trace found none
    /// ([`ErrorKind::NotFound`]); status `SM_ERR_NULL_POINTER`.
    TraceHasNoPoints,
    /// The two points a law dip's sphere zone runs between are `distance` apart, within
    /// the effective zero: the zone has no circles to run between
    /// ([`ErrorKind::Degenerate`]).
    ZoneEndsCoincide {
        /// The distance between the two points.
        distance: f64,
    },
    /// A point on one of a law dip's two circles inverts onto the image of the zone's other
    /// end, so the circle's ray in the inverted plane has no direction
    /// ([`ErrorKind::Degenerate`]).
    ZoneFootAtPole,
    /// A law dip's two circles open a wedge whose cosine `cos` is within the square root of
    /// the effective zero of a half turn, past what the zone's one rational quadratic patch
    /// represents to its accuracy ([`ErrorKind::Unsupported`]).
    ZoneWedgeNearHalfTurn {
        /// The cosine of the wedge's angle.
        cos: f64,
    },
    /// A weight of a law dip's sphere zone patch, `weight`, is not positive: the patch does
    /// not represent that zone ([`ErrorKind::Unsupported`]).
    ZoneWeightNotPositive {
        /// The weight.
        weight: f64,
    },
}

impl TraceFailure {
    /// The failure's class ([`ErrorKind`]): see each variant.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(&self) -> ErrorKind {
        match self {
            Self::BaseDerivativeZero => ErrorKind::Degenerate,
            Self::BaseNormalZero { .. } => ErrorKind::Degenerate,
            Self::BaseSurfaceNotOnFace { .. } => ErrorKind::InvalidInput,
            Self::BisectorMissesSegment { .. } => ErrorKind::NotFound,
            Self::ChordTooShort { .. } => ErrorKind::Degenerate,
            Self::CrossSectionsIncompatible { .. } => ErrorKind::InvalidInput,
            Self::CurveNotExtendable => ErrorKind::Unsupported,
            Self::EdgeFacesTangent { .. } => ErrorKind::InvalidInput,
            Self::JointWeightNotPositive { .. } => ErrorKind::Unsupported,
            Self::LawCornerEndMissing { .. } => ErrorKind::InvalidInput,
            Self::NoSingleSurfaceFoot { .. } => ErrorKind::NotFound,
            Self::RailKnotsNotRefinable { .. } => ErrorKind::InvalidInput,
            Self::RailPointsDiverged { .. } => ErrorKind::StationDidNotConverge,
            Self::SectionTangentZero { .. } => ErrorKind::Degenerate,
            Self::SectionValuesFailed => ErrorKind::Failed,
            Self::SkinningFailed => ErrorKind::Failed,
            Self::SmoothedCurveMissing => ErrorKind::InvalidInput,
            Self::SmoothingFailed => ErrorKind::Failed,
            Self::SurfaceGeneratorMissing => ErrorKind::InvalidInput,
            Self::TooFewCrossSections { .. } => ErrorKind::NotFound,
            Self::TraceBuiltNoSurface => ErrorKind::NotFound,
            Self::TraceHasNoPoints => ErrorKind::NotFound,
            Self::ZoneEndsCoincide { .. } => ErrorKind::Degenerate,
            Self::ZoneFootAtPole => ErrorKind::Degenerate,
            Self::ZoneWedgeNearHalfTurn { .. } => ErrorKind::Unsupported,
            Self::ZoneWeightNotPositive { .. } => ErrorKind::Unsupported,
        }
    }

    /// The status the C++ reports unless the site says otherwise (a comparison detail).
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::BaseDerivativeZero => SM_ERR,
            Self::BaseNormalZero { .. } => SM_ERR,
            Self::BaseSurfaceNotOnFace { .. } => SM_ERR_NULL_POINTER,
            Self::BisectorMissesSegment { .. } => SM_ERR,
            Self::ChordTooShort { .. } => SM_ERR,
            Self::CrossSectionsIncompatible { .. } => SM_ERR,
            Self::CurveNotExtendable => SM_ERR,
            Self::EdgeFacesTangent { .. } => SM_ERR,
            Self::JointWeightNotPositive { .. } => SM_ERR,
            Self::LawCornerEndMissing { .. } => SM_ERR,
            Self::NoSingleSurfaceFoot { .. } => SM_ERR,
            Self::RailKnotsNotRefinable { .. } => SM_ERR,
            Self::RailPointsDiverged { .. } => SM_ERR,
            Self::SectionTangentZero { .. } => SM_ERR,
            Self::SectionValuesFailed => SM_ERR,
            Self::SkinningFailed => SM_ERR,
            Self::SmoothedCurveMissing => SM_ERR_NULL_POINTER,
            Self::SmoothingFailed => SM_ERR,
            Self::SurfaceGeneratorMissing => SM_ERR_NULL_POINTER,
            Self::TooFewCrossSections { .. } => SM_ERR,
            Self::TraceBuiltNoSurface => SM_ERR_NULL_POINTER,
            Self::TraceHasNoPoints => SM_ERR_NULL_POINTER,
            Self::ZoneEndsCoincide { .. } => SM_ERR,
            Self::ZoneFootAtPole => SM_ERR,
            Self::ZoneWedgeNearHalfTurn { .. } => SM_ERR,
            Self::ZoneWeightNotPositive { .. } => SM_ERR,
        }
    }
}

impl core::fmt::Display for TraceFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BaseDerivativeZero => f.write_str("a solver's surface has a zero derivative"),
            Self::BaseNormalZero { side: Some(side) } => {
                write!(f, "base surface {side}'s normal is zero")
            }
            Self::BaseNormalZero { side: None } => f.write_str("a base surface normal is zero"),
            Self::BaseSurfaceNotOnFace { side } => {
                write!(f, "base surface {side} is not a face's surface")
            }
            Self::BisectorMissesSegment { solutions } => write!(
                f,
                "the bisector meets the segment at {solutions} points, not one"
            ),
            Self::ChordTooShort { length } => write!(
                f,
                "the chord standing for the section's tangent is {length} long"
            ),
            Self::CrossSectionsIncompatible {
                rail1_knots,
                rail2_knots,
                sections,
            } => write!(
                f,
                "the rails have {rail1_knots} and {rail2_knots} knots for {sections} cross sections"
            ),
            Self::CurveNotExtendable => {
                f.write_str("a curve that is not B-spline based cannot be extended")
            }
            Self::EdgeFacesTangent { dot: Some(dot) } => {
                write!(f, "the edge's faces meet tangentially (cosine {dot})")
            }
            Self::EdgeFacesTangent { dot: None } => {
                f.write_str("the edge is smooth or has no sector to fillet")
            }
            Self::JointWeightNotPositive { joined, piece } => write!(
                f,
                "the weights at a joint of the round's pieces, {joined} and {piece}, are not \
                 both positive with a finite ratio"
            ),
            Self::LawCornerEndMissing { piece } => {
                write!(f, "the law corner after piece {piece} has no end parameter")
            }
            Self::NoSingleSurfaceFoot { count } => write!(
                f,
                "the nearest-point search on the surface found {count} points, not one"
            ),
            Self::RailKnotsNotRefinable { knots } => {
                write!(f, "a knot inserted into rails of {knots} knots added none")
            }
            Self::RailPointsDiverged { gap, distance } => write!(
                f,
                "the rail points are {gap} apart, more than ten times the distance {distance}"
            ),
            Self::SectionTangentZero { t } => {
                write!(f, "the next section's tangent has no length at t = {t}")
            }
            Self::SectionValuesFailed => f.write_str("the section's values could not be computed"),
            Self::SkinningFailed => f.write_str("the fillet surface could not be skinned"),
            Self::SmoothedCurveMissing => {
                f.write_str("the solver's smoothing handed back fewer curves than it took")
            }
            Self::SmoothingFailed => {
                f.write_str("the self-intersecting fillet could not be smoothed")
            }
            Self::SurfaceGeneratorMissing => {
                f.write_str("the solver has no fillet surface generator")
            }
            Self::TooFewCrossSections { sections } => {
                write!(f, "only {sections} cross sections were kept")
            }
            Self::TraceBuiltNoSurface => f.write_str("the trace built no fillet surface"),
            Self::TraceHasNoPoints => f.write_str("the trace has no points to flush"),
            Self::ZoneEndsCoincide { distance } => write!(
                f,
                "the sphere zone's two ends are {distance} apart, within the effective zero"
            ),
            Self::ZoneFootAtPole => {
                f.write_str("a point of the sphere zone's circles inverts onto its other end")
            }
            Self::ZoneWedgeNearHalfTurn { cos } => write!(
                f,
                "the sphere zone's wedge (cosine {cos}) is too near a half turn for one patch"
            ),
            Self::ZoneWeightNotPositive { weight } => {
                write!(
                    f,
                    "a weight of the sphere zone's patch is {weight}, not positive"
                )
            }
        }
    }
}

impl SolidError {
    /// [`SolidError::Failed`] for a failure of the fillet trace
    /// ([`super::BlendFailure::Trace`]), with the cause's status.
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_trace(operation: &'static str, failure: TraceFailure) -> Self {
        Self::blend(operation, BlendFailure::Trace(failure))
    }
}

/// [`SolidError::Failed`] for a step of the fillet trace that is the kernel's fault, as
/// `why` says: [`BlendFailure::Defect`], keeping the status `SM_ERR` of the site it
/// replaces.
#[must_use]
#[track_caller]
pub(crate) fn defect(operation: &'static str, why: &'static str) -> SolidError {
    Failure::new(operation, BlendFailure::Defect { why }).into_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cause keeps its status through the record, stands in for
    /// the status its site returned, and takes its class from the cause.
    #[test]
    fn trace_causes_keep_their_status_and_class() {
        let causes = [
            (TraceFailure::LawCornerEndMissing { piece: 0 }, SM_ERR),
            (
                TraceFailure::BaseSurfaceNotOnFace { side: 1 },
                SM_ERR_NULL_POINTER,
            ),
            (TraceFailure::TraceBuiltNoSurface, SM_ERR_NULL_POINTER),
            (TraceFailure::SurfaceGeneratorMissing, SM_ERR_NULL_POINTER),
            (TraceFailure::SmoothedCurveMissing, SM_ERR_NULL_POINTER),
            (TraceFailure::TraceHasNoPoints, SM_ERR_NULL_POINTER),
            (TraceFailure::SkinningFailed, SM_ERR),
            (TraceFailure::ChordTooShort { length: 0.0 }, SM_ERR),
        ];
        for (cause, status) in causes {
            let e = SolidError::blend_trace("blend::op", cause);
            assert_eq!(e.sm_status(), Some(status), "{cause:?}");
            assert_eq!(e.upstream_status(), Some(status), "{cause:?}");
            assert_eq!(e.kind(), cause.class(), "{cause:?}");
        }
        // A solve that stops with its numbers is a defect,
        // and keeps the site's SM_ERR.
        let e = SolidError::blend_trace(
            "blend::op",
            TraceFailure::RailPointsDiverged {
                gap: 11.0,
                distance: 1.0,
            },
        );
        assert_eq!(e.kind(), ErrorKind::StationDidNotConverge);
        assert_eq!(e.sm_status(), Some(SM_ERR));
        assert!(e.is_defect());
        let d = SolidError::blend_null_defect("blend::op", "why");
        assert!(d.is_defect());
        assert_eq!(d.sm_status(), Some(SM_ERR_NULL_POINTER));
        let d = defect("blend::op", "why");
        assert!(d.is_defect());
        assert_eq!(d.upstream_status(), Some(SM_ERR));
    }

    /// The three wrapping causes keep `SM_ERR` and carry the caught error as their cause,
    /// as the sites in `fillet_intersector` build them; the caught error decides the
    /// defect-or-refusal reading.
    #[test]
    fn wrapping_causes_keep_the_caught_error() {
        for cause in [
            TraceFailure::SectionValuesFailed,
            TraceFailure::SmoothingFailed,
            TraceFailure::SkinningFailed,
        ] {
            let inner = SolidError::blend_null_defect("blend::inner", "why");
            let e = Failure::new("blend::op", BlendFailure::Trace(cause))
                .caused_by(inner.clone())
                .into_error();
            assert_eq!(e.sm_status(), Some(SM_ERR), "{cause:?}");
            assert_eq!(e.kind(), ErrorKind::Failed, "{cause:?}");
            assert_eq!(e.cause(), Some(&inner), "{cause:?}");
            // the caught error decides whether the failure is a defect or a refusal
            assert!(e.is_defect() && inner.is_defect(), "{cause:?}");
        }
    }
}
