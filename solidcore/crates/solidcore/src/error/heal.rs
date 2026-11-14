//! Why a healing or stitching step fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the healing subsystem (`crate::heal`:
//! the face, edge and gap properties the healer gathers, the heal sequence and the
//! stitcher). Each also records, for difftest only, the status the C++ reports for it
//! (crate-private `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                             | Status difftest compares  |
//! |----------------------------------|---------------------------|
//! | [`HealFailure::Failed`]          | `SM_ERR`                  |
//! | [`HealFailure::Missing`]         | `SM_ERR_NULL_POINTER`     |
//! | [`HealFailure::Inconsistent`]    | `SM_ERR_ASSERT_FAILURE`   |
//! | [`HealFailure::NotYetHealable`]  | `SM_ERR_NOTYET_HEAL_FACE` |
//!
//! Deviations: not upstream. A failure says in a phrase (or by its variant) what went
//! wrong; the failure's `operation` path says which routine. No failure names an
//! entity: the sites they replace named none. The failures that are checks of a
//! caller's arguments stay [`crate::SolidError::InvalidInput`].

use super::{
    ErrorKind, Failure, Reason, SM_ERR, SM_ERR_ASSERT_FAILURE, SM_ERR_NOTYET_HEAL_FACE,
    SM_ERR_NULL_POINTER,
};

/// Why a healing or stitching step fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HealFailure {
    /// The step could not continue, as the phrase says (for example "the faces cannot
    /// be stitched").
    Failed(&'static str),
    /// Something the step needs is missing, as the phrase says (for example "the
    /// edge").
    Missing(&'static str),
    /// A consistency check of the step's own data did not hold, as the phrase says.
    Inconsistent(&'static str),
    /// The face has problems the healer does not handle yet; the heal sequence sets it
    /// aside.
    NotYetHealable,
}

impl HealFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_) => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::Inconsistent(_) => SM_ERR_ASSERT_FAILURE,
            Self::NotYetHealable => SM_ERR_NOTYET_HEAL_FACE,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`HealFailure::Missing`], [`ErrorKind::Failed`] otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        ErrorKind::of_status(self.comparison_status())
    }
}

impl core::fmt::Display for HealFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
            Self::NotYetHealable => {
                f.write_str("the face has problems the healer does not handle yet")
            }
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a healing or stitching step that fails with
    /// `failure`.
    #[must_use]
    pub(crate) fn heal(operation: &'static str, failure: HealFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a healing or stitching step that could not
    /// continue, as the phrase says, with status [`SM_ERR`] (upstream
    /// `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn heal_failed(operation: &'static str, what: &'static str) -> Self {
        Self::heal(operation, HealFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a healing or stitching step needs
    /// that is not there, as the phrase says, with status [`SM_ERR_NULL_POINTER`]
    /// (upstream `NER(ptr)` on a `NULL` pointer).
    #[must_use]
    pub(crate) fn heal_missing(operation: &'static str, what: &'static str) -> Self {
        Self::heal(operation, HealFailure::Missing(what))
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
            SolidError::heal_missing("heal::op", "the edge").to_string(),
            "heal::op: the edge is missing"
        );
        assert_eq!(
            SolidError::heal_failed("heal::op", "x").kind(),
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
}
