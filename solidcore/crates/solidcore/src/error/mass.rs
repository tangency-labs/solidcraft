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
//! | [`MassFailure::Failed`]         | `SM_ERR`                 |
//! | [`MassFailure::Missing`]        | `SM_ERR_NULL_POINTER`    |
//! | [`MassFailure::StepControl`]    | `SM_ERR`                 |
//! | [`MassFailure::Integrand`]      | `SM_ERR`                 |
//! | [`MassFailure::SkippedSpan`]    | `SM_ERR`                 |
//! | [`MassFailure::Defect`]         | `SM_ERR` (or the site's, through `with_status`) |
//!
//! [`MassFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes are sites
//! not yet converted (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. A failure says in a phrase (or by its variant) what went
//! wrong; the failure's `operation` path says which routine. No failure names an
//! entity: the sites they replace named none. The failures that are checks of a
//! caller's arguments stay [`crate::SolidError::InvalidInput`].

use super::{ErrorKind, Failure, Reason, SM_ERR, SM_ERR_NULL_POINTER};

/// Why a mass-property computation fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MassFailure {
    /// The routine could not continue, as the phrase says (for example "the strip has
    /// no such side").
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// edge's curve").
    Missing(&'static str),
    /// The integrator's step control cannot meet the accuracy: the step shrank below
    /// its minimum, or the steps ran out, or the error owed exceeds the accuracy.
    StepControl,
    /// The integrand evaluated to no number (a surface evaluated to a non-finite
    /// point).
    Integrand,
    /// The integrator took no step over a span of a face.
    SkippedSpan,
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
}

impl MassFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::Failed(_)
            | Self::StepControl
            | Self::Integrand
            | Self::SkippedSpan
            | Self::Defect { .. } => SM_ERR,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`MassFailure::Missing`], [`ErrorKind::Failed`] otherwise.
    /// [`MassFailure::Defect`] is [`ErrorKind::StageFailed`], a defect.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } => ErrorKind::StageFailed,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for MassFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::StepControl => {
                f.write_str("the integrator's step control cannot meet the accuracy")
            }
            Self::Integrand => f.write_str("the integrand is not a finite number"),
            Self::SkippedSpan => f.write_str("the integrator took no step over a span of a face"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a mass-property routine that fails with
    /// `failure`.
    #[must_use]
    pub(crate) fn mass(operation: &'static str, failure: MassFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a mass-property routine that could not
    /// continue, as the phrase says, with status [`SM_ERR`] (upstream
    /// `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn mass_failed(operation: &'static str, what: &'static str) -> Self {
        Self::mass(operation, MassFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a mass-property routine needs that is
    /// not there, as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream
    /// `NER(ptr)` on a `NULL` pointer).
    #[must_use]
    pub(crate) fn mass_missing(operation: &'static str, what: &'static str) -> Self {
        Self::mass(operation, MassFailure::Missing(what))
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
            SolidError::mass_missing("mass::op", "the edge's curve").to_string(),
            "mass::op: the edge's curve is missing"
        );
        assert_eq!(
            SolidError::mass_failed("mass::op", "x").kind(),
            ErrorKind::Failed
        );
        assert_eq!(SolidError::sm_err("mass::op").mass_failure(), None);
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
                Builder::Method("mass_failed", Some(1)),
                Builder::Method("mass_missing", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/mass/enclosure.rs", 24),
            ("solidcore/src/mass/integrator/properties.rs", 29),
        ];
        assert_pinned(
            "mass-property phrase-cause sites",
            &crate_sites(&PHRASES),
            PINNED,
        );
    }
}
