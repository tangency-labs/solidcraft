//! Why a surface routine fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the surface routines (`crate::surface`: the
//! surface kinds' evaluation, inversion, solvers, trimming, conversion and approximation,
//! their caches, and the volume and deformation surfaces). Each also records, for difftest
//! only, the status the C++ reports for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                               | Status difftest compares       |
//! |------------------------------------|--------------------------------|
//! | [`SurfaceFailure::Failed`]         | `SM_ERR`                       |
//! | [`SurfaceFailure::Missing`]        | `SM_ERR_NULL_POINTER`          |
//! | [`SurfaceFailure::InvalidValue`]   | `SM_ERR_INVALID_INPUT`         |
//! | [`SurfaceFailure::OutsideDomain`]  | `SM_ERR_OUTSIDE_OF_DOMAIN`     |
//! | [`SurfaceFailure::OffSurface`]     | `SM_ERR_NOT_WITHIN_TOLERANCE`  |
//! | [`SurfaceFailure::Defect`]         | `SM_ERR` (or the site's, through `with_status`) |
//! | the named causes, from [`SurfaceFailure::OperationNotTaken`] on | the status of the phrase each replaced (`comparison_status`), or the site's through `with_status` |
//!
//! [`SurfaceFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes are sites
//! not yet converted (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. A failure says in a phrase (or by its variant) what went wrong;
//! the failure's `operation` path says which routine. A failure names an entity only where the
//! site it replaces named one (a blend face whose net fails its re-certification). A check of a
//! caller's argument that reports no status stays [`crate::SolidError::InvalidInput`]; one that
//! reports a status is a named cause of class `invalid_input`, keeping that status. Only part of
//! `crate::surface` uses these yet; the files that still build bare statuses are listed in this
//! module's ratchet test.

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT,
    SM_ERR_NOT_WITHIN_TOLERANCE, SM_ERR_NULL_POINTER, SM_ERR_OUTSIDE_OF_DOMAIN, SM_ERR_WARNING,
};

/// Why a surface routine fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SurfaceFailure {
    /// The routine could not continue, as the phrase says (for example "the generator
    /// curve has no NURB form").
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// surface's cache").
    Missing(&'static str),
    /// A value the routine computes with has no defined result, as the phrase says (for
    /// example "the line direction is the zero vector"). Upstream returns
    /// `SM_ERR_INVALID_INPUT` as a status here, so its kind is
    /// [`ErrorKind::InvalidInput`].
    InvalidValue(&'static str),
    /// A parameter or domain lies outside the surface's domain, as the phrase says (for
    /// example "the point is outside the sweep of the surface of revolution").
    OutsideDomain(&'static str),
    /// A point is farther from the surface than the tolerance, as the phrase says (for
    /// example "the point is off the surface"): an inversion's answer.
    OffSurface(&'static str),
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// The solve operation asked for is not one the routine takes (a plane's fast drop
    /// takes minimize, normalize and intersect). [`ErrorKind::InvalidInput`].
    OperationNotTaken,
    /// The direction of the line given is the zero vector. [`ErrorKind::InvalidInput`].
    ZeroLineDirection,
    /// The line given is not parallel to the plane within the angle test, but the
    /// determinant of the crossing's linear solve is below the kernel's zero (a line
    /// direction or plane scale too small to solve with). Class `failed`.
    LineCrossingSingular,
    /// The line where two planes meet is too nearly singular to solve for (a pivot below
    /// the kernel's zero), so it is not built. Upstream's warning: status
    /// `SM_ERR_WARNING`, class `failed`.
    PlanesLineNearlySingular,
    /// The scale given is not uniform (or, where the routine says so, not positive): an
    /// analytic surface takes only a uniform scale. [`ErrorKind::InvalidInput`].
    ScaleNotUniform,
    /// The trim domain given is not inside the surface's natural domain.
    /// [`ErrorKind::InvalidInput`].
    TrimDomainOutside,
    /// The NURB tested as a plane has parallel u and v directions at its origin, so it has
    /// no plane frame (the unitize's error is the cause). Class `failed`.
    NurbAxesParallel,
    /// The surface's NURB form could not be made (the make's failure is the cause, where it
    /// gave one). Class `failed`; whether it is a defect or a refusal is its cause's
    /// ([`crate::SolidError::is_defect`] follows the chain).
    NurbNotMade,
    /// The evaluation of the surface's NURB form failed (its error is the cause). Class
    /// `failed`; whether it is a defect or a refusal is its cause's.
    NurbEvaluationFailed,
    /// The radius given is not above the kernel's zero. [`ErrorKind::InvalidInput`].
    RadiusNotPositive,
    /// The swept surface given has no generator curve (an empty surface whose data was
    /// never read or set). [`ErrorKind::InvalidInput`].
    SurfaceWithoutGenerator,
    /// The domain given reaches past the cone's apex, where its radius would be negative.
    /// [`ErrorKind::InvalidInput`].
    DomainPastApex,
    /// A derivative of an order above `max` was asked of a form that evaluates up to `max`.
    /// [`ErrorKind::InvalidInput`].
    DerivativeOrderTooHigh {
        /// The highest order the form evaluates.
        max: usize,
    },
    /// The iso curve asked for has zero length (its parameter range on the surface is
    /// degenerate). [`ErrorKind::InvalidInput`].
    DegenerateIsoCurve,
    /// The surface of revolution has no polar converter circle (its NURB form was never
    /// made or read, or both sweep iso curves were degenerate when it was, which the make
    /// does not report). Class `failed`.
    NoPolarConverter,
}

impl SurfaceFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_)
            | Self::Defect { .. }
            | Self::OperationNotTaken
            | Self::LineCrossingSingular
            | Self::ScaleNotUniform
            | Self::TrimDomainOutside
            | Self::NurbAxesParallel
            | Self::NurbNotMade
            | Self::NurbEvaluationFailed
            | Self::RadiusNotPositive
            | Self::DerivativeOrderTooHigh { .. } => SM_ERR,
            Self::Missing(_) | Self::SurfaceWithoutGenerator | Self::NoPolarConverter => {
                SM_ERR_NULL_POINTER
            }
            Self::InvalidValue(_)
            | Self::ZeroLineDirection
            | Self::DomainPastApex
            | Self::DegenerateIsoCurve => SM_ERR_INVALID_INPUT,
            Self::OutsideDomain(_) => SM_ERR_OUTSIDE_OF_DOMAIN,
            Self::OffSurface(_) => SM_ERR_NOT_WITHIN_TOLERANCE,
            Self::PlanesLineNearlySingular => SM_ERR_WARNING,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::InvalidInput`] for a check of the
    /// caller's argument or surface, [`ErrorKind::Unsupported`] for a kind the routine does
    /// not take, otherwise the kind of the status it replaced ([`ErrorKind::Failed`],
    /// [`ErrorKind::Missing`], [`ErrorKind::InvalidInput`], [`ErrorKind::OutsideDomain`],
    /// [`ErrorKind::NotWithinTolerance`]). [`SurfaceFailure::Defect`] is
    /// [`ErrorKind::StageFailed`], a defect.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::OperationNotTaken
            | Self::ZeroLineDirection
            | Self::ScaleNotUniform
            | Self::TrimDomainOutside
            | Self::RadiusNotPositive
            | Self::SurfaceWithoutGenerator
            | Self::DomainPastApex
            | Self::DerivativeOrderTooHigh { .. }
            | Self::DegenerateIsoCurve => ErrorKind::InvalidInput,
            Self::NurbNotMade | Self::NurbEvaluationFailed | Self::NoPolarConverter => {
                ErrorKind::Failed
            }
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for SurfaceFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what)
            | Self::InvalidValue(what)
            | Self::OutsideDomain(what)
            | Self::OffSurface(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::OperationNotTaken => {
                f.write_str("the solve operation is not one the routine takes")
            }
            Self::ZeroLineDirection => f.write_str("the line direction is the zero vector"),
            Self::LineCrossingSingular => {
                f.write_str("the line's crossing with the plane is singular to solve for")
            }
            Self::PlanesLineNearlySingular => {
                f.write_str("the planes' line is nearly singular to solve for")
            }
            Self::ScaleNotUniform => f.write_str("an analytic surface takes only a uniform scale"),
            Self::TrimDomainOutside => {
                f.write_str("the trim domain is not inside the surface's natural domain")
            }
            Self::NurbAxesParallel => {
                f.write_str("the NURB's u and v directions are parallel at its origin")
            }
            Self::NurbNotMade => f.write_str("the surface's NURB form could not be made"),
            Self::NurbEvaluationFailed => {
                f.write_str("the evaluation of the surface's NURB form failed")
            }
            Self::RadiusNotPositive => f.write_str("the radius is not positive"),
            Self::SurfaceWithoutGenerator => f.write_str("the surface has no generator curve"),
            Self::DomainPastApex => f.write_str("the domain reaches past the apex"),
            Self::DerivativeOrderTooHigh { max } => {
                write!(f, "a derivative order above {max} was asked")
            }
            Self::DegenerateIsoCurve => f.write_str("the iso curve has zero length"),
            Self::NoPolarConverter => {
                f.write_str("the surface of revolution has no polar converter circle")
            }
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a surface routine that fails with `failure`.
    #[must_use]
    pub(crate) fn surface(operation: &'static str, failure: SurfaceFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a surface routine that fails with `failure` about
    /// `entity`.
    #[must_use]
    pub(crate) fn surface_at(
        operation: &'static str,
        failure: SurfaceFailure,
        entity: EntityRef,
    ) -> Self {
        Failure::new(operation, failure).at(entity).into_error()
    }

    /// [`crate::SolidError::Failed`] for a surface routine that could not continue, as the
    /// phrase says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn surface_failed(operation: &'static str, what: &'static str) -> Self {
        Self::surface(operation, SurfaceFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a surface routine needs that is not
    /// there, as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)`
    /// on a `NULL` pointer).
    #[must_use]
    pub(crate) fn surface_missing(operation: &'static str, what: &'static str) -> Self {
        Self::surface(operation, SurfaceFailure::Missing(what))
    }

    /// [`SurfaceFailure::NurbNotMade`] with status `status`, for a routine whose NURB form
    /// is still missing after `made`, the result of the call that was to make it: its
    /// error, when it failed, is the cause.
    #[must_use]
    pub(crate) fn surface_nurb_not_made(
        operation: &'static str,
        status: i32,
        made: Result<(), Self>,
    ) -> Self {
        let failure = Failure::new(operation, SurfaceFailure::NurbNotMade).with_status(status);
        match made {
            Err(e) => failure.caused_by(e),
            Ok(()) => failure,
        }
        .into_error()
    }

    /// The surface failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Surface`].
    #[must_use]
    pub fn surface_failure(&self) -> Option<&SurfaceFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Surface(s) => Some(s),
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
    fn surface_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (SurfaceFailure::Failed("x"), SM_ERR),
            (SurfaceFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (SurfaceFailure::InvalidValue("x"), SM_ERR_INVALID_INPUT),
            (SurfaceFailure::OutsideDomain("x"), SM_ERR_OUTSIDE_OF_DOMAIN),
            (SurfaceFailure::OffSurface("x"), SM_ERR_NOT_WITHIN_TOLERANCE),
        ];
        for (failure, code) in cases {
            let typed = SolidError::surface("surface::op", failure);
            let before = SolidError::Upstream {
                operation: "surface::op",
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
            assert_eq!(typed.surface_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::surface_missing("surface::op", "the surface's cache").to_string(),
            "surface::op: the surface's cache is missing"
        );
        assert_eq!(
            SolidError::surface_failed("surface::op", "the generator has no NURB form").to_string(),
            "surface::op: the generator has no NURB form"
        );
        assert_eq!(
            SolidError::surface(
                "surface::op",
                SurfaceFailure::OffSurface("the point is off")
            )
            .to_string(),
            "surface::op: the point is off"
        );
        assert_eq!(SolidError::sm_err("surface::op").surface_failure(), None);
    }

    /// A sample of the second set of converted files (`extend` and its sides, `side`,
    /// `implicit`, the volumes, `srf_in_volume`, `procedural_blend` and its offset,
    /// `trim_srf_cache`): each keeps the operation, status, kind and entity of its
    /// `Upstream`.
    #[test]
    fn the_extend_volume_and_blend_samples_keep_their_upstream_answers() {
        let samples = [
            (
                "surface::extend::analytic::analytic_extended_sides",
                SurfaceFailure::Failed("the joint is not an interior knot"),
                SM_ERR,
            ),
            (
                "surface::extend::ball",
                SurfaceFailure::Failed("the B-spline surface has no NURB"),
                SM_ERR,
            ),
            (
                "surface::volume::p1::Volume::copy",
                SurfaceFailure::Failed("a plain volume cannot be copied"),
                SM_ERR,
            ),
            (
                "surface::srf_in_volume",
                SurfaceFailure::Failed("the volume's inversion failed"),
                SM_ERR,
            ),
            (
                "surface::procedural_blend::ProceduralBlend::read_from_db",
                SurfaceFailure::Missing("the input stream"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "surface::trim_srf_cache::TrimSrfCache::build_tree_with_face",
                SurfaceFailure::InvalidValue("the face's domain and the surface's are disjoint"),
                SM_ERR_INVALID_INPUT,
            ),
            (
                "surface::side",
                SurfaceFailure::Failed("the iso curve has no NURB"),
                SM_ERR,
            ),
        ];
        for (operation, failure, code) in samples {
            let typed = SolidError::surface(operation, failure);
            let before = SolidError::Upstream {
                operation,
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{typed:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{typed:?}");
            assert_eq!(typed.kind(), before.kind(), "{typed:?}");
            assert_eq!(typed.operation(), before.operation(), "{typed:?}");
            assert_eq!(typed.entity(), None, "{typed:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
        }
        // the re-certification refusal keeps the face the bare status named
        let face = EntityRef::Face(crate::topology::FaceId::from_index(3));
        let typed = SolidError::surface_at(
            "surface::procedural_blend::recertify_faces",
            SurfaceFailure::Failed(
                "the face's net is not proved within its bound of its definition",
            ),
            face,
        );
        let before = SolidError::Upstream {
            operation: "surface::procedural_blend::recertify_faces",
            code: SM_ERR,
            entity: Some(face),
        };
        assert_eq!(typed.sm_status(), before.sm_status());
        assert_eq!(typed.upstream_status(), Some(SM_ERR));
        assert_eq!(typed.kind(), before.kind());
        assert_eq!(typed.operation(), before.operation());
        assert_eq!(typed.entity(), Some(face));
        assert_eq!(typed.is_status_answer(), before.is_status_answer());
    }

    /// The surface routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the files listed (with the bare sites each may still hold)
    /// whose conversion waits on other open work or a later part of the slice.
    #[test]
    fn the_surface_routines_build_no_bare_statuses() {
        super::super::bare_scan::assert_waiting("surface", 51, &[]);
    }

    /// The named causes keep the status of the phrase each replaced, so `upstream_status`
    /// (which most goldens units read) is unchanged; their class says whose the failure is,
    /// and a cause chained under one decides whether it is a defect.
    #[test]
    fn the_named_causes_keep_their_status_and_say_their_class() {
        let op = "surface::op";
        let cases = [
            (
                SurfaceFailure::OperationNotTaken,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::ZeroLineDirection,
                SM_ERR_INVALID_INPUT,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::LineCrossingSingular,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::PlanesLineNearlySingular,
                SM_ERR_WARNING,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::ScaleNotUniform,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::TrimDomainOutside,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (SurfaceFailure::NurbAxesParallel, SM_ERR, ErrorKind::Failed),
            (SurfaceFailure::NurbNotMade, SM_ERR, ErrorKind::Failed),
            (
                SurfaceFailure::NurbEvaluationFailed,
                SM_ERR,
                ErrorKind::Failed,
            ),
            (
                SurfaceFailure::RadiusNotPositive,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::SurfaceWithoutGenerator,
                SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::DomainPastApex,
                SM_ERR_INVALID_INPUT,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::DerivativeOrderTooHigh { max: 3 },
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::DegenerateIsoCurve,
                SM_ERR_INVALID_INPUT,
                ErrorKind::InvalidInput,
            ),
            (
                SurfaceFailure::NoPolarConverter,
                SM_ERR_NULL_POINTER,
                ErrorKind::Failed,
            ),
        ];
        for (failure, status, kind) in cases {
            let e = SolidError::surface(op, failure);
            assert_eq!(e.sm_status(), Some(status), "{failure:?}");
            assert_eq!(
                super::super::upstream_status(&e),
                Some(status),
                "{failure:?}"
            );
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.operation(), op, "{failure:?}");
            assert_eq!(e.surface_failure(), Some(&failure));
            assert!(!e.is_defect() && !e.is_refusal(), "{failure:?}");
        }
        // a NURB not made keeps the site's status, and its make's defect decides the class
        let made = Err(SolidError::surface(op, SurfaceFailure::Defect { why: "x" }));
        let e = SolidError::surface_nurb_not_made(op, SM_ERR_NULL_POINTER, made);
        assert_eq!(e.sm_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(e.surface_failure(), Some(&SurfaceFailure::NurbNotMade));
        assert!(e.is_defect());
        let e = SolidError::surface_nurb_not_made(op, SM_ERR, Ok(()));
        assert_eq!(e.sm_status(), Some(SM_ERR));
        assert!(e.cause().is_none() && !e.is_defect());
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the B-spline form the surface just converted to has no control net, which conversion always gives";
        let defect = SurfaceFailure::Defect { why };
        let op = "surface::op";
        super::super::tests::assert_defect(
            &SolidError::surface(op, defect),
            &SolidError::surface(op, SurfaceFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::surface(op, SurfaceFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.surface_failure(), Some(&defect));
    }

    /// The sites that build a surface phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_surface_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("SurfaceFailure", "Failed"),
                Builder::Variant("SurfaceFailure", "Missing"),
                Builder::Variant("SurfaceFailure", "InvalidValue"),
                Builder::Variant("SurfaceFailure", "OutsideDomain"),
                Builder::Variant("SurfaceFailure", "OffSurface"),
                Builder::Method("surface_failed", Some(1)),
                // the WR-57 replay seam's own constructor of a taped answer (feature
                // `wr57-replay`), which only the goldens call: not a kernel site
                Builder::Shared(
                    "solidcore/src/surface/surface/replay",
                    "surface_failed",
                    Some(1),
                ),
                Builder::Method("surface_missing", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/surface/b_spline_volume.rs", 4),
            ("solidcore/src/surface/b_spline_volume_n_lib.rs", 1),
            ("solidcore/src/surface/ball_patch.rs", 8),
            ("solidcore/src/surface/bend_volume.rs", 2),
            ("solidcore/src/surface/cone/p2.rs", 14),
            ("solidcore/src/surface/cone/p3.rs", 3),
            ("solidcore/src/surface/extend.rs", 2),
            ("solidcore/src/surface/extend/analytic.rs", 25),
            ("solidcore/src/surface/extend/ball.rs", 16),
            ("solidcore/src/surface/extend/blend.rs", 6),
            ("solidcore/src/surface/extend/sides.rs", 2),
            ("solidcore/src/surface/from_parameters.rs", 1),
            ("solidcore/src/surface/implicit.rs", 9),
            ("solidcore/src/surface/offset_surface.rs", 27),
            ("solidcore/src/surface/procedural_blend.rs", 9),
            ("solidcore/src/surface/procedural_blend/convert.rs", 13),
            ("solidcore/src/surface/procedural_blend/offset.rs", 5),
            ("solidcore/src/surface/side.rs", 7),
            ("solidcore/src/surface/sphere.rs", 16),
            ("solidcore/src/surface/srf_in_volume.rs", 4),
            ("solidcore/src/surface/surf_of_extrusion/p1.rs", 16),
            ("solidcore/src/surface/surf_of_extrusion/p2.rs", 13),
            ("solidcore/src/surface/surf_of_revolution/p1.rs", 10),
            ("solidcore/src/surface/surf_of_revolution/p2.rs", 6),
            ("solidcore/src/surface/surf_of_revolution/p3.rs", 21),
            ("solidcore/src/surface/surface/approximate.rs", 10),
            ("solidcore/src/surface/surface/p1.rs", 8),
            ("solidcore/src/surface/surface/p10.rs", 7),
            ("solidcore/src/surface/surface/p11.rs", 21),
            ("solidcore/src/surface/surface/p2.rs", 21),
            ("solidcore/src/surface/surface/p3.rs", 3),
            ("solidcore/src/surface/surface/p4.rs", 19),
            ("solidcore/src/surface/surface/p5.rs", 2),
            ("solidcore/src/surface/surface/p6.rs", 22),
            ("solidcore/src/surface/surface/p7.rs", 21),
            ("solidcore/src/surface/surface/p8.rs", 33),
            ("solidcore/src/surface/surface/p9.rs", 33),
            ("solidcore/src/surface/surface_cache/face_context.rs", 1),
            ("solidcore/src/surface/surface_cache/p1.rs", 17),
            ("solidcore/src/surface/surface_cache/p2.rs", 5),
            ("solidcore/src/surface/surface_cache/p3.rs", 1),
            ("solidcore/src/surface/surface_point_solve/p1.rs", 5),
            ("solidcore/src/surface/surface_point_solve/p2.rs", 8),
            ("solidcore/src/surface/torus.rs", 15),
            ("solidcore/src/surface/trim_srf_cache.rs", 4),
            ("solidcore/src/surface/unbend_volume.rs", 2),
            ("solidcore/src/surface/volume/p1.rs", 8),
            ("solidcore/src/surface/volume/p2.rs", 6),
        ];
        assert_pinned("surface phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
