//! Why a sweep or loft refuses, in solidcore's terms (#3140).
//!
//! Upstream: none. A railed loft ([`crate::sweep::loft_rails`]) certifies each guided edge
//! within the caller's tolerance of its rail, adding stations where the bound is above it; past
//! the caller's most wires it refuses and names the bound reached. The other failures describe
//! the sweep subsystem's routines (`crate::sweep`: the topology sweep and its translational and
//! rotational geometry, the screw sweep, the cornered and segmented sweeps and the sweep API's
//! surfaces), each recording, for difftest only, the status the C++ reports for it
//! (crate-private `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                                  | Status difftest compares |
//! |---------------------------------------|--------------------------|
//! | [`SweepFailure::RailPastStationCap`]  | `SM_ERR`                 |
//! | [`SweepFailure::Failed`]              | `SM_ERR`                 |
//! | [`SweepFailure::Missing`]             | `SM_ERR_NULL_POINTER`    |
//!
//! Deviations: not upstream. A failure says in a phrase what went wrong; the failure's
//! `operation` path says which routine. No failure names an entity: the sites they replace named
//! none. The failures that are checks of a caller's arguments stay
//! [`crate::SolidError::InvalidInput`].

use super::{ErrorKind, Failure, Reason, SM_ERR, SM_ERR_NULL_POINTER};

/// Why a sweep or loft refuses.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum SweepFailure {
    /// A railed loft's guided edges were asked within `asked` of their rails, and at the most
    /// wires asked, `wires`, the bound proved was still `reached`.
    RailPastStationCap {
        /// The tolerance asked.
        asked: f64,
        /// The largest bound proved between a rail and the skin at the cap.
        reached: f64,
        /// The wires (sections and stations) at the cap.
        wires: usize,
    },
    /// A sweep routine could not continue, as the phrase says (for example "the swept edge
    /// has no trimming curves").
    Failed(&'static str),
    /// Something a sweep routine needs is missing, as the phrase says (for example "the
    /// edge's curve").
    Missing(&'static str),
}

impl SweepFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::RailPastStationCap { .. } | Self::Failed(_) => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
        }
    }

    /// Its stable [`ErrorKind`]: [`ErrorKind::NotWithinTolerance`] for
    /// [`SweepFailure::RailPastStationCap`] (the result is not within the tolerance asked),
    /// [`ErrorKind::Missing`] for [`SweepFailure::Missing`], [`ErrorKind::Failed`] otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::RailPastStationCap { .. } => ErrorKind::NotWithinTolerance,
            Self::Failed(_) | Self::Missing(_) => ErrorKind::of_status(self.comparison_status()),
        }
    }

    /// True for a failure that replaced a C++ status the routine returns
    /// ([`SweepFailure::Failed`], [`SweepFailure::Missing`]): it answers with that status the
    /// way the `Upstream` it replaced did ([`crate::error::upstream_status`]). A railed loft's
    /// refusal is solidcore's own and replaced none.
    #[must_use]
    pub const fn replaces_upstream_status(&self) -> bool {
        match self {
            Self::RailPastStationCap { .. } => false,
            Self::Failed(_) | Self::Missing(_) => true,
        }
    }
}

impl core::fmt::Display for SweepFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::RailPastStationCap {
                asked,
                reached,
                wires,
            } => write!(
                f,
                "a railed loft's guided edges were asked within {asked:e} of their rails; at {wires} wires the bound proved is {reached:e}"
            ),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a sweep routine that fails with `failure`.
    #[must_use]
    pub(crate) fn sweep(operation: &'static str, failure: SweepFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a sweep routine that could not continue, as the phrase
    /// says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn sweep_failed(operation: &'static str, what: &'static str) -> Self {
        Self::sweep(operation, SweepFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a sweep routine needs that is not there, as
    /// the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)` on a `NULL`
    /// pointer).
    #[must_use]
    pub(crate) fn sweep_missing(operation: &'static str, what: &'static str) -> Self {
        Self::sweep(operation, SweepFailure::Missing(what))
    }

    /// The sweep failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Sweep`].
    #[must_use]
    pub fn sweep_failure(&self) -> Option<&SweepFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Sweep(w) => Some(w),
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
    fn sweep_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (SweepFailure::Failed("x"), SM_ERR),
            (SweepFailure::Missing("x"), SM_ERR_NULL_POINTER),
        ];
        for (failure, code) in cases {
            let typed = SolidError::sweep("sweep::op", failure);
            let before = SolidError::Upstream {
                operation: "sweep::op",
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
            assert_eq!(typed.sweep_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::sweep_missing("sweep::op", "the edge's curve").to_string(),
            "sweep::op: the edge's curve is missing"
        );
        assert_eq!(
            SolidError::sweep_failed("sweep::op", "the face was already swept").to_string(),
            "sweep::op: the face was already swept"
        );
        assert_eq!(SolidError::sm_err("sweep::op").sweep_failure(), None);
    }

    /// A railed loft's refusal reports what it reported before the sweep routines' failures
    /// joined it: status `SM_ERR`, kind `not_within_tolerance`, and no upstream status (it
    /// replaced none).
    #[test]
    fn a_railed_loft_refusal_reports_as_before() {
        let cap = SweepFailure::RailPastStationCap {
            asked: 1e-6,
            reached: 1e-3,
            wires: 64,
        };
        let e = SolidError::sweep("sweep::loft_rails", cap);
        assert_eq!(e.sm_status(), Some(SM_ERR));
        assert_eq!(e.kind(), ErrorKind::NotWithinTolerance);
        assert_eq!(e.upstream_status(), None);
        assert_eq!(super::super::upstream_status(&e), None);
        assert!(!cap.replaces_upstream_status());
        assert!(e.is_status_answer());
        assert!(!e.is_port_failure());
    }

    /// The sweep routines describe their failures: their library code builds no bare
    /// `Upstream` status (the count is the bare sites a file may still hold while a site
    /// waits on other work).
    #[test]
    fn the_sweep_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("sweep", 21, &[]);
    }
}
