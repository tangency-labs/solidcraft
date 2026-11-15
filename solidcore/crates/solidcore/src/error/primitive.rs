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
}

impl PrimitiveFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_) => SM_ERR,
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
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        ErrorKind::of_status(self.comparison_status())
    }
}

impl core::fmt::Display for PrimitiveFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
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
}
