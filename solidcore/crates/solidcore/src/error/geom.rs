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
//! | [`GeomFailure::Disjoint`], [`GeomFailure::PointOutsideExtent`], [`GeomFailure::ZeroDifferenceStep`] | `SM_ERR` |
//! | [`GeomFailure::StageFailed`]          | the site's (`SM_ERR`)        |
//!
//! **The record** (#15034). A check that refuses builds a typed cause with its numbers; a
//! stage that fails with no reason the kernel names is a [`GeomFailure::StageFailed`], a
//! defect ([`ErrorKind::StageFailed`]). The phrase causes ([`GeomFailure::Failed`],
//! [`GeomFailure::Missing`]) are sites not yet converted.
//!
//! Deviations: not upstream. The failure's `operation` path says which routine. No failure names an
//! entity: the sites they replace named none. The failures that are checks of a
//! caller's arguments stay [`crate::SolidError::InvalidInput`]. `expansion` does not use
//! these yet. `local_solve_nd` reads the singular system of
//! `Matrix::solve_linear_system` by its `SM_ERR` status.

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
    /// Two intervals to intersect are disjoint: they have no intersection.
    Disjoint {
        /// The first interval, `[min, max]`.
        a: [f64; 2],
        /// The second interval, `[min, max]`.
        b: [f64; 2],
    },
    /// A point to normalise lies outside the extent (or off a degenerate side of it), so
    /// it has no normalised coordinates.
    PointOutsideExtent {
        /// The point's `u`.
        u: f64,
        /// The point's `v`.
        v: f64,
    },
    /// A difference step for a Jacobian's column is zero: the parameter's interval leaves
    /// no room to step either way from `at`.
    ZeroDifferenceStep {
        /// The parameter's index.
        parameter: usize,
        /// The parameter's value.
        at: f64,
    },
    /// A stage of a geometry utility failed with no reason the kernel names: a defect
    /// ([`ErrorKind::StageFailed`]). The failure holds the operation, the status the C++
    /// reports (for the goldens only) and where it was built.
    StageFailed,
}

impl GeomFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_)
            | Self::Disjoint { .. }
            | Self::PointOutsideExtent { .. }
            | Self::ZeroDifferenceStep { .. }
            | Self::StageFailed => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::NearlySingular => SM_ERR_WARNING,
            // upstream returns FALSE, not a status
            Self::CoincidentPoints => 0,
            Self::ToleranceTooSmall { .. } => SM_ERR_INVALID_INPUT,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::StageFailed`] for
    /// [`GeomFailure::StageFailed`], a defect; [`ErrorKind::Missing`] for
    /// [`GeomFailure::Missing`], [`ErrorKind::InvalidInput`] for
    /// [`GeomFailure::ToleranceTooSmall`], [`ErrorKind::Failed`] otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::StageFailed => ErrorKind::StageFailed,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
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
            Self::Disjoint { a, b } => write!(
                f,
                "the intervals [{}, {}] and [{}, {}] are disjoint",
                a[0], a[1], b[0], b[1]
            ),
            Self::PointOutsideExtent { u, v } => {
                write!(f, "the point ({u}, {v}) lies outside the extent")
            }
            Self::ZeroDifferenceStep { parameter, at } => write!(
                f,
                "the difference step of parameter {parameter} at {at} is zero"
            ),
            Self::StageFailed => write!(f, "{}", ErrorKind::StageFailed),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a geometry utility that fails with `failure`.
    #[must_use]
    #[track_caller]
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

    /// [`crate::SolidError::Failed`] for a stage of a geometry utility that failed with no
    /// reason the kernel names ([`GeomFailure::StageFailed`], a defect), with status
    /// [`SM_ERR`].
    #[must_use]
    #[track_caller]
    pub(crate) fn geom_stage_failed(operation: &'static str) -> Self {
        Self::geom(operation, GeomFailure::StageFailed)
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

    #[test]
    fn a_stage_failure_is_a_defect_and_a_typed_cause_keeps_its_status() {
        super::super::tests::assert_stage_failure(
            &SolidError::geom_stage_failed("geom::op"),
            "geom::op",
            SM_ERR,
        );
        for cause in [
            GeomFailure::Disjoint {
                a: [0.0, 1.0],
                b: [2.0, 3.0],
            },
            GeomFailure::PointOutsideExtent { u: 2.0, v: 0.5 },
            GeomFailure::ZeroDifferenceStep {
                parameter: 0,
                at: 0.5,
            },
        ] {
            let e = SolidError::geom("geom::op", cause);
            assert_eq!(e.upstream_status(), Some(SM_ERR), "{cause:?}");
            assert_eq!(e.kind(), ErrorKind::Failed, "{cause:?}");
            assert!(!e.is_defect(), "{cause:?}");
        }
        assert_eq!(
            SolidError::geom(
                "geom::op",
                GeomFailure::Disjoint {
                    a: [0.0, 1.0],
                    b: [2.0, 3.0]
                }
            )
            .to_string(),
            "geom::op: the intervals [0, 1] and [2, 3] are disjoint"
        );
    }

    /// The geometry utilities describe their failures: their library code builds no bare
    /// `Upstream` status.
    #[test]
    fn the_geom_routines_build_no_bare_statuses() {
        assert_eq!(
            super::super::query::tests::bare_statuses_in("geom", 30),
            Vec::<(String, usize)>::new()
        );
    }
}
