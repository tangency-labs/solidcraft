//! `SmEdgeuse`, part 3: UV trim curve creation (`CreateUVTrimCurve`,
//! `GetOrCreateUVTrimCurve`), the binormal of an edge use (`EvaluateBinormal`) and the
//! normalised evaluation (`NormalizedEvaluate`).
//!
//! Upstream: `source/SMLib/src/SmEdgeuse.cpp`, `source/SMLib/inc/SmEdgeuse.h`.
//!
//! | Rust                                              | Upstream                                  |
//! |---------------------------------------------------|-------------------------------------------|
//! | [`create_uv_trim_curve`], [`CreatedUvTrimCurve`]  | `SmEdgeuse::CreateUVTrimCurve`            |
//! | [`get_or_create_uv_trim_curve`], [`UvTrimCurveFetch`] | `SmEdgeuse::GetOrCreateUVTrimCurve` (both overloads) |
//! | [`evaluate_binormal`], [`BinormalOptions`]        | `SmEdgeuse::EvaluateBinormal`             |
//! | [`normalized_evaluate`], [`NormalizedEval`]       | `SmEdgeuse::NormalizedEvaluate`           |
//! | [`calc_sector_angle`]                             | `SmEdgeuse::CalcSectorAngle` (no unit owns it; ported for the fork's `IsKnifeEdge`) |
//!
//! Deviations:
//! - **Proved trim raise (#18166).** A lazily built UV trim raises its edge by the trim's
//!   proved distance ([`trim_raise_distance`]), not upstream's sampled one, unless the kernel
//!   is running a file import ([`Kernel::with_sampled_trim_raise`]), when it is upstream's.
//! - **Object model.** As [`super::p1`]: an edge use is a record in the [`Brep`]'s
//!   arenas and the methods are free functions taking the B-rep and its id. Upstream
//!   casts the `const` away to attach UV trim curves and to edit the edge, so these
//!   functions take `&mut Brep` (and the kernel, which the surface evaluators need).
//!   `GetOrCreateUVTrimCurve` returns what it did; the curve is
//!   [`super::p1::uv_trim_curve`] afterwards.
//! - **Null pointers.** Where upstream dereferences a null pointer (an edge use
//!   without face use, face, surface, edge, edge curve, primary edge use or start
//!   vertex) the port returns [`SolidError::InvalidInput`]; `NER` is
//!   [`SolidError::null_pointer`], `SER` the callee's error.
//! - **Surface owner.** `CreateUVTrimCurve` clears the surface's owner
//!   (`SmTemporaryChangeValue`) for the rest of the call, so the solvers do not trim
//!   to the face; the port works on a copy of the face's surface whose owner is
//!   cleared, and clears the owner of the B-rep's face surface too, recording the
//!   scope on the B-rep ([`Brep::begin_surface_owner_cleared`]) so that a nested
//!   `smsurf_GetSurfaceCache` of that surface (`EvaluateBinormal`'s, the seam test's)
//!   finds no face and does not run `SmFace::CreateUVTrimCurves`, as upstream.
//!   `EvaluateBinormal` clears it only around `GlobalPointSolve` and
//!   `IsOnSeam` (a copy there); every other surface call uses the face's surface as it
//!   is. The surface cache flags `EvaluateBinormal` switches (`m_bPointTestEnabled`,
//!   `m_bProcessBoundaryCurves`) have nothing to act on because the Rust surface
//!   solvers build no surface cache. `EvaluateBinormal` first gets the surface cache of
//!   the face's surface (`smsurf_GetSurfaceCache`), whose construction creates the UV
//!   trim curves of the face's edge uses (`SmFace::CreateUVTrimCurves`): the port
//!   builds the face's trimmed surface cache over the B-rep
//!   ([`crate::query::face::p2::face_surface_cache`], the `&mut Brep` the function
//!   already takes) for that side effect and drops it. Upstream keeps the cache on the
//!   surface; the port has no object caches, so it is rebuilt on every call that
//!   reaches this point. The same
//!   side effect makes `CreateUVTrimCurve` clean up a UV trim curve the cache attached;
//!   the clean-up is kept.
//! - **Out-params.** `CreateUVTrimCurve` returns a [`CreatedUvTrimCurve`]; its optional
//!   `pdMaxDeviation` is `want_deviation`. `GetOrCreateUVTrimCurve` returns its status
//!   as [`UvTrimCurveFetch::status`] (upstream's status can be a failure while the
//!   curve pointer is valid, or null) and fails with `Err` only for [`SolidError::Unported`],
//!   [`SolidError::InvalidInput`] and [`SolidError::NotFound`]. `EvaluateBinormal`
//!   always computes the optional outputs, except the parameter direction
//!   ([`BinormalOptions::want_uv_vector`]), which is a separate surface evaluation.
//! - **Drop-curve records.** The surface and curve handles in the record are the
//!   face's and edge's ids (never dereferenced). `SM_SET_CREATE_UVTRIMCURVE_FAIL` is
//!   the record `Set` with the macro's arguments.
//! - **Statuses ignored upstream** (the surface and curve edits of
//!   `CreateUVTrimCurve`, `Trim` and `EditParameterization` aside, which are `SER`):
//!   `GetEnds`, `CalculateBoundingBox`, `ConvertTo2D`, `Transform`, `SetCanonical`,
//!   `Trim`, `EvaluatePoint` of the surface, `DropPoint` and the end evaluations of the
//!   two-singularity recovery, the `NormalizedEvaluate` of the seam test, the
//!   `GetOrCreateUVTrimCurve` and `Evaluate1stDerivatives`/`ComputeParamDirection` of
//!   `EvaluateBinormal`/`NormalizedEvaluate` are ignored; [`SolidError::Unported`]
//!   still propagates.
//! - **Upstream defects.** The two-singular-end-points recovery builds the joined UV
//!   curve into the temporary second-half array, which is destroyed unused, so it never
//!   produces a curve (`plan/upstream-issues.md` WR-158); the port builds and discards
//!   it as upstream does. `NormalizedEvaluate` returns `SM_SUCCESS` for a point it
//!   dropped to the surface even where upstream's trim curve was missing.
//! - **Drop curves.** `SmSurface::DropCurve` is wired (`intersect.surface_drop_lib_p3`;
//!   a plane runs its own override): the UV curves keep their kind (`Vec<Curve>`),
//!   and only the two-singular-end-points stitch takes their `SmBSplineCurve` parts.
//! - **Unported callees.** The paths of
//!   `SmSurface::DropPoint` / `LocalPointSolve` that reach `LocalPointSolve_1`
//!   (`surface.surface_point_solve_p3`) return [`SolidError::Unported`];
//!   `GlobalPointSolve` of a non-plane surface is `surface.surface_point_solve_p3`
//!   inside.
//! - **Recursion.** None: `EvaluateBinormal` calls `CreateUVTrimCurve` and
//!   `CreateUVTrimCurve` calls `EvaluateBinormal` only with `bUse3DOnly = TRUE`, so
//!   the depth is bounded by one level (as upstream comments).
//! - **Trim fit tolerance** (not upstream): `CreateUVTrimCurve` fits at the edge
//!   tolerance, except where that tolerance only covers the edge's edge/face gap (at
//!   most the gap gain times it): there it fits at the healer's tolerance for that gap
//!   without the edge/edge term, capped by the edge tolerance (`fit_tolerance`).
//!   A tolerance raised to cover a gap so never loosens a trim fitted later. The
//!   other edge uses' trims (`trim_other_uv_curves`, upstream `GetZoneTol3d` of the
//!   curve, the edge tolerance) snap to knots within the same value. The
//!   fillet's solver takes its approximation tolerance from the same value
//!   (`blend::fillet_executive` `SetFilletParameters`). The edge is told apart by its
//!   tolerance's value, not by what set it: a tolerance another operation set inside
//!   `(max(def, face tolerance), zone_tol3d(gain * g)]` takes the same tighter fit,
//!   which is never above the edge tolerance. Where the healer's edge/edge term
//!   dominated, the fit is below the tolerance the edge had before the raise. Cost: each
//!   call walks the edge's faces and, above that baseline with no gap cached, computes
//!   the uses' edge/face gaps without caching them.
//! - **Periodic projection.** The point-drop interpolation lifts consecutive UV samples
//!   across closed-surface seams before fitting. Interpolating their wrapped coordinates
//!   instead invents a whole-period excursion between nearby points on the surface. This
//!   continuous lift is an intermediate chart: import assembly completes the face's seams
//!   with `face::p1::complete_imported_face_seams` before publishing its stored trims.
//! - **Passed-on statuses (WR-663).** The statuses the fork passes on
//!   (tangency-labs/smlib 666242b: `CreateUVTrimCurve`'s box, seam transforms, 2D
//!   conversions, trims, end evaluations and drops, `NormalizedEvaluate`;
//!   `EvaluateBinormal`'s `GetOrCreateUVTrimCurve`, derivatives, normal and
//!   `ComputeParamDirection`; `NormalizedEvaluate`'s derivatives) are returned instead of
//!   ignored, as upstream does. `GetEnds` stays ignored, as in the fork.
//! - **Stitch covers (#18166, not upstream).** `cover_joined_faces` raises a stitched
//!   edge's tolerance to the proved distance of its curve from the faces a join gave it
//!   with no trim there, at the end of the stitch operation; `covered_trimless_faces` and
//!   `keep_trimless_faces_covered` (crate-private) keep that cover across `StitchAndOrient`'s
//!   orientation; `settle_face_bounds` (private) re-bounds the faces at the raised
//!   tolerance for at most `SETTLE_ROUNDS` rounds.

use crate::curve::crv_on_surf::CrvOnSurf;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::topology::TopologyFailure;
use crate::error::{ignored, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::geom_utility::p1::vec_lin_comb_two_vectors;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::object::ObjectRef;
use crate::geom::solution_array::ObjectKind;
use crate::geom::tol::{
    approx_tol_3d, are_same, brep_calc_edge_zone_tol_3d, gap_gain, xsect_tol_3d_from_zones,
    zone_tol3d, SmallTopoType,
};
use crate::geom::vector2d::Vec2;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_curve::p2::BoundingBoxRequest;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::crv_fit::p1::ParamMethod;
use crate::surface::surface::p2::{CreateUvTrimCurvePathType, DropCurveFail, DropCurveFailArgs};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1::{
    are_seam_curves, brep_handle, calc_dist_and_dev, delete_uv_trim_curve, drop_curve_fail, edge,
    faceuse, faceuse_face, mate, max_edge_face_gap, orientation, primary_edgeuse,
    set_uv_trim_curve, tolerance, uv_trim_curve, uv_trim_curve_pointer, UvTrimCurveArg,
};
use crate::topology::edgeuse::p2::BinormalEval;
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, ObjectHandle};

const OP: &str = "topology::edgeuse::p3";

// `SM_SUCCESS` (`SmMessages.h`).
use crate::error::SM_SUCCESS;

// `SM_ERR_UNKNOWN`.
use crate::error::SM_ERR_UNKNOWN;

// `UNSURE` of `SmBoolean`.
use crate::error::SM_UNSURE as UNSURE;

fn null(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// The status of `SmVector3d::AngleBetween` of a zero-length vector, which upstream
/// checks with `SER`: `SM_ERR_INVALID_INPUT` (1007), a failing status, not a null
/// dereference.
fn vec_status<T>(r: Result<T>) -> Result<T> {
    r.map_err(|e| match e {
        // the vector operation's own refusal of a zero-length vector is kept as the cause
        e @ SolidError::InvalidInput { .. } => Failure::new(OP, TopologyFailure::ZeroLengthVector)
            .caused_by(e)
            .into_error(),
        // not chained: a port invariant kept as the cause would make this status, which
        // upstream answers with, a port failure (`SolidError::is_port_failure`)
        SolidError::PortInvariant { .. } => {
            SolidError::topology(OP, TopologyFailure::ZeroLengthVector)
        }
        e => e,
    })
}

// ---- unported callees -------------------------------------------------------------------

/// Not upstream (tangency-labs/solidcore#3399): the share of its fit tolerance a trim that
/// would raise its edge's tolerance is fitted again at ([`get_or_create_uv_trim_curve`]).
const REFIT_SHARE: f64 = 1e-3;

/// What `SmSurface::DropCurve` produced.
#[derive(Debug)]
struct DropCurveOut {
    /// `rdMaxDropToSurf`.
    max_dist_to_surf: f64,
    /// `rdMaxApproxDev`.
    max_approx_dev: f64,
    /// `rUVCurves`: upstream's `SmBSplineCurve*` array, whose elements may be derived
    /// curves (a plane drops a line to an `SmLine`), so they keep their kind.
    uv_curves: Vec<Curve>,
}

/// `SmSurface::DropCurve(ctx, domain, curve, interval, tol, rdMaxDropToSurf,
/// rdMaxApproxDev, rUVCurves, bKeepAllDropCurves, pDropCurveFail)` (virtual, with the
/// default `bCreatingUVTrimCurve = TRUE`): the UV curves the curve over `interval`
/// drops to ([`Surface::drop_curve`]: a plane has its own override, which does not use
/// `pOptDropCurveFail` and passes `NULL` on to the general method; every other kind runs
/// the general one, `intersect.surface_drop_lib_p3`).
fn surface_drop_curve(
    k: &Kernel,
    surface: &Surface,
    domain: &crate::geom::extent2d::Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    approx_tol_3d: f64,
    keep_all_drop_curves: bool,
    record: Option<crate::intersect::surface_drop_lib::p1::DropFailRecord<'_, ObjectHandle>>,
) -> Result<DropCurveOut> {
    let d = surface.drop_curve::<ObjectHandle>(
        k,
        domain,
        curve,
        interval,
        approx_tol_3d,
        keep_all_drop_curves,
        record,
        true,
    )?;
    Ok(DropCurveOut {
        max_dist_to_surf: d.max_drop_to_surf,
        max_approx_dev: d.max_approx_dev,
        uv_curves: d.uv_curves,
    })
}

/// The `SmBSplineCurve` an element of a drop's curve array is (a B-spline, or the
/// B-spline part of a line).
fn b_spline_of(curve: Curve) -> Result<BSplineCurve> {
    match curve.into_kind() {
        CurveKind::BSpline(b) => Ok(b),
        CurveKind::Line(l) => Ok(l.base_with_nurb()?.into_owned()),
        _ => Err(SolidError::invalid_input(
            OP,
            "drop curve is not a B-spline curve",
        )),
    }
}

/// `pSurface->GetSingularities() != SM_SS_NONE` (the virtual, without pole outputs; it
/// returns the singular sides as a bit set, there is no status to test).
fn surface_has_singularities(k: &Kernel, surface: &Surface) -> Result<bool> {
    Ok(surface.get_singularities(k, false, false)?.singular != 0)
}

/// The outputs of `SmSurface::DropPoint`.
#[derive(Debug, Clone, Copy)]
struct DropPointOut {
    /// `rbFoundAnswer`.
    found: bool,
    /// `rUV`.
    uv: Point2,
}

/// `pSurface->DropPoint(point, domain, NULL, bOk, uv, gap, bIsMulti)`
/// (`SM_SO_MINIMIZE`, no guess).
fn surface_drop_point(
    k: &Kernel,
    surface: &Surface,
    point: Point3,
    domain: &crate::geom::extent2d::Extent2d,
) -> Result<DropPointOut> {
    let r = surface.drop_point::<NoObject>(
        k,
        point,
        domain,
        &crate::surface::surface::p5::DropPointOptions::default(),
    )?;
    Ok(DropPointOut {
        found: r.success,
        uv: r.result_uv,
    })
}

/// `pSurface->IsSingularity(uv, eDir)` (the virtual: default tolerance `SM_EFF_ZERO`,
/// surface and point tests).
fn surface_is_singularity(
    k: &Kernel,
    surface: &Surface,
    uv: Point2,
) -> Result<(bool, SurfParamType)> {
    let r = surface.is_singularity(k, uv, EFF_ZERO, false)?;
    Ok((r.singular, r.direction))
}

/// `pSurface->LocalPointSolve(domain, SM_SO_MINIMIZE, point, guess, bFound, sSol)`:
/// whether an answer was found, its UV point and its solution value.
fn surface_local_point_solve(
    k: &Kernel,
    surface: &Surface,
    domain: &crate::geom::extent2d::Extent2d,
    point: Point3,
    guess: Point2,
) -> Result<(bool, Point2, f64)> {
    let r = surface.local_point_solve::<NoObject>(
        k,
        domain,
        SolverOperation::Minimize,
        point,
        guess,
    )?;
    Ok((
        r.found,
        Point2::new(r.solution.start.param(0)?, r.solution.start.param(1)?),
        r.solution.start.solution_value,
    ))
}

/// The status of a callee whose failure upstream tests (`lResult != SM_SUCCESS`):
/// `None` for any failing status, soft [`SolidError::InvalidInput`] and
/// [`SolidError::NotFound`] included (`DropCurve`, `DropAndTrimCurve` and
/// `CreateByJoining` report their bad arguments as statuses);
/// [`SolidError::Unported`] propagates.
fn status<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => Err(e),
        Err(_) => Ok(None),
    }
}

/// `smsurf_GetSurfaceCache(pSurface)` of the surface of face `f` (`NER`), as
/// `EvaluateBinormal` calls it before clearing the surface's owner: the face's trimmed
/// surface cache, kept on the surface, whose construction (when the surface has none
/// yet) attaches a UV trim curve to every edge use of the face
/// (`SmFace::CreateUVTrimCurves`, `SmTrimSrfCache.cpp:209`), built over the B-rep
/// ([`crate::query::face::p2::face_surface_cache`]). The Rust surface solve that
/// follows builds its own cache; only the face's and the surface's state is kept.
fn face_surface_cache(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<()> {
    crate::query::face::p2::note_face_surface_cache(k, b, f)
}

/// `pEdge->SetTolerance(tolerance)` (default arguments).
fn edge_set_tolerance(b: &mut Brep, e: EdgeId, tolerance: f64) -> Result<()> {
    crate::topology::edge::set_tolerance(b, e, tolerance, true, true)
}

// ---- CreateUVTrimCurve ------------------------------------------------------------------

/// The outputs of [`create_uv_trim_curve`].
#[derive(Debug, Clone)]
pub struct CreatedUvTrimCurve {
    /// `rpNewUVCurve`: the edge use's new UV trim curve (not yet attached).
    pub curve: Curve,
    /// `rdMaxDistToSurf`: `MaxDist(DropPt, 3dCurvePt)`, found by sampling; the whole
    /// error (`sqrt(dist^2 + dev^2)`) without `want_deviation`.
    pub max_dist_to_surf: f64,
    /// `*pdMaxDeviation`: `MaxDist(DropPt->SrfNormalLine, 3dCurvePt)` (`Some` with
    /// `want_deviation`).
    pub max_deviation: Option<f64>,
}

/// An `SmObject*` handle (never dereferenced) naming a face's surface or an edge's
/// curve in a drop-curve record.
fn handle(kind: ObjectKind, index: u32) -> ObjectHandle {
    ObjectHandle {
        kind,
        key: crate::geom::attribute::ObjectKey(u64::from(index) + 1),
    }
}

/// The length of the UV curve mapped through the surface (`SmCrvOnSurf::ApproximateLength`
/// over the curve's natural interval with 5 samples).
fn crv_on_surf_length(k: &Kernel, surface: &Surface, uv_curve: &Curve) -> Result<f64> {
    let cos = CrvOnSurf::new(
        k,
        uv_curve.clone_for_evaluation()?,
        surface.clone_for_evaluation()?,
        None,
        0,
    )?;
    let ivl = uv_curve.natural_interval();
    Ok(Curve::from(cos).approximate_length(k, &ivl, 5, false)?.0)
}

/// Builds the other seam's UV curves for a UV curve along a seam: a copy moved by
/// `delta` (`Transform` of an axis at `delta`) and a plain copy, both made 2D.
fn seam_curves(k: &Kernel, uv_curve: &Curve, delta: Vec3) -> Result<Vec<Curve>> {
    let mut other = uv_curve.clone();
    let mut axis = Axis2Placement::default();
    axis.set_canonical(delta, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0))?;
    other.transform(k, &axis, None)?;
    other.convert_to_2d(k)?;

    let mut copy = uv_curve.clone();
    copy.convert_to_2d(k)?;
    Ok(vec![other, copy])
}

/// Not upstream: the tolerance geometry fitted along the edge `e` is made at (upstream:
/// the edge tolerance): [`create_uv_trim_curve`]'s UV trim curves of its edge uses, and
/// the approximation tolerance of a fillet solver along it
/// (`blend::fillet_executive::p2` `SetFilletParameters`). The fit is the edge tolerance,
/// except where that tolerance only covers the edge's gap: at most the gap gain times the
/// largest edge/face gap `g` of the edge's uses, as the healer sets it to cover a gap
/// (`Fix_TolSizes`). The fit is then the healer's own value for the edge without an
/// edge/edge gap, `max(def, max face tolerance, gain * g - def)`
/// ([`brep_calc_edge_zone_tol_3d`]), capped by the edge tolerance. A tolerance raised
/// to cover a gap so never loosens the fit, and the fit is never above upstream's (the
/// edge tolerance). An edge tolerance a read raised to cover a kept UV trim's distance
/// (not upstream, [`crate::topology::TrimCoverRaises`]) is taken as the tolerance before
/// that raise while the edge holds the raised value.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; the unported, cancelled and
/// preparation errors of [`crate::topology::edgeuse::p1::calc_max_edge_face_gap`].
pub(crate) fn fit_tolerance(k: &Kernel, b: &mut Brep, e: EdgeId) -> Result<f64> {
    use crate::topology::edgeuse::p1 as eu1;
    let mut edge_tol = b.edge(e)?.tolerance;
    // (a tolerance a read raised to cover a kept trim's distance, held still, fits as the
    // tolerance before the raise: [`crate::topology::TrimCoverRaises`])
    if let Some(&(before, after)) = b.trim_cover_raises().edges.get(&e) {
        if after.to_bits() == edge_tol.to_bits() {
            edge_tol = before;
        }
    }
    // (the largest face tolerance of the edge, as `Fix_TolSizes` takes it)
    let mut max_face_tol = zone_tol3d(k, 0.0);
    for f in crate::topology::edge::faces(b, e, None)? {
        let face_tol = crate::topology::face::p2::tolerance(b, f)?;
        if face_tol > max_face_tol {
            max_face_tol = face_tol;
        }
    }
    // (no gap can lower the fit below the tolerance the edge has without gaps)
    if edge_tol <= brep_calc_edge_zone_tol_3d(k, 0.0, 0.0, b, Some(max_face_tol)) {
        return Ok(edge_tol);
    }
    // (the cached gap, else the gap calculated without caching it, so the fit leaves
    // the edge uses' gap caches as it found them; a failed calculation, ignored as
    // `GetMaxEdgeFaceGap` ignores it, adds no gap)
    let mut gap = 0.0_f64;
    let mut measured_faces = std::collections::BTreeSet::new();
    for eu in b.edge(e)?.edgeuses.clone() {
        let g = if !eu1::has_edge_face_gap(b, eu)? {
            None
        } else if eu1::is_max_edge_face_gap_init(b, eu)? {
            eu1::cached_max_edge_face_gap(b, eu)?
        } else {
            // Uncached uses of this edge on one face measure the same curve over
            // the same interval against the same surface, including at a seam.
            let face = eu1::face(b, eu)?;
            if measured_faces.contains(&face) {
                continue;
            }
            let measured = ignored(eu1::calc_max_edge_face_gap(k, b, eu, false))?;
            if measured.is_some() {
                measured_faces.insert(face);
            }
            measured
        };
        if let Some(g) = g {
            if g.gap_3d > gap {
                gap = g.gap_3d;
            }
        }
    }
    // (NaN compares false: the edge tolerance stays the fit)
    if edge_tol <= zone_tol3d(k, gap_gain(k) * gap) {
        let healed = brep_calc_edge_zone_tol_3d(k, 0.0, gap, b, Some(max_face_tol));
        if healed < edge_tol {
            return Ok(healed);
        }
    }
    Ok(edge_tol)
}

/// `CreateUVTrimCurve`: builds the UV trim curve of an edge use by dropping the edge's
/// curve onto the face's surface. Tries, in order: the UV curve of an edge curve that
/// is a curve on this surface (a copy; two copies for a seam); `DropCurve` and
/// `DropAndTrimCurve` at the edge tolerance and again at a larger one (the larger of
/// twice the tolerance and twice the largest edge/face gap, raising the edge
/// tolerance to the error found), with the singularity recoveries; then 101 points
/// dropped to the surface and interpolated. Two UV curves are reduced to one (seams:
/// by the binormal; crossings: by the 1% / 10% rules), the edge or the UV curve is
/// trimmed when their parameter ranges differ by less than the vertex intersection
/// tolerance. `want_deviation` is `pdMaxDeviation != NULL`; `opt_drop_curve_fail` is
/// `pOptDropCurveFail`.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] where
/// upstream dereferences null; `SM_ERR` when every attempt fails (no sample point
/// drops); the cascading errors of the evaluators; [`SolidError::Unported`] (module
/// docs).
pub fn create_uv_trim_curve(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    want_deviation: bool,
    opt_drop_curve_fail: Option<&mut DropCurveFail<ObjectHandle>>,
) -> Result<CreatedUvTrimCurve> {
    create_uv_trim_curve_fitted(k, b, eu, want_deviation, opt_drop_curve_fail, None)
}

/// [`create_uv_trim_curve`] with its drops fitted at `fit` where given, in place of the edge's
/// fit tolerance ([`fit_tolerance`]).
#[allow(clippy::too_many_lines)]
fn create_uv_trim_curve_fitted(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    want_deviation: bool,
    opt_drop_curve_fail: Option<&mut DropCurveFail<ObjectHandle>>,
    fit: Option<f64>,
) -> Result<CreatedUvTrimCurve> {
    k.work_counters.add_uv_trim_build();
    // always collect DropCurveData
    let mut local_fail = DropCurveFail::<ObjectHandle>::default();
    let fail: &mut DropCurveFail<ObjectHandle> = match opt_drop_curve_fail {
        Some(f) => f,
        None => &mut local_fail,
    };

    // init output
    fail.un_init();
    fail.brep = brep_handle(b, eu)?;

    // locals
    let e = edge(b, eu)?.ok_or_else(|| null("CreateUVTrimCurve: edge use without edge"))?;
    let fu = faceuse(b, eu)?
        // an edge use of a wire lies on no face: the caller's edge use
        .ok_or_else(|| SolidError::topology(OP, TopologyFailure::EdgeUseNotOnFace))?;
    let f = faceuse_face(b, fu)?.ok_or_else(|| null("CreateUVTrimCurve: face use without face"))?;
    // not upstream: the fit tolerance, the edge tolerance unless that only covers a gap
    let fit_tol = match fit {
        Some(fit) => fit,
        None => fit_tolerance(k, b, e)?,
    };
    let face_rec = b.face(f)?;
    let face_surface = face_rec
        .surface
        .as_ref()
        .ok_or_else(|| null("CreateUVTrimCurve: face without surface"))?
        .clone_for_evaluation()?;
    let face_domain = face_rec.uv_domain;
    let edge_rec = b.edge(e)?;
    let mut curve = edge_rec
        .curve
        .as_ref()
        .ok_or_else(|| null("CreateUVTrimCurve: edge without curve"))?
        .clone_for_evaluation()?;
    // the drops build the edge curve's cache at the edge's tolerance
    // (SmTol::GetApproxTol3d(&m_crCurve) reads pEdge->GetTolerance() through the owner;
    // not upstream: at the fit tolerance, [`fit_tolerance`])
    curve.set_owner_zone_tol_3d(Some(fit_tol));
    let mut edge_domain = edge_rec.interval;
    let uv_trim_at_start = uv_trim_curve(b, eu)?.0.is_some();
    let surface_handle = handle(ObjectKind::Surface, f.index());
    let curve_handle = handle(ObjectKind::Curve, e.index());

    // DropCurve output arguments.
    // If we have a CrvOnSurf for a seam, we will populate uv_curves directly and skip
    // DropCurve
    let mut uv_curves: Vec<Curve> = Vec::new();
    let mut max_dist_to_surf = 0.0_f64;
    let mut max_drop_approx_dev = 0.0_f64;
    let mut max_deviation: Option<f64> = None;
    let mut have_curves = false;

    if let CurveKind::CrvOnSurf(cos) = curve.kind() {
        let base = cos.surface();
        // `pSurface == base || *pSurface == *base` (identity is lost in the port: value
        // equality only) && same orientation as the primary edge use
        if face_surface.approx_eq(k, base)?
            && orientation(b, eu)?
                == orientation(
                    b,
                    primary_edgeuse(b, e)?
                        .ok_or_else(|| null("CreateUVTrimCurve: edge without primary edge use"))?,
                )?
        {
            let uv_curve = cos.uv_curve();
            let surface_uv = face_surface.natural_uv_domain_with_nurb(k)?;
            let tol = tolerance(b, eu)?;

            let (start_pt, _end_pt) = match ignored(uv_curve.ends(k))? {
                Some(ends) => (ends.start, ends.end),
                None => (Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0)),
            };

            let uv_box = uv_curve
                .calculate_bounding_box(
                    k,
                    &uv_curve.natural_interval(),
                    BoundingBoxRequest {
                        normal: true,
                        pseudo: false,
                        polar: false,
                    },
                    true,
                )?
                .normal
                .unwrap_or_default();

            // Checking here to ensure that if this is a seam, we make sure to pick the
            // correct UV line. If this is not a line, then we're good. Then check to
            // see if we are along an edge of the domain for a periodic surface, aka
            // this edgeuse represents a seam.
            let u_length = uv_box.u_interval().length();
            let v_length = uv_box.v_interval().length();

            if u_length < tol
                && ((start_pt.x - surface_uv.u_min()).abs() < tol
                    || (start_pt.x - surface_uv.u_max()).abs() < tol)
                && face_surface.is_periodic(k, &surface_uv, SurfParamType::U)?
            {
                // Must be a seam with constant U, so build the other possible UV curve.
                let other_x = if (start_pt.x - surface_uv.u_min()).abs() < tol {
                    surface_uv.u_max()
                } else {
                    surface_uv.u_min()
                };
                let other_point = Vec3::new(other_x, start_pt.y, 0.0);
                uv_curves = seam_curves(k, uv_curve, other_point - start_pt)?;
                have_curves = true;
                max_dist_to_surf = 0.0;
                if want_deviation {
                    max_deviation = Some(0.0);
                }
            } else if v_length < tol
                && ((start_pt.y - surface_uv.v_min()).abs() < tol
                    || (start_pt.y - surface_uv.v_max()).abs() < tol)
                && face_surface.is_periodic(k, &surface_uv, SurfParamType::V)?
            {
                // Must be a seam with constant V, so build the other possible UV curve.
                let other_y = if (start_pt.y - surface_uv.v_min()).abs() < tol {
                    surface_uv.v_max()
                } else {
                    surface_uv.v_min()
                };
                let other_point = Vec3::new(start_pt.x, other_y, 0.0);
                uv_curves = seam_curves(k, uv_curve, other_point - start_pt)?;
                have_curves = true;
                max_dist_to_surf = 0.0;
                if want_deviation {
                    max_deviation = Some(0.0);
                }
            } else {
                // Not a seam, so just copy the curve
                let mut copy = uv_curve.clone();
                copy.convert_to_2d(k)?;
                // SM_SET_CREATE_UVTRIMCURVE_FAIL(SM_SUCCESS, SM_CTC_FROM_CRVONSURF)
                set_create_fail(
                    k,
                    fail,
                    CreateUvTrimCurvePathType::FromCrvOnSurf,
                    &face_surface,
                    &curve,
                    surface_handle,
                    curve_handle,
                );
                return Ok(CreatedUvTrimCurve {
                    curve: copy,
                    max_dist_to_surf: 0.0,
                    max_deviation: want_deviation.then_some(0.0),
                });
            }
        }
    }

    // Do not trim to Face boundaries: upstream clears the surface's owner for the rest
    // of the call (`SmTemporaryChangeValue`). The port works on a copy, and clears the
    // owner of the face's surface in the B-rep as well (recorded with
    // `begin_surface_owner_cleared`), so that the B-rep reads below
    // (`EvaluateBinormal`'s `smsurf_GetSurfaceCache`, the edge/face gap) see no face,
    // as upstream; it is restored on every return.
    let rescue = crate::topology::face::p2::RescueSurfaceOwner::new(
        b.face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| null("CreateUVTrimCurve: face without surface"))?,
    );
    if let Some(s) = b.face_mut(f)?.surface.as_mut() {
        s.set_owner(None);
    }
    b.begin_surface_owner_cleared(f);
    let result = (|| -> Result<CreatedUvTrimCurve> {
        let mut surface = face_surface;
        let _ = surface.set_owner(None::<ObjectRef>);

        // (not upstream: the fit tolerance, [`fit_tolerance`], for the edge tolerance)
        let mut zone_tol = fit_tol;

        // iter 0 = DropCurve(), 1 = DropAndTrimCurve() only if DropCurve() fails
        // iters 2 and 3 = same with bigger tolerance.
        // DropAndTrimCurve() will find curves that are just slightly too long,
        // extending just a bit across the surface boundary; DropCurve() returns
        // nothing in that case.
        for ii in 0..4_u32 {
            // drop curve to surface
            let mut drop_curve_called = false;
            if !have_curves {
                if ii == 2 {
                    // No success at original tolerance. Try a different tolerance based on
                    // gaps.
                    let gap = max_edge_face_gap(k, b, eu, false)?
                        .ok_or_else(|| null("CreateUVTrimCurve: edge use without gap"))?;

                    // if the gap size says to increase tolerances, try that
                    // else double the tolerance
                    if gap.gap_3d * 2.0 > zone_tol {
                        zone_tol = gap.gap_3d * 2.0;
                    } else {
                        zone_tol *= 2.0;
                    }
                }

                // drop curve to surface
                let mut l_ok;
                if ii == 0 || ii == 2 {
                    drop_curve_called = true;
                    // The status is tested below (`lResult`)
                    let r = status(surface_drop_curve(
                        k,
                        &surface,
                        &face_domain,
                        &curve,
                        &edge_domain,
                        zone_tol,
                        false,
                        Some(crate::intersect::surface_drop_lib::p1::DropFailRecord {
                            fail: &mut *fail,
                            surface: Some(surface_handle),
                            curve: Some(curve_handle),
                        }),
                    ))?;
                    l_ok = r.is_some();
                    if let Some(out) = r {
                        max_dist_to_surf = out.max_dist_to_surf;
                        max_drop_approx_dev = out.max_approx_dev;
                        uv_curves = out.uv_curves;
                    }
                } else {
                    // (ii==1 || ii==3) - Try DropAndTrimCurve branch
                    drop_curve_called = false;
                    let r = status(surface.drop_and_trim_curve::<ObjectHandle>(
                        k,
                        &face_domain,
                        &curve,
                        &edge_domain,
                        zone_tol,
                        false,
                        true,
                        Some(crate::intersect::surface_drop_lib::p1::DropFailRecord {
                            fail: &mut *fail,
                            surface: Some(surface_handle),
                            curve: Some(curve_handle),
                        }),
                    ))?;
                    l_ok = r.is_some();
                    if let Some(out) = r {
                        max_dist_to_surf = out.max_drop_to_surf;
                        max_drop_approx_dev = out.max_approx_dev;
                        uv_curves = out.uv_curves;
                    }
                    fail.brep = brep_handle(b, eu)?;
                }

                // when dropCurve fails and Surface has singularities - it's likely
                // DropCurve started at a surface singularity
                if (!l_ok || uv_curves.is_empty()) && surface_has_singularities(k, &surface)? {
                    // create a temp curve copy with reverse parameterizaton
                    let mut rev_curve = curve.clone();

                    // Have to trim this, otherwise the reversals mess things up. [081031]
                    // (may snap sIvl by tol to existing knots; status ignored upstream,
                    // passed on by the fork)
                    rev_curve.trim(k, &mut edge_domain, TrimMode::Notify)?;
                    let new_ivl = rev_curve.reverse_parameterization(k, &edge_domain)?;

                    // drop reverse curve to surface (DropCurve's default keep-all TRUE)
                    let r = status(surface_drop_curve(
                        k,
                        &surface,
                        &face_domain,
                        &rev_curve,
                        &new_ivl,
                        zone_tol,
                        true,
                        None,
                    ))?;
                    l_ok = r.is_some();
                    if let Some(out) = r {
                        max_dist_to_surf = out.max_dist_to_surf;
                        max_drop_approx_dev = out.max_approx_dev;
                        uv_curves = out.uv_curves;
                    }

                    // when reverse curve dropping works
                    if l_ok && !uv_curves.is_empty() {
                        // reverse array of sUVCurves
                        let mut cur_ivl = new_ivl;
                        for c in &mut uv_curves {
                            let old = cur_ivl;
                            cur_ivl = c.reverse_parameterization(k, &old)?;
                        }
                        fail.create_path_type = CreateUvTrimCurvePathType::ReverseDropRecovery;
                        fail.brep = brep_handle(b, eu)?;
                    }

                    // when dropReverseCurve fails - it might be that both end points sit on
                    // singularities
                    if !l_ok {
                        l_ok = two_singular_end_points(
                            k,
                            b,
                            eu,
                            &surface,
                            &face_domain,
                            &curve,
                            &edge_domain,
                            zone_tol,
                            fail,
                            &mut max_dist_to_surf,
                            &mut max_drop_approx_dev,
                        )?;
                    }
                    let _ = l_ok;
                }
            }

            // If no UVTrimCurves are made, try next iteration.
            if uv_curves.is_empty() {
                continue;
            }

            // Check for the DropCurve methods returning two curves.
            // The only legitimate ways for this to happen are (1) the curve runs along a
            // seam, or (2) it runs across a seam, but only by less than tolerance. The
            // second case can happen (hopefully only) during read-in: other systems create
            // Faces that span seams on closed surfaces; we split such Faces at the seams
            // when reading in.
            let on_seam = {
                let refs: Vec<&Curve> = uv_curves.iter().collect();
                are_seam_curves(&refs, &edge_domain, drop_curve_called)
            };

            // First check for the second case:
            // If either curve is shorter than tolerance, remove it.
            if uv_curves.len() == 2 && !on_seam {
                let edge_tol = b.edge(e)?.tolerance;
                let start_v = crate::topology::edge::start_vertex(b, e)?
                    .ok_or_else(|| null("CreateUVTrimCurve: edge without start vertex"))?;
                let vtx1_tol = b.vertex(start_v)?.tolerance;
                let other_v = crate::topology::edge::other_vertex(b, e, Some(start_v))?;
                let vtx2_tol = match other_v {
                    Some(v) => b.vertex(v)?.tolerance,
                    None => vtx1_tol,
                };

                // Remove only if one is much smaller than the other. [B513]
                // Have a case where they're seam curves, but Face's tol is bigger than the
                // surface.
                let len1 = crv_on_surf_length(
                    k,
                    &surface,
                    uv_curves.first().ok_or_else(|| null("curves"))?,
                )?;
                let len2 = crv_on_surf_length(
                    k,
                    &surface,
                    uv_curves.get(1).ok_or_else(|| null("curves"))?,
                )?;

                if len1 < len2 / 10.0 && len1 < edge_tol + vtx1_tol {
                    uv_curves.remove(0);
                    fail.create_path_type = CreateUvTrimCurvePathType::ShortPieceSeamCrossRecovery;
                    fail.brep = brep_handle(b, eu)?;
                } else if len2 < len1 / 10.0 && len2 < edge_tol + vtx2_tol {
                    uv_curves.remove(1);
                    fail.create_path_type = CreateUvTrimCurvePathType::ShortPieceSeamCrossRecovery;
                    fail.brep = brep_handle(b, eu)?;
                }
            } // end first 2-curve check: short seam crossing

            // If still two curves, do the seam check: have to decide which to keep.
            if uv_curves.len() == 2 && on_seam {
                // Take care of seam curves by choosing the correct projected curve. This
                // should work for both continuous and discontinuous seam curves.
                // (upstream's "failed to choose one seam curve" test after each removal,
                // `size > 1`, is dead code: a removal leaves one curve. When the tangent
                // matches neither U nor V within two degrees both curves stay and fall
                // through to the 10% rule below, which keeps curve 0.)
                pick_seam_curve(
                    k,
                    b,
                    eu,
                    &surface,
                    &face_domain,
                    &edge_domain,
                    &mut uv_curves,
                    fail,
                )?;
            } // end 2 UVCurves check -- looking for seam curves

            // no curves with DropCurve() (possibly curve crossing seam) - try again with
            // DropAndTrimCurve() which splits curves across seams
            if uv_curves.is_empty() {
                continue;
            }

            // If we have more than one then we may have split them because they cross the
            // boundary. Not upstream (WR-543, tangency-labs/solidcore#1383; the fork's
            // tangency-labs/smlib@e36feb1): keep the longest run of pieces that meet end to
            // end, joined, on the edge parameters it was dropped from. Upstream takes a
            // piece within 10% of the edge's size and stretches it onto the whole edge
            // interval, which puts every trim point off its edge point (up to 5 mm). A run
            // shorter than the edge is then kept only as any shorter trim below: when its
            // ends are within tolerance of the edge's vertices.
            if uv_curves.len() > 1 {
                longest_drop_run(k, &surface, zone_tol, &mut uv_curves)?;
                fail.create_path_type = CreateUvTrimCurvePathType::TenPercentSeamCrossRecovery;
                fail.brep = brep_handle(b, eu)?;
            } // end more than 1 UVTrimCurve due to splitting (not seams) check

            // State: sUVCurves.GetSize() == 1
            let uv_ivl = uv_curves
                .first()
                .ok_or_else(|| null("CreateUVTrimCurve: no UV curve"))?
                .natural_interval();

            // when single UVTrimCurve has same param range as edge - return success
            if are_same(uv_ivl.min(), edge_domain.min())
                && are_same(uv_ivl.max(), edge_domain.max())
            {
                let new_curve = uv_curves
                    .pop()
                    .ok_or_else(|| null("CreateUVTrimCurve: no UV curve"))?;
                if fail.create_path_type == CreateUvTrimCurvePathType::Unknown {
                    fail.create_path_type = CreateUvTrimCurvePathType::Okay;
                    fail.brep = brep_handle(b, eu)?;
                }

                // clean up Surface Cache side-effect of attaching 2nd UVTrimCurve
                if !uv_trim_at_start && uv_trim_curve(b, eu)?.0.is_some() {
                    delete_uv_trim_curve(b, eu)?; // TRUE = delete existing UVTrimCurve
                }

                // successfully constructed and selected a UVTrimCurve for this Edgeuse -
                // set output
                finish(
                    b,
                    e,
                    ii,
                    want_deviation,
                    &mut max_dist_to_surf,
                    &mut max_deviation,
                    max_drop_approx_dev,
                )?;
                return Ok(CreatedUvTrimCurve {
                    curve: new_curve,
                    max_dist_to_surf,
                    max_deviation,
                });
            }

            // single UVTrimCurve with shorter param range than edge

            // Don't edit the Edge unless we are keeping the UVCurve.
            let mut edit_uv_min = false;
            let mut edit_uv_max = false;

            // Get the 2D end points of the Drop Curve
            let (uv_start, uv_end) = match ignored(
                uv_curves
                    .first()
                    .ok_or_else(|| null("CreateUVTrimCurve: no UV curve"))?
                    .ends(k),
            )? {
                Some(ends) => (ends.start, ends.end),
                None => (Vec3::UNDEF, Vec3::UNDEF),
            };
            let uv_start_2d = Point2::new(uv_start.x, uv_start.y);
            let uv_end_2d = Point2::new(uv_end.x, uv_end.y);

            // Get the 3D end points of the Drop Curve
            let drop_curve_start_3d = surface.evaluate_point(k, uv_start_2d)?;
            let drop_curve_end_3d = surface.evaluate_point(k, uv_end_2d)?;

            // Get the local XSect Tolerances
            // (`SmTol::GetXSectTol3d(pEdge, pVertex)` takes the system zone tolerance for a
            // missing vertex; a missing vertex is dereferenced only where that end's
            // parameter differs)
            let (start_vertex, end_vertex) = crate::topology::edge::start_end_vertices(b, e)?;
            let edge_tol = b.edge(e)?.tolerance;
            let vertex_zone_tol = |v: Option<crate::topology::VertexId>| -> Result<f64> {
                match v {
                    Some(v) => Ok(b.vertex(v)?.tolerance),
                    None => Ok(crate::geom::tol::zone_tol_3d(k, SmallTopoType::NormalSize)),
                }
            };
            let start_xsect_tol =
                xsect_tol_3d_from_zones(k, edge_tol, vertex_zone_tol(start_vertex)?);
            let end_xsect_tol = xsect_tol_3d_from_zones(k, edge_tol, vertex_zone_tol(end_vertex)?);

            // when Edge/UVCurve min params differ
            if !are_same(uv_ivl.min(), edge_domain.min()) {
                let start_vertex = start_vertex
                    .ok_or_else(|| null("CreateUVTrimCurve: edge without start vertex"))?;
                if b.vertex(start_vertex)?.point.distance(drop_curve_start_3d) < start_xsect_tol {
                    edit_uv_min = true;
                } else {
                    // Edge StartParam differs by more than the intersection tolerance:
                    // curve may have stepped off the surface. Do not accept this uv curve.
                    uv_curves.clear();
                    continue;
                }
            }

            // when Edge/UVCurve max params differ
            if !are_same(uv_ivl.max(), edge_domain.max()) {
                let end_vertex =
                    end_vertex.ok_or_else(|| null("CreateUVTrimCurve: edge without end vertex"))?;
                if b.vertex(end_vertex)?.point.distance(drop_curve_end_3d) < end_xsect_tol {
                    edit_uv_max = true;
                } else {
                    // Curve may have stepped off the surface - try again with larger
                    // tolerance.
                    uv_curves.clear();
                    continue;
                }
            }

            // We are keeping the curve, edit as necessary
            if edit_uv_min {
                // Sometimes the uv curve can actually come back longer, due to noise. In
                // that case, trim the uv curve.
                if uv_ivl.min() < edge_domain.min() {
                    let mut new_ivl = Extent1d::from_min_max(edge_domain.min(), uv_ivl.max());
                    if let Some(c) = uv_curves.first_mut() {
                        // may snap sIvl by tol to existing knots
                        c.trim(k, &mut new_ivl, TrimMode::Notify)?;
                    }
                    fail.create_path_type =
                        CreateUvTrimCurvePathType::OnePercentUvTrimCurveTrimRecovery1;
                    fail.brep = brep_handle(b, eu)?;
                } else {
                    // change Edge parameter to match UVTrimCurve parameter
                    let mut new_ivl = Extent1d::from_min_max(uv_ivl.min(), edge_domain.max());
                    crate::topology::edge::set_interval(b, e, &new_ivl)?;
                    edge_domain = b.edge(e)?.interval;
                    fail.create_path_type = CreateUvTrimCurvePathType::OnePercentEdgeTrimRecovery1;
                    fail.brep = brep_handle(b, eu)?;

                    // Also have to update any other Edgeuses on this Edge.
                    trim_other_uv_curves(k, b, e, eu, &mut new_ivl)?;
                }
            }
            if edit_uv_max {
                // Sometimes the uv curve can actually come back longer, due to noise. In
                // that case, trim the uv curve.
                if uv_ivl.max() > edge_domain.max() {
                    let mut new_ivl = Extent1d::from_min_max(uv_ivl.min(), edge_domain.max());
                    if let Some(c) = uv_curves.first_mut() {
                        // may snap sIvl by tol to existing knots
                        c.trim(k, &mut new_ivl, TrimMode::Notify)?;
                    }
                    fail.create_path_type =
                        CreateUvTrimCurvePathType::OnePercentUvTrimCurveTrimRecovery2;
                    fail.brep = brep_handle(b, eu)?;
                } else {
                    // Change Edge parameter to match UVTrimCurve parameter.
                    let mut new_ivl = Extent1d::from_min_max(edge_domain.min(), uv_ivl.max());
                    crate::topology::edge::set_interval(b, e, &new_ivl)?;
                    // (sEdgeDomain = pEdge->GetInterval() is not read again upstream)
                    fail.create_path_type = CreateUvTrimCurvePathType::OnePercentEdgeTrimRecovery2;
                    fail.brep = brep_handle(b, eu)?;

                    // Also have to update any other Edgeuses on this Edge.
                    trim_other_uv_curves(k, b, e, eu, &mut new_ivl)?;
                }
            }

            // State: we have accepted the single UV Trim curve, and probably adjusted the
            // Edge's param range.
            let new_curve = uv_curves
                .pop()
                .ok_or_else(|| null("CreateUVTrimCurve: no UV curve"))?;

            // clean up Surface Cache side-effect of attaching 2nd UVTrimCurve
            if !uv_trim_at_start && uv_trim_curve(b, eu)?.0.is_some() {
                delete_uv_trim_curve(b, eu)?; // TRUE = delete existing UVTrimCurve
            }

            // successfully constructed and set a UVTrimCurve for this Edgeuse
            finish(
                b,
                e,
                ii,
                want_deviation,
                &mut max_dist_to_surf,
                &mut max_deviation,
                max_drop_approx_dev,
            )?;
            return Ok(CreatedUvTrimCurve {
                curve: new_curve,
                max_dist_to_surf,
                max_deviation,
            });
        } // end iters trying DropCurve and DropAndTrimCurve

        // If we make it to here all other attempts have failed to create a curve our last
        // attempt is to tessellate the curve and drop points.
        //
        // It works better to quit here, without doing the brute-force point-dropping
        // algorithm. It's often because the 3d curve runs across a seam, in an incomplete
        // model. In this case, this will create a uv curve that jumps wildly from one end
        // of the domain to the other, rendering it misleading and mostly useless. In such a
        // case, the caller can deal with the lack of a uv trim curve appropriately.
        // [B513 B556 B557] However, there are cases where this is necessary, so we have to
        // leave it in for now.
        brute_force(
            k,
            b,
            eu,
            e,
            &surface,
            &face_domain,
            &curve,
            &edge_domain,
            want_deviation,
            uv_trim_at_start,
            fail,
        )
    })();
    // the temporary change's destructor restores the owner on every return
    b.end_surface_owner_cleared(f);
    if let Some(s) = b.face_mut(f)?.surface.as_mut() {
        rescue.restore(s);
    }
    // the face's tolerance may have changed while the surface had no owner
    b.sync_face_surface_tolerance(f)?;
    result
}

/// `SM_SET_CREATE_UVTRIMCURVE_FAIL(SM_SUCCESS, path)`.
fn set_create_fail(
    k: &Kernel,
    fail: &mut DropCurveFail<ObjectHandle>,
    path: CreateUvTrimCurvePathType,
    surface: &Surface,
    curve: &Curve,
    surface_handle: ObjectHandle,
    curve_handle: ObjectHandle,
) {
    fail.set(&DropCurveFailArgs {
        fail_status: SM_SUCCESS,
        from_call: 3,
        create_path_type: path,
        has_context: true,
        // (kernel-free: a lazy analytic answers its NURB-less base's empty domain)
        uv_domain: surface.natural_uv_domain_if_built(),
        surface: Some(surface_handle),
        curve: Some(curve_handle),
        interval: curve.natural_interval(),
        approx_tol_3d: approx_tol_3d(k, SmallTopoType::NormalSize),
        max_drop_to_surf_arg: 0.0,
        max_approx_dev: 0.0,
        uv_curves_arg: None,
        keep_all_drop_curves: UNSURE,
        degen_curve: UNSURE,
        degen_surf_u: UNSURE,
        degen_surf_v: UNSURE,
        drop_domain_is_contained: UNSURE,
        max_drop_to_surf: 0.0,
        ..DropCurveFailArgs::default()
    });
}

/// The end of a successful `CreateUVTrimCurve`: sets the outputs and, when a larger
/// tolerance was needed (`ii > 1`), the edge tolerance from the actual gaps.
fn finish(
    b: &mut Brep,
    e: EdgeId,
    ii: u32,
    want_deviation: bool,
    max_dist_to_surf: &mut f64,
    max_deviation: &mut Option<f64>,
    max_drop_approx_dev: f64,
) -> Result<()> {
    if want_deviation {
        *max_deviation = Some(max_drop_approx_dev);
    } else {
        // Put the entire error into the one output.
        *max_dist_to_surf = crate::geom::vector3d::sqrt(
            *max_dist_to_surf * *max_dist_to_surf + max_drop_approx_dev * max_drop_approx_dev,
        );
    }

    // If we had to use a bigger tolerance, update the Edge's tolerance according to the
    // actual gaps.
    if ii > 1 {
        let error = if want_deviation {
            crate::geom::vector3d::sqrt(
                *max_dist_to_surf * *max_dist_to_surf + max_drop_approx_dev * max_drop_approx_dev,
            )
        } else {
            *max_dist_to_surf
        };
        edge_set_tolerance(b, e, error * 1.01)?;
    }
    Ok(())
}

/// Trims the UV trim curves of the edge's other edge uses to `new_ivl` (their statuses
/// are ignored; the snapped interval carries over from one to the next).
///
/// Upstream `SmBSplineCurve::Trim` reads `SmTol::GetZoneTol3d(this)`, which walks an
/// attached UV trim curve's `SmEdgeuse` owner to `e` and returns `pEdge->GetTolerance()`
/// (`SM_USE_OLDTOL`), rather than the system zone tolerance that owner-less
/// [`Curve::trim`] reads from `k` (as in `sm_TrimRail`, `blend::fillet_solver::p1`).
/// Not upstream: that tolerance is the fit tolerance ([`fit_tolerance`]), so a tolerance
/// raised to cover the edge's gap does not widen the knot snap either.
fn trim_other_uv_curves(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    this: EdgeuseId,
    new_ivl: &mut Extent1d,
) -> Result<()> {
    let mut tk = Kernel::new(&crate::kernel::KernelOptions {
        tolerances: crate::kernel::Tolerances {
            zone_tol_3d: zone_tol3d(k, fit_tolerance(k, b, e)?),
            ..k.tolerances
        },
        ..crate::kernel::KernelOptions::default()
    });
    tk.seed = k.seed;
    tk.cancel.clone_from(&k.cancel);
    tk.fixes.bspline_trim_preserves_uv_spans = k.fixes.bspline_trim_preserves_uv_spans;
    for x in crate::topology::edge::edgeuses(b, e, None, None)? {
        if x == this {
            continue;
        }
        if uv_trim_curve_pointer(b, x)?.is_none() {
            continue;
        }
        if let Some(c) = b.edgeuse_mut(x)?.uv_trim_curve.as_mut() {
            // may snap sIvl by tol to existing knots
            c.trim(&tk, new_ivl, TrimMode::Notify)?;
        }
    }
    Ok(())
}

/// The recovery for a curve whose both ends drop to singularities: drops the two halves
/// separately and joins them. Upstream leaves the joined curve in the array of the
/// second half, which is destroyed unused (module docs; `plan/upstream-issues.md`
/// WR-158): the result is always "no curves", with `lResult` success when the join worked. Returns `lResult`.
#[allow(clippy::too_many_arguments)]
fn two_singular_end_points(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    surface: &Surface,
    face_domain: &crate::geom::extent2d::Extent2d,
    curve: &Curve,
    edge_domain: &Extent1d,
    zone_tol: f64,
    fail: &mut DropCurveFail<ObjectHandle>,
    max_dist_to_surf: &mut f64,
    max_drop_approx_dev: &mut f64,
) -> Result<bool> {
    // see if both curve ends drop to singularities
    let mut start_singularity = false;
    let mut end_singularity = false;
    let surface_uv_domain = surface.natural_uv_domain_with_nurb(k)?;

    // Curve EndPoints
    let start_point = curve.evaluate_point(k, edge_domain.min())?;
    let end_point = curve.evaluate_point(k, edge_domain.max())?;

    // drop curve endpoints onto surface
    let start_drop = surface_drop_point(k, surface, start_point, &surface_uv_domain)?;
    if start_drop.found {
        start_singularity = surface_is_singularity(k, surface, start_drop.uv)?.0;
        if start_singularity {
            let end_drop = surface_drop_point(k, surface, end_point, &surface_uv_domain)?;
            if end_drop.found {
                end_singularity = surface_is_singularity(k, surface, end_drop.uv)?.0;
            }
        }
    }

    // if both ends map to singulartities
    if !(start_singularity && end_singularity) {
        return Ok(false);
    }

    // get the edge half domains
    let mut first_half = Extent1d::from_min_max(edge_domain.min(), edge_domain.evaluate(0.5));
    let mut second_half = Extent1d::from_min_max(edge_domain.evaluate(0.5), edge_domain.max());

    // create a pair of temp curve copies for each half of the target curve
    let mut first_curve = curve.clone();
    let mut second_curve = curve.clone();

    // trim curves to their half of the original curve (may snap sIvl by tol to
    // existing knots)
    first_curve.trim(k, &mut first_half, TrimMode::Notify)?;
    second_curve.trim(k, &mut second_half, TrimMode::Notify)?;

    // reverse the parameterization of the 1st half
    let new_interval = first_curve.reverse_parameterization(k, &first_half)?;

    // drop the 1st half curve to surface
    let first = status(surface_drop_curve(
        k,
        surface,
        face_domain,
        &first_curve,
        &new_interval,
        zone_tol,
        true,
        None,
    ))?;
    let mut l_result = false;
    // when 1st half dropped successfully - carry on
    if let Some(mut first_out) = first {
        *max_dist_to_surf = first_out.max_dist_to_surf;
        *max_drop_approx_dev = first_out.max_approx_dev;
        if first_out.uv_curves.len() == 1 {
            // drop the 2nd half curve to the surface
            let second = status(surface_drop_curve(
                k,
                surface,
                face_domain,
                &second_curve,
                &second_half,
                zone_tol,
                true,
                None,
            ))?;
            // when 2nd half dropped successfully - carry on
            if let Some(mut second_out) = second {
                *max_dist_to_surf = second_out.max_dist_to_surf;
                *max_drop_approx_dev = second_out.max_approx_dev;
                if second_out.uv_curves.len() == 1 {
                    // restore parameterization of 1stHalf drop curve
                    if let Some(c) = first_out.uv_curves.first_mut() {
                        let old = new_interval;
                        c.reverse_parameterization(k, &old)?;
                    }

                    // stitch the two halves together
                    let second_curve_out = second_out
                        .uv_curves
                        .pop()
                        .ok_or_else(|| null("two singular end points: no second curve"))?;
                    first_out.uv_curves.push(second_curve_out);
                    let mut halves = first_out
                        .uv_curves
                        .into_iter()
                        .map(b_spline_of)
                        .collect::<Result<Vec<BSplineCurve>>>()?;
                    let joined = status(BSplineCurve::create_by_joining(&mut halves, None))?;

                    // when stitching worked (the joined curve is dropped with the
                    // second half's array: upstream defect WR-158, module docs)
                    if joined.is_some() {
                        l_result = true;
                        fail.create_path_type =
                            CreateUvTrimCurvePathType::TwoSingularEndPtsRecovery;
                        fail.brep = brep_handle(b, eu)?;
                    }
                }
            }
        }
    }
    Ok(l_result)
}

/// The seam choice of `CreateUVTrimCurve` for two UV curves along either side of a
/// seam: keeps the one whose side the binormal points to. Where the edge tangent lines
/// up with neither the U nor the V derivative within two degrees both curves remain
/// (the caller falls through to the 10% rule).
#[allow(clippy::too_many_arguments)]
fn pick_seam_curve(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    surface: &Surface,
    face_domain: &crate::geom::extent2d::Extent2d,
    edge_domain: &Extent1d,
    uv_curves: &mut Vec<Curve>,
    fail: &mut DropCurveFail<ObjectHandle>,
) -> Result<()> {
    // Get this edgeuse's Position (3d) near edge's midPoint (status ignored upstream,
    // passed on by the fork).
    let mid_param = if orientation(b, eu)? == OrientType::Same {
        0.48765
    } else {
        1.0 - 0.48765
    };
    normalized_evaluate(k, b, eu, mid_param, false, false, true)?;

    // evaluate NearMid 1st UVCurve Point clamped to face domain (for seams: coincident
    // to sMidPoint, for crossing curves: not). Use the uv curve's domain, not the
    // Edge's. On a seam, it's the same as the Edge's, but if crossing, we need the
    // curve's own midpoint; Edge midpoint might not even be within its own domain.
    // [091030]
    let (uv_pnt1, uv_pnt2) = {
        let c1 = uv_curves.first().ok_or_else(|| null("seam: no curve"))?;
        let c2 = uv_curves
            .get(1)
            .ok_or_else(|| null("seam: no second curve"))?;
        (
            c1.evaluate_point(k, c1.natural_interval().evaluate(0.48765))?,
            c2.evaluate_point(k, c2.natural_interval().evaluate(0.48765))?,
        )
    };
    let uv1 = face_domain.clamp_point(Point2::new(uv_pnt1.x, uv_pnt1.y));
    let uv2 = face_domain.clamp_point(Point2::new(uv_pnt2.x, uv_pnt2.y));

    // project NearMid UVCurve Points to 3Space through Edgeuse->Face->Surface
    let (_p1, du1, dv1) = surface.evaluate_1st_derivatives(k, uv1, true, true)?;
    let (_p2, du2, dv2) = surface.evaluate_1st_derivatives(k, uv2, true, true)?;

    // When there are still 2 UVTrimCurves expect to have 2 UVCurves running down either
    // side of a seam curve. Find the right one for this Edgeuse.
    // bWorkIn3d = TRUE: proj Edge pts to Surface, FALSE: use pUVTrimCurve to calc
    // Surface points. We can't use UVTrimCurves here, because that's what we're
    // creating.
    let mid = evaluate_binormal(
        k,
        b,
        eu,
        edge_domain.evaluate(0.48765),
        true,
        &BinormalOptions::default(),
    )?;
    let (binormal, tangent) = (mid.binormal, mid.tangent);

    // Get angles between Binormal and the facePoints' U and V derivs.
    let ang_bin_u1 = vec_status(du1.angle_between(binormal))?;
    let ang_bin_u2 = vec_status(du2.angle_between(binormal))?;
    let (max_ang_bin_u, min_ang_bin_u) = if ang_bin_u1 > ang_bin_u2 {
        (ang_bin_u1, ang_bin_u2)
    } else {
        (ang_bin_u2, ang_bin_u1)
    };

    let ang_bin_v1 = vec_status(dv1.angle_between(binormal))?;
    let ang_bin_v2 = vec_status(dv2.angle_between(binormal))?;
    let (max_ang_bin_v, min_ang_bin_v) = if ang_bin_v1 > ang_bin_v2 {
        (ang_bin_v1, ang_bin_v2)
    } else {
        (ang_bin_v2, ang_bin_v1)
    };

    // get angles between tangent and the facePoints' U and V tangents
    let mut ang_tan_u1 = vec_status(du1.angle_between(tangent))?;
    let mut ang_tan_v1 = vec_status(dv1.angle_between(tangent))?;
    let mut ang_tan_u2 = vec_status(du2.angle_between(tangent))?;
    let mut ang_tan_v2 = vec_status(dv2.angle_between(tangent))?;

    // The angles with the edge-curve tangent are used only for checking which
    // derivative lines up with the edge. For that, we don't care parallal or
    // anti-parallel, so mirror them about 90 degrees so that 180 degrees maps to 0.
    for a in [
        &mut ang_tan_u1,
        &mut ang_tan_v1,
        &mut ang_tan_u2,
        &mut ang_tan_v2,
    ] {
        if *a > PI / 2.0 {
            *a = PI - *a;
        }
    }

    // The cross-boundary derivative points into the face at the low end of the domain,
    // and out of the face at the high end. The binormal always points into the face. So
    // check whether the CBD and the binormal are in the same or opposite directions.
    // The CBD is the one that does not line up with the curve tangent. We check that
    // here, within two degrees.
    let two_deg = 2.0 * PI / 180.0;

    if ang_tan_v1 < two_deg || ang_tan_v2 < two_deg {
        // V lines up, so U is the cross-boundary deriv (if curve is a seam).
        let keep_first = (PI - max_ang_bin_u < min_ang_bin_u && uv_pnt1.x > uv_pnt2.x) // keep max curve and max curve is 1st
            || (PI - max_ang_bin_u > min_ang_bin_u && uv_pnt1.x < uv_pnt2.x); // keep min curve and min curve is 1st
        remove_seam_curve(b, eu, uv_curves, fail, keep_first)?;
    } else if ang_tan_u1 < two_deg || ang_tan_u2 < two_deg {
        // U lines up, so V is the cross-boundary deriv (if curve is a seam).
        let keep_first = (PI - max_ang_bin_v < min_ang_bin_v && uv_pnt1.y > uv_pnt2.y) // keep max curve and max curve is 1st
            || (PI - max_ang_bin_v > min_ang_bin_v && uv_pnt1.y < uv_pnt2.y); // keep min curve and min curve is 1st
        remove_seam_curve(b, eu, uv_curves, fail, keep_first)?;
    }
    Ok(())
}

/// Removes the 2nd (`keep_first`) or the 1st UV curve of a seam pair.
fn remove_seam_curve(
    b: &Brep,
    eu: EdgeuseId,
    uv_curves: &mut Vec<Curve>,
    fail: &mut DropCurveFail<ObjectHandle>,
    keep_first: bool,
) -> Result<()> {
    if keep_first {
        // remove 2nd UVTrimCurve
        uv_curves.remove(1);
    } else {
        // remove 1st UVTrimCurve
        uv_curves.remove(0);
    }
    fail.create_path_type = CreateUvTrimCurvePathType::PickOneSeamRecovery;
    fail.brep = brep_handle(b, eu)?;
    Ok(())
}

/// The last attempt of `CreateUVTrimCurve`: drops 101 points of the edge curve to the
/// surface and interpolates the sample points that are far enough apart.
#[allow(clippy::too_many_arguments)]
fn brute_force(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    e: EdgeId,
    surface: &Surface,
    face_domain: &crate::geom::extent2d::Extent2d,
    curve: &Curve,
    edge_domain: &Extent1d,
    want_deviation: bool,
    uv_trim_at_start: bool,
    fail: &mut DropCurveFail<ObjectHandle>,
) -> Result<CreatedUvTrimCurve> {
    let mut max_dist = 0.0_f64;
    let mut max_dev = 0.0_f64;

    // locals for GlobalPointSolve
    let mut points: Vec<Point3> = Vec::new();
    let mut params: Vec<f64> = Vec::new();
    let uv_tol = EFF_ZERO_SQRT * (face_domain.size().length() + 1.0);
    let edge_tol = b.edge(e)?.tolerance;
    // Closure uses the kernel's rounding scale, not the fitting tolerance: a narrow
    // open patch does not acquire a period just because the edge has a large gap.
    let natural = surface.natural_uv_domain_with_nurb(k)?;
    let u_period = surface
        .is_closed(k, &natural, SurfParamType::U, None, false)?
        .0
        .then_some(natural.u_interval().length())
        .filter(|p| p.is_finite() && *p > 0.0);
    let v_period = surface
        .is_closed(k, &natural, SurfParamType::V, None, false)?
        .0
        .then_some(natural.v_interval().length())
        .filter(|p| p.is_finite() && *p > 0.0);

    // for 101 points
    for ii in 0..=100_u32 {
        // evaluate the curve at evenly spaced parameter points
        let param = edge_domain.evaluate(f64::from(ii) / 100.0);
        let pv = curve.evaluate(k, param, 1, true, true)?;
        let p0 = *pv.first().ok_or_else(|| null("brute force: no point"))?;

        // find closest point on surface to sample point
        let solutions = surface.global_point_solve::<NoObject>(
            k,
            Some(NoObject),
            face_domain,
            SolverOperation::Minimize,
            p0,
            edge_tol,
            None,
            SolutionRequested::All,
        )?;

        // when a closest point was found
        if let Some(sol) = solutions.as_slice().first() {
            // get the surface UV Point from the solution array
            let mut pnt = Point3::new(sol.start.param(0)?, sol.start.param(1)?, 0.0);
            // Interpolate a continuous lift of the surface parameters. A jump between
            // equivalent seam sides is a change of chart, not motion across the patch.
            if let Some(previous) = points.last() {
                pnt.x = crate::eval::uv::unwrap(pnt.x, previous.x, u_period);
                pnt.y = crate::eval::uv::unwrap(pnt.y, previous.y, v_period);
            }

            // save the first sample Point and subsequent ones that are far enough apart
            if points.last().is_none_or(|l| pnt.distance(*l) > uv_tol) {
                points.push(pnt);
                params.push(param);

                // (the solution's own first object, as `sm_CalcDistAndDev` reads it:
                // upstream every solution of `pTmpSurface->GlobalPointSolve` names that
                // surface, the fast drops `this` and `GlobalPointSolve_1` its branch owner,
                // while the port's solve returns them without objects; name the solved
                // surface again so the distance and deviation are measured, not -1)
                let mut named = *sol;
                if let Some(first) = named.objects.first_mut() {
                    *first = Some(NoObject);
                }
                let (dist, dev) =
                    calc_dist_and_dev(k, p0, &named, |_| Some(surface), want_deviation)?;
                max_dist = max_dist.max(dist);
                // (Note, dMaxDev is 0 if ptr is Null.)
                max_dev = max_dev.max(dev.unwrap_or(0.0));
            }
        }
    }

    let set_outputs = |max_dist: f64, max_dev: f64| -> (f64, Option<f64>) {
        // Set these anyway, could be useful to the caller.
        if want_deviation {
            (max_dist, Some(max_dev))
        } else {
            (
                crate::geom::vector3d::sqrt(max_dist * max_dist + max_dev * max_dev),
                None,
            )
        }
    };

    // when no sample points projected to the surface
    if params.is_empty() {
        return Err(SolidError::topology(OP, TopologyFailure::NoSampleOnSurface));
    }

    // when just 1 sample point projected to the surface
    if params.len() == 1 {
        // create and use a degenerate curve as the UVTrimCurve
        let first = *points
            .first()
            .ok_or_else(|| null("brute force: no point"))?;
        let degen = BSplineCurve::create_degenerate_curve(2, first)?;
        fail.create_path_type = CreateUvTrimCurvePathType::PointDropDegenCurveRecovery;
        fail.brep = brep_handle(b, eu)?;

        // clean up Surface Cache side-effect of attaching 2nd UVTrimCurve
        if !uv_trim_at_start && uv_trim_curve(b, eu)?.0.is_some() {
            delete_uv_trim_curve(b, eu)?;
        }

        // successfully constructed a UVTrimCurve
        let (d, dv) = set_outputs(max_dist, max_dev);
        return Ok(CreatedUvTrimCurve {
            curve: Curve::from(degen),
            max_dist_to_surf: d,
            max_deviation: dv,
        });
    }

    // else have several sample points - construct UVTrimCurve to interpolate sample
    // points
    fail.create_path_type = CreateUvTrimCurvePathType::PointDropDegenCurveRecovery;
    fail.brep = brep_handle(b, eu)?;

    // of degree 3, or as high as the points carry: degree p needs p + 1 points, and two
    // give the line through them at their parameters (WR-16922: upstream fits a cubic
    // with two or three control points, which no reader accepts)
    let mut new_curve = if let [_, _] = points.as_slice() {
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 1,
            control_points: &points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[2, 2],
            knots: &params,
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })?
    } else {
        BSplineCurve::interpolate_points(
            &points,
            Some(&params),
            (points.len() - 1).min(3), // in : degree
            None,
            None,
            false,
            ParamMethod::ChordLength,
        )?
    };
    new_curve.convert_to_2d()?;

    // Keep the lift on the edge's parameters until the body-level seam edit can split real
    // boundary edges and replace internal cuts. Wrapping individual samples here would fit
    // across the other side of the surface again; returning one selected piece would lose
    // the rest of the edge. Import assembly completes the topology once all lifts are known.

    // clean up Surface Cache side-effect of attaching 2nd UVTrimCurve
    if !uv_trim_at_start && uv_trim_curve(b, eu)?.0.is_some() {
        delete_uv_trim_curve(b, eu)?;
    }

    // successfully constructed a UVTrimCurve
    let (d, dv) = set_outputs(max_dist, max_dev);
    Ok(CreatedUvTrimCurve {
        curve: Curve::from(new_curve),
        max_dist_to_surf: d,
        max_deviation: dv,
    })
}

// ---- GetOrCreateUVTrimCurve -------------------------------------------------------------

/// The outputs of [`get_or_create_uv_trim_curve`].
#[derive(Debug, Clone)]
pub struct UvTrimCurveFetch {
    /// The returned status: `Ok` when the curve is newly built or a well made existing
    /// one; the failing status when `CreateUVTrimCurve` failed (no curve attached) or
    /// the existing curve's record holds one (the curve is attached).
    pub status: std::result::Result<(), SolidError>,
    /// `*pdOptDistToSurf`: the new curve's `MaxDist(DropPt, 3dCurvePt)`; for an old
    /// curve the largest edge/face gap when it is cached, else `-1`.
    pub dist_to_surf: f64,
    /// `*pdOptDeviation`: the new curve's deviation; `-1` for an old curve.
    pub deviation: f64,
}

/// `GetOrCreateUVTrimCurve`: the UV trim curve of the edge use (or its mate), built and
/// attached first when there is none (and `uv_space_ok`). Afterwards the curve is
/// [`uv_trim_curve`]. With `want_dist` / `want_deviation` the outputs are set as
/// upstream's `pdOptDistToSurf` / `pdOptDeviation` (`-1` when not built).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] where
/// upstream dereferences null; [`SolidError::Unported`] from `CreateUVTrimCurve`. Other
/// failures are in [`UvTrimCurveFetch::status`].
pub fn get_or_create_uv_trim_curve(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    want_dist: bool,
    want_deviation: bool,
    uv_space_ok: bool,
) -> Result<UvTrimCurveFetch> {
    // low work - UVTrimCurve already exists
    if let Some(out) = existing_uv_trim_curve(b, eu, want_dist)? {
        return Ok(out);
    }

    // init outputs
    let mut out = UvTrimCurveFetch {
        status: Ok(()),
        dist_to_surf: -1.0,
        deviation: -1.0,
    };

    // no work - no UVTrimCurve and not allowed to build one for fear of DropCurve
    // failure during healing - return NULL
    if !uv_space_ok {
        return Ok(out);
    }

    // arrive here to build UVTrimCurve: create a new UVTrimCurve: err return value =
    // CreateUVTrimCurve err return value
    let created = match create_uv_trim_curve(k, b, eu, want_deviation, None) {
        Ok(c) => c,
        Err(
            e @ (SolidError::Unported { .. }
            | SolidError::Cancelled { .. }
            | SolidError::InvalidInput { .. }
            | SolidError::PortInvariant { .. }
            | SolidError::NotFound { .. }),
        ) => return Err(e),
        Err(e) => {
            // the failing status
            out.status = Err(e);
            return Ok(out);
        }
    };
    // Not upstream (tangency-labs/solidcore#3399): a trim that would raise its edge is fitted
    // again, tighter ([`refit_raising_trim`]).
    let created = refit_raising_trim(k, b, eu, want_deviation, created)?;

    // Save max edge to projected TrimCurve distances. If one is Null and the other is
    // non-Null, return the whole distance in the non-Null one. (If pdOptDistToSurf is
    // non-Null and pdOptDeviation is Null, CreateUVTrimCurve takes care of that.)
    if want_dist {
        out.dist_to_surf = created.max_dist_to_surf;
        if let Some(d) = created.max_deviation {
            out.deviation = d;
        }
    } else if let Some(d) = created.max_deviation {
        out.deviation = crate::geom::vector3d::sqrt(
            created.max_dist_to_surf * created.max_dist_to_surf + d * d,
        );
    }

    // Fix duplicate UVTrimCurve Problem: in the process of creating a UV trim curve a
    // trimmed surface cache will be retrieved or created for the face's surface to which
    // this Edgeuse is attached. Constructing this trimmmed surface cache may create and
    // attach another UVTrimCurve to this edgeuse rendering our new one redundant.
    //
    // A call to SetUVTrimCurve will delete the duplicate UVTrimCurve (pass a copy of
    // DropCurveFail when it exists)
    let mut target_fail = drop_curve_fail(b, eu)?.0.map(|old| {
        let mut copy = DropCurveFail::default();
        copy.assign(old);
        copy
    });
    // Not upstream (#18162): the edge tolerance follows the trim's proved distance from the
    // edge curve, not only the drop's reported error, whose fit part is the fit tolerance (the
    // edge's own tolerance) and would raise the edge from its own tolerance on every refit.
    //
    // Not upstream (#21105): in a validate-only pass (`Kernel::with_held_trim_raise`, the
    // pass's `bValidateOnly`) the trim raises nothing, as `SetUVTrimCurve` raises nothing on
    // a B-rep marked validate-only; the raise distance is then not wanted, and not proved.
    let raise_by = if k.caches.hold_trim_raise || b.validate_only() {
        0.0
    } else {
        trim_raise_distance(k, b, eu, &created.curve, created.max_dist_to_surf)?
    };
    set_uv_trim_curve(
        b,
        eu,
        UvTrimCurveArg::New(Box::new(created.curve)),
        raise_by,
        true, // TRUE = delete existing UVTrimCurve
        target_fail.as_mut(),
    )?;
    Ok(out)
}

/// Not upstream (tangency-labs/solidcore#3399): `created`, the new trim of `eu`, or the same
/// trim fitted again at [`REFIT_SHARE`] of the fit tolerance where that one lies closer to the
/// edge, when `created` lies far enough from the edge to raise its tolerance
/// (`SetUVTrimCurve` raises a tolerance below twice the distance). The drop's fit error (up to
/// the fit tolerance, the edge's own) is part of that distance, and the edge is raised to
/// cover the trim, not the curve it was fitted from: a line dropped onto a cylinder at 4e-5
/// gave a trim 2.7e-5 from the line where the line is 2.5e-5 from the cylinder, and raised the
/// edge and its two vertices to 5.4e-5 instead of 5.0e-5. A validate-only pass raises nothing
/// (`Kernel::with_held_trim_raise`, `bValidateOnly`), so it fits once; a refit that fails
/// leaves `created`.
///
/// # Errors
///
/// Those of [`fit_tolerance`]; the hard statuses of [`create_uv_trim_curve`].
fn refit_raising_trim(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    want_deviation: bool,
    created: CreatedUvTrimCurve,
) -> Result<CreatedUvTrimCurve> {
    if k.caches.hold_trim_raise || b.validate_only() {
        return Ok(created);
    }
    let Some(e) = edge(b, eu)? else {
        return Ok(created);
    };
    if b.edge(e)?.tolerance >= 2.0 * created.max_dist_to_surf {
        return Ok(created);
    }
    let fit = zone_tol3d(k, REFIT_SHARE * fit_tolerance(k, b, e)?);
    let tight = ignored(create_uv_trim_curve_fitted(
        k,
        b,
        eu,
        want_deviation,
        None,
        Some(fit),
    ))?;
    Ok(match tight {
        Some(t) if t.max_dist_to_surf < created.max_dist_to_surf => t,
        _ => created,
    })
}

/// Not upstream (#18162, #18009): the distance `SetUVTrimCurve` raises an edge's tolerance to
/// cover (to twice it) for the new trim `trim` of `eu`, given upstream's distance `upstream`
/// (the drop's reported error, or what the caller passes): the trim's proved distance from
/// the edge curve ([`trim_deviation_bound`], `B`) wherever it is enclosed. The drop's error
/// counts its fit tolerance, which is the edge's own tolerance, so upstream raises the edge
/// from its own tolerance on every refit, whatever the trim's real distance; `B` bounds that
/// distance, so the raise covers the trim by proof. Where `B` is not enclosed, or `upstream`
/// raises nothing (the tolerance already covers twice it), `upstream`, as upstream.
///
/// Where `B` is above twice `upstream`, upstream's raise (to `2 * upstream`) would leave the
/// edge below a trim it is proved to be farther from: a tolerance below the trim's real
/// distance is invalid, one above it only loose. The raise is then to `B` itself (the distance
/// returned is `B / 2`, which `SetUVTrimCurve` doubles): the tightest tolerance the proof
/// gives. It adds no margin for the edge curve's evaluation rounding: the tolerance it leaves
/// is above upstream's, which counts none either, so it uncovers nothing upstream covers. A
/// lowering below upstream's raise is what must not uncover it, hence the rounding check
/// there (`lowered_covers_rounding`). The bound is the whole bisection's wherever it is above half the tolerance (to its
/// piece and depth limits, not stopped at `upstream`), so a loose enclosure is tightened as
/// far as it goes before the edge is raised to it; it is still a proved bound. Where `B` is
/// from `upstream` to twice it, upstream's raise already covers it, and stays.
///
/// A `B` below `upstream` is taken only where the tolerance it leaves still covers the proved
/// distance of every other UV trim of the edge, and the edge curve's proved distance to the
/// surface of every other face of the edge that holds no trim (`other_trims_bound`);
/// otherwise, or where one of them is not enclosed, `upstream`. A trim set without a raise
/// (read from a file, or attached by an operation that does not raise) may be covered by
/// upstream's larger raise only, and a smaller raise must not uncover it. The tolerance left
/// is `2 * B` below `upstream`, `2 * upstream` up to twice it, and `B` past that within the
/// B-rep check's limit: never decreasing in `B`, and at least `B`, up to that limit (past it,
/// `2 * upstream`, below `B`: the unproved fallback below).
///
/// A `B` above twice `upstream` raises the edge only where the B-rep check accepts the
/// tolerance it leaves ([`within_edge_tolerance_limit`]); past that, upstream's raise. A bound
/// that far above upstream's sampled distance is where the enclosure did not converge (a
/// bisection piece at its deepest whose second-order term does not shrink, or a
/// reparametrisation off the correspondence over a short stretch: `topology.brep_local_operation`
/// fc0000, bounds 0.43 and 8.7e-3 for trims 1.2e-6 from their edges by dense samples); raising
/// the edge to it would leave a body the check refuses, the edit refused with it.
///
/// Not covered: where `upstream` raises nothing, `B` is not computed, and an edge whose trim
/// lies farther than its tolerance keeps it (as upstream); bounding every refit trim there
/// is a follow-up. Nor is a trim whose `B` is past the B-rep check's limit: tightening that
/// enclosure is a follow-up.
///
/// # Errors
///
/// Those of [`trim_deviation_bound`].
pub(crate) fn trim_raise_distance(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    trim: &Curve,
    upstream: f64,
) -> Result<f64> {
    // (`SetUVTrimCurve` raises only a tolerance below twice the distance)
    let Some(e) = edge(b, eu)? else {
        return Ok(upstream);
    };
    let tol = b.edge(e)?.tolerance;
    let raises = tol < 2.0 * upstream;
    // (a file import takes upstream's raise, through [`Kernel::with_sampled_trim_raise`])
    if !raises || !k.caches.prove_lazy_trim_raise {
        return Ok(upstream);
    }
    // (the bounds are as tight as the tolerance the trim is fitted at asks: a tolerance the
    // healer raised to cover a gap leaves them as they are, as it leaves the fit)
    let fit = fit_tolerance(k, b, e)?;
    // (a bound at most half the tolerance raises nothing, whatever its value: the bisection
    // stops tightening it there; any other is the whole bisection's, not stopped at a cutoff,
    // so a loose enclosure is tightened as far as it goes before the edge is raised to it)
    let raise_by =
        match trim_deviation_bound(k, b, eu, trim, fit, [0.5 * tol, f64::INFINITY], None)? {
            Some(bound) if bound.is_finite() => bound,
            _ => return Ok(upstream),
        };
    if raise_by > 2.0 * upstream {
        // UNPROVED fallback (#18166): a `B` the B-rep check refuses as an edge tolerance is
        // the enclosure's limit, not a distance to raise to. The raise is upstream's, from
        // its sampled distance: the edge is not proved to cover the trim.
        if !within_edge_tolerance_limit(b, raise_by) {
            return Ok(upstream);
        }
        // (doubled by `SetUVTrimCurve` back to `B`, exactly: halving a double is exact for
        // any `B` of at least `2 * f64::MIN_POSITIVE` (normal halves), far below a tolerance)
        return Ok(0.5 * raise_by);
    }
    if raise_by >= upstream {
        return Ok(upstream);
    }
    // A smaller raise than upstream's must still leave the edge covering its other trims:
    // a trim set without a raise (read from a file, or attached by an operation that does
    // not raise) may be covered by upstream's larger raise only, and the smaller one must
    // not uncover it. Where the lowered tolerance does not cover each other trim's proved
    // distance (or one is not enclosed), the raise is upstream's.
    let lowered = tol.max(2.0 * raise_by);
    // The lowered tolerance must also cover what an evaluation of the edge curve can observe:
    // `B` plus the evaluation's own rounding where the curve lies (`sqrt 3` times
    // [`Curve::point_error_bound`] at its interval's ends, the larger: a line's grows with its
    // parameter). Where that rounding is not bounded, or the lowered tolerance does not cover
    // it (a curve far from the origin for its bound), the raise is upstream's.
    if !lowered_covers_rounding(b, e, raise_by, lowered)? {
        return Ok(upstream);
    }
    match other_trims_bound(k, b, eu, fit, lowered)? {
        Some(others) if others <= lowered => Ok(raise_by),
        _ => Ok(upstream),
    }
}

/// Not upstream (#18166): whether the tolerance `lowered` covers the proved distance `bound`
/// plus the evaluation rounding of `e`'s curve (`sqrt 3` times the larger
/// [`Curve::point_error_bound`] at its interval's ends, rounded up), as
/// [`trim_raise_distance`] requires of a lowering; `false` without a curve or a bound.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
fn lowered_covers_rounding(b: &Brep, e: EdgeId, bound: f64, lowered: f64) -> Result<bool> {
    let rec = b.edge(e)?;
    let Some(curve) = rec.curve.as_ref() else {
        return Ok(false);
    };
    let (Some(e0), Some(e1)) = (
        curve.point_error_bound(rec.interval.min()),
        curve.point_error_bound(rec.interval.max()),
    ) else {
        return Ok(false);
    };
    // (each of the sum's two roundings and the product's is at most one `EPSILON` relative:
    // `4 EPSILON` covers them, rounding the total up)
    let need = (bound + e0.max(e1) * 3.0_f64.sqrt()) * (1.0 + 4.0 * f64::EPSILON);
    Ok(need.is_finite() && need <= lowered)
}

/// Not upstream (#18166): whether `tolerance` is one the B-rep check accepts for an edge of
/// `b`: below [`LARGE_SMALL_SIZE_RATIO`](crate::geom::tol::LARGE_SMALL_SIZE_RATIO) times the
/// B-rep's tolerance (`ValidatePointers` test 59, the limit the validation's own tolerance
/// growth keeps under). A raise past it leaves a body the check refuses. The stitch's cover
/// raises past it on purpose where the gap is seen that wide ([`cover_joined_faces`]): the
/// edge then covers its face, and the body is one the check refuses for that edge, where
/// upstream's is accepted with the face uncovered.
pub(crate) fn within_edge_tolerance_limit(b: &Brep, tolerance: f64) -> bool {
    tolerance < crate::geom::tol::LARGE_SMALL_SIZE_RATIO * b.tolerance()
}

/// Not upstream (#18162): the largest [`trim_deviation_bound`] of the UV trims of `eu`'s
/// edge other than the one `eu` and its mate share (the one being replaced), and of the edge
/// curve's distance to the surface of each other face of the edge whose uses hold no trim
/// ([`edge_face_gap_bound`]), `0` without any; `None` where one is not enclosed or is above
/// `lowered` (the caller only asks whether they are all within it).
///
/// # Errors
///
/// Those of [`trim_deviation_bound`].
fn other_trims_bound(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    fit: f64,
    lowered: f64,
) -> Result<Option<f64>> {
    let Some(e) = edge(b, eu)? else {
        return Ok(Some(0.0));
    };
    let pair = [Some(eu), mate(b, eu)?];
    let mut worst = 0.0_f64;
    for &x in &b.edge(e)?.edgeuses {
        if pair.contains(&Some(x)) {
            continue;
        }
        let Some(other) = b.edgeuse(x)?.uv_trim_curve.as_ref() else {
            continue;
        };
        match trim_deviation_bound(k, b, x, other, fit, [lowered, lowered], None)? {
            Some(bound) if bound.is_finite() => worst = worst.max(bound),
            _ => return Ok(None),
        }
    }
    // A face of the edge none of whose uses holds a trim (other than `eu`'s, which the new
    // trim covers) is covered only by the edge curve's own distance to its surface: upstream's
    // larger raise may be all that covers it, as for a trim set without a raise (#18166:
    // `primitive.api_primitives` gen0083, `heal.stitch_p2` cov_cube2_do).
    let own = match faceuse(b, eu)? {
        Some(fu) => faceuse_face(b, fu)?,
        None => None,
    };
    let mut faces: Vec<(FaceId, bool)> = Vec::new();
    for &x in &b.edge(e)?.edgeuses {
        let Some(fu) = faceuse(b, x)? else {
            continue;
        };
        let Some(f) = faceuse_face(b, fu)? else {
            continue;
        };
        if Some(f) == own {
            continue;
        }
        let trimmed = b.edgeuse(x)?.uv_trim_curve.is_some();
        match faces.iter_mut().find(|(g, _)| *g == f) {
            Some(slot) => slot.1 |= trimmed,
            None => faces.push((f, trimmed)),
        }
    }
    for (f, _) in faces.into_iter().filter(|(_, trimmed)| !trimmed) {
        match edge_face_gap_bound(k, b, e, f, fit, [lowered, lowered])? {
            Some(gap) if gap.is_finite() && gap <= lowered => worst = worst.max(gap),
            _ => return Ok(None),
        }
    }
    Ok(Some(worst))
}

/// Not upstream (#18166): raises `e`'s tolerance to cover the proved distance of its curve to
/// the surface of each face of `faces` that a stitch's joins gave it (`GlueEdges`: the faces
/// of the edges they deleted), where none of `e`'s uses on that face holds a UV trim (a trim
/// raised the tolerance for its own face when it was set). The stitch runs this when its
/// operation ends, on the final trims, faces and curve
/// ([`crate::heal::stitch::p1::Stitch::finish_joins`]). A join keeps `e`'s curve for the
/// deleted edge's faces and raises the tolerance to the two curves' distance only
/// (`UpdateObjectTolerance`), which does not cover that curve's distance to those faces: the
/// deleted edge's own tolerance covered its own curve there (`heal.stitch_p2` `cov_tjunc8`: a
/// face joined 2.43e-4 from the kept curve under a tolerance of 2.18e-4). A face with no
/// finite bound, or whose bound is within the tolerance, is left as upstream leaves it, and
/// so is one whose bound is past the B-rep check's edge tolerance limit
/// ([`within_edge_tolerance_limit`]) and either more than [`COVER_CONVERGED`] times the largest
/// distance sampled from the edge to its surface ([`sampled_face_gap`]) or not sampled at all
/// (no curve or surface, or a sample not evaluated or dropped): UNPROVED, below. A bound past
/// the limit within that factor of the samples is still a proved raise: the tolerance goes to
/// the proved bound, which covers the face whatever the samples say, and the samples only rule
/// out a bound so loose it is an enclosure that did not converge (a bound the samples let
/// through is still a proved cover, at worst a loose one).
///
/// The tolerance is raised to the face's proved bound only, never to a sample (a raise covers a
/// proved distance, never an assumed one: `primitive.api_primitives` gen0353, an ellipse at
/// `z = 4.5e8` 1.2e-7 from its faces by samples, is bounded far from the origin only at
/// 4.76e4, and to 1e-6 by the enclosure moved to the curve, `query::curve_surface_gap`
/// `recentred_retry`). The join's own distance is no bound: the stitch samples it (five or
/// ten points, none for two lines), and on `cov_tjunc8` it is 0 under a deleted-edge
/// tolerance of 1e-5, where the kept curve is 2.43e-4 from the face.
///
/// After a raise, the faces' own bounds are settled at the raised tolerance
/// ([`settle_face_bounds`]): each bound is cut to a thousandth of the fit, and the fit after
/// the raise is the raised tolerance, at which a bound can come out a rounding above the one
/// it raised to, leaving the edge short of the face by its own measure. The raised edge is
/// then raised to cover its UV trims ([`cover_raised_trims`]) and its vertices the ends of
/// their edges ([`cover_vertex_ends`]). A joined edge no face raised covers its trims after
/// the stitch's orientation, on the trims the B-rep ends with ([`cover_joined_trims`]).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of `curve_face_gap_bound_within`.
pub(crate) fn cover_joined_faces(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    faces: &[FaceId],
) -> Result<()> {
    let mut trimmed: Vec<FaceId> = Vec::new();
    for &x in &b.edge(e)?.edgeuses {
        if b.edgeuse(x)?.uv_trim_curve.is_none() {
            continue;
        }
        if let Some(fu) = faceuse(b, x)? {
            if let Some(f) = faceuse_face(b, fu)? {
                trimmed.push(f);
            }
        }
    }
    let mut done: Vec<FaceId> = Vec::new();
    // (the faces with a proved bound, and whether any face raised)
    let mut settle: Vec<(FaceId, f64)> = Vec::new();
    let mut raised = false;
    for &f in faces {
        if done.contains(&f) || trimmed.contains(&f) {
            continue;
        }
        done.push(f);
        if !crate::topology::edge::faces(b, e, None)?.contains(&f) {
            continue;
        }
        let tol = b.edge(e)?.tolerance;
        let fit = fit_tolerance(k, b, e)?;
        let Some(gap) =
            edge_face_gap_bound(k, b, e, f, fit, [tol, f64::INFINITY])?.filter(|g| g.is_finite())
        else {
            continue;
        };
        // Past the limit (#18166): a bound the B-rep check refuses as an edge tolerance is a
        // proved raise taken only where the edge is seen that far from the face: at least
        // `1 / COVER_CONVERGED` (half) of it at a sample ([`sampled_face_gap`]; a stitch
        // over a real gap, as in `heal.stitch_p1`/`heal.stitch_p2`, where the gaps measured
        // 1.01e-2 to 1.02; the edge then covers
        // the face, and fails the B-rep check's test 59 as
        // [`within_edge_tolerance_limit`] says). Otherwise (the UNPROVED fallback) it is the
        // enclosure not converging (`create_cylindrical_box` at radius 1e4, thickness 1e-3:
        // 3.49 for a joined arc whose samples lie within rounding of the cylinder, B-rep
        // tolerance 1e-5), and the edge is left as upstream leaves it, not proved to cover the
        // face. The sample never stands in for a proof: it only chooses between the proved bound
        // and upstream's value, and the tolerance is set to one of those, never to a sample.
        // It may choose because the distance from a point of the curve to the surface is a
        // lower bound on the greatest gap, which the proved bound encloses: a bound within
        // twice it has converged. A sample as computed is that distance only when the
        // surface's drop finds the nearest point, which it does not certify; otherwise it is
        // larger (see [`sampled_face_gap`]). An overstated sample can only take the proved
        // bound (covering the face, if loosely), never an unproved value.
        // (the settling's re-bound, at the coarser fit of the raised tolerance, is held to the
        // same rule: under the limit for a bound taken under it, within the sampled gate's
        // reach for one taken past it)
        let reach = if within_edge_tolerance_limit(b, gap) {
            crate::geom::tol::LARGE_SMALL_SIZE_RATIO * b.tolerance()
        } else {
            match sampled_face_gap(k, b, e, f)? {
                Some(seen) if gap <= COVER_CONVERGED * seen => COVER_CONVERGED * seen,
                _ => continue,
            }
        };
        settle.push((f, reach));
        if gap > tol {
            crate::topology::edge::set_tolerance(b, e, gap, true, true)?;
            raised = true;
        }
    }
    if raised {
        settle_face_bounds(k, b, e, &settle)?;
        cover_raised_trims(k, b, e, true)?;
        cover_vertex_ends(k, b, e)?;
    }
    Ok(())
}

/// How many times the largest sampled distance ([`sampled_face_gap`]) a face bound past the
/// B-rep check's edge tolerance limit may be and still be taken by [`cover_joined_faces`]: a
/// bound within a small factor of what is seen is an enclosure that converged (the goal
/// #19784 sets for trim bounds), one far above it is not.
const COVER_CONVERGED: f64 = 2.0;

/// How many pieces [`sampled_face_gap`] cuts the edge's interval into.
const COVER_SAMPLES: u32 = 32;

/// Not upstream (#18166): the largest distance from `e`'s curve, at the ends of
/// [`COVER_SAMPLES`] equal pieces of its interval, to the closest point found on the surface
/// of face `f` (`surface_closest_point`), measured from the curve's point to the point found.
/// Each is a distance to a point of the surface, so
/// at least the true distance at that sample (MEASURED, not a bound): equal to it when the
/// drop's global search finds the nearest point, which it does not certify, and larger
/// otherwise; the evaluated point and the distance carry a few ulps of rounding of the
/// coordinates. Used only to tell a real gap from an enclosure that did not converge
/// ([`cover_joined_faces`]). `None` without a curve or surface, or where a sample is not
/// evaluated or dropped.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; the kernel's unported and cancelled statuses.
fn sampled_face_gap(k: &Kernel, b: &Brep, e: EdgeId, f: FaceId) -> Result<Option<f64>> {
    let rec = b.edge(e)?;
    let (Some(curve), Some(surface)) = (rec.curve.as_ref(), b.face(f)?.surface.as_ref()) else {
        return Ok(None);
    };
    let surface = surface.clone_for_evaluation()?;
    let mut most = 0.0_f64;
    for i in 0..=COVER_SAMPLES {
        let t = rec
            .interval
            .evaluate(f64::from(i) / f64::from(COVER_SAMPLES));
        let p = match curve.evaluate_point(k, t) {
            Ok(p) => p,
            Err(err) if err.is_unported_or_cancelled() => return Err(err),
            Err(_) => return Ok(None),
        };
        match crate::query::api_queries::surface_closest_point(k, &surface, p) {
            Ok(c) => most = most.max(p.distance(c.closest.point)),
            Err(err) if err.is_unported_or_cancelled() => return Err(err),
            Err(_) => return Ok(None),
        }
    }
    Ok(Some(most))
}

/// Not upstream (#18166, #20373): for an edge `e` the cover raised ([`cover_joined_faces`]),
/// or an edge the stitch joined ([`cover_joined_trims`]), raises it once to the largest
/// proved bound of its UV trims ([`raised_trim_bound`]) that its tolerance does not cover. With
/// `within`, a bound shown within the tolerance is taken as soon as it is shown (as every
/// cover in the stitch takes it); without, as [`cover_joined_edge_trims`] settles a raise,
/// each bound is taken in full at the fit, and only up to [`SETTLE_RISE`] above the tolerance
/// (an acceptance cap: a bound above it is not taken). In the stitch each bound is taken
/// once: a proved bound holds whatever the edge's tolerance, and bounding again at each
/// raised tolerance only coarsens the fit (gen1108 edge 9 climbed 2.48e-5, 2.97e-5, ...
/// 5.83e-5 in eight such rounds, for one trim bounded at 2.48e-5). A joined edge covers
/// everything it lies on: a trim set by an edit raised the tolerance when it was set
/// (`keep_trim_covered`), but a trim the operation was given is bounded only here
/// (`primitive.api_primitives` gen1108: a line 5.5e8 long, proved within its tolerance of
/// 1e-5 of its trimless plane face, with a trim bounded at 2.8e-5 under that tolerance).
/// A trim whose bound is not enclosed, or is past the B-rep check's limit
/// ([`within_edge_tolerance_limit`]), is left as upstream leaves it (UNPROVED; stricter
/// than [`cover_joined_faces`], which takes a face bound past the limit where it is seen).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`trim_deviation_bound`].
fn cover_raised_trims(k: &Kernel, b: &mut Brep, e: EdgeId, within: bool) -> Result<()> {
    let tol = b.edge(e)?.tolerance;
    let cap = if within {
        f64::INFINITY
    } else {
        tol * (1.0 + SETTLE_RISE) * (1.0 + 4.0 * f64::EPSILON)
    };
    let mut reach = tol;
    for &x in &b.edge(e)?.edgeuses {
        let Some(trim) = b.edgeuse(x)?.uv_trim_curve.as_ref() else {
            continue;
        };
        if let Some(bound) = raised_trim_bound(k, b, x, trim, tol, within)? {
            // (UNPROVED fallback: a bound past the B-rep check's limit is left as upstream
            // leaves it, with no sampled gate as [`cover_joined_faces`] has for a face; after a
            // raise past the limit no bound above the raised tolerance is taken, and a trim
            // whose bound is above it stays uncovered)
            if bound.is_finite()
                && bound > reach
                && bound <= cap
                && within_edge_tolerance_limit(b, bound)
            {
                reach = bound;
            }
        }
    }
    if reach > tol {
        crate::topology::edge::set_tolerance(b, e, reach, true, true)?;
    }
    Ok(())
}

/// The proved bound of the UV trim `trim` of `eu` ([`trim_deviation_bound`] at the fit `tol`,
/// shown within `tol` where `within`; in full otherwise, except that a bound within `tol` that
/// the trim's structure gives at once, whatever the fit, is taken as it is
/// ([`structured_trim_deviation_bound`]: the trim is covered either way, and the bisection it
/// spares is the cost #20373 removed), for [`cover_raised_trims`]. A UV line is bounded as
/// its NURB, plus how far the line's
/// analytic evaluation can be from that NURB in UV (`line_evaluation_slack`) carried to the
/// surface where it is a plane (`plane_uv_lipschitz`): its own rounding moves its parameters,
/// which `trim_image_gap_bound` refuses. `None` where it is not enclosed (a UV line on
/// another surface).
///
/// # Errors
///
/// Those of [`trim_deviation_bound`].
fn raised_trim_bound(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    trim: &Curve,
    tol: f64,
    within: bool,
) -> Result<Option<f64>> {
    use crate::query::curve_surface_gap::{line_evaluation_slack, plane_uv_lipschitz};
    let reach = if within { tol } else { 0.0 };
    let over = [reach, f64::INFINITY];
    let CurveKind::Line(l) = trim.kind() else {
        if !within {
            if let Some(at_once) = structured_trim_deviation_bound(k, b, eu, trim)? {
                if at_once <= tol {
                    return Ok(Some(at_once));
                }
            }
        }
        return trim_deviation_bound(k, b, eu, trim, tol, over, None);
    };
    let Some(fu) = faceuse(b, eu)? else {
        return Ok(None);
    };
    let Some(f) = faceuse_face(b, fu)? else {
        return Ok(None);
    };
    let Some(surface) = b.face(f)?.surface.as_ref() else {
        return Ok(None);
    };
    let nurb = l.base_with_nurb()?.into_owned();
    let cpts = nurb.control_points()?.to_vec();
    let Some(d) = line_evaluation_slack(l, true) else {
        return Ok(None);
    };
    let Some(lip) = plane_uv_lipschitz(surface, &cpts, d) else {
        return Ok(None);
    };
    let slack = lip * d * (1.0 + 4.0 * f64::EPSILON);
    // (the NURB's bound is shown within the tolerance less the slack, so that a bound found
    // within `enough` still leaves the line within the tolerance)
    let enough = ((reach - slack) * (1.0 - 4.0 * f64::EPSILON)).max(0.0);
    let Some(x) = trim_deviation_bound(k, b, eu, &Curve::from(nurb), tol, [enough, over[1]], None)?
    else {
        return Ok(None);
    };
    let bound = if slack == 0.0 {
        x
    } else {
        (x + slack) * (1.0 + 4.0 * f64::EPSILON)
    };
    Ok(bound.is_finite().then_some(bound))
}

/// Not upstream (#18166): raises each vertex of `e` to cover its distance to the ends of all
/// its edges, after the cover raised `e` ([`cover_joined_faces`]): a vertex stands for the
/// ends of its edges, and the stitch's joins can leave an edge's end farther from its vertex
/// than the vertex's tolerance (`heal.stitch_p1` `cov_tjunc9_into_shell`: a vertex 1.02 from
/// the end of an edge the stitch joined, under a tolerance of 1.1e-3). Each distance is the
/// evaluated end point's distance plus the evaluation's error, bounded per coordinate from
/// the curve's own data ([`Curve::point_error_bound`]: its control points, not the size of
/// the point it returns) times `√3`, each step rounded up. An end not evaluated, or with no
/// such bound, raises nothing.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
fn cover_vertex_ends(k: &Kernel, b: &mut Brep, e: EdgeId) -> Result<()> {
    let (v0, v1) = crate::topology::edge::start_end_vertices(b, e)?;
    let mut vertices: Vec<crate::topology::VertexId> = Vec::new();
    for v in [v0, v1].into_iter().flatten() {
        if !vertices.contains(&v) {
            vertices.push(v);
        }
    }
    for v in vertices {
        // (an end not evaluated, or with no bound on its evaluation's error, raises
        // nothing: the vertex is left as upstream leaves it for that end)
        let reach = crate::topology::vertex::edge_end_reach(k, b, v)?;
        if reach > b.vertex(v)?.tolerance {
            crate::topology::vertex::set_tolerance(b, v, reach, true, true)?;
        }
    }
    Ok(())
}

/// The faces of `e` none of whose uses of `e` holds a UV trim, in the order of the edge's uses.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
fn trimless_faces(b: &Brep, e: EdgeId) -> Result<Vec<FaceId>> {
    let mut faces: Vec<(FaceId, bool)> = Vec::new();
    for &x in &b.edge(e)?.edgeuses {
        let Some(fu) = faceuse(b, x)? else {
            continue;
        };
        let Some(f) = faceuse_face(b, fu)? else {
            continue;
        };
        let trimmed = b.edgeuse(x)?.uv_trim_curve.is_some();
        match faces.iter_mut().find(|(g, _)| *g == f) {
            Some(slot) => slot.1 |= trimmed,
            None => faces.push((f, trimmed)),
        }
    }
    Ok(faces
        .into_iter()
        .filter(|(_, trimmed)| !trimmed)
        .map(|(f, _)| f)
        .collect())
}

/// Not upstream (#18166): of each edge of `edges`, the faces it holds no UV trim on whose
/// proved distance from its curve, bounded at its tolerance ([`edge_face_gap_bound`] with the
/// tolerance as fit and target), is within that tolerance. [`keep_trimless_faces_covered`]
/// keeps them covered across an edit of the faces' surfaces.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`edge_face_gap_bound`].
pub(crate) fn covered_trimless_faces(
    k: &Kernel,
    b: &Brep,
    edges: &[EdgeId],
) -> Result<Vec<(EdgeId, FaceId)>> {
    let mut out = Vec::new();
    for &e in edges {
        let tol = b.edge(e)?.tolerance;
        for f in trimless_faces(b, e)? {
            if matches!(edge_face_gap_bound(k, b, e, f, tol, [tol, tol])?, Some(g) if g <= tol) {
                out.push((e, f));
            }
        }
    }
    Ok(out)
}

/// Not upstream (#20373): raises each edge of `edges` (the edges a stitch joined,
/// [`crate::topology::brep::p5`]'s `StitchRaises`) that holds a UV trim to cover its
/// trims ([`cover_raised_trims`]), and, where that raised it, its vertices the ends of their
/// edges ([`cover_vertex_ends`]). It runs after the stitch's orientation, which rewrites the
/// trims of the surfaces it reverses: a UV line trim, bounded with the slack of its analytic
/// evaluation, comes out of it a B-spline without that slack (`primitive.api_primitives`
/// gen1108 edges 0 to 3: 3.29e-5 to 3.90e-5 on the lines, 3.13e-5 to 3.54e-5 on the trims
/// the body ends with). A joined edge covers everything it lies on; before #20373 its trims
/// were covered only through a face raise, which a face bound proved within the tolerance no
/// longer makes (gen1108 edges 6, 9 and 15), and an edge the join itself raised
/// (`UpdateObjectTolerance`) but no face bound did had them covered nowhere. An edge a face
/// bound raised is covered again here, on its final trims. An edge that is gone, or whose
/// cover fails (other than unported or cancelled), is left as it stands.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub(crate) fn cover_joined_trims(k: &Kernel, b: &mut Brep, edges: &[EdgeId]) -> Result<()> {
    for &e in edges {
        ignored(cover_joined_edge_trims(k, b, e))?;
    }
    Ok(())
}

/// [`cover_joined_trims`] of one edge. A raise is settled as [`settle_face_bounds`] settles a
/// face's: each trim bound is cut to a thousandth of the fit, the fit after a raise is the
/// raised tolerance, and at it a bound can come out a rounding above the one found before
/// (gen1108 edge 0: 3.12378e-5 at the fit 1e-5, 3.12542e-5 at the fit 3.12378e-5), so after a
/// raise the cover is taken again, each bound in full at the raised tolerance and taken only
/// up to [`SETTLE_RISE`] above it (a heuristic cap; a bound refused leaves the edge at the
/// tolerance proved before), for at most
/// [`SETTLE_ROUNDS`] rounds, until it raises no further. Each bound is proved, so each raise
/// covers a proved distance. A raise is kept only where the raised edge's tube
/// ([`edge_tube_reaches`](crate::topology::brep::validation::edge_tube_reaches)) and each
/// vertex ball it grew ([`vertex_ball_reaches`](crate::topology::brep::validation::vertex_ball_reaches))
/// are proved not to reach topology they do not bound: otherwise the edge and its vertices
/// keep the tolerances they came with, the gap left for validation to see, as before #20373.
/// They keep them too where any step after the snapshot fails: the error is returned with
/// the old tolerances put back, so no raise stays without its reach proof.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`cover_raised_trims`],
/// [`cover_vertex_ends`] and the reach proofs' unported and cancelled statuses.
fn cover_joined_edge_trims(k: &Kernel, b: &mut Brep, e: EdgeId) -> Result<()> {
    let rec = b.edge(e)?;
    let before = rec.tolerance;
    let mut trimmed = false;
    for &x in &rec.edgeuses {
        trimmed |= b.edgeuse(x)?.uv_trim_curve.is_some();
    }
    if !trimmed {
        return Ok(());
    }
    // (the edge's vertices and their tolerances, put back if the raise is withheld)
    let (v0, v1) = crate::topology::edge::start_end_vertices(b, e)?;
    let mut ends: Vec<(crate::topology::VertexId, f64)> = Vec::new();
    for v in [v0, v1].into_iter().flatten() {
        if !ends.iter().any(|&(w, _)| w == v) {
            ends.push((v, b.vertex(v)?.tolerance));
        }
    }
    // (any error once the edge may be raised puts the old tolerances back before it is
    // returned: no raise is kept without the reach proof, #20373 review r4)
    let restore = |b: &mut Brep| -> Result<()> {
        b.edge_mut(e)?.tolerance = before;
        for &(v, was) in &ends {
            b.vertex_mut(v)?.tolerance = was;
        }
        Ok(())
    };
    match raise_joined_edge(k, b, e, before, &ends) {
        Ok(true) => Ok(()),
        Ok(false) => restore(b),
        Err(err) => {
            restore(b)?;
            Err(err)
        }
    }
}

/// The raise of [`cover_joined_edge_trims`]: covers the trims of `e` (tolerance `before`,
/// its end vertices and their tolerances `ends`) and its vertex ends, and answers whether
/// the raise may be kept: `true` where nothing was raised or neither the edge's tube nor
/// a raised vertex's ball may reach topology it does not bound (a proof that fails counts
/// as reaching).
///
/// # Errors
///
/// Those of [`cover_raised_trims`], [`cover_vertex_ends`] and the reach proofs' unported
/// and cancelled statuses.
fn raise_joined_edge(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    before: f64,
    ends: &[(crate::topology::VertexId, f64)],
) -> Result<bool> {
    for round in 0..SETTLE_ROUNDS {
        let was = b.edge(e)?.tolerance;
        cover_raised_trims(k, b, e, round == 0)?;
        if b.edge(e)?.tolerance <= was {
            break;
        }
    }
    if b.edge(e)?.tolerance <= before {
        return Ok(true);
    }
    cover_vertex_ends(k, b, e)?;
    let mut reaches = ignored(crate::topology::brep::validation::edge_tube_reaches(
        k, b, e,
    ))?
    .unwrap_or(true);
    for &(v, was) in ends {
        if !reaches && b.vertex(v)?.tolerance > was {
            reaches = ignored(crate::topology::brep::validation::vertex_ball_reaches(
                k, b, v,
            ))?
            .unwrap_or(true);
        }
    }
    Ok(!reaches)
}

/// Not upstream (#18166): keeps each edge of `covered` ([`covered_trimless_faces`]) covering
/// its face after an edit that rewrote the face's surface without moving it
/// (`OrientTrimmedSurfaces` reverses a surface: its knots are reflected, with rounding), where
/// the edge is still on the face and still holds no trim there: the face's bound, which
/// comes out a rounding apart on the rewritten surface (`heal.stitch_p1` `cov_tjunc11_stitch`
/// edge 12: 1.0687382852519145e-5 before the stitch's orientation, 1.0687382852519804e-5
/// after it), is settled at the edge's tolerance ([`settle_face_bounds`]), up to twice the
/// tolerance the edge had (a bound past that is no rounding, and is left as upstream leaves
/// it).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`edge_face_gap_bound`].
pub(crate) fn keep_trimless_faces_covered(
    k: &Kernel,
    b: &mut Brep,
    covered: &[(EdgeId, FaceId)],
) -> Result<()> {
    let mut edges: Vec<EdgeId> = Vec::new();
    for &(e, _) in covered {
        if !edges.contains(&e) {
            edges.push(e);
        }
    }
    for e in edges {
        if b.edge(e).is_err() {
            continue;
        }
        let now = trimless_faces(b, e)?;
        let cap = 2.0 * b.edge(e)?.tolerance;
        let faces: Vec<(FaceId, f64)> = covered
            .iter()
            .filter(|&&(x, f)| x == e && now.contains(&f))
            .map(|&(_, f)| (f, cap))
            .collect();
        settle_face_bounds(k, b, e, &faces)?;
    }
    Ok(())
}

/// The most rounds [`settle_face_bounds`] takes.
const SETTLE_ROUNDS: usize = 8;

/// Not upstream (#20373): how far above the tolerance `t` a bound taken in full at the fit `t`
/// may come out in a settle round of [`cover_joined_edge_trims`] and still be taken: an
/// acceptance cap, a heuristic, not a proved limit. Each round rebuilds the bound at its own
/// fit (the floor is [`TRIM_BOUND_FLOOR`] times the fit), so it bounds a different function
/// from the one the round before proved within `t`, and nothing proves how far above `t` a
/// bound at the coarser fit may come out: the stitch's cover, bounding again at each raise,
/// climbed gen1108 edge 9 from 2.48e-5 to 5.83e-5 in eight rounds. The cap admits the
/// rounding-sized rises the settle is for (gen1108 edge 0: 3.12378e-5 at the fit 1e-5,
/// 3.12542e-5 at the fit 3.12378e-5, a rise of 5e-4) and refuses the larger ones. It is safe
/// either way: every bound taken is proved, and a bound refused leaves the edge at the
/// tolerance it has, which covers the bound proved in the round before (round 0 takes its
/// bounds uncapped). The [`SETTLE_ROUNDS`] rounds raise by at most 1.6 % together.
const SETTLE_RISE: f64 = 2.0e-3;

/// Not upstream (#18166): after a raise of `e`, makes its tolerance cover each face of
/// `faces` (each with a bound past which it is not raised) by the face's own bound, as that
/// bound comes out at the raised tolerance: the bound is cut to a thousandth of the fit, and the
/// fit after the raise is the raised tolerance ([`edge_face_gap_bound`] with the tolerance as
/// both fit and target), at which a bound can come out a rounding above the one found before
/// (the bisection stops at a coarser floor). Raised again to such a bound (below the
/// face's cap: for a joined face the B-rep check's edge tolerance limit, or past it the
/// sampled gate's reach, [`cover_joined_faces`]; the one [`keep_trimless_faces_covered`]
/// gives) for at most
/// [`SETTLE_ROUNDS`] rounds; each bound is proved, so each raise covers a proved distance.
///
/// # Errors
///
/// Those of [`edge_face_gap_bound`].
fn settle_face_bounds(k: &Kernel, b: &mut Brep, e: EdgeId, faces: &[(FaceId, f64)]) -> Result<()> {
    for _ in 0..SETTLE_ROUNDS {
        let mut raised = false;
        for &(f, reach) in faces {
            let tol = b.edge(e)?.tolerance;
            match edge_face_gap_bound(k, b, e, f, tol, [tol, f64::INFINITY])? {
                Some(again) if again.is_finite() && again > tol && again < reach => {
                    crate::topology::edge::set_tolerance(b, e, again, true, true)?;
                    raised = true;
                }
                _ => {}
            }
        }
        if !raised {
            break;
        }
    }
    Ok(())
}

/// Not upstream (#18166): a proved bound on the distance from `e`'s curve over its interval to
/// the surface of its face `f` (`curve_face_gap_bound_within`, cut to a thousandth of `fit` as
/// [`trim_deviation_bound`], and until shown within `within` where it is; a bound whose end
/// values pass `over` may be infinite), plus the rounding of a line's analytic evaluation
/// against its NURBS data (`line_evaluation_slack`); `None` where it is not enclosed or the
/// curve is not a B-spline, an ellipse or a line.
///
/// # Errors
///
/// Those of `curve_face_gap_bound_within`.
fn edge_face_gap_bound(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    f: FaceId,
    fit: f64,
    [within, over]: [f64; 2],
) -> Result<Option<f64>> {
    let rec = b.edge(e)?;
    let Some(curve) = rec.curve.as_ref() else {
        return Ok(None);
    };
    let line;
    let (bs, slack) = match curve.kind() {
        CurveKind::BSpline(bs) => (bs, 0.0),
        CurveKind::Ellipse(el) => (el.base(), 0.0),
        CurveKind::Line(l) => {
            line = l.base_with_nurb()?;
            let Some(slack) = crate::query::curve_surface_gap::line_evaluation_slack(l, false)
            else {
                return Ok(None);
            };
            (&*line, slack)
        }
        _ => return Ok(None),
    };
    let ivl = rec.interval;
    // (the caller only asks whether the bound is within `within`)
    let enough = within / (1.0 + 4.0 * f64::EPSILON) - slack;
    let gap = crate::query::curve_surface_gap::curve_face_gap_bound_within(
        k,
        b,
        f,
        bs,
        [ivl.min(), ivl.max()],
        TRIM_BOUND_FLOOR * fit,
        [enough, over],
    )?;
    Ok(gap.map(|g| (g + slack) * (1.0 + 4.0 * f64::EPSILON)))
}

/// Not upstream (#18162): a proved upper bound on how far the image of `trim` (a UV trim of
/// `eu`, on the edge's parameter) on the surface of `eu`'s face lies from the edge's curve,
/// over the edge's interval ([`trim_image_gap_bound`]): the distance `SetUVTrimCurve` raises
/// the edge tolerance to cover. `None` when the bound is not enclosed (the caller then keeps
/// upstream's distance) or the edge use has no face, surface, edge or curve.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; the kernel's unported and cancelled statuses.
///
/// A bound above `over` is `Some(f64::INFINITY)`, found without bounding the rest of the trim
/// ([`trim_image_gap_bound_up_to`]): every caller only compares such a bound with `over`, and
/// on a surface with a dense net the full bound of a trim far off its edge costs many times
/// more (a union in the `crashes/2992` rig case took 153 s instead of 43 s). A bound at most
/// `enough` is any proved bound up to `enough` ([`trim_image_gap_bound_within`]): every caller
/// treats all such bounds alike, and tightening a trim that lies well within its tolerance on
/// a dense net is what made the same union take 274 s instead of 3.5 s.
///
/// `fit` is the tolerance the trim is fitted at ([`fit_tolerance`]): the bound is cut to a
/// thousandth of it ([`TRIM_BOUND_FLOOR`]). It is not the edge's tolerance, which the healer
/// raises to cover a gap without changing the fit: a bound at that tolerance would give a
/// trim made at the same fit a different raise on a healed edge than on one at the
/// tolerance before the gap cover (`Fillet/sphere_plane.smb`, by 7e-8 of it).
///
/// [`trim_image_gap_bound`]: crate::query::curve_surface_gap::trim_image_gap_bound
/// [`trim_image_gap_bound_up_to`]: crate::query::curve_surface_gap::trim_image_gap_bound_up_to
/// [`trim_image_gap_bound_within`]: crate::query::curve_surface_gap::trim_image_gap_bound_within
pub(crate) fn trim_deviation_bound(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    trim: &Curve,
    fit: f64,
    [enough, over]: [f64; 2],
    budget: Option<usize>,
) -> Result<Option<f64>> {
    let Some((e, surface)) = trim_pair_surface(b, eu)? else {
        return Ok(None);
    };
    let edge_rec = b.edge(e)?;
    let Some(curve) = edge_rec.curve.as_ref() else {
        return Ok(None);
    };
    let ivl = edge_rec.interval;
    // (below a thousandth of the fit tolerance a tighter bound changes nothing)
    let floor = TRIM_BOUND_FLOOR * fit;
    k.work_counters.add_trim_deviation_bound();
    crate::query::curve_surface_gap::trim_image_gap_bound_within(
        k,
        &surface,
        curve,
        trim,
        [ivl.min(), ivl.max()],
        floor,
        [enough, over],
        budget,
    )
}

/// Not upstream (WR-450, #18923): [`trim_deviation_bound`] at equal parameters
/// ([`trim_same_parameter_gap_bound_within`]): a proved bound on how far the image of the UV
/// trim `trim` of `eu` at each parameter of its edge's interval is from the edge curve's point
/// at that parameter, so also how far the two parameters slip. `fit`, `enough`, `over` and
/// `budget` mean what they mean there. `None` where the pair has no exact data to bound it
/// from (no surface or curve, or as [`trim_same_parameter_gap_bound_within`] says);
/// `Some(f64::INFINITY)` where it has but the gap is not enclosed.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; the kernel's unported and cancelled statuses.
///
/// [`trim_same_parameter_gap_bound_within`]: crate::query::curve_surface_gap::trim_same_parameter_gap_bound_within
pub(crate) fn trim_same_parameter_deviation_bound(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    trim: &Curve,
    fit: f64,
    [enough, over]: [f64; 2],
    budget: Option<usize>,
) -> Result<Option<f64>> {
    let Some((e, surface)) = trim_pair_surface(b, eu)? else {
        return Ok(None);
    };
    let edge_rec = b.edge(e)?;
    let Some(curve) = edge_rec.curve.as_ref() else {
        return Ok(None);
    };
    let ivl = edge_rec.interval;
    let floor = TRIM_BOUND_FLOOR * fit;
    k.work_counters.add_trim_deviation_bound();
    crate::query::curve_surface_gap::trim_same_parameter_gap_bound_within(
        k,
        &surface,
        curve,
        trim,
        [ivl.min(), ivl.max()],
        floor,
        [enough, over],
        budget,
    )
}

/// Not upstream (#20373): the bound of the UV trim `trim` of `eu` that its structure gives at
/// once, whatever the fit ([`crate::query::curve_surface_gap::structured_trim_image_gap_bound`],
/// the one [`trim_deviation_bound`] takes first where it is within `enough`); `None` where the
/// trim has no such structure.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; the kernel's unported and cancelled statuses.
fn structured_trim_deviation_bound(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    trim: &Curve,
) -> Result<Option<f64>> {
    let Some((e, surface)) = trim_pair_surface(b, eu)? else {
        return Ok(None);
    };
    let edge_rec = b.edge(e)?;
    let Some(curve) = edge_rec.curve.as_ref() else {
        return Ok(None);
    };
    let ivl = edge_rec.interval;
    crate::query::curve_surface_gap::structured_trim_image_gap_bound(
        k,
        &surface,
        curve,
        trim,
        [ivl.min(), ivl.max()],
    )
}

/// The edge of `eu` and a copy of its face's surface without owner, as the drops use (its
/// evaluations do not trim to the face), for [`trim_deviation_bound`] and
/// [`structured_trim_deviation_bound`]; `None` where `eu` has no edge, face, surface or edge
/// curve.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of the surface's copy.
fn trim_pair_surface(b: &Brep, eu: EdgeuseId) -> Result<Option<(EdgeId, Surface)>> {
    let (Some(e), Some(fu)) = (edge(b, eu)?, faceuse(b, eu)?) else {
        return Ok(None);
    };
    let Some(f) = faceuse_face(b, fu)? else {
        return Ok(None);
    };
    // (an edge without a curve is `None` before the surface is copied, as before the copy
    // moved here: a copy that fails is then no error)
    let (Some(_), Some(surface)) = (b.edge(e)?.curve.as_ref(), b.face(f)?.surface.as_ref()) else {
        return Ok(None);
    };
    let mut surface = surface.clone_for_evaluation()?;
    let _ = surface.set_owner(None::<ObjectRef>);
    Ok(Some((e, surface)))
}

/// The fraction of the fit tolerance below which [`trim_deviation_bound`] stops tightening.
const TRIM_BOUND_FLOOR: f64 = 1.0e-3;

/// [`get_or_create_uv_trim_curve`] of an edge use that has a UV trim curve (its own or
/// its mate's), which reads the B-rep only; `None` without one.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub(crate) fn existing_uv_trim_curve(
    b: &Brep,
    eu: EdgeuseId,
    want_dist: bool,
) -> Result<Option<UvTrimCurveFetch>> {
    // get cached UVTrimCurve (if any)
    if uv_trim_curve(b, eu)?.0.is_none() {
        return Ok(None);
    }
    let mut out = UvTrimCurveFetch {
        status: Ok(()),
        dist_to_surf: -1.0,
        deviation: -1.0,
    };
    // prevent infinite loop - only ask for FaceEdge Gap if it already exists (so it is
    // read, not calculated)
    if want_dist && crate::topology::edgeuse::p1::is_max_edge_face_gap_init(b, eu)? {
        let gap = crate::topology::edgeuse::p1::cached_max_edge_face_gap(b, eu)?;
        out.dist_to_surf = gap.map_or(0.0, |g| g.gap_3d);
    }
    // `m_pDropCurveFail` itself, not `GetDropCurveFail()`: the record of this edge
    // use only (`SetUVTrimCurve` nulls the non-owner's)
    if let Some(f) = b.edgeuse(eu)?.drop_curve_fail.as_deref() {
        if f.fail_status != SM_SUCCESS {
            out.status = Err(SolidError::topology(
                OP,
                TopologyFailure::DropFailed {
                    status: f.fail_status,
                },
            ));
        }
    }
    Ok(Some(out))
}

// ---- EvaluateBinormal -------------------------------------------------------------------

/// Two solutions that both lie on a seam: the first one's parameters (`sols.len() == 2 &&
/// IsOnSeam(first) && IsOnSeam(second)`); `None` otherwise.
fn seam_pair_uv(
    k: &Kernel,
    surface: &Surface,
    sols: &[crate::geom::solution_array::Solution<NoObject>],
) -> Result<Option<Point2>> {
    let [first, second] = sols else {
        return Ok(None);
    };
    let uv0 = Point2::new(first.start.param(0)?, first.start.param(1)?);
    if !surface.is_on_seam(k, uv0, None)?.on_seam {
        return Ok(None);
    }
    let uv1 = Point2::new(second.start.param(0)?, second.start.param(1)?);
    if !surface.is_on_seam(k, uv1, None)?.on_seam {
        return Ok(None);
    }
    Ok(Some(uv0))
}

/// The position and the first two derivatives of a curve evaluation (zero-filled where
/// the evaluation returned fewer).
fn pv3(v: &[Vec3]) -> [Vec3; 3] {
    let at = |i: usize| v.get(i).copied().unwrap_or(Vec3::new(0.0, 0.0, 0.0));
    [at(0), at(1), at(2)]
}

/// The optional outputs of [`evaluate_binormal`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BinormalOptions {
    /// `pOptUVVector`: also compute the surface parameter direction of the binormal
    /// ([`BinormalUv::uv_vector`]).
    pub want_uv_vector: bool,
}

/// [`evaluate_binormal`]'s result with the optional parameter direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BinormalUv {
    /// The binormal point, binormal, tangent, face use normal and surface parameters.
    pub eval: BinormalEval,
    /// `pOptUVVector`: the (not unitised) parameter direction of the binormal
    /// (`ComputeParamDirection`), when asked.
    pub uv_vector: Option<Vec2>,
}

/// The binormal point and vector at `parameter` on an edge use: a 3D point on the edge
/// and a unit vector perpendicular to the face use normal and the edge use tangent,
/// pointing into the face. With `use_3d_only` the edge point is dropped onto the surface
/// unless the edge use has a UV trim curve; without it a missing UV trim curve is
/// built first (`GetOrCreateUVTrimCurve`). A parameter outside the edge's interval is
/// snapped to it. Where the tangent or the binormal vanishes the tangent is taken from
/// the second derivative or from points stepped along the curve or the surface.
///
/// # Errors
///
/// `SM_ERR` (a `TopologyFailure`) for an edge use not in a loop, for no usable
/// tangent or binormal and when the edge point does not drop to the surface;
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] where
/// upstream dereferences null; the evaluators' errors; [`SolidError::Unported`]
/// (module docs).
#[allow(clippy::too_many_lines)]
pub fn evaluate_binormal(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    parameter: f64,
    use_3d_only: bool,
    options: &BinormalOptions,
) -> Result<BinormalEval> {
    Ok(evaluate_binormal_uv(k, b, eu, parameter, use_3d_only, options)?.eval)
}

/// [`evaluate_binormal`] with the optional parameter direction.
///
/// # Errors
///
/// As [`evaluate_binormal`].
#[allow(clippy::too_many_lines)]
pub fn evaluate_binormal_uv(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    parameter: f64,
    use_3d_only: bool,
    options: &BinormalOptions,
) -> Result<BinormalUv> {
    let mut parameter = parameter;
    let mut use_3d_only = use_3d_only;

    // check state - only Edgeuses in loops have a binormal vector
    if !crate::topology::edgeuse::p1::is_loop_edgeuse(b, eu)? {
        return Err(SolidError::topology(OP, TopologyFailure::EdgeUseNotInLoop));
    }

    let e = edge(b, eu)?.ok_or_else(|| null("EvaluateBinormal: edge use without edge"))?;

    // check state - dParameter not within Edge->Ivl
    if !b.edge(e)?.interval.contains_value(parameter, EFF_ZERO) {
        // inform the public - this is a bug and should be examined. A lot of these
        // calls seem to be coming from CurveOnClassify where Edges are found that stop
        // near but not on the Curve being classified. snap Param value to Edge->Ivl
        // endPoint
        parameter = b.edge(e)?.interval.clamp_value(parameter);
    }

    // locals
    let edge_ivl = b.edge(e)?.interval;
    let curve_3d = b
        .edge(e)?
        .curve
        .clone()
        // NER(p3DCurve)
        .ok_or_else(|| SolidError::topology(OP, TopologyFailure::EdgeWithoutCurve))?;
    let eu_orientation = orientation(b, eu)?;

    // evaluate the unoriented curve position and tangent values
    let mut curve_pv = pv3(&curve_3d.evaluate(k, parameter, 1, true, true)?);

    // while tangent length is zero - step along curve looking for a nonDegenerate
    // tangent direction
    let mut steps: u64 = 0;
    while curve_pv
        .get(1)
        .copied()
        .unwrap_or(Vec3::UNDEF)
        .length_squared()
        < EFF_ZERO_SQ
        && steps < 10
    {
        steps += 1;

        // First: check second derivative. If it's not zero, then its direction is what
        // the first deriv's direction will be as soon as we leave the singularity.
        // [bd, 13 Sep 05]

        // Try eval from right
        curve_pv = pv3(&curve_3d.evaluate(k, parameter, 2, true, true)?);
        if curve_pv[2].length_squared() >= EFF_ZERO_SQ {
            // assume 2nd deriv points pos at edge beg where parameterization is
            // expected to be increasing and neg at edge ends where parameterization is
            // expected to be decreasing
            curve_pv[1] = if edge_ivl.mid() > parameter {
                curve_pv[2]
            } else {
                -curve_pv[2]
            };
            break;
        }

        // Else try eval from left
        curve_pv = pv3(&curve_3d.evaluate(k, parameter, 2, false, true)?);
        if curve_pv[2].length_squared() >= EFF_ZERO_SQ {
            curve_pv[1] = if edge_ivl.mid() > parameter {
                curve_pv[2]
            } else {
                -curve_pv[2]
            };
            break;
        }

        // pick sequence of new param values progressively farther away from orig param
        let next_step = if steps % 2 == 0 {
            steps as f64 * EFF_ZERO_SQRT
        } else {
            steps as f64 * -EFF_ZERO_SQRT
        };
        let step_param = if edge_ivl.contains_value(parameter + next_step, 0.0) {
            parameter + next_step
        } else if edge_ivl.contains_value(parameter - next_step, 0.0) {
            parameter - next_step
        } else {
            return Err(SolidError::topology(
                OP,
                TopologyFailure::NoStepInsideInterval,
            ));
        };

        // evaluate unoriented curve at modified param value
        let pv2 = curve_3d.evaluate(k, step_param, 1, true, true)?;
        curve_pv[1] = pv2
            .get(1)
            .copied()
            .ok_or_else(|| null("curve evaluation"))?;
    } // end while tang vector length == 0.0 - seek nonDegenerate Tangent evals

    // Reverse orientation of curve tangent to conform to edgeuse tangent.
    if eu_orientation == OrientType::Opposite {
        curve_pv[1] = -curve_pv[1];
    }

    // face and surface locals
    let f = crate::topology::edgeuse::p1::face(b, eu)?
        .ok_or_else(|| null("EvaluateBinormal: edge use without face"))?;
    let face_rec = b.face(f)?;
    let face_surface = {
        // `SmEdgeuse::EvaluateBinormal` holds `pFace->GetSurface()` directly
        // (`SmEdgeuse.cpp:2875`), so the evaluation snapshot must retain the live
        // surface's owner, exact NURBS and shared cache slot instead of running
        // `Surface`'s copy constructor.
        face_rec
            .surface
            .as_ref()
            .ok_or_else(|| null("EvaluateBinormal: face without surface"))?
            .clone_for_evaluation()?
    };
    let domain = face_rec.uv_domain;
    let face_uv_orientation = {
        let fu = faceuse(b, eu)?.ok_or_else(|| null("EvaluateBinormal: no face use"))?;
        b.faceuse(fu)?.orientation
    };

    // If we already have a uv trim curve, then we can assume it's valid. [B556]
    let has_trim = uv_trim_curve(b, eu)?.0.is_some();
    let fail_status = drop_curve_fail(b, eu)?.0.map(|f| f.fail_status);

    // Work in UV Space when good pUVTrimCurve is available
    if has_trim && fail_status.is_none_or(|s| s != SM_SUCCESS && s != SM_ERR_UNKNOWN) {
        use_3d_only = false;
    }

    // when trying to work in UVSpace - make sure to get a UVTrimCurve
    if !has_trim && !use_3d_only && fail_status.is_none_or(|s| s != SM_ERR_UNKNOWN) {
        // Get our uv trim curve. Create it if necessary. [B438] Note: a bit of a
        // hack. (Besides the non-const cast.) CreateUVTrimCurve() can call this method
        // (EvaluateBinormal), which would seem to invite infinite recursion. However,
        // that method calls this one only with bUse3DOnly set to TRUE, so this will
        // never happen. (status ignored upstream, passed on by the fork)
        let fetch = get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
        fetch.status?;
    }
    let uv_trim: Option<Curve> = uv_trim_curve(b, eu)?.0.cloned();

    // the face's surface; its owner is cleared (a copy, `solve_surface`) only around
    // `GlobalPointSolve` and `IsOnSeam` below, as upstream's `SmTemporaryChangeValue`
    // scope does
    let surface = face_surface;

    let mut uv = Point2::UNDEF;
    let mut got_uv = false;
    let edge_tol = b.edge(e)?.tolerance;

    // eval pUVTrimCurve on clamped param values and clamp resulting uv point
    if let Some(trim) = uv_trim.as_ref() {
        // clamp the parameter value
        parameter = trim.natural_interval().clamp_value(parameter);

        // compute UVTrimCurve Point and clamp that to SurfaceDomain
        let pnt = trim.evaluate_point(k, parameter)?;
        uv = domain.clamp_point(Point2::new(pnt.x, pnt.y));
        got_uv = true;

        // Double check the validity of this uv point.
        let tol = edge_tol * 2.0;
        if tol > 0.0 {
            // Allow for the 3d curve point being off the surface. (It happens.) Check
            // the dist from the 3d curve point to the surface normal.
            // (status ignored upstream, passed on by the fork)
            let (this_srf_pt, this_su, this_sv) =
                surface.evaluate_1st_derivatives(k, uv, false, false)?;

            // try a cheap eval for surface normal
            let mut this_norm = this_su.cross(this_sv);
            let len = this_norm.length();
            if len > crate::surface::surface::p2::normal_limit() {
                this_norm /= len;
            } else {
                // when cheap is degenerate - try expensive normal eval (status ignored
                // upstream, passed on by the fork; sThisNorm is returned unitized)
                this_norm = surface.evaluate_normal(
                    k,
                    uv,
                    crate::nurbs::knots::Side::Right,
                    crate::nurbs::knots::Side::Right,
                )?;
            }

            // When minDist(surfacePt/surfaceNorm line, EdgePoint) < dTol - use the UV
            // point
            let gap = curve_pv[0] - this_srf_pt;
            let dot = gap.dot(this_norm);
            let norm_pt = this_srf_pt + dot * this_norm;

            got_uv = norm_pt.closer_than(tol, curve_pv[0]);

            // When edge point is too far from surface normal line, try a LocalSolve
            // using sUV as a guess. (Much cheaper than GlobalPointSolve, next.)
            if !got_uv {
                let (found, sol_uv, sol_value) =
                    surface_local_point_solve(k, &surface, &domain, curve_pv[0], uv)?;
                got_uv = found;
                if got_uv && sol_value > tol {
                    got_uv = false;
                }
                if got_uv {
                    uv = sol_uv;
                }
            }
        } // end if double-checking result of pUVTrimCurve (dTol > 0)
    } // end if have pUVTrimCurve

    // arrive here with or without a sUV point marking the face point for the given edge
    // point. May have a sUV point if(bUse3DOnly == TRUE) and UVTrimPoint projected to
    // surface was close to edge point.

    // when the edgePoint isn't matched to a surface sUV point
    if !got_uv {
        // Drop edge point to surface to get uv point.
        let mut tol = edge_tol;

        // get and lock the surface cache (of the face's surface: its owner is cleared
        // only below)
        face_surface_cache(k, b, f)?;

        // Turn off point testing so GlobalPointSolve() will keep all point solutions
        // without classifying the solution point against the trim boundaries
        // (`global_point_solve_point_test_off`; the boundary-curve flag is not
        // modelled, module docs). Use surface's domain instead of Face's domain.
        let srf_domain = surface.natural_uv_domain_with_nurb(k)?;

        // This 'turns off point testing' when GPS does not use the cache (a copy of
        // the surface without owner stands for upstream's temporary change)
        let mut solve_surface = surface.clone();
        let _ = solve_surface.set_owner(None::<ObjectRef>);

        // Find Surface sUV point for EdgePoint
        let mut solutions = solve_surface.global_point_solve_point_test_off::<NoObject>(
            k,
            Some(NoObject),
            &srf_domain,
            SolverOperation::Minimize,
            curve_pv[0],
            tol,
            None,
            SolutionRequested::All,
        )?;
        if solutions.is_empty() {
            // try again with a bigger tolerance
            tol *= 1000.0;
            solutions = solve_surface.global_point_solve_point_test_off::<NoObject>(
                k,
                Some(NoObject),
                &srf_domain,
                SolverOperation::Minimize,
                curve_pv[0],
                tol,
                None,
                SolutionRequested::All,
            )?;
        } // end no solutions check

        let sols = solutions.as_slice();
        if sols.is_empty() {
            // We should now have a UV curve, so use it unless indicated otherwise.
            if use_3d_only {
                // no hope - can't get edge point to project to surface
                return Err(SolidError::topology(OP, TopologyFailure::NoFootOnSurface));
            }
            // get sUVTrimCurve
            let trim = match uv_trim_curve(b, eu)?.0.cloned() {
                Some(c) => c,
                None => {
                    // (See Hack warning, above.) SER, then NER
                    create_uv_trim_curve(k, b, eu, false, None)?.curve
                }
            };

            // set sUV based on sUVTrimCurve eval clamped to surface domain
            parameter = trim.natural_interval().clamp_value(parameter);
            let pnt = trim.evaluate_point(k, parameter)?;
            uv = domain.clamp_point(Point2::new(pnt.x, pnt.y));
        } else if let [sol] = sols {
            // the solution is the sUV point
            uv = Point2::new(sol.start.param(0)?, sol.start.param(1)?);
        } else if let Some(seam_uv) = seam_pair_uv(k, &solve_surface, sols)? {
            // If on a seam, doesn't matter which we use, the normal vector will be the
            // same. either solution can be the sUVPoint and we have no way of telling
            // which to choose
            uv = seam_uv;
        } else {
            // Multiple solutions or not seam: let sUV be the solution closest to the
            // EdgePoint
            let mut min_dist = BIG_DOUBLE;
            for sol in sols {
                let uv2 = Point2::new(sol.start.param(0)?, sol.start.param(1)?);
                let dist = sol.start.solution_value;
                if dist < min_dist {
                    min_dist = dist;
                    uv = uv2;
                }
            }
        }
    } // end no pUVTrimCurve to use to get a surface uv point branch

    // State: we have the surface parameter in sUV.

    // get oriented Faceuse Normal vector and Binormal, which is Normal <cross> edge
    // tangent
    let mut normal = Vec3::new(0.0, 0.0, 0.0);
    let mut binormal = Vec3::new(0.0, 0.0, 0.0);

    // Deal with potential discontinuities in the curve or surface. If we get a zero
    // binormal, try small step-offs. [230227_crease]
    let crv_step = edge_ivl.length() / 1000.0;
    let srf_step = domain.size() / 1000.0;
    let mut step_uv = uv;
    let tol_uv = srf_step.x.min(srf_step.y) / 2.0;

    // Don't re-evaluate the curve step-offs on each surface iteration.
    let mut crv_eval_lo: Option<Vec3> = None;
    let mut crv_eval_hi: Option<Vec3> = None;

    // Loop over four step-off directions on the surface. In each iteration, try two
    // curve step-offs.
    for ii in 0..5_u32 {
        match ii {
            1 => step_uv = Point2::new(uv.x + srf_step.x, uv.y),
            2 => step_uv = Point2::new(uv.x - srf_step.x, uv.y),
            3 => step_uv = Point2::new(uv.x, uv.y + srf_step.y),
            4 => step_uv = Point2::new(uv.x, uv.y - srf_step.y),
            _ => {}
        }

        // If the step puts us outside the basic domain, don't use it. (But if the input
        // uv is out, skipping it could skip everything.)
        if ii > 0 && !domain.contains_point(step_uv, tol_uv) {
            continue;
        }

        normal = surface.evaluate_normal(
            k,
            step_uv,
            crate::nurbs::knots::Side::Left,
            crate::nurbs::knots::Side::Left,
        )?;

        if face_uv_orientation == OrientType::Opposite {
            normal = -normal;
        }

        // let binormal = crossProduct(Faceuse->Normal, Edgeuse->Normal). A person
        // walking along the Edgeuse from Start to End will have the interior of the
        // face to his lefthand side.
        binormal = normal.cross(curve_pv[1]);

        if binormal.length_squared() > EFF_ZERO_SQ {
            break; // we're good.
        }

        // when Binormal is zero length - try curve eval just below given param point
        if edge_ivl.min() < parameter - crv_step {
            if crv_eval_lo.is_none() {
                let pv = curve_3d.evaluate(k, parameter - crv_step, 1, true, true)?;
                crv_eval_lo = pv.get(1).copied();
            }
            curve_pv[1] = crv_eval_lo.ok_or_else(|| null("curve evaluation"))?;

            // Reverse orientation of curve tangent to conform to edgeuse tangent.
            if eu_orientation == OrientType::Opposite {
                curve_pv[1] = -curve_pv[1];
            }

            binormal = normal.cross(curve_pv[1]);

            if binormal.length_squared() > EFF_ZERO_SQ {
                break; // we're good.
            }
        }

        // when binormal is still zero length - try curve eval just above given param
        // point
        if edge_ivl.max() > parameter + crv_step {
            if crv_eval_hi.is_none() {
                let pv = curve_3d.evaluate(k, parameter + crv_step, 1, true, true)?;
                crv_eval_hi = pv.get(1).copied();
            }
            curve_pv[1] = crv_eval_hi.ok_or_else(|| null("curve evaluation"))?;

            // Reverse orientation of curve tangent to conform to edgeuse tangent.
            if eu_orientation == OrientType::Opposite {
                curve_pv[1] = -curve_pv[1];
            }

            binormal = normal.cross(curve_pv[1]);

            if binormal.length_squared() > EFF_ZERO_SQ {
                break; // we're good.
            }
        }
    } // end loop on surface-evaluation step-offs.

    // give up if binormal is still zero
    if binormal.length_squared() < EFF_ZERO_SQ {
        return Err(SolidError::topology(OP, TopologyFailure::BinormalStaysZero));
    }

    // normalize the return vector
    binormal = binormal.unitize_or_z();

    // store optional output values (ComputeParamDirection: status ignored upstream,
    // passed on by the fork)
    let uv_vector = if options.want_uv_vector {
        Some(surface.compute_param_direction(k, uv, binormal)?)
    } else {
        None
    };

    Ok(BinormalUv {
        eval: BinormalEval {
            point: curve_pv[0],
            binormal,
            tangent: curve_pv[1],
            face_normal: normal,
            uv,
        },
        uv_vector,
    })
}

// ---- NormalizedEvaluate -----------------------------------------------------------------

/// The outputs of [`normalized_evaluate`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalizedEval {
    /// `rPoint`: the UV point (`z = 0`) or the XYZ point along the edge use.
    pub point: Point3,
    /// `*pOptFirstDeriv`: the first derivative along the edge use (with
    /// `want_first_deriv`).
    pub first_deriv: Option<Vec3>,
}

/// The evaluation of `curve` over `ivl` at the oriented normalised parameter.
fn eval_on_curve(
    k: &Kernel,
    curve: &Curve,
    ivl: &Extent1d,
    orient: OrientType,
    normalized_parameter: f64,
    want_first_deriv: bool,
) -> Result<NormalizedEval> {
    // invert parameter for opposite running Edgeuses
    let mut oriented_param = normalized_parameter;
    if orient == OrientType::Opposite {
        oriented_param = 1.0 - oriented_param;
    }

    // get requested evaluation
    if want_first_deriv {
        let pv = curve.evaluate(k, ivl.evaluate(oriented_param), 1, true, true)?;
        let p = *pv
            .first()
            .ok_or_else(|| null("NormalizedEvaluate: no point"))?;
        let mut d = *pv
            .get(1)
            .ok_or_else(|| null("NormalizedEvaluate: no tangent"))?;

        // invert tangents for opposite running Edgeuses
        if orient == OrientType::Opposite {
            d = -d;
        }
        Ok(NormalizedEval {
            point: p,
            first_deriv: Some(d),
        })
    } else {
        Ok(NormalizedEval {
            point: curve.evaluate_point(k, ivl.evaluate(oriented_param))?,
            first_deriv: None,
        })
    }
}

/// `NormalizedEvaluate`: the edge use evaluated at the normalised parameter `0..1`
/// (orientation accounted for): in UV space (`parameter_space`, evaluating the UV trim
/// curve, built first when missing and `ok_to_make_uv_trim_curve`; without one the 3D
/// point is evaluated and dropped to the surface) or in 3D (the edge curve).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] where
/// upstream dereferences null; `SM_ERR` for a point that does not drop to the surface;
/// the evaluators' errors; [`SolidError::Unported`] (module docs).
#[allow(clippy::too_many_arguments)]
pub fn normalized_evaluate(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    normalized_parameter: f64,
    parameter_space: bool,
    want_first_deriv: bool,
    ok_to_make_uv_trim_curve: bool,
) -> Result<NormalizedEval> {
    if !(parameter_space && ok_to_make_uv_trim_curve) {
        return normalized_evaluate_read_only(
            k,
            b,
            eu,
            normalized_parameter,
            parameter_space,
            want_first_deriv,
        );
    }
    orientation(b, eu)?;
    // fetch or create UVTrimCurve (status ignored; the outputs are for the
    // suspicious-result debug check)
    let _ = get_or_create_uv_trim_curve(k, b, eu, true, true, true)?;
    evaluate_with_trim(k, b, eu, normalized_parameter, want_first_deriv)
}

/// [`normalized_evaluate`] in UV space with `ok_to_make_uv_trim_curve` of an edge use
/// whose UV trim curve exists (its own or its mate's): the same evaluation, reading the
/// B-rep only.
///
/// # Errors
///
/// As [`normalized_evaluate`]; [`SolidError::PreparationRequired`] where the edge use
/// has no UV trim curve (nothing is made).
pub(crate) fn normalized_evaluate_existing_uv(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    normalized_parameter: f64,
    want_first_deriv: bool,
) -> Result<NormalizedEval> {
    orientation(b, eu)?;
    if existing_uv_trim_curve(b, eu, true)?.is_none() {
        return Err(SolidError::PreparationRequired { operation: OP });
    }
    evaluate_with_trim(k, b, eu, normalized_parameter, want_first_deriv)
}

/// The UV evaluation of [`normalized_evaluate`] once the edge use's UV trim curve is
/// fetched (shared by [`normalized_evaluate_existing_uv`]).
fn evaluate_with_trim(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    normalized_parameter: f64,
    want_first_deriv: bool,
) -> Result<NormalizedEval> {
    on_own_surfaces(evaluate_fetched(
        k,
        b,
        eu,
        normalized_parameter,
        true,
        want_first_deriv,
        &own_surface(b),
    ))
}

/// [`normalized_evaluate`] without making a UV trim curve (`ok_to_make_uv_trim_curve`
/// false), which only reads the B-rep: the UV trim curve the edge use has, else its 3D
/// point dropped to the surface.
///
/// # Errors
///
/// As [`normalized_evaluate`].
pub fn normalized_evaluate_read_only(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    normalized_parameter: f64,
    parameter_space: bool,
    want_first_deriv: bool,
) -> Result<NormalizedEval> {
    on_own_surfaces(normalized_evaluate_reading(
        k,
        b,
        eu,
        normalized_parameter,
        parameter_space,
        want_first_deriv,
        &own_surface(b),
    ))
}

/// [`normalized_evaluate_read_only`] reading the face surface it drops onto through
/// `surface` (for face `f`: `Some` of the face's surface, or `Some(None)` for a face
/// without one); `Ok(None)` when `surface` gives no answer (`None`) for the face.
///
/// # Errors
///
/// As [`normalized_evaluate`].
pub(crate) fn normalized_evaluate_reading<'s, S: Fn(FaceId) -> Option<Option<&'s Surface>>>(
    k: &Kernel,
    b: &'s Brep,
    eu: EdgeuseId,
    normalized_parameter: f64,
    parameter_space: bool,
    want_first_deriv: bool,
    surface: &S,
) -> Result<Option<NormalizedEval>> {
    if parameter_space {
        orientation(b, eu)?;
        // fetch UVTrimCurve, making none (status ignored; the outputs are for the
        // suspicious-result debug check)
        let _ = existing_uv_trim_curve(b, eu, true)?;
    }
    evaluate_fetched(
        k,
        b,
        eu,
        normalized_parameter,
        parameter_space,
        want_first_deriv,
        surface,
    )
}

/// The face surfaces of `b` itself, for [`evaluate_fetched`].
fn own_surface<'s>(b: &'s Brep) -> impl Fn(FaceId) -> Option<Option<&'s Surface>> + 's {
    move |f| Some(b.face(f).ok().and_then(|face| face.surface.as_ref()))
}

/// The evaluation on the B-rep's own surfaces ([`own_surface`]), which always answer.
fn on_own_surfaces(r: Result<Option<NormalizedEval>>) -> Result<NormalizedEval> {
    r?.ok_or_else(|| SolidError::port_invariant(OP, "NormalizedEvaluate read no face surface"))
}

/// [`normalized_evaluate`] once its UV trim curve is fetched (or made), reading the face
/// surface it drops onto through `surface`; `Ok(None)` when `surface` gives no answer.
fn evaluate_fetched<'s, S: Fn(FaceId) -> Option<Option<&'s Surface>>>(
    k: &Kernel,
    b: &'s Brep,
    eu: EdgeuseId,
    normalized_parameter: f64,
    parameter_space: bool,
    want_first_deriv: bool,
    surface: &S,
) -> Result<Option<NormalizedEval>> {
    let orient = orientation(b, eu)?;

    // when looking for parameter space evaluations let p3dOrUVCurve = UVTrimCurve
    if parameter_space {
        if let Some(trim) = uv_trim_curve(b, eu)?.0 {
            let ivl = trim.natural_interval();
            return eval_on_curve(
                k,
                trim,
                &ivl,
                orient,
                normalized_parameter,
                want_first_deriv,
            )
            .map(Some);
        }
        {
            {
                // Could do: If a uv trim curve cannot be created, then evalute in 3d
                // and drop to the surface. And don't forget pOptFirstDeriv.

                // Evaluate in 3d, then drop to surface.
                let three_d = normalized_evaluate_3d(
                    k,
                    b,
                    eu,
                    orient,
                    normalized_parameter,
                    want_first_deriv,
                )?;
                let f = crate::topology::edgeuse::p1::face(b, eu)?
                    .ok_or_else(|| null("NormalizedEvaluate: edge use without face"))?;
                b.face(f)?;
                let Some(surf) = surface(f) else {
                    return Ok(None);
                };
                let surf = surf
                    // NER(pSurf)
                    .ok_or_else(|| SolidError::topology(OP, TopologyFailure::FaceWithoutSurface))?;
                let domain = surf.natural_uv_domain_with_nurb(k)?;
                let drop = surface_drop_point(k, surf, three_d.point, &domain)?;
                if !drop.found {
                    return Err(SolidError::topology(OP, TopologyFailure::NoFootOnSurface));
                }
                let uv = drop.uv;

                let mut first_deriv = None;
                if let Some(d3) = three_d.first_deriv {
                    // (status ignored upstream, passed on by the fork)
                    let (_, s_su, s_sv) = surf.evaluate_1st_derivatives(k, uv, false, false)?;
                    first_deriv = Some(match vec_lin_comb_two_vectors(s_su, s_sv, d3) {
                        Ok(d) => Vec3::new(d.x, d.y, 0.0),
                        Err(e) if e.is_unported_or_cancelled() => return Err(e),
                        Err(_) => {
                            // Probably a zero vector. Set that one to zero, the other to
                            // the dot product.
                            if s_su.length() < s_sv.length() {
                                Vec3::new(0.0, d3.dot(s_sv), 0.0)
                            } else {
                                Vec3::new(d3.dot(s_su), 0.0, 0.0)
                            }
                        }
                    });
                }

                // All done.
                return Ok(Some(NormalizedEval {
                    point: Vec3::new(uv.x, uv.y, 0.0),
                    first_deriv,
                }));
            }
        }
    }

    // xyzSpace - let p3dOrUVCurve = 3DCurve and sIvl = Edge domain.
    normalized_evaluate_3d(k, b, eu, orient, normalized_parameter, want_first_deriv).map(Some)
}

/// `NormalizedEvaluate(t, FALSE, ..)`: the edge curve evaluated at the oriented
/// normalised parameter.
fn normalized_evaluate_3d(
    k: &Kernel,
    b: &Brep,
    eu: EdgeuseId,
    orient: OrientType,
    normalized_parameter: f64,
    want_first_deriv: bool,
) -> Result<NormalizedEval> {
    // NER(pE)
    let e = edge(b, eu)?.ok_or_else(|| {
        Failure::new(
            OP,
            TopologyFailure::Defect {
                why: "the edge use names no edge; the kernel makes an edge's edge \
                    uses with the edge and deletes them with it",
            },
        )
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
    })?;
    let rec = b.edge(e)?;
    let curve = rec
        .curve
        .as_ref()
        // NER(p3dOrUVCurve)
        .ok_or_else(|| SolidError::topology(OP, TopologyFailure::EdgeWithoutCurve))?;
    eval_on_curve(
        k,
        curve,
        &rec.interval,
        orient,
        normalized_parameter,
        want_first_deriv,
    )
}

/// Not upstream (WR-543; the fork's tangency-labs/smlib@e36feb1, `sm_LongestDropRun`):
/// of a drop that came back in pieces (`DropAndTrimCurve` splits it where the curve
/// leaves the face's domain or crosses a seam), the longest run (in edge parameter) of
/// pieces that meet end to end, as one UV trim curve: the second starts at the edge
/// parameter where the first ends (`SM_ARE_SAME`) and at the same `(u, v)`, to within
/// the drop's UV tolerances (`tol_3d` over one plus the surface's derivative lengths)
/// and within the join's own end-to-end tolerance
/// ([`BSplineCurve::ends_meet_for_join`]). A run of several pieces is joined exactly
/// (`CreateByJoining`); no piece is reparameterized. `uv_curves` keeps the one run.
///
/// The join's test matters where the drop's UV tolerances are the looser ones (a large
/// `tol_3d` over a slow surface): pieces that end up to `u_tol` apart are not end to end,
/// and joining them fails (`segments not end to end`) instead of the longest piece being
/// kept. (Not in the fork, whose run test is the UV tolerances alone.)
///
/// # Errors
///
/// Those of the derivative lengths, the end evaluations and the join.
fn longest_drop_run(
    k: &Kernel,
    surface: &Surface,
    tol_3d: f64,
    uv_curves: &mut Vec<Curve>,
) -> Result<()> {
    if uv_curves.len() < 2 {
        return Ok(());
    }

    // UV tolerances proportional to the surface's 3d/UV mapping, as DropCurve's
    let speeds = crate::surface::surface::p7::approx_derivative_lengths(
        k,
        surface,
        &surface.natural_uv_domain_with_nurb(k)?,
    )?;
    let u_tol = tol_3d / (speeds.x + 1.0);
    let v_tol = tol_3d / (speeds.y + 1.0);

    // by the start of the edge interval each piece was dropped from
    uv_curves.sort_by(|a, b| {
        a.natural_interval()
            .min()
            .total_cmp(&b.natural_interval().min())
    });

    let pieces = core::mem::take(uv_curves);
    let mut ends = Vec::with_capacity(pieces.len());
    for piece in &pieces {
        let e = piece.ends(k)?;
        ends.push((piece.natural_interval(), e.start, e.end));
    }
    let mut best: Option<Curve> = None;
    let mut best_len = -1.0;
    let mut first = 0;
    while first < pieces.len() {
        // extend the run from `first` through every piece that meets the previous one
        let mut last = first;
        while let (Some(prev), Some(next)) = (ends.get(last), ends.get(last + 1)) {
            if !are_same(prev.0.max(), next.0.min())
                || (prev.2.x - next.1.x).abs() > u_tol
                || (prev.2.y - next.1.y).abs() > v_tol
                || !BSplineCurve::ends_meet_for_join(prev.2, next.1)
            {
                break;
            }
            last += 1;
        }

        // the run as one curve
        let run_pieces = pieces.get(first..=last).unwrap_or_default();
        let run = if let [one] = run_pieces {
            one.clone()
        } else {
            let mut segments = Vec::with_capacity(run_pieces.len());
            for piece in run_pieces {
                segments.push(b_spline_part(piece)?);
            }
            Curve::from(BSplineCurve::create_by_joining(&mut segments, None)?)
        };

        // keep the longest run
        let len = run.natural_interval().length();
        if len > best_len {
            best = Some(run);
            best_len = len;
        }
        first = last + 1;
    }
    uv_curves.extend(best);
    Ok(())
}

/// The B-spline data of a UV trim piece (an `SmBSplineCurve` kind: a B-spline, line or
/// conic; a line's NURB is made when it has none), as `CreateByJoining` reads it.
fn b_spline_part(c: &Curve) -> Result<BSplineCurve> {
    Ok(match c.kind() {
        CurveKind::BSpline(b) => b.clone(),
        CurveKind::Line(l) => l.base_with_nurb()?.into_owned(),
        CurveKind::Ellipse(e) => e.base().clone(),
        CurveKind::Hyperbola(h) => h.base().clone(),
        CurveKind::Parabola(p) => p.base().clone(),
        _ => return Err(null("CreateUVTrimCurve: a dropped piece is not a B-spline")),
    })
}

/// `SmEdgeuse::CalcSectorAngle`: the sector angle, in radians, from `eu` to its radial
/// edge use at the edge parameter `parameter` (in the edge's interval). The two
/// binormals are evaluated with `bUse3DOnly = FALSE` (a missing UV trim curve is built);
/// the angle between them is taken through the binormals' average when it lies inside
/// the sector from `eu`'s binormal round its face normal, otherwise its complement to a
/// whole turn. Where the average vanishes the angle between the binormals stands.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an edge use without radial (upstream dereferences
/// null) or zero binormals (`AngleBetween`); those of [`evaluate_binormal`].
pub fn calc_sector_angle(k: &Kernel, b: &mut Brep, eu: EdgeuseId, parameter: f64) -> Result<f64> {
    let eu2 = crate::topology::edgeuse::p1::radial(b, eu)?.ok_or(SolidError::InvalidInput {
        operation: OP,
        reason: "CalcSectorAngle: edge use without radial",
    })?;

    // sBinormalAvg will always point into the acute angle between sBinormal1 and
    // sBinormal2. If sBinormalAvg IsInsideSector( sBinormal1, sNormal1, sBinormal2 ),
    // then the sector angle is the angle returned by AngleBetween. Else, the sector
    // angle is 2pi - AngleBetween.
    let first = evaluate_binormal(k, b, eu, parameter, false, &BinormalOptions::default())?;
    let second = evaluate_binormal(k, b, eu2, parameter, false, &BinormalOptions::default())?;
    let (binormal1, normal1, binormal2) = (first.binormal, first.face_normal, second.binormal);

    let mut angle = binormal1.angle_between(binormal2)?;

    let average = (binormal1 + binormal2) / 2.0;
    // If Unitize fails, then dAngle = pi
    if let Ok(average) = average.unitize() {
        if !average.is_inside_sector(binormal1, normal1, binormal2) {
            angle = crate::math::TWO_PI - angle;
        }
    }
    Ok(angle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::Edgeuse;

    fn lone() -> (Brep, EdgeuseId) {
        let mut b = Brep::new();
        let eu = b.add_edgeuse_record(Edgeuse::default()).unwrap();
        (b, eu)
    }

    /// A `sphere_no_pole` of radius 23.567 centred at `center` (built at the origin and moved
    /// there), each B-spline edge's tolerance set to 1e-5, with one trimmed use of each.
    fn sphere_edges(center: Point3) -> (Kernel, Brep, Vec<(EdgeId, EdgeuseId, Curve)>) {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_sphere_no_pole(&mut k, center, 23.567)
            .unwrap();
        let mut out = Vec::new();
        let edges: Vec<EdgeId> = b.edges().iter().map(|(e, _)| e).collect();
        for e in edges {
            let rec = b.edge(e).unwrap();
            if !matches!(
                rec.curve.as_ref().map(Curve::kind),
                Some(CurveKind::BSpline(_))
            ) {
                continue;
            }
            let Some((eu, trim)) = rec.edgeuses.iter().find_map(|&eu| {
                let t = b.edgeuse(eu).ok()?.uv_trim_curve.as_ref()?.clone();
                Some((eu, t))
            }) else {
                continue;
            };
            edge_set_tolerance(&mut b, e, 1e-5).unwrap();
            out.push((e, eu, trim));
        }
        (k, b, out)
    }

    /// gen1157's centre, about 8e8 from the origin.
    const FAR: Point3 = Point3::new(-423123980.82400393, -605023292.1569545, 445743038.3134029);

    /// #18166: at the origin a sphere's edges, given upstream's distance as 1 (so every
    /// enclosed bound below it is a lowering), are lowered below it.
    #[test]
    fn a_sphere_s_edges_are_lowered_to_their_bound() {
        let (k, mut b, edges) = sphere_edges(Point3::new(0.0, 0.0, 0.0));
        assert!(!edges.is_empty());
        for (_, eu, trim) in &edges {
            let raise = trim_raise_distance(&k, &mut b, *eu, trim, 1.0).unwrap();
            assert!(raise > 0.0 && raise < 1.0, "lowered, got {raise:e}");
        }
    }

    /// A file import's sampled trim raise makes the same edges keep upstream's distance, not be
    /// lowered to a proved bound: no trim is proved, and the proved raise is back afterwards.
    #[test]
    fn without_the_proof_a_sphere_s_edges_keep_upstream_s_raise() {
        let (mut k, mut b, edges) = sphere_edges(Point3::new(0.0, 0.0, 0.0));
        assert!(!edges.is_empty());
        k.with_sampled_trim_raise(|k| {
            for (_, eu, trim) in &edges {
                assert_eq!(trim_raise_distance(k, &mut b, *eu, trim, 1.0).unwrap(), 1.0);
            }
        });
        assert!(k.caches.prove_lazy_trim_raise);
    }

    /// #18166: a lowering must cover the evaluation's rounding where the edge lies. gen1157's
    /// proved bound `B = 7.392e-6`, lowered to `2 B`, covers it at the origin; 8e8 away the
    /// edge curves' evaluation rounding (`sqrt 3` times their point error bound there, about
    /// 4e-5) is more than `2 B` leaves over `B`. There [`trim_raise_distance`] keeps
    /// upstream's raise for an edge whose bound leaves the rounding uncovered, and lowers
    /// only where it is covered. The check is of the edge where the raise is made: gen1157
    /// itself is lowered at the origin, where it passes, and then moved 8e8 away, which this
    /// guard does not see (a lowering carried across a later move: #19910).
    #[test]
    fn a_lowering_covers_the_evaluation_rounding_where_the_edge_lies() {
        let (b_proved, lowered) = (7.392e-6, 1.4784e-5);
        let (_, near, edges) = sphere_edges(Point3::new(0.0, 0.0, 0.0));
        assert!(!edges.is_empty());
        for (e, _, _) in &edges {
            assert!(lowered_covers_rounding(&near, *e, b_proved, lowered).unwrap());
        }
        let (k, mut far, far_edges) = sphere_edges(FAR);
        assert_eq!(far_edges.len(), edges.len());
        let mut kept = 0;
        for (e, eu, trim) in &far_edges {
            assert!(!lowered_covers_rounding(&far, *e, b_proved, lowered).unwrap());
            let raise = trim_raise_distance(&k, &mut far, *eu, trim, 1.0).unwrap();
            if raise == 1.0 {
                kept += 1;
            } else {
                // (a lowering far away: its tolerance covers the bound and the rounding)
                let tol = far.edge(*e).unwrap().tolerance;
                assert!(lowered_covers_rounding(&far, *e, raise, tol.max(2.0 * raise)).unwrap());
            }
        }
        assert!(kept > 0, "the rounding check keeps no far raise");
    }

    #[test]
    fn no_edge_is_an_error() {
        let (mut b, eu) = lone();
        let k = Kernel::default();
        assert!(create_uv_trim_curve(&k, &mut b, eu, false, None).is_err());
        assert!(evaluate_binormal(&k, &mut b, eu, 0.0, true, &BinormalOptions::default()).is_err());
        assert!(normalized_evaluate(&k, &mut b, eu, 0.5, false, false, true).is_err());
    }

    #[test]
    fn missing_curve_is_not_built_when_not_allowed() {
        let (mut b, eu) = lone();
        let k = Kernel::default();
        let r = get_or_create_uv_trim_curve(&k, &mut b, eu, false, false, false);
        // the mate lookup of an unlinked edge use fails like upstream's null dereference
        // or finds no curve; it never builds one
        if let Ok(r) = r {
            assert!(r.status.is_ok());
            assert_eq!(r.dist_to_surf, -1.0);
        }
    }

    /// The projected arc crosses the cylinder's 360-degree cut. Interpolating
    /// wrapped angles invents a curve across the opposite side of the cylinder.
    #[test]
    fn point_drop_interpolation_follows_an_arc_across_a_periodic_seam() {
        use crate::nurbs::b_spline_curve::p1::NurbCircleParam;
        use crate::primitive::api_primitives::{create_cylinder, create_cylinder_surface};

        let mut k = Kernel::default();
        let mut b = create_cylinder(&mut k, Vec3::ZERO, 5.0, 3.0).unwrap();
        let (eu, e) = b
            .edgeuses()
            .iter()
            .find_map(|(eu, u)| u.edge.map(|e| (eu, e)))
            .unwrap();
        let surface = Surface::from(create_cylinder_surface(&k, Vec3::ZERO, 5.0, 3.0).unwrap());
        let curve = Curve::from(
            BSplineCurve::create_circle_segment(
                3,
                &Axis2Placement::default(),
                5.0,
                -10.0,
                10.0,
                NurbCircleParam::Quadratic,
            )
            .unwrap(),
        );
        let interval = curve.natural_interval();
        let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let mut fail = DropCurveFail::default();
        let made = brute_force(
            &k, &mut b, eu, e, &surface, &domain, &curve, &interval, false, true, &mut fail,
        )
        .unwrap();
        let tolerance = b.edge(e).unwrap().tolerance;
        for i in 0..=400 {
            let t = interval.evaluate(f64::from(i) / 400.0);
            let uv = made.curve.evaluate_point(&k, t).unwrap();
            // Evaluate the periodic extension in the surface's own parameters.
            // The native evaluator clamps outside its active knot interval.
            let u = domain.u_interval();
            let wrapped_u = u.min() + (uv.x - u.min()).rem_euclid(u.length());
            let lifted = surface
                .evaluate_point(&k, Point2::new(wrapped_u, uv.y))
                .unwrap();
            let expected = curve.evaluate_point(&k, t).unwrap();
            assert!(
                lifted.distance(expected) <= tolerance,
                "at {t}: gap {}, tolerance {tolerance}",
                lifted.distance(expected)
            );
        }
    }

    #[test]
    fn uv_trim_fix_survives_edge_zone_kernel() {
        use crate::nurbs::b_spline_curve::p1::NurbCircleParam;
        use crate::nurbs::b_spline_curve::BSplineCurve;

        for enabled in [false, true] {
            let mut k = Kernel::default();
            k.fixes.bspline_trim_preserves_uv_spans = enabled;
            let uv = BSplineCurve::create_circle_segment(
                2,
                &Axis2Placement::default(),
                0.14,
                0.0,
                180.0,
                NurbCircleParam::Quadratic,
            )
            .unwrap();
            let domain = uv.natural_interval();
            let knot = uv.knots(None).unwrap().0[1];
            let original = Curve::from(uv);
            let mut b = Brep::new();
            let v0 = b.add_vertex(Vec3::ZERO).unwrap();
            let v1 = b.add_vertex(Vec3::X).unwrap();
            let edge = b.add_edge(v0, v1).unwrap();
            let this = b
                .add_edgeuse_record(Edgeuse {
                    edge: Some(edge),
                    ..Edgeuse::default()
                })
                .unwrap();
            let other = b
                .add_edgeuse_record(Edgeuse {
                    edge: Some(edge),
                    uv_trim_curve: Some(original.clone()),
                    ..Edgeuse::default()
                })
                .unwrap();
            b.edge_mut(edge).unwrap().edgeuses = vec![this, other];
            let mut ivl =
                Extent1d::from_min_max(knot - 3.5e-6 * domain.length(), domain.evaluate(0.9));
            trim_other_uv_curves(&k, &mut b, edge, this, &mut ivl).unwrap();
            let trimmed = b.edgeuse(other).unwrap().uv_trim_curve.as_ref().unwrap();
            let gap = (0..=64)
                .map(|i| {
                    let t = ivl.evaluate(f64::from(i) / 64.0);
                    let gap = (38.0
                        * (trimmed.evaluate_point(&k, t).unwrap()
                            - original.evaluate_point(&k, t).unwrap()))
                    .length();
                    assert!(gap.is_finite());
                    gap
                })
                .fold(0.0, f64::max);
            if enabled {
                assert!(gap < 1e-12, "enabled fix was lost: gap {gap}");
            } else {
                // the circle segment is rational: WR-575 keeps its short end span, so the
                // trimmed curve is the original's sub-arc with or without the UV fix
                assert!(gap < 1e-12, "rational end span was lost: gap {gap}");
            }
        }
    }

    /// As [`uv_trim_fix_survives_edge_zone_kernel`] on a polynomial UV curve (weights
    /// 1), whose short end span upstream's cleanup still removes by default: the UV fix
    /// must reach the edge-zone kernel the trim runs in, or the span goes and the curve
    /// moves.
    #[test]
    fn uv_trim_fix_survives_edge_zone_kernel_on_a_polynomial_curve() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::math::{CPoint, NOW};
        use crate::nurbs::nurbs_crv::allocate_nurb_curve;

        for enabled in [false, true] {
            let mut k = Kernel::default();
            k.fixes.bspline_trim_preserves_uv_spans = enabled;
            let r = 0.14;
            let mut nurb = allocate_nurb_curve(4, 2, 7).unwrap();
            nurb.cpts_mut().copy_from_slice(&[
                CPoint::new(r, 0.0, 0.0, NOW),
                CPoint::new(r, r, 0.0, NOW),
                CPoint::new(0.0, r, 0.0, NOW),
                CPoint::new(-r, r, 0.0, NOW),
                CPoint::new(-r, 0.0, 0.0, NOW),
            ]);
            nurb.knots_mut()
                .copy_from_slice(&[0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0]);
            let original = Curve::from(BSplineCurve::from_nurb(nurb, 2, false));
            let mut b = Brep::new();
            let v0 = b.add_vertex(Vec3::ZERO).unwrap();
            let v1 = b.add_vertex(Vec3::X).unwrap();
            let edge = b.add_edge(v0, v1).unwrap();
            let this = b
                .add_edgeuse_record(Edgeuse {
                    edge: Some(edge),
                    ..Edgeuse::default()
                })
                .unwrap();
            let other = b
                .add_edgeuse_record(Edgeuse {
                    edge: Some(edge),
                    uv_trim_curve: Some(original.clone()),
                    ..Edgeuse::default()
                })
                .unwrap();
            b.edge_mut(edge).unwrap().edgeuses = vec![this, other];
            let mut ivl = Extent1d::from_min_max(0.5 - 3.5e-6, 0.9);
            trim_other_uv_curves(&k, &mut b, edge, this, &mut ivl).unwrap();
            let trimmed = b.edgeuse(other).unwrap().uv_trim_curve.as_ref().unwrap();
            let gap = (0..=64)
                .map(|i| {
                    let t = ivl.evaluate(f64::from(i) / 64.0);
                    (trimmed.evaluate_point(&k, t).unwrap()
                        - original.evaluate_point(&k, t).unwrap())
                    .length()
                })
                .fold(0.0, f64::max);
            if enabled {
                assert!(gap < 1e-12, "enabled fix was lost: gap {gap}");
            } else {
                assert!(gap > 1e-9, "default gap {gap}");
            }
        }
    }

    /// WR-543 (tangency-labs/solidcore#1383; the fork's tangency-labs/smlib@e36feb1):
    /// of three drop pieces, two that meet end to end (edge parameters [0, 0.4] and
    /// [0.4, 1], the same (u, v) where they meet) are joined on their own parameters and
    /// kept; a shorter piece apart from them is dropped.
    #[test]
    fn longest_drop_run_joins_the_pieces_that_meet() {
        use crate::math::Vec3;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::{CPoint, NOW};

        let piece = |t0: f64, t1: f64, a: [f64; 2], b: [f64; 2]| {
            let pts = vec![
                CPoint::new(a[0], a[1], 0.0, NOW),
                CPoint::new(b[0], b[1], 0.0, NOW),
            ];
            let nl = NlCurve::new(pts, 1, vec![t0, t0, t1, t1]).unwrap();
            Curve::from(BSplineCurve::from_nurb(nl, 2, false))
        };
        let k = Kernel::default();
        let plane = Surface::from(
            crate::primitive::api_primitives::create_plane_surface(
                Vec3::new(0.0, 0.0, 0.0),
                1.0,
                1.0,
            )
            .unwrap(),
        );
        let mut pieces = vec![
            piece(0.4, 1.0, [0.4, 0.0], [1.0, 0.0]),
            piece(-0.5, -0.3, [0.5, 0.5], [0.6, 0.5]),
            piece(0.0, 0.4, [0.0, 0.0], [0.4, 0.0]),
        ];
        longest_drop_run(&k, &plane, 1e-6, &mut pieces).unwrap();
        assert_eq!(pieces.len(), 1);
        let run = &pieces[0];
        let ivl = run.natural_interval();
        assert!(
            are_same(ivl.min(), 0.0) && are_same(ivl.max(), 1.0),
            "{ivl:?}"
        );
        let p = run.evaluate_point(&k, 0.4).unwrap();
        assert!((p.x - 0.4).abs() < 1e-12 && p.y.abs() < 1e-12, "{p:?}");
    }

    /// Two drop pieces on consecutive edge parameters whose ends are 0.54 apart in UV, the
    /// pair `remove_topo_edges_mouse_two_struts` drops at its edge's zone tolerance 3.63.
    /// The drop's UV tolerances (3.63 over one plus the plane's speeds, about 1.8) pass the
    /// gap; the join's own end test (`1000 sqrt(eps) (1 + 28.56) = 0.0296`) does not. They
    /// are no run: the longer piece is kept on its own, where joining them failed with
    /// `segments not end to end`.
    #[test]
    fn longest_drop_run_does_not_join_pieces_the_join_refuses() {
        use crate::math::Vec3;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::{CPoint, NOW};

        let piece = |t0: f64, t1: f64, a: [f64; 2], b: [f64; 2]| {
            let pts = vec![
                CPoint::new(a[0], a[1], 0.0, NOW),
                CPoint::new(b[0], b[1], 0.0, NOW),
            ];
            let nl = NlCurve::new(pts, 1, vec![t0, t0, t1, t1]).unwrap();
            Curve::from(BSplineCurve::from_nurb(nl, 2, false))
        };
        let k = Kernel::default();
        let plane = Surface::from(
            crate::primitive::api_primitives::create_plane_surface(
                Vec3::new(0.0, 0.0, 0.0),
                1.0,
                1.0,
            )
            .unwrap(),
        );
        let (a_end, b_start) = (
            [28.560_706_814_852_54, 2.175_278_413_029_963],
            [28.465_894_268_602_494, 1.641_316_435_609_047_7],
        );
        // the gap passes the drop's UV tolerances and fails the join's end test
        let gap = Vec3::new(a_end[0] - b_start[0], a_end[1] - b_start[1], 0.0).length();
        assert!(gap > 0.5 && gap < 3.63 / 2.0, "{gap}");
        assert!(!BSplineCurve::ends_meet_for_join(
            Vec3::new(a_end[0], a_end[1], 0.0),
            Vec3::new(b_start[0], b_start[1], 0.0)
        ));
        let mut pieces = vec![
            piece(
                0.0,
                0.847_722_980_184_200_7,
                [28.560_706_814_852_54, 0.0],
                a_end,
            ),
            piece(
                0.847_722_980_184_200_7,
                26.992_802_825_478_737,
                b_start,
                [0.651_850_362_241_519_7, 1.726_906_332_096_931],
            ),
        ];
        longest_drop_run(&k, &plane, 3.630_206_397_575_422_6, &mut pieces).unwrap();
        assert_eq!(pieces.len(), 1);
        let ivl = pieces[0].natural_interval();
        assert!(
            are_same(ivl.min(), 0.847_722_980_184_200_7)
                && are_same(ivl.max(), 26.992_802_825_478_737),
            "the longer piece is kept: {ivl:?}"
        );
    }

    /// A 1 x 2 x 3 box, one of its line edges with a trim moved `d` off it in UV, square to
    /// the trim and toward the middle of its plane's parameter box (a trim off the box is
    /// not bounded): the box, the edge and the edge use whose trim moved.
    fn box_with_moved_trim(d: f64) -> (Kernel, Brep, EdgeId, EdgeuseId) {
        moved_trim_box([1.0, 2.0, 3.0], d, 0).unwrap()
    }

    /// [`box_with_moved_trim`] for a box of sides `dims`, the trim moved the `n`-th of the
    /// held trims of the edges with two or more (in edge order, then use order); `None` past
    /// the last.
    fn moved_trim_box(
        dims: [f64; 3],
        d: f64,
        n: usize,
    ) -> Option<(Kernel, Brep, EdgeId, EdgeuseId)> {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::{CPoint, NOW, NOZ};
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_box(
            &mut k,
            Point3::new(0.0, 0.0, 0.0),
            dims[0],
            dims[1],
            dims[2],
        )
        .unwrap();
        let (e, x) = b
            .edge_list()
            .iter()
            .flat_map(|&e| {
                let uses = &b.edge(e).unwrap().edgeuses;
                let trimmed: Vec<EdgeuseId> = uses
                    .iter()
                    .copied()
                    .filter(|&x| b.edgeuse(x).unwrap().uv_trim_curve.is_some())
                    .collect();
                let held = if trimmed.len() >= 2 {
                    trimmed
                } else {
                    Vec::new()
                };
                held.into_iter().map(move |x| (e, x))
            })
            .nth(n)?;
        let iv = b.edge(e).unwrap().interval;
        let [a, z] = [iv.min(), iv.max()].map(|t| {
            let trim = b.edgeuse(x).unwrap().uv_trim_curve.as_ref().unwrap();
            trim.evaluate_point(&k, t).unwrap()
        });
        let f = crate::topology::edgeuse::p1::face(&b, x).unwrap().unwrap();
        let bx = b
            .face(f)
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .natural_uv_domain_if_built();
        let (mx, my) = (0.5 * (bx.min.x + bx.max.x), 0.5 * (bx.min.y + bx.max.y));
        let (nx, ny) = (a.y - z.y, z.x - a.x);
        let len = (nx * nx + ny * ny).sqrt();
        let toward = if (mx - a.x) * nx + (my - a.y) * ny >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let (dx, dy) = (toward * d * nx / len, toward * d * ny / len);
        let pw = vec![
            CPoint::new(a.x + dx, a.y + dy, NOZ, NOW),
            CPoint::new(z.x + dx, z.y + dy, NOZ, NOW),
        ];
        let nl = NlCurve::new(pw, 1, vec![iv.min(), iv.min(), iv.max(), iv.max()]).unwrap();
        b.edgeuse_mut(x).unwrap().uv_trim_curve =
            Some(Curve::from(BSplineCurve::from_nurb(nl, 2, false)));
        Some((k, b, e, x))
    }

    /// #20373 review: a joined edge's trim cover ([`cover_joined_edge_trims`]) keeps a raise
    /// only where the raised tube and vertex balls are proved clear of topology they do not
    /// bound. On a 1 x 2 x 3 box an edge whose trim moved `1e-4` is raised to its bound (the
    /// tube is `1` and more from every face and edge it does not bound). On a box `5e-5` thick
    /// the same raise would reach the face across the thickness: an edge whose trim moved
    /// `1e-4` in a wide face, with a bound above its tolerance, keeps the tolerances it came
    /// with, and so do its vertices.
    #[test]
    fn a_joined_trim_raise_whose_tube_would_reach_is_withheld() {
        let (k, mut b, e, x) = box_with_moved_trim(1e-4);
        let before = b.edge(e).unwrap().tolerance;
        let trim = b.edgeuse(x).unwrap().uv_trim_curve.clone().unwrap();
        let bound = raised_trim_bound(&k, &b, x, &trim, before, true)
            .unwrap()
            .unwrap();
        assert!(bound > before, "{bound:e} over {before:e}");
        cover_joined_edge_trims(&k, &mut b, e).unwrap();
        assert!(b.edge(e).unwrap().tolerance >= bound, "a clear raise kept");
        let mut withheld = 0;
        for n in 0.. {
            let Some((k, mut b, e, x)) = moved_trim_box([1.0, 2.0, 5e-5], 1e-4, n) else {
                break;
            };
            let before = b.edge(e).unwrap().tolerance;
            let trim = b.edgeuse(x).unwrap().uv_trim_curve.clone().unwrap();
            let Some(bound) = raised_trim_bound(&k, &b, x, &trim, before, true).unwrap() else {
                continue;
            };
            if !(bound > before && within_edge_tolerance_limit(&b, bound)) {
                continue;
            }
            let (v0, v1) = crate::topology::edge::start_end_vertices(&b, e).unwrap();
            let ends: Vec<_> = [v0, v1].into_iter().flatten().collect();
            let held: Vec<f64> = ends
                .iter()
                .map(|&v| b.vertex(v).unwrap().tolerance)
                .collect();
            // (the same box raised to the bound: its tube reaches)
            let (_, mut probe, _, _) = moved_trim_box([1.0, 2.0, 5e-5], 1e-4, n).unwrap();
            crate::topology::edge::set_tolerance(&mut probe, e, bound, true, true).unwrap();
            if !crate::topology::brep::validation::edge_tube_reaches(&k, &mut probe, e).unwrap() {
                continue;
            }
            cover_joined_edge_trims(&k, &mut b, e).unwrap();
            assert_eq!(b.edge(e).unwrap().tolerance, before, "edge {}", e.index());
            for (&v, &t) in ends.iter().zip(&held) {
                assert_eq!(b.vertex(v).unwrap().tolerance, t);
            }
            withheld += 1;
        }
        assert!(withheld >= 1, "no raise that would reach");
    }

    /// #20373 review r4: an error once a joined edge may be raised returns with the edge's
    /// and its vertices' old tolerances put back, so no raise stays without its reach proof.
    /// The box edge whose trim moved `1e-4` (a raise that is kept when nothing fails) is run
    /// cancelled at 24 polls spread over those the whole cover makes and at its last two:
    /// each cancelled run returns `Cancelled` with every tolerance as it came (without the
    /// restore, the run cancelled at poll 1401 of 4199 keeps a raise).
    #[test]
    fn a_joined_trim_raise_cut_short_by_an_error_is_put_back() {
        use crate::kernel::CancelToken;
        let state = |b: &Brep, e: EdgeId| {
            let (v0, v1) = crate::topology::edge::start_end_vertices(b, e).unwrap();
            let mut t = vec![b.edge(e).unwrap().tolerance];
            for v in [v0, v1].into_iter().flatten() {
                t.push(b.vertex(v).unwrap().tolerance);
            }
            t
        };
        // (the polls made, and for a run that failed: whether it was `Cancelled`, whether
        // every tolerance is back)
        let run = |n: u64| {
            let (mut k, mut b, e, _) = box_with_moved_trim(1e-4);
            let before = state(&b, e);
            let token = CancelToken::after_polls(n);
            k.set_cancel_token(token.clone());
            let r = cover_joined_edge_trims(&k, &mut b, e);
            k.clear_cancel_token();
            let made = token.polls().unwrap();
            match r {
                Ok(()) => (made, None, state(&b, e) != before),
                Err(err) => (
                    made,
                    Some((err.is_cancelled(), state(&b, e) == before)),
                    false,
                ),
            }
        };
        let (all, failed, raised) = run(u64::MAX);
        assert!(failed.is_none() && raised, "uncancelled, the raise is kept");
        assert!(all > 1, "the cover polls its kernel {all} times");
        // (24 polls spread over the run, and its last two: cancelled in the reach proofs,
        // after the raise)
        let step = usize::try_from(all.div_ceil(24)).unwrap().max(1);
        for n in (1..=all).step_by(step).chain([all - 1, all]) {
            let (_, r, _) = run(n);
            assert_eq!(r, Some((true, true)), "cancelled at poll {n} of {all}");
        }
    }

    /// #20373: a settle round ([`cover_raised_trims`] without `within`) takes a trim's bound
    /// only up to [`SETTLE_RISE`] above the edge's tolerance. A box edge with a trim moved
    /// `1e-4` off it, at a tolerance 1 % under the trim's bound, is left as it is by a settle
    /// round (and raised over the bound by a first round, which takes bounds uncapped); at
    /// a tolerance a thousandth under the bound it is raised to the bound.
    #[test]
    fn a_settle_round_refuses_a_bound_past_its_cap() {
        let (k, mut b, e, x) = box_with_moved_trim(1e-4);
        let trim = b.edgeuse(x).unwrap().uv_trim_curve.clone().unwrap();
        let bound = |b: &Brep, t: f64| {
            raised_trim_bound(&k, b, x, &trim, t, false)
                .unwrap()
                .unwrap()
        };
        let cap = |t: f64| t * (1.0 + SETTLE_RISE) * (1.0 + 4.0 * f64::EPSILON);
        let far = bound(&b, 1e-5) / 1.01;
        b.edge_mut(e).unwrap().tolerance = far;
        let over = bound(&b, far);
        assert!(over > cap(far), "{over:e} past the cap at {far:e}");
        cover_raised_trims(&k, &mut b, e, false).unwrap();
        assert_eq!(b.edge(e).unwrap().tolerance, far, "refused");
        cover_raised_trims(&k, &mut b, e, true).unwrap();
        assert!(
            b.edge(e).unwrap().tolerance >= over,
            "a first round takes it"
        );
        let near = over / (1.0 + 1e-3);
        b.edge_mut(e).unwrap().tolerance = near;
        let within = bound(&b, near);
        assert!(
            within > near && within <= cap(near),
            "{within:e} under the cap at {near:e}"
        );
        cover_raised_trims(&k, &mut b, e, false).unwrap();
        assert_eq!(b.edge(e).unwrap().tolerance, within, "taken");
    }

    /// #20373: a settle round's bound of a stitched round profile's circle trim is the
    /// structured bound, taken at once ([`structured_trim_deviation_bound`]), not the
    /// bisection's: the bound [`raised_trim_bound`] gives without `within` is that structured
    /// bound bit for bit, which the bisection's full bound at the same fit is not.
    #[test]
    fn a_settle_round_takes_a_circle_trims_structured_bound_at_once() {
        use crate::curve::arc::ArcDefinition;
        use crate::curve::ellipse::Ellipse;
        use crate::curve::path::{PathTolerances, Segment};
        use crate::sweep::segments::sweep_along_segments;
        let mut k = Kernel::default();
        let d = ArcDefinition {
            centre: Point3::ZERO,
            axis: Vec3::X,
            reference: Vec3::Y,
            radius: 12.5,
            start: 0.0,
            end: std::f64::consts::TAU,
        };
        let profile = vec![Curve::from(Ellipse::arc(&k, &d).unwrap())];
        let path = [Segment::Line {
            start: Point3::ZERO,
            end: Point3::new(50.0, 0.0, 0.0),
        }];
        let tol = PathTolerances {
            gap: 1e-9,
            turn: 1e-12,
            length: 1e-12,
            closed: 1e-6,
        };
        let (b, _) = sweep_along_segments(&mut k, &profile, &path, true, &tol).unwrap();
        let mut seen = 0;
        for (e, rec) in b.edges().iter() {
            if !matches!(
                rec.curve.as_ref().map(Curve::kind),
                Some(CurveKind::Ellipse(_))
            ) {
                continue;
            }
            for &x in &rec.edgeuses {
                let Some(trim) = b.edgeuse(x).unwrap().uv_trim_curve.as_ref() else {
                    continue;
                };
                if matches!(trim.kind(), CurveKind::Line(_)) {
                    continue;
                }
                let t = b.edge(e).unwrap().tolerance;
                let at_once = structured_trim_deviation_bound(&k, &b, x, trim)
                    .unwrap()
                    .unwrap();
                assert!(at_once <= t, "{at_once:e} within {t:e}");
                let full = trim_deviation_bound(&k, &b, x, trim, t, [0.0, f64::INFINITY], None)
                    .unwrap()
                    .unwrap();
                assert_ne!(full.to_bits(), at_once.to_bits(), "the bisection's differs");
                let settled = raised_trim_bound(&k, &b, x, trim, t, false)
                    .unwrap()
                    .unwrap();
                assert_eq!(settled.to_bits(), at_once.to_bits(), "{settled:e} at once");
                seen += 1;
            }
        }
        assert!(seen >= 1, "a circle trim that is not a UV line");
    }
}
