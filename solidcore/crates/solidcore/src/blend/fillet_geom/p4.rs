//! Fillet geometry, part 4: the UV trim curve of a fillet edge between two fillet
//! vertices on a surface (`sm_CreateUVCurve`), the fillet-surface intersection vertex
//! calculators (`sm_CalcNx2VertFilletXFillet`, `CalcFilletInt2Fillets`,
//! `CalcRailIntExtendedEdgeuse`), the rail index of a fillet vertex, the face of a
//! fillet geometry in the fillet B-rep (`MakeFaceBrep`), the insertion of the rails and
//! side curves into the target B-rep (`InsertIntersectionTopology`) and the blending
//! geometry (`CalcBlendingGeom`).
//!
//! Upstream: `source/SMLib/src/SmFilletGeom.cpp`, `source/SMLib/inc/SmFilletGeom.h`.
//!
//! | Rust                                              | Upstream                                     |
//! |---------------------------------------------------|----------------------------------------------|
//! | [`create_uv_curve`], [`UvCurves`]                 | `sm_CreateUVCurve`                           |
//! | [`FilletBrep::calc_nx2_vert_fillet_x_fillet`]     | `sm_CalcNx2VertFilletXFillet`                |
//! | [`FilletBrep::calc_fillet_int_2_fillets`], [`FilletBrep::calc_fillet_int_2_fillets_with_context`] | `SmFilletVertex::CalcFilletInt2Fillets` |
//! | [`FilletBrep::calc_rail_int_extended_edgeuse`]    | `SmFilletVertex::CalcRailIntExtendedEdgeuse` |
//! | [`FilletBrep::make_face_brep`]                    | `SmFilletGeom::MakeFaceBrep`                 |
//! | [`FilletBrep::rail_index_of_vertex`]              | `SmFilletGeom::GetRailIndexOfVertex`         |
//! | [`FilletBrep::insert_intersection_topology_with_context`] | `SmFilletGeom::InsertIntersectionTopology` |
//! | [`FilletBrep::calc_blending_geom`]                | `SmFilletGeom::CalcBlendingGeom`             |
//! | `degenerate_side_at` (private)                    | none (solidcore #2403)                       |
//!
//! Deviations:
//! - **Degenerate sides (solidcore #2403, not in the fork).** `sm_CreateUVCurve` takes an end that drops to one solution on a degenerate side of
//!   the surface (a patch's pole, `degenerate_side_at`) onto the iso-curve through the
//!   other end, as its rule for an end with more than two solutions does: the global
//!   solve reports such an end once, anywhere along the side.
//! - **Object model.** As in `blend.fillet_geom_p1`, a fillet vertex, edge or geometry
//!   is an id of the [`FilletBrep`] (`self`). `GetFilletSolver()` is an opaque
//!   [`FilletSolverId`] resolved in the caller's `solvers` table (index `id.0`); an id
//!   not in the table is [`SolidError::InvalidInput`], a geometry without a solver
//!   `SM_ERR_NULL_POINTER` (upstream dereferences `NULL`). The target B-rep the
//!   classifications, original faces and solver edge uses refer to is `target`; the
//!   fillet B-rep `MakeFaceBrep` builds into (`SmFilletExecutive::m_pFilletBrep`) is
//!   `brep`. The `SmContext` of a corner or solver is the [`Kernel`]; a corner that
//!   upstream dereferences only for its context must still be present (`NULL` is
//!   `SM_ERR_NULL_POINTER`).
//! - **Fillet corners.** A vertex's `SmFilletCorner*` is a
//!   [`FilletCornerId`](super::p1::FilletCornerId) that the caller's [`CornerLookup`]
//!   resolves to the [`FilletCorner`](crate::blend::fillet_corner::p3::FilletCorner) (unit
//!   `blend.fillet_corner_p1`) whose corner type and filleted vertex the calculators
//!   read; the lookup also holds the executive's extended surfaces
//!   (`GetExtendedSurface`).
//! - **Cones.** `sm_CreateUVCurve`'s `IsKindOf(SmCone_TYPE)` test is a match on
//!   [`SurfaceKind::Cone`] (which also holds `SmCylinder`s).
//! - **Curves.** Upstream's curve arrays hold pointers into the edges and copies are
//!   made at the end of `MakeFaceBrep`; here the arrays hold clones, and the final
//!   copies are made the same way (`Copy` for the 3D curves, the copy constructor for
//!   the UV curves). The retained session transfers the original fillet surface to
//!   its managed face, recording early destruction and partial installation on
//!   failure. The legacy indexed API retains its copied-surface contract.
//!   `SetCurve` on a fillet edge applies the
//!   `SM_NO_CHANGE_GEOMETRY` resets directly (as `blend.fillet_geom_p1` does: the
//!   pseudo B-rep has no notification environment) and `SetOwner` of the new curve is
//!   implied by the edge holding it.
//! - **Statuses.** `SER`/`NER` failures are errors (`SM_ERR`, `SM_ERR_NULL_POINTER`);
//!   the failure statuses `sm_CreateUVCurve` swallows (its surface/plane intersections)
//!   are swallowed here too, except that [`SolidError::Unported`] always propagates.
//!   The double tolerances passed to `GlobalPointSolve` convert to `SmZoneTol3d`
//!   (`USE_DEFAULT` becomes the zone tolerance, then `fabs`).
//! - **Failures (#15034).** Each `SER`/`NER` keeps upstream's status for the goldens
//!   (`SM_ERR_NULL_POINTER` for a `NER`, `SM_ERR` otherwise) and has its own cause.
//!   These routines are public and take a fillet B-rep the caller may set up by hand, so
//!   a link the fillet executive would have made (a vertex's corner or cross-section
//!   edge, a geometry's solver, rail or traced surface, an edge's primary use or curve,
//!   a corner's filleted vertex, ...) that is not there is the caller's set-up:
//!   [`BlendFailure::FilletSetUpInvalid`] naming the link (at the topology entity, where
//!   there is one), [`BlendFailure::FilletVertexWithoutCorner`], a solver-state cause
//!   raised through [`BlendFailure::Solver`]
//!   ([`crate::error::blend::SolverFailure::FilletGeomWithoutRail`],
//!   [`crate::error::blend::SolverFailure::FilletGeomWithoutSolver`],
//!   [`crate::error::blend::SolverFailure::FilletGeomWithoutSurface`],
//!   [`crate::error::blend::SolverFailure::SolverEdgeUseMissing`],
//!   [`crate::error::blend::SolverFailure::EdgeUseWithoutFace`]), a shared body cause
//!   ([`BlendFailure::EdgeWithoutCurve`], [`BlendFailure::EdgeUseWithoutEdge`]), or a
//!   cause of its own ([`BlendFailure::NeighbourVertexNotProcessed`],
//!   [`BlendFailure::RailNotBlending`], [`BlendFailure::SeamSurfaceNotClosed`], ...), class
//!   [`crate::error::ErrorKind::InvalidInput`]. Only a state the routine itself just
//!   built or checked (a surface traced above, a curve stored above, a result a kernel
//!   routine always returns) that is not there is a [`BlendFailure::Defect`] naming the
//!   step and why it is the kernel's fault (the `const`s at the end of this file). An
//!   intersection or drop that found nothing or too much is a named cause carrying its
//!   numbers ([`BlendFailure::UvCurveEndOffSurface`],
//!   [`BlendFailure::CornerCurvesMeetMoreThanOnce`], ...); an intersection's own error is
//!   chained under [`BlendFailure::CornerSurfacesNotIntersected`]; a surface or curve of a
//!   kind upstream casts to a B-spline is [`BlendFailure::UvCurveSurfaceNotBSpline`],
//!   [`BlendFailure::MateFaceSurfaceNotBSpline`] or [`BlendFailure::CurveWithoutNet`]; the
//!   caller's own input (no vertex given, a body face without a surface,
//!   [`BlendFailure::FaceHasNoSurface`] with status `SM_ERR_NULL_POINTER`) is an
//!   invalid-input cause. Where a missed intersection
//!   matters to the caller, the vertex's status says so, as `SurfIntFailure` does for a
//!   rail that misses the end face; no caller decides by the error.
//! - **Surface/surface intersections.** `sm_SrfSrfIntersection` is
//!   `blend.fillet_geom_p2`'s, called with the fillet vertices' points; its curves are
//!   retained as their actual B-spline-derived objects for virtual operations, and
//!   [`UvCurves`] returns them as such (an `SmEllipse` stays an ellipse).
//! - **Part 6.** `GetRailIndexOfVertexuse`, `JoinTwoFilletVertsByCurves`,
//!   `SmFilletEdge::CalcBlendingRail` and `RecordFilletError` are `blend.fillet_geom_p6`'s;
//!   `MakeFaceBrep`'s 3D curve list holds `Option<Curve>` as the join appends them.
//! - **Executive state.** `InsertIntersectionTopology` runs against the executive's
//!   topology intersector, attribute users, marker attributes (777/778) and topology
//!   bookkeeping through the caller's [`IntersectionTopologyContext`] (the live
//!   executive session).
//! - **B-spline casts.** `SM_CAST_PTR(SmBSplineCurve, ..)` succeeds for every kind
//!   derived from `SmBSplineCurve` upstream (`SmLine`, `SmEllipse`/`SmCircle`,
//!   `SmHyperbola`, `SmParabola`): `as_b_spline` returns their B-spline base.
//! - **Corner blend curves.** `CalcBlendingGeom` passes `pFE->GetCurve()` (an
//!   `SmCurve*`, uncast and unchecked) to `CreateCornerBlend`; the Rust
//!   `BSplineSurface::create_corner_blend` takes B-spline curves, so a fillet edge
//!   without a curve, or with a curve of a kind not derived from `SmBSplineCurve`, is
//!   `SM_ERR_NULL_POINTER` here.
//!   Its base surfaces are likewise `Option<&BSplineSurface>`: a base face or fillet
//!   surface whose kind has no B-spline net (offset, curve-bounded, STEP surfaces) is
//!   passed as `None`, where upstream passes the real `SmSurface*`.
//! - **Deleted edges.** `InsertIntersectionTopology` keeps one `aDeletedEdges` list for
//!   the whole routine, as upstream: each `MergeCurveOnSurfaces` appends to it and
//!   every `ClearFilletBrepEdges` clears against all edges deleted so far.
//! - **Missing mates and UV curves.** Upstream fetches `pPrimEU->GetMate()` and the
//!   UV curves of `sm_SrfSrfIntersection` early but dereferences them only where they
//!   are used; here they stay `Option`s until then, and a `NULL` one is
//!   `SM_ERR_NULL_POINTER` only at that use (upstream dereferences `NULL`).
//! - **Arguments.** Out-params become return values; `iDebugLevel` and debug drawing
//!   are dropped.

use crate::blend::fillet_solver::p1::{FilletSolver, FilletSurfaceGeneratorType};
use crate::curve::curve::{Curve, CurveKind};
use crate::error::blend::{BlendFailure, FilletSetUp, SolverFailure};
use crate::error::{EntityRef, Failure, Result, SolidError, SM_ERR, SM_ERR_NULL_POINTER};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::solution_array::{ObjectKind, SolutionArray, SolutionObject};
use crate::geom::tol::zone_tol3d;
use crate::geom::vector2d::{Point2, Vec2};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::{BlendNeighbour, OrientType};
use crate::nurbs::crv_fit::p1::ParamMethod;
use crate::surface::plane::Plane;
use crate::surface::surface::p1::map_to_2d_dir;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p7::{
    make_face_with_curves, make_face_with_curves_transfer, FaceCurvesInput, NewFace,
};
use crate::topology::edge;
use crate::topology::edgeuse::p1::{self as eu1, primary_edgeuse};
use crate::topology::trimming_tools::{trim_surface_with_model_space_curves, TrimInput};
use crate::topology::vertex;
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, VertexId};

use super::p1::{
    cleanup_noise, join_verts_by_capping, CornerLookup, CurveObjectHandle, FilStatus, FilletBrep,
    FilletCornerType, FilletEdgeType, FilletGeomId, FilletGeomType, FilletSolverId,
};
use super::p6::join_two_fillet_verts_by_curves_resolved;

/// The indexed value API retains its historical copied-surface behavior. The live
/// retained-solver API transfers the original surface into its actual managed face.
#[derive(Clone, Copy)]
enum FaceSurfaceBackend {
    IndexedValue,
    RetainedTransfer,
}

const OP_UV: &str = "blend::fillet_geom::p4::create_uv_curve";
const OP_NX2: &str = "blend::fillet_geom::p4::FilletBrep::calc_nx2_vert_fillet_x_fillet";
const OP_INT2: &str = "blend::fillet_geom::p4::FilletBrep::calc_fillet_int_2_fillets";
const OP_EXT: &str = "blend::fillet_geom::p4::FilletBrep::calc_rail_int_extended_edgeuse";
const OP_FACE: &str = "blend::fillet_geom::p4::FilletBrep::make_face_brep";
const OP_RAIL_IDX: &str = "blend::fillet_geom::p4::FilletBrep::rail_index_of_vertex";
const OP_INSERT: &str = "blend::fillet_geom::p4::FilletBrep::insert_intersection_topology";
const OP_BLEND: &str = "blend::fillet_geom::p4::FilletBrep::calc_blending_geom";

/// The solver of a fillet geometry in the caller's table (index `id.0`).
pub(crate) fn solver_of<'s>(
    solvers: &'s [FilletSolver],
    id: FilletSolverId,
    op: &'static str,
) -> Result<&'s FilletSolver> {
    usize::try_from(id.0)
        .ok()
        .and_then(|i| solvers.get(i))
        .ok_or_else(|| SolidError::invalid_input(op, "fillet solver id not in the solver table"))
}

/// `smos_Min`.
fn smin(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `SmVertex::SetPoint` on a fillet vertex: the `SM_NO_CHANGE_GEOMETRY` notification
/// resets the cached gaps of every vertex use of the vertex, then the point is stored
/// (as `blend.fillet_geom_p1` does).
pub(crate) fn set_vertex_point(brep: &mut Brep, v: VertexId, point: Point3) -> Result<()> {
    for vu in brep.vertex(v)?.vertexuses.clone() {
        let vu = brep.vertexuse_mut(vu)?;
        vu.vertex_edge_gap.reset();
        vu.vertex_face_gap.reset();
    }
    brep.vertex_mut(v)?.point = point;
    Ok(())
}

/// `SmEdge::SetCurve(pCurve, TRUE)` on a fillet edge: the `SM_NO_CHANGE_GEOMETRY`
/// notification clears each edge use's UV trim curve and drop-curve record and resets
/// its cached edge/face gap (its mate's too) and the cached gaps of the edge's vertex
/// uses; then the curve (possibly `NULL`) replaces the old one (as
/// `blend.fillet_geom_p1` does for a rail).
pub(crate) fn set_edge_curve(brep: &mut Brep, e: EdgeId, curve: Option<Curve>) -> Result<()> {
    for eu in edge::edgeuses(brep, e, None, None)? {
        let mate_eu = eu1::mate(brep, eu)?;
        let rec = brep.edgeuse_mut(eu)?;
        rec.uv_trim_curve = None;
        rec.drop_curve_fail = None;
        rec.max_edge_face_gap.reset();
        let vu = rec.vertexuse;
        if let Some(m) = mate_eu {
            brep.edgeuse_mut(m)?.max_edge_face_gap.reset();
        }
        if let Some(vu) = vu {
            let vu = brep.vertexuse_mut(vu)?;
            vu.vertex_edge_gap.reset();
            vu.vertex_face_gap.reset();
        }
    }
    brep.edge_mut(e)?.curve = curve;
    Ok(())
}

/// The crossing of the rail curve `rail` with `surface` nearest `near`, its parameter and
/// point, as an End's second pass places the rail's end
/// ([`FilletBrep::calc_rail_int_end_surface`]); `None` where they do not cross.
///
/// # Errors
///
/// Those of the intersection and evaluations.
pub(crate) fn rail_crossing_nearest(
    k: &Kernel,
    rail: &Curve,
    surface: &Surface,
    near: Point3,
    tol: f64,
) -> Result<Option<(f64, Point3)>> {
    let interval = rail.natural_interval();
    let solutions = surface.global_curve_intersect(
        k,
        [
            super::p1::CliffHandle::Curve(rail),
            super::p1::CliffHandle::Surface(surface),
        ],
        &surface.natural_uv_domain_with_nurb(k)?,
        rail,
        &interval,
        tol,
    )?;
    let mut best = None;
    let mut best_dist = BIG_DOUBLE;
    for sol in solutions.as_slice() {
        let t = sol.start.param(0)?;
        let pnt = rail.evaluate_point(k, t)?;
        let dist = pnt.distance(near);
        if dist < best_dist {
            best_dist = dist;
            best = Some((t, pnt));
        }
    }
    Ok(best)
}

/// `SmPoint3d(SmPoint2d)`: `z = 0`.
const fn p3(uv: Point2) -> Point3 {
    Point3::new(uv.x, uv.y, 0.0)
}

/// The surface `sm_CreateUVCurve`'s point solves name (upstream `pSurface`, `this` of
/// `SmSurface::GlobalPointSolve`), with the natural UV domain
/// `SmSolution::GetParameterInterval` reads back from it:
/// `SmGlobalSolver::IdenticalSolutions` (SmGlobalSolver.cpp:1910-1931) merges two
/// solutions whose parameters differ by less than `1e-4` of that domain's sides.
#[derive(Debug, Clone, Copy, PartialEq)]
struct UvSurfaceHandle(Extent2d);

impl SolutionObject for UvSurfaceHandle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }

    fn natural_uv_domain(&self) -> Option<Extent2d> {
        Some(self.0)
    }
}

/// The UV parameters (`m_vStart[0]`, `m_vStart[1]`) of solution `i`.
fn solution_uv<O: SolutionObject>(
    sols: &SolutionArray<O>,
    i: usize,
    op: &'static str,
) -> Result<Point2> {
    let s = sols
        .get(i)
        .ok_or_else(|| SolidError::invalid_input(op, "solution index out of range"))?;
    Ok(Point2::new(s.start.parameters[0], s.start.parameters[1]))
}

/// `SER(p)` at a step of the routine `op` that is the kernel's fault, for the reason `why`
/// (one of the `const`s at the end of this file): a [`BlendFailure::Defect`], status
/// `SM_ERR`; the failure's origin is the call's file and line.
#[track_caller]
fn defect(op: &'static str, why: &'static str) -> SolidError {
    SolidError::blend(op, BlendFailure::Defect { why })
}

/// `NER(p)` on a state the kernel itself just built in this routine (a traced surface,
/// a stored rail or UV curve) that is not there, for the reason `why`: a
/// [`BlendFailure::Defect`] keeping upstream's `SM_ERR_NULL_POINTER` for the goldens
/// (#15034 rule 2).
#[track_caller]
fn broken_link(op: &'static str, why: &'static str) -> SolidError {
    Failure::new(op, BlendFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// `NER(p)` on a link of the caller's fillet B-rep (or of a solver or corner it names)
/// that is not there: the executive makes every such link before it runs these routines,
/// so only a fillet B-rep set up by hand through the public API lacks one
/// ([`BlendFailure::FilletSetUpInvalid`] with `part`, status `SM_ERR_NULL_POINTER`; the
/// failure is at `at`, where the site has a topology entity).
#[track_caller]
fn set_up(op: &'static str, part: FilletSetUp, at: Option<EntityRef>) -> SolidError {
    set_up_failure(op, part, at).into_error()
}

/// [`set_up`] as a [`Failure`], for a site that keeps another status.
#[track_caller]
fn set_up_failure(op: &'static str, part: FilletSetUp, at: Option<EntityRef>) -> Failure {
    let failure = Failure::new(op, BlendFailure::FilletSetUpInvalid(part));
    match at {
        Some(entity) => failure.at(entity),
        None => failure,
    }
}

/// The fillet vertex `v` has no fillet corner, which upstream dereferences
/// ([`BlendFailure::FilletVertexWithoutCorner`] at the vertex, status
/// `SM_ERR_NULL_POINTER`).
#[track_caller]
fn without_corner(op: &'static str, v: VertexId) -> SolidError {
    Failure::new(op, BlendFailure::FilletVertexWithoutCorner)
        .at(EntityRef::Vertex(v))
        .into_error()
}

/// The caller's fillet geometry `fillet_geom` has no rail on side `side`, which upstream
/// dereferences
/// ([`SolverFailure::FilletGeomWithoutRail`], status `SM_ERR_NULL_POINTER`).
#[track_caller]
fn no_rail(op: &'static str, fillet_geom: FilletGeomId, side: usize) -> SolidError {
    SolidError::blend_solver(
        op,
        SolverFailure::FilletGeomWithoutRail { fillet_geom, side },
    )
}

/// The caller's fillet geometry `g` names no fillet solver, which upstream dereferences
/// ([`SolverFailure::FilletGeomWithoutSolver`], status `SM_ERR_NULL_POINTER`).
#[track_caller]
fn no_solver(op: &'static str, g: FilletGeomId) -> SolidError {
    SolidError::blend_solver(
        op,
        SolverFailure::FilletGeomWithoutSolver { fillet_geom: g },
    )
}

/// The caller's fillet geometry `g` has no fillet surface, which upstream dereferences
/// ([`SolverFailure::FilletGeomWithoutSurface`], status `SM_ERR_NULL_POINTER` unless the
/// site says otherwise).
const fn no_surface(g: FilletGeomId) -> SolverFailure {
    SolverFailure::FilletGeomWithoutSurface { fillet_geom: g }
}

/// The caller's fillet B-rep's edge `e` has no curve, which upstream dereferences
/// ([`BlendFailure::EdgeWithoutCurve`] at the edge, status `SM_ERR_NULL_POINTER`).
#[track_caller]
fn edge_without_curve(op: &'static str, e: EdgeId) -> SolidError {
    Failure::new(op, BlendFailure::EdgeWithoutCurve)
        .at(EntityRef::Edge(e))
        .into_error()
}

/// The fillet vertex `vertex` across a fillet edge is not processed yet, where the
/// executive's order processes it first ([`BlendFailure::NeighbourVertexNotProcessed`],
/// status `SM_ERR`, upstream's `SER`).
#[track_caller]
fn not_processed(op: &'static str, vertex: VertexId) -> SolidError {
    Failure::new(op, BlendFailure::NeighbourVertexNotProcessed { vertex })
        .at(EntityRef::Vertex(vertex))
        .into_error()
}

/// The caller's body's edge use `eu` has no face, which upstream dereferences
/// ([`SolverFailure::EdgeUseWithoutFace`], status `SM_ERR_NULL_POINTER`).
#[track_caller]
fn eu_without_face(op: &'static str, eu: EdgeuseId) -> SolidError {
    Failure::new(op, BlendFailure::Solver(SolverFailure::EdgeUseWithoutFace))
        .at(EntityRef::Edgeuse(eu))
        .into_error()
}

/// The caller's body's face `face` without a surface, which upstream dereferences
/// ([`BlendFailure::FaceHasNoSurface`] at the face, status `SM_ERR_NULL_POINTER`).
#[track_caller]
fn face_without_surface(op: &'static str, face: FaceId) -> SolidError {
    Failure::new(op, BlendFailure::FaceHasNoSurface)
        .with_status(SM_ERR_NULL_POINTER)
        .at(EntityRef::Face(face))
        .into_error()
}

/// `curve` (of fillet edge `edge`, where the step names one) is of a kind not derived
/// from a B-spline curve, where upstream casts it to `SmBSplineCurve` and dereferences
/// the result ([`BlendFailure::CurveWithoutNet`], keeping `SM_ERR_NULL_POINTER`).
#[track_caller]
fn curve_without_net(op: &'static str, edge: Option<EdgeId>, curve: &Curve) -> SolidError {
    Failure::new(
        op,
        BlendFailure::CurveWithoutNet {
            edge,
            kind: curve.type_string(),
        },
    )
    .with_status(SM_ERR_NULL_POINTER)
    .into_error()
}

/// The ends of a UV trim curve drop onto its surface `start` and `end` times, in a way
/// no curve is built for ([`BlendFailure::UvCurveEndsUnresolved`], status `SM_ERR`).
#[track_caller]
fn ends_unresolved(start: usize, end: usize) -> SolidError {
    SolidError::blend(
        OP_UV,
        BlendFailure::UvCurveEndsUnresolved {
            start_solutions: start,
            end_solutions: end,
        },
    )
}

/// A surface/surface intersection at a fillet corner that gave no curve: its error
/// (`Some`) chained as the cause of [`BlendFailure::CornerSurfacesNotIntersected`], so
/// that error decides defect or refusal, or, for an intersection that returned
/// no curve (`None`), a reviewed [`BlendFailure::Defect`] ([`CORNER_SSI_NO_CURVE`]: the
/// corners' calls never get `Ok` without a curve). Status `SM_ERR` either way, as
/// upstream's `SER`.
#[track_caller]
fn corner_intersection_failed(op: &'static str, error: Option<SolidError>) -> SolidError {
    match error {
        Some(e) => Failure::new(op, BlendFailure::CornerSurfacesNotIntersected)
            .caused_by(e)
            .into_error(),
        None => defect(op, CORNER_SSI_NO_CURVE),
    }
}

/// A B-spline 3D or UV curve held as a [`Curve`] (`SM_CAST_PTR(SmBSplineCurve, ..)`):
/// the B-spline (base) of the kinds derived from `SmBSplineCurve` (`SmLine`,
/// `SmEllipse`/`SmCircle`, `SmHyperbola`, `SmParabola`), `None` for the others.
const fn as_b_spline(c: &Curve) -> Option<&BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Line(l) => Some(l.base()),
        CurveKind::Ellipse(e) => Some(e.base()),
        CurveKind::Hyperbola(h) => Some(h.base()),
        CurveKind::Parabola(p) => Some(p.base()),
        CurveKind::Hermite(_)
        | CurveKind::Conic(_)
        | CurveKind::Projected(_)
        | CurveKind::Iso(_)
        | CurveKind::Offset(_)
        | CurveKind::CrvInVolume(_)
        | CurveKind::CrvOnSurf(_)
        | CurveKind::TangentField(_)
        | CurveKind::Composite(_) => None,
    }
}

/// The B-spline net of a surface that is an `SmBSplineSurface` (a B-spline surface, a
/// plane, a surface of revolution, a cone, a sphere or a torus):
/// `SM_CAST_PTR(SmBSplineSurface, ..)`.
fn b_spline_net(s: &Surface) -> Option<&BSplineSurface> {
    match s.kind() {
        SurfaceKind::BSpline(b) => Some(b),
        SurfaceKind::Plane(p) => Some(p.base()),
        SurfaceKind::SurfOfRevolution(r) => Some(r.base()),
        SurfaceKind::SurfOfExtrusion(r) => Some(r.base()),
        SurfaceKind::Cone(c) => Some(c.rev().base()),
        SurfaceKind::Sphere(s) => Some(s.base().base()),
        SurfaceKind::Torus(t) => Some(t.base().base()),
        // (the fork's SmProceduralBlend is an SmBSplineSurface: its net)
        SurfaceKind::ProceduralBlend(pb) => Some(pb.net()),
        // (the fork's SmBallPatch is an SmBSplineSurface too: its net)
        SurfaceKind::BallPatch(pb) => Some(pb.net()),
        _ => None,
    }
}

// ---- unported callees ---------------------------------------------------------------

/// The curves `sm_SrfSrfIntersection` returns: the 3D intersection curve (`NULL` on a
/// success without a curve) and its UV curves on both surfaces.
/// Also used by `blend.fillet_geom_p5`.
pub(super) struct SrfSrfCurves {
    pub(super) curve_3d: Option<Curve>,
    pub(super) uv_curve1: Option<Curve>,
    pub(super) uv_curve2: Option<Curve>,
}

/// `sm_SrfSrfIntersection` ([`super::p2::srf_srf_intersection`]) with fillet vertices
/// (their points) for its `SmVertex` arguments. The native B-spline casts retain
/// the actual analytic curve objects. Also used by `blend.fillet_geom_p5`.
#[allow(clippy::too_many_arguments)]
pub(super) fn srf_srf_intersection(
    k: &Kernel,
    fb: &FilletBrep,
    start_fv: Option<VertexId>,
    end_fv: Option<VertexId>,
    dir: Option<&mut Vec3>,
    end_dir: Option<Vec3>,
    surface1: &Surface,
    surface2: &Surface,
    approx_tol: f64,
    ang_tol: f64,
    skip_two_pnts_intersection: bool,
    ref_point: Option<Point3>,
) -> Result<SrfSrfCurves> {
    srf_srf_intersection_with_context(
        k,
        fb,
        start_fv,
        end_fv,
        dir,
        end_dir,
        surface1,
        surface2,
        approx_tol,
        ang_tol,
        skip_two_pnts_intersection,
        ref_point,
        None,
    )
}

/// [`srf_srf_intersection`] with the general intersector's face-aware cache context
/// ([`super::p2::srf_srf_intersection_with_context`]). Also used by
/// `blend.fillet_geom_p5`.
#[allow(clippy::too_many_arguments)]
pub(super) fn srf_srf_intersection_with_context(
    k: &Kernel,
    fb: &FilletBrep,
    start_fv: Option<VertexId>,
    end_fv: Option<VertexId>,
    dir: Option<&mut Vec3>,
    end_dir: Option<Vec3>,
    surface1: &Surface,
    surface2: &Surface,
    approx_tol: f64,
    ang_tol: f64,
    skip_two_pnts_intersection: bool,
    ref_point: Option<Point3>,
    context: Option<super::p2::SrfSrfContext<'_>>,
) -> Result<SrfSrfCurves> {
    let start = start_fv.map(|v| fb.vertex_point(v)).transpose()?;
    let end = end_fv.map(|v| fb.vertex_point(v)).transpose()?;
    let r = super::p2::srf_srf_intersection_with_context(
        k,
        start,
        end,
        dir,
        end_dir,
        surface1,
        surface2,
        approx_tol,
        ang_tol,
        super::p2::SrfSrfOptions {
            skip_two_pnts_intersection,
            ref_point,
            closed_by_start_direction: false,
        },
        context,
    )?;
    let part = |c: Option<Curve>| c.filter(|curve| as_b_spline(curve).is_some());
    Ok(SrfSrfCurves {
        curve_3d: part(r.curve_3d),
        uv_curve1: part(r.uv_curve1),
        uv_curve2: part(r.uv_curve2),
    })
}

/// The curve lists `MakeFaceBrep` grows: 3D curves (`NULL` entries where a side edge
/// has no curve, as `JoinTwoFilletVertsByCurves` appends them), UV curves (`NULL`
/// entries for a cliff side patch) and orientations.
#[derive(Debug, Default)]
struct CurveLists {
    curves_3d: Vec<Option<Curve>>,
    uv_curves: Vec<Option<Curve>>,
    orients: Vec<OrientType>,
}

/// Native lifetime of a face marker during geometry insertion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntersectionFaceRole {
    /// Rail merge: temporarily enable editing and retain marker 777.
    Rail,
    /// Side merge: retain marker 778 without changing editing.
    Side,
}

/// Retained objects and scoped edits used by `InsertIntersectionTopology`.
/// Surface bindings must identify the original object; resolving a replacement
/// surface as though it were that object is an error.
pub trait IntersectionTopologyContext {
    /// The actual face marker owned for one native scope.
    type FaceMarker;
    /// Authority for the original fillet surface, not a copied surface.
    type SurfaceBinding;

    /// The caller's kernel.
    fn kernel(&self) -> &Kernel;
    /// Resolve the executive's intersector at the native getter position.
    /// # Errors
    /// Missing live intersector context.
    fn begin_intersection(&mut self, solver: &FilletSolver) -> Result<()>;
    /// Capture the original surface at the native getter position.
    /// # Errors
    /// Unresolvable geometry or surface authority.
    fn surface_binding(
        &mut self,
        pseudo: &FilletBrep,
        geometry: FilletGeomId,
    ) -> Result<Option<Self::SurfaceBinding>>;
    /// Read the current users of the executive's actual edge attribute.
    /// # Errors
    /// Missing attribute or unresolvable users.
    fn attribute_edges(&self) -> Result<Vec<EdgeId>>;
    /// Attach the marker, run the body, and clean up on every return. Rail scopes
    /// also restore the previous editing flag after the body.
    /// # Errors
    /// Marker setup or the body fails.
    fn with_face_scope<T>(
        &mut self,
        target: &mut Brep,
        face: FaceId,
        role: IntersectionFaceRole,
        body: impl FnOnce(&mut Self, &mut Brep, &Self::FaceMarker) -> Result<T>,
    ) -> Result<T>;
    /// Run `body` on the B-rep of the original face of solver `solver`'s rail side
    /// `side` (`pBaseFace->GetBrep()`): `target`, unless the context holds another
    /// base B-rep for that rail (a surface/surface fillet's second surface).
    /// # Errors
    /// Those of `body`, or a base B-rep the context cannot lend.
    fn with_rail_base<T>(
        &mut self,
        target: &mut Brep,
        solver: FilletSolverId,
        side: usize,
        body: impl FnOnce(&mut Self, &mut Brep) -> Result<T>,
    ) -> Result<T> {
        let _ = (solver, side);
        body(self, target)
    }
    /// Read the current tolerance through a short borrow of the managed edge.
    /// # Errors
    /// The managed edge is no longer live or belongs to another body.
    fn managed_edge_tolerance(&self, edge: EdgeId) -> Result<f64>;
    /// Merge the actual curve into its target face and associated managed edge.
    /// # Errors
    /// The native merge fails or lacks a required capability. Deleted edges are
    /// appended even on ordinary failure; the existing list is never cleared.
    #[allow(clippy::too_many_arguments)]
    fn merge_curve(
        &mut self,
        pseudo: &mut FilletBrep,
        target: &mut Brep,
        face: FaceId,
        tolerance: f64,
        pseudo_edge: EdgeId,
        managed_edge: EdgeId,
        deleted_edges: &mut Vec<EdgeId>,
    ) -> Result<()>;
    /// Process users of the still-live marker after a merge.
    /// # Errors
    /// Topology bookkeeping fails.
    fn update_topology(
        &mut self,
        pseudo: &mut FilletBrep,
        target: &mut Brep,
        marker: &Self::FaceMarker,
        face: FaceId,
    ) -> Result<()>;
    /// Register the native empty intersection against the original surface.
    /// # Errors
    /// Registration fails, or the original surface is no longer resolvable.
    fn register_intersection(
        &mut self,
        pseudo: &FilletBrep,
        target: &mut Brep,
        face: FaceId,
        surface: &Option<Self::SurfaceBinding>,
    ) -> Result<()>;
    /// Record the ordinary rail-merge failure at its native position.
    /// # Errors
    /// Error recording fails or lacks actual context.
    #[allow(clippy::too_many_arguments)]
    fn record_bad_merge(
        &mut self,
        pseudo: &FilletBrep,
        geometry: FilletGeomId,
        filleted_edge: Option<EdgeId>,
        target: &Brep,
        face: FaceId,
        rail: EdgeId,
        surface: &Option<Self::SurfaceBinding>,
    ) -> Result<()>;
}

// ---- sm_CreateUVCurve -----------------------------------------------------------------

/// The curves [`create_uv_curve`] builds: `rp3DCurve` (only when the UV curve is not
/// an iso-parametric line), `rpUVCurve1` and `rpUVCurve2` (the second seam curve).
#[derive(Debug, Clone, Default)]
pub struct UvCurves {
    /// `rp3DCurve`: the actual `SmBSplineCurve`-derived object (an `SmEllipse` from a
    /// plane cut of a cylinder, for one).
    pub curve_3d: Option<Curve>,
    /// `rpUVCurve1`: the actual `SmBSplineCurve`-derived object.
    pub uv_curve1: Option<Curve>,
    /// `rpUVCurve2`.
    pub uv_curve2: Option<BSplineCurve>,
}

/// `sm_CreateUVCurve`: the UV trim curve on `surface` running from the point of
/// `start_fv` to that of `end_fv`.
///
/// With `create_two_boundary_curves` the points are on the seam of a surface closed in
/// u: the two u-boundary iso-lines (trimmed at the cut corner when one v corner of the
/// surface is cut off) are returned. Otherwise both points are dropped onto the surface
/// (intersecting with growing tolerances, then minimising); multiple solutions on a
/// seam or a degenerate edge are sorted out, and the result is an iso line, a second
/// seam line when both points are on the seam, or, for a curve that is not an iso
/// line, the intersection of the surface with a plane through the end point.
///
/// # Errors
///
/// `SM_ERR` when the seam surface is not closed, a cut corner is not found, a point
/// does not drop or multiple solutions cannot be sorted out; `SM_ERR_NULL_POINTER` for
/// a surface that is not an `SmBSplineSurface` in the plane branch; those of the
/// evaluations, solves and line creations; [`SolidError::Unported`] from the plane
/// branch's `sm_SrfSrfIntersection` ([`super::p2::srf_srf_intersection`]).
pub fn create_uv_curve(
    k: &Kernel,
    fb: &FilletBrep,
    start_fv: VertexId,
    end_fv: VertexId,
    create_two_boundary_curves: bool,
    surface: &Surface,
    approx_tol: f64,
) -> Result<UvCurves> {
    create_uv_curve_on_face(
        k,
        fb,
        start_fv,
        end_fv,
        create_two_boundary_curves,
        surface,
        approx_tol,
        None,
    )
}

/// [`create_uv_curve`] of `surface` standing for the surface of face `f` of `b`
/// (`on_face`): the point drops' fast-drop trim test classifies on the face
/// (`pFace->PointClassify` in `GlobalPointSolve`), making the face's trimmed surface
/// cache when it has none, as upstream's face-owned surface does
/// (`crate::intersect::point_class::live_face_fast_drop_test`). With `None` the
/// drops run as [`create_uv_curve`]'s.
///
/// # Errors
///
/// As [`create_uv_curve`], and those of the face classification.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)] // one upstream routine
pub fn create_uv_curve_on_face(
    k: &Kernel,
    fb: &FilletBrep,
    start_fv: VertexId,
    end_fv: VertexId,
    create_two_boundary_curves: bool,
    surface: &Surface,
    approx_tol: f64,
    mut on_face: Option<(&mut Brep, FaceId)>,
) -> Result<UvCurves> {
    // init output
    let mut out = UvCurves::default();

    // locals
    let start_pnt = fb.vertex_point(start_fv)?;
    let end_pnt = fb.vertex_point(end_fv)?;
    let uv_domain = surface.natural_uv_domain_with_nurb(k)?;

    // when asked - create 2 UVTrimCurves running along surface seam
    if create_two_boundary_curves {
        // First double-check that the surface is indeed closed.
        let mut uv00 = uv_domain.min();
        let mut uv11 = uv_domain.max();
        let mut uv01 = Point2::new(uv00.x, uv11.y);
        let mut uv10 = Point2::new(uv11.x, uv00.y);

        // evaluate Vmin corners
        let srf_pt00 = surface.evaluate_point(k, uv00)?;
        let srf_pt10 = surface.evaluate_point(k, uv10)?;
        if srf_pt00.distance(srf_pt10) > approx_tol {
            return Err(SolidError::blend(OP_UV, BlendFailure::SeamSurfaceNotClosed));
        }

        // evaluate Vmax corners
        let srf_pt01 = surface.evaluate_point(k, uv01)?;
        let srf_pt11 = surface.evaluate_point(k, uv11)?;
        if srf_pt01.distance(srf_pt11) > approx_tol {
            return Err(SolidError::blend(OP_UV, BlendFailure::SeamSurfaceNotClosed));
        }

        // Now check which v-direction is indicated by Start & End verts
        let mut which_corner_cut: i32 = 0; // -1 for vMin, +1 for vMax
        let mut direction: i32 = if start_pnt.distance(srf_pt00) < approx_tol {
            1
        } else if end_pnt.distance(srf_pt00) < approx_tol {
            -1
        } else {
            0
        };

        if direction == 0 {
            // Perhaps the SMin corner has been cut off.  Check SMax corner.
            direction = if end_pnt.distance(srf_pt01) < approx_tol {
                1
            } else if start_pnt.distance(srf_pt01) < approx_tol {
                -1
            } else {
                0
            };

            // Not currently handling it if both corners are cut off.
            if direction == 0 {
                return Err(SolidError::blend(OP_UV, BlendFailure::SeamCornersBothCut));
            }

            which_corner_cut = -1;
        } else {
            // the low-v corner is not cut off: is the high-v corner ok?
            which_corner_cut = if (direction > 0 && end_pnt.distance(srf_pt01) > approx_tol)
                || (direction < 0 && start_pnt.distance(srf_pt01) > approx_tol)
            {
                1
            } else {
                which_corner_cut
            };
        }

        // the rib is circular and less than 180 degrees: the midpoint converges
        let guess_param = 0.5;

        // when a corner has been cut - find the param value that trims the isoCurve to
        // the cut corner
        if which_corner_cut != 0 {
            let pt_to_drop = if which_corner_cut * direction > 0 {
                end_pnt
            } else {
                start_pnt
            };

            // Extract the full-length constant start-u isoParameter rib curve.
            let rib_iso_curve = surface
                .create_iso_parametric_curve(k, SurfParamType::U, uv00.x, approx_tol, None, false)?
                .curve;

            // Drop sPtToDrop to full curve to find trim param value for cut corner
            let dropped = rib_iso_curve.drop_point(
                k,
                &rib_iso_curve.natural_interval(),
                pt_to_drop,
                None,
                approx_tol,
                Some(guess_param),
                SolverOperation::Minimize,
            )?;
            if !dropped.success {
                return Err(SolidError::blend(
                    OP_UV,
                    BlendFailure::SeamCutOffRib {
                        tolerance: approx_tol,
                    },
                ));
            }
            let drop_param = dropped.parameter;

            // trim the UVTrimCurve on the seam side of the surface which has been cut
            if which_corner_cut < 0 {
                uv00 = Point2::new(uv00.x, drop_param);
                uv10 = Point2::new(uv10.x, drop_param);
            } else {
                uv01 = Point2::new(uv01.x, drop_param);
                uv11 = Point2::new(uv11.x, drop_param);
            }
        }

        // set up the desired IsoParameter intervals
        let (start_uv0, end_uv0, start_uv1, end_uv1) = if direction > 0 {
            // run constant u IsoCurve from Surface v Min to Max
            (uv00, uv01, uv10, uv11)
        } else {
            // run constant u IsoCurve from Surface v Max to Min
            (uv01, uv00, uv11, uv10)
        };

        // make Surface constant uMin isoParam curve
        out.uv_curve1 = Some(Curve::from(BSplineCurve::create_line_segment(
            2,
            p3(start_uv0),
            p3(end_uv0),
        )?));

        // make Surface constant uMax isoParam curve
        out.uv_curve2 = Some(BSplineCurve::create_line_segment(
            2,
            p3(start_uv1),
            p3(end_uv1),
        )?);

        // all done
        return Ok(out);
    }

    // arrive here when desired UVTrimCurve is not known to sit on a Seam

    // Compute dropping tolerance - start with a tight tolerance and relax that as needed
    let bbox = surface
        .calculate_bounding_box(k, &uv_domain, [true, false, false], true)?
        .normal_box
        .ok_or_else(|| broken_link(OP_UV, UV_NO_BOX))?;
    let mut dropping_tol = 1.0e-9 * bbox.size().max_dimension();

    // Param tolerance
    let param_tol = (uv_domain.size().max_dimension() + 1.0) * EFF_ZERO;

    // the solutions name pSurface (SmFilletGeom.cpp:4461-4504), see UvSurfaceHandle
    let handle = UvSurfaceHandle(surface.natural_uv_domain_with_nurb(k)?);
    let mut solve = |pnt: Point3, op: SolverOperation, tol: f64, req: SolutionRequested| {
        if let Some((b, f)) = on_face.as_mut() {
            let f = *f;
            let mut test_on: Option<bool> = None;
            let mut classify = |k: &Kernel, uv: Point2, src_tol: f64| -> Result<bool> {
                crate::intersect::point_class::live_face_fast_drop_test(
                    k,
                    b,
                    f,
                    &mut test_on,
                    uv,
                    src_tol,
                )
            };
            surface.global_point_solve_face_tested(
                k,
                Some(handle),
                &uv_domain,
                op,
                pnt,
                zone_tol3d(k, tol),
                None,
                req,
                Some(&mut classify),
            )
        } else {
            surface.global_point_solve(
                k,
                Some(handle),
                &uv_domain,
                op,
                pnt,
                zone_tol3d(k, tol),
                None,
                req,
            )
        }
    };

    // try increasing tolerance sized drops of Start/End Pts to surface
    let mut solutions1: SolutionArray<UvSurfaceHandle> = SolutionArray::new();
    let mut solutions2: SolutionArray<UvSurfaceHandle> = SolutionArray::new();
    let mut num_solutions1 = 0usize;
    let mut num_solutions2 = 0usize;
    for _ in 0..7 {
        // Drop the start point
        if num_solutions1 == 0 {
            solutions1 = solve(
                start_pnt,
                SolverOperation::Intersect,
                dropping_tol,
                SolutionRequested::All,
            )?;
        }

        // Drop the end point
        if num_solutions2 == 0 {
            solutions2 = solve(
                end_pnt,
                SolverOperation::Intersect,
                dropping_tol,
                SolutionRequested::All,
            )?;
        }

        // increase the drop tolerance
        dropping_tol *= 10.0;

        // quit iterating once both points drop to surface
        num_solutions1 = solutions1.len();
        num_solutions2 = solutions2.len();
        if num_solutions1 > 0 && num_solutions2 > 0 {
            break;
        }
    }

    // when DropPoint Intersection failed - try DropPoint Minimize
    if num_solutions1 == 0 {
        solutions1 = solve(
            start_pnt,
            SolverOperation::Minimize,
            approx_tol,
            SolutionRequested::Single,
        )?;
        num_solutions1 = solutions1.len();
    }
    if num_solutions2 == 0 {
        solutions2 = solve(
            end_pnt,
            SolverOperation::Minimize,
            approx_tol,
            SolutionRequested::Single,
        )?;
        num_solutions2 = solutions2.len();
    }

    // failure case: start or end pt failed to drop to surface
    if num_solutions1 == 0 || num_solutions2 == 0 {
        return Err(SolidError::blend(
            OP_UV,
            BlendFailure::UvCurveEndOffSurface {
                start: num_solutions1 == 0,
                end: num_solutions2 == 0,
                tolerance: approx_tol,
            },
        ));
    }

    // Discard solutions within dParamTol of the first one
    let dedupe = |sols: &mut SolutionArray<UvSurfaceHandle>, n: usize| -> Result<usize> {
        if n > 1 {
            let uv1 = solution_uv(sols, 0, OP_UV)?;
            for ii in (1..n).rev() {
                let uv2 = solution_uv(sols, ii, OP_UV)?;
                let param_dist = uv1.distance(uv2);
                if param_dist < param_tol {
                    sols.remove_at(ii, 1);
                }
            }
            return Ok(sols.len());
        }
        Ok(n)
    };
    num_solutions1 = dedupe(&mut solutions1, num_solutions1)?;
    num_solutions2 = dedupe(&mut solutions2, num_solutions2)?;

    // Sort out dual(or multi-) solution cases
    let mut start_uv = solution_uv(&solutions1, 0, OP_UV)?;
    let mut end_uv = solution_uv(&solutions2, 0, OP_UV)?;
    let mut start_uv1 = Point2::UNDEF;
    let mut end_uv1 = Point2::UNDEF;

    // Not in the fork (#2403): an end on a degenerate side of the surface (a patch's pole,
    // one point for every value of the side's free parameter) drops to one solution
    // anywhere along that side, where the multi-solution rule below needs more than two
    // to know it. The curve is the iso-curve through the other end, as that rule makes
    // it: the end takes the other end's free parameter.
    if num_solutions1 == 1 && num_solutions2 == 1 {
        let on_side1 = degenerate_side_at(k, surface, &uv_domain, start_uv, start_pnt, approx_tol)?;
        let on_side2 = degenerate_side_at(k, surface, &uv_domain, end_uv, end_pnt, approx_tol)?;
        match (on_side1, on_side2) {
            (Some(side), None) => start_uv = side.through(end_uv),
            (None, Some(side)) => end_uv = side.through(start_uv),
            _ => {}
        }
    }

    // when start point still dropped to surface multiple times
    if num_solutions1 > 1 {
        start_uv1 = solution_uv(&solutions1, 1, OP_UV)?;
        if num_solutions1 == 2 && num_solutions2 == 1 {
            // Start point is on seam but end point is not: take the solution closest
            // to sEndUV
            let dist_to_end = start_uv.distance(end_uv);
            let dist_to_end1 = start_uv1.distance(end_uv);
            if dist_to_end1 < dist_to_end {
                start_uv = start_uv1;
            }
            num_solutions1 = 1;
        }
    }

    // when end point still dropped to surface multiple times
    if num_solutions2 > 1 {
        end_uv1 = solution_uv(&solutions2, 1, OP_UV)?;
        if num_solutions2 == 2 && num_solutions1 == 1 {
            // Could be seam or degenerate cases: take the solution closest to sStartUV
            let dist_to_start = end_uv.distance(start_uv);
            let dist_to_start1 = end_uv1.distance(start_uv);
            if dist_to_start1 < dist_to_start {
                end_uv = end_uv1;
            }
            num_solutions2 = 1;
        }
    }

    // remember when Start and End Pts both dropped to a seam
    let make_two_curves = num_solutions1 >= 2 && num_solutions2 >= 2;

    // when start point still dropped to surface multiple times
    if num_solutions1 > 2 {
        if num_solutions2 > 2 {
            // Should not happen!!
            return Err(ends_unresolved(num_solutions1, num_solutions2));
        }

        // Start point lies on degenerate edge
        if (start_uv.x - start_uv1.x).abs() < param_tol {
            start_uv.y = end_uv.y;
            if num_solutions2 == 2 {
                start_uv1.y = end_uv1.y;
            }
        } else if (start_uv.y - start_uv1.y).abs() < param_tol {
            start_uv.x = end_uv.x;
            if num_solutions2 == 2 {
                start_uv1.x = end_uv1.x;
            }
        } else {
            return Err(ends_unresolved(num_solutions1, num_solutions2));
        }
    }

    // when end point still dropped to surface multiple times
    if num_solutions2 > 2 {
        // End point lies on degenerate edge
        if (end_uv.x - end_uv1.x).abs() < param_tol {
            end_uv.y = start_uv.y;
            if num_solutions1 == 2 {
                end_uv1.y = start_uv1.y;
            }
        } else if (end_uv.y - end_uv1.y).abs() < param_tol {
            end_uv.x = start_uv.x;
            if num_solutions1 == 2 {
                end_uv1.x = start_uv1.x;
            }
        } else {
            return Err(ends_unresolved(num_solutions1, num_solutions2));
        }
    }

    // when needed - Make the second UVcurve
    if make_two_curves {
        let start_uv1 = cleanup_noise(start_uv1);
        let end_uv1 = cleanup_noise(end_uv1);
        out.uv_curve2 = Some(BSplineCurve::create_line_segment(
            2,
            p3(start_uv1),
            p3(end_uv1),
        )?);
    }

    // Now, make the first curve
    let start_uv = cleanup_noise(start_uv);
    let end_uv = cleanup_noise(end_uv);

    // when curve is not an iso-curve, intersect the surface with a plane
    let domain_size = uv_domain.size();
    if (start_uv.x - end_uv.x).abs() > 0.01 * domain_size.x
        && (start_uv.y - end_uv.y).abs() > 0.01 * domain_size.y
    {
        // SM_CAST_PTR(SmBSplineSurface, pSurface): upstream dereferences the result
        if b_spline_net(surface).is_none() {
            let face = on_face.as_ref().map(|(_, f)| *f);
            let failure = Failure::new(
                OP_UV,
                BlendFailure::UvCurveSurfaceNotBSpline {
                    face,
                    kind: surface.type_string(),
                },
            );
            return Err(match face {
                Some(face) => failure.at(EntityRef::Face(face)),
                None => failure,
            }
            .into_error());
        }

        // StartPt to EndPt 3d unit-vector
        let mut dir = (end_pnt - start_pnt).unitize()?;

        // pick a likely ZAxis: the cone's axis, else a fillet vertex use's curve
        // derivative
        let mut z_axis = Vec3::new(0.0, 0.0, 0.0);
        if let SurfaceKind::Cone(cone) = surface.kind() {
            // SmCone_TYPE (an SmCylinder too): the cone's axis
            z_axis = cone.rev().position().z_axis();
        } else {
            // pick a likely ZAxis from the Start/EndFV->GetTsectPnt().CrvDeriv() values
            // find a fillet vertexuse to target
            let mut vu = None;
            let geoms = fb.vertex_fillet_geoms(start_fv)?;
            if geoms.len() == 1 {
                vu = fb.vu_at_rail_end(start_fv, geoms.first().copied(), None, None)?;
            } else {
                let geoms = fb.vertex_fillet_geoms(end_fv)?;
                if geoms.len() == 1 {
                    vu = fb.vu_at_rail_end(end_fv, geoms.first().copied(), None, None)?;
                }
            }
            // when a vertexuse was found
            if let Some(vu) = vu {
                z_axis = fb.fillet_vertexuse(vu)?.tsect_pnt().crv_deriv();
            }
        }

        // when ZAxis was set
        if z_axis.length() > EFF_ZERO_SQRT {
            // build plane with planeNormal perp to sDir in [sZAxis sDir] plane
            let view_vec = z_axis.cross(dir);
            let plane_normal = view_vec.cross(dir).unitize()?;
            let mut plane = Plane::from_normal(end_pnt, plane_normal);
            plane.make_nurb()?;
            let plane = Surface::from(plane);

            // surface intersection initial tol
            let mut ssi_tol = crate::geom::tol::approx_tol3d(k, approx_tol / 100.0);

            // iter a sequence of SrfSrfIntersections with increasing tolerance
            for _ in 0..4 {
                match srf_srf_intersection(
                    k,
                    fb,
                    Some(start_fv),
                    Some(end_fv),
                    Some(&mut dir),
                    None,
                    surface,
                    &plane,
                    ssi_tol,
                    30.0 * PI / 180.0,
                    false,
                    None,
                ) {
                    Ok(r) => {
                        // when Srf/Srf worked - don't need 2nd UVcurve
                        // (the out-params are `SmBSplineCurve *&`: the derived objects
                        // themselves, not copies of their B-spline bases)
                        out.curve_3d = r.curve_3d.filter(|c| as_b_spline(c).is_some());
                        out.uv_curve1 = r.uv_curve1.filter(|c| as_b_spline(c).is_some());
                        return Ok(out);
                    }
                    Err(e) if e.is_unported_or_cancelled() => return Err(e),
                    Err(_) => {}
                }

                // try Srf/Srf xsect again with larger tolerance
                ssi_tol = crate::geom::tol::tol_mul(ssi_tol, 10.0);
            }
        }
    }

    // Otherwise, make an iso-curve
    out.uv_curve1 = Some(Curve::from(BSplineCurve::create_line_segment(
        2,
        p3(start_uv),
        p3(end_uv),
    )?));

    // all done
    Ok(out)
}

/// A degenerate side of a surface's domain: its constant parameter and value.
#[derive(Clone, Copy)]
struct DegenerateSide {
    iso: SurfParamType,
    value: f64,
}

impl DegenerateSide {
    /// The point of the side on the iso-curve through `other` (the side's free
    /// parameter is `other`'s).
    const fn through(self, other: Point2) -> Point2 {
        match self.iso {
            SurfParamType::U => Point2::new(self.value, other.y),
            _ => Point2::new(other.x, self.value),
        }
    }
}

/// Not in the fork (#2403): the side of `uv_domain` a drop of `point` at `uv` lies
/// on when that side is degenerate, one point (`IsDegenerate` of its iso-curve at
/// `SM_EFF_ZERO`, as `CreateNaturalUVTrimCurves` culls such sides): the side's point
/// under `uv` is `point` to `tol`. `None` off every degenerate side.
fn degenerate_side_at(
    k: &Kernel,
    surface: &Surface,
    uv_domain: &Extent2d,
    uv: Point2,
    point: Point3,
    tol: f64,
) -> Result<Option<DegenerateSide>> {
    let (lo, hi) = (uv_domain.min(), uv_domain.max());
    for (iso, value) in [
        (SurfParamType::U, lo.x),
        (SurfParamType::U, hi.x),
        (SurfParamType::V, lo.y),
        (SurfParamType::V, hi.y),
    ] {
        let side = DegenerateSide { iso, value };
        if surface.evaluate_point(k, side.through(uv))?.distance(point) > tol {
            continue;
        }
        let curve = surface
            .create_iso_parametric_curve(k, iso, value, 0.0, Some(uv_domain), false)?
            .curve;
        if curve.is_degenerate(k, EFF_ZERO, None)? {
            return Ok(Some(side));
        }
    }
    Ok(None)
}

// ---- vertex calculators -----------------------------------------------------------------

/// Which end of the 3D intersection curve `curve` the reference point is nearer to, and
/// the end points (`EvaluatePoint` at the natural interval's ends).
fn curve_ends(k: &Kernel, curve: &Curve) -> Result<(Extent1d, Point3, Point3)> {
    let ivl = curve.natural_interval();
    let pnt1 = curve.evaluate_point(k, ivl.min())?;
    let pnt2 = curve.evaluate_point(k, ivl.max())?;
    Ok((ivl, pnt1, pnt2))
}

impl FilletBrep {
    /// The fillet surface and solver tolerances of the fillet geometry of edge use
    /// `eu` (`missing` names the link when `eu` spans no fillet geometry: a
    /// cross-section's or another fillet edge's).
    #[track_caller]
    fn eu_geom_solver_with<'s>(
        &self,
        eu: EdgeuseId,
        context: &'s impl super::p5::FilletEdgeContext,
        op: &'static str,
        missing: FilletSetUp,
    ) -> Result<(FilletGeomId, &'s FilletSolver)> {
        let Some(g) = self.fillet_edgeuse(eu)?.fillet_geom() else {
            return Err(set_up(op, missing, Some(EntityRef::Edgeuse(eu))));
        };
        let Some(sid) = self.geom(g)?.fillet_solver() else {
            return Err(no_solver(op, g));
        };
        Ok((g, context.solver(sid)?))
    }

    /// The primary edge use of `e` and its mate.
    #[track_caller]
    fn prim_and_mate(&self, e: EdgeId, op: &'static str) -> Result<(EdgeuseId, EdgeuseId)> {
        match self.prim_and_opt_mate(e, op)? {
            (prim, Some(mate)) => Ok((prim, mate)),
            (_, None) => Err(set_up(
                op,
                FilletSetUp::PrimaryEdgeUseMate,
                Some(EntityRef::Edge(e)),
            )),
        }
    }

    /// The primary edge use of `e` and its mate, if any (`pPrimEU->GetMate()`, which
    /// upstream fetches early but dereferences only where it is used).
    #[track_caller]
    fn prim_and_opt_mate(
        &self,
        e: EdgeId,
        op: &'static str,
    ) -> Result<(EdgeuseId, Option<EdgeuseId>)> {
        let Some(prim) = primary_edgeuse(self.brep(), e)? else {
            return Err(set_up(
                op,
                FilletSetUp::PrimaryEdgeUse,
                Some(EntityRef::Edge(e)),
            ));
        };
        Ok((prim, eu1::mate(self.brep(), prim)?))
    }

    /// The primary edge use of fillet edge `e` (`GetPrimaryEdgeuse()`, dereferenced).
    #[track_caller]
    fn primary_of(&self, e: EdgeId, op: &'static str) -> Result<EdgeuseId> {
        primary_edgeuse(self.brep(), e)?
            .ok_or_else(|| set_up(op, FilletSetUp::PrimaryEdgeUse, Some(EntityRef::Edge(e))))
    }

    /// Stores a new intersection curve on fillet edge `e` (`SetCurve(curve, TRUE)`,
    /// `SetOwner`, `SetInterval(ivl)`) and its UV curves on the primary edge use and
    /// its mate (`SetUVCurve`).
    #[allow(clippy::too_many_arguments)]
    fn set_edge_curves(
        &mut self,
        e: EdgeId,
        prim: EdgeuseId,
        mate: EdgeuseId,
        curve: Curve,
        ivl: &Extent1d,
        uv_curve1: Option<Curve>,
        uv_curve2: Option<Curve>,
    ) -> Result<()> {
        set_edge_curve(self.brep_mut(), e, Some(curve))?;
        edge::set_interval(self.brep_mut(), e, ivl)?;
        self.set_edgeuse_uv_curve(prim, uv_curve1, true, true)?;
        self.set_edgeuse_uv_curve(mate, uv_curve2, true, true)?;
        Ok(())
    }

    /// `sm_CalcNx2VertFilletXFillet`: the vertex at the end of the intersection curve
    /// of the two fillets of an Nx2 concave corner whose cross-section edges meet at
    /// `fv`, the end nearer the processed vertex across the fillet/side-face edge.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when that other vertex is not processed, or when the intersection
    /// fails (also for a missing fillet surface, which `sm_SrfSrfIntersection` rejects;
    /// the vertex is then `SM_FIL_SURF_INT_FAILURE`); `SM_ERR_NULL_POINTER` for a
    /// cross-section edge without fillet geometry or solver, or a vertex without
    /// corner; [`SolidError::Unported`] from the intersection (`sm_SrfSrfIntersection`).
    pub fn calc_nx2_vert_fillet_x_fillet(
        &mut self,
        k: &Kernel,
        fv: VertexId,
        solvers: &[FilletSolver],
    ) -> Result<()> {
        self.calc_nx2_vert_fillet_x_fillet_with_context(
            k,
            fv,
            &super::p5::IndexedEdgeContext {
                solvers,
                corners: CornerLookup::default(),
                op: OP_NX2,
            },
        )
    }

    /// [`Self::calc_nx2_vert_fillet_x_fillet`] resolving the solvers through `context`.
    ///
    /// # Errors
    /// As [`Self::calc_nx2_vert_fillet_x_fillet`], with `context`'s resolution failures.
    pub fn calc_nx2_vert_fillet_x_fillet_with_context(
        &mut self,
        k: &Kernel,
        fv: VertexId,
        context: &impl super::p5::FilletEdgeContext,
    ) -> Result<()> {
        let mut ref_pnt = Point3::UNDEF;
        let mut approx_tol = BIG_DOUBLE;
        let mut ang_tol = BIG_DOUBLE;
        let mut fillet_surface1: Option<FilletGeomId> = None;
        let mut fillet_surface2: Option<FilletGeomId> = None;

        for e in vertex::edges(self.brep(), fv, None)? {
            match self.fillet_edge(e)?.fillet_edge_type() {
                FilletEdgeType::CrossSection => {
                    let prim = primary_edgeuse(self.brep(), e)?.ok_or_else(|| {
                        set_up(
                            OP_NX2,
                            FilletSetUp::PrimaryEdgeUse,
                            Some(EntityRef::Edge(e)),
                        )
                    })?;
                    let (g, fs) = self.eu_geom_solver_with(
                        prim,
                        context,
                        OP_NX2,
                        FilletSetUp::CrossSectionGeom,
                    )?;
                    approx_tol = smin(approx_tol, fs.approx_tol_3d);
                    ang_tol = smin(ang_tol, fs.ang_tol_rad);
                    if fillet_surface1.is_none() {
                        fillet_surface1 = Some(g);
                    } else {
                        fillet_surface2 = Some(g);
                    }
                }
                FilletEdgeType::FilletXSideFace => {
                    let other_fv =
                        edge::other_vertex(self.brep(), e, Some(fv))?.ok_or_else(|| {
                            set_up(OP_NX2, FilletSetUp::EdgeVertices, Some(EntityRef::Edge(e)))
                        })?;
                    if !self.fillet_vertex(other_fv)?.is_processed() {
                        return Err(not_processed(OP_NX2, other_fv));
                    }
                    ref_pnt = self.vertex_point(other_fv)?;
                }
                _ => {}
            }
        }

        // Do surface/surface intersection
        let ref_point = self.vertex_point(fv)?;
        // pFV->GetFilletCorner()->GetContext(): the kernel; the corner is dereferenced
        if self.fillet_vertex(fv)?.fillet_corner().is_none() {
            return Err(without_corner(OP_NX2, fv));
        }
        // GetFilletSurface() of each geometry; NULL when no cross-section edge was
        // seen or the geometry has no fillet surface yet
        let surface_of = |g: Option<FilletGeomId>| -> Result<Option<&Surface>> {
            Ok(match g {
                Some(g) => self.geom(g)?.fillet_surface(None)?,
                None => None,
            })
        };
        // (`None` when a surface is not there)
        let result = match (surface_of(fillet_surface1)?, surface_of(fillet_surface2)?) {
            (Some(s1), Some(s2)) => Some(srf_srf_intersection(
                k,
                self,
                None,
                None,
                None,
                None,
                s1,
                s2,
                approx_tol,
                ang_tol,
                false,
                Some(ref_point),
            )),
            _ => None,
        };
        let curve_3d = match result {
            Some(Ok(SrfSrfCurves {
                curve_3d: Some(c), ..
            })) => c,
            Some(Err(e)) if e.is_unported_or_cancelled() => return Err(e),
            failed => {
                self.fillet_vertex_mut(fv)?
                    .set_status(FilStatus::SurfIntFailure);
                return Err(match failed {
                    // sm_SrfSrfIntersection starts with NER(pSurf1); NER(pSurf2): a
                    // failed intersection for the caller (SM_ERR). Fewer than two
                    // cross-section edges, or a geometry not traced yet: the
                    // executive calls this only past both, so it is the caller's
                    // set-up
                    None => match (fillet_surface1, fillet_surface2) {
                        (Some(g1), Some(g2)) => {
                            let untraced = if self.geom(g1)?.fillet_surface(None)?.is_none() {
                                g1
                            } else {
                                g2
                            };
                            Failure::new(OP_NX2, BlendFailure::Solver(no_surface(untraced)))
                                .with_status(SM_ERR)
                                .into_error()
                        }
                        _ => set_up_failure(
                            OP_NX2,
                            FilletSetUp::VertexCrossSection,
                            Some(EntityRef::Vertex(fv)),
                        )
                        .with_status(SM_ERR)
                        .into_error(),
                    },
                    Some(failed) => corner_intersection_failed(OP_NX2, failed.err()),
                });
            }
        };

        let (_, pnt1, pnt2) = curve_ends(k, &curve_3d)?;
        let mut vert_geom = pnt1;
        if ref_pnt.distance(pnt1) > ref_pnt.distance(pnt2) {
            vert_geom = pnt2;
        }

        set_vertex_point(self.brep_mut(), fv, vert_geom)?;
        self.fillet_vertex_mut(fv)?.set_status(FilStatus::Processed);
        Ok(())
    }

    /// `SmFilletVertex::CalcFilletInt2Fillets`: the vertex where three fillets meet at
    /// a corner, as the common point of the intersection curves of the fillet pairs
    /// across its fillet/fillet edges (which get those curves).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without corner, fillet geometry or solver; `SM_ERR` when
    /// a neighbour vertex is not processed or the curves do not meet once;
    /// those of [`CornerLookup::corner`] (`corners` resolves the vertex's corner);
    /// [`SolidError::Unported`] from the intersection (`sm_SrfSrfIntersection`).
    pub fn calc_fillet_int_2_fillets(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        solvers: &[FilletSolver],
        target: &Brep,
    ) -> Result<()> {
        self.calc_fillet_int_2_fillets_with_context(
            k,
            v,
            corners,
            &super::p5::IndexedEdgeContext {
                solvers,
                corners: CornerLookup::default(),
                op: OP_INT2,
            },
            target,
        )
    }

    /// [`Self::calc_fillet_int_2_fillets`] resolving the geometries' solvers through
    /// `context` (a live executive's retained solvers).
    ///
    /// # Errors
    /// As [`Self::calc_fillet_int_2_fillets`], with `context`'s resolution failures.
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn calc_fillet_int_2_fillets_with_context(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        context: &impl super::p5::FilletEdgeContext,
        target: &Brep,
    ) -> Result<()> {
        let corner = self
            .fillet_vertex(v)?
            .fillet_corner()
            .ok_or_else(|| without_corner(OP_INT2, v))?;
        let filleted_vertex = corners.corner(corner)?.filleted_vertex();

        // Intersection curves of fillet #1 & #2 and of #2 & #3 (their edges)
        let mut curve1: Option<EdgeId> = None;
        let mut curve2: Option<EdgeId> = None;
        let mut min_tol = BIG_DOUBLE;

        for curr_edge in vertex::edges(self.brep(), v, None)? {
            if self.fillet_edge(curr_edge)?.fillet_edge_type() != FilletEdgeType::FilletXFillet {
                continue;
            }

            let other_fv =
                edge::other_vertex(self.brep(), curr_edge, Some(v))?.ok_or_else(|| {
                    set_up(
                        OP_INT2,
                        FilletSetUp::EdgeVertices,
                        Some(EntityRef::Edge(curr_edge)),
                    )
                })?;
            if !self.fillet_vertex(other_fv)?.is_processed() {
                return Err(not_processed(OP_INT2, other_fv));
            }
            let other_pnt = self.vertex_point(other_fv)?;

            // Do surface/surface intersection
            let (prim, mate) = self.prim_and_mate(curr_edge, OP_INT2)?;
            let g1 = self.fillet_edgeuse(prim)?.fillet_geom().ok_or_else(|| {
                set_up(
                    OP_INT2,
                    FilletSetUp::EdgeUseGeom,
                    Some(EntityRef::Edgeuse(prim)),
                )
            })?;
            let g2 = self.fillet_edgeuse(mate)?.fillet_geom().ok_or_else(|| {
                set_up(
                    OP_INT2,
                    FilletSetUp::EdgeUseGeom,
                    Some(EntityRef::Edgeuse(mate)),
                )
            })?;
            let (Some(fillet_surface1), Some(fillet_surface2)) = (
                self.geom(g1)?.fillet_surface(None)?,
                self.geom(g2)?.fillet_surface(None)?,
            ) else {
                return Ok(()); // Fillet surfaces have not yet been computed
            };
            // The solvers are dereferenced only past the surface test (upstream
            // fetches the pointers before it but does not use them)
            let (_, fs1) =
                self.eu_geom_solver_with(prim, context, OP_INT2, FilletSetUp::EdgeUseGeom)?;
            let (_, fs2) =
                self.eu_geom_solver_with(mate, context, OP_INT2, FilletSetUp::EdgeUseGeom)?;

            let approx_tol = smin(fs1.approx_tol_3d, fs2.approx_tol_3d);
            let ang_tol = smin(fs1.ang_tol_rad, fs2.ang_tol_rad);
            if approx_tol < min_tol {
                min_tol = approx_tol;
            }
            let filleted_pnt = target
                .vertex(
                    filleted_vertex
                        .ok_or_else(|| set_up(OP_INT2, FilletSetUp::CornerVertex, None))?,
                )?
                .point;
            let mut dir = filleted_pnt - other_pnt;
            let skip_two_pnts_intersection = true;

            let result = srf_srf_intersection(
                k,
                self,
                Some(other_fv),
                None,
                Some(&mut dir),
                None,
                fillet_surface1,
                fillet_surface2,
                approx_tol,
                ang_tol,
                skip_two_pnts_intersection,
                None,
            );
            let (mut new_3d_curve, uv_curve1, uv_curve2) = match result {
                Ok(SrfSrfCurves {
                    curve_3d: Some(c),
                    uv_curve1,
                    uv_curve2,
                }) => (c, uv_curve1, uv_curve2),
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                failed => {
                    self.fillet_vertex_mut(v)?
                        .set_status(FilStatus::SurfIntFailure);
                    self.fillet_edge_mut(curr_edge)?
                        .set_status(FilStatus::SurfIntFailure);
                    return Err(corner_intersection_failed(OP_INT2, failed.err()));
                }
            };
            let (mut uv_curve1, mut uv_curve2) = (uv_curve1, uv_curve2);

            // Determine whether the curves need to be reversed (before they are stored:
            // upstream reverses the stored objects in place)
            let crv_ivl = new_3d_curve.natural_interval();
            let (mut ivl, pnt1, pnt2) = curve_ends(k, &new_3d_curve)?;
            let near_start = other_pnt.distance(pnt1) < other_pnt.distance(pnt2);
            let reverse_curves = if edge::vertex(self.brep(), curr_edge)? == Some(v) {
                near_start
            } else {
                !near_start
            };
            if reverse_curves {
                // the UV curves are dereferenced only here upstream
                ivl = new_3d_curve.reverse_parameterization(k, &ivl)?;
                ivl = uv_curve1
                    .as_mut()
                    .ok_or_else(|| broken_link(OP_INT2, SSI_NO_UV_CURVE))?
                    .reverse_parameterization(k, &ivl)?;
                let _ = uv_curve2
                    .as_mut()
                    .ok_or_else(|| broken_link(OP_INT2, SSI_NO_UV_CURVE))?
                    .reverse_parameterization(k, &ivl)?;
            }
            self.set_edge_curves(
                curr_edge,
                prim,
                mate,
                new_3d_curve,
                &crv_ivl,
                uv_curve1,
                uv_curve2,
            )?;

            if curve1.is_none() {
                curve1 = Some(curr_edge);
            } else if curve2.is_none() {
                curve2 = Some(curr_edge);
            }
        }

        // Now find the common intersection of all the curves.
        let curve_of = |e: Option<EdgeId>| -> Result<&Curve> {
            // fewer than two fillet/fillet edges at the vertex: the executive calls this
            // only at a corner of three fillets, which has two
            let e = e.ok_or_else(|| {
                set_up(
                    OP_INT2,
                    FilletSetUp::VertexFilletFilletEdges,
                    Some(EntityRef::Vertex(v)),
                )
            })?;
            self.brep()
                .edge(e)?
                .curve
                .as_ref()
                .ok_or_else(|| broken_link(OP_INT2, INT2_CURVE_UNSET))
        };
        let c1 = curve_of(curve1)?;
        let c2 = curve_of(curve2)?;
        // the solutions name the two curves (`this` and `rCurve2`) and their analytic
        // replacements, as upstream's do: `IdenticalSolutions` merges near-duplicates
        // within `1e-4` of the curve's interval before the `!= 1` count below
        let (ivl1, ivl2) = (c1.natural_interval(), c2.natural_interval());
        let handle = |id: u8, ivl| Some(CurveObjectHandle { id, ivl });
        let solutions = c1.global_curve_intersect(
            k,
            &ivl1,
            c2,
            &ivl2,
            min_tol,
            &crate::curve::curve::p8::CurveIntersectOptions {
                skip_global_coincidence_check: false,
                handles: [handle(0, ivl1), handle(1, ivl2)],
                analytic_handles: [handle(2, ivl1), handle(3, ivl2)],
            },
        )?;
        // (one crossing exactly: `get(0)` is `None` only for none)
        let sol = match solutions.get(0) {
            Some(sol) if solutions.len() == 1 => sol,
            Some(_) => {
                return Err(SolidError::blend(
                    OP_INT2,
                    BlendFailure::CornerCurvesMeetMoreThanOnce {
                        crossings: solutions.len(),
                    },
                ))
            }
            None => {
                return Err(SolidError::blend(
                    OP_INT2,
                    BlendFailure::CornerCurvesDoNotMeet,
                ))
            }
        };
        let vert_geom = c1.evaluate_point(k, sol.start.parameters[0])?;
        set_vertex_point(self.brep_mut(), v, vert_geom)?;
        self.fillet_vertex_mut(v)?.set_status(FilStatus::Processed);
        Ok(())
    }

    /// `SmFilletVertex::CalcRailIntExtendedEdgeuse`: the vertex of a 3x2 mixed corner,
    /// at the end of the intersection of its cross section's fillet surface with the
    /// side face's extended surface that lies on the face the vertex is on. Every other
    /// vertex (one with no corner, or with a corner of another type) is the rail's
    /// crossing with the end face's surface: it returns
    /// [`Self::calc_rail_int_end_surface`] (the fork's, WR-697). A cross section whose
    /// fillet surface is not traced yet is left for later (`Ok`, unchanged).
    ///
    /// # Errors
    ///
    /// Those of [`Self::calc_rail_int_end_surface`] for a vertex that is not a 3x2 mixed
    /// corner's. For a 3x2 mixed corner (status `SM_ERR_NULL_POINTER` for each missing
    /// link, class [`crate::error::ErrorKind::InvalidInput`]): a
    /// [`BlendFailure::FilletSetUpInvalid`] for a missing cross-section edge, fillet
    /// geometry, classification or extended surface (and a missing primary edge use),
    /// [`crate::error::blend::SolverFailure::FilletGeomWithoutSolver`] for a geometry
    /// without its solver, [`BlendFailure::FilletSetUpInvalid`] or
    /// [`crate::error::blend::SolverFailure::EdgeUseWithoutFace`] for the body's side edge
    /// use without a radial use or a face, and
    /// [`crate::error::blend::SolverFailure::SolverEdgeUseMissing`] for a solver without
    /// its edge uses;
    /// [`SolidError::InvalidInput`] when the vertex is classified to an entity
    /// that is not an edge use; [`BlendFailure::MateFaceSurfaceNotBSpline`] when the
    /// extended surface is not of a B-spline class;
    /// [`BlendFailure::CornerSurfacesNotIntersected`] with the
    /// intersection's error as its cause when the intersection fails (the vertex and
    /// edge are marked `SurfIntFailure`); [`BlendFailure::FaceHasNoSurface`] when
    /// the face the vertex is on has no surface; [`BlendFailure::IntersectionEndsOffFace`]
    /// when neither end of the curve lies on that face; those of
    /// [`CornerLookup::corner`] and [`CornerLookup::extended_surface`] (`corners`
    /// resolves the vertex's corner and holds the executive's extended surfaces);
    /// [`SolidError::Unported`] or a cancellation from the intersection
    /// (`sm_SrfSrfIntersection`), passed on; and those of the curve's evaluations and of
    /// the on-face's fast-drop trim test (`SmFace::PointClassify` on `target`, which may
    /// make the face's trimmed surface cache).
    pub fn calc_rail_int_extended_edgeuse(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        solvers: &[FilletSolver],
        target: &mut Brep,
    ) -> Result<()> {
        let context = super::p5::IndexedEdgeContext {
            solvers,
            corners,
            op: OP_EXT,
        };
        self.calc_rail_int_extended_edgeuse_with_context(k, v, corners, &context, target)
    }

    /// [`Self::calc_rail_int_extended_edgeuse`] resolving the geometry's solver and
    /// the side face's extended surface through the caller's live objects.
    ///
    /// # Errors
    /// As [`Self::calc_rail_int_extended_edgeuse`]; those of `context`.
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn calc_rail_int_extended_edgeuse_with_context(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        context: &impl super::p5::FilletEdgeContext,
        target: &mut Brep,
    ) -> Result<()> {
        // The fork's (WR-697, 3e26f76): at an End's end rib the rail's end is its crossing
        // with the end face's surface (CalcRailIntEndSurface); the end rib is then the
        // intersection between the two rail ends (CalcFilletIntSideFace). Only a 3x2 mixed
        // corner (`b3x2MixedCornerCase`), which has its corner, goes on.
        let corner = match self.fillet_vertex(v)?.fillet_corner() {
            Some(c) if corners.corner(c)?.corner_type() == FilletCornerType::ThreeByTwoMixed => c,
            _ => return self.calc_rail_int_end_surface(k, v, corners, context, target),
        };

        // Find adjacent fillet edge which was classified (only the 3x2 mixed corner
        // reaches here: its cross section)
        let mut curr_edge = None;
        for e in vertex::edges(self.brep(), v, None)? {
            if self.fillet_edge(e)?.fillet_edge_type() == FilletEdgeType::CrossSection {
                curr_edge = Some(e);
                break;
            }
        }
        let curr_edge = curr_edge.ok_or_else(|| {
            set_up(
                OP_EXT,
                FilletSetUp::VertexCrossSection,
                Some(EntityRef::Vertex(v)),
            )
        })?;

        // get FilletSurface stored in filletEdge->PrimaryEU->FilletGeom->FilletSurface
        // (pMateEU is dereferenced only by the SM_FE_FILLET_X_SIDE_FACE tail, which the
        // mixed corner never reaches)
        let (prim, _) = self.prim_and_opt_mate(curr_edge, OP_EXT)?;
        let fillet_geom = self.fillet_edgeuse(prim)?.fillet_geom().ok_or_else(|| {
            set_up(
                OP_EXT,
                FilletSetUp::CrossSectionGeom,
                Some(EntityRef::Edgeuse(prim)),
            )
        })?;
        if !self.geom(fillet_geom)?.has_fillet_surface() {
            // Need fillet surface, will process it later
            return Ok(());
        }
        // pFilletGeom->GetFilletSolver(), dereferenced where upstream does
        let sid = self.geom(fillet_geom)?.fillet_solver();
        let fil_solver = || -> Result<&FilletSolver> {
            context.solver(sid.ok_or_else(|| no_solver(OP_EXT, fillet_geom))?)
        };

        // get OtherFaceSurface: will intersect fillet with a side-face of the original
        // brep
        let side_eu = match self.fillet_vertex(v)?.point_class_object() {
            Some(EntityRef::Edgeuse(eu)) => eu,
            None => {
                return Err(set_up(
                    OP_EXT,
                    FilletSetUp::PointClassEdgeUse,
                    Some(EntityRef::Vertex(v)),
                ))
            }
            Some(_) => {
                return Err(SolidError::invalid_input(
                    OP_EXT,
                    "classification object is not an edge use",
                ))
            }
        };
        let mut sf = eu1::face(target, side_eu)?;
        let mut sf_src = side_eu;
        let radial = eu1::radial(target, side_eu)?.ok_or_else(|| {
            set_up(
                OP_EXT,
                FilletSetUp::RadialEdgeUse,
                Some(EntityRef::Edgeuse(side_eu)),
            )
        })?;
        let mut of = eu1::face(target, radial)?;
        let mut of_src = radial;
        for kk in 0..2 {
            let eu = fil_solver()?.edgeuse(kk)?.ok_or_else(|| {
                SolidError::blend_solver(OP_EXT, SolverFailure::SolverEdgeUseMissing { index: kk })
            })?;
            if eu1::face(target, eu)? == sf {
                core::mem::swap(&mut sf, &mut of);
                core::mem::swap(&mut sf_src, &mut of_src);
                break;
            }
        }
        let on_face = of;
        // The error names the edge use whose face is absent (after the swap, the radial).
        let side_face = sf.ok_or_else(|| eu_without_face(OP_EXT, sf_src))?;

        // get face->extendedSurface
        let surface2 = context
            .extended_surface(corners.corner(corner)?, side_face)?
            .ok_or_else(|| set_up(OP_EXT, FilletSetUp::CornerExtendedSurface, None))?;
        if b_spline_net(surface2).is_none() {
            return Err(Failure::new(
                OP_EXT,
                BlendFailure::MateFaceSurfaceNotBSpline {
                    face: side_face,
                    kind: surface2.type_string(),
                },
            )
            .at(EntityRef::Face(side_face))
            .into_error());
        }

        // locals surface/surface intersection
        let approx_tol = fil_solver()?.approx_tol_3d;
        let ang_tol = fil_solver()?.ang_tol_rad;
        let fillet_surface = self
            .geom(fillet_geom)?
            .fillet_surface(None)?
            .ok_or_else(|| broken_link(OP_EXT, GEOM_SURFACE_GONE))?;

        let result = srf_srf_intersection(
            k,
            self,
            None,
            None,
            None,
            None,
            fillet_surface,
            surface2,
            approx_tol,
            ang_tol,
            false,
            None,
        );
        let new_3d_curve = match result {
            Ok(SrfSrfCurves {
                curve_3d: Some(c), ..
            }) => c,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            failed => {
                self.fillet_vertex_mut(v)?
                    .set_status(FilStatus::SurfIntFailure);
                self.fillet_edge_mut(curr_edge)?
                    .set_status(FilStatus::SurfIntFailure);
                return Err(corner_intersection_failed(OP_EXT, failed.err()));
            }
        };

        // get surface/surface xSectCurve->endPoints
        let (_, pnt1, pnt2) = curve_ends(k, &new_3d_curve)?;

        // Find which end point lies on the pOnFace (the curves are dropped)
        let on_face = on_face.ok_or_else(|| eu_without_face(OP_EXT, radial))?;
        // pOnFace->GetSurface(): the face owns it, so the fast drop's trim test
        // classifies on the face (`pFace->PointClassify`); the solve reads a copy
        // sharing the surface's cache slot
        let surf1 = target
            .face(on_face)?
            .surface
            .as_ref()
            .ok_or_else(|| face_without_surface(OP_EXT, on_face))?
            .clone_for_evaluation()?;
        let tol = fil_solver()?.approx_tol_3d;
        let domain = surf1.natural_uv_domain_with_nurb(k)?;
        for pnt in [pnt1, pnt2] {
            let solutions = crate::intersect::point_class::live_owned_surface_global_point_solve(
                k,
                &surf1,
                target,
                on_face,
                &domain,
                SolverOperation::Intersect,
                pnt,
                zone_tol3d(k, 10.0 * tol),
                SolutionRequested::All,
            )?;
            if !solutions.is_empty() {
                set_vertex_point(self.brep_mut(), v, pnt)?;
                self.fillet_vertex_mut(v)?.set_status(FilStatus::Processed);
                return Ok(());
            }
        }
        Err(SolidError::blend(
            OP_EXT,
            BlendFailure::IntersectionEndsOffFace { face: on_face },
        ))
    }

    /// The fork's `SmFilletVertex::CalcRailIntEndSurface` (`SmBlendCornerEnd.cpp`,
    /// 3e26f76; WR-697): the rail's end at a mixed-convexity End
    /// (`SM_FV_RAIL_X_EXTENDED_EDGEUSE` of an end rib, `SM_FE_FILLET_X_SIDE_FACE`), the rail
    /// curve's crossing with the end face's surface continued (the corner's extended
    /// surface of the end face, else the face's own), the crossing nearest the corner's
    /// vertex. Before the blend is traced (no fillet surface or rail curve yet) the vertex
    /// is left unprocessed.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without corner, end rib, rail, primary edge use, fillet
    /// geometry, end face, its surface, solver or filleted vertex; `SM_ERR` after setting
    /// `SM_FIL_SURF_INT_FAILURE` when the rail does not meet the surface; those of the
    /// intersection and evaluations.
    pub fn calc_rail_int_end_surface(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        context: &(impl super::p5::FilletEdgeContext + ?Sized),
        target: &Brep,
    ) -> Result<()> {
        const OP: &str = "blend::fillet_geom::p4::FilletBrep::calc_rail_int_end_surface";
        let corner_id = self
            .fillet_vertex(v)?
            .fillet_corner()
            .ok_or_else(|| without_corner(OP, v))?;
        let corner = corners.corner(corner_id)?;

        // the end rib and this vertex's rail
        let mut rib = None;
        let mut rail = None;
        for e in vertex::edges(self.brep(), v, None)? {
            let edge_type = self.fillet_edge(e)?.fillet_edge_type();
            if edge_type == FilletEdgeType::FilletXSideFace && rib.is_none() {
                rib = Some(e);
            } else if edge_type == FilletEdgeType::Rail && rail.is_none() {
                rail = Some(e);
            }
        }
        let rib =
            rib.ok_or_else(|| set_up(OP, FilletSetUp::VertexEndRib, Some(EntityRef::Vertex(v))))?;
        let rail =
            rail.ok_or_else(|| set_up(OP, FilletSetUp::VertexRail, Some(EntityRef::Vertex(v))))?;
        let rib_eu = primary_edgeuse(self.brep(), rib)?
            .ok_or_else(|| set_up(OP, FilletSetUp::PrimaryEdgeUse, Some(EntityRef::Edge(rib))))?;
        let fillet_geom = self.fillet_edgeuse(rib_eu)?.fillet_geom().ok_or_else(|| {
            set_up(
                OP,
                FilletSetUp::EdgeUseGeom,
                Some(EntityRef::Edgeuse(rib_eu)),
            )
        })?;
        let rail_curve = edge::curve(self.brep(), rail)?.cloned();
        let (true, Some(rail_curve)) = (self.geom(fillet_geom)?.has_fillet_surface(), rail_curve)
        else {
            // not traced yet: the corner's second pass finds it
            return Ok(());
        };

        // an End at a smooth vertex: its plane (the fork's 41a39ec); else the end face
        // continued, or as it is
        let end_surface = if let Some(cap) = corner.end_cap.as_ref() {
            cap.clone()
        } else {
            let end_face = self
                .fillet_edge(rib)?
                .original_face()
                .ok_or_else(|| set_up(OP, FilletSetUp::OriginalFace, Some(EntityRef::Edge(rib))))?;
            match context.extended_surface(corner, end_face)? {
                Some(s) => s.clone(),
                None => target
                    .face(end_face)?
                    .surface
                    .as_ref()
                    .ok_or_else(|| face_without_surface(OP, end_face))?
                    .clone_for_evaluation()?,
            }
        };

        let sid = self
            .geom(fillet_geom)?
            .fillet_solver()
            .ok_or_else(|| no_solver(OP, fillet_geom))?;
        let tol = context.solver(sid)?.this_approx_tol_3d();

        let vert_pnt = vertex::point(
            target,
            corner
                .filleted_vertex()
                .ok_or_else(|| set_up(OP, FilletSetUp::CornerVertex, None))?,
        )?;
        let best = rail_crossing_nearest(k, &rail_curve, &end_surface, vert_pnt, tol)?;
        let Some((_, best)) = best else {
            self.fillet_vertex_mut(v)?
                .set_status(FilStatus::SurfIntFailure);
            // SER_MSG: the rail does not meet the end face's surface
            return Err(SolidError::blend(
                OP,
                BlendFailure::RailMissesEndSurface { tolerance: tol },
            ));
        };

        set_vertex_point(self.brep_mut(), v, best)?;
        self.fillet_vertex_mut(v)?.set_status(FilStatus::Processed);
        Ok(())
    }

    // ---- SmFilletGeom ----------------------------------------------------------------

    /// `SmFilletGeom::GetRailIndexOfVertex`: the index of the rail of geometry `g`
    /// with a rail vertex use on `fvtx` (via `GetRailIndexOfVertexuse`).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` for a `None` vertex or a missing rail vertex use (upstream
    /// dereferences it); `SM_ERR` from `GetRailIndexOfVertexuse` when no rail end
    /// matches.
    pub fn rail_index_of_vertex(&self, g: FilletGeomId, fvtx: Option<VertexId>) -> Result<usize> {
        let fvtx =
            fvtx.ok_or_else(|| SolidError::blend(OP_RAIL_IDX, BlendFailure::NoVertexGiven))?;

        // Find the FilletVertexuse of this FilletVertex
        let mut fvu_to_use = None;
        'outer: for i in 0..2 {
            for j in 0..2 {
                let fvu_tmp = self
                    .rail_vertexuse(g, i, j)?
                    .ok_or_else(|| set_up(OP_RAIL_IDX, FilletSetUp::RailEndVertexUse, None))?;
                if self.brep().vertexuse(fvu_tmp)?.vertex == Some(fvtx) {
                    fvu_to_use = Some(fvu_tmp);
                    break 'outer;
                }
            }
        }

        self.rail_index_of_vertexuse(g, fvu_to_use)
    }

    /// `SmFilletGeom::MakeFaceBrep`: makes the face of geometry `g` in `brep` (the
    /// fillet B-rep) on its fillet surface, bounded by the min-v rail(s), the side
    /// curves to the max-v rail(s), those reversed and the side curves back (gaps
    /// capped when the side curves do not connect); with UV curves
    /// (`MakeFaceWithCurves`) unless the geometry is a cliff side patch
    /// (`TrimSurfaceWithModelSpaceCurves`). Sets the geometry's capped flags. `env`
    /// carries the notifications the face makers send to `brep`'s entities. This
    /// indexed value API retains its historical surface-copy backend; shared surface
    /// binding requires [`Self::make_face_brep_with_solver`].
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without fillet surface, solver, rails, edge uses or UV
    /// curves; those of the evaluations, `JoinTwoFilletVertsByCurves`, the capping and
    /// the face makers.
    pub fn make_face_brep(
        &mut self,
        k: &mut Kernel,
        g: FilletGeomId,
        solvers: &[FilletSolver],
        brep: &mut Brep,
        env: &mut edge::NotifyEnv<'_>,
    ) -> Result<()> {
        self.make_face_brep_resolved(
            k,
            g,
            &|id, op| solver_of(solvers, id, op),
            brep,
            env,
            FaceSurfaceBackend::IndexedValue,
        )
    }

    /// Build a face using the actual retained solver without a copied base table.
    ///
    /// # Errors
    /// As [`Self::make_face_brep`], or a geometry naming a different solver.
    pub fn make_face_brep_with_solver(
        &mut self,
        k: &mut Kernel,
        g: FilletGeomId,
        solver_id: FilletSolverId,
        solver: &FilletSolver,
        brep: &mut Brep,
        env: &mut edge::NotifyEnv<'_>,
    ) -> Result<()> {
        self.make_face_brep_resolved(
            k,
            g,
            &|id, op| {
                if id == solver_id {
                    Ok(solver)
                } else {
                    Err(SolidError::invalid_input(op, "unresolved geometry solver"))
                }
            },
            brep,
            env,
            FaceSurfaceBackend::RetainedTransfer,
        )
    }

    #[allow(clippy::too_many_lines)] // one upstream routine
    fn make_face_brep_resolved<'s>(
        &mut self,
        k: &mut Kernel,
        g: FilletGeomId,
        resolve: &impl Fn(FilletSolverId, &'static str) -> Result<&'s FilletSolver>,
        brep: &mut Brep,
        env: &mut edge::NotifyEnv<'_>,
        surface_backend: FaceSurfaceBackend,
    ) -> Result<()> {
        // check state - must have a FilletSurface
        let uv_domain = self
            .geom(g)?
            .fillet_surface(None)?
            .ok_or_else(|| SolidError::blend_solver(OP_FACE, no_surface(g)))?
            .natural_uv_domain_with_nurb(k)?;
        let (geom_type, solver_id, rails, other_rails1, other_rails2, side_eus) = {
            let geom = self.geom(g)?;
            (
                geom.fillet_geom_type(),
                geom.fillet_solver(),
                geom.rails,
                geom.other_rails1.clone(),
                geom.other_rails2.clone(),
                geom.side_eus.clone(),
            )
        };

        // make 1st loop edge the MinV rail curve
        let mut start_rail_indx = 0usize;
        let mut use_uv_curves = false;

        if geom_type != FilletGeomType::CliffSidePatch {
            // UVTrimCurves are available - work in cheaper UVSpace
            use_uv_curves = true;

            // See which rail is at low-v.  Check the midpoint of each.
            let mut test_uv = [Point3::UNDEF; 2];
            for (ii, slot) in test_uv.iter_mut().enumerate() {
                let this_rail = rails
                    .get(ii)
                    .copied()
                    .flatten()
                    .ok_or_else(|| no_rail(OP_FACE, g, ii))?;
                let prim = self.primary_of(this_rail, OP_FACE)?;
                let uv_curve = eu1::uv_trim_curve(self.brep(), prim)?.0.ok_or_else(|| {
                    set_up(
                        OP_FACE,
                        FilletSetUp::UvTrimCurve,
                        Some(EntityRef::Edgeuse(prim)),
                    )
                })?;
                let t = self.brep().edge(this_rail)?.interval.evaluate(0.5);
                *slot = uv_curve.evaluate_point(k, t)?;
            }
            // (sTestUV[0].y < sTestUV[1].y) ? 0 : 1
            start_rail_indx = usize::from(test_uv[0].y >= test_uv[1].y);
        }

        // pick 2d or 3d tolerance depending on bUseUVCurve
        let sid = solver_id.ok_or_else(|| no_solver(OP_FACE, g))?;
        // SmApproxTol3d sApproxTol3d = GetThisApproxTol3d(): USE_DEFAULT, then fabs
        let approx_tol_3d = crate::geom::tol::approx_tol3d(k, resolve(sid, OP_FACE)?.approx_tol_3d);
        let d3d_tol = approx_tol_3d;
        let mut uv_tol = d3d_tol;
        if use_uv_curves {
            let uv = uv_domain.mid();
            let uv_dir = Vec2::new(1.0, 0.0);
            uv_tol = map_to_2d_dir(
                k,
                approx_tol_3d,
                uv,
                uv_dir,
                self.geom(g)?
                    .fillet_surface(None)?
                    .ok_or_else(|| broken_link(OP_FACE, GEOM_SURFACE_GONE))?,
            )?;
        }

        let rail_curve = |fb: &Self, e: EdgeId| -> Result<Curve> {
            fb.brep()
                .edge(e)?
                .curve
                .clone()
                .ok_or_else(|| edge_without_curve(OP_FACE, e))
        };
        let eu_uv_curve = |fb: &Self, eu: EdgeuseId| -> Result<Option<Curve>> {
            Ok(eu1::uv_trim_curve(fb.brep(), eu)?.0.cloned())
        };

        let mut lists1 = CurveLists::default();
        let mut lists2 = CurveLists::default();

        // First the start rail:
        let rail0 = rails
            .get(start_rail_indx)
            .copied()
            .flatten()
            .ok_or_else(|| no_rail(OP_FACE, g, start_rail_indx))?;
        let mut prim = self.primary_of(rail0, OP_FACE)?;
        let uv_curve0 = if use_uv_curves {
            eu_uv_curve(self, prim)?
        } else {
            None
        };
        let other_rails = if start_rail_indx == 1 {
            &other_rails2
        } else {
            &other_rails1
        };

        lists1.curves_3d.push(Some(rail_curve(self, rail0)?));
        lists1.uv_curves.push(uv_curve0);
        lists1.orients.push(OrientType::Same);

        // Get current start & end verts (in the correct order).
        let (start_fv0, mut end_fv0);
        if self.brep().edgeuse(prim)?.orientation == OrientType::Opposite {
            end_fv0 = edge::vertex(self.brep(), rail0)?;
            // (upstream passes the still-NULL start vertex)
            start_fv0 = edge::other_vertex(self.brep(), rail0, None)?;
        } else {
            start_fv0 = edge::vertex(self.brep(), rail0)?;
            end_fv0 = edge::other_vertex(self.brep(), rail0, start_fv0)?;
        }

        // Next Add MinV OtherRails
        let total = other_rails.len();
        for (ii, &other_rail) in other_rails.iter().enumerate() {
            prim = self.primary_of(other_rail, OP_FACE)?;
            lists1.curves_3d.push(Some(rail_curve(self, other_rail)?));
            lists1.uv_curves.push(eu_uv_curve(self, prim)?);
            lists1.orients.push(OrientType::Same);

            // for the last MinV Rail edge - update the pEndFV0 value
            if ii + 1 == total {
                let first = edge::vertex(self.brep(), other_rail)?;
                end_fv0 = edge::other_vertex(self.brep(), other_rail, first)?;
            }
        }

        // Load MaxV rail(s) into OrderedEdges List2
        let rail1 = rails
            .get(1 - start_rail_indx)
            .copied()
            .flatten()
            .ok_or_else(|| no_rail(OP_FACE, g, 1 - start_rail_indx))?;
        let mut start_fv1 = edge::vertex(self.brep(), rail1)?;
        let mut end_fv1 = edge::other_vertex(self.brep(), rail1, start_fv1)?;
        let other_rails1 = if start_rail_indx == 1 {
            &other_rails1
        } else {
            &other_rails2
        };

        // For this side go backwards
        for (ii, &other_rail) in other_rails1.iter().enumerate().rev() {
            prim = self.primary_of(other_rail, OP_FACE)?;
            lists2.curves_3d.push(Some(rail_curve(self, other_rail)?));
            lists2.uv_curves.push(eu_uv_curve(self, prim)?);
            lists2.orients.push(OrientType::Opposite);

            // for the first edge - update the EndVertex value
            if ii == 0 {
                let first = edge::vertex(self.brep(), other_rail)?;
                end_fv1 = edge::other_vertex(self.brep(), other_rail, first)?;
            }
        }

        // Finally load the MaxV rail into OrderedEdge List2.
        prim = self.primary_of(rail1, OP_FACE)?;
        lists2.curves_3d.push(Some(rail_curve(self, rail1)?));
        lists2.uv_curves.push(eu_uv_curve(self, prim)?);
        lists2.orients.push(OrientType::Opposite);

        // order the MaxV Rail vertices
        if self.brep().edgeuse(prim)?.orientation == OrientType::Opposite {
            core::mem::swap(&mut start_fv1, &mut end_fv1);
        }

        // Add a sequence of m_vSideEUs->Curves to traverse from pEndFV0 to pEndFV1
        let made_connection = join_two_fillet_verts_by_curves_resolved(
            k,
            self,
            g,
            end_fv0,
            end_fv1,
            &side_eus,
            uv_tol,
            &mut lists1.curves_3d,
            &mut lists1.uv_curves,
            &mut lists1.orients,
            resolve,
        )?;

        // when railCurveEnd/FilletEdgeStart has too big a gap: cap it
        self.geom_mut(g)?.capped[1] = false;
        if use_uv_curves && !made_connection {
            self.geom_mut(g)?.capped[1] = true;
            let last = lists1.uv_curves.last().cloned();
            let first = lists2.uv_curves.first().cloned();
            let mut caps = CurveLists::default();
            Self::cap_gap(
                k,
                approx_tol_3d,
                self.geom(g)?
                    .fillet_surface(None)?
                    .ok_or_else(|| broken_link(OP_FACE, GEOM_SURFACE_GONE))?,
                [
                    (last.as_ref(), OrientType::Same),
                    (first.as_ref(), OrientType::Same),
                ],
                &mut caps,
            )?;
            lists1.curves_3d.append(&mut caps.curves_3d);
            lists1.uv_curves.append(&mut caps.uv_curves);
            lists1.orients.append(&mut caps.orients);
        }

        // Add MaxV rail sequence
        lists1.curves_3d.append(&mut lists2.curves_3d);
        lists1.uv_curves.append(&mut lists2.uv_curves);
        lists1.orients.append(&mut lists2.orients);

        // Add in m_vSideEUs Edge sequence to traverse from pStartFV1 to pStartFV0
        let made_connection = join_two_fillet_verts_by_curves_resolved(
            k,
            self,
            g,
            start_fv1,
            start_fv0,
            &side_eus,
            uv_tol,
            &mut lists1.curves_3d,
            &mut lists1.uv_curves,
            &mut lists1.orients,
            resolve,
        )?;

        self.geom_mut(g)?.capped[0] = false;
        if use_uv_curves && !made_connection {
            self.geom_mut(g)?.capped[0] = true;
            let last = lists1.uv_curves.last().cloned();
            let first = lists1.uv_curves.first().cloned();
            let mut caps = CurveLists::default();
            Self::cap_gap(
                k,
                approx_tol_3d,
                self.geom(g)?
                    .fillet_surface(None)?
                    .ok_or_else(|| broken_link(OP_FACE, GEOM_SURFACE_GONE))?,
                [
                    (last.as_ref(), OrientType::Opposite),
                    (first.as_ref(), OrientType::Opposite),
                ],
                &mut caps,
            )?;
            lists1.curves_3d.append(&mut caps.curves_3d);
            lists1.uv_curves.append(&mut caps.uv_curves);
            lists1.orients.append(&mut caps.orients);
        }

        // remove any degenerate curves added to the curve sequence. The rails' curves are
        // checked as they are pushed, but the side-edge join (p6) pushes a side edge's curve
        // unchecked, so an edge of the caller's fillet B-rep without a curve arrives here as
        // `None` (upstream dereferences it): the caller's set-up, not a defect. The list
        // keeps no edge ids, so the failure has no entity.
        for m in (0..lists1.curves_3d.len()).rev() {
            let degenerate = lists1
                .curves_3d
                .get(m)
                .and_then(Option::as_ref)
                .ok_or_else(|| SolidError::blend(OP_FACE, BlendFailure::EdgeWithoutCurve))?
                .is_degenerate(k, EFF_ZERO, None)?;
            if degenerate {
                lists1.curves_3d.remove(m);
                lists1.uv_curves.remove(m);
                lists1.orients.remove(m);
            }
        }

        // every curve left is there: the loop above errors on a missing one and only removes
        // entries, so the checked curves are carried on as they are
        let loop_curves: Vec<Curve> = core::mem::take(&mut lists1.curves_3d)
            .into_iter()
            .flatten()
            .collect();

        // set the number of curves forming this filletSurfaces boundary loop
        let curve_loops = vec![loop_curves.len()];

        // turn off UseUVCurves if any UVCurve array member is set to NULL
        if lists1.uv_curves.iter().any(Option::is_none) {
            use_uv_curves = false;
        }

        // Make a copy of each 3D & UV curve
        let mut curves_3d = Vec::new();
        let mut uv_curves = Vec::with_capacity(lists1.uv_curves.len());
        for (c, uv) in loop_curves.iter().zip(&lists1.uv_curves) {
            as_b_spline(c).ok_or_else(|| curve_without_net(OP_FACE, None, c))?;
            // (`Copy` keeps each kind `as_b_spline` accepts, so the copy is a B-spline
            // curve too)
            curves_3d.push(c.copy(k)?);

            // skip copying uvTrimCurves when not using them (each is there when they are
            // used: a missing one turned them off above)
            if use_uv_curves {
                uv_curves.extend(uv.clone());
            }
        }

        // Make Face: transfer the original surface only at the native call boundary.
        let region = brep.infinite_region();
        // The fork's (WR-697, 32fd80a): the face's edge uses keep these trim curves, the
        // fillet's own (each its edge's curve on this surface, made with it: a corner's
        // exact ones exactly), where upstream's MakeFaceWithCurves drops them and makes each
        // again later by projecting its edge's curve onto the surface.
        let keep_uv_curves = Some(true);
        if matches!(surface_backend, FaceSurfaceBackend::IndexedValue) {
            // Preserve the existing indexed value contract, including its cliff path.
            // This API does not provide shared source-surface/managed-face binding.
            let surface = self
                .geom(g)?
                .fillet_surface(None)?
                .ok_or_else(|| broken_link(OP_FACE, GEOM_SURFACE_GONE))?
                .clone();
            if use_uv_curves {
                let input = FaceCurvesInput {
                    loops: curve_loops,
                    curves_3d: Some(curves_3d.into_iter().map(Some).collect()),
                    uv_curves: Some(uv_curves),
                    orientations: lists1.orients,
                    loop_points: Vec::new(),
                };
                let key_of = env.key_of;
                make_face_with_curves(
                    k,
                    brep,
                    region,
                    input,
                    surface,
                    &uv_domain,
                    OrientType::Same,
                    keep_uv_curves,
                    &|face| key_of(EntityRef::Face(face)),
                    env,
                )?;
            } else {
                let input = TrimInput {
                    loop_counts: curve_loops,
                    curves: curves_3d,
                    orientations: lists1.orients,
                    loop_points: Vec::new(),
                    surface,
                    uv_domain,
                    surface_orientation: OrientType::Same,
                };
                trim_surface_with_model_space_curves(
                    k, brep, region, d3d_tol, input, true, false, false, env,
                )?;
            }
            return Ok(());
        }
        if use_uv_curves {
            let input = FaceCurvesInput {
                loops: curve_loops,
                curves_3d: Some(curves_3d.into_iter().map(Some).collect()),
                uv_curves: Some(uv_curves),
                orientations: lists1.orients,
                loop_points: Vec::new(),
            };
            let key_of = env.key_of;
            let face_ref = |f: FaceId| key_of(EntityRef::Face(f));
            let owned = self
                .geom_mut(g)?
                .fillet_surface
                .take()
                .ok_or_else(|| broken_link(OP_FACE, GEOM_SURFACE_GONE))?;
            let super::p1::FilletSurfaceStorage::Owned(surface) = owned else {
                // Earlier checked reads already reject a transferred surface.
                self.geom_mut(g)?.fillet_surface = Some(owned);
                return Err(SolidError::invalid_input(
                    OP_FACE,
                    "surface already transferred",
                ));
            };
            let mut transfer = crate::topology::brep::p6::FaceSurfaceTransfer::new(*surface);
            let status = make_face_with_curves_transfer(
                k,
                brep,
                region,
                input,
                &mut transfer,
                &uv_domain,
                OrientType::Same,
                keep_uv_curves,
                &face_ref,
                env,
                &mut NewFace::default(),
            );
            let installed = transfer.installed_face();
            let location = match transfer.into_location()? {
                crate::topology::brep::p6::FaceSurfaceLocation::Owned(surface) => {
                    super::p1::FilletSurfaceStorage::Owned(surface)
                }
                crate::topology::brep::p6::FaceSurfaceLocation::Installed(face) => {
                    debug_assert_eq!(installed, Some(face));
                    super::p1::FilletSurfaceStorage::ManagedFace {
                        body: key_of(EntityRef::Brep(crate::topology::BrepId::from_index(0))),
                        face,
                    }
                }
            };
            // Preserve the actual storage location even when construction failed.
            self.geom_mut(g)?.fillet_surface = Some(location);
            status?;
        } else {
            use crate::topology::trimming_tools::{
                trim_surface_with_model_space_curves_transfer, TrimCurvesInput,
                TrimSurfaceLocation, TrimSurfaceTransfer,
            };

            let input = TrimCurvesInput {
                loop_counts: curve_loops,
                curves: curves_3d,
                orientations: lists1.orients,
                loop_points: Vec::new(),
                uv_domain,
                surface_orientation: OrientType::Same,
            };
            let owned = self
                .geom_mut(g)?
                .fillet_surface
                .take()
                .ok_or_else(|| broken_link(OP_FACE, GEOM_SURFACE_GONE))?;
            let super::p1::FilletSurfaceStorage::Owned(surface) = owned else {
                self.geom_mut(g)?.fillet_surface = Some(owned);
                return Err(SolidError::invalid_input(
                    OP_FACE,
                    "surface already transferred",
                ));
            };
            let mut transfer = TrimSurfaceTransfer::new(*surface);
            let status = trim_surface_with_model_space_curves_transfer(
                k,
                brep,
                region,
                d3d_tol,
                input,
                &mut transfer,
                env,
            );
            self.geom_mut(g)?.fillet_surface = match transfer.into_location()? {
                TrimSurfaceLocation::Owned(surface) => {
                    Some(super::p1::FilletSurfaceStorage::Owned(surface))
                }
                TrimSurfaceLocation::Installed(face) => {
                    Some(super::p1::FilletSurfaceStorage::ManagedFace {
                        body: (env.key_of)(EntityRef::Brep(crate::topology::BrepId::from_index(0))),
                        face,
                    })
                }
                TrimSurfaceLocation::Destroyed => None,
            };
            status?;
        }

        Ok(())
    }

    /// `JoinVertsByCapping` between two UV curves of the loop being built (upstream
    /// dereferences a `NULL` curve), appending the caps to `caps`.
    fn cap_gap(
        k: &Kernel,
        approx_tol_3d: f64,
        fillet_surface: &Surface,
        ends: [(Option<&Option<Curve>>, OrientType); 2],
        caps: &mut CurveLists,
    ) -> Result<()> {
        let [(c1, o1), (c2, o2)] = ends;
        let c1 = c1
            .and_then(Option::as_ref)
            .ok_or_else(|| set_up(OP_FACE, FilletSetUp::BoundaryUvCurve, None))?;
        let c2 = c2
            .and_then(Option::as_ref)
            .ok_or_else(|| set_up(OP_FACE, FilletSetUp::BoundaryUvCurve, None))?;
        let mut curves_3d = Vec::new();
        let mut uv_curves = Vec::new();
        let mut orients = Vec::new();
        join_verts_by_capping(
            k,
            approx_tol_3d,
            fillet_surface,
            c1,
            o1,
            c2,
            o2,
            &mut curves_3d,
            &mut uv_curves,
            &mut orients,
        )?;
        caps.curves_3d.extend(curves_3d.into_iter().map(Some));
        caps.uv_curves.extend(uv_curves.into_iter().map(Some));
        caps.orients.append(&mut orients);
        Ok(())
    }

    /// `SmFilletGeom::InsertIntersectionTopology`: merges each rail and each side curve
    /// of geometry `g` into its original face of the target B-rep through the fillet
    /// executive's topology intersector (`context`, the live executive's merge state),
    /// and registers the (empty) intersections of the fillet surface with those faces
    /// and with the faces around the filleted edge's uncapped vertices.
    ///
    /// `solver` is the geometry's actual retained solver; the context preserves
    /// surface identity and scoped marker lifetimes.
    ///
    /// # Errors
    /// Native insertion failures and missing live capabilities, with preceding
    /// topology and marker/error mutations retained.
    #[allow(clippy::too_many_lines)] // native ordered insertion routine
    pub fn insert_intersection_topology_with_context(
        &mut self,
        g: FilletGeomId,
        solver: &FilletSolver,
        target: &mut Brep,
        context: &mut impl IntersectionTopologyContext,
    ) -> Result<()> {
        let Some(solver_id) = self.geom(g)?.fillet_solver() else {
            return Err(no_solver(OP_INSERT, g));
        };
        if solver.executive.is_none() {
            return Err(set_up(OP_INSERT, FilletSetUp::SolverExecutive, None));
        }
        context.begin_intersection(solver)?;
        let fillet_surface = context.surface_binding(self, g)?;
        let mut deleted_edges = Vec::new();

        for ii in 0..2 {
            let geom = self.geom(g)?;
            let mut rails = vec![geom.rail(ii).ok_or_else(|| no_rail(OP_INSERT, g, ii))?];
            rails.extend_from_slice(if ii == 0 {
                &geom.other_rails1
            } else {
                &geom.other_rails2
            });
            for rail in rails {
                let face = self.fillet_edge(rail)?.original_face();
                let managed_edge = self.fillet_edge(rail)?.fillet_brep_edge();
                let users = context.attribute_edges()?;
                let curve = self
                    .brep()
                    .edge(rail)?
                    .curve
                    .as_ref()
                    .ok_or_else(|| edge_without_curve(OP_INSERT, rail))?;
                // Native evaluates degeneracy before the nullable face/edge checks.
                if curve.is_degenerate(context.kernel(), EFF_ZERO, None)? {
                    continue;
                }
                let (Some(face), Some(managed_edge)) = (face, managed_edge) else {
                    continue;
                };
                if !users.contains(&managed_edge) {
                    continue;
                }
                context.with_rail_base(target, solver_id, ii, |context, target| {
                    context.with_face_scope(
                        target,
                        face,
                        IntersectionFaceRole::Rail,
                        |context, target, marker| {
                            let tolerance = context.managed_edge_tolerance(managed_edge)?;
                            let merged = context.merge_curve(
                                self,
                                target,
                                face,
                                tolerance,
                                rail,
                                managed_edge,
                                &mut deleted_edges,
                            );
                            self.fillet_edge_mut(rail)?
                                .clear_fillet_brep_edges(&deleted_edges);
                            context.update_topology(self, target, marker, face)?;
                            if let Err(error) = merged {
                                if error.is_unported_or_cancelled() {
                                    return Err(error);
                                }
                                if let Some(edgeuse) = solver.edgeuse(0)? {
                                    context.record_bad_merge(
                                        self,
                                        g,
                                        eu1::edge(target, edgeuse)?,
                                        target,
                                        face,
                                        rail,
                                        &fillet_surface,
                                    )?;
                                }
                                return Err(error);
                            }
                            context.register_intersection(self, target, face, &fillet_surface)
                        },
                    )
                })?;
            }
        }
        let side_eus = self.geom(g)?.side_eus.clone();
        for side_eu in side_eus {
            let edge = eu1::edge(self.brep(), side_eu)?.ok_or_else(|| {
                Failure::new(OP_INSERT, BlendFailure::EdgeUseWithoutEdge)
                    .at(EntityRef::Edgeuse(side_eu))
                    .into_error()
            })?;
            let face = self.fillet_edge(edge)?.original_face();
            let managed_edge = self.fillet_edge(edge)?.fillet_brep_edge();
            let users = context.attribute_edges()?;
            let Some(face) = face else {
                continue;
            };
            let Some(managed_edge) = managed_edge.filter(|edge| users.contains(edge)) else {
                continue;
            };
            context.with_face_scope(
                target,
                face,
                IntersectionFaceRole::Side,
                |context, target, marker| {
                    let tolerance = context.managed_edge_tolerance(managed_edge)?;
                    context.merge_curve(
                        self,
                        target,
                        face,
                        tolerance,
                        edge,
                        managed_edge,
                        &mut deleted_edges,
                    )?;
                    self.fillet_edge_mut(edge)?
                        .clear_fillet_brep_edges(&deleted_edges);
                    context.update_topology(self, target, marker, face)?;
                    context.register_intersection(self, target, face, &fillet_surface)
                },
            )?;
        }
        let mut vertex = None;
        for ii in 0..2 {
            let previous = vertex;
            vertex = solver.vertex(target, ii)?;
            if vertex == previous {
                continue;
            }
            if let Some(vertex) = vertex {
                if !self.geom(g)?.capped.get(ii).copied().unwrap_or(false) {
                    for face in vertex::faces(target, vertex)? {
                        context.register_intersection(self, target, face, &fillet_surface)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// One pass of `CalcBlendingGeom`'s rail loop: rail `i` of geometry `g` is
    /// recomputed (`CalcBlendingRail`), resampled at ten arc-length steps in its mate's UV
    /// curve and lifted again onto its face; returns the rail (`pRail1`/`pRail2`).
    fn resample_blending_rail(
        &mut self,
        k: &Kernel,
        g: FilletGeomId,
        i: usize,
        approx_tol_3d: f64,
        target: &mut Brep,
    ) -> Result<EdgeId> {
        let rail = self
            .geom(g)?
            .rail(i)
            .ok_or_else(|| no_rail(OP_BLEND, g, i))?;
        if self.fillet_edge(rail)?.fillet_edge_type() != FilletEdgeType::BlendingRail {
            return Err(
                Failure::new(OP_BLEND, BlendFailure::RailNotBlending { rail })
                    .at(EntityRef::Edge(rail))
                    .into_error(),
            );
        }
        self.calc_blending_rail(k, rail, target)?;
        let rail_curve = self
            .brep()
            .edge(rail)?
            .curve
            .clone()
            .ok_or_else(|| broken_link(OP_BLEND, BLEND_RAIL_NO_CURVE))?;
        as_b_spline(&rail_curve)
            .ok_or_else(|| curve_without_net(OP_BLEND, Some(rail), &rail_curve))?;
        let (_, mate) = self.prim_and_mate(rail, OP_BLEND)?;
        let uv_curve = eu1::uv_trim_curve(self.brep(), mate)?
            .0
            .cloned()
            .ok_or_else(|| broken_link(OP_BLEND, BLEND_RAIL_NO_UV))?;
        let face = self.fillet_edge(rail)?.original_face().ok_or_else(|| {
            set_up(
                OP_BLEND,
                FilletSetUp::OriginalFace,
                Some(EntityRef::Edge(rail)),
            )
        })?;
        // the face's own surface (evaluated as a copy: the lift's trimmed surface
        // cache is made on `target` through the face, as upstream's GetFace())
        let base_surf = &target
            .face(face)?
            .surface
            .as_ref()
            .ok_or_else(|| face_without_surface(OP_BLEND, face))?
            .clone_for_evaluation()?;
        let ivl = rail_curve.natural_interval();
        let accuracy = approx_tol_3d / 10.0;
        let crv_len = rail_curve.length(k, &ivl, accuracy)?;

        // Step along this rail curve, at points evenly spaced in arc length.
        let total_samples = 10u32;
        let step = crv_len / f64::from(total_samples);
        let mut start_param = ivl.min();
        let mut uv_points = vec![uv_curve.evaluate_point(k, start_param)?];
        for _ in 1..total_samples {
            // Find param on curve at arc length dStep from previous point.
            let end_param = rail_curve
                .find_parameter_at_arc_length(k, start_param, step)?
                .end_param;
            start_param = end_param;

            // Find uv from the corresponding uv curve.
            uv_points.push(uv_curve.evaluate_point(k, start_param)?);
        }

        // Get the final point.
        uv_points.push(uv_curve.evaluate_point(k, ivl.max())?);

        // pRail->SetCurve(NULL, TRUE): deletes the curve and the UV trim curves
        set_edge_curve(self.brep_mut(), rail, None)?;
        let new_uv_curve = BSplineCurve::create_interpolating_curve(
            ParamMethod::ChordLength,
            2,
            2,
            &uv_points,
            &[],
            None,
            true,
        )?;

        // Lift it to 3D
        let uv_domain = base_surf.natural_uv_domain_with_nurb(k)?;
        let face_tol = crate::topology::face::p2::tolerance(target, face)?;
        let lifted = base_surf.lift_curve(
            k,
            &uv_domain,
            &Curve::from(new_uv_curve.clone()),
            &ivl,
            crate::geom::tol::approx_tol3d(k, face_tol),
            false,
        )?;
        let crv_ivl = lifted.curve.natural_interval();
        set_edge_curve(self.brep_mut(), rail, Some(lifted.curve))?;
        edge::set_interval(self.brep_mut(), rail, &crv_ivl)?;
        self.set_edgeuse_uv_curve(mate, Some(Curve::from(new_uv_curve)), true, true)?;
        Ok(rail)
    }

    /// `SmFilletGeom::CalcBlendingGeom`: the surface of a blending geometry: both
    /// blending rails are recomputed, resampled at ten arc-length steps in their UV
    /// curves and lifted again; then a ruled surface between the rails for a linear
    /// cross-section, else a 4-sided corner blend over the start cross-section, rail
    /// 0, the end cross-section and rail 1.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a rail that is not a blending rail; `SM_ERR_NULL_POINTER` for a
    /// missing solver, rail, curve, UV curve, face, surface or cross-section edge;
    /// those of the length, arc-length, interpolation, lift and surface creations and
    /// of `SmFilletEdge::CalcBlendingRail`.
    pub fn calc_blending_geom(
        &mut self,
        k: &Kernel,
        g: FilletGeomId,
        solvers: &[FilletSolver],
        target: &mut Brep,
    ) -> Result<()> {
        let sid = self
            .geom(g)?
            .fillet_solver()
            .ok_or_else(|| no_solver(OP_BLEND, g))?;
        let fil_solver = solver_of(solvers, sid, OP_BLEND)?;
        self.calc_blending_geom_of_solver(k, g, fil_solver, target)
    }

    /// [`FilletBrep::calc_blending_geom`] with the geometry's solver
    /// (`GetFilletSolver()`) given directly: for a caller that is that solver (the
    /// fillet solver's `CalcFilletGeom`, unit `blend.fillet_solver_p4`, whose
    /// geometries all name it). A geometry without a solver is still
    /// `SM_ERR_NULL_POINTER`.
    ///
    /// # Errors
    ///
    /// As [`FilletBrep::calc_blending_geom`].
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn calc_blending_geom_of_solver(
        &mut self,
        k: &Kernel,
        g: FilletGeomId,
        fil_solver: &FilletSolver,
        target: &mut Brep,
    ) -> Result<()> {
        if self.geom(g)?.fillet_solver().is_none() {
            return Err(no_solver(OP_BLEND, g));
        }
        let approx_tol_3d = fil_solver.approx_tol_3d;
        let tangency_tol_radians = fil_solver.tangency_tolerance;

        // Recreate each rail curve by sampling points evenly spaced in arc length
        // (m_vRails[i] is read as the loop reaches it: a failure on rail 0 comes first)
        let rail1 = self.resample_blending_rail(k, g, 0, approx_tol_3d, target)?;
        let rail2 = self.resample_blending_rail(k, g, 1, approx_tol_3d, target)?;

        // Create ruled surface if we have linear cross sections
        let fsg = fil_solver
            .fsg
            .ok_or_else(|| set_up(OP_BLEND, FilletSetUp::SurfaceGenerator, None))?;
        if fsg.fillet_surface_generator_type() == FilletSurfaceGeneratorType::Linear {
            let c1 = self
                .brep()
                .edge(rail1)?
                .curve
                .as_ref()
                .ok_or_else(|| broken_link(OP_BLEND, BLEND_RAIL_NO_CURVE))?;
            let c2 = self
                .brep()
                .edge(rail2)?
                .curve
                .as_ref()
                .ok_or_else(|| broken_link(OP_BLEND, BLEND_RAIL_NO_CURVE))?;
            let ruled = BSplineSurface::create_ruled_surface(
                as_b_spline(c1).ok_or_else(|| curve_without_net(OP_BLEND, Some(rail1), c1))?,
                as_b_spline(c2).ok_or_else(|| curve_without_net(OP_BLEND, Some(rail2), c2))?,
                SurfParamType::V,
            )?;
            self.geom_mut(g)?
                .set_fillet_surface(Some(Surface::from(ruled)));
            return Ok(());
        }

        // Find 2 side edges (with type SM_FE_CROSS_SECTION)
        let cross_section_at =
            |fb: &Self, v: Option<VertexId>| -> Result<(VertexId, Option<EdgeId>)> {
                let v = v.ok_or_else(|| {
                    set_up(
                        OP_BLEND,
                        FilletSetUp::EdgeVertices,
                        Some(EntityRef::Edge(rail1)),
                    )
                })?;
                for e in vertex::edges(fb.brep(), v, None)? {
                    if fb.fillet_edge(e)?.fillet_edge_type() == FilletEdgeType::CrossSection {
                        return Ok((v, Some(e)));
                    }
                }
                Ok((v, None))
            };
        let start_vert = edge::vertex(self.brep(), rail1)?;
        let (start_v, start_edge) = cross_section_at(self, start_vert)?;
        let end_vert = edge::other_vertex(self.brep(), rail1, start_vert)?;
        let (end_v, end_edge) = cross_section_at(self, end_vert)?;
        let start_edge = start_edge.ok_or_else(|| {
            set_up(
                OP_BLEND,
                FilletSetUp::VertexCrossSection,
                Some(EntityRef::Vertex(start_v)),
            )
        })?;
        let end_edge = end_edge.ok_or_else(|| {
            set_up(
                OP_BLEND,
                FilletSetUp::VertexCrossSection,
                Some(EntityRef::Vertex(end_v)),
            )
        })?;

        let fil_edges = [start_edge, rail1, end_edge, rail2];
        let orients = [
            OrientType::Opposite,
            OrientType::Same,
            OrientType::Same,
            OrientType::Opposite,
        ];
        let mut ordered_uv: Vec<Option<BSplineCurve>> = Vec::new();
        let mut ordered_3d: Vec<BSplineCurve> = Vec::new();
        let mut ordered_surfs: Vec<Option<&Surface>> = Vec::new();
        for &fe in &fil_edges {
            // pMateEU is dereferenced only for the rails
            let (prim, mate) = self.prim_and_opt_mate(fe, OP_BLEND)?;
            let (ps_curve, base_surface) = match self.fillet_edge(fe)?.fillet_edge_type() {
                FilletEdgeType::BlendingRail
                | FilletEdgeType::RailRailInterpolation
                | FilletEdgeType::SetbackRail => {
                    let mate = mate.ok_or_else(|| {
                        set_up(
                            OP_BLEND,
                            FilletSetUp::PrimaryEdgeUseMate,
                            Some(EntityRef::Edge(fe)),
                        )
                    })?;
                    let face = self.fillet_edge(fe)?.original_face().ok_or_else(|| {
                        set_up(
                            OP_BLEND,
                            FilletSetUp::OriginalFace,
                            Some(EntityRef::Edge(fe)),
                        )
                    })?;
                    (
                        eu1::uv_trim_curve(self.brep(), mate)?.0,
                        target.face(face)?.surface.as_ref(),
                    )
                }
                // Only the two cross-sections reach this arm: `cross_section_at` picks
                // `start_edge`/`end_edge` by `FilletEdgeType::CrossSection`, and
                // `resample_blending_rail` refuses a rail that is not a `BlendingRail`
                // (`RailNotBlending`), so `rail1`/`rail2` always take the arm above.
                // The missing link here is therefore always a cross-section's geometry.
                // (A rail's 3D curve cannot be missing below either: the resampling has
                // just stored the lifted curve on it.)
                _ => {
                    let fg = self.fillet_edgeuse(prim)?.fillet_geom().ok_or_else(|| {
                        set_up(
                            OP_BLEND,
                            FilletSetUp::CrossSectionGeom,
                            Some(EntityRef::Edgeuse(prim)),
                        )
                    })?;
                    (
                        eu1::uv_trim_curve(self.brep(), prim)?.0,
                        self.geom(fg)?.fillet_surface(None)?,
                    )
                }
            };
            ordered_uv.push(ps_curve.and_then(as_b_spline).cloned());
            let c = self
                .brep()
                .edge(fe)?
                .curve
                .as_ref()
                .ok_or_else(|| edge_without_curve(OP_BLEND, fe))?;
            ordered_3d.push(
                as_b_spline(c)
                    .ok_or_else(|| curve_without_net(OP_BLEND, Some(fe), c))?
                    .clone(),
            );
            ordered_surfs.push(base_surface);
        }
        let nets: Vec<Option<BlendNeighbour<'_>>> = ordered_surfs
            .iter()
            .map(|s| s.map(|surface| BlendNeighbour { surface }))
            .collect();
        let surfaces = BSplineSurface::create_corner_blend(
            k,
            &ordered_3d,
            &orients,
            &ordered_uv,
            &nets,
            approx_tol_3d,
            tangency_tol_radians,
            false,
        )?;
        let first = surfaces
            .into_iter()
            .next()
            .ok_or_else(|| broken_link(OP_BLEND, CORNER_BLEND_EMPTY))?;
        self.geom_mut(g)?
            .set_fillet_surface(Some(Surface::from(first)));
        Ok(())
    }
}

// ---- the reasons of the defects (#15034) -------------------------------------------------
//
// Each names the step that failed and why that is the kernel's fault: the state named is
// what the routine itself (or a kernel routine it just called) built or checked, so no
// set-up of the caller's reaches it. A link of the caller's fillet B-rep that is missing is
// not a defect but [`BlendFailure::FilletSetUpInvalid`] (or another caller cause).

/// `sm_CreateUVCurve`'s dropping tolerance.
const UV_NO_BOX: &str = "the surface's bounding box came back without its box, though the \
    box was asked for and the bounding box routine makes every box asked for";
/// `CalcFilletInt2Fillets`'s reversal of the intersection's UV curves.
const SSI_NO_UV_CURVE: &str = "the intersection of the corner's fillet surfaces returned its \
    curve without a UV curve, which the reversal reverses with it, though the intersection \
    returns a curve only with both UV curves: the corners ask for no closure, so a UV curve \
    the intersector leaves out is made by dropping the curve, and a drop that makes none \
    fails the intersection; the curves it keeps and drops are B-spline kinds, which the \
    corner keeps";
/// The corners' intersections of two surfaces (`CalcFilletIntNx2Fillets`,
/// `CalcFilletInt2Fillets`, `CalcRailIntExtendedEdgeuse`) that returned no curve.
const CORNER_SSI_NO_CURVE: &str = "the intersection of the corner's surfaces returned no \
    curve, though it returns its curve on every success: the corners' calls pass no end \
    point (so the plane-curve path is not taken) and either no start point or the two-point \
    search skipped (so the two-point path is not taken), and the general path fails rather \
    than return a curve of a kind that is not a B-spline, the kinds the corner keeps";
/// `CalcFilletInt2Fillets`'s curve, stored above.
const INT2_CURVE_UNSET: &str = "the fillet/fillet edge's intersection curve, stored on it \
    above, is not there";
/// A geometry's fillet surface, found above.
const GEOM_SURFACE_GONE: &str = "the fillet geometry's surface, found above, is not there";
/// `CalcBlendingGeom`'s rail curve, after `CalcBlendingRail`.
const BLEND_RAIL_NO_CURVE: &str = "the blending rail has no curve, though CalcBlendingRail \
    makes it (and the resampling above stores it again)";
/// `CalcBlendingGeom`'s UV curve on the rail's face.
const BLEND_RAIL_NO_UV: &str = "the blending rail's mate has no UV trim curve on its face, \
    though CalcBlendingRail makes the rail with it";
/// `CreateCornerBlend`'s result.
const CORNER_BLEND_EMPTY: &str = "the corner blend returned no surface and no error, though \
    it makes a surface or fails";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::fillet_geom::p1::{FilletGeom, FilletVertex};
    use crate::error::ErrorKind;

    /// The operation of `e`, its typed blend failure and the upstream status it keeps
    /// (#13790).
    fn failure(e: &SolidError) -> (&'static str, Option<&BlendFailure>, Option<i32>) {
        (e.operation(), e.blend_failure(), e.upstream_status())
    }

    fn kernel() -> Kernel {
        Kernel::default()
    }

    /// A bilinear plane patch over `[0, 1]^2` scaled by `s`.
    fn plane_patch(s: f64) -> Surface {
        let mut p = Plane::from_normal(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
        p.make_nurb().unwrap();
        let _ = s;
        Surface::from(p)
    }

    fn fb_with_points(k: &Kernel, pts: &[Point3]) -> (FilletBrep, Vec<VertexId>) {
        let mut fb = FilletBrep::new();
        let mut ids = Vec::new();
        for &p in pts {
            let v = fb.brep_mut().add_vertex(p).unwrap();
            fb.insert_fillet_vertex(v, FilletVertex::new(k, None));
            ids.push(v);
        }
        (fb, ids)
    }

    #[test]
    fn uv_curve_on_plane_is_a_line() {
        let k = kernel();
        let srf = plane_patch(1.0);
        let dom = srf.natural_uv_domain_if_built();
        let a = srf.evaluate_point(&k, dom.evaluate(0.2, 0.3)).unwrap();
        let b = srf.evaluate_point(&k, dom.evaluate(0.2, 0.9)).unwrap();
        let (fb, v) = fb_with_points(&k, &[a, b]);
        let r = create_uv_curve(&k, &fb, v[0], v[1], false, &srf, 1e-6).unwrap();
        assert!(r.curve_3d.is_none());
        assert!(r.uv_curve2.is_none());
        let c = r.uv_curve1.unwrap();
        let ivl = c.natural_interval();
        let s = c.evaluate_point(&k, ivl.min()).unwrap();
        let e = c.evaluate_point(&k, ivl.max()).unwrap();
        let ua = dom.evaluate(0.2, 0.3);
        let ub = dom.evaluate(0.2, 0.9);
        assert!((s.x - ua.x).abs() < 1e-9 && (s.y - ua.y).abs() < 1e-9);
        assert!((e.x - ub.x).abs() < 1e-9 && (e.y - ub.y).abs() < 1e-9);
    }

    #[test]
    fn uv_curve_diagonal_without_vertexuse_falls_back_to_line() {
        let k = kernel();
        let srf = plane_patch(1.0);
        let dom = srf.natural_uv_domain_if_built();
        let a = srf.evaluate_point(&k, dom.evaluate(0.1, 0.1)).unwrap();
        let b = srf.evaluate_point(&k, dom.evaluate(0.8, 0.7)).unwrap();
        let (fb, v) = fb_with_points(&k, &[a, b]);
        let r = create_uv_curve(&k, &fb, v[0], v[1], false, &srf, 1e-6).unwrap();
        assert!(r.uv_curve1.is_some() && r.curve_3d.is_none());
    }

    #[test]
    fn seam_on_open_surface_fails() {
        let k = kernel();
        let srf = plane_patch(1.0);
        let (fb, v) = fb_with_points(
            &k,
            &[Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        );
        let e = create_uv_curve(&k, &fb, v[0], v[1], true, &srf, 1e-6).unwrap_err();
        assert_eq!(
            failure(&e),
            (
                OP_UV,
                Some(&BlendFailure::SeamSurfaceNotClosed),
                Some(SM_ERR)
            )
        );
        // the caller's surface, not the kernel's fault
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
        assert!(!e.is_defect());
    }

    /// #2403: a curve from a patch's pole (a ball's triangle, the pole its `v = min` side)
    /// to a corner of its other side is that corner's `u` iso-curve.
    #[test]
    fn uv_curve_from_a_pole_is_the_iso_curve_through_its_other_end() {
        use crate::blend::blend_corner::ball::ball_triangle_patch;
        let dirs = [
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        let (net, _, _) = ball_triangle_patch(Point3::ZERO, 2.0, &dirs, 0, false).unwrap();
        let srf = Surface::from(net);
        let k = kernel();
        let dom = srf.natural_uv_domain_with_nurb(&k).unwrap();
        let (lo, hi) = (dom.min(), dom.max());
        let pole = Point3::new(0.0, 0.0, 2.0);
        for t in [0.0, 0.3, 1.0] {
            let at = Point2::new(lo.x + (hi.x - lo.x) * t, lo.y);
            assert!(srf.evaluate_point(&k, at).unwrap().distance(pole) < 1e-12);
        }
        let isos = |k: &Kernel| -> Vec<bool> {
            [lo.x, hi.x]
                .iter()
                .map(|&u| {
                    let end = srf.evaluate_point(k, Point2::new(u, hi.y)).unwrap();
                    let (fb, v) = fb_with_points(k, &[pole, end]);
                    let Ok(r) = create_uv_curve(k, &fb, v[0], v[1], false, &srf, 1e-6) else {
                        return false;
                    };
                    let c = r.uv_curve1.unwrap();
                    let ivl = c.natural_interval();
                    let s = c.evaluate_point(k, ivl.min()).unwrap();
                    let e = c.evaluate_point(k, ivl.max()).unwrap();
                    r.curve_3d.is_none()
                        && (s.x - u).abs() < 1e-12
                        && (s.y - lo.y).abs() < 1e-12
                        && (e.x - u).abs() < 1e-12
                        && (e.y - hi.y).abs() < 1e-12
                })
                .collect()
        };
        assert_eq!(isos(&k), [true, true]);
    }

    #[test]
    fn b_spline_cast_accepts_b_spline_derived_kinds() {
        // SM_CAST_PTR(SmBSplineCurve, ..) succeeds for an SmLine (derived from
        // SmBSplineCurve upstream)
        let line = Curve::from(crate::curve::line::Line::from_points(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 3.0),
            3,
        ));
        assert!(as_b_spline(&line).is_some());
        let seg = BSplineCurve::create_line_segment(
            3,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(as_b_spline(&Curve::from(seg)).is_some());
    }

    #[test]
    fn rail_index_of_missing_vertex_is_null() {
        let k = kernel();
        let mut fb = FilletBrep::new();
        let g = fb.add_geom(FilletGeom::new(&k, None, FilletGeomType::Default).unwrap());
        let e = fb.rail_index_of_vertex(g, None).unwrap_err();
        assert_eq!(
            failure(&e),
            (
                OP_RAIL_IDX,
                Some(&BlendFailure::NoVertexGiven),
                Some(SM_ERR_NULL_POINTER)
            )
        );
        let v = fb
            .brep_mut()
            .add_vertex(Point3::new(0.0, 0.0, 0.0))
            .unwrap();
        // no rails: upstream dereferences the missing rail; p1's `rail_vertexuse`
        // (outside this file) refuses it first, keeping the status
        let e = fb.rail_index_of_vertex(g, Some(v)).unwrap_err();
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
    }

    #[test]
    fn nx2_without_fillet_surfaces_is_surf_int_failure() {
        // No cross-section edge: both fillet surfaces stay NULL, sm_SrfSrfIntersection's
        // NER fails, so upstream marks the vertex SM_FIL_SURF_INT_FAILURE and returns
        // SM_ERR (not SM_ERR_NULL_POINTER).
        let k = kernel();
        let (mut fb, v) = fb_with_points(&k, &[Point3::new(0.0, 0.0, 0.0)]);
        fb.fillet_vertex_mut(v[0])
            .unwrap()
            .set_fillet_corner(Some(super::super::p1::FilletCornerId(0)));
        let e = fb.calc_nx2_vert_fillet_x_fillet(&k, v[0], &[]).unwrap_err();
        assert_eq!(
            failure(&e),
            (
                OP_NX2,
                Some(&BlendFailure::FilletSetUpInvalid(
                    FilletSetUp::VertexCrossSection
                )),
                Some(SM_ERR)
            )
        );
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
        assert!(!e.is_defect());
        assert_eq!(
            fb.fillet_vertex(v[0]).unwrap().status(),
            FilStatus::SurfIntFailure
        );
    }

    #[test]
    fn int_2_fillets_without_corner_is_null() {
        let k = kernel();
        let (mut fb, v) = fb_with_points(&k, &[Point3::new(0.0, 0.0, 0.0)]);
        let e = fb
            .calc_fillet_int_2_fillets(&k, v[0], CornerLookup::default(), &[], &Brep::new())
            .unwrap_err();
        assert_eq!(
            failure(&e),
            (
                OP_INT2,
                Some(&BlendFailure::FilletVertexWithoutCorner),
                Some(SM_ERR_NULL_POINTER)
            )
        );
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
        assert!(!e.is_defect());
        fb.fillet_vertex_mut(v[0])
            .unwrap()
            .set_fillet_corner(Some(super::super::p1::FilletCornerId(0)));
        let e = fb
            .calc_fillet_int_2_fillets(&k, v[0], CornerLookup::default(), &[], &Brep::new())
            .unwrap_err();
        // a typed blend failure keeps the status of the bare `Upstream` (#13790)
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
    }

    #[test]
    fn rail_int_extended_without_edge_is_null() {
        let k = kernel();
        let (mut fb, v) = fb_with_points(&k, &[Point3::new(0.0, 0.0, 0.0)]);
        let e = fb
            .calc_rail_int_extended_edgeuse(
                &k,
                v[0],
                CornerLookup::default(),
                &[],
                &mut Brep::new(),
            )
            .unwrap_err();
        assert_eq!(
            failure(&e),
            (
                // the corner is looked up first, in CalcRailIntEndSurface
                "blend::fillet_geom::p4::FilletBrep::calc_rail_int_end_surface",
                Some(&BlendFailure::FilletVertexWithoutCorner),
                Some(SM_ERR_NULL_POINTER)
            )
        );
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn nx2_without_corner_is_null() {
        let k = kernel();
        let (mut fb, v) = fb_with_points(&k, &[Point3::new(0.0, 0.0, 0.0)]);
        let e = fb.calc_nx2_vert_fillet_x_fillet(&k, v[0], &[]).unwrap_err();
        assert_eq!(
            failure(&e),
            (
                OP_NX2,
                Some(&BlendFailure::FilletVertexWithoutCorner),
                Some(SM_ERR_NULL_POINTER)
            )
        );
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
        assert_eq!(e.entity(), Some(EntityRef::Vertex(v[0])));
    }

    #[test]
    fn make_face_without_surface_is_null() {
        let mut k = kernel();
        let mut fb = FilletBrep::new();
        let g = fb.add_geom(FilletGeom::new(&k, None, FilletGeomType::Default).unwrap());
        let mut b = Brep::new();
        let mut records = crate::topology::topology::NotifyRecords::new();
        let mut attributes = crate::geom::attribute::AttributeStore::default();
        let key_of = |_e: EntityRef| crate::geom::object::ObjectRef {
            type_code: 0,
            key: crate::geom::attribute::ObjectKey(0),
        };
        let mut env = edge::NotifyEnv {
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            key_of: &key_of,
        };
        // an untraced geometry: only a caller can ask for its face
        let e = fb
            .make_face_brep(&mut k, g, &[], &mut b, &mut env)
            .unwrap_err();
        assert_eq!(
            failure(&e),
            (
                OP_FACE,
                Some(&BlendFailure::Solver(
                    SolverFailure::FilletGeomWithoutSurface { fillet_geom: g }
                )),
                Some(SM_ERR_NULL_POINTER)
            )
        );
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
        assert!(!e.is_defect());
    }

    /// A side edge without a curve, set up by a caller through the public API (the
    /// fillet B-rep's body, a geometry's rails and side edge uses), reaches the face's
    /// loop through the side-edge join (p6 pushes the side edge's curve unchecked):
    /// the caller's input, an [`BlendFailure::EdgeWithoutCurve`] with no entity (the
    /// curve list keeps no edge ids), not a defect.
    #[test]
    fn make_face_with_a_curveless_side_edge_is_the_callers_input() {
        use crate::blend::fillet_geom::p1::{FilletGeomType, FilletSolverId};
        let mut k = kernel();
        let solvers = [FilletSolver::new(1e-6, 1e-3, 1e-3)];
        let mut fb = FilletBrep::new();
        *fb.brep_mut() =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0)
                .unwrap();
        // one face of the box: two opposite edges as the rails, the other two as the
        // side edges, both without their curves
        let face = fb.brep().face_list()[0];
        let lp = fb.brep().face(face).unwrap().loops[0];
        let edges: Vec<EdgeId> = fb
            .brep()
            .loop_(lp)
            .unwrap()
            .edges
            .iter()
            .map(|u| u.edge)
            .collect();
        assert_eq!(edges.len(), 4);
        let mut side_eus = Vec::new();
        for &side in &[edges[1], edges[3]] {
            side_eus.push(primary_edgeuse(fb.brep(), side).unwrap().unwrap());
            fb.brep_mut().edge_mut(side).unwrap().curve = None;
        }
        let mut geom = FilletGeom::new(
            &k,
            Some((FilletSolverId(0), &solvers[0])),
            FilletGeomType::CliffSidePatch,
        )
        .unwrap();
        geom.set_fillet_surface(Some(plane_patch(1.0)));
        geom.rails = [Some(edges[0]), Some(edges[2])];
        geom.side_eus = side_eus;
        let g = fb.add_geom(geom);
        let mut b = Brep::new();
        let mut records = crate::topology::topology::NotifyRecords::new();
        let mut attributes = crate::geom::attribute::AttributeStore::default();
        let key_of = |_e: EntityRef| crate::geom::object::ObjectRef {
            type_code: 0,
            key: crate::geom::attribute::ObjectKey(0),
        };
        let mut env = edge::NotifyEnv {
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            key_of: &key_of,
        };
        let e = fb
            .make_face_brep(&mut k, g, &solvers, &mut b, &mut env)
            .unwrap_err();
        assert_eq!(
            failure(&e),
            (
                OP_FACE,
                Some(&BlendFailure::EdgeWithoutCurve),
                Some(SM_ERR_NULL_POINTER)
            )
        );
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
        assert_eq!(e.entity(), None);
        assert!(!e.is_defect());
    }

    #[test]
    fn blending_without_solver_is_null() {
        let k = kernel();
        let mut fb = FilletBrep::new();
        let g = fb.add_geom(FilletGeom::new(&k, None, FilletGeomType::Blends).unwrap());
        let e = fb
            .calc_blending_geom(&k, g, &[], &mut Brep::new())
            .unwrap_err();
        assert_eq!(
            failure(&e),
            (
                OP_BLEND,
                Some(&BlendFailure::Solver(
                    SolverFailure::FilletGeomWithoutSolver { fillet_geom: g }
                )),
                Some(SM_ERR_NULL_POINTER)
            )
        );
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
        assert!(!e.is_defect());
    }
}
