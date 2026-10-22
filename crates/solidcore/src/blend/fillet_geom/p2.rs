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
//! | [`FilletBrep::calc_rail_int_edgeuse`], [`FilletBrep::calc_rail_int_edgeuse_with_solver`] | `SmFilletVertex::CalcRailIntEdgeuse`         |
//! | [`FilletBrep::calc_vert_on_cross_section`]        | `SmFilletVertex::CalcVertOnCrossSection`     |
//! | [`FilletBrep::calc_set_back_vert`]                | `SmFilletVertex::CalcSetBackVert`            |
//! | [`FilletBrep::new_fillet_edge`](super::p1::FilletBrep::new_fillet_edge) (in `p1`) | `SmFilletEdge::SmFilletEdge` |
//! | [`FilletBrep::calc_rail_rail_interpolation`]      | `SmFilletEdge::CalcRailRailInterpolation`    |
//! | [`FilletBrep::update_edge_split_face`]            | `SmFilletEdge::UpdateSplitFace`              |
//! | [`FilletBrep::destroy_geom`]                      | `SmFilletGeom::~SmFilletGeom`                |
//!
//! Deviations:
//! - **Object model.** As in `blend.fillet_geom_p1` and `_p4`, fillet vertices, edges
//!   and geometries are ids of the [`FilletBrep`] (`self`). `GetFilletSolver()` is an
//!   opaque [`FilletSolverId`] resolved in the caller's `solvers` table (index `id.0`;
//!   an id not in the table is [`SolidError::InvalidInput`], a geometry without a
//!   solver where upstream dereferences it `SM_ERR_NULL_POINTER`). The rollover
//!   caller can supply its single live solver with its actual id through
//!   `calc_rail_int_edgeuse_with_solver`; an unresolved adjacent id is an error.
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
//! - **Statuses.** Upstream ignores the status of `IntersectWithPlane`,
//!   `sm_TwoPntsIntersection`, `GetRailIndexOfSideEU` and `SetMinMax` here; so does the
//!   port, except that [`SolidError::Unported`] always propagates. An intersector that
//!   fails part-way may have appended curves upstream; the Rust intersectors return
//!   nothing on failure. `FindParameterAtArcLength`'s `SM_ERR_NOT_CONVERGING` is an
//!   error where upstream `SER`s it.
//! - **Fillet corners.** `SmFilletCorner` (unit `blend.fillet_corner_p1`) is an opaque
//!   [`FilletCornerId`] here: its filleted vertex, set-back distance, flags, fillet
//!   edges, type and `TestRailRailInterpolationCurve` are `UNPORTED(blend.fillet_corner_p1)`
//!   stubs (the four vertex calculators stop at the filleted vertex, right after the
//!   corner's null check; `CalcRailRailInterpolation` only with a corner).
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
//!   are dropped. No input-driven recursion.

use std::borrow::Cow;

use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_custom_solver::ConstantRadiusAssistedFs;
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSolverOps, FilletSurfaceGeneratorType};
use crate::curve::curve::p3::PointSolveOptions;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::tol::{approx_tol3d, zone_tol3d};
use crate::geom::vector2d::Point2;
use crate::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, PI};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::{EvaluateFlags, OrientType};
use crate::nurbs::knots::Side;
use crate::surface::plane::Plane;
use crate::surface::surface::p2::SurfaceIntersection;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1::{self as eu1, primary_edgeuse};
use crate::topology::edgeuse::p3::{
    evaluate_binormal, get_or_create_uv_trim_curve, BinormalOptions,
};
use crate::topology::face::p2 as face2;
use crate::topology::vertex;
use crate::topology::vertexuse as vertexuse_unit;
use crate::topology::{edge, Brep, EdgeId, EdgeuseId, FaceId, VertexId, VertexuseId};

use super::p1::{
    adjust_uv_on_seam, create_cubic_rail_rail_interpolant, evaluate_mate,
    interpolate_endpts_and_tangents, two_pnts_intersection, FilStatus, FilletBrep, FilletCornerId,
    FilletCornerType, FilletEdgeType, FilletGeomId, FilletSolverId, FilletVertexType,
    SurfaceHandle,
};
use super::p4::{set_edge_curve, set_vertex_point};

const OP_SRF_SRF: &str = "blend::fillet_geom::p2::srf_srf_intersection";
const OP_RAIL_RAIL: &str = "blend::fillet_geom::p2::FilletBrep::calc_rail_int_rail";
const OP_RAIL_EU: &str = "blend::fillet_geom::p2::FilletBrep::calc_rail_int_edgeuse";
const OP_CROSS: &str = "blend::fillet_geom::p2::FilletBrep::calc_vert_on_cross_section";
const OP_SETBACK: &str = "blend::fillet_geom::p2::FilletBrep::calc_set_back_vert";
const OP_INTERP: &str = "blend::fillet_geom::p2::FilletBrep::calc_rail_rail_interpolation";
const OP_SPLIT: &str = "blend::fillet_geom::p2::FilletBrep::update_edge_split_face";
const OP_DESTROY: &str = "blend::fillet_geom::p2::FilletBrep::destroy_geom";

/// The "nothing found" index of `sm_SrfSrfIntersection`'s curve selection.
const NO_CURVE: usize = 9999;

/// A call into a unit that is not ported yet; `_args` are upstream's arguments.
fn unported_on<A, T>(_args: A, upstream: &'static str, unit: &'static str) -> Result<T> {
    Err(SolidError::unported(upstream, unit))
}

/// `SER(..)` of a call whose failure upstream ignores: [`SolidError::Unported`]
/// propagates, any other error becomes `None`.
fn ignore_status<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_unported() => Err(e),
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
enum RailSolvers<'a> {
    Indexed(&'a mut [FilletSolver]),
    Single(FilletSolverId, &'a mut FilletSolver),
}

impl RailSolvers<'_> {
    fn solver(&self, id: FilletSolverId) -> Result<&FilletSolver> {
        match self {
            Self::Indexed(solvers) => {
                solver_at(solvers, solver_index(solvers, id, OP_RAIL_EU)?, OP_RAIL_EU)
            }
            Self::Single(key, solver) if *key == id => Ok(solver),
            Self::Single(..) => Err(SolidError::invalid_input(
                OP_RAIL_EU,
                "unresolved adjacent fillet solver",
            )),
        }
    }

    fn solver_mut(&mut self, id: FilletSolverId) -> Result<&mut FilletSolver> {
        match self {
            Self::Indexed(solvers) => {
                let index = solver_index(solvers, id, OP_RAIL_EU)?;
                solvers.get_mut(index).ok_or_else(|| {
                    SolidError::invalid_input(
                        OP_RAIL_EU,
                        "fillet solver id not in the solver table",
                    )
                })
            }
            Self::Single(key, solver) if *key == id => Ok(solver),
            Self::Single(..) => Err(SolidError::invalid_input(
                OP_RAIL_EU,
                "unresolved adjacent fillet solver",
            )),
        }
    }

    fn geom_solver(fb: &FilletBrep, g: FilletGeomId) -> Result<FilletSolverId> {
        fb.geom(g)?
            .fillet_solver()
            .ok_or_else(|| SolidError::null_pointer(OP_RAIL_EU))
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

/// `SmFilletSolver::GetEdgeuse(i)`, dereferenced (`NULL` is `SM_ERR_NULL_POINTER`).
fn solver_edgeuse(fs: &FilletSolver, i: usize, op: &'static str) -> Result<EdgeuseId> {
    fs.edgeuse(i)?.ok_or_else(|| SolidError::null_pointer(op))
}

/// `SmFilletSolver::GetSurface(i)`, dereferenced.
fn solver_surface<'s>(
    fs: &'s FilletSolver,
    i: usize,
    op: &'static str,
) -> Result<&'s crate::surface::offset_surface::OffsetSurface> {
    fs.surface(i)?
        .map(|slot| &slot.surface.surface)
        .ok_or_else(|| SolidError::null_pointer(op))
}

/// `FindParameterAtArcLength` under `SER`: `SM_ERR_NOT_CONVERGING` is an error.
fn arc_length_param(
    k: &Kernel,
    curve: &Curve,
    start: f64,
    length: f64,
    op: &'static str,
) -> Result<f64> {
    let r = curve.find_parameter_at_arc_length(k, start, length)?;
    if r.converged {
        Ok(r.end_param)
    } else {
        Err(SolidError::Upstream {
            operation: op,
            code: r.status(),
            entity: None,
        })
    }
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

/// `SM_CAST_PTR(SmBSplineSurface, ..)`: the B-spline net of a B-spline surface, a
/// plane or a surface of revolution.
const fn b_spline_net(s: &Surface) -> Option<&BSplineSurface> {
    match s.kind() {
        SurfaceKind::BSpline(b) => Some(b),
        SurfaceKind::Plane(p) => Some(p.base()),
        SurfaceKind::SurfOfRevolution(r) => Some(r.base()),
        SurfaceKind::SurfOfExtrusion(r) => Some(r.base()),
        _ => None,
    }
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

// ---- unported callees -----------------------------------------------------------------

/// `SmFilletCorner::GetFilletedVertex` (unit `blend.fillet_corner_p1`).
/// Needs the executive corner store (`blend.fillet_executive_p2`, #267) to resolve the
/// id; wired in a follow-up once it merges.
fn corner_filleted_vertex(corner: FilletCornerId) -> Result<Option<VertexId>> {
    // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::GetFilletedVertex
    unported_on(
        corner,
        "SmFilletCorner::GetFilletedVertex",
        "blend.fillet_corner_p1",
    )
}

/// `SmFilletCorner::GetSetBackDist` (unit `blend.fillet_corner_p1`).
/// Needs the executive corner store (`blend.fillet_executive_p2`, #267) to resolve the
/// id; wired in a follow-up once it merges.
fn corner_set_back_dist(corner: FilletCornerId) -> Result<f64> {
    // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::GetSetBackDist
    unported_on(
        corner,
        "SmFilletCorner::GetSetBackDist",
        "blend.fillet_corner_p1",
    )
}

/// `SmFilletCorner::IsCalcAsConstRad` (unit `blend.fillet_corner_p1`).
/// Needs the executive corner store (`blend.fillet_executive_p2`, #267) to resolve the
/// id; wired in a follow-up once it merges.
fn corner_is_calc_as_const_rad(corner: Option<FilletCornerId>) -> Result<bool> {
    // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::IsCalcAsConstRad
    unported_on(
        corner,
        "SmFilletCorner::IsCalcAsConstRad",
        "blend.fillet_corner_p1",
    )
}

/// `SmFilletCorner::IsBlendingCorner` (unit `blend.fillet_corner_p1`).
/// Needs the executive corner store (`blend.fillet_executive_p2`, #267) to resolve the
/// id; wired in a follow-up once it merges.
fn corner_is_blending(corner: FilletCornerId) -> Result<bool> {
    // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::IsBlendingCorner
    unported_on(
        corner,
        "SmFilletCorner::IsBlendingCorner",
        "blend.fillet_corner_p1",
    )
}

/// `SmFilletCorner::GetFilletEdges` (unit `blend.fillet_corner_p1`).
/// Needs the executive corner store (`blend.fillet_executive_p2`, #267) to resolve the
/// id; wired in a follow-up once it merges.
fn corner_fillet_edges(corner: FilletCornerId) -> Result<Vec<EdgeId>> {
    // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::GetFilletEdges
    unported_on(
        corner,
        "SmFilletCorner::GetFilletEdges",
        "blend.fillet_corner_p1",
    )
}

/// `SmFilletCorner::GetCornerType` (unit `blend.fillet_corner_p1`).
/// Needs the executive corner store (`blend.fillet_executive_p2`, #267) to resolve the
/// id; wired in a follow-up once it merges.
fn corner_type(corner: FilletCornerId) -> Result<FilletCornerType> {
    // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::GetCornerType
    unported_on(
        corner,
        "SmFilletCorner::GetCornerType",
        "blend.fillet_corner_p1",
    )
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

/// `SmFilletConcaveNxNCorner::TestRailRailInterpolationCurve` (unit
/// `blend.fillet_corner_p1`).
/// Needs the executive corner store (`blend.fillet_executive_p2`, #267) to resolve the
/// id; wired in a follow-up once it merges.
fn corner_test_rail_rail_interpolation_curve(
    corner: FilletCornerId,
    curve: &BSplineCurve,
) -> Result<bool> {
    // UNPORTED(blend.fillet_corner_p1): SmFilletConcaveNxNCorner::TestRailRailInterpolationCurve
    unported_on(
        (corner, curve),
        "SmFilletConcaveNxNCorner::TestRailRailInterpolationCurve",
        "blend.fillet_corner_p1",
    )
}

/// `pFilSolver1->RailRailIntersect(sUVGuess, lRailIndex1, pFilSolver2, lRailIndex2,
/// rbFound, rTsectPnt1, rTsectPnt2)` on solvers `solver_index1` and `solver_index2` of
/// the table: whether the rails meet; the two intersection points are in/out.
#[allow(clippy::too_many_arguments)]
fn rail_rail_intersect(
    k: &Kernel,
    target: &Brep,
    solvers: &mut [FilletSolver],
    solver_index1: usize,
    uv_guess: Point2,
    rail_index1: usize,
    solver_index2: usize,
    rail_index2: usize,
    tsect1: &mut TsectPnt,
    tsect2: &mut TsectPnt,
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
        )
    })
}

/// `pFilSolver->PointOnPlaneSolve(crPlaneOrig, crPlaneNormal, crDomain1, crDomain2,
/// crUV1, crUV2, rbFound, rTsectPnt)` (a virtual, here the base
/// `SmFilletSolver::PointOnPlaneSolve` of the table's solver): whether a fillet point was
/// found on the plane, and the point (in/out).
#[allow(clippy::too_many_arguments)]
fn point_on_plane_solve(
    k: &Kernel,
    fs: &mut FilletSolver,
    plane_orig: Point3,
    plane_normal: Vec3,
    domain1: &Extent2d,
    domain2: &Extent2d,
    uv1: Point2,
    uv2: Point2,
    tsect_pnt: &TsectPnt,
) -> Result<(bool, TsectPnt)> {
    let mut tsect = tsect_pnt.clone();
    // the virtual: the base version for a base solver
    let found = FilletSolverOps::point_on_plane_solve(
        fs,
        k,
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

/// `SmFilletGeom::RailEdgeuseIntersect(lRailIndex, pSideEU, dGuess, rbFound,
/// rTsectPnt, rdEdgeuseParameter)` (part 6) on geometry `g`'s solver in `solvers`: the
/// edge use parameter when the rail meets the side edge use; the intersection point
/// `tsect` is in/out. While it runs the solver holds `g` as its geometry for surfaces,
/// as upstream's `SetFilletGeomForSurfaces(this)`.
#[allow(clippy::too_many_arguments)]
fn rail_edgeuse_intersect(
    fb: &FilletBrep,
    k: &Kernel,
    g: FilletGeomId,
    solvers: &mut RailSolvers<'_>,
    target: &mut Brep,
    rail_index: usize,
    side_eu: EdgeuseId,
    guess: f64,
    tsect: &mut TsectPnt,
) -> Result<Option<f64>> {
    let si = RailSolvers::geom_solver(fb, g)?;
    let fs = solvers.solver_mut(si)?;
    fb.rail_edgeuse_intersect(k, g, fs, target, rail_index, side_eu, guess, tsect)
}

/// `SmTemporaryRadiusChange(pFS, lRailIndex, pSideEU)` (unit
/// `blend.fillet_executive_p2`): its set-up returns at once for a `NULL` solver (not an
/// `SmVariableRadiusFS`); for a solver it is unported.
fn temporary_radius_change(
    fs: Option<&FilletSolver>,
    rail_index: usize,
    side_eu: EdgeuseId,
) -> Result<()> {
    match fs {
        None => Ok(()),
        Some(fs) => {
            // UNPORTED(blend.fillet_executive_p2): SmTemporaryRadiusChange::SetUp
            unported_on(
                (fs, rail_index, side_eu),
                "SmTemporaryRadiusChange::SetUp",
                "blend.fillet_executive_p2",
            )
        }
    }
}

// ---- sm_SrfSrfIntersection --------------------------------------------------------------

/// The curves of [`srf_srf_intersection`]: the 3D intersection curve (`None` only on a
/// two-point success without a curve) and its UV curves on both surfaces.
#[derive(Debug, Clone, Default)]
pub struct SrfSrfCurves {
    /// `rp3DCurve`.
    pub curve_3d: Option<Curve>,
    /// `rpUVCurve1`: its UV curve on the first surface.
    pub uv_curve1: Option<Curve>,
    /// `rpUVCurve2`: its UV curve on the second surface.
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
}

/// One candidate curve of the selection loop.
struct Candidate {
    curve_3d: Curve,
    uv_curve1: Option<Curve>,
    uv_curve2: Option<Curve>,
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
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one upstream routine
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
    // SmApproxTol3d dApproxTol: the double converts at the call
    let approx_tol = approx_tol3d(k, approx_tol);

    // surface and surf/surf intersection locals
    let mut curves_3d: Vec<Curve> = Vec::new();
    let mut surface1_uv_curves: Vec<Option<Curve>> = Vec::new();
    let mut surface2_uv_curves: Vec<Option<Curve>> = Vec::new();

    let domain1 = surf1.natural_uv_domain();
    let domain2 = surf2.natural_uv_domain();
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
        let r = ignore_status(surf2.intersect_with_plane(
            k,
            &domain2,
            plane,
            &domain1,
            use_surface_edges,
            Some(ssi_tol),
            Some(angle_tol),
        ))?;
        push(
            r,
            true,
            &mut curves_3d,
            &mut surface1_uv_curves,
            &mut surface2_uv_curves,
        );
    } else if let SurfaceKind::Plane(plane) = surf2.kind() {
        // get planar intersection
        let r = ignore_status(surf1.intersect_with_plane(
            k,
            &domain1,
            plane,
            &domain2,
            use_surface_edges,
            Some(ssi_tol),
            Some(angle_tol),
        ))?;
        push(
            r,
            false,
            &mut curves_3d,
            &mut surface1_uv_curves,
            &mut surface2_uv_curves,
        );
    }

    // Use two-pts intersector (pOptDir is a required argument of it).
    if curves_3d.is_empty() && !opts.skip_two_pnts_intersection {
        if let (Some(start_pnt), Some(dir)) = (start, opt_dir) {
            // an error here just means that it couldn't find a solution
            let r = ignore_status(two_pnts_intersection(
                k,
                start_pnt,
                end,
                dir,
                opt_end_dir,
                surf1,
                surf2,
                ssi_tol,
            ))?;
            if let Some(pi) = r {
                if let Some(curve_3d) = pi.curve_3d {
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
        let xs = surf1.global_surface_intersect(
            k,
            &domain1,
            surf2,
            &domain2,
            [true, true],
            Some(ssi_tol),
            Some(angle_tol),
        )?;
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
            });
            continue;
        }

        // when given an endPoint - trim intersection curves to endPoints
        if end.is_some() {
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
        });
    }

    // error: didn't find a curve within tol of endPoints
    if best_dist == NO_CURVE && best_len == NO_CURVE {
        return Err(SolidError::sm_err(OP_SRF_SRF));
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
        .ok_or_else(|| SolidError::sm_err(OP_SRF_SRF))?;

    // set output (SM_CAST_PTR to SmBSplineCurve)
    if !is_b_spline_kind(&best.curve_3d) {
        return Err(SolidError::null_pointer(OP_SRF_SRF));
    }
    let curve_3d = best.curve_3d;
    let mut uv_curve1 = best.uv_curve1.filter(is_b_spline_kind);
    let mut uv_curve2 = best.uv_curve2.filter(is_b_spline_kind);
    let trim_ivl = curve_3d.natural_interval();

    // generate surf1 UVTrimCurve if it's missing (some intersectors don't make them)
    if uv_curve1.is_none() {
        let uv_curves = surface_drop_curve(k, surf1, &domain1, &curve_3d, &trim_ivl, approx_tol)?;
        let first = uv_curves
            .into_iter()
            .next()
            .ok_or_else(|| SolidError::sm_err(OP_SRF_SRF))?;
        uv_curve1 = Some(first);
    }

    // generate surf2 UVTrimCurve if it's missing
    if uv_curve2.is_none() {
        let uv_curves = surface_drop_curve(k, surf2, &domain2, &curve_3d, &trim_ivl, approx_tol)?;
        let first = uv_curves
            .into_iter()
            .next()
            .ok_or_else(|| SolidError::sm_err(OP_SRF_SRF))?;
        uv_curve2 = Some(first);
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

/// The surface of a target face (`pFace->GetSurface()`, dereferenced).
fn face_surface<'b>(
    target: &'b Brep,
    face: Option<FaceId>,
    op: &'static str,
) -> Result<&'b Surface> {
    let f = face.ok_or_else(|| SolidError::null_pointer(op))?;
    face2::surface(target, f)?.ok_or_else(|| SolidError::null_pointer(op))
}

impl FilletBrep {
    /// The solver index of fillet geometry `g` (`GetFilletSolver()`, dereferenced).
    fn geom_solver_index(
        &self,
        g: FilletGeomId,
        solvers: &[FilletSolver],
        op: &'static str,
    ) -> Result<usize> {
        let id = self
            .geom(g)?
            .fillet_solver()
            .ok_or_else(|| SolidError::null_pointer(op))?;
        solver_index(solvers, id, op)
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
            .ok_or_else(|| SolidError::null_pointer(op))?;
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
    /// evaluations; [`SolidError::Unported`] for the corner's filleted vertex
    /// (`blend.fillet_corner_p1`) and those of `RailRailIntersect` (its
    /// `SmTemporaryRadiusChange`, `blend.fillet_executive_p2`).
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn calc_rail_int_rail(
        &mut self,
        k: &Kernel,
        v: VertexId,
        solvers: &mut [FilletSolver],
        target: &mut Brep,
    ) -> Result<()> {
        const OP: &str = OP_RAIL_RAIL;
        // First, make sure we have a corner and exactly two FilletGeom's.
        let corner = self
            .fillet_vertex(v)?
            .fillet_corner()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let geoms = self.vertex_fillet_geoms(v)?;
        if geoms.len() != 2 {
            return Err(SolidError::sm_err(OP));
        }
        let (fillet_geom1, fillet_geom2) = match geoms.as_slice() {
            [a, b] => (*a, *b),
            _ => return Err(SolidError::sm_err(OP)),
        };

        let cp_vertex = corner_filleted_vertex(corner)?;
        let s1 = self.geom_solver_index(fillet_geom1, solvers, OP)?;
        let s2 = self.geom_solver_index(fillet_geom2, solvers, OP)?;
        let fs1 = solver_at(solvers, s1, OP)?;
        let fs2 = solver_at(solvers, s2, OP)?;

        // Get the common face, and the indices of the rail curves.
        let face = as_face(self.fillet_vertex(v)?.point_class_object(), OP)?;
        let rail_index1 = fs1.find_index_of_rail_on_face(target, face)?;
        let rail_index2 = fs2.find_index_of_rail_on_face(target, face)?;

        // Figure a guess uv for the intersection of the rail curves in the face: twice
        // the fillet radius along each filleted edge from the common vertex.
        let mut uv = [Point3::new(0.0, 0.0, 0.0); 2];
        let pairs = [(fs1, rail_index1), (fs2, rail_index2)];
        for (slot, (fs, index)) in uv.iter_mut().zip(pairs) {
            let offset_distance = solver_surface(fs, index, OP)?.offset_distance().abs();
            let eu = solver_edgeuse(fs, index, OP)?;
            let e = eu1::edge(target, eu)?.ok_or_else(|| SolidError::null_pointer(OP))?;
            let ivl = edge::interval(target, e)?;
            let curve = edge::curve(target, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;

            let vu = eu1::vertexuse(target, eu)?.ok_or_else(|| SolidError::null_pointer(OP))?;
            let pv = vertexuse_unit::vertex(target, vu)?;
            let orient = eu1::orientation(target, eu)?;
            let offset_param = if (pv == cp_vertex && orient == OrientType::Same)
                || (pv != cp_vertex && orient == OrientType::Opposite)
            {
                arc_length_param(k, curve, ivl.min(), 2.0 * offset_distance, OP)?
            } else {
                arc_length_param(k, curve, ivl.max(), -2.0 * offset_distance, OP)?
            };
            let fetch = get_or_create_uv_trim_curve(k, target, eu, false, false, true)?;
            fetch.status?;
            let trim_curve = eu1::uv_trim_curve(target, eu)?
                .0
                .filter(|c| is_b_spline_kind(c))
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            *slot = trim_curve.evaluate_point(k, offset_param)?;
        }

        // Their midpoint is the guess.
        let [uv0, uv1] = uv;
        let mid_uv = (uv0 + uv1) * 0.5;
        let uv_guess = Point2::new(mid_uv.x, mid_uv.y);

        // Get the TsectPnts and call the solver.
        let (vu1, mut tsect1) = self.rail_end_tsect(v, Some(fillet_geom1), None, OP)?;
        let (vu2, mut tsect2) = self.rail_end_tsect(v, Some(fillet_geom2), None, OP)?;
        let found = rail_rail_intersect(
            k,
            target,
            solvers,
            s1,
            uv_guess,
            rail_index1,
            s2,
            rail_index2,
            &mut tsect1,
            &mut tsect2,
            OP,
        );
        // the vertex uses' points are written in place, also on a failure
        self.fillet_vertexuse_mut(vu1)?.set_tsect_pnt(&tsect1);
        self.fillet_vertexuse_mut(vu2)?.set_tsect_pnt(&tsect2);
        let found = found?;
        if !found {
            self.fillet_vertex_mut(v)?
                .set_status(FilStatus::SolverNotConverge);
            return Err(SolidError::sm_err(OP));
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
    /// [`SolidError::Unported`] for the corner (`blend.fillet_corner_p1`), the
    /// temporary radius change of a solver (`blend.fillet_executive_p2`) and those of
    /// `SmFilletSolver::RailEdgeuseIntersect`.
    pub fn calc_rail_int_edgeuse(
        &mut self,
        k: &Kernel,
        v: VertexId,
        opt_fg: Option<FilletGeomId>,
        solvers: &mut [FilletSolver],
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_rail_int_edgeuse_with_solvers(
            k,
            v,
            opt_fg,
            &mut RailSolvers::Indexed(solvers),
            target,
        )
    }

    /// Calculate a rail/side-edge vertex using one caller-owned solver and its
    /// actual identity. Geometry-owned offset surfaces are used before the solver.
    ///
    /// # Errors
    /// As [`Self::calc_rail_int_edgeuse`]; resolving an adjacent solver with another
    /// id returns [`SolidError::InvalidInput`] rather than fabricating its state.
    pub fn calc_rail_int_edgeuse_with_solver(
        &mut self,
        k: &Kernel,
        v: VertexId,
        opt_fg: Option<FilletGeomId>,
        solver_id: FilletSolverId,
        solver: &mut FilletSolver,
        target: &mut Brep,
    ) -> Result<()> {
        self.calc_rail_int_edgeuse_with_solvers(
            k,
            v,
            opt_fg,
            &mut RailSolvers::Single(solver_id, solver),
            target,
        )
    }

    #[allow(clippy::too_many_lines)] // one upstream routine
    fn calc_rail_int_edgeuse_with_solvers(
        &mut self,
        k: &Kernel,
        v: VertexId,
        opt_fg: Option<FilletGeomId>,
        solvers: &mut RailSolvers<'_>,
        target: &mut Brep,
    ) -> Result<()> {
        const OP: &str = OP_RAIL_EU;
        let corner = self
            .fillet_vertex(v)?
            .fillet_corner()
            .ok_or_else(|| SolidError::null_pointer(OP))?;

        // get original Brep vertex
        let cp_vertex = corner_filleted_vertex(corner)?;

        // get this filletVertex->filletEdgeuse[type == SM_FE_RAIL]->FilletGeom(s)
        let geoms = self.vertex_fillet_geoms(v)?;

        // check state - at least 1 railEdge must be connected to this vertex
        let total_geoms = geoms.len();
        let first = *geoms.first().ok_or_else(|| SolidError::sm_err(OP))?;
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
        let side_eu1 = side_eu1.ok_or_else(|| SolidError::null_pointer(OP))?;
        let side_edge = eu1::edge(target, side_eu1)?.ok_or_else(|| SolidError::null_pointer(OP))?;
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
            .ok_or_else(|| SolidError::null_pointer(OP))?
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
            // its status is ignored upstream (lRailIndex stays 0)
            let rail_index = ignore_status(self.rail_index_of_side_eu_resolved(
                g1,
                side_eu1,
                target,
                &|id| solvers.solver(id),
            ))?
            .unwrap_or(0);

            let vu1 = vu1.ok_or_else(|| SolidError::null_pointer(OP))?;
            let mut tsect1 = self.fillet_vertexuse(vu1)?.tsect_pnt().clone();
            let mut found = false;
            let mut edgeuse_parameter = 0.0;
            for &guess in &guess_edge_parameters {
                // Temporarily change this to a constant-radius function.
                let fs_for_change = if corner_is_calc_as_const_rad(Some(corner))? {
                    let si = RailSolvers::geom_solver(self, g1)?;
                    Some(solvers.solver(si)?)
                } else {
                    None
                };
                temporary_radius_change(fs_for_change, rail_index, side_eu1)?;

                // rTsectPnt1 is the vertex use's own point: written in place also on a
                // failure
                let r = rail_edgeuse_intersect(
                    self,
                    k,
                    g1,
                    solvers,
                    target,
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

                for &guess in &guess_edge_parameters {
                    temporary_radius_change(Some(solvers.solver(si)?), rail_index, side_eu1)?;
                    let r = rail_edgeuse_intersect(
                        self,
                        k,
                        g1,
                        solvers,
                        target,
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

            // when still not found
            if !found {
                self.fillet_vertex_mut(v)?
                    .set_status(FilStatus::NoIntRailEu);
                // Sometimes the fillet can still succeed.
                if corner_is_blending(corner)? {
                    return Err(SolidError::sm_err(OP));
                }
                return Ok(());
            }

            // force solution to lie on SideEdge->Curve: the rail/base surface point
            let uv = tsect1.uv_pos(srf(rail_index, OP)?);
            let pnt = solvers
                .offset_surface(self, g1, rail_index)?
                .ok_or_else(|| SolidError::null_pointer(OP))?
                .base_surface()
                .evaluate_point(k, uv)?;

            // project it onto the side edge's curve
            let side_e =
                eu1::edge(target, side_eu1)?.ok_or_else(|| SolidError::null_pointer(OP))?;
            let curve = edge::curve(target, side_e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
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
                    .ok_or_else(|| SolidError::null_pointer(OP))?
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
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                let uv1 = tsect1.uv_pos(other_srf(rail_index, OP)?);
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
            // its status is ignored upstream
            let other_rail_index =
                // upstream dereferences a NULL side edge use there
                ignore_status(match side_eu2 {
                    Some(eu) => self.rail_index_of_side_eu_resolved(g2, eu, target, &|id| solvers.solver(id)),
                    None => Err(SolidError::null_pointer(OP)),
                })?
                .unwrap_or(0);
            let side_eu2 = side_eu2.ok_or_else(|| SolidError::null_pointer(OP))?;

            let (vu2, mut tsect2) = self.rail_end_tsect(v, Some(g2), vu1, OP)?;
            let mut found = false;
            let mut edgeuse_parameter2 = 0.0;
            for &guess in &guess_edge_parameters {
                // Temporarily change this to a constant-radius function.
                let si = RailSolvers::geom_solver(self, g2)?;
                temporary_radius_change(Some(solvers.solver(si)?), other_rail_index, side_eu2)?;
                let r = rail_edgeuse_intersect(
                    self,
                    k,
                    g2,
                    solvers,
                    target,
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
                    .ok_or_else(|| SolidError::null_pointer(OP))?
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
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                let uv1 = tsect2.uv_pos(other_srf(other_rail_index, OP)?);
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
    /// without a fillet geometry; those of the evaluations; [`SolidError::Unported`] for
    /// the corner (`blend.fillet_corner_p1`).
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn calc_vert_on_cross_section(
        &mut self,
        k: &Kernel,
        v: VertexId,
        opt_edge_param: Option<f64>,
        solvers: &mut [FilletSolver],
        target: &mut Brep,
    ) -> Result<()> {
        const OP: &str = OP_CROSS;
        let corner = self
            .fillet_vertex(v)?
            .fillet_corner()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let cp_vertex = corner_filleted_vertex(corner)?;

        // Determine where the cross-sectional plane is
        let geoms = self.vertex_fillet_geoms(v)?;
        let total_geoms = geoms.len();
        let fillet_geom1 = *geoms.first().ok_or_else(|| SolidError::sm_err(OP))?;
        let s1 = self.geom_solver_index(fillet_geom1, solvers, OP)?;
        let fs1 = solver_at(solvers, s1, OP)?;
        let eu = solver_edgeuse(fs1, 0, OP)?;
        let e = eu1::edge(target, eu)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        let ivl = edge::interval(target, e)?;
        let mut vert_at_edge_start = true;
        let mut param = ivl.min();
        if let Some(p) = opt_edge_param {
            param = p;
        } else if edge::start_vertex(target, e)? != cp_vertex {
            param = ivl.max();
            vert_at_edge_start = false;
        }
        let curve = edge::curve(target, e)?
            .ok_or_else(|| SolidError::null_pointer(OP))?
            .clone();
        let pv = curve.evaluate(k, param, 1, true, true)?;
        let plane_orig = pv.first().copied().ok_or_else(|| SolidError::sm_err(OP))?;
        let plane_normal = pv.get(1).copied().ok_or_else(|| SolidError::sm_err(OP))?;
        let (vu1, mut tsect) = self.rail_end_tsect(v, Some(fillet_geom1), None, OP)?;

        // the vertex's UV points on the solver's two faces
        let vertexuses = match cp_vertex {
            Some(cv) => vertex::vertexuses(target, cv)?,
            None => return Err(SolidError::null_pointer(OP)),
        };
        let mut uvs = [Point2::new(0.0, 0.0); 2];
        for (i, slot) in uvs.iter_mut().enumerate() {
            let face = eu1::face(target, solver_edgeuse(fs1, i, OP)?)?;
            for &vu in &vertexuses {
                let fu = vertexuse_unit::faceuse(target, vu)?
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                if target.faceuse(fu)?.face == face {
                    // its status is ignored upstream (the point is then whatever the
                    // call left; here the previous value)
                    if let Some(uv) =
                        ignore_status(vertexuse_unit::compute_uv_point(k, target, vu, true))?
                    {
                        *slot = uv;
                    }
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
                .ok_or_else(|| SolidError::sm_err(OP))?;
            let dv = m
                .first()
                .and_then(|r| r.get(1))
                .copied()
                .ok_or_else(|| SolidError::sm_err(OP))?;
            let deg = rad2deg(du.angle_between(dv)?);
            if !(30.0..=150.0).contains(&deg) {
                // May lead to potential failures, step off the boundary: along the
                // edge first, then off the boundary edge
                param = arc_length_param(k, &curve, param, 2.0 * curve_stepoff_dist, OP)?;
                uvs = find_stepoff_uv(k, target, fs1, param, stepoff_dist)?;
                break;
            }
        }

        let domain1 = solver_surface(fs1, 0, OP)?.natural_uv_domain();
        let domain2 = solver_surface(fs1, 1, OP)?.natural_uv_domain();
        let mut found_solution = false;

        // If the solver doesn't succeed the first time, step off along the edge.
        for _ in 0..3 {
            let [u0, u1] = uvs;
            let (found, t) = point_on_plane_solve(
                k,
                solver_at_mut(solvers, s1, OP)?,
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
            param = arc_length_param(k, &curve, param, curve_stepoff_dist, OP)?;
            uvs = find_stepoff_uv(k, target, solver_at(solvers, s1, OP)?, param, stepoff_dist)?;
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
            let fillet_geom2 = *geoms.get(1).ok_or_else(|| SolidError::sm_err(OP))?;
            let vu2 = self.vu_at_rail_end(v, Some(fillet_geom2), Some(vu1), None)?;
            let s2 = self.geom_solver_index(fillet_geom2, solvers, OP)?;
            let eu2 = solver_edgeuse(solver_at(solvers, s2, OP)?, 0, OP)?;
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

    /// `SmFilletVertex::CalcSetBackVert`: the vertex on the plane normal to the
    /// filleted edge at the corner's set-back distance from the corner vertex, solved by
    /// `PointOnPlaneSolve` from the plane origin's closest points on the solver's
    /// surfaces; its mate gets the matching point.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without corner, solver, edge use, curve, surface or face;
    /// `SM_ERR` without exactly one fillet geometry and unless each surface has exactly
    /// one closest point; those of the evaluations and solves;
    /// [`SolidError::Unported`] for the corner (`blend.fillet_corner_p1`).
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn calc_set_back_vert(
        &mut self,
        k: &Kernel,
        v: VertexId,
        solvers: &mut [FilletSolver],
        target: &Brep,
    ) -> Result<()> {
        const OP: &str = OP_SETBACK;
        let corner = self
            .fillet_vertex(v)?
            .fillet_corner()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let cp_vertex = corner_filleted_vertex(corner)?;
        let set_back_dist = corner_set_back_dist(corner)?;

        let geoms = self.vertex_fillet_geoms(v)?;
        if geoms.len() != 1 {
            return Err(SolidError::sm_err(OP));
        }
        let fillet_geom = *geoms.first().ok_or_else(|| SolidError::sm_err(OP))?;
        let si = self.geom_solver_index(fillet_geom, solvers, OP)?;
        let fs = solver_at(solvers, si, OP)?;
        let eu = solver_edgeuse(fs, 0, OP)?;
        let e = eu1::edge(target, eu)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        let curve = edge::curve(target, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        let ivl = curve.natural_interval();

        // Walk along the curve for dSetBackDist
        let pv = if edge::vertex(target, e)? == cp_vertex {
            let start_param = ivl.min();
            let end_param = arc_length_param(k, curve, start_param, set_back_dist, OP)?;
            curve.evaluate(k, end_param, 1, true, true)?
        } else {
            let mut new_curve = curve.copy(k)?;
            let new_ivl = new_curve.reverse_parameterization(k, &ivl)?;
            let start_param = new_ivl.min();
            let end_param = arc_length_param(k, &new_curve, start_param, set_back_dist, OP)?;
            new_curve.evaluate(k, end_param, 1, true, true)?
        };
        let plane_orig = pv.first().copied().ok_or_else(|| SolidError::sm_err(OP))?;
        let plane_normal = pv.get(1).copied().ok_or_else(|| SolidError::sm_err(OP))?;

        let surf1 = solver_surface(fs, 0, OP)?;
        let surf2 = solver_surface(fs, 1, OP)?;
        let domain1 = surf1.natural_uv_domain();
        let domain2 = surf2.natural_uv_domain();
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
            if solutions.len() != 1 {
                return Err(SolidError::sm_err(OP));
            }
            let s = solutions.get(0).ok_or_else(|| SolidError::sm_err(OP))?;
            Ok(Point2::new(s.start.param(0)?, s.start.param(1)?))
        };
        let uv1 = closest(surf1, &domain1)?;
        let uv2 = closest(surf2, &domain2)?;

        // find filletPoint on given plane that satisfies geometry requirements
        let (found, tsect) = point_on_plane_solve(
            k,
            solver_at_mut(solvers, si, OP)?,
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
            .ok_or_else(|| SolidError::null_pointer(OP))?;
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
    /// cubic rail/rail interpolant.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without original face, B-spline surface, rail, edge use or
    /// curve; `SM_ERR` when a vertex does not drop onto its rail within 100 times the
    /// tolerance or a drop gives other than one UV curve; those of the evaluations,
    /// constructions, lifts and `SmSurface::DropCurve`; [`SolidError::Unported`] for the
    /// corner (`blend.fillet_corner_p1`).
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn calc_rail_rail_interpolation(
        &mut self,
        k: &Kernel,
        e: EdgeId,
        opt_corner: Option<FilletCornerId>,
        solvers: &mut [FilletSolver],
        target: &mut Brep,
    ) -> Result<()> {
        const OP: &str = OP_INTERP;
        let mut fsg_type = FilletSurfaceGeneratorType::Circular; // Default
        let mut num_derivatives: usize = 1; // 0 - linear; 1, 2, or 3 for G1, G2 or G3

        // when given a corner - use its properties to pick a lNumDerivatives value
        if let Some(corner) = opt_corner {
            let fillet_edges = corner_fillet_edges(corner)?;
            let total_edges = fillet_edges.len();
            if total_edges <= 4 {
                let index = fillet_edges
                    .iter()
                    .position(|&x| x == e)
                    .ok_or_else(|| SolidError::sm_err(OP))?;
                let test_edge = *fillet_edges
                    .get((index + 2) % total_edges)
                    .ok_or_else(|| SolidError::sm_err(OP))?;
                if self.fillet_edge(test_edge)?.fillet_edge_type() != FilletEdgeType::CrossSection {
                    return Err(SolidError::sm_err(OP));
                }
                let prim_eu = primary_edgeuse(self.brep(), test_edge)?
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                let fillet_geom = self
                    .fillet_edgeuse(prim_eu)?
                    .fillet_geom()
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                let si = self.geom_solver_index(fillet_geom, solvers, OP)?;
                let fsg = solver_at(solvers, si, OP)?
                    .fillet_surface_generator()
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
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
        // sPntVec[4]: at most three derivatives are read
        let num_derivatives = num_derivatives.min(3);

        let start_fv = edge::vertex(self.brep(), e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        let end_fv = edge::other_vertex(self.brep(), e, Some(start_fv))?
            .ok_or_else(|| SolidError::null_pointer(OP))?;

        let face = self
            .fillet_edge(e)?
            .original_face()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        // a copy: the cubic interpolant below needs the target B-rep mutably
        let face_srf = face2::surface(target, face)?
            .ok_or_else(|| SolidError::null_pointer(OP))?
            .clone();
        let face_srf = &face_srf;
        let surf_net = b_spline_net(face_srf).ok_or_else(|| SolidError::null_pointer(OP))?;
        let uv_domain = face_srf.natural_uv_domain();
        let tol = face2::tolerance(target, face)?;

        // Determine if we want to work on 2D or 3D
        let mut project_and_lift = false;
        let mut create_crv_on_3d = false;
        let is_plane = match Plane::is_nurb_surface_plane(k, surf_net, 1.0) {
            Ok(p) => p.is_some(),
            Err(err) if err.is_unported() => return Err(err),
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
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let eu1_id = primary_edgeuse(self.brep(), rail1)?
            .and_then(|p| eu1::mate(self.brep(), p).ok().flatten())
            .ok_or_else(|| SolidError::null_pointer(OP))?;
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
        let curve1 = curve1.ok_or_else(|| SolidError::null_pointer(OP))?;

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
            Err(SolidError::sm_err(OP))
        };
        let param1 = drop_onto(&curve1, start_pnt)?;

        let orient1 = eu1::orientation(self.brep(), eu1_id)?;
        let rail1_vertex = edge::vertex(self.brep(), rail1)?;
        let same_orient1 = !((rail1_vertex != Some(start_fv) && orient1 == OrientType::Same)
            || (rail1_vertex == Some(start_fv) && orient1 == OrientType::Opposite));

        // Get "incoming" rail-curve parameter at end vertex
        let rail2 = self
            .first_rail_at(end_fv)?
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let eu2_id = primary_edgeuse(self.brep(), rail2)?
            .and_then(|p| eu1::mate(self.brep(), p).ok().flatten())
            .ok_or_else(|| SolidError::null_pointer(OP))?;
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
        let curve2 = curve2.ok_or_else(|| SolidError::null_pointer(OP))?;
        let param2 = drop_onto(&curve2, end_pnt)?;

        let orient2 = eu1::orientation(self.brep(), eu2_id)?;
        let rail2_vertex = edge::vertex(self.brep(), rail2)?;
        let same_orient2 = (rail2_vertex != Some(end_fv) && orient2 == OrientType::Same)
            || (rail2_vertex == Some(end_fv) && orient2 == OrientType::Opposite);

        // Calc tangent (and higher derivatives) from the rail at start vertex
        let mut pnt_vec1 = curve1.evaluate(k, param1, num_derivatives, true, true)?;
        if !same_orient1 {
            for d in pnt_vec1.iter_mut().skip(1).take(num_derivatives) {
                *d = -*d;
            }
        }
        // Calc tangent from the other rail
        let mut pnt_vec2 = curve2.evaluate(k, param2, num_derivatives, true, true)?;
        if !same_orient2 {
            for d in pnt_vec2.iter_mut().skip(1).take(num_derivatives) {
                *d = -*d;
            }
        }

        let new_curve = if fsg_type == FilletSurfaceGeneratorType::Linear {
            // Linear cases
            BSplineCurve::create_line_segment(3, start_pnt, end_pnt)?
        } else {
            // In G1 cases, we create the 3d curve and then drop it onto the surface
            // (the `TRUE` branch: upstream's G2/G3 fairing branch is unreachable)
            let mut start_vec = pnt_vec1.get(1).copied().unwrap_or(Vec3::UNDEF);
            let mut end_vec = pnt_vec2.get(1).copied().unwrap_or(Vec3::UNDEF);
            interpolate_endpts_and_tangents(start_pnt, end_pnt, &mut start_vec, &mut end_vec, 3)?
        };

        // p3DCurve, an SmBSplineCurve: a B-spline, or the line / conic iso curve
        // LiftCurve's sm_ExtractIsoCurve makes
        let mut curve_3d: Option<Curve>;
        let mut uv_curve: Option<Curve>;
        if create_crv_on_3d {
            // Compute UV-curve by projecting 3d curve onto the surface
            let c3 = Curve::from(new_curve);
            let uv_curves =
                surface_drop_curve(k, face_srf, &uv_domain, &c3, &c3.natural_interval(), tol)?;
            // Will replace the original UV-curve when we have only one result
            if uv_curves.len() != 1 {
                return Err(SolidError::sm_err(OP));
            }
            uv_curve = uv_curves.into_iter().next();
            curve_3d = Some(c3);
        } else {
            let mut uv = new_curve;
            // its status is ignored upstream
            ignore_status(uv.convert_to_2d())?;
            uv_curve = Some(Curve::from(uv));
            curve_3d = None;
        }

        if !create_crv_on_3d || project_and_lift {
            // Lift the 3d curve
            let uv = uv_curve
                .as_ref()
                .ok_or_else(|| SolidError::null_pointer(OP))?;
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
                    .ok_or_else(|| SolidError::null_pointer(OP))?
                    .reparametrize_with_arc_length()?;
                // Reproject the 3d curve back onto the surface
                let uv_curves =
                    surface_drop_curve(k, face_srf, &uv_domain, &c3, &c3.natural_interval(), tol)?;
                if uv_curves.len() != 1 {
                    return Err(SolidError::sm_err(OP));
                }
                uv_curve = uv_curves.into_iter().next();
            }
            curve_3d = Some(c3);
        }

        if let Some(corner) = opt_corner {
            if corner_type(corner)? == FilletCornerType::NByNConcave {
                let c3 = curve_3d
                    .as_ref()
                    .and_then(b_spline_base)
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                // Test and see if this edge intersects with the original corner
                if corner_test_rail_rail_interpolation_curve(corner, c3)? {
                    // Corner vertex is on the opposite side of the curve
                    let solver_of_single_geom = |fb: &Self, fv: VertexId| -> Result<usize> {
                        let geoms = fb.vertex_fillet_geoms(fv)?;
                        if geoms.len() != 1 {
                            return Err(SolidError::sm_err(OP));
                        }
                        let g = *geoms.first().ok_or_else(|| SolidError::sm_err(OP))?;
                        fb.geom_solver_index(g, solvers, OP)
                    };
                    let si1 = solver_of_single_geom(self, start_fv)?;
                    let si2 = solver_of_single_geom(self, end_fv)?;

                    // Re-compute UV-curve
                    let uv = uv_curve
                        .as_ref()
                        .ok_or_else(|| SolidError::null_pointer(OP))?;
                    let ivl = uv.natural_interval();
                    let pv = uv.evaluate(k, ivl.min(), 1, true, true)?;
                    let start_uv_pnt = pv.first().copied().ok_or_else(|| SolidError::sm_err(OP))?;
                    let start_uv_vec = pv.get(1).copied().ok_or_else(|| SolidError::sm_err(OP))?;
                    let pv = uv.evaluate(k, ivl.max(), 1, true, true)?;
                    let end_uv_pnt = pv.first().copied().ok_or_else(|| SolidError::sm_err(OP))?;
                    let end_uv_vec = -pv.get(1).copied().ok_or_else(|| SolidError::sm_err(OP))?;
                    let corner_vertex = corner_filleted_vertex(corner)?
                        .ok_or_else(|| SolidError::null_pointer(OP))?;
                    let new_uv = with_solver_pair(solvers, si1, si2, OP, |s1, s2| {
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
                        )
                    })?;
                    let new_uv = Curve::from(new_uv);
                    // Lift the UV-curve
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
        let curve_3d = curve_3d.ok_or_else(|| SolidError::null_pointer(OP))?;
        set_edge_curve(self.brep_mut(), e, Some(curve_3d))?;
        let mate_eu = primary_edgeuse(self.brep(), e)?
            .and_then(|p| eu1::mate(self.brep(), p).ok().flatten())
            .ok_or_else(|| SolidError::null_pointer(OP))?;
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
                return Err(SolidError::null_pointer(OP_SPLIT));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::b_spline_curve::BSplineCurve;

    #[test]
    fn single_solver_lookup_preserves_nonzero_identity_and_live_mutations() {
        let id = FilletSolverId(37);
        let mut solver = FilletSolver::new(1e-5, 1e-3, 0.01);
        let mut table = RailSolvers::Single(id, &mut solver);
        table.solver_mut(id).unwrap().set_extension_factor(4.5);
        assert_eq!(table.solver(id).unwrap().extension_factor(), 4.5);
        assert!(matches!(
            table.solver(FilletSolverId(0)),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            table.solver_mut(FilletSolverId(38)),
            Err(SolidError::InvalidInput { .. })
        ));
        assert_eq!(solver.extension_factor(), 4.5);
    }

    #[test]
    fn geometry_offsets_precede_single_solver_resolution() {
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
        let mut solver = FilletSolver::new(1e-5, 1e-3, 0.01);
        let table = RailSolvers::Single(FilletSolverId(37), &mut solver);
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
            .unwrap(),
        );
        fb.destroy_geom(g).unwrap();
        assert!(fb.geom(g).unwrap().center_line_curve.is_none());
        assert!(fb.geom(g).unwrap().vertices.is_empty());
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
        let null =
            |r: Result<()>| matches!(r, Err(SolidError::Upstream { code, .. }) if code == 1006);
        assert!(null(fb.calc_rail_int_rail(
            &k,
            v,
            &mut solvers,
            &mut target
        )));
        assert!(null(fb.calc_rail_int_edgeuse(
            &k,
            v,
            None,
            &mut solvers,
            &mut target
        )));
        assert!(null(fb.calc_vert_on_cross_section(
            &k,
            v,
            None,
            &mut solvers,
            &mut target
        )));
        assert!(null(fb.calc_set_back_vert(&k, v, &mut solvers, &target)));
        // with a corner: no rail geometries is SM_ERR, the filleted vertex is the
        // corner unit's
        fb.fillet_vertex_mut(v)
            .unwrap()
            .set_fillet_corner(Some(FilletCornerId(0)));
        assert!(matches!(
            fb.calc_rail_int_rail(&k, v, &mut solvers, &mut target),
            Err(SolidError::Upstream { code: 1001, .. })
        ));
        assert!(fb
            .calc_rail_int_edgeuse(&k, v, None, &mut solvers, &mut target)
            .unwrap_err()
            .is_unported());
    }
}
