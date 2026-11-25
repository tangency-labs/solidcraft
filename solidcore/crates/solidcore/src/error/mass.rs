//! Why a mass-property computation fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the mass-property subsystem
//! (`crate::mass`: the step-controlled integrator, the edge, face and B-rep property
//! integrations and the enclosed-volume bound). Each also records, for difftest only,
//! the status the C++ reports for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                            | Status difftest compares |
//! |---------------------------------|--------------------------|
//! | [`MassFailure::Defect`]         | `SM_ERR` (or the site's, through `with_status`) |
//! | [`MassFailure::EdgeCurveSingular`], [`MassFailure::TrimNotFinite`] | `SM_ERR`  |
//! | [`MassFailure::EdgeWithoutCurve`], [`MassFailure::FaceWithoutSurface`], [`MassFailure::FaceuseWithoutRegion`] | `SM_ERR_NULL_POINTER` (or the site's, through `with_status`) |
//! | [`MassFailure::FaceWithoutFaceuse`], [`MassFailure::FaceuseWithoutMate`], [`MassFailure::EdgeuseWithoutEdge`], [`MassFailure::EdgeuseWithoutVertexuse`], [`MassFailure::VertexuseWithoutVertex`] | `SM_ERR_NULL_POINTER` |
//! | [`MassFailure::Failed`]         | `SM_ERR`                 |
//! | [`MassFailure::Integrand`]      | `SM_ERR`                 |
//! | [`MassFailure::Missing`]        | `SM_ERR_NULL_POINTER`    |
//! | [`MassFailure::SkippedSpan`]    | `SM_ERR`                 |
//! | [`MassFailure::StepControl`]    | `SM_ERR`                 |
//!
//! [`MassFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The named causes from
//! [`MassFailure::EdgeWithoutCurve`] on are the caller's body ([`ErrorKind::InvalidInput`])
//! or its degenerate geometry. The phrase variants ([`MassFailure::Failed`],
//! [`MassFailure::Missing`]) are no longer built here (the conversion guide:
//! `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. A failure says by its variant what went wrong; the failure's
//! `operation` path says which routine. No failure names an entity: the sites they
//! replace named none. Checks of a caller's arguments that report no status stay
//! [`crate::SolidError::InvalidInput`].

use super::{ErrorKind, Failure, Reason, SM_ERR, SM_ERR_NULL_POINTER};

/// Why a mass-property computation fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MassFailure {
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// The edge's curve has a zero or non-finite derivative where the face's closure looks
    /// for the point nearest a trim point, from both seeds (or the trim point itself is not
    /// finite), so the strip onto the edge has no parameter there.
    /// [`ErrorKind::Degenerate`]; status `SM_ERR`.
    EdgeCurveSingular,
    /// The edge the caller gave has no curve. [`ErrorKind::InvalidInput`]; status
    /// `SM_ERR_NULL_POINTER` unless its site says otherwise.
    EdgeWithoutCurve,
    /// An edge use of a loop of the caller's body has no edge.
    /// [`ErrorKind::InvalidInput`]; status `SM_ERR_NULL_POINTER`.
    EdgeuseWithoutEdge,
    /// An edge use of a loop of the caller's body has no vertex use.
    /// [`ErrorKind::InvalidInput`]; status `SM_ERR_NULL_POINTER`.
    EdgeuseWithoutVertexuse,
    /// A face of the caller's body has no (upward) face use. The B-rep's links are public,
    /// and the mass routines do not validate the body first.
    /// [`ErrorKind::InvalidInput`]; status `SM_ERR_NULL_POINTER`.
    FaceWithoutFaceuse,
    /// The face the caller gave has no surface. [`ErrorKind::InvalidInput`]; status
    /// `SM_ERR_NULL_POINTER`.
    FaceWithoutSurface,
    /// A face use of the caller's body has no mate. [`ErrorKind::InvalidInput`]; status
    /// `SM_ERR_NULL_POINTER`.
    FaceuseWithoutMate,
    /// A face use of the caller's body is in no shell or its shell in no region.
    /// [`ErrorKind::InvalidInput`]; status `SM_ERR_NULL_POINTER`.
    FaceuseWithoutRegion,
    /// The routine could not continue, as the phrase says (for example "the strip has
    /// no such side").
    Failed(&'static str),
    /// The integrand evaluated to no number (a surface evaluated to a non-finite
    /// point).
    Integrand,
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// edge's curve").
    Missing(&'static str),
    /// The integrator took no step over a span of a face.
    SkippedSpan,
    /// The integrator's step control cannot meet the accuracy: the step shrank below
    /// its minimum, or the steps ran out, or the error owed exceeds the accuracy.
    StepControl,
    /// A trim of the face evaluates to a point or tangent that is not finite.
    /// [`ErrorKind::InvalidInput`]; status `SM_ERR`.
    TrimNotFinite,
    /// A vertex use of the caller's body has no vertex. [`ErrorKind::InvalidInput`];
    /// status `SM_ERR_NULL_POINTER`.
    VertexuseWithoutVertex,
}

impl MassFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Defect { .. }
            | Self::EdgeCurveSingular
            | Self::Failed(_)
            | Self::Integrand
            | Self::SkippedSpan
            | Self::StepControl
            | Self::TrimNotFinite => SM_ERR,
            Self::EdgeWithoutCurve
            | Self::EdgeuseWithoutEdge
            | Self::EdgeuseWithoutVertexuse
            | Self::FaceWithoutFaceuse
            | Self::FaceWithoutSurface
            | Self::FaceuseWithoutMate
            | Self::FaceuseWithoutRegion
            | Self::Missing(_)
            | Self::VertexuseWithoutVertex => SM_ERR_NULL_POINTER,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`MassFailure::Missing`], [`ErrorKind::InvalidInput`] for the caller's body (an
    /// edge or face without its geometry, a trim that is not finite, a face use in no
    /// region, a missing link of its topology), [`ErrorKind::Degenerate`] for
    /// [`MassFailure::EdgeCurveSingular`], [`ErrorKind::Failed`] otherwise.
    /// [`MassFailure::Defect`] is [`ErrorKind::StageFailed`], a defect.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::EdgeCurveSingular => ErrorKind::Degenerate,
            Self::EdgeWithoutCurve
            | Self::EdgeuseWithoutEdge
            | Self::EdgeuseWithoutVertexuse
            | Self::FaceWithoutFaceuse
            | Self::FaceWithoutSurface
            | Self::FaceuseWithoutMate
            | Self::FaceuseWithoutRegion
            | Self::TrimNotFinite
            | Self::VertexuseWithoutVertex => ErrorKind::InvalidInput,
            Self::Failed(_) | Self::Integrand | Self::SkippedSpan | Self::StepControl => {
                ErrorKind::Failed
            }
            Self::Missing(_) => ErrorKind::Missing,
        }
    }
}

impl core::fmt::Display for MassFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::EdgeCurveSingular => f.write_str(
                "the edge curve's derivative is zero or not finite where the face's closure \
                 meets it",
            ),
            Self::EdgeWithoutCurve => f.write_str("the edge has no curve"),
            Self::EdgeuseWithoutEdge => f.write_str("the edge use has no edge"),
            Self::EdgeuseWithoutVertexuse => f.write_str("the edge use has no vertex use"),
            Self::FaceWithoutFaceuse => f.write_str("the face has no face use"),
            Self::FaceWithoutSurface => f.write_str("the face has no surface"),
            Self::FaceuseWithoutMate => f.write_str("the face use has no mate"),
            Self::FaceuseWithoutRegion => f.write_str("the face use has no region"),
            Self::Failed(what) => f.write_str(what),
            Self::Integrand => f.write_str("the integrand is not a finite number"),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::SkippedSpan => f.write_str("the integrator took no step over a span of a face"),
            Self::StepControl => {
                f.write_str("the integrator's step control cannot meet the accuracy")
            }
            Self::TrimNotFinite => f.write_str("the trim's point or tangent is not finite"),
            Self::VertexuseWithoutVertex => f.write_str("the vertex use has no vertex"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a mass-property routine that fails with
    /// `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn mass(operation: &'static str, failure: MassFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// The mass-property failure behind this error, for a [`crate::SolidError::Failed`]
    /// with a [`Reason::Mass`].
    #[must_use]
    pub fn mass_failure(&self) -> Option<&MassFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Mass(m) => Some(m),
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
    fn mass_causes_have_their_class_whatever_their_status() {
        crate::error::tests::assert_kind_of_cause(&[
            (MassFailure::Failed("x"), ErrorKind::Failed),
            (MassFailure::Integrand, ErrorKind::Failed),
            (MassFailure::Missing("x"), ErrorKind::Missing),
            (MassFailure::SkippedSpan, ErrorKind::Failed),
            (MassFailure::StepControl, ErrorKind::Failed),
        ]);
    }

    #[test]
    fn mass_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (MassFailure::Failed("x"), SM_ERR),
            (MassFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (MassFailure::StepControl, SM_ERR),
            (MassFailure::Integrand, SM_ERR),
            (MassFailure::SkippedSpan, SM_ERR),
        ];
        for (failure, code) in cases {
            let typed = SolidError::mass("mass::op", failure);
            let before = SolidError::Upstream {
                operation: "mass::op",
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
            assert_eq!(typed.mass_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::mass("mass::op", MassFailure::Missing("the edge's curve")).to_string(),
            "mass::op: the edge's curve is missing"
        );
        assert_eq!(
            SolidError::mass("mass::op", MassFailure::Failed("x")).kind(),
            ErrorKind::Failed
        );
        assert_eq!(SolidError::sm_err("mass::op").mass_failure(), None);
    }

    /// The named causes of the mass properties keep the status of the phrase each
    /// replaced, and have their class.
    #[test]
    fn the_mass_causes_keep_their_status_and_have_their_class() {
        use ErrorKind as K;
        let cases = [
            (
                MassFailure::EdgeWithoutCurve,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                MassFailure::FaceWithoutSurface,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (MassFailure::EdgeCurveSingular, SM_ERR, K::Degenerate),
            (MassFailure::TrimNotFinite, SM_ERR, K::InvalidInput),
            (
                MassFailure::FaceuseWithoutRegion,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                MassFailure::FaceWithoutFaceuse,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                MassFailure::FaceuseWithoutMate,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                MassFailure::EdgeuseWithoutEdge,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                MassFailure::EdgeuseWithoutVertexuse,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
            (
                MassFailure::VertexuseWithoutVertex,
                SM_ERR_NULL_POINTER,
                K::InvalidInput,
            ),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::mass("mass::op", failure);
            assert_eq!(e.upstream_status(), Some(status), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status));
            assert_eq!(e.sm_status(), Some(status), "{failure:?}");
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.mass_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
        // `edge_length`'s check keeps the `SM_ERR` of the phrase it replaced
        let e = Failure::new("mass::op", MassFailure::EdgeWithoutCurve)
            .with_status(SM_ERR)
            .into_error();
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert_eq!(e.kind(), K::InvalidInput);
    }

    /// The mass-property routines build no bare `Upstream` status in their library
    /// code.
    #[test]
    fn the_mass_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("mass", 3, &[]);
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why =
            "the span list the integrator built has a gap, which its own subdivision never leaves";
        let defect = MassFailure::Defect { why };
        let op = "mass::op";
        super::super::tests::assert_defect(
            &SolidError::mass(op, defect),
            &SolidError::mass(op, MassFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::mass(op, MassFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.mass_failure(), Some(&defect));
    }

    /// The sites that build a mass-property phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_mass_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("MassFailure", "Failed"),
                Builder::Variant("MassFailure", "Missing"),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
        ];
        assert_pinned(
            "mass-property phrase-cause sites",
            &crate_sites(&PHRASES),
            PINNED,
        );
    }
}
