//! `SmSurfaceIntersector`, part 5: the top level surface/surface intersection
//! ([`do_intersection`]) and its grazing-box shortcut
//! ([`SurfaceIntersector::grazing_box_process`]).
//!
//! Upstream: `source/SMLib/src/SmSurfaceIntersector.cpp`.
//!
//! | Rust                                                  | Upstream                      |
//! |-------------------------------------------------------|-------------------------------|
//! | [`do_intersection`], [`AdvSurfaceIntersector::do_intersection_with_caches`] | `DoIntersection` |
//! | [`intersection_surface_caches`], [`IntersectionSurface`] | the `smsurf_GetSurfaceCache` / `GetTree` calls at the top of `DoIntersection` |
//! | [`SurfaceIntersector::grazing_box_process`], [`GrazingBox`] | `GrazingBoxProcess`     |
//!
//! Deviations:
//! - **Dispatch.** `DoIntersection` is a base class method, but every upstream caller
//!   (`SmSurface::GlobalSurfaceIntersect`, `SmSurface::ParallelProjectCurve`) runs it
//!   on an `SmAdvSurfaceIntersector`, whose `SolveBranch` / `LocalSolve` overrides
//!   answer the `SM_SO_INTERSECTION_TEST` solve and whose `FindInteriorCurves`
//!   override finds the interior loops. It is therefore ported on
//!   [`AdvSurfaceIntersector`] (on a plain `SmSurfaceIntersector` the base
//!   `LocalSolve` stores node-only solutions whose box and domain values are never
//!   set, a case no caller reaches). `ComputePointValues` (part 2, called by
//!   `FindBoundaryStartPoints` and part 4's `TraceCurve`) reaches the advanced
//!   `ComputeSingularityPoint` override through the base's `advanced` flag, which
//!   [`AdvSurfaceIntersector::new`] sets.
//! - **Surface caches.** Rust surfaces keep no cache, so the caller hands the caches
//!   in ([`intersection_surface_caches`] builds them as `smsurf_GetSurfaceCache` /
//!   `GetTree` would; [`do_intersection`] does both). One
//!   cache per surface, one for a self intersection (upstream gets the same cache
//!   twice). The temporary flags (`m_bPointTestEnabled = FALSE`,
//!   `m_bProcessBoundaryCurves = TRUE`) are set before the first solve rather than
//!   after it (that `SM_SO_INTERSECTION_TEST` solve and `GrazingBoxProcess` read
//!   neither) and stay set on the caller's caches (upstream restores them on return; here
//!   the caches stay borrowed by the solver for `'t`, so they cannot be written
//!   back on exit; [`do_intersection`]'s caches are local, so it is unaffected).
//!   The boundary bounding planes are computed on copies of the caches' (each lazily
//!   computed plane is the same), and `FindBoundaryStartPoints` runs on copies of the
//!   caches, flags included.
//! - **Closure is no self intersection** (not upstream, #18117). A self intersection
//!   drops the curves that are the surface's own closure: UV curves that are the iso-lines
//!   at the two ends of a closing direction, at the same parameter across, proved for the
//!   whole curve from their control points ([`is_closure_curve`]); upstream reports a
//!   closed surface's seam as a self intersection, so a closed offset surface failed its
//!   own validity test. Every other curve, a crossing on or next to the seam too, is kept.
//! - **Outputs.** The out-params become one returned [`SurfaceIntersection`] (a
//!   `NULL` UV curve is `None`); on an error nothing is returned. `GrazingBoxProcess`
//!   returns its curve instead of appending it to the out-params.
//! - **Statuses.** `TraceCurve`'s status, ignored upstream (no curve is flushed), is
//!   ignored except [`SolidError::Unported`], which always propagates; so is
//!   `EvaluatePoint`'s in the degenerate curve sweep (an unset point is
//!   [`Vec3::UNDEF`]). `IsDegenerate()` (default tolerance `SM_EFF_ZERO`) and
//!   `IsPointOnCurve` return `SmBoolean` upstream and cannot fail; their Rust ports
//!   return `Result`, and an error from them (unreachable today) propagates instead
//!   of reading as `FALSE`. Mismatched curve arrays in the degenerate
//!   curve sweep (an `SM_ASSERT`, then a read past the array upstream) are
//!   [`SolidError::InvalidInput`].
//! - **Bounded loops.** The start point loop pops one start point per pass; no pass
//!   adds start points (`TraceCurve` adds none), so it ends after at most as many
//!   passes as `FindBoundaryStartPoints` made points.
//! - **Touching points left for a start point** (not upstream). After the interior
//!   curves and the removal of degenerate curves that lie on other curves, a touching
//!   point `FindBoundaryStartPoints` dropped because a start point lies on it is
//!   added when its surface points are within the tolerance and it is on none of the
//!   curves left ([`SurfaceIntersector::add_deferred_touching_points`]); such points
//!   come after the traced curves, where upstream's boundary pass would have put them
//!   first.
//! - **Fix (WR-76, #392).** A traced curve removes, before it is flushed, the start
//!   points left in a tangent cluster with either of its ends
//!   (`SurfaceIntersector::remove_end_tangent_clusters`); the SolidCraft fork's
//!   tangency-labs/smlib@0ce09f2. Upstream traces each of them again.
//! - **Fix (WR-77, #393, #2110).** The curves are split at the singular points they
//!   pass through after the tangent nodes are added (`super::singular_splits`); the
//!   SolidCraft fork's tangency-labs/smlib@3c9d1d3. Always on (once the opt-in
//!   `ssi_split_curves_at_singular_points`).
//! - **Tangential curves (WR-75, #395).** After the interior curves, the SolidCraft fork's
//!   tangential curve pass traces the curves along which the surfaces touch and the crossing
//!   branches through their contact points (`super::tangent_curves`); always on (once the
//!   opt-in `ssi_tangent_curve_tracer`). Not in a self intersection whose two domains overlap
//!   or touch (#18930, `AdvSurfaceIntersector::trivial_solution_in_both_domains`), where the
//!   surface touches itself everywhere at equal parameters; a self tangency at two different
//!   parameters there is left to the marcher. A self intersection also
//!   skips the boundary start point search, the tangential contact point search (WR-74) and
//!   the singular point splits (WR-77).
//! - **Empty trace (WR-80).** A curve is traced back the other way
//!   ([`SurfaceIntersector::trace_back`], part 4) only when `TraceCurve` produced
//!   points. Upstream also reverses the empty curve of a trace
//!   whose start already lies on a found curve, and `ReverseCurveDirection` fails the
//!   whole intersection with `SM_ERR_NULL_POINTER` (the fork's
//!   tangency-labs/smlib@0dbe681).
//! - **Not ported here.** The `SM_DEBUG_CODE` blocks (graphics, dumps, the start
//!   point reordering experiment), `SmCacheCheckOutIn` (the caches are owned by the
//!   caller), the unused context argument and the `SM_ASSERT`s.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::intersect::IntersectFailure;
use crate::error::{ignored, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::extent1d::ExtentPointType;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::{GlobalSolve, SolutionRequested, SolveRequest, SolverOperation};
use crate::geom::solution_array::{SolutionArray, SolutionObject};
use crate::geom::tol::is_zero_to_tol;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::intersect::adv_surface_intersector::{AdvSurfaceIntersector, InteriorCurves};
use crate::kernel::Kernel;
use crate::math::{BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::knots::Side;
use crate::nurbs::math::gamma;
use crate::surface::surface::p1::SurfaceSolveContext;
use crate::surface::surface::p2::{surface_cache, SurfaceIntersection, TsectCurveType};
use crate::surface::surface::Surface;
use crate::surface::surface_cache::face_context::FaceQueries;
use crate::surface::surface_cache::p1::{PatchBoundaryBoundingPlanes, SolveModes, SurfaceCache};

use super::p1::{SrfIndex, SurfaceIntersector, TsectPnt};
use super::p2::{BoundaryPlaneTestResult, TsectCurves};

const OP_DI: &str = "intersect::surface_intersector::p5::AdvSurfaceIntersector::do_intersection";
const OP_CACHES: &str = "intersect::surface_intersector::p5::intersection_surface_caches";

/// The point intersection [`SurfaceIntersector::grazing_box_process`] found: a
/// degenerate 3D curve, a degenerate UV curve on each surface (all degree 1, both
/// control points at the point), type touching and the distance between the two
/// surface points as deviation.
#[derive(Debug, Clone)]
pub struct GrazingBox {
    /// `p3DCurves` entry.
    pub curve_3d: Curve,
    /// `pSurface1UVCurves`, `pSurface2UVCurves` entries.
    pub uv_curves: [Curve; 2],
    /// `pCurveTypes` entry (always [`TsectCurveType::Touching`]).
    pub curve_type: TsectCurveType,
    /// `pDeviations` entry.
    pub deviation: f64,
}

/// `smsurf_GetSurfaceCache(m_cpSurface[i])` and its `GetTree()`, as `DoIntersection`
/// fetches them: the cache of the first surface and, unless the intersector
/// intersects a surface with itself, of the second, each with its decomposition tree
/// built. `handles` name the two surfaces in their caches (upstream's caches and
/// trees name their surface; the leaf solves of `GlobalCurveIntersect`, which runs on
/// these caches, read it).
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` for a surface without cache or tree (`NER`);
/// [`SolidError::Unported`] from the cache and tree builds.
pub fn intersection_surface_caches<O: Copy + PartialEq>(
    k: &Kernel,
    si: &SurfaceIntersector<'_>,
    handles: [O; 2],
) -> Result<Vec<SurfaceCache<O>>> {
    intersection_surface_caches_in(k, si, handles.map(Some), None)
}

/// [`intersection_surface_caches`] through the caller's face-aware cache context:
/// the context builds each cache with the surface handle that names it, so a surface
/// owned by a face reaches that face (upstream's `GetFace()`).
///
/// # Errors
///
/// Those of [`intersection_surface_caches`] and of the context's cache preparation.
pub fn intersection_surface_caches_with_context<O: Copy + PartialEq>(
    k: &Kernel,
    si: &SurfaceIntersector<'_>,
    handles: [Option<O>; 2],
    context: &mut dyn SurfaceSolveContext<O>,
) -> Result<Vec<SurfaceCache<O>>> {
    intersection_surface_caches_in(k, si, handles, Some(context))
}

fn intersection_surface_caches_in<O: Copy + PartialEq>(
    k: &Kernel,
    si: &SurfaceIntersector<'_>,
    handles: [Option<O>; 2],
    mut context: Option<&mut dyn SurfaceSolveContext<O>>,
) -> Result<Vec<SurfaceCache<O>>> {
    let ner = |e: SolidError| {
        if e.is_unported_or_cancelled() {
            e
        } else {
            Failure::new(OP_CACHES, IntersectFailure::SurfaceCacheNotBuilt)
                .caused_by(e)
                .into_error()
        }
    };
    let [h1, h2] = handles;
    let mut get = |s, h| -> Result<SurfaceCache<O>> {
        let (built, from_context) = match context.as_mut() {
            Some(ctx) => (ctx.build_surface_cache(k, s, h), true),
            None => (surface_cache::<O>(k, s, h), false),
        };
        let mut c = built.map_err(ner)?;
        // A context builds the cache with its tree ready (`GetTree` through the face
        // that owns the surface, `SurfaceSolveContext::build_surface_cache`): calling
        // `GetTree` again here, in the same state, would rebuild the same tree.
        if !(from_context && c.tree().is_some()) {
            c.get_tree(k).map_err(ner)?;
        }
        Ok(c)
    };
    let mut caches = vec![get(si.surface(SrfIndex::First), h1)?];
    if !si.doing_self_intersection {
        caches.push(get(si.surface(SrfIndex::Second), h2)?);
    }
    Ok(caches)
}

/// The surface handles of the caches [`do_intersection`] builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntersectionSurface {
    /// The first surface.
    First,
    /// The second surface.
    Second,
}

impl SolutionObject for IntersectionSurface {
    fn kind(&self) -> crate::geom::solution_array::ObjectKind {
        crate::geom::solution_array::ObjectKind::Surface
    }
}

impl SurfaceIntersector<'_> {
    /// The intersection of surfaces whose common box is thin (`GrazingBoxProcess`):
    /// for each corner of the first target domain inside `uv_domain1` and each
    /// corner of the second inside `uv_domain2` (u then v, minimum first), whose
    /// surface points are closer than `this_approx_tol_3d` and whose corner tangents
    /// (pointing into the domains) are not degenerate: when both tangents of the
    /// second surface, projected onto the first surface's tangent plane, are tiny,
    /// or one of them points into the first surface's corner sector, more work is
    /// needed; otherwise the corner is a touching point and no more work is needed.
    /// The first such corner pair decides; without one, more work is needed.
    ///
    /// Returns whether more work is needed and the touching point, if any.
    ///
    /// # Errors
    ///
    /// Those of the evaluations, unit vectors and degenerate curves.
    pub fn grazing_box_process(
        &self,
        k: &Kernel,
        uv_domain1: &Extent2d,
        uv_domain2: &Extent2d,
    ) -> Result<(bool, Option<GrazingBox>)> {
        // locals
        let approx_tol_3d = self.this_approx_tol_3d;

        // for all 4 Surface1->UVcorners of the crUVDomain1
        for iu1 in 0..2_u32 {
            for iv1 in 0..2_u32 {
                // skip UVcorner[iu1,uv1] not contained by crUVDomain1
                let uv1 = self.uv_domains[0].evaluate(f64::from(iu1), f64::from(iv1));
                if !uv_domain1.contains_point(uv1, 0.0) {
                    continue;
                }

                // for all 4 Surface2->UVCorners of the crUVDomain2
                for iu2 in 0..2_u32 {
                    for iv2 in 0..2_u32 {
                        // skip UVcorner[iu2,iv2] not contained by crUVDomain2
                        let uv2 = self.uv_domains[1].evaluate(f64::from(iu2), f64::from(iv2));
                        if !uv_domain2.contains_point(uv2, 0.0) {
                            continue;
                        }

                        // a corner inside both intersection domains: compare the
                        // 3D points
                        let (pnt1, mut du1, mut dv1) =
                            evaluate_1st_derivatives(k, self.surface(SrfIndex::First), uv1)?;
                        let (pnt2, mut du2, mut dv2) =
                            evaluate_1st_derivatives(k, self.surface(SrfIndex::Second), uv2)?;
                        if iu1 == 1 {
                            du1 = -du1;
                        }
                        if iv1 == 1 {
                            dv1 = -dv1;
                        }
                        if iu2 == 1 {
                            du2 = -du2;
                        }
                        if iv2 == 1 {
                            dv2 = -dv2;
                        }

                        // when 3D Points are within tolerance
                        if pnt1.distance(pnt2) < approx_tol_3d {
                            // skip singular surfaces
                            if du1.length_squared() < EFF_ZERO
                                || dv1.length_squared() < EFF_ZERO
                                || du2.length_squared() < EFF_ZERO
                                || dv2.length_squared() < EFF_ZERO
                            {
                                continue;
                            }

                            // normalize the tangents
                            let du1 = du1.unitize()?;
                            let dv1 = dv1.unitize()?;
                            let du2 = du2.unitize()?;
                            let dv2 = dv2.unitize()?;

                            // get Surface1 Normal
                            let norm1 = du1.cross(dv1);
                            if norm1.length_squared() < EFF_ZERO {
                                continue;
                            }

                            // sBin: orthogonalized sDV1, in the sDU1/sDV1 plane
                            let bin = norm1.cross(du1);

                            // project both sDU2 and sDV2 onto Surface1 plane
                            let proj_du2 = norm1.cross(du2).cross(norm1);
                            let proj_dv2 = norm1.cross(dv2).cross(norm1);
                            let mut u_inside = false;
                            let mut v_inside = false;

                            // both derivative vectors of the second surface are
                            // perpendicular to the first tangent plane: no good read
                            if proj_du2.length_squared() < EFF_ZERO_SQ
                                && proj_dv2.length_squared() < EFF_ZERO_SQ
                            {
                                return Ok((true, None));
                            }

                            // does the Surface2 domain corner extend into the
                            // Surface1 domain corner sector
                            if proj_du2.length_squared() > EFF_ZERO_SQ {
                                u_inside = proj_du2.is_inside_sector(du1, bin, dv1);
                            }
                            if proj_dv2.length_squared() > EFF_ZERO_SQ {
                                v_inside = proj_dv2.is_inside_sector(du1, bin, dv1);
                            }

                            if !u_inside && !v_inside {
                                // a point intersection - mark it with degenerate curves
                                let curve_3d =
                                    Curve::from(BSplineCurve::create_degenerate_curve(3, pnt1)?);
                                let uv_curve1 = Curve::from(BSplineCurve::create_degenerate_curve(
                                    2,
                                    Vec3::from(uv1),
                                )?);
                                let uv_curve2 = Curve::from(BSplineCurve::create_degenerate_curve(
                                    2,
                                    Vec3::from(uv2),
                                )?);
                                let dist = pnt1.distance(pnt2);
                                return Ok((
                                    false,
                                    Some(GrazingBox {
                                        curve_3d,
                                        uv_curves: [uv_curve1, uv_curve2],
                                        curve_type: TsectCurveType::Touching,
                                        deviation: dist,
                                    }),
                                ));
                            }

                            // further work is needed
                            return Ok((true, None));
                        }
                    }
                }
            }
        }
        Ok((true, None))
    }
}

/// `DoIntersection` as `SmSurface::GlobalSurfaceIntersect` runs it: an
/// [`AdvSurfaceIntersector`] of `surface1` over `uv_domain1` and `surface2` over
/// `uv_domain2` (not from a fillet intersector) using the boundaries of the surfaces
/// flagged in `use_surface_edges`, run on the surface caches
/// ([`intersection_surface_caches`], built for the call) with the optional
/// tolerances ([`AdvSurfaceIntersector::do_intersection_with_caches`]).
///
/// # Errors
///
/// Those of [`AdvSurfaceIntersector::new`], [`intersection_surface_caches`] and
/// [`AdvSurfaceIntersector::do_intersection_with_caches`].
pub fn do_intersection(
    k: &Kernel,
    surface1: &Surface,
    uv_domain1: &Extent2d,
    surface2: &Surface,
    uv_domain2: &Extent2d,
    use_surface_edges: [bool; 2],
    opt_approx_tol_3d: Option<f64>,
    opt_ang_tol_rad: Option<f64>,
) -> Result<SurfaceIntersection> {
    crate::profile::profile_scope!(k, "intersect::ssi::do_intersection");
    let mut caches: Vec<SurfaceCache<IntersectionSurface>>;
    let mut ssi: AdvSurfaceIntersector<'_, '_, IntersectionSurface> =
        AdvSurfaceIntersector::new(k, surface1, uv_domain1, surface2, uv_domain2, false)?;
    ssi.base
        .set_use_surface_edges(use_surface_edges[0], use_surface_edges[1]);
    caches = intersection_surface_caches(
        k,
        &ssi.base,
        [IntersectionSurface::First, IntersectionSurface::Second],
    )?;
    ssi.do_intersection_with_caches(k, &mut caches, opt_approx_tol_3d, opt_ang_tol_rad)
}

/// [`do_intersection`] through the caller's face-aware cache context. `handles` name
/// the two surfaces in their caches; the context builds each cache (and so reaches
/// the face that owns the surface) instead of the handle-less default.
///
/// # Errors
///
/// Those of [`do_intersection`] and of the context's cache preparation.
pub fn do_intersection_with_context<O: SolutionObject + Copy + PartialEq>(
    k: &Kernel,
    surface1: &Surface,
    uv_domain1: &Extent2d,
    surface2: &Surface,
    uv_domain2: &Extent2d,
    use_surface_edges: [bool; 2],
    opt_approx_tol_3d: Option<f64>,
    opt_ang_tol_rad: Option<f64>,
    context: &mut dyn SurfaceSolveContext<O>,
    handles: [Option<O>; 2],
) -> Result<SurfaceIntersection> {
    let mut caches: Vec<SurfaceCache<O>>;
    let mut ssi: AdvSurfaceIntersector<'_, '_, O> =
        AdvSurfaceIntersector::new(k, surface1, uv_domain1, surface2, uv_domain2, false)?;
    ssi.base
        .set_use_surface_edges(use_surface_edges[0], use_surface_edges[1]);
    caches = intersection_surface_caches_with_context(k, &ssi.base, handles, context)?;
    ssi.do_intersection_in(
        k,
        &mut caches,
        opt_approx_tol_3d,
        opt_ang_tol_rad,
        context.face_queries(),
        true,
    )
}

impl<'t, O: SolutionObject + Copy + PartialEq + 't> AdvSurfaceIntersector<'_, 't, O> {
    /// The surface/surface intersection (`DoIntersection`), on `caches`: the surface
    /// cache of the first surface and (unless it is the only one, for a self
    /// intersection) of the second; their trees are built when missing and become
    /// the solver trees.
    ///
    /// `opt_approx_tol_3d` / `opt_ang_tol_rad` replace `this_approx_tol_3d` /
    /// `this_ang_tol_rad` when given. Then:
    ///
    /// 1. Root boxes apart by more than the tolerance: no intersection. Else the
    ///    `SM_SO_INTERSECTION_TEST` solve collects the leaf pairs whose patches may
    ///    meet; none: no intersection. Their box (union of the leaf box overlaps)
    ///    and domains are the intersection region.
    /// 2. One leaf pair whose box is thinner than twice the tolerance in some
    ///    dimension: [`SurfaceIntersector::grazing_box_process`]; a touching corner
    ///    ends the intersection with that point.
    /// 3. The boundary bounding plane test
    ///    ([`SurfaceIntersector::do_boundary_plane_test`]): disjoint surfaces have no
    ///    intersection; grazing surfaces intersect in the boundary curve
    ///    intersections it found (without UV curves).
    /// 4. Unless intersecting a surface with itself, the boundary start points
    ///    ([`SurfaceIntersector::find_boundary_start_points`], which may add
    ///    boundary curves). From the last start point on, each is traced
    ///    ([`SurfaceIntersector::trace_curve`]; for a start point whose UV tangents
    ///    run along every boundary it is on, also backwards when the curve is not
    ///    closed) and flushed into a curve; a start point that gives no curve becomes
    ///    a through point.
    /// 5. The interior curves ([`AdvSurfaceIntersector::find_interior_curves`]).
    /// 6. Degenerate curves (`IsDegenerate()`, tolerance `SM_EFF_ZERO`) whose point
    ///    is on another curve are removed with their UV curves, type and deviation.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without a cache or tree; those of the solves, the
    /// boundary tests, the flushes, the reversals, the backward traces and the curve
    /// predicates; [`SolidError::InvalidInput`] for curve arrays of different
    /// lengths; [`SolidError::Unported`] from unported callees.
    #[allow(clippy::too_many_lines)] // one upstream function
    pub fn do_intersection_with_caches(
        &mut self,
        k: &Kernel,
        caches: &'t mut [SurfaceCache<O>],
        opt_approx_tol_3d: Option<f64>,
        opt_ang_tol_rad: Option<f64>,
    ) -> Result<SurfaceIntersection> {
        self.do_intersection_with_caches_and_queries(
            k,
            caches,
            opt_approx_tol_3d,
            opt_ang_tol_rad,
            None,
        )
    }

    /// [`AdvSurfaceIntersector::do_intersection_with_caches`] with the face
    /// classifications of the context that made the caches: the boundary start
    /// points' solves on a face's trimmed cache classify on that face
    /// ([`SurfaceIntersector::find_boundary_start_points_with_queries`]).
    ///
    /// # Errors
    ///
    /// As [`AdvSurfaceIntersector::do_intersection_with_caches`].
    pub fn do_intersection_with_caches_and_queries(
        &mut self,
        k: &Kernel,
        caches: &'t mut [SurfaceCache<O>],
        opt_approx_tol_3d: Option<f64>,
        opt_ang_tol_rad: Option<f64>,
        queries: Option<&dyn FaceQueries<O>>,
    ) -> Result<SurfaceIntersection> {
        self.do_intersection_in(
            k,
            caches,
            opt_approx_tol_3d,
            opt_ang_tol_rad,
            queries,
            false,
        )
    }

    /// [`AdvSurfaceIntersector::do_intersection_with_caches_and_queries`]; with
    /// `trees_ready` the caches came from a face-aware context with their trees ready
    /// (`GetTree` just ran through their faces), so a cache with a tree is not asked
    /// for it again: in the same state that `GetTree` would rebuild the same tree, and
    /// the two flags its rebuild sets are overwritten below.
    #[allow(clippy::too_many_lines)] // one upstream function
    fn do_intersection_in(
        &mut self,
        k: &Kernel,
        caches: &'t mut [SurfaceCache<O>],
        opt_approx_tol_3d: Option<f64>,
        opt_ang_tol_rad: Option<f64>,
        queries: Option<&dyn FaceQueries<O>>,
        trees_ready: bool,
    ) -> Result<SurfaceIntersection> {
        let mut result = SurfaceIntersection::default();
        // filled by FindBoundaryStartPoints, read at the end (not upstream)
        self.base.deferred_touching.clear();
        // filled by FindInteriorCurves, read by the tangential curve pass (WR-75)
        self.tangent_curve_seeds.clear();

        if let Some(t) = opt_approx_tol_3d {
            self.base.this_approx_tol_3d = t;
        }
        if let Some(a) = opt_ang_tol_rad {
            self.base.this_ang_tol_rad = a;
        }
        let tol = self.base.this_approx_tol_3d;

        // get surface caches and their decomposition trees (NER)
        let Some((first, rest)) = caches.split_first_mut() else {
            return Err(SolidError::intersect(
                OP_DI,
                IntersectFailure::NoSurfaceCaches,
            ));
        };
        for c in core::iter::once(&mut *first).chain(rest.iter_mut()).take(2) {
            if !(trees_ready && c.tree().is_some()) {
                c.get_tree(k).map_err(|e| {
                    if e.is_unported_or_cancelled() {
                        e
                    } else {
                        Failure::new(OP_DI, IntersectFailure::SurfaceCacheNotBuilt)
                            .caused_by(e)
                            .into_error()
                    }
                })?;
            }
        }

        // the boundary bounding planes of both caches (copies: lazily computed)
        let mut planes1: PatchBoundaryBoundingPlanes =
            first.patch_boundary_bounding_planes().clone();
        let mut planes2 = match rest.first_mut() {
            Some(c) => c.patch_boundary_bounding_planes().clone(),
            None => planes1.clone(),
        };

        let first: &'t SurfaceCache<O> = first;
        let rest: &'t [SurfaceCache<O>] = rest;
        let cache1 = first;
        let cache2 = rest.first().unwrap_or(cache1);
        self.bind_caches(cache1, cache2)?;

        // no work - root treeNode bounding boxes are disjoint
        let root_box = |c: &SurfaceCache<O>| -> Result<Extent3d> {
            let tree = c.tree().ok_or_else(|| {
                Failure::new(
                    OP_DI,
                    IntersectFailure::Defect {
                        why: "both caches' trees were built (or found ready) above, so a \
                              cache without one lost the tree it was given",
                    },
                )
                .with_status(SM_ERR_NULL_POINTER)
                .into_error()
            })?;
            let top = tree
                .top_node()
                .ok_or(SolidError::invalid_input(OP_DI, "tree without top node"))?;
            Ok(tree.node(top)?.bbox)
        };
        let root1 = root_box(cache1)?;
        let root2 = root_box(cache2)?;
        let mut bbox = root1;
        bbox.expand_absolute(tol);
        if bbox.are_disjoint(&root2, 0.0) {
            return Ok(result);
        }

        // get bounding box intersection - the region containing a possible
        // intersection (status ignored upstream; passed on as the fork does, bcc6f01)
        bbox = bbox.intersect(&root2)?;
        bbox.expand_absolute(tol);

        // Find node pairs in the tree where intersections might exist
        let mut solutions = SolutionArray::with_capacity(10);
        let request = SolveRequest {
            operation: SolverOperation::IntersectionTest,
            solution_requested: SolutionRequested::All,
            tolerance_3d: tol,
            best_answer_so_far_sq: BIG_DOUBLE,
            opt_vectors: &[],
        };
        self.solve_it(&request, &mut solutions)?;

        // no work - no intersecting leafNode bounding boxes
        let Some(sol) = solutions.get(0) else {
            return Ok(result);
        };
        let s = sol.start.parameters;
        let e = sol.end.parameters;

        // refine the intersection box from the leaf box intersections (status ignored
        // upstream, passed on as the fork does)
        bbox.set_min_max(Vec3::new(s[0], s[1], s[2]), Vec3::new(s[3], s[4], s[5]))?;

        // a single leafNode box intersection - may be a point
        if s[6] < 2.0 {
            let size = bbox.size();

            // count the number of small dimensions: 3 = a point intersection
            let small_count = usize::from(size.x < 2.0 * tol)
                + usize::from(size.y < 2.0 * tol)
                + usize::from(size.z < 2.0 * tol);

            // when any dimension is smaller than 2*tolerance
            if small_count > 0 {
                let uv_domain1 =
                    Extent2d::from_points(Point2::new(e[0], e[1]), Point2::new(e[2], e[3]));
                let uv_domain2 =
                    Extent2d::from_points(Point2::new(e[4], e[5]), Point2::new(e[6], e[7]));

                // resolve the intersection
                let (more_work_required, touching) =
                    self.base.grazing_box_process(k, &uv_domain1, &uv_domain2)?;
                if !more_work_required {
                    if let Some(g) = touching {
                        let [uv1, uv2] = g.uv_curves;
                        result.curves_3d.push(g.curve_3d);
                        result.surface1_uv_curves.push(Some(uv1));
                        result.surface2_uv_curves.push(Some(uv2));
                        result.curve_types.push(g.curve_type);
                        result.deviations.push(g.deviation);
                    }
                    return Ok(result);
                }
            }
        }

        // arrive here when we need to do full intersections
        let mut out = TsectCurves::default();

        // check boundary curve bounding planes for possible disjointness or grazing
        let boundary_test_result = self.base.do_boundary_plane_test(
            k,
            &mut planes1,
            &mut planes2,
            tol,
            &bbox,
            &mut out,
        )?;

        // set output with any graze intersections
        if boundary_test_result == BoundaryPlaneTestResult::Graze && !out.curves_3d.is_empty() {
            let n = out.curves_3d.len();
            result.curves_3d = core::mem::take(&mut out.curves_3d);
            result.curve_types = core::mem::take(&mut out.curve_types);
            result.deviations = core::mem::take(&mut out.deviations);
            result.surface1_uv_curves = vec![None; n];
            result.surface2_uv_curves = vec![None; n];
        }

        // Return if the boundary plane test detected disjointness or grazing
        if matches!(
            boundary_test_result,
            BoundaryPlaneTestResult::Graze | BoundaryPlaneTestResult::Disjoint
        ) {
            return Ok(result);
        }

        // get StartPoints = Surf1->NaturalBoundaryCurve/Surf2 Intersections and
        // Surf1/Surf2->NaturalBoundaryCurve Intersections (on copies of the caches:
        // exact ones, since upstream searches its own caches)
        if !self.base.doing_self_intersection {
            let mut boundary_caches = core::iter::once(first)
                .chain(rest)
                .map(SurfaceCache::clone_for_evaluation)
                .collect::<Result<Vec<_>>>()?;
            // Point testing off so GlobalPointSolve() keeps all point solutions;
            // boundary curve processing on so LocalSolve looks for drop points on
            // boundary curves (upstream sets both on its caches before the first
            // solve; here the boundary solves run in them, the caches keep theirs).
            let modes = SolveModes {
                point_test_enabled: false,
                process_boundary_curves: true,
            };
            self.base.find_boundary_start_points_under(
                k,
                &mut boundary_caches,
                [modes; 2],
                &bbox,
                &mut out,
                queries,
            )?;
        }

        // while there are boundaryCurve/Surface xsect startPoints, use them to trace
        // out intersection curves (each pass pops one; none adds any)
        // (not upstream: one bound on the trace steps of all the start points, HANG-52)
        self.base.set_trace_budget(true);
        while let Some(&start) = self.base.start_points.last() {
            k.check_cancelled(OP_DI)?;
            // keep a copy of the start point (operator= into a fresh element)
            let mut tmp = TsectPnt::zeroed();
            tmp.assign(self.base.point(start)?);
            let tmp_id = self.base.alloc_point(tmp);
            self.base.curve_trace_direction = 1.0;
            let num_curves = out.curves_3d.len();

            // Remove Last StartPoint from m_vStartPoint array
            self.base.start_points.pop();

            // Trace an intersection curve onto m_vCurvePoints.
            let input_trace_direction = self.base.curve_trace_direction;
            let traced_ok = ignored(self.base.trace_curve(k, start, &out.curves_3d))?;
            // (the bound is no status upstream ignores)
            self.base.check_trace_budget()?;
            if traced_ok.is_some() {
                // StartPoint's UVTangent is tangent to the Surface domain boundary:
                // trace the curve in both directions
                let sp = self.base.point(start)?;
                let along = |srf: SrfIndex| {
                    (sp.u_param_type(srf) == ExtentPointType::Inside
                        || is_zero_to_tol(sp.uv_deriv(srf).x, EFF_ZERO))
                        && (sp.v_param_type(srf) == ExtentPointType::Inside
                            || is_zero_to_tol(sp.uv_deriv(srf).y, EFF_ZERO))
                };
                // trace the curve in the opposite direction (not an empty trace, WR-80)
                if !self.base.curve_is_closed
                    && input_trace_direction == self.base.curve_trace_direction
                    && along(SrfIndex::First)
                    && along(SrfIndex::Second)
                {
                    self.base.trace_back(k, start, &out.curves_3d)?;
                }

                // fix (WR-76, #392): a traced curve that ends on a boundary inside
                // a tangential contact consumes the start points of that contact's
                // boundary cluster
                self.base.remove_end_tangent_clusters(k)?;

                // create the curves from the points in m_vCurvePoints
                self.base.flush_curve(k, &mut out)?;
            }

            // when an intersection curve was not generated from this startPoint
            if num_curves == out.curves_3d.len() {
                self.base.through_points.push(tmp_id);
            }
        }

        self.base.set_trace_budget(false);

        // Now that we have found all boundary intersections - look for non boundary
        // intersections (the SmAdvSurfaceIntersector override)
        let mut interior = InteriorCurves {
            curves_3d: core::mem::take(&mut out.curves_3d),
            surface1_uv_curves: core::mem::take(&mut out.uv_curves[0]),
            surface2_uv_curves: core::mem::take(&mut out.uv_curves[1]),
            curve_types: core::mem::take(&mut out.curve_types),
            deviations: core::mem::take(&mut out.deviations),
        };
        self.find_interior_curves(k, &mut interior)?;

        // not upstream (WR-75; the fork's FindTangentCurves): trace the
        // tangential curves the marcher above cannot follow. Not in a self intersection
        // whose two domains overlap or touch (#18930): there every point of the surface solves
        // the tracer's equations with itself at equal parameters (S(x) - S(x) = 0, n = n),
        // and its solves seeded beside a fold land on that trivial solution, so it traced
        // the surface against itself.
        let mut tangent_nodes = Vec::new();
        let seeds = core::mem::take(&mut self.tangent_curve_seeds);
        if !self.trivial_solution_in_both_domains() {
            let size = crate::math::min2(root1.size().length(), root2.size().length());
            let mut curves = TsectCurves {
                curves_3d: core::mem::take(&mut interior.curves_3d),
                uv_curves: [
                    core::mem::take(&mut interior.surface1_uv_curves),
                    core::mem::take(&mut interior.surface2_uv_curves),
                ],
                curve_types: core::mem::take(&mut interior.curve_types),
                deviations: core::mem::take(&mut interior.deviations),
            };
            let traced = super::tangent_curves::find_tangent_curves(
                k,
                &mut self.base,
                size,
                &seeds,
                &mut curves,
            );
            let TsectCurves {
                curves_3d,
                uv_curves: [surface1_uv_curves, surface2_uv_curves],
                curve_types,
                deviations,
            } = curves;
            interior = InteriorCurves {
                curves_3d,
                surface1_uv_curves,
                surface2_uv_curves,
                curve_types,
                deviations,
            };
            tangent_nodes = traced?;
        }
        let InteriorCurves {
            mut curves_3d,
            mut surface1_uv_curves,
            mut surface2_uv_curves,
            mut curve_types,
            mut deviations,
        } = interior;

        // remove degenerate Curves whose points lie on nonDegenerate intersections, then
        // (not upstream) add the touching points left for a start point that no curve
        // left covers
        let mut found = TsectCurves {
            curves_3d: core::mem::take(&mut curves_3d),
            uv_curves: [
                core::mem::take(&mut surface1_uv_curves),
                core::mem::take(&mut surface2_uv_curves),
            ],
            curve_types: core::mem::take(&mut curve_types),
            deviations: core::mem::take(&mut deviations),
        };
        remove_covered_degenerate_curves_then_add_touching(&mut self.base, k, &mut found)?;
        let TsectCurves {
            curves_3d: found_3d,
            uv_curves: [found_uv1, found_uv2],
            curve_types: found_types,
            deviations: found_deviations,
        } = found;
        curves_3d = found_3d;
        surface1_uv_curves = found_uv1;
        surface2_uv_curves = found_uv2;
        curve_types = found_types;
        deviations = found_deviations;

        // not upstream (WR-75; the fork's AddTangentNodes): report the singular points
        // where crossing branches meet
        super::tangent_curves::add_tangent_nodes(
            k,
            &self.base,
            &tangent_nodes,
            &mut curves_3d,
            &mut surface1_uv_curves,
            &mut surface2_uv_curves,
            &mut curve_types,
            &mut deviations,
        )?;

        // not upstream (WR-77; the fork's sm_SplitCurvesAtSingularPoints): split crossing
        // curves at singular points (parallel normals) they pass through
        if !self.base.doing_self_intersection {
            super::singular_splits::split_curves_at_singular_points(
                k,
                self.base.surface(SrfIndex::First),
                self.base.surface(SrfIndex::Second),
                self.base.this_approx_tol_3d,
                &mut curves_3d,
                &mut surface1_uv_curves,
                &mut surface2_uv_curves,
                &mut curve_types,
                &mut deviations,
            )?;
        }

        // not upstream (#18117): a self intersection drops the curves that are the
        // surface's own closure, its seam's two sides meeting, and keeps every other
        if self.base.doing_self_intersection {
            let mut kept = TsectCurves {
                curves_3d: Vec::new(),
                uv_curves: [Vec::new(), Vec::new()],
                curve_types: Vec::new(),
                deviations: Vec::new(),
            };
            if curves_3d.len() != surface1_uv_curves.len()
                || curves_3d.len() != surface2_uv_curves.len()
                || curves_3d.len() != curve_types.len()
                || curves_3d.len() != deviations.len()
            {
                return Err(SolidError::invalid_input(
                    OP_DI,
                    "intersection curve arrays of different lengths",
                ));
            }
            for ((((c, uv1), uv2), t), d) in curves_3d
                .into_iter()
                .zip(surface1_uv_curves)
                .zip(surface2_uv_curves)
                .zip(curve_types)
                .zip(deviations)
            {
                if is_closure_curve(&self.base, &uv1, &uv2)? {
                    continue;
                }
                kept.curves_3d.push(c);
                kept.uv_curves[0].push(uv1);
                kept.uv_curves[1].push(uv2);
                kept.curve_types.push(t);
                kept.deviations.push(d);
            }
            let TsectCurves {
                curves_3d: c,
                uv_curves: [u1, u2],
                curve_types: t,
                deviations: d,
            } = kept;
            curves_3d = c;
            surface1_uv_curves = u1;
            surface2_uv_curves = u2;
            curve_types = t;
            deviations = d;
        }

        // set outputs
        result.curves_3d.extend(curves_3d);
        result
            .surface1_uv_curves
            .extend(surface1_uv_curves.into_iter().map(Some));
        result
            .surface2_uv_curves
            .extend(surface2_uv_curves.into_iter().map(Some));
        result.deviations.extend(deviations);
        result.curve_types.extend(curve_types);
        Ok(result)
    }
}

/// Not upstream (#18117): whether the curve of a self intersection with the UV curves
/// `uv1` and `uv2` is the surface's own closure, proved for the whole curve from its UV
/// curves alone. In a direction the surface closes in over the intersected domain
/// (`m_bClosedU`, `m_bClosedV`), one UV curve is the iso-line at that direction's one end
/// and the other the iso-line at its other end, and at every curve parameter the two have
/// the same parameter across: the two sides of the seam at one point of it. The closure is
/// where a closed surface meets itself by being closed, no self intersection; upstream
/// reports a closed surface's seam as one, so a closed offset surface failed its own
/// validity test.
///
/// The proof is the B-splines' convex hull: the two UV curves are B-splines with the same
/// degree, knots and weights (the trace builds both on the curve's knots), so every point
/// of each is a convex combination, with the same coefficients, of its control points. When
/// every control point of the one lies on its end, every control point of the other on the
/// other end and each pair has the same parameter across, so does every point of the
/// curves. Each comparison allows only the rounding of the traced UV points
/// ([`uv_rounding`]). 3D positions decide nothing: a crossing whose points lie on the seam
/// in space, its branches elsewhere in the domain, is kept, and so is a curve that touches
/// the seam only at some points, or one whose UV curves are not such B-splines.
///
/// # Errors
///
/// Those of the control polygons.
fn is_closure_curve(si: &SurfaceIntersector<'_>, uv1: &Curve, uv2: &Curve) -> Result<bool> {
    let (CurveKind::BSpline(c1), CurveKind::BSpline(c2)) = (uv1.kind(), uv2.kind()) else {
        return Ok(false);
    };
    if c1.degree()? != c2.degree()? || c1.knots(None)? != c2.knots(None)? {
        return Ok(false);
    }
    let ((p1, w1), (p2, w2)) = (c1.control_polygon()?, c2.control_polygon()?);
    if p1.len() != p2.len() || p1.is_empty() || w1 != w2 || w1.iter().any(|&w| w <= 0.0) {
        return Ok(false);
    }
    let dom = si.uv_domains[0];
    let tol = uv_rounding(&dom);
    let same = |a: f64, b: f64| (a - b).abs() <= tol;
    for (closed, along_u, lo, hi) in [
        (si.closed_u[0], true, dom.u_min(), dom.u_max()),
        (si.closed_v[0], false, dom.v_min(), dom.v_max()),
    ] {
        if !closed {
            continue;
        }
        // (the parameter in the closing direction, the parameter across)
        let split = |p: &Point3| if along_u { (p.x, p.y) } else { (p.y, p.x) };
        for (end1, end2) in [(lo, hi), (hi, lo)] {
            let closure = p1.iter().zip(&p2).all(|(a, b)| {
                let ((at_a, across_a), (at_b, across_b)) = (split(a), split(b));
                same(at_a, end1) && same(at_b, end2) && same(across_a, across_b)
            });
            if closure {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The rounding of the traced UV points in the domain `dom`: `8 gamma(16)` of its largest
/// coordinate (at least 1), the bound the boundary search takes for its chord distances. A traced
/// UV curve's control points are its points plus a third of their scaled tangents, a few
/// rounded operations on numbers of the domain's size, so a curve that is an iso-line in
/// exact arithmetic is one up to this; nothing geometric is decided by it.
fn uv_rounding(dom: &Extent2d) -> f64 {
    let size = [dom.u_min(), dom.u_max(), dom.v_min(), dom.v_max()]
        .iter()
        .fold(1.0f64, |m, c| m.max(c.abs()));
    8.0 * gamma(16) * size
}

/// `SmSurface::Evaluate1stDerivatives(uv, TRUE, TRUE, ..)`.
fn evaluate_1st_derivatives(
    k: &Kernel,
    surface: &Surface,
    uv: Point2,
) -> Result<(Point3, Vec3, Vec3)> {
    crate::surface::surface::p1::evaluate_1st_derivatives(k, surface, uv, Side::Left, Side::Left)
}

/// The end of `DoIntersection`'s curve list: removes each degenerate curve of `found`
/// that lies on another of its curves (upstream), then adds the touching points that
/// `FindBoundaryStartPoints` left for a start point and that no remaining curve covers
/// (not upstream, [`SurfaceIntersector::add_deferred_touching_points`]). The removal
/// comes first so that a degenerate curve it drops covers no touching point.
///
/// # Errors
///
/// Those of [`SurfaceIntersector::is_point_on_curve`] and of the curves; curve arrays
/// of different lengths are invalid input.
fn remove_covered_degenerate_curves_then_add_touching(
    base: &mut SurfaceIntersector<'_>,
    k: &Kernel,
    found: &mut TsectCurves,
) -> Result<()> {
    let TsectCurves {
        curves_3d,
        uv_curves: [surface1_uv_curves, surface2_uv_curves],
        curve_types,
        deviations,
    } = found;
    let mut ii = 0;
    while let Some(curve) = curves_3d.get(ii) {
        if curve.is_degenerate(k, EFF_ZERO, None)? {
            // status ignored upstream, an unset point (passed on as the fork does,
            // bcc6f01)
            let point = curve.evaluate_point(k, 0.0)?;
            if base.is_point_on_curve(k, point, curves_3d, Some(ii))? {
                if ii >= surface1_uv_curves.len()
                    || ii >= surface2_uv_curves.len()
                    || ii >= deviations.len()
                    || ii >= curve_types.len()
                {
                    return Err(SolidError::invalid_input(
                        OP_DI,
                        "curve arrays of different lengths (upstream reads past them)",
                    ));
                }
                curves_3d.remove(ii);
                surface1_uv_curves.remove(ii);
                surface2_uv_curves.remove(ii);
                deviations.remove(ii);
                curve_types.remove(ii);
                continue;
            }
        }
        ii += 1;
    }
    base.add_deferred_touching_points(k, found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SM_ERR_NULL_POINTER;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::intersect::adv_surface_intersector::NoSurfaceHandle;
    use crate::intersect::surface_intersector::p4::MAX_START_LOOP_TRACE_STEPS;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::surface::plane::Plane;
    use crate::surface::sphere::Sphere;

    fn plane(o: [f64; 3], x: [f64; 3], y: [f64; 3]) -> Surface {
        Surface::from(Plane::new(
            Vec3::from_array(o),
            Vec3::from_array(x),
            Vec3::from_array(y),
            Point2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ))
    }

    fn unit() -> Extent2d {
        Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0)
    }

    fn corner(u: f64, v: f64) -> Extent2d {
        Extent2d::from_bounds(u - 0.1, v - 0.1, u + 0.1, v + 0.1)
    }

    #[test]
    fn grazing_corner_leaving_the_other_surface_is_a_touching_point() {
        let k = &Kernel::default();
        // B starts at A's corner (1, 1, 0) and leaves A (along (1, 1, 0) and z)
        let s = 0.5_f64.sqrt();
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([1.0, 1.0, 0.0], [s, s, 0.0], [0.0, 0.0, 1.0]);
        let si = SurfaceIntersector::new(k, &a, &unit(), &b, &unit(), false).unwrap();
        let (more, touching) = si
            .grazing_box_process(k, &corner(1.0, 1.0), &corner(0.0, 0.0))
            .unwrap();
        assert!(!more);
        let g = touching.unwrap();
        assert_eq!(g.curve_type, TsectCurveType::Touching);
        assert_eq!(g.deviation, 0.0);
        let p = g.curve_3d.evaluate_point(k, 0.5).unwrap();
        assert!(p.distance(Vec3::new(1.0, 1.0, 0.0)) < 1e-15);
        let uv2 = g.uv_curves[1].evaluate_point(k, 0.0).unwrap();
        assert!(uv2.distance(Vec3::ZERO) < 1e-15);
    }

    #[test]
    fn grazing_corner_entering_the_other_surface_needs_more_work() {
        let k = &Kernel::default();
        // B's u axis runs back over A from its corner
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([1.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let si = SurfaceIntersector::new(k, &a, &unit(), &b, &unit(), false).unwrap();
        let (more, touching) = si
            .grazing_box_process(k, &corner(1.0, 1.0), &corner(0.0, 0.0))
            .unwrap();
        assert!(more);
        assert!(touching.is_none());
    }

    #[test]
    fn grazing_without_coincident_corners_needs_more_work() {
        let k = &Kernel::default();
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let si = SurfaceIntersector::new(k, &a, &unit(), &b, &unit(), false).unwrap();
        // corners in both domains, 1 apart
        let (more, touching) = si.grazing_box_process(k, &unit(), &unit()).unwrap();
        assert!(more);
        assert!(touching.is_none());
        // no corner inside the intersection domains
        let inner = Extent2d::from_bounds(0.4, 0.4, 0.6, 0.6);
        let (more, _) = si.grazing_box_process(k, &inner, &inner).unwrap();
        assert!(more);
    }

    /// Two plane patches whose corners pass 9.9 apart (they do not cross: the line
    /// where their planes meet leaves A at t = -23158.91 and enters B only up to
    /// t = -23169.29), intersected with a tolerance of 2785 (goldens
    /// `intersect.surface_intersector_p5` gen0236). The boundary search finds a
    /// touching point at A's corner and a start point within the tolerance of it,
    /// which traces no curve. The touching point used to be dropped for that start
    /// point, leaving no answer; it is now added at the end, since no curve found
    /// covers it (WR-51 made the start point test a real distance of the tolerance,
    /// which reaches this start point).
    /// A touching point left for a start point is checked against the curves that
    /// remain once the degenerate curves lying on other curves are removed. Here the
    /// touching point P is covered only by a degenerate curve D (within 4 tol of it).
    /// D lies on the line C (0.9 tol past its end) and is removed. P, on the planes'
    /// meet 3 tol past C's end, is on no curve left (its surface gaps are all zero, so
    /// the mid-gap test does not join it to C) and is the contact's only report.
    /// Checked against the curves before the removal, P was dropped together with D.
    #[test]
    fn a_touching_point_covered_only_by_a_removed_degenerate_curve_is_kept() {
        use super::super::p3::{push_touching_curve, DeferredTouching};
        let k = &Kernel::default();
        // z = 0 and y = 0.5: they meet along x
        let a = plane([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut si = SurfaceIntersector::new(k, &a, &d, &b, &d, false).unwrap();
        let tol = si.this_approx_tol_3d();
        assert!(tol > 0.0 && tol < 0.01, "{tol}");
        let touching = |p: Point3, uv: [f64; 2]| {
            let mut t = TsectPnt::new();
            *t.srf_pos_mut(SrfIndex::First) = p;
            *t.srf_pos_mut(SrfIndex::Second) = p;
            *t.uv_pos_mut(SrfIndex::First) = Point2::new(uv[0], uv[1]);
            *t.uv_pos_mut(SrfIndex::Second) = Point2::new(uv[0], uv[1]);
            t
        };
        let c_end = Vec3::new(0.5, 0.5, 0.0);
        let dp = c_end + Vec3::new(0.9 * tol, 0.0, 0.0);
        let pp = c_end + Vec3::new(3.0 * tol, 0.0, 0.0);
        let mut found = TsectCurves::default();
        let line = |dim, p: Point3, q: Point3| {
            Curve::from(BSplineCurve::create_line_segment(dim, p, q).unwrap())
        };
        found
            .curves_3d
            .push(line(3, Vec3::new(0.1, 0.5, 0.0), c_end));
        found.uv_curves[0].push(line(2, Vec3::new(0.1, 0.5, 0.0), Vec3::new(0.5, 0.5, 0.0)));
        found.uv_curves[1].push(line(2, Vec3::new(0.1, 0.5, 0.0), Vec3::new(0.5, 0.5, 0.0)));
        found.curve_types.push(TsectCurveType::Crossing);
        found.deviations.push(0.0);
        push_touching_curve(&mut found, dp, &touching(dp, [dp.x, dp.y])).unwrap();
        // P's boundary curve: A's iso line u = P.x across C, which C does not run
        // along, so only the removal of D decides P (not the #11749 crossing rule)
        let boundary = line(3, Vec3::new(pp.x, 0.0, 0.0), Vec3::new(pp.x, 1.0, 0.0));
        si.deferred_touching = vec![DeferredTouching::new(
            pp,
            touching(pp, [pp.x, pp.y]),
            boundary,
        )];
        // the premises: D is on C, P is on D and not on C
        assert!(si
            .is_point_on_curve(k, dp, &found.curves_3d[..1], None)
            .unwrap());
        assert!(pp.distance(dp) < 4.0 * tol);
        assert!(!si
            .is_point_on_curve(k, pp, &found.curves_3d[..1], None)
            .unwrap());
        assert!(si.is_point_on_curve(k, pp, &found.curves_3d, None).unwrap());

        remove_covered_degenerate_curves_then_add_touching(&mut si, k, &mut found).unwrap();
        assert!(si.deferred_touching.is_empty());
        assert_eq!(found.curves_3d.len(), 2);
        assert_eq!(
            found.curve_types,
            [TsectCurveType::Crossing, TsectCurveType::Touching]
        );
        assert_eq!(found.curves_3d[1].evaluate_point(k, 0.0).unwrap(), pp);
        assert_eq!(found.uv_curves[0].len(), 2);
        assert_eq!(found.uv_curves[1].len(), 2);
        assert_eq!(found.deviations, [0.0, 0.0]);
    }

    #[test]
    fn a_touching_point_no_curve_covers_is_kept() {
        let k = &Kernel::default();
        let la = 17_328.120_691_213_055;
        let lb = 11_527.965_487_154_615;
        let oa = [
            -534.257_546_242_803_1,
            3_609.235_629_132_970_6,
            14_227.419_314_424_104,
        ];
        let xa = [
            -0.996_769_538_633_399_7,
            -0.073_244_498_176_162_2,
            -0.032_950_422_447_696_89,
        ];
        let ya = [
            -0.073_610_447_371_212_76,
            0.669_038_560_874_377_2,
            0.739_573_462_274_676_4,
        ];
        let ob = [
            -27_685.723_579_420_985,
            102.485_228_779_159_39,
            26_996.495_008_515_747,
        ];
        let xb = [
            0.967_335_014_604_463_7,
            0.228_281_315_927_273_74,
            -0.110_229_806_852_292_79,
        ];
        let yb = [
            -0.222_193_158_638_955_42,
            0.972_844_457_931_221_4,
            0.064_837_187_836_549_17,
        ];
        let patch = |o, x, y, l: f64| {
            Surface::from(Plane::new(
                Vec3::from_array(o),
                Vec3::from_array(x),
                Vec3::from_array(y),
                Point2::new(1.0, 1.0),
                &Extent2d::from_bounds(0.0, 0.0, l, l),
            ))
        };
        let (a, b) = (patch(oa, xa, ya, la), patch(ob, xb, yb, lb));
        let (da, db) = (
            Extent2d::from_bounds(0.0, 0.0, la, la),
            Extent2d::from_bounds(0.0, 0.0, lb, lb),
        );
        let tol = 2_785.330_913_332_592_4;
        let r = do_intersection(k, &a, &da, &b, &db, [false, true], Some(tol), None).unwrap();

        assert_eq!(r.curves_3d.len(), 1, "{r:?}");
        assert_eq!(r.curve_types, vec![TsectCurveType::Touching]);
        let c3 = r.curves_3d.first().unwrap();
        assert!(c3.is_degenerate(k, EFF_ZERO, None).unwrap());
        let p = c3.evaluate_point(k, 0.0).unwrap();
        let uv1 = r.surface1_uv_curves[0]
            .as_ref()
            .unwrap()
            .evaluate_point(k, 0.0)
            .unwrap();
        let uv2 = r.surface2_uv_curves[0]
            .as_ref()
            .unwrap()
            .evaluate_point(k, 0.0)
            .unwrap();
        // A's corner (la, la), on B's edge u = lb, inside B's v range
        assert_eq!((uv1.x, uv1.y), (la, la));
        assert_eq!(uv2.x, lb);
        assert!(uv2.y > 0.0 && uv2.y < lb);
        // The point is midway between its two surface points, which are its UV
        // points on A and B; its deviation is their distance, and it lies within
        // the tolerance of both patches.
        let on = |o: [f64; 3], x: [f64; 3], y: [f64; 3], uv: Vec3| {
            Vec3::from_array(o) + Vec3::from_array(x) * uv.x + Vec3::from_array(y) * uv.y
        };
        let (pa, pb) = (on(oa, xa, ya, uv1), on(ob, xb, yb, uv2));
        let dev = *r.deviations.first().unwrap();
        assert!((dev - pa.distance(pb)).abs() <= 1e-9 * dev);
        // never closer than the patches' 9.9 separation, and far inside the tolerance
        assert!(dev > 9.9 && dev < 11.0, "{dev}");
        assert!(p.distance(pa) <= 0.5 * dev * (1.0 + 1e-9));
        assert!(p.distance(pb) <= 0.5 * dev * (1.0 + 1e-9));
        assert!(p.distance(pa) < tol && p.distance(pb) < tol);
    }

    #[test]
    fn no_cache_is_a_null_pointer() {
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let k = Kernel::default();
        let mut ssi: AdvSurfaceIntersector<'_, '_, NoSurfaceHandle> =
            AdvSurfaceIntersector::new(&k, &a, &unit(), &b, &unit(), false).unwrap();
        let mut caches: Vec<SurfaceCache<NoSurfaceHandle>> = Vec::new();
        let e = ssi
            .do_intersection_with_caches(&k, &mut caches, Some(0.5), Some(0.1))
            .unwrap_err();
        assert_eq!(
            e.intersect_failure(),
            Some(&IntersectFailure::NoSurfaceCaches)
        );
        assert_eq!(e.failed_routine_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput);
        // the tolerances are set first
        assert_eq!(ssi.base.this_approx_tol_3d, 0.5);
        assert_eq!(ssi.base.this_ang_tol_rad, 0.1);
    }

    /// [`do_intersection`] of a unit sphere with the plane z = 0.5 (the surfaces of
    /// `tangent_tracer`'s tests), the start-point loop bounded to `steps` trace steps.
    fn sphere_cut_with_trace_steps(k: &Kernel, steps: u64) -> Result<SurfaceIntersection> {
        let sphere =
            Surface::from(Sphere::create_canonical(k, &Axis2Placement::default(), 1.0).unwrap());
        let plane = Surface::from(
            BSplineSurface::create_bilinear_surface(
                Vec3::new(-1.0, -1.0, 0.5),
                Vec3::new(1.0, -1.0, 0.5),
                Vec3::new(-1.0, 1.0, 0.5),
                Vec3::new(1.0, 1.0, 0.5),
            )
            .unwrap(),
        );
        let d1 = sphere.natural_uv_domain_with_nurb(k).unwrap();
        let d2 = plane.natural_uv_domain_with_nurb(k).unwrap();
        // as `do_intersection`, with the bound lowered
        let mut caches: Vec<SurfaceCache<IntersectionSurface>>;
        let mut ssi: AdvSurfaceIntersector<'_, '_, IntersectionSurface> =
            AdvSurfaceIntersector::new(k, &sphere, &d1, &plane, &d2, false).unwrap();
        ssi.base.start_loop_trace_steps = steps;
        caches = intersection_surface_caches(
            k,
            &ssi.base,
            [IntersectionSurface::First, IntersectionSurface::Second],
        )
        .unwrap();
        ssi.do_intersection_with_caches(k, &mut caches, None, None)
    }

    /// The passed-on statuses (WR-662 addendum, fork bcc6f01): with no failing status
    /// the passed-on sites (the boundary intervals and trims, the box intersection, the
    /// start and through points, the degenerate-curve point) refuse nothing. Coverage
    /// only: a sphere cut inside the plane's domain and two planes crossing at their
    /// domains' boundaries give curves.
    #[test]
    fn intersections_are_the_same_with_statuses_passed_on() {
        let sphere_k = Kernel::default();
        let sphere = Surface::from(
            Sphere::create_canonical(&sphere_k, &Axis2Placement::default(), 1.0).unwrap(),
        );
        let cut = Surface::from(
            BSplineSurface::create_bilinear_surface(
                Vec3::new(-1.0, -1.0, 0.5),
                Vec3::new(1.0, -1.0, 0.5),
                Vec3::new(-1.0, 1.0, 0.5),
                Vec3::new(1.0, 1.0, 0.5),
            )
            .unwrap(),
        );
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let run = |s1: &Surface, s2: &Surface| {
            let k = Kernel::default();
            let d1 = s1.natural_uv_domain_with_nurb(&k).unwrap();
            let d2 = s2.natural_uv_domain_with_nurb(&k).unwrap();
            let r = do_intersection(&k, s1, &d1, s2, &d2, [true, true], None, None).unwrap();
            r.curves_3d
                .iter()
                .map(|c| {
                    let ivl = c.natural_interval();
                    (
                        c.evaluate_point(&k, ivl.min()).unwrap(),
                        c.evaluate_point(&k, ivl.max()).unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        };
        for (s1, s2) in [(&sphere, &cut), (&a, &b)] {
            assert!(!run(s1, s2).is_empty());
        }
    }

    /// HANG-52 through `DoIntersection`: the start-point loop's traces are bounded, the
    /// bound's refusal fails the intersection with the too-many error, and the real
    /// bound changes no curve.
    #[test]
    fn do_intersection_bounds_its_start_point_loop_traces() {
        let k = Kernel::default();
        let on = sphere_cut_with_trace_steps(&k, MAX_START_LOOP_TRACE_STEPS).unwrap();
        assert!(!on.curves_3d.is_empty());

        assert!(matches!(
            sphere_cut_with_trace_steps(&k, 1),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    /// A cache context whose cache preparation fails (as a caller's identity
    /// resolution can).
    struct FailingContext;

    impl SurfaceSolveContext<NoSurfaceHandle> for FailingContext {
        fn build_surface_cache(
            &mut self,
            _k: &Kernel,
            _surface: &Surface,
            _handle: Option<NoSurfaceHandle>,
        ) -> Result<SurfaceCache<NoSurfaceHandle>> {
            Err(SolidError::invalid_input("test::identity", "no such face"))
        }
        fn face_queries(&self) -> Option<&dyn FaceQueries<NoSurfaceHandle>> {
            None
        }
    }

    #[test]
    fn intersection_caches_keep_the_context_error_as_their_cause() {
        let k = Kernel::default();
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let si = SurfaceIntersector::new(&k, &a, &unit(), &b, &unit(), false).unwrap();
        let e = intersection_surface_caches_with_context(
            &k,
            &si,
            [Some(NoSurfaceHandle), Some(NoSurfaceHandle)],
            &mut FailingContext,
        )
        .unwrap_err();
        assert_eq!(
            e.intersect_failure(),
            Some(&IntersectFailure::SurfaceCacheNotBuilt)
        );
        assert_eq!(e.failed_routine_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(
            e.cause(),
            Some(&SolidError::invalid_input("test::identity", "no such face"))
        );
    }

    #[test]
    fn do_intersection_keeps_the_tree_build_error_as_its_cause() {
        let k = Kernel::default();
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let mut ssi: AdvSurfaceIntersector<'_, '_, NoSurfaceHandle> =
            AdvSurfaceIntersector::new(&k, &a, &unit(), &b, &unit(), false).unwrap();
        // a B-spline surface with no NURB: its tree cannot be built
        let empty = Surface::from(BSplineSurface::new());
        let mut cache: SurfaceCache<NoSurfaceHandle> = SurfaceCache::new(
            empty.clone(),
            unit(),
            crate::surface::surface_cache::SurfaceCacheOptions::default(),
        );
        let built = cache.get_tree(&k).map(|_| ()).unwrap_err();
        let mut caches = vec![
            SurfaceCache::new(
                empty.clone(),
                unit(),
                crate::surface::surface_cache::SurfaceCacheOptions::default(),
            ),
            SurfaceCache::new(
                empty,
                unit(),
                crate::surface::surface_cache::SurfaceCacheOptions::default(),
            ),
        ];
        let e = ssi
            .do_intersection_with_caches(&k, &mut caches, Some(0.5), Some(0.1))
            .unwrap_err();
        assert_eq!(
            e.intersect_failure(),
            Some(&IntersectFailure::SurfaceCacheNotBuilt)
        );
        assert_eq!(e.failed_routine_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(e.cause(), Some(&built));
    }

    /// A ribbon closed round a figure eight (#18117): a vertical segment of height 2 swept
    /// along the curve (10 sin t, 10 sin t cos t) from `t = start` round once, closed on a seam
    /// at that point; the eight crosses itself at the origin.
    fn figure_eight_ribbon(start: f64) -> Surface {
        use crate::nurbs::crv_fit::p1::ParamMethod;
        let mut points: Vec<Point3> = (0..16)
            .map(|i| {
                let t = start + std::f64::consts::TAU * f64::from(i) / 16.0;
                Point3::new(10.0 * t.sin(), 10.0 * t.sin() * t.cos(), 0.0)
            })
            .collect();
        let first = points.first().copied().unwrap();
        points.push(first);
        let path = BSplineCurve::interpolate_points(
            &points,
            None,
            3,
            None,
            None,
            true,
            ParamMethod::ChordLength,
        )
        .unwrap();
        let segment = BSplineCurve::interpolate_points(
            &[Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 2.0)],
            None,
            1,
            None,
            None,
            false,
            ParamMethod::ChordLength,
        )
        .unwrap();
        Surface::from(
            BSplineSurface::create_sweep_surface(&segment, &path, None, true, 1e-4).unwrap(),
        )
    }

    /// Whether `c` is a vertical line (one x and y along it) through a point within `r` of
    /// the axis.
    fn vertical_line_near_axis(k: &Kernel, c: &Curve, r: f64) -> bool {
        let iv = c.natural_interval();
        let p: Vec<Point3> = [0.0, 0.5, 1.0]
            .iter()
            .map(|&f| c.evaluate_point(k, iv.evaluate(f)).unwrap())
            .collect();
        let first = p.first().copied().unwrap();
        p.iter()
            .all(|q| (q.x - first.x).abs() < 1e-3 && (q.y - first.y).abs() < 1e-3)
            && first.x.hypot(first.y) < r
    }

    /// A closed ribbon (#18117): the offset by 0.5 of the figure-eight ribbon closed a quarter
    /// turn from its crossing. Its self intersection is the one vertical line where the ribbon
    /// crosses itself; the seam, where it meets itself by being closed, is no self
    /// intersection.
    #[test]
    fn a_closed_ribbon_crosses_itself_once_and_its_seam_is_no_crossing() {
        use crate::nurbs::b_spline_surface::p1::SurfParamType;
        let k = Kernel {
            fixes: crate::kernel::UpstreamFixes::all(),
            ..Default::default()
        };
        // seam at (10, 0), a quarter turn from the crossing at the origin
        let swept = figure_eight_ribbon(std::f64::consts::FRAC_PI_2);
        // (an offset surface: the kind whose validity test asks its self intersection)
        let ribbon = swept.create_offset_surface(&k, 0.5, 1e-4).unwrap().unwrap();
        assert!(matches!(
            ribbon.kind(),
            crate::surface::surface::SurfaceKind::OffsetSurface(_)
        ));
        let dom = ribbon.natural_uv_domain_with_nurb(&k).unwrap();
        let closed = |p| ribbon.is_closed(&k, &dom, p, None, false).unwrap().0;
        assert!(
            closed(SurfParamType::U) || closed(SurfParamType::V),
            "the ribbon closes"
        );
        let x = ribbon
            .global_surface_intersect(&k, &dom, &ribbon, &dom, [false, false], None, None)
            .unwrap();
        assert_eq!(x.curves_3d.len(), 1, "{:?}", x.curve_types);
        assert_eq!(x.curve_types.first(), Some(&TsectCurveType::Crossing));
        assert!(
            vertical_line_near_axis(&k, x.curves_3d.first().unwrap(), 5.0),
            "the one curve is the crossing"
        );
    }

    /// The figure-eight ribbon closed at its crossing (#18117): there the ribbon's two
    /// branches cross on the seam line itself, so every point of the crossing is a point of the
    /// seam in space. It is still a crossing (one branch on the seam, the other halfway round)
    /// and is kept; only the seam's two sides meeting is no self intersection.
    #[test]
    fn a_crossing_on_the_seam_line_is_kept() {
        let k = Kernel {
            fixes: crate::kernel::UpstreamFixes::all(),
            ..Default::default()
        };
        let ribbon = figure_eight_ribbon(0.0);
        let dom = ribbon.natural_uv_domain_with_nurb(&k).unwrap();
        let x = ribbon
            .global_surface_intersect(&k, &dom, &ribbon, &dom, [false, false], None, None)
            .unwrap();
        assert!(!x.curves_3d.is_empty(), "the crossing is kept");
        for (c, t) in x.curves_3d.iter().zip(&x.curve_types) {
            assert_eq!(*t, TsectCurveType::Crossing);
            assert!(
                vertical_line_near_axis(&k, c, 1e-3),
                "the crossing at the origin"
            );
        }
    }

    /// A curve that touches the seam only at some points is no closure (#18117): its first
    /// UV curve is on the seam's low side at its ends and at a quarter, half and three
    /// quarters along, and off it between; its second runs along the seam's high side.
    #[test]
    fn a_curve_touching_the_seam_at_points_is_no_closure() {
        use crate::nurbs::crv_fit::p1::ParamMethod;
        let k = Kernel::default();
        let ribbon = figure_eight_ribbon(std::f64::consts::FRAC_PI_2);
        let dom = ribbon.natural_uv_domain_with_nurb(&k).unwrap();
        let si = SurfaceIntersector::new(&k, &ribbon, &dom, &ribbon, &dom, false).unwrap();
        assert!(si.doing_self_intersection);
        assert!(si.closed_v[0] && !si.closed_u[0], "the ribbon closes in v");
        let (lo, hi) = (dom.v_min(), dom.v_max());
        // polylines through equally spaced points (uniform parameters): along u, the first
        // at v = lo on every other point and 0.05 above it between
        let polyline = |v: &dyn Fn(u32) -> f64| {
            let points: Vec<Point3> = (0..9u32)
                .map(|i| {
                    let u = dom.u_min() + (dom.u_max() - dom.u_min()) * f64::from(i) / 8.0;
                    Point3::new(u, v(i), 0.0)
                })
                .collect();
            Curve::from(
                BSplineCurve::interpolate_points(
                    &points,
                    None,
                    1,
                    None,
                    None,
                    false,
                    ParamMethod::Uniform,
                )
                .unwrap(),
            )
        };
        let touching = polyline(&|i| if i % 2 == 0 { lo } else { lo + 0.05 });
        let along_seam = polyline(&|_| hi);
        let seam = polyline(&|_| lo);
        // at the five points the UV curves are the seam's two sides
        let iv = touching.natural_interval();
        for f in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let p = touching.evaluate_point(&k, iv.evaluate(f)).unwrap();
            assert!((p.y - lo).abs() < 1e-12, "{f}: {p:?}");
        }
        assert!(!is_closure_curve(&si, &touching, &along_seam).unwrap());
        // the seam itself is the closure
        assert!(is_closure_curve(&si, &seam, &along_seam).unwrap());
    }

    /// A ribbon folded back on itself (#18930): a vertical segment of height 2 swept along the
    /// offset by 2 towards its centres of curvature of the parabola y = x^2 / 2 over
    /// [-2, 2] (101 points), a swallowtail: past the apex's radius 1 the offset has two cusps
    /// and crosses itself where 1 + x^2 = 4, at (0, 2.5).
    fn swallowtail_ribbon() -> Surface {
        use crate::nurbs::crv_fit::p1::ParamMethod;
        let points: Vec<Point3> = (0..101)
            .map(|i| {
                let x = -2.0 + 4.0 * f64::from(i) / 100.0;
                let l = (1.0 + x * x).sqrt();
                Point3::new(x - 2.0 * x / l, x * x / 2.0 + 2.0 / l, 0.0)
            })
            .collect();
        let path = BSplineCurve::interpolate_points(
            &points,
            None,
            3,
            None,
            None,
            false,
            ParamMethod::ChordLength,
        )
        .unwrap();
        let segment = BSplineCurve::interpolate_points(
            &[Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 2.0)],
            None,
            1,
            None,
            None,
            false,
            ParamMethod::ChordLength,
        )
        .unwrap();
        Surface::from(
            BSplineSurface::create_sweep_surface(&segment, &path, None, false, 1e-4).unwrap(),
        )
    }

    /// A surface folded back on itself (#18930): the swallowtail ribbon's self intersection is
    /// the one vertical line where it crosses itself, at (0, 2.5), over the ribbon's height
    /// 0..2. A self intersection once also returned six tangent curves: seeded beside the two
    /// cusp lines, where the ribbon turns back and its sheets meet at equal parameters, the
    /// tangential curve pass traced the surface against itself.
    #[test]
    fn a_folded_ribbon_crosses_itself_once_with_the_tangent_curve_tracer() {
        let ribbon = swallowtail_ribbon();
        let k = Kernel::default();
        let dom = ribbon.natural_uv_domain_with_nurb(&k).unwrap();
        let x = ribbon
            .global_surface_intersect(&k, &dom, &ribbon, &dom, [false, false], None, None)
            .unwrap();
        assert_eq!(x.curve_types, [TsectCurveType::Crossing]);
        let c = x.curves_3d.first().unwrap();
        let iv = c.natural_interval();
        let mut z = Vec::new();
        for f in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let p = c.evaluate_point(&k, iv.evaluate(f)).unwrap();
            assert!(p.x.abs() < 1e-3 && (p.y - 2.5).abs() < 1e-3, "{f}: {p:?}");
            z.push(p.z);
        }
        // end to end over the height, monotone
        let (z0, z1) = (*z.first().unwrap(), *z.last().unwrap());
        assert!(
            z0.min(z1).abs() < 1e-4 && (z0.max(z1) - 2.0).abs() < 1e-4,
            "{z:?}"
        );
        assert!(
            z.windows(2)
                .all(|w| matches!(w, [a, b] if (b - a) * (z1 - z0) > 0.0)),
            "{z:?}"
        );
    }

    /// Where a self intersection's trivial solution lies (#18930): two domains of one surface
    /// that are the same, overlap or touch both hold the points the surface shares with itself
    /// at equal parameters, so the tangential curve pass does not run; two disjoint domains of
    /// one surface, and two surfaces over the same domain, hold none, so it runs.
    #[test]
    fn a_self_intersection_meets_its_trivial_solution_where_its_domains_overlap_or_touch() {
        let k = Kernel::default();
        let ribbon = swallowtail_ribbon();
        let copy = swallowtail_ribbon();
        let dom = ribbon.natural_uv_domain_with_nurb(&k).unwrap();
        let (u0, u1, v0, v1) = (dom.u_min(), dom.u_max(), dom.v_min(), dom.v_max());
        let part =
            |a: f64, b: f64| Extent2d::from_bounds(u0 + a * (u1 - u0), v0, u0 + b * (u1 - u0), v1);
        let meets = |other: &Surface, d1: &Extent2d, d2: &Extent2d| {
            AdvSurfaceIntersector::<'_, '_, NoSurfaceHandle>::new(&k, &ribbon, d1, other, d2, false)
                .unwrap()
                .trivial_solution_in_both_domains()
        };
        assert!(meets(&ribbon, &dom, &dom), "the same domain");
        assert!(
            meets(&ribbon, &part(0.0, 0.6), &part(0.4, 1.0)),
            "overlapping"
        );
        assert!(meets(&ribbon, &part(0.0, 0.5), &part(0.5, 1.0)), "touching");
        assert!(
            !meets(&ribbon, &part(0.0, 0.4), &part(0.6, 1.0)),
            "disjoint"
        );
        assert!(!meets(&copy, &dom, &dom), "two surfaces");
    }
}
