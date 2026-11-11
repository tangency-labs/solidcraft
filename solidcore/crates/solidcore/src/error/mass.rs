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
}

impl MassFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::Failed(_) | Self::StepControl | Self::Integrand | Self::SkippedSpan => SM_ERR,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`MassFailure::Missing`], [`ErrorKind::Failed`] otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        ErrorKind::of_status(self.comparison_status())
    }
}

impl core::fmt::Display for MassFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
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
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/mass");
        for rel in ["integrator.rs", "integrator/properties.rs", "enclosure.rs"] {
            let text = std::fs::read_to_string(src.join(rel))
                .expect("read")
                .replace("\r\n", "\n");
            let library = text
                .find("#[cfg(test)]\nmod tests")
                .map_or(text.as_str(), |at| &text[..at]);
            for needle in [
                "SolidError::sm_err(",
                "SolidError::null_pointer(",
                "SolidError::Upstream {",
            ] {
                assert!(!library.contains(needle), "{needle} in {rel}");
            }
        }
    }
}
