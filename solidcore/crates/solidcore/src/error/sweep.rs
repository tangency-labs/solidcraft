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
//! | [`SweepFailure::LoftSectionsTooLarge`] | `SM_ERR`                |
//! | [`SweepFailure::LoftMatchTooLarge`]   | `SM_ERR`                 |
//! | [`SweepFailure::Failed`]              | `SM_ERR`                 |
//! | [`SweepFailure::Missing`]             | `SM_ERR_NULL_POINTER`    |
//! | [`SweepFailure::Defect`]              | `SM_ERR` (or the site's, through `with_status`) |
//!
//! [`SweepFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes are sites
//! not yet converted (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
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
    /// Two loft sections too large to pair: the band their pairing is searched in
    /// ([`crate::sweep::loft_sections`]) would have no room within its cells, with `first` and
    /// `second` segments. Sections of equal counts up to `largest` segments pair.
    LoftSectionsTooLarge {
        /// The first section's segments.
        first: usize,
        /// The second section's segments.
        second: usize,
        /// The most segments two sections of equal counts may have.
        largest: usize,
    },
    /// Loft sections whose match ([`crate::sweep::loft_sections`]) would give a section
    /// `segments` segments, more than the caller's bound `most`.
    LoftMatchTooLarge {
        /// The segments the section would get (at least).
        segments: usize,
        /// The caller's bound.
        most: usize,
    },
    /// A sweep routine could not continue, as the phrase says (for example "the swept edge
    /// has no trimming curves").
    Failed(&'static str),
    /// Something a sweep routine needs is missing, as the phrase says (for example "the
    /// edge's curve").
    Missing(&'static str),
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
}

impl SweepFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::RailPastStationCap { .. }
            | Self::LoftSectionsTooLarge { .. }
            | Self::LoftMatchTooLarge { .. }
            | Self::Failed(_)
            | Self::Defect { .. } => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
        }
    }

    /// Its stable [`ErrorKind`]: [`ErrorKind::NotWithinTolerance`] for
    /// [`SweepFailure::RailPastStationCap`] (the result is not within the tolerance asked),
    /// [`ErrorKind::Missing`] for [`SweepFailure::Missing`], [`ErrorKind::StageFailed`] for
    /// [`SweepFailure::Defect`], [`ErrorKind::Failed`] otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::RailPastStationCap { .. } => ErrorKind::NotWithinTolerance,
            Self::LoftSectionsTooLarge { .. } | Self::LoftMatchTooLarge { .. } => {
                ErrorKind::Unsupported
            }
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::Failed(_) | Self::Missing(_) => ErrorKind::of_status(self.comparison_status()),
        }
    }

    /// True for a failure that replaced a C++ status the routine returns
    /// ([`SweepFailure::Failed`], [`SweepFailure::Missing`], [`SweepFailure::Defect`]): it answers with that status the
    /// way the `Upstream` it replaced did ([`crate::error::upstream_status`]). A railed loft's
    /// refusal is solidcore's own and replaced none.
    #[must_use]
    pub const fn replaces_upstream_status(&self) -> bool {
        match self {
            Self::RailPastStationCap { .. }
            | Self::LoftSectionsTooLarge { .. }
            | Self::LoftMatchTooLarge { .. } => false,
            Self::Failed(_) | Self::Missing(_) | Self::Defect { .. } => true,
        }
    }
}

impl core::fmt::Display for SweepFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::RailPastStationCap {
                asked,
                reached,
                wires,
            } => write!(
                f,
                "a railed loft's guided edges were asked within {asked:e} of their rails; at {wires} wires the bound proved is {reached:e}"
            ),
            Self::LoftSectionsTooLarge {
                first,
                second,
                largest,
            } => write!(
                f,
                "loft sections of {first} and {second} segments are too large to pair; sections of equal counts up to {largest} segments pair"
            ),
            Self::LoftMatchTooLarge { segments, most } => write!(
                f,
                "matching loft sections would give a section {segments} segments, more than the {most} allowed"
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

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why =
            "the swept edge the sweep just made has no curve, which the sweep always gives it";
        let defect = SweepFailure::Defect { why };
        let op = "sweep::op";
        super::super::tests::assert_defect(
            &SolidError::sweep(op, defect),
            &SolidError::sweep(op, SweepFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::sweep(op, SweepFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.sweep_failure(), Some(&defect));
    }

    /// The sites that build a sweep phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_sweep_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("SweepFailure", "Failed"),
                Builder::Variant("SweepFailure", "Missing"),
                Builder::Method("sweep_failed", Some(1)),
                Builder::Method("sweep_missing", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/sweep/api_surfaces.rs", 4),
            ("solidcore/src/sweep/cornered.rs", 2),
            ("solidcore/src/sweep/rotational_sweep_geometry.rs", 26),
            ("solidcore/src/sweep/screw.rs", 10),
            ("solidcore/src/sweep/segments.rs", 1),
            ("solidcore/src/sweep/topology_sweep.rs", 4),
            ("solidcore/src/sweep/topology_sweep/access.rs", 4),
            ("solidcore/src/sweep/topology_sweep/engine.rs", 14),
            ("solidcore/src/sweep/topology_sweep/geometry.rs", 11),
            ("solidcore/src/sweep/topology_sweep/store.rs", 3),
            ("solidcore/src/sweep/topology_sweep/store/merge.rs", 6),
            ("solidcore/src/sweep/trans_sweep_geometry.rs", 13),
        ];
        assert_pinned("sweep phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
