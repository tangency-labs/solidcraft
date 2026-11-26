//! Fillet geometry, part 2: the surface/surface intersection curve between two points
//! (`sm_SrfSrfIntersection`), the fillet vertex calculators that intersect two rails,
//! a rail with a side edge, or a fillet with a cross-sectional or set-back plane, the
//! rail/rail interpolation edge of a corner, the split-face bookkeeping of a fillet
//! edge, and the fillet vertex / fillet edge constructors and the fillet geometry
//! destructor.
//!
//! Upstream: `source/SMLib/src/SmFilletGeom.cpp`, `source/SMLib/inc/SmFilletGeom.h`.
//!
//! | Rust                                              | Upstream                                     |
//! |---------------------------------------------------|----------------------------------------------|
//! | [`srf_srf_intersection`], [`SrfSrfCurves`], [`SrfSrfOptions`] | `sm_SrfSrfIntersection`          |
//! | [`FilletVertex::new`](super::p1::FilletVertex::new) (in `p1`) | `SmFilletVertex::SmFilletVertex` |
//! | [`FilletBrep::calc_rail_int_rail`]                | `SmFilletVertex::CalcRailIntRail`            |
//! | [`FilletBrep::calc_rail_int_edgeuse`]             | `SmFilletVertex::CalcRailIntEdgeuse`         |
//! | [`FilletBrep::calc_vert_on_cross_section`]        | `SmFilletVertex::CalcVertOnCrossSection`     |
//! | [`FilletBrep::calc_set_back_vert`]                | `SmFilletVertex::CalcSetBackVert`            |
//! | [`FilletBrep::calc_join_section`]                 | the fork's `SmFilletVertex::CalcJoinSection` (`SmBlendCornerJoin.cpp`, 543d25a; WR-697) |
//! | [`FilletBrep::new_fillet_edge`](super::p1::FilletBrep::new_fillet_edge) (in `p1`) | `SmFilletEdge::SmFilletEdge` |
//! | [`FilletBrep::calc_rail_rail_interpolation`]      | `SmFilletEdge::CalcRailRailInterpolation`    |
//! | [`FilletBrep::update_edge_split_face`]            | `SmFilletEdge::UpdateSplitFace`              |
//! | [`FilletBrep::destroy_geom`]                      | `SmFilletGeom::~SmFilletGeom`                |
//!
//! Deviations:
//! - **A blend's net to the intersection's tolerance (#2510).** `SrfSrfIntersection`
//!   evaluates a blend by its definition as its certified net where the net's bound is
//!   within its approximation tolerance
//!   ([`crate::surface::procedural_blend::NetScope::within`]).
//! - **Rail end on the continued surface (WR-630, #960).** `CalcRailIntEdgeuse` evaluates the
//!   rail's end on its base surface with out-of-bounds evaluation on (the fork's
//!   tangency-labs/smlib@3f22e97).
//! - **A closed curve between two points (not upstream).** `SrfSrfIntersection` trims a
//!   closed candidate curve between the parameters its end points drop to, so the curve could
//!   run from the start the other way round: a point at the curve's closure has two parameters
//!   (the interval's ends) and the drop gives either, and the curve between the points may run
//!   through the closure. With `SrfSrfOptions::closed_by_start_direction` (off by default,
//!   set for the cap edge of a rail ending inside the face it rolled onto, whose start
//!   direction is the edge's own tangent there), the curve that leaves the start along the
//!   start direction is taken (`closure_ends`, `through_closure`); one run through the closure
//!   has no UV curve on a surface whose seam it crosses. A curve that leaves the start
//!   against it, traced or a candidate, is passed over (`leaves_along`), and with none left
//!   the intersection fails (`BlendFailure::SrfSrfHeadingNotFollowed`).
//! - **A rail's end at a side edge starts from the round's corner section (#13538).**
//!   `CalcRailIntEdgeuse`'s first guess on the side edge is where the ball's section through
//!   the filleted edge's end there touches it ([`edge_section`]), not upstream's fraction of
//!   the edge's length by the radius. Faces that hold a second ball at the side edge (two
//!   crossing cylinders, whose offsets continued as themselves are whole circles, hold the
//!   round of each crossing) let a guess away from the corner solve onto the other ball. The
//!   rest of upstream's guesses follow unchanged.
//! - **Two rails' crossing on an edge shorter than twice the radius (#17511, not upstream).**
//!   `CalcRailIntRail`'s guess is the midpoint of the points twice the radius along each edge.
//!   Where that length runs past an edge's far end, the point is not on the edge (a quarter
//!   circle of radius 10 rounded 8: it lay by the arc's far end, and the solve found the rails'
//!   other crossing, a ball past the arc's end). There, and only there, the guess is the rails'
//!   crossing to second order at the vertex (`second_order_rail_guess`, `rails_crossing`): each
//!   rail its edge's osculating circle on the edge's own span, offset as far as the round's
//!   centre stands off the side face (the side's offset surface), the crossing ahead of the
//!   vertex along both nearest the rails' points there, else the midpoint of those points. For
//!   straight and circular edges on a plane between square faces it is the crossing itself.
//! - **Rails rolled over a smooth far edge, the whole length (#15767, solidcore-native).** A
//!   solver's rail rolled before the trace onto the face beyond a smooth far edge of its face
//!   (`FilletSolver::rolled_faces`, [`crate::blend::fillet_tangent_rollover`]) has its section's
//!   parameters on that face's surface: a mate on that side is evaluated on it (`rolled_face_of`),
//!   and a join's shared section takes its feet guess there (the vertex's foot taken to the
//!   rolled-over edge's nearest point, dropped on that surface) and its ends from a blend whose
//!   rails roll on the section's two faces. At a corner where only the second blend rolled, the
//!   side it rolled on being the mate's (`CalcRailIntEdgeuse`), the rolled blend places the vertex
//!   and its mate: the other's section there lies on its own face's continuation, which it leaves
//!   only in its own rollover after the trace. Without a rolled rail all of this is upstream's.
//! - **Object model.** As in `blend.fillet_geom_p1` and `_p4`, fillet vertices, edges
//!   and geometries are ids of the [`FilletBrep`] (`self`). `GetFilletSolver()` is an
//!   opaque [`FilletSolverId`] resolved in the caller's `solvers` table (index `id.0`;
//!   an id not in the table is [`SolidError::InvalidInput`], a geometry without a
//!   solver where upstream dereferences it `SM_ERR_NULL_POINTER`). The rollover
//!   caller can supply its single live solver with its actual id through
//!   `calc_rail_int_edgeuse_with_solver_ops`, which retains derived virtual
//!   operations. An unresolved adjacent id is an error.
//!   The target B-rep
//!   (the classifications, original faces and the solvers' edge uses) is `target`. The
//!   constructors `SmFilletVertex(SmFilletCorner*)` and `SmFilletEdge(SmFilletBrep*,
//!   SmFilletCorner*)` were co-ported with `blend.fillet_geom_p1` as
//!   [`FilletVertex::new`](super::p1::FilletVertex::new) and
//!   [`FilletBrep::new_fillet_edge`](super::p1::FilletBrep::new_fillet_edge) and are
//!   this unit's; `~SmFilletGeom` is [`FilletBrep::destroy_geom`] (the geometry stays in
//!   the append-only table; its centre line, owned vertices and offset surfaces go).
//! - **Vertices.** `sm_SrfSrfIntersection` takes the points of its `SmVertex`
//!   arguments (as `sm_TwoPntsIntersection` does in `blend.fillet_geom_p1`); the start
//!   direction is updated in place (`sm_TwoPntsIntersection` unitises it).
//! - **Curves.** The intersection curves keep their kind ([`Curve`]): upstream returns
//!   `SmBSplineCurve*` pointers to whatever `SmBSplineCurve`-derived object the
//!   intersector made (`SmLine`, `SmEllipse`, ...). A best curve of a kind not derived
//!   from `SmBSplineCurve` (upstream: `SM_CAST_PTR` gives `NULL`, then dereferenced) is
//!   `SM_ERR_NULL_POINTER`; such a UV curve counts as missing, as upstream, and is
//!   rebuilt by `DropCurve`.
//! - **Lazy plane NURBs.** `sm_SrfSrfIntersection` starts with `GetNaturalUVDomain` of
//!   both surfaces, which upstream makes a plane's NURB in place (`plan/findings.md`,
//!   "Lazy NURB through `const_cast`"); the port's query does not, so the routine works on
//!   copies of such planes with their NURB made (the caller's surfaces stay as they
//!   were, where upstream's now hold a NURB).
//! - **Tolerances.** `sm_SrfSrfIntersection`'s tolerance is an `SmApproxTol3d`
//!   parameter: the double converts on entry ([`approx_tol3d`]); `DropCurve`'s too.
//!   Face tolerances passed to `DropCurve` / `LiftCurve` convert the same way.
//!   Arc-length walks on edge curves explicitly carry the live edge's zone
//!   tolerance into their length caches, independently of integration accuracy.
//!   A reversed copy has no owner and retains the ordinary system tolerance.
//! - **Face surfaces.** `CalcRailRailInterpolation` drops onto and lifts from
//!   `pFace->GetSurface()`. The port evaluates a copy that keeps the surface's owner and
//!   face tolerance (`clone_for_evaluation`, not the copy constructor), through
//!   `BrepFaceServices` with the face's handle, so the surface cache is the face's
//!   trimmed one made on `target`, as `smsurf_GetSurfaceCache` finds it through
//!   `GetFace()`. The drop and the re-drop share one services object, so the cache the
//!   first one makes is the one the other uses; the lifts read the surface's own
//!   continuities and make no cache (upstream's `LiftCurve` reads the cache's arrays
//!   through `smsurf_GetSurfaceCache`). Upstream keeps that cache in
//!   the surface's slot (`GetCacheObj`; `SM_USE_GLOBAL_CACHE` is off), even when it is
//!   the plain cache `smsurf_GetSurfaceCache` falls back to after a failed trimmed
//!   build: `CacheMakeOrValidate(SM_OC_TRIMSRF, ..)` returns it as it is, and remakes
//!   only the cache of a face marked modified while B-rep editing is off. Nothing
//!   changes `target` between these calls. The lift of the concave `NxN` cubic
//!   interpolant runs after `sm_CreateCubicRailRailInterpolant`, which may change
//!   `target` (its `ComputeUVPoint` can make UV trim curves, and `RailRailIntersect`
//!   runs on it). Upstream's lift reuses the slot's cache there unless the face was
//!   marked modified; the port's makes none. The difftest does not reach that lift (the
//!   shim's corner is `SM_FCR_UNKNOWN`). The copy keeps the
//!   face tolerance it had when copied: a cache build whose `CreateUVTrimCurves` raises
//!   the face's tolerance (even on failure) leaves later calls reading the old value
//!   through the copy, where `pSurf` reads `GetFace()->GetTolerance()` live
//!   (`calc_blending_rail` has the same limit; no case shows it).
//! - **Statuses.** Upstream ignores the status of `IntersectWithPlane`,
//!   `sm_TwoPntsIntersection`, `GetRailIndexOfSideEU` and `SetMinMax` here; so does the
//!   port, except that [`SolidError::Unported`] always propagates. An intersector that
//!   fails part-way may have appended curves upstream; the Rust intersectors return
//!   nothing on failure. `FindParameterAtArcLength`'s `SM_ERR_NOT_CONVERGING` is an
//!   error where upstream `SER`s it.
//! - **Failures (#15034, TE-B-02 #19810).** No check in this file decides a refusal (the
//!   rail/edge-use miss at a corner that is not a blending corner is the vertex's
//!   `NoIntRailEu` status, which the executive reads, not a failure). Each failure keeps
//!   upstream's status and is built where it happens (its origin is that file and line):
//!   a solve that stopped is a not-converged cause with what it knows
//!   ([`BlendFailure::ArcLengthNotConverged`], [`BlendFailure::RailCrossingNotConverged`],
//!   [`BlendFailure::JoinSectionNotConverged`]); an empty search is a not-found cause
//!   ([`BlendFailure::SrfSrfNoCurveThroughEnds`], [`BlendFailure::RailEndMissesSideEdge`]);
//!   what the kernel does not build is an unsupported cause naming it (a curve or surface
//!   kind, a seam-crossing curve with no UV curve, a side edge chain past its bound, a
//!   set-back point with several feet, a Join whose blends roll off its section); a
//!   heading no curve follows is an empty search ([`BlendFailure::SrfSrfHeadingNotFollowed`],
//!   the heading is the kernel's own); an edge without its curve or a face without its
//!   surface ([`BlendFailure::EdgeWithoutCurve`], [`BlendFailure::FaceHasNoSurface`]) and a
//!   split face recorded without its faces are the caller's input. So is every link of
//!   the fillet structures the routines are given, which a caller can build with the
//!   public API (`FilletBrep::new`, `insert_fillet_vertex`, `set_fillet_corner`,
//!   `add_geom`, a caller's solvers and corners): a vertex without its corner
//!   ([`BlendFailure::FilletVertexWithoutCorner`]) and any other link that is not there or
//!   not as its kind needs ([`BlendFailure::FilletSetUpInvalid`], naming the link;
//!   `SM_ERR_NULL_POINTER` for a `NER`, else `SM_ERR`). The rest are reviewed defects
//!   ([`BlendFailure::Defect`]) whose reasons are the `const`s at the end of the file, each
//!   about state the kernel itself just built or computed: an evaluation without the
//!   derivatives it was asked for, an index or element that the code before it guarantees,
//!   a curve the kernel built of a kind it always builds, and a drop or closest point over a
//!   bounded domain, which has a minimum, finding none.
//! - **Fillet corners.** A vertex's or edge's `SmFilletCorner*` is a [`FilletCornerId`]
//!   that the caller's [`CornerLookup`] resolves to the [`FilletCorner`] (unit
//!   `blend.fillet_corner_p1`) whose filleted vertex, set-back distance, flags, fillet
//!   edges, type and `TestRailRailInterpolationCurve` the calculators read.
//! - **Solver virtuals.** The `solvers` table holds base [`FilletSolver`]s, so
//!   `RailRailIntersect` and the virtual `PointOnPlaneSolve` run with the base
//!   `SmFilletSolver` virtuals (`impl FilletSolverOps for FilletSolver`), as for the
//!   base `SmFilletSolver` the difftest shims build; the virtuals' changes to the
//!   solver (offsets, surface extensions) stay in the table. When both solvers of
//!   `RailRailIntersect` / `sm_CreateCubicRailRailInterpolant` are the same table entry
//!   (one object upstream, which Rust cannot lend twice) the second runs on a copy and
//!   its changes are dropped (`plan/findings.md`, "`blend.fillet_solver_p2`: back-edges
//!   into the solver from `blend.fillet_geom`").
//! - **Unported callees.** `SmTemporaryRadiusChange` (`blend.fillet_executive_p2`)
//!   for a solver (its set-up is a no-op for a `NULL` solver). Each stub replaces only
//!   its call; the code behind it is ported.
//! - **Record writes.** `SetPoint` on a fillet vertex and `SetCurve` on a fillet edge
//!   apply the `SM_NO_CHANGE_GEOMETRY` resets directly (as `_p1` and `_p4` do: the
//!   pseudo B-rep has no notification environment); `SetOwner` of the new curve is
//!   implied by the edge holding it.
//! - **Arguments.** Out-params become return values; `iDebugLevel` and debug drawing
//!   are dropped. One input-driven recursion: `calc_rail_int_edgeuse_with_solvers`
//!   continues a missed rail end on the next side edge across a seam's vertex (the fork's
//!   63c7962) by calling itself, at most `SIDE_EDGE_CHAIN_STEPS` (64) deep, then `SM_ERR`
//!   (the fork has no bound; a chain of seam vertices is a few edges long).
//! - **Eased corners (the fork's, tangency-labs/smlib `e74bbd3`, #1376).** At a corner
//!   with an ease (`FilletCorner::ease_dist > 0`, never by default)
//!   `calc_rail_rail_interpolation` takes at least two derivatives and the G2/G3 fairing
//!   branch (`g2_rail_rail_curve`, private) that upstream's `TRUE` condition leaves
//!   unreachable, with the fork's changes to it. A rail running backwards negates only
//!   its odd derivatives (upstream negates all); only the first is read without an ease,
//!   so the pinned results are unchanged.

use std::borrow::Cow;

use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_custom_solver::ConstantRadiusAssistedFs;
use crate::blend::fillet_solver::edge_section::edge_section;
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSolverOps, FilletSurfaceGeneratorType};
use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::p3::PointSolveOptions;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::blend::{BlendFailure, FilletSetUp, SolverFailure};
use crate::error::{EntityRef, Failure, Result, SolidError, SM_ERR, SM_ERR_NULL_POINTER};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p1::line_line_closest_point;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::tol::{approx_tol3d, zone_tol3d, ANG_TOL_RAD};
use crate::geom::vector2d::Point2;
use crate::intersect::surface_drop_lib::p3::DropSolveContext;
use crate::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, PI};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::{EvaluateFlags, OrientType};
use crate::nurbs::crv_fit::p1::ParamMethod;
use crate::nurbs::knots::Side;
use crate::nurbs::math::gamma;
use crate::query::brep::BrepFaceServices;
use crate::surface::normal_cone::{crossing_line_in_ball, CrossingLine};
use crate::surface::plane::p2::stop_on_warning;
use crate::surface::plane::Plane;
use crate::surface::procedural_blend::NetScope;
use crate::surface::surface::p1::SurfaceSolveContext;
use crate::surface::surface::p2::SurfaceIntersection;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::surface_point_solve::p2::with_out_of_bounds_enabled_in_place;
use crate::topology::brep_cache::CacheObject;
use crate::topology::edgeuse::p1::{self as eu1, primary_edgeuse};
use crate::topology::edgeuse::p3::{
    evaluate_binormal, get_or_create_uv_trim_curve, BinormalOptions,
};
use crate::topology::face::p2 as face2;
use crate::topology::vertex;
use crate::topology::vertexuse as vertexuse_unit;
use crate::topology::{edge, Brep, EdgeId, EdgeuseId, FaceId, ObjectHandle, VertexId, VertexuseId};

use super::p1::{
    adjust_uv_on_seam, create_cubic_rail_rail_interpolant, evaluate_mate,
    interpolate_endpts_and_tangents, two_pnts_intersection, CornerLookup, FilStatus, FilletBrep,
    FilletCornerId, FilletCornerType, FilletEdgeType, FilletGeomId, FilletSolverId,
    FilletVertexType, SurfaceHandle,
};
use super::p4::{set_edge_curve, set_vertex_point};
use super::surface_plane_curve::{ends_on_both, surface_plane_curve};

const OP_SRF_SRF: &str = "blend::fillet_geom::p2::srf_srf_intersection";
const OP_RAIL_RAIL: &str = "blend::fillet_geom::p2::FilletBrep::calc_rail_int_rail";
const OP_RAIL_EU: &str = "blend::fillet_geom::p2::FilletBrep::calc_rail_int_edgeuse";
const OP_CROSS: &str = "blend::fillet_geom::p2::FilletBrep::calc_vert_on_cross_section";
const OP_SETBACK: &str = "blend::fillet_geom::p2::FilletBrep::calc_set_back_vert";
const OP_JOIN: &str = "blend::fillet_geom::p2::FilletBrep::calc_join_section";
const OP_INTERP: &str = "blend::fillet_geom::p2::FilletBrep::calc_rail_rail_interpolation";
const OP_SPLIT: &str = "blend::fillet_geom::p2::FilletBrep::update_edge_split_face";
const OP_DESTROY: &str = "blend::fillet_geom::p2::FilletBrep::destroy_geom";

/// The "nothing found" index of `sm_SrfSrfIntersection`'s curve selection.
const NO_CURVE: usize = 9999;

/// `SER(..)` of a call whose failure upstream ignores: [`SolidError::Unported`]
/// propagates, any other error becomes `None`.
fn ignore_status<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
    }
}

/// `SrfIndex` of a rail index (`0` or `1`; anything else is out of bounds upstream).
fn srf(i: usize, op: &'static str) -> Result<SrfIndex> {
    match i {
        0 => Ok(SrfIndex::First),
        1 => Ok(SrfIndex::Second),
        _ => Err(SolidError::invalid_input(op, "rail index past 1")),
    }
}

/// `SrfIndex` of `1 - i` for a rail index `i` (`0` or `1`).
fn other_srf(i: usize, op: &'static str) -> Result<SrfIndex> {
    match i {
        0 => Ok(SrfIndex::Second),
        1 => Ok(SrfIndex::First),
        _ => Err(SolidError::invalid_input(op, "rail index past 1")),
    }
}

/// `SM_RAD2DEG`.
fn rad2deg(rad: f64) -> f64 {
    rad * (180.0 / PI)
}

/// A co-ported `ConstantRadiusAssistedFs` view of a base [`FilletSolver`], for the
/// `blend.fillet_geom_p1` helpers that take one (they read only base members).
fn as_assisted(fs: &FilletSolver) -> ConstantRadiusAssistedFs {
    ConstantRadiusAssistedFs {
        solver: fs.clone(),
        fillet_radii: [0.0; 2],
        plain: false,
    }
}

/// `solvers[i]` for an index from [`solver_index`], for writing (the virtuals change the
/// solver: offsets, surface extensions).
fn solver_at_mut<'s>(
    solvers: &'s mut [FilletSolver],
    i: usize,
    op: &'static str,
) -> Result<&'s mut FilletSolver> {
    solvers
        .get_mut(i)
        .ok_or_else(|| SolidError::invalid_input(op, "fillet solver id not in the solver table"))
}

/// Runs `f` on solvers `i` and `j` of the table, the two `SmFilletSolver*` upstream
/// passes (`this` and `pOtherFillet`). When both are the same table entry (one object
/// upstream, which Rust cannot borrow twice) the second runs on a copy of it: its changes
/// are dropped (`plan/findings.md`, "`blend.fillet_solver_p2`: back-edges into the
/// solver from `blend.fillet_geom`").
fn with_solver_pair<R>(
    solvers: &mut [FilletSolver],
    i: usize,
    j: usize,
    op: &'static str,
    f: impl FnOnce(&mut FilletSolver, &mut FilletSolver) -> Result<R>,
) -> Result<R> {
    if i == j {
        let mut copy = solver_at(solvers, j, op)?.clone();
        return f(solver_at_mut(solvers, i, op)?, &mut copy);
    }
    let missing = || SolidError::invalid_input(op, "fillet solver id not in the solver table");
    let (lo, hi) = if i < j { (i, j) } else { (j, i) };
    let (head, tail) = solvers.split_at_mut(hi);
    let a = head.get_mut(lo).ok_or_else(missing)?;
    let b = tail.first_mut().ok_or_else(missing)?;
    if i < j {
        f(a, b)
    } else {
        f(b, a)
    }
}

/// The index of solver `id` in the caller's table (index `id.0`).
fn solver_index(solvers: &[FilletSolver], id: FilletSolverId, op: &'static str) -> Result<usize> {
    usize::try_from(id.0)
        .ok()
        .filter(|&i| i < solvers.len())
        .ok_or_else(|| SolidError::invalid_input(op, "fillet solver id not in the solver table"))
}

/// `solvers[i]` for an index from [`solver_index`].
fn solver_at<'s>(
    solvers: &'s [FilletSolver],
    i: usize,
    op: &'static str,
) -> Result<&'s FilletSolver> {
    solvers
        .get(i)
        .ok_or_else(|| SolidError::invalid_input(op, "fillet solver id not in the solver table"))
}

// Preserve real solver ids when a caller owns one solver rather than an indexed
// executive table. Missing adjacent solvers are resolver errors, never defaults.
/// The solvers `SmFilletVertex::CalcRailIntRail` reaches through its two geometries:
/// the caller's indexed table, or the live session's retained concrete solvers.
enum RailRailSolvers<'a> {
    Indexed(&'a mut [FilletSolver]),
    Live(Vec<(FilletSolverId, &'a mut dyn FilletSolverOps)>),
}

impl RailRailSolvers<'_> {
    /// The position of solver `id` (table index or live entry).
    fn index(&self, id: FilletSolverId, op: &'static str) -> Result<usize> {
        match self {
            Self::Indexed(solvers) => solver_index(solvers, id, op),
            Self::Live(solvers) => solvers
                .iter()
                .position(|(key, _)| *key == id)
                .ok_or_else(|| SolidError::invalid_input(op, "unresolved fillet solver")),
        }
    }

    fn at(&self, i: usize, op: &'static str) -> Result<&FilletSolver> {
        match self {
            Self::Indexed(solvers) => solver_at(solvers, i, op),
            Self::Live(solvers) => solvers
                .get(i)
                .map(|(_, ops)| ops.solver())
                .ok_or_else(|| SolidError::invalid_input(op, "unresolved fillet solver")),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rail_rail_intersect(
        &mut self,
        k: &Kernel,
        target: &mut Brep,
        i: usize,
        uv_guess: Point2,
        rail_index1: usize,
        j: usize,
        rail_index2: usize,
        tsect1: &mut TsectPnt,
        tsect2: &mut TsectPnt,
        hold_end_radii: bool,
        op: &'static str,
    ) -> Result<bool> {
        match self {
            Self::Indexed(solvers) => rail_rail_intersect(
                k,
                target,
                solvers,
                i,
                uv_guess,
                rail_index1,
                j,
                rail_index2,
                tsect1,
                tsect2,
                hold_end_radii,
                op,
            ),
            Self::Live(solvers) => {
                let missing = || SolidError::invalid_input(op, "unresolved fillet solver pair");
                if i == j {
                    return Err(missing());
                }
                let (lo, hi) = if i < j { (i, j) } else { (j, i) };
                let (head, tail) = solvers.split_at_mut(hi);
                let a = &mut *head.get_mut(lo).ok_or_else(missing)?.1;
                let b = &mut *tail.first_mut().ok_or_else(missing)?.1;
                let (s1, s2) = if i < j { (a, b) } else { (b, a) };
                FilletSolver::rail_rail_intersect(
                    s1,
                    s2,
                    k,
                    Some(target),
                    uv_guess,
                    rail_index1,
                    rail_index2,
                    tsect1,
                    tsect2,
                    hold_end_radii,
                )
            }
        }
    }
}

/// The solvers `SmFilletEdge::CalcRailRailInterpolation` reaches: the caller's
/// indexed table, or a live session's retained solvers.
enum InterpolationSolvers<'a> {
    Indexed(&'a mut [FilletSolver]),
    Context(&'a mut dyn super::p5::FilletEdgeContext),
}

impl InterpolationSolvers<'_> {
    fn solver(&self, id: FilletSolverId, op: &'static str) -> Result<&FilletSolver> {
        match self {
            Self::Indexed(solvers) => solver_at(solvers, solver_index(solvers, id, op)?, op),
            Self::Context(context) => context.solver(id),
        }
    }

    fn cubic<R>(
        &mut self,
        id1: FilletSolverId,
        id2: FilletSolverId,
        op: &'static str,
        f: impl FnOnce(&mut dyn FilletSolverOps, &mut dyn FilletSolverOps) -> Result<R>,
    ) -> Result<R> {
        match self {
            Self::Indexed(solvers) => {
                let i = solver_index(solvers, id1, op)?;
                let j = solver_index(solvers, id2, op)?;
                with_solver_pair(solvers, i, j, op, |a, b| f(a, b))
            }
            Self::Context(context) => {
                let (a, b) = context.solver_pair_ops_mut(id1, id2)?;
                f(a, b)
            }
        }
    }
}

enum RailSolvers<'a> {
    Indexed(&'a mut [FilletSolver]),
    Live(FilletSolverId, &'a mut dyn FilletSolverOps),
    LiveSet(Vec<(FilletSolverId, &'a mut dyn FilletSolverOps)>),
}

impl RailSolvers<'_> {
    fn solver(&self, id: FilletSolverId) -> Result<&FilletSolver> {
        match self {
            Self::Indexed(solvers) => {
                solver_at(solvers, solver_index(solvers, id, OP_RAIL_EU)?, OP_RAIL_EU)
            }
            Self::Live(key, solver) if *key == id => Ok(solver.solver()),
            Self::LiveSet(solvers) => solvers
                .iter()
                .find(|(key, _)| *key == id)
                .map(|(_, solver)| solver.solver())
                .ok_or_else(|| {
                    SolidError::invalid_input(OP_RAIL_EU, "unresolved adjacent fillet solver")
                }),
            Self::Live(..) => Err(SolidError::invalid_input(
                OP_RAIL_EU,
                "unresolved adjacent fillet solver",
            )),
        }
    }

    fn solver_mut(&mut self, id: FilletSolverId) -> Result<&mut FilletSolver> {
        Ok(self.solver_ops_mut(id)?.solver_mut())
    }

    fn solver_ops_mut(&mut self, id: FilletSolverId) -> Result<&mut (dyn FilletSolverOps + '_)> {
        match self {
            Self::Indexed(solvers) => {
                let index = solver_index(solvers, id, OP_RAIL_EU)?;
                let solver = solvers.get_mut(index).ok_or_else(|| {
                    SolidError::invalid_input(
                        OP_RAIL_EU,
                        "fillet solver id not in the solver table",
                    )
                })?;
                Ok(solver)
            }
            Self::Live(key, solver) if *key == id => Ok(&mut **solver),
            Self::LiveSet(solvers) => solvers
                .iter_mut()
                .find(|(key, _)| *key == id)
                .map(|(_, solver)| -> &mut dyn FilletSolverOps { &mut **solver })
                .ok_or_else(|| {
                    SolidError::invalid_input(OP_RAIL_EU, "unresolved adjacent fillet solver")
                }),
            Self::Live(..) => Err(SolidError::invalid_input(
                OP_RAIL_EU,
                "unresolved adjacent fillet solver",
            )),
        }
    }

    fn variable_radius_mut(
        &mut self,
        id: FilletSolverId,
    ) -> Result<Option<&mut crate::blend::fillet_standard_solver::p1::VariableRadiusFs>> {
        Ok(self.solver_ops_mut(id)?.variable_radius_mut())
    }

    fn geom_solver(fb: &FilletBrep, g: FilletGeomId) -> Result<FilletSolverId> {
        fb.geom(g)?.fillet_solver().ok_or_else(|| {
            SolidError::blend_solver(
                OP_RAIL_EU,
                SolverFailure::FilletGeomWithoutSolver { fillet_geom: g },
            )
        })
    }

    fn offset_surface<'a>(
        &'a self,
        fb: &'a FilletBrep,
        g: FilletGeomId,
        i: usize,
    ) -> Result<Option<&'a crate::surface::offset_surface::OffsetSurface>> {
        let geom = fb.geom(g)?;
        let own = geom
            .off_surfs
            .get(i)
            .ok_or_else(|| SolidError::invalid_input(OP_RAIL_EU, "rail index past 1"))?;
        if let Some(surface) = own {
            return Ok(Some(&surface.surface));
        }
        Ok(self
            .solver(Self::geom_solver(fb, g)?)?
            .surface(i)?
            .map(|slot| &slot.surface.surface))
    }
}

/// `SmFilletSolver::GetEdgeuse(i)`, dereferenced (`NULL` is
/// [`SolverFailure::SolverEdgeUseMissing`] at the caller's line: `SM_ERR_NULL_POINTER`).
#[track_caller]
fn solver_edgeuse(fs: &FilletSolver, i: usize, op: &'static str) -> Result<EdgeuseId> {
    match fs.edgeuse(i)? {
        Some(eu) => Ok(eu),
        None => Err(SolidError::blend_solver(
            op,
            SolverFailure::SolverEdgeUseMissing { index: i },
        )),
    }
}

/// `SmFilletSolver::GetSurface(i)`, dereferenced (`NULL` is a [`missing_set_up`] at the
/// caller's line).
#[track_caller]
fn solver_surface<'s>(
    fs: &'s FilletSolver,
    i: usize,
    op: &'static str,
) -> Result<&'s crate::surface::offset_surface::OffsetSurface> {
    match fs.surface(i)? {
        Some(slot) => Ok(&slot.surface.surface),
        None => Err(missing_set_up(op, FilletSetUp::SolverOffsetSurface)),
    }
}

/// A step of the routine `op` that cannot fail unless the kernel broke its own contract: a
/// reviewed [`BlendFailure::Defect`] whose `why` says why (the reasons are the `const`s at
/// the end of this file), keeping upstream's `SM_ERR`; its origin is the call's file and line
/// (guide section 6, rules 2 and 4).
#[track_caller]
fn defect(op: &'static str, why: &'static str) -> SolidError {
    SolidError::blend(op, BlendFailure::Defect { why })
}

/// An edge of the caller's body without its curve: [`BlendFailure::EdgeWithoutCurve`] at the
/// edge, keeping upstream's `SM_ERR_NULL_POINTER` (the body is the caller's, and the
/// executive's check covers only the edges of a smooth sequence).
#[track_caller]
fn edge_without_curve(op: &'static str, e: EdgeId) -> SolidError {
    edge_without_curve_status(op, e, SM_ERR_NULL_POINTER)
}

/// [`edge_without_curve`] where the site's upstream status is `status`.
#[track_caller]
fn edge_without_curve_status(op: &'static str, e: EdgeId, status: i32) -> SolidError {
    Failure::new(op, BlendFailure::EdgeWithoutCurve)
        .with_status(status)
        .at(EntityRef::Edge(e))
        .into_error()
}

/// A face of the caller's body without its surface: [`BlendFailure::FaceHasNoSurface`] at the
/// face, keeping upstream's `SM_ERR_NULL_POINTER` of the `NER` it replaces.
#[track_caller]
fn face_without_surface(op: &'static str, f: FaceId) -> SolidError {
    Failure::new(op, BlendFailure::FaceHasNoSurface)
        .with_status(SM_ERR_NULL_POINTER)
        .at(EntityRef::Face(f))
        .into_error()
}

/// `NER(p)`: an object the kernel built or linked that is not there. A reviewed
/// [`BlendFailure::Defect`] whose `why` names the link, keeping upstream's
/// `SM_ERR_NULL_POINTER`; its origin is the call's file and line (guide section 6, rule 2).
#[track_caller]
fn broken_link(op: &'static str, why: &'static str) -> SolidError {
    Failure::new(op, BlendFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// `NER(p)` of a link of the fillet structures the routine is given (the fillet body, its
/// corners and solvers, which a caller can build with the public API and leave without it):
/// [`BlendFailure::FilletSetUpInvalid`] naming the link, keeping upstream's
/// `SM_ERR_NULL_POINTER` (guide section 6, rule 12).
#[track_caller]
fn missing_set_up(op: &'static str, part: FilletSetUp) -> SolidError {
    Failure::new(op, BlendFailure::FilletSetUpInvalid(part))
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// The edge use `eu` of the target body has no edge (`NER` of `GetEdge()`):
/// [`BlendFailure::EdgeUseWithoutEdge`] at the edge use, status `SM_ERR_NULL_POINTER`.
#[track_caller]
fn edge_use_without_edge(op: &'static str, eu: EdgeuseId) -> SolidError {
    SolidError::blend_at(op, BlendFailure::EdgeUseWithoutEdge, EntityRef::Edgeuse(eu))
}

/// [`missing_set_up`] where upstream returns `SM_ERR`: a set-up the routine is given that
/// is not as its kind needs (a vertex without the geometries its kind joins, an edge
/// outside its corner, ...).
#[track_caller]
fn invalid_set_up(op: &'static str, part: FilletSetUp) -> SolidError {
    Failure::new(op, BlendFailure::FilletSetUpInvalid(part))
        .with_status(SM_ERR)
        .into_error()
}

/// The fillet vertex a vertex calculator runs on has no fillet corner (`NER` of
/// `GetFilletCorner()`): [`BlendFailure::FilletVertexWithoutCorner`], keeping upstream's
/// `SM_ERR_NULL_POINTER`. The calculators are public, and a caller's fillet vertex need not
/// have a corner.
#[track_caller]
fn no_corner(op: &'static str) -> SolidError {
    Failure::new(op, BlendFailure::FilletVertexWithoutCorner)
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// Not upstream (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): the face beyond
/// that rail `rail` of geometry `g`'s solver rolls on, where it rolls over a smooth far edge of
/// its face (its section's parameters there are on that face's surface), else `None`.
fn rolled_face_of(
    fb: &FilletBrep,
    solvers: &RailSolvers<'_>,
    g: FilletGeomId,
    rail: usize,
) -> Result<Option<FaceId>> {
    let solver = solvers.solver(RailSolvers::geom_solver(fb, g)?)?;
    Ok(solver.rolled_faces.get(rail).copied().flatten())
}

/// Whether geometry `g`'s rail on the far side from the side edge use `side_eu` (its mate's
/// side at a Join) rolled over a smooth far edge of its face, the whole length, and its rail on
/// `side_eu`'s face did not (`FilletSolver::rolled_faces`, not upstream). `false` where `g` has
/// no rail on `side_eu`'s face.
fn rolls_only_at_mate(
    fb: &FilletBrep,
    solvers: &RailSolvers<'_>,
    g: FilletGeomId,
    side_eu: EdgeuseId,
    target: &Brep,
) -> Result<bool> {
    let Some(rail) = ignore_status(
        fb.rail_index_of_side_eu_resolved(g, side_eu, target, &|id| solvers.solver(id)),
    )?
    else {
        return Ok(false);
    };
    let other = other_srf(rail, OP_RAIL_EU)?.index();
    Ok(rolled_face_of(fb, solvers, g, rail)?.is_none()
        && rolled_face_of(fb, solvers, g, other)?.is_some())
}

/// Whether either rail of geometry `g`'s solver rolled over a smooth far edge of its face, the
/// whole length (`FilletSolver::rolled_faces`, not upstream).
fn rolls_anywhere(fb: &FilletBrep, solvers: &RailSolvers<'_>, g: FilletGeomId) -> Result<bool> {
    let solver = solvers.solver(RailSolvers::geom_solver(fb, g)?)?;
    Ok(solver.rolled_faces.iter().any(Option::is_some))
}

/// `FindParameterAtArcLength` under `SER`: `SM_ERR_NOT_CONVERGING` is an error.
fn arc_length_param(
    k: &Kernel,
    curve: &Curve,
    start: f64,
    length: f64,
    owner_zone_tol_3d: Option<f64>,
    op: &'static str,
) -> Result<f64> {
    let r =
        curve.find_parameter_at_arc_length_with_zone_tol(k, start, length, owner_zone_tol_3d)?;
    if r.converged {
        Ok(r.end_param)
    } else {
        // the cause's status is `r.status()` of an unconverged search, `SM_ERR_NOT_CONVERGING`
        Err(SolidError::blend(
            op,
            BlendFailure::ArcLengthNotConverged {
                start,
                length,
                stopped_at: r.end_param,
            },
        ))
    }
}

/// The G2/G3 branch of `SmFilletEdge::CalcRailRailInterpolation` as the fork runs it at an
/// eased corner (tangency-labs/smlib `e74bbd3`, #1376): each end's odd derivatives turned
/// to run the way the curve does (the start tangent towards where the two tangent lines
/// meet, the end tangent from there), whatever the rails' orientation flags say; the
/// points are the vertices themselves (`start_pnt`, `end_pnt`, not the rail points
/// dropped there); the tangents and higher derivatives faired
/// (`FairBlendCurveDerivatives`, preventing inflection), then the uniform interpolant of
/// degree 3, 5 or 7 for 1, 2 or 3 derivatives.
///
/// # Errors
///
/// `SM_ERR` for fewer than two entries in a derivative list; those of
/// `FairBlendCurveDerivatives` and `CreateInterpolatingCurve`.
fn g2_rail_rail_curve(
    start_pnt: Point3,
    end_pnt: Point3,
    pnt_vec1: &mut [Vec3],
    pnt_vec2: &mut [Vec3],
    num_derivatives: usize,
) -> Result<BSplineCurve> {
    const OP: &str = OP_INTERP;
    let tangent1 = pnt_vec1
        .get(1)
        .copied()
        .ok_or_else(|| defect(OP, SHORT_DERIVATIVES))?;
    let tangent2 = pnt_vec2
        .get(1)
        .copied()
        .ok_or_else(|| defect(OP, SHORT_DERIVATIVES))?;
    // smgu_LineLineClosestPoint == SM_SUCCESS (any failure skips the turn)
    if let Ok((t1, t2)) = line_line_closest_point(start_pnt, tangent1, end_pnt, tangent2) {
        if t1.abs() > EFF_ZERO && t2.abs() > EFF_ZERO {
            for ii in (1..=num_derivatives).step_by(2) {
                if t1 < 0.0 {
                    if let Some(d) = pnt_vec1.get_mut(ii) {
                        *d = -*d;
                    }
                }
                if t2 > 0.0 {
                    if let Some(d) = pnt_vec2.get_mut(ii) {
                        *d = -*d;
                    }
                }
            }
        }
    }
    // Fair the 3d points & tangents (from the vertices themselves)
    let points = [start_pnt, end_pnt];
    let mut tangents = [
        pnt_vec1
            .get(1)
            .copied()
            .ok_or_else(|| defect(OP, SHORT_DERIVATIVES))?,
        pnt_vec2
            .get(1)
            .copied()
            .ok_or_else(|| defect(OP, SHORT_DERIVATIVES))?,
    ];
    let mut high_derivs = Vec::new();
    for kk in 2..=num_derivatives {
        high_derivs.push(
            pnt_vec1
                .get(kk)
                .copied()
                .ok_or_else(|| defect(OP, SHORT_DERIVATIVES))?,
        );
        high_derivs.push(
            pnt_vec2
                .get(kk)
                .copied()
                .ok_or_else(|| defect(OP, SHORT_DERIVATIVES))?,
        );
    }
    let prevent_inflection = true;
    BSplineCurve::fair_blend_curve_derivatives(
        &points,
        &mut tangents,
        Some(&mut high_derivs),
        prevent_inflection,
    )?;
    // Create 3d curve
    let mut degree = 3;
    if num_derivatives == 2 {
        degree = 5;
    } else if num_derivatives == 3 {
        degree = 7;
    }
    BSplineCurve::create_interpolating_curve(
        ParamMethod::Uniform,
        3,
        degree,
        &points,
        &tangents,
        Some(&high_derivs),
        true,
    )
}

/// The point and the tangent `set_back_dist` along edge `e` from the filleted vertex
/// `cp_vertex` (`CalcSetBackVert`'s walk along the edge's curve).
///
/// It walks the edge's own interval from the end `cp_vertex` marks (`GetStartVertex()` marks
/// the start), a negative length from the other end, as the fork does. Upstream chooses the
/// direction with `GetVertex()` (the primary edge use's vertex) and walks the curve's natural
/// interval, so on an edge whose primary edge use runs against its curve the point is
/// measured from the far vertex (WR-445).
pub(crate) fn set_back_frame(
    k: &Kernel,
    target: &Brep,
    e: EdgeId,
    cp_vertex: Option<VertexId>,
    set_back_dist: f64,
    op: &'static str,
) -> Result<(Point3, Point3)> {
    let (curve, _, end_param) = set_back_params(k, target, e, cp_vertex, set_back_dist, op)?;
    let pv = curve.evaluate(k, end_param, 1, true, true)?;
    let orig = pv
        .first()
        .copied()
        .ok_or_else(|| defect(op, SHORT_EVALUATION))?;
    let normal = pv
        .get(1)
        .copied()
        .ok_or_else(|| defect(op, SHORT_EVALUATION))?;
    Ok((orig, normal))
}

/// Edge `e`'s curve and the parameters on it of [`set_back_frame`]'s walk: where it starts
/// (the end `cp_vertex` marks, as there) and the point `set_back_dist` along it.
pub(crate) fn set_back_params<'b>(
    k: &Kernel,
    target: &'b Brep,
    e: EdgeId,
    cp_vertex: Option<VertexId>,
    set_back_dist: f64,
    op: &'static str,
) -> Result<(&'b Curve, f64, f64)> {
    let curve = edge::curve(target, e)?.ok_or_else(|| edge_without_curve(op, e))?;
    let owner_zone_tol_3d = Some(edge::tolerance(target, e)?);
    let ivl = edge::interval(target, e)?;
    let from_start = edge::start_vertex(target, e)? == cp_vertex;
    let (start, length) = if from_start {
        (ivl.min(), set_back_dist)
    } else {
        (ivl.max(), -set_back_dist)
    };
    let end_param = arc_length_param(k, curve, start, length, owner_zone_tol_3d, op)?;
    Ok((curve, start, end_param))
}

/// `SM_CAST_PTR(SmBSplineCurve, ..)` succeeds: the kinds derived from
/// `SmBSplineCurve` (`SmLine`, `SmEllipse`/`SmCircle`, `SmHyperbola`, `SmParabola`).
const fn is_b_spline_kind(c: &Curve) -> bool {
    matches!(
        c.kind(),
        CurveKind::BSpline(_)
            | CurveKind::Line(_)
            | CurveKind::Ellipse(_)
            | CurveKind::Hyperbola(_)
            | CurveKind::Parabola(_)
    )
}

/// `pSurface->DropCurve(crContext, crDomain, cr3dCurve, crInterval, dApproxTol,
/// rdMaxDist, rdDeviation, rUVCurves)` with the defaults (`bKeepAllDropCurves =
/// TRUE`, no fail record, `bCreatingUVTrimCurve = TRUE`): the UV curves
/// ([`Surface::drop_curve`]; the `SmPlane` override or the general drop).
pub(super) fn surface_drop_curve(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    approx_tol: f64,
) -> Result<Vec<Curve>> {
    let approx_tol = approx_tol3d(k, approx_tol);
    // no failure record (`pOptDropCurveFail = NULL`)
    Ok(surface
        .drop_curve::<NoObject>(k, domain, curve, interval, approx_tol, true, None, true)?
        .uv_curves)
}

/// [`surface_drop_curve`] onto a face's surface under the caller's face context
/// (`context.handle` names the surface in its cache), so the drop's surface cache is
/// the face's trimmed one, as upstream's `smsurf_GetSurfaceCache` finds it through
/// `GetFace()`.
///
/// # Errors
///
/// As [`surface_drop_curve`], and those of the context.
pub(super) fn surface_drop_curve_with_context(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    approx_tol: f64,
    context: DropSolveContext<'_, CacheObject>,
) -> Result<Vec<Curve>> {
    let approx_tol = approx_tol3d(k, approx_tol);
    // no failure record (`pOptDropCurveFail = NULL`)
    Ok(surface
        .drop_curve_with_context::<CacheObject>(
            k, domain, curve, interval, approx_tol, true, None, true, context,
        )?
        .uv_curves)
}

/// The surface as upstream's `GetNaturalUVDomain` leaves it: a plane without a NURB gets
/// it (`GetOrCreateGwNurbPointer`; a NURB that cannot be made stays missing, as
/// upstream's `NULL`).
fn with_lazy_nurb(s: &Surface) -> Cow<'_, Surface> {
    if let SurfaceKind::Plane(p) = s.kind() {
        if p.base().gw_nurb().is_none() {
            let mut copy = s.clone();
            if let SurfaceKind::Plane(pm) = copy.kind_mut() {
                if pm.make_nurb().is_ok() {
                    return Cow::Owned(copy);
                }
            }
        }
    }
    Cow::Borrowed(s)
}

/// The `SmBSplineCurve` (base) of a curve kind derived from it (`SmLine`,
/// `SmEllipse`/`SmCircle`, `SmHyperbola`, `SmParabola`), `None` for the others.
const fn b_spline_base(c: &Curve) -> Option<&BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Line(l) => Some(l.base()),
        CurveKind::Ellipse(e) => Some(e.base()),
        CurveKind::Hyperbola(h) => Some(h.base()),
        CurveKind::Parabola(p) => Some(p.base()),
        _ => None,
    }
}

/// [`b_spline_base`], mutably: the receiver of a non-virtual `SmBSplineCurve` method.
fn b_spline_base_mut(c: &mut Curve) -> Option<&mut BSplineCurve> {
    match c.kind_mut() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Line(l) => Some(l.base_mut()),
        CurveKind::Ellipse(e) => Some(e.base_mut()),
        CurveKind::Hyperbola(h) => Some(h.base_mut()),
        CurveKind::Parabola(p) => Some(p.base_mut()),
        _ => None,
    }
}

/// `pFilSolver1->RailRailIntersect(sUVGuess, lRailIndex1, pFilSolver2, lRailIndex2,
/// rbFound, rTsectPnt1, rTsectPnt2)` on solvers `solver_index1` and `solver_index2` of
/// the table: whether the rails meet; the two intersection points are in/out.
#[allow(clippy::too_many_arguments)]
fn rail_rail_intersect(
    k: &Kernel,
    target: &mut Brep,
    solvers: &mut [FilletSolver],
    solver_index1: usize,
    uv_guess: Point2,
    rail_index1: usize,
    solver_index2: usize,
    rail_index2: usize,
    tsect1: &mut TsectPnt,
    tsect2: &mut TsectPnt,
    hold_end_radii: bool,
    op: &'static str,
) -> Result<bool> {
    with_solver_pair(solvers, solver_index1, solver_index2, op, |s1, s2| {
        FilletSolver::rail_rail_intersect(
            s1,
            s2,
            k,
            Some(target),
            uv_guess,
            rail_index1,
            rail_index2,
            tsect1,
            tsect2,
            hold_end_radii,
        )
    })
}

/// `pFilSolver->PointOnPlaneSolve(crPlaneOrig, crPlaneNormal, crDomain1, crDomain2,
/// crUV1, crUV2, rbFound, rTsectPnt)` (a virtual: the base
/// `SmFilletSolver::PointOnPlaneSolve` of an indexed table's solver, the concrete
/// solver's of a live session): whether a fillet point was found on the plane, and the
/// point (in/out).
#[allow(clippy::too_many_arguments)]
fn point_on_plane_solve(
    k: &Kernel,
    fs: &mut dyn FilletSolverOps,
    target: Option<&mut Brep>,
    plane_orig: Point3,
    plane_normal: Vec3,
    domain1: &Extent2d,
    domain2: &Extent2d,
    uv1: Point2,
    uv2: Point2,
    tsect_pnt: &TsectPnt,
) -> Result<(bool, TsectPnt)> {
    let mut tsect = tsect_pnt.clone();
    let found = FilletSolverOps::point_on_plane_solve(
        fs,
        k,
        target,
        plane_orig,
        plane_normal,
        domain1,
        domain2,
        uv1,
        uv2,
        &mut tsect,
    )?;
    Ok((found, tsect))
}

/// The two contact points in the order of the shared section's vertices. The
/// continued plane is evaluated beyond the cliff rim, and a curved support keeps
/// the same extension that its solver used.
fn cliff_section_feet(
    k: &Kernel,
    fs: &FilletSolver,
    tsect: &TsectPnt,
    first: usize,
) -> Result<[Point3; 2]> {
    let foot = |side: SrfIndex| {
        let offset = solver_surface(fs, side.index(), OP_JOIN)?;
        let uv = tsect.uv_pos(side);
        if let SurfaceKind::Plane(plane) = offset.base_surface().kind() {
            return Ok(plane.evaluate_point(uv));
        }
        let support = offset.clone();
        support.set_offset_distance(0.0);
        support.evaluate_point(k, uv)
    };
    Ok([
        foot(srf(first, OP_JOIN)?)?,
        foot(other_srf(first, OP_JOIN)?)?,
    ])
}

fn check_cliff_section(reference: [Point3; 2], current: [Point3; 2], tol: f64) -> Result<()> {
    if !tol.is_finite()
        || reference.into_iter().zip(current).any(|(a, b)| {
            let gap = a.distance(b);
            !gap.is_finite() || gap > tol
        })
    {
        return Err(SolidError::blend(
            OP_JOIN,
            BlendFailure::NoRoom(crate::blend::fillet_executive::NoRoomReason::JoinOffsetsDiffer),
        ));
    }
    Ok(())
}

/// `SmFilletGeom::RailEdgeuseIntersect(lRailIndex, pSideEU, dGuess, rbFound,
/// rTsectPnt, rdEdgeuseParameter)` (part 6) on geometry `g`'s solver in `solvers`: the
/// edge use parameter when the rail meets the side edge use; the intersection point
/// `tsect` is in/out. While it runs the solver holds `g` as its geometry for surfaces,
/// as upstream's `SetFilletGeomForSurfaces(this)`.
#[allow(clippy::too_many_arguments)]
fn rail_edgeuse_intersect(
    fb: &mut FilletBrep,
    k: &Kernel,
    g: FilletGeomId,
    solvers: &mut RailSolvers<'_>,
    target: &mut Brep,
    temp_radius_solver: Option<FilletSolverId>,
    rail_index: usize,
    side_eu: EdgeuseId,
    guess: f64,
    tsect: &mut TsectPnt,
) -> Result<Option<f64>> {
    use crate::blend::fillet_executive::p2::DirectTemporaryRadiusChange;

    let change = match temp_radius_solver {
        Some(si) => {
            let vrfs = solvers.variable_radius_mut(si)?;
            DirectTemporaryRadiusChange::new(vrfs, target, rail_index, side_eu)?
        }
        None => DirectTemporaryRadiusChange::default(),
    };
    let si = RailSolvers::geom_solver(fb, g)?;
    let result = {
        let fs = solvers.solver_ops_mut(si)?;
        fb.rail_edgeuse_intersect_with_geometry(k, g, fs, target, rail_index, side_eu, guess, tsect)
    };
    if let Some(si) = temp_radius_solver {
        if let Ok(vrfs) = solvers.variable_radius_mut(si) {
            change.revert(vrfs);
        }
    }
    result
}

/// The side edge parameters the search for a rail's end tries, in order. `defaults` are the
/// radius-based guess and then the fixed fractions of the side edge; `corner_guess` is where the
/// corner's section touches the side edge ([`corner_section_on_side_edge`]), tried first. At the
/// corner's own side edge (`at_corner`) it takes the radius-based guess's place. On a side edge
/// further along a chain every default stays after it, so the radius-based guess, which looked
/// for the end there before the corner's section did, is still tried.
fn rail_end_guesses(
    corner_guess: Option<f64>,
    at_corner: bool,
    defaults: &[f64],
) -> impl Iterator<Item = f64> + '_ {
    let skip = usize::from(at_corner && corner_guess.is_some());
    corner_guess
        .into_iter()
        .chain(defaults.iter().skip(skip).copied())
}

/// Where the round of `ops`'s edge touches `side_edge` at the corner `vertex`: the rail
/// `rail_index`'s contact of the ball's section in the plane through the edge's end at
/// `vertex`, normal to the edge there ([`edge_section`]), dropped onto the side edge within
/// `interval`. That section starts on the edge, so it is the ball of this round, not one the
/// faces hold elsewhere. `None` where `vertex` ends neither end of the solver's edge, or the
/// section is not found.
///
/// # Errors
///
/// Those of the topology queries, the section, the evaluation and the drop.
fn corner_section_on_side_edge(
    k: &Kernel,
    ops: &mut dyn FilletSolverOps,
    target: &mut Brep,
    vertex: Option<VertexId>,
    rail_index: usize,
    side_edge: EdgeId,
    interval: &Extent1d,
) -> Result<Option<f64>> {
    let (Some(vertex), Some(edgeuse)) = (vertex, ops.solver().edgeuse(0)?) else {
        return Ok(None);
    };
    let Some(edge_id) = eu1::edge(target, edgeuse)? else {
        return Ok(None);
    };
    let edge_interval = edge::interval(target, edge_id)?;
    let t = if edge::start_vertex(target, edge_id)? == Some(vertex) {
        edge_interval.min()
    } else if edge::end_vertex(target, edge_id)? == Some(vertex) {
        edge_interval.max()
    } else {
        return Ok(None);
    };
    let Some(curve) = edge::curve(target, edge_id)?.cloned() else {
        return Ok(None);
    };
    let (Some(first), Some(second)) = (ops.solver().surface(0)?, ops.solver().surface(1)?) else {
        return Ok(None);
    };
    let domains = [
        first
            .surface
            .base_surface()
            .natural_uv_domain_with_nurb(k)?,
        second
            .surface
            .base_surface()
            .natural_uv_domain_with_nurb(k)?,
    ];
    let Some(section) = edge_section(ops, k, target, &curve, t, &domains)? else {
        return Ok(None);
    };
    let Some(rail) = ops.solver().surface(rail_index)? else {
        return Ok(None);
    };
    let srf = if rail_index == 0 {
        SrfIndex::First
    } else {
        SrfIndex::Second
    };
    let contact = rail
        .surface
        .base_surface()
        .evaluate_point(k, section.uv_pos(srf))?;
    let Some(side_curve) = edge::curve(target, side_edge)? else {
        return Ok(None);
    };
    let sols = side_curve.global_point_solve(
        k,
        interval,
        SolverOperation::Minimize,
        contact,
        ops.solver().approx_tol_3d,
        SolutionRequested::Single,
        &GlobalPointSolveOptions::<ObjectHandle>::default(),
    )?;
    if sols.len() != 1 {
        return Ok(None);
    }
    sols.get(0).map(|sol| sol.start.param(0)).transpose()
}

// ---- sm_SrfSrfIntersection --------------------------------------------------------------

/// The curves of [`srf_srf_intersection`]: the 3D intersection curve (`None` only on a
/// two-point success without a curve) and its UV curves on both surfaces.
#[derive(Debug, Clone, Default)]
pub struct SrfSrfCurves {
    /// `rp3DCurve`.
    pub curve_3d: Option<Curve>,
    /// `rpUVCurve1`: its UV curve on the first surface (`None` also where a curve run through
    /// a closed candidate's closure crosses that surface's seam, not upstream).
    pub uv_curve1: Option<Curve>,
    /// `rpUVCurve2`: its UV curve on the second surface (as `uv_curve1`).
    pub uv_curve2: Option<Curve>,
}

/// The optional arguments of [`srf_srf_intersection`] (upstream defaults: try the
/// two-point intersection, no reference point).
#[derive(Debug, Clone, Copy, Default)]
pub struct SrfSrfOptions {
    /// `bOptSkipTwoPntsIntersection`: skip the cheap `sm_TwoPntsIntersection`.
    pub skip_two_pnts_intersection: bool,
    /// `pOptRefPoint`: with neither end point, the curve nearest to this point wins.
    pub ref_point: Option<Point3>,
    /// Not upstream (solidcore-native, off by default): the start direction is the wanted
    /// curve's own heading at the start (its tangent there, not the chord), and only a curve
    /// that leaves the start along it, within the angle tolerance, is taken. A closed
    /// candidate curve between the two points is taken the way round that does
    /// (`closure_ends`), not as the span between the parameters the points drop to; a
    /// candidate with no way between the points that does is passed over, as is a curve the
    /// surface/plane and two-point tracers give that does not, and with no candidate left the
    /// intersection fails ([`BlendFailure::SrfSrfHeadingNotFollowed`]) rather than build
    /// another curve.
    /// Only for a caller that has that heading: most pass
    /// the chord `end - start`, which heads along a circle's arc only where it is shorter than
    /// a half turn, and upstream's span is then the right curve, also a longer arc.
    pub closed_by_start_direction: bool,
}

/// One candidate curve of the selection loop.
struct Candidate {
    curve_3d: Curve,
    uv_curve1: Option<Curve>,
    uv_curve2: Option<Curve>,
    /// Not upstream: where the curve runs through a closed candidate's closure
    /// (`through_closure`), the closure's point; `None` otherwise.
    through: Option<Point3>,
}

/// `sm_SrfSrfIntersection`: the intersection curve of two surfaces through `start`
/// (and `end`, when given). Planes go to the analytic plane intersector; otherwise,
/// with a start point and direction, `sm_TwoPntsIntersection` is tried (its failure is
/// no answer) and accepted when it has no end point to meet or both its ends lie within
/// `100 * approx_tol` of the points; otherwise the general surface/surface intersector
/// runs. Of the candidate curves through the given points (trimmed to them and
/// oriented from `start` to `end` when both are given) the one closest to
/// `ref_point` (only with neither point), else the longest, wins; missing UV curves
/// are made with `DropCurve`.
///
/// # Errors
///
/// `SM_ERR` when no candidate passes; `SM_ERR_NULL_POINTER` for a best curve of a kind
/// not derived from `SmBSplineCurve`; those of the general intersector, the drops,
/// trims, reversals and evaluations; [`SolidError::Unported`] from
/// `sm_TwoPntsIntersection`.
#[allow(clippy::too_many_arguments)] // one upstream routine
pub fn srf_srf_intersection(
    k: &Kernel,
    start: Option<Point3>,
    end: Option<Point3>,
    opt_dir: Option<&mut Vec3>,
    opt_end_dir: Option<Vec3>,
    surf1: &Surface,
    surf2: &Surface,
    approx_tol: f64,
    angle_tol: f64,
    opts: SrfSrfOptions,
) -> Result<SrfSrfCurves> {
    srf_srf_intersection_with_context(
        k,
        start,
        end,
        opt_dir,
        opt_end_dir,
        surf1,
        surf2,
        approx_tol,
        angle_tol,
        opts,
        None,
    )
}

/// Not upstream (solidcore-native): how a candidate curve of `sm_SrfSrfIntersection` that is
/// closed runs between the given start and end points ([`closure_ends`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum ClosureEnds {
    /// Over the span between these parameters of the start and the end, as upstream trims.
    Span(f64, f64),
    /// From the start's parameter to the end's the other way round, through the curve's
    /// closure ([`through_closure`]).
    Through(f64, f64),
}

/// Not upstream (solidcore-native, `SrfSrfOptions::closed_by_start_direction`): the wanted
/// curve's own heading at its start, `dir`, and the cosine of the intersection's angle
/// tolerance, `cos_tol`.
#[derive(Debug, Clone, Copy)]
struct Heading {
    dir: Vec3,
    cos_tol: f64,
}

impl Heading {
    /// Whether a curve whose tangent at the start is `tangent` leaves the start along the
    /// heading: within the angle tolerance of it, and at a positive dot product whatever the
    /// tolerance. A zero vector, and NaN, do not: every comparison with NaN fails.
    fn along(self, tangent: Vec3) -> bool {
        let d = tangent.dot(self.dir);
        d > 0.0 && d >= self.cos_tol * tangent.length() * self.dir.length()
    }
}

/// Not upstream (solidcore-native, with `SrfSrfOptions::closed_by_start_direction`): how the
/// candidate `curve`, closed over `ivl`, runs between the given start and end `points`
/// (dropped to `params`) when it leaves the start along `heading`, the curve's own tangent
/// there ([`Heading::along`]), not the chord. `sm_SrfSrfIntersection` trims between the two
/// parameters as they came, which can take the curve the other way round, through its far
/// side (a cap edge along a boss's foot circle from inside the boss face to the vertex at the
/// boss's seam: the circle's parameter starts at the seam). Two cases:
/// - a point at the curve's closure has two parameters, the ends of `ivl`, and the drop gives
///   either: of the parameters each point has, the pair whose span leaves the start along
///   the heading is taken ([`ClosureEnds::Span`]);
/// - with neither point at the closure, the curve that leaves the start along the heading may
///   run through the closure: the span's tangent at the start points against it
///   ([`ClosureEnds::Through`]).
///
/// The span as dropped is kept when it leaves along the heading and when the two points
/// coincide (a whole closed curve or a point, which the closure cannot tell apart). `None`
/// when no way between the points leaves along it: the span of a curve that is not closed
/// does not, or no alternative of a closed one does; the candidate is then not the curve the
/// heading belongs to. The two ways round a closed curve leave the start in opposite
/// directions, so where the heading is the curve's own tangent exactly one of them leaves
/// along it, whatever the angle between the points.
///
/// # Errors
/// Those of the closure test and the evaluations; a [`BlendFailure::Defect`] for an
/// evaluation without the tangent.
fn closure_ends(
    k: &Kernel,
    curve: &Curve,
    ivl: &Extent1d,
    points: [Point3; 2],
    params: [f64; 2],
    heading: Heading,
    tol: f64,
) -> Result<Option<ClosureEnds>> {
    let [start, end] = points;
    let dropped = ClosureEnds::Span(params[0], params[1]);
    if start.distance(end) <= tol {
        return Ok(Some(dropped));
    }
    let closed = curve.is_closed(k, ivl, tol)?;
    let closure = curve.evaluate_point(k, ivl.min())?;
    let at_closure = [start, end].map(|p| p.distance(closure) <= tol);
    let choices = |at: bool, param: f64| {
        if at {
            [
                param,
                if param - ivl.min() <= ivl.max() - param {
                    ivl.max()
                } else {
                    ivl.min()
                },
            ]
        } else {
            [param, param]
        }
    };
    // the tangent at `from` towards `to` (`None` for an empty span)
    let towards = |from: f64, to: f64| -> Result<Option<Vec3>> {
        if from == to {
            return Ok(None);
        }
        let pv = curve.evaluate(k, from, 1, to < from, true)?;
        // (an evaluation with one derivative gives it: a defect otherwise)
        let tangent = pv
            .get(1)
            .copied()
            .ok_or_else(|| defect(OP_SRF_SRF, SHORT_EVALUATION))?;
        Ok(Some(if to > from { tangent } else { -tangent }))
    };
    let as_dropped = towards(params[0], params[1])?;
    if as_dropped.is_some_and(|t| heading.along(t)) {
        return Ok(Some(dropped));
    }
    if !closed {
        return Ok(None);
    }
    let [start_at, end_at] = at_closure;
    if start_at || end_at {
        for from in choices(start_at, params[0]) {
            for to in choices(end_at, params[1]) {
                if towards(from, to)?.is_some_and(|t| heading.along(t)) {
                    return Ok(Some(ClosureEnds::Span(from, to)));
                }
            }
        }
        return Ok(None);
    }
    // the way through the closure leaves the start against the span's tangent
    if as_dropped.is_some_and(|t| heading.along(-t)) {
        return Ok(Some(ClosureEnds::Through(params[0], params[1])));
    }
    Ok(None)
}

/// Not upstream (solidcore-native, with `SrfSrfOptions::closed_by_start_direction`): whether
/// `curve` leaves its natural start along `heading` ([`Heading::along`]).
///
/// # Errors
/// Those of the evaluation; a [`BlendFailure::Defect`] for one without the tangent.
fn leaves_along(k: &Kernel, curve: &Curve, heading: Heading) -> Result<bool> {
    let ivl = curve.natural_interval();
    let pv = curve.evaluate(k, ivl.min(), 1, false, true)?;
    // (an evaluation with one derivative gives it: a defect otherwise)
    let tangent = pv
        .get(1)
        .copied()
        .ok_or_else(|| defect(OP_SRF_SRF, SHORT_EVALUATION))?;
    Ok(heading.along(tangent))
}

/// Not upstream (solidcore-native): the curve from parameter `from` to `to` of `curve`, closed
/// over `ivl` (neither at its closure), the way round through the closure: the pieces from the
/// greater of the two to the interval's end and from its start to the lesser, joined as one
/// B-spline curve, and reversed when it runs from the lesser. Its UV curves are made by
/// `DropCurve` as missing ones are; on a surface whose seam it crosses there is none.
///
/// # Errors
/// [`BlendFailure::IntersectionCurveNotBSpline`] for a curve of a kind not derived from
/// `SmBSplineCurve`;
/// those of the trims, the join and the reversal.
fn through_closure(k: &Kernel, curve: &Curve, ivl: &Extent1d, from: f64, to: f64) -> Result<Curve> {
    let (high, low) = if from > to { (from, to) } else { (to, from) };
    let piece = |min: f64, max: f64| -> Result<BSplineCurve> {
        let mut part = curve.clone();
        let mut span = Extent1d::from_min_max(min, max);
        part.trim(k, &mut span, TrimMode::Notify)?;
        // (a trimmed piece keeps the candidate's kind, a B-spline curve's: a defect otherwise)
        b_spline_base(&part).cloned().ok_or_else(|| {
            SolidError::blend(
                OP_SRF_SRF,
                BlendFailure::IntersectionCurveNotBSpline {
                    kind: part.type_string(),
                },
            )
        })
    };
    let mut pieces = [piece(high, ivl.max())?, piece(ivl.min(), low)?];
    let mut joined = Curve::from(BSplineCurve::create_by_joining(&mut pieces, None)?);
    if from < to {
        let span = joined.natural_interval();
        let _ = joined.reverse_parameterization(k, &span)?;
    }
    Ok(joined)
}

/// Not upstream (solidcore-native, with [`through_closure`]): whether `surface` is closed over
/// `domain` in a direction whose seam holds the closure's point `closure` (a curve run through
/// its closure crosses that seam there, so on that surface it has no one UV curve): for each
/// closed direction, the point dropped onto the surface and moved along that direction to the
/// domain's start lies within `tol` of it. `false` for no closure, for a surface closed in
/// neither direction, for a point the surface does not take and where the distance is not a
/// number.
///
/// # Errors
/// Those of the closure tests, the drop (but its not finding the point) and the evaluations.
fn on_closed_seam(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    closure: Option<Point3>,
    tol: f64,
) -> Result<bool> {
    let Some(closure) = closure else {
        return Ok(false);
    };
    let closed_u = surface
        .is_closed(k, domain, SurfParamType::U, None, false)?
        .0;
    let closed_v = surface
        .is_closed(k, domain, SurfParamType::V, None, false)?
        .0;
    if !closed_u && !closed_v {
        return Ok(false);
    }
    let dropped =
        surface.drop_point::<NoObject>(k, closure, domain, &DropPointOptions::default())?;
    if !dropped.success {
        return Ok(false);
    }
    let uv = dropped.result_uv;
    for (closed, seam) in [
        (closed_u, Point2::new(domain.u_interval().min(), uv.y)),
        (closed_v, Point2::new(uv.x, domain.v_interval().min())),
    ] {
        if closed && surface.evaluate_point(k, seam)?.distance(closure) <= tol {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Not upstream (solidcore-native, #21416): the intersection curve `traced` from the start,
/// which stopped at `stop` on the seam of a surface closed there (a trace runs within its
/// surfaces' domains, and a closed surface's domain ends at its seam), run on across that seam
/// to `end`: the rest is traced from `stop` again, taken on the side of the seam that `end`
/// lies nearer (the seam's other side, a short end curve), and the two pieces are joined. On the surface whose seam the curve crosses its UV image is no one curve on the
/// surface's domain: it has none there, as a curve run through a closed candidate's closure
/// has none (`on_closed_seam`); on the other surface its UV curve is the joined curve dropped
/// onto it. `None` where `stop` is not on such a seam, where the rest is not traced, or where it
/// does not run from `stop` to `end`: the curve is then a candidate as it was.
///
/// # Errors
/// Those of the seam tests, the evaluations, the join and the drop.
#[allow(clippy::too_many_arguments)]
fn on_across_closed_seam(
    k: &Kernel,
    traced: &Curve,
    stop: Point3,
    end: Point3,
    end_dir: Option<Vec3>,
    surfaces: [(&Surface, &Extent2d); 2],
    ssi_tol: f64,
    approx_tol: f64,
) -> Result<Option<SrfSrfCurves>> {
    let near = 100.0 * approx_tol;
    if stop.distance(end) < near {
        return Ok(None);
    }
    let [(surf1, domain1), (surf2, domain2)] = surfaces;
    let Some(crossed) = crossed_seam(k, stop, surfaces, approx_tol)? else {
        return Ok(None);
    };
    let ivl = traced.natural_interval();
    let dir = traced
        .evaluate(k, ivl.max(), 1, true, true)?
        .get(1)
        .copied()
        .ok_or_else(|| defect(OP_SRF_SRF, SHORT_EVALUATION))?;
    let Ok(dir) = dir.unitize() else {
        return Ok(None);
    };
    // the two points on each surface; on the surface whose seam the curve crosses, `stop` on
    // the seam's side the rest runs to `end` from (the side nearer `end`)
    let mut uvs = [[Point2::new(0.0, 0.0); 2]; 2];
    for (slot, ((surface, domain), crosses)) in
        uvs.iter_mut().zip(surfaces.into_iter().zip(crossed))
    {
        let mut dropped = [Point2::new(0.0, 0.0); 2];
        for (uv, p) in dropped.iter_mut().zip([stop, end]) {
            let d = surface.drop_point::<NoObject>(k, p, domain, &DropPointOptions::default())?;
            if !d.success || surface.evaluate_point(k, d.result_uv)?.distance(p) > near {
                return Ok(None);
            }
            *uv = d.result_uv;
        }
        if crosses {
            let [at, to] = &mut dropped;
            for (param, along_u) in [(SurfParamType::U, true), (SurfParamType::V, false)] {
                if !surface.is_closed(k, domain, param, None, false)?.0 {
                    continue;
                }
                let (lo, hi) = if along_u {
                    (domain.u_interval().min(), domain.u_interval().max())
                } else {
                    (domain.v_interval().min(), domain.v_interval().max())
                };
                let seam = if along_u {
                    Point2::new(lo, at.y)
                } else {
                    Point2::new(at.x, lo)
                };
                if surface.evaluate_point(k, seam)?.distance(stop) > approx_tol {
                    continue;
                }
                let toward = if along_u { to.x } else { to.y };
                let side = if toward - lo <= hi - toward { lo } else { hi };
                if along_u {
                    at.x = side;
                } else {
                    at.y = side;
                }
            }
        }
        *slot = dropped;
    }
    let [uvs1, uvs2] = &uvs;
    let Some(rest) = ignore_status(surf1.point_based_surface_intersect(
        k,
        domain1,
        surf2,
        domain2,
        uvs1,
        uvs2,
        [false, false],
        [Some(dir), end_dir],
        Some(ssi_tol),
        Some(PI / 6.0),
    ))?
    .and_then(|pi| pi.curve_3d) else {
        return Ok(None);
    };
    let rest_ivl = rest.natural_interval();
    if rest.evaluate_point(k, rest_ivl.min())?.distance(stop) >= near
        || rest.evaluate_point(k, rest_ivl.max())?.distance(end) >= near
    {
        return Ok(None);
    }
    joined_across_seam(k, [traced, &rest], crossed, surfaces, approx_tol)
}

/// Not upstream (solidcore-native, #21416): which of the two `surfaces` is closed with its
/// seam through `at` (`on_closed_seam`), where exactly one is: a curve crossing that seam at
/// `at` has no UV curve on it, and its UV curve on the other surface is whole. `None` where
/// neither or both are.
///
/// # Errors
/// Those of the seam tests.
fn crossed_seam(
    k: &Kernel,
    at: Point3,
    surfaces: [(&Surface, &Extent2d); 2],
    tol: f64,
) -> Result<Option<[bool; 2]>> {
    let [(surf1, domain1), (surf2, domain2)] = surfaces;
    let crossed = [
        on_closed_seam(k, surf1, domain1, Some(at), tol)?,
        on_closed_seam(k, surf2, domain2, Some(at), tol)?,
    ];
    let [on1, on2] = crossed;
    Ok((on1 != on2).then_some(crossed))
}

/// Not upstream (solidcore-native, #21416): whether `pieces`, the first ending where the second
/// starts (within `100 * approx_tol`), are one branch of the intersection of `surfaces` run on
/// through their meeting point, proved from the surfaces' definitions:
/// - the meeting point is a regular point of the intersection: the surfaces' normal cones over
///   a ball holding both pieces' ends there and the intersection's points they stand for (each
///   within `approx_tol` of its piece, so the ball is no wider: its radius gives the turn below
///   no more room than the pieces' own fit) prove the surfaces cross, never tangent, at every
///   point of both in it ([`crossing_line_in_ball`]), so no branch point, touch point or
///   tangent curve lies there (docs/ssi.md §4.4, §8 item 7) and the intersection is one smooth
///   curve through it, along `n1 x n2`;
/// - the pieces' tangents there agree in direction along it: the intersection's tangent line
///   lies within the angle of sine `spread` of the crossing line `L` everywhere in the ball, so
///   oriented along `L` it is one direction `T` there; a tangent `t` with `|t . L| > spread |t|`
///   (rounding included) lies within a quarter turn less that angle of `L` or `-L`, so within
///   a quarter turn of `T` or `-T`. Both pieces' tangents on the side of `T`, or both on the
///   side of `-T`, run the branch one way through the point: neither turns back over the other
///   nor runs off across it;
/// - and neither kinks there: the branch's tangent line turns by less than twice the angle of
///   sine `spread` over the ball, so the two tangents, rounding included, are no further apart
///   than that and the kernel's angle tolerance ([`ANG_TOL_RAD`]) for the pieces' own fit.
///
/// `false` where either is not proved: a surface of a kind without a closed-form normal cone,
/// normals parallel or nearly so at the point (a branch point, where both branches end, or a
/// touch point), a piece turning back, a tangent too far across the branch, a kink, or a tangent
/// not finite. The pieces are then not joined, and the search
/// is left as it was.
///
/// # Errors
/// Those of the evaluations.
fn continues_across(
    k: &Kernel,
    pieces: [&Curve; 2],
    surfaces: [(&Surface, &Extent2d); 2],
    approx_tol: f64,
) -> Result<bool> {
    let [first, second] = pieces;
    let [(surf1, _), (surf2, _)] = surfaces;
    // each piece's point and tangent at the meeting point: the first's end, the second's start
    let mut ends = [(Point3::ZERO, Vec3::ZERO); 2];
    for (slot, (piece, at_end)) in ends.iter_mut().zip([(first, true), (second, false)]) {
        let ivl = piece.natural_interval();
        let t = if at_end { ivl.max() } else { ivl.min() };
        let d = piece.evaluate(k, t, 1, at_end, true)?;
        let (Some(&p), Some(&tangent)) = (d.first(), d.get(1)) else {
            return Ok(false);
        };
        *slot = (p, tangent);
    }
    let [(p, t1), (q, t2)] = ends;
    // the ball holds both ends and the intersection's points within `approx_tol` of them, and no
    // more: its spread is the turn allowed below, so a wider ball would pass a kink (#21959 review)
    let radius = (p.distance(q) + approx_tol) * (1.0 + gamma(2));
    if !radius.is_finite() {
        return Ok(false);
    }
    let Some(CrossingLine { line, spread }) = crossing_line_in_ball(surf1, surf2, p, radius) else {
        return Ok(false);
    };
    // the side of the branch's direction a tangent lies on, where its component along the line
    // is beyond the spread and the rounding of the dot product with a unit vector
    let heading = |t: Vec3| {
        let along = t.dot(line);
        let len = t.length();
        (along.is_finite() && len.is_finite() && along.abs() > (spread + gamma(8)) * len)
            .then_some(along > 0.0)
    };
    if !matches!((heading(t1), heading(t2)), (Some(a), Some(b)) if a == b) {
        return Ok(false);
    }
    // the angle between the tangents, read high by its rounding
    let between = t1.cross(t2).length().atan2(t1.dot(t2)) * (1.0 + gamma(8)) + gamma(8);
    let turn = 2.0 * spread.asin() + ANG_TOL_RAD;
    Ok(between.is_finite() && between <= turn)
}

/// Not upstream (solidcore-native, #21416): the two pieces of an intersection curve on either
/// side of the seam of one of its surfaces (`crossed`, [`crossed_seam`]), the first ending where
/// the second starts, joined as one B-spline curve. On the surface whose seam it crosses its UV
/// image is no one curve on the surface's domain, so it has none there, as a curve run through
/// a closed candidate's closure has none (`on_closed_seam`): the merge cuts the curve at the
/// seam where it meets the face. On the other surface its UV curve is the joined curve dropped
/// onto it. `None` where the pieces are not proved one curve through their meeting point
/// ([`continues_across`]), where a piece is not a B-spline curve (no curve of the
/// intersectors') or the joined curve does not drop onto that other surface: the pieces then
/// stay what they were.
///
/// # Errors
/// Those of the evaluations, the join and the drop.
fn joined_across_seam(
    k: &Kernel,
    pieces: [&Curve; 2],
    crossed: [bool; 2],
    surfaces: [(&Surface, &Extent2d); 2],
    approx_tol: f64,
) -> Result<Option<SrfSrfCurves>> {
    if !continues_across(k, pieces, surfaces, approx_tol)? {
        return Ok(None);
    }
    let [first, second] = pieces;
    let (Some(first), Some(second)) = (b_spline_base(first), b_spline_base(second)) else {
        return Ok(None);
    };
    let mut pieces = [first.clone(), second.clone()];
    // each piece's parameters as long as the piece, so the joined curve runs on through the
    // seam at about the speed it had (pieces of the intersectors' own parameters can differ
    // in speed many times, and the joined curve's UV images would nearly stop there)
    let mut from = 0.0;
    for piece in &mut pieces {
        let curve = Curve::from(piece.clone());
        let length = curve
            .approximate_length(k, &curve.natural_interval(), 5, false)?
            .0;
        if length.is_nan() || length <= 0.0 {
            return Ok(None);
        }
        piece.edit_parameterization(&Extent1d::from_min_max(from, from + length))?;
        from += length;
    }
    let curve_3d = Curve::from(BSplineCurve::create_by_joining(&mut pieces, None)?);
    let joined_ivl = curve_3d.natural_interval();
    let mut uv = [None, None];
    for (slot, ((surface, domain), crosses)) in uv.iter_mut().zip(surfaces.into_iter().zip(crossed))
    {
        if crosses {
            continue;
        }
        let dropped = surface_drop_curve(k, surface, domain, &curve_3d, &joined_ivl, approx_tol)?;
        let Some(c) = dropped.into_iter().next() else {
            return Ok(None);
        };
        *slot = Some(c);
    }
    let [uv_curve1, uv_curve2] = uv;
    Ok(Some(SrfSrfCurves {
        curve_3d: Some(curve_3d),
        uv_curve1,
        uv_curve2,
    }))
}

/// Not upstream (solidcore-native, #21416): of the general intersection's `candidates`, none
/// through both `start` and `end`, two that are one curve cut at the seam of a closed surface
/// (the intersectors trace within their surfaces' domains, which end at a closed surface's
/// seam): one through `start` with an end on that seam where the other, through `end`, has an
/// end too. The piece of each between its point and the seam, joined ([`joined_across_seam`]).
/// `None` where no two candidates are.
///
/// # Errors
/// Those of the drops, trims, reversals, the seam tests and the join.
fn candidates_across_seam(
    k: &Kernel,
    candidates: &[Candidate],
    start: Point3,
    end: Point3,
    surfaces: [(&Surface, &Extent2d); 2],
    approx_tol: f64,
) -> Result<Option<SrfSrfCurves>> {
    let near = 100.0 * approx_tol;
    // the piece of `c` from the point `from` to `to`, oriented that way; `None` where either
    // point is not on it or they drop to one parameter
    let piece = |c: &Curve, from: Point3, to: Point3| -> Result<Option<Curve>> {
        let ivl = c.natural_interval();
        let mut params = [0.0; 2];
        for (t, p) in params.iter_mut().zip([from, to]) {
            let d = c.drop_point(
                k,
                &ivl,
                p,
                None,
                approx_tol,
                None,
                SolverOperation::Intersect,
            )?;
            if !d.success {
                return Ok(None);
            }
            *t = d.parameter;
        }
        let [a, b] = params;
        if a == b {
            return Ok(None);
        }
        let mut part = c.clone();
        let mut span = Extent1d::from_min_max(a.min(b), a.max(b));
        part.trim(k, &mut span, TrimMode::Notify)?;
        if a > b {
            let _ = part.reverse_parameterization(k, &span)?;
        }
        Ok(Some(part))
    };
    for (i, a) in candidates.iter().enumerate() {
        let a_ivl = a.curve_3d.natural_interval();
        for t in [a_ivl.min(), a_ivl.max()] {
            let seam = a.curve_3d.evaluate_point(k, t)?;
            if seam.distance(start) < near || seam.distance(end) < near {
                continue;
            }
            let Some(crossed) = crossed_seam(k, seam, surfaces, approx_tol)? else {
                continue;
            };
            for (j, b) in candidates.iter().enumerate() {
                if i == j {
                    continue;
                }
                let b_ivl = b.curve_3d.natural_interval();
                let mut meets = false;
                for u in [b_ivl.min(), b_ivl.max()] {
                    meets |= b.curve_3d.evaluate_point(k, u)?.distance(seam) < near;
                }
                if !meets {
                    continue;
                }
                let (Some(first), Some(second)) = (
                    piece(&a.curve_3d, start, seam)?,
                    piece(&b.curve_3d, seam, end)?,
                ) else {
                    continue;
                };
                if let Some(joined) =
                    joined_across_seam(k, [&first, &second], crossed, surfaces, approx_tol)?
                {
                    return Ok(Some(joined));
                }
            }
        }
    }
    Ok(None)
}

/// The face-aware surface cache context of a [`srf_srf_intersection_with_context`]: the
/// caller's context and the two surfaces' handles in it (a face's surface is named
/// by its face, so its trimmed surface cache is made with the face's boundary, as
/// upstream's `smsurf_GetSurfaceCache` of a face-owned surface does).
pub type SrfSrfContext<'a> = (
    &'a mut dyn SurfaceSolveContext<CacheObject>,
    [Option<CacheObject>; 2],
);

/// [`srf_srf_intersection`] whose general surface/surface intersection runs through the
/// caller's face-aware cache `context` (`None`: as [`srf_srf_intersection`]).
///
/// # Errors
///
/// As [`srf_srf_intersection`], and those of the context's cache preparation.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one upstream routine
pub fn srf_srf_intersection_with_context(
    k: &Kernel,
    start: Option<Point3>,
    end: Option<Point3>,
    opt_dir: Option<&mut Vec3>,
    opt_end_dir: Option<Vec3>,
    surf1: &Surface,
    surf2: &Surface,
    approx_tol: f64,
    angle_tol: f64,
    opts: SrfSrfOptions,
    context: Option<SrfSrfContext<'_>>,
) -> Result<SrfSrfCurves> {
    // SmApproxTol3d dApproxTol: the double converts at the call
    let approx_tol = approx_tol3d(k, approx_tol);
    // the intersection is to `approx_tol`: a blend by its definition whose net's bound is
    // within it evaluates as its net (#2510)
    let _scope = NetScope::within(k, approx_tol);
    // the direction the curve leaves its start in, as given (the two-point intersector
    // below unitises it in place)
    let start_dir = opt_dir.as_deref().copied();
    // Not upstream: the wanted curve's own heading at the start, with
    // `closed_by_start_direction`
    let wanted_heading = start_dir
        .filter(|_| opts.closed_by_start_direction)
        .map(|dir| Heading {
            dir,
            cos_tol: angle_tol.cos(),
        });

    // surface and surf/surf intersection locals
    let mut curves_3d: Vec<Curve> = Vec::new();
    let mut surface1_uv_curves: Vec<Option<Curve>> = Vec::new();
    let mut surface2_uv_curves: Vec<Option<Curve>> = Vec::new();

    let domain1 = surf1.natural_uv_domain_with_nurb(k)?;
    let domain2 = surf2.natural_uv_domain_with_nurb(k)?;
    // GetNaturalUVDomain makes a plane's NURB in place (through a const cast) and keeps
    // it; the port's query does not, so the routine continues on such copies
    let surf1 = &*with_lazy_nurb(surf1);
    let surf2 = &*with_lazy_nurb(surf2);

    let ssi_tol = approx_tol;

    // find intersection endPoints from naturalBoundary/Surface intersections
    let use_surface_edges = [true, true];

    // See if we have analytic cases (their status is ignored)
    let push = |r: Option<(bool, SurfaceIntersection)>,
                swap: bool,
                c3: &mut Vec<Curve>,
                u1: &mut Vec<Option<Curve>>,
                u2: &mut Vec<Option<Curve>>| {
        // A surface of extrusion lying in the plane answers its coincident patch's
        // boundary; that is no section curve, so this search goes on to the paths
        // below as it did when the special case asked for more.
        let r = r.filter(|(_, xs)| {
            !xs.curve_types
                .contains(&crate::surface::surface::p2::TsectCurveType::RegionBoundary)
        });
        if let Some((_, xs)) = r {
            c3.extend(xs.curves_3d);
            if swap {
                u2.extend(xs.surface1_uv_curves);
                u1.extend(xs.surface2_uv_curves);
            } else {
                u1.extend(xs.surface1_uv_curves);
                u2.extend(xs.surface2_uv_curves);
            }
        }
    };
    if let SurfaceKind::Plane(plane) = surf1.kind() {
        // get planar intersection
        // (the plane override's warning was its status, so it drops the result too)
        let r = ignore_status(
            surf2
                .intersect_with_plane(
                    k,
                    &domain2,
                    plane,
                    &domain1,
                    use_surface_edges,
                    Some(ssi_tol),
                    Some(angle_tol),
                )
                .and_then(stop_on_warning),
        )?;
        push(
            r,
            true,
            &mut curves_3d,
            &mut surface1_uv_curves,
            &mut surface2_uv_curves,
        );
    } else if let SurfaceKind::Plane(plane) = surf2.kind() {
        // get planar intersection
        // (the plane override's warning was its status, so it drops the result too)
        let r = ignore_status(
            surf1
                .intersect_with_plane(
                    k,
                    &domain1,
                    plane,
                    &domain2,
                    use_surface_edges,
                    Some(ssi_tol),
                    Some(angle_tol),
                )
                .and_then(stop_on_warning),
        )?;
        push(
            r,
            false,
            &mut curves_3d,
            &mut surface1_uv_curves,
            &mut surface2_uv_curves,
        );
    }

    // The fork's sm_SurfacePlaneCurve: a surface and a plane between two given points that lie
    // on both, where they cross: the plane goes on as itself past the face it was made for, so
    // its domain does not end the curve. A flat surface meets the plane in a line, which the
    // analytic intersector gives exactly. Points off a surface, and an end where the surface
    // touches the plane, are met by the intersectors below. A round of touch circles needs it
    // (#2704): its end section leaves the end face's plane, so its side curve is this one.
    if curves_3d.is_empty() {
        let plane_first = matches!(surf1.kind(), SurfaceKind::Plane(_));
        if let (Some(start_pnt), Some(end_pnt), true) = (
            start,
            end,
            plane_first != matches!(surf2.kind(), SurfaceKind::Plane(_)),
        ) {
            let (surface, plane) = if plane_first {
                (surf2, surf1)
            } else {
                (surf1, surf2)
            };
            // a curve between two points: ends that coincide are no such curve
            if start_pnt.distance(end_pnt) > approx_tol
                && !surface.is_planar(k, approx_tol)?.is_planar
                && ends_on_both(k, [surf1, surf2], [start_pnt, end_pnt], approx_tol)?
            {
                let c = surface_plane_curve(
                    k,
                    surface,
                    plane,
                    start_pnt,
                    end_pnt,
                    opt_dir.as_deref().copied(),
                    approx_tol,
                    angle_tol,
                )?;
                // Not upstream: with `closed_by_start_direction`, a curve that does not leave
                // the start along its heading is passed over (the intersectors below go on)
                let headed = match wanted_heading {
                    Some(h) => leaves_along(k, &c.curve_3d, h)?,
                    None => true,
                };
                if headed {
                    let (uv1, uv2) = if plane_first {
                        (c.uv_plane, c.uv_surface)
                    } else {
                        (c.uv_surface, c.uv_plane)
                    };
                    return Ok(SrfSrfCurves {
                        curve_3d: Some(c.curve_3d),
                        uv_curve1: Some(uv1),
                        uv_curve2: Some(uv2),
                    });
                }
            }
        }
    }

    // Use two-pts intersector (pOptDir is a required argument of it).
    if curves_3d.is_empty() && !opts.skip_two_pnts_intersection {
        if let (Some(start_pnt), Some(dir)) = (start, opt_dir) {
            // A point that drops onto neither surface is no curve here: the call answers
            // `None`, not an error. Upstream also ignores the call's other statuses (an
            // error here just means that it couldn't find a solution), so the intersector's
            // own failures still mean "no curve" and the search goes on below.
            let r = ignore_status(two_pnts_intersection(
                k,
                start_pnt,
                end,
                dir,
                opt_end_dir,
                surf1,
                surf2,
                ssi_tol,
            ))?
            .flatten();
            if let Some(pi) = r {
                // Not upstream: with `closed_by_start_direction`, a curve that does not leave
                // the start along its heading is passed over
                let mut traced = pi.curve_3d;
                if let (Some(c), Some(h)) = (traced.as_ref(), wanted_heading) {
                    if !leaves_along(k, c, h)? {
                        traced = None;
                    }
                }
                if let Some(curve_3d) = traced {
                    let uv1 = pi.surface1_uv_curve;
                    let uv2 = pi.surface2_uv_curve;

                    // skip cases where pEndV was not given
                    let Some(end_pnt) = end else {
                        return Ok(SrfSrfCurves {
                            curve_3d: Some(curve_3d),
                            uv_curve1: uv1,
                            uv_curve2: uv2,
                        });
                    };

                    // Check start and end points vs. given points.
                    let trim_ivl = curve_3d.natural_interval();
                    let p1 = curve_3d.evaluate_point(k, trim_ivl.min())?;
                    let p2 = curve_3d.evaluate_point(k, trim_ivl.max())?;
                    if p1.distance(start_pnt) < 100.0 * approx_tol
                        && p2.distance(end_pnt) < 100.0 * approx_tol
                    {
                        return Ok(SrfSrfCurves {
                            curve_3d: Some(curve_3d),
                            uv_curve1: uv1,
                            uv_curve2: uv2,
                        });
                    }

                    // Not upstream (#21416): a trace from the start stops where it reaches the
                    // seam of a closed surface, its domain's side; the curve runs on across the
                    // seam, traced from the seam's other side to the end
                    if p1.distance(start_pnt) < 100.0 * approx_tol {
                        // (a failure of the continuation leaves the trace a candidate)
                        if let Some(across) = ignore_status(on_across_closed_seam(
                            k,
                            &curve_3d,
                            p2,
                            end_pnt,
                            opt_end_dir,
                            [(surf1, &domain1), (surf2, &domain2)],
                            ssi_tol,
                            approx_tol,
                        ))?
                        .flatten()
                        {
                            return Ok(across);
                        }
                    }

                    // Got a result, but apparently not the whole curve
                    curves_3d.push(curve_3d);
                    surface1_uv_curves.push(uv1);
                    surface2_uv_curves.push(uv2);
                }
            }
        }
    }

    // general surf/surf intersector
    if curves_3d.is_empty() {
        let mut context = context;
        let xs = if let Some((context, handles)) = context.as_mut() {
            surf1.global_surface_intersect_with_context(
                k,
                *handles,
                &mut **context,
                &domain1,
                surf2,
                &domain2,
                [true, true],
                Some(ssi_tol),
                Some(angle_tol),
            )?
        } else {
            surf1.global_surface_intersect(
                k,
                &domain1,
                surf2,
                &domain2,
                [true, true],
                Some(ssi_tol),
                Some(angle_tol),
            )?
        };
        // The plane / extrusion dispatch answers an extrusion lying in the plane with
        // its coincident region's boundary, no section curve (as for the special cases
        // above): the general intersection, as that dispatch ran when the special case
        // asked for more (with its tolerance: the plane's dispatch passes it on, the
        // extrusion's converts it).
        let plane_extrusion = match (surf1.kind(), surf2.kind()) {
            (SurfaceKind::Plane(_), SurfaceKind::SurfOfExtrusion(_)) => Some(Some(ssi_tol)),
            (SurfaceKind::SurfOfExtrusion(_), SurfaceKind::Plane(_)) => {
                Some(Some(approx_tol3d(k, ssi_tol)))
            }
            _ => None,
        };
        let region = xs
            .curve_types
            .contains(&crate::surface::surface::p2::TsectCurveType::RegionBoundary);
        let xs = match plane_extrusion {
            Some(tol) if region => crate::surface::surface::p2::base_global_surface_intersect_in(
                k,
                surf1,
                &domain1,
                surf2,
                &domain2,
                [true, true],
                tol,
                Some(angle_tol),
                context.map(|(solver, handles)| {
                    crate::surface::surface::p1::SurfaceIntersectContext { solver, handles }
                }),
            )?,
            _ => xs,
        };
        curves_3d.extend(xs.curves_3d);
        surface1_uv_curves.extend(xs.surface1_uv_curves);
        surface2_uv_curves.extend(xs.surface2_uv_curves);
    }

    // Check every surf/surf intersection solution
    let mut best_dist = NO_CURVE;
    let mut best_len = NO_CURVE;
    let mut min_dist = BIG_DOUBLE;
    let mut max_len = 0.0;
    let mut candidates: Vec<Candidate> = Vec::new();
    // Not upstream: a candidate through the points was passed over for its heading
    let mut headed_away = false;
    let mut uv1_iter = surface1_uv_curves.into_iter();
    let mut uv2_iter = surface2_uv_curves.into_iter();
    for (crv_index, mut new_3d_curve) in curves_3d.into_iter().enumerate() {
        let mut new_uv_curve1 = uv1_iter.next().flatten();
        let mut new_uv_curve2 = uv2_iter.next().flatten();
        let mut trim_ivl = new_3d_curve.natural_interval();
        let mut param1 = 0.0;
        let mut param2 = 0.0;
        let mut reverse_curves = false;
        let mut success = true;

        // drop StartPoint to curve within dApproxTol
        if let Some(start_pnt) = start {
            let d = new_3d_curve.drop_point(
                k,
                &trim_ivl,
                start_pnt,
                None,
                approx_tol,
                None,
                SolverOperation::Intersect,
            )?;
            success = d.success;
            param1 = d.parameter;
        }

        // drop EndPoint to curve within dApproxTol
        if success {
            if let Some(end_pnt) = end {
                let d = new_3d_curve.drop_point(
                    k,
                    &trim_ivl,
                    end_pnt,
                    None,
                    approx_tol,
                    None,
                    SolverOperation::Intersect,
                )?;
                success = d.success;
                param2 = d.parameter;
            }
        }

        // skip curves that are not close to endPoints (deleted with the others)
        if !success {
            candidates.push(Candidate {
                curve_3d: new_3d_curve,
                uv_curve1: new_uv_curve1,
                uv_curve2: new_uv_curve2,
                through: None,
            });
            continue;
        }

        // Not upstream: on a closed curve, the curve between the points that leaves the start
        // in the start direction, when the caller asks for it (`closure_ends`); a candidate
        // with no way between them that does is passed over as one off the points is
        let mut through = None;
        if let (Some(start_pnt), Some(end_pnt), Some(heading)) = (start, end, wanted_heading) {
            match closure_ends(
                k,
                &new_3d_curve,
                &trim_ivl,
                [start_pnt, end_pnt],
                [param1, param2],
                heading,
                approx_tol,
            )? {
                None => {
                    headed_away = true;
                    candidates.push(Candidate {
                        curve_3d: new_3d_curve,
                        uv_curve1: new_uv_curve1,
                        uv_curve2: new_uv_curve2,
                        through: None,
                    });
                    continue;
                }
                Some(ClosureEnds::Span(from, to)) => (param1, param2) = (from, to),
                Some(ClosureEnds::Through(from, to)) => {
                    // its UV curves are made by `DropCurve` below, as missing ones are
                    let closure = new_3d_curve.evaluate_point(k, trim_ivl.min())?;
                    new_3d_curve = through_closure(k, &new_3d_curve, &trim_ivl, from, to)?;
                    new_uv_curve1 = None;
                    new_uv_curve2 = None;
                    trim_ivl = new_3d_curve.natural_interval();
                    through = Some(closure);
                }
            }
        }

        // when given an endPoint - trim intersection curves to endPoints
        if end.is_some() && through.is_none() {
            // see if intersection curve needs to be reversed
            if param1 > param2 {
                reverse_curves = true;
                std::mem::swap(&mut param1, &mut param2);
            }
            // SetMinMax's status is ignored upstream
            if trim_ivl.set_min_max(param1, param2).is_err() {
                // the interval stays as it was
            }
            new_3d_curve.trim(k, &mut trim_ivl, TrimMode::Notify)?;
            if let Some(c) = new_uv_curve1.as_mut() {
                c.trim(k, &mut trim_ivl, TrimMode::Notify)?;
            }
            if let Some(c) = new_uv_curve2.as_mut() {
                c.trim(k, &mut trim_ivl, TrimMode::Notify)?;
            }
        }

        // when reversing curves - reverse curve parameterizations
        if reverse_curves {
            trim_ivl = new_3d_curve.reverse_parameterization(k, &trim_ivl)?;
            if let Some(c) = new_uv_curve1.as_mut() {
                trim_ivl = c.reverse_parameterization(k, &trim_ivl)?;
            }
            if let Some(c) = new_uv_curve2.as_mut() {
                trim_ivl = c.reverse_parameterization(k, &trim_ivl)?;
            }
        }

        // If no pStartV or pEndV, see if we were given a reference point.
        if start.is_none() && end.is_none() {
            if let Some(ref_point) = opts.ref_point {
                let d = new_3d_curve.drop_point(
                    k,
                    &trim_ivl,
                    ref_point,
                    None,
                    approx_tol,
                    None,
                    SolverOperation::Minimize,
                )?;
                // save the best curve seen - best = closest to reference point
                if d.success && d.distance < min_dist {
                    min_dist = d.distance;
                    best_dist = crv_index;
                }
            }
        }

        // save the best curve seen - best = longest that is close to points
        let crv_len = new_3d_curve
            .approximate_length(k, &new_3d_curve.natural_interval(), 5, false)?
            .0;
        if crv_len > max_len {
            max_len = crv_len;
            best_len = crv_index;
        }
        candidates.push(Candidate {
            curve_3d: new_3d_curve,
            uv_curve1: new_uv_curve1,
            uv_curve2: new_uv_curve2,
            through,
        });
    }

    // error: didn't find a curve within tol of endPoints
    if best_dist == NO_CURVE && best_len == NO_CURVE {
        // Not upstream (#21416): one curve the intersectors cut at a closed surface's seam
        if let (Some(start_pnt), Some(end_pnt), None) = (start, end, wanted_heading) {
            // (a failure of the pairing leaves the search empty, as it was)
            if let Some(across) = ignore_status(candidates_across_seam(
                k,
                &candidates,
                start_pnt,
                end_pnt,
                [(surf1, &domain1), (surf2, &domain2)],
                approx_tol,
            ))?
            .flatten()
            {
                return Ok(across);
            }
        }
        // Not upstream: a curve through the points that heads away from the wanted heading is
        // not the curve the caller asked for, and no other is built in its place
        if headed_away {
            return Err(SolidError::blend(
                OP_SRF_SRF,
                BlendFailure::SrfSrfHeadingNotFollowed,
            ));
        }
        return Err(SolidError::blend(
            OP_SRF_SRF,
            BlendFailure::SrfSrfNoCurveThroughEnds {
                candidates: candidates.len(),
            },
        ));
    }

    // arrive here after finding a longest curve within tol of given endPoints; the
    // other solution curves are dropped
    let best_index = if best_dist < NO_CURVE {
        best_dist
    } else {
        best_len
    };
    let best = candidates
        .into_iter()
        .nth(best_index)
        .ok_or_else(|| defect(OP_SRF_SRF, CANDIDATE_INDEX))?;

    // set output (SM_CAST_PTR to SmBSplineCurve)
    if !is_b_spline_kind(&best.curve_3d) {
        return Err(Failure::new(
            OP_SRF_SRF,
            BlendFailure::IntersectionCurveNotBSpline {
                kind: best.curve_3d.type_string(),
            },
        )
        .with_status(SM_ERR_NULL_POINTER)
        .into_error());
    }
    let curve_3d = best.curve_3d;
    let mut uv_curve1 = best.uv_curve1.filter(is_b_spline_kind);
    let mut uv_curve2 = best.uv_curve2.filter(is_b_spline_kind);
    let trim_ivl = curve_3d.natural_interval();
    // Not upstream: a curve through a closed candidate's closure crosses the seam of a closed
    // surface it lies on (the closure of a plane/cylinder circle is the cylinder's seam), where
    // its UV image is no one curve on the surface's domain: on that surface it has none. Only
    // there: on a surface not closed across the closure's point, a missing UV curve is the
    // drop's failure, as for any other curve (`on_closed_seam`)
    let through = best.through;

    // generate surf1 UVTrimCurve if it's missing (some intersectors don't make them)
    if uv_curve1.is_none() {
        let uv_curves = surface_drop_curve(k, surf1, &domain1, &curve_3d, &trim_ivl, approx_tol)?;
        let first = uv_curves.into_iter().next();
        if first.is_none() && !on_closed_seam(k, surf1, &domain1, through, approx_tol)? {
            return Err(SolidError::blend(
                OP_SRF_SRF,
                BlendFailure::IntersectionCurveHasNoUvCurve { on_second: false },
            ));
        }
        uv_curve1 = first;
    }

    // generate surf2 UVTrimCurve if it's missing
    if uv_curve2.is_none() {
        let uv_curves = surface_drop_curve(k, surf2, &domain2, &curve_3d, &trim_ivl, approx_tol)?;
        let first = uv_curves.into_iter().next();
        if first.is_none() && !on_closed_seam(k, surf2, &domain2, through, approx_tol)? {
            return Err(SolidError::blend(
                OP_SRF_SRF,
                BlendFailure::IntersectionCurveHasNoUvCurve { on_second: true },
            ));
        }
        uv_curve2 = first;
    }

    // all done
    Ok(SrfSrfCurves {
        curve_3d: Some(curve_3d),
        uv_curve1,
        uv_curve2,
    })
}

// ---- fillet vertex calculators -------------------------------------------------------------

/// `(SmFace*)GetPointClassObject()`: the classification object as a face (`None` for
/// `NULL`; another kind of object is [`SolidError::InvalidInput`], upstream casts
/// blindly).
fn as_face(obj: Option<EntityRef>, op: &'static str) -> Result<Option<FaceId>> {
    match obj {
        None => Ok(None),
        Some(EntityRef::Face(f)) => Ok(Some(f)),
        Some(_) => Err(SolidError::invalid_input(
            op,
            "classification object is not a face",
        )),
    }
}

/// `(SmEdgeuse*)GetPointClassObject()`, as [`as_face`].
fn as_edgeuse(obj: Option<EntityRef>, op: &'static str) -> Result<Option<EdgeuseId>> {
    match obj {
        None => Ok(None),
        Some(EntityRef::Edgeuse(eu)) => Ok(Some(eu)),
        Some(_) => Err(SolidError::invalid_input(
            op,
            "classification object is not an edge use",
        )),
    }
}

/// One rail of a corner, to second order at the common vertex: where the filleted edge leaves the
/// vertex (`point`), its unit direction leaving it (`leaving`), the unit direction in the common
/// face from the edge to the rail (`binormal`), the edge's curvature in the face along it
/// (`curvature`, positive where the edge turns towards the rail) and the rail's distance from the
/// edge (`offset`).
#[derive(Debug, Clone, Copy)]
struct RailAtVertex {
    point: Point3,
    leaving: Vec3,
    binormal: Vec3,
    curvature: f64,
    offset: f64,
}

/// A rail's osculating circle at the vertex in a plane frame, as the generalised circle
/// `k |X - q|^2 - 2 (X - q) . b = 0`: its point at the vertex `q`, the unit normal `b` towards
/// its centre side, its curvature `k` (`0` for a line) and the unit direction it leaves the
/// vertex along (`leaving`).
#[derive(Debug, Clone, Copy)]
struct PlaneRail {
    q: Point2,
    b: Point2,
    k: f64,
    leaving: Point2,
}

impl PlaneRail {
    fn value(self, x: Point2) -> f64 {
        let d = x - self.q;
        self.k * d.dot(d) - 2.0 * d.dot(self.b)
    }
}

/// Where the rails of two rounds meeting at a vertex cross, to second order: each rail is its
/// edge's osculating circle (a line where the edge is straight) offset into the face, the
/// round's contact line with the face for a straight or circular edge on a plane exactly. Of
/// their crossings (two for a circle) the one ahead of the vertex along both rails and nearest
/// their points at the vertex is taken. The other is a ball past an edge's end (#17511) or,
/// where the edges run on tangent at the vertex, behind both. Where no crossing lies ahead
/// along both (a concave corner, whose rails cross behind the vertex, or rails that run on
/// into each other, as round a rim split into arcs) the guess is the midpoint of the rails'
/// points at the vertex, which those rails meet at or next to.
fn rails_crossing(rails: [RailAtVertex; 2]) -> Point3 {
    let [r0, r1] = rails;
    let at_vertex = |r: RailAtVertex| r.point + r.binormal * r.offset;
    let mid = (at_vertex(r0) + at_vertex(r1)) * 0.5;
    let origin = r0.point;
    let (eu, ev) = (r0.leaving, r0.binormal);
    let to2 = |w: Vec3| Point2::new(w.dot(eu), w.dot(ev));
    let plane = |r: RailAtVertex| -> Option<PlaneRail> {
        // the rail's curvature: the edge's over the offset's shrink, its direction reversed
        // where the offset passes the edge's centre (none where it shrinks to a point)
        let shrink = 1.0 - r.curvature * r.offset;
        if shrink == 0.0 {
            return None;
        }
        let leaving = to2(r.leaving) * shrink.signum();
        Some(PlaneRail {
            q: to2(at_vertex(r) - origin),
            b: to2(r.binormal),
            k: r.curvature / shrink,
            leaving,
        })
    };
    let (Some(p0), Some(p1)) = (plane(r0), plane(r1)) else {
        return mid;
    };
    let m = to2(mid - origin);
    let best = plane_rail_crossings(p0, p1)
        .into_iter()
        .flatten()
        .filter(|x| {
            x.x.is_finite()
                && x.y.is_finite()
                && (*x - p0.q).dot(p0.leaving) > 0.0
                && (*x - p1.q).dot(p1.leaving) > 0.0
        })
        .min_by(|a, b| a.distance(m).total_cmp(&b.distance(m)));
    match best {
        Some(x) => origin + eu * x.x + ev * x.y,
        None => mid,
    }
}

/// The crossings of two generalised circles in the plane ([`PlaneRail`]): at most two, none
/// for coincident, parallel or apart ones.
fn plane_rail_crossings(a: PlaneRail, b: PlaneRail) -> [Option<Point2>; 2] {
    let cross = |u: Point2, v: Point2| u.x * v.y - u.y * v.x;
    // b.k a - a.k b is linear: X . w = h
    let w = (a.q - b.q) * (a.k * b.k) + a.b * b.k - b.b * a.k;
    let h =
        0.5 * (a.k * b.k * (a.q.dot(a.q) - b.q.dot(b.q))) + b.k * a.q.dot(a.b) - a.k * b.q.dot(b.b);
    if a.k == 0.0 && b.k == 0.0 {
        // two lines: X . a.b = a.q . a.b and X . b.b = b.q . b.b
        let det = cross(a.b, b.b);
        if det == 0.0 {
            return [None, None];
        }
        let (ha, hb) = (a.q.dot(a.b), b.q.dot(b.b));
        let x = Point2::new(
            (ha * b.b.y - hb * a.b.y) / det,
            (a.b.x * hb - b.b.x * ha) / det,
        );
        return [Some(x), None];
    }
    let ww = w.dot(w);
    if ww == 0.0 {
        return [None, None];
    }
    // the line X = x0 + t m, met with the rail of the larger curvature (where one rail is a
    // line, the line is that rail and the other is the circle it meets)
    let x0 = w * (h / ww);
    let m = Point2::new(-w.y, w.x) * (1.0 / ww.sqrt());
    let c = if a.k.abs() >= b.k.abs() { a } else { b };
    let d = x0 - c.q;
    let qa = c.k;
    let qb = 2.0 * (c.k * d.dot(m) - m.dot(c.b));
    let qc = c.value(x0);
    if qa == 0.0 {
        if qb == 0.0 {
            return [None, None];
        }
        return [Some(x0 + m * (-qc / qb)), None];
    }
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return [None, None];
    }
    // the root of the larger magnitude first, the other from the product (no cancellation)
    let big = -0.5 * (qb + qb.signum() * disc.sqrt());
    let t0 = big / qa;
    let t1 = if big == 0.0 { 0.0 } else { qc / big };
    [Some(x0 + m * t0), Some(x0 + m * t1)]
}

/// Not upstream (#17511): `CalcRailIntRail`'s guess where an edge is shorter than twice the
/// radius, in the common face's parameters: the rails' crossing to second order at the vertex
/// ([`rails_crossing`]). Each rail is its edge's osculating circle there (a line for a straight
/// edge), on the edge's own span of its curve (`ends`: the edge, its parameter at the vertex
/// and whether it leaves the vertex forwards), offset as far as the round's centre stands off
/// the side face, read from that face's offset surface and taken within the common face.
///
/// # Errors
/// A [`BlendFailure::Defect`] for a missing end, an edge curve without a second derivative
/// or a point that drops onto a face nowhere; [`BlendFailure::EdgeWithoutCurve`] for an edge
/// without its curve and [`BlendFailure::FilletSetUpInvalid`] for no common face; those of
/// the evaluations, drops and unitisations.
fn second_order_rail_guess(
    k: &Kernel,
    target: &Brep,
    face: Option<FaceId>,
    pairs: [(&FilletSolver, usize); 2],
    ends: [Option<(EdgeId, f64, bool)>; 2],
) -> Result<Point2> {
    const OP: &str = OP_RAIL_RAIL;
    let common = face_surface(target, face, OP)?;
    let uv_at = |srf: &Surface, p: Point3| -> Result<Point2> {
        let domain = srf.natural_uv_domain_with_nurb(k)?;
        let dropped = srf.drop_point::<NoObject>(k, p, &domain, &DropPointOptions::default())?;
        if !dropped.success {
            return Err(defect(OP, FOOT_NO_DROP));
        }
        Ok(dropped.result_uv)
    };
    let mut rails = [RailAtVertex {
        point: Point3::new(0.0, 0.0, 0.0),
        leaving: Vec3::new(0.0, 0.0, 0.0),
        binormal: Vec3::new(0.0, 0.0, 0.0),
        curvature: 0.0,
        offset: 0.0,
    }; 2];
    for ((slot, (fs, index)), end) in rails.iter_mut().zip(pairs).zip(ends) {
        let (e, vertex_param, from_min) = end.ok_or_else(|| defect(OP, NO_RAIL_END))?;
        let ivl = edge::interval(target, e)?;
        let curve =
            edge::curve(target, e)?.ok_or_else(|| edge_without_curve_status(OP, e, SM_ERR))?;
        // the span the edge runs along from the vertex (the flag picks the span that starts
        // at the parameter; an edge's curve can run on past its end, into the next edge's)
        let pv = curve.evaluate(k, vertex_param, 2, from_min, true)?;
        let (Some(&at), Some(&d1), Some(&d2)) = (pv.first(), pv.get(1), pv.get(2)) else {
            return Err(defect(OP, SHORT_EVALUATION));
        };
        // the way the edge leaves the vertex: towards its middle
        let unit = d1.unitize()?;
        let mid = curve.evaluate_point(k, ivl.mid())?;
        let leaving = if unit.dot(mid - at) < 0.0 {
            -unit
        } else {
            unit
        };
        // the round's centre lies on the side face's offset surface: its point over the
        // vertex is the vertex moved by the offset, taken within the common face
        let side = solver_surface(fs, usize::from(index == 0), OP)?;
        let side_offset = side.evaluate_point(k, uv_at(side.base_surface(), at)?)? - at;
        let common_normal =
            common.evaluate_normal(k, uv_at(common, at)?, Side::Left, Side::Left)?;
        let in_face = side_offset - common_normal * side_offset.dot(common_normal);
        let binormal = in_face.unitize()?;
        // the edge's curvature towards the rail side (either direction of travel)
        let curvature = (d2 - unit * d2.dot(unit)).dot(binormal) / d1.dot(d1);
        *slot = RailAtVertex {
            point: at,
            leaving,
            binormal,
            curvature,
            offset: in_face.length(),
        };
    }
    uv_at(common, rails_crossing(rails))
}

/// The surface of a target face (`pFace->GetSurface()`, dereferenced).
fn face_surface<'b>(
    target: &'b Brep,
    face: Option<FaceId>,
    op: &'static str,
) -> Result<&'b Surface> {
    let f = face.ok_or_else(|| missing_set_up(op, FilletSetUp::PointClassFace))?;
    face2::surface(target, f)?.ok_or_else(|| face_without_surface(op, f))
}

impl FilletBrep {
    /// The solver index of fillet geometry `g` (`GetFilletSolver()`, dereferenced).
    fn geom_solver_id(&self, g: FilletGeomId, op: &'static str) -> Result<FilletSolverId> {
        self.geom(g)?.fillet_solver().ok_or_else(|| {
            SolidError::blend_solver(
                op,
                SolverFailure::FilletGeomWithoutSolver { fillet_geom: g },
            )
        })
    }

    /// `GetVUAtRailEnd(pFG)->GetTsectPnt()`, dereferenced: the vertex use and a copy of
    /// its intersection point.
    fn rail_end_tsect(
        &self,
        v: VertexId,
        g: Option<FilletGeomId>,
        exclude: Option<VertexuseId>,
        op: &'static str,
    ) -> Result<(VertexuseId, TsectPnt)> {
        // pVU->GetTsectPnt() of a NULL vertex use is dereferenced
        let vu = self
            .vu_at_rail_end(v, g, exclude, None)?
            .ok_or_else(|| missing_set_up(op, FilletSetUp::RailEndVertexUse))?;
        Ok((vu, self.fillet_vertexuse(vu)?.tsect_pnt().clone()))
    }

    /// `SetOriginalUV(uv)`, `SetPoint(pnt)`, `SetStatus(SM_FIL_PROCESSED)` of a fillet
    /// vertex.
    fn set_processed(&mut self, v: VertexId, uv: Point2, pnt: Point3) -> Result<()> {
        self.fillet_vertex_mut(v)?.set_original_uv(uv);
        set_vertex_point(self.brep_mut(), v, pnt)?;
        self.fillet_vertex_mut(v)?.set_status(FilStatus::Processed);
        Ok(())
    }

    /// `SmFilletVertex::CalcRailIntRail`: the vertex where the rails of its two fillet
    /// geometries meet in their common face (the vertex's classification face), from a
    /// guess between the points two fillet radii along each filleted edge; its mates get
    /// the matching points on their faces.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without corner, solver, offset surface, edge use, curve or
    /// UV trim curve; `SM_ERR` without exactly two fillet geometries and when the rails
    /// do not meet (the vertex is then `SM_FV_SOLVER_NOT_CONVERGE`); those of the
    /// evaluations; those of [`CornerLookup::corner`] (`corners` resolves the vertex's corner) and those
    /// of `RailRailIntersect` (its `SmTemporaryRadiusChange`,
    /// `blend.fillet_executive_p2`).
    pub fn calc_rail_int_rail(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        solvers: &mut [FilletSolver],
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_rail_int_rail_with_solvers(
            k,
            v,
            corners,
            &mut RailRailSolvers::Indexed(solvers),
            target,
        )
    }

    /// [`Self::calc_rail_int_rail`] with the retained concrete solvers of the vertex's
    /// two fillet geometries, each with its actual identity (live executive sessions,
    /// whose solvers are not one indexed table).
    ///
    /// # Errors
    /// As [`Self::calc_rail_int_rail`]; a geometry whose solver is not one of `solvers`
    /// (or both geometries on one solver) is [`SolidError::InvalidInput`].
    pub fn calc_rail_int_rail_with_solver_ops(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        solvers: Vec<(FilletSolverId, &mut dyn FilletSolverOps)>,
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_rail_int_rail_with_solvers(
            k,
            v,
            corners,
            &mut RailRailSolvers::Live(solvers),
            target,
        )
    }

    #[allow(clippy::too_many_lines)] // one upstream routine
    fn calc_rail_int_rail_with_solvers(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        solvers: &mut RailRailSolvers<'_>,
        target: &mut Brep,
    ) -> Result<()> {
        const OP: &str = OP_RAIL_RAIL;
        // First, make sure we have a corner and exactly two FilletGeom's.
        let corner = self
            .fillet_vertex(v)?
            .fillet_corner()
            .ok_or_else(|| no_corner(OP))?;
        let geoms = self.vertex_fillet_geoms(v)?;
        let [fillet_geom1, fillet_geom2] = *geoms.as_slice() else {
            return Err(invalid_set_up(OP, FilletSetUp::VertexGeoms));
        };

        let cp_vertex = corners.corner(corner)?.filleted_vertex();
        let s1 = solvers.index(self.geom_solver_id(fillet_geom1, OP)?, OP)?;
        let s2 = solvers.index(self.geom_solver_id(fillet_geom2, OP)?, OP)?;
        let fs1 = solvers.at(s1, OP)?;
        let fs2 = solvers.at(s2, OP)?;

        // Get the common face, and the indices of the rail curves.
        let face = as_face(self.fillet_vertex(v)?.point_class_object(), OP)?;
        let rail_index1 = fs1.find_index_of_rail_on_face(target, face)?;
        let rail_index2 = fs2.find_index_of_rail_on_face(target, face)?;

        // Figure a guess uv for the intersection of the rail curves in the face: twice
        // the fillet radius along each filleted edge from the common vertex.
        let mut uv = [Point3::new(0.0, 0.0, 0.0); 2];
        let mut ends: [Option<(EdgeId, f64, bool)>; 2] = [None, None];
        let mut past_end = false;
        let pairs = [(fs1, rail_index1), (fs2, rail_index2)];
        for ((slot, end), (fs, index)) in uv.iter_mut().zip(ends.iter_mut()).zip(pairs) {
            let offset_distance = solver_surface(fs, index, OP)?.offset_distance().abs();
            let eu = solver_edgeuse(fs, index, OP)?;
            let e = eu1::edge(target, eu)?.ok_or_else(|| edge_use_without_edge(OP, eu))?;
            let ivl = edge::interval(target, e)?;
            let curve = edge::curve(target, e)?.ok_or_else(|| edge_without_curve(OP, e))?;
            let owner_zone_tol_3d = Some(edge::tolerance(target, e)?);

            let vu = eu1::vertexuse(target, eu)?
                .ok_or_else(|| missing_set_up(OP, FilletSetUp::EdgeUseVertexUse))?;
            let pv = vertexuse_unit::vertex(target, vu)?;
            let orient = eu1::orientation(target, eu)?;
            let from_min = (pv == cp_vertex && orient == OrientType::Same)
                || (pv != cp_vertex && orient == OrientType::Opposite);
            let offset_param = if from_min {
                arc_length_param(
                    k,
                    curve,
                    ivl.min(),
                    2.0 * offset_distance,
                    owner_zone_tol_3d,
                    OP,
                )?
            } else {
                arc_length_param(
                    k,
                    curve,
                    ivl.max(),
                    -2.0 * offset_distance,
                    owner_zone_tol_3d,
                    OP,
                )?
            };
            // Not upstream (#17511): whether twice the radius runs past the edge's far end
            past_end |= offset_param < ivl.min() || offset_param > ivl.max();
            *end = Some((e, if from_min { ivl.min() } else { ivl.max() }, from_min));
            let fetch = get_or_create_uv_trim_curve(k, target, eu, false, false, true)?;
            fetch.status?;
            let trim_curve = eu1::uv_trim_curve(target, eu)?
                .0
                .filter(|c| is_b_spline_kind(c))
                .ok_or_else(|| missing_set_up(OP, FilletSetUp::UvTrimCurve))?;
            *slot = trim_curve.evaluate_point(k, offset_param)?;
        }

        // Their midpoint is the guess. Not upstream (#17511): where an edge is shorter than
        // twice the radius, that point lies past the edge's end and so does not exist on it
        // (a round of 8 on a quarter circle of radius 10: the midpoint lay by the arc's far
        // end and the solve found the rails' other crossing, a ball past the arc's end);
        // there the guess is where the rails cross to second order at the vertex
        // ([`rails_crossing`]).
        let uv_guess = if past_end {
            second_order_rail_guess(
                k,
                target,
                face,
                [(fs1, rail_index1), (fs2, rail_index2)],
                ends,
            )?
        } else {
            let [uv0, uv1] = uv;
            let mid_uv = (uv0 + uv1) * 0.5;
            Point2::new(mid_uv.x, mid_uv.y)
        };

        // Get the TsectPnts and call the solver.
        let (vu1, mut tsect1) = self.rail_end_tsect(v, Some(fillet_geom1), None, OP)?;
        let (vu2, mut tsect2) = self.rail_end_tsect(v, Some(fillet_geom2), None, OP)?;
        // WR-300 (the fork's tangency-labs/smlib@465b62b): the radii are held at their
        // end values only at a corner calculated as constant radius, as the rail/side-edge
        // search holds them; upstream holds them anywhere, so the rails met at a ball that
        // is not the law's.
        let hold_end_radii = corners.corner(corner)?.is_calc_as_const_rad();
        let found = solvers.rail_rail_intersect(
            k,
            target,
            s1,
            uv_guess,
            rail_index1,
            s2,
            rail_index2,
            &mut tsect1,
            &mut tsect2,
            hold_end_radii,
            OP,
        );
        // the vertex uses' points are written in place, also on a failure
        self.fillet_vertexuse_mut(vu1)?.set_tsect_pnt(&tsect1);
        self.fillet_vertexuse_mut(vu2)?.set_tsect_pnt(&tsect2);
        let found = found?;
        if !found {
            self.fillet_vertex_mut(v)?
                .set_status(FilStatus::SolverNotConverge);
            return Err(match face {
                Some(face) => Failure::new(OP, BlendFailure::RailCrossingNotConverged { face })
                    .at(EntityRef::Face(face))
                    .into_error(),
                // no common face: a stop of the solve all the same, keeping `SM_ERR`
                None => invalid_set_up(OP, FilletSetUp::PointClassFace),
            });
        }

        // Save the SolutionPoints
        let uv_pt = tsect1.uv_pos(srf(rail_index1, OP)?);
        let pnt = face_surface(target, face, OP)?.evaluate_point(k, uv_pt)?;
        self.set_processed(v, uv_pt, pnt)?;

        // Calculate Mate if needed
        for j in 0..2 {
            let Some(mate) = self.fillet_vertex(v)?.mate(j) else {
                continue;
            };
            let f = as_face(self.fillet_vertex(mate)?.point_class_object(), OP)?;
            let vu = self.vu_at_rail_end(mate, Some(fillet_geom1), None, None)?;
            let uv_pt = if vu.is_some() {
                tsect1.uv_pos(other_srf(rail_index1, OP)?)
            } else {
                tsect2.uv_pos(other_srf(rail_index2, OP)?)
            };
            let pt = face_surface(target, f, OP)?.evaluate_point(k, uv_pt)?;
            self.set_processed(mate, uv_pt, pt)?;
        }
        Ok(())
    }

    /// `SmFilletVertex::CalcRailIntEdgeuse(pOptFG)`: the vertex where the rail of its
    /// first (and second) fillet geometry crosses the side edge use its classification
    /// maps to, sought from a sequence of guesses along the side edge (a second round
    /// with a doubled extension factor), projected onto the side edge's curve; its mates
    /// get the matching points. With `opt_fg`, only that geometry is processed.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without corner, side edge use, curve, mate face or solver;
    /// `SM_ERR` without a fillet geometry and, for a blending corner, when no
    /// intersection is found; those of the evaluations and solves;
    /// those of [`CornerLookup::corner`] (`corners` resolves the vertex's corner);
    /// [`SolidError::Unported`] for the temporary radius change of a solver
    /// (`blend.fillet_executive_p2`) and those of `SmFilletSolver::RailEdgeuseIntersect`.
    pub fn calc_rail_int_edgeuse(
        &mut self,
        k: &Kernel,
        v: VertexId,
        opt_fg: Option<FilletGeomId>,
        corners: CornerLookup<'_>,
        solvers: &mut [FilletSolver],
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_rail_int_edgeuse_with_solvers(
            k,
            v,
            opt_fg,
            corners,
            &mut RailSolvers::Indexed(solvers),
            target,
            None,
            0,
        )
    }

    /// Calculate a rail/side-edge vertex using the retained concrete solver's
    /// virtual operations and actual identity. Geometry-owned offset surfaces are
    /// used before the solver.
    ///
    /// # Errors
    /// As [`Self::calc_rail_int_edgeuse`]; resolving an adjacent solver with another
    /// id returns [`SolidError::InvalidInput`] rather than fabricating its state.
    pub fn calc_rail_int_edgeuse_with_solver_ops(
        &mut self,
        k: &Kernel,
        v: VertexId,
        opt_fg: Option<FilletGeomId>,
        corners: CornerLookup<'_>,
        solver_id: FilletSolverId,
        solver: &mut dyn FilletSolverOps,
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_rail_int_edgeuse_with_solvers(
            k,
            v,
            opt_fg,
            corners,
            &mut RailSolvers::Live(solver_id, solver),
            target,
            None,
            0,
        )
    }

    /// Calculate a rail/side-edge vertex using the retained concrete solvers of all
    /// its fillet geometries (an Nx2 corner's side vertex lies on two), each with its
    /// actual identity.
    ///
    /// # Errors
    /// As [`Self::calc_rail_int_edgeuse`]; a geometry whose solver is not one of
    /// `solvers` is [`SolidError::InvalidInput`].
    pub fn calc_rail_int_edgeuse_with_live_solvers(
        &mut self,
        k: &Kernel,
        v: VertexId,
        opt_fg: Option<FilletGeomId>,
        corners: CornerLookup<'_>,
        solvers: Vec<(FilletSolverId, &mut dyn FilletSolverOps)>,
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_rail_int_edgeuse_with_solvers(
            k,
            v,
            opt_fg,
            corners,
            &mut RailSolvers::LiveSet(solvers),
            target,
            None,
            0,
        )
    }

    #[allow(clippy::too_many_lines)] // one upstream routine
    /// `near` (the fork's `cpNear`, 63c7962): the side edge's end the search starts from,
    /// `None` for the corner's vertex, else where the side edge continues a missed one;
    /// `depth` counts those continuations (at most
    /// [`crate::blend::blend_corner::end::SIDE_EDGE_CHAIN_STEPS`], then `SM_ERR`; the fork
    /// has no bound).
    #[allow(clippy::too_many_arguments)]
    fn calc_rail_int_edgeuse_with_solvers(
        &mut self,
        k: &Kernel,
        v: VertexId,
        opt_fg: Option<FilletGeomId>,
        corners: CornerLookup<'_>,
        solvers: &mut RailSolvers<'_>,
        target: &mut Brep,
        near: Option<VertexId>,
        depth: usize,
    ) -> Result<()> {
        const OP: &str = OP_RAIL_EU;
        let corner = self
            .fillet_vertex(v)?
            .fillet_corner()
            .ok_or_else(|| no_corner(OP))?;

        // get original Brep vertex (or the side edge's near end)
        let cp_vertex = match near {
            Some(near) => Some(near),
            None => corners.corner(corner)?.filleted_vertex(),
        };

        // get this filletVertex->filletEdgeuse[type == SM_FE_RAIL]->FilletGeom(s)
        let geoms = self.vertex_fillet_geoms(v)?;

        // check state - at least 1 railEdge must be connected to this vertex
        let total_geoms = geoms.len();
        let first = *geoms
            .first()
            .ok_or_else(|| invalid_set_up(OP, FilletSetUp::VertexGeoms))?;
        let mut fillet_geom1 = Some(first);
        let mut fillet_geom2 = if total_geoms == 2 {
            geoms.get(1).copied()
        } else {
            None
        };

        // get original Brep Edgeuse connected to this FilletVertex
        let side_eu1 = as_edgeuse(self.fillet_vertex(v)?.point_class_object(), OP)?;

        // get vertexuse that is connected to a pFilletGeom1->filletRailEdge
        let vu1 = self.vu_at_rail_end(v, Some(first), None, None)?;

        // Set dGuessEdgeParameters to run from one edgeEnd to the other
        let side_eu1 =
            side_eu1.ok_or_else(|| missing_set_up(OP, FilletSetUp::PointClassEdgeUse))?;
        let side_edge =
            eu1::edge(target, side_eu1)?.ok_or_else(|| edge_use_without_edge(OP, side_eu1))?;
        let ivl = edge::interval(target, side_edge)?;

        // side edgeuse was carefully selected based on its orientation
        let is_closed = edge::is_closed(target, side_edge)?;
        let from_start = if is_closed {
            eu1::orientation(target, side_eu1)? == OrientType::Same
        } else {
            edge::start_vertex(target, side_edge)? == cp_vertex
        };

        // Make the first guess a fraction of the length based on radius.
        let mut guess_param = if from_start {
            ivl.evaluate(0.01)
        } else {
            ivl.evaluate(0.99)
        };
        let side_edge_length = edge::curve(target, side_edge)?
            .ok_or_else(|| edge_without_curve(OP, side_edge))?
            .approximate_length(k, &ivl, 5, false)?
            .0;
        if let Some(off_srf) = solvers.offset_surface(self, first, 0)? {
            let rad = off_srf.offset_distance().abs();
            if side_edge_length > rad / 2.0 {
                let mut frac = rad / side_edge_length;
                if !from_start {
                    frac = 1.0 - frac;
                }
                guess_param = ivl.evaluate(frac);
            }
        }

        let fracs: [f64; 8] = if from_start {
            [0.05, 0.1, 0.23, 0.5, 0.77, 0.9, 0.95, 0.99]
        } else {
            [0.95, 0.9, 0.77, 0.5, 0.23, 0.1, 0.05, 0.01]
        };
        let mut guess_edge_parameters = [guess_param; 9];
        for (slot, f) in guess_edge_parameters.iter_mut().skip(1).zip(fracs) {
            *slot = ivl.evaluate(f);
        }

        // Not upstream (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): at a Join
        // whose mate lies on the face the second blend's rail rolled onto, the whole length
        // (`join::rolled_sides`), while the first blend rolled nowhere, the second blend's
        // section at the corner is the ball there: the first blend's, on its own face's offset,
        // touches that face's continuation past the rolled-over edge, where its rail rolls over
        // only later, in its own rollover. The rolled blend's end then places the vertex and its
        // mate, solved on the side edge's other use (its own face's), and the first blend's end
        // is solved as the second blend's always is. Both corners' sections then end the rails
        // on the vertices they are trimmed to; the first blend's rollover solves the vertex
        // again, from its piece rolled over, at the same ball. The guesses on the side edge stay
        // those set up above from the side edge use the corner chose (`from_start` from its
        // orientation on a closed edge) and the first blend's radius: they are parameters of
        // the edge's curve, shared by both edge uses, and upstream's second blend below already
        // searches its edge use with them.
        let (mut side_eu1, mut vu1) = (side_eu1, vu1);
        if let (None, Some(g1), Some(g2)) = (opt_fg, fillet_geom1, fillet_geom2) {
            if g1 != g2 {
                let side_eu2 = eu1::radial(target, side_eu1)?;
                if let Some(side_eu2) = side_eu2 {
                    if rolls_only_at_mate(self, solvers, g2, side_eu2, target)?
                        && !rolls_anywhere(self, solvers, g1)?
                    {
                        fillet_geom1 = Some(g2);
                        fillet_geom2 = Some(g1);
                        side_eu1 = side_eu2;
                        vu1 = self.vu_at_rail_end(v, Some(g2), None, None)?;
                    }
                }
            }
        }

        // Sometimes, we may just need to do one side
        if let Some(fg) = opt_fg {
            if Some(fg) == fillet_geom1 {
                fillet_geom2 = None;
            } else if Some(fg) == fillet_geom2 {
                fillet_geom1 = None;
            }
        }

        // For FilletGeom1: find the rail/sideEdge intersection, place it on the
        // sideEdge curve, save it and update any mates
        if let Some(g1) = fillet_geom1 {
            // its status is ignored upstream (lRailIndex stays 0); fork 9b7cd48 passes
            // this on (SER)
            let rail_index = self
                .rail_index_of_side_eu_resolved(g1, side_eu1, target, &|id| solvers.solver(id))?;

            // Not upstream (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): a rail rolled over a smooth far
            // edge of its solver's face, the whole length, lies on the face beyond, which the
            // side edge at the corner does not bound. The rail ends on the next side edge, across
            // the vertex where the rolled-over edge meets this one; it is solved there, on that
            // edge's use on the rail's face (the side edge's UV curve must be on the rail's
            // surface), as a rail continues across a seam's vertex.
            let rolled = {
                let si = RailSolvers::geom_solver(self, g1)?;
                let solver = solvers.solver(si)?;
                solver
                    .rolled_faces
                    .get(rail_index)
                    .copied()
                    .flatten()
                    .map(|face| (face, solver.tangency_tolerance))
            };
            if let Some((rolled, tol_rad)) = rolled {
                let on_rolled = eu1::face(target, side_eu1)? == Some(rolled)
                    || match eu1::radial(target, side_eu1)? {
                        Some(r) => eu1::face(target, r)? == Some(rolled),
                        None => false,
                    };
                let far = if edge::start_vertex(target, side_edge)? == cp_vertex {
                    edge::end_vertex(target, side_edge)?
                } else if edge::end_vertex(target, side_edge)? == cp_vertex {
                    edge::start_vertex(target, side_edge)?
                } else {
                    None
                };
                if let (false, Some(far)) = (on_rolled, far) {
                    let beyond = crate::blend::blend_corner::end::side_edgeuse_across_rolled_edge(
                        k,
                        target,
                        side_eu1,
                        far,
                        Some(rolled),
                        tol_rad,
                    )?;
                    let on_face = match beyond {
                        Some(b) => eu1::radial(target, b)?,
                        None => None,
                    };
                    if let Some(next) = on_face {
                        if depth >= crate::blend::blend_corner::end::SIDE_EDGE_CHAIN_STEPS {
                            return Err(SolidError::blend(
                                OP,
                                BlendFailure::SideEdgeChainPastBound {
                                    steps: crate::blend::blend_corner::end::SIDE_EDGE_CHAIN_STEPS,
                                },
                            ));
                        }
                        self.fillet_vertex_mut(v)?.set_point_class(
                            crate::intersect::point_class::PointClassType::Edgeuse,
                            Some(EntityRef::Edgeuse(next)),
                        );
                        return self.calc_rail_int_edgeuse_with_solvers(
                            k,
                            v,
                            opt_fg,
                            corners,
                            solvers,
                            target,
                            Some(far),
                            depth + 1,
                        );
                    }
                }
            }

            let vu1 = vu1.ok_or_else(|| missing_set_up(OP, FilletSetUp::RailEndVertexUse))?;
            let mut tsect1 = self.fillet_vertexuse(vu1)?.tsect_pnt().clone();
            // Not upstream (#13538): the rail's end continues the round from its section at
            // the corner, so the search starts where that section touches the side edge, not
            // at a fraction of the edge's length. Where the faces meet the rail end's solve
            // twice (two cylinders crossing: the round at each of their crossings), the first
            // guess decides which ball the solve finds. The section is the one at the corner's
            // vertex also where the rail's end is looked for on a side edge past it (a rail
            // rolled over a smooth far edge of its face, or one running on across a seam's
            // vertex), where the side edge's near end `cp_vertex` ends no end of the solver's
            // edge. On a side edge chain (the side edge's near end is not the corner's vertex)
            // its guess goes in front of the default ones, which all stay: the radius-based
            // guess, which looked for the end there before, is still tried next. At the corner's
            // own side edge it takes the radius-based guess's place.
            let si = RailSolvers::geom_solver(self, g1)?;
            let section_vertex = corners.corner(corner)?.filleted_vertex();
            let corner_guess = corner_section_on_side_edge(
                k,
                solvers.solver_ops_mut(si)?,
                target,
                section_vertex,
                rail_index,
                side_edge,
                &ivl,
            )?;
            let at_corner = section_vertex == cp_vertex;
            let mut found = false;
            let mut edgeuse_parameter = 0.0;
            for guess in rail_end_guesses(corner_guess, at_corner, &guess_edge_parameters) {
                // Temporarily change this to a constant-radius function.
                let temp_si = if corners.corner(corner)?.is_calc_as_const_rad() {
                    let si = RailSolvers::geom_solver(self, g1)?;
                    let _ = solvers.solver(si)?;
                    Some(si)
                } else {
                    None
                };

                // rTsectPnt1 is the vertex use's own point: written in place also on a
                // failure
                let r = rail_edgeuse_intersect(
                    self,
                    k,
                    g1,
                    solvers,
                    target,
                    temp_si,
                    rail_index,
                    side_eu1,
                    guess,
                    &mut tsect1,
                );
                if r.is_err() {
                    self.fillet_vertexuse_mut(vu1)?.set_tsect_pnt(&tsect1);
                }
                if let Some(p) = r? {
                    found = true;
                    edgeuse_parameter = p;
                    break;
                }
            }

            // Try once more with a bigger extension. [B465]
            if !found {
                let si = RailSolvers::geom_solver(self, g1)?;
                let fs = solvers.solver_mut(si)?;
                let ext_factor = fs.extension_factor();
                fs.set_extension_factor(2.0 * ext_factor);

                // WR-300 (#1023): upstream's retry holds the radius at a corner not
                // calculated as constant radius too; this holds it only as the first
                // search did.
                let retry_si = if corners.corner(corner)?.is_calc_as_const_rad() {
                    Some(si)
                } else {
                    None
                };
                for guess in rail_end_guesses(corner_guess, at_corner, &guess_edge_parameters) {
                    let _ = solvers.solver(si)?;
                    let r = rail_edgeuse_intersect(
                        self,
                        k,
                        g1,
                        solvers,
                        target,
                        retry_si,
                        rail_index,
                        side_eu1,
                        guess,
                        &mut tsect1,
                    );
                    if r.is_err() {
                        self.fillet_vertexuse_mut(vu1)?.set_tsect_pnt(&tsect1);
                    }
                    if let Some(p) = r? {
                        found = true;
                        edgeuse_parameter = p;
                        break;
                    }
                }
            }
            self.fillet_vertexuse_mut(vu1)?.set_tsect_pnt(&tsect1);

            // Not upstream (#10876): in the Roll-over's second pass (its corner at V), a
            // rail's end found on a straight side edge's line continued past its far end (the
            // end away from the corner's vertex) is not on the side edge: it is looked for
            // across the cliff's tangent seam there (below). One found behind the corner's
            // vertex stays as any pass takes it.
            if found && corners.corner(corner)?.roll_over {
                let slack = EFF_ZERO * (1.0 + ivl.length());
                let far_is_max = edge::start_vertex(target, side_edge)? == cp_vertex;
                let past_far = if far_is_max {
                    edgeuse_parameter > ivl.max() + slack
                } else {
                    edgeuse_parameter < ivl.min() - slack
                };
                if past_far {
                    found = false;
                }
            }

            // SolidCraft change (the fork's 63c7962): a side edge
            // split only by a seam ending on it (a cylinder's seam on its rim) continues
            // across that vertex: the rail that runs past the edge's far end ends on the next
            // edge, between the same two faces. Its end is looked for there, from that vertex
            // on.
            if !found && !is_closed {
                let far = if edge::start_vertex(target, side_edge)? == cp_vertex {
                    edge::end_vertex(target, side_edge)?
                } else if edge::end_vertex(target, side_edge)? == cp_vertex {
                    edge::start_vertex(target, side_edge)?
                } else {
                    None
                };
                let tol_deg = crate::math::rad_to_deg(corners.corner(corner)?.tangency_tol_radians);
                let mut next = match far {
                    Some(far) => crate::blend::blend_corner::end::side_edgeuse_across_seam_vertex(
                        k, target, side_eu1, far, tol_deg,
                    )?,
                    None => None,
                };
                // Not upstream (#10876): the Roll-over's concave chain ends on the cliff,
                // whose faces run on tangentially across a seam that meets the side edge's
                // far end; the rail that runs past that end ends on the next edge, between
                // the face beyond the seam and the same face of the rail (the edge the
                // rolled-over rail's search takes, `side_edgeuse_across_rolled_edge`), its
                // use on the face that corresponds to the side edge use's
                if next.is_none() && corners.corner(corner)?.roll_over {
                    let tol_rad = corners.corner(corner)?.tangency_tol_radians;
                    let beyond = match far {
                        Some(far) => {
                            crate::blend::blend_corner::end::side_edgeuse_across_rolled_edge(
                                k, target, side_eu1, far, None, tol_rad,
                            )?
                        }
                        None => None,
                    };
                    if let Some(b) = beyond {
                        next = if eu1::face(target, b)? == eu1::face(target, side_eu1)? {
                            Some(b)
                        } else {
                            eu1::radial(target, b)?
                        };
                    }
                }
                if let (Some(next), Some(far)) = (next, far) {
                    if depth >= crate::blend::blend_corner::end::SIDE_EDGE_CHAIN_STEPS {
                        return Err(SolidError::blend(
                            OP,
                            BlendFailure::SideEdgeChainPastBound {
                                steps: crate::blend::blend_corner::end::SIDE_EDGE_CHAIN_STEPS,
                            },
                        ));
                    }
                    self.fillet_vertex_mut(v)?.set_point_class(
                        crate::intersect::point_class::PointClassType::Edgeuse,
                        Some(EntityRef::Edgeuse(next)),
                    );
                    return self.calc_rail_int_edgeuse_with_solvers(
                        k,
                        v,
                        opt_fg,
                        corners,
                        solvers,
                        target,
                        Some(far),
                        depth + 1,
                    );
                }
            }

            // when still not found
            if !found {
                self.fillet_vertex_mut(v)?
                    .set_status(FilStatus::NoIntRailEu);
                // Sometimes the fillet can still succeed.
                if corners.corner(corner)?.is_blending_corner() {
                    return Err(Failure::new(
                        OP,
                        BlendFailure::RailEndMissesSideEdge { edge: side_edge },
                    )
                    .at(EntityRef::Edge(side_edge))
                    .into_error());
                }
                return Ok(());
            }

            // force solution to lie on SideEdge->Curve: the rail/base surface point
            let uv = tsect1.uv_pos(srf(rail_index, OP)?);
            let base = solvers
                .offset_surface(self, g1, rail_index)?
                .ok_or_else(|| missing_set_up(OP, FilletSetUp::RailOffsetSurface))?
                .base_surface();
            // Not upstream (WR-630;
            // the fork's tangency-labs/smlib@3f22e97): on the base surface continued, as
            // the solve found it (over the surface's extended domain). Clamped to the
            // surface's own domain, a rail meeting a side edge's line just past the
            // edge's end (a cliff overflow) was placed on the edge's end vertex instead.
            let pnt = with_out_of_bounds_enabled_in_place(base, || base.evaluate_point(k, uv))?;

            // project it onto the side edge's curve
            let side_e =
                eu1::edge(target, side_eu1)?.ok_or_else(|| edge_use_without_edge(OP, side_eu1))?;
            let curve =
                edge::curve(target, side_e)?.ok_or_else(|| edge_without_curve(OP, side_e))?;
            let (_found, solution) = curve.local_point_solve::<()>(
                k,
                &curve.natural_interval(),
                SolverOperation::Minimize,
                pnt,
                &PointSolveOptions::default(),
                edgeuse_parameter,
            )?;
            edgeuse_parameter = solution.start.param(0)?;

            // set outputs: the point and the classification's curve parameter
            set_vertex_point(self.brep_mut(), v, pnt)?;
            self.fillet_vertex_mut(v)?
                .set_original_t_param(edgeuse_parameter);

            if fillet_geom1 == fillet_geom2 {
                // Seam cases: the uv on the other side may need adjusting
                let bin = evaluate_binormal(
                    k,
                    target,
                    side_eu1,
                    edgeuse_parameter,
                    false,
                    &BinormalOptions::default(),
                )?;
                let other_index = other_srf(rail_index, OP)?.index();
                let other_surf = solvers
                    .offset_surface(self, g1, other_index)?
                    .ok_or_else(|| missing_set_up(OP, FilletSetUp::RailOffsetSurface))?
                    .base_surface();
                let mut other_uv = tsect1.uv_pos(srf(other_index, OP)?);
                adjust_uv_on_seam(k, other_surf, bin.binormal, &mut other_uv)?;
                *tsect1.uv_pos_mut(srf(other_index, OP)?) = other_uv;
                self.fillet_vertexuse_mut(vu1)?.set_tsect_pnt(&tsect1);
            }

            // Calculate FilletVertex->Mates if needed (at most two)
            for j in 0..2 {
                let Some(mate) = self.fillet_vertex(v)?.mate(j) else {
                    continue;
                };
                if self.vu_at_rail_end(mate, Some(g1), None, None)?.is_none() {
                    continue;
                }
                let face = as_face(self.fillet_vertex(mate)?.point_class_object(), OP)?
                    .ok_or_else(|| missing_set_up(OP, FilletSetUp::PointClassFace))?;
                let other_index = other_srf(rail_index, OP)?;
                let face = rolled_face_of(self, solvers, g1, other_index.index())?.unwrap_or(face);
                let uv1 = tsect1.uv_pos(other_index);
                let pnt1 = evaluate_mate(k, target, face, uv1)?;
                self.set_processed(mate, uv1, pnt1)?;
                break;
            }

            // done with this FilletVertex
            self.fillet_vertex_mut(v)?.set_status(FilStatus::Processed);
        }

        // Process intersection for adjacent solver if presented
        if let Some(g2) = fillet_geom2 {
            let side_eu2 = eu1::radial(target, side_eu1)?;
            // its status is ignored upstream; fork 9b7cd48 passes this on (SER)
            // upstream dereferences a NULL side edge use there
            let side_eu2 =
                side_eu2.ok_or_else(|| missing_set_up(OP, FilletSetUp::RadialEdgeUse))?;
            let other_rail_index =
                self.rail_index_of_side_eu_resolved(g2, side_eu2, target, &|id| {
                    solvers.solver(id)
                })?;

            let (vu2, mut tsect2) = self.rail_end_tsect(v, Some(g2), vu1, OP)?;
            let mut found = false;
            let mut edgeuse_parameter2 = 0.0;
            for &guess in &guess_edge_parameters {
                // Temporarily change this to a constant-radius function.
                let si = RailSolvers::geom_solver(self, g2)?;
                let _ = solvers.solver(si)?;
                // WR-300 (#1023): upstream holds the adjacent round's radius here at every
                // corner; this holds it only at a corner calculated as constant radius, as
                // the first round's search and retry do. Where the side edge is not square
                // to the round's edge, the ball meeting it is not the end's, and the end
                // radius put the rail's end off the round.
                let temp_si = if corners.corner(corner)?.is_calc_as_const_rad() {
                    Some(si)
                } else {
                    None
                };
                let r = rail_edgeuse_intersect(
                    self,
                    k,
                    g2,
                    solvers,
                    target,
                    temp_si,
                    other_rail_index,
                    side_eu2,
                    guess,
                    &mut tsect2,
                );
                if r.is_err() {
                    self.fillet_vertexuse_mut(vu2)?.set_tsect_pnt(&tsect2);
                }
                if let Some(p) = r? {
                    found = true;
                    edgeuse_parameter2 = p;
                    break;
                }
            }
            self.fillet_vertexuse_mut(vu2)?.set_tsect_pnt(&tsect2);

            // when no intersection was found
            if !found {
                self.fillet_vertex_mut(v)?
                    .set_status(FilStatus::SolverNotConverge);
                return Ok(());
            }

            if fillet_geom1 == fillet_geom2 {
                // Seam cases
                let bin = evaluate_binormal(
                    k,
                    target,
                    side_eu2,
                    edgeuse_parameter2,
                    false,
                    &BinormalOptions::default(),
                )?;
                let other_index = other_srf(other_rail_index, OP)?.index();
                let other_surf = solvers
                    .offset_surface(self, g2, other_index)?
                    .ok_or_else(|| missing_set_up(OP, FilletSetUp::RailOffsetSurface))?
                    .base_surface();
                let mut other_uv = tsect2.uv_pos(srf(other_index, OP)?);
                adjust_uv_on_seam(k, other_surf, bin.binormal, &mut other_uv)?;
                *tsect2.uv_pos_mut(srf(other_index, OP)?) = other_uv;
                self.fillet_vertexuse_mut(vu2)?.set_tsect_pnt(&tsect2);
            }

            // Calculate Mate if needed (at most two)
            for jj in 0..2 {
                let Some(mate) = self.fillet_vertex(v)?.mate(jj) else {
                    continue;
                };
                if self.fillet_vertex(mate)?.is_processed() {
                    continue;
                }
                if self.vu_at_rail_end(mate, Some(g2), None, None)?.is_none() {
                    continue;
                }
                let face = as_face(self.fillet_vertex(mate)?.point_class_object(), OP)?
                    .ok_or_else(|| missing_set_up(OP, FilletSetUp::PointClassFace))?;
                let other_index = other_srf(other_rail_index, OP)?;
                let face = rolled_face_of(self, solvers, g2, other_index.index())?.unwrap_or(face);
                let uv1 = tsect2.uv_pos(other_index);
                let pnt1 = evaluate_mate(k, target, face, uv1)?;
                self.set_processed(mate, uv1, pnt1)?;
                break;
            }

            // done with this FilletVertex
            self.fillet_vertex_mut(v)?.set_status(FilStatus::Processed);
        }

        Ok(())
    }

    /// `SmFilletVertex::CalcVertOnCrossSection(pOptEdgeParam)`: the vertex on the
    /// cross-sectional plane of the filleted edge at the corner vertex (or at
    /// `opt_edge_param`), solved by `PointOnPlaneSolve` from the vertex's UV points on
    /// the two faces (stepped off the edge first where a surface's first derivatives are
    /// nearly parallel, and again, up to three tries, when the solve fails); its other
    /// rail end and its mate get the matching points.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without corner, solver, edge use, curve or surface; `SM_ERR`
    /// without a fillet geometry; those of the evaluations; those of [`CornerLookup::corner`]
    /// (`corners` resolves the vertex's corner).
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn calc_vert_on_cross_section(
        &mut self,
        k: &Kernel,
        v: VertexId,
        opt_edge_param: Option<f64>,
        corners: CornerLookup<'_>,
        solvers: &mut [FilletSolver],
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_vert_on_cross_section_with_solvers(
            k,
            v,
            opt_edge_param,
            corners,
            &mut RailSolvers::Indexed(solvers),
            target,
        )
    }

    /// [`Self::calc_vert_on_cross_section`] with the live session's concrete solvers of
    /// the vertex's geometries (`PointOnPlaneSolve` is the concrete solver's virtual).
    ///
    /// # Errors
    /// As [`Self::calc_vert_on_cross_section`]; an unresolved geometry solver.
    pub fn calc_vert_on_cross_section_with_live_solvers(
        &mut self,
        k: &Kernel,
        v: VertexId,
        opt_edge_param: Option<f64>,
        corners: CornerLookup<'_>,
        solvers: Vec<(FilletSolverId, &mut dyn FilletSolverOps)>,
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_vert_on_cross_section_with_solvers(
            k,
            v,
            opt_edge_param,
            corners,
            &mut RailSolvers::LiveSet(solvers),
            target,
        )
    }

    #[allow(clippy::too_many_lines)] // one upstream routine
    fn calc_vert_on_cross_section_with_solvers(
        &mut self,
        k: &Kernel,
        v: VertexId,
        opt_edge_param: Option<f64>,
        corners: CornerLookup<'_>,
        solvers: &mut RailSolvers<'_>,
        target: &mut Brep,
    ) -> Result<()> {
        const OP: &str = OP_CROSS;
        let corner = self
            .fillet_vertex(v)?
            .fillet_corner()
            .ok_or_else(|| no_corner(OP))?;
        let cp_vertex = corners.corner(corner)?.filleted_vertex();

        // Determine where the cross-sectional plane is
        let geoms = self.vertex_fillet_geoms(v)?;
        let total_geoms = geoms.len();
        let fillet_geom1 = *geoms
            .first()
            .ok_or_else(|| invalid_set_up(OP, FilletSetUp::VertexGeoms))?;
        let s1 = self.geom_solver_id(fillet_geom1, OP)?;
        let fs1 = solvers.solver(s1)?;
        let eu = solver_edgeuse(fs1, 0, OP)?;
        let e = eu1::edge(target, eu)?.ok_or_else(|| edge_use_without_edge(OP, eu))?;
        let ivl = edge::interval(target, e)?;
        let owner_zone_tol_3d = Some(edge::tolerance(target, e)?);
        let mut vert_at_edge_start = true;
        let mut param = ivl.min();
        if let Some(p) = opt_edge_param {
            param = p;
        } else if edge::start_vertex(target, e)? != cp_vertex {
            param = ivl.max();
            vert_at_edge_start = false;
        }
        let curve = edge::curve(target, e)?
            .ok_or_else(|| edge_without_curve(OP, e))?
            .clone();
        let pv = curve.evaluate(k, param, 1, true, true)?;
        let plane_orig = pv
            .first()
            .copied()
            .ok_or_else(|| defect(OP, SHORT_EVALUATION))?;
        let plane_normal = pv
            .get(1)
            .copied()
            .ok_or_else(|| defect(OP, SHORT_EVALUATION))?;
        let (vu1, mut tsect) = self.rail_end_tsect(v, Some(fillet_geom1), None, OP)?;

        // the vertex's UV points on the solver's two faces
        let vertexuses = match cp_vertex {
            Some(cv) => vertex::vertexuses(target, cv)?,
            None => return Err(missing_set_up(OP, FilletSetUp::CornerVertex)),
        };
        let mut uvs = [Point2::new(0.0, 0.0); 2];
        for (i, slot) in uvs.iter_mut().enumerate() {
            let face = eu1::face(target, solver_edgeuse(fs1, i, OP)?)?;
            for &vu in &vertexuses {
                let fu = vertexuse_unit::faceuse(target, vu)?
                    .ok_or_else(|| missing_set_up(OP, FilletSetUp::VertexUseFaceUse))?;
                if target.faceuse(fu)?.face == face {
                    // its status is ignored upstream (the point is then whatever the
                    // call left; here the previous value); fork 9b7cd48 passes this on
                    // (SER)
                    *slot = vertexuse_unit::compute_uv_point(k, target, vu, true)?;
                    break;
                }
            }
        }

        // Determine if we need to step off the edge to avoid convergence problems
        let stepoff_dist = solver_surface(fs1, 0, OP)?.offset_distance().abs();
        let mut curve_stepoff_dist = stepoff_dist;
        let curve_len = curve.approximate_length(k, &ivl, 10, false)?.0;
        let max_stepoff_dist = curve_len * 0.15;
        if curve_stepoff_dist > max_stepoff_dist {
            curve_stepoff_dist = max_stepoff_dist;
        }
        if !vert_at_edge_start {
            curve_stepoff_dist = -curve_stepoff_dist;
        }

        for j in 0..2 {
            let surf = solver_surface(fs1, j, OP)?;
            let uv = uvs.get(j).copied().unwrap_or(Point2::new(0.0, 0.0));
            let flags = EvaluateFlags {
                u_side: Side::Left,
                v_side: Side::Left,
                only_upper_half: true,
                non_zero_tangents: true,
            };
            let m = surf.evaluate(k, uv, 1, 1, flags)?;
            let du = m
                .get(1)
                .and_then(|r| r.first())
                .copied()
                .ok_or_else(|| defect(OP, SHORT_EVALUATION))?;
            let dv = m
                .first()
                .and_then(|r| r.get(1))
                .copied()
                .ok_or_else(|| defect(OP, SHORT_EVALUATION))?;
            let deg = rad2deg(du.angle_between(dv)?);
            if !(30.0..=150.0).contains(&deg) {
                // May lead to potential failures, step off the boundary: along the
                // edge first, then off the boundary edge
                param = arc_length_param(
                    k,
                    &curve,
                    param,
                    2.0 * curve_stepoff_dist,
                    owner_zone_tol_3d,
                    OP,
                )?;
                uvs = find_stepoff_uv(k, target, fs1, param, stepoff_dist)?;
                break;
            }
        }

        let domain1 = solver_surface(fs1, 0, OP)?.natural_uv_domain_with_nurb(k)?;
        let domain2 = solver_surface(fs1, 1, OP)?.natural_uv_domain_with_nurb(k)?;
        let mut found_solution = false;

        // If the solver doesn't succeed the first time, step off along the edge.
        for _ in 0..3 {
            let [u0, u1] = uvs;
            let (found, t) = point_on_plane_solve(
                k,
                solvers.solver_ops_mut(s1)?,
                None,
                plane_orig,
                plane_normal,
                &domain1,
                &domain2,
                u0,
                u1,
                &tsect,
            )?;
            found_solution = found;
            tsect = t;
            if found_solution {
                break;
            }

            // No solution found there, step off the vertex along the edge and again
            param = arc_length_param(k, &curve, param, curve_stepoff_dist, owner_zone_tol_3d, OP)?;
            uvs = find_stepoff_uv(k, target, solvers.solver(s1)?, param, stepoff_dist)?;
        }
        self.fillet_vertexuse_mut(vu1)?.set_tsect_pnt(&tsect);

        if !found_solution {
            self.fillet_vertex_mut(v)?
                .set_status(FilStatus::SolverNotConverge);
            return Ok(());
        }

        let face1 = as_face(self.fillet_vertex(v)?.point_class_object(), OP)?;
        let mut rail_index = 0;
        if eu1::face(target, eu)? != face1 {
            rail_index = 1;
        }
        let uv1 = tsect.uv_pos(srf(rail_index, OP)?);
        let pnt1 = face_surface(target, face1, OP)?.evaluate_point(k, uv1)?;
        self.set_processed(v, uv1, pnt1)?;

        let mut other_rail_index = 0;
        let vu2 = if total_geoms == 1 {
            other_rail_index = rail_index;
            self.vu_at_rail_end(v, Some(fillet_geom1), Some(vu1), None)?
        } else {
            let fillet_geom2 = *geoms.get(1).ok_or_else(|| defect(OP, SECOND_GEOM))?;
            let vu2 = self.vu_at_rail_end(v, Some(fillet_geom2), Some(vu1), None)?;
            let s2 = self.geom_solver_id(fillet_geom2, OP)?;
            let eu2 = solver_edgeuse(solvers.solver(s2)?, 0, OP)?;
            if eu1::face(target, eu2)? != face1 {
                other_rail_index = 1;
            }
            vu2
        };
        if let Some(vu2) = vu2 {
            let mut tsect2 = self.fillet_vertexuse(vu2)?.tsect_pnt().clone();
            *tsect2.uv_pos_mut(srf(other_rail_index, OP)?) = tsect.uv_pos(srf(rail_index, OP)?);
            *tsect2.uv_pos_mut(other_srf(other_rail_index, OP)?) =
                tsect.uv_pos(other_srf(rail_index, OP)?);
            self.fillet_vertexuse_mut(vu2)?.set_tsect_pnt(&tsect2);
        }

        // Calculate Mate if needed
        let Some(mate) = self.fillet_vertex(v)?.mate(0) else {
            return Ok(());
        };
        if self
            .vu_at_rail_end(mate, Some(fillet_geom1), None, None)?
            .is_none()
        {
            return Ok(());
        }
        let face2 = as_face(self.fillet_vertex(mate)?.point_class_object(), OP)?;
        let uv2 = tsect.uv_pos(other_srf(rail_index, OP)?);
        let pnt2 = face_surface(target, face2, OP)?.evaluate_point(k, uv2)?;
        self.set_processed(mate, uv2, pnt2)?;
        Ok(())
    }

    /// The fork's `SmFilletVertex::CalcJoinSection` (`SmBlendCornerJoin.cpp`, 543d25a;
    /// WR-697): a Join's shared section (`SM_FV_JOIN_SECTION` `v` and its mate), each
    /// blend's section in the plane through the corner's vertex square to the first
    /// blend's edge there. Each blend's section is its ball's: `PointOnPlaneSolve` on the
    /// blend's own faces, started from the ball's feet at the vertex (the vertex stepped
    /// the ball's radius along each face's binormal of the edge). It sets the blend's rail
    /// end here (and a closed blend's other one). The section's ends are the first
    /// blend's feet, on `v`'s face and on its mate's.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without corner, mate, faces, filleted vertex, solver, edge
    /// use, edge, curve, offset or base surface or rail-end vertex use; `SM_ERR` without a
    /// fillet geometry and, after setting `SM_FV_SOLVER_NOT_CONVERGE`, when a blend has
    /// no ball in the plane; a stationwise cliff join whose contact points differ;
    /// those of the evaluations and solves.
    pub fn calc_join_section(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        solvers: &mut [FilletSolver],
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_join_section_with_solvers(
            k,
            v,
            corners,
            &mut RailSolvers::Indexed(solvers),
            target,
        )
    }

    /// [`Self::calc_join_section`] with the live session's concrete solvers of the
    /// vertex's geometries (`PointOnPlaneSolve` is the concrete solver's virtual).
    ///
    /// # Errors
    /// As [`Self::calc_join_section`]; an unresolved geometry solver.
    pub fn calc_join_section_with_live_solvers(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        solvers: Vec<(FilletSolverId, &mut dyn FilletSolverOps)>,
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_join_section_with_solvers(
            k,
            v,
            corners,
            &mut RailSolvers::LiveSet(solvers),
            target,
        )
    }

    #[allow(clippy::too_many_lines)] // one fork routine
    fn calc_join_section_with_solvers(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        solvers: &mut RailSolvers<'_>,
        target: &mut Brep,
    ) -> Result<()> {
        const OP: &str = OP_JOIN;
        let corner = self
            .fillet_vertex(v)?
            .fillet_corner()
            .ok_or_else(|| no_corner(OP))?;
        let mate = self
            .fillet_vertex(v)?
            .mate(0)
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::Mate))?;
        let first_face = as_face(self.fillet_vertex(v)?.point_class_object(), OP)?
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::PointClassFace))?;
        let other_face = as_face(self.fillet_vertex(mate)?.point_class_object(), OP)?
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::PointClassFace))?;
        let owner = corners.corner(corner)?;
        let at_cliff = owner
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_cliff_join);
        let cp_vertex = owner
            .filleted_vertex()
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::CornerVertex))?;

        let geoms = self.vertex_fillet_geoms(v)?;
        if geoms.is_empty() {
            return Err(invalid_set_up(OP, FilletSetUp::VertexGeoms));
        }

        // the plane: through the vertex, square to the first blend's edge
        let mut plane_orig = Point3::default();
        let mut plane_normal = Vec3::default();
        // Not the fork's (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): whether
        // a blend of the join rolled a rail over a smooth far edge of its face
        let mut any_rolled = false;
        let mut stationwise = false;
        for &g in &geoms {
            let fs = solvers.solver(self.geom_solver_id(g, OP)?)?;
            any_rolled |= fs.rolled_faces.iter().any(Option::is_some);
            stationwise |= fs.station_setback().is_some();
        }
        let check_cliff = at_cliff && stationwise && !any_rolled && geoms.len() == 2;
        let mut cliff_section = None;
        let mut ends = None;
        for (gg, &g) in geoms.iter().enumerate() {
            let sid = self.geom_solver_id(g, OP)?;
            let fs = solvers.solver(sid)?;
            let eu = solver_edgeuse(fs, 0, OP)?;
            let e = eu1::edge(target, eu)?.ok_or_else(|| edge_use_without_edge(OP, eu))?;
            let curve = edge::curve(target, e)?
                .ok_or_else(|| edge_without_curve(OP, e))?
                .clone();
            let ivl = edge::interval(target, e)?;
            let param = if edge::start_vertex(target, e)? == Some(cp_vertex) {
                ivl.min()
            } else {
                ivl.max()
            };
            if gg == 0 {
                let pv = curve.evaluate(k, param, 1, true, true)?;
                plane_orig = pv
                    .first()
                    .copied()
                    .ok_or_else(|| defect(OP, SHORT_EVALUATION))?;
                plane_normal = pv
                    .get(1)
                    .copied()
                    .ok_or_else(|| defect(OP, SHORT_EVALUATION))?;
            }

            // the ball's feet at the vertex, on each of the blend's faces
            let mut uvs = [Point2::new(0.0, 0.0); 2];
            for (ii, slot) in uvs.iter_mut().enumerate() {
                let fs = solvers.solver(sid)?;
                let side_eu = solver_edgeuse(fs, ii, OP)?;
                let offset = solver_surface(fs, ii, OP)?;
                let distance = offset.offset_distance().abs();
                let base = offset.base_surface().clone();
                let b = evaluate_binormal(
                    k,
                    target,
                    side_eu,
                    param,
                    false,
                    &BinormalOptions::default(),
                )?;
                let bin = b.binormal * distance;
                // Not the fork's (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): a
                // rail rolled over a smooth far edge of its face rolls on the face beyond, whose
                // surface the edge use's parameters are not on. Its foot is the vertex stepped
                // into the edge use's face, taken to the nearest point of the rolled-over edge
                // (on the face beyond) and dropped on that face's surface.
                // The join is on the supports continued across the sharp rim.
                // Project onto the infinite plane. A trimmed drop would pin this
                // foot to the old rim and shorten the chord.
                if at_cliff {
                    if let SurfaceKind::Plane(plane) = base.kind() {
                        *slot = plane.project_point_to_uv_domain(b.point + bin);
                        continue;
                    }
                }
                if let Some(onto) = fs.rolled_faces.get(ii).copied().flatten() {
                    let tangency = fs.tangency_tolerance;
                    let foot = b.point + bin;
                    let Some(from) = eu1::face(target, side_eu)? else {
                        return Err(Failure::new(
                            OP,
                            BlendFailure::Solver(SolverFailure::EdgeUseWithoutFace),
                        )
                        .with_status(SM_ERR)
                        .into_error());
                    };
                    let seed = crate::blend::fillet_tangent_rollover::rolled_edge_point(
                        k, target, from, onto, foot, tangency,
                    )?
                    .unwrap_or(foot);
                    let domain = base.natural_uv_domain_with_nurb(k)?;
                    let dropped = base.drop_point::<NoObject>(
                        k,
                        seed,
                        &domain,
                        &DropPointOptions::default(),
                    )?;
                    if !dropped.success {
                        // the rolled rail's foot does not drop onto the face beyond
                        return Err(defect(OP, FOOT_NO_DROP));
                    }
                    *slot = dropped.result_uv;
                    continue;
                }
                // Not the fork's: the foot is the surface's point nearest the vertex stepped
                // along the binormal, solved in 3D from the edge use's parameters, which keep it
                // on the edge use's side of a seam. The fork stepped the parameters by the
                // binormal's first-order image, which is no foot where the parameterisation is
                // singular on or near the step: at a sphere's pole the image is unbounded, and
                // from a vertex a few degrees off a pole the step crosses it onto the wrong
                // meridian. The section solve then starts away from the ball and cannot reach it.
                let domain = base.natural_uv_domain_with_nurb(k)?;
                let dropped = base.drop_point::<NoObject>(
                    k,
                    b.point + bin,
                    &domain,
                    &DropPointOptions {
                        guess_uv: Some(b.uv),
                        ..DropPointOptions::default()
                    },
                )?;
                if !dropped.success {
                    // the foot does not drop onto the surface: a defect, as for a rolled
                    // rail's foot above
                    return Err(defect(OP, FOOT_NO_DROP));
                }
                // a closed surface names the foot once per period: take the name nearest the
                // edge use's parameters, as the section solve continues from them across a seam
                // (`period_name_nearest`, which leaves a direction collapsing there, a sphere's
                // pole, alone).
                let foot = crate::blend::fillet_solver::p1::period_name_nearest(
                    k,
                    &base,
                    dropped.result_uv,
                    b.uv,
                )?;
                *slot = foot;
            }

            let (vu, tsect) = self.rail_end_tsect(v, Some(g), None, OP)?;
            let fs = solvers.solver(sid)?;
            let mut domain1 = solver_surface(fs, 0, OP)?.natural_uv_domain_with_nurb(k)?;
            let mut domain2 = solver_surface(fs, 1, OP)?.natural_uv_domain_with_nurb(k)?;
            let [uv0, uv1] = uvs;
            if at_cliff {
                domain1.add_point(uv0);
                domain2.add_point(uv1);
            }
            let (found, tsect) = point_on_plane_solve(
                k,
                solvers.solver_ops_mut(sid)?,
                // The constant-distance solver seeds from the other edge use.
                (at_cliff && stationwise).then_some(&mut *target),
                plane_orig,
                plane_normal,
                &domain1,
                &domain2,
                uv0,
                uv1,
                &tsect,
            )?;
            if !found {
                if !check_cliff {
                    self.fillet_vertexuse_mut(vu)?.set_tsect_pnt(&tsect);
                }
                self.fillet_vertex_mut(v)?
                    .set_status(FilStatus::SolverNotConverge);
                // SER_MSG: no ball of the blend lies in the shared section's plane
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::JoinSectionNotConverged { blend: gg },
                ));
            }
            if check_cliff {
                let fs = solvers.solver(sid)?;
                let first = self.rail_index_of_vertexuse(g, Some(vu))?;
                let feet = cliff_section_feet(k, fs, &tsect, first)?;
                let tol = fs.approx_tol_3d.max(target.tolerance());
                let rail_ends = [Some(vu), self.vu_at_rail_end(v, Some(g), Some(vu), None)?];
                if let Some((reference_ends, reference_tsect, reference, reference_tol)) =
                    cliff_section.take()
                {
                    check_cliff_section(reference, feet, reference_tol + tol)?;
                    for (ends, point) in [(reference_ends, &reference_tsect), (rail_ends, &tsect)] {
                        for end in ends.into_iter().flatten() {
                            self.fillet_vertexuse_mut(end)?.set_tsect_pnt(point);
                        }
                    }
                } else {
                    let [p_first, p_other] = feet;
                    ends = Some((
                        tsect.uv_pos(srf(first, OP)?),
                        p_first,
                        tsect.uv_pos(other_srf(first, OP)?),
                        p_other,
                    ));
                    cliff_section = Some((rail_ends, tsect, feet, tol));
                }
                continue;
            }

            self.fillet_vertexuse_mut(vu)?.set_tsect_pnt(&tsect);
            // a closed blend's other rail end here
            if let Some(other_vu) = self.vu_at_rail_end(v, Some(g), Some(vu), None)? {
                self.fillet_vertexuse_mut(other_vu)?.set_tsect_pnt(&tsect);
            }

            // the section's ends: the first blend's feet. Not the fork's (solidcore-native,
            // [`crate::blend::fillet_tangent_rollover`]): where a blend of the join rolled a rail
            // over a smooth far edge of its face, those of the first blend whose rails roll on
            // the section's two faces, a rolled rail on the face beyond (`join::rolled_sides`
            // puts the section's end there when only the other blend rolls over); a blend
            // still on its own face there has its feet on that face's surface, not the face
            // beyond's
            let first = if !any_rolled {
                (gg == 0).then_some(usize::from(eu1::face(target, eu)? != Some(first_face)))
            } else if ends.is_some() {
                None
            } else {
                let fs = solvers.solver(sid)?;
                let mut rolls_on = [None; 2];
                for (rail, slot) in rolls_on.iter_mut().enumerate() {
                    *slot = match fs.rolled_faces.get(rail).copied().flatten() {
                        Some(face) => Some(face),
                        None => eu1::face(target, solver_edgeuse(fs, rail, OP)?)?,
                    };
                }
                match rolls_on {
                    [a, b] if a == Some(first_face) && b == Some(other_face) => Some(0),
                    [a, b] if b == Some(first_face) && a == Some(other_face) => Some(1),
                    _ => None,
                }
            };
            if let Some(first) = first {
                let uv_first = tsect.uv_pos(srf(first, OP)?);
                let uv_other = tsect.uv_pos(other_srf(first, OP)?);
                let foot_on_support = |face, uv| -> Result<Point3> {
                    let surface = face_surface(target, Some(face), OP)?;
                    if at_cliff && matches!(surface.kind(), SurfaceKind::Plane(_)) {
                        let mut extended = surface.clone_for_evaluation()?;
                        crate::surface::surface::p3::set_out_of_bounds_enabled(&mut extended, true);
                        return extended.evaluate_point(k, uv);
                    }
                    surface.evaluate_point(k, uv)
                };
                let p_first = foot_on_support(first_face, uv_first)?;
                let p_other = foot_on_support(other_face, uv_other)?;
                ends = Some((uv_first, p_first, uv_other, p_other));
            }
        }
        if let Some((uv_first, p_first, uv_other, p_other)) = ends {
            self.set_processed(v, uv_first, p_first)?;
            self.set_processed(mate, uv_other, p_other)?;
        } else if any_rolled {
            // no blend of the join rolls on the two faces its shared section ends on
            return Err(SolidError::blend(
                OP,
                BlendFailure::JoinBlendsRollOffSection,
            ));
        }
        Ok(())
    }

    /// `SmFilletVertex::CalcSetBackVert`: the vertex on the plane normal to the
    /// filleted edge at the corner's set-back distance from the corner vertex, solved by
    /// `PointOnPlaneSolve` from the plane origin's closest points on the solver's
    /// surfaces; its mate gets the matching point.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without corner, solver, edge use, curve, surface or face;
    /// `SM_ERR` without exactly one fillet geometry and unless each surface has exactly
    /// one closest point; those of the evaluations and solves; those of [`CornerLookup::corner`]
    /// (`corners` resolves the vertex's corner).
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn calc_set_back_vert(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        solvers: &mut [FilletSolver],
        target: &Brep,
    ) -> Result<()> {
        self.calc_set_back_vert_with_solvers(
            k,
            v,
            corners,
            &mut RailSolvers::Indexed(solvers),
            target,
        )
    }

    /// [`Self::calc_set_back_vert`] with the live session's concrete solver of the
    /// vertex's geometry (`PointOnPlaneSolve` is the concrete solver's virtual).
    ///
    /// # Errors
    /// As [`Self::calc_set_back_vert`]; an unresolved geometry solver.
    pub fn calc_set_back_vert_with_live_solvers(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        solvers: Vec<(FilletSolverId, &mut dyn FilletSolverOps)>,
        target: &Brep,
    ) -> Result<()> {
        self.calc_set_back_vert_with_solvers(
            k,
            v,
            corners,
            &mut RailSolvers::LiveSet(solvers),
            target,
        )
    }

    #[allow(clippy::too_many_lines)] // one upstream routine
    fn calc_set_back_vert_with_solvers(
        &mut self,
        k: &Kernel,
        v: VertexId,
        corners: CornerLookup<'_>,
        solvers: &mut RailSolvers<'_>,
        target: &Brep,
    ) -> Result<()> {
        const OP: &str = OP_SETBACK;
        let corner = self
            .fillet_vertex(v)?
            .fillet_corner()
            .ok_or_else(|| no_corner(OP))?;
        let cp_vertex = corners.corner(corner)?.filleted_vertex();

        let geoms = self.vertex_fillet_geoms(v)?;
        let [fillet_geom] = *geoms.as_slice() else {
            return Err(invalid_set_up(OP, FilletSetUp::VertexGeoms));
        };
        let si = self.geom_solver_id(fillet_geom, OP)?;
        let fs = solvers.solver(si)?;
        let eu = solver_edgeuse(fs, 0, OP)?;
        let e = eu1::edge(target, eu)?.ok_or_else(|| edge_use_without_edge(OP, eu))?;
        // each edge's own (the fork's 61547af, GetSetBackDistAlong; the corner's one setback
        // elsewhere)
        let set_back_dist = corners.corner(corner)?.set_back_dist_along(target, e)?;
        let (plane_orig, plane_normal) =
            set_back_frame(k, target, e, cp_vertex, set_back_dist, OP)?;

        let surf1 = solver_surface(fs, 0, OP)?;
        let surf2 = solver_surface(fs, 1, OP)?;
        let domain1 = surf1.natural_uv_domain_with_nurb(k)?;
        let domain2 = surf2.natural_uv_domain_with_nurb(k)?;
        let (vu, tsect) = self.rail_end_tsect(v, Some(fillet_geom), None, OP)?;

        let tol = fs.this_approx_tol_3d();
        let closest = |surf: &crate::surface::offset_surface::OffsetSurface,
                       domain: &Extent2d|
         -> Result<Point2> {
            let surf = Surface::from(surf.clone());
            // the solutions name the surface (`this` of `GlobalPointSolve`), as upstream's
            // do: `IdenticalSolutions` merges near-duplicates within `1e-4` of its domain
            // before the `!= 1` count below
            let solutions = surf.global_point_solve(
                k,
                Some(SurfaceHandle(&surf)),
                domain,
                SolverOperation::Minimize,
                plane_orig,
                zone_tol3d(k, tol),
                None,
                SolutionRequested::All,
            )?;
            let s = match solutions.as_slice() {
                [s] => s,
                [] => return Err(defect(OP, NO_CLOSEST_POINT)),
                feet => {
                    return Err(SolidError::blend(
                        OP,
                        BlendFailure::SetBackFootNotUnique { feet: feet.len() },
                    ))
                }
            };
            Ok(Point2::new(s.start.param(0)?, s.start.param(1)?))
        };
        let uv1 = closest(surf1, &domain1)?;
        let uv2 = closest(surf2, &domain2)?;

        // find filletPoint on given plane that satisfies geometry requirements
        let (found, tsect) = point_on_plane_solve(
            k,
            solvers.solver_ops_mut(si)?,
            None,
            plane_orig,
            plane_normal,
            &domain1,
            &domain2,
            uv1,
            uv2,
            &tsect,
        )?;
        self.fillet_vertexuse_mut(vu)?.set_tsect_pnt(&tsect);
        if !found {
            self.fillet_vertex_mut(v)?
                .set_status(FilStatus::SolverNotConverge);
            return Ok(());
        }
        let face1 = as_face(self.fillet_vertex(v)?.point_class_object(), OP)?
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::PointClassFace))?;
        let mut rail_index = 1;
        if eu1::face(target, eu)? == Some(face1) {
            rail_index = 0;
        }
        let uv1 = tsect.uv_pos(srf(rail_index, OP)?);
        let pnt1 = face_surface(target, Some(face1), OP)?.evaluate_point(k, uv1)?;
        self.set_processed(v, uv1, pnt1)?;

        // Calculate Mate if needed
        let Some(mate) = self.fillet_vertex(v)?.mate(0) else {
            return Ok(());
        };
        let Some(vu2) = self.vu_at_rail_end(mate, Some(fillet_geom), None, None)? else {
            return Ok(());
        };
        let mut tsect2 = self.fillet_vertexuse(vu2)?.tsect_pnt().clone();
        for s in [SrfIndex::First, SrfIndex::Second] {
            *tsect2.uv_pos_mut(s) = tsect.uv_pos(s);
        }
        self.fillet_vertexuse_mut(vu2)?.set_tsect_pnt(&tsect2);
        let face2 = as_face(self.fillet_vertex(mate)?.point_class_object(), OP)?;
        let uv2 = tsect.uv_pos(other_srf(rail_index, OP)?);
        let pnt2 = face_surface(target, face2, OP)?.evaluate_point(k, uv2)?;
        self.set_processed(mate, uv2, pnt2)?;
        Ok(())
    }

    // ---- fillet edges ----------------------------------------------------------------

    /// The first `SM_FE_RAIL` edge at fillet vertex `v` (`GetEdges` order).
    pub(crate) fn first_rail_at(&self, v: VertexId) -> Result<Option<EdgeId>> {
        for e in vertex::edges(self.brep(), v, None)? {
            if self.fillet_edge(e)?.fillet_edge_type() == FilletEdgeType::Rail {
                return Ok(Some(e));
            }
        }
        Ok(None)
    }

    /// `SmFilletEdge::CalcRailRailInterpolation(crContext, pOptCorner)`: the curve of a
    /// rail/rail interpolation edge between its two fillet vertices, tangent to the
    /// incoming rails: in 3D on a planar original face, when a vertex is not a mate or
    /// when a rail has no UV curve (then lifted, reparametrised by arc length and
    /// dropped again), else in the face's UV space (lifted to 3D). The edge gets the 3D
    /// curve, its mate edge use the UV curve. With a corner, the corner's cross-section
    /// solver picks the continuity, and a concave `NxN` corner may replace the curve by a
    /// cubic rail/rail interpolant. At a corner with an ease (the fork's, `e74bbd3`) the
    /// curve is the faired G2 (or G3) interpolant of the rails' ends instead, meeting the
    /// rails curvature-continuously. The drops and lifts run on the original face's own
    /// surface (`target` holds the face): its trimmed surface cache and the face
    /// tolerance it carries.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without original face, B-spline surface, rail, edge use or
    /// curve; `SM_ERR` when a vertex does not drop onto its rail within 100 times the
    /// tolerance or a drop gives other than one UV curve; those of the evaluations,
    /// constructions, lifts and `SmSurface::DropCurve` (and of the face's surface cache
    /// the drops make); those of [`CornerLookup::corner`] (`corners`
    /// resolves `opt_corner`) and of the corner's `TestRailRailInterpolationCurve`.
    pub fn calc_rail_rail_interpolation(
        &mut self,
        k: &Kernel,
        e: EdgeId,
        opt_corner: Option<FilletCornerId>,
        corners: CornerLookup<'_>,
        solvers: &mut [FilletSolver],
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_rail_rail_interpolation_with_solvers(
            k,
            e,
            opt_corner,
            corners,
            &mut InterpolationSolvers::Indexed(solvers),
            target,
        )
    }

    /// [`Self::calc_rail_rail_interpolation`] with the retained solvers of a live
    /// executive session ([`super::p5::FilletEdgeContext`]).
    ///
    /// # Errors
    /// As [`Self::calc_rail_rail_interpolation`]; those of
    /// [`super::p5::FilletEdgeContext::solver_pair_ops_mut`] where a concave `NxN`
    /// corner's curve crosses the corner (the cubic interpolant's rail/rail solve runs
    /// on the live concrete solvers).
    pub fn calc_rail_rail_interpolation_with_context(
        &mut self,
        k: &Kernel,
        e: EdgeId,
        opt_corner: Option<FilletCornerId>,
        corners: CornerLookup<'_>,
        context: &mut dyn super::p5::FilletEdgeContext,
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_rail_rail_interpolation_with_solvers(
            k,
            e,
            opt_corner,
            corners,
            &mut InterpolationSolvers::Context(context),
            target,
        )
    }

    #[allow(clippy::too_many_lines)] // one upstream routine
    fn calc_rail_rail_interpolation_with_solvers(
        &mut self,
        k: &Kernel,
        e: EdgeId,
        opt_corner: Option<FilletCornerId>,
        corners: CornerLookup<'_>,
        solvers: &mut InterpolationSolvers<'_>,
        target: &mut Brep,
    ) -> Result<()> {
        const OP: &str = OP_INTERP;
        let mut fsg_type = FilletSurfaceGeneratorType::Circular; // Default
        let mut num_derivatives: usize = 1; // 0 - linear; 1, 2, or 3 for G1, G2 or G3

        // when given a corner - use its properties to pick a lNumDerivatives value
        if let Some(corner) = opt_corner {
            let mut fillet_edges = Vec::new();
            corners.corner(corner)?.fillet_edges(&mut fillet_edges);
            let total_edges = fillet_edges.len();
            if total_edges <= 4 {
                let index = fillet_edges
                    .iter()
                    .position(|&x| x == e)
                    .ok_or_else(|| invalid_set_up(OP, FilletSetUp::InterpolationEdgeAtCorner))?;
                let test_edge = *fillet_edges
                    .get((index + 2) % total_edges)
                    .ok_or_else(|| defect(OP, INDEX_IN_LIST))?;
                if self.fillet_edge(test_edge)?.fillet_edge_type() != FilletEdgeType::CrossSection {
                    return Err(invalid_set_up(OP, FilletSetUp::OppositeCrossSection));
                }
                let prim_eu = primary_edgeuse(self.brep(), test_edge)?
                    .ok_or_else(|| missing_set_up(OP, FilletSetUp::PrimaryEdgeUse))?;
                let fillet_geom = self
                    .fillet_edgeuse(prim_eu)?
                    .fillet_geom()
                    .ok_or_else(|| missing_set_up(OP, FilletSetUp::CrossSectionGeom))?;
                let id = self.geom_solver_id(fillet_geom, OP)?;
                let fsg = solvers
                    .solver(id, OP)?
                    .fillet_surface_generator()
                    .ok_or_else(|| missing_set_up(OP, FilletSetUp::SurfaceGenerator))?;
                fsg_type = fsg.fillet_surface_generator_type();
                num_derivatives = match fsg_type {
                    FilletSurfaceGeneratorType::Linear => 0,
                    FilletSurfaceGeneratorType::BlendCurve => {
                        usize::try_from(fsg.continuity().unwrap_or(1)).unwrap_or(1)
                    }
                    _ => 1,
                };
            }
        }
        // The fork's (tangency-labs/smlib `e74bbd3`, #1376): at a corner whose rounds ease
        // into their curvature-continuous sections before the setback
        // (`SmFilletCorner::m_dEaseDist`), the curve meets each rail curvature-continuously:
        // the G2 interpolation of the rails' ends below, which meets a straight rail with
        // zero curvature. Without an ease (the default) nothing changes.
        let meet_rails_g2 = match opt_corner {
            Some(corner) => corners.corner(corner)?.ease_dist() > 0.0,
            None => false,
        };
        if meet_rails_g2 && num_derivatives < 2 {
            num_derivatives = 2;
        }
        // sPntVec[4]: at most three derivatives are read
        let num_derivatives = num_derivatives.min(3);

        let start_fv = edge::vertex(self.brep(), e)?
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::EdgeVertices))?;
        let end_fv = edge::other_vertex(self.brep(), e, Some(start_fv))?
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::EdgeVertices))?;

        let face = self
            .fillet_edge(e)?
            .original_face()
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::OriginalFace))?;
        // the face's own surface (evaluated as a copy that keeps its owner and the face
        // tolerance: the drops below make its trimmed surface cache on `target`
        // through the face, as `smsurf_GetSurfaceCache` reaches `GetFace()`; the cubic
        // interpolant needs the target B-rep mutably)
        let face_srf = face2::surface(target, face)?
            .ok_or_else(|| face_without_surface(OP, face))?
            .clone_for_evaluation()?;
        let face_srf = &face_srf;
        let handle = Some(CacheObject::Surface(face));
        // SM_CAST_PTR(SmBSplineSurface, ..) and NER: every kind derived from
        // SmBSplineSurface (planes, cones and cylinders, spheres, tori, surfaces of
        // revolution and extrusion), with the NURB its GetDegree reads
        let surf_net = crate::topology::brep::p3::with_nurb(k, face_srf)?.ok_or_else(|| {
            Failure::new(
                OP,
                BlendFailure::MateFaceSurfaceNotBSpline {
                    face,
                    kind: face_srf.type_string(),
                },
            )
            .at(EntityRef::Face(face))
            .into_error()
        })?;
        let uv_domain = face_srf.natural_uv_domain_with_nurb(k)?;
        let tol = face2::tolerance(target, face)?;

        // Determine if we want to work on 2D or 3D
        let mut project_and_lift = false;
        let mut create_crv_on_3d = false;
        let is_plane = match Plane::is_nurb_surface_plane(k, &surf_net, 1.0) {
            Ok(p) => p.is_some(),
            Err(err) if err.is_unported_or_cancelled() => return Err(err),
            // an SER status inside is a TRUE return upstream
            Err(_) => true,
        };
        if is_plane
            || self.fillet_vertex(start_fv)?.fillet_vertex_type() != FilletVertexType::Mate
            || self.fillet_vertex(end_fv)?.fillet_vertex_type() != FilletVertexType::Mate
        {
            // planar face, or the special 4x2 concave cases
            create_crv_on_3d = true;
        }

        // Get "incoming" rail-curve parameter at start vertex
        let rail1 = self
            .first_rail_at(start_fv)?
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::InterpolationEndRail))?;
        let eu1_id = primary_edgeuse(self.brep(), rail1)?
            .and_then(|p| eu1::mate(self.brep(), p).ok().flatten())
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::PrimaryEdgeUseMate))?;
        let mut curve1: Option<Curve> = eu1::uv_trim_curve(self.brep(), eu1_id)?.0.cloned();
        if !create_crv_on_3d && curve1.is_none() {
            create_crv_on_3d = true;
            project_and_lift = true;
        }

        let mut start_pnt;
        if create_crv_on_3d {
            curve1 = edge::curve(self.brep(), rail1)?.cloned();
            start_pnt = self.vertex_point(start_fv)?;
        } else {
            let uv = self.fillet_vertex(start_fv)?.original_uv();
            start_pnt = Point3::new(uv.x, uv.y, 0.0);
        }
        let curve1 = curve1.ok_or_else(|| missing_set_up(OP, FilletSetUp::RailCurve))?;

        let mut dropping_tol = tol;
        if !create_crv_on_3d {
            dropping_tol = (uv_domain.size().max_dimension() + 1.0) * EFF_ZERO;
        }

        let drop_onto = |curve: &Curve, pnt: Point3| -> Result<f64> {
            let mut scale = 1.0;
            while scale < 100.1 {
                let d = curve.drop_point(
                    k,
                    &curve.natural_interval(),
                    pnt,
                    None,
                    scale * dropping_tol,
                    None,
                    SolverOperation::Minimize,
                )?;
                if d.success {
                    return Ok(d.parameter);
                }
                scale *= 10.0;
            }
            Err(invalid_set_up(OP, FilletSetUp::RailEndOffRail))
        };
        let param1 = drop_onto(&curve1, start_pnt)?;

        let orient1 = eu1::orientation(self.brep(), eu1_id)?;
        let rail1_vertex = edge::vertex(self.brep(), rail1)?;
        let same_orient1 = !((rail1_vertex != Some(start_fv) && orient1 == OrientType::Same)
            || (rail1_vertex == Some(start_fv) && orient1 == OrientType::Opposite));

        // Get "incoming" rail-curve parameter at end vertex
        let rail2 = self
            .first_rail_at(end_fv)?
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::InterpolationEndRail))?;
        let eu2_id = primary_edgeuse(self.brep(), rail2)?
            .and_then(|p| eu1::mate(self.brep(), p).ok().flatten())
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::PrimaryEdgeUseMate))?;
        let mut curve2: Option<Curve> = eu1::uv_trim_curve(self.brep(), eu2_id)?.0.cloned();
        if !create_crv_on_3d && curve2.is_none() {
            create_crv_on_3d = true;
            project_and_lift = true;
        }

        let end_pnt;
        if create_crv_on_3d {
            curve2 = edge::curve(self.brep(), rail2)?.cloned();
            start_pnt = self.vertex_point(start_fv)?;
            end_pnt = self.vertex_point(end_fv)?;
        } else {
            let uv = self.fillet_vertex(end_fv)?.original_uv();
            end_pnt = Point3::new(uv.x, uv.y, 0.0);
        }
        let curve2 = curve2.ok_or_else(|| missing_set_up(OP, FilletSetUp::RailCurve))?;
        let param2 = drop_onto(&curve2, end_pnt)?;

        let orient2 = eu1::orientation(self.brep(), eu2_id)?;
        let rail2_vertex = edge::vertex(self.brep(), rail2)?;
        let same_orient2 = (rail2_vertex != Some(end_fv) && orient2 == OrientType::Same)
            || (rail2_vertex == Some(end_fv) && orient2 == OrientType::Opposite);

        // Calc tangent (and higher derivatives) from the rail at start vertex
        // (the fork's, `e74bbd3`: run backwards, the odd derivatives change sign, the even
        // ones do not; upstream negates every derivative. Only the first derivative is read
        // unless the curve meets the rails G2, so without an ease this is the pinned result.)
        let mut pnt_vec1 = curve1.evaluate(k, param1, num_derivatives, true, true)?;
        if !same_orient1 {
            for d in pnt_vec1.iter_mut().skip(1).take(num_derivatives).step_by(2) {
                *d = -*d;
            }
        }
        // Calc tangent from the other rail
        let mut pnt_vec2 = curve2.evaluate(k, param2, num_derivatives, true, true)?;
        if !same_orient2 {
            for d in pnt_vec2.iter_mut().skip(1).take(num_derivatives).step_by(2) {
                *d = -*d;
            }
        }

        let new_curve = if fsg_type == FilletSurfaceGeneratorType::Linear {
            // Linear cases
            BSplineCurve::create_line_segment(3, start_pnt, end_pnt)?
        } else if !meet_rails_g2 {
            // In G1 cases, we create the 3d curve and then drop it onto the surface
            // (upstream's `TRUE` branch; the fork's `!bMeetRailsG2`: without an ease the
            // G2/G3 fairing branch below is unreachable, as upstream)
            let mut start_vec = pnt_vec1.get(1).copied().unwrap_or(Vec3::UNDEF);
            let mut end_vec = pnt_vec2.get(1).copied().unwrap_or(Vec3::UNDEF);
            interpolate_endpts_and_tangents(start_pnt, end_pnt, &mut start_vec, &mut end_vec, 3)?
        } else {
            // G1, G2 & G3 cases (reached only at an eased corner, the fork's `e74bbd3`)
            g2_rail_rail_curve(
                start_pnt,
                end_pnt,
                &mut pnt_vec1,
                &mut pnt_vec2,
                num_derivatives,
            )?
        };

        // The drop and the re-drop share the face's surface cache (the lift reads the
        // surface's own continuities and makes none): upstream keeps it in the surface's
        // cache slot, the plain cache smsurf_GetSurfaceCache falls back to when the
        // trimmed build fails included, and nothing edits the face between these calls
        let mut services = BrepFaceServices::new(target);

        // p3DCurve, an SmBSplineCurve: a B-spline, or the line / conic iso curve
        // LiftCurve's sm_ExtractIsoCurve makes
        let mut curve_3d: Option<Curve>;
        let mut uv_curve: Option<Curve>;
        if create_crv_on_3d {
            // Compute UV-curve by projecting 3d curve onto the surface
            let c3 = Curve::from(new_curve);
            let uv_curves = surface_drop_curve_with_context(
                k,
                face_srf,
                &uv_domain,
                &c3,
                &c3.natural_interval(),
                tol,
                DropSolveContext {
                    solver: &mut services,
                    handle,
                },
            )?;
            // Will replace the original UV-curve when we have only one result
            if uv_curves.len() != 1 {
                return Err(Failure::new(
                    OP,
                    BlendFailure::InterpolantDropsToCurves {
                        face,
                        curves: uv_curves.len(),
                    },
                )
                .at(EntityRef::Face(face))
                .into_error());
            }
            uv_curve = uv_curves.into_iter().next();
            curve_3d = Some(c3);
        } else {
            let mut uv = new_curve;
            // its status is ignored upstream; fork 9b7cd48 passes this on (SER)
            uv.convert_to_2d()?;
            uv_curve = Some(Curve::from(uv));
            curve_3d = None;
        }

        if !create_crv_on_3d || project_and_lift {
            // Lift the 3d curve
            let uv = uv_curve
                .as_ref()
                .ok_or_else(|| broken_link(OP, NO_UV_CURVE))?;
            let lifted = face_srf.lift_curve(
                k,
                &uv_domain,
                uv,
                &uv.natural_interval(),
                approx_tol3d(k, tol),
                false,
            )?;
            let mut c3 = lifted.curve;

            // Reparametrize the curve with arc-length for non-blend type
            if fsg_type != FilletSurfaceGeneratorType::BlendCurve {
                b_spline_base_mut(&mut c3)
                    .ok_or_else(|| broken_link(OP, LIFT_NOT_BSPLINE))?
                    .reparametrize_with_arc_length()?;
                // Reproject the 3d curve back onto the surface
                let uv_curves = surface_drop_curve_with_context(
                    k,
                    face_srf,
                    &uv_domain,
                    &c3,
                    &c3.natural_interval(),
                    tol,
                    DropSolveContext {
                        solver: &mut services,
                        handle,
                    },
                )?;
                // the UV curve may cross the seam of a closed surface or leave its
                // domain (the 2D interpolant is built in UV), so the re-drop need not
                // give one curve
                if uv_curves.len() != 1 {
                    return Err(Failure::new(
                        OP,
                        BlendFailure::InterpolantDropsToCurves {
                            face,
                            curves: uv_curves.len(),
                        },
                    )
                    .at(EntityRef::Face(face))
                    .into_error());
                }
                uv_curve = uv_curves.into_iter().next();
            }
            curve_3d = Some(c3);
        }
        // the services lend `target`, which the corner test reads and the cubic
        // interpolant below changes
        drop(services);

        if let Some(corner) = opt_corner {
            let the_corner = corners.corner(corner)?;
            if the_corner.corner_type() == FilletCornerType::NByNConcave {
                // p3DCurve is an SmBSplineCurve* (its virtuals dispatch on the curve)
                let c3 = curve_3d
                    .as_ref()
                    .filter(|c| b_spline_base(c).is_some())
                    .ok_or_else(|| broken_link(OP, LIFT_NOT_BSPLINE))?;
                // Test and see if this edge intersects with the original corner
                if the_corner.test_rail_rail_interpolation_curve(k, target, c3)? {
                    // Corner vertex is on the opposite side of the curve
                    let solver_of_single_geom =
                        |fb: &Self, fv: VertexId| -> Result<FilletSolverId> {
                            let geoms = fb.vertex_fillet_geoms(fv)?;
                            let [g] = *geoms.as_slice() else {
                                return Err(invalid_set_up(OP, FilletSetUp::VertexGeoms));
                            };
                            let id = fb.geom_solver_id(g, OP)?;
                            solvers.solver(id, OP)?;
                            Ok(id)
                        };
                    let si1 = solver_of_single_geom(self, start_fv)?;
                    let si2 = solver_of_single_geom(self, end_fv)?;

                    // Re-compute UV-curve
                    let uv = uv_curve
                        .as_ref()
                        .ok_or_else(|| broken_link(OP, NO_UV_CURVE))?;
                    let ivl = uv.natural_interval();
                    let pv = uv.evaluate(k, ivl.min(), 1, true, true)?;
                    let start_uv_pnt = pv
                        .first()
                        .copied()
                        .ok_or_else(|| defect(OP, SHORT_EVALUATION))?;
                    let start_uv_vec = pv
                        .get(1)
                        .copied()
                        .ok_or_else(|| defect(OP, SHORT_EVALUATION))?;
                    let pv = uv.evaluate(k, ivl.max(), 1, true, true)?;
                    let end_uv_pnt = pv
                        .first()
                        .copied()
                        .ok_or_else(|| defect(OP, SHORT_EVALUATION))?;
                    let end_uv_vec = -pv
                        .get(1)
                        .copied()
                        .ok_or_else(|| defect(OP, SHORT_EVALUATION))?;
                    let corner_vertex = corners
                        .corner(corner)?
                        .filleted_vertex()
                        .ok_or_else(|| missing_set_up(OP, FilletSetUp::CornerVertex))?;
                    // WR-300 (the fork's tangency-labs/smlib@465b62b): the rails' meeting
                    // point holds the radii only at a corner calculated as constant radius.
                    let hold_end_radii = the_corner.is_calc_as_const_rad();
                    let new_uv = solvers.cubic(si1, si2, OP, |s1, s2| {
                        create_cubic_rail_rail_interpolant(
                            k,
                            target,
                            s1,
                            s2,
                            face,
                            corner_vertex,
                            start_uv_pnt,
                            start_uv_vec,
                            end_uv_pnt,
                            end_uv_vec,
                            0.3,
                            hold_end_radii,
                        )
                    })?;
                    let new_uv = Curve::from(new_uv);
                    // Lift the UV-curve (on the face's surface again; the lift reads the
                    // surface's own continuities and makes no cache, where upstream's
                    // reuses the cache its surface keeps unless the face was marked
                    // modified)
                    let lifted = face_srf.lift_curve(
                        k,
                        &uv_domain,
                        &new_uv,
                        &new_uv.natural_interval(),
                        approx_tol3d(k, tol),
                        false,
                    )?;
                    curve_3d = Some(lifted.curve);
                    uv_curve = Some(new_uv);
                    self.fillet_edge_mut(e)?
                        .set_status(FilStatus::CubicRailRailInterpolate);
                }
            }
        }

        // set p3DCurve as this FilletEdge's geometry (side effect: the edge uses' UV
        // trim curves go), then the mate edge use's UV curve
        let curve_3d = curve_3d.ok_or_else(|| broken_link(OP, NO_CURVE_3D))?;
        set_edge_curve(self.brep_mut(), e, Some(curve_3d))?;
        let mate_eu = primary_edgeuse(self.brep(), e)?
            .and_then(|p| eu1::mate(self.brep(), p).ok().flatten())
            .ok_or_else(|| missing_set_up(OP, FilletSetUp::PrimaryEdgeUseMate))?;
        self.set_edgeuse_uv_curve(mate_eu, uv_curve, true, true)?;
        Ok(())
    }

    /// `SmFilletEdge::UpdateSplitFace(pOrigFace, pNewFace)`: when the edge's original
    /// face is `orig_face` (which was split, making `new_face`), records the split:
    /// the new face becomes the original face (and the old one a split face) when
    /// `ShouldSwapFaces` says so, else the new face is a split face.
    ///
    /// # Errors
    ///
    /// [`SolidError::NotFound`] when `e` is not a fillet edge; `SM_ERR_NULL_POINTER`
    /// for a `NULL` face (upstream's `ShouldSwapFaces` dereferences both); those of
    /// `ShouldSwapFaces` (`target` holds the faces, `corners` resolves the edge's
    /// corner).
    pub fn update_edge_split_face(
        &mut self,
        k: &Kernel,
        e: EdgeId,
        orig_face: Option<FaceId>,
        new_face: Option<FaceId>,
        target: &mut Brep,
        corners: &[FilletCorner],
    ) -> Result<()> {
        let rail_face = self.fillet_edge(e)?.original_face();
        if rail_face == orig_face {
            // This FilletEdge was in the Face that got split.
            // a NULL face has no id here (sm_ClassifyFacePoint would dereference it)
            let (Some(orig), Some(new)) = (orig_face, new_face) else {
                return Err(SolidError::blend(OP_SPLIT, BlendFailure::SplitFaceNotGiven));
            };
            let swap = self.should_swap_faces(k, e, orig, new, target, corners)?;
            if swap {
                let fe = self.fillet_edge_mut(e)?;
                fe.set_original_face(Some(new));
                fe.add_split_face(orig); // pOrigFace is now a Split Face.
            } else {
                self.fillet_edge_mut(e)?.add_split_face(new);
            }
        }
        Ok(())
    }

    /// `SmFilletGeom::~SmFilletGeom`: drops the centre line, deletes the vertices the
    /// geometry owns (`~SmFilletVertex`, [`FilletBrep::delete_fillet_vertex`]) and drops
    /// its offset surfaces. The geometry record itself stays in the table (ids are
    /// stable), emptied of those members.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for an unknown geometry; those of
    /// [`FilletBrep::delete_fillet_vertex`] (an owned vertex that is not a fillet vertex).
    pub fn destroy_geom(&mut self, g: FilletGeomId) -> Result<()> {
        let vertices = {
            let geom = self.geom_mut(g)?;
            // delete centerLine
            geom.center_line_curve = None;
            std::mem::take(&mut geom.vertices)
        };
        // delete Vertices
        for v in vertices {
            self.delete_fillet_vertex(v).map_err(|e| match e {
                SolidError::NotFound { .. } => {
                    SolidError::invalid_input(OP_DESTROY, "owned vertex is not a fillet vertex")
                }
                other => other,
            })?;
        }
        // delete offset surfaces
        self.geom_mut(g)?.off_surfs = [None, None];
        Ok(())
    }
}

/// `sm_FindStepoffUV(pFilSolver, dParam, dStepoffDist, sUVs)` over a base
/// [`FilletSolver`]: for each of its edge uses, the step-off point dropped onto the
/// edge use's face.
fn find_stepoff_uv(
    k: &Kernel,
    target: &mut Brep,
    fs: &FilletSolver,
    curve_param: f64,
    stepoff_dist: f64,
) -> Result<[Point2; 2]> {
    super::p1::find_stepoff_uv(k, target, &as_assisted(fs), curve_param, stepoff_dist)
}

// The reviewed defects' reasons (TE-B-02, #19810), one per reason, shared by its sites.

/// A derivative list shorter than its evaluation's order.
const SHORT_DERIVATIVES: &str =
    "the rails' derivative lists hold the point and each derivative their evaluations were asked for";
/// An evaluation without what it was asked for.
const SHORT_EVALUATION: &str =
    "an evaluation asked for n derivatives returns the point and n derivatives";
/// The second of the geometries counted above.
const SECOND_GEOM: &str = "a vertex with more than one fillet geometry has a second";
/// A drop over a bounded domain that found no point.
const FOOT_NO_DROP: &str =
    "a drop onto a surface over its natural domain minimises the distance over a bounded domain, which has a minimum";
/// A closest point over a bounded domain that was not found.
const NO_CLOSEST_POINT: &str =
    "the closest points on an offset surface over its natural domain include its minimum, which exists";
/// A rail's edge end not recorded before the guess.
const NO_RAIL_END: &str = "both rails' edge ends are recorded before their crossing is guessed";
/// An index modulo the list's length.
const INDEX_IN_LIST: &str = "an index modulo a list's length is in the list";
/// A lift that is not a B-spline curve.
const LIFT_NOT_BSPLINE: &str = "a lifted curve, and the curve built from it, is a B-spline curve";
/// The interpolant's UV curve not set.
const NO_UV_CURVE: &str = "the interpolant's UV curve is set on both of its paths";
/// The interpolant's 3D curve not set.
const NO_CURVE_3D: &str = "the interpolant's 3D curve is set on both of its paths";
/// A candidate index past the candidates.
const CANDIDATE_INDEX: &str = "the best index is that of a candidate pushed";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::surface::cone::Cone;

    #[test]
    fn an_edge_use_without_an_edge_records_the_lookup_that_failed() {
        let eu = EdgeuseId::from_index(3);
        let first = edge_use_without_edge("blend::fillet_geom::p2::a", eu);
        let first_line = line!() - 1;
        let second = edge_use_without_edge("blend::fillet_geom::p2::a", eu);
        let second_line = line!() - 1;
        for (e, line) in [(&first, first_line), (&second, second_line)] {
            let origin = e.failure().unwrap().origin();
            assert_eq!(origin.line(), line);
            assert!(
                std::path::Path::new(origin.file()).ends_with("fillet_geom/p2.rs"),
                "{}",
                origin.file()
            );
            assert_eq!(e.entity(), Some(EntityRef::Edgeuse(eu)));
            assert_eq!(e.sm_status(), Some(SM_ERR_NULL_POINTER));
        }
        // where a failure was built is not part of what failed
        assert_eq!(first, second);
    }

    /// The cylinder of radius 10 about z, height 40, its seam at `seam_deg` degrees about z.
    fn seamed_cylinder(k: &Kernel, seam_deg: f64) -> Surface {
        let t = seam_deg.to_radians();
        let frame = Axis2Placement::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(t.cos(), t.sin(), 0.0),
            Vec3::new(-t.sin(), t.cos(), 0.0),
        );
        let mut cylinder = Cone::create_canonical_cylinder(k, &frame, 10.0).unwrap();
        let mut domain = Extent2d::default();
        domain.set_min_max_coords(0.0, 0.0, 360.0, 40.0).unwrap();
        cylinder.adjust_step_uv_domain(k, &domain).unwrap();
        Surface::from(cylinder)
    }

    #[test]
    fn stationwise_cliff_section_requires_both_physical_feet_to_match() -> Result<()> {
        use crate::blend::fillet_custom_solver::{OffsetSurface, SurfaceId};
        use crate::blend::fillet_solver::p1::OffsetSurfaceSlot;
        use crate::blend::fillet_standard_solver::p1::ConstantDistanceFs;
        use crate::geom::vector2d::Vec2;
        use crate::surface::offset_surface::SS_UNKNOWN;
        use crate::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};

        let k = Kernel::default();
        let sharp = Point3::new(10.0, 0.0, 0.0);
        // Both sides have a right wedge between z = 0 and the cylinder of
        // radius 10. Its section in y = 0 has the exact feet (10 - d, 0, 0)
        // and (10, 0, d), so the compatible chord is d sqrt(2).
        let solve = |chord: f64, setback: Option<f64>, reversed: bool| -> Result<[Point3; 2]> {
            let plane = Plane::new(
                sharp + f64::from(u8::from(reversed)) * Vec3::Y,
                Vec3::X,
                Vec3::Y,
                Vec2::new(1.0, 1.0),
                &Extent2d::from_bounds(-2.0, -2.0, 2.0, 2.0),
            );
            let plane_uv = plane.project_point_to_uv_domain(sharp - 0.25 * Vec3::X);
            let cylinder = seamed_cylinder(&k, -90.0);
            let domain = cylinder.natural_uv_domain_with_nurb(&k)?;
            let dropped = cylinder.drop_point::<NoObject>(
                &k,
                sharp + 0.25 * Vec3::Z,
                &domain,
                &DropPointOptions::default(),
            )?;
            assert!(dropped.success);
            let mut sides = [
                (Surface::from(plane), plane_uv, 1.0),
                (cylinder, dropped.result_uv, -1.0),
            ];
            if reversed {
                sides.reverse();
            }
            let [a, b] = &sides;
            let (uv0, uv1) = (a.1, b.1);
            let domain0 = a.0.natural_uv_domain_with_nurb(&k)?;
            let domain1 = b.0.natural_uv_domain_with_nurb(&k)?;
            let mut solver = FilletSolver::new(1e-7, 1e-6, 1e-6);
            if let Some(d) = setback {
                solver.chamfer = Some(
                    ChamferDefinition::new(ChamferSize::Distances([d, d]), PI / 2.0)?
                        .at_every_station()?,
                );
            }
            for (rail, (surface, _, sign)) in sides.into_iter().enumerate() {
                let id = rail as u32;
                let offset = OffsetSurface::new(
                    &k,
                    sign * chord * std::f64::consts::FRAC_1_SQRT_2,
                    surface,
                    SurfaceId::Free(id),
                    SS_UNKNOWN,
                )?;
                solver.orientations[rail] = sign;
                solver.set_surface(
                    rail,
                    Some(OffsetSurfaceSlot {
                        key: u64::from(id),
                        surface: offset,
                    }),
                )?;
            }
            let mut fs = ConstantDistanceFs {
                solver,
                distance: chord,
            };
            let (found, point) = point_on_plane_solve(
                &k,
                &mut fs,
                None,
                sharp,
                Vec3::Y,
                &domain0,
                &domain1,
                uv0,
                uv1,
                &TsectPnt::new(),
            )?;
            assert!(found);
            cliff_section_feet(&k, &fs.solver, &point, usize::from(reversed))
        };
        let reference = solve(0.4 * std::f64::consts::SQRT_2, Some(0.25), false)?;
        let exact = [Point3::new(9.75, 0.0, 0.0), Point3::new(10.0, 0.0, 0.25)];
        for (found, expected) in reference.into_iter().zip(exact) {
            assert!(found.distance(expected) < 1e-7, "{found:?}");
        }
        let compatible = solve(0.25 * std::f64::consts::SQRT_2, None, true)?;
        check_cliff_section(reference, compatible, 1e-7)?;

        let too_wide = solve(0.5 * std::f64::consts::SQRT_2, None, true)?;
        let refused = check_cliff_section(reference, too_wide, 1e-7).unwrap_err();
        assert_eq!(refused.kind(), crate::error::ErrorKind::JoinOffsetsDiffer);
        let [a, mut b] = compatible;
        b.x = f64::NAN;
        assert!(check_cliff_section(reference, [a, b], 1e-7).is_err());
        Ok(())
    }

    /// The ends of a curve.
    fn curve_ends(k: &Kernel, c: &Curve) -> (Point3, Point3) {
        let ivl = c.natural_interval();
        (
            c.evaluate_point(k, ivl.min()).unwrap(),
            c.evaluate_point(k, ivl.max()).unwrap(),
        )
    }

    /// Review r2 M3 of #18352: an extrusion lying in a plane reaches the general
    /// intersector here (no end points: neither tracer runs), whose plane / extrusion
    /// dispatch answers the coincident region's boundary. The intersection keeps the
    /// general intersection's answer instead, in both orders, as before that
    /// dispatch answered the region.
    #[test]
    fn plane_and_extrusion_in_it_keep_the_general_intersection() {
        use crate::geom::vector2d::Vec2;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};
        use crate::surface::plane::Plane;
        use crate::surface::surf_of_extrusion::SurfOfExtrusion;
        use crate::surface::surface::p1::SurfaceIntersectContext;
        use crate::surface::surface::p2::{base_global_surface_intersect_in, TsectCurveType};
        use crate::surface::surface::p4::NoObject;
        let k = Kernel::default();
        let pw = vec![
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(1.0, 0.0, 0.0, NOW),
            CPoint::new(2.0, 0.0, 1.0, NOW),
        ];
        let nurb = NurbCurve::new(pw, 1, vec![0.0, 0.0, 1.0, 2.0, 2.0]).unwrap();
        let gen = Curve::from(BSplineCurve::from_nurb(nurb, 3, false));
        let ext = Surface::from(SurfOfExtrusion::create_canonical(&k, gen, Vec3::Y).unwrap());
        let dom = Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0);
        let mut plane = Plane::new(Point3::ZERO, Vec3::X, Vec3::Y, Vec2::new(1.0, 1.0), &dom);
        plane.make_nurb().unwrap();
        let plane = Surface::from(plane);
        let (tol, ang) = (1e-6, 0.1);
        for (s1, s2) in [(&plane, &ext), (&ext, &plane)] {
            let d1 = s1.natural_uv_domain_with_nurb(&k).unwrap();
            let d2 = s2.natural_uv_domain_with_nurb(&k).unwrap();
            // the dispatch the general intersector reaches answers the region
            let dispatched = s1
                .global_surface_intersect(&k, &d1, s2, &d2, [true, true], Some(tol), Some(ang))
                .unwrap();
            assert!(dispatched
                .curve_types
                .contains(&TsectCurveType::RegionBoundary));
            // the general intersection, as the dispatch ran before
            let tol_in = match s1.kind() {
                SurfaceKind::Plane(_) => tol,
                _ => approx_tol3d(&k, tol),
            };
            let general = base_global_surface_intersect_in::<NoObject>(
                &k,
                s1,
                &d1,
                s2,
                &d2,
                [true, true],
                Some(tol_in),
                Some(ang),
                None::<SurfaceIntersectContext<'_, NoObject>>,
            )
            .unwrap();
            assert!(!general
                .curve_types
                .contains(&TsectCurveType::RegionBoundary));
            assert!(!general.curves_3d.is_empty());
            let got = srf_srf_intersection_with_context(
                &k,
                None,
                None,
                None,
                None,
                s1,
                s2,
                tol,
                ang,
                // nearest the origin: the region's directrix x = 0 there, which the
                // general intersection does not hold
                SrfSrfOptions {
                    ref_point: Some(Point3::ZERO),
                    ..SrfSrfOptions::default()
                },
                None,
            );
            let wanted: Vec<_> = general
                .curves_3d
                .iter()
                .map(|c| curve_ends(&k, c))
                .collect();
            match got.ok().and_then(|g| g.curve_3d) {
                Some(c) => {
                    let (a, b) = curve_ends(&k, &c);
                    assert!(
                        wanted
                            .iter()
                            .any(|&(p, q)| (a.distance(p) <= 1e-9 && b.distance(q) <= 1e-9)
                                || (a.distance(q) <= 1e-9 && b.distance(p) <= 1e-9)),
                        "{a:?}-{b:?} not among {wanted:?}"
                    );
                }
                None => panic!("no curve: {wanted:?}"),
            }
        }
    }

    /// The point at `deg` degrees on the cylinder's circle at height 5.
    fn on_circle(deg: f64) -> Point3 {
        let t = deg.to_radians();
        Point3::new(10.0 * t.cos(), 10.0 * t.sin(), 5.0)
    }

    /// A curve through a closed candidate's closure may go without a UV curve only on a
    /// surface closed across the closure's point: on the cylinder's seam, not beside it, not on
    /// a plane (closed in neither direction) and not without a closure. The old test took a
    /// missing UV curve on every surface the moment the curve ran through a closure.
    #[test]
    fn a_curve_through_its_closure_needs_a_seam_there_to_go_without_a_uv_curve() {
        let k = Kernel::default();
        let tol = 1e-6;
        let cylinder = seamed_cylinder(&k, 5.0);
        let domain = cylinder.natural_uv_domain_with_nurb(&k).unwrap();
        // on the seam, and within the tolerance of it
        assert!(on_closed_seam(&k, &cylinder, &domain, Some(on_circle(5.0)), tol).unwrap());
        let t = 5.0_f64.to_radians();
        let near = on_circle(5.0) + Vec3::new(t.cos(), t.sin(), 0.0) * (0.5 * tol);
        assert!(on_closed_seam(&k, &cylinder, &domain, Some(near), tol).unwrap());
        // beside it (10 degrees off and more, and 1e-3 along the circle) and no closure at all
        for deg in [15.0, -5.0, 180.0, 5.0 + 1e-4_f64.to_degrees()] {
            assert!(
                !on_closed_seam(&k, &cylinder, &domain, Some(on_circle(deg)), tol).unwrap(),
                "{deg} degrees"
            );
        }
        assert!(!on_closed_seam(&k, &cylinder, &domain, None, tol).unwrap());
        // a plane through the seam's point: closed in neither direction
        let frame = Axis2Placement::new(
            Point3::new(0.0, 0.0, 5.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let mut plane = Plane::create_canonical(&frame).unwrap();
        let mut pd = Extent2d::default();
        pd.set_min_max_coords(-30.0, -30.0, 30.0, 30.0).unwrap();
        plane.adjust_step_uv_domain(&pd).unwrap();
        let plane = Surface::from(plane);
        let plane_domain = plane.natural_uv_domain_with_nurb(&k).unwrap();
        assert!(!on_closed_seam(&k, &plane, &plane_domain, Some(on_circle(5.0)), tol).unwrap());
    }

    #[test]
    fn a_side_edge_chain_keeps_every_default_guess_after_the_corner_section() {
        let defaults = [0.4, 0.05, 0.1, 0.23, 0.5, 0.77, 0.9, 0.95, 0.99];
        // further along a chain: the corner's guess first, then all nine defaults, the
        // radius-based one included
        let chain: Vec<f64> = rail_end_guesses(Some(0.7), false, &defaults).collect();
        assert_eq!(chain.len(), 10);
        assert_eq!(chain[0], 0.7);
        assert_eq!(chain[1..], defaults);
        // at the corner's own side edge it takes the radius-based guess's place
        let at_corner: Vec<f64> = rail_end_guesses(Some(0.7), true, &defaults).collect();
        assert_eq!(at_corner.len(), 9);
        assert_eq!(at_corner[0], 0.7);
        assert_eq!(at_corner[1..], defaults[1..]);
        // without a corner guess the defaults are tried as they are, on either edge
        for edge in [false, true] {
            let none: Vec<f64> = rail_end_guesses(None, edge, &defaults).collect();
            assert_eq!(none, defaults);
        }
    }

    #[test]
    fn live_solver_lookup_retains_virtuals_and_actual_identity() {
        struct Derived(FilletSolver);
        impl FilletSolverOps for Derived {
            fn solver(&self) -> &FilletSolver {
                &self.0
            }
            fn solver_mut(&mut self) -> &mut FilletSolver {
                &mut self.0
            }
            fn jacobian_size(&self) -> Result<usize> {
                Ok(7)
            }
        }
        let id = FilletSolverId(37);
        let mut derived = Derived(FilletSolver::new(1e-5, 1e-3, 0.01));
        let mut table = RailSolvers::Live(id, &mut derived);
        assert_eq!(
            table.solver_ops_mut(id).unwrap().jacobian_size().unwrap(),
            7
        );
        table.solver_mut(id).unwrap().set_extension_factor(4.5);
        assert_eq!(table.solver(id).unwrap().extension_factor(), 4.5);
        assert!(matches!(
            table.solver_ops_mut(FilletSolverId(0)),
            Err(SolidError::InvalidInput { .. })
        ));
        assert_eq!(derived.0.extension_factor(), 4.5);
    }

    #[test]
    fn geometry_offsets_precede_solver_resolution() {
        let k = Kernel::default();
        let mut fb = FilletBrep::new();
        let mut geom =
            super::super::p1::FilletGeom::new(&k, None, super::super::p1::FilletGeomType::Default)
                .unwrap();
        geom.solver = Some(FilletSolverId(91));
        geom.off_surfs[0] = Some(
            crate::blend::fillet_custom_solver::OffsetSurface::new(
                &k,
                2.75,
                bilinear(0.0),
                crate::blend::fillet_custom_solver::SurfaceId::Free(8),
                crate::surface::offset_surface::SS_UNKNOWN,
            )
            .unwrap(),
        );
        let g = fb.add_geom(geom);
        // An empty table: the geometry's own offset surface is read before the
        // solver is resolved, so reaching the solver at all would be an error here.
        let table = RailSolvers::Indexed(&mut []);
        assert_eq!(
            table
                .offset_surface(&fb, g, 0)
                .unwrap()
                .unwrap()
                .offset_distance(),
            2.75
        );
        assert!(matches!(
            table.offset_surface(&fb, g, 1),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    fn bilinear(z: f64) -> Surface {
        // a plane at height z with its NURB
        let mut p = Plane::from_normal(Point3::new(0.0, 0.0, z), Vec3::new(0.0, 0.0, 1.0));
        p.make_nurb().unwrap();
        Surface::from(p)
    }

    #[test]
    fn srf_srf_of_parallel_planes_fails() {
        let k = Kernel::default();
        let a = bilinear(0.0);
        let b = bilinear(1.0);
        let r = srf_srf_intersection(
            &k,
            None,
            None,
            None,
            None,
            &a,
            &b,
            1e-6,
            0.5,
            SrfSrfOptions::default(),
        );
        assert!(r.is_err());
    }

    #[test]
    fn destroy_geom_clears_members() {
        let k = Kernel::default();
        let mut fb = FilletBrep::new();
        let geom =
            super::super::p1::FilletGeom::new(&k, None, super::super::p1::FilletGeomType::Default)
                .unwrap();
        let g = fb.add_geom(geom);
        fb.geom_mut(g).unwrap().center_line_curve = Some(
            BSplineCurve::create_line_segment(
                3,
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
            )
            .unwrap()
            .into(),
        );
        fb.destroy_geom(g).unwrap();
        assert!(fb.geom(g).unwrap().center_line_curve.is_none());
        assert!(fb.geom(g).unwrap().vertices.is_empty());
    }

    /// The setback is measured from the filleted vertex along the edge (WR-445), also on an
    /// edge whose primary edge use runs against its curve, where upstream measures from the
    /// far vertex whichever end is filleted.
    #[test]
    fn a_setback_is_measured_from_the_filleted_vertex() {
        let mut k = Kernel::default();
        let target =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 60.0, 40.0, 20.0)
                .unwrap();
        let dist = 8.0;
        // (the edge runs against its curve, the distance of the setback point from where it
        // belongs) for each end of each edge of the box
        let errors = |k: &Kernel| {
            let mut out = Vec::new();
            for e in crate::topology::brep::p1::edges(&target, None).unwrap() {
                let (start, end) = edge::start_end_vertices(&target, e).unwrap();
                let against = edge::vertex(&target, e).unwrap() != start;
                for (cp, other) in [(start, end), (end, start)] {
                    let cp_pt = vertex::point(&target, cp.unwrap()).unwrap();
                    let other_pt = vertex::point(&target, other.unwrap()).unwrap();
                    let expected = cp_pt + (other_pt - cp_pt) * (dist / cp_pt.distance(other_pt));
                    let (orig, _) = set_back_frame(k, &target, e, cp, dist, "test").unwrap();
                    out.push((against, orig.distance(expected)));
                }
            }
            out
        };

        let found = errors(&k);
        assert!(
            found.iter().any(|&(against, _)| against),
            "no edge runs against its curve"
        );
        assert!(found.iter().all(|&(_, err)| err < 1e-9), "{found:?}");
    }

    /// The setback walks the edge's own interval, not its curve's natural interval (WR-445):
    /// each box edge's curve is replaced by a line running 10 past both its ends, the edge
    /// keeping the middle part as its interval. Upstream walks from the natural interval's
    /// start, 10 outside the edge, from either end.
    #[test]
    fn a_setback_walks_the_edges_own_interval() {
        let mut k = Kernel::default();
        let mut target =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 60.0, 40.0, 20.0)
                .unwrap();
        let (dist, past) = (8.0, 10.0);
        let edges = crate::topology::brep::p1::edges(&target, None).unwrap();
        for &e in &edges {
            let (start, end) = edge::start_end_vertices(&target, e).unwrap();
            let a = vertex::point(&target, start.unwrap()).unwrap();
            let b = vertex::point(&target, end.unwrap()).unwrap();
            let len = a.distance(b);
            let dir = (b - a) * (1.0 / len);
            let line = Curve::from(crate::curve::line::Line::from_points(
                a - dir * past,
                b + dir * past,
                3,
            ));
            let total = len + 2.0 * past;
            let data = target.edge_mut(e).unwrap();
            data.curve = Some(line);
            data.interval = Extent1d::from_min_max(past / total, (past + len) / total);
        }
        let errors = |k: &Kernel| {
            let mut out = Vec::new();
            for &e in &edges {
                let natural = edge::curve(&target, e).unwrap().unwrap().natural_interval();
                let own = edge::interval(&target, e).unwrap();
                assert!(
                    own.min() > natural.min() && own.max() < natural.max(),
                    "{own:?} in {natural:?}"
                );
                let (start, end) = edge::start_end_vertices(&target, e).unwrap();
                for (cp, other) in [(start, end), (end, start)] {
                    let cp_pt = vertex::point(&target, cp.unwrap()).unwrap();
                    let other_pt = vertex::point(&target, other.unwrap()).unwrap();
                    let expected = cp_pt + (other_pt - cp_pt) * (dist / cp_pt.distance(other_pt));
                    let (orig, _) = set_back_frame(k, &target, e, cp, dist, "test").unwrap();
                    out.push(orig.distance(expected));
                }
            }
            out
        };

        let fixed = errors(&k);
        assert!(fixed.iter().all(|&err| err < 1e-9), "{fixed:?}");
    }

    #[test]
    fn vertex_calculators_need_a_corner() {
        let k = Kernel::default();
        let mut fb = FilletBrep::new();
        let v = fb
            .brep_mut()
            .add_vertex(Point3::new(0.0, 0.0, 0.0))
            .unwrap();
        fb.insert_fillet_vertex(v, super::super::p1::FilletVertex::new(&k, None));
        let mut target = Brep::new();
        let mut solvers: Vec<FilletSolver> = Vec::new();
        // #13790, #15034, #19810: the caller's input (this vertex, built with the public
        // API, has no corner), keeping the status of the bare `Upstream` it replaced
        let no_corner = |r: Result<()>, op: &'static str| {
            let e = r.unwrap_err();
            assert_eq!(
                (e.operation(), e.blend_failure()),
                (op, Some(&BlendFailure::FilletVertexWithoutCorner))
            );
            assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput);
            assert!(!e.is_defect());
            assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
        };
        let none = CornerLookup::default();
        no_corner(
            fb.calc_rail_int_rail(&k, v, none, &mut solvers, &mut target),
            OP_RAIL_RAIL,
        );
        no_corner(
            fb.calc_rail_int_edgeuse(&k, v, None, none, &mut solvers, &mut target),
            OP_RAIL_EU,
        );
        no_corner(
            fb.calc_vert_on_cross_section(&k, v, None, none, &mut solvers, &mut target),
            OP_CROSS,
        );
        no_corner(
            fb.calc_set_back_vert(&k, v, none, &mut solvers, &target),
            OP_SETBACK,
        );
        // with a corner: no rail geometries is SM_ERR; without a corner store (an
        // empty table) the corner id is not in the table
        fb.fillet_vertex_mut(v)
            .unwrap()
            .set_fillet_corner(Some(FilletCornerId(0)));
        let e = fb
            .calc_rail_int_rail(&k, v, none, &mut solvers, &mut target)
            .unwrap_err();
        assert_eq!(
            (e.operation(), e.blend_failure()),
            (
                OP_RAIL_RAIL,
                Some(&BlendFailure::FilletSetUpInvalid(FilletSetUp::VertexGeoms))
            )
        );
        assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput);
        assert_eq!(e.upstream_status(), Some(1001));
        let e = fb
            .calc_rail_int_edgeuse(&k, v, None, none, &mut solvers, &mut target)
            .unwrap_err();
        // the NULL corner (`CornerLookup::corner`, p1): a stage failure (#15034)
        assert_eq!(
            (
                e.operation(),
                e.kind() == crate::error::ErrorKind::StageFailed && e.is_defect()
            ),
            ("blend::fillet_geom::p1::CornerLookup::corner", true),
            "{e:?}"
        );
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
        // an id outside the table is the NULL corner (`CornerLookup::corner`, p1, a stage
        // failure since #15034); a corner in it gets to the geometry check
        let e = fb
            .calc_set_back_vert(&k, v, CornerLookup::table(&[]), &mut solvers, &target)
            .unwrap_err();
        assert_eq!(
            (
                e.operation(),
                e.kind() == crate::error::ErrorKind::StageFailed && e.is_defect()
            ),
            ("blend::fillet_geom::p1::CornerLookup::corner", true),
            "{e:?}"
        );
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
        let corners = [FilletCorner::new(
            crate::blend::fillet_corner::p3::FilletCornerKind::FourByThree,
            None,
            None,
            None,
            1e-6,
            1e-3,
        )];
        let table = CornerLookup::table(&corners);
        for (r, expected) in [
            (
                fb.calc_rail_int_edgeuse(&k, v, None, table, &mut solvers, &mut target),
                OP_RAIL_EU,
            ),
            (
                fb.calc_vert_on_cross_section(&k, v, None, table, &mut solvers, &mut target),
                OP_CROSS,
            ),
            (
                fb.calc_set_back_vert(&k, v, table, &mut solvers, &target),
                OP_SETBACK,
            ),
        ] {
            let e = r.unwrap_err();
            assert_eq!(
                (e.operation(), e.blend_failure()),
                (
                    expected,
                    Some(&BlendFailure::FilletSetUpInvalid(FilletSetUp::VertexGeoms))
                )
            );
            assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput);
            assert!(!e.is_defect());
            assert_eq!(e.upstream_status(), Some(1001));
        }
    }

    /// The cylinder of radius 1 about the line through `origin` along `seam x across`, 10 long
    /// from `origin`, its seam along `seam` from the axis.
    fn unit_cylinder(k: &Kernel, origin: Point3, seam: Vec3, across: Vec3) -> (Surface, Extent2d) {
        let frame = Axis2Placement::new(origin, seam, across);
        let mut cylinder = Cone::create_canonical_cylinder(k, &frame, 1.0).unwrap();
        let mut domain = Extent2d::default();
        domain.set_min_max_coords(0.0, 0.0, 360.0, 10.0).unwrap();
        cylinder.adjust_step_uv_domain(k, &domain).unwrap();
        let surface = Surface::from(cylinder);
        let domain = surface.natural_uv_domain_with_nurb(k).unwrap();
        (surface, domain)
    }

    /// The ellipse `(cos s, ±cos s, sin s)` over `from..to` degrees: a branch of the
    /// intersection of the unit cylinders about y and about x, in the plane x = ±y.
    fn branch(k: &Kernel, sign: f64, from: f64, to: f64) -> Curve {
        use crate::curve::ellipse::{Ellipse, EllipseOptions};
        let x = Vec3::new(1.0, sign, 0.0) / 2.0_f64.sqrt();
        let domain = Extent1d::from_min_max(from, to);
        let opts = EllipseOptions::default();
        let e = Ellipse::new(
            k,
            Point3::ZERO,
            x,
            Vec3::new(0.0, 0.0, 1.0),
            domain,
            2.0_f64.sqrt(),
            1.0,
            &opts,
        );
        Curve::from(e.unwrap())
    }

    fn candidate(curve_3d: Curve) -> Candidate {
        Candidate {
            curve_3d,
            uv_curve1: None,
            uv_curve2: None,
            through: None,
        }
    }

    /// The two surfaces of a pair, each with its domain, as the intersectors take them.
    fn surfaces<'a>(
        a: &'a (Surface, Extent2d),
        b: &'a (Surface, Extent2d),
    ) -> [(&'a Surface, &'a Extent2d); 2] {
        [(&a.0, &a.1), (&b.0, &b.1)]
    }

    fn on_branch(sign: f64, deg: f64) -> Point3 {
        let s = deg.to_radians();
        Point3::new(s.cos(), sign * s.cos(), s.sin())
    }

    /// #21416 review: two candidates that meet on a closed surface's seam are joined only where
    /// the meeting point is a regular point of the intersection and the two run on through it.
    /// The unit cylinders about y (`a`) and about x (`b`) meet in the ellipses in the planes
    /// x = y and x = -y, which cross at the branch points (0, 0, ±1), where both normals are z.
    /// - With `a`'s seam through (0, 0, 1), the branch x = y from (1, 1, 0) up to it and the
    ///   branch x = -y on from it to (-1, 1, 0) meet on the seam, two different branches turning
    ///   a right angle there: not joined.
    /// - With `a`'s seam at 45 degrees, through the regular point at 45 degrees of the branch
    ///   x = y (the normals 60 degrees apart), that branch cut there is joined, and runs from
    ///   the start to the end.
    /// - The same seam, the piece from 20 to 45 degrees and the piece from 45 back to 0: one
    ///   branch, but the second turns back over the first: not joined.
    /// - The piece from 0 to 45 degrees and a segment from the seam point: it runs on along the
    ///   branch's tangent, kinks 11 degrees off it (beyond the kernel's angle tolerance) or
    ///   leaves across the branch; only the first continues the piece.
    #[test]
    fn candidates_meeting_on_a_seam_are_joined_only_where_they_run_on() {
        let k = Kernel::default();
        let approx_tol = 5e-6;
        let (z, x, y) = (
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let about_x = unit_cylinder(&k, Point3::new(-5.0, 0.0, 0.0), -z, y);
        let top = unit_cylinder(&k, Point3::new(0.0, -5.0, 0.0), z, x);
        let r = 0.5_f64.sqrt();
        let at_45 = unit_cylinder(
            &k,
            Point3::new(0.0, -5.0, 0.0),
            Vec3::new(r, 0.0, r),
            Vec3::new(r, 0.0, -r),
        );
        // two branches meeting at the branch point (0, 0, 1) on `a`'s seam
        let cands = [
            candidate(branch(&k, 1.0, 0.0, 90.0)),
            candidate(branch(&k, -1.0, 90.0, 180.0)),
        ];
        let (start, end) = (on_branch(1.0, 0.0), on_branch(-1.0, 180.0));
        assert!(on_closed_seam(&k, &top.0, &top.1, Some(z), approx_tol).unwrap());
        let got =
            candidates_across_seam(&k, &cands, start, end, surfaces(&top, &about_x), approx_tol);
        assert!(got.unwrap().is_none());

        // one branch cut at the regular point at 45 degrees, on `a`'s seam
        let seam = on_branch(1.0, 45.0);
        assert!(on_closed_seam(&k, &at_45.0, &at_45.1, Some(seam), approx_tol).unwrap());
        let cands = [
            candidate(branch(&k, 1.0, 0.0, 45.0)),
            candidate(branch(&k, 1.0, 45.0, 80.0)),
        ];
        let (start, end) = (on_branch(1.0, 0.0), on_branch(1.0, 80.0));
        let got = candidates_across_seam(
            &k,
            &cands,
            start,
            end,
            surfaces(&at_45, &about_x),
            approx_tol,
        );
        let joined = got.unwrap().and_then(|c| c.curve_3d).expect("joined");
        let (a, b) = curve_ends(&k, &joined);
        assert!(
            a.distance(start) <= 1e-9 && b.distance(end) <= 1e-9,
            "{a:?} {b:?}"
        );

        // the second piece turns back over the first at the seam
        let cands = [
            candidate(branch(&k, 1.0, 20.0, 45.0)),
            candidate(branch(&k, 1.0, 0.0, 45.0)),
        ];
        let (start, end) = (on_branch(1.0, 20.0), on_branch(1.0, 0.0));
        let got = candidates_across_seam(
            &k,
            &cands,
            start,
            end,
            surfaces(&at_45, &about_x),
            approx_tol,
        );
        assert!(got.unwrap().is_none());

        // a second piece that kinks at the regular point at 45 degrees: a segment leaving it
        // along the branch's tangent `t` runs on; one turned 11 degrees off it toward `n`,
        // still on the branch's side, kinks; one along `n`, across the branch, turns off it
        let first = branch(&k, 1.0, 0.0, 45.0);
        let t = Vec3::new(-1.0, -1.0, 1.0) / 3.0_f64.sqrt();
        let n = Vec3::new(1.0, -1.0, 0.0) / 2.0_f64.sqrt();
        // (at a fit a hundred times looser too: the ball the turn is read over is no wider than
        // the fit, so the kink is still one; #21959 review)
        for tol in [approx_tol, 100.0 * approx_tol] {
            for (dir, runs_on) in [(t, true), (t + n * 0.2, false), (n, false)] {
                let second = Curve::from(crate::curve::line::Line::from_points(
                    seam,
                    seam + dir * 0.1,
                    3,
                ));
                let got = continues_across(&k, [&first, &second], surfaces(&at_45, &about_x), tol);
                assert_eq!(got.unwrap(), runs_on, "{dir:?} at {tol}");
            }
        }
    }
}
