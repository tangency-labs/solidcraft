//! Why a blend routine fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the blend subsystem (`crate::blend`: the
//! fillet and feature executives, the fillet geometry, solvers, sections, corners,
//! overflows, intersectors, outlines, removal and the exact and rolling-ball fillets).
//! Each also records, for difftest only, the status the C++ reports for it (crate-private
//! `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                                   | Status difftest compares          |
//! |----------------------------------------|-----------------------------------|
//! | [`BlendFailure::Failed`]               | `SM_ERR`                          |
//! | [`BlendFailure::Missing`]              | `SM_ERR_NULL_POINTER`             |
//! | [`BlendFailure::InvalidValue`]         | `SM_ERR_INVALID_INPUT`            |
//! | [`BlendFailure::Inconsistent`]         | `SM_ERR_ASSERT_FAILURE`           |
//! | [`BlendFailure::BadIntersections`]     | `SM_ERR_BAD_INTERSECTIONS`        |
//! | [`BlendFailure::NotConverged`]         | `SM_ERR_NOT_CONVERGING`           |
//! | [`BlendFailure::OutsideDomain`]        | `SM_ERR_OUTSIDE_OF_DOMAIN`        |
//! | [`BlendFailure::NotWithinTolerance`]   | `SM_ERR_NOT_WITHIN_TOLERANCE`     |
//! | [`BlendFailure::PassedOn`]             | the status of the stage passed on |
//!
//! Deviations: not upstream. A failure says in a phrase what went wrong; the failure's
//! `operation` path says which routine. No reason names an entity of its own: a failure is
//! at the entity the site it replaces named (`SolidError::blend_at`). The failures that are
//! checks of a caller's arguments stay [`crate::SolidError::InvalidInput`]. Every reason here
//! replaced a bare status ([`BlendFailure::replaces_upstream_status`]), so the decisions that
//! read an upstream status read these alike. Only part of `crate::blend` uses these yet; the
//! files that still build bare statuses are listed, with their counts, in this module's
//! ratchet test.

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_ASSERT_FAILURE, SM_ERR_BAD_INTERSECTIONS,
    SM_ERR_INVALID_INPUT, SM_ERR_NOT_CONVERGING, SM_ERR_NOT_WITHIN_TOLERANCE, SM_ERR_NULL_POINTER,
    SM_ERR_OUTSIDE_OF_DOMAIN,
};

/// Why a blend routine fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlendFailure {
    /// The routine could not continue, as the phrase says (for example "the fillet's law
    /// has no value at the base"). Upstream `return SE(SM_ERR)`.
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the edge
    /// use's edge"). Upstream `NER(ptr)` on a `NULL` pointer, or a `NULL` it would
    /// dereference.
    Missing(&'static str),
    /// A value the routine is given or computes with is not usable, as the phrase says
    /// (for example "a G^n section's order"). Upstream returns `SM_ERR_INVALID_INPUT` as a
    /// status here, so its kind is [`ErrorKind::InvalidInput`].
    InvalidValue(&'static str),
    /// A consistency check of the routine's own data did not hold, as the phrase says
    /// (upstream `AERN`, `SM_ERR_ASSERT_FAILURE`).
    Inconsistent(&'static str),
    /// Intersections the routine needs are not usable, as the phrase says (for example "the
    /// two surfaces meet in more than two points"); upstream `SM_ERR_BAD_INTERSECTIONS`.
    BadIntersections(&'static str),
    /// An iteration did not converge, as the phrase says; upstream
    /// `SM_ERR_NOT_CONVERGING`.
    NotConverged(&'static str),
    /// A parameter lies outside its domain, as the phrase says; upstream
    /// `SM_ERR_OUTSIDE_OF_DOMAIN`.
    OutsideDomain(&'static str),
    /// A result is farther than the tolerance from where it must be, as the phrase says;
    /// upstream `SM_ERR_NOT_WITHIN_TOLERANCE`.
    NotWithinTolerance(&'static str),
    /// A stage the routine calls failed, as the phrase says, and the routine answers with
    /// that stage's `status` (upstream `SER` of the stage's status, or a status chosen at
    /// run time). The stage's error is not kept as a cause: it may be a solidcore-side
    /// failure, which the bare status it replaces did not carry.
    PassedOn {
        /// What failed.
        what: &'static str,
        /// The status the routine answers with (a comparison detail).
        status: i32,
    },
}

impl BlendFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_) => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_) => SM_ERR_INVALID_INPUT,
            Self::Inconsistent(_) => SM_ERR_ASSERT_FAILURE,
            Self::BadIntersections(_) => SM_ERR_BAD_INTERSECTIONS,
            Self::NotConverged(_) => SM_ERR_NOT_CONVERGING,
            Self::OutsideDomain(_) => SM_ERR_OUTSIDE_OF_DOMAIN,
            Self::NotWithinTolerance(_) => SM_ERR_NOT_WITHIN_TOLERANCE,
            Self::PassedOn { status, .. } => *status,
        }
    }

    /// The failure's stable [`ErrorKind`]: the kind of the status it replaced
    /// ([`ErrorKind::Failed`], [`ErrorKind::Missing`], [`ErrorKind::InvalidInput`],
    /// [`ErrorKind::NotConverged`], [`ErrorKind::OutsideDomain`],
    /// [`ErrorKind::NotWithinTolerance`], or the passed-on status's kind).
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        ErrorKind::of_status(self.comparison_status())
    }

    /// True when the failure replaced a bare status the routine returned (an
    /// [`crate::SolidError::Upstream`] before solidcore described it): the decisions that
    /// read an upstream status ([`crate::error::upstream_status`]) treat it as that status.
    /// Every reason here does; a refusal native to solidcore added later would not.
    #[must_use]
    pub const fn replaces_upstream_status(&self) -> bool {
        match self {
            Self::Failed(_)
            | Self::Missing(_)
            | Self::InvalidValue(_)
            | Self::Inconsistent(_)
            | Self::BadIntersections(_)
            | Self::NotConverged(_)
            | Self::OutsideDomain(_)
            | Self::NotWithinTolerance(_)
            | Self::PassedOn { .. } => true,
        }
    }
}

impl core::fmt::Display for BlendFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what)
            | Self::InvalidValue(what)
            | Self::BadIntersections(what)
            | Self::NotConverged(what)
            | Self::OutsideDomain(what)
            | Self::NotWithinTolerance(what)
            | Self::PassedOn { what, .. } => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a blend routine that fails with `failure`.
    #[must_use]
    pub(crate) fn blend(operation: &'static str, failure: BlendFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a blend routine that fails with `failure` about
    /// `entity` (where the bare status it replaces named one).
    #[must_use]
    pub(crate) fn blend_at(
        operation: &'static str,
        failure: BlendFailure,
        entity: EntityRef,
    ) -> Self {
        Failure::new(operation, failure).at(entity).into_error()
    }

    /// [`crate::SolidError::Failed`] for a blend routine that could not continue, as the
    /// phrase says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn blend_failed(operation: &'static str, what: &'static str) -> Self {
        Self::blend(operation, BlendFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a blend routine needs that is not there,
    /// as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)` on a
    /// `NULL` pointer).
    #[must_use]
    pub(crate) fn blend_missing(operation: &'static str, what: &'static str) -> Self {
        Self::blend(operation, BlendFailure::Missing(what))
    }

    /// [`crate::SolidError::Failed`] for a value a blend routine is given or computes with
    /// that is not usable, as the phrase says, with status [`SM_ERR_INVALID_INPUT`]
    /// (upstream `return(SM_ERR_INVALID_INPUT)`).
    #[must_use]
    pub(crate) fn blend_invalid_value(operation: &'static str, what: &'static str) -> Self {
        Self::blend(operation, BlendFailure::InvalidValue(what))
    }

    /// The blend failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Blend`].
    #[must_use]
    pub fn blend_failure(&self) -> Option<&BlendFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Blend(b) => Some(b),
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
    fn blend_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (BlendFailure::Failed("x"), SM_ERR),
            (BlendFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (BlendFailure::InvalidValue("x"), SM_ERR_INVALID_INPUT),
            (BlendFailure::Inconsistent("x"), SM_ERR_ASSERT_FAILURE),
            (
                BlendFailure::BadIntersections("x"),
                SM_ERR_BAD_INTERSECTIONS,
            ),
            (BlendFailure::NotConverged("x"), SM_ERR_NOT_CONVERGING),
            (BlendFailure::OutsideDomain("x"), SM_ERR_OUTSIDE_OF_DOMAIN),
            (
                BlendFailure::NotWithinTolerance("x"),
                SM_ERR_NOT_WITHIN_TOLERANCE,
            ),
            (
                BlendFailure::PassedOn {
                    what: "x",
                    status: SM_ERR_NOT_CONVERGING,
                },
                SM_ERR_NOT_CONVERGING,
            ),
            (
                BlendFailure::PassedOn {
                    what: "x",
                    status: SM_ERR_NULL_POINTER,
                },
                SM_ERR_NULL_POINTER,
            ),
        ];
        let edge = EntityRef::Edge(crate::topology::EdgeId::from_index(4));
        for (failure, code) in cases {
            for entity in [None, Some(edge)] {
                let typed = match entity {
                    Some(e) => SolidError::blend_at("blend::op", failure, e),
                    None => SolidError::blend("blend::op", failure),
                };
                let before = SolidError::Upstream {
                    operation: "blend::op",
                    code,
                    entity,
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
                assert_eq!(typed.blend_failure(), Some(&failure));
            }
        }
        assert_eq!(
            SolidError::blend_missing("blend::op", "the edge use's edge").to_string(),
            "blend::op: the edge use's edge is missing"
        );
        assert_eq!(
            SolidError::blend_failed("blend::op", "the law has no value").to_string(),
            "blend::op: the law has no value"
        );
        assert_eq!(
            SolidError::blend_at(
                "blend::op",
                BlendFailure::Failed("the result is not a manifold solid"),
                edge
            )
            .to_string(),
            "blend::op: the result is not a manifold solid (edge 4)"
        );
        assert_eq!(
            SolidError::blend("blend::op", BlendFailure::Inconsistent("two loops")).to_string(),
            "blend::op: a consistency check failed: two loops"
        );
        assert_eq!(
            SolidError::blend_invalid_value("blend::op", "x").kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(SolidError::sm_err("blend::op").blend_failure(), None);
    }

    /// The blend routines describe their failures: their library code builds no bare
    /// `Upstream` status, except in the files listed, each with the exact number of bare
    /// sites it still holds. A part of the typed-errors work that converts a file edits
    /// only that file's line: lowers its count, or removes the line once the file holds
    /// none.
    #[test]
    fn the_blend_routines_build_no_bare_statuses() {
        const WAITING: &[(&str, usize)] = &[
            ("blend/blend_corner.rs", 5),
            ("blend/blend_corner/ball.rs", 15),
            ("blend/blend_corner/chamfer.rs", 8),
            ("blend/blend_corner/end.rs", 9),
            ("blend/blend_corner/mixed.rs", 25),
            ("blend/blend_corner/mixed/algebra.rs", 1),
            ("blend/blend_corner/setback.rs", 27),
            ("blend/blend_corner/swept.rs", 7),
            ("blend/blend_setback.rs", 4),
            ("blend/fillet_cliff_overflow.rs", 56),
            ("blend/fillet_corner/p1.rs", 46),
            ("blend/fillet_corner/p2.rs", 17),
            ("blend/fillet_corner/p3.rs", 25),
            ("blend/fillet_corner/topology.rs", 7),
            ("blend/fillet_corner/topology/ball.rs", 2),
            ("blend/fillet_corner/topology/closed_turn.rs", 2),
            ("blend/fillet_corner/topology/concave_4x2.rs", 2),
            ("blend/fillet_corner/topology/concave_nxn.rs", 2),
            ("blend/fillet_corner/topology/end_corners.rs", 4),
            ("blend/fillet_corner/topology/end_smooth.rs", 3),
            ("blend/fillet_corner/topology/join.rs", 19),
            ("blend/fillet_corner/topology/mixed.rs", 2),
            ("blend/fillet_corner/topology/mixed_3x2.rs", 5),
            ("blend/fillet_corner/topology/nx1_end.rs", 1),
            ("blend/fillet_corner/topology/swept_turn.rs", 3),
            ("blend/fillet_custom_solver.rs", 20),
            ("blend/fillet_executive/session.rs", 43),
            ("blend/fillet_executive/session/ball_regular.rs", 8),
            ("blend/fillet_executive/session/band_cuts.rs", 13),
            ("blend/fillet_executive/session/cliff_overflow.rs", 21),
            ("blend/fillet_executive/session/concave_nxn.rs", 1),
            ("blend/fillet_executive/session/corner_marks.rs", 1),
            ("blend/fillet_executive/session/ease.rs", 3),
            ("blend/fillet_executive/session/finish.rs", 4),
            ("blend/fillet_executive/session/fit_size.rs", 20),
            ("blend/fillet_executive/session/insertion/geometry.rs", 2),
            (
                "blend/fillet_executive/session/insertion/geometry/intersector.rs",
                1,
            ),
            (
                "blend/fillet_executive/session/insertion/geometry/topology_objects.rs",
                1,
            ),
            ("blend/fillet_executive/session/insertion/live.rs", 3),
            ("blend/fillet_executive/session/keep_features.rs", 16),
            ("blend/fillet_executive/session/law_corners.rs", 9),
            ("blend/fillet_executive/session/original_trim.rs", 1),
            ("blend/fillet_executive/session/outline_rims.rs", 4),
            ("blend/fillet_executive/session/piecewise.rs", 13),
            ("blend/fillet_executive/session/surface.rs", 4),
            ("blend/fillet_executive/session/tangent_rollover.rs", 5),
            ("blend/fillet_executive/session/trace_reach.rs", 3),
            ("blend/fillet_executive/session/trim_at_creases.rs", 34),
            ("blend/fillet_executive/session/two_offsets.rs", 1),
            ("blend/fillet_geom/p1.rs", 62),
            ("blend/fillet_geom/p2.rs", 106),
            ("blend/fillet_geom/p3.rs", 27),
            ("blend/fillet_geom/p4.rs", 63),
            ("blend/fillet_geom/p5.rs", 113),
            ("blend/fillet_geom/p6.rs", 53),
            ("blend/fillet_geom/surface_plane_curve.rs", 3),
            ("blend/fillet_geom/tangent_end_rib.rs", 6),
            ("blend/fillet_overflow.rs", 31),
            ("blend/fillet_section/certificate.rs", 13),
            ("blend/fillet_section/continuation.rs", 2),
            ("blend/fillet_section/definition.rs", 6),
            ("blend/fillet_section/definition_box.rs", 12),
            ("blend/fillet_section/feet_tm.rs", 6),
            ("blend/fillet_section/recertify.rs", 14),
            ("blend/fillet_section/section.rs", 27),
            ("blend/fillet_section/series.rs", 4),
            ("blend/fillet_section/station.rs", 15),
            ("blend/fillet_solver/curve_blend_sih.rs", 14),
            ("blend/fillet_solver/p1.rs", 16),
            ("blend/fillet_solver/p1/rails.rs", 7),
            ("blend/fillet_solver/p2.rs", 8),
            ("blend/fillet_solver/p3.rs", 18),
            ("blend/fillet_solver/p4.rs", 6),
            ("blend/fillet_solver/p5.rs", 7),
            ("blend/fillet_solver/p5/end_sections_fit.rs", 13),
            ("blend/fillet_solver/p5/rails_fit.rs", 25),
            ("blend/fillet_solver/p5/recovery.rs", 6),
            ("blend/fillet_solver/surfaces.rs", 2),
            ("blend/fillet_standard_solver/p1.rs", 32),
            ("blend/fillet_standard_solver/p2.rs", 28),
            ("blend/fillet_standard_solver/surface_surface.rs", 2),
            ("blend/fillet_standard_solver/touch_circle.rs", 1),
        ];
        // the cut at each file's test module is the shared one, `bare_statuses_in`
        let found = super::super::query::tests::bare_statuses_in("blend", 100);
        let waiting: Vec<(String, usize)> = WAITING
            .iter()
            .map(|(file, n)| ((*file).to_owned(), *n))
            .collect();
        assert_eq!(found, waiting, "bare statuses per blend file");
        for (file, n) in WAITING {
            assert!(*n > 0, "{file} holds no bare status: remove its line");
        }
        assert!(
            WAITING.windows(2).all(|w| w[0].0 < w[1].0),
            "WAITING is sorted by path, one file per line"
        );
    }
}
