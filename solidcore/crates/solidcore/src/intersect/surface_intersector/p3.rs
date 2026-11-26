//! `SmSurfaceIntersector`, part 3: the start points on the natural boundaries
//! ([`SurfaceIntersector::find_boundary_start_points`]), the tracing step
//! ([`SurfaceIntersector::compute_next_point`]), the end-of-trace test with its
//! snapping to through, start and end points ([`SurfaceIntersector::test_point`],
//! [`SurfaceIntersector::update_tsect_point`]) and the fillet-law span split
//! ([`SurfaceIntersector::split_span`]).
//!
//! Upstream: `source/SMLib/src/SmSurfaceIntersector.cpp`.
//!
//! | Rust                                                  | Upstream                                   |
//! |-------------------------------------------------------|--------------------------------------------|
//! | [`SurfaceIntersector::find_boundary_start_points`], [`SurfaceIntersector::find_boundary_start_points_with_caches`], [`SurfaceIntersector::find_boundary_start_points_with_queries`] | `FindBoundaryStartPoints` |
//! | [`SurfaceIntersector::compute_next_point`], [`NextPoint`] | `ComputeNextPoint`                     |
//! | [`SurfaceIntersector::update_tsect_point`], [`update_tsect_point_with`] | `sm_UpdateTsectPoint` (static) |
//! | [`SurfaceIntersector::test_point`], [`test_point_with`] | `TestPoint`                              |
//! | [`SurfaceIntersector::split_span`], [`split_span_with`] | `SplitSpan`                              |
//! | [`TsectPnt::zeroed`]                                  | a point fresh from `m_vTSPntMgr` (zero-filled) |
//!
//! Deviations:
//! - **Virtual calls.** `TestPoint` and `sm_UpdateTsectPoint` are generic over the
//!   intersector (`intersect::surface_intersector::p4::IntersectorVirtuals`), so the
//!   `ComputePointValues` of `sm_UpdateTsectPoint` is the derived intersector's when
//!   `TraceCurve` runs on one ([`test_point_with`], [`update_tsect_point_with`]); the
//!   methods run them on the basic intersector. `SplitSpan` is generic the same way
//!   ([`split_span_with`]), for the virtual calls of `ComputeLawPointValues`.
//! - **Points.** Points are arena ids ([`TsectId`]), as in part 1: `rNextTSP`, the
//!   previous point and the snap targets of `TestPoint` / `sm_UpdateTsectPoint` are
//!   points of the arena (`TraceCurve` takes `rNextTSP` from `m_vTSPntMgr` too). A next
//!   point that is also the previous point (the last curve point) is rejected with
//!   [`SolidError::InvalidInput`]: upstream would evaluate a point onto itself
//!   through two aliases. Every other alias (a snap target that is the next point)
//!   behaves as upstream: the arena is re-read after each write.
//! - **Null previous point.** `TestPoint` dereferences the last curve point
//!   whenever there are through points, a start point (`m_pStartPoint`) or an end
//!   point; with an empty curve list that is a null dereference upstream and
//!   [`SolidError::InvalidInput`] here.
//! - **Outputs.** Out-params become return values. `TestPoint`'s `rbDone` is
//!   `None` when upstream leaves it unchanged (a through point was hit);
//!   `ComputeNextPoint`'s deviation and angle are `None` when upstream leaves them
//!   unset. `SplitSpan` returns the new point's id. `FindBoundaryStartPoints`'
//!   output arrays are a [`TsectCurves`] (part 2).
//! - **Surface caches.** `smsurf_GetSurfaceCache` ([`surface_cache`]) builds a
//!   fresh cache per call (the cache store keeps none), so the one of each surface
//!   is fetched once per call (once for a self intersection, where upstream gets
//!   the same cache twice); a surface without one is `SM_ERR_NULL_POINTER` (`NER`).
//!   Their `process_boundary_curves` flags are set as upstream's
//!   `SmTemporaryChangeValue`s set them; a callee that fetches its own cache does not
//!   see them. [`SurfaceIntersector::find_boundary_start_points_with_caches`] takes
//!   the caches from the caller instead. Their iso boundary curves are read, and
//!   `GlobalCurveIntersect`, which upstream runs on the other surface's one cache
//!   (`smsurf_GetSurfaceCache` returns the intersector's), runs on a copy of the
//!   caller's cache whose handles name the curve and surface objects of the solve
//!   ([`Surface::global_curve_intersect_in`], `SurfaceCache::map_handles`), so its
//!   line and tree solves see the same tree. The temporary removal of the other surface's owner (point validation)
//!   has no counterpart: Rust surfaces have no owner.
//! - **Points of `FindBoundaryStartPoints`.** Upstream takes a point from the block
//!   manager for every solution and leaks those it does not keep; here a solution's
//!   point is a local [`TsectPnt::zeroed`] value that gets its arena slot when it
//!   becomes a start point.
//! - **Touching points on a start point** (not upstream). `FindBoundaryStartPoints`
//!   drops a touching point on a start point for good; here it is kept aside, and
//!   `DoIntersection` adds it at the end when no curve found covers it
//!   ([`SurfaceIntersector::add_deferred_touching_points`]). A crossing curve through
//!   it that runs along the boundary curve it lies on is that touch and gives way to it
//!   ([`SurfaceIntersector::runs_along_boundary`]).
//! - **Dead code.** The face edge branch of `FindBoundaryStartPoints`
//!   (`static constexpr bGWCAddEdgeThroughPoints = FALSE`: target curves beyond the
//!   four natural boundaries, `ApproximateCurve` of non-B-spline targets, through
//!   points, `RefinePoint` of edge crossings) and the parts of `TestPoint` behind
//!   `static bStopAtSingularity = FALSE` (stop at singular points, the final through
//!   and end point snapping of a boundary point) are unreachable upstream and not
//!   ported. The cycle back-edges of the plan (`topology.edge`, `topology.edgeuse_p1`,
//!   `topology.edgeuse_p3`, `topology.face_p4`) are all in the face edge branch.
//! - **Previous UVs.** `ComputeNextPoint` hands `RefinePoint` the address of the
//!   previous point's first UV position, which the callee may read as the array
//!   `m_vUVCurvePV` (`[uv0, duv0, uv1, duv1]`); that array is passed.
//! - **A step to a pole past a boundary it does not reach** (not upstream). Where the
//!   curve runs into a pole at the corner of a domain (a sphere's pole on its seam
//!   meridian), `ComputeNextPoint` clipped the step at the seam, short of the pole, though
//!   the refined point was not on the seam; the following short step to the pole left the
//!   pole point with an ill-conditioned UV tangent and singular parameter, and the trims
//!   failed the body check. Such a step goes on to the pole
//!   ([`SurfaceIntersector::pole_corner_step`]).
//! - **Boundary curve intervals** (not upstream). `FindBoundaryStartPoints` intersects
//!   each iso boundary curve with the other surface over the domain's interval
//!   (`sIntervals`), though the curve's trim to that domain can end it at a knot just
//!   inside it (a short end span of a rational curve snapped to the knot, or a
//!   degenerate one removed). The solve then returned range ends outside the curve, and
//!   trimming the curve to them failed (#17450). The interval is the domain's within
//!   the curve's own, so its ends are the curve's where the curve is the shorter.
//! - **Unported callees.**
//!   `SmSurface::DropAndTrimCurve`'s trace (`intersect.surface_drop_curve`, through
//!   [`Surface::drop_and_trim_curve`]) return [`SolidError::Unported`], which always propagates (also where upstream
//!   ignores the status).
//! - **Not ported here.** The `SM_DEBUG_CODE` blocks (graphics, dumps), the
//!   `SM_ASSERT`s (no-ops in release builds) and the user pointers
//!   (`m_apUserPointer`, not modelled by [`TsectPnt`]).

use crate::blend::blend_corner::on_face::{bezier_pieces, pieces_rounding, Hom};
use crate::blend::fillet_standard_solver::p1::FilletLaw;
use crate::curve::curve::Curve;
use crate::error::intersect::IntersectFailure;
use crate::error::{Failure, Result, SolidError};
use crate::geom::extent1d::{Extent1d, ExtentPointType};
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::geom_utility::p1::line_closest_point;
use crate::geom::global_solver::SolverOperation;
use crate::geom::solution_array::{
    ObjectKind, Solution, SolutionEnd, SolutionObject, SolutionType,
};
use crate::geom::tol::{is_zero, ZONE_TOL_3D};
use crate::geom::vector2d::{Point2, Vec2};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{max2, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::p2::BoundingBoxRequest;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::math::gamma;
use crate::surface::surface::p1::SurfaceSolveContext;
use crate::surface::surface::p2::surface_cache;
use crate::surface::surface::p2::TsectCurveType;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::surface_cache::face_context::FaceQueries;
use crate::surface::surface_cache::{SolveModes, SurfaceCache};
use crate::topology::ObjectHandle;

use super::p1::{
    compute_curve_type, is_point_on_tsect_curve, IntersectionPointType, SrfIndex,
    SurfaceIntersector, TsectId, TsectPnt, MAX_USER_DOUBLES,
};
use super::p2::{LawPointOptions, TsectCurves};
use super::tangent_curves::remove_curve;

const OP_FBSP: &str =
    "intersect::surface_intersector::p3::SurfaceIntersector::find_boundary_start_points";
const OP_NEXT: &str = "intersect::surface_intersector::p3::SurfaceIntersector::compute_next_point";
const OP_UPDATE: &str =
    "intersect::surface_intersector::p3::SurfaceIntersector::update_tsect_point";
const OP_TEST: &str = "intersect::surface_intersector::p3::SurfaceIntersector::test_point";
const OP_ALONG: &str =
    "intersect::surface_intersector::p3::SurfaceIntersector::runs_along_boundary";

/// The step to a pole of [`SurfaceIntersector::pole_corner_step`] (not upstream) and the
/// degenerate boundary it reaches.
#[derive(Debug, Clone, Copy)]
struct PoleCorner {
    /// The step along the UV tangents.
    step: f64,
    /// The surface with the pole.
    srf: SrfIndex,
    /// Whether the degenerate boundary is one of constant v (else of constant u).
    on_v_boundary: bool,
    /// The parameter of the degenerate boundary.
    bound: f64,
    /// The UV tolerance "on the boundary" is measured with.
    uv_tol: f64,
    /// The pole: the surface's point at the corner its singularity was tested at.
    pole: Point3,
}

impl PoleCorner {
    /// The refined UV point `uvs` on the surface with the pole.
    fn uv(&self, uvs: [Point2; 2]) -> Point2 {
        let [uv0, uv1] = uvs;
        if self.srf == SrfIndex::First {
            uv0
        } else {
            uv1
        }
    }

    /// Whether the refined UV points `uvs` are on the degenerate boundary.
    fn on_boundary(&self, uvs: [Point2; 2]) -> bool {
        let uv = self.uv(uvs);
        let at = if self.on_v_boundary { uv.y } else { uv.x };
        (at - self.bound).abs() <= self.uv_tol
    }
}

/// The outputs of [`SurfaceIntersector::compute_next_point`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NextPoint {
    /// `rbFoundGoodPoint`: the next point passed the direction, angle and deviation
    /// tests.
    pub found_good_point: bool,
    /// `rdDeviationFound`: the largest curve/surface distance of the span test
    /// points; `None` when upstream leaves it unset.
    pub deviation_found: Option<f64>,
    /// `rdAngleFoundRad`: the angle between the two curve tangents; `None` when
    /// upstream leaves it unset.
    pub angle_found_rad: Option<f64>,
    /// `rbClipped`: the step was shortened to keep the next point in both domains.
    pub clipped: bool,
    /// `rbBoundaryHit`: the step leaves a domain (or the next point could not be
    /// evaluated); the trace ends.
    pub boundary_hit: bool,
}

impl TsectPnt {
    /// A point as `m_vTSPntMgr.GetNewElement()` hands it out: zero-filled memory,
    /// never constructed (type `SM_IP_UNDEFINED`, parameter types `SM_EP_START`,
    /// every vector and value zero).
    #[must_use]
    pub const fn zeroed() -> Self {
        Self {
            point_type: IntersectionPointType::Undefined,
            uv_param_type: [[ExtentPointType::Start; 2]; 2],
            uv_curve_pv: [[Vec2::new(0.0, 0.0); 2]; 2],
            surface_pv: [[[Vec3::ZERO; 3]; 3]; 2],
            curve_pv: [Vec3::ZERO; 2],
            curve_parameter: 0.0,
            tangent_plane_angle_rad: 0.0,
            deviation: 0.0,
            user_doubles: [0.0; MAX_USER_DOUBLES],
            user_pointer: [None, None],
        }
    }
}

/// The last output 3D curve (the one being added).
fn last_curve(out: &mut TsectCurves) -> Result<&mut Curve> {
    out.curves_3d
        .last_mut()
        .ok_or_else(|| SolidError::invalid_input(OP_FBSP, "no output curve"))
}

/// `rSol.m_vStart[i]` / `m_vEnd[i]`.
fn par(e: &SolutionEnd, i: usize) -> f64 {
    e.parameters.get(i).copied().unwrap_or(0.0)
}

/// Removes `id` from a point list (`SmTList::Remove`).
fn unlink(list: &mut Vec<TsectId>, id: TsectId) {
    if let Some(pos) = list.iter().position(|&x| x == id) {
        list.remove(pos);
    }
}

/// A status upstream ignores (`Unported` propagates).
fn ignored<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
    }
}

/// `a[srf]`.
const fn at<T>(a: &[T; 2], s: SrfIndex) -> &T {
    match s {
        SrfIndex::First => &a[0],
        SrfIndex::Second => &a[1],
    }
}

/// `a[srf]`, mutable.
fn at_mut<T>(a: &mut [T; 2], s: SrfIndex) -> &mut T {
    match s {
        SrfIndex::First => &mut a[0],
        SrfIndex::Second => &mut a[1],
    }
}

/// The cache of surface `i` (0 or 1) among `caches`: the last one when there are fewer
/// (a self intersection passes one), [`IntersectFailure::NoSurfaceCaches`] for none.
fn cache_mut<T>(caches: &mut [T], i: usize) -> Result<&mut T> {
    match caches.split_last_mut() {
        Some((last, init)) => Ok(init.get_mut(i).unwrap_or(last)),
        None => Err(SolidError::intersect(
            OP_FBSP,
            IntersectFailure::NoSurfaceCaches,
        )),
    }
}

/// As [`cache_mut`], shared.
fn cache_ref<T>(caches: &[T], i: usize) -> Result<&T> {
    match caches.split_last() {
        Some((last, init)) => Ok(init.get(i).unwrap_or(last)),
        None => Err(SolidError::intersect(
            OP_FBSP,
            IntersectFailure::NoSurfaceCaches,
        )),
    }
}

/// `SrfIndex` of `m_cpSurface[i]`.
const fn srf(i: usize) -> SrfIndex {
    if i == 0 {
        SrfIndex::First
    } else {
        SrfIndex::Second
    }
}

/// A boundary start point waiting for classification: the point, the boundary curve
/// point and the index of the other surface (`sDegeneratePoints`, `s3DPoints`,
/// `sSurfIndex`), and the boundary curve itself (not upstream; for
/// [`SurfaceIntersector::add_deferred_touching_points`]).
struct DegeneratePoint {
    point: TsectPnt,
    curve_point: Point3,
    surf_index: SrfIndex,
    boundary: Curve,
}

/// A touching point [`SurfaceIntersector::find_boundary_start_points`] left out because
/// a start point lies on it. Upstream drops it for good, on the premise that the curve
/// traced from that start point covers it;
/// [`SurfaceIntersector::add_deferred_touching_points`] checks that premise once every
/// curve is found.
#[derive(Debug, Clone)]
pub(crate) struct DeferredTouching {
    /// The touching point: its 3D point (the average of the boundary curve point and
    /// the other surface's point) and the intersection point it came from.
    point: Point3,
    tsp: TsectPnt,
    /// The boundary curve the point lies on.
    boundary: Curve,
}

#[cfg(test)]
impl DeferredTouching {
    /// A deferred touching point at `point` from `tsp`, on the boundary curve
    /// `boundary`.
    pub(crate) fn new(point: Point3, tsp: TsectPnt, boundary: Curve) -> Self {
        Self {
            point,
            tsp,
            boundary,
        }
    }
}

/// Appends the touching curve of `tsp` to `out`: degenerate 3D and UV curves at
/// `point` and the UV points of `tsp`, of type touching, deviated by the distance
/// between `tsp`'s surface points (the degenerate point branch of
/// `FindBoundaryStartPoints`).
pub(crate) fn push_touching_curve(
    out: &mut TsectCurves,
    point: Point3,
    tsp: &TsectPnt,
) -> Result<()> {
    // 3D curve, then the UV curves
    let c3 = BSplineCurve::create_degenerate_curve(3, point)?;
    out.curves_3d.push(Curve::from(c3));
    let c0 = BSplineCurve::create_degenerate_curve(2, Vec3::from(tsp.uv_pos(SrfIndex::First)))?;
    out.uv_curves[0].push(Curve::from(c0));
    let c1 = BSplineCurve::create_degenerate_curve(2, Vec3::from(tsp.uv_pos(SrfIndex::Second)))?;
    out.uv_curves[1].push(Curve::from(c1));

    // deviation and type
    let dist = tsp
        .srf_pos(SrfIndex::First)
        .distance(tsp.srf_pos(SrfIndex::Second));
    out.deviations.push(dist);
    out.curve_types.push(TsectCurveType::Touching);
    Ok(())
}

/// The working lists of [`SurfaceIntersector::find_boundary_start_points`].
#[derive(Default)]
struct BoundaryWork {
    /// `sEdgeCrossings`.
    edge_crossings: Vec<TsectPnt>,
    /// `sDegeneratePoints`, `s3DPoints`, `sSurfIndex`.
    degenerate: Vec<DegeneratePoint>,
}

impl SurfaceIntersector<'_> {
    // ---- boundary start points ---------------------------------------------------------

    /// Finds intersection start points on the natural boundaries
    /// (`FindBoundaryStartPoints`): for each surface whose edges are used
    /// (`use_surface_edges`), each non-degenerate boundary iso curve of its domain
    /// (minimum u, minimum v, maximum u, maximum v, within `this_approx_tol_3d / 3` of
    /// the surface) whose box, expanded by `this_approx_tol_3d`, meets
    /// `intersection_box` is intersected with the other surface.
    ///
    /// A point solution (or a range shorter than a hundredth of the boundary, refined
    /// by a local curve solve) off the surface singularities is evaluated; a
    /// degenerate one (touching, [`SurfaceIntersector::classify_boundary_start_point_intersection`])
    /// becomes a degenerate output curve of type touching unless it is on an output
    /// curve or a start point (one on a start point is kept aside for
    /// [`SurfaceIntersector::add_deferred_touching_points`]); else, unless it is on an output curve, it becomes a
    /// start point (crossings on the other surface's boundary last). A longer range
    /// whose end deviations differ by less than a factor of 100 and whose middle is on
    /// no output curve becomes an output curve: the boundary curve trimmed to it and
    /// dropped onto both surfaces, kept when within `this_approx_tol_3d` of both, with
    /// its type; start points and crossings on it are then dropped.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` for a surface without a surface cache; those of the iso
    /// curves, the bounding boxes, the evaluations, the trims and the curve solves;
    /// [`SolidError::Unported`] from unported callees.
    pub fn find_boundary_start_points(
        &mut self,
        k: &Kernel,
        intersection_box: &Extent3d,
        out: &mut TsectCurves,
    ) -> Result<()> {
        // get surface caches (NER: no cache is SM_ERR_NULL_POINTER)
        let get = |i: SrfIndex| match surface_cache::<SrfIndex>(k, self.surface(i), Some(i)) {
            Ok(c) => Ok(c),
            Err(e) if e.is_unported_or_cancelled() => Err(e),
            Err(e) => Err(
                Failure::new(OP_FBSP, IntersectFailure::SurfaceCacheNotBuilt)
                    .caused_by(e)
                    .into_error(),
            ),
        };
        let mut caches = vec![get(SrfIndex::First)?];
        if !self.doing_self_intersection {
            caches.push(get(SrfIndex::Second)?);
        }

        self.find_boundary_start_points_with_caches(k, &mut caches, intersection_box, out)
    }

    /// [`SurfaceIntersector::find_boundary_start_points`] with the given surface
    /// caches: `caches[0]` of the first surface, `caches[1]` (or `caches[0]` when only
    /// one is given, as for a self intersection) of the second. Their iso boundary
    /// curves are used, and the boundary curve solves on them run with boundary curve
    /// processing on (upstream sets the caches' flag for the call,
    /// `SmTemporaryChangeValue`); the caches keep their flags.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without a cache; else as
    /// [`SurfaceIntersector::find_boundary_start_points`].
    pub fn find_boundary_start_points_with_caches<O: Copy + PartialEq>(
        &mut self,
        k: &Kernel,
        caches: &mut [SurfaceCache<O>],
        intersection_box: &Extent3d,
        out: &mut TsectCurves,
    ) -> Result<()> {
        self.find_boundary_start_points_with_queries(k, caches, intersection_box, out, None)
    }

    /// [`SurfaceIntersector::find_boundary_start_points_with_caches`] with the face
    /// classifications of the caller's context for trimmed caches: the boundary
    /// curve intersections' point solves test their fast-drop solutions against the
    /// face of a trimmed cache with point testing on (`GlobalPointSolve` reaching
    /// `pFace->PointClassify`).
    ///
    /// # Errors
    ///
    /// As [`SurfaceIntersector::find_boundary_start_points_with_caches`], and those of
    /// the classifications.
    pub fn find_boundary_start_points_with_queries<O: Copy + PartialEq>(
        &mut self,
        k: &Kernel,
        caches: &mut [SurfaceCache<O>],
        intersection_box: &Extent3d,
        out: &mut TsectCurves,
        queries: Option<&dyn FaceQueries<O>>,
    ) -> Result<()> {
        // Turn on boundary curve processing to force LocalSolve to look for drop
        // points on boundary curves: the solves run with it on, each cache's point
        // testing as it is (the caches keep their flags).
        // (without a cache the body fails before any solve: its modes are not read)
        let boundary_on = |c: &SurfaceCache<O>| SolveModes {
            process_boundary_curves: true,
            ..c.solve_modes()
        };
        let first = caches.first().map_or(
            SolveModes {
                point_test_enabled: false,
                process_boundary_curves: true,
            },
            boundary_on,
        );
        let second = caches.get(1).map_or(first, boundary_on);
        self.find_boundary_start_points_under(
            k,
            caches,
            [first, second],
            intersection_box,
            out,
            queries,
        )
    }

    /// [`SurfaceIntersector::find_boundary_start_points_with_queries`] whose solves
    /// on `caches[i]` run in `modes[i]` (indexed as the caches are), not in the
    /// caches' own modes; the caches keep their flags.
    ///
    /// # Errors
    ///
    /// As [`SurfaceIntersector::find_boundary_start_points_with_queries`].
    #[allow(clippy::too_many_lines)]
    pub(crate) fn find_boundary_start_points_under<O: Copy + PartialEq>(
        &mut self,
        k: &Kernel,
        caches: &mut [SurfaceCache<O>],
        modes: [SolveModes; 2],
        intersection_box: &Extent3d,
        out: &mut TsectCurves,
        queries: Option<&dyn FaceQueries<O>>,
    ) -> Result<()> {
        if caches.is_empty() {
            return Err(SolidError::intersect(
                OP_FBSP,
                IntersectFailure::NoSurfaceCaches,
            ));
        }
        let mut work = BoundaryWork::default();
        self.deferred_touching.clear();
        // sIntervals keeps its values from surface to surface (SetMinMax failures)
        let mut intervals = [Extent1d::new(); 4];

        // for both surfaces - gather and intersect surf NaturalBoundaryCurves with
        // the other surface
        for l_srf in 0..2_usize {
            let this = srf(l_srf);
            let other = srf(1 - l_srf);

            // skip surfaces marked as not using Surface edges
            if !*at(&self.use_surface_edges, this) {
                continue;
            }

            let domain = *at(&self.uv_domains, this);
            let other_domain = *at(&self.uv_domains, other);

            // Get thisSurface->3DBoundaryCurves: [0] min u, [1] min v, [2] max u,
            // [3] max v
            let cache = cache_mut(caches, l_srf)?;
            let iso = cache.iso_boundary_curves(k, &domain, self.this_approx_tol_3d / 3.0)?;

            // add the intervals of the iso curves (statuses ignored upstream, passed on
            // as the fork does, bcc6f01)
            for (ii, ivl) in intervals.iter_mut().enumerate() {
                if ii % 2 == 1 {
                    ivl.set_min_max(domain.min().x, domain.max().x)?;
                } else {
                    ivl.set_min_max(domain.min().y, domain.max().y)?;
                }
            }

            // for every TargetCurve - intersect ThisSurface->BoundaryCurves with
            // OtherSurface
            for (l_crv, tgt) in iso.curves.iter().enumerate() {
                let norm_param = if l_crv < 2 { 0.0 } else { 1.0 };
                let l_coord = l_crv % 2;
                let l_side = usize::from(l_crv >= 2);
                // the boundary curve's own interval within the domain's: its trim to the
                // domain may end it at a knot just inside (a short end span kept or removed),
                // and a solve over the domain's interval then finds ends outside the curve
                let Ok(bsc_ivl) = intervals
                    .get(l_crv)
                    .copied()
                    .unwrap_or_default()
                    .intersect(&tgt.natural_interval())
                else {
                    continue;
                };

                // skip degenerate TargetCurves
                if tgt.is_degenerate(k, EFF_ZERO, Some(&bsc_ivl))? {
                    continue;
                }

                // BoundaryCurve BBox + Tol
                let boxes = tgt.calculate_bounding_box(
                    k,
                    &bsc_ivl,
                    BoundingBoxRequest {
                        normal: true,
                        pseudo: false,
                        polar: false,
                    },
                    true,
                )?;
                let mut bbox = boxes.normal.unwrap_or_default();
                bbox.expand_absolute(self.this_approx_tol_3d);

                // when the intersection box meets the boundary curve box, intersect
                // OtherSurface with ThisSurface->BoundaryCurve
                let tgt_curve = tgt.clone();
                let solutions = if bbox.are_disjoint(intersection_box, 0.0) {
                    Vec::new()
                } else {
                    // on OtherSurface's cache: upstream's GlobalCurveIntersect fetches
                    // that same cache (smsurf_GetSurfaceCache) and solves on its tree
                    let other_index = (1 - l_srf).min(caches.len() - 1);
                    let other_cache = cache_ref(caches, other_index)?;
                    let [first_modes, second_modes] = modes;
                    let other_modes = if other_index == 0 {
                        first_modes
                    } else {
                        second_modes
                    };
                    surface_global_curve_intersect(
                        k,
                        self.surface(other),
                        other_cache,
                        other_modes,
                        &other_domain,
                        &tgt_curve,
                        &bsc_ivl,
                        self.this_approx_tol_3d,
                        queries,
                    )?
                };

                // for every intersection solution
                for sol in &solutions {
                    let mut sol = *sol;
                    let iso_uv = domain.evaluate(norm_param, norm_param);
                    // the uv point on this surface of curve parameter t
                    let this_uv = |t: f64| {
                        if l_coord == 0 {
                            Point2::new(iso_uv.x, t)
                        } else {
                            Point2::new(t, iso_uv.y)
                        }
                    };

                    // a point for this solution (from the block manager upstream)
                    let mut tsp = TsectPnt::zeroed();
                    tsp.point_type = IntersectionPointType::Unknown;

                    // For single intersection points
                    if sol.solution_type == SolutionType::SingleValue {
                        let mut uvs = [Point2::new(0.0, 0.0); 2];
                        *at_mut(&mut uvs, other) =
                            Point2::new(par(&sol.start, 1), par(&sol.start, 2));
                        *at_mut(&mut uvs, this) = this_uv(par(&sol.start, 0));

                        // skip start points which are on surface singularities
                        if surface_is_singularity(k, self.surface(this), *at(&uvs, this))?
                            || surface_is_singularity(k, self.surface(other), *at(&uvs, other))?
                        {
                            continue;
                        }

                        // Compute xsect point values for the mated UVPoints (a start
                        // point: no previous point)
                        if ignored(self.compute_point_values(k, uvs, &mut tsp, None, None))?
                            .is_none()
                        {
                            continue;
                        }

                        self.use_boundary_point(
                            k,
                            tgt,
                            &sol,
                            uvs,
                            tsp,
                            other,
                            &other_domain,
                            out,
                            &mut work,
                        )?;
                    }
                    // For interval intersections
                    else if sol.solution_type == SolutionType::RangeOfValues {
                        let size = par(&sol.end, 0) - par(&sol.start, 0);
                        let ivl_size = bsc_ivl.max() - bsc_ivl.min();

                        if size > ivl_size / 100.0 {
                            // Large intersection: a coincident boundary curve
                            if sol.end.solution_value < EFF_ZERO {
                                sol.end.solution_value = EFF_ZERO;
                            }
                            if sol.start.solution_value < EFF_ZERO {
                                sol.start.solution_value = EFF_ZERO;
                            }
                            let size_ratio = sol.start.solution_value / sol.end.solution_value;

                            // Skip curves whose deviation changes too much
                            if size_ratio > 100.00 || size_ratio < 0.01 {
                                continue;
                            }

                            // skip curves that are already found
                            let test_param = (par(&sol.start, 0) + par(&sol.end, 0)) / 2.0;
                            let test_pnt = tgt.evaluate_point(k, test_param)?;
                            if self.is_point_on_curve(k, test_pnt, &out.curves_3d, None)? {
                                continue;
                            }

                            self.add_boundary_curve(k, caches, tgt, &sol, l_side, out, &mut work)?;
                        } else {
                            // Small intersection - just use as start points
                            let mut t = par(&sol.start, 0);
                            let mut uv = Point2::new(par(&sol.start, 1), par(&sol.start, 2));
                            let mut uv_ivl2 =
                                Extent1d::from_min_max(par(&sol.start, 0), par(&sol.end, 0));
                            uv_ivl2.expand_absolute(ivl_size * EFF_ZERO_SQRT);
                            // (status ignored upstream, passed on as the fork does)
                            uv_ivl2 = uv_ivl2.intersect(&tgt.natural_interval())?;

                            // use the better end unless the start is on a boundary
                            if sol.start.solution_value > sol.end.solution_value
                                && !other_domain.is_point_on_boundary(uv, EFF_ZERO).0
                            {
                                sol.start = sol.end;
                            }

                            // refine the point
                            let found = self.surface(other).local_curve_solve(
                                k,
                                &other_domain,
                                &tgt_curve,
                                &uv_ivl2,
                                SolverOperation::Normalize,
                                self.this_approx_tol_3d,
                                &[],
                                uv,
                                t,
                            )?;
                            if let Some(s) = found {
                                if s.distance < sol.start.solution_value {
                                    sol.start.solution_value = s.distance;
                                    if let Some(p) = sol.start.parameters.get_mut(0) {
                                        *p = s.t;
                                    }
                                    if let Some(p) = sol.start.parameters.get_mut(1) {
                                        *p = s.uv.x;
                                    }
                                    if let Some(p) = sol.start.parameters.get_mut(2) {
                                        *p = s.uv.y;
                                    }
                                }
                            }
                            t = par(&sol.start, 0);
                            uv = Point2::new(par(&sol.start, 1), par(&sol.start, 2));

                            let mut uvs = [Point2::new(0.0, 0.0); 2];
                            *at_mut(&mut uvs, this) = this_uv(t);
                            *at_mut(&mut uvs, other) = uv;

                            // skip start points on surface singularities
                            if surface_is_singularity(k, self.surface(SrfIndex::First), uvs[0])?
                                || surface_is_singularity(
                                    k,
                                    self.surface(SrfIndex::Second),
                                    uvs[1],
                                )?
                            {
                                continue;
                            }

                            if ignored(self.compute_point_values(k, uvs, &mut tsp, None, None))?
                                .is_some()
                            {
                                self.use_boundary_point(
                                    k,
                                    tgt,
                                    &sol,
                                    uvs,
                                    tsp,
                                    other,
                                    &other_domain,
                                    out,
                                    &mut work,
                                )?;
                            }
                        }
                    }
                }
            }
        }

        // Add the surface-edge crossings last (statuses ignored)
        for p in core::mem::take(&mut work.edge_crossings) {
            let id = self.alloc_point(p);
            self.add_start_point(k, id)?;
        }

        // make degenerate curves of the touching points
        for d in &work.degenerate {
            let tsp = &d.point;
            let degenerate_point = (d.curve_point + tsp.srf_pos(d.surf_index)) / 2.0;

            // skip points that are on a curve already
            if self.is_point_on_curve(k, degenerate_point, &out.curves_3d, None)? {
                continue;
            }
            // and, for now, points on a start point: the curve traced from it should
            // cover them, which DoIntersection checks once all curves are found
            let starts = self.start_points().to_vec();
            if self.is_point_on_start_point(tsp, &starts, None)? {
                self.deferred_touching.push(DeferredTouching {
                    point: degenerate_point,
                    tsp: tsp.clone(),
                    boundary: d.boundary.clone(),
                });
                continue;
            }

            push_touching_curve(out, degenerate_point, tsp)?;
        }
        Ok(())
    }

    /// Adds the touching points [`SurfaceIntersector::find_boundary_start_points`]
    /// left out for a start point on them (in the order it found them) whose two
    /// surface points are closer than `this_approx_tol_3d` and which are on none of
    /// the curves `out` holds; the list is emptied. `DoIntersection` calls it last,
    /// on the curves left once it has removed degenerate curves that lie on others, so
    /// a removed curve covers nothing.
    ///
    /// Upstream drops a touching point on a start point for good, as the curve traced
    /// from that start point is to cover it ("due to seams"). A start point that traces
    /// nothing, or a curve that leaves the point behind, then loses the only report of
    /// that contact: two plane patches 9.9 apart at their corners, with a tolerance of
    /// 2785, gave no curve at all. A point whose surface points are not within the
    /// tolerance is no contact and stays dropped.
    ///
    /// A crossing curve (type crossing or near tangent) through a contact that runs along
    /// the boundary curve the contact lies on ([`SurfaceIntersector::runs_along_boundary`])
    /// is that contact (not upstream; #11749): it is removed and the touching point takes
    /// its place. The boundary curve's own solve with the other surface gave a point
    /// there (a single solution, or a range shorter than a hundredth of the boundary
    /// refined to one), and the curve never leaves the tolerance of that boundary: an
    /// edge on it would cut from the face a sliver along its boundary edge narrower than
    /// the tolerance. Such a curve is traced when the start point on the contact lands
    /// just inside the domain (1.4e-12 inside a half pipe's cap, against 3e-15 outside
    /// it at another length of the same cut), and the boolean kept or dropped a whole
    /// corner of the cut with it. Tangent, coincident and region boundary curves stay.
    ///
    /// # Errors
    ///
    /// Those of [`SurfaceIntersector::is_point_on_curve`],
    /// [`SurfaceIntersector::runs_along_boundary`] and of the degenerate curves.
    pub(crate) fn add_deferred_touching_points(
        &mut self,
        k: &Kernel,
        out: &mut TsectCurves,
    ) -> Result<()> {
        for d in core::mem::take(&mut self.deferred_touching) {
            // the curve's deviation; NaN is no contact
            let gap = d
                .tsp
                .srf_pos(SrfIndex::First)
                .distance(d.tsp.srf_pos(SrfIndex::Second));
            let contact = gap < self.this_approx_tol_3d;
            if !contact {
                continue;
            }
            // the crossing curves through the point that run along its boundary curve
            // are the point
            let mut along = Vec::new();
            for (ii, c) in out.curves_3d.iter().enumerate() {
                let crossing = matches!(
                    out.curve_types.get(ii),
                    Some(TsectCurveType::Crossing | TsectCurveType::NearTangent)
                );
                if crossing
                    && self.is_point_on_curve(k, d.point, core::slice::from_ref(c), None)?
                    && self.runs_along_boundary(k, c, &d.boundary)?
                {
                    along.push(ii);
                }
            }
            for &ii in along.iter().rev() {
                remove_curve(out, ii);
            }
            if self.is_point_on_curve(k, d.point, &out.curves_3d, None)? {
                continue;
            }
            push_touching_curve(out, d.point, &d.tsp)?;
        }
        Ok(())
    }

    /// True when `curve` is proved to stay within `this_approx_tol_3d` of `boundary` over
    /// its whole length, decided on control points, never at sample points
    /// (`docs/definitions-and-certificates.md` §7.6). A rational Bézier piece with positive
    /// weights lies in the convex hull of its control points (Farin), and the distance to a
    /// segment is convex, so a piece of the curve whose control points are within `d` of a
    /// chord `[a, b]` stays within `d` of it. A piece of the boundary from `a` to `b` whose
    /// control points are within `e` of the line through them passes within `e` of each
    /// point of the chord (its foot on the line runs from `a` to `b`). Where the computed
    /// points are within `r` of the exact ones ([`pieces_rounding`]), the boundary piece is
    /// within `e + r` of the line and its exact ends within `r` of `a` and `b`, so within
    /// `e + 2 r` of each point of the chord. Each piece of the curve needs one piece of the
    /// boundary with `d + e`, the curve's `r` and twice the boundary's below the tolerance,
    /// the sum rounded up by `gamma(4)`. False otherwise: also for a curve or boundary
    /// [`bezier_pieces`] does not cut (another kind, a weight not positive, a failed
    /// decomposition) or whose rounding is not bounded. A boundary piece whose ends coincide
    /// gives no chord, and one that bends away from its chord by the tolerance gives none a
    /// curve piece is near, so a curve along an arc of the boundary is kept.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] and a cancellation.
    fn runs_along_boundary(&self, k: &Kernel, curve: &Curve, boundary: &Curve) -> Result<bool> {
        match self.hulls_near_boundary(k, curve, boundary) {
            Ok(along) => Ok(along),
            Err(e) if e.is_unported_or_cancelled() => Err(e),
            Err(_) => Ok(false),
        }
    }

    /// The test of [`SurfaceIntersector::runs_along_boundary`], with every failure an
    /// error.
    fn hulls_near_boundary(&self, k: &Kernel, curve: &Curve, boundary: &Curve) -> Result<bool> {
        let tol = self.this_approx_tol_3d;
        let (Some(pieces), Some(along)) = (bezier_pieces(k, curve)?, bezier_pieces(k, boundary)?)
        else {
            return Ok(false);
        };
        let (Some(r_curve), Some(r_boundary)) =
            (pieces_rounding(k, curve)?, pieces_rounding(k, boundary)?)
        else {
            return Ok(false);
        };
        // each boundary piece's chord and how far the piece strays from it, its rounding
        // counted twice (its points off the line, its ends off the chord's)
        let mut chords = Vec::new();
        for piece in &along {
            k.check_cancelled(OP_ALONG)?;
            let Some(points) = euclidean(piece) else {
                return Ok(false);
            };
            let (Some(&a), Some(&b)) = (points.first(), points.last()) else {
                continue;
            };
            let stray = points
                .iter()
                .map(|&q| distance_to_chord(q, a, b, false))
                .try_fold(0.0f64, |m, d| d.map(|d| m.max(d)));
            if let Some(stray) = stray {
                chords.push((a, b, stray + 2.0 * r_boundary));
            }
        }
        for piece in &pieces {
            let Some(points) = euclidean(piece) else {
                return Ok(false);
            };
            let mut near = false;
            for &(a, b, stray) in &chords {
                k.check_cancelled(OP_ALONG)?;
                let far = points
                    .iter()
                    .map(|&p| distance_to_chord(p, a, b, true))
                    .try_fold(0.0f64, |m, d| d.map(|d| m.max(d)));
                if far.is_some_and(|far| (far + r_curve + stray) * (1.0 + gamma(4)) < tol) {
                    near = true;
                    break;
                }
            }
            if !near {
                return Ok(false);
            }
        }
        Ok(!pieces.is_empty())
    }

    /// The tail of a boundary point solution (single value or small range): the
    /// degenerate classification and the start point lists.
    #[allow(clippy::too_many_arguments)]
    fn use_boundary_point(
        &mut self,
        k: &Kernel,
        tgt: &Curve,
        sol: &Solution<()>,
        uvs: [Point2; 2],
        mut tsp: TsectPnt,
        other: SrfIndex,
        other_domain: &Extent2d,
        out: &TsectCurves,
        work: &mut BoundaryWork,
    ) -> Result<()> {
        // boundary curve point (status ignored upstream, an uninitialised point;
        // passed on as the fork does, bcc6f01)
        let pt = tgt
            .evaluate(k, par(&sol.start, 0), 1, true, true)?
            .first()
            .copied()
            .unwrap_or(Vec3::UNDEF);

        // Set bIsDegenerate = TRUE: a degenerate point xsect curve (status ignored
        // upstream, passed on as the fork does)
        let is_degenerate = self.classify_boundary_start_point_intersection(k, &tsp)?;

        // when a degenerate point not on an output curve yet: classify it at the end
        if is_degenerate && !self.is_point_on_curve(k, tsp.crv_pos(), &out.curves_3d, None)? {
            work.degenerate.push(DegeneratePoint {
                point: tsp,
                curve_point: pt,
                surf_index: other,
                boundary: tgt.clone(),
            });
            return Ok(());
        }

        // when the point is not on an output curve already
        if !self.is_point_on_curve(k, tsp.crv_pos(), &out.curves_3d, None)? {
            tsp.deviation = sol.start.solution_value;
            if other_domain
                .is_point_on_boundary(*at(&uvs, other), EFF_ZERO * 100.0)
                .0
            {
                // a crossing on the other surface's edge: added last
                work.edge_crossings.push(tsp);
            } else {
                // add the point to the list of start points (status ignored upstream,
                // passed on as the fork does)
                let id = self.alloc_point(tsp);
                self.add_start_point(k, id)?;
            }
        }
        Ok(())
    }

    /// A long range solution of a boundary curve: the curve trimmed to it, dropped
    /// onto both surfaces, becomes an output curve when close to both.
    #[allow(clippy::too_many_arguments)]
    fn add_boundary_curve<O: Copy + PartialEq>(
        &mut self,
        k: &Kernel,
        caches: &[SurfaceCache<O>],
        tgt: &Curve,
        sol: &Solution<()>,
        l_side: usize,
        out: &mut TsectCurves,
        work: &mut BoundaryWork,
    ) -> Result<()> {
        let tol = self.this_approx_tol_3d;

        // copy and trim the boundary curve (may snap the interval to knots)
        // (`new SmBSplineCurve(*pTgtBSC)`: a plain B-spline copy)
        let mut result = crate::surface::surface_cache::p1::copy_as_bspline(tgt)?;
        let mut trim_ivl = Extent1d::from_min_max(par(&sol.start, 0), par(&sol.end, 0));
        result.trim(k, &mut trim_ivl, TrimMode::Notify)?;
        out.curves_3d.push(result);

        // Get UVCurves on surface 1
        let drop1 = {
            let c = last_curve(out)?;
            let ivl = c.natural_interval();
            surface_drop_and_trim_curve(
                k,
                self.surface(SrfIndex::First),
                caches.first(),
                &self.uv_domains[0],
                c,
                &ivl,
                tol,
            )?
        };
        match drop1.uv_curves.len() {
            2 => out.uv_curves[0].extend(drop1.uv_curves.get(l_side).cloned()),
            1 => out.uv_curves[0].extend(drop1.uv_curves.first().cloned()),
            _ => {
                out.curves_3d.pop();
                return Ok(());
            }
        }

        // Trim the 3D curve to the UV curve's interval
        let uv_ivl = out.uv_curves[0]
            .last()
            .map(Curve::natural_interval)
            .unwrap_or_default();
        if !trim_ivl.is_contained_by(&uv_ivl, 0.0) {
            trim_ivl = uv_ivl;
            last_curve(out)?.trim(k, &mut trim_ivl, TrimMode::Notify)?;
        }

        // Get UVCurves on surface 2
        let drop2 = {
            let c = last_curve(out)?;
            let ivl = c.natural_interval();
            surface_drop_and_trim_curve(
                k,
                self.surface(SrfIndex::Second),
                caches.get(1.min(caches.len().saturating_sub(1))),
                &self.uv_domains[1],
                c,
                &ivl,
                tol,
            )?
        };
        match drop2.uv_curves.len() {
            2 => out.uv_curves[1].extend(drop2.uv_curves.get(l_side).cloned()),
            1 => out.uv_curves[1].extend(drop2.uv_curves.first().cloned()),
            _ => {
                out.curves_3d.pop();
                out.uv_curves[0].pop();
                return Ok(());
            }
        }

        // Trim the 3D curve and the first UV curve to the second UV curve's interval
        let uv_ivl2 = out.uv_curves[1]
            .last()
            .map(Curve::natural_interval)
            .unwrap_or_default();
        if !trim_ivl.is_contained_by(&uv_ivl2, 0.0) {
            trim_ivl = uv_ivl2;
            last_curve(out)?.trim(k, &mut trim_ivl, TrimMode::Notify)?;
            if let Some(c) = out.uv_curves[0].last_mut() {
                c.trim(k, &mut trim_ivl, TrimMode::Notify)?;
            }
        }

        if drop1.dist_to_surf > tol || drop2.dist_to_surf > tol {
            // don't use this curve
            out.uv_curves[0].pop();
            out.uv_curves[1].pop();
            out.curves_3d.pop();
            return Ok(());
        }

        // set the curve deviation and type
        out.deviations
            .push(max2(drop1.dist_to_surf, drop2.dist_to_surf));
        let curve_type = match (out.uv_curves[0].last(), out.uv_curves[1].last()) {
            (Some(c0), Some(c1)) => compute_curve_type(
                k,
                self.surface(SrfIndex::First),
                self.surface(SrfIndex::Second),
                c0,
                c1,
            )?,
            _ => return Err(SolidError::invalid_input(OP_FBSP, "no uv curve")),
        };
        if matches!(
            curve_type,
            TsectCurveType::Coincident | TsectCurveType::RegionBoundary
        ) {
            self.check_for_interior_curves = false;
        }
        out.curve_types.push(curve_type);

        // remove start points on the new curve
        for id in self.start_points().to_vec() {
            let p = self.point(id)?.crv_pos();
            if self.is_point_on_curve(k, p, &out.curves_3d, None)? {
                unlink(&mut self.start_points, id);
            }
        }

        // remove edge crossings on the new curve
        let mut kept = Vec::with_capacity(work.edge_crossings.len());
        for p in core::mem::take(&mut work.edge_crossings) {
            if !self.is_point_on_curve(k, p.crv_pos(), &out.curves_3d, None)? {
                kept.push(p);
            }
        }
        work.edge_crossings = kept;
        Ok(())
    }

    // ---- tracing step --------------------------------------------------------------------

    /// Steps from the point `prev` by `step_size` along its UV tangents to the next
    /// intersection point, evaluated into `next` (`ComputeNextPoint`).
    ///
    /// The step is clipped to both domains (`clipped`); a zero step is a boundary
    /// hit. A step clipped short of a pole at a boundary the curve does not reach goes on
    /// to the pole (not upstream, [`SurfaceIntersector::pole_corner_step`]). The guesses
    /// are refined ([`SurfaceIntersector::refine_point`] with
    /// boundary testing, then without when the step collapses below a hundredth),
    /// the step scaled by the refinement's UV step on the first surface. A failed
    /// evaluation is a boundary hit; a tangent or coincident next point, or one behind
    /// `prev` on the first surface's UV tangent line (a singular one becomes a
    /// crossing), is not good. Else the curve parameter advances by the 3D distance
    /// and [`SurfaceIntersector::test_span_accuracy`] decides.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when `prev` is outside a domain (by `SM_EFF_ZERO`); `InvalidInput`
    /// when `next` is `prev` or an id is unknown; those of the span test;
    /// [`SolidError::Unported`] from unported callees.
    pub fn compute_next_point(
        &mut self,
        k: &Kernel,
        prev: TsectId,
        step_size: f64,
        next: TsectId,
    ) -> Result<NextPoint> {
        if prev == next {
            return Err(SolidError::invalid_input(
                OP_NEXT,
                "the next point is the previous point",
            ));
        }
        let prev_tsp = self.point(prev)?.clone();
        self.point(next)?;
        let s0 = SrfIndex::First;
        let s1 = SrfIndex::Second;

        // init output
        let mut res = NextPoint {
            found_good_point: false,
            deviation_found: None,
            angle_found_rad: None,
            clipped: false,
            boundary_hit: false,
        };

        // check state - both surfaces contain rPrevTSP
        if !self.uv_domains[0].contains_point(prev_tsp.uv_pos(s0), EFF_ZERO)
            || !self.uv_domains[1].contains_point(prev_tsp.uv_pos(s1), EFF_ZERO)
        {
            let surface =
                usize::from(self.uv_domains[0].contains_point(prev_tsp.uv_pos(s0), EFF_ZERO));
            return Err(SolidError::intersect(
                OP_NEXT,
                IntersectFailure::PointOutsideDomain { surface },
            ));
        }

        // if needed: clip stepsize to keep nextTSP in both surfaces
        let unclipped_step_size = step_size;
        let mut step_size =
            self.uv_domains[0].clip_line(prev_tsp.uv_pos(s0), prev_tsp.uv_deriv(s0), step_size);
        step_size =
            self.uv_domains[1].clip_line(prev_tsp.uv_pos(s1), prev_tsp.uv_deriv(s1), step_size);

        // remember clipping when it happens
        res.clipped = step_size < unclipped_step_size - EFF_ZERO * (1.0 + unclipped_step_size);

        // when StepSize is clipped to zero - return a boundary hit with no next point
        if is_zero(step_size) {
            res.boundary_hit = true;
            return Ok(res);
        }

        // Set NextStep UVs for Surface0 and Surface1 (this safety clamp should no longer
        // fire since StepSize has already been clipped)
        let guesses = |step: f64| {
            [
                self.uv_domains[0].clamp_point(prev_tsp.uv_pos(s0) + prev_tsp.uv_deriv(s0) * step),
                self.uv_domains[1].clamp_point(prev_tsp.uv_pos(s1) + prev_tsp.uv_deriv(s1) * step),
            ]
        };
        let [mut guess_uv0, mut guess_uv1] = guesses(step_size);

        // the previous point's m_vUVCurvePV, as RefinePoint may read it
        let previous_uv = [
            prev_tsp.uv_pos(s0),
            prev_tsp.uv_deriv(s0),
            prev_tsp.uv_pos(s1),
            prev_tsp.uv_deriv(s1),
        ];

        // Find surf/surf xSectPoint near the guess points (a failure is no answer)
        let refine =
            |guess: [Point2; 2], do_boundary_testing: bool| -> Result<Option<[Point2; 2]>> {
                match self.refine_point(
                    k,
                    guess[0],
                    guess[1],
                    prev_tsp.point_type,
                    Some(prev_tsp.crv_deriv()),
                    Some(&previous_uv),
                    do_boundary_testing,
                ) {
                    Ok(r) => Ok(r),
                    Err(e) if e.is_unported_or_cancelled() => Err(e),
                    Err(_) => Ok(None),
                }
            };
        let Some(mut next_uvs) = refine([guess_uv0, guess_uv1], true)? else {
            return Ok(res);
        };

        // Not upstream: a step clipped at a boundary the curve does not reach, where the
        // line goes on to a degenerate boundary (a pole), steps to the pole instead
        // (`pole_corner_step`)
        if res.clipped {
            if let Some(pole) =
                self.pole_corner_step(k, &prev_tsp, step_size, unclipped_step_size, next_uvs)?
            {
                let corner = guesses(pole.step);
                // taken only when the curve reaches the pole there
                let refined = refine(corner, true)?;
                let reached = match refined {
                    Some(uvs) => self.pole_reached(k, &pole, uvs)?,
                    None => false,
                };
                if let Some(uvs) = refined.filter(|_| reached) {
                    step_size = pole.step;
                    [guess_uv0, guess_uv1] = corner;
                    next_uvs = uvs;
                }
            }
        }

        // compute actual step to guess step
        let guess_step = guess_uv0 - prev_tsp.uv_pos(s0);
        let actual_step = next_uvs[0] - prev_tsp.uv_pos(s0);
        let mut guess_step_size = guess_step.length();
        let mut actual_step_size = actual_step.length();
        if guess_step_size > EFF_ZERO * actual_step_size {
            step_size *= actual_step_size / guess_step_size;
        }

        // Step size may go to zero again so test it again.
        if is_zero(step_size) || actual_step_size < guess_step_size / 100.0 {
            // Before bailing out try local intersection again without boundary
            // testing.
            let Some(uvs) = refine([guess_uv0, guess_uv1], false)? else {
                return Ok(res);
            };
            next_uvs = uvs;

            // update StepSize for this refined point
            let guess_step = guess_uv0 - prev_tsp.uv_pos(s0);
            let actual_step = next_uvs[0] - prev_tsp.uv_pos(s0);
            guess_step_size = guess_step.length();
            actual_step_size = actual_step.length();
            if guess_step_size > EFF_ZERO * actual_step_size {
                step_size *= actual_step_size / guess_step_size;
            }

            // if this 2nd refined point also ends up with a zero stepSize - quit
            if is_zero(step_size) {
                res.boundary_hit = true;
                return Ok(res);
            }
        }

        // set NextTSP values given its surface UV values (a next point: with the
        // previous point)
        let mut next_tsp = self.point(next)?.clone();
        let r =
            self.compute_point_values(k, next_uvs, &mut next_tsp, Some(&prev_tsp), Some(step_size));
        *self.point_mut(next)? = next_tsp.clone();
        if ignored(r)?.is_none() {
            res.boundary_hit = true; // causes termination of current tracing
            return Ok(res);
        }

        // end iteration for tangent and coincident points
        if matches!(
            next_tsp.point_type,
            IntersectionPointType::TangentPoint | IntersectionPointType::Coincidence
        ) {
            return Ok(res);
        }

        // Also make sure that the point is going in the same direction
        let line_param = line_closest_point(
            Vec3::from(prev_tsp.uv_pos(s0)),
            Vec3::from(prev_tsp.uv_deriv(s0)),
            Vec3::from(next_tsp.uv_pos(s0)),
        );
        // when next point lies behind TsectPnt
        if line_param < 0.0 {
            // mark NextTSP as SM_IP_CROSSING and quit
            if next_tsp.point_type == IntersectionPointType::Singularity {
                self.point_mut(next)?.point_type = IntersectionPointType::Crossing;
            }
            return Ok(res);
        }

        // let NextTSP param = TsectPnt.Param + 3DCurve ArcLength
        let dist_moved = next_tsp.crv_pos().distance(prev_tsp.crv_pos());
        self.point_mut(next)?.curve_parameter = prev_tsp.curve_parameter + dist_moved;

        // the angle and deviation tests of the span
        let acc = self.test_span_accuracy(k, prev, next)?;
        res.found_good_point = acc.found_good_point;
        res.deviation_found = acc.deviation_found;
        res.angle_found_rad = Some(acc.angle_found_rad);
        Ok(res)
    }

    /// Whether the refined UV points `uvs` reach `pole`'s pole: on its degenerate boundary
    /// ([`PoleCorner::on_boundary`]) and at the pole itself, the surface's point there within
    /// `this_approx_tol_3d` of the point at the corner the singularity was tested at. The
    /// singularity test reads one corner, and a boundary can be singular there and regular
    /// elsewhere: a refined point on such a boundary away from the corner is not a pole. On a
    /// boundary that collapses whole, every point of it is the pole, whatever its coordinate
    /// along the boundary. A point the surface does not evaluate at is not reached.
    ///
    /// # Errors
    ///
    /// An unported callee or a cancellation in the evaluation.
    fn pole_reached(&self, k: &Kernel, pole: &PoleCorner, uvs: [Point2; 2]) -> Result<bool> {
        if !pole.on_boundary(uvs) {
            return Ok(false);
        }
        match self.surface(pole.srf).evaluate_point(k, pole.uv(uvs)) {
            Ok(at) => Ok((at - pole.pole).length() <= self.this_approx_tol_3d),
            Err(e) if e.is_unported_or_cancelled() => Err(e),
            Err(_) => Ok(false),
        }
    }

    /// Not upstream: the step of [`SurfaceIntersector::compute_next_point`] that reaches a
    /// pole, when the step was clipped at a boundary the curve does not reach on the way.
    ///
    /// Where the intersection curve runs into a pole at a corner of a domain, the corner of
    /// the degenerate boundary and a boundary across it (a sphere's pole on its seam
    /// meridian), the UV tangent line reaches both boundaries at nearly the same step. The
    /// UV curve bends, so the line can meet the other boundary first although the curve
    /// meets it only at the pole. The step is then clipped short of the pole, and its
    /// refined point is not on the boundary it was clipped at: the boundary solve of
    /// `RefinePoint` finds no crossing there. The trace took such a step, then a shorter one
    /// to the pole. `StepBackFromSingularity` finds the pole point's singular parameter and
    /// UV tangent 1 % of the last span away from the pole, and on a span that short the
    /// singular parameter's derivative there is below the precision of the coordinates: the
    /// pole point got a UV tangent of the wrong sign or a singular parameter across the
    /// seam, and the trims made from the curve failed the body check.
    ///
    /// For each surface: when its domain clipped the step at a boundary (the guess is on
    /// it), the refined point is not on that boundary, the UV tangent line reaches the
    /// surface's other boundary within `unclipped_step_size`, and the surface is singular
    /// at that exit, collapsing along that boundary ([`Surface::is_singularity`] at
    /// `SM_EFF_ZERO`), the step to that exit, clipped to the other domain, is returned when
    /// it is longer than `step_size`, with that boundary: the caller takes the step only
    /// when its refined point is on it at the pole ([`SurfaceIntersector::pole_reached`]), so a curve that passes
    /// near the pole without reaching it keeps the clipped step. "On a boundary" is within
    /// `SM_EFF_ZERO` scaled by the domain, as `TraceCurve` scales its UV tolerance.
    ///
    /// # Errors
    ///
    /// Those of the singularity test, and an unported callee or a cancellation in the
    /// evaluation of the pole.
    fn pole_corner_step(
        &self,
        k: &Kernel,
        prev: &TsectPnt,
        step_size: f64,
        unclipped_step_size: f64,
        next_uvs: [Point2; 2],
    ) -> Result<Option<PoleCorner>> {
        let srfs = [SrfIndex::First, SrfIndex::Second];
        for ((s, dom), next) in srfs.into_iter().zip(self.uv_domains).zip(next_uvs) {
            let p = prev.uv_pos(s);
            let d = prev.uv_deriv(s);
            // the parameter along the tangent line where it leaves through each boundary
            let exit = |pc: f64, dc: f64, lo: f64, hi: f64| {
                if dc > 0.0 {
                    ((hi - pc) / dc, hi)
                } else if dc < 0.0 {
                    ((lo - pc) / dc, lo)
                } else {
                    (f64::INFINITY, hi)
                }
            };
            let (exit_u, bound_u) = exit(p.x, d.x, dom.min.x, dom.max.x);
            let (exit_v, bound_v) = exit(p.y, d.y, dom.min.y, dom.max.y);
            let uv_tol = EFF_ZERO * (1.0 + dom.max_dimension());
            let guess = dom.clamp_point(p + d * step_size);
            // the boundary this domain clipped the step at, and the exit through the other
            let (clipped, reached, other_exit, collapse, pole_bound) = if exit_u <= exit_v {
                (
                    (guess.x - bound_u).abs() <= uv_tol,
                    (next.x - bound_u).abs() <= uv_tol,
                    exit_v,
                    SurfParamType::U,
                    bound_v,
                )
            } else {
                (
                    (guess.y - bound_v).abs() <= uv_tol,
                    (next.y - bound_v).abs() <= uv_tol,
                    exit_u,
                    SurfParamType::V,
                    bound_u,
                )
            };
            if !clipped || reached || !(other_exit > step_size && other_exit <= unclipped_step_size)
            {
                continue;
            }
            let corner = dom.clamp_point(p + d * other_exit);
            let sing = self.surface(s).is_singularity(k, corner, EFF_ZERO, false)?;
            if !sing.singular
                || !(sing.direction == collapse || sing.direction == SurfParamType::Both)
            {
                continue;
            }
            // the other domain clips that step as it clipped the first
            let mut corner_step = other_exit;
            for (o, other_dom) in srfs.into_iter().zip(self.uv_domains) {
                if o != s {
                    corner_step =
                        other_dom.clip_line(prev.uv_pos(o), prev.uv_deriv(o), corner_step);
                }
            }
            if corner_step > step_size {
                let pole = match self.surface(s).evaluate_point(k, corner) {
                    Ok(at) => at,
                    Err(e) if e.is_unported_or_cancelled() => return Err(e),
                    Err(_) => continue,
                };
                return Ok(Some(PoleCorner {
                    step: corner_step,
                    srf: s,
                    on_v_boundary: collapse == SurfParamType::U,
                    bound: pole_bound,
                    uv_tol,
                    pole,
                }));
            }
        }
        Ok(None)
    }

    // ---- end-of-trace tests ----------------------------------------------------------------

    /// Trims the traced span from `prev` to `next` to the point `new`
    /// (`sm_UpdateTsectPoint`): `next` takes `new`'s user values and is evaluated at
    /// `new`'s UV points (as a next point of `prev`); its curve parameter is scaled
    /// with the chord length; `new` takes that parameter when its own is negative.
    ///
    /// # Errors
    ///
    /// `InvalidInput` when `next` is `prev` or an id is unknown; those of
    /// [`SurfaceIntersector::compute_point_values`] (`next` keeps what was computed).
    pub fn update_tsect_point(
        &mut self,
        k: &Kernel,
        prev: TsectId,
        next: TsectId,
        new: TsectId,
    ) -> Result<()> {
        update_tsect_point_with(self, k, prev, next, new)
    }

    /// Tests the next traced point `next` against the targets of the trace and the
    /// domain boundaries (`TestPoint`), snapping it
    /// ([`SurfaceIntersector::update_tsect_point`]) to the first target on the span
    /// from the last curve point to `next` ([`is_point_on_tsect_curve`]):
    ///
    /// 1. the first through point: it is removed (the last one sets
    ///    `last_through_parameter` when extension clipping), `clipped_by_through_point`
    ///    is set, and the done flag is left unchanged (`None`);
    /// 2. a start point, newest first (skipped when the last curve point is the start
    ///    of the trace, or is at the start point without crossing a seam): a crossing
    ///    start point with a non-parallel tangent plane is removed; the trace is done
    ///    unless the start point is a crossing on a domain boundary the curve runs
    ///    along;
    /// 3. the start of the trace (a closed curve, at least two spans on);
    /// 4. the end point (unless the span starts the curve at it, a seam).
    ///
    /// Without an end point, extension clipping ends the trace past
    /// `last_through_parameter + extension_distance`. Else the trace is done when
    /// `next` is on a domain boundary with its UV tangent pointing across it; the
    /// start points at `next` are then removed.
    ///
    /// # Errors
    ///
    /// `InvalidInput` when `next` is the last curve point or an id is unknown, or
    /// when the curve list is empty and the last curve point is needed (upstream
    /// dereferences null); those of the snapping; [`SolidError::Unported`] from
    /// unported callees.
    pub fn test_point(&mut self, k: &Kernel, next: TsectId) -> Result<Option<bool>> {
        test_point_with(self, k, next)
    }

    // ---- fillet law spans ------------------------------------------------------------------

    /// Splits the fillet-law span from `curr` to `next` (`SplitSpan`): a new crossing
    /// point at the mean law parameter (`user_doubles[1]`), UV points and curve
    /// parameter, evaluated by [`SurfaceIntersector::compute_law_point_values`]; its
    /// curve parameter becomes `curr`'s plus the chord when the chord exceeds
    /// `SM_ZONE_TOL_3D` times the law parameter step. Returns the new point's id.
    ///
    /// # Errors
    ///
    /// Those of [`SurfaceIntersector::compute_law_point_values`] (the basic
    /// intersector's `ComputePointValuesDelta` always fails); `InvalidInput` for an
    /// unknown id.
    pub fn split_span(
        &mut self,
        k: &Kernel,
        curr: TsectId,
        next: TsectId,
        delta_step: f64,
        law: &LawPointOptions<'_>,
        law_curve: &mut FilletLaw,
    ) -> Result<TsectId> {
        split_span_with(self, k, curr, next, delta_step, law, law_curve)
    }
}

/// [`SurfaceIntersector::split_span`] on the intersector of `v`, the virtual calls of
/// `ComputeLawPointValues` dispatched to `v`.
///
/// # Errors
///
/// As [`SurfaceIntersector::split_span`], with the errors of `v`'s overrides.
pub fn split_span_with<'s, V: super::p4::IntersectorVirtuals<'s> + ?Sized>(
    v: &mut V,
    k: &Kernel,
    curr: TsectId,
    next: TsectId,
    delta_step: f64,
    law: &LawPointOptions<'_>,
    law_curve: &mut FilletLaw,
) -> Result<TsectId> {
    let curr_tsp = v.intersector().point(curr)?.clone();
    let next_tsp = v.intersector().point(next)?.clone();

    let mut new_tsp = TsectPnt::zeroed();
    new_tsp.point_type = IntersectionPointType::Crossing;

    // the law parameters of the span ends and their mean
    let curr_param = curr_tsp.user_doubles[1];
    let next_param = next_tsp.user_doubles[1];
    let mid_param = (curr_param + next_param) / 2.0;
    new_tsp.user_doubles[1] = mid_param;

    let uv0 = (curr_tsp.uv_pos(SrfIndex::First) + next_tsp.uv_pos(SrfIndex::First)) / 2.0;
    let uv1 = (curr_tsp.uv_pos(SrfIndex::Second) + next_tsp.uv_pos(SrfIndex::Second)) / 2.0;

    new_tsp.curve_parameter = (curr_tsp.curve_parameter + next_tsp.curve_parameter) / 2.0;

    super::p2::compute_law_point_values_with(
        v,
        k,
        mid_param,
        super::p2::LawGuess::Given([uv0, uv1]),
        delta_step,
        law,
        law_curve,
        false,
        &mut new_tsp,
    )?;

    let dist = curr_tsp.crv_pos().distance(new_tsp.crv_pos());
    if dist > ZONE_TOL_3D * (next_param - curr_param) {
        new_tsp.curve_parameter = curr_tsp.curve_parameter + dist;
    }
    Ok(v.intersector_mut().alloc_point(new_tsp))
}

/// [`SurfaceIntersector::update_tsect_point`] on the intersector of `v`, its
/// `ComputePointValues` dispatched to `v` (`sm_UpdateTsectPoint` calls the virtual).
///
/// # Errors
///
/// As [`SurfaceIntersector::update_tsect_point`].
pub fn update_tsect_point_with<'s, V: super::p4::IntersectorVirtuals<'s> + ?Sized>(
    v: &mut V,
    k: &Kernel,
    prev: TsectId,
    next: TsectId,
    new: TsectId,
) -> Result<()> {
    if prev == next {
        return Err(SolidError::invalid_input(
            OP_UPDATE,
            "the next point is the previous point",
        ));
    }
    let prev_tsp = v.intersector().point(prev)?.clone();
    let new_tsp = v.intersector().point(new)?.clone();
    let mut next_tsp = v.intersector().point(next)?.clone();

    // get pPrev1/rNextTSP 3DCurve arcLength distance
    let dist = prev_tsp.crv_pos().distance(next_tsp.crv_pos());

    // get parameter Step between pPrev1 and rNextTSP
    let step = next_tsp.curve_parameter - prev_tsp.curve_parameter;

    // get NewTSP UV values
    let uvs = [
        new_tsp.uv_pos(SrfIndex::First),
        new_tsp.uv_pos(SrfIndex::Second),
    ];

    // copy rNextTSP user values from pNewTSP
    next_tsp.user_doubles = new_tsp.user_doubles;
    next_tsp.user_pointer.clone_from(&new_tsp.user_pointer);

    // ComputePointValues with pNewTSP UV values and store results in rNextTSP
    let r = v.compute_point_values(k, uvs, &mut next_tsp, Some(&prev_tsp), None);
    *v.intersector_mut().point_mut(next)? = next_tsp.clone();
    r?;

    // set parameter step size based on current curve parameterization
    let new_dist = prev_tsp.crv_pos().distance(next_tsp.crv_pos());
    let new_step = new_dist / dist * step;
    let next_param = prev_tsp.curve_parameter + new_step;
    v.intersector_mut().point_mut(next)?.curve_parameter = next_param;

    // set pNewTSP parameter value so that it knows if/where it was used
    let p = v.intersector_mut().point_mut(new)?;
    if p.curve_parameter < 0.0 {
        p.curve_parameter = next_param;
    }
    Ok(())
}

/// [`SurfaceIntersector::test_point`] on the intersector of `v`, the snapping
/// ([`update_tsect_point_with`]) dispatched to `v`.
///
/// # Errors
///
/// As [`SurfaceIntersector::test_point`].
#[allow(clippy::too_many_lines)]
pub fn test_point_with<'s, V: super::p4::IntersectorVirtuals<'s> + ?Sized>(
    v: &mut V,
    k: &Kernel,
    next: TsectId,
) -> Result<Option<bool>> {
    v.intersector().point(next)?;
    let prev1 = v.intersector().curve_points.last().copied();
    if prev1 == Some(next) {
        return Err(SolidError::invalid_input(
            OP_TEST,
            "the next point is the last curve point",
        ));
    }
    let need_prev = || prev1.ok_or_else(|| SolidError::invalid_input(OP_TEST, "no curve point"));
    let tol = v.intersector().this_approx_tol_3d;

    // If we are given through points check the first one: they are in order
    if let Some(&through) = v.intersector().through_points.first() {
        let p1 = need_prev()?;

        // if through point is on hermite curve from pPrev1 point to rNextTSP point
        if is_point_on_tsect_curve(
            k,
            v.intersector().point(through)?,
            v.intersector().point(p1)?,
            v.intersector().point(next)?,
            tol,
        )? {
            // modify the rNextTSP to correspond to the through point
            update_tsect_point_with(v, k, p1, next, through)?;

            // remove the through point from the list - we're done with it
            unlink(&mut v.intersector_mut().through_points, through);

            // mark the last throughParameter distance when extension clipping
            if v.intersector().doing_extension_clipping && v.intersector().through_points.is_empty()
            {
                let param = v.intersector().point(through)?.curve_parameter;
                v.intersector_mut().last_through_parameter = param;
            }

            // remember that a through point was hit
            v.intersector_mut().clipped_by_through_point = true;
            return Ok(None);
        }
    }

    // Check against Start Points, the better ones (at the end) first.
    let starts = v.intersector().start_points.clone();
    for &start in starts.iter().rev() {
        // Skip pPrev1 if it's the same as m_pStartPoint.
        if prev1 == v.intersector().start_point {
            continue;
        }
        let p1 = need_prev()?;

        // Also skip it if it's at the same place as pStartTSP, unless a seam
        {
            let prev_tsp = v.intersector().point(p1)?;
            let start_tsp = v.intersector().point(start)?;
            let dist = prev_tsp.crv_pos().distance(start_tsp.crv_pos());
            if dist < tol {
                let dom1_size = v.intersector().uv_domains[0].size().length();
                let dom2_size = v.intersector().uv_domains[1].size().length();
                let dist1 = prev_tsp
                    .uv_pos(SrfIndex::First)
                    .distance(start_tsp.uv_pos(SrfIndex::First));
                let dist2 = prev_tsp
                    .uv_pos(SrfIndex::Second)
                    .distance(start_tsp.uv_pos(SrfIndex::Second));
                if dist1 < dom1_size / 2.0 && dist2 < dom2_size / 2.0 {
                    continue;
                }
            }
        }

        // if StartTsp is on xsect curve segment
        if is_point_on_tsect_curve(
            k,
            v.intersector().point(start)?,
            v.intersector().point(p1)?,
            v.intersector().point(next)?,
            tol,
        )? {
            // Modify the rNextTSP to correspond to that start point
            update_tsect_point_with(v, k, p1, next, start)?;

            // Remove the start point we just crossed (not singular or tangent ones)
            let next_type = v.intersector().point(next)?.point_type;
            let start_tsp = v.intersector().point(start)?.clone();
            if next_type == IntersectionPointType::Crossing
                && start_tsp.tangent_plane_angle_rad > v.intersector().stepping_parallel_tol_radians
            {
                unlink(&mut v.intersector_mut().start_points, start);
            }

            // a crossing start point on a boundary the curve runs along is a
            // through point
            let done = if start_tsp.point_type == IntersectionPointType::Crossing {
                let along = |on: ExtentPointType, across: ExtentPointType, d: f64| {
                    (on == ExtentPointType::Start || on == ExtentPointType::End)
                        && across == ExtentPointType::Inside
                        && is_zero(d)
                };
                let u0 = along(
                    start_tsp.u_param_type(SrfIndex::First),
                    start_tsp.v_param_type(SrfIndex::First),
                    start_tsp.uv_deriv(SrfIndex::First).x,
                );
                let u1 = along(
                    start_tsp.u_param_type(SrfIndex::Second),
                    start_tsp.v_param_type(SrfIndex::Second),
                    start_tsp.uv_deriv(SrfIndex::Second).x,
                );
                let v0 = along(
                    start_tsp.v_param_type(SrfIndex::First),
                    start_tsp.u_param_type(SrfIndex::First),
                    start_tsp.uv_deriv(SrfIndex::First).y,
                );
                let v1 = along(
                    start_tsp.v_param_type(SrfIndex::Second),
                    start_tsp.u_param_type(SrfIndex::Second),
                    start_tsp.uv_deriv(SrfIndex::Second).y,
                );
                !(u0 || u1 || v0 || v1)
            } else {
                true
            };
            return Ok(Some(done));
        }
    }

    // If we have a start point check for a closed curve situation
    if let Some(sp) = v.intersector().start_point {
        // Don't check if we are only starting the curve
        if prev1 != Some(sp) {
            let p1 = need_prev()?;
            // the node before pPrev1 (circular list: itself when alone)
            let n = v.intersector().curve_points.len();
            let prev2 = v
                .intersector()
                .curve_points
                .get(n.saturating_sub(2))
                .copied();
            if prev2 != Some(sp)
                && is_point_on_tsect_curve(
                    k,
                    v.intersector().point(sp)?,
                    v.intersector().point(p1)?,
                    v.intersector().point(next)?,
                    tol,
                )?
            {
                // Have found loop around
                update_tsect_point_with(v, k, p1, next, sp)?;
                v.intersector_mut().curve_is_closed = true;
                return Ok(Some(true));
            }
        }
    }

    // If we are given an ending point - check it
    if let Some(ep) = v.intersector().end_point {
        let p1 = need_prev()?;
        if is_point_on_tsect_curve(
            k,
            v.intersector().point(ep)?,
            v.intersector().point(p1)?,
            v.intersector().point(next)?,
            tol,
        )? {
            let mut on_seam = false;
            if v.intersector().curve_points.first() == Some(&p1) {
                // a single span: the end point at its start is a seam
                let dist1 = v
                    .intersector()
                    .point(p1)?
                    .crv_pos()
                    .distance(v.intersector().point(ep)?.crv_pos());
                if dist1 < tol {
                    on_seam = true;
                }
            }
            if !on_seam {
                update_tsect_point_with(v, k, p1, next, ep)?;
                return Ok(Some(true));
            }
        }
    } else if v.intersector().doing_extension_clipping
        && v.intersector().last_through_parameter != 0.0
        && v.intersector().point(next)?.curve_parameter
            > v.intersector().last_through_parameter + v.intersector().extension_distance
    {
        return Ok(Some(true));
    }

    // Not upstream (#2500): an extension stops where it comes back onto the traced
    // curve, in 3D, once it has left that point
    if let Some(closure) = v.intersector().extension_closes_at {
        let p1 = need_prev()?;
        let on_span = super::p1::is_point_on_tsect_span_3d(
            k,
            closure,
            v.intersector().point(p1)?,
            v.intersector().point(next)?,
            tol,
        )?;
        if v.intersector().extension_left_closure {
            if on_span {
                v.intersector_mut().extension_closed = true;
                return Ok(Some(true));
            }
        } else {
            // left once a span no longer reaches it and the curve is clear of it
            let away = v.intersector().point(next)?.crv_pos().distance(closure) > 10.0 * tol;
            v.intersector_mut().extension_left_closure = away && !on_span;
        }
    }

    // Check whether the point is inside of the domain of the surfaces.
    let next_tsp = v.intersector().point(next)?.clone();
    let mut on_boundary = false;
    for s in [SrfIndex::First, SrfIndex::Second] {
        // let tol scale to surface domain size
        let domain = at(&v.intersector().uv_domains, s);
        let size = domain.max_dimension();
        let tol = EFF_ZERO * (1.0 + size) * 100.0;
        let ang_tol = EFF_ZERO_SQRT * (1.0 + size) * 10.0;

        // check NextTSP to see if its crossing a boundary
        let (on, bi_norm) = domain.is_point_on_boundary(next_tsp.uv_pos(s), tol);
        let d = next_tsp.uv_deriv(s);
        if on
            && ((bi_norm.x != 0.0 && d.x.abs() > ang_tol)
                || (bi_norm.y != 0.0 && d.y.abs() > ang_tol))
        {
            on_boundary = true;
        }

        // done when point is on and heading out of any boundary
        if on_boundary {
            break;
        }
    }

    // mark points not crossing boundaries as not done and return
    if !on_boundary {
        return Ok(Some(false));
    }

    // else this point is crossing a boundary: remove the start points at it
    for id in v.intersector().start_points.clone() {
        let tsp = v.intersector().point(id)?;
        let mut same = true;
        for s in [SrfIndex::First, SrfIndex::Second] {
            let size = at(&v.intersector().uv_domains, s).max_dimension();
            let tol = EFF_ZERO * (1.0 + size) * 100.0;
            let dist = tsp.uv_pos(s).distance_squared(next_tsp.uv_pos(s));
            if dist > tol {
                same = false;
                break;
            }
        }
        if same {
            unlink(&mut v.intersector_mut().start_points, id);
        }
    }
    Ok(Some(true))
}

// ---- callees ----------------------------------------------------------------------------------

/// `surface.IsSingularity(uv, eSingDir)` (default tolerance): the plane override
/// (never singular); other kinds run `SmSurface::IsSingularity`.
fn surface_is_singularity(k: &Kernel, surface: &Surface, uv: Point2) -> Result<bool> {
    match surface.kind() {
        SurfaceKind::Plane(p) => Ok(p.is_singularity().0),
        SurfaceKind::Cone(_)
        | SurfaceKind::Sphere(_)
        | SurfaceKind::Torus(_)
        | SurfaceKind::BSpline(_)
        | SurfaceKind::ProceduralBlend(_)
        | SurfaceKind::BallPatch(_)
        | SurfaceKind::CurveBounded(_)
        | SurfaceKind::SurfOfRevolution(_)
        | SurfaceKind::SurfOfExtrusion(_)
        | SurfaceKind::Step(_)
        | SurfaceKind::OffsetSurface(_)
        | SurfaceKind::SrfInVolume(_) => {
            Ok(surface.is_singularity(k, uv, EFF_ZERO, false)?.singular)
        }
    }
}

/// The objects of the solve of [`surface_global_curve_intersect`]: the boundary curve
/// and the surface, carrying the natural interval / domain that the solver's
/// identical-solution test reads through upstream's object pointers
/// (`GetNaturalInterval`, `GetNaturalUVDomain`). The solutions returned name no objects.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SolveObject {
    Curve(Extent1d),
    Surface(Extent2d),
}

impl SolutionObject for SolveObject {
    fn kind(&self) -> ObjectKind {
        match self {
            Self::Curve(_) => ObjectKind::Curve,
            Self::Surface(_) => ObjectKind::Surface,
        }
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        match self {
            Self::Curve(ivl) => Some(*ivl),
            Self::Surface(_) => None,
        }
    }

    fn natural_uv_domain(&self) -> Option<Extent2d> {
        match self {
            Self::Curve(_) => None,
            Self::Surface(domain) => Some(*domain),
        }
    }
}

/// The owning-face service of a trimmed cache for [`surface_global_curve_intersect`]'s
/// solve, on its [`SolveObject`] handles: the cache (the surface's, renamed) and the
/// caller's classifications of the cache's face.
struct TrimmedFaceContext<'q, O> {
    cache: SurfaceCache<SolveObject>,
    face: O,
    queries: &'q dyn FaceQueries<O>,
}

impl<O> core::fmt::Debug for TrimmedFaceContext<'_, O> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TrimmedFaceContext").finish_non_exhaustive()
    }
}

impl<O: Copy + PartialEq> SurfaceSolveContext<SolveObject> for TrimmedFaceContext<'_, O> {
    fn build_surface_cache(
        &mut self,
        _: &Kernel,
        _: &Surface,
        _: Option<SolveObject>,
    ) -> Result<SurfaceCache<SolveObject>> {
        self.cache.map_handles(Ok)
    }

    fn face_queries(&self) -> Option<&dyn FaceQueries<SolveObject>> {
        Some(self)
    }
}

impl<O: Copy + PartialEq> FaceQueries<SolveObject> for TrimmedFaceContext<'_, O> {
    fn face_zone_tol_3d(&self, _: SolveObject) -> Result<f64> {
        self.queries.face_zone_tol_3d(self.face)
    }

    fn face_has_loopuses(&self, _: SolveObject) -> Result<bool> {
        self.queries.face_has_loopuses(self.face)
    }

    fn face_point_classify(
        &self,
        k: &Kernel,
        _: SolveObject,
        uv: Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<SolveObject>)> {
        // (the classification object does not name a solve object)
        Ok((
            self.queries
                .face_point_classify(k, self.face, uv, src_tol)?
                .0,
            None,
        ))
    }

    fn face_point_classify_fast_drop(
        &self,
        k: &Kernel,
        _: SolveObject,
        uv: Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<SolveObject>)> {
        Ok((
            self.queries
                .face_point_classify_fast_drop(k, self.face, uv, src_tol)?
                .0,
            None,
        ))
    }
}

/// `surface.GlobalCurveIntersect(domain, curve, interval, tol, solutions)`, which
/// upstream runs on the surface's one cache: here on a copy of `cache` (the
/// intersector's cache of `surface`; its handles, which name only the surface, become
/// the [`SolveObject`] of the surface; a cache naming no surface cannot serve the leaf
/// solves, so the solve then builds its own, in its own modes). The solve on the copy
/// runs in `modes`. The solutions, `[t, u, v]` each (their objects and nodes are not
/// kept).
#[allow(clippy::too_many_arguments)]
fn surface_global_curve_intersect<O: Copy + PartialEq>(
    k: &Kernel,
    surface: &Surface,
    cache: &SurfaceCache<O>,
    modes: SolveModes,
    domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    tol: f64,
    queries: Option<&dyn FaceQueries<O>>,
) -> Result<Vec<Solution<()>>> {
    let surface_object = SolveObject::Surface(surface.natural_uv_domain_with_nurb(k)?);
    let curve_object = SolveObject::Curve(curve.natural_interval());
    // a trimmed cache of a face's surface: the solve's point solves classify their
    // fast-drop solutions on that face (GlobalPointSolve -> pFace->PointClassify)
    let face_context = match queries {
        Some(queries) if cache.surface_handle().is_some() && cache.is_trimmed() => cache
            .face()?
            .map(|face| -> Result<TrimmedFaceContext<'_, O>> {
                Ok(TrimmedFaceContext {
                    cache: cache.map_handles(|_| Ok(surface_object))?,
                    face,
                    queries,
                })
            })
            .transpose()?,
        _ => None,
    };
    let found = if let Some(mut face_context) = face_context {
        let mut solve_cache = cache.map_handles(|_| Ok(surface_object))?;
        surface.global_curve_intersect_from::<SolveObject>(
            k,
            [Some(curve_object), Some(surface_object)],
            Some(&mut solve_cache),
            Some(&mut face_context),
            Some(modes),
            domain,
            curve,
            interval,
            tol,
        )?
    } else if cache.surface_handle().is_some() {
        let mut solve_cache = cache.map_handles(|_| Ok(surface_object))?;
        surface.global_curve_intersect_from::<SolveObject>(
            k,
            [Some(curve_object), Some(surface_object)],
            Some(&mut solve_cache),
            None,
            Some(modes),
            domain,
            curve,
            interval,
            tol,
        )?
    } else {
        // a cache that does not name its surface cannot serve the leaf solves
        // (sm_GetOriginalSurface reads the tree's owner): the solve builds its own
        surface.global_curve_intersect::<SolveObject>(
            k,
            [curve_object, surface_object],
            domain,
            curve,
            interval,
            tol,
        )?
    };
    Ok(found
        .as_slice()
        .iter()
        .map(|s| {
            let mut out = Solution::<()>::new();
            out.solution_type = s.solution_type;
            out.num_variables = s.num_variables;
            out.start = s.start;
            out.end = s.end;
            out
        })
        .collect())
}

/// The outputs of `SmSurface::DropAndTrimCurve` used here.
struct DropAndTrim {
    /// `rdDistToSurf`.
    dist_to_surf: f64,
    /// `rUVCurves`.
    uv_curves: Vec<Curve>,
}

/// `surface.DropAndTrimCurve(ctx, domain, curve, interval, tol, dist, deviation,
/// uv_curves, TRUE, TRUE)`. Its trace (`smsurf_GetSurfaceCache` inside `DoTrace`)
/// runs on a copy of `cache`, the surface's cache the intersector holds, as
/// [`surface_global_curve_intersect`] does; with no cache, or one that does not name
/// its surface, the trace builds its own.
fn surface_drop_and_trim_curve<O: Copy + PartialEq>(
    k: &Kernel,
    surface: &Surface,
    cache: Option<&SurfaceCache<O>>,
    domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    tol: f64,
) -> Result<DropAndTrim> {
    let r = match cache {
        Some(c) if c.surface_handle().is_some() => {
            let surface_object = SolveObject::Surface(surface.natural_uv_domain_with_nurb(k)?);
            let mut trace_cache = c.map_handles(|_| Ok(surface_object))?;
            surface.drop_and_trim_curve_in::<ObjectHandle, SolveObject>(
                k,
                Some(&mut trace_cache),
                domain,
                curve,
                interval,
                tol,
                true,
                true,
                None,
            )?
        }
        _ => surface.drop_and_trim_curve::<ObjectHandle>(
            k, domain, curve, interval, tol, true, true, None,
        )?,
    };
    Ok(DropAndTrim {
        dist_to_surf: r.max_drop_to_surf,
        uv_curves: r.uv_curves,
    })
}

/// The Euclidean control points of a rational Bézier piece (its homogeneous points);
/// `None` where a weight is not positive or a coordinate not finite.
fn euclidean(piece: &[Hom]) -> Option<Vec<Vec3>> {
    let mut points = Vec::new();
    for h in piece {
        let positive = h[3] > 0.0;
        if !(positive && h.iter().all(|c| c.is_finite())) {
            return None;
        }
        points.push(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]));
    }
    Some(points)
}

/// The distance from `p` to the segment `[a, b]` (`clamp`) or to the line through `a` and
/// `b`, plus the bound of its rounding: `8 gamma(16)` of the largest coordinate `s` of the
/// three. Whatever the quotient `t` rounds to, the point `a + t (b - a)` is on the line (in
/// the segment once clamped), so the distance to it bounds the distance from above; the
/// differences, products, sums and root are at most 16 rounded operations on numbers of
/// at most `4 sqrt 3 s`. `None` where `a` and `b` coincide or a number is not finite.
fn distance_to_chord(p: Vec3, a: Vec3, b: Vec3, clamp: bool) -> Option<f64> {
    let size = [p, a, b].iter().fold(0.0f64, |m, c| {
        m.max(c.x.abs()).max(c.y.abs()).max(c.z.abs())
    });
    let (v, w) = (b - a, p - a);
    let vv = v.dot(v);
    let apart = vv > 0.0;
    if !apart {
        return None;
    }
    let t = w.dot(v) / vv;
    let t = if clamp { t.clamp(0.0, 1.0) } else { t };
    let d = (w - v * t).length() + 8.0 * gamma(16) * size;
    d.is_finite().then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::vector3d::Vec3;
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::surface::plane::Plane;

    fn plane(o: [f64; 3], x: [f64; 3], y: [f64; 3]) -> Surface {
        Surface::from(Plane::new(
            Vec3::from_array(o),
            Vec3::from_array(x),
            Vec3::from_array(y),
            Point2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ))
    }

    fn two_planes() -> (Surface, Surface) {
        // z = 0 and y = 0.5: they meet along x
        (
            plane([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            plane([0.0, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        )
    }

    /// The straight segment from `p` to `q`.
    fn line(p: [f64; 3], q: [f64; 3]) -> Curve {
        let segment =
            BSplineCurve::create_line_segment(3, Vec3::from_array(p), Vec3::from_array(q));
        Curve::from(segment.expect("line"))
    }

    fn intersector<'s>(k: &Kernel, a: &'s Surface, b: &'s Surface) -> SurfaceIntersector<'s> {
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        SurfaceIntersector::new(k, a, &d, b, &d, false).expect("intersector")
    }

    /// A refined point on the boundary the singularity was tested on, but away from the
    /// tested corner, is not the pole where that boundary does not collapse: the first
    /// plane's boundary v = 1 is regular, so of the points on it only the corner (0, 1)
    /// itself is the corner's point. (Without the 3D check, `pole_reached` read only the
    /// coordinate across the boundary and took (0.5, 1) as the pole.)
    #[test]
    fn a_point_on_a_regular_boundary_away_from_the_corner_is_not_the_pole() {
        let k = Kernel::default();
        let (a, b) = two_planes();
        let si = intersector(&k, &a, &b);
        let corner = Point2::new(0.0, 1.0);
        let pole = PoleCorner {
            step: 1.0,
            srf: SrfIndex::First,
            on_v_boundary: true,
            bound: 1.0,
            uv_tol: EFF_ZERO * 2.0,
            pole: a.evaluate_point(&k, corner).expect("corner"),
        };
        let other = Point2::new(0.5, 0.5);
        assert!(si
            .pole_reached(&k, &pole, [corner, other])
            .expect("at the corner"));
        let along = [Point2::new(0.5, 1.0), other];
        assert!(pole.on_boundary(along), "on the boundary");
        assert!(
            !si.pole_reached(&k, &pole, along).expect("along"),
            "not the pole"
        );
        let off = [Point2::new(0.0, 0.9), other];
        assert!(
            !si.pole_reached(&k, &pole, off).expect("off"),
            "off the boundary"
        );
    }

    /// The crossing point of the two planes at `x`.
    fn crossing(si: &SurfaceIntersector<'_>, x: f64, param: f64) -> TsectPnt {
        let mut p = TsectPnt::new();
        si.compute_point_values(
            &Kernel::default(),
            [Point2::new(x, 0.5), Point2::new(x, 0.5)],
            &mut p,
            None,
            None,
        )
        .expect("cpv");
        p.curve_parameter = param;
        p
    }

    #[test]
    fn zeroed_point() {
        let p = TsectPnt::zeroed();
        assert_eq!(p.point_type, IntersectionPointType::Undefined);
        assert_eq!(p.uv_param_type, [[ExtentPointType::Start; 2]; 2]);
        assert_eq!(p.crv_pos(), Vec3::ZERO);
    }

    #[test]
    fn next_point_from_a_tangent_point_type_is_not_refined() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let mut si = intersector(k, &a, &b);
        let mut prev = crossing(&si, 0.2, 0.0);
        prev.point_type = IntersectionPointType::Unknown;
        let p = si.alloc_point(prev);
        let n = si.alloc_point(TsectPnt::new());
        // an unknown type is not refined: no good point
        let r = si
            .compute_next_point(&Kernel::default(), p, 0.1, n)
            .expect("next");
        assert!(!r.found_good_point);
        assert!(!r.boundary_hit);
        assert!(si
            .compute_next_point(&Kernel::default(), p, 0.1, p)
            .is_err());
    }

    #[test]
    fn next_point_from_a_tangent_point_back_to_the_corner_is_good() {
        let k = &Kernel::default();
        // z = 0 and y = 0 meet along x; both planes' corner (0, 0) is the origin
        let a = plane([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let mut si = intersector(k, &a, &b);
        let mut prev = TsectPnt::new();
        si.compute_point_values(
            &Kernel::default(),
            [Point2::new(0.5, 0.0), Point2::new(0.5, 0.0)],
            &mut prev,
            None,
            None,
        )
        .expect("cpv");
        prev.point_type = IntersectionPointType::TangentPoint;
        prev.uv_curve_pv[0][1] = Vec2::new(-1.0, 0.0);
        prev.uv_curve_pv[1][1] = Vec2::new(-1.0, 0.0);
        prev.curve_pv[1] = Vec3::new(-1.0, 0.0, 0.0);
        let p = si.alloc_point(prev);
        let n = si.alloc_point(TsectPnt::new());
        // a tangent point is not refined: the next point is the corner
        let r = si
            .compute_next_point(&Kernel::default(), p, 0.1, n)
            .expect("next");
        assert!(r.found_good_point, "{r:?}");
        let next = si.point(n).expect("next");
        assert!(next.crv_pos().length() < 1e-12);
        assert!((next.curve_parameter - 0.5).abs() < 1e-12);
    }

    #[test]
    fn next_point_outside_the_domain_is_sm_err() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let mut si = intersector(k, &a, &b);
        let mut prev = crossing(&si, 0.2, 0.0);
        prev.uv_curve_pv[0][0] = Point2::new(2.0, 0.5);
        let p = si.alloc_point(prev);
        let n = si.alloc_point(TsectPnt::new());
        assert!(si
            .compute_next_point(&Kernel::default(), p, 0.1, n)
            .is_err());
    }

    #[test]
    fn next_point_clipped_to_zero_is_a_boundary_hit() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let mut si = intersector(k, &a, &b);
        let mut prev = crossing(&si, 1.0, 0.0);
        // heading out of both domains
        prev.uv_curve_pv[0][1] = Vec2::new(1.0, 0.0);
        prev.uv_curve_pv[1][1] = Vec2::new(1.0, 0.0);
        let p = si.alloc_point(prev);
        let n = si.alloc_point(TsectPnt::new());
        let r = si
            .compute_next_point(&Kernel::default(), p, 0.1, n)
            .expect("next");
        assert!(r.boundary_hit);
        assert!(r.clipped);
    }

    #[test]
    fn test_point_snaps_to_end_point() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let mut si = intersector(k, &a, &b);
        let p0 = si.alloc_point(crossing(&si, 0.2, 0.0));
        let end = si.alloc_point(crossing(&si, 0.5, -1.0));
        let next = si.alloc_point(crossing(&si, 0.6, 0.4));
        si.add_point_to_curve(p0).expect("curve");
        si.end_point = Some(end);
        let done = si.test_point(&Kernel::default(), next).expect("test");
        assert_eq!(done, Some(true));
        let n = si.point(next).expect("next");
        assert!((n.crv_pos().x - 0.5).abs() < 1e-12);
        // the end point learns its parameter
        assert!(si.point(end).expect("end").curve_parameter >= 0.0);
    }

    #[test]
    fn test_point_through_point_leaves_done_unset() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let mut si = intersector(k, &a, &b);
        let p0 = si.alloc_point(crossing(&si, 0.2, 0.0));
        let th = si.alloc_point(crossing(&si, 0.4, -1.0));
        let next = si.alloc_point(crossing(&si, 0.6, 0.4));
        si.add_point_to_curve(p0).expect("curve");
        si.add_through_point(th).expect("through");
        assert_eq!(si.test_point(&Kernel::default(), next).expect("test"), None);
        assert!(si.through_points().is_empty());
        assert!(si.clipped_by_through_point);
    }

    #[test]
    fn test_point_needs_a_previous_point() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let mut si = intersector(k, &a, &b);
        let next = si.alloc_point(crossing(&si, 0.6, 0.4));
        let end = si.alloc_point(crossing(&si, 0.5, 0.0));
        // nothing to test against: inside the domain, not done
        assert_eq!(
            si.test_point(&Kernel::default(), next).expect("test"),
            Some(false)
        );
        si.end_point = Some(end);
        assert!(si.test_point(&Kernel::default(), next).is_err());
    }

    #[test]
    fn test_point_on_boundary_is_done() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let mut si = intersector(k, &a, &b);
        let p0 = si.alloc_point(crossing(&si, 0.8, 0.0));
        let s = si.alloc_point(crossing(&si, 1.0, 0.0));
        si.add_point_to_curve(p0).expect("curve");
        let next = si.alloc_point(crossing(&si, 1.0, 0.2));
        si.add_start_point(k, s).expect("start");
        assert_eq!(
            si.test_point(&Kernel::default(), next).expect("test"),
            Some(true)
        );
        assert!(si.start_points().is_empty());
    }

    #[test]
    fn update_rejects_alias() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let mut si = intersector(k, &a, &b);
        let p0 = si.alloc_point(crossing(&si, 0.2, 0.0));
        assert!(si
            .update_tsect_point(&Kernel::default(), p0, p0, p0)
            .is_err());
    }

    /// WR-51: a touching point left out for a start point is added at the end only
    /// when its surface points are within the tolerance and no curve found covers it;
    /// a point too far apart, a NaN point and a covered point stay dropped.
    #[test]
    fn deferred_touching_points_are_added_only_as_uncovered_contacts() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let mut si = intersector(k, &a, &b);
        let tol = si.this_approx_tol_3d;
        assert!(tol > 0.0 && tol < 0.01, "{tol}");
        let touching = |p: [f64; 3], gap: f64, uv: f64| {
            let mut t = TsectPnt::new();
            *t.srf_pos_mut(SrfIndex::First) = Vec3::from_array(p);
            *t.srf_pos_mut(SrfIndex::Second) = Vec3::from_array(p) + Vec3::new(0.0, 0.0, gap);
            *t.uv_pos_mut(SrfIndex::First) = Point2::new(uv, 0.5);
            *t.uv_pos_mut(SrfIndex::Second) = Point2::new(uv, 0.5);
            let point = (t.srf_pos(SrfIndex::First) + t.srf_pos(SrfIndex::Second)) / 2.0;
            let boundary = line([0.0, 0.5, 0.0], [1.0, 0.5, 0.0]);
            DeferredTouching {
                point,
                tsp: t,
                boundary,
            }
        };
        // a curve already found at q
        let q = Vec3::new(0.7, 0.5, 0.0);
        let mut out = TsectCurves::default();
        push_touching_curve(&mut out, q, &touching([0.7, 0.5, 0.0], 0.0, 0.7).tsp).unwrap();
        si.deferred_touching = vec![
            touching([0.2, 0.5, 0.0], 0.5 * tol, 0.2), // an uncovered contact
            touching([0.4, 0.5, 0.0], 2.0 * tol, 0.4), // not within the tolerance
            touching([f64::NAN, 0.5, 0.0], 0.0, 0.5),  // not a point
            touching([0.7, 0.5, 0.0], 0.0, 0.7),       // on the curve at q
        ];
        si.add_deferred_touching_points(k, &mut out).unwrap();
        assert!(si.deferred_touching.is_empty());
        assert_eq!(out.curves_3d.len(), 2);
        assert_eq!(out.curve_types, [TsectCurveType::Touching; 2]);
        let p = out.curves_3d[1].evaluate_point(k, 0.0).unwrap();
        assert_eq!(p, Vec3::new(0.2, 0.5, 0.25 * tol));
        assert_eq!(out.deviations[1], 0.5 * tol);
        let uv = out.uv_curves[0][1].evaluate_point(k, 0.0).unwrap();
        assert_eq!((uv.x, uv.y), (0.2, 0.5));
    }

    /// #11749: a crossing curve through a deferred contact that runs within the tolerance
    /// along the contact's boundary curve, either way round, is the contact and gives way
    /// to its touching point, a cubic of two pieces that wavers within half the tolerance
    /// too. A curve through it that leaves the boundary stays and covers it: by twice the
    /// tolerance at its far end, by 0.1, by 0.012 in its middle with both ends on the
    /// boundary, by 3 tolerances between the points it is on the boundary at, or along the
    /// boundary's line past its end. A tangent curve along the boundary stays too.
    #[test]
    fn a_crossing_curve_along_the_boundary_of_a_deferred_contact_gives_way_to_it() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let mut si = intersector(k, &a, &b);
        let tol = si.this_approx_tol_3d;
        let at = [0.2, 0.5, 0.0];
        let mut tsp = TsectPnt::new();
        *tsp.srf_pos_mut(SrfIndex::First) = Vec3::from_array(at);
        *tsp.srf_pos_mut(SrfIndex::Second) = Vec3::from_array(at);
        *tsp.uv_pos_mut(SrfIndex::First) = Point2::new(0.2, 0.5);
        *tsp.uv_pos_mut(SrfIndex::Second) = Point2::new(0.2, 0.5);
        let contact = DeferredTouching {
            point: Vec3::from_array(at),
            tsp,
            boundary: line([0.0, 0.5, 0.0], [1.0, 0.5, 0.0]),
        };
        let mut run = |c: Curve, ty: TsectCurveType| {
            let mut out = TsectCurves::default();
            out.curves_3d.push(c);
            for uv in &mut out.uv_curves {
                uv.push(line([0.2, 0.5, 0.0], [0.3, 0.5, 0.0]));
            }
            out.curve_types.push(ty);
            out.deviations.push(0.0);
            si.deferred_touching = vec![contact.clone()];
            si.add_deferred_touching_points(k, &mut out).unwrap();
            assert!(si.deferred_touching.is_empty());
            out
        };
        // at most half the tolerance from the boundary, either way round: the point
        for c in [
            line(at, [0.3, 0.5, 0.5 * tol]),
            line([0.3, 0.5, 0.5 * tol], at),
        ] {
            let out = run(c, TsectCurveType::Crossing);
            assert_eq!(out.curve_types, [TsectCurveType::Touching]);
            assert_eq!(out.uv_curves.map(|uv| uv.len()), [1, 1]);
            let p = out.curves_3d[0].evaluate_point(k, 0.0).unwrap();
            assert_eq!(p, Vec3::from_array(at));
        }
        let out = run(line(at, [0.3, 0.5, 0.5 * tol]), TsectCurveType::NearTangent);
        assert_eq!(out.curve_types, [TsectCurveType::Touching]);
        let cubic = |zs: [f64; 5], knots: Vec<f64>| {
            let pw = [0.0, 0.25, 0.5, 0.75, 1.0]
                .iter()
                .zip(zs)
                .map(|(&s, z)| CPoint::new(0.2 + 0.1 * s, 0.5, z * tol, NOW))
                .collect();
            let c = NurbCurve::new(pw, 3, knots).expect("cubic");
            Curve::from(BSplineCurve::from_nurb(c, 3, false))
        };
        let two = vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0];
        let waver = cubic([0.0, 0.4, 0.1, 0.45, 0.3], two);
        let out = run(waver, TsectCurveType::Crossing);
        assert_eq!(out.curve_types, [TsectCurveType::Touching]);
        // leaving the boundary: the curve stays and covers the point
        for far in [2.0 * tol, 0.1] {
            let out = run(line(at, [0.3, 0.5, far]), TsectCurveType::Crossing);
            assert_eq!(out.curve_types, [TsectCurveType::Crossing], "{far}");
        }
        // both ends on the boundary, the middle 0.0118 off it
        let bulge = BSplineCurve::create_arc_from_points(
            Vec3::new(0.25, 0.5, -0.1),
            Vec3::from_array(at),
            Vec3::new(0.3, 0.5, 0.0),
            crate::nurbs::b_spline_curve::p1::NurbCircleParam::Quadratic,
        )
        .unwrap();
        let out = run(Curve::from(bulge), TsectCurveType::Crossing);
        assert_eq!(out.curve_types, [TsectCurveType::Crossing]);
        // a cubic on the boundary at its ends and its middle, 3 tol off it at a quarter:
        // z = 64 tol t (t - 1/2) (t - 1), Bernstein coefficients 64 tol (0, 1/6, -1/6, 0)
        let z = 64.0 * tol / 6.0;
        let pw = [(0.0, 0.0), (1.0 / 3.0, z), (2.0 / 3.0, -z), (1.0, 0.0)]
            .iter()
            .map(|&(s, z)| CPoint::new(0.2 + 0.1 * s, 0.5, z, NOW))
            .collect();
        let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let wave = NurbCurve::new(pw, 3, knots).expect("cubic");
        let wave = Curve::from(BSplineCurve::from_nurb(wave, 3, false));
        let off = wave.evaluate_point(k, 0.25).unwrap().z;
        assert!((off - 3.0 * tol).abs() < 1e-3 * tol, "{off}");
        let out = run(wave, TsectCurveType::Crossing);
        assert_eq!(out.curve_types, [TsectCurveType::Crossing]);
        // along the boundary's line past its end at x = 1
        let out = run(line(at, [1.2, 0.5, 0.0]), TsectCurveType::Crossing);
        assert_eq!(out.curve_types, [TsectCurveType::Crossing]);
        // a tangent curve along the boundary is no crossing
        let out = run(line(at, [0.3, 0.5, 0.5 * tol]), TsectCurveType::Tangent);
        assert_eq!(out.curve_types, [TsectCurveType::Tangent]);
    }

    #[test]
    fn find_boundary_start_points_with_a_far_box_finds_nothing() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let mut si = intersector(k, &a, &b);
        si.set_use_surface_edges(false, false);
        let k = Kernel::default();
        let mut out = TsectCurves::default();
        let far = Extent3d::from_bounds(50.0, 50.0, 50.0, 51.0, 51.0, 51.0);
        // (the surface cache tree build is not ported: no fetched cache)
        let mut caches = vec![SurfaceCache::<()>::new(
            a.clone(),
            a.natural_uv_domain_if_built(),
            crate::surface::surface_cache::SurfaceCacheOptions::default(),
        )];
        si.find_boundary_start_points_with_caches(&k, &mut caches, &far, &mut out)
            .expect("fbsp");
        assert!(si
            .find_boundary_start_points_with_caches::<()>(&k, &mut [], &far, &mut out)
            .is_err());
        assert!(out.curves_3d.is_empty());
        assert!(si.start_points().is_empty());
    }

    /// The boundary solves run in the modes the caller gives, not in the caches' flags,
    /// and leave the caches' flags as built: overriding caches with point testing on and
    /// boundary curve processing off finds what caches holding the override do.
    #[test]
    fn boundary_solves_run_in_the_given_modes_and_leave_the_cache_flags() {
        let k = Kernel::default();
        let (a, b) = two_planes();
        let all = Extent3d::from_bounds(-10.0, -10.0, -10.0, 10.0, 10.0, 10.0);
        let caches_with = |modes: SolveModes| {
            [(&a, SrfIndex::First), (&b, SrfIndex::Second)]
                .map(|(s, i)| {
                    let mut c = surface_cache::<SrfIndex>(&k, s, Some(i)).expect("cache");
                    c.point_test_enabled = modes.point_test_enabled;
                    c.process_boundary_curves = modes.process_boundary_curves;
                    c
                })
                .to_vec()
        };
        let found = |si: &SurfaceIntersector<'_>, out: &TsectCurves| {
            let points: Vec<String> = si
                .start_points()
                .iter()
                .map(|&id| format!("{:?}", si.point(id).expect("point")))
                .collect();
            (points, format!("{out:?}"))
        };
        let built = SolveModes {
            point_test_enabled: true,
            process_boundary_curves: false,
        };
        let boundary = SolveModes {
            point_test_enabled: false,
            process_boundary_curves: true,
        };

        // the override, over caches built with other flags
        let mut caches = caches_with(built);
        let mut si = intersector(&k, &a, &b);
        let mut out = TsectCurves::default();
        si.find_boundary_start_points_under(&k, &mut caches, [boundary; 2], &all, &mut out, None)
            .expect("boundary start points");
        assert!(caches.iter().all(|c| c.solve_modes() == built));
        let overridden = found(&si, &out);
        assert!(!overridden.0.is_empty());

        // the same solves on caches that hold the override's flags themselves
        let mut caches = caches_with(boundary);
        let mut si = intersector(&k, &a, &b);
        let mut out = TsectCurves::default();
        si.find_boundary_start_points_with_caches(&k, &mut caches, &all, &mut out)
            .expect("boundary start points");
        assert!(caches.iter().all(|c| c.solve_modes() == boundary));
        assert_eq!(found(&si, &out), overridden);

        // and the public entry turns boundary curve processing on for its solves only
        let mut caches = caches_with(built);
        let mut si = intersector(&k, &a, &b);
        let mut out = TsectCurves::default();
        si.find_boundary_start_points_with_caches(&k, &mut caches, &all, &mut out)
            .expect("boundary start points");
        assert!(caches.iter().all(|c| c.solve_modes() == built));
    }

    /// A boundary curve whose trim to the domain ends at a knot just inside it is
    /// intersected over its own interval (#17450). The first surface is the unit square
    /// of the plane z = 0 as a rational B-spline with a double knot at u = 0.5, and its
    /// domain starts a hundredth of the zone tolerance before that knot: the trim of its
    /// v = 1 boundary curve keeps no span that short and starts at the knot. That curve
    /// lies in the plane y = 1, a coincident range up to that plane's edge. Solved over
    /// the domain's interval, the range started before the curve, and trimming the
    /// curve to it failed.
    #[test]
    fn a_boundary_curve_ending_at_a_knot_inside_the_domain_is_intersected_over_its_interval() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as NurbSurface;

        let k = Kernel::default();
        let net = [0.0, 0.25, 0.5, 0.75, 1.0]
            .iter()
            .map(|&x| vec![CPoint::new(x, 0.0, 0.0, 1.0), CPoint::new(x, 1.0, 0.0, 1.0)])
            .collect();
        let square = Surface::from(BSplineSurface::from_nurb(
            NurbSurface::new(
                net,
                2,
                1,
                vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .expect("net"),
            false,
        ));
        // the plane y = 1 over x in [-2, 0.75] and z in [-2, 2]: the range ends inside
        // the curve, so the curve is trimmed to it
        let wall = Surface::from(Plane::new(
            Vec3::new(-2.0, 1.0, -2.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Point2::new(2.75, 4.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ));
        let short = ZONE_TOL_3D / 100.0;
        let domain = Extent2d::from_bounds(0.5 - short, 0.0, 1.0, 1.0);
        let whole = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut si = SurfaceIntersector::new(&k, &square, &domain, &wall, &whole, false)
            .expect("intersector");
        let mut out = TsectCurves::default();
        let all = Extent3d::from_bounds(-10.0, -10.0, -10.0, 10.0, 10.0, 10.0);
        si.find_boundary_start_points(&k, &all, &mut out)
            .expect("boundary start points");
        // the boundary curve from the knot to the wall's edge
        let found: Vec<Extent1d> = out.curves_3d.iter().map(Curve::natural_interval).collect();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found.iter().all(|ivl| ivl.min() == 0.5 && ivl.max() < 1.0),
            "{found:?}"
        );
    }
}
