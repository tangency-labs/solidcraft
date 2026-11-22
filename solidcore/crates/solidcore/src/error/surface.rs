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
//! | the named causes of the volume surfaces and the implicit checks | `SM_ERR`, except [`SurfaceFailure::NoNurb`]: `SM_ERR_NULL_POINTER` (a site that reported `SM_ERR` keeps it through `with_status`) |
//!
//! [`SurfaceFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes are sites
//! not yet converted (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! **Classes of the named causes.** A check of the caller's argument or object that is
//! malformed is [`ErrorKind::InvalidInput`]; a box or point outside the domain it must lie
//! in is [`ErrorKind::OutsideDomain`]; a kind or an ask the routine has no code for is
//! [`ErrorKind::Unsupported`]; geometry degenerate where the routine needs it not to be is
//! [`ErrorKind::Degenerate`]; a routine whose callee failed keeps that error as its cause
//! ([`SurfaceFailure::CallFailed`]).
//!
//! Deviations: not upstream. A failure says in a phrase what went wrong; the failure's
//! `operation` path says which routine. A failure names an entity only where the site it
//! replaces named one (a blend face whose net fails its re-certification). The failures that are checks of a caller's arguments stay
//! [`crate::SolidError::InvalidInput`]. Only part of `crate::surface` uses these yet; the
//! files that still build bare statuses are listed in this module's ratchet test.

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT,
    SM_ERR_NOT_WITHIN_TOLERANCE, SM_ERR_NULL_POINTER, SM_ERR_OUTSIDE_OF_DOMAIN,
};

/// Why a surface routine fails.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// A knot vector of the caller's volume has fewer than two distinct knots.
    /// [`ErrorKind::InvalidInput`].
    TooFewKnots {
        /// The distinct knots.
        knots: usize,
    },
    /// The parameter box given to a bend volume's trim has a NaN bound, a degenerate
    /// infinite v range or a negative volume, or the bend's own numbers are NaN.
    /// [`ErrorKind::InvalidInput`].
    BendBoxMalformed,
    /// The surface in volume does not own its base surface, so it cannot edit it.
    /// [`ErrorKind::InvalidInput`].
    BaseSurfaceNotOwned,
    /// A derivative of an order above `max` was asked of a form that evaluates up to `max`.
    /// [`ErrorKind::InvalidInput`].
    DerivativeOrderTooHigh {
        /// The highest order the form evaluates.
        max: usize,
    },
    /// The volume type read from a file is none the reader knows.
    /// [`ErrorKind::InvalidInput`].
    UnknownVolumeType {
        /// The type read.
        volume_type: i64,
    },
    /// The surface given has no NURB form, which the routine reads.
    /// [`ErrorKind::InvalidInput`].
    NoNurb,
    /// The caller's B-spline curve has no NURB data. [`ErrorKind::InvalidInput`].
    CurveWithoutNurb,
    /// The caller's B-spline curve has no control points. [`ErrorKind::InvalidInput`].
    CurveWithoutControlPoints,
    /// The parameter box reaches no further out than the bend's singularity: its largest
    /// u, `u_max`, is below the bend's legal minimum `limit`.
    /// [`ErrorKind::OutsideDomain`].
    BoxOutsideBendHalfSpace {
        /// The box's largest u.
        u_max: f64,
        /// The bend's singularity tolerance.
        limit: f64,
    },
    /// A bend of zero period takes only v = 0, and the box's v range misses it by more
    /// than the kernel's zero. [`ErrorKind::OutsideDomain`].
    BoxOutsideZeroPeriod {
        /// The box's smallest v.
        v_min: f64,
        /// The box's largest v.
        v_max: f64,
    },
    /// The point is outside the volume's parameter domain.
    /// [`ErrorKind::OutsideDomain`].
    PointOutsideVolumeDomain {
        /// The point.
        point: [f64; 3],
    },
    /// The point is outside the parameter domain the caller gave.
    /// [`ErrorKind::OutsideDomain`].
    PointOutsideGivenDomain {
        /// The point.
        point: [f64; 3],
    },
    /// The parameter box is outside the volume's natural parameter domain.
    /// [`ErrorKind::OutsideDomain`].
    BoxOutsideParamDomain,
    /// The in-space pseudo box maps outside the volume's natural parameter domain.
    /// [`ErrorKind::OutsideDomain`].
    PseudoBoxOutsideDomain,
    /// The surface in volume evaluates its u and v derivatives up to orders `u` and `v`
    /// only together as upstream supports them (equal orders with the upper half only or
    /// at most 1, or one of them 0). [`ErrorKind::Unsupported`].
    DerivativeCombinationUnsupported {
        /// The highest u derivative asked.
        u: usize,
        /// The highest v derivative asked.
        v: usize,
    },
    /// A plain volume (upstream's base `SmVolume`) has no code for this: only the derived
    /// volumes copy, evaluate and invert. [`ErrorKind::Unsupported`].
    PlainVolume,
    /// B-spline volumes are not supported: upstream's build has no volume NURBS library
    /// (`USE_VOLUMES`), so they do not evaluate, transform, read or write.
    /// [`ErrorKind::Unsupported`].
    BSplineVolumeUnsupported,
    /// The point lies within the bend's singularity (its distance from the bend axis is
    /// below the bend's legal minimum), where the bend has no inverse.
    /// [`ErrorKind::Degenerate`].
    PointInBendSingularity,
    /// A tangent of the volume has zero length at the point: its squared length is below
    /// the kernel's zero squared. [`ErrorKind::Degenerate`].
    ZeroVolumeTangent {
        /// The shortest tangent's squared length.
        length_squared: f64,
    },
    /// The volume's inversion of the point found no parameter point.
    /// [`ErrorKind::Failed`].
    NoVolumeInverse,
    /// A routine this one calls failed; the failure keeps that routine's error as its
    /// cause. [`ErrorKind::Failed`].
    CallFailed,
}

impl SurfaceFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Missing(_) | Self::NoNurb => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_) => SM_ERR_INVALID_INPUT,
            Self::OutsideDomain(_) => SM_ERR_OUTSIDE_OF_DOMAIN,
            Self::OffSurface(_) => SM_ERR_NOT_WITHIN_TOLERANCE,
            _ => SM_ERR,
        }
    }

    /// The failure's stable [`ErrorKind`]: the kind of the status it replaced
    /// ([`ErrorKind::Failed`], [`ErrorKind::Missing`], [`ErrorKind::InvalidInput`],
    /// [`ErrorKind::OutsideDomain`], [`ErrorKind::NotWithinTolerance`]).
    /// [`SurfaceFailure::Defect`] is [`ErrorKind::StageFailed`], a defect. Each named
    /// cause has the class its doc names.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::TooFewKnots { .. }
            | Self::BendBoxMalformed
            | Self::BaseSurfaceNotOwned
            | Self::DerivativeOrderTooHigh { .. }
            | Self::UnknownVolumeType { .. }
            | Self::NoNurb
            | Self::CurveWithoutNurb
            | Self::CurveWithoutControlPoints => ErrorKind::InvalidInput,
            Self::BoxOutsideBendHalfSpace { .. }
            | Self::BoxOutsideZeroPeriod { .. }
            | Self::PointOutsideVolumeDomain { .. }
            | Self::PointOutsideGivenDomain { .. }
            | Self::BoxOutsideParamDomain
            | Self::PseudoBoxOutsideDomain => ErrorKind::OutsideDomain,
            Self::DerivativeCombinationUnsupported { .. }
            | Self::PlainVolume
            | Self::BSplineVolumeUnsupported => ErrorKind::Unsupported,
            Self::PointInBendSingularity | Self::ZeroVolumeTangent { .. } => ErrorKind::Degenerate,
            Self::NoVolumeInverse | Self::CallFailed => ErrorKind::Failed,
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
            Self::TooFewKnots { knots } => write!(
                f,
                "the knot vector has {knots} distinct knots, fewer than two"
            ),
            Self::BendBoxMalformed => f.write_str(
                "the box has a NaN bound, a degenerate infinite v range or a negative volume, or the bend's numbers are NaN",
            ),
            Self::BaseSurfaceNotOwned => {
                f.write_str("the surface in volume does not own its base surface")
            }
            Self::DerivativeOrderTooHigh { max } => {
                write!(f, "a derivative order above {max} was asked")
            }
            Self::UnknownVolumeType { volume_type } => {
                write!(f, "the volume type {volume_type} is unknown")
            }
            Self::NoNurb => f.write_str("the surface has no NURB form"),
            Self::CurveWithoutNurb => f.write_str("the B-spline curve has no NURB"),
            Self::CurveWithoutControlPoints => f.write_str("the curve has no control points"),
            Self::BoxOutsideBendHalfSpace { u_max, limit } => write!(
                f,
                "the box's largest u, {u_max:e}, is below the bend's singularity tolerance {limit:e}"
            ),
            Self::BoxOutsideZeroPeriod { v_min, v_max } => write!(
                f,
                "the box's v range [{v_min:e}, {v_max:e}] misses the bend's zero period"
            ),
            Self::PointOutsideVolumeDomain { point: [x, y, z] } => write!(
                f,
                "the point ({x}, {y}, {z}) is outside the volume's parameter domain"
            ),
            Self::PointOutsideGivenDomain { point: [x, y, z] } => write!(
                f,
                "the point ({x}, {y}, {z}) is outside the given parameter domain"
            ),
            Self::BoxOutsideParamDomain => {
                f.write_str("the parameter box is outside the parameter domain")
            }
            Self::PseudoBoxOutsideDomain => {
                f.write_str("the pseudo box maps outside the in-space domain")
            }
            Self::DerivativeCombinationUnsupported { u, v } => write!(
                f,
                "derivatives up to order {u} in u and {v} in v are not supported together"
            ),
            Self::PlainVolume => f.write_str(
                "a plain volume has no code for this; only the derived volumes do",
            ),
            Self::BSplineVolumeUnsupported => f.write_str(
                "B-spline volumes are not supported (upstream's build has no volume NURBS library)",
            ),
            Self::PointInBendSingularity => {
                f.write_str("the point lies within the bend's singularity")
            }
            Self::ZeroVolumeTangent { length_squared } => write!(
                f,
                "a volume tangent has zero length (its squared length is {length_squared:e})"
            ),
            Self::NoVolumeInverse => f.write_str("the volume's inversion found no point"),
            Self::CallFailed => f.write_str("a routine it calls failed"),
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
            ("solidcore/src/surface/ball_patch.rs", 8),
            ("solidcore/src/surface/cone/p1.rs", 12),
            ("solidcore/src/surface/cone/p2.rs", 14),
            ("solidcore/src/surface/cone/p3.rs", 3),
            ("solidcore/src/surface/extend.rs", 2),
            ("solidcore/src/surface/extend/analytic.rs", 25),
            ("solidcore/src/surface/extend/ball.rs", 16),
            ("solidcore/src/surface/extend/blend.rs", 6),
            ("solidcore/src/surface/extend/sides.rs", 2),
            ("solidcore/src/surface/offset_surface.rs", 27),
            ("solidcore/src/surface/plane/p1.rs", 3),
            ("solidcore/src/surface/plane/p2.rs", 9),
            ("solidcore/src/surface/procedural_blend.rs", 9),
            ("solidcore/src/surface/procedural_blend/convert.rs", 13),
            ("solidcore/src/surface/procedural_blend/offset.rs", 5),
            ("solidcore/src/surface/side.rs", 7),
            ("solidcore/src/surface/sphere.rs", 16),
            ("solidcore/src/surface/surf_of_extrusion/p1.rs", 16),
            ("solidcore/src/surface/surf_of_extrusion/p2.rs", 13),
            ("solidcore/src/surface/surf_of_revolution/p1.rs", 10),
            ("solidcore/src/surface/surf_of_revolution/p2.rs", 6),
            ("solidcore/src/surface/surf_of_revolution/p3.rs", 21),
            ("solidcore/src/surface/surface/approximate.rs", 10),
            ("solidcore/src/surface/surface/p1.rs", 8),
            ("solidcore/src/surface/surface/p10.rs", 7),
            ("solidcore/src/surface/surface/p11.rs", 20),
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
        ];
        assert_pinned("surface phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
