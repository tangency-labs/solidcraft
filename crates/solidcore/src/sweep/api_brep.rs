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
//! - **Cached topology.** Every returned B-rep has the port's own cached fields
//!   ([`crate::topology::Edge::vertices`], `Face::shell`, ..., not maintained by topology
//!   edits) rewritten from its use links by [`Brep::refresh_cached_topology`]; nothing
//!   else changes.
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
//! - Every entry point validates and copies its inputs as upstream, then runs the
//!   `SmPrimitiveCreation` method it wraps on a new B-rep through a
//!   [`PrimitiveModel`]. Path and scale curves become their `SmBSplineCurve` part.
//!   The faces and edges of the curve sweeps are `(&Brep, id)` pairs: upstream
//!   objects reach their own B-rep.

use crate::curve::curve::p2::CurveKind;
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::tol::ZONE_TOL_3D;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::primitive::primitive_creation::model::PrimitiveModel;
use crate::primitive::primitive_creation::p1::PrimitiveCreation;
use crate::primitive::primitive_creation::p3::RepeatedSweepOptions;
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
    k: &mut Kernel,
    curves_to_sweep: &[Option<&Curve>],
    sweep_dir: Vec3,
    sweep_dist: f64,
    cap_ends: bool,
) -> Result<Brep> {
    // Sweep copies of the inputs: CreateLinearSweep consumes the curves.
    let curve_copies = copy_curves_for_consuming_sweep(k, curves_to_sweep)?;

    let mut curve_copies: Vec<Option<Curve>> = curve_copies.into_iter().map(Some).collect();
    build(k, |pc, model| {
        pc.create_repeated_linear_sweep(
            model,
            PrimitiveModel::MAIN,
            &mut curve_copies,
            sweep_dir,
            sweep_dist,
            repeated(1, cap_ends),
        )
    })
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
    k: &mut Kernel,
    curves_to_sweep: &[Option<&Curve>],
    base_pt: Point3,
    axis: Vec3,
    angle_deg: f64,
    cap_ends: bool,
) -> Result<Brep> {
    // Sweep copies of the inputs: CreateRotationalSweep consumes the curves.
    let curve_copies = copy_curves_for_consuming_sweep(k, curves_to_sweep)?;

    let mut curve_copies: Vec<Option<Curve>> = curve_copies.into_iter().map(Some).collect();
    build(k, |pc, model| {
        pc.create_partial_rotational_sweep(
            model,
            PrimitiveModel::MAIN,
            &mut curve_copies,
            base_pt,
            axis,
            angle_deg,
            repeated(1, cap_ends),
        )
    })
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
/// [`SolidError::Unported`] otherwise (`SmBrep::SmBrep`, until the B-rep constructor is ported).
pub fn create_draft_sweep(
    k: &mut Kernel,
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

    let mut crv_normal = crv_normal;
    build(k, |pc, model| {
        pc.create_taper_extrude(
            model,
            PrimitiveModel::MAIN,
            curve_copies,
            height,
            angle_deg,
            cap_ends,
            &mut crv_normal,
            corner,
        )
    })
}

/// The pipe of radius `pipe_radius` swept along `path_curve`
/// (`SmApiCreatePipeSweep`), capped at both ends with `cap_ends`; not
/// translational.
///
/// # Errors
///
/// `InvalidInput` for no path curve or a radius `<= 0` (a NaN radius passes, as
/// upstream); [`SolidError::Unported`] otherwise
/// (`SmBrep::SmBrep`, until the B-rep constructor is ported).
pub fn create_pipe_sweep(
    k: &mut Kernel,
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

    let path = bspline_of(path_curve, OP_PIPE)?;
    let approx_tol = 0.0;
    let translational_sweep = false;
    build(k, |pc, model| {
        let (mut start, mut side, mut end) = (Vec::new(), Vec::new(), Vec::new());
        pc.create_pipe_sweep(
            model,
            PrimitiveModel::MAIN,
            pipe_radius,
            &path,
            approx_tol,
            cap_ends,
            translational_sweep,
            &mut start,
            &mut side,
            &mut end,
        )
    })
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
/// (`SmBrep::SmBrep`, until the B-rep constructor is ported).
pub fn create_sweep_along_planar_path(
    k: &mut Kernel,
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

    let profiles: Vec<Curve> = profile_curves.iter().map(|c| (*c).clone()).collect();
    let paths: Vec<Curve> = path_curves.iter().map(|c| (*c).clone()).collect();
    // The static entry point makes its own B-rep in the context of the model's.
    let (_, mut result) = PrimitiveModel::build_new(k, |_, model| {
        let id = PrimitiveCreation::create_sweep_along_planar_path(
            model,
            PrimitiveModel::MAIN,
            &profiles,
            &paths,
            tol,
            cap_start,
            cap_end,
            move_profile,
        )?;
        model.take_brep(id)
    })?;
    // (not upstream: the port's cached topology fields, true for the caller)
    result.refresh_cached_topology();
    Ok(result)
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
    k: &mut Kernel,
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

    let approx_tol = ZONE_TOL_3D;
    let path = bspline_of(path_curve, OP_CURVE_SWEEP)?;
    let scale = scale_curve
        .map(|c| bspline_of(c, OP_CURVE_SWEEP))
        .transpose()?;
    let profiles: Vec<Option<Curve>> = profile_curves.iter().map(|c| c.cloned()).collect();
    curve_sweep(k, |pc, model, faces| {
        pc.create_curve_sweep(
            model,
            PrimitiveModel::MAIN,
            &profiles,
            &path,
            scale_reference,
            scale.as_ref(),
            approx_tol,
            options,
            true,
            &mut faces.0,
            &mut faces.1,
            &mut faces.2,
        )
    })
}

/// The solid swept by the boundaries of `faces` (each with its B-rep) along
/// `path_curve` (`SmApiCreateCurveSweepFromFaces`), scaled and optioned as
/// [`create_curve_sweep`]. The faces and their curves are not changed.
///
/// # Errors
///
/// `InvalidInput` for no faces or no path curve; those of
/// `SmPrimitiveCreation::CreateCurveSweepFromFaces`.
pub fn create_curve_sweep_from_faces(
    k: &mut Kernel,
    faces: &[(&Brep, FaceId)],
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

    let approx_tol = ZONE_TOL_3D;
    let (path, scale_reference, scale) =
        sweep_curves(path_curve, scale_reference, scale_curve, OP_FROM_FACES)?;
    curve_sweep(k, |pc, model, out| {
        let input = model.add_owner_breps(faces)?;
        pc.create_curve_sweep_from_faces(
            model,
            PrimitiveModel::MAIN,
            &input,
            &path,
            scale_reference.as_ref(),
            scale.as_ref(),
            approx_tol,
            options,
            &mut out.0,
            &mut out.1,
            &mut out.2,
        )
    })
}

/// The solid swept by the curves of `edges` (each with its B-rep) along
/// `path_curve` (`SmApiCreateCurveSweepFromEdges`), scaled and optioned as
/// [`create_curve_sweep`]. The edges and their curves are not changed.
///
/// # Errors
///
/// `InvalidInput` for no edges or no path curve; those of
/// `SmPrimitiveCreation::CreateCurveSweepFromEdges`.
pub fn create_curve_sweep_from_edges(
    k: &mut Kernel,
    edges: &[(&Brep, EdgeId)],
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

    let approx_tol = ZONE_TOL_3D;
    let (path, scale_reference, scale) =
        sweep_curves(path_curve, scale_reference, scale_curve, OP_FROM_EDGES)?;
    curve_sweep(k, |pc, model, out| {
        let input = model.add_owner_breps(edges)?;
        pc.create_curve_sweep_from_edges(
            model,
            PrimitiveModel::MAIN,
            &input,
            &path,
            scale_reference.as_ref(),
            scale.as_ref(),
            approx_tol,
            options,
            &mut out.0,
            &mut out.1,
            &mut out.2,
        )
    })
}

/// The solid extruded from `curves` to `height` with a draft angle of
/// `draft_angle_deg` degrees (`SmApiCreateTaperExtrude`). `end_caps` is `0` none,
/// `1` at the curves, `2` at the offset, `3` both; `fillet_corner` selects fillet
/// corners over linear extensions. The curve normal is computed by the extrusion.
/// The caller's curves are copied, never changed.
///
/// As in the C++ code, only the sign of `draft_angle_deg` picks the side (positive
/// inner, negative outer) and the in-plane offset is `|height * tan(|draft_angle_deg|)|`,
/// so an angle over 90 does not give the outer draft the C++ header promises. The
/// curves must form one chain (`DOC-5` in `plan/upstream-issues.md`).
///
/// # Errors
///
/// `InvalidInput` for no curves or a `None` curve; the error of a failing curve
/// copy; [`SolidError::Unported`] otherwise
/// (`SmBrep::SmBrep`, until the B-rep constructor is ported).
pub fn create_taper_extrude(
    k: &mut Kernel,
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

    let mut normal = normal;
    build(k, |pc, model| {
        pc.create_taper_extrude(
            model,
            PrimitiveModel::MAIN,
            curve_copies,
            height,
            draft_angle_deg,
            end_caps,
            &mut normal,
            corner,
        )
    })
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
    k: &mut Kernel,
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

    let mut curve_copies: Vec<Option<Curve>> = curve_copies.into_iter().map(Some).collect();
    build(k, |pc, model| {
        pc.create_repeated_linear_sweep(
            model,
            PrimitiveModel::MAIN,
            &mut curve_copies,
            sweep_dir,
            sweep_dist,
            repeated(repetitions, cap_ends),
        )
    })
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
    k: &mut Kernel,
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

    let mut curve_copies: Vec<Option<Curve>> = curve_copies.into_iter().map(Some).collect();
    build(k, |pc, model| {
        pc.create_partial_rotational_sweep(
            model,
            PrimitiveModel::MAIN,
            &mut curve_copies,
            base_pt,
            axis,
            angle_deg,
            repeated(repetitions, cap_ends),
        )
    })
}

/// `nRepetitions` and `bCapEnds` with `bTestContinuity` at its default, `TRUE`.
fn repeated(repetitions: u64, cap_ends: bool) -> RepeatedSweepOptions {
    RepeatedSweepOptions {
        repetitions,
        cap_ends,
        test_continuity: true,
    }
}

/// A new B-rep filled by `body` (`new SmBrep()`, `SmPrimitiveCreation`, the call).
fn build(
    k: &mut Kernel,
    body: impl FnOnce(&mut PrimitiveCreation, &mut PrimitiveModel<'_>) -> Result<()>,
) -> Result<Brep> {
    PrimitiveModel::build_new(k, body).map(|(mut brep, ())| {
        // (not upstream: the port's cached topology fields, true for the caller)
        brep.refresh_cached_topology();
        brep
    })
}

/// The start, side and end faces of a curve sweep.
type SweepFaces = (Vec<FaceId>, Vec<FaceId>, Vec<FaceId>);

/// A curve sweep into a new B-rep with its face lists.
fn curve_sweep(
    k: &mut Kernel,
    body: impl FnOnce(&mut PrimitiveCreation, &mut PrimitiveModel<'_>, &mut SweepFaces) -> Result<()>,
) -> Result<CurveSweep> {
    let (mut brep, (start_faces, side_faces, end_faces)) =
        PrimitiveModel::build_new(k, |pc, model| {
            let mut faces = SweepFaces::default();
            body(pc, model, &mut faces)?;
            Ok(faces)
        })?;
    // (not upstream: the port's cached topology fields, true for the caller)
    brep.refresh_cached_topology();
    Ok(CurveSweep {
        brep,
        start_faces,
        side_faces,
        end_faces,
    })
}

/// The `SmBSplineCurve` a path or scale curve is (upstream's parameter type): a
/// B-spline, or the B-spline part of a line or conic derived from it.
fn bspline_of(curve: &Curve, op: &'static str) -> Result<BSplineCurve> {
    match curve.clone().into_kind() {
        CurveKind::BSpline(b) => Ok(b),
        CurveKind::Line(l) => Ok(l.base_with_nurb()?.into_owned()),
        CurveKind::Ellipse(e) => Ok(e.base().clone()),
        CurveKind::Hyperbola(h) => Ok(h.base().clone()),
        CurveKind::Parabola(p) => Ok(p.base().clone()),
        _ => Err(SolidError::invalid_input(
            op,
            "curve is not an SmBSplineCurve",
        )),
    }
}

/// The path, scale-reference and scale curves of a face or edge curve sweep as
/// `SmBSplineCurve`s.
fn sweep_curves(
    path: &Curve,
    scale_reference: Option<&Curve>,
    scale: Option<&Curve>,
    op: &'static str,
) -> Result<(BSplineCurve, Option<BSplineCurve>, Option<BSplineCurve>)> {
    Ok((
        bspline_of(path, op)?,
        scale_reference.map(|c| bspline_of(c, op)).transpose()?,
        scale.map(|c| bspline_of(c, op)).transpose()?,
    ))
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

    #[test]
    fn consuming_sweeps_copy_then_run_or_defer() {
        let mut k = Kernel::default();
        let (c, l, h) = (circle(), line(), hermite());
        let ok = [Some(&c), Some(&l)];
        let z = Vec3::new(0.0, 0.0, 1.0);
        let o = Point3::new(0.0, 0.0, 0.0);
        // the copies are made and the sweep runs (topology.brep_p4 is ported)
        let b = create_linear_sweep(&mut k, &ok, z, 10.0, true).expect("linear sweep");
        assert_eq!(
            (
                b.face_list().len(),
                b.edge_list().len(),
                b.vertex_list().len()
            ),
            (2, 7, 6)
        );
        // no profile curves: a successful empty sweep, as upstream
        let b = create_linear_sweep(&mut k, &[], z, 10.0, true).expect("empty sweep");
        assert!(b.face_list().is_empty());
        assert!(b.edge_list().is_empty());
        assert!(b.vertex_list().is_empty());
        assert!(invalid(&create_linear_sweep(
            &mut k,
            &[Some(&c), None],
            z,
            1.0,
            false
        )));
        // the circle around the axis sweeps a surface degenerate to a curve
        assert!(invalid(&create_rotational_sweep(
            &mut k, &ok, o, z, 90.0, false
        )));
        // the draft runs through the taper's merges (boolean.merge_p2 is ported)
        let b = create_draft_sweep(&mut k, &ok, 10.0, -5.0, true, 3).expect("draft sweep");
        assert_eq!(
            (
                b.face_list().len(),
                b.edge_list().len(),
                b.vertex_list().len()
            ),
            (2, 3, 2)
        );
        // no curves: CreateTaperExtrude's SM_ERR
        assert!(
            create_draft_sweep(&mut k, &[], 10.0, -5.0, false, 3).is_err_and(|e| !e.is_unported())
        );
        assert!(invalid(&create_draft_sweep(
            &mut k,
            &[None],
            10.0,
            5.0,
            false,
            0
        )));
        // SM_ERR of the Hermite curve's base Copy, before the later NULL is seen
        let err =
            create_rotational_sweep(&mut k, &[Some(&h), None], o, z, 90.0, false).unwrap_err();
        assert!(matches!(
            err,
            SolidError::Upstream {
                code: crate::error::SM_ERR,
                ..
            }
        ));
        assert!(invalid(&create_taper_extrude(
            &mut k,
            &[],
            10.0,
            5.0,
            3,
            false
        )));
        assert!(invalid(&create_taper_extrude(
            &mut k,
            &[Some(&l), None],
            10.0,
            5.0,
            3,
            false
        )));
        let b = create_taper_extrude(&mut k, &ok, 10.0, 5.0, 3, true).expect("taper extrude");
        assert_eq!(
            (
                b.face_list().len(),
                b.edge_list().len(),
                b.vertex_list().len()
            ),
            (2, 3, 2)
        );
        assert!(invalid(&create_linear_sweep_with_repetitions(
            &mut k,
            &[],
            z,
            5.0,
            2,
            false
        )));
        // capped repeated sweeps need exactly one repetition: SM_ERR
        assert!(
            create_linear_sweep_with_repetitions(&mut k, &ok, z, 5.0, 0, true)
                .is_err_and(|e| !e.is_unported())
        );
        assert!(invalid(&create_rotational_sweep_with_repetitions(
            &mut k,
            &[],
            o,
            z,
            5.0,
            2,
            false
        )));
        assert!(invalid(&create_rotational_sweep_with_repetitions(
            &mut k, &ok, o, z, 5.0, 2, false
        )));
    }

    #[test]
    fn pipe_and_planar_path_validate_then_run_or_defer() {
        let mut k = Kernel::default();
        let p = path();
        assert!(invalid(&create_pipe_sweep(&mut k, 1.0, None, true)));
        assert!(invalid(&create_pipe_sweep(&mut k, 0.0, Some(&p), true)));
        assert!(invalid(&create_pipe_sweep(&mut k, -1.0, Some(&p), false)));
        // a NaN radius passes the API's validation; upstream returns an empty B-rep
        let b = create_pipe_sweep(&mut k, f64::NAN, Some(&p), false).expect("nan pipe");
        assert!(b.face_list().is_empty());
        assert!(b.edge_list().is_empty());
        assert!(b.vertex_list().is_empty());
        let b = create_pipe_sweep(&mut k, 1.0, Some(&p), false).expect("pipe");
        assert_eq!(b.face_list().len(), 1);
        let capped = create_pipe_sweep(&mut k, 1.0, Some(&p), true).expect("capped pipe");
        assert!(crate::topology::brep::p2::is_manifold_solid(&capped).unwrap());

        let (c, l) = (circle(), line());
        assert!(invalid(&create_sweep_along_planar_path(
            &mut k,
            &[],
            &[&l],
            true,
            3
        )));
        assert!(invalid(&create_sweep_along_planar_path(
            &mut k,
            &[&c],
            &[],
            true,
            3
        )));
        assert!(invalid(&create_sweep_along_planar_path(
            &mut k,
            &[&c],
            &[&l],
            true,
            -1
        )));
        assert!(invalid(&create_sweep_along_planar_path(
            &mut k,
            &[&c],
            &[&l],
            true,
            4
        )));
        for caps in 0..=3 {
            let result = create_sweep_along_planar_path(&mut k, &[&c], &[&l], false, caps)
                .expect("planar path");
            let cap_count = (caps & 1) + ((caps >> 1) & 1);
            assert_eq!(
                (
                    result.face_list().len(),
                    result.edge_list().len(),
                    result.vertex_list().len()
                ),
                (1 + cap_count as usize, 3, 2)
            );
            assert_eq!(
                crate::topology::brep::p2::is_manifold_solid(&result).unwrap(),
                caps == 3
            );
        }
    }

    #[test]
    fn curve_sweeps_validate_then_run_or_defer() {
        let mut k = Kernel::default();
        let p = path();
        let c = circle();
        let opts = SweepOptions {
            cap_ends: true,
            ..SweepOptions::default()
        };
        assert!(invalid(&create_curve_sweep(
            &mut k,
            &[],
            Some(&p),
            None,
            None,
            None
        )));
        assert!(invalid(&create_curve_sweep(
            &mut k,
            &[Some(&c)],
            None,
            None,
            None,
            None
        )));
        let swept = create_curve_sweep(&mut k, &[Some(&c)], Some(&p), Some(&p), None, Some(&opts))
            .expect("capped curve sweep");
        assert!(crate::topology::brep::p2::is_manifold_solid(&swept.brep).unwrap());
        // no B-spline profile left once the NULL entry is skipped
        assert!(invalid(&create_curve_sweep(
            &mut k,
            &[None],
            Some(&p),
            None,
            None,
            None
        )));
        assert!(invalid(&create_curve_sweep_from_faces(
            &mut k,
            &[],
            Some(&p),
            None,
            None,
            None
        )));
        assert!(invalid(&create_curve_sweep_from_edges(
            &mut k,
            &[],
            Some(&p),
            None,
            None,
            None
        )));
    }
}
