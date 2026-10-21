//! High-level surface and skin creation (`SM_API`): extrusions, sweeps, ruled and
//! skinned B-spline surfaces, surfaces of revolution, and lofted (skinned) solids
//! from profile curves or face loops.
//!
//! Upstream: `source/SM_API/src/SmApiSurfaces.cpp`, `source/SM_API/src/SmApiPrimitives.cpp`,
//! `source/SM_API/inc/SmApiSurfaces.h`, `source/SM_API/inc/SmApiPrimitives.h`.
//!
//! | Rust                                         | Upstream                                       |
//! |----------------------------------------------|------------------------------------------------|
//! | `validate_skin_primitive_inputs` (private)   | `SmApiValidateSkinPrimitiveInputs` (static)    |
//! | `copy_skin_primitive_curves` (private)       | `SmApiCopySkinPrimitiveCurves` (static)        |
//! | [`create_skin_primitive`], [`SkinPrimitive`] | `SmApiCreateSkinPrimitive`                     |
//! | [`create_skin_from_faces`], [`SkinFromFaces`] | `SmApiCreateSkinFromFaces`                    |
//! | [`create_extrude`]                           | `SmApiCreateExtrude`                           |
//! | [`create_sweep`]                             | `SmApiCreateSweep`                             |
//! | [`create_ruled_surface`]                     | `SmApiCreateRuledSurface`                      |
//! | [`create_surface_revolution`]                | `SmApiCreateSurfaceRevolution`                 |
//! | [`create_skin`]                              | `SmApiCreateSkin`                              |
//!
//! Deviations:
//! - **Out-params.** The created surface, B-rep or faces are the return value; on
//!   failure there is none (upstream clears `rpResult` and the optional face arrays).
//! - **Result types.** The extrude, sweep, ruled and skin entry points return the
//!   [`BSplineSurface`] upstream creates (declared `SmSurface*&`); the surface of
//!   revolution is the first [`Surface`] of the swept B-rep, as upstream.
//! - **Context.** `SmApiGetOrCreateContext()` is the caller's [`Kernel`], passed only
//!   where a callee takes it (curve copies).
//! - **Pointers.** A `NULL` curve that upstream tests for is `None`
//!   ([`create_surface_revolution`], the profile curves of [`create_skin_primitive`]);
//!   the other entry points dereference their curves unchecked upstream and take
//!   references.
//! - **Statuses.** `SM_ERR_INVALID_INPUT` is [`SolidError::InvalidInput`]; `SM_ERR` is
//!   [`SolidError::Upstream`] with [`crate::error::SM_ERR`]. Errors of the called
//!   constructors pass through unchanged.
//! - [`create_extrude`] makes a 2D curve's NURB 3D in place, as upstream (the curve
//!   is the caller's, taken `&mut`).
//! - The B-rep entry points ([`create_skin_primitive`], [`create_skin_from_faces`],
//!   [`create_surface_revolution`]) validate and copy their inputs as upstream, build
//!   the new B-rep ([`crate::topology::brep::p3::new_brep`] in the caller's kernel,
//!   upstream's API context; it stops at `SmRegion::SetIsVoid`, `topology.shell`) and
//!   return [`SolidError::Unported`]: `SmPrimitiveCreation` (units
//!   `primitive.primitive_creation_p1..p3`) is not ported yet.
//! - [`create_surface_revolution`] copies the curve with the infallible
//!   [`BSplineCurve::copy`]; upstream `SmCurve::Copy` can fail (only on allocation
//!   failure) and then returns `SM_ERR`, a path the port does not have.

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::tol;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::nurbs::b_spline_surface_n_lib::p2::SkinnedSurfaceOptions;
use crate::surface::surface::Surface;
use crate::topology::brep::p3 as brep_p3;
use crate::topology::{Brep, FaceId};

const OP_SKIN_PRIMITIVE: &str = "sweep::api_surfaces::create_skin_primitive";
const OP_REVOLUTION: &str = "sweep::api_surfaces::create_surface_revolution";
const OP_SKIN: &str = "sweep::api_surfaces::create_skin";

/// The sweep approximation tolerance `SmApiCreateSweep` passes to
/// `CreateSweepSurface` (`dTol`).
pub const SWEEP_APPROX_TOL: f64 = 1.0e-5;

/// A lofted solid and its faces ([`create_skin_primitive`]'s outputs).
#[derive(Debug, Clone, Default)]
pub struct SkinPrimitive {
    /// `rpResult`: the lofted solid.
    pub brep: Brep,
    /// `pOptStartFaces`: the start cap faces.
    pub start_faces: Vec<FaceId>,
    /// `pOptSideFaces`: the side loft faces.
    pub side_faces: Vec<FaceId>,
    /// `pOptEndFaces`: the end cap faces.
    pub end_faces: Vec<FaceId>,
}

/// A solid lofted between face loops ([`create_skin_from_faces`]'s outputs).
#[derive(Debug, Clone, Default)]
pub struct SkinFromFaces {
    /// `rpResult`: the B-rep holding the lofted faces.
    pub brep: Brep,
    /// `rNewFaces`: the lofted faces.
    pub new_faces: Vec<FaceId>,
}

/// Validates the public skin-profile structure before any input curve is copied
/// (`SmApiValidateSkinPrimitiveInputs`): at least two profiles, a degree of 1 to 3,
/// every profile non-empty, the profile counts adding up to the curve count and no
/// `NULL` curve.
fn validate_skin_primitive_inputs(
    curves_in_each_profile: &[usize],
    curves_3d: &[Option<&Curve>],
    degree: usize,
) -> Result<()> {
    if curves_in_each_profile.len() < 2 || !(1..=3).contains(&degree) {
        return Err(SolidError::invalid_input(
            OP_SKIN_PRIMITIVE,
            "fewer than two profiles or degree outside 1..=3",
        ));
    }

    let mut curve_count: usize = 0;
    for &profile_curve_count in curves_in_each_profile {
        // curve_count never exceeds the curve count (checked below on every step)
        let remaining = curves_3d.len().saturating_sub(curve_count);
        if profile_curve_count == 0 || profile_curve_count > remaining {
            return Err(SolidError::invalid_input(
                OP_SKIN_PRIMITIVE,
                "empty profile or more profile curves than curves",
            ));
        }
        curve_count += profile_curve_count;
    }

    if curve_count != curves_3d.len() {
        return Err(SolidError::invalid_input(
            OP_SKIN_PRIMITIVE,
            "profile curve counts do not add up to the curve count",
        ));
    }

    if curves_3d.iter().any(Option::is_none) {
        return Err(SolidError::invalid_input(
            OP_SKIN_PRIMITIVE,
            "NULL profile curve",
        ));
    }

    Ok(())
}

/// Deep-copies the caller's skin profiles for the consuming
/// `SmPrimitiveCreation::CreateSkinPrimitive` (`SmApiCopySkinPrimitiveCurves`).
///
/// # Errors
///
/// The first failing `Copy`; `SM_ERR_NULL_POINTER` for a `NULL` curve (rejected by
/// the validation before upstream gets here).
fn copy_skin_primitive_curves(k: &Kernel, input_curves: &[Option<&Curve>]) -> Result<Vec<Curve>> {
    let mut curve_copies = Vec::new();
    for curve in input_curves {
        let curve = curve.ok_or_else(|| SolidError::null_pointer(OP_SKIN_PRIMITIVE))?;
        curve_copies.push(curve.copy(k)?);
    }
    Ok(curve_copies)
}

/// A lofted solid (skin) through profiles of `curves_3d`, `curves_in_each_profile`
/// curves each, of degree `degree` in the loft direction (1 linear, 2 quadratic,
/// 3 cubic), capped at both ends with `cap_ends` (`SmApiCreateSkinPrimitive`). The
/// caller's curves are copied, never changed.
///
/// # Errors
///
/// `InvalidInput` for fewer than two profiles, a degree outside `1..=3`, an empty
/// profile, profile counts that do not add up to the curve count or a `None` curve;
/// the error of a failing curve copy; [`SolidError::Unported`] otherwise
/// (`SmBrep::SmBrep`, until the B-rep constructor is ported).
pub fn create_skin_primitive(
    k: &mut Kernel,
    curves_in_each_profile: &[usize],
    curves_3d: &[Option<&Curve>],
    degree: usize,
    cap_ends: bool,
) -> Result<SkinPrimitive> {
    validate_skin_primitive_inputs(curves_in_each_profile, curves_3d, degree)?;

    let curve_copies = copy_skin_primitive_curves(k, curves_3d)?;

    // Arguments of the unported legacy call: the profile counts, the private curve
    // copies, an approximation tolerance of 0.0, the degree and the cap flag.
    let _ = (curve_copies, cap_ends);
    // the new B-rep (`new SmBrep()`, defaults SM_USE_DEFAULT) and its infinite region
    let new_brep = brep_p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT)?;
    let _ = new_brep.infinite_region();
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::SmPrimitiveCreation
    // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreateSkinPrimitive
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateSkinPrimitive",
        "primitive.primitive_creation_p2",
    ))
}

/// A solid lofted between the matching loops of the face sequence `faces` of
/// `brep`, of degree `degree` in the loft direction (`SmApiCreateSkinFromFaces`).
///
/// # Errors
///
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateSkinFromFaces`).
pub fn create_skin_from_faces(
    k: &mut Kernel,
    brep: &Brep,
    faces: &[FaceId],
    degree: usize,
) -> Result<SkinFromFaces> {
    // Arguments of the unported legacy call: the faces, no guide curves, an
    // approximation tolerance of 0.0 and the degree.
    let _ = (brep, faces, degree);
    // the new B-rep (`new SmBrep()`, defaults SM_USE_DEFAULT) and its infinite region
    let new_brep = brep_p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT)?;
    let _ = new_brep.infinite_region();
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::SmPrimitiveCreation
    // UNPORTED(primitive.primitive_creation_p3): SmPrimitiveCreation::CreateSkinFromFaces
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateSkinFromFaces",
        "primitive.primitive_creation_p3",
    ))
}

/// The surface extruded from `curve` along `sweep_vector` (`SmApiCreateExtrude`,
/// `SmBSplineSurface::CreateLinearSweep`): linear in u, the curve's parameterisation
/// in v. A 2D curve's NURB is made 3D in place.
///
/// # Errors
///
/// Those of [`BSplineSurface::create_linear_sweep`] (`InvalidInput` for a sweep
/// vector shorter than `SM_EFF_ZERO`).
pub fn create_extrude(curve: &mut BSplineCurve, sweep_vector: Vec3) -> Result<BSplineSurface> {
    BSplineSurface::create_linear_sweep(curve, sweep_vector)
}

/// The surface swept by `curve` along `path` (`SmApiCreateSweep`,
/// `SmBSplineSurface::CreateSweepSurface` without scale curve, not translational,
/// within [`SWEEP_APPROX_TOL`]).
///
/// # Errors
///
/// Those of [`BSplineSurface::create_sweep_surface`].
pub fn create_sweep(curve: &BSplineCurve, path: &BSplineCurve) -> Result<BSplineSurface> {
    let tol = SWEEP_APPROX_TOL;

    BSplineSurface::create_sweep_surface(curve, path, None, false, tol)
}

/// The ruled surface between `curve1` and `curve2` (`SmApiCreateRuledSurface`,
/// `SmBSplineSurface::CreateRuledSurface` with `SM_SP_UNKNOWN`: linear in v).
///
/// # Errors
///
/// Those of [`BSplineSurface::create_ruled_surface`].
pub fn create_ruled_surface(
    curve1: &BSplineCurve,
    curve2: &BSplineCurve,
) -> Result<BSplineSurface> {
    BSplineSurface::create_ruled_surface(curve1, curve2, SurfParamType::Unknown)
}

/// The surface of revolution of `curve` about the axis through `origin` along
/// `axis_dir`, over `angle_deg` degrees (`SmApiCreateSurfaceRevolution`): a copy of
/// the curve is swept rotationally into a B-rep and its first surface returned.
///
/// # Errors
///
/// `InvalidInput` for no curve; [`SolidError::Unported`] otherwise
/// (`SmPrimitiveCreation::CreateRotationalSweep`).
pub fn create_surface_revolution(
    k: &mut Kernel,
    curve: Option<&BSplineCurve>,
    origin: Point3,
    axis_dir: Vec3,
    angle_deg: f64,
) -> Result<Surface> {
    let Some(curve) = curve else {
        return Err(SolidError::invalid_input(OP_REVOLUTION, "no curve"));
    };

    // Revolve a copy of the input: CreateRotationalSweep consumes the curve
    // (upstream's `SM_ERR` on a failed copy cannot happen: the copy is infallible)
    let curve_copy = Curve::from(curve.copy());
    let curves_to_sweep = vec![curve_copy];

    // Arguments of the unported call: the curves, the axis, the angle, one segment
    // and no capping.
    let _ = (curves_to_sweep, origin, axis_dir, angle_deg);
    // the new B-rep (`new SmBrep()`, defaults SM_USE_DEFAULT) and its infinite region
    let new_brep = brep_p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT)?;
    let _ = new_brep.infinite_region();
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::SmPrimitiveCreation
    // UNPORTED(primitive.primitive_creation_p3): SmPrimitiveCreation::CreateRotationalSweep
    // ported (topology.brep_p1): SmBrep::GetSurfaces is topology::brep::p1::surfaces, on the
    // sweep's B-rep after the call above
    Err(SolidError::unported("SmBrep::SmBrep", "topology.brep_p3"))
}

/// The surface skinned through `profiles` (`SmApiCreateSkin`,
/// `SmBSplineSurface::CreateSkinnedSurface`): not synchronized, the profiles become
/// u isoparameter curves, interpolated (tolerance `0.0`), no rails, an empty
/// derivative-surface array and the default degree 3.
///
/// # Errors
///
/// `InvalidInput` for fewer than two profiles; those of
/// [`BSplineSurface::create_skinned_surface`].
pub fn create_skin(k: &Kernel, profiles: &[BSplineCurve]) -> Result<BSplineSurface> {
    if profiles.len() < 2 {
        return Err(SolidError::invalid_input(
            OP_SKIN,
            "fewer than two profiles",
        ));
    }

    BSplineSurface::create_skinned_surface(
        k,
        profiles,
        SkinnedSurfaceOptions {
            synchronized: false,
            cross_section_surf_param: SurfParamType::U, // cross sections become U isoparameters
            approx_tol: 0.0,                            // interpolate (not approximate)
            rail1: None,
            rail2: None,
            rail1_used_as_spine_curve: false,
            intersections_with_rails: None,
            deriv_surfs: Some([None, None]), // derivative surfaces
            degree: 3,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_curves;

    fn wave(z: f64) -> BSplineCurve {
        let pts = [
            [0.0, 0.0],
            [4.0, 2.0],
            [6.0, -2.0],
            [8.0, 2.0],
            [10.0, 4.0],
            [12.0, 0.0],
        ]
        .map(|[x, y]| Point3::new(x, y, z));
        api_curves::create_curve(&pts).expect("curve")
    }

    #[test]
    fn extrude_and_ruled_succeed() {
        let mut c = wave(0.0);
        let s = create_extrude(&mut c, Vec3::new(0.0, 0.0, 1.0)).expect("extrude");
        assert!(s.gw_nurb().is_some());
        let err = create_extrude(&mut c, Vec3::new(0.0, 0.0, 0.0)).unwrap_err();
        assert!(matches!(err, SolidError::InvalidInput { .. }));
        let r = create_ruled_surface(&wave(0.0), &wave(10.0)).expect("ruled");
        assert!(r.gw_nurb().is_some());
    }

    #[test]
    fn skin_needs_two_profiles() {
        let k = &Kernel::default();
        let one = [api_curves::create_circle(Point3::new(0.0, 0.0, 0.0), 5.0).expect("c")];
        assert!(matches!(
            create_skin(k, &one),
            Err(SolidError::InvalidInput { .. })
        ));
        let three = [0.0, 5.0, 10.0].map(|z| {
            api_curves::create_circle(Point3::new(0.0, 0.0, z), 5.0 - z / 5.0).expect("c")
        });
        assert!(create_skin(k, &three).is_ok());
    }

    #[test]
    fn revolution_and_skin_primitive_validate_then_defer() {
        let o = Point3::new(0.0, 0.0, 0.0);
        let x = Vec3::new(1.0, 0.0, 0.0);
        assert!(matches!(
            create_surface_revolution(&mut Kernel::default(), None, o, x, 180.0),
            Err(SolidError::InvalidInput { .. })
        ));
        let c = wave(0.0);
        assert!(
            create_surface_revolution(&mut Kernel::default(), Some(&c), o, x, 180.0)
                .unwrap_err()
                .is_unported()
        );

        let mut k = Kernel::default();
        let a = Curve::from(wave(0.0));
        let b = Curve::from(wave(5.0));
        let ok = [Some(&a), Some(&b)];
        assert!(create_skin_primitive(&mut k, &[1, 1], &ok, 3, true)
            .unwrap_err()
            .is_unported());
        for (counts, degree) in [(&[1usize, 1][..], 0), (&[1, 1], 4), (&[2], 1), (&[1, 0], 1)] {
            assert!(matches!(
                create_skin_primitive(&mut k, counts, &ok, degree, true),
                Err(SolidError::InvalidInput { .. })
            ));
        }
        assert!(matches!(
            create_skin_primitive(&mut k, &[1, 1], &[Some(&a), None], 1, false),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            create_skin_primitive(&mut k, &[1, 2], &ok, 1, false),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(create_skin_from_faces(&mut k, &Brep::default(), &[], 3)
            .unwrap_err()
            .is_unported());
    }
}
