//! Why a primitive, extrusion, revolution, skin or taper fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the primitive-creation subsystem
//! (`crate::primitive`: the primitive solids and sheets, extrusions, revolutions, curve
//! and planar-path sweeps, skins, tapered walls and the curve and surface helpers of
//! the primitive API). Each also records, for difftest only, the status the C++ reports
//! for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                                   | Status difftest compares      |
//! |----------------------------------------|-------------------------------|
//! | [`PrimitiveFailure::Failed`]           | `SM_ERR`                      |
//! | [`PrimitiveFailure::Missing`]          | `SM_ERR_NULL_POINTER`         |
//! | [`PrimitiveFailure::HelixDeviates`]    | `SM_ERR_NOT_WITHIN_TOLERANCE` |
//! | [`PrimitiveFailure::AxisInsideFace`]   | `SM_ERR_AXIS_INSIDE_FACE`     |
//! | [`PrimitiveFailure::PlaneCannotCover`] | `SM_ERR_INVALID_INPUT`        |
//! | [`PrimitiveFailure::Defect`]           | `SM_ERR` (or the site's, through `with_status`) |
//!
//! [`PrimitiveFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes are sites
//! not yet converted (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. A failure says in a phrase (or its data) what went wrong;
//! the failure's `operation` path says which routine. No failure names an entity: the
//! sites they replace named none. The failures that are checks of a caller's arguments
//! stay [`crate::SolidError::InvalidInput`]. [`PrimitiveFailure::PlaneCannotCover`] is
//! not upstream: it is the refusal of WR-536's fix (`Boolean2D` refuses a plane it cannot grow
//! to cover the other faces).

use super::{
    ErrorKind, Failure, Reason, SM_ERR, SM_ERR_AXIS_INSIDE_FACE, SM_ERR_INVALID_INPUT,
    SM_ERR_NOT_WITHIN_TOLERANCE, SM_ERR_NULL_POINTER,
};

/// Why a primitive, extrusion, revolution, skin or taper fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum PrimitiveFailure {
    /// The routine could not continue, as the phrase says (for example "a cone side has
    /// zero length").
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// face's surface").
    Missing(&'static str),
    /// The fitted helix strays from the analytic helix by more than the caller's
    /// tolerance.
    HelixDeviates {
        /// The largest distance found between the fitted and the analytic helix.
        deviation: f64,
        /// The caller's tolerance.
        tolerance: f64,
    },
    /// A revolution's axis runs inside the face it would turn, so the revolved solid
    /// would cross itself.
    AxisInsideFace,
    /// A planar 2D boolean's face lies on a plane that cannot be grown to cover the
    /// other faces, so the faces rebuilt on it would carry edges far off their surface
    /// (not upstream: WR-536's fix refuses these, where upstream builds them).
    PlaneCannotCover,
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
}

impl PrimitiveFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_) | Self::Defect { .. } => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::HelixDeviates { .. } => SM_ERR_NOT_WITHIN_TOLERANCE,
            Self::AxisInsideFace => SM_ERR_AXIS_INSIDE_FACE,
            Self::PlaneCannotCover => SM_ERR_INVALID_INPUT,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`PrimitiveFailure::Missing`], [`ErrorKind::NotWithinTolerance`] for
    /// [`PrimitiveFailure::HelixDeviates`], [`ErrorKind::InvalidInput`] for
    /// [`PrimitiveFailure::PlaneCannotCover`], [`ErrorKind::Failed`] otherwise.
    /// [`PrimitiveFailure::Defect`] is [`ErrorKind::StageFailed`], a defect.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } => ErrorKind::StageFailed,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for PrimitiveFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::HelixDeviates {
                deviation,
                tolerance,
            } => write!(
                f,
                "the fitted helix strays {deviation} from the helix, more than the tolerance {tolerance}"
            ),
            Self::AxisInsideFace => {
                f.write_str("the revolution axis runs inside the face it would turn")
            }
            Self::PlaneCannotCover => {
                f.write_str("a face's plane cannot be grown to cover the other faces")
            }
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a primitive-creation routine that fails with
    /// `failure`.
    #[must_use]
    pub(crate) fn primitive(operation: &'static str, failure: PrimitiveFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a primitive-creation routine that could not
    /// continue, as the phrase says, with status [`SM_ERR`] (upstream
    /// `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn primitive_failed(operation: &'static str, what: &'static str) -> Self {
        Self::primitive(operation, PrimitiveFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a primitive-creation routine needs
    /// that is not there, as the phrase says, with status [`SM_ERR_NULL_POINTER`]
    /// (upstream `NER(ptr)` on a `NULL` pointer).
    #[must_use]
    pub(crate) fn primitive_missing(operation: &'static str, what: &'static str) -> Self {
        Self::primitive(operation, PrimitiveFailure::Missing(what))
    }

    /// The primitive-creation failure behind this error, for a
    /// [`crate::SolidError::Failed`] with a [`Reason::Primitive`].
    #[must_use]
    pub fn primitive_failure(&self) -> Option<&PrimitiveFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Primitive(p) => Some(p),
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
    fn primitive_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (PrimitiveFailure::Failed("x"), SM_ERR),
            (PrimitiveFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (
                PrimitiveFailure::HelixDeviates {
                    deviation: 2.0,
                    tolerance: 1.0,
                },
                SM_ERR_NOT_WITHIN_TOLERANCE,
            ),
            (PrimitiveFailure::AxisInsideFace, SM_ERR_AXIS_INSIDE_FACE),
            (PrimitiveFailure::PlaneCannotCover, SM_ERR_INVALID_INPUT),
        ];
        for (failure, code) in cases {
            let typed = SolidError::primitive("primitive::op", failure);
            let before = SolidError::Upstream {
                operation: "primitive::op",
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
            assert_eq!(typed.primitive_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::primitive_missing("primitive::op", "the face's surface").to_string(),
            "primitive::op: the face's surface is missing"
        );
        assert_eq!(
            SolidError::primitive_failed("primitive::op", "a cone side has zero length")
                .to_string(),
            "primitive::op: a cone side has zero length"
        );
        assert_eq!(
            SolidError::primitive(
                "primitive::op",
                PrimitiveFailure::HelixDeviates {
                    deviation: 0.5,
                    tolerance: 0.25,
                }
            )
            .kind(),
            ErrorKind::NotWithinTolerance
        );
        assert_eq!(
            SolidError::primitive_failed("primitive::op", "x").primitive_failure(),
            Some(&PrimitiveFailure::Failed("x"))
        );
        assert_eq!(
            SolidError::sm_err("primitive::op").primitive_failure(),
            None
        );
    }

    /// The primitive-creation routines describe their failures: their library code builds
    /// no bare `Upstream` status.
    #[test]
    fn the_primitive_routines_build_no_bare_statuses() {
        assert_eq!(
            super::super::query::tests::bare_statuses_in("primitive", 25),
            Vec::<(String, usize)>::new()
        );
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the face the extrusion just built has no loop, which its own construction always gives";
        let defect = PrimitiveFailure::Defect { why };
        let op = "primitive::op";
        super::super::tests::assert_defect(
            &SolidError::primitive(op, defect),
            &SolidError::primitive(op, PrimitiveFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::primitive(op, PrimitiveFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.primitive_failure(), Some(&defect));
    }

    /// The sites that build a primitive phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_primitive_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("PrimitiveFailure", "Failed"),
                Builder::Variant("PrimitiveFailure", "Missing"),
                Builder::Method("primitive_failed", Some(1)),
                Builder::Method("primitive_missing", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/primitive/api_curve_ops.rs", 1),
            ("solidcore/src/primitive/api_curves.rs", 4),
            ("solidcore/src/primitive/api_primitives.rs", 2),
            ("solidcore/src/primitive/api_surface_ops.rs", 1),
            ("solidcore/src/primitive/primitive_creation/model.rs", 18),
            ("solidcore/src/primitive/primitive_creation/model/merge.rs", 13),
            ("solidcore/src/primitive/primitive_creation/p1.rs", 1),
            ("solidcore/src/primitive/primitive_creation/p1/boolean.rs", 5),
            ("solidcore/src/primitive/primitive_creation/p1/offset_profile.rs", 2),
            ("solidcore/src/primitive/primitive_creation/p1/primitives.rs", 26),
            ("solidcore/src/primitive/primitive_creation/p2/access.rs", 2),
            ("solidcore/src/primitive/primitive_creation/p2/curve_sweeps.rs", 4),
            ("solidcore/src/primitive/primitive_creation/p2/planar_path.rs", 17),
            ("solidcore/src/primitive/primitive_creation/p2/rotational.rs", 10),
            ("solidcore/src/primitive/primitive_creation/p2/skin.rs", 17),
            ("solidcore/src/primitive/primitive_creation/p2/taper.rs", 7),
            ("solidcore/src/primitive/primitive_creation/p2/taper_walls.rs", 22),
            ("solidcore/src/primitive/primitive_creation/p3/access.rs", 1),
            ("solidcore/src/primitive/primitive_creation/p3/construct.rs", 7),
            ("solidcore/src/primitive/primitive_creation/p3/geometry.rs", 5),
            ("solidcore/src/primitive/primitive_creation/p3/ruled.rs", 9),
            ("solidcore/src/primitive/primitive_creation/p3/sweeps.rs", 10),
        ];
        assert_pinned(
            "primitive phrase-cause sites",
            &crate_sites(&PHRASES),
            PINNED,
        );
    }
}
