//! `SmFace` queries, part 3: the tight bounding box of a face
//! (`SmFace::CalculateTightBoundingBox`), the classification of a UV line against a face
//! (`sm_FaceClassifyUVLine`), the loop bisector `SmFace::ClassifyLoops` classifies a
//! loop with (`sms_FindAndClassifyLoopBisector`), and the sector classification of a
//! curve crossing the face boundary at an edge or vertex
//! (`SmFace::SectorClassifyCurveOn`).
//!
//! Upstream: `source/SMLib/src/SmFace.cpp`, `source/SMLib/inc/SmFace.h`,
//! `source/SMLib/inc/SmCurveClass.h` (`SmClassifyProblem`).
//!
//! | Rust                                          | Upstream                                    |
//! |-----------------------------------------------|---------------------------------------------|
//! | [`calculate_tight_bounding_box`], [`calculate_tight_bounding_box_into`] | `SmFace::CalculateTightBoundingBox` |
//! | `face_directed_max` (opt-in, WR-588), `cell_directed_max`, `cell_monotone`, `cell_division` (WR-651), `TrimCover` and `cell_outside_face` (#16394, not upstream's) | `sm_FaceDirectedMax`, `sm_CellDirectedMax`, `sm_CellMonotone` (the fork tangency-labs/smlib@9d44c1b), `sm_CellDivision` (tangency-labs/smlib@326de73) |
//! | [`face_classify_uv_line`]                     | `sm_FaceClassifyUVLine` (file static)       |
//! | [`face_classify_uv_line_mut`]                 | `sm_FaceClassifyUVLine` (in place)          |
//! | [`find_and_classify_loop_bisector`], [`LoopBisector`] | `sms_FindAndClassifyLoopBisector`   |
//! | [`sector_classify_curve_on`], [`SectorEdgeuse`], [`SectorClassifyInput`], [`SectorClass`] | `SmFace::SectorClassifyCurveOn` |
//! | [`ClassifyProblem`] (re-export of [`crate::query::face::p1::ClassifyProblem`]) | `SmClassifyProblem` (`SmCurveClass.h`) |
//! | [`ClassifyCurveObjects`], [`LoopObjectCurves`] | the curves `ReSet` stores in an `SmCurveClassification` |
//! | `surface_is_planar`, `surface_bounding_box` (private) | `SmSurface::IsPlanar`, `SmSurface::CalculateBoundingBox` of the face's surface |
//! | `find_element_in_masked_array` (private)      | `SmTArray::FindElementInMaskedArray`        |
//! | [`check_face_coordinates`]                    | none (HANG-29 guard)                        |
//!
//! Deviations:
//! - **Coordinate bound (HANG-29).** [`calculate_tight_bounding_box`] and the face
//!   point and edge-curve classifications of `query::face::p1` first refuse a face whose
//!   vertices, loose edge boxes or loose surface box reach a coordinate above `1e150`
//!   ([`check_face_coordinates`]); upstream does not return on such a face.
//! - **Object model.** The face is a record of a [`Brep`]; the functions take the B-rep
//!   and the face's id. Statuses are results: `SM_SUCCESS` is `Ok`, `SER` / `NER`
//!   returns propagate as errors, `AERN_MSG(.., SM_ERR_INVALID_INPUT, ..)` is
//!   [`SolidError::InvalidInput`] (the bisector's `SM_ERR_METHOD_FAILURE_QUITING`
//!   check is unreachable: a failed drop skips the edge use first). Null pointers upstream dereferences (an edge use without
//!   edge, an edge without curve, a face without surface, a loop without loop use, a
//!   missing mate or vertex use) are [`SolidError::InvalidInput`].
//! - **Classification handles.** The curves a classification classifies are handles
//!   of the caller's choosing ([`CurveOnClassifyObjects`]); the curves these functions
//!   make (the UV line of [`face_classify_uv_line`], the curve on surface and UV trim
//!   line of [`find_and_classify_loop_bisector`]) are handed to the caller's
//!   [`ClassifyCurveObjects`], which returns their handles and resolves them (upstream:
//!   pointers the classification or the caller deletes; the `SmObjDelete` cleanups and
//!   `ReplaceObj` are the replacement of the stored curves). [`LoopObjectCurves`]
//!   borrows the B-rep and resolves the face, surface, topology, edge curve and trim
//!   curve handles `CurveOnClassify` makes against it.
//! - **B-rep snapshots.** `PointClassify` (tight box) and `SmEdge::IsSeam` (bisector)
//!   may attach missing UV trim curves to the B-rep, and `SetUVTrimCurve` then raises
//!   the edge (and vertex) tolerances, which changes later tolerance-expanded boxes.
//!   The tight box takes a [`LazyBrep`]: given `&mut Brep` the edits persist, as
//!   upstream; given `&Brep` they go to a private copy and are lost. The bisector's
//!   `IsSeam` and classifications share one snapshot of the B-rep (a
//!   `CurveOnClassifyCache`, shared by every loop of `SmFace::ClassifyLoops`): the
//!   healer copies its UV trims, tolerance growth and gap caches back to its B-rep, as
//!   upstream's stay on the face's own B-rep; the public entry points discard it.
//! - **Surface boxes of a face's surface.** `IsPlanar` and `CalculateBoundingBox` of a
//!   B-spline, plane or surface of revolution expand their boxes by the owning face's
//!   tolerance (`GetFace()->GetTolerance()`); the port's surface cannot reach its face,
//!   so `calculate_tight_bounding_box` computes the unexpanded boxes and expands them
//!   by the face's tolerance itself (the expansion is the last step upstream).
//! - **Ignored statuses.** Where upstream ignores a status (`GlobalPointSolve` and
//!   `PointClassify` in the tight box, `EuclidianStepOff`, `CurveOnClassify` in the
//!   bisector, `SetMinMax`) the port ignores the error, except
//!   [`SolidError::Unported`], which propagates. A failed `GlobalPointSolve` gives no
//!   solutions; a failed `EuclidianStepOff` leaves its start parameter, unclamped (its
//!   outputs as upstream initialises them); a failed surface evaluation in the tight
//!   box adds the partially evaluated point, as `SmSurfaceEval::Pos` returns it. The
//!   fork's `sm_CellDirectedMax` (WR-588) propagates the solve's and the classification's
//!   errors where the tight box above ignores them; the port's search ignores them as the
//!   tight box does, so the two differ only where those calls fail outright. A failed solve
//!   in the trimmed-face search gives a cell with no solution, which counts with its loose
//!   box, so the box only grows (safer than the fork's SER, which would fail the box).
//! - **Same curve on two edges.** `SectorClassifyCurveOn` compares the curve pointers
//!   of two different edges; an edge owns its curve in the port, so two edges never
//!   share one (`bSingleCurveOnEdges` is always false), as in `query::edge`.
//! - **`SmCrvOnSurf`** preserves a face surface's concrete dispatch.
//! - **`FindElementInMaskedArray` (`WR-167`).** Each element is searched only when
//!   its own mask bit is set; indices beyond the mask width have no bit. This corrects
//!   upstream's `1<ii` comparison (`plan/upstream-issues/` `WR-167`).
//! - **Dropped** (no effect on results in the reference build, which defines neither
//!   `SM_DEBUG_CODE` nor active asserts): the debug graphics and dumps, every
//!   `SM_ASSERT*` and `SM_DBG_WARN`, `lFaceIndx` of `SectorClassifyCurveOn` (debug only),
//!   the never-read `dUVScale` of `sm_FaceClassifyUVLine` (its
//!   `ApproxDerivativeLengths` call is kept for its [`SolidError::Unported`]).
//! - **Unported callees.** Only through callees (`SmEdgeuse::EvaluateBinormal` is wired
//!   to `topology.edgeuse_p3`, whose own stubs propagate).
//! - **Loops.** The trimmed-face search (WR-588, opt-in) takes its cells from an explicit
//!   stack, not recursion, bounded at `FACE_EXTREME_MAX_DEPTH` (16 halvings) and
//!   `FACE_EXTREME_MAX_CELLS` (4096 cells); a cell past either counts with its greatest value.
//!   Not the fork's (#14452): each cell of a B-spline face open both ways is solved on its
//!   exact subpatch over the cell, not on the whole surface (a solve's control-net guesses
//!   read every control point of the surface it is given), and a cell whose subpatch's
//!   control points reach no higher than the box so far is skipped unsolved. A cell whose
//!   subpatch gives no solution is divided, and at the limits counts with that bound, its
//!   control-point hull raised by the subpatch's rounding (its loose box when that has no
//!   finite bound, a typed error when neither has).
//!   Not the fork's (#14460): a cell is divided only the ways a knot lies inside it while
//!   either way has one, halved both ways only once it lies within one span each way.
//!   Not the fork's (#16394): a cell that no box of the face's UV trim curves meets
//!   (`TrimCover`) and whose middle classifies outside the face is passed over unsolved
//!   (`cell_outside_face`), so the cells of a surface run on past its face (a blend's) are
//!   neither divided to the cell limit nor counted with their greatest values there. A
//!   blend face (procedural blend, ball patch) is divided at its net's knots, as the fork's
//!   `IsKindOf(SmBSplineSurface)` reads them, and also bounded by its net's subpatch hull
//!   raised by the net's certified bound (`blend_net_slack`), solved on its own surface;
//!   the port had no knots for it, so a cell over a knot never came within one span and was
//!   never monotone, and every cell along its trim was divided to the cell limit.
//!   The step-off loops keep upstream's bounds (20 rounds in the sector
//!   classification; its UV fallback doubles the step-off until it leaves the curve
//!   interval, and stops after [`MAX_UV_STEP_OFF_ROUNDS`] rounds when the curve
//!   derivative vanishes and the step-off never clamps). The bisector tries at most 6
//!   angles per edge use. No recursion.

use std::collections::BTreeSet;

use crate::curve::crv_on_surf::CrvOnSurf;
use crate::curve::curve::p2::{euclidian_step_off, CurveKind, StepOff};
use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::{ignored, EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::point_set::PointContainment;
use crate::geom::pseudo_box::PseudoBox;
use crate::geom::tol::{is_degenerate_vec3, scaled_zero2, xsect_tol_3d_from_zones, zone_tol3d};
use crate::geom::vector2d::Vec2;
use crate::heal::face_props::{ClassifyLoopIo, ContainmentType, FaceProps, LoopObject, SS_NONE};
use crate::heal::face_props::{SS_UMAX, SS_UMIN, SS_VMAX, SS_VMIN};
use crate::intersect::curve_class::p1::CurveClassification;
use crate::intersect::curve_class::p2::ResetOptions;
use crate::intersect::point_class::{
    ClassGeometry, ClassObject, PointClassType, PointClassification,
};
use crate::kernel::Kernel;
use crate::math::{max2, min3, Point2, Point3, Vec3, EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface::BoundingBoxRequest;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::nurbs::math::gamma;
use crate::query::brep::LazyBrep;
use crate::query::edge as qe;
use crate::query::face::p2::{
    curve_on_classify, curve_on_classify_cached, curve_on_classify_mut, face_vertices,
    point_classify, CurveOnClassifyBrep, CurveOnClassifyCache, CurveOnClassifyObjects,
    PointClassifyOptions,
};
use crate::query::r#loop::contains_uv_point;
use crate::surface::surface::p2::SurfaceEval;
use crate::surface::surface::p4::global_point_solve_starts;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::p7::{approx_derivative_lengths, drop_vectors_at};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::volume::Volume;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p2::BinormalEval;
use crate::topology::topology_traverser::face_edges;
use crate::topology::{edge as topo_edge, loopuse, vertexuse};
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, LoopId};

const OP_TIGHT: &str = "query::face::p3::calculate_tight_bounding_box";
const OP_UV_LINE: &str = "query::face::p3::face_classify_uv_line";
const OP_BISECTOR: &str = "query::face::p3::find_and_classify_loop_bisector";
const OP_SECTOR: &str = "query::face::p3::sector_classify_curve_on";
const OP_OBJECTS: &str = "query::face::p3::LoopObjectCurves";

// `SM_ERR_INVALID_INPUT`.

/// The most step-off rounds of the sector classification (`lCount < 20`).
const MAX_STEP_OFF_ROUNDS: u32 = 20;

/// The most rounds of the UV fallback of [`sector_classify_curve_on`] when the curve's
/// derivative vanishes at its end (upstream doubles the step-off until
/// `EuclidianStepOff` clamps, which a curve that never gets that far from its start
/// point would not do). Far above what a doubling step-off of a finite curve needs.
pub const MAX_UV_STEP_OFF_ROUNDS: u32 = 2048;

/// The reason of the [`SolidError::InvalidInput`] of a handle type that cannot name
/// the curves a classification makes ([`LoopObjectCurves`]).
pub const NO_CURVE_HANDLES: &str = "the classification handle type cannot name its curves";

fn null(operation: &'static str, reason: &'static str) -> SolidError {
    SolidError::invalid_input(operation, reason)
}

/// `return(SM_ERR_INVALID_INPUT)`.
fn invalid_status(operation: &'static str) -> SolidError {
    SolidError::query_invalid_value(operation, "a vector of zero length has no direction")
}

/// `SER(v.Unitize())`: the unit vector, or the `SM_ERR_INVALID_INPUT` status of a zero
/// vector.
fn unitize_ser(v: Vec3, operation: &'static str) -> Result<Vec3> {
    v.unitize().map_err(|_| invalid_status(operation))
}

/// `SER(a.AngleBetween(b, ang))`: the angle, or the `SM_ERR_INVALID_INPUT` status of a
/// zero vector.
fn angle_ser(a: Vec3, b: Vec3, operation: &'static str) -> Result<f64> {
    a.angle_between(b).map_err(|_| invalid_status(operation))
}

/// The point and first derivative of an evaluation (`sPV[0]`, `sPV[1]`).
fn pv(v: &[Vec3], operation: &'static str) -> Result<[Vec3; 2]> {
    match v {
        [p, d, ..] => Ok([*p, *d]),
        _ => Err(null(operation, "short curve evaluation")),
    }
}

/// `SM_IS_BETWEEN(c, a, b)`: `c` strictly inside the open interval between `a` and
/// `b` shrunk by their scaled zero.
fn is_between(c: f64, a: f64, b: f64) -> bool {
    let z = scaled_zero2(&a, &b);
    ((a + z) < c && c < (b - z)) || ((b + z) < c && c < (a - z))
}

/// `SM_RAD2DEG`.
fn rad2deg(r: f64) -> f64 {
    r * (180.0 / PI)
}

/// `SM_DEG2RAD`.
fn deg2rad(d: f64) -> f64 {
    d * (PI / 180.0)
}

/// The face's surface, which upstream dereferences.
fn face_surface<'b>(b: &'b Brep, f: FaceId, operation: &'static str) -> Result<&'b Surface> {
    b.face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| null(operation, "face without surface"))
}

/// The edge of an edge use, which upstream dereferences.
fn eu_edge(b: &Brep, eu: EdgeuseId, operation: &'static str) -> Result<EdgeId> {
    eu1::edge(b, eu)?.ok_or_else(|| null(operation, "edge use without edge"))
}

/// The curve of an edge, which upstream dereferences.
fn edge_curve<'b>(b: &'b Brep, e: EdgeId, operation: &'static str) -> Result<&'b Curve> {
    b.edge(e)?
        .curve
        .as_ref()
        .ok_or_else(|| null(operation, "edge without curve"))
}

/// `EuclidianStepOff` with its status ignored: the parameter found and whether the
/// search was clamped (on a failure, the outputs as upstream initialises them: the
/// start parameter, not clamped).
fn step_off(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    start_param: f64,
    orient: OrientType,
    distance: f64,
    tol_3d: f64,
) -> Result<StepOff> {
    Ok(ignored(euclidian_step_off(
        k,
        curve,
        interval,
        start_param,
        orient == OrientType::Same,
        distance,
        tol_3d,
    ))?
    .unwrap_or(StepOff {
        converged: false,
        clamped: false,
        end_param: start_param,
    }))
}

// ---- surface boxes of the face's surface ----------------------------------------------------

/// Whether `s` is a surface whose box `SmBSplineSurface::CalculateBoundingBox` makes
/// (expanding it by its face's tolerance).
fn has_b_spline_box(s: &Surface) -> bool {
    matches!(
        s.kind(),
        SurfaceKind::BSpline(_)
            | SurfaceKind::Plane(_)
            | SurfaceKind::SurfOfRevolution(_)
            | SurfaceKind::SurfOfExtrusion(_)
            | SurfaceKind::Cone(_)
            | SurfaceKind::Sphere(_)
            | SurfaceKind::Torus(_)
    )
}

/// The zone tolerance the B-spline box override expands by when `s` is owned by a
/// face: that face's tolerance (`GetFace()->GetTolerance()`); `None` without face
/// owner.
fn owning_face_tolerance(b: &Brep, f: FaceId, s: &Surface) -> Result<Option<f64>> {
    Ok(if has_b_spline_box(s) && s.face()?.is_some() {
        Some(b.face(f)?.tolerance)
    } else {
        None
    })
}

/// `pSurface->IsPlanar(dTol3d)` of face `f`'s surface.
pub(crate) fn surface_is_planar(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    s: &Surface,
    tol_3d: f64,
) -> Result<bool> {
    let base = match s.kind_with_nurb(k)? {
        SurfaceKind::BSpline(bs) => Some(bs),
        SurfaceKind::SurfOfRevolution(r) => Some(r.base()),
        SurfaceKind::SurfOfExtrusion(r) => Some(r.base()),
        SurfaceKind::Cone(c) => Some(c.rev().base()),
        SurfaceKind::Sphere(s) => Some(s.base().base()),
        SurfaceKind::Torus(s) => Some(s.base().base()),
        _ => None,
    };
    if let (Some(bs), Some(face_tol)) = (base, owning_face_tolerance(b, f, s)?) {
        // SmSurface::IsPlanar: the planarity of the PseudoBox of
        // CalculateBoundingBox(GetNaturalUVDomain()), expanded by the face tolerance
        let req = BoundingBoxRequest {
            normal: false,
            pseudo: true,
            polar: false,
            pseudo_guess: None,
            polar_guess: None,
            expand_by_zone_tol: false,
            expand_tolerance: None,
        };
        let pseudo =
            ignored(bs.calculate_bounding_box(k, &s.natural_uv_domain_with_nurb(k)?, &req))?
                .and_then(|r| r.pseudo)
                .map_or_else(PseudoBox::new, |mut p| {
                    p.expand_absolute(face_tol);
                    p
                });
        return Ok(pseudo.is_planar(tol_3d).is_planar);
    }
    Ok(s.is_planar(k, tol_3d)?.is_planar)
}

/// `pSurface->CalculateBoundingBox(crUVDomain, &sBox)` (axis-aligned box only, expanded
/// by the zone tolerance) of face `f`'s surface.
pub(crate) fn surface_bounding_box(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    s: &Surface,
    uv_domain: &Extent2d,
) -> Result<Extent3d> {
    let face_tol = owning_face_tolerance(b, f, s)?;
    let boxes = s.calculate_bounding_box(k, uv_domain, [true, false, false], face_tol.is_none())?;
    let mut normal = boxes.normal_box.unwrap_or_default();
    if let Some(t) = face_tol {
        normal.expand_absolute(t);
    }
    Ok(normal)
}

// ---- coordinate bound (HANG-29) ---------------------------------------------------------------

/// The loose axis-aligned edge box [`check_face_coordinates`] reads.
const EDGE_NORMAL_BOX: crate::nurbs::b_spline_curve::BoundingBoxRequest =
    crate::nurbs::b_spline_curve::BoundingBoxRequest {
        normal: true,
        pseudo: false,
        polar: false,
    };

/// Refuses a face whose geometry reaches a coordinate beyond
/// [`crate::limits::MAX_COORDINATE_MAGNITUDE`] (`1e150`): its vertex points, the loose
/// boxes of its edges and the loose box of its surface over the face's UV domain. The
/// face queries that classify points on the face run it first
/// ([`calculate_tight_bounding_box`], [`crate::query::face::p1::point_3d_classify`],
/// [`crate::query::face::p1::edge_curve_on_classify`]): upstream creates the face's UV
/// trim curves there, and with such a coordinate `GlobalCoincidenceChecker` under
/// `CreateUVTrimCurve` does not return (HANG-29; nightly fuzz `query.face_p1`
/// `fuzz00201`, a B-spline face with a control point of `1e300`). A lookup that fails
/// here is left to the query itself, which reports it in upstream's order.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] (`operation`) for such a face.
pub fn check_face_coordinates(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    operation: &'static str,
) -> Result<()> {
    use crate::limits::exceeds_coordinate_bound as beyond;
    let point_beyond = |p: Point3| beyond(p.x) || beyond(p.y) || beyond(p.z);
    let box_beyond = |e: &Extent3d| {
        (0..3).any(|i| {
            let (lo, hi) = (e.min_at(i), e.max_at(i));
            lo <= hi && (beyond(lo) || beyond(hi))
        })
    };
    let refuse = || {
        Err(SolidError::invalid_input(
            operation,
            "face geometry coordinate above 1e150 (HANG-29)",
        ))
    };
    let Ok(face) = b.face(f) else {
        return Ok(());
    };
    for v in face_vertices(b, f, false, None).unwrap_or_default() {
        if b.vertex(v).is_ok_and(|v| point_beyond(v.point)) {
            return refuse();
        }
    }
    for e in face_edges(b, f).unwrap_or_default() {
        if let Ok(boxes) = qe::calculate_bounding_box(k, b, e, EDGE_NORMAL_BOX, false) {
            if boxes.normal.as_ref().is_some_and(box_beyond) {
                return refuse();
            }
        }
    }
    if let Some(s) = face.surface.as_ref() {
        let uv_domain = face.uv_domain;
        if surface_bounding_box(k, b, f, s, &uv_domain).is_ok_and(|e| box_beyond(&e)) {
            return refuse();
        }
    }
    Ok(())
}

// ---- SmFace::CalculateTightBoundingBox --------------------------------------------------------

/// `SmFace::CalculateTightBoundingBox`: the box of the face's vertices, grown by the
/// tight box of every face edge; unless the surface is planar (`SM_EFF_ZERO`) or its
/// loose box over the face's UV domain already lies inside, grown by the surface points
/// that maximise the signed distance along `±x`, `±y`, `±z` over the face's domain and
/// classify on the face (not `SM_PC_UNKNOWN`). With `Kernel::fixes.face_tight_box_searches_trim`
/// (WR-588, #1799) those extremes are searched inside the trim (`face_directed_max`)
/// instead of taken from the surface's global solve.
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER`-like [`SolidError::InvalidInput`] for a face without surface;
/// [`SolidError::InvalidInput`] for a face with a coordinate above `1e150` (HANG-29,
/// [`check_face_coordinates`]); those of [`face_vertices`], the edge boxes ([`qe::calculate_tight_bounding_box`]),
/// the planarity test and the surface's loose box; [`SolidError::Unported`] from the
/// global solves and point classifications.
pub fn calculate_tight_bounding_box<'a>(
    k: &Kernel,
    b: impl Into<LazyBrep<'a>>,
    f: FaceId,
) -> Result<Extent3d> {
    let mut face_bbox = Extent3d::new();
    calculate_tight_bounding_box_into(k, &mut b.into(), f, &mut face_bbox)?;
    Ok(face_bbox)
}

/// [`calculate_tight_bounding_box`] into the caller's `face_bbox` (`rFaceBBox`), for
/// callers that ignore the status: as upstream, the box of the vertices and edges is
/// written before the surface's part is computed, so a failure there leaves that box;
/// a failure before it (vertices, edge boxes) leaves `face_bbox` as it was.
///
/// # Errors
///
/// Those of [`calculate_tight_bounding_box`].
pub fn calculate_tight_bounding_box_into(
    k: &Kernel,
    lb: &mut LazyBrep<'_>,
    f: FaceId,
    out: &mut Extent3d,
) -> Result<()> {
    let b = lb.get();
    // HANG-29: refuse coordinates the point classifications below cannot handle
    check_face_coordinates(k, b, f, OP_TIGHT)?;
    let mut face_bbox = Extent3d::new();

    // (a copy that keeps the owning face: the point classifications below may edit
    // the B-rep)
    let surface = &face_surface(b, f, OP_TIGHT)?.clone_for_evaluation()?;
    let face_zone_tol_3d = zone_tol3d(k, b.face(f)?.tolerance);

    // Method: get the bounding box of the Vertices.
    // Then get the tight box of each Edge and union it in.
    // Then get our surface's loose BBox (cheap).
    // If it's not entirely contained in the box so far,
    // calculate its tight box and union it in.

    // Vertices:
    for v in face_vertices(b, f, false, None)? {
        face_bbox.add_point(b.vertex(v)?.point);
    } // end loop on Vertices

    // Edges:
    for e in face_edges(b, f)? {
        // Get this Edge's tight box
        let e_bbox = qe::calculate_tight_bounding_box(k, b, e)?.unwrap_or_default();
        face_bbox = face_bbox.union(&e_bbox);
    } // end loop on Edges

    // Set output to what we have so far.
    *out = face_bbox;

    // Now the surface.
    // If it's a plane, then we're done.
    if surface_is_planar(k, b, f, surface, EFF_ZERO)? {
        return Ok(());
    }

    // First check the approximate box.
    let uv_domain = b.face(f)?.uv_domain;
    let srf_bbox = surface_bounding_box(k, b, f, surface, &uv_domain)?;
    if srf_bbox.is_contained_by(&face_bbox, 0.0) {
        return Ok(());
    }

    // WR-588 (#1799; tangency-labs/smlib@9d44c1b), opt-in: the face's own extremes inside
    // its trim (`sm_FaceDirectedMax`), not the surface's global ones alone. The starting
    // values are the box of the vertices and edges, as the fork's.
    if k.fixes.face_tight_box_searches_trim {
        // (#16394) the boxes of the trim curves, by which the six searches pass over a
        // cell lying wholly outside the face
        let cover = TrimCover::of(k, lb, f)?;
        let cover = cover.as_ref();
        let (lo, hi) = (face_bbox.min(), face_bbox.max());
        let x_max = face_directed_max(
            k,
            lb,
            f,
            surface,
            &uv_domain,
            face_zone_tol_3d,
            cover,
            Vec3::new(1.0, 0.0, 0.0),
            hi.x,
        )?;
        let x_min = -face_directed_max(
            k,
            lb,
            f,
            surface,
            &uv_domain,
            face_zone_tol_3d,
            cover,
            Vec3::new(-1.0, 0.0, 0.0),
            -lo.x,
        )?;
        let y_max = face_directed_max(
            k,
            lb,
            f,
            surface,
            &uv_domain,
            face_zone_tol_3d,
            cover,
            Vec3::new(0.0, 1.0, 0.0),
            hi.y,
        )?;
        let y_min = -face_directed_max(
            k,
            lb,
            f,
            surface,
            &uv_domain,
            face_zone_tol_3d,
            cover,
            Vec3::new(0.0, -1.0, 0.0),
            -lo.y,
        )?;
        let z_max = face_directed_max(
            k,
            lb,
            f,
            surface,
            &uv_domain,
            face_zone_tol_3d,
            cover,
            Vec3::new(0.0, 0.0, 1.0),
            hi.z,
        )?;
        let z_min = -face_directed_max(
            k,
            lb,
            f,
            surface,
            &uv_domain,
            face_zone_tol_3d,
            cover,
            Vec3::new(0.0, 0.0, -1.0),
            -lo.z,
        )?;
        face_bbox.add_point(Point3::new(x_min, y_min, z_min));
        face_bbox.add_point(Point3::new(x_max, y_max, z_max));
        *out = face_bbox;
        return Ok(());
    }

    // Unfortunately, we can't just call the surface method with our domain,
    // that box could well be larger than that of the Face.
    // So we have to reproduce the Surface method here,
    // and check each solution for being inside the Face.

    // Begin copy of Surface::CalculateTightBoundingBox():
    let mut pt_class = PointClassification::new(face_zone_tol_3d);
    let mut srf_bbox = Extent3d::new();

    let mut dir_vec = Vec3::new(1.0, 0.0, 0.0); // +/- x
    for idx in 0..3 {
        if idx == 1 {
            dir_vec = Vec3::new(0.0, 1.0, 0.0); // +/- y
        }
        if idx == 2 {
            dir_vec = Vec3::new(0.0, 0.0, 1.0); // +/- z
        }

        // Positive direction, then negative direction:
        for pass in 0..2 {
            if pass == 1 {
                dir_vec = -dir_vec;
            }
            let solutions = ignored(global_point_solve_starts(
                k,
                surface,
                &uv_domain,
                SolverOperation::Signed3dDirectedMaximize,
                dir_vec,
                EFF_ZERO_SQRT,
                SolutionRequested::All,
            ))?
            .unwrap_or_default();

            for sol in &solutions {
                let uv = Point2::new(
                    sol.parameters.first().copied().unwrap_or(0.0),
                    sol.parameters.get(1).copied().unwrap_or(0.0),
                );
                let mut eval = SurfaceEval::with_uv(k, Some(surface), uv.x, uv.y);
                // (status ignored upstream)
                // (it may attach missing UV trim curves to the B-rep)
                ignored(point_classify(
                    k,
                    lb.get_mut()?,
                    f,
                    uv,
                    &mut pt_class,
                    PointClassifyOptions {
                        src_zone_tol_3d: face_zone_tol_3d,
                        do_boundary_intersections: false,
                        use_uv_classification: true,
                        ..PointClassifyOptions::default()
                    },
                ))?;
                if pt_class.point_class() != PointClassType::Unknown {
                    srf_bbox.add_point(eval_pos(&mut eval)?);
                }
            }
        }
    } // end for on x, y, z directions

    face_bbox = face_bbox.union(&srf_bbox);

    // End copy of Surface::CalculateTightBoundingBox():
    *out = face_bbox;
    Ok(())
}

/// `SmSurfaceEval::Pos()`: the position, also when the evaluation failed (then what
/// the failed evaluation wrote).
fn eval_pos(eval: &mut SurfaceEval<'_>) -> Result<Point3> {
    match eval.pos() {
        Ok(p) => Ok(p),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(eval
            .values()
            .first()
            .and_then(|r| r.first())
            .copied()
            .unwrap_or(Vec3::UNDEF)),
    }
}

// ---- WR-588 (#1799): the trimmed face's own extremes (`sm_FaceDirectedMax`) -------------------

/// The most cells [`face_directed_max`] divides a face's domain into
/// (`sm_lFaceExtremeMaxCells`). A cell left undecided past it counts with its own greatest
/// value, so the box is never smaller than the face.
const FACE_EXTREME_MAX_CELLS: usize = 4096;

/// How deep [`face_directed_max`] divides a cell (`sm_lFaceExtremeMaxDepth`), each way. A
/// cell left undecided at this depth counts with its own greatest value.
const FACE_EXTREME_MAX_DEPTH: u32 = 16;

/// How near a cell's side a knot may lie and still divide it
/// (`sm_dFaceExtremeKnotSnap`): `CopySubPatch` snaps a requested side onto a knot within
/// this distance, so a part nearer than that is no cell of its own.
const FACE_EXTREME_KNOT_SNAP: f64 = 1.0e-08;

/// The knot strictly inside `[lo, hi]` (by more than [`FACE_EXTREME_KNOT_SNAP`]) nearest
/// its middle, the first such in `knots` on a tie (`sm_CellDivision`, WR-651,
/// tangency-labs/smlib@326de73); `None` when the cell lies within one knot span that way.
/// Divided at knots, a cell comes to lie within one knot span, as [`cell_monotone`] asks of
/// a rational patch; halved at middles alone, a cell over a knot off the dyadic points of
/// the domain (a screw surface's at every eighth of a turn) never does.
fn cell_division_knot(knots: &[f64], lo: f64, hi: f64) -> Option<f64> {
    let mid = 0.5 * (lo + hi);
    let mut at: Option<(f64, f64)> = None;
    for &knot in knots {
        if knot > lo + FACE_EXTREME_KNOT_SNAP
            && knot < hi - FACE_EXTREME_KNOT_SNAP
            && at.is_none_or(|(_, off)| (knot - mid).abs() < off)
        {
            at = Some((knot, (knot - mid).abs()));
        }
    }
    at.map(|(knot, _)| knot)
}

/// Where [`face_directed_max`] divides an undecided cell: each way at the knot inside it
/// nearest its middle ([`cell_division_knot`]). A way the cell lies within one knot span
/// is left whole while the other way still has a knot inside the cell, and both ways are
/// halved at their middles once neither has (not the fork's, which halves a way with no
/// knot inside at once: #14460). Until a cell lies within one span each way the
/// monotone test of a rational patch cannot decide it, so halving the way it already
/// lies within one span only multiplies the cells along a trim: a thread's screw wall,
/// a knot every eighth of a turn along u and one span across v, came to as many cells as
/// it has spans along u, each solved twice. Returns the u and v of the division, `None`
/// for a way left whole.
fn cell_division(knots_u: &[f64], knots_v: &[f64], cell: &Extent2d) -> (Option<f64>, Option<f64>) {
    let (u0, v0, u1, v1) = (cell.min().x, cell.min().y, cell.max().x, cell.max().y);
    match (
        cell_division_knot(knots_u, u0, u1),
        cell_division_knot(knots_v, v0, v1),
    ) {
        (None, None) => (Some(0.5 * (u0 + u1)), Some(0.5 * (v0 + v1))),
        at => at,
    }
}

/// Whether `x` is above zero, false for NaN (the negation of `x > 0`, without the lint).
fn is_positive(x: f64) -> bool {
    x > 0.0
}

/// The greatest `S . dir` over `cell` of the face's surface, and whether a point taking
/// it lies in the face (`sm_CellDirectedMax`).
struct CellDirectedMax {
    /// The greatest value (`-BIG_DOUBLE` without a solution).
    top: f64,
    /// Whether the point taking it is inside the face.
    in_face: bool,
    /// Whether the solver gave a solution.
    found: bool,
}

/// `sm_CellDirectedMax`: `GlobalPointSolve(SM_SO_3D_SIGNED_DIRECTED_MAXIMIZE)` over `cell`
/// (the solver's solutions within its tolerance of the greatest, each classified; only
/// points inside the face, `SM_PC_FACE`, count, as the fork's tangency-labs/smlib@95bf434). Status handling as the tight box above: a failed solve gives no solutions and a
/// failed classification keeps the point as classified.
fn cell_directed_max(
    k: &Kernel,
    lb: &mut LazyBrep<'_>,
    f: FaceId,
    surface: &Surface,
    solve_on: &Surface,
    face_zone_tol_3d: f64,
    cell: &Extent2d,
    dir: Vec3,
) -> Result<CellDirectedMax> {
    let net = solve_on
        .number_control_points(k, SurfParamType::U)
        .saturating_mul(solve_on.number_control_points(k, SurfParamType::V));
    k.work_counters
        .add_face_extreme_control_points(u64::try_from(net).unwrap_or(u64::MAX));
    let solutions = ignored(global_point_solve_starts(
        k,
        solve_on,
        cell,
        SolverOperation::Signed3dDirectedMaximize,
        dir,
        EFF_ZERO_SQRT,
        SolutionRequested::All,
    ))?
    .unwrap_or_default();
    let mut out = CellDirectedMax {
        top: -crate::math::BIG_DOUBLE,
        in_face: false,
        found: !solutions.is_empty(),
    };
    let mut in_face_top = -crate::math::BIG_DOUBLE;
    let mut pt_class = PointClassification::new(face_zone_tol_3d);
    for sol in &solutions {
        let uv = Point2::new(
            sol.parameters.first().copied().unwrap_or(0.0),
            sol.parameters.get(1).copied().unwrap_or(0.0),
        );
        let mut eval = SurfaceEval::with_uv(k, Some(surface), uv.x, uv.y);
        let pos = eval_pos(&mut eval)?;
        let value = pos.x * dir.x + pos.y * dir.y + pos.z * dir.z;
        out.top = out.top.max(value);
        ignored(point_classify(
            k,
            lb.get_mut()?,
            f,
            uv,
            &mut pt_class,
            PointClassifyOptions {
                src_zone_tol_3d: face_zone_tol_3d,
                do_boundary_intersections: false,
                use_uv_classification: true,
                ..PointClassifyOptions::default()
            },
        ))?;
        // Inside the face only (`SM_PC_FACE`, tangency-labs/smlib@95bf434). A point on an
        // edge or a vertex lies within the face's zone tolerance of its boundary, inside or
        // out: where the surface runs on past the boundary it is up to that tolerance
        // outside the face (a box's rounded corner octant boxed to 1.0000243 where it ends
        // at 1). The boundary's own extremes come from the edges' tight boxes.
        if pt_class.point_class() == PointClassType::Face {
            in_face_top = in_face_top.max(value);
        }
    }
    // Every solution takes the greatest value to the solver's tolerance.
    out.in_face = out.found && in_face_top >= out.top - EFF_ZERO_SQRT;
    if out.in_face {
        out.top = out.top.max(in_face_top);
    }
    Ok(out)
}

/// `C(n, k)` as upstream's loop computes it (`k <= n`).
fn binomial(n: usize, k: usize) -> f64 {
    let mut c = 1.0;
    for t in 1..=k {
        c = c * (n - k + t) as f64 / t as f64;
    }
    c
}

/// The control net of a patch, as `sm_CellMonotone` reads it: `S . dir` of each control
/// point and its weight (`i` along u, `j` along v, at `i * nv + j`).
struct MonotoneNet<'a> {
    nu: usize,
    nv: usize,
    points: &'a [Point3],
    weights: &'a [f64],
    rational: bool,
    dir: Vec3,
}

impl MonotoneNet<'_> {
    fn value(&self, i: usize, j: usize) -> Option<f64> {
        self.points
            .get(i * self.nv + j)
            .map(|p| p.x * self.dir.x + p.y * self.dir.y + p.z * self.dir.z)
    }

    fn weight(&self, i: usize, j: usize) -> Option<f64> {
        if self.rational {
            self.weights.get(i * self.nv + j).copied()
        } else {
            Some(1.0)
        }
    }
}

/// Whether the control values of `net` rise (`sign` 1) or fall (`sign` -1) from each index
/// along the patch to the next, at every index across it (`sm_CellMonotone`'s `rises`):
/// along v when `along_v`, else along u. A rational net's coefficients are the Bezier of
/// the sign of the isocurve's difference, computed exactly from `B_i B_k`. `None` when a
/// control value is missing.
fn rises(net: &MonotoneNet<'_>, along_v: bool, sign: f64) -> Option<bool> {
    let (along, across) = if along_v {
        (net.nv, net.nu)
    } else {
        (net.nu, net.nv)
    };
    // The control value and weight at (index across, index along).
    let c = |a: usize, t: usize| {
        if along_v {
            net.value(a, t)
        } else {
            net.value(t, a)
        }
    };
    let w = |a: usize, t: usize| {
        if along_v {
            net.weight(a, t)
        } else {
            net.weight(t, a)
        }
    };
    for t in 0..along.saturating_sub(1) {
        if !net.rational {
            for a in 0..across {
                let d = sign * (c(a, t + 1)? - c(a, t)?);
                if !is_positive(d) {
                    return Some(false);
                }
            }
            continue;
        }
        let p = across.saturating_sub(1);
        for m in 0..=2 * p {
            let mut coefficient = 0.0;
            for i in m.saturating_sub(p)..=m.min(p) {
                let k = m - i;
                coefficient += binomial(p, i)
                    * binomial(p, k)
                    * w(i, t + 1)?
                    * w(k, t)?
                    * (c(i, t + 1)? - c(k, t)?);
            }
            if !is_positive(sign * coefficient) {
                return Some(false);
            }
        }
    }
    Some(true)
}

/// Whether `S . dir` is monotone along u, or along v, over all of `cell` of the surface's
/// B-spline base, so that no point inside the cell is a local extreme of it
/// (`sm_CellMonotone`), from the exact subpatch over the cell. The base is what
/// `SmBSplineSurface` `IsKindOf` names: the B-spline itself, or the B-spline part of a plane,
/// surface of revolution or extrusion, cone, sphere or torus (as [`surface_is_planar`]
/// picks it). A polynomial patch: its control values rise row to row entry by entry, and
/// a B-spline whose control values rise rises. A rational patch of one Bezier span (several
/// spans are not decided here): the isocurve's control values rise, the sign of their
/// differences being a Bezier in the other parameter. `false` when it cannot be told (a
/// surface kind with no B-spline base, a subpatch short of the cell, a weight not positive).
fn cell_monotone(k: &Kernel, surface: &Surface, cell: &Extent2d, dir: Vec3) -> Result<bool> {
    match cell_patch(k, surface, cell)? {
        Some(patch) => Ok(patch_monotone(&patch, dir)),
        None => Ok(false),
    }
}

/// The exact subpatch of the surface's B-spline base over `cell` (`CopySubPatch`), as
/// [`cell_monotone`] reads it: `None` for a surface kind with no B-spline base, or a
/// subpatch short of the cell (`CopySubPatch` snaps to knots).
fn cell_patch(k: &Kernel, surface: &Surface, cell: &Extent2d) -> Result<Option<BSplineSurface>> {
    let Some(base) = patch_base(k, surface)? else {
        return Ok(None);
    };
    // CopySubPatch snaps to knots: a patch short of the cell says nothing about the rest.
    let Ok(patch) = base.copy_sub_patch(cell) else {
        return Ok(None);
    };
    let patch_domain = patch.natural_uv_domain();
    if patch_domain.min().x > cell.min().x
        || patch_domain.min().y > cell.min().y
        || patch_domain.max().x < cell.max().x
        || patch_domain.max().y < cell.max().y
    {
        return Ok(None);
    }
    Ok(Some(patch))
}

/// The B-spline base whose subpatches [`cell_patch`] takes (what `SmBSplineSurface`
/// `IsKindOf` names), `None` for a surface kind with none.
fn patch_base<'s>(k: &Kernel, surface: &'s Surface) -> Result<Option<&'s BSplineSurface>> {
    Ok(Some(match surface.kind_with_nurb(k)? {
        SurfaceKind::BSpline(bs) => bs,
        SurfaceKind::Plane(p) => p.base(),
        SurfaceKind::SurfOfRevolution(r) => r.base(),
        SurfaceKind::SurfOfExtrusion(r) => r.base(),
        SurfaceKind::Cone(c) => c.rev().base(),
        SurfaceKind::Sphere(s) => s.base().base(),
        SurfaceKind::Torus(s) => s.base().base(),
        // (IsKindOf(SmBSplineSurface_TYPE): the fork's SmProceduralBlend, its net's patch)
        SurfaceKind::ProceduralBlend(pb) => pb.net(),
        // (the fork's SmBallPatch is an SmBSplineSurface too: its net)
        SurfaceKind::BallPatch(pb) => pb.net(),
        _ => return Ok(None),
    }))
}

/// How far, along a unit direction, a control point of a subpatch [`cell_patch`] takes
/// of `base` may lie from the exact subpatch's, so that [`patch_hull_max`] plus it bounds
/// the base surface over the cell although the subpatch was computed in floating point.
/// `M` is the largest coordinate of the base's Euclidean control points, `W` and `w` its
/// largest and smallest weights, `u` the unit roundoff, in Higham's standard model.
/// - Each point of the subpatch comes from at most `n = 2 (p + q)` knot insertions (`p` at
///   each end of the cell along u, `q` along v: `N_SrfExtractPatch`). Each insertion is
///   `alf P1 + (1 - alf) P0` of homogeneous points no larger than `W M` (a convex
///   combination does not grow them), with `alf = (t - a) / (b - a)`: `alf` carries
///   `gamma(3)` (two differences, one division), `1 - alf` that plus one rounding, and the
///   two products and the sum two more, so one insertion adds at most
///   `(3 alf + 3 alf + (1 - alf) + 2) u W M <= 9 u W M`; `n` of them `gamma(9n) W M`. The
///   weight likewise, at most `W` and at least `w`.
/// - Dividing back to the Euclidean point:
///   `|ΔP| <= (gamma(9n) W M + M gamma(9n) W) / (w (1 - gamma(9n))) + u M`.
/// - The dot product with a unit `dir` (3 products, 2 sums) adds `gamma(5) M`; for the unit
///   axis directions the tight box passes it is exact.
///
/// `3 gamma(9n + 5) M W / w` holds all three. `None` when it is not finite (no net, a weight
/// not positive, a NaN, an overflow).
fn sub_patch_rounding(base: &BSplineSurface) -> Option<f64> {
    let net = base.control_point_net().ok()?;
    let mut largest = 0.0_f64;
    for p in &net.points {
        for c in [p.x, p.y, p.z] {
            if c.is_nan() {
                return None;
            }
            largest = largest.max(c.abs());
        }
    }
    let ratio = if net.weights.is_empty() {
        1.0
    } else {
        let (mut least, mut most) = (f64::INFINITY, 0.0_f64);
        for &w in &net.weights {
            if !is_positive(w) {
                return None;
            }
            least = least.min(w);
            most = most.max(w);
        }
        most / least
    };
    let insertions = 2 * (base.degree(SurfParamType::U) + base.degree(SurfParamType::V));
    let rounding = 3.0 * gamma(insertions.saturating_mul(9).saturating_add(5)) * largest * ratio;
    rounding.is_finite().then_some(rounding)
}

/// [`cell_monotone`] of the subpatch over the cell ([`cell_patch`]).
fn patch_monotone(patch: &BSplineSurface, dir: Vec3) -> bool {
    let Ok(control) = patch.control_point_net() else {
        return false;
    };
    let (nu, nv) = (control.u_count, control.v_count);
    if nu < 2 || nv < 2 || control.points.len() != nu * nv {
        return false;
    }
    let mut rational = false;
    if !control.weights.is_empty() {
        if control.weights.len() != control.points.len() {
            return false;
        }
        let Some(&w0) = control.weights.first() else {
            return false;
        };
        for &w in &control.weights {
            if w.is_nan() || w <= 0.0 {
                return false;
            }
            rational = rational || w != w0;
        }
    }
    if rational
        && (nu != patch.degree(SurfParamType::U) + 1 || nv != patch.degree(SurfParamType::V) + 1)
    {
        return false;
    }
    let net = MonotoneNet {
        nu,
        nv,
        points: &control.points,
        weights: &control.weights,
        rational,
        dir,
    };
    [(true, 1.0), (true, -1.0), (false, 1.0), (false, -1.0)]
        .into_iter()
        .any(|(along_v, sign)| rises(&net, along_v, sign).unwrap_or(false))
}

/// The greatest `P . dir` of the control points of `patch`, which bounds `S . dir` over
/// all of it (each point of a B-spline with positive weights is a convex combination of
/// its control points); `None` when it cannot be told (no net, a weight not positive).
fn patch_hull_max(patch: &BSplineSurface, dir: Vec3) -> Option<f64> {
    let control = patch.control_point_net().ok()?;
    if !control.weights.is_empty()
        && (control.weights.len() != control.points.len()
            || control.weights.iter().any(|w| !is_positive(*w)))
    {
        return None;
    }
    let mut hull: Option<f64> = None;
    for p in &control.points {
        let value = p.x * dir.x + p.y * dir.y + p.z * dir.z;
        if value.is_nan() {
            return None;
        }
        hull = Some(hull.map_or(value, |h| h.max(value)));
    }
    hull
}

/// How far from the face's surface the net of a blend surface lies (#16394), for a face
/// [`face_directed_max`] does not solve on its subpatches but bounds and divides by its
/// net: a procedural blend's certified bound (`|B - S| <= bound`, its net is no more than
/// that from its definition), a ball patch's 0 (its net is the surface). `None` for any
/// other kind (a B-spline's subpatch is its own surface, [`face_directed_max`]'s `by_patch`)
/// and for a bound that is not finite and non-negative.
fn blend_net_slack(surface: &Surface) -> Option<f64> {
    match surface.kind() {
        SurfaceKind::ProceduralBlend(pb) => Some(pb.bound()).filter(|b| b.is_finite() && *b >= 0.0),
        SurfaceKind::BallPatch(_) => Some(0.0),
        _ => None,
    }
}

/// The face's B-spline surface over `cell` alone: its exact subpatch there
/// ([`cell_patch`]), owned by the same face with the same zone tolerance, so a solve over
/// the cell starts from the cell's own control net (the control-net guesses of the
/// global solve read every control point of the surface it is given, the whole net for
/// a cell of a few spans) and takes the same tolerances.
fn cell_sub_surface(surface: &Surface, patch: BSplineSurface) -> Surface {
    let mut sub = Surface::from(patch);
    sub.set_owner(surface.owner());
    sub.set_owner_zone_tol_3d(surface.owner_zone_tol_3d());
    sub
}

/// The greatest `S . dir` the surface's loose box over `cell` allows.
fn loose_top(
    k: &Kernel,
    lb: &LazyBrep<'_>,
    f: FaceId,
    surface: &Surface,
    cell: &Extent2d,
    dir: Vec3,
) -> Result<f64> {
    let loose = surface_bounding_box(k, lb.get(), f, surface, cell)?;
    let (lo, hi) = (loose.min(), loose.max());
    Ok((lo.x * dir.x).max(hi.x * dir.x)
        + (lo.y * dir.y).max(hi.y * dir.y)
        + (lo.z * dir.z).max(hi.z * dir.z))
}

/// A bound on `S . dir` over a cell [`face_directed_max`] leaves unsolved at its limits:
/// its subpatch's control-point hull (`hull`, from [`patch_hull_max`]) raised by the
/// subpatch's rounding ([`sub_patch_rounding`]); without a subpatch, or when that rounding
/// has no finite bound (a weight not positive, a NaN or an overflow anywhere in the base's
/// net), the surface's loose box over the cell ([`loose_top`]), the bound the search
/// already takes for an unsolved cell without a subpatch.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] when the loose box is not finite either: then no bound on
/// the cell exists to name, and a box that may not hold the face is refused rather than
/// returned; those of [`loose_top`].
fn unsolved_cell_top(
    k: &Kernel,
    lb: &LazyBrep<'_>,
    f: FaceId,
    surface: &Surface,
    cell: &Extent2d,
    hull: Option<f64>,
    dir: Vec3,
) -> Result<f64> {
    let rounded = match hull {
        Some(hull) => patch_base(k, surface)?
            .and_then(sub_patch_rounding)
            .map(|rounding| hull + rounding)
            .filter(|top| top.is_finite()),
        None => None,
    };
    if let Some(top) = rounded {
        return Ok(top);
    }
    let loose = loose_top(k, lb, f, surface, cell, dir)?;
    if loose.is_finite() {
        Ok(loose)
    } else {
        Err(SolidError::invalid_input(
            OP_TIGHT,
            "a trimmed-face cell left unsolved at the search's limits has no finite bound",
        ))
    }
}

/// The greatest `S . dir` over the trimmed region of the face's surface, raised to `best`,
/// which comes in as the greatest over the face's vertices and edges (`sm_FaceDirectedMax`,
/// WR-588). One solve over the whole domain gives the surface's greatest point there, which
/// may lie outside the trim while the face's own greatest lies inside it, so the domain is
/// divided into cells, each decided by:
/// 1. its greatest value is no more than `best`: nothing in it is higher;
/// 2. a point taking its greatest value lies in the face: that value is the face's there;
/// 3. its least value exceeds `best`: the cell lies in one region above `best`, which no
///    point of the face's boundary reaches, so the region is wholly outside the face (the
///    point of 2 says outside);
/// 4. `S . dir` is monotone over it ([`cell_monotone`]): the face's highest point in it is
///    on the face's boundary or on the cell's own sides, which `best` and the neighbouring
///    cells hold;
/// 5. (#16394, not the fork's; tested before the cell is solved) no UV trim curve of the
///    face reaches it ([`TrimCover`]) and its middle classifies outside the face: no point
///    of it lies in the face ([`cell_outside_face`]);
///
/// otherwise the cell is divided ([`cell_division`]: at the knots inside it, a way within
/// one span left whole while the other has a knot inside, else at its middles; WR-651).
/// A cell left at [`FACE_EXTREME_MAX_DEPTH`] or past [`FACE_EXTREME_MAX_CELLS`] counts
/// with its greatest value, or unsolved with its own bound ([`unsolved_cell_top`]). The cells are an explicit stack (no recursion), taken last
/// first, as the fork's. `cover` is the face's [`TrimCover`], `None` where rule 5 is not
/// taken.
///
/// A blend face (a procedural blend or ball patch, #16394) is divided at its net's knots
/// and bounded (1.), decided (4.) and, unsolved at the limits, bounded by its net's subpatch
/// over the cell, the hull raised by the net's distance from the surface
/// ([`blend_net_slack`]); its cells are solved on the face's own surface. A cell of it with
/// no solution goes on to 4. as a B-spline's does, not straight to its loose box.
fn face_directed_max(
    k: &Kernel,
    lb: &mut LazyBrep<'_>,
    f: FaceId,
    surface: &Surface,
    uv_domain: &Extent2d,
    face_zone_tol_3d: f64,
    cover: Option<&TrimCover>,
    dir: Vec3,
    best: f64,
) -> Result<f64> {
    face_directed_max_with(
        k,
        lb,
        f,
        surface,
        uv_domain,
        dir,
        best,
        SearchLimits {
            max_depth: FACE_EXTREME_MAX_DEPTH,
            max_cells: FACE_EXTREME_MAX_CELLS,
        },
        &mut |lb, solve_on, cell, dir| {
            cell_directed_max(k, lb, f, surface, solve_on, face_zone_tol_3d, cell, dir)
        },
        &mut |lb, cell| cell_outside_face(k, lb, f, face_zone_tol_3d, cover, cell),
    )
}

/// How far [`face_directed_max_with`] divides: [`FACE_EXTREME_MAX_DEPTH`] and
/// [`FACE_EXTREME_MAX_CELLS`] in [`face_directed_max`].
#[derive(Debug, Clone, Copy)]
struct SearchLimits {
    max_depth: u32,
    max_cells: usize,
}

/// Solves a cell for [`face_directed_max_with`]: the surface to solve on (the cell's
/// subpatch, or the face's surface), the cell and the direction; [`cell_directed_max`] in
/// [`face_directed_max`].
type CellSolve<'a, 'b> =
    dyn FnMut(&mut LazyBrep<'b>, &Surface, &Extent2d, Vec3) -> Result<CellDirectedMax> + 'a;

/// Tells [`face_directed_max_with`] whether a cell lies wholly outside the face (its rule
/// 5); [`cell_outside_face`] in [`face_directed_max`].
type CellOutside<'a, 'b> = dyn FnMut(&mut LazyBrep<'b>, &Extent2d) -> Result<bool> + 'a;

/// [`face_directed_max`] within `limits`, each cell solved by `solve` and passed over
/// unsolved where `outside` says it lies wholly outside the face.
#[allow(clippy::too_many_arguments)]
fn face_directed_max_with<'b>(
    k: &Kernel,
    lb: &mut LazyBrep<'b>,
    f: FaceId,
    surface: &Surface,
    uv_domain: &Extent2d,
    dir: Vec3,
    mut best: f64,
    limits: SearchLimits,
    solve: &mut CellSolve<'_, 'b>,
    outside: &mut CellOutside<'_, 'b>,
) -> Result<f64> {
    // (#16394) a blend surface's net: its knots divide the cells and its subpatches bound
    // and decide them as a B-spline's do, the face's own surface still solved
    let net_slack = blend_net_slack(surface);
    // (`IsKindOf(SmBSplineSurface_TYPE)`: `GetKnots` through `GetOrCreateGwNurbPointer`; the
    // fork's `SmProceduralBlend` and `SmBallPatch` are `SmBSplineSurface`s, their nets' knots)
    let (knots_u, knots_v) = if has_b_spline_box(surface) || net_slack.is_some() {
        (
            surface.knots_with_nurb(k, SurfParamType::U, None)?.0,
            surface.knots_with_nurb(k, SurfParamType::V, None)?.0,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    // A B-spline face open both ways is decided cell by cell on its exact subpatch over
    // the cell ([`cell_patch`]); any other face on its own surface.
    let by_patch = matches!(surface.kind(), SurfaceKind::BSpline(_))
        && !surface
            .is_closed(k, uv_domain, SurfParamType::U, None, false)?
            .0
        && !surface
            .is_closed(k, uv_domain, SurfParamType::V, None, false)?
            .0;
    let mut cells: Vec<(Extent2d, u32)> = vec![(*uv_domain, 0)];
    let mut divided = 0usize;
    while let Some((cell, depth)) = cells.pop() {
        // The cell's subpatch and the greatest `P . dir` of its control points, which no
        // point of the cell exceeds (each is a convex combination of them). A blend's: its
        // net's, raised by how far the net may lie from the surface (#16394).
        let patch = match (by_patch, net_slack) {
            (true, _) => cell_patch(k, surface, &cell)?
                .and_then(|patch| patch_hull_max(&patch, dir).map(|hull| (patch, hull))),
            (false, Some(slack)) => cell_patch(k, surface, &cell)?.and_then(|patch| {
                patch_hull_max(&patch, dir)
                    .map(|hull| hull + slack)
                    .filter(|top| top.is_finite())
                    .map(|top| (patch, top))
            }),
            (false, None) => None,
        };
        // 1., from the control points: nothing in the cell is higher than `best`.
        if patch
            .as_ref()
            .is_some_and(|(_, hull)| *hull <= best + EFF_ZERO_SQRT)
        {
            continue;
        }
        // 5. (#16394) no trim curve reaches the cell and its middle is outside the face:
        // nothing in it is the face's. Without it a cell outside the trim whose surface
        // rises past `best` (a blend surface run on past its face) was divided along its
        // level of `best` until the cell limit, and the cells left then counted with their
        // greatest values, far outside the face.
        if outside(lb, &cell)? {
            continue;
        }
        // (a blend is solved on its own surface, not on its net, which lies only within its
        // bound of it)
        let sub = patch
            .as_ref()
            .filter(|_| by_patch)
            .map(|(patch, _)| cell_sub_surface(surface, patch.clone()));
        let solve_on = sub.as_ref().unwrap_or(surface);
        let top = solve(lb, solve_on, &cell, dir)?;
        if top.found {
            if top.top <= best + EFF_ZERO_SQRT {
                continue;
            }
            if top.in_face {
                best = top.top;
                continue;
            }
            let least = solve(lb, solve_on, &cell, -dir)?;
            if least.found && -least.top > best {
                continue;
            }
        } else if patch.is_none() {
            // No solution to decide by: the surface's loose box over the cell holds it.
            best = best.max(loose_top(k, lb, f, surface, &cell, dir)?);
            continue;
        }
        // (a cell whose subpatch gives no solution, its greatest value on its sides, is
        // left to the tests below and its parts, as one whose greatest is outside the face)
        let monotone = match &patch {
            Some((patch, _)) => patch_monotone(patch, dir),
            None => cell_monotone(k, surface, &cell, dir)?,
        };
        if monotone {
            continue;
        }
        let exhausted = depth >= limits.max_depth || {
            divided += 1;
            divided > limits.max_cells
        };
        if exhausted {
            // Solved, the solver's greatest, above `best` (a lower one was passed over
            // above). Unsolved, the cell's own bound, never below `best`: no point of the
            // cell lies above it (#16249: its `-BIG_DOUBLE` threw away the box so far).
            best = if top.found {
                top.top
            } else {
                best.max(unsolved_cell_top(
                    k,
                    lb,
                    f,
                    surface,
                    &cell,
                    patch.as_ref().map(|(_, hull)| *hull),
                    dir,
                )?)
            };
            continue;
        }
        let (u0, v0, u1, v1) = (cell.min().x, cell.min().y, cell.max().x, cell.max().y);
        // WR-651 (tangency-labs/smlib@326de73): divided at a knot inside the cell, a way
        // within one span left whole while the other has one, else at the middles.
        let (at_u, at_v) = cell_division(&knots_u, &knots_v, &cell);
        let us = match at_u {
            Some(u) => [(u0, u), (u, u1)].to_vec(),
            None => [(u0, u1)].to_vec(),
        };
        let vs = match at_v {
            Some(v) => [(v0, v), (v, v1)].to_vec(),
            None => [(v0, v1)].to_vec(),
        };
        let child_depth = depth + 1;
        for &(lo_v, hi_v) in &vs {
            for &(lo_u, hi_u) in &us {
                cells.push((Extent2d::from_bounds(lo_u, lo_v, hi_u, hi_v), child_depth));
            }
        }
    }
    Ok(best)
}

// ---- #16394: cells wholly outside the face (rule 5 of the trimmed-face search) -------------

/// How many equal pieces [`TrimCover`] boxes a straight UV trim curve in.
const TRIM_COVER_LINE_PIECES: u32 = 32;

/// How far [`TrimCover`] grows each box, relative to its size and its coordinates, for the
/// rounding of the points it is made from (as the box of `query::loop`'s
/// `curve_box_reaches`).
const TRIM_COVER_PAD: f64 = 1.0e-6;

/// Boxes in the face's UV space that together hold every point of its UV trim curves,
/// piece by piece (#16394): for a B-spline trim curve the box of each knot span's control
/// points (with positive weights each point of a span is a convex combination of them), for
/// a line the boxes of equal pieces of the segment between its ends, each box grown by
/// [`TRIM_COVER_PAD`]. A cell that meets none of them meets no trim curve: the face's
/// boundary inside the surface's domain is its trim curves, so such a cell lies wholly
/// inside the face or wholly outside it, and one point of it tells which
/// ([`cell_outside_face`]).
#[derive(Debug, Clone, Default)]
struct TrimCover {
    /// `(u_lo, v_lo, u_hi, v_hi)` of each box.
    boxes: Vec<[f64; 4]>,
}

impl TrimCover {
    /// The cover of face `f`'s UV trim curves, each over its edge's interval (a UV trim
    /// curve is parameterised as its edge, as `query::loop`'s containment walk reads it),
    /// made first where missing (as the search's point classifications make them). `None`
    /// when a curve cannot be bounded so: a face without edge uses, an edge use without
    /// edge or trim curve, a trim curve that cannot be made, a curve neither a line nor a
    /// B-spline with a well-made basis and positive weights, a point not finite. Then the
    /// search takes no cell as outside.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] (and a cancellation) from making a trim curve or evaluating
    /// a line; those of the topology reads.
    fn of(k: &Kernel, lb: &mut LazyBrep<'_>, f: FaceId) -> Result<Option<Self>> {
        let b = lb.get_mut()?;
        let mut edgeuses = Vec::new();
        for l in crate::topology::face::p2::loops(b, f, false, None)? {
            let Some(lu) = b.loop_(l)?.loopuse else {
                return Ok(None);
            };
            edgeuses.extend(loopuse::edgeuses(b, lu, None, None)?);
        }
        if edgeuses.is_empty() {
            return Ok(None);
        }
        let mut cover = Self::default();
        for eu in edgeuses {
            match crate::topology::edgeuse::p3::get_or_create_uv_trim_curve(
                k, b, eu, false, false, true,
            ) {
                Ok(fetch) if fetch.status.is_ok() => {}
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Ok(_) | Err(_) => return Ok(None),
            }
            let Some(e) = eu1::edge(b, eu)? else {
                return Ok(None);
            };
            let interval = b.edge(e)?.interval;
            let Some(curve) = eu1::uv_trim_curve(b, eu)?.0 else {
                return Ok(None);
            };
            if !cover.add_curve(k, curve, &interval)? {
                return Ok(None);
            }
        }
        Ok(Some(cover))
    }

    /// Adds the boxes of `curve` over `interval`; `false` when they cannot be told (see
    /// [`TrimCover::of`]). A B-spline takes the spans that meet `interval` when `interval`
    /// lies in its domain, else all of them.
    fn add_curve(&mut self, k: &Kernel, curve: &Curve, interval: &Extent1d) -> Result<bool> {
        match curve.kind() {
            CurveKind::Line(_) => {
                let (t0, t1) = (interval.min(), interval.max());
                if !(t0.is_finite() && t1.is_finite()) {
                    return Ok(false);
                }
                let (Some(a), Some(c)) = (
                    ignored(curve.evaluate_point(k, t0))?,
                    ignored(curve.evaluate_point(k, t1))?,
                ) else {
                    return Ok(false);
                };
                // (the line's points over the interval are the segment between its ends)
                let at = |r: f64| Point2::new(a.x + (c.x - a.x) * r, a.y + (c.y - a.y) * r);
                let pieces = f64::from(TRIM_COVER_LINE_PIECES);
                for j in 0..TRIM_COVER_LINE_PIECES {
                    let (r0, r1) = (f64::from(j) / pieces, f64::from(j + 1) / pieces);
                    if !self.push(&[at(r0), at(r1)]) {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            CurveKind::BSpline(c) => {
                let Some(nurb) = c.nurb.as_ref() else {
                    return Ok(false);
                };
                let p = nurb.degree();
                let cpts = nurb.cpts();
                let knots = nurb.knots();
                let well_made = p >= 1
                    && cpts.len() > p
                    && knots.len() == cpts.len() + p + 1
                    && knots.windows(2).all(|w| matches!(w, [a, b] if a <= b));
                let (Some(&lo), Some(&hi)) = (knots.get(p), knots.get(cpts.len())) else {
                    return Ok(false);
                };
                if !well_made {
                    return Ok(false);
                }
                let (lo, hi) = if interval.min() >= lo && interval.max() <= hi {
                    (interval.min(), interval.max())
                } else {
                    (lo, hi)
                };
                // span i (from p to n) is [knots[i], knots[i + 1]], made of control points
                // i - p to i
                for i in p..cpts.len() {
                    let (Some(&t0), Some(&t1)) = (knots.get(i), knots.get(i + 1)) else {
                        return Ok(false);
                    };
                    if t1 <= t0 || t1 < lo || t0 > hi {
                        continue;
                    }
                    let Some(span) = i.checked_sub(p).and_then(|first| cpts.get(first..=i)) else {
                        return Ok(false);
                    };
                    let mut points = Vec::new();
                    for cp in span {
                        let weight_ok = cp.w == crate::nurbs::math::NOW
                            || (is_positive(cp.w) && cp.w.is_finite());
                        if !weight_ok {
                            return Ok(false);
                        }
                        let e = cp.to_euclid();
                        points.push(Point2::new(e.x, e.y));
                    }
                    if !self.push(&points) {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Adds the box of `points`, grown by [`TRIM_COVER_PAD`]; `false` when a coordinate is
    /// not finite or there is no point.
    fn push(&mut self, points: &[Point2]) -> bool {
        let (mut u0, mut v0) = (f64::INFINITY, f64::INFINITY);
        let (mut u1, mut v1) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in points {
            if !(p.x.is_finite() && p.y.is_finite()) {
                return false;
            }
            (u0, v0) = (u0.min(p.x), v0.min(p.y));
            (u1, v1) = (u1.max(p.x), v1.max(p.y));
        }
        if !(u0 <= u1 && v0 <= v1) {
            return false;
        }
        let size = (u1 - u0).max(v1 - v0);
        let reach = u0.abs().max(v0.abs()).max(u1.abs()).max(v1.abs());
        let pad = TRIM_COVER_PAD * (1.0 + size + reach);
        self.boxes.push([u0 - pad, v0 - pad, u1 + pad, v1 + pad]);
        true
    }

    /// Whether `cell` meets one of the boxes (sides included).
    fn meets(&self, cell: &Extent2d) -> bool {
        let (lo, hi) = (cell.min(), cell.max());
        self.boxes.iter().any(|&[u0, v0, u1, v1]| {
            // (a NaN side meets: never taken as apart)
            !(u1 < lo.x || u0 > hi.x || v1 < lo.y || v0 > hi.y)
        })
    }
}

/// Rule 5 of [`face_directed_max`] (#16394): whether `cell` lies wholly outside the face.
/// True when no box of `cover` meets it ([`TrimCover::meets`]) and its middle classifies
/// outside the face: a successful point classification that leaves the class at
/// `SM_PC_UNKNOWN`, which is outside the face ([`point_classify`]: outside the face's UV
/// domain or its outer loop, inside a hole, outside by the trim cache). A failed
/// classification, a point on an edge or a vertex, and a face without `cover` decide
/// nothing.
///
/// # Errors
///
/// [`SolidError::Unported`] (and a cancellation) from the classification; those of
/// [`LazyBrep::get_mut`].
fn cell_outside_face(
    k: &Kernel,
    lb: &mut LazyBrep<'_>,
    f: FaceId,
    face_zone_tol_3d: f64,
    cover: Option<&TrimCover>,
    cell: &Extent2d,
) -> Result<bool> {
    let Some(cover) = cover else {
        return Ok(false);
    };
    if cover.meets(cell) {
        return Ok(false);
    }
    let mut pt_class = PointClassification::new(face_zone_tol_3d);
    let classified = ignored(point_classify(
        k,
        lb.get_mut()?,
        f,
        cell.mid(),
        &mut pt_class,
        PointClassifyOptions {
            src_zone_tol_3d: face_zone_tol_3d,
            do_boundary_intersections: false,
            use_uv_classification: true,
            ..PointClassifyOptions::default()
        },
    ))?;
    Ok(classified.is_some() && pt_class.point_class() == PointClassType::Unknown)
}

// ---- classification curves ----------------------------------------------------------------

/// What the classifications of this module need of the caller's classification
/// handles: besides naming and resolving them ([`CurveOnClassifyObjects`]), taking the
/// curves a classification makes.
pub trait ClassifyCurveObjects<O: ClassObject>: CurveOnClassifyObjects<O> {
    /// Takes `curve` (the classification curve, `m_cpCurve`) and `uv_curve` (its UV
    /// curve, `m_cpUVCurve1`, when it is a different curve), replacing curves stored by
    /// an earlier call, and returns their handles.
    ///
    /// # Errors
    ///
    /// When the handle type cannot name them.
    fn set_classify_curves(
        &mut self,
        curve: Curve,
        uv_curve: Option<Curve>,
    ) -> Result<(O, Option<O>)>;
}

/// [`ClassifyCurveObjects`] over [`LoopObject`] handles of the B-rep `b`: the face, its
/// surface, the B-rep topology, edge curves and UV trim curves are the handles
/// [`LoopObject::from_face`], [`LoopObject::from_face_surface`],
/// [`LoopObject::from_topology`], [`LoopObject::from_edge_curve`] and
/// [`LoopObject::from_trim_curve`] name, the two classification curves those of
/// [`LoopObject::from_classify_curve`], the target loops [`LoopObject::as_loop`].
/// Handles resolve to the geometry and stored tolerances of `b`, as the B-rep objects
/// upstream's pointers name.
#[derive(Debug)]
pub struct LoopObjectCurves<'b, O> {
    b: &'b Brep,
    f: FaceId,
    face: O,
    surface: Option<O>,
    curve_handles: (O, O),
    curve: Option<(O, Curve)>,
    uv_curve: Option<(O, Curve)>,
}

impl<'b, O: LoopObject> LoopObjectCurves<'b, O> {
    /// The handles of face `f` of `b`.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] ([`NO_CURVE_HANDLES`]) when `O` cannot name the face,
    /// the two classification curves, the B-rep topology, edge curves or trim curves (a
    /// limitation of the handle type, which callers must not mistake for an upstream
    /// status).
    pub fn new(b: &'b Brep, f: FaceId) -> Result<Self> {
        let no = || null(OP_OBJECTS, NO_CURVE_HANDLES);
        // a handle type names every entity of a kind or none (LoopObject), so the
        // first ids answer for all
        let v0 = crate::topology::VertexId::from_index(0);
        let e0 = EdgeId::from_index(0);
        let eu0 = EdgeuseId::from_index(0);
        let vu0 = crate::topology::VertexuseId::from_index(0);
        let names_topology = O::from_topology(EntityRef::Vertex(v0)).is_some()
            && O::from_topology(EntityRef::Edge(e0)).is_some()
            && O::from_topology(EntityRef::Vertexuse(vu0)).is_some()
            && O::from_topology(EntityRef::Edgeuse(eu0)).is_some()
            && O::from_edge_curve(e0).is_some()
            && O::from_trim_curve(eu0).is_some();
        if !names_topology {
            return Err(no());
        }
        let surface = if b.face(f)?.surface.is_some() {
            O::from_face_surface(f)
        } else {
            None
        };
        Ok(Self {
            b,
            f,
            face: O::from_face(f).ok_or_else(no)?,
            surface,
            curve_handles: (
                O::from_classify_curve(false).ok_or_else(no)?,
                O::from_classify_curve(true).ok_or_else(no)?,
            ),
            curve: None,
            uv_curve: None,
        })
    }

    /// The stored classification curve.
    #[must_use]
    pub fn classify_curve(&self) -> Option<&Curve> {
        self.curve.as_ref().map(|(_, c)| c)
    }

    /// The stored UV curve of the classification curve.
    #[must_use]
    pub fn classify_uv_curve(&self) -> Option<&Curve> {
        self.uv_curve.as_ref().map(|(_, c)| c)
    }

    /// The same handles and classification curves resolved against `b`, a copy of the
    /// B-rep these were made for (same ids) that holds what the classification's lazy
    /// queries left on it: the UV trim curves and tolerances upstream's later queries
    /// read from the one B-rep.
    #[must_use]
    pub(crate) fn rebind(self, b: &Brep) -> LoopObjectCurves<'_, O> {
        LoopObjectCurves {
            b,
            f: self.f,
            face: self.face,
            surface: self.surface,
            curve_handles: self.curve_handles,
            curve: self.curve,
            uv_curve: self.uv_curve,
        }
    }

    /// The handle of `t`; [`Self::new`] checked that `O` names topology, so the face
    /// handle fallback is never taken.
    fn topology(&self, t: EntityRef) -> O {
        O::from_topology(t).unwrap_or(self.face)
    }
}

impl<O: LoopObject + PartialEq> ClassGeometry<O> for LoopObjectCurves<'_, O> {
    fn curve(&self, o: O) -> Option<&Curve> {
        if let Some((_, c)) = [&self.curve, &self.uv_curve]
            .into_iter()
            .flatten()
            .find(|(h, _)| *h == o)
        {
            return Some(c);
        }
        if let Some(e) = o.as_edge_curve() {
            return self.b.edge(e).ok()?.curve.as_ref();
        }
        if let Some(eu) = o.as_trim_curve() {
            return eu1::uv_trim_curve(self.b, eu).ok()?.0;
        }
        None
    }

    fn surface(&self, o: O) -> Option<&Surface> {
        if self.surface == Some(o) {
            return self.b.face(self.f).ok()?.surface.as_ref();
        }
        None
    }

    fn volume(&self, _o: O) -> Option<&Volume> {
        None
    }

    fn point(&self, _o: O) -> Option<Point3> {
        None
    }

    fn classification_vertex_point(&self, vertex: O) -> Option<Point3> {
        match vertex.as_topology()? {
            EntityRef::Vertex(v) => self.b.vertex(v).ok().map(|v| v.point),
            _ => None,
        }
    }

    fn topology_zone_tol_3d(&self, object: O) -> Option<f64> {
        if object == self.face {
            return self.b.face(self.f).ok().map(|f| f.tolerance);
        }
        match object.as_topology()? {
            EntityRef::Vertex(v) => self.b.vertex(v).ok().map(|v| v.tolerance),
            EntityRef::Edge(e) => self.b.edge(e).ok().map(|e| e.tolerance),
            EntityRef::Face(f) => self.b.face(f).ok().map(|f| f.tolerance),
            _ => None,
        }
    }

    /// The face handle and the topology handles ([`LoopObject::as_topology`]) name
    /// entities of `b`, so the classification reaches their members (`SmEdge::GetCurve`,
    /// `SmEdge::GetVertex`, ...) as upstream's pointers do.
    fn topology_entity(&self, object: O) -> Option<(&Brep, EntityRef)> {
        if object == self.face {
            return Some((self.b, EntityRef::Face(self.f)));
        }
        object.as_topology().map(|t| (self.b, t))
    }

    fn topology_handle(&self, entity: EntityRef) -> Option<O> {
        match entity {
            EntityRef::Face(f) if f == self.f => Some(self.face),
            _ => O::from_topology(entity),
        }
    }
}

impl<O: LoopObject + PartialEq> CurveOnClassifyObjects<O> for LoopObjectCurves<'_, O> {
    fn face(&self, _f: FaceId) -> O {
        self.face
    }

    fn vertex(&self, v: crate::topology::VertexId) -> O {
        self.topology(EntityRef::Vertex(v))
    }

    fn edge(&self, e: EdgeId) -> O {
        self.topology(EntityRef::Edge(e))
    }

    fn vertexuse(&self, vu: crate::topology::VertexuseId) -> O {
        self.topology(EntityRef::Vertexuse(vu))
    }

    fn edgeuse(&self, eu: EdgeuseId) -> O {
        self.topology(EntityRef::Edgeuse(eu))
    }

    fn edge_curve(&self, e: EdgeId) -> O {
        O::from_edge_curve(e).unwrap_or(self.face)
    }

    fn trim_curve(&self, eu: EdgeuseId) -> O {
        O::from_trim_curve(eu).unwrap_or(self.face)
    }

    fn face_surface(&self, _f: FaceId) -> Option<O> {
        self.surface
    }

    fn loop_id(&self, tgt_loop: O) -> Option<LoopId> {
        tgt_loop.as_loop()
    }
}

impl<O: LoopObject + PartialEq> ClassifyCurveObjects<O> for LoopObjectCurves<'_, O> {
    fn set_classify_curves(
        &mut self,
        curve: Curve,
        uv_curve: Option<Curve>,
    ) -> Result<(O, Option<O>)> {
        let (h, hu) = self.curve_handles;
        self.curve = Some((h, curve));
        self.uv_curve = None;
        let Some(uv) = uv_curve else {
            return Ok((h, None));
        };
        self.uv_curve = Some((hu, uv));
        Ok((h, Some(hu)))
    }
}

// ---- sm_FaceClassifyUVLine --------------------------------------------------------------------

/// `sm_FaceClassifyUVLine`: classifies the UV line from `uv_start` to `uv_end` (a new
/// 2D line, handed to `objects`) against face `f` into `cc` (reset, then the line with
/// its natural interval, zone tolerance `src_zone_tol_3d` and the face's surface as
/// surface 1, then [`curve_on_classify`] with point classification).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for coinciding end points (upstream's `NER` of the
/// missing line) and a face without surface; those of the classification reset,
/// `objects` and [`curve_on_classify`] (whose status is returned, `SER`).
pub fn face_classify_uv_line<O: ClassObject>(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    objects: &mut impl ClassifyCurveObjects<O>,
    uv_start: Point2,
    uv_end: Point2,
    src_zone_tol_3d: f64,
    cc: &mut CurveClassification<O>,
) -> Result<()> {
    let line = uv_line(k, uv_start, uv_end, cc)?;

    // Figure a tolerance for the CurveClassification (never read upstream)
    let surface = face_surface(b, f, OP_UV_LINE)?;
    approx_derivative_lengths(k, surface, &b.face(f)?.uv_domain)?;

    load_uv_line(k, f, objects, line, src_zone_tol_3d, cc)?;

    // ask the face to classify the curve
    curve_on_classify(k, b, f, objects, true, cc, false, false, true)?;
    Ok(())
}

/// [`face_classify_uv_line`] on the face's own B-rep in place (`objects`'
/// [`crate::query::face::p2::FaceBrep::brep`], [`curve_on_classify_mut`]): what the classification makes on the
/// B-rep stays there, as upstream's `const` `CurveOnClassify` leaves it.
///
/// # Errors
///
/// As [`face_classify_uv_line`].
pub fn face_classify_uv_line_mut<O: ClassObject>(
    k: &Kernel,
    f: FaceId,
    objects: &mut (impl ClassifyCurveObjects<O> + CurveOnClassifyBrep<O>),
    uv_start: Point2,
    uv_end: Point2,
    src_zone_tol_3d: f64,
    cc: &mut CurveClassification<O>,
) -> Result<()> {
    let line = uv_line(k, uv_start, uv_end, cc)?;

    // Figure a tolerance for the CurveClassification (never read upstream)
    let b = objects.brep();
    let surface = face_surface(b, f, OP_UV_LINE)?;
    approx_derivative_lengths(k, surface, &b.face(f)?.uv_domain)?;

    load_uv_line(k, f, objects, line, src_zone_tol_3d, cc)?;

    // ask the face to classify the curve
    curve_on_classify_mut(k, f, objects, true, cc, false, false, true)?;
    Ok(())
}

/// `sm_FaceClassifyUVLine` up to the tolerance: resets `cc` and builds the UV line.
fn uv_line<O: ClassObject>(
    k: &Kernel,
    uv_start: Point2,
    uv_end: Point2,
    cc: &mut CurveClassification<O>,
) -> Result<Line> {
    // init output
    cc.reset(k, &ResetOptions::default())?;

    // build UVLine
    let start = Point3::new(uv_start.x, uv_start.y, 0.0);
    let end = Point3::new(uv_end.x, uv_end.y, 0.0);
    Line::create_line_segment(2, start, end, None)
        .map_err(|_| SolidError::query_missing(OP_UV_LINE, "the UV line"))
}

/// `sm_FaceClassifyUVLine` after the tolerance: loads `line` into `cc`.
fn load_uv_line<O: ClassObject>(
    k: &Kernel,
    f: FaceId,
    objects: &mut impl ClassifyCurveObjects<O>,
    line: Line,
    src_zone_tol_3d: f64,
    cc: &mut CurveClassification<O>,
) -> Result<()> {
    // Load the new curve into the CurveClassification so that curve gets deleted by
    // rCurveClassification. Also need to pass Domain and Tolerance. [B327]
    let domain = line.natural_interval();

    // Note: ReSet() tells the CC that pNewUVLine is owned, so it will be deleted.
    let (h, _) = objects.set_classify_curves(Curve::from(line), None)?;
    cc.reset(
        k,
        &ResetOptions {
            curve: Some(h),
            interval: Some(domain),
            uv_curve: Some(h),
            zone_tol_3d: src_zone_tol_3d,
            is_curve_owned: true,
            is_curve_current: true,
            surface1: objects.face_surface(f),
            ..ResetOptions::default()
        },
    )
}

// ---- sms_FindAndClassifyLoopBisector -------------------------------------------------------

/// The outputs of [`find_and_classify_loop_bisector`] besides the classification and
/// the curves left in the caller's [`ClassifyCurveObjects`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopBisector {
    /// `rbClassifyCrv_StartOnSeam`.
    pub start_on_seam: bool,
    /// `rbClassifyCrv_StopOnSeam`.
    pub stop_on_seam: bool,
    /// `rebCLassifyCrv_Closed3d`: `None` (`SM_TS_UNINIT`) when no classification curve
    /// was built, else whether its end points coincide (`SmTol::IsDegenerate`).
    pub closed_3d: Option<bool>,
    /// Whether a classification curve was built (`rpCrvOnSurf != NULL`).
    pub has_curve: bool,
}

/// Searches points whose corresponding mask bit is set; indices >= 64 have no bit.
fn find_element_in_masked_array(arr: &[Point3], p: Point3, mask: u64) -> bool {
    arr.iter().enumerate().any(|(ii, &e)| {
        let bit = u32::try_from(ii)
            .ok()
            .and_then(|s| 1u64.checked_shl(s))
            .unwrap_or(0);
        (mask & bit) != 0 && p.approx_eq(e)
    })
}

/// `sms_FindAndClassifyLoopBisector`: for `SmFace::ClassifyLoops`, classifies a UV
/// line through the boundary of target loop `loops[target]` of face `f` into `cc`
/// against the loops not yet classified (not in `marked`) and the outer, top and bottom
/// loops. Edge uses of the loop are tried in CCW order (skipping struts, cracks, seams,
/// edges on a pole and short edges unless none is left: then the longest one); each
/// mid point drops to the surface, and lines through it at up to 6 angles (2 iso lines
/// on a doubly periodic surface, angles spanning the domain for a missing seam) are
/// classified until one meets no edge coincidentally and no vertex (or one vertex on a
/// pole). The last line tried stays in `cc` and `objects`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face domain not inside the surface's natural
/// domain, an edge use without edge or an edge without curve (upstream's
/// `AERN_MSG(.., SM_ERR_INVALID_INPUT, ..)`), an edge interval not inside its curve's
/// interval (`SM_EFF_ZERO * 1000`), a target index out of range, a missing loop use and
/// a face without surface; those of the surface drop,
/// the classification reset and `objects`; [`SolidError::Unported`] from the callees.
#[allow(clippy::too_many_arguments)]
pub fn find_and_classify_loop_bisector<O: LoopObject + PartialEq>(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    target: usize,
    loops: &[LoopId],
    ios: &[ClassifyLoopIo],
    marked: &BTreeSet<LoopId>,
    props: &FaceProps<O>,
    objects: &mut impl ClassifyCurveObjects<O>,
    cc: &mut CurveClassification<O>,
) -> Result<LoopBisector> {
    let mut out = LoopBisector {
        start_on_seam: false,
        stop_on_seam: false,
        closed_3d: None,
        has_curve: false,
    };
    find_and_classify_loop_bisector_into(
        k, b, f, target, loops, ios, marked, props, objects, cc, &mut out,
    )?;
    Ok(out)
}

/// [`find_and_classify_loop_bisector`] with upstream's out-parameters: `out` is
/// initialised (no seam, `closed_3d` `None`, no curve) after the face domain check and
/// keeps what the bisector set when it fails later (`SmFace::ClassifyLoops` ignores the
/// status and reads the seam flags), as upstream's references do; `has_curve` is set
/// only on success (`rpCrvOnSurf` is assigned at the end).
///
/// # Errors
///
/// As [`find_and_classify_loop_bisector`].
#[allow(clippy::too_many_arguments)]
pub fn find_and_classify_loop_bisector_into<O: LoopObject + PartialEq>(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    target: usize,
    loops: &[LoopId],
    ios: &[ClassifyLoopIo],
    marked: &BTreeSet<LoopId>,
    props: &FaceProps<O>,
    objects: &mut impl ClassifyCurveObjects<O>,
    cc: &mut CurveClassification<O>,
    out: &mut LoopBisector,
) -> Result<()> {
    find_and_classify_loop_bisector_cached(
        k,
        b,
        f,
        target,
        loops,
        ios,
        marked,
        props,
        objects,
        cc,
        out,
        &mut CurveOnClassifyCache::default(),
    )
}

/// [`find_and_classify_loop_bisector_into`] on the query state `cache`: `IsSeam` and the
/// classifications run on its snapshot of `b`, where the UV trim curves, tolerance growth
/// and gap caches they make stay for the caller to keep
/// ([`CurveOnClassifyCache::apply_uv_trim_updates`]), as upstream's on the face's own
/// B-rep.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) fn find_and_classify_loop_bisector_cached<O: LoopObject + PartialEq>(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    target: usize,
    loops: &[LoopId],
    ios: &[ClassifyLoopIo],
    marked: &BTreeSet<LoopId>,
    props: &FaceProps<O>,
    objects: &mut impl ClassifyCurveObjects<O>,
    cc: &mut CurveClassification<O>,
    out: &mut LoopBisector,
    cache: &mut CurveOnClassifyCache,
) -> Result<()> {
    let surface = face_surface(b, f, OP_BISECTOR)?;

    // check state - FaceDomain within SurfaceDomain
    let face_uv_domain = b.face(f)?.uv_domain;
    let surf_uv_domain = surface.natural_uv_domain_with_nurb(k)?;
    if !face_uv_domain.is_contained_by(&surf_uv_domain, 0.0) {
        return Err(null(
            OP_BISECTOR,
            "FaceUVDomain Not contained by SurfaceUVDomain",
        ));
    }

    // init output
    *out = LoopBisector {
        start_on_seam: false,
        stop_on_seam: false,
        closed_3d: None,
        has_curve: false,
    };
    cc.reset(k, &ResetOptions::default())?;
    let mut has_curve = false;

    // locals
    let tgt_loop = *loops
        .get(target)
        .ok_or_else(|| null(OP_BISECTOR, "target loop index out of range"))?;
    let mut sample_cnt: usize = 6; // number of angles checked for UVCurve (even SampleCounts work best)
    let mut found = false;
    let mut trim_interval = Extent1d::new();
    let mut longest_eu: Option<EdgeuseId> = None;
    let mut longest_is_seam = false;
    let mut max_edge_length = 0.0;
    let doubly_periodic = !props.closed_u.is_false() && !props.closed_v.is_false();
    let zone_tol_3d_face = zone_tol3d(k, b.face(f)?.tolerance);
    // (sPN, sEdgeEnds and sUV keep their values when an ignored evaluation fails)
    let mut sp = [Vec3::UNDEF; 2];
    let mut ends = [Vec3::UNDEF; 2];
    let mut uv1 = Vec2::UNDEF;
    let mut start_uv = Vec2::UNDEF;
    let mut end_uv = Vec2::UNDEF;

    // get pLoop->Edgeuses for the Target Loop (guarantees UVTrimLine XSects the TgtLoop)
    let tgt_lu = b
        .loop_(tgt_loop)?
        .loopuse
        .ok_or_else(|| null(OP_BISECTOR, "loop without loop use"))?;
    let edgeuses = loopuse::edgeuses(b, tgt_lu, None, None)?;
    let n_eu = edgeuses.len();

    // For pLoop EveryEdgeuse - evaluate a MidPoint and Try and find A UVTrimCurve
    // orientation that satisfies the above constraints
    for (ii, &eu_ii) in edgeuses.iter().enumerate() {
        let mut eu = eu_ii;
        let is_last = ii + 1 >= n_eu;
        let e = eu1::edge(b, eu)?
            .ok_or_else(|| null(OP_BISECTOR, "Given Loop->Edgeuse is not attached to Edge"))?;
        let curve = b
            .edge(e)?
            .curve
            .as_ref()
            .ok_or_else(|| null(OP_BISECTOR, "Given Loop->Edge is not attached to Curve"))?;
        let edge_ivl = b.edge(e)?.interval;
        let curve_ivl = curve.natural_interval();
        if !edge_ivl.is_contained_by(&curve_ivl, EFF_ZERO * 1000.0) {
            return Err(null(
                OP_BISECTOR,
                "TargetEdgeuse->Edge->Ivl is not contained in TargetEdgeuse->Edge->Curve->Ivl",
            ));
        }

        // Edge midpoint (status ignored)
        if let Some(v) = ignored(curve.evaluate(k, edge_ivl.mid(), 1, true, true))? {
            sp = pv(&v, OP_BISECTOR)?;
        }

        // Get Edge length - save longest edge in case we have to use it later
        if let Some(p) = ignored(curve.evaluate_point(k, edge_ivl.min()))? {
            ends[0] = p;
        }
        if let Some(p) = ignored(curve.evaluate_point(k, edge_ivl.max()))? {
            ends[1] = p;
        }
        let half0 = ends[0].distance(sp[0]);
        let half1 = ends[1].distance(sp[0]);
        let this_length = max2(half0, half1);
        // (IsSeam may attach missing UV trim curves: on the classifications' snapshot)
        let edge_is_seam = topo_edge::is_seam(k, cache.snapshot(b), e)?;
        if longest_eu.is_none()
            || (this_length > max_edge_length && (!longest_is_seam || edge_is_seam))
        {
            max_edge_length = this_length;
            longest_eu = Some(eu);
            longest_is_seam = edge_is_seam;
        }

        // skip strut, crack, and seam edges - count the edgeuses on this Edge in this
        // loop, if even - its a crack, strut, or seam edge
        let in_loop = topo_edge::edgeuses(b, e, None, Some(EntityRef::Loopuse(tgt_lu)))?;
        if in_loop.len() % 2 == 0 {
            // when more edges remain to be tested - skip this strut, crack, or seam edge
            if !is_last {
                continue;
            } else if let Some(l) = longest_eu {
                // all edges fail culling tests - use the longest one seen
                eu = l;
            } else {
                continue;
            }
        } // end pEdge is a Struct, Crack or Seam check

        // skip edge evaluations on singularities
        if props.poles != SS_NONE
            && find_element_in_masked_array(&props.pole_points, sp[0], props.poles)
        {
            // when more edges remain to be tested - skip this edge on Singularity
            if !is_last {
                continue;
            } else if let Some(l) = longest_eu {
                eu = l;
            } else {
                continue;
            }
        } // end EdgeSmpPt is on the SurfSingularity check

        // skip short edges - if all edges are short use the longest edge
        let edge_zone_tol = zone_tol3d(k, b.edge(e)?.tolerance);
        if half0 < edge_zone_tol * 10.0 || half1 < edge_zone_tol * 10.0 {
            // when more edges remain to be tested - skip this short edge
            if !is_last {
                continue;
            } else if let Some(l) = longest_eu {
                eu = l;
            } else {
                continue;
            }
        } // end skip small edge check

        // arrive here when we have a likely Edgeuse and EdgeuseSmpPt from which to run
        // the classification

        // when UVTrimCurve is available - use UVTrimCurve to get pUVGuess
        // (note: the edge interval and sample point stay those of edge use ii)
        let uv_guess = match eu1::uv_trim_curve(b, eu)?.0 {
            Some(uv_trim) => {
                let g = ignored(uv_trim.evaluate_point(k, edge_ivl.mid()))?.unwrap_or(Vec3::UNDEF);
                Some(Point2::new(g.x, g.y))
            }
            None => None,
        };

        // Drop Edge->MidPoint to Surface to UVPoint ThroughPoint for upcoming UVTrimCurve.
        let drop = surface.drop_point::<crate::surface::surface::p4::NoObject>(
            k,
            sp[0],
            &surf_uv_domain,
            &DropPointOptions {
                guess_uv: uv_guess,
                ..DropPointOptions::default()
            },
        )?;
        // skip this edgeuse if something goes wrong
        if !drop.success {
            continue;
        }
        let uv0 = drop.result_uv;

        // Drop EdgeTan to Surface (status ignored)
        if let Some(v) = ignored(drop_vectors_at(
            k,
            surface,
            uv0,
            Side::Left,
            Side::Left,
            &[sp[1]],
        ))?
        .and_then(|v| v.first().copied())
        {
            uv1 = v;
        }

        // Surface normal at Drop Point (status ignored; only drawn upstream)
        ignored(surface.evaluate_normal(k, uv0, Side::Left, Side::Left))?;

        // Rotation Range to search for 'good' classify curve
        let drop_tan_deg = rad2deg(uv1.y.atan2(uv1.x)); // Rtn: [-Pi to Pi] => [-180 to 180]
        let mut ang_range_deg = Extent1d::from_raw(drop_tan_deg - 90.0, drop_tan_deg + 90.0);
        let mut missing_seam_sampling = false;

        // Adjust sAngRangeDeg as needed
        if doubly_periodic {
            // doubly periodic surface -> ClassifyCrv must run SeamToSeam and be Closed3d
            let _ = ang_range_deg.set_min_max(0.0, 90.0);
            sample_cnt = 2;
            out.start_on_seam = true;
            out.stop_on_seam = true;
        } else if matches!(
            props.bad_missing_seam_dir,
            SurfParamType::U | SurfParamType::V | SurfParamType::Both
        ) {
            // remember we went through this branch - later we pick the sampling order
            // based on this value
            missing_seam_sampling = true;

            // get the angle to every corner from the DropPoint
            let mut corner_ang_deg = [0.0_f64; 4];
            for (jj, ang) in corner_ang_deg.iter_mut().enumerate() {
                let corner = match jj {
                    0 => Point2::new(face_uv_domain.u_min(), face_uv_domain.v_min()),
                    1 => Point2::new(face_uv_domain.u_max(), face_uv_domain.v_min()),
                    2 => Point2::new(face_uv_domain.u_max(), face_uv_domain.v_max()),
                    _ => Point2::new(face_uv_domain.u_min(), face_uv_domain.v_max()),
                };
                let dir = corner - uv0;
                *ang = rad2deg(dir.y.atan2(dir.x)); // Rtn: -Pi to Pi => -180, 180

                // place all angles in range 0 to 360 picking 0.0 for case 2 and 360.0
                // for case 1 when angle is 0.0
                // (case 0: range 180 to 270; case 1: range 270 to 360)
                if (jj == 0 && *ang < (180.0 - EFF_ZERO)) || (jj == 1 && *ang < (270.0 - EFF_ZERO))
                {
                    *ang += 360.0;
                }

                if *ang < 0.0 {
                    *ang += 360.0;
                }
            }

            // adjust the UVCurve angle range so only UVCurves that span the UVDomain in
            // the proper direction are considered
            let [mut c0, c1, mut c2, mut c3] = corner_ang_deg;
            let (d_min, d_max) = if props.bad_missing_seam_dir == SurfParamType::U {
                c0 -= 180.0;
                let c1 = c1 - 180.0;
                (max2(c0, c2), crate::math::min2(c1, c3))
            } else {
                c0 += 180.0;
                c2 += 360.0;
                c3 += 180.0;
                (max2(c1, c3), crate::math::min2(c0, c2))
            };
            // (status ignored upstream: an empty range leaves the interval unchanged)
            let _ = ang_range_deg.set_min_max(d_min, d_max);

            // shorten the rotation angle to back off the corners (avoids vertex
            // intersections
            ang_range_deg.expand_relative(-0.05);
        } // end MissingSeam need to adjust the rotation angle range check

        // for every trial angle
        for jj in 0..sample_cnt {
            // Make next CrvOnSurf to classify from DropPoint:[UVPoint, UVVec]
            let smp_inc = if missing_seam_sampling {
                sample_cnt / 2
            } else {
                0
            };
            let tgt_indx = (jj + smp_inc) % sample_cnt;
            #[allow(clippy::cast_precision_loss)] // small sample indices
            let t = if tgt_indx == 0 {
                0.0
            } else if tgt_indx == sample_cnt - 1 {
                1.0
            } else {
                tgt_indx as f64 / (sample_cnt - 1) as f64
            };
            let mut angle_deg = ang_range_deg.evaluate(t);

            // avoid ClassifyCurves nearly tangent to EdgeCurve at DropPoint
            if (angle_deg - drop_tan_deg).abs() < 5.0 {
                angle_deg += 5.0;
            }

            // for doubly periodic surfaces - only build isolines
            if doubly_periodic {
                angle_deg = if jj == 0 { 0.0 } else { 90.0 };
            }

            // Map dAngleDeg to a dir vector
            let uv_dir = Vec2::new(deg2rad(angle_deg).cos(), deg2rad(angle_deg).sin());

            // build UVLine to use as the UVClassificationCurve
            if let Some(ivl) = surf_uv_domain.intersect_with_infinite_line(uv0, uv_dir, None, None)
            {
                trim_interval = ivl;
            }
            start_uv = surf_uv_domain.clamp_point(uv0 + uv_dir * trim_interval.min());
            end_uv = surf_uv_domain.clamp_point(uv0 + uv_dir * trim_interval.max());

            let uv_trim_line = Line::from_points(
                Point3::new(start_uv.x, start_uv.y, 0.0),
                Point3::new(end_uv.x, end_uv.y, 0.0),
                2,
            );

            // Map UVClassificationCurve to 3d through the Face->Surface
            let uv_curve = Curve::from(uv_trim_line);
            let crv_on_surf = CrvOnSurf::new(
                k,
                uv_curve.clone_for_evaluation()?,
                surface.clone_for_evaluation()?,
                Some(surf_uv_domain),
                0,
            )?;
            let crv_ivl = crv_on_surf.natural_interval();
            let (start_pt, end_pt) = (
                crv_on_surf.evaluate_point(k, crv_ivl.min()),
                crv_on_surf.evaluate_point(k, crv_ivl.max()),
            );
            let (h_curve, h_uv) =
                objects.set_classify_curves(Curve::from(crv_on_surf), Some(uv_curve))?;
            has_curve = true;

            // set CrvClassification object with current ClassifyCrv
            cc.reset(
                k,
                &ResetOptions {
                    curve: Some(h_curve),
                    interval: Some(crv_ivl),
                    uv_curve: h_uv,
                    zone_tol_3d: props.zone_tol_3d,
                    ..ResetOptions::default()
                },
            )?;

            // set Classify to Target TgtLoops only
            cc.set_tgt_loops_only(true);

            // build the TgtLoop list (all unmarked loops and the outer loop if it exists
            // and is known)
            for (kk, &l) in loops.iter().enumerate() {
                // skip marked loops (except an identified outer loop or potential outer
                // loop)
                if marked.contains(&l) {
                    let containment = ios
                        .get(kk)
                        .ok_or_else(|| {
                            null(OP_BISECTOR, "classify loop record index out of range")
                        })?
                        .containment;
                    if !matches!(
                        containment,
                        ContainmentType::OuterLoop
                            | ContainmentType::TopLoop
                            | ContainmentType::BotLoop
                    ) {
                        continue;
                    }
                }

                // add Unmarked or Outer Loop to CrvClassification Tgt list
                cc.add_tgt_loop(O::from_loop(l));
            } // end iter kk every Loop building current TgtLoop list

            // Classify the sCrvOnSurf against this Loop and all unmarked loops only
            // (status ignored)
            ignored(curve_on_classify_cached(
                k, b, f, objects, true, cc, true, true, false, cache,
            ))?;

            // get the classification curve vertex intersections
            let vertices = cc.find_vertex_objects(&*objects, None)?;
            let coin_edges = cc.find_edge_objects(&*objects, None, 1)?; // 1 = get CoincidentEdges only

            let zone_tol_3d_sq = zone_tol_3d_face * zone_tol_3d_face;

            // When the pCrvClassification is far from any tolerance problems - use it to
            // classify the loop
            if coin_edges.is_empty() {
                let verts = &vertices.vertices;
                let clean = if verts.is_empty() {
                    true
                } else if props.poles != SS_NONE && verts.len() == 1 {
                    let v0 = verts
                        .first()
                        .and_then(LoopObject::as_vertex)
                        .ok_or_else(|| null(OP_BISECTOR, "vertex classification without vertex"))?;
                    let vp = b.vertex(v0)?.point;
                    let pole = |i: usize| -> Result<Point3> {
                        props
                            .pole_points
                            .get(i)
                            .copied()
                            .ok_or_else(|| null(OP_BISECTOR, "pole point index out of range"))
                    };
                    ((props.poles & SS_UMIN) != 0
                        && zone_tol_3d_sq > (pole(0)? - vp).length_squared())
                        || ((props.poles & SS_VMIN) != 0
                            && zone_tol_3d_sq > (pole(1)? - vp).length_squared())
                        || ((props.poles & SS_UMAX) != 0
                            && zone_tol_3d_sq > (pole(2)? - vp).length_squared())
                        || ((props.poles & SS_VMAX) != 0
                            && zone_tol_3d_sq > (pole(3)? - vp).length_squared())
                } else {
                    false
                };
                if clean {
                    found = true;
                }
            }

            // other output - rebCLassifyCrv_Closed3d (computed after the loop upstream,
            // on the last curve built)
            // (statuses ignored upstream)
            let start_pt = ignored(start_pt)?.unwrap_or(Vec3::UNDEF);
            let end_pt = ignored(end_pt)?.unwrap_or(Vec3::UNDEF);
            out.closed_3d = Some(is_degenerate_vec3(k, end_pt - start_pt, None));

            if found {
                break;
            }
        } // end iter jj, trial ClassifyCurves

        // arrive here when bFound == FALSE - did not find a good ClassifyCurve (need to
        // use an okay one); bFound == TRUE - found a tolerance free clean classification
        if found {
            // use this classification in the next section
            break;
        }
    } // end iter ii, every Loop->edge looking for a good UVTrimCurve Loop bisector

    // Classify the actual endpoints, including circumferential samples on a singly
    // periodic surface. Surface periodicity alone does not identify them.
    out.start_on_seam = surface.is_on_seam(k, start_uv, None)?.on_seam;
    out.stop_on_seam = surface.is_on_seam(k, end_uv, None)?.on_seam;

    // set remaining output
    out.has_curve = has_curve;

    // all done
    Ok(())
}

// ---- SmFace::SectorClassifyCurveOn ----------------------------------------------------------

pub use crate::query::face::p1::ClassifyProblem;

/// One edge use bounding the sector of [`sector_classify_curve_on`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectorEdgeuse {
    /// `cpEU1` / `cpEU2`: the face edge use crossed by the curve.
    pub edgeuse: EdgeuseId,
    /// `dEU1Param`: the edge parameter of the curve/edge intersection.
    pub param: f64,
    /// `sEdge1ZoneTol3d`: the zone tolerance of the edge.
    pub zone_tol_3d: f64,
    /// `eEU1Orientation`: [`OrientType::Same`] for a sector bounded by the edge moving
    /// forward, [`OrientType::Opposite`] backward.
    pub orientation: OrientType,
}

/// The arguments of [`sector_classify_curve_on`].
#[derive(Debug, Clone, Copy)]
pub struct SectorClassifyInput<'c> {
    /// The first edge use (`cpEU1`, `dEU1Param`, `sEdge1ZoneTol3d`, `eEU1Orientation`).
    pub eu1: SectorEdgeuse,
    /// The second edge use (the same as the first, with the opposite orientation, for
    /// a crossing in an edge's interior).
    pub eu2: SectorEdgeuse,
    /// `crCurve`: a 2D or 3D curve on the face.
    pub curve: &'c Curve,
    /// `crCurveInterval`: the interval with an end at the crossing.
    pub curve_interval: Extent1d,
    /// `eCurveOrientation`: [`OrientType::Same`] classifies the curve at the interval's
    /// minimum moving forward, [`OrientType::Opposite`] at its maximum moving backward.
    pub curve_orientation: OrientType,
    /// `sCurveZoneTol3d`: the zone tolerance of the curve's source.
    pub curve_zone_tol_3d: f64,
    /// `bIsOnSingularityPoint`: the crossing is a surface singularity.
    pub on_singularity: bool,
    /// `sUVofSingularity`.
    pub singularity_uv: Point2,
    /// `bUVSpaceOkay` (upstream default `TRUE`): the UV loop containment may resolve a
    /// doubtful classification.
    pub uv_space_okay: bool,
}

/// The result of [`sector_classify_curve_on`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SectorClass {
    /// `rbIsInsideThisFaceSector`.
    pub inside: bool,
    /// `rbPossibleProblem`.
    pub possible_problem: bool,
    /// `reClassifyProblem`.
    pub problem: ClassifyProblem,
}

/// `SmEdgeuse::EvaluateBinormal(t, bUse3DOnly, rPnt, rBin, &rTan)`: no parameter
/// direction asked (`pOptUVVector` defaults to null).
fn evaluate_binormal(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    parameter: f64,
    use_3d_only: bool,
) -> Result<BinormalEval> {
    crate::topology::edgeuse::p3::evaluate_binormal(
        k,
        b,
        eu,
        parameter,
        use_3d_only,
        &crate::topology::edgeuse::p3::BinormalOptions::default(),
    )
}

/// `(p * v) * p`: `v` projected onto the plane with normal `p` (and rotated a quarter
/// turn), the upstream cross products.
fn project(p: Vec3, v: Vec3) -> Vec3 {
    p.cross(v).cross(p)
}

/// `SmFace::SectorClassifyCurveOn`: classifies the interval of a curve on face `f`
/// that crosses the face boundary at an edge (both edge uses the same) or a vertex
/// (two neighbouring edge uses of a loop) as inside or outside the face sector the
/// edge uses bound, from the edge use binormals and the curve and edge tangents at the
/// crossing and at growing step-off distances (projected onto the averaged normal at a
/// surface singularity). A doubtful classification is marked a possible problem; with
/// `uv_space_okay` it is then retried by loop containment of UV points stepped along
/// the curve.
///
/// `b` is mutable because the edge use binormals may build missing UV trim curves.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] where upstream dereferences null (an edge use without
/// edge or loop use, an edge without curve, a face without surface, a missing vertex use
/// or mate); `SM_ERR_INVALID_INPUT`
/// ([`QueryFailure::InvalidValue`](crate::error::query::QueryFailure::InvalidValue)) for a zero vector the
/// projection or step-off tangents unitize or measure angles with (`SER`); those of the
/// curve and edge evaluations and of `EvaluateNormalAtSingularity`;
/// [`SolidError::Unported`]: those of `SmEdgeuse::EvaluateBinormal` and a 2D curve on a
/// face whose surface is no B-spline (`curve.crv_on_surf`).
#[allow(clippy::too_many_lines)]
pub fn sector_classify_curve_on(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    input: &SectorClassifyInput<'_>,
) -> Result<SectorClass> {
    let eu1_in = input.eu1;
    let eu2_in = input.eu2;
    let cp_eu1 = eu1_in.edgeuse;
    let cp_eu2 = eu2_in.edgeuse;
    let curve_interval = input.curve_interval;
    let curve_orientation = input.curve_orientation;
    let curve_zone_tol_3d = input.curve_zone_tol_3d;

    // init return values
    let mut res = SectorClass::default();

    // local curve params
    let curve_param = if curve_orientation == OrientType::Same {
        curve_interval.min()
    } else {
        curve_interval.max()
    };

    // work in 3d: when crCurve's domain_dim == 2, use it as an SmCrvOnSurf object to
    // make a 3D curve
    let surf_curve;
    let cp_curve: &Curve = if input.curve.dim() == 2 {
        let surface = face_surface(b, f, OP_SECTOR)?;
        surf_curve = Curve::from(CrvOnSurf::new(
            k,
            input.curve.clone_for_evaluation()?,
            surface.clone_for_evaluation()?,
            None,
            0,
        )?);
        &surf_curve
    } else {
        input.curve
    };

    // set 1st and 2nd Edgeuse orientation
    let eu1_curve_orient = if eu1_in.orientation == eu1::orientation(b, cp_eu1)? {
        OrientType::Same
    } else {
        OrientType::Opposite
    };
    let eu2_curve_orient = if eu2_in.orientation == eu1::orientation(b, cp_eu2)? {
        OrientType::Same
    } else {
        OrientType::Opposite
    };

    // tolerance = sum of edgeuse and curve tolerances
    let xsect_tol = xsect_tol_3d_from_zones(
        k,
        max2(eu1_in.zone_tol_3d, eu2_in.zone_tol_3d),
        curve_zone_tol_3d,
    );

    // init edgeuse params at curve/edge intersection
    let mut new_eu1_param = eu1_in.param;
    let mut new_eu2_param = eu2_in.param;
    let mut new_curve_param = curve_param;
    let edge1 = eu_edge(b, cp_eu1, OP_SECTOR)?;
    let edge2 = eu_edge(b, cp_eu2, OP_SECTOR)?;
    let eu1_curve = edge_curve(b, edge1, OP_SECTOR)?.clone();
    let eu2_curve = edge_curve(b, edge2, OP_SECTOR)?.clone();
    let ivl_eu1 = b.edge(edge1)?.interval;
    let ivl_eu2 = b.edge(edge2)?.interval;

    // detect special case: all curves are lines
    let is_line = |c: &Curve| matches!(c.kind(), CurveKind::Line(_));
    let mut all_lines = is_line(input.curve) && is_line(&eu1_curve) && is_line(&eu2_curve);

    // note if 2 different edges are using the same curve (an edge owns its curve here)
    let single_curve_on_edges = false;

    // singularity points: project onto an averaged normal plane
    let mut projection_plane = Vec3::new(0.0, 0.0, 0.0);
    let mut do_projection = false;

    // when the crossing point is known to mark a surface Singularity
    if input.on_singularity {
        // Trivial case where only one edge comes into singularity
        // all results must be inside
        if eu1::radial(b, cp_eu1)? == Some(cp_eu2) {
            res.inside = true;
            return Ok(res);
        }

        // compute an average surface normal about this point and use that as the
        // reference point to project the binormals and tangents.
        do_projection = true;
        projection_plane = face_surface(b, f, OP_SECTOR)?.evaluate_normal_at_singularity(
            k,
            input.singularity_uv,
            curve_zone_tol_3d,
        )?;

        // Step off each curve by the tolerance/2.0
        new_eu1_param = step_off(
            k,
            &eu1_curve,
            &ivl_eu1,
            eu1_in.param,
            eu1_curve_orient,
            xsect_tol / 2.0,
            xsect_tol / 1000.0,
        )?
        .end_param;
        new_eu2_param = step_off(
            k,
            &eu2_curve,
            &ivl_eu2,
            eu2_in.param,
            eu2_curve_orient,
            xsect_tol / 2.0,
            xsect_tol / 1000.0,
        )?
        .end_param;
        new_curve_param = step_off(
            k,
            cp_curve,
            &curve_interval,
            curve_param,
            curve_orientation,
            xsect_tol / 2.0,
            xsect_tol / 1000.0,
        )?
        .end_param;
    } // end crossing point is a surface singularity point check

    // Locals for stepping
    let mut done = false;
    let mut inside = false;
    let mut count: u32 = 0;
    let mut step_off_distance = 0.0;
    let mut min_dist = crate::math::BIG_DOUBLE;
    let mut first_max_dist = crate::math::BIG_DOUBLE;
    let mut inside_last = false;
    let mut first_pnt1 = Vec3::UNDEF;

    // until the classification is complete
    while !done {
        // assume upcoming classification is good
        done = true;

        // get pEU1 pos, tang, and binorm
        let use_3d_only = !input.uv_space_okay; // was FALSE [B462]
        let bin1_eval = match evaluate_binormal(k, b, cp_eu1, new_eu1_param, use_3d_only) {
            Ok(v) => v,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => {
                res.possible_problem = true;
                res.problem = ClassifyProblem::UnresolvedClassification;
                return Ok(res); // Note, SER would cause failure of an entire MergeCurveOnSurface().
            }
        };
        let pnt1 = bin1_eval.point;
        let mut bin1 = bin1_eval.binormal;
        let mut tan1 = bin1_eval.tangent;

        if count == 0 {
            first_pnt1 = pnt1;
        }
        // Note: use the passed-in EU orientation here, because EvaluateBinormal() has
        // already flipped tangent according to the EU's own orientation.
        if eu1_in.orientation == OrientType::Opposite {
            tan1 = -tan1;
        }
        let save_tan1 = tan1;

        // get pEU2 position and tangent
        let mut eu2_pv = pv(
            &eu2_curve.evaluate(k, new_eu2_param, 1, true, true)?,
            OP_SECTOR,
        )?;
        // Note: use the composite orientation here.
        if eu2_curve_orient == OrientType::Opposite {
            eu2_pv[1] = -eu2_pv[1];
        }
        let save_tan2 = eu2_pv[1];

        // get curve position and tangent
        let mut curve_pv = pv(
            &cp_curve.evaluate(k, new_curve_param, 1, true, true)?,
            OP_SECTOR,
        )?;
        if curve_orientation == OrientType::Opposite {
            curve_pv[1] = -curve_pv[1];
        }

        // Low work: if both binormals generally line up with each other, and they're
        // generally the same or opposite to the curve tangent, we can call it good.
        // [230227_crease]
        match evaluate_binormal(k, b, cp_eu2, new_eu2_param, use_3d_only) {
            Ok(bin2_eval) => {
                if curve_pv[1].unitize().is_ok() {
                    let dot1 = bin1.dot(curve_pv[1]);
                    let dot2 = bin2_eval.binormal.dot(curve_pv[1]);
                    if dot1 > 0.1 && dot2 > 0.1 {
                        res.inside = true;
                        return Ok(res);
                    }
                    if dot1 < -0.1 && dot2 < -0.1 {
                        res.inside = false;
                        return Ok(res);
                    }
                }
            }
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => {}
        }

        // Only use bAllLines optimization when all EndPoints are close together.
        if count == 0 {
            // let dFirstMaxDist = max distance between edgeuse and curve endPoints
            first_max_dist = max2(pnt1.distance(eu2_pv[0]), pnt1.distance(curve_pv[0]));

            // turn off optimization when endPoint tolerances are large
            if all_lines && first_max_dist > curve_zone_tol_3d / 1000.0 {
                all_lines = false;
            }
        } // end large endPoint tolerance check

        // when asked - project binormal and all tangents vectors onto single projection
        // plane
        if do_projection {
            tan1 = project(projection_plane, tan1);
            bin1 = project(projection_plane, bin1);
            eu2_pv[1] = project(projection_plane, eu2_pv[1]);
            curve_pv[1] = project(projection_plane, curve_pv[1]);

            tan1 = unitize_ser(tan1, OP_SECTOR)?;
            bin1 = unitize_ser(bin1, OP_SECTOR)?;
            eu2_pv[1] = unitize_ser(eu2_pv[1], OP_SECTOR)?;
            curve_pv[1] = unitize_ser(curve_pv[1], OP_SECTOR)?;
        } // end DoProjection check

        // after the first iteration - compute tangents with position differences.
        if count > 0 {
            // get EU1 tangent   = sPnt1      - sFirstPnt1
            // get EU2 tangent   = sPnt2      - sFirstPnt1
            // get Curve tangent = CurvePoint - sFirstPnt1
            tan1 = pnt1 - first_pnt1;
            eu2_pv[1] = eu2_pv[0] - first_pnt1;

            // let CurveTangent = ( sCurvePV[0] - sFirstPnt1 ) unless that vector opposes
            // the computed CurveTangent direction.
            let tmp_v = curve_pv[0] - first_pnt1;
            if tmp_v.dot(curve_pv[1]) > 0.0 {
                curve_pv[1] = tmp_v;
            }

            // Skip this case if any of the tangent vectors are zero length after
            // increasing the stepOffDistance
            if tan1.length_squared() < EFF_ZERO_SQ
                || eu2_pv[1].length_squared() < EFF_ZERO_SQ
                || curve_pv[1].length_squared() < EFF_ZERO_SQ
            {
                done = count >= MAX_STEP_OFF_ROUNDS;
                count += 1;
                step_off_distance = if count == 1 {
                    xsect_tol
                } else if count == 2 {
                    xsect_tol * 2.0
                } else {
                    4.0 * step_off_distance
                };
                continue;
            }

            // unitize the tangents
            tan1 = unitize_ser(tan1, OP_SECTOR)?;
            eu2_pv[1] = unitize_ser(eu2_pv[1], OP_SECTOR)?;
            curve_pv[1] = unitize_ser(curve_pv[1], OP_SECTOR)?;

            // when requested, project tangents to ProjectionPlane
            if do_projection {
                tan1 = project(projection_plane, tan1);
                eu2_pv[1] = project(projection_plane, eu2_pv[1]);
                curve_pv[1] = project(projection_plane, curve_pv[1]);

                tan1 = unitize_ser(tan1, OP_SECTOR)?;
                eu2_pv[1] = unitize_ser(eu2_pv[1], OP_SECTOR)?;
                curve_pv[1] = unitize_ser(curve_pv[1], OP_SECTOR)?;
            }

            // save min distance between EU1, EU2, and Curve sample points
            let diff1 = pnt1.distance(eu2_pv[0]);
            let diff2 = pnt1.distance(curve_pv[0]);
            let diff3 = eu2_pv[0].distance(curve_pv[0]);
            min_dist = min3(diff1, diff2, diff3);
        } // end after first iteration check

        // classify curve's tangent as in (or on) sector defined by EU1 and EU2 tangents
        inside = curve_pv[1].is_inside_sector(tan1, bin1, eu2_pv[1]);

        // when forced to high iteration classifications and they toggle - note a
        // possible problem
        if count > 0 && inside != inside_last {
            res.possible_problem = true;
            res.problem = if res.problem == ClassifyProblem::None {
                ClassifyProblem::ToggledClassification
            } else {
                ClassifyProblem::SeesawClassification
            };
        }
        inside_last = inside;

        // compute angles between tangent vectors
        //  when any tangent vectors are zero length - set all angles to zero
        let (ang_eu1_eu2, ang_eu1_curve, ang_eu2_curve) = if curve_pv[1].length_squared()
            > EFF_ZERO_SQ
            && eu2_pv[1].length_squared() > EFF_ZERO_SQ
            && tan1.length_squared() > EFF_ZERO_SQ
        {
            (
                angle_ser(tan1, eu2_pv[1], OP_SECTOR)?,
                angle_ser(tan1, curve_pv[1], OP_SECTOR)?,
                angle_ser(eu2_pv[1], curve_pv[1], OP_SECTOR)?,
            )
        } else {
            (0.0, 0.0, 0.0)
        };

        // If the sector is too small then we can't classify it as inside.
        if ang_eu1_eu2 < EFF_ZERO_SQRT * 1000.0 {
            inside = false;
        }

        // were mostly done - unless on the first pass, or if any of the angles between
        // tangents are less than 10 degrees, or if the dMinDist is less than tolerance
        let tol_factor = 20.0; // 2.0 is too small [B606 x-hatch] (But 50 is too big: prog_test)
        let ten_deg = 10.0 * PI / 180.0;
        let two_deg = 2.0 * PI / 180.0;
        if count == 0
            || ang_eu1_eu2 < ten_deg
            || ang_eu1_curve < ten_deg
            || ang_eu2_curve < ten_deg
            || min_dist < xsect_tol * tol_factor
        {
            // end criteria: after 2nd iteration, Sample Distances are large compared to
            // tolerance
            if min_dist > xsect_tol * tol_factor && count > 2 {
                break;
            }

            // end criteria: Sectoring a single edge with no small angles between edge
            // and curve or a large sector
            if (cp_eu1 == cp_eu2
                || (count == 0
                    && first_max_dist < curve_zone_tol_3d / 1000.0
                    && is_between(ang_eu1_eu2, 178.0 * PI / 180.0, 182.0 * PI / 180.0)))
                && ang_eu1_eu2 > two_deg
                && ang_eu1_curve > two_deg
                && ang_eu2_curve > two_deg
            {
                // Edge Sectoring - use the latest values
                inside = curve_pv[1].is_inside_sector(tan1, bin1, eu2_pv[1]);
                break;
            }

            // end criteria: All Lines - always done
            if all_lines {
                break;
            }

            // prepare for another iteration
            count += 1;
            if count < MAX_STEP_OFF_ROUNDS {
                done = false;
            } else if !res.possible_problem {
                res.possible_problem = true;
                res.problem = ClassifyProblem::UnresolvedClassification;
            }

            // increase StepOffDistance
            step_off_distance = if count == 1 {
                xsect_tol
            } else if count == 2 {
                xsect_tol * 2.0
            } else {
                4.0 * step_off_distance
            };

            // compute dNewEU1Param for point StepOffDistance from EU1 end
            let s1 = step_off(
                k,
                &eu1_curve,
                &ivl_eu1,
                eu1_in.param,
                eu1_curve_orient,
                step_off_distance,
                xsect_tol / 1000.0,
            )?;
            new_eu1_param = s1.end_param;

            // if we run out of Edgeuse1 and angles are small
            if s1.clamped && (ang_eu1_eu2 < ten_deg || ang_eu1_curve < ten_deg) {
                // flag a possible problem - and quit using current classification
                res.possible_problem = true;
                res.problem = ClassifyProblem::ShortSectorEdgeuse1;
                done = true;
            }

            // compute dNewEU2Param for point StepOffDistance from EU2 end
            let s2 = step_off(
                k,
                &eu2_curve,
                &ivl_eu2,
                eu2_in.param,
                eu2_curve_orient,
                step_off_distance,
                xsect_tol / 1000.0,
            )?;
            new_eu2_param = s2.end_param;

            // if we run out of Edgeuse2 and angles are small
            if s2.clamped && (ang_eu1_eu2 < ten_deg || ang_eu2_curve < ten_deg) {
                res.possible_problem = true;
                res.problem = ClassifyProblem::ShortSectorEdgeuse2;
                done = true;
            }

            // compute dNewCurveParam for point StepOffDistance from Curve end
            let sc = step_off(
                k,
                cp_curve,
                &curve_interval,
                curve_param,
                curve_orientation,
                step_off_distance,
                xsect_tol / 1000.0,
            )?;
            new_curve_param = sc.end_param;

            // if we run out of Curve
            if sc.clamped {
                res.possible_problem = true;
                res.problem = ClassifyProblem::ShortInterval;
                done = true;
            }
        } // end first case, small tangent angle, or small sample point distance check

        // This should catch the case where we have a discontinuous curve with a large
        // angle between the segments.
        if single_curve_on_edges
            && save_tan1.length_squared() > EFF_ZERO_SQ
            && save_tan2.length_squared() > EFF_ZERO_SQ
        {
            let tan_ang = angle_ser(save_tan1, save_tan2, OP_SECTOR)?;
            if tan_ang < deg2rad(120.0) {
                inside = curve_pv[1].dot(bin1) > 0.0;
                break;
            }
        }
    } // end while !bDone

    // set output
    res.inside = inside;

    // under normal operation - all done
    if !res.possible_problem || !input.uv_space_okay {
        // SmLoop::ContainsUVPoint() requires uv trim curves.
        return Ok(res);
    }

    //====================================
    // PART 2: When there might be a problem: get a uv point slightly into the curve
    // (start at 2*tol and move along), then use SmLoop::ContainsUVPoint().

    // Do not use this next classification approach when either edge is connected
    // multiply to the face
    if topo_edge::number_face_occurances(b, edge1, Some(f))? != 1
        || topo_edge::number_face_occurances(b, edge2, Some(f))? != 1
    {
        // live with the current classification - don't classify against the loop
        return Ok(res);
    }

    // Get a point on the curve close to the vertex, and classify its uv surface
    // parameter against the Loop. Start very near the vertex and move away, until a
    // good classification is found.
    let curve_pv = pv(
        &cp_curve.evaluate(k, curve_param, 1, true, true)?,
        OP_SECTOR,
    )?;
    let deriv_len = curve_pv[1].length();

    let mut step_off_dist = 2.0 * xsect_tol;
    let mut curve_step_off_param;

    // Get the uv of the vertexuse, to use as a guess for dropping to the curve-offset
    // point.
    let vtxuse = if eu1_in.orientation == OrientType::Same {
        eu1::vertexuse(b, cp_eu1)?
    } else if eu2_in.orientation == OrientType::Same {
        eu1::vertexuse(b, cp_eu2)?
    } else {
        let mate = eu1::mate(b, cp_eu1)?.ok_or_else(|| null(OP_SECTOR, "edge use without mate"))?;
        eu1::vertexuse(b, mate)?
    }
    .ok_or_else(|| null(OP_SECTOR, "edge use without vertex use"))?;

    let mut guess_uv = match vertexuse::compute_uv_point(k, b, vtxuse, true) {
        Ok(uv) => uv,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(res), // just revert to old behavior.
    };

    // Some surface info:
    let srf_domain = face_surface(b, f, OP_SECTOR)?.natural_uv_domain_with_nurb(k)?;
    let lu1 = eu1::loopuse(b, cp_eu1)?.ok_or_else(|| null(OP_SECTOR, "edge use not in a loop"))?;
    let loop1 = b
        .loopuse(lu1)?
        .loop_
        .ok_or_else(|| null(OP_SECTOR, "loop use without loop"))?;

    // Loop, moving out from the curve start point.
    done = false;
    let mut rounds: u32 = 0;
    let mut crv_pt = Vec3::UNDEF;
    while !done {
        rounds += 1;
        if rounds > MAX_UV_STEP_OFF_ROUNDS {
            return Err(null(
                OP_SECTOR,
                "UV step-off did not leave the curve interval",
            ));
        }

        // Step along the curve.
        if deriv_len > EFF_ZERO {
            let mut delta_t = step_off_dist / deriv_len;
            if curve_orientation == OrientType::Opposite {
                delta_t *= -1.0;
            }
            curve_step_off_param = curve_param + delta_t;

            if !curve_interval.contains_value(curve_step_off_param, 0.0) {
                done = true;
                curve_step_off_param = curve_interval.clamp_value(curve_step_off_param);
            }
        } else {
            // in case curve is singular at end.
            let s = step_off(
                k,
                cp_curve,
                &curve_interval,
                curve_param,
                curve_orientation,
                step_off_dist,
                xsect_tol / 1000.0,
            )?;
            curve_step_off_param = s.end_param;
            if s.clamped {
                done = true;
            }
        }

        // (status ignored upstream: the point keeps its last value)
        if let Some(p) = ignored(cp_curve.evaluate_point(k, curve_step_off_param))? {
            crv_pt = p;
        }

        // Get surface uv for the stepped-off curve point.
        let drop = match face_surface(b, f, OP_SECTOR)?
            .drop_point::<crate::surface::surface::p4::NoObject>(
                k,
                crv_pt,
                &srf_domain,
                &DropPointOptions {
                    guess_uv: Some(guess_uv),
                    ..DropPointOptions::default()
                },
            ) {
            Ok(d) => d,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(res), // just revert to old behavior.
        };
        let step_uv = drop.result_uv;

        // (Move the guess for the next step.)
        guess_uv = step_uv;

        // See if we can get a good reading from this uv position.
        let c = contains_uv_point(k, b, loop1, None, step_uv, xsect_tol)?;
        let mut cont = c.containment;

        // If it's inside, but the loop is backwards, that's really the outside.
        if cont == PointContainment::Inside && c.orientation == OrientType::Opposite {
            cont = PointContainment::Outside;
        }

        if cont == PointContainment::Inside && c.orientation == OrientType::Same {
            res.inside = true;
            res.possible_problem = false;
            res.problem = ClassifyProblem::None;
            return Ok(res);
        } else if cont == PointContainment::Outside {
            res.inside = false;
            res.possible_problem = false;
            res.problem = ClassifyProblem::None;
            return Ok(res);
        }

        // Otherwise, try again with a bigger stepoff.
        step_off_dist *= 2.0;
    } // end while stepping along curve.

    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::BoundedLineOptions;
    use crate::intersect::point_class::ClassObjectKind;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::nurbs::srf_basic::Surface as NurbSurface;
    use crate::primitive::api_trim_ops::trim_surface_with_3d_curves;
    use crate::surface::plane::Plane;
    use crate::topology::{
        Edge, Edgeuse, EdgeuseOwner, Face, Faceuse, FaceuseId, Loop, Loopuse, LoopuseId,
        LoopuseStart, Vertex, VertexId, Vertexuse, VertexuseId, VertexuseOwner,
    };

    /// A unit-square planar face over `[0, 1]^2` (lifted to `z`) with UV trim lines.
    fn square(b: &mut Brep, z: f64) -> FaceId {
        let s = b.add_shell().unwrap();
        let o = Point3::new(0.0, 0.0, z);
        let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let plane = Plane::new(o, Vec3::X, Vec3::Y, Vec2::new(1.0, 1.0), &dom);
        let f = b
            .add_face_record(Face {
                surface: Some(Surface::from(plane)),
                faceuse: Some(FaceuseId::from_index(0)),
                uv_domain: dom,
                tolerance: 1e-6,
                ..Face::new(s)
            })
            .unwrap();
        let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        for &(x, y) in &corners {
            b.add_vertex_record(Vertex {
                point: Point3::new(x, y, z),
                tolerance: 1e-6,
                ..Vertex::default()
            })
            .unwrap();
        }
        b.add_faceuse_record(Faceuse {
            shell: Some(s),
            face: Some(f),
            mate: None,
            orientation: OrientType::Same,
            loopuses: vec![LoopuseId::from_index(0)],
            ..Faceuse::default()
        })
        .unwrap();
        b.add_loopuse_record(Loopuse {
            faceuse: Some(FaceuseId::from_index(0)),
            loop_: Some(LoopId::from_index(0)),
            mate: None,
            start: LoopuseStart::Edgeuse(Some(EdgeuseId::from_index(0))),
            ..Loopuse::default()
        })
        .unwrap();
        b.add_loop_record(Loop {
            face: f,
            edges: Vec::new(),
            loopuse: Some(LoopuseId::from_index(0)),
        })
        .unwrap();
        for i in 0..4u32 {
            let (x0, y0) = corners[i as usize];
            let (x1, y1) = corners[((i + 1) % 4) as usize];
            let unit = Extent1d::from_min_max(0.0, 1.0);
            let line = |a: Point3, c: Point3, dim: u64| {
                Curve::from(Line::bounded(
                    a,
                    c - a,
                    &unit,
                    &BoundedLineOptions {
                        dim,
                        ..BoundedLineOptions::default()
                    },
                ))
            };
            b.add_edge_record(Edge {
                vertices: [VertexId::from_index(i), VertexId::from_index((i + 1) % 4)],
                edgeuses: vec![EdgeuseId::from_index(i)],
                interval: Extent1d::from_min_max(0.0, 1.0),
                tolerance: 1e-6,
                curve: Some(line(Point3::new(x0, y0, z), Point3::new(x1, y1, z), 3)),
            })
            .unwrap();
            b.add_edgeuse_record(Edgeuse {
                edge: Some(EdgeId::from_index(i)),
                owner: EdgeuseOwner::Loopuse(Some(LoopuseId::from_index(0))),
                ccw: Some(EdgeuseId::from_index((i + 1) % 4)),
                cw: Some(EdgeuseId::from_index((i + 3) % 4)),
                vertexuse: Some(VertexuseId::from_index(i)),
                orientation: OrientType::Same,
                uv_trim_curve: Some(line(Point3::new(x0, y0, 0.0), Point3::new(x1, y1, 0.0), 2)),
                ..Edgeuse::default()
            })
            .unwrap();
            b.add_vertexuse_record(Vertexuse {
                vertex: Some(VertexId::from_index(i)),
                owner: VertexuseOwner::Edgeuse(Some(EdgeuseId::from_index(i))),
                ..Vertexuse::default()
            })
            .unwrap();
        }
        f
    }

    #[test]
    fn tight_box_of_a_plane_face_is_its_edges_box() {
        let k = Kernel::default();
        let mut b = Brep::new();
        let f = square(&mut b, 2.0);
        let bx = calculate_tight_bounding_box(&k, &b, f).unwrap();
        assert_eq!(bx.min(), Point3::new(0.0, 0.0, 2.0));
        assert_eq!(bx.max(), Point3::new(1.0, 1.0, 2.0));
    }

    /// A bicubic height field over `[-20, 20]^2` (eight uniform spans each way, control
    /// points at their Greville abscissae, so its parameters are `x` and `y`). Control
    /// point `(5, 5)` has height 9, whose basis function is 2/3 at its middle; control
    /// point `(0, 0)` has height `corner`, nonzero on `[-20, -15)` each way only.
    fn crowned_height_field(corner: f64) -> Surface {
        let n = 11;
        // Unique knots -20, -15, ..., 20 with end multiplicity 4 (cubic).
        let mut knots = vec![-20.0; 4];
        knots.extend((1..8).map(|k| -20.0 + 5.0 * f64::from(k)));
        knots.extend([20.0; 4]);
        let greville: Vec<f64> = (0..n)
            .map(|i| (knots[i + 1] + knots[i + 2] + knots[i + 3]) / 3.0)
            .collect();
        let net: Vec<Vec<CPoint>> = (0..n)
            .map(|i| {
                (0..n)
                    .map(|j| {
                        let z = match (i, j) {
                            (5, 5) => 9.0,
                            (0, 0) => corner,
                            _ => 0.0,
                        };
                        CPoint::new(greville[i], greville[j], z, NOW)
                    })
                    .collect()
            })
            .collect();
        let nurb = NurbSurface::new(net, 3, 3, knots.clone(), knots).unwrap();
        Surface::from(BSplineSurface::from_nurb(nurb, false))
    }

    /// The tight box of the crowned height field trimmed to the square `[-10, 10]^2` (its
    /// loop is four lines on `z = 0`), with the fix `searches_trim` on or off.
    fn crowned_trimmed_face_box(corner: f64, searches_trim: bool) -> Extent3d {
        let (k, b, f) = crowned_trimmed_face(corner, searches_trim);
        calculate_tight_bounding_box(&k, &b, f).unwrap()
    }

    /// The kernel, B-rep and face of [`crowned_trimmed_face_box`].
    fn crowned_trimmed_face(corner: f64, searches_trim: bool) -> (Kernel, Brep, FaceId) {
        let mut k = Kernel::default();
        k.fixes.face_tight_box_searches_trim = searches_trim;
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let corners = [(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)];
        let curves = (0..4)
            .map(|i| {
                let (x0, y0) = corners[i];
                let (x1, y1) = corners[(i + 1) % 4];
                Curve::from(
                    Line::create_line_segment(
                        3,
                        Point3::new(x0, y0, 0.0),
                        Point3::new(x1, y1, 0.0),
                        Some(&interval),
                    )
                    .unwrap(),
                )
            })
            .collect();
        let b = trim_surface_with_3d_curves(&mut k, crowned_height_field(corner), &[4], curves)
            .unwrap();
        let faces = crate::topology::brep::p1::faces(&b, None).unwrap();
        assert_eq!(faces.len(), 1);
        (k, b, faces[0])
    }

    /// WR-588 (#1799): the trimmed face rises to 4 at its middle, where its surface's
    /// global maximum (50 at the corner outside the trim) is not inside the face.
    #[test]
    fn tight_box_finds_a_trimmed_faces_peak_when_the_surface_peaks_outside() {
        let bx = crowned_trimmed_face_box(50.0, true);
        assert!((bx.max().z - 4.0).abs() <= 1e-9, "z max {}", bx.max().z);
        assert!((bx.min().z - 0.0).abs() <= 1e-9, "z min {}", bx.min().z);
        assert!((bx.min().x + 10.0).abs() <= 1e-9 && (bx.max().x - 10.0).abs() <= 1e-9);
        assert!((bx.min().y + 10.0).abs() <= 1e-9 && (bx.max().y - 10.0).abs() <= 1e-9);
    }

    /// Upstream (fix off): the surface's global solve gives only the peak outside the
    /// trim, so the box comes from the edges alone (z max 0).
    #[test]
    fn tight_box_of_a_trimmed_face_without_the_fix_is_upstreams() {
        let bx = crowned_trimmed_face_box(50.0, false);
        assert!((bx.max().z - 0.0).abs() <= 1e-9, "z max {}", bx.max().z);
    }

    /// A rational single-span patch (degree 2 in u, 1 in v) of the quarter cylinder of
    /// radius 1 about the z axis (z = v): control points `(1, 0)`, `(mid_x, mid_y)`, `(0, 1)`
    /// along u with weights `[1, sqrt(2)/2, 1]` (homogeneous `(w x, w y, w z, w)`).
    fn quarter_cylinder_patch(mid_x: f64, mid_y: f64) -> Surface {
        let w = [1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0];
        let xy = [(1.0, 0.0), (mid_x, mid_y), (0.0, 1.0)];
        let net: Vec<Vec<CPoint>> = (0..3)
            .map(|i| {
                let (x, y) = xy[i];
                [0.0, 1.0]
                    .iter()
                    .map(|&z| CPoint::new(x * w[i], y * w[i], z * w[i], w[i]))
                    .collect()
            })
            .collect();
        let u = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let v = vec![0.0, 0.0, 1.0, 1.0];
        let nurb = NurbSurface::new(net, 2, 1, u, v).unwrap();
        Surface::from(BSplineSurface::from_nurb(nurb, false))
    }

    /// The cylinder's rational patch is monotone along its axis (`S . z = v` rises over
    /// the whole cell), so [`cell_monotone`] settles a cell there.
    #[test]
    fn rational_cylinder_patch_is_monotone_along_its_axis() {
        let k = Kernel::default();
        let patch = quarter_cylinder_patch(1.0, 1.0);
        let cell = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        assert!(cell_monotone(&k, &patch, &cell, Vec3::new(0.0, 0.0, 1.0)).unwrap());
    }

    /// With the middle control point raised to `(2, 2)`, `S . x` rises then falls along u
    /// (control values 1, 2, 0): no cell of it is settled by [`cell_monotone`].
    #[test]
    fn rational_patch_with_raised_middle_is_not_monotone_along_x() {
        let k = Kernel::default();
        let patch = quarter_cylinder_patch(2.0, 2.0);
        let cell = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        assert!(!cell_monotone(&k, &patch, &cell, Vec3::new(1.0, 0.0, 0.0)).unwrap());
    }

    /// An analytic face (the quarter-cylinder side of a partial cylinder of radius 5 and
    /// height 10, a cone-kind surface whose base is a B-spline) boxes at `[0,5] x [0,5] x
    /// [0,10]` with the search on and off, both in the same time.
    #[test]
    fn tight_box_of_an_analytic_cylinder_face_is_the_same_with_the_search() {
        for searches_trim in [false, true] {
            let mut k = Kernel::default();
            k.fixes.face_tight_box_searches_trim = searches_trim;
            let brep = crate::primitive::api_primitives::create_partial_cylinder(
                &mut k,
                Point3::new(0.0, 0.0, 0.0),
                5.0,
                10.0,
                0.0,
                90.0,
            )
            .unwrap();
            let faces = crate::topology::brep::p1::faces(&brep, None).unwrap();
            let side = faces
                .into_iter()
                .find(|&f| {
                    matches!(
                        brep.face(f).unwrap().surface.as_ref().map(Surface::kind),
                        Some(SurfaceKind::Cone(_))
                    )
                })
                .unwrap();
            // Its height is linear in v, so the whole face is monotone along z through the
            // B-spline base of the cone-kind surface (rule 4 of the search).
            let face = brep.face(side).unwrap();
            let surface = face.surface.as_ref().unwrap();
            assert!(cell_monotone(&k, surface, &face.uv_domain, Vec3::new(0.0, 0.0, 1.0)).unwrap());
            let bx = calculate_tight_bounding_box(&k, &brep, side).unwrap();
            let (lo, hi) = (bx.min(), bx.max());
            for (got, want) in [
                (lo.x, 0.0),
                (lo.y, 0.0),
                (lo.z, 0.0),
                (hi.x, 5.0),
                (hi.y, 5.0),
                (hi.z, 10.0),
            ] {
                assert!(
                    (got - want).abs() <= 1e-6,
                    "fix {searches_trim}: {got} vs {want}"
                );
            }
        }
    }

    /// #16249: a cell left unsolved at the search's limits counts with its subpatch's
    /// control-point hull (plus its rounding), raised into the bound so far, not with the
    /// solve's `-BIG_DOUBLE`, which threw away the box of the edges (z 0) and every cell
    /// before it. Every cell here is unsolved (a solve that fails, whose error is
    /// ignored) and the crown is no cell's monotone, so the cell holding the peak reaches
    /// the cell limit, the depth limit, or both. The face rises to 4 at its middle; its
    /// whole net's hull is 9.
    #[test]
    fn unsolved_cell_at_the_search_limits_keeps_a_bound_on_the_face() {
        let (k, b, f) = crowned_trimmed_face(0.0, true);
        let face = b.face(f).unwrap();
        let surface = face.surface.clone().unwrap();
        let domain = face.uv_domain;
        let mut lb: LazyBrep<'_> = (&b).into();
        let mut unsolved = |_: &mut LazyBrep<'_>, _: &Surface, _: &Extent2d, _: Vec3| {
            Ok(CellDirectedMax {
                top: -crate::math::BIG_DOUBLE,
                in_face: false,
                found: false,
            })
        };
        let up = Vec3::new(0.0, 0.0, 1.0);
        for (max_depth, max_cells) in [(16, 0), (0, 4096), (2, 4096), (2, 3)] {
            let limits = SearchLimits {
                max_depth,
                max_cells,
            };
            let top = face_directed_max_with(
                &k,
                &mut lb,
                f,
                &surface,
                &domain,
                up,
                0.0,
                limits,
                &mut unsolved,
                &mut |_, _| Ok(false),
            )
            .unwrap();
            assert!(
                (4.0..=9.0 + 1e-9).contains(&top),
                "depth {max_depth}, cells {max_cells}: z max {top}"
            );
        }
    }

    /// The rounding of a subpatch's hull is a small finite bound for the crown, and none
    /// when it overflows (a rational net with coordinates near `1e100` and weights `1e-300`
    /// and 1): an unsolved cell then counts with the surface's loose box, not the hull.
    #[test]
    fn sub_patch_rounding_overflow_falls_back_to_the_loose_box() {
        let crown = crowned_height_field(0.0);
        let SurfaceKind::BSpline(bs) = crown.kind() else {
            panic!("the crown is a B-spline");
        };
        let r = sub_patch_rounding(bs).unwrap();
        assert!(r > 0.0 && r < 1e-11, "rounding {r}");

        let w = [[1e-300, 1.0], [1.0, 1.0]];
        let net: Vec<Vec<CPoint>> = (0..2)
            .map(|i| {
                (0..2)
                    .map(|j| {
                        let (x, y) = ([0.0, 1e100][i], [0.0, 1e100][j]);
                        CPoint::new(x * w[i][j], y * w[i][j], 1e100 * w[i][j], w[i][j])
                    })
                    .collect()
            })
            .collect();
        let knots = vec![0.0, 0.0, 1.0, 1.0];
        let nurb = NurbSurface::new(net, 1, 1, knots.clone(), knots).unwrap();
        let wild = Surface::from(BSplineSurface::from_nurb(nurb, false));
        let SurfaceKind::BSpline(wild_bs) = wild.kind() else {
            panic!("a B-spline");
        };
        assert!(sub_patch_rounding(wild_bs).is_none());
        let (k, b, f) = crowned_trimmed_face(0.0, true);
        let lb: LazyBrep<'_> = (&b).into();
        let cell = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        // the cell then counts with the surface's loose box over it, which holds the
        // patch's highest control value 1e100, not with the hull 1 alone
        let top = unsolved_cell_top(&k, &lb, f, &wild, &cell, Some(1.0), Vec3::Z).unwrap();
        assert!(top >= 1e100, "loose top {top}");
    }

    /// The surface's global maximum lies inside the trim (no peak outside): both paths
    /// give the face's extreme 4.
    #[test]
    fn tight_box_with_the_peak_inside_the_trim_agrees_with_upstream() {
        for searches_trim in [false, true] {
            let bx = crowned_trimmed_face_box(0.0, searches_trim);
            assert!(
                (bx.max().z - 4.0).abs() <= 1e-9,
                "fix {searches_trim}: z max {}",
                bx.max().z
            );
        }
    }

    /// #16394: the cover of the unit square's four UV trim lines meets a cell across one of
    /// them and none inside or outside the square away from them. Of the cells it meets
    /// none, the one inside the face is not outside it, the ones outside the square are.
    #[test]
    fn trim_cover_of_a_square_tells_cells_outside_the_face() {
        let k = Kernel::default();
        let mut b = Brep::new();
        let f = square(&mut b, 0.0);
        let tol = zone_tol3d(&k, b.face(f).unwrap().tolerance);
        let mut lb: LazyBrep<'_> = (&b).into();
        let cover = TrimCover::of(&k, &mut lb, f)
            .unwrap()
            .expect("the square's trim lines are bounded");
        let inside = Extent2d::from_bounds(0.25, 0.25, 0.75, 0.75);
        let across = Extent2d::from_bounds(0.9, 0.4, 1.1, 0.6);
        let beyond = Extent2d::from_bounds(1.2, 1.2, 1.5, 1.5);
        assert!(!cover.meets(&inside));
        assert!(cover.meets(&across));
        assert!(!cover.meets(&beyond));
        let outside = |lb: &mut LazyBrep<'_>, cell: &Extent2d| {
            cell_outside_face(&k, lb, f, tol, Some(&cover), cell)
        };
        assert!(
            !outside(&mut lb, &inside).unwrap(),
            "the middle of the face"
        );
        assert!(
            !outside(&mut lb, &across).unwrap(),
            "a trim line crosses it"
        );
        assert!(outside(&mut lb, &beyond).unwrap(), "past the square");
        // without a cover no cell is taken as outside
        assert!(!cell_outside_face(&k, &mut lb, f, tol, None, &beyond).unwrap());
    }

    /// #16394: the crowned face's trim square `[-10, 10]^2` inside its surface's domain
    /// `[-20, 20]^2`, the surface rising to 50 at the corner outside the trim (the case of a
    /// blend surface run on past its face). With rule 5 the search solves no cell lying
    /// wholly outside the square, and fewer cells in all, and finds the same peak 4. Without
    /// it the corner's cells are solved and divided (the issue's blend face was divided so
    /// until the cell limit, and its box reached 44 mm past the face).
    #[test]
    fn search_passes_over_cells_outside_the_trim_and_keeps_the_peak() {
        let (k, b, f) = crowned_trimmed_face(50.0, true);
        let face = b.face(f).unwrap();
        let surface = face.surface.clone().unwrap();
        let domain = face.uv_domain;
        let tol = zone_tol3d(&k, face.tolerance);
        let mut lb: LazyBrep<'_> = (&b).into();
        let cover = TrimCover::of(&k, &mut lb, f)
            .unwrap()
            .expect("the square's trim curves are bounded");
        assert!(!cover.meets(&Extent2d::from_bounds(-20.0, -20.0, -15.0, -15.0)));
        assert!(!cover.meets(&Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0)));
        assert!(cover.meets(&Extent2d::from_bounds(-12.0, -1.0, -9.0, 1.0)));
        let limits = SearchLimits {
            max_depth: FACE_EXTREME_MAX_DEPTH,
            max_cells: FACE_EXTREME_MAX_CELLS,
        };
        let mut search = |cover: Option<&TrimCover>| {
            let mut solved: Vec<Extent2d> = Vec::new();
            let top = face_directed_max_with(
                &k,
                &mut lb,
                f,
                &surface,
                &domain,
                Vec3::Z,
                0.0,
                limits,
                &mut |lb, solve_on, cell, dir| {
                    solved.push(*cell);
                    cell_directed_max(&k, lb, f, &surface, solve_on, tol, cell, dir)
                },
                &mut |lb, cell| cell_outside_face(&k, lb, f, tol, cover, cell),
            )
            .unwrap();
            (top, solved)
        };
        let (with_top, with_cells) = search(Some(&cover));
        let (without_top, without_cells) = search(None);
        assert!(
            (with_top - 4.0).abs() <= 1e-9,
            "with the rule: z max {with_top}"
        );
        assert!(
            (without_top - 4.0).abs() <= 1e-9,
            "without: z max {without_top}"
        );
        let past_the_square = |c: &Extent2d| {
            c.max().x < -10.01 || c.min().x > 10.01 || c.max().y < -10.01 || c.min().y > 10.01
        };
        assert!(
            !with_cells.iter().any(past_the_square),
            "a cell outside the trim was solved"
        );
        assert!(
            without_cells.iter().any(past_the_square),
            "without the rule the corner's cells are solved"
        );
        assert!(
            with_cells.len() < without_cells.len(),
            "solves with the rule {}, without {}",
            with_cells.len(),
            without_cells.len()
        );
    }

    #[test]
    fn sector_classification_into_the_face_is_inside() {
        let k = Kernel::default();
        let mut b = Brep::new();
        let f = square(&mut b, 0.0);
        let curve = Curve::from(Line::from_points(
            Point3::new(0.5, 0.0, 0.0),
            Point3::new(0.5, 0.5, 0.0),
            3,
        ));
        let eu = |orientation| SectorEdgeuse {
            edgeuse: EdgeuseId::from_index(0),
            param: 0.5,
            zone_tol_3d: 1e-6,
            orientation,
        };
        let input = SectorClassifyInput {
            eu1: eu(OrientType::Same),
            eu2: eu(OrientType::Opposite),
            curve: &curve,
            curve_interval: curve.natural_interval(),
            curve_orientation: OrientType::Same,
            curve_zone_tol_3d: 1e-6,
            on_singularity: false,
            singularity_uv: Point2::new(0.0, 0.0),
            uv_space_okay: true,
        };
        // both binormals point into the face along the curve: the "low work" exit
        let r = sector_classify_curve_on(&k, &mut b, f, &input).unwrap();
        assert_eq!(
            r,
            SectorClass {
                inside: true,
                possible_problem: false,
                problem: ClassifyProblem::None,
            }
        );
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum H {
        Topo(EntityRef),
        Surface(FaceId),
        Curve(bool),
        EdgeCurve(EdgeId),
        Trim(EdgeuseId),
    }

    impl ClassObject for H {
        fn class_kind(&self) -> ClassObjectKind {
            match self {
                Self::Topo(e) => e.class_kind(),
                Self::Surface(_) => ClassObjectKind::Surface,
                Self::Curve(_) | Self::EdgeCurve(_) | Self::Trim(_) => ClassObjectKind::Curve,
            }
        }
    }

    impl LoopObject for H {
        fn from_loop(l: LoopId) -> Self {
            Self::Topo(EntityRef::Loop(l))
        }

        fn as_loop(&self) -> Option<LoopId> {
            match self {
                Self::Topo(e) => e.as_loop(),
                _ => None,
            }
        }

        fn as_vertex(&self) -> Option<VertexId> {
            match self {
                Self::Topo(e) => e.as_vertex(),
                _ => None,
            }
        }

        fn from_face(f: FaceId) -> Option<Self> {
            Some(Self::Topo(EntityRef::Face(f)))
        }

        fn from_face_surface(f: FaceId) -> Option<Self> {
            Some(Self::Surface(f))
        }

        fn from_classify_curve(uv: bool) -> Option<Self> {
            Some(Self::Curve(uv))
        }

        fn from_topology(t: EntityRef) -> Option<Self> {
            Some(Self::Topo(t))
        }

        fn as_topology(&self) -> Option<EntityRef> {
            match self {
                Self::Topo(t) => Some(*t),
                _ => None,
            }
        }

        fn from_edge_curve(e: EdgeId) -> Option<Self> {
            Some(Self::EdgeCurve(e))
        }

        fn as_edge_curve(&self) -> Option<EdgeId> {
            match self {
                Self::EdgeCurve(e) => Some(*e),
                _ => None,
            }
        }

        fn from_trim_curve(eu: EdgeuseId) -> Option<Self> {
            Some(Self::Trim(eu))
        }

        fn as_trim_curve(&self) -> Option<EdgeuseId> {
            match self {
                Self::Trim(eu) => Some(*eu),
                _ => None,
            }
        }
    }

    #[test]
    fn loop_object_curves_resolve_the_stored_curves() {
        let mut b = Brep::new();
        let f = square(&mut b, 0.0);
        let mut objects: LoopObjectCurves<H> = LoopObjectCurves::new(&b, f).unwrap();
        let line = |x: f64| {
            Curve::from(Line::from_points(
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(x, 1.0, 0.0),
                2,
            ))
        };
        let (h, hu) = objects
            .set_classify_curves(line(1.0), Some(line(2.0)))
            .unwrap();
        assert_eq!((h, hu), (H::Curve(false), Some(H::Curve(true))));
        assert!(ClassGeometry::curve(&objects, h).is_some());
        assert!(ClassGeometry::curve(&objects, H::Curve(true)).is_some());
        // the face's surface and edge curves resolve through the B-rep
        let s = objects.face_surface(f).unwrap();
        assert_eq!(s, H::Surface(f));
        assert!(ClassGeometry::surface(&objects, s).is_some());
        let e = EdgeId::from_index(0);
        let ec = objects.edge_curve(e);
        assert_eq!(ec, H::EdgeCurve(e));
        assert!(ClassGeometry::curve(&objects, ec).is_some());
        assert_eq!(
            objects.vertex(VertexId::from_index(0)),
            H::Topo(EntityRef::Vertex(VertexId::from_index(0)))
        );
        // a later call replaces both
        let (h2, hu2) = objects.set_classify_curves(line(3.0), None).unwrap();
        assert_eq!((h2, hu2), (H::Curve(false), None));
        assert!(objects.classify_uv_curve().is_none());
        // a handle type that cannot name curves cannot make them
        assert!(LoopObjectCurves::<EntityRef>::new(&b, f).is_err());
    }

    #[test]
    fn bisector_target_out_of_range_is_invalid() {
        let k = Kernel::default();
        let mut b = Brep::new();
        let f = square(&mut b, 0.0);
        let props: FaceProps<H> = FaceProps::for_face(f, 0);
        let mut objects: LoopObjectCurves<H> = LoopObjectCurves::new(&b, f).unwrap();
        let mut cc: CurveClassification<H> = CurveClassification::new(&k);
        let r = find_and_classify_loop_bisector(
            &k,
            &b,
            f,
            3,
            &[LoopId::from_index(0)],
            &[],
            &BTreeSet::new(),
            &props,
            &mut objects,
            &mut cc,
        );
        assert!(matches!(r, Err(SolidError::InvalidInput { .. })));
    }

    #[test]
    fn bisector_into_sets_its_outputs_before_failing() {
        // upstream initialises the seam flags (references to the caller's variables)
        // before it fails on the target loop; ClassifyLoops ignores the status and reads
        // them
        let k = Kernel::default();
        let mut b = Brep::new();
        let f = square(&mut b, 0.0);
        let props: FaceProps<H> = FaceProps::for_face(f, 0);
        let mut objects: LoopObjectCurves<H> = LoopObjectCurves::new(&b, f).unwrap();
        let mut cc: CurveClassification<H> = CurveClassification::new(&k);
        let mut out = LoopBisector {
            start_on_seam: true,
            stop_on_seam: true,
            closed_3d: Some(true),
            has_curve: true,
        };
        let r = find_and_classify_loop_bisector_into(
            &k,
            &b,
            f,
            3,
            &[LoopId::from_index(0)],
            &[],
            &BTreeSet::new(),
            &props,
            &mut objects,
            &mut cc,
            &mut out,
        );
        assert!(matches!(r, Err(SolidError::InvalidInput { .. })));
        assert_eq!(
            out,
            LoopBisector {
                start_on_seam: false,
                stop_on_seam: false,
                closed_3d: None,
                has_curve: false,
            }
        );
    }

    #[test]
    fn heal_loop_handles_name_the_bisector_objects() {
        // SmBrep::HealBrep's handles reach the bisector through SetProps_Stage3
        use crate::topology::brep::p1::HealLoopHandle;
        let mut b = Brep::new();
        let f = square(&mut b, 0.0);
        assert!(LoopObjectCurves::<HealLoopHandle>::new(&b, f).is_ok());
    }

    #[test]
    fn wr167_default_mask_membership() {
        let pts = [
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
        ];
        for mask in 0u64..16 {
            for (i, &p) in pts.iter().enumerate() {
                assert_eq!(
                    find_element_in_masked_array(&pts, p, mask),
                    mask & (1 << i) != 0,
                    "mask={mask:#x}, i={i}"
                );
            }
            assert!(!find_element_in_masked_array(
                &pts,
                Point3::new(9.0, 0.0, 0.0),
                mask
            ));
        }
        let duplicates = [pts[0], pts[1], pts[0], pts[3]];
        assert!(find_element_in_masked_array(&duplicates, pts[0], 4));
        assert!(!find_element_in_masked_array(&duplicates, pts[0], 2));
        let mut long = vec![pts[0]; 70];
        for i in [63, 64, 69] {
            long[i] = Point3::new(i as f64, 0.0, 0.0);
        }
        assert!(find_element_in_masked_array(&long, long[63], 1u64 << 63));
        assert!(!find_element_in_masked_array(
            &long,
            long[63],
            !(1u64 << 63)
        ));
        for i in [64, 69] {
            assert!(!find_element_in_masked_array(&long, long[i], u64::MAX));
        }
    }

    #[test]
    fn wr167_find_element_in_masked_array_by_bit() {
        let pts = [
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(-3.0, 0.0, 0.0),
            Point3::new(0.0, -4.0, 0.0),
        ];
        assert_eq!([SS_UMIN, SS_VMIN, SS_UMAX, SS_VMAX], [1, 2, 4, 8]);
        // UMIN must not select either maximum side.
        assert!(!find_element_in_masked_array(&pts, pts[2], SS_UMIN));
        assert!(!find_element_in_masked_array(&pts, pts[3], SS_UMIN));

        // The default matches element `ii` if and only if `(mask & (1 << ii)) != 0`:
        for mask in 0u64..16 {
            for (ii, &pt) in pts.iter().enumerate() {
                let want = (mask & (1u64 << ii)) != 0;
                assert_eq!(
                    find_element_in_masked_array(&pts, pt, mask),
                    want,
                    "mask={mask:#x}, ii={ii}"
                );
            }
        }
        assert!(!find_element_in_masked_array(
            &pts,
            Point3::new(9.0, 9.0, 9.0),
            0xF
        ));

        // Shift bounds safety for indices >= 64 (and bit 63 boundary):
        let mut long = vec![Point3::new(-1.0, -1.0, -1.0); 70];
        let p63 = Point3::new(63.0, 0.0, 0.0);
        let p64 = Point3::new(64.0, 0.0, 0.0);
        let p69 = Point3::new(69.0, 0.0, 0.0);
        long[63] = p63;
        long[64] = p64;
        long[69] = p69;
        assert!(find_element_in_masked_array(&long, p63, 1u64 << 63));
        assert!(!find_element_in_masked_array(&long, p63, !(1u64 << 63)));
        assert!(!find_element_in_masked_array(&long, p64, u64::MAX));
        assert!(!find_element_in_masked_array(&long, p69, u64::MAX));
    }

    #[test]
    fn wr167_bisector_skips_each_masked_pole_edge() {
        use crate::heal::face_props::Tri;

        let mut k_build = Kernel::default();

        // Asymmetric rectangle `[2, 8] x [2, 7]` on the plane `S(u, v) = (u, v, 0)` over
        // `[0, 12] x [0, 11]`, so adjacent edges are perpendicular and their classifying
        // bisectors have distinct start points, midpoints and orthogonal directions.
        let dom = Extent2d::from_bounds(0.0, 0.0, 12.0, 11.0);
        let plane = Plane::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &dom,
        );
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let corners = [(2.0, 2.0), (8.0, 2.0), (8.0, 7.0), (2.0, 7.0)];
        let curves = (0..4)
            .map(|i| {
                let (x0, y0) = corners[i];
                let (x1, y1) = corners[(i + 1) % 4];
                Curve::from(
                    Line::create_line_segment(
                        3,
                        Point3::new(x0, y0, 0.0),
                        Point3::new(x1, y1, 0.0),
                        Some(&interval),
                    )
                    .unwrap(),
                )
            })
            .collect();
        let b =
            trim_surface_with_3d_curves(&mut k_build, Surface::from(plane), &[4], curves).unwrap();
        let faces = crate::topology::brep::p1::faces(&b, None).unwrap();
        assert_eq!(faces.len(), 1);
        let f = faces[0];
        let loops = crate::topology::face::p2::loops(&b, f, false, None).unwrap();
        assert_eq!(loops.len(), 1);
        let tgt_loop = loops[0];
        let tgt_lu = b.loop_(tgt_loop).unwrap().loopuse.unwrap();
        let eus = loopuse::edgeuses(&b, tgt_lu, None, None).unwrap();
        assert_eq!(eus.len(), 4);

        let edge_midpoint = |eu: EdgeuseId| -> Point3 {
            let e = eu1::edge(&b, eu).unwrap().unwrap();
            let rec = b.edge(e).unwrap();
            rec.curve
                .as_ref()
                .unwrap()
                .evaluate_point(&k_build, rec.interval.mid())
                .unwrap()
        };
        let mid0 = edge_midpoint(eus[0]);
        let mid1 = edge_midpoint(eus[1]);
        assert!((mid0 - mid1).length() > 3.0);

        let run_case = |poles: u64, pole_points: [Point3; 4]| -> (Point3, Point3, Vec3) {
            let k = Kernel::default();
            let mut props: FaceProps<H> = FaceProps::for_face(f, 0);
            props.closed_u = Tri::False;
            props.closed_v = Tri::False;
            props.zone_tol_3d = b.face(f).unwrap().tolerance;
            props.poles = poles;
            props.pole_points = pole_points.to_vec();

            let mut objects: LoopObjectCurves<H> = LoopObjectCurves::new(&b, f).unwrap();
            let mut cc: CurveClassification<H> = CurveClassification::new(&k);
            let out = find_and_classify_loop_bisector(
                &k,
                &b,
                f,
                0,
                &[tgt_loop],
                &[ClassifyLoopIo::default()],
                &BTreeSet::new(),
                &props,
                &mut objects,
                &mut cc,
            )
            .unwrap();
            assert!(out.has_curve);
            assert_eq!(out.closed_3d, Some(false));
            assert!(cc.size() >= 1);

            let crv = objects.classify_curve().unwrap();
            let ivl = crv.natural_interval();
            let p_start = crv.evaluate_point(&k, ivl.min()).unwrap();
            let p_mid = crv.evaluate_point(&k, ivl.mid()).unwrap();
            let p_end = crv.evaluate_point(&k, ivl.max()).unwrap();
            let dir = (p_end - p_start).unitize().unwrap();
            (p_start, p_mid, dir)
        };

        let far = Point3::new(-99.0, -99.0, 0.0);

        // Each singular side selects its own pole and skips edge 0 for edge 1.
        for (slot, side_mask) in [(0, SS_UMIN), (1, SS_VMIN), (2, SS_UMAX), (3, SS_VMAX)] {
            let mut poles_arr = [far; 4];
            poles_arr[slot] = mid0;
            let (start, _, dir) = run_case(side_mask, poles_arr);
            assert!((mid1 - start).cross(dir).length() < 1e-9);
            assert!((mid0 - start).cross(dir).length() > 2.0);
        }

        // UMIN alone must not select the UMAX or VMAX pole slot.
        for slot in [2usize, 3] {
            let mut poles_arr = [far; 4];
            poles_arr[slot] = mid0;
            let (start, _, dir) = run_case(SS_UMIN, poles_arr);
            assert!((mid0 - start).cross(dir).length() < 1e-9);
            assert!((mid1 - start).cross(dir).length() > 2.0);
        }
    }

    #[test]
    fn is_between_is_open() {
        assert!(is_between(1.0, 0.0, 2.0));
        assert!(is_between(1.0, 2.0, 0.0));
        assert!(!is_between(0.0, 0.0, 2.0));
        assert!(!is_between(2.0, 0.0, 2.0));
    }

    #[test]
    fn project_is_double_cross() {
        let p = Vec3::new(0.0, 0.0, 1.0);
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(project(p, v), Vec3::new(1.0, 2.0, 0.0));
    }

    #[test]
    fn classify_problem_codes() {
        assert_eq!(ClassifyProblem::None as i32, 0);
        assert_eq!(ClassifyProblem::UnresolvedClassification as i32, 7);
    }
}
