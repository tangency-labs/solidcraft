//! Why the fillet trace fails: the standard and custom fillet solvers
//! (`crate::blend::fillet_standard_solver`, `crate::blend::fillet_custom_solver`), the fillet
//! intersector (`crate::blend::fillet_intersector`) and the law corners
//! (`crate::blend::law_corners`), as [`super::BlendFailure::Trace`].
//!
//! Upstream: none. Each cause names its class ([`TraceFailure::class`]) and keeps the
//! status the site it replaces returned. None is a defect: the trace's defects are
//! [`super::BlendFailure::Defect`], with their reason, built by the crate's `defect` and
//! `null_defect` here.
//!
//! Deviations: not upstream.

use super::super::{ErrorKind, Failure, SM_ERR, SM_ERR_NULL_POINTER};
use super::BlendFailure;
use crate::SolidError;

/// Why the fillet trace fails ([`super::BlendFailure::Trace`]).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum TraceFailure {
    /// The law corners' classification was given no end parameter for the corner after
    /// piece `piece`: its `ends` hold fewer than one parameter per piece and one more. The
    /// caller's input ([`ErrorKind::InvalidInput`]); status `SM_ERR`.
    LawCornerEndMissing {
        /// The piece before the corner.
        piece: usize,
    },
    /// A law corner's rail has no tangent at its parameter `t`, so whether its corner dips
    /// or peaks along the rail is not defined there ([`ErrorKind::Degenerate`]).
    RailTangentZero {
        /// The rail's parameter at the corner.
        t: f64,
    },
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
    /// The surface/surface solver's base surface `side` is not a face's surface, and the
    /// trace in a live executive puts each rail on its base surface's face. The caller's
    /// input ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    BaseSurfaceNotOnFace {
        /// The base surface, 0 or 1.
        side: usize,
    },
    /// The surface/surface solver's trace of its offset surfaces' intersection succeeded
    /// but left its fillet geometry with no fillet surface: the trace found no curve to skin
    /// ([`ErrorKind::NotFound`]); status `SM_ERR_NULL_POINTER`.
    TraceBuiltNoSurface,
    /// The solver could not compute a section's values: the solver's error is the
    /// failure's cause, and decides whether it is a defect or a refusal
    /// ([`ErrorKind::Failed`]).
    SectionValuesFailed,
    /// The chord between a section and its neighbour along the trace, which stands for the
    /// section's tangent, is `length` long, under the effective zero
    /// ([`ErrorKind::Degenerate`]).
    ChordTooShort {
        /// The chord's length.
        length: f64,
    },
    /// The next section of the trace, at curve parameter `t`, has no tangent, so the span
    /// to it cannot be tested ([`ErrorKind::Degenerate`]).
    SectionTangentZero {
        /// The section's curve parameter.
        t: f64,
    },
    /// The trace's curve has no points to flush into a curve: the trace found none
    /// ([`ErrorKind::NotFound`]); status `SM_ERR_NULL_POINTER`.
    TraceHasNoPoints,
    /// The solve of the section in the middle of a span, at curve parameter `t`, found no
    /// section on its plane: an empty search ([`ErrorKind::NotFound`]).
    MidSectionNotFound {
        /// The curve parameter the section was solved at.
        t: f64,
    },
    /// The self-intersecting fillet's curves could not be smoothed: the smoothing
    /// handler's error is the failure's cause ([`ErrorKind::Failed`]).
    SmoothingFailed,
    /// The fillet surface could not be skinned from the traced curves: the surface
    /// generator's error is the failure's cause ([`ErrorKind::Failed`]).
    SkinningFailed,
    /// The custom solver's drop of a fillet point onto the other filleted edge (or, where
    /// not `onto_edge`, onto the second base surface) found `solutions` feet, not one: the
    /// search did not find the one foot it needs ([`ErrorKind::NotFound`]).
    PointDropNotSingle {
        /// True for the drop onto the edge, false for the drop onto the surface.
        onto_edge: bool,
        /// The feet found.
        solutions: usize,
    },
    /// The custom solver's solve for the fillet cross section on its plane found no
    /// section: an empty search ([`ErrorKind::NotFound`]).
    CrossSectionNotFound,
    /// A base surface's first partial derivative has no length at the custom solver's
    /// current solution, so its equations have no Jacobian there
    /// ([`ErrorKind::Degenerate`]).
    BaseDerivativeZero,
    /// A base surface's normal has no length at the custom solver's current solution:
    /// base surface `side` where the check names one, either where it tests both
    /// ([`ErrorKind::Degenerate`]).
    BaseNormalZero {
        /// The base surface, 0 or 1, where the check names one.
        side: Option<usize>,
    },
    /// The Eiffel tower cross section's bisector meets the segment it must cross in
    /// `solutions` points, not one, so the second arc's centre is not found
    /// ([`ErrorKind::NotFound`]).
    BisectorMissesSegment {
        /// The intersection points found.
        solutions: usize,
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
    /// The faces of the edge the caller asked to fillet meet tangentially (the cosine
    /// `dot` between one face's binormal and the other's normal is within the square root
    /// of the effective zero, where the check measures one), or the edge has no sector to
    /// fillet: the constant radius solver has no side to offset each surface to. The
    /// caller's input ([`ErrorKind::InvalidInput`]); status `SM_ERR`.
    EdgeFacesTangent {
        /// The cosine measured, where the check measures one.
        dot: Option<f64>,
    },
    /// The chamfer solver's rail points are `gap` apart, more than ten times its distance
    /// `distance`: the Newton step has run away from the solution
    /// ([`ErrorKind::NotConverged`]).
    RailPointsDiverged {
        /// The distance between the rail points.
        gap: f64,
        /// The chamfer's distance.
        distance: f64,
    },
    /// The curve-based solver's base curve is not B-spline based, and only a B-spline
    /// based curve is extended past its interval ([`ErrorKind::Unsupported`]).
    CurveNotExtendable,
    /// A knot inserted in the middle of the largest span of the rails, which have
    /// `knots` distinct knots, added none, so the rails cannot be refined to four cross
    /// sections. The caller's rails ([`ErrorKind::InvalidInput`]); status `SM_ERR`.
    RailKnotsNotRefinable {
        /// The rails' distinct knots.
        knots: usize,
    },
    /// The circular cross section surface generator kept `sections` cross sections (it
    /// skips those whose rails are at radii more than a tenth apart), fewer than the
    /// surface it builds needs ([`ErrorKind::NotFound`]).
    TooFewCrossSections {
        /// The cross sections kept.
        sections: usize,
    },
    /// A circular cross section at rail parameter `t` has a radius under the effective
    /// zero: a rail meets the centre line inside the surface ([`ErrorKind::Degenerate`]).
    CrossSectionZeroRadius {
        /// The rails' parameter.
        t: f64,
    },
    /// A circular cross section's angle, `angle` radians, is zero or a half turn: the
    /// rails and the centre line are in line ([`ErrorKind::Degenerate`]).
    CrossSectionAngleDegenerate {
        /// The angle.
        angle: f64,
    },
}

impl TraceFailure {
    /// The failure's class ([`ErrorKind`]): see each variant.
    #[must_use]
    pub const fn class(&self) -> ErrorKind {
        match self {
            Self::LawCornerEndMissing { .. }
            | Self::BaseSurfaceNotOnFace { .. }
            | Self::CrossSectionsIncompatible { .. }
            | Self::EdgeFacesTangent { .. }
            | Self::RailKnotsNotRefinable { .. } => ErrorKind::InvalidInput,
            Self::RailPointsDiverged { .. } => ErrorKind::NotConverged,
            Self::RailTangentZero { .. }
            | Self::ZoneEndsCoincide { .. }
            | Self::ZoneFootAtPole
            | Self::ChordTooShort { .. }
            | Self::SectionTangentZero { .. }
            | Self::BaseDerivativeZero
            | Self::BaseNormalZero { .. }
            | Self::CrossSectionZeroRadius { .. }
            | Self::CrossSectionAngleDegenerate { .. } => ErrorKind::Degenerate,
            Self::ZoneWedgeNearHalfTurn { .. }
            | Self::ZoneWeightNotPositive { .. }
            | Self::CurveNotExtendable => ErrorKind::Unsupported,
            Self::TraceBuiltNoSurface
            | Self::TraceHasNoPoints
            | Self::MidSectionNotFound { .. }
            | Self::PointDropNotSingle { .. }
            | Self::CrossSectionNotFound
            | Self::BisectorMissesSegment { .. }
            | Self::TooFewCrossSections { .. } => ErrorKind::NotFound,
            Self::SectionValuesFailed | Self::SmoothingFailed | Self::SkinningFailed => {
                ErrorKind::Failed
            }
        }
    }

    /// The status the C++ reports unless the site says otherwise (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::BaseSurfaceNotOnFace { .. }
            | Self::TraceBuiltNoSurface
            | Self::TraceHasNoPoints => SM_ERR_NULL_POINTER,
            Self::LawCornerEndMissing { .. }
            | Self::RailTangentZero { .. }
            | Self::ZoneEndsCoincide { .. }
            | Self::ZoneFootAtPole
            | Self::ZoneWedgeNearHalfTurn { .. }
            | Self::ZoneWeightNotPositive { .. }
            | Self::SectionValuesFailed
            | Self::ChordTooShort { .. }
            | Self::SectionTangentZero { .. }
            | Self::MidSectionNotFound { .. }
            | Self::SmoothingFailed
            | Self::SkinningFailed
            | Self::PointDropNotSingle { .. }
            | Self::CrossSectionNotFound
            | Self::BaseDerivativeZero
            | Self::BaseNormalZero { .. }
            | Self::BisectorMissesSegment { .. }
            | Self::CrossSectionsIncompatible { .. }
            | Self::EdgeFacesTangent { .. }
            | Self::RailPointsDiverged { .. }
            | Self::CurveNotExtendable
            | Self::RailKnotsNotRefinable { .. }
            | Self::TooFewCrossSections { .. }
            | Self::CrossSectionZeroRadius { .. }
            | Self::CrossSectionAngleDegenerate { .. } => SM_ERR,
        }
    }
}

impl core::fmt::Display for TraceFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::LawCornerEndMissing { piece } => {
                write!(f, "the law corner after piece {piece} has no end parameter")
            }
            Self::RailTangentZero { t } => {
                write!(f, "the rail's tangent has no length at t = {t}")
            }
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
            Self::BaseSurfaceNotOnFace { side } => {
                write!(f, "base surface {side} is not a face's surface")
            }
            Self::TraceBuiltNoSurface => f.write_str("the trace built no fillet surface"),
            Self::SectionValuesFailed => f.write_str("the section's values could not be computed"),
            Self::ChordTooShort { length } => write!(
                f,
                "the chord standing for the section's tangent is {length} long"
            ),
            Self::SectionTangentZero { t } => {
                write!(f, "the next section's tangent has no length at t = {t}")
            }
            Self::TraceHasNoPoints => f.write_str("the trace has no points to flush"),
            Self::MidSectionNotFound { t } => {
                write!(
                    f,
                    "no section was found in the middle of the span at t = {t}"
                )
            }
            Self::SmoothingFailed => {
                f.write_str("the self-intersecting fillet could not be smoothed")
            }
            Self::SkinningFailed => f.write_str("the fillet surface could not be skinned"),
            Self::PointDropNotSingle {
                onto_edge,
                solutions,
            } => write!(
                f,
                "the point drops onto the {} at {solutions} feet, not one",
                if *onto_edge {
                    "other edge"
                } else {
                    "second surface"
                }
            ),
            Self::CrossSectionNotFound => f.write_str("the fillet cross section has no solution"),
            Self::BaseDerivativeZero => f.write_str("a base surface has a zero derivative"),
            Self::BaseNormalZero { side: Some(side) } => {
                write!(f, "base surface {side}'s normal is zero")
            }
            Self::BaseNormalZero { side: None } => f.write_str("a base surface normal is zero"),
            Self::BisectorMissesSegment { solutions } => write!(
                f,
                "the bisector meets the segment at {solutions} points, not one"
            ),
            Self::CrossSectionsIncompatible {
                rail1_knots,
                rail2_knots,
                sections,
            } => write!(
                f,
                "the rails have {rail1_knots} and {rail2_knots} knots for {sections} cross sections"
            ),
            Self::EdgeFacesTangent { dot: Some(dot) } => {
                write!(f, "the edge's faces meet tangentially (cosine {dot})")
            }
            Self::EdgeFacesTangent { dot: None } => {
                f.write_str("the edge is smooth or has no sector to fillet")
            }
            Self::RailPointsDiverged { gap, distance } => write!(
                f,
                "the rail points are {gap} apart, more than ten times the distance {distance}"
            ),
            Self::CurveNotExtendable => {
                f.write_str("a curve that is not B-spline based cannot be extended")
            }
            Self::RailKnotsNotRefinable { knots } => {
                write!(f, "a knot inserted into rails of {knots} knots added none")
            }
            Self::TooFewCrossSections { sections } => {
                write!(f, "only {sections} cross sections were kept")
            }
            Self::CrossSectionZeroRadius { t } => {
                write!(f, "the cross section at t = {t} has zero radius")
            }
            Self::CrossSectionAngleDegenerate { angle } => write!(
                f,
                "a cross section's angle ({angle} rad) is zero or a half turn"
            ),
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
    Failure::new(operation, BlendFailure::Defect { why })
        .with_status(SM_ERR)
        .into_error()
}

/// [`SolidError::Failed`] for a value the kernel built itself and then did not find
/// (upstream `NER(ptr)` on a `NULL` pointer), as `why` says: [`BlendFailure::Defect`],
/// keeping the status `SM_ERR_NULL_POINTER`.
#[must_use]
#[track_caller]
pub(crate) fn null_defect(operation: &'static str, why: &'static str) -> SolidError {
    Failure::new(operation, BlendFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cause keeps its status through the record, reads as no phrase, stands in for
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
            (TraceFailure::TraceHasNoPoints, SM_ERR_NULL_POINTER),
            (TraceFailure::SkinningFailed, SM_ERR),
            (TraceFailure::ChordTooShort { length: 0.0 }, SM_ERR),
        ];
        for (cause, status) in causes {
            let e = SolidError::blend_trace("blend::op", cause);
            assert_eq!(e.sm_status(), Some(status), "{cause:?}");
            assert_eq!(e.upstream_status(), Some(status), "{cause:?}");
            assert_eq!(e.kind(), cause.class(), "{cause:?}");
            let b = e.blend_failure().unwrap();
            assert!(!b.is_phrase() && b.replaces_upstream_status(), "{cause:?}");
        }
        let d = null_defect("blend::op", "why");
        assert!(d.is_defect());
        assert_eq!(d.sm_status(), Some(SM_ERR_NULL_POINTER));
        let d = defect("blend::op", "why");
        assert!(d.is_defect());
        assert_eq!(d.upstream_status(), Some(SM_ERR));
    }
}
