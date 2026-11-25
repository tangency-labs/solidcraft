//! Why a healing or stitching step fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the healing subsystem (`crate::heal`:
//! the face, edge and gap properties the healer gathers, the heal sequence and the
//! stitcher). Each also records, for difftest only, the status the C++ reports for it
//! (crate-private `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                             | Status difftest compares  |
//! |----------------------------------|---------------------------|
//! | [`HealFailure::Defect`]          | `SM_ERR` (or the site's, through `with_status`) |
//! | [`HealFailure::Failed`]          | `SM_ERR`                  |
//! | [`HealFailure::Inconsistent`]    | `SM_ERR_ASSERT_FAILURE`   |
//! | [`HealFailure::Missing`]         | `SM_ERR_NULL_POINTER`     |
//! | [`HealFailure::NotYetHealable`]  | `SM_ERR_NOTYET_HEAL_FACE` |
//! | the named causes, from [`HealFailure::PairOfDifferentKinds`] on | the status of the phrase each replaced (`comparison_status`) |
//!
//! [`HealFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The named causes from
//! [`HealFailure::PairOfDifferentKinds`] on say what happened, with their numbers where a
//! check measured them. The phrase causes ([`HealFailure::Failed`], `Missing`,
//! `Inconsistent`) are sites not yet converted (the conversion guide:
//! `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. A failure says by its variant (or a phrase) what went wrong;
//! the failure's `operation` path says which routine. No failure names an entity: the
//! sites they replace named none. Checks of a caller's arguments that report no status
//! (upstream dereferences null) stay [`crate::SolidError::InvalidInput`].

use super::{
    ErrorKind, Failure, Reason, SM_ERR, SM_ERR_ASSERT_FAILURE, SM_ERR_NOTYET_HEAL_FACE,
    SM_ERR_NULL_POINTER,
};

/// Why a healing or stitching step fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum HealFailure {
    /// The topology tracker whose changes the caller applies is still tracking.
    /// [`ErrorKind::InvalidInput`]; status `SM_ERR`.
    ChangesStillTracked,
    /// Coincident edges are listed, and the healer has no fix for them (none upstream).
    /// [`ErrorKind::Unsupported`]; status `SM_ERR`.
    CoincidentEdgesUnfixed,
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// The edge's parameter interval is empty or reversed, so a parameter on the edge
    /// cannot be placed in it. [`ErrorKind::Degenerate`]; status `SM_ERR`.
    DegenerateEdgeInterval,
    /// The edge split declined: the point to split at is within tolerance of one of the
    /// edge's ends or outside its interval, or the B-rep is not open for editing. Class
    /// `failed`; status `SM_ERR`.
    EdgeNotSplit,
    /// The edge the caller gave has no curve. [`ErrorKind::InvalidInput`]; status
    /// `SM_ERR_NULL_POINTER`.
    EdgeWithoutCurve,
    /// Two edges to stitch are `distance` apart, at their ends or along them, farther than
    /// `tolerance`. [`ErrorKind::NotWithinTolerance`]; status `SM_ERR`.
    EdgesApart {
        /// The distance between the edges.
        distance: f64,
        /// The tolerance they must be within.
        tolerance: f64,
    },
    /// Gluing a pair of edges failed; the glue's error is the cause. Class `failed`;
    /// whether it is a defect or a refusal is its cause's
    /// ([`crate::SolidError::is_defect`] follows the chain). Status `SM_ERR`.
    EdgesNotGlued,
    /// The sample interval the caller gave the gap function holds no knot interval: its
    /// ends agree to the parametric tolerance. [`ErrorKind::InvalidInput`]; status
    /// `SM_ERR_ASSERT_FAILURE`, of the assert upstream reports.
    EmptySampleInterval,
    /// The caller's lists of faces to keep and to delete are empty or of different
    /// lengths. [`ErrorKind::InvalidInput`]; status `SM_ERR`.
    FaceListsDiffer,
    /// A face to keep and its face to delete do not match: they have different numbers of
    /// vertices or edges, an edge of the face to keep lacks an end vertex, or an edge of one
    /// has no edge of the other between the paired vertices. [`ErrorKind::InvalidInput`];
    /// status `SM_ERR`.
    FacesDoNotMatch,
    /// The step could not continue, as the phrase says (for example "the faces cannot
    /// be stitched").
    Failed(&'static str),
    /// The gap record the caller gave does not define exactly two end points (one of its
    /// parameters is undefined). [`ErrorKind::InvalidInput`]; status `SM_ERR`.
    GapWithoutTwoEnds,
    /// The caller asked for properties of a heal step before the heal step they depend on
    /// has run on the B-rep (the tolerance sizes, or the edge splits at the seams).
    /// [`ErrorKind::InvalidInput`]; status `SM_ERR`.
    HealStepNotRun,
    /// A consistency check of the step's own data did not hold, as the phrase says.
    Inconsistent(&'static str),
    /// Missed edge intersections are listed, and the healer has no fix for them (none
    /// upstream). [`ErrorKind::Unsupported`]; status `SM_ERR`.
    MissedEdgeIntersectionsUnfixed,
    /// Something the step needs is missing, as the phrase says (for example "the
    /// edge").
    Missing(&'static str),
    /// The faces, edges or vertices the caller gave are not part of the B-rep (or the
    /// lists hold none). [`ErrorKind::InvalidInput`]; status `SM_ERR`.
    NotInBrep,
    /// The face has problems the healer does not handle yet; the heal sequence sets it
    /// aside.
    NotYetHealable,
    /// The two entities the stitcher's keep selection is to choose between are not of one
    /// kind (a vertex and an edge): the caller's arguments. [`ErrorKind::InvalidInput`];
    /// status `SM_ERR_NULL_POINTER`, of the null pointer upstream reports.
    PairOfDifferentKinds,
    /// Two vertices to stitch are `distance` apart, farther than `tolerance`.
    /// [`ErrorKind::NotWithinTolerance`]; status `SM_ERR`.
    VerticesApart {
        /// The distance between the vertices (for a vertex with no partner, to the nearest
        /// one).
        distance: f64,
        /// The tolerance they must be within.
        tolerance: f64,
    },
}

impl HealFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::ChangesStillTracked
            | Self::CoincidentEdgesUnfixed
            | Self::Defect { .. }
            | Self::DegenerateEdgeInterval
            | Self::EdgeNotSplit
            | Self::EdgesApart { .. }
            | Self::EdgesNotGlued
            | Self::FaceListsDiffer
            | Self::FacesDoNotMatch
            | Self::Failed(_)
            | Self::GapWithoutTwoEnds
            | Self::HealStepNotRun
            | Self::MissedEdgeIntersectionsUnfixed
            | Self::NotInBrep
            | Self::VerticesApart { .. } => SM_ERR,
            Self::Inconsistent(_) | Self::EmptySampleInterval => SM_ERR_ASSERT_FAILURE,
            Self::Missing(_) | Self::PairOfDifferentKinds | Self::EdgeWithoutCurve => {
                SM_ERR_NULL_POINTER
            }
            Self::NotYetHealable => SM_ERR_NOTYET_HEAL_FACE,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`HealFailure::Missing`], [`ErrorKind::InvalidInput`] for a check of the caller's
    /// arguments or body, [`ErrorKind::Degenerate`] for a degenerate edge interval,
    /// [`ErrorKind::NotWithinTolerance`] for entities to stitch that are too far apart,
    /// [`ErrorKind::Unsupported`] for problems the healer has no fix for,
    /// [`ErrorKind::Failed`] otherwise. [`HealFailure::Defect`] is
    /// [`ErrorKind::StageFailed`], a defect.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::ChangesStillTracked
            | Self::EdgeWithoutCurve
            | Self::EmptySampleInterval
            | Self::FaceListsDiffer
            | Self::FacesDoNotMatch
            | Self::GapWithoutTwoEnds
            | Self::HealStepNotRun
            | Self::NotInBrep
            | Self::PairOfDifferentKinds => ErrorKind::InvalidInput,
            Self::CoincidentEdgesUnfixed | Self::MissedEdgeIntersectionsUnfixed => {
                ErrorKind::Unsupported
            }
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::DegenerateEdgeInterval => ErrorKind::Degenerate,
            Self::EdgeNotSplit
            | Self::EdgesNotGlued
            | Self::Failed(_)
            | Self::Inconsistent(_)
            | Self::NotYetHealable => ErrorKind::Failed,
            Self::Missing(_) => ErrorKind::Missing,
            Self::VerticesApart { .. } | Self::EdgesApart { .. } => ErrorKind::NotWithinTolerance,
        }
    }
}

impl core::fmt::Display for HealFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ChangesStillTracked => {
                f.write_str("the topology changes are still being tracked")
            }
            Self::CoincidentEdgesUnfixed => f.write_str("coincident edges cannot be fixed"),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::DegenerateEdgeInterval => {
                f.write_str("the edge's parameter interval has no length")
            }
            Self::EdgeNotSplit => f.write_str("the edge was not split"),
            Self::EdgeWithoutCurve => f.write_str("the edge has no curve"),
            Self::EdgesApart {
                distance,
                tolerance,
            } => write!(
                f,
                "the edges are {distance} apart, farther than the tolerance {tolerance}"
            ),
            Self::EdgesNotGlued => f.write_str("the edges could not be glued"),
            Self::EmptySampleInterval => f.write_str("the sample interval holds no knot interval"),
            Self::FaceListsDiffer => f.write_str(
                "the lists of faces to keep and to delete are empty or of different lengths",
            ),
            Self::FacesDoNotMatch => {
                f.write_str("the face to keep and the face to delete do not match")
            }
            Self::Failed(what) => f.write_str(what),
            Self::GapWithoutTwoEnds => f.write_str("the gap does not have two end points"),
            Self::HealStepNotRun => {
                f.write_str("the heal step these properties depend on has not run yet")
            }
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
            Self::MissedEdgeIntersectionsUnfixed => {
                f.write_str("missed edge intersections cannot be fixed")
            }
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::NotInBrep => f.write_str("the topology given is not part of the B-rep"),
            Self::NotYetHealable => {
                f.write_str("the face has problems the healer does not handle yet")
            }
            Self::PairOfDifferentKinds => {
                f.write_str("the two entities to choose between are not of one kind")
            }
            Self::VerticesApart {
                distance,
                tolerance,
            } => write!(
                f,
                "the vertices are {distance} apart, farther than the tolerance {tolerance}"
            ),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a healing or stitching step that fails with
    /// `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn heal(operation: &'static str, failure: HealFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// The healing failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Heal`].
    #[must_use]
    pub fn heal_failure(&self) -> Option<&HealFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Heal(h) => Some(h),
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

    /// The causes that took their class from their status have it whatever status they are
    /// recorded with (#21871).
    #[test]
    fn heal_causes_have_their_class_whatever_their_status() {
        crate::error::tests::assert_kind_of_cause(&[
            (HealFailure::Failed("x"), ErrorKind::Failed),
            (HealFailure::Inconsistent("x"), ErrorKind::Failed),
            (HealFailure::Missing("x"), ErrorKind::Missing),
            (HealFailure::NotYetHealable, ErrorKind::Failed),
        ]);
    }

    #[test]
    fn heal_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (HealFailure::Failed("x"), SM_ERR),
            (HealFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (HealFailure::Inconsistent("x"), SM_ERR_ASSERT_FAILURE),
            (HealFailure::NotYetHealable, SM_ERR_NOTYET_HEAL_FACE),
        ];
        for (failure, code) in cases {
            let typed = SolidError::heal("heal::op", failure);
            let before = SolidError::Upstream {
                operation: "heal::op",
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
            assert_eq!(typed.heal_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::heal("heal::op", HealFailure::Missing("the edge")).to_string(),
            "heal::op: the edge is missing"
        );
        assert_eq!(
            SolidError::heal("heal::op", HealFailure::Failed("x")).kind(),
            ErrorKind::Failed
        );
        assert_eq!(SolidError::sm_err("heal::op").heal_failure(), None);
    }

    /// The healing routines build no bare `Upstream` status in their library code, and
    /// their messages name no SMLib API.
    #[test]
    fn the_heal_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("heal", 7, &[]);
        for (rel, text) in super::super::bare_scan::library_files("heal", 7) {
            let library = super::super::bare_scan::library_text(&text);
            assert!(!library.contains("\"SmApi"), "\"SmApi in {rel}");
        }
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why =
            "the stitcher lost the edge pairing it had just made, which its own matching keeps";
        let defect = HealFailure::Defect { why };
        let op = "heal::op";
        super::super::tests::assert_defect(
            &SolidError::heal(op, defect),
            &SolidError::heal(op, HealFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::heal(op, HealFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.heal_failure(), Some(&defect));
    }

    /// The named causes of the stitcher and the healer keep the status of the phrase each
    /// replaced, and have their class.
    #[test]
    fn the_heal_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        let (distance, tolerance) = (1.5, 0.25);
        let cases = [
            (
                HealFailure::PairOfDifferentKinds,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                HealFailure::EdgeWithoutCurve,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (HealFailure::DegenerateEdgeInterval, SM_ERR, K::Degenerate),
            (HealFailure::EdgeNotSplit, SM_ERR, K::Failed),
            (HealFailure::FaceListsDiffer, SM_ERR, K::InvalidInput),
            (HealFailure::NotInBrep, SM_ERR, K::InvalidInput),
            (HealFailure::FacesDoNotMatch, SM_ERR, K::InvalidInput),
            (
                HealFailure::VerticesApart {
                    distance,
                    tolerance,
                },
                SM_ERR,
                K::NotWithinTolerance,
            ),
            (
                HealFailure::EdgesApart {
                    distance,
                    tolerance,
                },
                SM_ERR,
                K::NotWithinTolerance,
            ),
            (HealFailure::EdgesNotGlued, SM_ERR, K::Failed),
            (HealFailure::GapWithoutTwoEnds, SM_ERR, K::InvalidInput),
            (
                HealFailure::EmptySampleInterval,
                SM_ERR_ASSERT_FAILURE,
                K::InvalidInput,
            ),
            (HealFailure::HealStepNotRun, SM_ERR, K::InvalidInput),
            (HealFailure::CoincidentEdgesUnfixed, SM_ERR, K::Unsupported),
            (
                HealFailure::MissedEdgeIntersectionsUnfixed,
                SM_ERR,
                K::Unsupported,
            ),
            (HealFailure::ChangesStillTracked, SM_ERR, K::InvalidInput),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::heal("heal::op", failure);
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status));
            assert_eq!(e.sm_status(), Some(status), "{failure:?}");
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.heal_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
        let apart = HealFailure::EdgesApart {
            distance,
            tolerance,
        };
        assert_eq!(
            SolidError::heal("heal::op", apart).to_string(),
            "heal::op: the edges are 1.5 apart, farther than the tolerance 0.25"
        );
        // the glue's failure stays its cause, and decides defect or refusal
        let why = "a step the kernel got wrong";
        let cause = SolidError::heal("heal::op", HealFailure::Defect { why });
        let e = Failure::new("heal::op", HealFailure::EdgesNotGlued)
            .caused_by(cause.clone())
            .into_error();
        assert_eq!(e.cause(), Some(&cause));
        assert!(e.is_defect());
        assert_eq!(e.upstream_status(), Some(SM_ERR));
    }

    /// The sites that build a healing phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_heal_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("HealFailure", "Failed"),
                Builder::Variant("HealFailure", "Missing"),
                Builder::Variant("HealFailure", "Inconsistent"),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
        ];
        assert_pinned("healing phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
