//! `SmSurfaceIntersector`, part 4: tracing one intersection curve from a start point
//! ([`SurfaceIntersector::trace_curve`]), the search for closed interior curves
//! ([`SurfaceIntersector::find_interior_curves`]) and the two single-curve drivers,
//! through given points ([`SurfaceIntersector::do_point_intersection`]) and along a
//! fillet law ([`SurfaceIntersector::do_law_intersection`]).
//!
//! Upstream: `source/SMLib/src/SmSurfaceIntersector.cpp`.
//!
//! | Rust                                                  | Upstream                      |
//! |-------------------------------------------------------|-------------------------------|
//! | [`SurfaceIntersector::trace_curve`]                   | `TraceCurve`                  |
//! | [`SurfaceIntersector::find_interior_curves`], [`SurfaceIntersector::find_interior_curves_from_node_pairs`], [`InteriorNodePair`] | `FindInteriorCurves` |
//! | [`SurfaceIntersector::do_point_intersection`], [`PointIntersectionOptions`] | `DoPointIntersection` |
//! | [`SurfaceIntersector::do_law_intersection`], [`LawIntersectionOptions`] | `DoLawIntersection` |
//! | [`IntersectionCurve`]                                 | the curve, type and deviation out-params of both drivers |
//!
//! Deviations:
//! - **Points.** Points are arena ids ([`TsectId`]), as in parts 1 to 3: the start
//!   point of `TraceCurve`, the input points of the drivers (`crTsectPoints`) and the
//!   points upstream takes from `m_vTSPntMgr` (zero-filled, [`TsectPnt::zeroed`]) or
//!   builds on the stack (`sTempTSP`, `sTempEndTSP`: default points; the end point
//!   `sEndTSP` of the singular-point retry becomes an arena point). Linking a point
//!   that is already on a list (undefined upstream) is
//!   [`SolidError::InvalidInput`], as in part 1.
//! - **Outputs.** Out-params become return values: the drivers return an
//!   [`IntersectionCurve`] whose members are `None` where upstream leaves the
//!   out-param `NULL` or unset (more or fewer than one curve flushed). On an error
//!   no curve is returned (upstream may already have handed the caller a curve).
//!   `FindInteriorCurves`' output arrays are a [`TsectCurves`] (part 2).
//! - **Virtual calls.** `ComputeNextPoint`, `FlushCurve`, `ComputePointValues` and
//!   `TestSpanAccuracy` are the basic intersector's (`SmFilletIntersector`'s
//!   overrides belong to its own unit).
//! - **Bounded loops.** `TraceCurve`'s step loops end on finite step sizes within a
//!   few thousand halvings or growths, and the trace on its 5000th point (`SM_ERR`
//!   upstream); a NaN step (from NaN tolerances) would loop forever upstream. Here
//!   each step loop stops after [`MAX_STEP_TRIES`] tries and the trace after
//!   [`MAX_TRACE_ITERATIONS`] steps (also counting the snapped points upstream does
//!   not count) with [`SolidError::InvalidInput`]. `DoLawIntersection`'s span
//!   subdivision stops at [`MAX_LAW_POINTS`] points the same way.
//! - **Recursion.** `DoPointIntersection` retries once from a singular end point
//!   through itself, guarded by the function-static `bInsideMe` (set for the nested
//!   call by an `SmTemporaryChangeValue`); here the flag is a parameter of the
//!   private body, so the depth is at most 1.
//! - **Unused arguments.** `DoPointIntersection`'s context (only stored in
//!   `m_cpContext`, not modelled) and `DoLawIntersection`'s context, uniform-steps
//!   flag and angle tolerance (unused upstream) are dropped. `DoLawIntersection`
//!   takes the fillet law (`SmFilletLaw`, not ported) as an opaque argument, as part
//!   2 does.
//! - **Interior node pairs.** `FindInteriorCurves` walks the tree node pairs of an
//!   `SM_SR_NODES` global surface solve, reading each node's box and Bezier patch UV
//!   domain: [`Surface::global_surface_solve_with`] returns them with the solutions,
//!   and the walk over the pairs is
//!   [`SurfaceIntersector::find_interior_curves_from_node_pairs`].
//! - **Offset surfaces.** `DoLawIntersection` needs both surfaces to be
//!   `SmOffsetSurface`s ([`SurfaceKind::OffsetSurface`], else `SM_ERR_NULL_POINTER`).
//!   Past that check its law points wait on `SmFilletLaw::Evaluate`
//!   (`blend.fillet_executive_p1`).
//! - **Unported callees.** `SmSurface::IsSingularity` of a surface that is not a plane
//!   (`surface.surface_p5`) and the callees of `SmSurface::LocalSurfaceIntersect` and of
//!   the global surface solve's leaf solve that are not ported yet (`surface.surface_p3`,
//!   `surface.surface_p8`) return [`SolidError::Unported`], which always propagates
//!   (also where upstream ignores the status).
//! - **Not ported here.** The `SM_DEBUG_CODE` blocks (graphics, dumps), the
//!   `SM_ASSERT`s and the point-count warnings (`MSG`).

use crate::curve::curve::p5::make_curve_cache;
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::hermite_curve::HermiteCurve;
use crate::error::{ignored, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::solution_array::{ObjectKind, SolutionObject};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::math::{EFF_ZERO, EFF_ZERO_SQRT};
use crate::surface::surface::p2::TsectCurveType;
use crate::surface::surface::{Surface, SurfaceKind};

use super::p1::{
    surface_local_surface_intersect, IntersectionPointType, SrfIndex, SurfaceIntersector, TsectId,
    TsectPnt,
};
use super::p2::{LawPointOptions, TsectCurves};

const OP_TRACE: &str = "intersect::surface_intersector::p4::SurfaceIntersector::trace_curve";
const OP_FIC: &str = "intersect::surface_intersector::p4::SurfaceIntersector::find_interior_curves";
const OP_DPI: &str =
    "intersect::surface_intersector::p4::SurfaceIntersector::do_point_intersection";
const OP_DLI: &str = "intersect::surface_intersector::p4::SurfaceIntersector::do_law_intersection";

/// The tries of one step-size loop of [`SurfaceIntersector::trace_curve`]: a finite
/// positive step leaves `[100 SM_EFF_ZERO, f64::MAX]` within about 2500 halvings or
/// growths by 1.5.
pub const MAX_STEP_TRIES: usize = 4096;

/// The steps of one [`SurfaceIntersector::trace_curve`] call (upstream fails on the
/// 5000th counted point).
pub const MAX_TRACE_ITERATIONS: usize = 100_000;

/// The points of a [`SurfaceIntersector::do_law_intersection`] subdivision.
pub const MAX_LAW_POINTS: usize = 100_000;

/// The single intersection curve of [`SurfaceIntersector::do_point_intersection`] /
/// [`SurfaceIntersector::do_law_intersection`]; each member is `None` where upstream
/// leaves the out-param `NULL` or unset (unless exactly one curve was flushed).
#[derive(Debug, Clone, Default)]
pub struct IntersectionCurve {
    /// `rp3DCurve`: the 3D curve.
    pub curve_3d: Option<Curve>,
    /// `rpSurface1UVCurve`, `rpSurface2UVCurve`: the UV curves on each surface.
    pub uv_curves: [Option<Curve>; 2],
    /// `reCurveType`.
    pub curve_type: Option<TsectCurveType>,
    /// `rdDeviation`: the largest curve/surface distance.
    pub deviation: Option<f64>,
}

impl IntersectionCurve {
    /// The single curve of `out` (`if (size == 1) out = array[0]` for each array).
    fn from_single(out: TsectCurves) -> Self {
        fn single<T>(mut v: Vec<T>) -> Option<T> {
            if v.len() == 1 {
                v.pop()
            } else {
                None
            }
        }
        let TsectCurves {
            curves_3d,
            uv_curves: [uv0, uv1],
            curve_types,
            deviations,
        } = out;
        Self {
            curve_3d: single(curves_3d),
            uv_curves: [single(uv0), single(uv1)],
            curve_type: single(curve_types),
            deviation: single(deviations),
        }
    }
}

/// The options of [`SurfaceIntersector::do_point_intersection`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PointIntersectionOptions {
    /// `bExtendBeforeStart`: also trace backwards from the first point.
    pub extend_before_start: bool,
    /// `bExtendAfterEnd`: the last point does not end the curve.
    pub extend_after_end: bool,
    /// `pOptStartDirection`: the direction to trace from the first point.
    pub start_direction: Option<Vec3>,
    /// `pOptEndDirection`: the curve direction at the last point.
    pub end_direction: Option<Vec3>,
    /// `pOptApproxTol3d`: replaces `this_approx_tol_3d`.
    pub approx_tol_3d: Option<f64>,
    /// `pdOptAngTolRad`: replaces `this_ang_tol_rad`.
    pub ang_tol_rad: Option<f64>,
}

/// The options of [`SurfaceIntersector::do_law_intersection`].
#[derive(Debug, Clone, Copy)]
pub struct LawIntersectionOptions<'a, L: ?Sized> {
    /// `crLawCurve`: the fillet law (`SmFilletLaw`, opaque: not ported).
    pub law: &'a L,
    /// `bLawOrient`: the law runs from the interval end to its start.
    pub law_orient: bool,
    /// `crPointCurve`: the edge curve (arc length parameterised).
    pub point_curve: &'a Curve,
    /// `crPointCurveInterval`: the range of the fillet edge.
    pub point_curve_interval: Extent1d,
    /// `dSurfaceOrientations`: the offset directions of the two surfaces (`1` or `-1`).
    pub surface_orientations: [f64; 2],
    /// `bUseNormalPlaneAveraging`: interpolate the section plane normals between the
    /// end points' curve tangents.
    pub use_normal_plane_averaging: bool,
    /// `pOptApproxTol3d`: the tolerance (`1e-3` when `None`).
    pub approx_tol_3d: Option<f64>,
}

/// A tree node pair of the global surface solve in
/// [`SurfaceIntersector::find_interior_curves`]: the boxes of both nodes and the UV
/// domains of their Bezier patches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteriorNodePair {
    /// `pNode1->m_sBBox`, `pNode2->m_sBBox`.
    pub bboxes: [Extent3d; 2],
    /// `pBezPatch1->GetUVDomain()`, `pBezPatch2->GetUVDomain()`.
    pub uv_domains: [Extent2d; 2],
}

/// The two surfaces as objects of the global surface solve.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SolveSurface(u8);

impl SolutionObject for SolveSurface {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }
}

/// The outputs of `ComputeNextPoint` that `TraceCurve` keeps between calls.
#[derive(Debug, Clone, Copy)]
struct Step {
    found_good_point: bool,
    deviation: f64,
    angle_found: f64,
    clipped: bool,
    hit_boundary: bool,
}

fn too_many(op: &'static str) -> SolidError {
    SolidError::invalid_input(op, "the iteration does not terminate (non-finite values)")
}

impl SurfaceIntersector<'_> {
    // ---- tracing -----------------------------------------------------------------------

    /// One `ComputeNextPoint` call of `TraceCurve`: updates `step` (the deviation and
    /// angle only when set); `Ok(false)` when the call failed (`Unported` propagates).
    fn trace_step(
        &mut self,
        k: &Kernel,
        prev: TsectId,
        step_size: f64,
        next: TsectId,
        step: &mut Step,
    ) -> Result<bool> {
        match self.compute_next_point(k, prev, step_size, next) {
            Ok(n) => {
                step.found_good_point = n.found_good_point;
                if let Some(d) = n.deviation_found {
                    step.deviation = d;
                }
                if let Some(a) = n.angle_found_rad {
                    step.angle_found = a;
                }
                step.clipped = n.clipped;
                step.hit_boundary = n.boundary_hit;
                Ok(true)
            }
            Err(e) if e.is_unported() => Err(e),
            Err(_) => Ok(false),
        }
    }

    /// `SER(SM_ERR)` unless the UV points of `next` are in both domains (enlarged by
    /// `uv_tol`).
    fn check_in_domains(&self, next: TsectId, uv_tol: [f64; 2]) -> Result<()> {
        let p = self.point(next)?;
        if !self.uv_domains[0].contains_point(p.uv_pos(SrfIndex::First), uv_tol[0]) {
            return Err(SolidError::sm_err(OP_TRACE));
        }
        if !self.uv_domains[1].contains_point(p.uv_pos(SrfIndex::Second), uv_tol[1]) {
            return Err(SolidError::sm_err(OP_TRACE));
        }
        Ok(())
    }

    /// `TestPoint(next, bDone)`: `done` keeps its value when upstream leaves it.
    fn trace_test(&mut self, k: &Kernel, next: TsectId, done: &mut bool) -> Result<()> {
        if let Some(d) = self.test_point(k, next)? {
            *done = d;
        }
        Ok(())
    }

    /// Traces the intersection curve from `start` into the curve points
    /// (`TraceCurve`); `curves_3d` are the curves found so far, which the trace must
    /// not run along.
    ///
    /// With no curve points yet, a tangent or coincident start point becomes a
    /// single-point (closed) curve unless it is on one of `curves_3d`; else the start
    /// point (parameter and deviation zero) starts the curve, and when a tiny first
    /// step (`10 SM_EFF_ZERO_SQRT` along the UV tangents) leaves a domain the trace
    /// direction is reversed (or, for a bounded start point on one of `curves_3d`, the
    /// curve is dropped).
    ///
    /// Then steps from the last curve point
    /// ([`SurfaceIntersector::compute_next_point`]) with the step size shrunk by 1.5
    /// until a good point is found (giving up below `100 SM_EFF_ZERO`) and, for an
    /// unclipped first try with small deviation and angle, grown by 1.5 while the
    /// points stay good; each point is tested against the trace targets
    /// ([`SurfaceIntersector::test_point`]) and appended. The trace ends at a domain
    /// boundary, a target that ends it, a step that does not move, a second point on
    /// a curve of `curves_3d`, or a failing step.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a good next point outside a domain, on the 5000th point, or from
    /// the last regrowth step; `InvalidInput` when the start point is already on a
    /// list, an id is unknown, or a loop does not terminate; those of the curve
    /// reversal, the point tests and the curve predicates; [`SolidError::Unported`]
    /// from unported callees.
    #[allow(clippy::too_many_lines)]
    pub fn trace_curve(&mut self, k: &Kernel, start: TsectId, curves_3d: &[Curve]) -> Result<()> {
        // scale tolerances by UV domain size
        let uv_tol = [
            EFF_ZERO * (1.0 + self.uv_domains[0].max_dimension()),
            EFF_ZERO * (1.0 + self.uv_domains[1].max_dimension()),
        ];

        // when there are no points on the curve: pick a walking direction from the
        // start point
        if self.curve_points.is_empty() {
            let sp = self.point(start)?.clone();

            // 1. a tangent or coincidence start point: a single point curve
            if matches!(
                sp.point_type,
                IntersectionPointType::TangentPoint | IntersectionPointType::Coincidence
            ) {
                if !self.is_point_on_curve(k, sp.crv_pos(), curves_3d, None)? {
                    let p = self.point_mut(start)?;
                    p.curve_parameter = 0.0; // curves start at 0.0
                    p.deviation = 0.0;
                    self.add_point_to_curve(start)?;
                    self.curve_is_closed = true; // Closed curve finishes the job
                }
                return Ok(());
            }

            // 2. place the start point on the curve
            {
                let p = self.point_mut(start)?;
                p.curve_parameter = 0.0;
                p.deviation = 0.0;
            }
            self.add_point_to_curve(start)?;

            // 3. when a tiny first step is out of bounds: reverse the direction
            let delta_0 =
                sp.uv_pos(SrfIndex::First) + (10.0 * EFF_ZERO_SQRT) * sp.uv_deriv(SrfIndex::First);
            let delta_1 = sp.uv_pos(SrfIndex::Second)
                + (10.0 * EFF_ZERO_SQRT) * sp.uv_deriv(SrfIndex::Second);
            if !self.uv_domains[0].contains_point(delta_0, uv_tol[0])
                || !self.uv_domains[1].contains_point(delta_1, uv_tol[1])
            {
                if sp.is_bounded() && self.is_point_on_curve(k, sp.crv_pos(), curves_3d, None)? {
                    // no curve to add here
                    self.curve_points.clear();
                    return Ok(());
                }
                self.curve_trace_direction *= -1.0; // Reverse curve tracing direction
                self.reverse_curve_direction()?;
            }
        }

        // Curve is not closed prior to tracing
        self.curve_is_closed = false;

        let mut point_count: u64 = 0;
        let mut step_size = 0.0;
        let mut done = false;
        // (dAngleFound is uninitialised upstream; it is only read after a good point,
        // whose span test set it)
        let mut st = Step {
            found_good_point: false,
            deviation: 0.0,
            angle_found: 0.0,
            clipped: false,
            hit_boundary: false,
        };

        // 4. iterate the trace, adding points to the curve
        let mut iterations = 0usize;
        while !done {
            iterations += 1;
            if iterations > MAX_TRACE_ITERATIONS {
                return Err(too_many(OP_TRACE));
            }

            // 4a. the curve's last point
            let tsp = *self
                .curve_points
                .last()
                .ok_or_else(|| SolidError::invalid_input(OP_TRACE, "no curve point"))?;

            // 4b. last step size if given, else 100 * this_approx_tol_3d
            step_size = self.compute_step_size(step_size);

            // 4c. a new point
            let mut fresh = TsectPnt::zeroed();
            fresh.point_type = IntersectionPointType::Unknown;
            let next = self.alloc_point(fresh);

            // 4d. the next point for the current step size
            if !self.trace_step(k, tsp, step_size, next, &mut st)? {
                // an error return indicates that it can never succeed
                break;
            }

            // when the next guess steps out of bounds (with a modest step) - stop
            if st.hit_boundary && step_size < self.this_approx_tol_3d * 100.0 {
                break;
            }

            // a good next point must be in both domains
            if st.found_good_point {
                self.check_in_domains(next, uv_tol)?;
            }

            let mut decreased_step_size = false;

            // try smaller and smaller steps until the point is good
            let mut tries = 0usize;
            while !st.found_good_point {
                tries += 1;
                if tries > MAX_STEP_TRIES {
                    return Err(too_many(OP_TRACE));
                }
                step_size /= 1.5;
                if step_size < EFF_ZERO * 100.0 {
                    break;
                }
                decreased_step_size = true;
                if !self.trace_step(k, tsp, step_size, next, &mut st)? {
                    st.found_good_point = false;
                    break;
                }
                if st.found_good_point {
                    self.check_in_domains(next, uv_tol)?;
                }
            }

            // no good point even with smaller steps - quit
            if !st.found_good_point {
                break;
            }

            // snap the good point to a through, start or end point it passed
            let mut last_curve_param = self.point(next)?.curve_parameter;
            self.trace_test(k, next, &mut done)?;
            if done || self.point(next)?.curve_parameter != last_curve_param {
                self.add_point_to_curve(next)?;
                if done {
                    break;
                }
                continue;
            }

            // try bigger steps when this one was easy
            if !st.clipped
                && !decreased_step_size
                && st.deviation < self.this_approx_tol_3d / 8.0
                && st.angle_found < self.this_ang_tol_rad / 4.0
            {
                last_curve_param = self.point(next)?.curve_parameter;

                let mut tries = 0usize;
                while st.found_good_point && !st.clipped {
                    tries += 1;
                    if tries > MAX_STEP_TRIES {
                        return Err(too_many(OP_TRACE));
                    }
                    step_size *= 1.5;
                    if !self.trace_step(k, tsp, step_size, next, &mut st)? {
                        st.found_good_point = false;
                        break;
                    }

                    // quit increasing after a point that is no good
                    if !st.found_good_point {
                        continue;
                    }

                    self.check_in_domains(next, uv_tol)?;

                    // a refined step of zero: probably on a boundary
                    let param = self.point(next)?.curve_parameter;
                    if param <= last_curve_param + EFF_ZERO {
                        st.found_good_point = false;
                        break;
                    }
                    last_curve_param = param;

                    // passed a through or end point, or a singularity
                    self.trace_test(k, next, &mut done)?;
                    if done || self.point(next)?.curve_parameter != last_curve_param {
                        if done {
                            // an end condition: done tracing
                            self.add_point_to_curve(next)?;
                            return Ok(());
                        }
                        break;
                    }
                }

                // the last increase gave a bad point: step back to the last good one
                if !st.found_good_point {
                    step_size /= 1.5;
                    let n = self.compute_next_point(k, tsp, step_size, next)?;
                    st.found_good_point = n.found_good_point;
                    if let Some(d) = n.deviation_found {
                        st.deviation = d;
                    }
                    if let Some(a) = n.angle_found_rad {
                        st.angle_found = a;
                    }
                    st.clipped = n.clipped;
                    st.hit_boundary = n.boundary_hit;
                    if st.found_good_point {
                        self.check_in_domains(next, uv_tol)?;
                    }
                }
            }

            self.clipped_by_through_point = false;

            // 4g. a step size this small: stop near the problem
            if step_size < EFF_ZERO * 100.0 {
                // (bDone = TRUE: done tracing)
                break;
            }

            let (tsp_pos, tsp_deriv) = {
                let p = self.point(tsp)?;
                (p.crv_pos(), p.crv_deriv())
            };
            let (next_pos, next_deriv, next_type) = {
                let p = self.point(next)?;
                (p.crv_pos(), p.crv_deriv(), p.point_type)
            };
            let dist_moved = next_pos.distance(tsp_pos);
            let tol = EFF_ZERO * 100.0 * (1.0 + tsp_pos.max_dimension());
            if dist_moved < tol {
                // (bDone = TRUE: done tracing)
                break;
            }

            // the second point of the trace on a curve already found
            if point_count == 0
                && (next_type != IntersectionPointType::Singularity
                    && self.is_point_on_curve(k, next_pos, curves_3d, None)?)
            {
                // unclipped points have to be on curves already traced
                if !st.clipped {
                    // (bDone = TRUE: done tracing)
                    break;
                }

                // test the span's mid point
                let herm = HermiteCurve::new(
                    tsp_pos,
                    tsp_deriv * step_size,
                    next_pos,
                    next_deriv * step_size,
                    3,
                );
                let mid = herm.evaluate_point(0.5);
                if self.is_point_on_curve(k, mid, curves_3d, None)? {
                    // (bDone = TRUE: done tracing)
                    break;
                }
            }

            // snap to a through or end point passed, then add the point
            if !done {
                self.trace_test(k, next, &mut done)?;
                self.point_mut(next)?.deviation = st.deviation;
                self.add_point_to_curve(next)?;
            }

            // 4i. count the points
            point_count += 1;
            if point_count == 5000 {
                // the tolerance is too small or a surface is poorly defined
                return Err(SolidError::sm_err(OP_TRACE));
            }
        }
        Ok(())
    }

    // ---- interior curves ---------------------------------------------------------------

    /// Finds the closed intersection curves inside both domains
    /// (`FindInteriorCurves`): the tree node pairs of a global surface/surface solve
    /// (`SM_SO_INTERSECT`, `SM_SR_NODES`) are traced as
    /// [`SurfaceIntersector::find_interior_curves_from_node_pairs`] does; the curves
    /// are appended to `out`.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a self intersection; those of the solve;
    /// [`SolidError::Unported`]: the global surface solve.
    pub fn find_interior_curves(&mut self, k: &Kernel, out: &mut TsectCurves) -> Result<()> {
        // The basic intersector does not allow self intersection testing.
        if self.doing_self_intersection {
            return Err(SolidError::sm_err(OP_FIC));
        }

        // Find node pairs in the tree (SER: checked; surface::surface::p7, its leaf
        // solve SmSSGlobalSolver::LocalSolve in surface::surface::p11)
        let (_, nodes) = self
            .surface(SrfIndex::First)
            .global_surface_solve_with::<SolveSurface>(
                k,
                &self.uv_domains[0],
                self.surface(SrfIndex::Second),
                &self.uv_domains[1],
                SolverOperation::Intersect,
                self.this_approx_tol_3d,
                None,
                &[],
                SolutionRequested::Nodes,
                [Some(SolveSurface(0)), Some(SolveSurface(1))],
            )?;

        // each solution's two tree nodes: their boxes and Bezier patch UV domains
        let pairs: Vec<InteriorNodePair> = nodes
            .iter()
            .map(|n| InteriorNodePair {
                bboxes: n.bboxes,
                uv_domains: n.uv_domains,
            })
            .collect();
        self.find_interior_curves_from_node_pairs(k, &pairs, out)
    }

    /// The loop of `FindInteriorCurves` over the node pairs of the global solve: a
    /// pair whose boxes meet (the first expanded by `this_approx_tol_3d`) near the
    /// tree of a curve of `out` is skipped; else the local surface/surface
    /// intersection from the patches' mid UV points gives a start point which, unless
    /// it is on a curve of `out` or cannot be evaluated, is traced forwards and (for an
    /// open curve from an unbounded point) backwards, and the curve is flushed into
    /// `out`.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a self intersection or disjoint boxes; `SM_ERR_NULL_POINTER` for a
    /// curve without a curve cache tree; those of the evaluations, traces, reversals
    /// and flushes; [`SolidError::Unported`]: `SmSurface::LocalSurfaceIntersect`.
    pub fn find_interior_curves_from_node_pairs(
        &mut self,
        k: &Kernel,
        pairs: &[InteriorNodePair],
        out: &mut TsectCurves,
    ) -> Result<()> {
        if self.doing_self_intersection {
            return Err(SolidError::sm_err(OP_FIC));
        }
        for pair in pairs {
            // a fast test: is the intersection of these two nodes near a 3D curve
            let mut int_bbox = pair.bboxes[0];
            int_bbox.expand_absolute(self.this_approx_tol_3d);
            // (disjoint boxes: SM_ERR_INVALID_INPUT)
            let int_bbox = int_bbox.intersect(&pair.bboxes[1])?;

            let mut is_near_curve = false;
            for curve in &out.curves_3d {
                // NER: no cache (or no tree) is SM_ERR_NULL_POINTER
                let cache = match make_curve_cache::<()>(k, curve, None) {
                    Ok(c) => c,
                    Err(e) if e.is_unported() => return Err(e),
                    Err(_) => return Err(SolidError::null_pointer(OP_FIC)),
                };
                let tree = cache
                    .tree()
                    .ok_or_else(|| SolidError::null_pointer(OP_FIC))?;
                if tree.intersects_box(&int_bbox)? {
                    is_near_curve = true;
                    break;
                }
            }
            if is_near_curve {
                continue;
            }

            // an intersection curve start point
            let (found_answer, sol) = surface_local_surface_intersect(
                k,
                self.surface(SrfIndex::First),
                &self.uv_domains[0],
                self.surface(SrfIndex::Second),
                &self.uv_domains[1],
                self.this_approx_tol_3d,
                pair.uv_domains[0].evaluate(0.5, 0.5),
                pair.uv_domains[1].evaluate(0.5, 0.5),
                None,
                None,
                true,
            )?;
            if !found_answer {
                continue;
            }
            let [s0, s1, s2, s3] = sol;
            let uv = Point2::new(s0, s1);
            let surf_pnt = self.surface(SrfIndex::First).evaluate_point(k, uv)?;

            // a point on a curve already found
            if self.is_point_on_curve(k, surf_pnt, &out.curves_3d, None)? {
                continue;
            }

            // load the start point and trace it
            let uvs = [uv, Point2::new(s2, s3)];
            let mut fresh = TsectPnt::zeroed();
            fresh.point_type = IntersectionPointType::Unknown;
            let start = self.alloc_point(fresh);
            self.curve_trace_direction = 1.0;

            // a start point: no previous point
            let mut sp = self.point(start)?.clone();
            let r = self.compute_point_values(k, uvs, &mut sp, None, None);
            *self.point_mut(start)? = sp;
            if ignored(r)?.is_none() {
                continue;
            }

            self.start_point = Some(start);
            let input_trace_direction = self.curve_trace_direction;
            self.trace_curve(k, start, &out.curves_3d)?;

            // an open curve from an unbounded point, not reversed by the trace: trace
            // the other way too
            if !self.curve_is_closed
                && !self.point(start)?.is_bounded()
                && input_trace_direction == self.curve_trace_direction
            {
                self.curve_trace_direction *= -1.0;
                self.reverse_curve_direction()?;
                self.trace_curve(k, start, &out.curves_3d)?;
                // (upstream multiplies by 1.0 here)
                self.curve_trace_direction *= 1.0;
                self.reverse_curve_direction()?;
            }

            // add the curve of the curve points (which are cleared)
            self.flush_curve(k, out)?;
            self.start_point = None;
        }
        Ok(())
    }

    // ---- single curve drivers ----------------------------------------------------------

    /// Intersects the surfaces in a single curve through `points`
    /// (`DoPointIntersection`). The first point starts the trace (traced backwards
    /// too with `extend_before_start`); with a start direction, the last point ends it
    /// unless `extend_after_end`; the points between are through points. Each point is
    /// evaluated at its UV points (the first with the start direction as previous
    /// tangent, the last with the end direction); the first must be on both surfaces
    /// within `this_approx_tol_3d`. When the trace stops short of the last point at a
    /// singular point, the curve from there to the last point is joined on.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without points, for a point that cannot be evaluated or a first point
    /// off the intersection; `SM_ERR_NULL_POINTER` when no start point was added;
    /// `InvalidInput` when a through point is already on a list or an id is unknown;
    /// those of the reversals, the flush, the singular-point retry and its joins;
    /// [`SolidError::Unported`] from unported callees.
    pub fn do_point_intersection(
        &mut self,
        k: &Kernel,
        points: &[TsectId],
        opts: &PointIntersectionOptions,
    ) -> Result<IntersectionCurve> {
        self.do_point_intersection_body(k, points, opts, false)
    }

    #[allow(clippy::too_many_lines)]
    fn do_point_intersection_body(
        &mut self,
        k: &Kernel,
        points: &[TsectId],
        opts: &PointIntersectionOptions,
        inside_me: bool,
    ) -> Result<IntersectionCurve> {
        // init output
        self.curve_trace_direction = 1.0;

        // no work - no through points specified
        if points.is_empty() {
            return Err(SolidError::sm_err(OP_DPI));
        }

        // Use supplied tolerances when given
        if let Some(t) = opts.approx_tol_3d {
            self.this_approx_tol_3d = t;
        }
        if let Some(a) = opts.ang_tol_rad {
            self.this_ang_tol_rad = a;
        }

        let mut out = TsectCurves::default();

        // init the through and start point lists
        self.through_points.clear();
        self.start_points.clear();

        // an optional start direction as the 3D tangent of a previous point
        let temp_tsp = opts.start_direction.map(|d| {
            let mut t = TsectPnt::new();
            t.point_type = IntersectionPointType::Unknown;
            t.curve_pv[1] = d;
            t
        });
        // an optional end direction likewise
        let temp_end_tsp = opts.end_direction.map(|d| {
            let mut t = TsectPnt::new();
            t.curve_pv[1] = d;
            t
        });
        let mut prev_tsp: Option<&TsectPnt> = temp_tsp.as_ref();

        let n = points.len();
        for (ii, &pid) in points.iter().enumerate() {
            // initialize its values to unknown
            let mut tsp = {
                let p = self.point_mut(pid)?;
                p.point_type = IntersectionPointType::Unknown;
                p.curve_parameter = -1.0;
                p.clone()
            };
            let uvs = [tsp.uv_pos(SrfIndex::First), tsp.uv_pos(SrfIndex::Second)];

            // the start direction for the first point, the end direction for the last
            if ii > 0 {
                prev_tsp = None;
            }
            if ii == n - 1 {
                prev_tsp = temp_end_tsp.as_ref();
            }

            let r = self.compute_point_values(k, uvs, &mut tsp, prev_tsp, None);
            *self.point_mut(pid)? = tsp.clone();
            match r {
                Ok(()) => {}
                Err(e) if e.is_unported() => return Err(e),
                Err(_) => return Err(SolidError::sm_err(OP_DPI)),
            }

            // the first points must be on both surfaces
            if ii == 0
                && !tsp
                    .srf_pos(SrfIndex::First)
                    .closer_than(self.this_approx_tol_3d, tsp.srf_pos(SrfIndex::Second))
            {
                return Err(SolidError::sm_err(OP_DPI));
            }

            if ii == 0 {
                // a copy of the first point is the start point
                let mut start = TsectPnt::zeroed();
                start.assign(&tsp);
                let start_id = self.alloc_point(start);
                // (status ignored)
                ignored(self.add_start_point(k, start_id))?;
                self.point_mut(pid)?.curve_parameter = 0.0;
                if !(opts.start_direction.is_some() && n > 1) {
                    self.start_point = Some(start_id);
                }
            } else if ii == n - 1 && opts.start_direction.is_some() && !opts.extend_after_end {
                // the last point ends the curve
                self.end_point = Some(pid);
            } else {
                // the points between are through points (status ignored, but linking
                // a linked point is undefined upstream)
                if let Err(e @ (SolidError::InvalidInput { .. } | SolidError::Unported { .. })) =
                    self.add_through_point(pid)
                {
                    return Err(e);
                }
            }
        }

        // trace from the last start point
        let tsp_id = self
            .start_points
            .pop()
            .ok_or_else(|| SolidError::null_pointer(OP_DPI))?;
        self.curve_trace_direction = 1.0;

        // tracing beyond the end: clip the extension
        if self.extension_distance > 0.0 && opts.extend_after_end && self.end_point.is_none() {
            self.doing_extension_clipping = true;
        }

        // Reverse direction of tracing if indicated by the start direction
        if let Some(d) = opts.start_direction {
            let p = self.point_mut(tsp_id)?;
            if d.dot(p.crv_deriv()) < -EFF_ZERO {
                p.curve_pv[1] = -p.curve_pv[1];
                p.uv_curve_pv[0][1] = -p.uv_curve_pv[0][1];
                p.uv_curve_pv[1][1] = -p.uv_curve_pv[1][1];
                self.curve_trace_direction = -1.0;
            }
        }

        // the last curve point of the trace
        let mut final_tsp = None;

        let input_trace_direction = self.curve_trace_direction;
        let traced = match self.trace_curve(k, tsp_id, &out.curves_3d) {
            Ok(()) => true,
            Err(e) if e.is_unported() => return Err(e),
            Err(_) => false,
        };
        if traced {
            // trace backwards from the first point too
            if opts.extend_before_start
                && !self.curve_is_closed
                && !self.curve_points.is_empty()
                && input_trace_direction == self.curve_trace_direction
            {
                self.reverse_curve_direction()?;
                self.curve_trace_direction *= -1.0;
                self.trace_curve(k, tsp_id, &out.curves_3d)?;
                self.reverse_curve_direction()?;
                self.curve_trace_direction *= -1.0;
            }

            final_tsp = self.curve_points.last().copied();

            // the curve points become curves
            self.flush_curve(k, &mut out)?;
        }

        // a trace that stopped at a singular point before the last point
        let last = *points
            .last()
            .ok_or_else(|| SolidError::invalid_input(OP_DPI, "no points"))?;
        let mut is_singular = false;
        if out.curves_3d.len() == 1 && self.point(last)?.curve_parameter < 0.0 && !inside_me {
            if let Some(f) = final_tsp {
                let (uv0, uv1) = {
                    let p = self.point(f)?;
                    (p.uv_pos(SrfIndex::First), p.uv_pos(SrfIndex::Second))
                };
                is_singular = surface_is_singularity(k, self.surface(SrfIndex::First), uv0)?;
                is_singular |= surface_is_singularity(k, self.surface(SrfIndex::Second), uv1)?;
            }
        }

        let mut res = IntersectionCurve::from_single(out);
        if is_singular {
            self.join_from_singular_point(k, last, opts, &mut res)?;
        }
        Ok(res)
    }

    /// The singular-point retry of `DoPointIntersection`: the curve from the end of
    /// `res` to the point `last` is joined onto `res` when its start is within
    /// `this_approx_tol_3d` of that end.
    fn join_from_singular_point(
        &mut self,
        k: &Kernel,
        last: TsectId,
        opts: &PointIntersectionOptions,
        res: &mut IntersectionCurve,
    ) -> Result<()> {
        let no_curve = || SolidError::invalid_input(OP_DPI, "no flushed curve");
        let ivl1 = res
            .curve_3d
            .as_ref()
            .ok_or_else(no_curve)?
            .natural_interval();

        // the end point of the current curve
        let mut end_tsp = TsectPnt::new();
        let uv_curve0 = res.uv_curves[0].as_ref().ok_or_else(no_curve)?;
        let pnt = uv_curve0.evaluate_point(k, ivl1.max())?;
        *end_tsp.uv_pos_mut(SrfIndex::First) = Point2::new(pnt.x, pnt.y);
        let uv_curve1 = res.uv_curves[1].as_ref().ok_or_else(no_curve)?;
        let pnt = uv_curve1.evaluate_point(k, ivl1.max())?;
        *end_tsp.uv_pos_mut(SrfIndex::Second) = Point2::new(pnt.x, pnt.y);
        let end_id = self.alloc_point(end_tsp);

        // init the lists for the nested call
        self.curve_points.clear();
        self.start_points.clear();
        self.through_points.clear();

        // the intersection from the other end to here
        let second = self.do_point_intersection_body(k, &[end_id, last], opts, true)?;

        if let Some(mut c2) = second.curve_3d {
            let c1 = res.curve_3d.as_mut().ok_or_else(no_curve)?;
            let crv1_pnt = c1.evaluate_point(k, ivl1.max())?;
            let ivl2 = c2.natural_interval();
            let crv2_pnt = c2.evaluate_point(k, ivl2.min())?;

            // curve end points close enough: join the curves
            if crv1_pnt.closer_than(self.this_approx_tol_3d, crv2_pnt) {
                join_curves(k, c1, &mut c2)?;
                let [u0, u1] = second.uv_curves;
                let mut u0 = u0.ok_or_else(no_curve)?;
                let mut u1 = u1.ok_or_else(no_curve)?;
                join_curves(k, res.uv_curves[0].as_mut().ok_or_else(no_curve)?, &mut u0)?;
                join_curves(k, res.uv_curves[1].as_mut().ok_or_else(no_curve)?, &mut u1)?;
            }
        }
        Ok(())
    }

    /// Traces a fillet law curve between the two points of `points`
    /// (`DoLawIntersection`): the intersection of the two offset surfaces (offset by
    /// the law's radius) with the section planes along the point curve. The end points
    /// are evaluated at the interval ends ([`SurfaceIntersector::compute_law_point_values`]);
    /// spans are split ([`SurfaceIntersector::split_span`]) until they pass
    /// [`SurfaceIntersector::test_span_accuracy`], then spans more than 2.7 times as
    /// long as a neighbour are split; the points are flushed into one curve.
    /// `this_approx_tol_3d` becomes the given tolerance (`1e-3` by default).
    ///
    /// # Errors
    ///
    /// `SM_ERR` unless there are two points; `SM_ERR_NULL_POINTER` unless both
    /// surfaces are offset surfaces; those of the law point evaluations, the splits,
    /// the span tests and the flush; `InvalidInput` for an unknown id or a subdivision
    /// that does not terminate; [`SolidError::Unported`] from unported callees.
    #[allow(clippy::too_many_lines)]
    pub fn do_law_intersection<L: ?Sized>(
        &mut self,
        k: &Kernel,
        points: &[TsectId],
        opts: &LawIntersectionOptions<'_, L>,
    ) -> Result<IntersectionCurve> {
        let [first, last] = points else {
            return Err(SolidError::sm_err(OP_DLI));
        };
        let (first, last) = (*first, *last);

        // the point curve tolerance
        self.this_approx_tol_3d = opts.approx_tol_3d.unwrap_or(1.0e-3);

        let delta_step = 1.0e-8;

        // the offset surfaces
        if !is_offset_surface(self.surface(SrfIndex::First)) {
            return Err(SolidError::null_pointer(OP_DLI));
        }
        if !is_offset_surface(self.surface(SrfIndex::Second)) {
            return Err(SolidError::null_pointer(OP_DLI));
        }

        let normals = [
            self.point(first)?.crv_deriv(),
            self.point(last)?.crv_deriv(),
        ];
        let ivl = opts.point_curve_interval;
        {
            let p = self.point_mut(first)?;
            p.user_doubles[1] = ivl.min();
            p.user_doubles[0] = p.user_doubles[1];
        }
        {
            let p = self.point_mut(last)?;
            p.user_doubles[1] = ivl.max();
            p.user_doubles[0] = p.user_doubles[1];
        }

        let law = LawPointOptions {
            law: opts.law,
            law_orient: opts.law_orient,
            point_curve: opts.point_curve,
            point_curve_interval: ivl,
            normals,
            average_normals: opts.use_normal_plane_averaging,
            surface_orientations: opts.surface_orientations,
        };

        for (id, delta) in [(first, delta_step), (last, -delta_step)] {
            let mut p = self.point(id)?.clone();
            let r = self.compute_law_point_values(
                k,
                p.user_doubles[1],
                p.uv_pos(SrfIndex::First),
                p.uv_pos(SrfIndex::Second),
                delta,
                &law,
                false,
                &mut p,
            );
            *self.point_mut(id)? = p;
            r?;
        }

        self.point_mut(first)?.curve_parameter = 0.0;
        let first_pos = self.point(first)?.crv_pos();
        let d = first_pos.distance(self.point(last)?.crv_pos());
        self.point_mut(last)?.curve_parameter = d;

        let mut stack = vec![last, first];
        let mut curve_pnts = vec![first];

        while stack.len() > 1 {
            if stack.len() + curve_pnts.len() > MAX_LAW_POINTS {
                return Err(too_many(OP_DLI));
            }
            let curr = stack
                .pop()
                .ok_or_else(|| SolidError::invalid_input(OP_DLI, "empty stack"))?;
            let next = *stack
                .last()
                .ok_or_else(|| SolidError::invalid_input(OP_DLI, "empty stack"))?;

            let (curr_param, curr_pos) = {
                let p = self.point(curr)?;
                (p.curve_parameter, p.crv_pos())
            };
            let next_pos = self.point(next)?.crv_pos();
            self.point_mut(next)?.curve_parameter = curr_param + curr_pos.distance(next_pos);

            // the new mid point
            let new_tsp = self.split_span(k, curr, next, delta_step, &law)?;

            let acc = self.test_span_accuracy(k, curr, next)?;
            if acc.found_good_point {
                // accept the point
                curve_pnts.push(next);
            } else {
                // subdivide more
                stack.push(new_tsp);
                stack.push(curr);
            }
        }

        // split spans much longer than their neighbours
        let split_factor = 2.7;
        let mut done = false;
        while !done {
            done = true;
            let mut ip = 1;
            while ip < curve_pnts.len() {
                if curve_pnts.len() > MAX_LAW_POINTS {
                    return Err(too_many(OP_DLI));
                }
                let at = |i: usize| {
                    curve_pnts
                        .get(i)
                        .copied()
                        .ok_or_else(|| SolidError::invalid_input(OP_DLI, "point index"))
                };
                let curr = at(ip - 1)?;
                let next = at(ip)?;
                let curr_param = self.point(curr)?.curve_parameter;
                let next_param = self.point(next)?.curve_parameter;
                let curr_param_range = next_param - curr_param;
                let mut split = false;
                if ip > 1 {
                    let prev = at(ip - 2)?;
                    let prev_param_range = curr_param - self.point(prev)?.curve_parameter;
                    if curr_param_range > prev_param_range * split_factor {
                        split = true;
                    }
                }
                if ip + 1 < curve_pnts.len() {
                    let next_next = at(ip + 1)?;
                    let next_param_range = self.point(next_next)?.curve_parameter - next_param;
                    if curr_param_range > next_param_range * split_factor {
                        split = true;
                    }
                }
                if split {
                    let new_tsect = self.split_span(k, curr, next, delta_step, &law)?;
                    curve_pnts.insert(ip, new_tsect);
                    done = false;
                }
                ip += 1;
            }
        }

        for &id in &curve_pnts {
            // (status ignored, but linking a linked point is undefined upstream)
            if let Err(e @ (SolidError::InvalidInput { .. } | SolidError::Unported { .. })) =
                self.add_point_to_curve(id)
            {
                return Err(e);
            }
        }

        self.through_points.clear();
        self.start_points.clear();

        let mut out = TsectCurves::default();
        self.flush_curve(k, &mut out)?;
        Ok(IntersectionCurve::from_single(out))
    }
}

// ---- callees ----------------------------------------------------------------------------------

/// `rpCurve->JoinWith(1, pOther, 0)` of two flushed (B-spline) curves.
fn join_curves(k: &Kernel, this: &mut Curve, other: &mut Curve) -> Result<()> {
    let CurveKind::BSpline(other) = other.kind_mut() else {
        return Err(SolidError::invalid_input(
            OP_DPI,
            "flushed curve is not a B-spline",
        ));
    };
    let CurveKind::BSpline(this) = this.kind_mut() else {
        return Err(SolidError::invalid_input(
            OP_DPI,
            "flushed curve is not a B-spline",
        ));
    };
    this.join_with(k, true, other, false, None)
}

/// `SM_CAST_PTR(SmOffsetSurface, surface) != NULL`.
const fn is_offset_surface(surface: &Surface) -> bool {
    matches!(surface.kind(), SurfaceKind::OffsetSurface(_))
}

/// `surface.IsSingularity(uv, eWhichDir)` (default tolerance): the plane override
/// (never singular); other kinds run `SmSurface::IsSingularity`.
fn surface_is_singularity(k: &Kernel, surface: &Surface, uv: Point2) -> Result<bool> {
    match surface.kind() {
        SurfaceKind::Plane(p) => Ok(p.is_singularity().0),
        SurfaceKind::Cone(_)
        | SurfaceKind::Sphere(_)
        | SurfaceKind::Torus(_)
        | SurfaceKind::BSpline(_)
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

#[cfg(test)]
mod tests {
    use super::*;
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

    fn unit() -> Extent2d {
        Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0)
    }

    #[test]
    fn tangent_start_point_is_a_single_point_curve() {
        let k = &Kernel::default();
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let mut si = SurfaceIntersector::new(k, &a, &unit(), &b, &unit(), false).unwrap();
        let mut p = TsectPnt::new();
        p.point_type = IntersectionPointType::TangentPoint;
        p.curve_pv[0] = Vec3::new(0.5, 0.5, 0.0);
        p.curve_parameter = 3.0;
        let id = si.alloc_point(p);
        si.trace_curve(&Kernel::default(), id, &[]).unwrap();
        assert_eq!(si.curve_points(), &[id]);
        assert!(si.curve_is_closed);
        assert_eq!(si.point(id).unwrap().curve_parameter, 0.0);
    }

    #[test]
    fn unknown_start_point_traces_nothing_more() {
        let k = &Kernel::default();
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let mut si = SurfaceIntersector::new(k, &a, &unit(), &b, &unit(), false).unwrap();
        let mut p = TsectPnt::zeroed();
        p.point_type = IntersectionPointType::Unknown;
        *p.uv_pos_mut(SrfIndex::First) = Point2::new(0.5, 0.5);
        *p.uv_pos_mut(SrfIndex::Second) = Point2::new(0.5, 0.5);
        *p.uv_deriv_mut(SrfIndex::First) = Point2::new(1.0, 0.0);
        *p.uv_deriv_mut(SrfIndex::Second) = Point2::new(1.0, 0.0);
        let id = si.alloc_point(p);
        // RefinePoint fails for unknown points: no next point is ever good
        si.trace_curve(&Kernel::default(), id, &[]).unwrap();
        assert_eq!(si.curve_points(), &[id]);
        assert!(!si.curve_is_closed);
    }

    #[test]
    fn linked_start_point_is_rejected() {
        let k = &Kernel::default();
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let mut si = SurfaceIntersector::new(k, &a, &unit(), &b, &unit(), false).unwrap();
        let id = si.alloc_point(TsectPnt::zeroed());
        si.add_start_point(k, id).unwrap();
        assert!(matches!(
            si.trace_curve(&Kernel::default(), id, &[]),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    #[test]
    fn point_intersection_needs_points_on_both_surfaces() {
        let k = &Kernel::default();
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let mut si = SurfaceIntersector::new(k, &a, &unit(), &b, &unit(), false).unwrap();
        let k = Kernel::default();
        let opts = PointIntersectionOptions::default();
        assert!(si.do_point_intersection(&k, &[], &opts).is_err());
        let mut p = TsectPnt::new();
        *p.uv_pos_mut(SrfIndex::First) = Point2::new(0.5, 0.1);
        *p.uv_pos_mut(SrfIndex::Second) = Point2::new(0.5, 0.5);
        let id = si.alloc_point(p);
        let r = si.do_point_intersection(&k, &[id], &opts);
        assert!(matches!(r, Err(SolidError::Upstream { .. })));
        assert_eq!(si.point(id).unwrap().curve_parameter, -1.0);
    }

    #[test]
    fn law_intersection_needs_two_offset_surfaces() {
        let k = &Kernel::default();
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = plane([0.0, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let mut si = SurfaceIntersector::new(k, &a, &unit(), &b, &unit(), false).unwrap();
        let curve = Curve::from(
            crate::curve::line::Line::create_line_segment(
                3,
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                None,
            )
            .unwrap(),
        );
        let opts = LawIntersectionOptions {
            law: &(),
            law_orient: false,
            point_curve: &curve,
            point_curve_interval: Extent1d::from_min_max(0.0, 1.0),
            surface_orientations: [1.0, 1.0],
            use_normal_plane_averaging: false,
            approx_tol_3d: Some(0.25),
        };
        let p = si.alloc_point(TsectPnt::new());
        let q = si.alloc_point(TsectPnt::new());
        assert!(matches!(
            si.do_law_intersection(k, &[p], &opts),
            Err(SolidError::Upstream {
                code: crate::error::SM_ERR,
                ..
            })
        ));
        assert!(matches!(
            si.do_law_intersection(k, &[p, q], &opts),
            Err(SolidError::Upstream {
                code: crate::error::SM_ERR_NULL_POINTER,
                ..
            })
        ));
        assert_eq!(si.this_approx_tol_3d, 0.25);
    }

    #[test]
    fn interior_curves_reject_self_intersection_and_skip_near_pairs() {
        let a = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let k = Kernel::default();
        let mut si = SurfaceIntersector::new(&k, &a, &unit(), &a, &unit(), false).unwrap();
        let mut out = TsectCurves::default();
        assert!(si.find_interior_curves(&k, &mut out).is_err());

        let b = plane([0.0, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let mut si = SurfaceIntersector::new(&k, &a, &unit(), &b, &unit(), false).unwrap();
        // (completes since the generic IsoCurve holds a plane's iso boundaries, #76;
        // the curves are compared by difftest)
        let mut found = TsectCurves::default();
        si.find_interior_curves(&k, &mut found).unwrap();
        // a pair next to a curve already found is skipped
        out.curves_3d.push(Curve::from(
            crate::curve::line::Line::create_line_segment(
                3,
                Vec3::new(0.0, 0.5, 0.0),
                Vec3::new(1.0, 0.5, 0.0),
                None,
            )
            .unwrap(),
        ));
        let bx = Extent3d::from_bounds(0.4, 0.4, -0.1, 0.6, 0.6, 0.1);
        let pair = InteriorNodePair {
            bboxes: [bx, bx],
            uv_domains: [unit(), unit()],
        };
        si.find_interior_curves_from_node_pairs(&k, &[pair], &mut out)
            .unwrap();
        assert_eq!(out.curves_3d.len(), 1);
        // a far pair reaches the local intersection (surface.surface_p11, ported): it
        // succeeds, or stops at an unported boundary solve (surface.surface_p8)
        let far = Extent3d::from_bounds(5.0, 5.0, 5.0, 6.0, 6.0, 6.0);
        let pair = InteriorNodePair {
            bboxes: [far, far],
            uv_domains: [unit(), unit()],
        };
        if let Err(e) = si.find_interior_curves_from_node_pairs(&k, &[pair], &mut out) {
            assert!(e.is_unported(), "{e}");
        }
    }
}
