//! High-level B-rep sweeps (`SM_API`): linear, rotational, draft (taper), pipe and
//! curve sweeps of profile curves, faces or edges into solids, and sweeps along a
//! planar path.
//!
//! Upstream: `source/SM_API/src/SmApiBrep.cpp`, `source/SM_API/inc/SmApiBrep.h`.
//!
//! | Rust                                         | Upstream                                       |
//! |----------------------------------------------|------------------------------------------------|
//! | `copy_curves_for_consuming_sweep` (private)  | `SmApiCopyCurvesForConsumingSweep` (static)    |
//! | [`create_linear_sweep`]                      | `SmApiCreateLinearSweep`                       |
//! | [`create_rotational_sweep`]                  | `SmApiCreateRotationalSweep`                   |
//! | [`create_draft_sweep`]                       | `SmApiCreateDraftSweep`                        |
//! | [`create_pipe_sweep`]                        | `SmApiCreatePipeSweep`                         |
//! | [`create_sweep_along_planar_path`]           | `SmApiCreateSweepAlongPlanarPath`              |
//! | [`create_curve_sweep`], [`CurveSweep`]       | `SmApiCreateCurveSweep`                        |
//! | [`create_curve_sweep_from_faces`]            | `SmApiCreateCurveSweepFromFaces`               |
//! | [`create_curve_sweep_from_edges`]            | `SmApiCreateCurveSweepFromEdges`               |
//! | [`create_taper_extrude`]                     | `SmApiCreateTaperExtrude`                      |
//! | [`create_linear_sweep_with_repetitions`]     | `SmApiCreateLinearSweepWithRepetitions`        |
//! | [`create_rotational_sweep_with_repetitions`] | `SmApiCreateRotationalSweepWithRepetitions`    |
//!
//! Deviations:
//! - **Out-params.** The created B-rep (and, for the curve sweeps, its start, side and
//!   end faces) is the return value; on failure there is none (upstream clears
//!   `rpResult`). The optional face arrays of the curve sweeps are always returned.
//! - **Context.** `SmApiGetOrCreateContext()` is the caller's [`Kernel`], used for the
//!   curve copies.
//! - **Pointers.** A `NULL` curve that upstream tests for is `None`: the entries of the
//!   consumed curve arrays (copied first, `SM_ERR_INVALID_INPUT` for `NULL`), the
//!   entries of a curve-sweep profile (passed on, upstream skips them), and the path,
//!   scale-reference and scale curves of the curve and pipe sweeps. The profile and
//!   path curves of [`create_sweep_along_planar_path`], which upstream dereferences
//!   unchecked, are references. Upstream's path and scale curves are
//!   `SmBSplineCurve*`, which includes lines and ellipses; they are [`Curve`]s here.
//!   The faces and edges of the curve sweeps are ids into
//!   the caller's [`Brep`]. A `NULL` `pOptions` is `None` (the default options).
//! - **Types.** `SmBoolean` flags are `bool` (`bCornerType` of
//!   [`create_draft_sweep`]: any non-zero value is a fillet corner); the cap selectors
//!   `iCapEnds` / `iEndCaps` stay `i32`; `ULONG` repetition counts are `u64`.
//! - **Statuses.** `SM_ERR_INVALID_INPUT` is [`SolidError::InvalidInput`]; errors of
//!   the curve copies pass through unchanged (`SM_ERR` for a curve kind without
//!   `Copy`, e.g. a Hermite curve).
//! - Every entry point validates and copies its inputs as upstream, then returns
//!   [`SolidError::Unported`]: `SmPrimitiveCreation` (units
//!   `primitive.primitive_creation_p1..p3`) and the `SmBrep` construction it fills
//!   (`topology.brep_p1`, `topology.brep_p3`) are not ported yet. The error names the
//!   `SmPrimitiveCreation` method each entry point wraps.

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::tol::ZONE_TOL_3D;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::primitive::primitive_creation::SweepOptions;
use crate::topology::{Brep, EdgeId, FaceId};

const OP_COPY: &str = "sweep::api_brep::copy_curves_for_consuming_sweep";
const OP_PIPE: &str = "sweep::api_brep::create_pipe_sweep";
const OP_PLANAR_PATH: &str = "sweep::api_brep::create_sweep_along_planar_path";
const OP_CURVE_SWEEP: &str = "sweep::api_brep::create_curve_sweep";
const OP_FROM_FACES: &str = "sweep::api_brep::create_curve_sweep_from_faces";
const OP_FROM_EDGES: &str = "sweep::api_brep::create_curve_sweep_from_edges";
const OP_TAPER: &str = "sweep::api_brep::create_taper_extrude";
const OP_LINEAR_REPS: &str = "sweep::api_brep::create_linear_sweep_with_repetitions";
const OP_ROTATIONAL_REPS: &str = "sweep::api_brep::create_rotational_sweep_with_repetitions";

const UNIT_P2: &str = "primitive.primitive_creation_p2";

/// How the gaps between offset profile curves are filled by a draft sweep or taper
/// extrusion (`SmOffsetCornerType`).
pub use crate::curve::composite_curve::OffsetCornerType;

/// A swept solid and its faces (the outputs of the curve sweeps).
#[derive(Debug, Clone, Default)]
pub struct CurveSweep {
    /// `rpResult`: the swept solid.
    pub brep: Brep,
    /// `pOptStartFaces`: the start cap faces.
    pub start_faces: Vec<FaceId>,
    /// `pOptSideFaces`: the swept side faces.
    pub side_faces: Vec<FaceId>,
    /// `pOptEndFaces`: the end cap faces.
    pub end_faces: Vec<FaceId>,
}

/// Deep-copies the caller's profile curves for the consuming `SmPrimitiveCreation`
/// sweeps, which strip or delete the curves they are given
/// (`SmApiCopyCurvesForConsumingSweep`). The caller's curves are never changed.
///
/// # Errors
///
/// `InvalidInput` for a `None` curve; the error of the first failing copy (`SM_ERR`
/// for a curve kind without `Copy`). Curves after the first failure are not copied.
fn copy_curves_for_consuming_sweep(
    k: &Kernel,
    input_curves: &[Option<&Curve>],
) -> Result<Vec<Curve>> {
    let mut curve_copies = Vec::new();
    for input in input_curves {
        let Some(input) = input else {
            return Err(SolidError::invalid_input(OP_COPY, "NULL curve"));
        };
        curve_copies.push(input.copy(k)?);
    }
    Ok(curve_copies)
}

/// The solid swept by `curves_to_sweep` along `sweep_dir` over `sweep_dist`
/// (`SmApiCreateLinearSweep`): one repetition, capped at both ends with `cap_ends`.
/// The caller's curves are copied, never changed.
///
/// # Errors
///
/// `InvalidInput` for a `None` curve; the error of a failing curve copy;
/// [`SolidError::Unported`] otherwise (`SmPrimitiveCreation::CreateLinearSweep`).
pub fn create_linear_sweep(
    k: &Kernel,
    curves_to_sweep: &[Option<&Curve>],
    sweep_dir: Vec3,
    sweep_dist: f64,
    cap_ends: bool,
) -> Result<Brep> {
    // Sweep copies of the inputs: CreateLinearSweep consumes the curves.
    let curve_copies = copy_curves_for_consuming_sweep(k, curves_to_sweep)?;

    // Arguments of the unported call: the copies, the direction and distance, one
    // repetition and the cap flag (SmPrimitiveCreation(region, 0, true)).
    let repetitions: u64 = 1;
    let _ = (curve_copies, sweep_dir, sweep_dist, repetitions, cap_ends);
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // ported (topology.brep_p1): SmBrep::GetInfiniteRegion is Brep::infinite_region, called
    // on the new B-rep once the constructor above lands
    // Ported: primitive::primitive_creation::p1::PrimitiveCreation::new;
    // instantiate with the new B-rep's infinite region when its constructor lands.
    // Ported: primitive::primitive_creation::p3::PrimitiveCreation::create_repeated_linear_sweep;
    // supply the new B-rep through PrimitiveAccess after its constructor is ported.
    Err(SolidError::unported("SmBrep::SmBrep", "topology.brep_p3"))
}

/// The solid swept by revolving `curves_to_sweep` about the axis through `base_pt`
/// along `axis` by `angle_deg` degrees (`SmApiCreateRotationalSweep`): one
/// repetition, capped at both ends with `cap_ends`. The caller's curves are copied,
/// never changed.
///
/// # Errors
///
/// `InvalidInput` for a `None` curve; the error of a failing curve copy;
/// [`SolidError::Unported`] otherwise (`SmPrimitiveCreation::CreateRotationalSweep`).
pub fn create_rotational_sweep(
    k: &Kernel,
    curves_to_sweep: &[Option<&Curve>],
    base_pt: Point3,
    axis: Vec3,
    angle_deg: f64,
    cap_ends: bool,
) -> Result<Brep> {
    // Sweep copies of the inputs: CreateRotationalSweep consumes the curves.
    let curve_copies = copy_curves_for_consuming_sweep(k, curves_to_sweep)?;

    // Arguments of the unported call: the copies, the axis, the angle, one
    // repetition and the cap flag (SmPrimitiveCreation(region, 0, true)).
    let repetitions: u64 = 1;
    let _ = (
        curve_copies,
        base_pt,
        axis,
        angle_deg,
        repetitions,
        cap_ends,
    );
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // ported (topology.brep_p1): SmBrep::GetInfiniteRegion is Brep::infinite_region, called
    // on the new B-rep once the constructor above lands
    // Ported: primitive::primitive_creation::p1::PrimitiveCreation::new;
    // instantiate with the new B-rep's infinite region when its constructor lands.
    // Ported: primitive::primitive_creation::p3::PrimitiveCreation::create_partial_rotational_sweep;
    // supply the new B-rep through PrimitiveAccess after its constructor is ported.
    Err(SolidError::unported("SmBrep::SmBrep", "topology.brep_p3"))
}

/// The solid extruded from `curves_to_sweep` to `height` with a draft (taper)
/// angle of `angle_deg` degrees (`SmApiCreateDraftSweep`). `corner_type` selects
/// fillet corners (`true`) or linear extensions (`false`) between the offset curves;
/// `cap_ends` is `0` none, `1` at the curves, `2` at the offset, `3` both ends. The
/// curve normal is computed by the extrusion (a zero normal is passed). The caller's
/// curves are copied, never changed.
///
/// # Errors
///
/// `InvalidInput` for a `None` curve; the error of a failing curve copy;
/// [`SolidError::Unported`] otherwise (`SmPrimitiveCreation::CreateTaperExtrude`).
pub fn create_draft_sweep(
    k: &Kernel,
    curves_to_sweep: &[Option<&Curve>],
    height: f64,
    angle_deg: f64,
    corner_type: bool,
    cap_ends: i32,
) -> Result<Brep> {
    // Sweep copies of the inputs: CreateTaperExtrude consumes the curves.
    let curve_copies = copy_curves_for_consuming_sweep(k, curves_to_sweep)?;

    // Let CreateTaperExtrude compute the normal
    let crv_normal = Vec3::new(0.0, 0.0, 0.0);

    let corner = if corner_type {
        OffsetCornerType::FilletCorner
    } else {
        OffsetCornerType::LinearExtension
    };

    // Arguments of the unported call (SmPrimitiveCreation(region, 0, true)).
    let _ = (
        curve_copies,
        height,
        angle_deg,
        cap_ends,
        crv_normal,
        corner,
    );
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // ported (topology.brep_p1): SmBrep::GetInfiniteRegion is Brep::infinite_region, called
    // on the new B-rep once the constructor above lands
    // Ported: primitive::primitive_creation::p1::PrimitiveCreation::new;
    // instantiate with the new B-rep's infinite region when its constructor lands.
    // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreateTaperExtrude
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateTaperExtrude",
        UNIT_P2,
    ))
}

/// The pipe of radius `pipe_radius` swept along `path_curve`
/// (`SmApiCreatePipeSweep`), capped at both ends with `cap_ends`; not
/// translational.
///
/// # Errors
///
/// `InvalidInput` for no path curve or a radius `<= 0` (a NaN radius passes, as
/// upstream); [`SolidError::Unported`] otherwise
/// (`SmPrimitiveCreation::CreatePipeSweep`).
pub fn create_pipe_sweep(
    k: &Kernel,
    pipe_radius: f64,
    path_curve: Option<&Curve>,
    cap_ends: bool,
) -> Result<Brep> {
    let Some(path_curve) = path_curve else {
        return Err(SolidError::invalid_input(OP_PIPE, "no path curve"));
    };
    if pipe_radius <= 0.0 {
        return Err(SolidError::invalid_input(OP_PIPE, "pipe radius <= 0"));
    }

    // Arguments of the unported call: the radius, the path, an approximation
    // tolerance of 0.0, the cap flag and not translational
    // (SmPrimitiveCreation(region, 0, true)).
    let approx_tol = 0.0;
    let translational_sweep = false;
    let _ = (k, path_curve, approx_tol, cap_ends, translational_sweep);
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // ported (topology.brep_p1): SmBrep::GetInfiniteRegion is Brep::infinite_region, called
    // on the new B-rep once the constructor above lands
    // Ported: primitive::primitive_creation::p1::PrimitiveCreation::new;
    // instantiate with the new B-rep's infinite region when its constructor lands.
    // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreatePipeSweep
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreatePipeSweep",
        UNIT_P2,
    ))
}

/// The solid swept by the profile boundary `profile_curves` along the ordered,
/// endpoint-connected planar path `path_curves` (`SmApiCreateSweepAlongPlanarPath`).
/// `move_profile` anchors the profile center (`true`) or its first point (`false`)
/// at the path start; `cap_ends` is `0` none, `1` profile/start, `2` far end, `3`
/// both (ignored for a closed path). The inputs are not changed.
///
/// # Errors
///
/// `InvalidInput` for no profile or path curve or `cap_ends` outside `0..=3`;
/// [`SolidError::Unported`] otherwise
/// (`SmPrimitiveCreation::CreateSweepAlongPlanarPath`).
pub fn create_sweep_along_planar_path(
    k: &Kernel,
    profile_curves: &[&Curve],
    path_curves: &[&Curve],
    move_profile: bool,
    cap_ends: i32,
) -> Result<Brep> {
    if profile_curves.is_empty() || path_curves.is_empty() {
        return Err(SolidError::invalid_input(
            OP_PLANAR_PATH,
            "no profile or path curves",
        ));
    }
    if !(0..=3).contains(&cap_ends) {
        return Err(SolidError::invalid_input(
            OP_PLANAR_PATH,
            "cap selector outside 0..=3",
        ));
    }

    let tol = ZONE_TOL_3D;
    let cap_start = (cap_ends & 1) != 0;
    let cap_end = (cap_ends & 2) != 0;

    // Arguments of the unported call.
    let _ = (
        k,
        profile_curves,
        path_curves,
        tol,
        cap_start,
        cap_end,
        move_profile,
    );
    // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreateSweepAlongPlanarPath
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateSweepAlongPlanarPath",
        UNIT_P2,
    ))
}

/// The solid swept by `profile_curves` along `path_curve`, scaled as
/// `scale_curve` relative to `scale_reference` when given, with `options` (`None`:
/// the defaults) (`SmApiCreateCurveSweep`). The kernel sweeps copies of the
/// profile; `None` profile entries are passed on (upstream skips them).
///
/// # Errors
///
/// `InvalidInput` for an empty profile or no path curve; [`SolidError::Unported`]
/// otherwise (`SmPrimitiveCreation::CreateCurveSweep`).
pub fn create_curve_sweep(
    k: &Kernel,
    profile_curves: &[Option<&Curve>],
    path_curve: Option<&Curve>,
    scale_reference: Option<&Curve>,
    scale_curve: Option<&Curve>,
    options: Option<&SweepOptions>,
) -> Result<CurveSweep> {
    let Some(path_curve) = path_curve.filter(|_| !profile_curves.is_empty()) else {
        return Err(SolidError::invalid_input(
            OP_CURVE_SWEEP,
            "no profile curves or no path curve",
        ));
    };

    let options = options.copied().unwrap_or_default();

    // Arguments of the unported call: the profile, path and scale curves, an
    // approximation tolerance of SM_ZONE_TOL_3D and the options
    // (SmPrimitiveCreation(region) with its default arguments). CreateCurveSweep's
    // trailing bTestContinuity is left at its default, TRUE: the p3 porter must pass
    // true there.
    let approx_tol = ZONE_TOL_3D;
    let _ = (k, profile_curves, path_curve, scale_reference, scale_curve);
    let _ = (approx_tol, options);
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // ported (topology.brep_p1): SmBrep::GetInfiniteRegion is Brep::infinite_region, called
    // on the new B-rep once the constructor above lands
    // Ported: primitive::primitive_creation::p1::PrimitiveCreation::new;
    // instantiate with the new B-rep's infinite region when its constructor lands.
    // Ported: primitive::primitive_creation::p3::PrimitiveCreation::create_curve_sweep;
    // supply the new B-rep through PrimitiveAccess after its constructor is ported.
    Err(SolidError::unported("SmBrep::SmBrep", "topology.brep_p3"))
}

/// The solid swept by the boundaries of `faces` of `brep` along `path_curve`
/// (`SmApiCreateCurveSweepFromFaces`), scaled and optioned as
/// [`create_curve_sweep`]. The faces and their curves are not changed.
///
/// # Errors
///
/// `InvalidInput` for no faces or no path curve; [`SolidError::Unported`] otherwise
/// (`SmPrimitiveCreation::CreateCurveSweepFromFaces`).
pub fn create_curve_sweep_from_faces(
    k: &Kernel,
    brep: &Brep,
    faces: &[FaceId],
    path_curve: Option<&Curve>,
    scale_reference: Option<&Curve>,
    scale_curve: Option<&Curve>,
    options: Option<&SweepOptions>,
) -> Result<CurveSweep> {
    let Some(path_curve) = path_curve.filter(|_| !faces.is_empty()) else {
        return Err(SolidError::invalid_input(
            OP_FROM_FACES,
            "no faces or no path curve",
        ));
    };

    let options = options.copied().unwrap_or_default();

    // Arguments of the unported call (as create_curve_sweep).
    let approx_tol = ZONE_TOL_3D;
    let _ = (k, brep, faces, path_curve, scale_reference, scale_curve);
    let _ = (approx_tol, options);
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // ported (topology.brep_p1): SmBrep::GetInfiniteRegion is Brep::infinite_region, called
    // on the new B-rep once the constructor above lands
    // Ported: primitive::primitive_creation::p1::PrimitiveCreation::new;
    // instantiate with the new B-rep's infinite region when its constructor lands.
    // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreateCurveSweepFromFaces
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateCurveSweepFromFaces",
        UNIT_P2,
    ))
}

/// The solid swept by the curves of `edges` of `brep` along `path_curve`
/// (`SmApiCreateCurveSweepFromEdges`), scaled and optioned as
/// [`create_curve_sweep`]. The edges and their curves are not changed.
///
/// # Errors
///
/// `InvalidInput` for no edges or no path curve; [`SolidError::Unported`] otherwise
/// (`SmPrimitiveCreation::CreateCurveSweepFromEdges`).
pub fn create_curve_sweep_from_edges(
    k: &Kernel,
    brep: &Brep,
    edges: &[EdgeId],
    path_curve: Option<&Curve>,
    scale_reference: Option<&Curve>,
    scale_curve: Option<&Curve>,
    options: Option<&SweepOptions>,
) -> Result<CurveSweep> {
    let Some(path_curve) = path_curve.filter(|_| !edges.is_empty()) else {
        return Err(SolidError::invalid_input(
            OP_FROM_EDGES,
            "no edges or no path curve",
        ));
    };

    let options = options.copied().unwrap_or_default();

    // Arguments of the unported call (as create_curve_sweep).
    let approx_tol = ZONE_TOL_3D;
    let _ = (k, brep, edges, path_curve, scale_reference, scale_curve);
    let _ = (approx_tol, options);
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // ported (topology.brep_p1): SmBrep::GetInfiniteRegion is Brep::infinite_region, called
    // on the new B-rep once the constructor above lands
    // Ported: primitive::primitive_creation::p1::PrimitiveCreation::new;
    // instantiate with the new B-rep's infinite region when its constructor lands.
    // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreateCurveSweepFromEdges
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateCurveSweepFromEdges",
        UNIT_P2,
    ))
}

/// The solid extruded from `curves` to `height` with a draft angle of
/// `draft_angle_deg` degrees (`SmApiCreateTaperExtrude`). `end_caps` is `0` none,
/// `1` at the curves, `2` at the offset, `3` both; `fillet_corner` selects fillet
/// corners over linear extensions. The curve normal is computed by the extrusion.
/// The caller's curves are copied, never changed.
///
/// # Errors
///
/// `InvalidInput` for no curves or a `None` curve; the error of a failing curve
/// copy; [`SolidError::Unported`] otherwise
/// (`SmPrimitiveCreation::CreateTaperExtrude`).
pub fn create_taper_extrude(
    k: &Kernel,
    curves: &[Option<&Curve>],
    height: f64,
    draft_angle_deg: f64,
    end_caps: i32,
    fillet_corner: bool,
) -> Result<Brep> {
    if curves.is_empty() {
        return Err(SolidError::invalid_input(OP_TAPER, "no curves"));
    }

    // Sweep copies of the inputs: CreateTaperExtrude consumes the curves.
    let curve_copies = copy_curves_for_consuming_sweep(k, curves)?;

    let normal = Vec3::new(0.0, 0.0, 0.0);
    let corner = if fillet_corner {
        OffsetCornerType::FilletCorner
    } else {
        OffsetCornerType::LinearExtension
    };

    // Arguments of the unported call (SmPrimitiveCreation(region)).
    let _ = (
        curve_copies,
        height,
        draft_angle_deg,
        end_caps,
        normal,
        corner,
    );
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // ported (topology.brep_p1): SmBrep::GetInfiniteRegion is Brep::infinite_region, called
    // on the new B-rep once the constructor above lands
    // Ported: primitive::primitive_creation::p1::PrimitiveCreation::new;
    // instantiate with the new B-rep's infinite region when its constructor lands.
    // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreateTaperExtrude
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateTaperExtrude",
        UNIT_P2,
    ))
}

/// The solid swept by `curves_to_sweep` along `sweep_dir` over `sweep_dist`,
/// repeated end to end `repetitions` times (`SmApiCreateLinearSweepWithRepetitions`).
/// The caller's curves are copied, never changed.
///
/// # Errors
///
/// `InvalidInput` for no curves or a `None` curve; the error of a failing curve
/// copy; [`SolidError::Unported`] otherwise
/// (`SmPrimitiveCreation::CreateLinearSweep`).
pub fn create_linear_sweep_with_repetitions(
    k: &Kernel,
    curves_to_sweep: &[Option<&Curve>],
    sweep_dir: Vec3,
    sweep_dist: f64,
    repetitions: u64,
    cap_ends: bool,
) -> Result<Brep> {
    if curves_to_sweep.is_empty() {
        return Err(SolidError::invalid_input(OP_LINEAR_REPS, "no curves"));
    }

    // Sweep copies of the inputs: CreateLinearSweep consumes the curves.
    let curve_copies = copy_curves_for_consuming_sweep(k, curves_to_sweep)?;

    // Arguments of the unported call (SmPrimitiveCreation(region)).
    let _ = (curve_copies, sweep_dir, sweep_dist, repetitions, cap_ends);
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // ported (topology.brep_p1): SmBrep::GetInfiniteRegion is Brep::infinite_region, called
    // on the new B-rep once the constructor above lands
    // Ported: primitive::primitive_creation::p1::PrimitiveCreation::new;
    // instantiate with the new B-rep's infinite region when its constructor lands.
    // Ported: primitive::primitive_creation::p3::PrimitiveCreation::create_repeated_linear_sweep;
    // supply the new B-rep through PrimitiveAccess after its constructor is ported.
    Err(SolidError::unported("SmBrep::SmBrep", "topology.brep_p3"))
}

/// The solid swept by revolving `curves_to_sweep` about the axis through `base_pt`
/// along `axis` by `angle_deg` degrees, repeated end to end `repetitions` times
/// (`SmApiCreateRotationalSweepWithRepetitions`). The caller's curves are copied,
/// never changed.
///
/// # Errors
///
/// `InvalidInput` for no curves or a `None` curve; the error of a failing curve
/// copy; [`SolidError::Unported`] otherwise
/// (`SmPrimitiveCreation::CreateRotationalSweep`).
pub fn create_rotational_sweep_with_repetitions(
    k: &Kernel,
    curves_to_sweep: &[Option<&Curve>],
    base_pt: Point3,
    axis: Vec3,
    angle_deg: f64,
    repetitions: u64,
    cap_ends: bool,
) -> Result<Brep> {
    if curves_to_sweep.is_empty() {
        return Err(SolidError::invalid_input(OP_ROTATIONAL_REPS, "no curves"));
    }

    // Sweep copies of the inputs: CreateRotationalSweep consumes the curves.
    let curve_copies = copy_curves_for_consuming_sweep(k, curves_to_sweep)?;

    // Arguments of the unported call (SmPrimitiveCreation(region)).
    let _ = (
        curve_copies,
        base_pt,
        axis,
        angle_deg,
        repetitions,
        cap_ends,
    );
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // ported (topology.brep_p1): SmBrep::GetInfiniteRegion is Brep::infinite_region, called
    // on the new B-rep once the constructor above lands
    // Ported: primitive::primitive_creation::p1::PrimitiveCreation::new;
    // instantiate with the new B-rep's infinite region when its constructor lands.
    // Ported: primitive::primitive_creation::p3::PrimitiveCreation::create_partial_rotational_sweep;
    // supply the new B-rep through PrimitiveAccess after its constructor is ported.
    Err(SolidError::unported("SmBrep::SmBrep", "topology.brep_p3"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::hermite_curve::HermiteCurve;
    use crate::primitive::api_curves;

    fn circle() -> Curve {
        Curve::from(api_curves::create_circle(Point3::new(0.0, 0.0, 0.0), 2.0).expect("circle"))
    }

    fn line() -> Curve {
        Curve::from(
            api_curves::create_line_segment(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0))
                .expect("line"),
        )
    }

    fn hermite() -> Curve {
        let o = Point3::new(0.0, 0.0, 0.0);
        let x = Vec3::new(1.0, 0.0, 0.0);
        Curve::from(HermiteCurve::new(o, x, Point3::new(1.0, 1.0, 0.0), x, 3))
    }

    fn path() -> Curve {
        let pts = [0.0, 3.0, 7.0, 10.0].map(|z| Point3::new(0.0, 0.0, z));
        Curve::from(api_curves::create_curve(&pts).expect("path"))
    }

    fn invalid<T>(r: &Result<T>) -> bool {
        matches!(r, Err(SolidError::InvalidInput { .. }))
    }

    fn unported<T>(r: &Result<T>) -> bool {
        r.as_ref().err().is_some_and(SolidError::is_unported)
    }

    #[test]
    fn consuming_sweeps_copy_then_defer() {
        let k = Kernel::default();
        let (c, l, h) = (circle(), line(), hermite());
        let ok = [Some(&c), Some(&l)];
        let z = Vec3::new(0.0, 0.0, 1.0);
        let o = Point3::new(0.0, 0.0, 0.0);
        assert!(unported(&create_linear_sweep(&k, &ok, z, 10.0, true)));
        assert!(unported(&create_linear_sweep(&k, &[], z, 10.0, true)));
        assert!(invalid(&create_linear_sweep(
            &k,
            &[Some(&c), None],
            z,
            1.0,
            false
        )));
        assert!(unported(&create_rotational_sweep(
            &k, &ok, o, z, 90.0, false
        )));
        assert!(unported(&create_draft_sweep(&k, &ok, 10.0, -5.0, true, 3)));
        assert!(unported(&create_draft_sweep(&k, &[], 10.0, -5.0, false, 3)));
        assert!(invalid(&create_draft_sweep(
            &k,
            &[None],
            10.0,
            5.0,
            false,
            0
        )));
        // SM_ERR of the Hermite curve's base Copy, before the later NULL is seen
        let err = create_rotational_sweep(&k, &[Some(&h), None], o, z, 90.0, false).unwrap_err();
        assert!(matches!(
            err,
            SolidError::Upstream {
                code: crate::error::SM_ERR,
                ..
            }
        ));
        assert!(invalid(&create_taper_extrude(&k, &[], 10.0, 5.0, 3, false)));
        assert!(invalid(&create_taper_extrude(
            &k,
            &[Some(&l), None],
            10.0,
            5.0,
            3,
            false
        )));
        assert!(unported(&create_taper_extrude(&k, &ok, 10.0, 5.0, 3, true)));
        assert!(invalid(&create_linear_sweep_with_repetitions(
            &k,
            &[],
            z,
            5.0,
            2,
            false
        )));
        assert!(unported(&create_linear_sweep_with_repetitions(
            &k, &ok, z, 5.0, 0, true
        )));
        assert!(invalid(&create_rotational_sweep_with_repetitions(
            &k,
            &[],
            o,
            z,
            5.0,
            2,
            false
        )));
        assert!(unported(&create_rotational_sweep_with_repetitions(
            &k, &ok, o, z, 5.0, 2, false
        )));
    }

    #[test]
    fn pipe_and_planar_path_validate_then_defer() {
        let k = Kernel::default();
        let p = path();
        assert!(invalid(&create_pipe_sweep(&k, 1.0, None, true)));
        assert!(invalid(&create_pipe_sweep(&k, 0.0, Some(&p), true)));
        assert!(invalid(&create_pipe_sweep(&k, -1.0, Some(&p), false)));
        assert!(unported(&create_pipe_sweep(&k, f64::NAN, Some(&p), false)));
        assert!(unported(&create_pipe_sweep(&k, 1.0, Some(&p), true)));

        let (c, l) = (circle(), line());
        assert!(invalid(&create_sweep_along_planar_path(
            &k,
            &[],
            &[&l],
            true,
            3
        )));
        assert!(invalid(&create_sweep_along_planar_path(
            &k,
            &[&c],
            &[],
            true,
            3
        )));
        assert!(invalid(&create_sweep_along_planar_path(
            &k,
            &[&c],
            &[&l],
            true,
            -1
        )));
        assert!(invalid(&create_sweep_along_planar_path(
            &k,
            &[&c],
            &[&l],
            true,
            4
        )));
        for caps in 0..=3 {
            assert!(unported(&create_sweep_along_planar_path(
                &k,
                &[&c],
                &[&l],
                false,
                caps
            )));
        }
    }

    #[test]
    fn curve_sweeps_validate_then_defer() {
        let k = Kernel::default();
        let p = path();
        let c = circle();
        let b = Brep::default();
        let opts = SweepOptions {
            cap_ends: true,
            ..SweepOptions::default()
        };
        assert!(invalid(&create_curve_sweep(
            &k,
            &[],
            Some(&p),
            None,
            None,
            None
        )));
        assert!(invalid(&create_curve_sweep(
            &k,
            &[Some(&c)],
            None,
            None,
            None,
            None
        )));
        assert!(unported(&create_curve_sweep(
            &k,
            &[Some(&c)],
            Some(&p),
            Some(&p),
            None,
            Some(&opts)
        )));
        assert!(unported(&create_curve_sweep(
            &k,
            &[None],
            Some(&p),
            None,
            None,
            None
        )));
        assert!(invalid(&create_curve_sweep_from_faces(
            &k,
            &b,
            &[],
            Some(&p),
            None,
            None,
            None
        )));
        assert!(invalid(&create_curve_sweep_from_edges(
            &k,
            &b,
            &[],
            Some(&p),
            None,
            None,
            None
        )));
    }
}
