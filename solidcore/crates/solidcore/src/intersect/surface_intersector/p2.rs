//! `SmSurfaceIntersector`, part 2: the evaluation of an intersection point
//! ([`SurfaceIntersector::compute_point_values`]) and of a fillet-law point, the
//! classification of boundary start points, the boundary bounding-plane disjointness
//! and grazing test, and the flushing of a traced curve into B-spline curves.
//!
//! Upstream: `source/SMLib/src/SmSurfaceIntersector.cpp`.
//!
//! | Rust                                                           | Upstream                                              |
//! |----------------------------------------------------------------|-------------------------------------------------------|
//! | [`SurfaceIntersector::do_boundary_plane_test`], [`BoundaryPlaneTestResult`] | `DoBoundaryPlaneTest`, `SmBoundaryPlaneTestResult` |
//! | [`SurfaceIntersector::compute_point_values`]                   | `ComputePointValues`                                  |
//! | [`SurfaceIntersector::flush_curve`], [`TsectCurves`]           | `FlushCurve` and its output arrays                    |
//! | [`SurfaceIntersector::compute_law_point_values`], [`compute_law_point_values_with`], [`LawPointOptions`] | `ComputeLawPointValues`                        |
//! | [`SurfaceIntersector::classify_boundary_start_point_intersection`] | `ClassifyBoundaryStartPointIntersection`          |
//! | [`is_point_unique`]                                            | `sm_IsPointUnique` (static)                           |
//!
//! Deviations:
//! - **Outputs.** Out-params become return values; the output arrays of `FlushCurve`
//!   and `DoBoundaryPlaneTest` (`r3DCurves`, the two UV curve arrays, `rCurveTypes`,
//!   `rDeviations`) are the fields of one [`TsectCurves`] (`DoBoundaryPlaneTest` has
//!   no UV curves and leaves them alone). `SmBoundaryPlaneTestResult` (declared in
//!   `SmSurfaceCache.h`, no functions) is defined here.
//! - **Points.** `ComputePointValues` and `ClassifyBoundaryStartPointIntersection`
//!   take the point (and the optional previous point) by reference, as upstream,
//!   instead of an arena [`TsectId`](super::p1::TsectId). The previous point is only read
//!   (`StepBackFromSingularity` steps off the current point).
//! - **Virtuals.** `ComputePointValues`, `FlushCurve`, `ComputeSingularityPoint` and
//!   `ComputePointValuesDelta` are virtual upstream; the `SmSurfaceIntersector` bodies
//!   are ported (the derived intersectors are other units), except that
//!   `ComputePointValues` calls the `SmAdvSurfaceIntersector` override of
//!   `ComputeSingularityPoint` on an intersector made by
//!   `AdvSurfaceIntersector::new` (its `advanced` flag). The recursive call of
//!   `ComputePointValues` passes no previous point, so it recurses at most once.
//! - **Loop bound.** The gap-function walk of
//!   `ClassifyBoundaryStartPointIntersection` grows its step by 5 per sample and
//!   upstream ends it only when a sample leaves a domain. With a zero UV tangent on
//!   both surfaces no sample ever does: the samples stay at the point until `delta`
//!   overflows to `inf` (about 440 samples), after which `0 * inf` is NaN and
//!   `ContainsPoint2d` treats a NaN point as inside, so upstream never ends. It stops
//!   with `SM_ERR` after [`MAX_GAP_SAMPLES`] samples (upstream classifies a side in a
//!   handful).
//! - **Fillet laws.** The law is passed separately from [`LawPointOptions`] as
//!   `&mut FilletLaw` (its evaluation caches a B-spline law's extension, through a
//!   `const` cast upstream). `ComputeLawPointValues` is the driver
//!   [`compute_law_point_values_with`]: its `EvaluateLawPoint` and
//!   `ComputePointValuesDelta` calls are virtual (`IntersectorVirtuals`, part 4). The
//!   curve parameter the basic `EvaluateLawPoint` never sets (an uninitialised
//!   `double` upstream) is stored as `SM_UNDEF_DOUBLE`. A failing
//!   `ComputePointValuesDelta` retries, except `Unported`, which propagates.
//! - **Unported callees.** `SmSurface::IsSingularity` of a surface that is not a
//!   plane and `SmSurface::LocalPointSolve` (`surface.surface_p5`) return
//!   [`SolidError::Unported`], which always propagates (also where upstream ignores
//!   the status).
//! - **Short last span (solidcore-native).** (WR-697 addendum, end-rib flush merge)
//!   `FlushCurve` merges a last span shorter
//!   than a tenth of the one before only when the merged span passes the trace's span
//!   test (`SurfaceIntersector::span_keeps_accuracy`); the fork (18e96f1) merges it
//!   untested, as pinned SMLib does.
//! - **A point at a pole is the pole (not upstream).** A trace point that
//!   `ComputePointValues` finds singular on a surface and that lies on that surface's
//!   degenerate boundary (within the UV tolerance `TestPoint` reads "on a boundary" with)
//!   is given the boundary's parameter across it, the pole as its point on that surface,
//!   and the pole's foot on the other surface
//!   ([`SurfaceIntersector::place_at_pole`]): the curves that end at one pole end at one
//!   point, and their trims end on the degenerate boundary, before
//!   `StepBackFromSingularity` places the singular parameter along the curve's tangent
//!   (part 1).
//! - **A point near a pole but off it is not stepped back from (not upstream).**
//!   A point `IsSingularity` calls singular because it is near a pole (a collapsed side
//!   of the domain), off the degenerate boundary, keeps its own surface derivatives and UV
//!   tangent ([`SurfaceIntersector::near_a_pole_off_it`]); upstream replaced them with
//!   those of a point stepped back along the span. Only on an analytic sphere, and only
//!   where its own UV tangent's certified error is no larger than the step-back
//!   tangent's (both bounded rigorously: an interval enclosure of the net's exact
//!   derivatives and of the chain rule's residual); on every other surface kind,
//!   elsewhere, and at interior singularities, the step-back stays.
//! - **Not ported here.** The `SM_DEBUG_CODE` blocks (graphics, dumps) and the
//!   `SM_ASSERT`s (no-ops in release builds).

use core::cmp::Ordering;

use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::station::{ders_basis, span, Nurbs};
use crate::blend::fillet_standard_solver::p1::FilletLaw;
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::curve::curve::Curve;
use crate::error::{passed_on, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::matrix::Matrix;
use crate::geom::solution_array::{ObjectKind, SolutionObject, SolutionType};
use crate::geom::tol::are_same;
use crate::geom::vector2d::{Point2, Vec2};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{max2, EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT, PI, UNDEF_DOUBLE};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::surface::p2::{drop_vectors, TsectCurveType, B_SPLINE_SURFACE_TYPE};
use crate::surface::surface::p5::{SS_UMAX, SS_UMIN, SS_VMAX, SS_VMIN};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::surface_cache::PatchBoundaryBoundingPlanes;

use super::p1::{flush_crv, IntersectionPointType, SrfIndex, SurfaceIntersector, TsectPnt};
use super::p4::IntersectorVirtuals;

const OP_CPV: &str = "intersect::surface_intersector::p2::SurfaceIntersector::compute_point_values";
const OP_SING: &str =
    "intersect::surface_intersector::p2::SurfaceIntersector::compute_singularity_point";
const OP_BPT: &str =
    "intersect::surface_intersector::p2::SurfaceIntersector::do_boundary_plane_test";
const OP_LAW: &str =
    "intersect::surface_intersector::p2::SurfaceIntersector::compute_law_point_values";
const OP_FLUSH: &str = "intersect::surface_intersector::p2::SurfaceIntersector::flush_curve";
const OP_CLASSIFY: &str = "intersect::surface_intersector::p2::SurfaceIntersector::classify_boundary_start_point_intersection";

/// `SM_PB_SAMPLECOUNT`: sample points per boundary bounding plane.
const PB_SAMPLE_COUNT: usize = crate::surface::surface_cache::p1::PB_SAMPLE_COUNT;

/// Most gap-function samples per side in
/// [`SurfaceIntersector::classify_boundary_start_point_intersection`] before it gives
/// up with `SM_ERR` (upstream has no bound; it needs a handful).
pub const MAX_GAP_SAMPLES: usize = 2000;

/// `SM_DEG2RAD(deg)`.
fn deg2rad(deg: f64) -> f64 {
    deg * (PI / 180.0)
}

/// A status upstream ignores (`Unported` propagates).
fn ignored<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
    }
}

/// The result of [`SurfaceIntersector::do_boundary_plane_test`]
/// (`SmBoundaryPlaneTestResult`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryPlaneTestResult {
    /// `SM_BR_GRAZE`: a pair of boundary bounding planes coincides and the surfaces
    /// are on opposite sides of it.
    Graze = 0,
    /// `SM_BR_DISJOINT`: a pair of boundary bounding planes is parallel and the
    /// surfaces are off the region between them.
    Disjoint = 1,
    /// `SM_BR_UNKNOWN`: an intersection is possible.
    Unknown = 2,
}

/// Intersection curves as upstream accumulates them in its output arrays: the 3D
/// curves, the UV curves on each surface, and each curve's type and deviation.
#[derive(Debug, Clone, Default)]
pub struct TsectCurves {
    /// `r3DCurves`.
    pub curves_3d: Vec<Curve>,
    /// `rSurface1UVCurves`, `rSurface2UVCurves`.
    pub uv_curves: [Vec<Curve>; 2],
    /// `rCurveTypes`.
    pub curve_types: Vec<TsectCurveType>,
    /// `rDeviations`.
    pub deviations: Vec<f64>,
}

/// The guess UV points `crUV0`, `crUV1` of [`compute_law_point_values_with`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LawGuess {
    /// Points of their own (`SplitSpan`, a caller's values).
    Given([Point2; 2]),
    /// `rTsectPnt.UVPos(0)`, `rTsectPnt.UVPos(1)` of the point being computed, as
    /// `DoLawIntersection` passes them: references upstream, so a try whose
    /// `ComputePointValuesDelta` fails after writing the point's UV points (the
    /// fillet intersector's does) changes the guesses of the next try.
    PointUvs,
}

/// The fillet-law inputs of [`SurfaceIntersector::compute_law_point_values`] (the
/// law itself, `crLawCurve`, is passed separately as `&mut FilletLaw`).
#[derive(Debug, Clone, Copy)]
pub struct LawPointOptions<'a> {
    /// `bLawOrient`: the parameters run from the interval end to its start.
    pub law_orient: bool,
    /// `crPointCurve`: the fillet edge curve (arc length parameterised).
    pub point_curve: &'a Curve,
    /// `crPointCurveInterval`: the range of the fillet edge.
    pub point_curve_interval: Extent1d,
    /// `caNormals`: the plane normals at the interval ends, used when
    /// `average_normals`.
    pub normals: [Vec3; 2],
    /// `bAverageNormals`: interpolate `normals` instead of using the curve tangent.
    pub average_normals: bool,
    /// `dSurfaceOrientations`: the offset direction (`-1` or `1`) of each surface.
    pub surface_orientations: [f64; 2],
}

/// The test of `ComputePointValues` step 3.a: the starter direction leaves a
/// surface at a natural boundary.
fn leaves_domain(p: &TsectPnt) -> bool {
    use crate::geom::extent1d::ExtentPointType::{End, Start};
    let bad = |s: SrfIndex| {
        let d = p.uv_deriv(s);
        (p.u_param_type(s) == Start && not_ge(d.x, -EFF_ZERO))
            || (p.u_param_type(s) == End && not_le(d.x, EFF_ZERO))
            || (p.v_param_type(s) == Start && not_ge(d.y, -EFF_ZERO))
            || (p.v_param_type(s) == End && not_le(d.y, EFF_ZERO))
    };
    bad(SrfIndex::First) || bad(SrfIndex::Second)
}

/// `!(a >= b)` (true for NaN).
fn not_ge(a: f64, b: f64) -> bool {
    !matches!(a.partial_cmp(&b), Some(Ordering::Greater | Ordering::Equal))
}

/// `!(a <= b)` (true for NaN).
fn not_le(a: f64, b: f64) -> bool {
    !matches!(a.partial_cmp(&b), Some(Ordering::Less | Ordering::Equal))
}

/// `smsurf_DropVectors(du, dv, 1, &CrvDeriv, &UVDeriv(srf))` for both surfaces.
fn drop_crv_deriv(p: &mut TsectPnt) -> Result<()> {
    for s in [SrfIndex::First, SrfIndex::Second] {
        let v = drop_vectors(p.srf_du(s), p.srf_dv(s), &[p.crv_deriv()])?;
        *p.uv_deriv_mut(s) = v.first().copied().unwrap_or(Vec2::new(0.0, 0.0));
    }
    Ok(())
}

/// `AngleBetween` of the surface normals, folded into `[0, PI/2]`.
fn normal_angle(p: &mut TsectPnt) -> Result<()> {
    p.tangent_plane_angle_rad = p
        .srf_norm(SrfIndex::First)
        .angle_between(p.srf_norm(SrfIndex::Second))?;
    if p.tangent_plane_angle_rad > PI / 2.0 {
        p.tangent_plane_angle_rad = PI - p.tangent_plane_angle_rad;
    }
    Ok(())
}

/// Not upstream: rigorous bounds on the absolute errors of the first derivatives `su` and
/// `sv` that `Surface::evaluate` returned at `uv` on an analytic sphere: `None` for any
/// other surface kind (it then steps back, as upstream) and where the enclosure fails. At
/// an interior knot of the net both neighbouring patches are enclosed and the larger
/// distance is taken (the side the evaluation took is not modelled).
///
/// `Surface::evaluate` evaluates a sphere through its rational net (its
/// `SmBSplineSurface` base). The exact first derivatives of that net at `uv` are enclosed
/// in rounded (outward) interval arithmetic: the basis functions and their first
/// derivatives at `uv` (`ders_basis` on the exact knots and parameters), the homogeneous
/// sums `A = Σ N_i M_j w_ij P_ij`, `A_u`, `A_v` and the weights `w`, `w_u`, `w_v`, and the
/// quotient rule `S_u = (A_u - w_u A / w) / w`. Every quantity a running-error bound would
/// count (`Σ |N'_i| w_i |P_i|`, the `w_u` terms, the weight ratio wherever it enters)
/// enters the enclosure's width. The bound on `|su - S_u|` is the distance from the
/// computed `su` to the far corner of the box, rounded up: it holds whatever operation
/// order the evaluator used, as long as the box encloses the exact value (PROVEN by the
/// interval arithmetic's outward rounding). Parameter scaling cancels: `S_u` and its
/// error both scale with the u parameter's scale.
fn sphere_derivative_errors(
    surface: &Surface,
    uv: Point2,
    su: Vec3,
    sv: Vec3,
) -> Option<(f64, f64)> {
    let SurfaceKind::Sphere(sphere) = surface.kind() else {
        return None;
    };
    let net = Nurbs::of(sphere.base().base()).ok()?;
    // the spans a parameter's evaluation may take: both neighbours at an interior knot
    let spans = |knots: &[f64], deg: usize, n: usize, x: f64| {
        let k = span(knots, deg, n, x);
        let mut out = vec![k];
        if knots.get(k).copied() == Some(x) && k > deg {
            if let Some(left) = (deg..k).rev().find(|&j| knots.get(j).copied() < Some(x)) {
                out.push(left);
            }
        }
        out
    };
    let (mut eu, mut ev) = (0.0_f64, 0.0_f64);
    for ku in spans(&net.u, net.p, net.nu, uv.x) {
        for kv in spans(&net.v, net.q, net.nv, uv.y) {
            let (bu, bv) = derivative_box_errors(&net, ku, kv, uv, su, sv)?;
            eu = eu.max(bu);
            ev = ev.max(bv);
        }
    }
    let finite = eu.is_finite() && ev.is_finite();
    finite.then_some((eu, ev))
}

/// The distances from `su` and `sv` to the far corners of enclosures of the exact first
/// derivatives at `uv` of `net`'s polynomial patch `(ku, kv)` ([`sphere_derivative_errors`]).
fn derivative_box_errors(
    net: &Nurbs,
    ku: usize,
    kv: usize,
    uv: Point2,
    su: Vec3,
    sv: Vec3,
) -> Option<(f64, f64)> {
    let nu = ders_basis(&net.u, ku, net.p, Ivl::from(uv.x), 1);
    let nv = ders_basis(&net.v, kv, net.q, Ivl::from(uv.y), 1);
    let basis = |m: &Vec<Vec<Ivl>>, a: usize, i: usize| m.get(a).and_then(|r| r.get(i)).copied();
    // the homogeneous sums H[a][b] = ∂^(a+b) (A, w) / ∂u^a ∂v^b
    let mut h = [[[Ivl::from(0.0); 4]; 2]; 2];
    for (a, row) in h.iter_mut().enumerate() {
        for (b, hab) in row.iter_mut().enumerate() {
            for i in 0..=net.p {
                for j in 0..=net.q {
                    let li = 4
                        * ((ku + i).checked_sub(net.p)? * net.nv + (kv + j).checked_sub(net.q)?);
                    let c = basis(&nu, a, i)? * basis(&nv, b, j)?;
                    for (e, he) in hab.iter_mut().enumerate() {
                        *he = *he + c * Ivl::from(*net.pw.get(li + e)?);
                    }
                }
            }
        }
    }
    let [[h00, h01], [h10, _]] = h;
    let w = h00[3];
    let positive = w.lo > 0.0;
    if !positive {
        return None;
    }
    let point = [h00[0] / w, h00[1] / w, h00[2] / w];
    let [px, py, pz] = point;
    let rate = |[x, y, z, wd]: [Ivl; 4]| [(x - wd * px) / w, (y - wd * py) / w, (z - wd * pz) / w];
    // the distance from the computed vector to the box's far corner, rounded up
    let far = |d: [Ivl; 3], c: Vec3| {
        let gap = |x: Ivl, v: f64| {
            Ivl::from(
                (Ivl::from(v) - Ivl::from(x.lo))
                    .mag()
                    .max((Ivl::from(x.hi) - Ivl::from(v)).mag()),
            )
        };
        let (gx, gy, gz) = (gap(d[0], c.x), gap(d[1], c.y), gap(d[2], c.z));
        (gx * gx + gy * gy + gz * gz).sqrt().hi
    };
    Some((far(rate(h10), su), far(rate(h01), sv)))
}

/// Not upstream: certified lower and upper bounds on the error of the UV tangent `d` as
/// the solution of the chain rule at a point: on `|T - (S_u d_u + S_v d_v)|`, `S_u` and
/// `S_v` the exact first derivatives there, given the computed `su` and `sv` and bounds
/// `err_u` and `err_v` on their errors ([`sphere_derivative_errors`]).
///
/// The residual `T - (su d_u + sv d_v)` of the computed values is enclosed in rounded
/// interval arithmetic, and by the triangle inequality the exact error lies within
/// `err_u |d_u| + err_v |d_v|` of the residual's length (PROVEN: the enclosure's outward
/// rounding and the triangle inequality). The residual counts every error of the solve
/// that produced `d`, the regular solve's and the projection fallback's alike, so no
/// operation count is needed. A NaN gives `(0, ∞)`.
fn tangent_error_bounds(
    su: Vec3,
    sv: Vec3,
    err_u: f64,
    err_v: f64,
    t: Vec3,
    d: Vec2,
) -> (f64, f64) {
    let (du, dv) = (Ivl::from(d.x), Ivl::from(d.y));
    let residual =
        |tc: f64, a: f64, b: f64| Ivl::from(tc) - (Ivl::from(a) * du + Ivl::from(b) * dv);
    let r = [
        residual(t.x, su.x, sv.x),
        residual(t.y, su.y, sv.y),
        residual(t.z, su.z, sv.z),
    ];
    // the least magnitude of an interval: 0 where it holds 0
    let least = |x: Ivl| {
        if x.lo > 0.0 {
            x.lo
        } else if x.hi < 0.0 {
            -x.hi
        } else {
            0.0
        }
    };
    let sq = |a: f64| Ivl::from(a) * Ivl::from(a);
    let res_hi = r
        .iter()
        .fold(Ivl::from(0.0), |acc, x| acc + sq(x.mag()))
        .sqrt()
        .hi;
    let res_lo = r
        .iter()
        .fold(Ivl::from(0.0), |acc, x| acc + sq(least(*x)))
        .sqrt()
        .lo;
    let derivs = Ivl::from(err_u) * Ivl::from(d.x.abs()) + Ivl::from(err_v) * Ivl::from(d.y.abs());
    let hi = (Ivl::from(res_hi) + derivs).hi;
    let lo = (Ivl::from(res_lo) - derivs).lo.max(0.0);
    // (written to accept: a NaN is no bound)
    let finite = hi.is_finite() && lo.is_finite();
    if finite {
        (lo, hi)
    } else {
        (0.0, f64::INFINITY)
    }
}

const fn other(s: SrfIndex) -> SrfIndex {
    match s {
        SrfIndex::First => SrfIndex::Second,
        SrfIndex::Second => SrfIndex::First,
    }
}

impl SurfaceIntersector<'_> {
    /// Disjointness and grazing test from the boundary bounding planes of both
    /// surfaces (`DoBoundaryPlaneTest`). Only B-spline surfaces (and planes, a
    /// B-spline kind upstream) have control polygons; for other kinds the answer is
    /// [`BoundaryPlaneTestResult::Unknown`].
    ///
    /// For every pair of planar boundaries whose plane normals are opposite within
    /// `1e-4` (as a dot product): when every sample point of each boundary is beyond
    /// the other boundary's plane (by the planes' tolerances, at least
    /// `min(1e-2, tol_3d / |bbox|) / 1e5`, scaled by the base points' size) the
    /// surfaces are [`BoundaryPlaneTestResult::Disjoint`]; a pair that overlaps by more
    /// than `min(1e-2, tol_3d / |bbox|) / 100` is skipped; otherwise the surfaces
    /// [`BoundaryPlaneTestResult::Graze`]: the two boundary curves are intersected and
    /// each point solution becomes a degenerate curve (type touching), each range a
    /// copy of the first boundary curve trimmed to it (type coincident, tangent or
    /// crossing from the point types of its end points), appended to `out` with its
    /// deviation.
    ///
    /// # Errors
    ///
    /// Those of the bounding planes, the boundary curves, the curve intersection and
    /// the degenerate curve upstream checks; [`SolidError::Unported`] from unported
    /// callees.
    #[allow(clippy::too_many_lines)]
    pub fn do_boundary_plane_test(
        &self,
        k: &Kernel,
        planes1: &mut PatchBoundaryBoundingPlanes,
        planes2: &mut PatchBoundaryBoundingPlanes,
        tol_3d: f64,
        bbox: &Extent3d,
        out: &mut TsectCurves,
    ) -> Result<BoundaryPlaneTestResult> {
        // hard coded tolerances
        let size = bbox.size().length();
        let angle_tol = 1e-4; // -hof- ??
        let distance_tol = min_f(1e-2, tol_3d / size) / 100.0; // used for plane coincidence -hof- ??

        // locals
        let surf1 = self.surface(SrfIndex::First);
        let surf2 = self.surface(SrfIndex::Second);

        // Control polygons are not available for general surfaces: bail out if not
        // a BSplineSurface (SM_CAST_PTR(SmBSplineSurface, ..): every kind derived from
        // it, planes, cones, spheres, tori and surfaces of revolution and extrusion;
        // curve-bounded, STEP, offset and volume surfaces derive from SmSurface only)
        let is_bss = |s: &Surface| s.is_kind_of(B_SPLINE_SURFACE_TYPE);
        if !is_bss(surf1)? || !is_bss(surf2)? {
            return Ok(BoundaryPlaneTestResult::Unknown);
        }

        // for all combinations of boundaries between both surfaces
        for ii in 0..4 {
            // get surf1 ith boundary planarity (and plane values when it is)
            let bp1 = planes1.nth_bounding_plane(k, surf1, ii)?;
            for jj in 0..4 {
                // get surf2 jth boundary planarity (and plane values when it is)
                let bp2 = planes2.nth_bounding_plane(k, surf2, jj)?;

                // skip cases that are not planar/planar
                let (Some(p1), Some(p2)) = (bp1.plane.as_ref(), bp2.plane.as_ref()) else {
                    continue;
                };
                if !bp1.plane_exists || !bp2.plane_exists {
                    continue;
                }

                // The plane normals point 'off the material': they must be opposite
                // and parallel to within tolerance
                let dot_discr = p1.normal.dot(p2.normal);
                if dot_discr > -1.0 + angle_tol {
                    continue;
                }

                // check for disjoint or overlap cases
                let mut disjoint = true;
                let mut overlap = false;
                let max1 = p1.base_point.max_dimension();
                let max2v = p2.base_point.max_dimension();
                let tol_sum = max2(distance_tol / 1000.0, p1.tolerance + p2.tolerance)
                    * (1.0 + max2(max1, max2v));
                for smp in 0..PB_SAMPLE_COUNT {
                    let s1 = p1.sample_points.get(smp).copied().unwrap_or(Vec3::UNDEF);
                    let s2 = p2.sample_points.get(smp).copied().unwrap_or(Vec3::UNDEF);
                    let signed_plane_dist2 = p2.normal.dot(s1 - p2.base_point);
                    let signed_plane_dist1 = p1.normal.dot(s2 - p1.base_point);

                    // check for non Disjoint boundary surfaces (overlapping or coincident)
                    if signed_plane_dist2 <= tol_sum || signed_plane_dist1 <= tol_sum {
                        disjoint = false;
                    }

                    // check for Overlapping surfaces
                    if signed_plane_dist1 < -distance_tol || signed_plane_dist2 < -distance_tol {
                        overlap = true;
                        break;
                    }
                }

                // disjoint Surface Bounding planes - no intersection possible
                if disjoint {
                    return Ok(BoundaryPlaneTestResult::Disjoint);
                }

                // overlapping Surface Bounding planes - go onto next boundary plane pair
                if overlap {
                    continue;
                }

                // grazing or 'coincident boundary planes': load the output with the
                // natural boundary curve/curve intersections and return.
                let bsc1 = planes1.create_boundary_curve_for_index(k, surf1, ii)?;
                let bsc2 = planes2.create_boundary_curve_for_index(k, surf2, jj)?;
                let (min1, max1p) = planes1.boundary_uv_end_points(ii);
                let (min2, max2p) = planes2.boundary_uv_end_points(jj);

                // Intersect Curves
                let ivl1 = bsc1.natural_interval();
                let ivl2 = bsc2.natural_interval();
                let crv1 = bsc1.clone();
                let crv2 = bsc2;
                let opts = CurveIntersectOptions {
                    skip_global_coincidence_check: false,
                    handles: [
                        Some(BoundaryCurve(0, Some(ivl1))),
                        Some(BoundaryCurve(1, Some(ivl2))),
                    ],
                    analytic_handles: [Some(BoundaryCurve(2, None)), Some(BoundaryCurve(3, None))],
                };
                let solutions =
                    crv1.global_curve_intersect(k, &ivl1, &crv2, &ivl2, distance_tol, &opts)?;

                // all done for no intersections
                if solutions.as_slice().is_empty() {
                    return Ok(BoundaryPlaneTestResult::Graze);
                }

                // for every solution
                for sol in solutions.as_slice() {
                    let start0 = sol.start.parameters.first().copied().unwrap_or(0.0);
                    if sol.solution_type == SolutionType::SingleValue {
                        // single point intersection - output a degenerate curve
                        let solution_point = bsc1.evaluate_point(k, start0)?;
                        let degenerate = BSplineCurve::create_degenerate_curve(3, solution_point)?;
                        out.curves_3d.push(Curve::from(degenerate));
                        out.curve_types.push(TsectCurveType::Touching);
                        out.deviations.push(sol.start.solution_value);
                    } else {
                        // interval intersection - a boundary curve trimmed to it
                        let start_param1 = start0;
                        let mut start_param2 = sol.start.parameters.get(1).copied().unwrap_or(0.0);
                        let end_param1 = sol.end.parameters.first().copied().unwrap_or(0.0);
                        let mut end_param2 = sol.end.parameters.get(1).copied().unwrap_or(0.0);
                        if start_param2 > end_param2 {
                            core::mem::swap(&mut start_param2, &mut end_param2);
                        }
                        let mut trim_ivl1 = Extent1d::from_min_max(start_param1, end_param1);
                        let trim_ivl2 = Extent1d::from_min_max(start_param2, end_param2);

                        // for nonZero intersection intervals
                        if trim_ivl1.length() > EFF_ZERO {
                            // copy and trim the Surf1->NaturalBoundaryCurve (status
                            // ignored; may snap the interval to knots)
                            // (`new SmBSplineCurve(*pBSC1)`: a plain B-spline copy)
                            let mut bsc =
                                crate::surface::surface_cache::p1::copy_as_bspline(&bsc1)?;
                            // (passed on with `ssi_proto_topology_statuses_pass_on`,
                            // fork bcc6f01)
                            ignored(passed_on(
                                bsc.trim(k, &mut trim_ivl1, TrimMode::Notify),
                                k.fixes.ssi_proto_topology_statuses_pass_on,
                            )?)?;

                            // get SurfacePoints for each interval endPoint
                            let (s0, e0) = if are_same(min1.x, max1p.x) {
                                (
                                    Point2::new(min1.x, trim_ivl1.min()),
                                    Point2::new(max1p.x, trim_ivl1.max()),
                                )
                            } else {
                                (
                                    Point2::new(trim_ivl1.min(), min1.y),
                                    Point2::new(trim_ivl1.max(), max1p.y),
                                )
                            };
                            let (s1, e1) = if are_same(min2.x, max2p.x) {
                                (
                                    Point2::new(min2.x, trim_ivl2.min()),
                                    Point2::new(max2p.x, trim_ivl2.max()),
                                )
                            } else {
                                (
                                    Point2::new(trim_ivl2.min(), min2.y),
                                    Point2::new(trim_ivl2.max(), max2p.y),
                                )
                            };

                            // get endPoint SurfaceValues (statuses ignored)
                            let mut start_pnt = TsectPnt::new();
                            let mut end_pnt = TsectPnt::new();
                            ignored(passed_on(
                                self.compute_point_values(k, [s0, s1], &mut start_pnt, None, None),
                                k.fixes.ssi_proto_topology_statuses_pass_on,
                            )?)?;
                            ignored(passed_on(
                                self.compute_point_values(k, [e0, e1], &mut end_pnt, None, None),
                                k.fixes.ssi_proto_topology_statuses_pass_on,
                            )?)?;

                            // classify IntersectionCurve from its endPoint classifications
                            let ty = if start_pnt.point_type == IntersectionPointType::Coincidence
                                && end_pnt.point_type == IntersectionPointType::Coincidence
                            {
                                TsectCurveType::Coincident
                            } else if start_pnt.point_type >= IntersectionPointType::TangentCurve
                                && end_pnt.point_type >= IntersectionPointType::TangentCurve
                            {
                                TsectCurveType::Tangent
                            } else {
                                TsectCurveType::Crossing
                            };

                            // output the curve
                            out.curves_3d.push(bsc);
                            out.curve_types.push(ty);
                            out.deviations
                                .push(max2(sol.start.solution_value, sol.end.solution_value));
                        }
                    }
                }

                // all done - the case is grazing and all intersections have been output
                return Ok(BoundaryPlaneTestResult::Graze);
            }
        }

        // no pair of surface natural boundaries were found to be grazing
        let _ = OP_BPT;
        Ok(BoundaryPlaneTestResult::Unknown)
    }

    /// Evaluates the intersection point at the UV points `uv_values` into `point`
    /// (`ComputePointValues`; upstream defaults `previous = NULL`, `step_size =
    /// NULL`): both surfaces' evaluation matrices (to second order) and unit normals,
    /// the UV classification against the domains, the curve point (the mean of the
    /// surface points) and deviation; unless the point is already singular, its type
    /// (crossing, else from the singularity handling) and the unit curve tangent
    /// `N0 x N1` (times the trace direction) — turned into both domains at a
    /// natural boundary for a start point (`previous` = `None`), or made consistent
    /// with `previous`. Near a surface singularity with `previous` and `step_size`
    /// the point steps back from it. Then the UV tangents and the normal angle; for a
    /// crossing point with normals within 3 degrees of a previous point a nearby
    /// tangency is sought.
    ///
    /// A curve tangent shorter than `start_parallel_tol_radians` makes a tangent
    /// point. `point` keeps what was computed before an error.
    ///
    /// # Errors
    ///
    /// Those of the surface evaluations and the vector drops; `SM_ERR` for a surface
    /// without a normal and from the (unimplemented) singularity point; the errors of
    /// `AngleBetween`; [`SolidError::Unported`] from unported callees.
    #[allow(clippy::too_many_lines)]
    pub fn compute_point_values(
        &self,
        k: &Kernel,
        uv_values: [Point2; 2],
        point: &mut TsectPnt,
        previous: Option<&TsectPnt>,
        step_size: Option<f64>,
    ) -> Result<()> {
        // 1. For each surface, store UVPoint values and compute and save surface
        //    values (pos, 1st & 2nd derivs, and unitized normal).
        for (s, uv) in [SrfIndex::First, SrfIndex::Second]
            .into_iter()
            .zip(uv_values)
        {
            *point.uv_pos_mut(s) = uv;

            // load Surface eval matrix into the point
            let m = match self.surface(s).evaluate(k, uv, 2, 2, LEFT_UPPER) {
                Ok(m) => m,
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(e) => {
                    // upstream evaluates into the point's matrix before the zero
                    // tangent replacement fails: those exact derivatives stay
                    if let Ok(raw) = self.surface(s).evaluate(k, uv, 2, 2, LEFT_UPPER_EXACT) {
                        copy_upper_half(point.srf_eval_matrix_mut(s), &raw, 2);
                    }
                    return Err(e);
                }
            };
            copy_upper_half(point.srf_eval_matrix_mut(s), &m, 2);

            // store a unitized surface normal
            let mut norm = point.srf_du(s).cross(point.srf_dv(s));
            if norm.length_squared() < EFF_ZERO_SQ {
                // try to evaluate a normal for pole and singularity points
                norm = surface_evaluate_normal(k, self.surface(s), uv)?;

                // error case - surface point has no normal
                if norm.length_squared() < EFF_ZERO_SQ {
                    return Err(SolidError::intersect_failed(
                        OP_CPV,
                        "the surface point has no normal",
                    ));
                }
            }

            // save the unitized normal (status ignored)
            point.srf_eval_matrix_mut(s)[2][2] = norm.unitize_or_z();
        }

        // Classify the UVPoint Params
        let surfaces = [
            Some(self.surface(SrfIndex::First)),
            Some(self.surface(SrfIndex::Second)),
        ];
        point.classify_point_params(
            uv_values,
            surfaces,
            &self.uv_domains,
            self.this_approx_tol_3d,
        );

        // 2. CurvePoint = average of SurfacePoints
        point.curve_pv[0] =
            (point.srf_pos(SrfIndex::First) + point.srf_pos(SrfIndex::Second)) / 2.0;

        // init deviation with max distance between CrvPos and SurfacePoints
        point.deviation = point.crv_pos().distance(point.srf_pos(SrfIndex::First));

        // 3. for Points which are NOT already marked SM_IP_SINGULARITY
        if point.point_type != IntersectionPointType::Singularity {
            // assume crossing and compute the curve tangent direction
            point.point_type = IntersectionPointType::Crossing;
            point.curve_pv[1] = point
                .srf_norm(SrfIndex::First)
                .cross(point.srf_norm(SrfIndex::Second))
                * self.curve_trace_direction;

            // angle between normal vectors to test the crossing assumption
            normal_angle(point)?;

            match previous {
                None => {
                    // 3.a ensure the starter direction points into the surfaces
                    drop_crv_deriv(point)?;
                    if leaves_domain(point) {
                        // negate the xsect starter direction
                        point.uv_curve_pv[0][1] = -point.uv_curve_pv[0][1];
                        point.uv_curve_pv[1][1] = -point.uv_curve_pv[1][1];
                        point.curve_pv[1] = -point.curve_pv[1];
                    }
                }
                Some(prev) => {
                    // 3.b ensure trace direction is the same as the last
                    let same_dir = if prev.point_type == IntersectionPointType::Crossing {
                        let last_same_dir = 0.0
                            < prev.crv_deriv().dot(
                                prev.srf_norm(SrfIndex::First)
                                    .cross(prev.srf_norm(SrfIndex::Second)),
                            );
                        let this_same_dir = 0.0
                            < point.crv_deriv().dot(
                                point
                                    .srf_norm(SrfIndex::First)
                                    .cross(point.srf_norm(SrfIndex::Second)),
                            );
                        last_same_dir == this_same_dir
                    } else {
                        0.0 < prev.crv_deriv().dot(point.crv_deriv())
                    };
                    if !same_dir {
                        point.curve_pv[1] = -point.curve_pv[1];
                    }
                }
            }

            // 4. a very small tangent plane angle or curve tangent: a surface pole, or
            //    running into or along a tangent intersection
            if point.tangent_plane_angle_rad < self.start_parallel_tol_radians
                || point.crv_deriv().length() < self.start_parallel_tol_radians
                || previous.is_some_and(|p| p.point_type == IntersectionPointType::TangentCurve)
            {
                let at_singularity = match previous {
                    Some(_) => {
                        is_singularity(k, self.surface(SrfIndex::First), uv_values[0])?.0
                            || is_singularity(k, self.surface(SrfIndex::Second), uv_values[1])?.0
                    }
                    None => false,
                };
                if let (true, Some(prev)) = (at_singularity, previous) {
                    let last_vec = prev.crv_deriv();
                    let n0 = point.srf_norm(SrfIndex::First);
                    point.curve_pv[1] = n0.cross(last_vec).cross(n0);
                    point.point_type = IntersectionPointType::TangentCurve;
                } else {
                    // probably on an intersection singularity: its type and curve
                    // direction (tangent and coincident points are not traced)
                    // (virtual: the SmAdvSurfaceIntersector override on an advanced
                    // intersector; any failure is SM_ERR)
                    if self.advanced {
                        match self.adv_compute_singularity_point(
                            uv_values,
                            point,
                            previous.map(TsectPnt::crv_deriv),
                            k.fixes.dupin_bottom_hyperbola_axis,
                        ) {
                            Ok(()) => {}
                            Err(e) if e.is_unported_or_cancelled() => return Err(e),
                            Err(_) => {
                                return Err(SolidError::intersect_failed(
                                    OP_SING,
                                    "the advanced intersector could not place the singular point",
                                ))
                            }
                        }
                    } else {
                        compute_singularity_point_base(uv_values, point, previous)?;
                    }
                    if point.point_type == IntersectionPointType::TangentPoint
                        || point.point_type == IntersectionPointType::Coincidence
                    {
                        return Ok(());
                    }
                }
            }
        }

        // if xSectTangent length is still bad, we have a TangentPoint intersection
        let tangent_length = point.crv_deriv().length();
        if tangent_length < self.start_parallel_tol_radians {
            point.point_type = IntersectionPointType::TangentPoint;
            return Ok(());
        }

        // the xSectCurve Tangent is not zero, so normalize it.
        point.curve_pv[1] /= tangent_length;

        // near-zero first derivatives: check for singularities (needs a previous
        // point and a step size)
        if let (Some(prev), Some(step)) = (previous, step_size) {
            for s in [SrfIndex::First, SrfIndex::Second] {
                let du_len_sq = point.srf_du(s).length_squared();
                let dv_len_sq = point.srf_dv(s).length_squared();
                if du_len_sq < EFF_ZERO_SQRT || dv_len_sq < EFF_ZERO_SQRT {
                    let uv = if s == SrfIndex::First {
                        uv_values[0]
                    } else {
                        uv_values[1]
                    };
                    let (sing, sing_dir) = is_singularity(k, self.surface(s), uv)?;
                    // Not upstream: a point near a pole but off it keeps its own
                    // derivatives (`near_a_pole_off_it`)
                    let keeps = if sing {
                        match self.near_a_pole_off_it(
                            k,
                            s,
                            sing_dir,
                            uv,
                            point.srf_du(s),
                            point.srf_dv(s),
                        )? {
                            Some(errs) => self.own_tangent_beats_step_back(
                                k, s, sing_dir, prev, point, step, errs,
                            )?,
                            None => false,
                        }
                    } else {
                        false
                    };
                    if sing && !keeps {
                        // Not upstream: a point on the degenerate boundary is the pole
                        if self.place_at_pole(k, s, sing_dir, point)? {
                            return Ok(());
                        }
                        // step back along the 3D curve and recompute the tangents
                        let mut p = prev.clone();
                        self.step_back_from_singularity_points(
                            k, sing_dir, s, true, &mut p, point, step,
                        )?;
                    }
                }
            }
        }

        // 5. set IntersectCurve UV Tangents
        drop_crv_deriv(point)?;

        // 6. Compute/Store angle between normal vectors
        normal_angle(point)?;

        // 7. normals within 3 degrees: seek a nearby singular point
        if let Some(prev) = previous {
            if point.point_type == IntersectionPointType::Crossing
                && point.tangent_plane_angle_rad < deg2rad(3.0)
            {
                let mut found: Option<[f64; 4]> = None;

                // skip Previous Points of unknown or singular type
                if prev.point_type != IntersectionPointType::Unknown
                    && prev.point_type != IntersectionPointType::Singularity
                {
                    // domains centred on this point, large enough to include the
                    // previous point
                    let mut uv_domain1 = Extent2d::from_point(prev.uv_pos(SrfIndex::First));
                    let bigger1 = point.uv_pos(SrfIndex::First) - prev.uv_pos(SrfIndex::First);
                    let new_pnt1 = point.uv_pos(SrfIndex::First) + bigger1;
                    uv_domain1.add_point(self.uv_domains[0].clamp_point(new_pnt1));

                    let mut uv_domain2 = Extent2d::from_point(prev.uv_pos(SrfIndex::Second));
                    let bigger2 = point.uv_pos(SrfIndex::Second) - prev.uv_pos(SrfIndex::Second);
                    let new_pnt2 = point.uv_pos(SrfIndex::Second) + bigger2;
                    uv_domain2.add_point(self.uv_domains[1].clamp_point(new_pnt2));

                    // seek a point where the surfaces are parallel
                    found = surface_local_surface_solve_normalize(
                        k,
                        self.surface(SrfIndex::First),
                        &uv_domain1,
                        self.surface(SrfIndex::Second),
                        &uv_domain2,
                        EFF_ZERO_SQRT,
                        uv_domain1.evaluate(0.5, 0.5),
                        uv_domain2.evaluate(0.5, 0.5),
                    )?;
                }

                if let Some([a, b, c, d]) = found {
                    // the values of the parallel point (no previous point: no
                    // further recursion)
                    let uvs_found = [Point2::new(a, b), Point2::new(c, d)];
                    point.point_type = IntersectionPointType::Unknown;
                    self.compute_point_values(k, uvs_found, point, None, None)?;

                    // opposite tangents: invert and recompute the UV tangents
                    if prev.crv_deriv().dot(point.crv_deriv()) < 0.0 {
                        point.curve_pv[1] = -point.curve_pv[1];
                        drop_crv_deriv(point)?;
                    }
                } else if point.point_type == IntersectionPointType::Crossing
                    && point.tangent_plane_angle_rad < self.start_parallel_tol_radians
                {
                    // the surfaces are tangent: take the previous curve tangent
                    // (unitized, status ignored) and recompute the UV tangents
                    point.curve_pv[1] = prev.crv_deriv().unitize_or_z();
                    drop_crv_deriv(point)?;
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Not upstream: whether `uv`, a point `IsSingularity` calls singular on surface `s`
    /// (collapsing along `sing_dir`), with first derivatives `su` and `sv` there, is a
    /// regular point near a pole that may keep its own derivatives: all of
    /// - on an analytic sphere (the only kind whose derivatives' errors are bounded);
    /// - off the degenerate boundary, by more than the UV tolerance `TestPoint` reads "on a
    ///   boundary" with (`100 SM_EFF_ZERO` scaled by the domain, as
    ///   [`Self::place_at_pole`] reads it);
    /// - its derivatives the surface's own, not a replacement for a vanishing tangent
    ///   (as `ComputePointValues` evaluates them);
    /// - on the side of the natural domain nearer it across `sing_dir`, that side a pole
    ///   (`GetSingularities`: its samples within `SM_EFF_ZERO` of one another).
    ///
    /// For such a point it returns certified bounds on its derivatives' errors
    /// ([`sphere_derivative_errors`]); [`Self::own_tangent_beats_step_back`] then decides
    /// whether it keeps its own derivatives. `IsSingularity` calls a point singular when
    /// `|S_u|` is below `ZONE_TOL_3D` times `|S_v|` (both scaled by the domain): within
    /// about 1e-4 of the pole of a sphere of radius 6. Upstream stepped back from such a
    /// point and replaced its singular parameter and its surface derivatives with those of
    /// a point stepped back along the span. Its UV tangent then no longer mapped onto its
    /// own 3D tangent: 0.04 to 0.66 rad off at the knots of a sphere/cylinder trace passing
    /// a pole 1e-5 to 2e-5 away, and a span test reading the images' directions never passed
    /// however short the step.
    ///
    /// `None` (the point is stepped back from, as upstream) for a point on any surface other
    /// than an analytic sphere (B-splines, cones, tori and the rest: their derivatives'
    /// errors need their own certified bound, #16515), a point on the boundary, replaced
    /// derivatives, a point whose nearer side is not a pole, and a point where
    /// `GetSingularities` fails.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] and [`SolidError::Cancelled`] from `GetSingularities`.
    pub(super) fn near_a_pole_off_it(
        &self,
        k: &Kernel,
        s: SrfIndex,
        sing_dir: SurfParamType,
        uv: Point2,
        su: Vec3,
        sv: Vec3,
    ) -> Result<Option<(f64, f64)>> {
        let Some(dom) = self.uv_domains.get(s.index()).copied() else {
            return Ok(None);
        };
        let on_tol = EFF_ZERO * (1.0 + dom.max_dimension()) * 100.0;
        let (across, lo, hi) = match sing_dir {
            SurfParamType::U => (uv.y, dom.min().y, dom.max().y),
            SurfParamType::V => (uv.x, dom.min().x, dom.max().x),
            _ => return Ok(None),
        };
        // (written to accept: a NaN parameter is not near a pole)
        let off_boundary = (across - lo).abs() > on_tol && (across - hi).abs() > on_tol;
        if !off_boundary {
            return Ok(None);
        }
        let surface = self.surface(s);
        // the point's derivatives must be the surface's own: `ComputePointValues`
        // evaluates to second order with zero tangents replaced, and a replaced tangent
        // (where `S_u` vanishes, as on a fold) does not solve the chain rule; the same
        // order without the replacement gives the same values where nothing was replaced
        let exact = match surface.evaluate(k, uv, 2, 2, LEFT_UPPER_EXACT) {
            Ok(m) => m,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(None),
        };
        let own = |i: usize, j: usize| exact.get(i).and_then(|r| r.get(j)).copied();
        let (Some(exact_su), Some(exact_sv)) = (own(1, 0), own(0, 1)) else {
            return Ok(None);
        };
        if exact_su != su || exact_sv != sv {
            return Ok(None);
        }
        // only an analytic sphere has a derivative error bound
        let Some((err_u, err_v)) = sphere_derivative_errors(surface, uv, su, sv) else {
            return Ok(None);
        };
        let natural = match surface.natural_uv_domain_with_nurb(k) {
            Ok(d) => d,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(None),
        };
        let sides = match surface.get_singularities(k, false, false) {
            Ok(r) => u64::from(r.singular),
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(None),
        };
        // the nearer side across the collapsing direction, as a `SM_SS_*` bit
        let side = match sing_dir {
            SurfParamType::U => {
                if (uv.y - natural.min().y).abs() <= (natural.max().y - uv.y).abs() {
                    SS_VMIN
                } else {
                    SS_VMAX
                }
            }
            _ => {
                if (uv.x - natural.min().x).abs() <= (natural.max().x - uv.x).abs() {
                    SS_UMIN
                } else {
                    SS_UMAX
                }
            }
        };
        Ok((sides & side != 0).then_some((err_u, err_v)))
    }

    /// Not upstream: whether `point`, a point [`Self::near_a_pole_off_it`] admits on
    /// surface `s` with derivative error bounds `err_u` and `err_v`, keeps its own
    /// derivatives: whether the certified error of its own UV tangent is no larger than
    /// that of the tangent the step-back would give it (`StepBackFromSingularity` from
    /// `prev` with `step`, then `DropVectors`), both measured as solutions of the chain rule
    /// at the point itself ([`tangent_error_bounds`]: the own tangent's upper bound against
    /// the step-back tangent's lower bound). No tolerance enters: the point takes its own
    /// tangent only where it is certified at least as accurate. The step-back also moves
    /// the point's singular parameter, an error not counted here, so the comparison leans
    /// to the step-back (upstream's choice). A step-back that fails, as upstream's would,
    /// keeps upstream's path.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] and [`SolidError::Cancelled`] from the step-back and the
    /// solves.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn own_tangent_beats_step_back(
        &self,
        k: &Kernel,
        s: SrfIndex,
        sing_dir: SurfParamType,
        prev: &TsectPnt,
        point: &TsectPnt,
        step: f64,
        (err_u, err_v): (f64, f64),
    ) -> Result<bool> {
        let t = point.crv_deriv();
        let (su, sv) = (point.srf_du(s), point.srf_dv(s));
        let Some(own) = drop_vectors(su, sv, &[t])?.first().copied() else {
            return Ok(false);
        };
        let (_, own_hi) = tangent_error_bounds(su, sv, err_u, err_v, t, own);
        let (mut p, mut c) = (prev.clone(), point.clone());
        match self.step_back_from_singularity_points(k, sing_dir, s, true, &mut p, &mut c, step) {
            Ok(()) => {}
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(false),
        }
        match drop_crv_deriv(&mut c) {
            Ok(()) => {}
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(false),
        }
        let (step_back_lo, _) = tangent_error_bounds(su, sv, err_u, err_v, t, c.uv_deriv(s));
        // (written to accept: a NaN keeps the step-back)
        let keeps = own_hi <= step_back_lo;
        Ok(keeps)
    }

    /// Not upstream: places `point`, singular on surface `s` (collapsing along
    /// `sing_dir`), at the pole when it lies on that surface's degenerate boundary.
    ///
    /// A trace that runs into a pole, or passes it within the tolerance it refines with,
    /// ends a curve at a point `RefinePoint` put on the degenerate boundary only to within
    /// its convergence: its parameter across the boundary, its point on the other surface
    /// and so its curve point differ from those of another curve ending at the same pole
    /// by the solves' noise (1e-11 in UV, 1e-9 in 3D). The edges meeting at the pole are
    /// then trimmed short of their curves' ends by that much, and the trims end off the
    /// degenerate boundary, where nothing closes the face's loop along the pole: the body's
    /// volume enclosure, which bounds a closing segment on the pole only on the side
    /// itself, could not decide the volume's sign. The pole is one point. When the point's
    /// parameter across the boundary is within the UV tolerance `TestPoint` reads "on a
    /// boundary" with (`100 SM_EFF_ZERO` scaled by the domain), the point is the pole: that
    /// parameter is the boundary's, its point on `s` is the pole, its point on the other
    /// surface is the pole's foot there (`LocalPointSolve` from the refined point), and its
    /// curve point, deviation and tangent (`n0 x n1`, the sense kept) follow from those. A
    /// point off the boundary, a surface collapsing in both directions or neither, a pole
    /// whose foot is itself singular, and a point where an evaluation or the foot's solve
    /// fails or the foot has no normal are left as upstream leaves them. Where the
    /// surfaces are tangent at the pole the point is a tangent point, as
    /// `ComputePointValues` makes any point with a vanishing tangent, and `true` is
    /// returned (the caller returns at once).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] and [`SolidError::Cancelled`] from the evaluations and the
    /// solve.
    pub(super) fn place_at_pole(
        &self,
        k: &Kernel,
        s: SrfIndex,
        sing_dir: SurfParamType,
        point: &mut TsectPnt,
    ) -> Result<bool> {
        // a failed evaluation or solve leaves the point as upstream has it
        fn kept<T>(r: Result<T>) -> Result<Option<T>> {
            match r {
                Ok(x) => Ok(Some(x)),
                Err(e) if e.is_unported_or_cancelled() => Err(e),
                Err(_) => Ok(None),
            }
        }
        let (si, oi) = if s == SrfIndex::First {
            (0, SrfIndex::Second)
        } else {
            (1, SrfIndex::First)
        };
        let (Some(dom), Some(odom)) = (
            self.uv_domains.get(si).copied(),
            self.uv_domains.get(1 - si).copied(),
        ) else {
            return Ok(false);
        };
        let on_tol = EFF_ZERO * (1.0 + dom.max_dimension()) * 100.0;
        let mut uv = point.uv_pos(s);
        let (across, lo, hi) = match sing_dir {
            SurfParamType::U => (&mut uv.y, dom.min().y, dom.max().y),
            SurfParamType::V => (&mut uv.x, dom.min().x, dom.max().x),
            _ => return Ok(false),
        };
        if (*across - lo).abs() <= on_tol {
            *across = lo;
        } else if (*across - hi).abs() <= on_tol {
            *across = hi;
        } else {
            return Ok(false);
        }
        // a pole on the other surface's pole is left as upstream has it
        match kept(is_singularity(k, self.surface(oi), point.uv_pos(oi)))? {
            Some((false, _)) => {}
            _ => return Ok(false),
        }
        let Some(pole) = kept(self.surface(s).evaluate_point(k, uv))? else {
            return Ok(false);
        };
        let Some(Some([fu, fv])) = kept(surface_local_point_solve_normalize(
            k,
            self.surface(oi),
            &odom,
            pole,
            point.uv_pos(oi),
        ))?
        else {
            return Ok(false);
        };
        let foot = Point2::new(fu, fv);
        let Some(m) = kept(self.surface(oi).evaluate(k, foot, 2, 2, LEFT_UPPER))? else {
            return Ok(false);
        };
        let mut eval = *point.srf_eval_matrix(oi);
        copy_upper_half(&mut eval, &m, 2);
        let norm = eval[1][0].cross(eval[0][1]);
        if norm.length_squared() < EFF_ZERO_SQ {
            return Ok(false);
        }
        eval[2][2] = norm.unitize_or_z();
        *point.uv_pos_mut(s) = uv;
        point.srf_eval_matrix_mut(s)[0][0] = pole;
        *point.uv_pos_mut(oi) = foot;
        *point.srf_eval_matrix_mut(oi) = eval;
        point.curve_pv[0] =
            (point.srf_pos(SrfIndex::First) + point.srf_pos(SrfIndex::Second)) / 2.0;
        point.deviation = point.crv_pos().distance(point.srf_pos(SrfIndex::First));
        let mut tangent = point
            .srf_norm(SrfIndex::First)
            .cross(point.srf_norm(SrfIndex::Second));
        if tangent.dot(point.crv_deriv()) < 0.0 {
            tangent = -tangent;
        }
        // the surfaces tangent at the pole: a tangent point, as for any point
        let length = tangent.length();
        if length < self.start_parallel_tol_radians {
            point.curve_pv[1] = tangent;
            point.point_type = IntersectionPointType::TangentPoint;
            return Ok(true);
        }
        point.curve_pv[1] = tangent / length;
        Ok(false)
    }

    /// Flushes the traced curve (the curve points, which are unlinked) into `out`
    /// (`FlushCurve`): a 3D curve and the two UV curves, degree 3 piecewise C0
    /// B-splines sharing the knots (the curve parameters), with the curve's type and
    /// largest point deviation.
    ///
    /// A single point becomes degenerate curves (type touching) when it is a tangent
    /// point or unique ([`is_point_unique`] against the start points and the 3D
    /// curves' ends). A curve whose consecutive points are all within
    /// `this_approx_tol_3d` is dropped. A last span shorter than a tenth of the one
    /// before is merged into it (only when the merged
    /// span passes `SurfaceIntersector::span_keeps_accuracy`). A crossing curve with
    /// more than one span whose normals stay within 5 degrees is near tangent.
    ///
    /// # Errors
    ///
    /// Those of the degenerate curves and of [`flush_crv`]; [`SolidError::Unported`]
    /// from the curve evaluations of [`is_point_unique`].
    pub fn flush_curve(&mut self, k: &Kernel, out: &mut TsectCurves) -> Result<()> {
        // take (and clear) the curve points
        let ids = core::mem::take(&mut self.curve_points);
        let pnts = ids
            .iter()
            .map(|&id| self.point(id).cloned())
            .collect::<Result<Vec<_>>>()?;

        // check no points to flush.
        let Some(first) = pnts.first() else {
            return Ok(());
        };

        // When intersection is single point, output a degenerate curve
        if pnts.len() == 1 {
            let unique = first.point_type == IntersectionPointType::TangentPoint || {
                let starts = self
                    .start_points
                    .iter()
                    .map(|&id| self.point(id))
                    .collect::<Result<Vec<_>>>()?;
                is_point_unique(
                    k,
                    first.crv_pos(),
                    &starts,
                    &out.curves_3d,
                    self.this_approx_tol_3d,
                )?
            };
            if unique {
                let c3 = BSplineCurve::create_degenerate_curve(3, first.crv_pos())?;
                out.curves_3d.push(Curve::from(c3));
                let c0 = BSplineCurve::create_degenerate_curve(
                    2,
                    Vec3::from(first.uv_pos(SrfIndex::First)),
                )?;
                out.uv_curves[0].push(Curve::from(c0));
                let c1 = BSplineCurve::create_degenerate_curve(
                    2,
                    Vec3::from(first.uv_pos(SrfIndex::Second)),
                )?;
                out.uv_curves[1].push(Curve::from(c1));

                // set curve dist and type parameters
                let dist = first
                    .srf_pos(SrfIndex::First)
                    .distance(first.srf_pos(SrfIndex::Second));
                out.deviations.push(dist);
                out.curve_types.push(TsectCurveType::Touching);
                return Ok(());
            }
        }

        // Eliminate curves whose points are all within tolerance of each other
        let too_short = pnts
            .windows(2)
            .all(|w| matches!(w, [a, b] if a.crv_pos().closer_than(self.this_approx_tol_3d, b.crv_pos())));
        if too_short {
            return Ok(());
        }

        // knot and control point arrays
        let mut knots = vec![first.curve_parameter];
        let mut ctrl_3d = vec![first.crv_pos()];
        let mut ctrl_uv = [
            vec![Vec3::from(first.uv_pos(SrfIndex::First))],
            vec![Vec3::from(first.uv_pos(SrfIndex::Second))],
        ];

        // init curve statistics
        let mut max_deviation = first.deviation;
        let mut max_angle = first.tangent_plane_angle_rad;
        let mut curve_type = if first.point_type == IntersectionPointType::TangentCurve {
            TsectCurveType::Tangent
        } else {
            TsectCurveType::Crossing
        };

        // for every point to flush
        let n = pnts.len();
        let at = |i: usize| {
            pnts.get(i).ok_or_else(|| {
                SolidError::intersect_failed(OP_FLUSH, "the curve has no point at that index")
            })
        };
        let mut i = 1;
        while i < n {
            let prev = at(i - 1)?;
            let mut curr = at(i)?;

            // merge a very short end segment into its neighbour
            if i == n - 2 {
                let fin = at(i + 1)?;
                let t_delta = curr.curve_parameter - prev.curve_parameter;
                let t_delta_final = fin.curve_parameter - curr.curve_parameter;
                // Not upstream (WR-697 addendum, solidcore-native): only when the merged
                // span keeps the trace's accuracy. The trace tested the two spans it made,
                // not the one the merge makes, which can leave the 3D curve and a UV curve
                // further apart than the tolerance
                if t_delta_final / t_delta < 0.1 && self.span_keeps_accuracy(k, prev, fin)? {
                    // skip next to last xsect point
                    i += 1;
                    curr = at(i)?;
                }
            }

            // Collect maximum tangent plane angle and deviation
            if curr.tangent_plane_angle_rad > max_angle {
                max_angle = curr.tangent_plane_angle_rad;
            }
            if curr.deviation > max_deviation {
                max_deviation = curr.deviation;
            }

            // 1. add the curve's next knot
            knots.push(curr.curve_parameter);

            // 2. the segment's Hermite end points and tangents (scaled by the span)
            let p1 = prev.crv_pos();
            let p4 = curr.crv_pos();
            let scale = curr.curve_parameter - prev.curve_parameter;
            let d1 = prev.crv_deriv() * scale;
            let d2 = curr.crv_deriv() * scale;

            // 3. cubic Hermite to cubic Bezier
            let p2 = p1 + d1 / 3.0;
            let p3 = p4 - d2 / 3.0;

            // 4. add the points (don't duplicate segment end points)
            ctrl_3d.extend([p2, p3, p4]);

            // the uv curves
            for (s, ctrl) in [SrfIndex::First, SrfIndex::Second]
                .into_iter()
                .zip(&mut ctrl_uv)
            {
                let pt1 = prev.uv_pos(s);
                let pt4 = curr.uv_pos(s);
                let drv1 = prev.uv_deriv(s) * scale;
                let drv2 = curr.uv_deriv(s) * scale;
                let pt2 = pt1 + drv1 / 3.0;
                let pt3 = pt4 - drv2 / 3.0;
                ctrl.extend([Vec3::from(pt2), Vec3::from(pt3), Vec3::from(pt4)]);
            }
            i += 1;
        }

        // build the B-spline curves
        flush_crv(3, &ctrl_3d, &knots, &mut out.curves_3d)?;
        let [uv0, uv1] = &ctrl_uv;
        flush_crv(2, uv0, &knots, &mut out.uv_curves[0])?;
        flush_crv(2, uv1, &knots, &mut out.uv_curves[1])?;

        // store the maxDeviation
        out.deviations.push(max_deviation);

        // near tangent crossing curves
        if max_angle < deg2rad(5.0) && curve_type == TsectCurveType::Crossing && ctrl_3d.len() > 4 {
            curve_type = TsectCurveType::NearTangent;
        }
        out.curve_types.push(curve_type);
        Ok(())
    }

    /// The point values at the offset surface / offset surface / plane intersection
    /// for the fillet edge parameter `parameter` (`ComputeLawPointValues`): the plane
    /// through the edge curve point with the curve tangent (or the interpolated
    /// `normals`) as normal, at `parameter` and `parameter + delta_step`, is
    /// intersected with the offset surfaces ([`SurfaceIntersector::evaluate_law_point`],
    /// guesses `uv0`, `uv1`; with `skip_computing_uvs` the first intersection is
    /// `point`'s UV points and `user_doubles[0]`), and both points are evaluated with
    /// `ComputePointValuesDelta`. Up to six tries, the delta step growing tenfold.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a zero plane normal; those of
    /// [`SurfaceIntersector::evaluate_law_point`]; `SM_ERR` when no try succeeds;
    /// [`SolidError::Unported`] from unported callees.
    #[allow(clippy::too_many_arguments)]
    pub fn compute_law_point_values(
        &mut self,
        k: &Kernel,
        parameter: f64,
        uv0: Point2,
        uv1: Point2,
        delta_step: f64,
        law: &LawPointOptions<'_>,
        law_curve: &mut FilletLaw,
        skip_computing_uvs: bool,
        point: &mut TsectPnt,
    ) -> Result<()> {
        compute_law_point_values_with(
            self,
            k,
            parameter,
            LawGuess::Given([uv0, uv1]),
            delta_step,
            law,
            law_curve,
            skip_computing_uvs,
            point,
        )
    }

    /// Whether the boundary start point `point` is degenerate: not part of an
    /// intersection curve because of tangency or boundary effects
    /// (`ClassifyBoundaryStartPointIntersection`).
    ///
    /// Tangent points are degenerate; a point whose small steps (`SM_EFF_ZERO_SQRT`
    /// along the UV tangents) both leave a domain is degenerate; tangent-curve,
    /// singular and coincident points are not. A crossing point is degenerate only
    /// when it lies on a natural boundary tangent to the other surface (normal
    /// perpendicular to the boundary within `SM_DEG2RAD(2)`, taken as degrees by
    /// `IsPerpendicularTo`) and, on both sides, the boundary/surface gap function
    /// diverges monotonically from the point (or the side is too short to sample).
    ///
    /// # Errors
    ///
    /// `SM_ERR` after [`MAX_GAP_SAMPLES`] samples of one side;
    /// [`SolidError::Unported`] from `SmSurface::LocalPointSolve`.
    pub fn classify_boundary_start_point_intersection(
        &self,
        k: &Kernel,
        point: &TsectPnt,
    ) -> Result<bool> {
        // TangentPoints are always degenerate
        if point.point_type == IntersectionPointType::TangentPoint {
            return Ok(true);
        }

        // Classify small Pos and Neg steps from the point in the curve direction
        let [dom0, dom1] = &self.uv_domains;
        let p_delta = [
            point.uv_pos(SrfIndex::First) + point.uv_deriv(SrfIndex::First) * EFF_ZERO_SQRT,
            point.uv_pos(SrfIndex::Second) + point.uv_deriv(SrfIndex::Second) * EFF_ZERO_SQRT,
        ];
        let p_inside = dom0.contains_point(p_delta[0], 10.0 * EFF_ZERO)
            && dom1.contains_point(p_delta[1], 10.0 * EFF_ZERO);
        let n_delta = [
            point.uv_pos(SrfIndex::First) - point.uv_deriv(SrfIndex::First) * EFF_ZERO_SQRT,
            point.uv_pos(SrfIndex::Second) - point.uv_deriv(SrfIndex::Second) * EFF_ZERO_SQRT,
        ];
        let n_inside = dom0.contains_point(n_delta[0], 10.0 * EFF_ZERO)
            && dom1.contains_point(n_delta[1], 10.0 * EFF_ZERO);

        // both small steps leave a domain: no intersection curve is possible
        if !p_inside && !n_inside {
            return Ok(true);
        }

        // tangent curve, singular and coincident points have intersection curves
        if matches!(
            point.point_type,
            IntersectionPointType::TangentCurve
                | IntersectionPointType::Singularity
                | IntersectionPointType::Coincidence
        ) {
            return Ok(false);
        }

        // additional checks on crossing points: find tangent boundary curve / surface
        // intersections of both surface points
        let bounds = [
            self.boundary_tangency(point, SrfIndex::First),
            self.boundary_tangency(point, SrfIndex::Second),
        ];

        // not on a tangent natural boundary: the curve is not degenerate
        if !bounds.iter().any(|b| b.tangent_u() || b.tangent_v()) {
            return Ok(false);
        }

        // test the gap function shape on both sides of the point
        let mut walk = GapWalk {
            curve_point: [Vec3::UNDEF; 3],
            o_surf_point: [Vec3::UNDEF; 3],
            gap: [Vec3::UNDEF; 3],
            gap_length: [UNDEF_DOUBLE; 3],
            inside: [false; 3],
            sample_dist: 0.0,
            xsect_gap: point.srf_pos(SrfIndex::Second) - point.srf_pos(SrfIndex::First),
            scaled_zero: EFF_ZERO * (1.0 + point.crv_pos().max_dimension()),
        };

        for (s, b) in [SrfIndex::First, SrfIndex::Second].into_iter().zip(&bounds) {
            if !b.tangent_u() && !b.tangent_v() {
                continue;
            }

            // sample the natural boundary / other surface gap function along the
            // curve tangent direction
            let bi_norm_of = |side: Option<(bool, Vec3)>| side.map_or(Vec3::UNDEF, |(_, n)| n);
            let (bi_norm, tangent) = if b.tangent_u() {
                (bi_norm_of(b.u), point.srf_dv(s))
            } else {
                (bi_norm_of(b.v), point.srf_du(s))
            };
            let mut stride = self.this_approx_tol_3d / tangent.length() / 2.0;
            stride *= 100.0;

            // test the gap function on both sides of the point
            let mut o_surf_pv = [[Vec3::UNDEF; 2]; 2];
            let mut side_degenerate = [false; 2];
            for (side, (degenerate, inside_dir)) in side_degenerate
                .iter_mut()
                .zip([p_inside, n_inside])
                .enumerate()
            {
                // skip directions which take us out of the surface domains
                if !inside_dir {
                    *degenerate = true;
                    continue;
                }
                let delta = if side == 0 { stride } else { -stride };
                *degenerate = self.walk_gap_side(
                    k,
                    point,
                    s,
                    bi_norm,
                    tangent,
                    delta,
                    &mut o_surf_pv,
                    &mut walk,
                )?;
            }

            // degenerate if both sides are degenerate
            if side_degenerate[0] && side_degenerate[1] {
                return Ok(true);
            }
        }

        // a crossing point on an actual intersection curve
        Ok(false)
    }

    /// The natural boundaries of surface `s` the point is on and whether they are
    /// tangent to the other surface (the first part of
    /// `ClassifyBoundaryStartPointIntersection`).
    fn boundary_tangency(&self, point: &TsectPnt, s: SrfIndex) -> BoundaryTangency {
        // IsPerpendicularTo takes degrees: upstream passes the radian value as is
        let angle_tol = deg2rad(2.0);
        let uv = point.uv_pos(s);
        let domain = self.domain(s);
        let other_norm = point.srf_norm(other(s));
        let tan_u = point.srf_du(s);
        let tan_v = point.srf_dv(s);
        let u = if are_same(domain.min().x, uv.x) {
            Some((other_norm.is_perpendicular_to(tan_v, angle_tol), tan_u))
        } else if are_same(domain.max().x, uv.x) {
            Some((other_norm.is_perpendicular_to(tan_v, angle_tol), -tan_u))
        } else {
            None
        };
        let v = if are_same(domain.min().y, uv.y) {
            Some((other_norm.is_perpendicular_to(tan_u, angle_tol), tan_v))
        } else if are_same(domain.max().y, uv.y) {
            Some((other_norm.is_perpendicular_to(tan_u, angle_tol), -tan_v))
        } else {
            None
        };
        BoundaryTangency { u, v }
    }

    /// `m_vUVDomain[srf]`.
    const fn domain(&self, s: SrfIndex) -> &Extent2d {
        match s {
            SrfIndex::First => &self.uv_domains[0],
            SrfIndex::Second => &self.uv_domains[1],
        }
    }

    /// Walks the gap function of surface `s`'s tangent boundary from the point in
    /// steps `delta, 5 delta, 25 delta, ..` until one side of the point is
    /// classified (the sampling loop of `ClassifyBoundaryStartPointIntersection`):
    /// true when that side is degenerate.
    #[allow(clippy::too_many_arguments)]
    fn walk_gap_side(
        &self,
        k: &Kernel,
        point: &TsectPnt,
        s: SrfIndex,
        bi_norm: Vec3,
        tangent: Vec3,
        delta: f64,
        o_surf_pv: &mut [[Vec3; 2]; 2],
        w: &mut GapWalk,
    ) -> Result<bool> {
        let o = other(s);
        let mut zero = true;
        let mut mono = true;
        let mut div = true;

        // init the sample arrays with the point
        w.curve_point[0] = point.srf_pos(s);
        w.o_surf_point[0] = point.srf_pos(o);
        w.gap[0] = Vec3::new(0.0, 0.0, 0.0);
        w.gap_length[0] = 0.0;
        w.inside[0] = true;

        let mut delta = delta;
        let mut ii = 1usize;
        loop {
            if ii > MAX_GAP_SAMPLES {
                return Err(SolidError::intersect_failed(
                    OP_CLASSIFY,
                    "the boundary start point is not classified within the sample limit",
                ));
            }
            // the sample array index: 1, then 2 (the last sample moves to 1)
            let tgt = if ii > 1 { 2 } else { 1 };
            set3(&mut w.inside, tgt, false);

            // the sample point UV values
            let uv0 = point.uv_pos(s) + point.uv_deriv(s) * delta;
            let uv1 = point.uv_pos(o) + point.uv_deriv(o) * delta;
            let mut uv_drop = Vec2::UNDEF;

            // the sample point is in both domains
            if self.domain(s).contains_point(uv0, 10.0 * EFF_ZERO)
                && self.domain(o).contains_point(uv1, 10.0 * EFF_ZERO)
            {
                // this surface's natural boundary point (status ignored, passed on with
                // `ssi_proto_topology_statuses_pass_on`, fork bcc6f01)
                let pass = k.fixes.ssi_proto_topology_statuses_pass_on;
                if let Some(p) = ignored(passed_on(self.surface(s).evaluate_point(k, uv0), pass)?)?
                {
                    set3(&mut w.curve_point, tgt, p);
                }
                let cp = get3(&w.curve_point, tgt);

                // origin / current point distance
                w.sample_dist = (point.srf_pos(SrfIndex::Second) - cp).length();

                // the in-domain other surface point nearest to it
                let found = surface_local_point_solve_normalize(
                    k,
                    self.surface(o),
                    self.domain(o),
                    cp,
                    uv1,
                )?;
                if let Some([du, dv]) = found {
                    uv_drop = Point2::new(du, dv);
                    // status ignored: the old values stay (passed on with the flag)
                    if let Some(m) = ignored(passed_on(
                        self.surface(o).evaluate(k, uv_drop, 1, 1, LEFT_UPPER),
                        pass,
                    )?)? {
                        copy_upper_half(o_surf_pv, &m, 1);
                    }

                    // the gap vector, minus the point's own gap (keeps tolerance
                    // noise from creating opposite gap vectors)
                    set3(&mut w.o_surf_point, tgt, o_surf_pv[0][0]);
                    let own = if s == SrfIndex::First {
                        -w.xsect_gap
                    } else {
                        w.xsect_gap
                    };
                    let g = (o_surf_pv[0][0] - cp) + own;
                    set3(&mut w.gap, tgt, g);
                    set3(&mut w.gap_length, tgt, g.length());
                    set3(&mut w.inside, tgt, true);
                }
            }

            let tgt1 = ((tgt + 3) - 1) % 3;
            let tgt2 = ((tgt + 3) - 2) % 3;
            if !get3(&w.inside, tgt) {
                // stepped off the domain: too short to sample, or continuously
                // diverging while shorter than 10 * approxTol, is degenerate
                return Ok(ii < 3
                    || (mono
                        && div
                        && bi_norm.dot(get3(&w.gap, tgt1)) > 0.0
                        && w.sample_dist < self.this_approx_tol_3d * 10.0));
            }
            if ii >= 2 {
                let cp = get3(&w.curve_point, tgt);
                let g = get3(&w.gap, tgt);
                let gl = get3(&w.gap_length, tgt);

                // 1st: is the gap zero (coincident curves)
                if gl > w.scaled_zero {
                    zero = false;
                }

                // 2nd: is the gap function open: the first order intersection of the
                // planes (C, biNorm, tangent) and (S, Su, Sv) must be off a surface
                let open = self.is_gap_open(o, bi_norm, tangent, cp, uv_drop, o_surf_pv)?;

                // monotonic and diverging are only tested for a nonzero gap
                if !zero {
                    // 3rd: is the gap function monotonic
                    if g.dot(get3(&w.gap, tgt1)) <= 0.0 {
                        mono = false;
                    }
                    // 4th: is the gap diverging (growing faster than linear)
                    let gl1 = get3(&w.gap_length, tgt1);
                    let gl2 = get3(&w.gap_length, tgt2);
                    if gl == 0.0 || gl < gl1 + 1.0001 * (gl1 - gl2) {
                        div = false;
                    }
                }

                // classify this side
                if !zero && mono && div && open {
                    return Ok(true);
                } else if zero && mono && div {
                    // try another point: move the current sample to the last
                    w.curve_point[1] = w.curve_point[2];
                    w.o_surf_point[1] = w.o_surf_point[2];
                    w.gap[1] = w.gap[2];
                    w.gap_length[1] = w.gap_length[2];
                    w.inside[1] = w.inside[2];
                } else {
                    // no longer diverging monotonically: this side can intersect
                    return Ok(false);
                }
            }
            ii += 1;
            delta *= 5.0;
        }
    }

    /// The `bOpen` test of `ClassifyBoundaryStartPointIntersection`: the point
    /// `P = S + du Su + dv Sv` nearest to `C` on the plane `(C, biNorm, tangent)` is
    /// outside the other surface's domain or behind the boundary; a singular system
    /// (parallel planes) is open.
    fn is_gap_open(
        &self,
        o: SrfIndex,
        bi_norm: Vec3,
        tangent: Vec3,
        cp: Point3,
        uv_drop: Point2,
        o_surf_pv: &[[Vec3; 2]; 2],
    ) -> Result<bool> {
        //  Solve: [ 2*Su*Su  2*Su*Sv  -(BXT)*Su][  du  ]   [ -2(S-C)*Su ]
        //         [ 2*Su*Sv  2*Sv*Sv  -(BXT)*Sv][  dv  ] = [ -2(S-C)*Sv ]
        //         [ (BXT)*Su (BXT)*Sv  0       ][lambda] = [-(BXT)*(S-C)]
        let bxt = bi_norm.cross(tangent);
        let sc = o_surf_pv[0][0] - cp;
        let su = o_surf_pv[1][0];
        let sv = o_surf_pv[0][1];
        let a = Matrix::from_row_major(
            3,
            3,
            vec![
                2.0 * su.dot(su),
                2.0 * su.dot(sv),
                -bxt.dot(su),
                2.0 * sv.dot(su),
                2.0 * sv.dot(sv),
                -bxt.dot(sv),
                bxt.dot(su),
                bxt.dot(sv),
                0.0,
            ],
        )?;
        let b = [-2.0 * sc.dot(su), -2.0 * sc.dot(sv), -bxt.dot(sc)];

        // solve: the solver fails when the two planes are parallel (a near-zero pivot
        // is a warning status upstream, not SM_SUCCESS)
        match a.solve_linear_system(&b) {
            Ok(sol) if !sol.near_singular => {
                let x0 = sol.x.first().copied().unwrap_or(0.0);
                let x1 = sol.x.get(1).copied().unwrap_or(0.0);
                let p_uv = Point2::new(uv_drop.x + x0, uv_drop.y + x1);
                let pc = o_surf_pv[0][0] + su * x0 + sv * x1 - cp;
                let pc_dot_bi_norm = pc.dot(bi_norm);
                Ok(!self.domain(o).contains_point(p_uv, EFF_ZERO) || pc_dot_bi_norm < -EFF_ZERO)
            }
            Err(e) if e.is_unported_or_cancelled() => Err(e),
            _ => Ok(true), // open for nonzero gaps
        }
    }
}

/// The boundary classification of one surface point in
/// `ClassifyBoundaryStartPointIntersection` (`bBoundaryU/V`, `bTangentU/V`,
/// `sBiNormU/V`).
/// `None` when the point is not on that boundary, else `(tangent, bi_norm)`.
#[derive(Debug, Clone, Copy)]
struct BoundaryTangency {
    u: Option<(bool, Vec3)>,
    v: Option<(bool, Vec3)>,
}

impl BoundaryTangency {
    /// `bTangentU` (false off the boundary).
    fn tangent_u(&self) -> bool {
        self.u.is_some_and(|(t, _)| t)
    }

    /// `bTangentV` (false off the boundary).
    fn tangent_v(&self) -> bool {
        self.v.is_some_and(|(t, _)| t)
    }
}

/// The gap-function samples of `ClassifyBoundaryStartPointIntersection` (function
/// scope upstream: they carry over between sides and surfaces).
#[derive(Debug, Clone)]
struct GapWalk {
    curve_point: [Point3; 3],
    o_surf_point: [Point3; 3],
    gap: [Vec3; 3],
    gap_length: [f64; 3],
    inside: [bool; 3],
    sample_dist: f64,
    xsect_gap: Vec3,
    scaled_zero: f64,
}

/// Copies the entries `D[i][j]`, `i + j <= highest`, of an `Evaluate(.., bOnlyUpperHalf
/// = TRUE, ..)` result into `dst`: upstream writes only those, the others keep their
/// values.
fn copy_upper_half<const N: usize>(dst: &mut [[Vec3; N]; N], m: &[Vec<Vec3>], highest: usize) {
    for (i, (drow, mrow)) in dst.iter_mut().zip(m).enumerate() {
        for (j, (d, v)) in drow.iter_mut().zip(mrow).enumerate() {
            if i + j <= highest {
                *d = *v;
            }
        }
    }
}

/// `a[i]` for an index below 3.
const fn get3<T: Copy>(a: &[T; 3], i: usize) -> T {
    match i {
        0 => a[0],
        1 => a[1],
        _ => a[2],
    }
}

/// `a[i] = v` for an index below 3.
fn set3<T>(a: &mut [T; 3], i: usize, v: T) {
    match i {
        0 => a[0] = v,
        1 => a[1] = v,
        _ => a[2] = v,
    }
}

/// `ComputeSingularityPoint` (virtual) on points outside the arena: the basic
/// intersector does not handle singular or tangent points (`SM_ERR`).
fn compute_singularity_point_base(
    _uv_values: [Point2; 2],
    _point: &mut TsectPnt,
    _previous: Option<&TsectPnt>,
) -> Result<()> {
    Err(SolidError::intersect_failed(
        OP_SING,
        "the basic intersector does not handle singular or tangent points",
    ))
}

/// True when `point` is farther than `tol_3d` from every start point's curve point
/// and from both ends of every curve in `curves_3d` (`sm_IsPointUnique`; an end that
/// fails to evaluate keeps the previous end's value, initially undefined).
///
/// # Errors
///
/// [`SolidError::Unported`] from the curve evaluations.
pub fn is_point_unique(
    k: &Kernel,
    point: Point3,
    start_points: &[&TsectPnt],
    curves_3d: &[Curve],
    tol_3d: f64,
) -> Result<bool> {
    // check the start points for duplicates
    for sp in start_points {
        if point.distance(sp.crv_pos()) < tol_3d {
            return Ok(false);
        }
    }

    // check the curve start/end points for duplicates
    for curve in curves_3d {
        let ivl = curve.natural_interval();
        let mut p = Vec3::UNDEF;
        if let Some(v) = ignored(curve.evaluate_point(k, ivl.min()))? {
            p = v;
        }
        if point.distance(p) < tol_3d {
            return Ok(false);
        }
        if let Some(v) = ignored(curve.evaluate_point(k, ivl.max()))? {
            p = v;
        }
        if point.distance(p) < tol_3d {
            return Ok(false);
        }
    }
    Ok(true)
}

// ---- helpers --------------------------------------------------------------------------------

/// `smos_Min`.
fn min_f(a: f64, b: f64) -> f64 {
    crate::math::min2(a, b)
}

/// `Evaluate(uv, h, h, TRUE, TRUE, TRUE, .., TRUE)`.
const LEFT_UPPER: EvaluateFlags = EvaluateFlags {
    u_side: Side::Left,
    v_side: Side::Left,
    only_upper_half: true,
    non_zero_tangents: true,
};

/// [`LEFT_UPPER`] with exact tangents (what upstream has written when the zero
/// tangent replacement fails).
const LEFT_UPPER_EXACT: EvaluateFlags = EvaluateFlags {
    non_zero_tangents: false,
    ..LEFT_UPPER
};

/// A boundary curve of [`SurfaceIntersector::do_boundary_plane_test`] in its curve
/// intersection solutions (`0`: the first curve, `1`: the second, `2` / `3`: their
/// analytic replacements) with its natural interval (`None` for the analytic
/// replacements, which the handle cannot reach).
#[derive(Debug, Clone, Copy, PartialEq)]
struct BoundaryCurve(u8, Option<Extent1d>);

impl SolutionObject for BoundaryCurve {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        self.1
    }
}

// ---- unported callees --------------------------------------------------------------------------

/// `crSurface.EvaluateNormal(uv, TRUE, TRUE, ..)` (virtual: the plane override, the
/// curve-bounded surface's basis surface, `SmSurface::EvaluateNormal` otherwise).
fn surface_evaluate_normal(k: &Kernel, surface: &Surface, uv: Point2) -> Result<Vec3> {
    surface.evaluate_normal(k, uv, Side::Left, Side::Left)
}

/// `crSurface.IsSingularity(uv, eSingDir)`: the plane override (none); other kinds
/// run `SmSurface::IsSingularity`.
fn is_singularity(k: &Kernel, surface: &Surface, uv: Point2) -> Result<(bool, SurfParamType)> {
    match surface.kind() {
        SurfaceKind::Plane(p) => Ok(p.is_singularity()),

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
            let r = surface.is_singularity(k, uv, EFF_ZERO, false)?;
            Ok((r.singular, r.direction))
        }
    }
}

/// `crSurface.LocalPointSolve(domain, SM_SO_NORMALIZE, point, guess, found, sol)`:
/// the solution's start parameters when found.
fn surface_local_point_solve_normalize(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    point: Point3,
    guess: Point2,
) -> Result<Option<[f64; 2]>> {
    let r = surface.local_point_solve::<crate::surface::surface::p4::NoObject>(
        k,
        domain,
        SolverOperation::Normalize,
        point,
        guess,
    )?;
    if r.found {
        Ok(Some([
            r.solution.start.param(0)?,
            r.solution.start.param(1)?,
        ]))
    } else {
        Ok(None)
    }
}

/// `surface.LocalSurfaceSolve(domain, other, other_domain, SM_SO_NORMALIZE, tol, NULL,
/// NULL, guess, other_guess, found, sol)`: the solution's start parameters `[u0, v0,
/// u1, v1]` when found.
fn surface_local_surface_solve_normalize(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    other: &Surface,
    other_domain: &Extent2d,
    _tol: f64,
    guess: Point2,
    other_guess: Point2,
) -> Result<Option<[f64; 4]>> {
    Ok(surface
        .local_surface_solve(
            k,
            domain,
            other,
            other_domain,
            SolverOperation::Normalize,
            &[],
            guess,
            other_guess,
        )?
        .map(|s| [s.uv.x, s.uv.y, s.other_uv.x, s.other_uv.y]))
}

/// [`SurfaceIntersector::compute_law_point_values`] on the intersector of `v`, its
/// virtual calls (`EvaluateLawPoint`, `ComputePointValuesDelta`) dispatched to `v`,
/// with the guess UV points `guess`.
///
/// # Errors
///
/// As [`SurfaceIntersector::compute_law_point_values`], with the errors of `v`'s
/// overrides.
#[allow(clippy::too_many_arguments)]
pub fn compute_law_point_values_with<'s, V: IntersectorVirtuals<'s> + ?Sized>(
    v: &mut V,
    k: &Kernel,
    parameter: f64,
    guess: LawGuess,
    delta_step: f64,
    law: &LawPointOptions<'_>,
    law_curve: &mut FilletLaw,
    skip_computing_uvs: bool,
    point: &mut TsectPnt,
) -> Result<()> {
    let mut delta_step = delta_step;
    let ivl = &law.point_curve_interval;

    // for 6 iterations
    for _ in 0..6 {
        // the guesses (read through the references on every try)
        let [uv0, uv1] = match guess {
            LawGuess::Given(uvs) => uvs,
            LawGuess::PointUvs => [
                point.uv_pos(SrfIndex::First),
                point.uv_pos(SrfIndex::Second),
            ],
        };

        // the plane and delta plane to intersect (statuses ignored; passed on with
        // `ssi_proto_topology_statuses_pass_on`, fork bcc6f01)
        let pass = k.fixes.ssi_proto_topology_statuses_pass_on;
        let delta_param = parameter + delta_step;
        let mut pv = [Vec3::UNDEF; 2];
        let mut pv_delta = [Vec3::UNDEF; 2];
        if let Some(r) = ignored(passed_on(
            law.point_curve.evaluate(k, parameter, 1, true, true),
            pass,
        )?)? {
            for (d, s) in pv.iter_mut().zip(r) {
                *d = s;
            }
        }
        if let Some(r) = ignored(passed_on(
            law.point_curve.evaluate(k, delta_param, 1, true, true),
            pass,
        )?)? {
            for (d, s) in pv_delta.iter_mut().zip(r) {
                *d = s;
            }
        }

        if law.average_normals {
            let [n0, n1] = law.normals;
            let inv = ivl.inversion(parameter);
            // `SER( Inversion(dParameter, ..) )`: SM_ERR outside the interval or for a
            // zero-length one
            if pass && !inv.is_success() {
                return Err(SolidError::intersect_failed(
                    OP_LAW,
                    "the law curve's normal could not be inverted",
                ));
            }
            let t = inv.parameter.unwrap_or(UNDEF_DOUBLE);
            pv[1] = n0 * (1.0 - t) + n1 * t;
            let inv = ivl.inversion(delta_param);
            // the step may end just past the interval, where the unclamped parameter
            // is set and extrapolates as wanted: only an undefined one is passed on
            if pass && inv.parameter.is_none() {
                return Err(SolidError::intersect_failed(
                    OP_LAW,
                    "the law curve's normal inversion gave no parameter",
                ));
            }
            let t = inv.parameter.unwrap_or(UNDEF_DOUBLE);
            pv_delta[1] = n0 * (1.0 - t) + n1 * t;
        }

        // unitize the plane normals
        pv[1] = pv[1].unitize()?;
        pv_delta[1] = pv_delta[1].unitize()?;

        // the surface/surface/plane intersections
        let uvs = if skip_computing_uvs {
            [
                point.uv_pos(SrfIndex::First),
                point.uv_pos(SrfIndex::Second),
            ]
        } else {
            let (curve_param, uvs) = v.evaluate_law_point(
                k,
                parameter,
                uv0,
                uv1,
                law_curve,
                law.law_orient,
                ivl,
                law.surface_orientations,
                pv[0],
                pv[1],
            )?;
            // (an override that leaves the curve parameter unset: undefined)
            if let Some(u) = point.user_doubles.get_mut(0) {
                *u = curve_param.unwrap_or(UNDEF_DOUBLE);
            }
            uvs
        };
        let (curve_delta_param, uv_deltas) = v.evaluate_law_point(
            k,
            delta_param,
            uv0,
            uv1,
            law_curve,
            law.law_orient,
            ivl,
            law.surface_orientations,
            pv_delta[0],
            pv_delta[1],
        )?;

        // build the delta point
        let mut delta_pnt = TsectPnt::new();
        delta_pnt.curve_parameter = point.curve_parameter + delta_step;
        delta_pnt.user_doubles[0] = curve_delta_param.unwrap_or(UNDEF_DOUBLE);
        delta_pnt.user_doubles[1] = delta_param;

        // surface and curve values for both parameters (any failure retries;
        // `Unported` propagates)
        match v.compute_point_values_delta(k, uvs, uv_deltas, point, &mut delta_pnt) {
            Ok(()) => return Ok(()),
            Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => return Err(e),
            Err(_) => {}
        }

        // next iteration with a bigger delta step
        delta_step *= 10.0;
    }
    Err(SolidError::intersect_failed(
        OP_LAW,
        "no delta step gives the law point's values",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::plane::Plane;

    fn plane(o: [f64; 3], x: [f64; 3], y: [f64; 3]) -> Surface {
        Surface::from(Plane::new(
            Vec3::from_array(o),
            Vec3::from_array(x),
            Vec3::from_array(y),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ))
    }

    fn two_planes() -> (Surface, Surface) {
        (
            plane([0.0, -0.5, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            plane([0.0, 0.0, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        )
    }

    #[test]
    fn crossing_point_of_two_planes() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let si = SurfaceIntersector::new(k, &a, &d, &b, &d, false).unwrap();
        let mut p = TsectPnt::new();
        si.compute_point_values(
            &Kernel::default(),
            [Point2::new(0.3, 0.5), Point2::new(0.3, 0.5)],
            &mut p,
            None,
            None,
        )
        .unwrap();
        assert_eq!(p.point_type, IntersectionPointType::Crossing);
        assert_eq!(p.crv_pos(), Vec3::new(0.3, 0.0, 0.0));
        assert!((p.crv_deriv().length() - 1.0).abs() < 1e-15);
        assert!((p.crv_deriv().x.abs() - 1.0).abs() < 1e-15);
        assert!((p.tangent_plane_angle_rad - PI / 2.0).abs() < 1e-15);
        assert!(!si
            .classify_boundary_start_point_intersection(&Kernel::default(), &p)
            .unwrap());
    }

    #[test]
    fn parallel_planes_make_a_tangent_point() {
        let k = &Kernel::default();
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let si = SurfaceIntersector::new(k, &a, &d, &b, &d, false).unwrap();
        let mut p = TsectPnt::new();
        // no previous point: the singularity handling is SM_ERR
        assert!(si
            .compute_point_values(
                &Kernel::default(),
                [Point2::new(0.3, 0.5), Point2::new(0.3, 0.5)],
                &mut p,
                None,
                None
            )
            .is_err());
        p.point_type = IntersectionPointType::TangentPoint;
        assert!(si
            .classify_boundary_start_point_intersection(&Kernel::default(), &p)
            .unwrap());
    }

    #[test]
    fn flush_single_and_traced_points() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut si = SurfaceIntersector::new(k, &a, &d, &b, &d, false).unwrap();
        let mut out = TsectCurves::default();
        si.flush_curve(k, &mut out).unwrap();
        assert!(out.curves_3d.is_empty());

        let mut ids = Vec::new();
        for i in 0..4 {
            let t = 0.25 * f64::from(i);
            let mut p = TsectPnt::new();
            si.compute_point_values(
                &Kernel::default(),
                [Point2::new(t, 0.5), Point2::new(t, 0.5)],
                &mut p,
                None,
                None,
            )
            .unwrap();
            p.curve_parameter = t;
            ids.push(si.alloc_point(p));
        }
        let first = ids[0];
        si.add_point_to_curve(first).unwrap();
        si.flush_curve(k, &mut out).unwrap();
        assert_eq!(out.curves_3d.len(), 1);
        assert_eq!(out.curve_types, vec![TsectCurveType::Touching]);
        assert!(si.curve_points().is_empty());

        for &id in &ids {
            si.add_point_to_curve(id).unwrap();
        }
        si.flush_curve(k, &mut out).unwrap();
        assert_eq!(out.curves_3d.len(), 2);
        assert_eq!(out.uv_curves[0].len(), 2);
        assert_eq!(out.curve_types[1], TsectCurveType::Crossing);

        // the single point is no longer unique
        si.add_point_to_curve(first).unwrap();
        si.flush_curve(k, &mut out).unwrap();
        assert_eq!(out.curves_3d.len(), 2);
    }

    #[test]
    fn unique_points() {
        let k = &Kernel::default();
        let p = TsectPnt::new();
        assert!(is_point_unique(k, Vec3::new(0.0, 0.0, 0.0), &[], &[], 1e-3).unwrap());
        let mut q = p.clone();
        q.curve_pv[0] = Vec3::new(0.0, 0.0, 1e-4);
        assert!(!is_point_unique(k, Vec3::new(0.0, 0.0, 0.0), &[&q], &[], 1e-3).unwrap());
    }

    #[test]
    fn plane_boundary_test_of_perpendicular_planes_is_unknown() {
        let k = &Kernel::default();
        let (a, b) = two_planes();
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let si = SurfaceIntersector::new(k, &a, &d, &b, &d, false).unwrap();
        let k = Kernel::default();
        let mut p1 = PatchBoundaryBoundingPlanes::new(d);
        let mut p2 = PatchBoundaryBoundingPlanes::new(d);
        let mut out = TsectCurves::default();
        let bbox = Extent3d::from_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let r = si.do_boundary_plane_test(&k, &mut p1, &mut p2, 1e-3, &bbox, &mut out);
        // (the bounding planes of a plane go through `SmPlane::CreateIsoParametricCurve`,
        // surface.plane_p1, ported): no pair of boundary planes has opposite normals
        assert_eq!(r.unwrap(), BoundaryPlaneTestResult::Unknown);
    }

    /// The r 6 ball about (0, 50 + `delta`, 20), its poles on z and its seam turned `turn`
    /// from +x, and the r 50 cylinder about z: the line where they meet passes each pole
    /// `delta` from it. Returns the largest angle, over the traced curve's knots off the
    /// ball's poles, between the image of the UV curve's tangent on the ball
    /// (`S_u u' + S_v v'`, the chain rule at the knot's own UV point) and the 3D curve's
    /// tangent there, from either side of the knot, with how many knots lie within 1e-3
    /// of a pole.
    fn knot_image_turn(delta: f64, turn: f64) -> (f64, usize) {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::surface::cone::Cone;
        use crate::surface::sphere::Sphere;
        let k = &Kernel::default();
        let (sn, cs) = turn.sin_cos();
        let ball = Surface::from(
            Sphere::create_canonical(
                k,
                &Axis2Placement::new(
                    Vec3::new(0.0, 50.0 + delta, 20.0),
                    Vec3::new(cs, sn, 0.0),
                    Vec3::new(-sn, cs, 0.0),
                ),
                6.0,
            )
            .unwrap(),
        );
        let wall = Surface::from(
            Cone::create_canonical_cylinder(
                k,
                &Axis2Placement::new(
                    Vec3::ZERO,
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                ),
                50.0,
            )
            .unwrap(),
        );
        let ball_domain = ball.natural_uv_domain_with_nurb(k).unwrap();
        let wall_domain = wall.natural_uv_domain_with_nurb(k).unwrap();
        let (out, status) = ball.global_surface_intersect_with_partial(
            k,
            &ball_domain,
            &wall,
            &wall_domain,
            [true, true],
            None,
            None,
        );
        status.unwrap();
        let poles = [
            Vec3::new(0.0, 50.0 + delta, 14.0),
            Vec3::new(0.0, 50.0 + delta, 26.0),
        ];
        let (mut worst, mut near): (f64, usize) = (0.0, 0);
        for (c3, uv) in out.curves_3d.iter().zip(&out.surface1_uv_curves) {
            let uv = uv.as_ref().unwrap();
            let (knots, _) = c3.knots(k, None).unwrap();
            for &t in &knots {
                for from_left in [true, false] {
                    let d3 = c3.evaluate(k, t, 1, from_left, false).unwrap();
                    let d2 = uv.evaluate(k, t, 1, from_left, false).unwrap();
                    let at = Point2::new(d2[0].x, d2[0].y);
                    let on_pole = [ball_domain.min().y, ball_domain.max().y]
                        .iter()
                        .any(|&b| (at.y - b).abs() <= 1e-9 * (1.0 + b.abs()));
                    if on_pole {
                        continue;
                    }
                    if poles.iter().any(|p| p.distance(d3[0]) <= 1e-3) {
                        near += 1;
                    }
                    let m = ball.evaluate(k, at, 2, 2, LEFT_UPPER).unwrap();
                    let (su, sv) = (m[1][0], m[0][1]);
                    // a knot whose own tangent is not certified to 1e-6 may have taken the
                    // step-back: its tangent need only be finite
                    let t = d3[1].unitize_or_z();
                    let certified =
                        sphere_derivative_errors(&ball, at, su, sv).is_some_and(|(eu, ev)| {
                            let own = drop_vectors(su, sv, &[t]).unwrap()[0];
                            tangent_error_bounds(su, sv, eu, ev, t, own).1 <= 1e-6
                        });
                    if !certified {
                        if !(d2[1].x.is_finite() && d2[1].y.is_finite()) {
                            worst = f64::NAN;
                        }
                        continue;
                    }
                    let image = su * d2[1].x + sv * d2[1].y;
                    if image.length() == 0.0 || d3[1].length() == 0.0 {
                        continue;
                    }
                    let turn = image.angle_between(d3[1]).unwrap();
                    worst = if turn.is_nan() || worst.is_nan() {
                        f64::NAN
                    } else {
                        worst.max(turn)
                    };
                }
            }
        }
        (worst, near)
    }

    /// A trace point near a pole but off it keeps the UV tangent of its own UV point. A point
    /// within about 1e-4 of a sphere's pole passes `IsSingularity`'s ratio test (`|S_u|` far below
    /// `|S_v|`), and the step-back from a singularity replaced its singular parameter and its
    /// surface derivatives with those of a point stepped back along the span, so its UV
    /// tangent no longer mapped onto its 3D tangent (0.04 to 0.66 rad off before this
    /// change, in each case below). There the chain rule holds at the point itself, so the
    /// image of the UV tangent is the 3D tangent up to rounding; the bound 1e-5 rad is a
    /// margin over the worst measured with the change (2.2e-6, where `|S_u|` is 1e-4 of
    /// `|S_v|`), far below the turn before it. Each case puts knots within 1e-3 of a pole.
    #[test]
    fn a_trace_point_near_a_pole_keeps_its_own_uv_tangent() {
        let mut failed = Vec::new();
        let cases = [
            (2e-5, 0.0),
            (-2e-5, 0.0),
            (-2e-5, PI / 2.0),
            (-1.2e-5, 0.0),
            (-1.2e-5, PI / 2.0),
            (-1e-5, 0.0),
            (-1e-5, PI / 2.0),
        ];
        for (delta, turn) in cases {
            let (worst, near) = knot_image_turn(delta, turn);
            // (written to accept: a NaN fails)
            if !(worst <= 1e-5 && near > 0) {
                failed.push(format!(
                    "delta {delta:e} turn {turn:.3}: {worst:e} rad, {near} near"
                ));
            }
        }
        // the pole 1e-9 off the line: knots too close to it for a well-conditioned solve
        // are stepped back from and keep finite tangents; the others keep their own
        for turn in [0.0, PI / 2.0] {
            let (worst, _) = knot_image_turn(1e-9, turn);
            // (written to accept: a NaN fails)
            let ok = worst <= 1e-5;
            if !ok {
                failed.push(format!("delta 1e-9 turn {turn:.3}: {worst:e} rad"));
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
    }

    /// The derivatives and their rigorous error bounds ([`sphere_derivative_errors`]) at
    /// `uv` on `ball`, evaluated as `ComputePointValues` does.
    fn ball_derivs(k: &Kernel, ball: &Surface, uv: Point2) -> (Vec3, Vec3, f64, f64) {
        let m = ball.evaluate(k, uv, 2, 2, LEFT_UPPER).unwrap();
        let (su, sv) = (m[1][0], m[0][1]);
        let (eu, ev) = sphere_derivative_errors(ball, uv, su, sv).unwrap();
        (su, sv, eu, ev)
    }

    /// The derivative error bound against independent properties. On the r 6 ball at the
    /// origin, the r 6 ball about (0, 50, 20), the r 1 ball about (1000, 0, 0) and the r 6
    /// ball remapped onto the uneven domain [0, 1] x [0, 100], at points from 1e-6 of the
    /// domain from a pole to the equator:
    /// - a sphere's partials are tangent to it, so their radial components (along the
    ///   closed-form normal `(S - c) / R`, itself off by the point's rounding) are errors:
    ///   they stay within the bound plus that rounding;
    /// - the bound is parameter-invariant: on the remapped ball, at the same points, each
    ///   relative bound `err / |S|` agrees with the unremapped ball's within a factor 4;
    /// - the bound is a few hundred `ε` of the net's homogeneous scale at most (it does
    ///   not blow up), and on the far ball it exceeds `8 ε max(|S_u|, |S_v|)` (the
    ///   cancellation it counts is real).
    #[test]
    fn the_sphere_derivative_bound_holds_and_is_parameter_invariant() {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::surface::sphere::Sphere;
        let k = &Kernel::default();
        let make = |centre: Vec3, radius: f64| {
            Sphere::create_canonical(
                k,
                &Axis2Placement::new(centre, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
                radius,
            )
            .unwrap()
        };
        let fractions = [1e-6, 1e-4, 1e-2, 0.25, 0.5, 1.0 - 1e-5];
        let mut far_counts = false;
        let mut relative = Vec::new();
        for (centre, radius) in [
            (Vec3::ZERO, 6.0),
            (Vec3::new(0.0, 50.0, 20.0), 6.0),
            (Vec3::new(1000.0, 0.0, 0.0), 1.0),
        ] {
            let ball = Surface::from(make(centre, radius));
            let dom = ball.natural_uv_domain_with_nurb(k).unwrap();
            let (w, h) = (dom.max().x - dom.min().x, dom.max().y - dom.min().y);
            for fu in [0.1, 0.37, 0.8] {
                for &fv in &fractions {
                    let uv = Point2::new(dom.min().x + fu * w, dom.min().y + fv * h);
                    let m = ball.evaluate(k, uv, 2, 2, LEFT_UPPER).unwrap();
                    let (su, sv, eu, ev) = ball_derivs(k, &ball, uv);
                    let n = (m[0][0] - centre) / radius;
                    let slack = 8.0 * f64::EPSILON * (centre.length() + radius) / radius;
                    assert!(
                        su.dot(n).abs() <= eu + slack * su.length(),
                        "{centre:?} {uv:?}"
                    );
                    assert!(
                        sv.dot(n).abs() <= ev + slack * sv.length(),
                        "{centre:?} {uv:?}"
                    );
                    let longest = su.length().max(sv.length());
                    let homogeneous = (centre.length() + 2.0 * radius) / radius * longest;
                    assert!(
                        eu.max(ev) <= 512.0 * f64::EPSILON * homogeneous,
                        "{centre:?} {uv:?}"
                    );
                    if eu.max(ev) > 8.0 * f64::EPSILON * longest {
                        far_counts |= centre.x > 0.0;
                    }
                    if centre == Vec3::ZERO {
                        relative.push((uv, fu, fv, eu / su.length(), ev / sv.length()));
                    }
                }
            }
        }
        assert!(far_counts);
        // the same ball on an uneven domain
        let mut remapped = make(Vec3::ZERO, 6.0);
        remapped
            .reparameterize(k, &Extent2d::from_bounds(0.0, 0.0, 1.0, 100.0))
            .unwrap();
        let remapped = Surface::from(remapped);
        let dom = remapped.natural_uv_domain_with_nurb(k).unwrap();
        for (_, fu, fv, ru, rv) in relative {
            let uv = Point2::new(
                dom.min().x + fu * (dom.max().x - dom.min().x),
                dom.min().y + fv * (dom.max().y - dom.min().y),
            );
            let (su, sv, eu, ev) = ball_derivs(k, &remapped, uv);
            let (qu, qv) = (eu / su.length(), ev / sv.length());
            let close = |a: f64, b: f64| a <= 4.0 * b && b <= 4.0 * a;
            assert!(
                close(qu, ru) && close(qv, rv),
                "{fu} {fv}: {qu:e} {ru:e} {qv:e} {rv:e}"
            );
        }
    }

    /// The tangent error bounds against closed forms. With `S_u = (1, 0, 0)`,
    /// `S_v = (0, 2, 0)` and `T = (0.5, 0.75, 0)` (all exact in binary), the chain rule's
    /// solution is `(0.5, 0.375)` exactly: its residual is 0, so the bounds are
    /// `[0, 0.5 e_u + 0.375 e_v]` for derivative errors `e_u`, `e_v`. A tangent off by
    /// `2^-10` in `u'` has the residual `2^-10` exactly, so its bounds are
    /// `2^-10 ∓ (e_u |u'| + e_v |v'|)`. `DropVectors`' own solution
    /// in the band where the regular solve refuses (`|S_u| = |S_v| = 1e-11`,
    /// `sin θ = 7e-3`, `T` along `S_u`'s normal in the plane: `|u'|` past
    /// `1 / SM_EFF_ZERO`) is the projection fallback, and its bounds show it (an error above
    /// 0.1), where at 100 times those lengths its error is certified below 1e-9. A NaN gives
    /// no bound.
    #[test]
    fn the_tangent_error_bounds_match_closed_forms() {
        let (su, sv) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0));
        let t = Vec3::new(0.5, 0.75, 0.0);
        let (eu, ev) = (1e-9, 2e-9);
        let exact = Vec2::new(0.5, 0.375);
        let (lo, hi) = tangent_error_bounds(su, sv, eu, ev, t, exact);
        let errs = 0.5 * eu + 0.375 * ev;
        assert!(
            lo == 0.0 && hi >= errs && hi <= errs * (1.0 + 1e-12) + 1e-15,
            "{lo:e} {hi:e}"
        );
        let step = 2f64.powi(-10);
        let off = Vec2::new(0.5 + step, 0.375);
        let (lo, hi) = tangent_error_bounds(su, sv, eu, ev, t, off);
        let errs = (0.5 + step) * eu + 0.375 * ev;
        assert!(
            (lo - (step - errs)).abs() <= 1e-15 && (hi - (step + errs)).abs() <= 1e-15,
            "{lo:e} {hi:e}"
        );
        // the refusal band, through `DropVectors`
        let sin: f64 = 7e-3;
        let x = Vec3::new(1.0, 0.0, 0.0);
        let leaning = Vec3::new((1.0 - sin * sin).sqrt(), sin, 0.0);
        let t = Vec3::new(0.0, 1.0, 0.0);
        for (len, refused) in [(1e-11, true), (1e-9, false)] {
            let (a, b) = (x * len, leaning * len);
            let d = drop_vectors(a, b, &[t]).unwrap()[0];
            let (lo, hi) = tangent_error_bounds(a, b, 0.0, 0.0, t, d);
            if refused {
                assert!(lo > 0.1, "{len:e}: {lo:e}");
            } else {
                assert!(hi < 1e-9, "{len:e}: {hi:e}");
            }
        }
        let (lo, hi) = tangent_error_bounds(su, sv, f64::NAN, 0.0, t, exact);
        assert!(lo == 0.0 && hi == f64::INFINITY);
    }

    /// The kind filter: the r 6 ball about (0, 50, 20) and its own net as a B-spline surface
    /// (`as_b_spline`: the same net `Surface::evaluate` uses for the sphere, so the same
    /// derivatives) differ only in kind. At a point 1e-6 of the domain from the pole, which
    /// `IsSingularity` calls singular, the sphere keeps its own derivatives and the
    /// B-spline takes the step-back, as upstream (no derivative model for a B-spline).
    #[test]
    fn a_nurbs_ball_near_its_pole_takes_the_step_back() {
        use crate::blend::fillet_section::station::as_b_spline;
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::surface::sphere::Sphere;
        let k = &Kernel::default();
        let ball = Surface::from(
            Sphere::create_canonical(
                k,
                &Axis2Placement::new(
                    Vec3::new(0.0, 50.0, 20.0),
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                ),
                6.0,
            )
            .unwrap(),
        );
        let nurbs = Surface::from(as_b_spline(k, &ball).unwrap().unwrap().into_owned());
        let other = plane([0.0, 0.0, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        for (surface, keeps) in [(&ball, true), (&nurbs, false)] {
            let dom = surface.natural_uv_domain_with_nurb(k).unwrap();
            let si = SurfaceIntersector::new(k, surface, &dom, &other, &d, false).unwrap();
            let (w, h) = (dom.max().x - dom.min().x, dom.max().y - dom.min().y);
            let uv = Point2::new(dom.min().x + 0.3 * w, dom.min().y + 1e-6 * h);
            let (sing, dir) = is_singularity(k, surface, uv).unwrap();
            assert!(sing, "{keeps}: {dir:?}");
            let m = surface.evaluate(k, uv, 2, 2, LEFT_UPPER).unwrap();
            let (su, sv) = (m[1][0], m[0][1]);
            assert_eq!(
                si.near_a_pole_off_it(k, SrfIndex::First, dir, uv, su, sv)
                    .unwrap()
                    .is_some(),
                keeps
            );
        }
    }

    /// A fold in the half of the domain nearer a pole is not near a pole. The quadratic
    /// patch whose first and last u rows coincide folds back along u = 1/2 (`S_u` vanishes
    /// there), and its v = 1 side is a pole. A point on the fold at v = 0.9 is singular and
    /// in the half nearer the pole, but `S_u` vanishes there: the derivatives
    /// `ComputePointValues` evaluates (zero tangents replaced) are not the surface's own,
    /// and the point is stepped back from. A point 1e-9 off the fold is stepped back from
    /// too (a B-spline takes the step-back). A point 2e-5 from a sphere's pole keeps its own derivatives.
    /// All derivatives are evaluated as `ComputePointValues` does (to second order).
    #[test]
    fn a_fold_near_a_pole_side_is_stepped_back_from() {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::nurbs::b_spline_curve::p1::KnotType;
        use crate::nurbs::b_spline_surface::{
            BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
        };
        use crate::surface::sphere::Sphere;
        let k = &Kernel::default();
        let pole = Point3::new(0.0, 5.0, 0.0);
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            pole,
            Point3::new(1.0, 0.0, 1.0),
            pole,
            Point3::new(0.0, 0.0, 0.0),
            pole,
        ];
        let fold = Surface::from(
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: 2,
                v_degree: 1,
                control_points: &points,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &[3, 3],
                v_multiplicities: &[2, 2],
                u_knots: &[0.0, 1.0],
                v_knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: None,
                uv_domain: None,
            })
            .unwrap(),
        );
        let other = plane([0.0, 0.0, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let d = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let si = SurfaceIntersector::new(k, &fold, &d, &other, &d, false).unwrap();
        for u in [0.5, 0.5 + 1e-9] {
            let uv = Point2::new(u, 0.9);
            let (sing, dir) = is_singularity(k, &fold, uv).unwrap();
            assert!(sing, "{u}: {dir:?}");
            let m = fold.evaluate(k, uv, 2, 2, LEFT_UPPER).unwrap();
            let (su, sv) = (m[1][0], m[0][1]);
            assert!(
                si.near_a_pole_off_it(k, SrfIndex::First, dir, uv, su, sv)
                    .unwrap()
                    .is_none(),
                "{u}"
            );
        }

        let ball =
            Surface::from(Sphere::create_canonical(k, &Axis2Placement::default(), 6.0).unwrap());
        let bd = ball.natural_uv_domain_with_nurb(k).unwrap();
        let si = SurfaceIntersector::new(k, &ball, &bd, &other, &d, false).unwrap();
        let v = bd.min().y + 2e-5 / 6.0 * (bd.max().y - bd.min().y) / PI;
        let uv = Point2::new(bd.u_interval().mid(), v);
        let (sing, dir) = is_singularity(k, &ball, uv).unwrap();
        assert!(sing, "{dir:?}");
        let m = ball.evaluate(k, uv, 2, 2, LEFT_UPPER).unwrap();
        let (su, sv) = (m[1][0], m[0][1]);
        assert!(si
            .near_a_pole_off_it(k, SrfIndex::First, dir, uv, su, sv)
            .unwrap()
            .is_some());
    }
}
