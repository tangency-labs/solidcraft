//! Why a geometry utility fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the geometry utilities (`crate::geom`:
//! vectors, matrices and transforms, boxes and extents, the point-set and plane
//! utilities, the one-dimensional and global solvers, grids, tolerance relations,
//! attributes and object caches). Each also records, for difftest only, the status the
//! C++ reports for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                                  | Status difftest compares     |
//! |---------------------------------------|------------------------------|
//! | [`GeomFailure::Failed`]               | `SM_ERR`                     |
//! | [`GeomFailure::Missing`]              | `SM_ERR_NULL_POINTER`        |
//! | [`GeomFailure::NearlySingular`]       | `SM_ERR_WARNING`             |
//! | [`GeomFailure::CoincidentPoints`]     | `FALSE` (0)                  |
//! | [`GeomFailure::ToleranceTooSmall`]    | `SM_ERR_INVALID_INPUT`       |
//!
//! Deviations: not upstream. A failure says in a phrase (or by its variant) what went
//! wrong; the failure's `operation` path says which routine. No failure names an
//! entity: the sites they replace named none. The failures that are checks of a
//! caller's arguments stay [`crate::SolidError::InvalidInput`]. `extent1d`, `extent2d`,
//! `expansion` and `local_solve_nd` do not use these yet. `local_solve_nd` reads the
//! singular system of `Matrix::solve_linear_system` by its `SM_ERR` status.

use super::{
    ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT, SM_ERR_NULL_POINTER, SM_ERR_WARNING,
};

/// Why a geometry utility fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum GeomFailure {
    /// The routine could not continue, as the phrase says (for example "the matrix is
    /// singular").
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// edge's curve").
    Missing(&'static str),
    /// The linear system for the circle's centre is nearly singular: the three points
    /// lie (nearly) on one line.
    NearlySingular,
    /// Two of the three points a circle should pass through coincide.
    CoincidentPoints,
    /// The tolerance to set is below the smallest tolerance the kernel tells apart from
    /// zero.
    ToleranceTooSmall {
        /// The tolerance that would be set.
        tolerance: f64,
    },
}

impl GeomFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_) => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::NearlySingular => SM_ERR_WARNING,
            // upstream returns FALSE, not a status
            Self::CoincidentPoints => 0,
            Self::ToleranceTooSmall { .. } => SM_ERR_INVALID_INPUT,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`GeomFailure::Missing`], [`ErrorKind::InvalidInput`] for
    /// [`GeomFailure::ToleranceTooSmall`], [`ErrorKind::Failed`] otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        ErrorKind::of_status(self.comparison_status())
    }
}

impl core::fmt::Display for GeomFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::NearlySingular => {
                f.write_str("the points lie (nearly) on one line: the system is nearly singular")
            }
            Self::CoincidentPoints => f.write_str("two of the points coincide"),
            Self::ToleranceTooSmall { tolerance } => write!(
                f,
                "the tolerance {tolerance} is too small to tell apart from zero"
            ),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a geometry utility that fails with `failure`.
    #[must_use]
    pub(crate) fn geom(operation: &'static str, failure: GeomFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a geometry utility that could not continue, as
    /// the phrase says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn geom_failed(operation: &'static str, what: &'static str) -> Self {
        Self::geom(operation, GeomFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a geometry utility needs that is not
    /// there, as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream
    /// `NER(ptr)` on a `NULL` pointer).
    #[must_use]
    pub(crate) fn geom_missing(operation: &'static str, what: &'static str) -> Self {
        Self::geom(operation, GeomFailure::Missing(what))
    }

    /// The geometry-utility failure behind this error, for a
    /// [`crate::SolidError::Failed`] with a [`Reason::Geom`].
    #[must_use]
    pub fn geom_failure(&self) -> Option<&GeomFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Geom(g) => Some(g),
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
    fn geom_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (GeomFailure::Failed("x"), SM_ERR),
            (GeomFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (GeomFailure::NearlySingular, SM_ERR_WARNING),
            (GeomFailure::CoincidentPoints, 0),
            (
                GeomFailure::ToleranceTooSmall { tolerance: 0.0 },
                SM_ERR_INVALID_INPUT,
            ),
        ];
        for (failure, code) in cases {
            let typed = SolidError::geom("geom::op", failure);
            let before = SolidError::Upstream {
                operation: "geom::op",
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
            assert_eq!(typed.geom_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::geom_missing("geom::op", "the edge's curve").to_string(),
            "geom::op: the edge's curve is missing"
        );
        assert_eq!(
            SolidError::geom(
                "geom::op",
                GeomFailure::ToleranceTooSmall { tolerance: 0.0 }
            )
            .kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(SolidError::sm_err("geom::op").geom_failure(), None);
    }

    /// The geometry utilities describe their failures: their library code builds no bare
    /// `Upstream` status, except the files listed (with the bare sites each may still hold)
    /// whose conversion waits on other open work.
    #[test]
    fn the_geom_routines_build_no_bare_statuses() {
        assert_eq!(
            super::super::query::tests::bare_statuses_in("geom", 30),
            [
                ("geom/extent1d.rs".to_owned(), 1),
                ("geom/extent2d.rs".to_owned(), 2),
                ("geom/local_solve_nd.rs".to_owned(), 1),
            ]
        );
    }
}
