//! Classification of a parameter-space point against one boundary loop of a face
//! (`SmLoop::ContainsUVPoint`): a winding-number walk along the loop's UV trim curves
//! with on-boundary detection, plus the loop corner / surface side test it uses for
//! poles.
//!
//! Upstream: `source/SMLib/src/SmLoop.cpp`, `source/SMLib/inc/SmLoop.h`.
//!
//! | Rust                         | Upstream                                       |
//! |------------------------------|------------------------------------------------|
//! | [`contains_uv_point`]        | `SmLoop::ContainsUVPoint`                      |
//! | [`UvContainment`]            | its return value and out-params                |
//! | [`FaceLoop`], [`LoopEdgeuse`] | the `SmLoop` / `SmLoopuse` / `SmEdgeuse` / `SmEdge` / `SmFace` data it reads (see Deviations) |
//! | [`uv_corner_boundaries`]     | `SmLoop::GetUVCornerBoundaries` (CO-PORTED from `topology.loopuse`) |
//! | [`UvCornerBoundaries`]       | its return value and out-params                |
//! | `calc_quad_num` (private)    | `sm_CalcQuadNum` (file static, unit `topology.loopuse`) |
//!
//! Deviations:
//! - **Topology view.** The arena topology ([`crate::topology`]) does not bind
//!   geometry yet (faces have no surface, edges no curve, there are no edge uses), so
//!   the loop is passed as a borrowed [`FaceLoop`]: the owning face's surface and zone
//!   tolerance (`SmFace::GetTolerance`, which is what `SmTol::GetZoneTol3d(pSurface)`
//!   returns for a surface with a face under `SM_USE_OLDTOL`) and the edge uses of
//!   the primary loopuse in `GetEdgeuses` order, each with its edge id, 3D curve,
//!   interval, zone tolerance (`SmEdge::GetTolerance`), orientation and UV trim
//!   curve. A face without surface is `None` (upstream: `SM_POC_UNKNOWN`). The edge
//!   out-param is the [`EdgeId`] given in the view.
//! - **UV trim curves.** `SmEdgeuse::GetOrCreateUVTrimCurve` returns the existing
//!   trim curve (of the edge use or its mate) as given in [`LoopEdgeuse::uv_trim_curve`];
//!   when there is none it would build one (`SmEdgeuse::CreateUVTrimCurve`, unit
//!   `topology.edgeuse_p3`), which returns [`SolidError::Unported`].
//! - **Poles.** `SmSurface::GetSingularities` (unit `surface.surface_p3`) is only
//!   reached when the point lies on a side of the surface's natural domain and an edge
//!   use end of the loop does too.
//! - **Ignored statuses.** Upstream ignores the status of `EvaluatePoint` /
//!   `Evaluate` of the surface and the trim curves and then reads the (uninitialised)
//!   outputs; the port returns their error instead (no valid input reaches it).
//!   `DropPoint` and `LocalPointSolve` failures are "no answer" (both reset their
//!   success flag first upstream). [`SolidError::Unported`] from any callee is
//!   returned.
//! - `dTol3d` is unused upstream and is kept as `_tol_3d`.
//! - **Loops.** Both walk loops keep upstream's bounds (1000 steps per edge use and
//!   1000 re-evaluations per step, then `SM_POC_UNKNOWN`); the `goto Got_dThisParam`
//!   re-evaluations are an inner loop. No recursion.

use crate::curve::curve::p3::PointSolveOptions;
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::{Extent2d, SS_NONE};
use crate::geom::geom_utility::p1::line_point_distance;
use crate::geom::global_solver::SolverOperation;
use crate::geom::point_set::PointContainment;
use crate::geom::solution_array::Solution;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::p1::map_to_2d;
use crate::surface::surface::Surface;
use crate::topology::EdgeId;

/// Upper bound of the steps along one edge use and of the re-evaluations of one step
/// (upstream's infinite-loop guards).
const MAX_LOOP_COUNT: u32 = 1000;

/// One edge use of a loop as [`contains_uv_point`] reads it (`SmEdgeuse` with its
/// `SmEdge`).
#[derive(Debug, Clone, Copy)]
pub struct LoopEdgeuse<'a> {
    /// The edge (`GetEdge`), reported back as the on-boundary edge.
    pub edge: EdgeId,
    /// The edge's 3D curve (`SmEdge::GetCurve`).
    pub curve: &'a Curve,
    /// The edge's parameter interval (`SmEdge::GetInterval`).
    pub interval: Extent1d,
    /// The edge's zone tolerance (`SmEdge::GetTolerance`, the stored `m_sZoneTol3d`).
    pub tolerance: f64,
    /// The edge use's orientation relative to its edge (`GetOrientation`).
    pub orientation: OrientType,
    /// The UV trim curve of the edge use or its mate (`GetUVTrimCurve`); `None` when
    /// neither has one.
    pub uv_trim_curve: Option<&'a Curve>,
}

/// The face a loop bounds, as [`contains_uv_point`] reads it.
#[derive(Debug, Clone, Copy)]
pub struct LoopFace<'a> {
    /// The face's surface (`SmFace::GetSurface`).
    pub surface: &'a Surface,
    /// The face's zone tolerance (`SmFace::GetTolerance`).
    pub tolerance: f64,
}

/// A face loop (`SmLoop`) as [`contains_uv_point`] reads it.
#[derive(Debug, Clone)]
pub struct FaceLoop<'a> {
    /// The owning face with its surface (`GetFace`, `GetSurface`); `None` for a loop
    /// without face or a face without surface.
    pub face: Option<LoopFace<'a>>,
    /// The edge uses of the primary loopuse in loop order (`m_pLU->GetEdgeuses`);
    /// empty for a vertex loop.
    pub edgeuses: Vec<LoopEdgeuse<'a>>,
}

/// The result of [`contains_uv_point`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvContainment {
    /// `SM_POC_INSIDE`, `SM_POC_OUTSIDE`, `SM_POC_ON_BOUNDARY`, or `SM_POC_UNKNOWN`
    /// on failure.
    pub containment: PointContainment,
    /// `rpOnEdge`: the edge the point lies on (only for `OnBoundary`).
    pub on_edge: Option<EdgeId>,
    /// `rdEdgeParam`: its edge parameter (only for `OnBoundary`; else `0`).
    pub edge_param: f64,
    /// `eOrientation`: [`OrientType::Same`] for a counter-clockwise loop around an
    /// inside point, [`OrientType::Opposite`] for a clockwise one (only for `Inside`;
    /// else [`OrientType::Unknown`]).
    pub orientation: OrientType,
}

impl UvContainment {
    const fn of(containment: PointContainment) -> Self {
        Self {
            containment,
            on_edge: None,
            edge_param: 0.0,
            orientation: OrientType::Unknown,
        }
    }

    const fn on(edge: EdgeId, edge_param: f64) -> Self {
        Self {
            containment: PointContainment::OnBoundary,
            on_edge: Some(edge),
            edge_param,
            orientation: OrientType::Unknown,
        }
    }
}

/// The result of [`uv_corner_boundaries`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UvCornerBoundaries {
    /// `SM_SS_NONE` or an OR of `SM_SS_UMIN`, `SM_SS_VMIN`, `SM_SS_UMAX`, `SM_SS_VMAX`.
    pub boundaries: u32,
    /// `rOnEdges`: the edges with an end on a side, one entry per end.
    pub on_edges: Vec<EdgeId>,
    /// `rOnBndries`: the sides each of those ends is on.
    pub on_boundaries: Vec<u32>,
    /// `rOnParams`: the trim curve parameter of each of those ends.
    pub on_params: Vec<f64>,
}

/// `GetOrCreateUVTrimCurve` of an edge use: its existing trim curve.
fn uv_trim_curve<'a>(eu: &LoopEdgeuse<'a>) -> Result<&'a Curve> {
    eu.uv_trim_curve.ok_or_else(|| {
        // UNPORTED(topology.edgeuse_p3): SmEdgeuse::GetOrCreateUVTrimCurve
        SolidError::unported("SmEdgeuse::GetOrCreateUVTrimCurve", "topology.edgeuse_p3")
    })
}

/// `SmLoop::GetUVCornerBoundaries`: the sides of `uv_domain` touched (within `tol`)
/// by the start or end of a UV trim curve of the loop, with the edge and trim curve
/// parameter of every touching end.
///
/// # Errors
///
/// [`SolidError::Unported`] for an edge use without UV trim curve
/// (`SmEdgeuse::GetOrCreateUVTrimCurve`, `topology.edgeuse_p3`); those of the trim
/// curve evaluations.
pub fn uv_corner_boundaries(
    lp: &FaceLoop<'_>,
    uv_domain: &Extent2d,
    tol: f64,
) -> Result<UvCornerBoundaries> {
    // init return values
    let mut out = UvCornerBoundaries {
        boundaries: SS_NONE,
        ..UvCornerBoundaries::default()
    };

    // for every edgeuse - classify the end points
    for eu in &lp.edgeuses {
        // get UVTrimCurve (the recovery branch for a missing trim curve is behind
        // its creation)
        let trim = uv_trim_curve(eu)?;

        // end points
        let ivl = trim.natural_interval();
        let uv_beg = trim.evaluate_point(ivl.evaluate(0.0))?;
        let uv_end = trim.evaluate_point(ivl.evaluate(1.0))?;
        let uv_pnt_beg = Point2::new(uv_beg.x, uv_beg.y);
        let uv_pnt_end = Point2::new(uv_end.x, uv_end.y);

        // classify the BegPoint
        let these = uv_domain.point_boundaries(uv_pnt_beg, tol);
        out.boundaries |= these;
        if these != 0 {
            out.on_edges.push(eu.edge);
            out.on_boundaries.push(these);
            out.on_params.push(ivl.evaluate(0.0));
        }

        // classify the endPoint - in a perfect world we can skip this, but allow for
        // tolerance errors
        let these = uv_domain.point_boundaries(uv_pnt_end, tol);
        out.boundaries |= these;
        if these != 0 {
            out.on_edges.push(eu.edge);
            out.on_boundaries.push(these);
            out.on_params.push(ivl.evaluate(1.0));
        }
    }

    // all done
    Ok(out)
}

/// `sm_CalcQuadNum` (unit `topology.loopuse`): the quadrant of `test` in a frame
/// centred on `origin` (`1 | 0` above, `2 | 3` below).
fn calc_quad_num(origin: Point3, test: Point3) -> i32 {
    crate::topology::loopuse::calc_quad_num(origin, test) as i32
}

/// `SmSurface::GetSingularities`: the sides of the natural domain that are poles.
fn surface_singularities(k: &Kernel, surface: &Surface) -> Result<u32> {
    Ok(surface.get_singularities(k, false, false)?.singular)
}

/// `DropPoint` of the test point onto an edge curve (`SM_SO_INTERSECT`, no snapping
/// vector, no guess): the parameter when it lies within `tol`. A failing drop is no
/// drop (its status is ignored upstream).
fn drop_on_edge(k: &Kernel, eu: &LoopEdgeuse<'_>, point: Point3, tol: f64) -> Result<Option<f64>> {
    match eu.curve.drop_point(
        k,
        &eu.interval,
        point,
        None,
        tol,
        None,
        SolverOperation::Intersect,
    ) {
        Ok(d) => Ok(d.success.then_some(d.parameter)),
        Err(e @ SolidError::Unported { .. }) => Err(e),
        Err(_) => Ok(None),
    }
}

/// `LocalPointSolve(SM_SO_MINIMIZE)` of the test point on a UV trim curve: whether it
/// converged and the solution. A failing solve has not converged (its status is
/// ignored upstream and the flag is reset first).
fn local_minimize(
    trim: &Curve,
    interval: &Extent1d,
    point: Point3,
    tol_uv: f64,
    guess: f64,
) -> Result<Option<Solution<()>>> {
    let opts = PointSolveOptions {
        distance_tolerance: Some(tol_uv),
        ..PointSolveOptions::default()
    };
    match trim.local_point_solve::<()>(interval, SolverOperation::Minimize, point, &opts, guess) {
        Ok((true, sol)) => Ok(Some(sol)),
        Err(e @ SolidError::Unported { .. }) => Err(e),
        Ok((false, _)) | Err(_) => Ok(None),
    }
}

/// The point and unit tangent (in the edge use's direction) of a trim curve at `t`,
/// with the tangent's length (`1` when below `SM_EFF_ZERO`).
fn eval_with_tangent(trim: &Curve, t: f64, orient: OrientType) -> Result<(Point3, Vec3, f64)> {
    let der = trim.evaluate(t, 1, false, true)?;
    let pt = der.first().copied().unwrap_or(Vec3::UNDEF);
    let mut tan = der.get(1).copied().unwrap_or(Vec3::UNDEF);
    if orient == OrientType::Opposite {
        tan = -tan;
    }
    let mut len = tan.length();
    if len < EFF_ZERO {
        len = 1.0;
    }
    tan /= len;
    Ok((pt, tan, len))
}

/// The unit vector from `origin` to `p` (unscaled when shorter than `SM_EFF_ZERO`)
/// and the distance.
fn unit_from(origin: Point3, p: Point3) -> (Vec3, f64) {
    let mut v = p - origin;
    let dist = v.length();
    if dist > EFF_ZERO {
        v /= dist;
    }
    (v, dist)
}

/// `SmLoop::ContainsUVPoint`: classifies the parameter-space point `uv` against this
/// loop only (other loops of the face are not looked at) by counting the quadrant
/// changes of the loop's UV trim curves around it: `±4` is inside (counter-clockwise
/// [`OrientType::Same`], clockwise [`OrientType::Opposite`]), `0` outside, with one
/// quadrant of noise allowed. The point is on the boundary when its surface point
/// lies on an edge curve within the edge's zone tolerance, when a trim curve passes
/// within the face's zone tolerance mapped to UV space, or when it lies on a gap
/// between consecutive edge uses (a pole); a point on a pole side shared by the loop
/// is on the boundary too. A vertex loop contains nothing. `_tol_3d` is unused, as
/// upstream.
///
/// # Errors
///
/// [`SolidError::Unported`]: `SmEdgeuse::GetOrCreateUVTrimCurve` for an edge use
/// without trim curve (`topology.edgeuse_p3`), and those of the callees (the
/// tolerance of a face-owned surface in `SmSurface::GetSingularities`); the errors of the surface and trim curve
/// evaluations (see the module Deviations).
pub fn contains_uv_point(
    k: &Kernel,
    lp: &FaceLoop<'_>,
    uv: Point2,
    _tol_3d: f64,
) -> Result<UvContainment> {
    // Work from Loopuses, because they will give us an ordered list of Edgeuses.
    let eus = &lp.edgeuses;
    let Some(last_eu) = eus.last() else {
        // could be a vertex loopuse
        return Ok(UvContainment::of(PointContainment::Outside));
    };

    // check state - must have a Face and surface
    let Some(face) = lp.face else {
        return Ok(UvContainment::of(PointContainment::Unknown));
    };
    let surface = face.surface;

    let srf_uv_domain = surface.natural_uv_domain();
    let point_3d = surface.evaluate_point(uv)?;
    let uv_pt = Point3::new(uv.x, uv.y, 0.0);

    // Get a tolerance in uv space.
    // Scale the input tolerance by the UV deriv size.
    let zone_tol = face.tolerance;
    let mut tol_uv = map_to_2d(zone_tol, uv, surface)?;

    // minimize pole checking - if UVPnt on boundary -then-> if Loop on boundary -then->
    // if GetSingularities -then-> expensive check

    // Is crUVPoint on Surface Natural UVBoundary (where poles are allowed)
    let uv_boundaries = srf_uv_domain.point_boundaries(uv, tol_uv);

    // When crUVPoint is on Surface UVBoundary - does Loop have any edgeuse ends on
    // Surface Natural UVBoundary
    let corners = if uv_boundaries != 0 {
        uv_corner_boundaries(lp, &srf_uv_domain, tol_uv)?
    } else {
        UvCornerBoundaries::default()
    };
    let loop_boundaries = corners.boundaries;

    // When point and loop touch the surface UVBoundaries - does surface have poles
    let poles = if uv_boundaries & loop_boundaries != 0 {
        surface_singularities(k, surface)?
    } else {
        0
    };

    // remember when checking for points on poles is required
    let common_poles = poles & loop_boundaries & uv_boundaries;

    // when crUVPoint and Loop share a common singular edge - classify the point
    if common_poles != 0 {
        let mut out = UvContainment::of(PointContainment::OnBoundary);
        // set output with first hit on a singluar boundary
        for ((edge, bnd), param) in corners
            .on_edges
            .iter()
            .zip(&corners.on_boundaries)
            .zip(&corners.on_params)
        {
            if bnd & common_poles != 0 {
                out.on_edge = Some(*edge);
                out.edge_param = *param;
                break;
            }
        }

        // UVPoints on Singular boundaries shared by the loop are always classified
        // ON_BOUNDARY
        return Ok(out);
    }

    // check input:
    if tol_uv < EFF_ZERO * 100.0 {
        tol_uv = EFF_ZERO * 100.0;
    }

    // Locals:
    let mut accum: i32 = 0; // This is the main result: 0, 4 or -4 when done.
    let mut this_quad_num: i32 = 0; // Quadrant numbers: 0-3 going CCW.
    let mut this_param = 0.0;
    // Uninitialised upstream; first written by the walk of an edge use longer than
    // SM_EFF_ZERO.
    let mut this_uv = Vec3::UNDEF;

    // Start with the end of the final Edgeuse.
    let mut last_end_eu = last_eu;
    let mut last_end_param = if last_end_eu.orientation == OrientType::Same {
        last_end_eu.interval.max()
    } else {
        last_end_eu.interval.min()
    };
    let last_trim = uv_trim_curve(last_end_eu)?;
    let mut last_end_uv = last_trim.evaluate_point(last_end_param)?;
    let mut last_end_quad_num = calc_quad_num(uv_pt, last_end_uv);

    // Now start the outer loop, on Edgeuses.
    for this_eu in eus {
        // low work: see if TgtPoint is on this EdgeCurve
        if let Some(param) = drop_on_edge(k, this_eu, point_3d, this_eu.tolerance)? {
            // TgtPoint is on EdgeCurve - set output and return
            return Ok(UvContainment::on(this_eu.edge, param));
        }

        let trim = uv_trim_curve(this_eu)?;

        // Grab some info about this EU and its uv curve.
        let orient = this_eu.orientation;
        let domain = this_eu.interval;
        let (mut prev_param, end_param) = if orient == OrientType::Opposite {
            (domain.max(), domain.min())
        } else {
            (domain.min(), domain.max())
        };

        // These have to do with step size:
        // (dDefaultParamStep: every step below overwrites it, so it is never used)
        let _default_param_step = domain.length() / trim.number_control_points()? as f64;
        let min_param_step = domain.length() / 1000.0;
        let max_param_step = domain.length() / 20.0;
        let curve_is_linear = trim.is_linear(tol_uv, false)?.linear;

        // Eval info for start point of this EU.
        let (mut prev_uv, mut prev_uv_tan, mut prev_uv_tan_len) =
            eval_with_tangent(trim, prev_param, orient)?;
        let (prev_vec, mut prev_dist) = unit_from(uv_pt, prev_uv);
        let mut prev_cos = prev_vec.dot(prev_uv_tan);

        // Note: if dPrevDist < tol, then this uv pt is on the edge.
        if prev_dist < tol_uv {
            // We should refine the point.
            if let Some(sol) = local_minimize(trim, &this_eu.interval, uv_pt, tol_uv, prev_param)? {
                // since the guess was good enough - we expect the refined solution to
                // be as good or better
                if sol.start.solution_value <= tol_uv {
                    // return the refined answer.
                    return Ok(UvContainment::on(this_eu.edge, sol.start.param(0)?));
                }
            }
            // Should not arrive here... (inconsistent classification; continue)
        }

        // Calculate the initial quadrant.
        let mut prev_quad_num = calc_quad_num(uv_pt, prev_uv);

        // Check here for non-closed loops.
        let delta = prev_quad_num - last_end_quad_num;

        // Check for and accumulate any change in quadrant.
        if delta != 0 {
            // First check 'on'.
            let (seg_dist, seg_param) =
                line_point_distance(last_end_uv, prev_uv - last_end_uv, uv_pt);
            if seg_dist <= tol_uv && seg_param > -EFF_ZERO && seg_param < 1.0 + EFF_ZERO {
                // On. Return closer end point.
                return Ok(if seg_param <= 0.5 {
                    UvContainment::on(last_end_eu.edge, last_end_param)
                } else {
                    UvContainment::on(this_eu.edge, prev_param)
                });
            }

            if delta == 1 || delta == -3 {
                accum += 1;
            } else if delta == -1 || delta == 3 {
                accum -= 1;
            } else {
                // +- 2: diagonal.  Here, we're just shooting from one point to the
                // next, so treat it as a straight line. See whether we're heading CW
                // or CCW around the test point: use a cross product.
                let cross = (prev_uv - last_end_uv).cross(uv_pt - last_end_uv);
                if cross.z > 0.0 {
                    accum += 2; // CCW
                } else {
                    accum -= 2; // CW
                }
            }
        } // end check for change from end of last curve

        // A flag to be used in this next loop:
        let mut did_local_solve_last_time = false;

        // Now step along this Edgeuse.
        let mut loop_count1 = 0;
        while (end_param - prev_param).abs() > EFF_ZERO {
            loop_count1 += 1;
            if loop_count1 > MAX_LOOP_COUNT {
                // Infinite Loop.
                return Ok(UvContainment::of(PointContainment::Unknown));
            }

            // Get the parameter for the next sample point.
            let param_step = if curve_is_linear {
                // For straight lines, we can shoot right to the end.
                (end_param - prev_param).abs()
            } else {
                // When we're far from the test point, we can take bigger steps.
                // Also increase step size if we're moving away from the test point:
                // a fraction of the distance to the test point, from 1/4 (directly
                // towards) through 1/2 (perpendicular) to 1 (directly away).
                // Fraction = 1/8 cos^2  +  3/8 cos  +  1/2.
                let frac = prev_cos * (prev_cos + 3.0) / 8.0 + 0.5;
                let move_dist = frac * prev_dist;
                let mut param_step = move_dist / prev_uv_tan_len;

                if param_step < min_param_step {
                    param_step = min_param_step;
                }
                if param_step > max_param_step {
                    param_step = max_param_step;
                }
                param_step
            };

            // Got param step, set dThisParam
            if orient == OrientType::Same {
                this_param = prev_param + param_step;
                if this_param > end_param {
                    this_param = end_param;
                }
            } else {
                this_param = prev_param - param_step;
                if this_param < end_param {
                    this_param = end_param;
                }
            }

            // Here we have the next candidate curve parameter, for a step. Evaluate
            // the curve and get all of the state variables we need; when a better
            // value is found, come back here (upstream `goto Got_dThisParam`).
            let mut loop_count2 = 0;
            let (this_uv_tan, this_uv_tan_len, this_dist, this_cos) = loop {
                loop_count2 += 1;
                if loop_count2 > MAX_LOOP_COUNT {
                    // Infinite Loop2.
                    return Ok(UvContainment::of(PointContainment::Unknown));
                }

                let (uv_at, this_uv_tan, this_uv_tan_len) =
                    eval_with_tangent(trim, this_param, orient)?;
                this_uv = uv_at;
                let (this_vec, this_dist) = unit_from(uv_pt, this_uv);
                let this_cos = this_vec.dot(this_uv_tan);

                // A check for too big a step: if the direction changed by more than,
                // say, 90 degrees. But, if the curve is linear, and it doubles back on
                // itself (it happens), we don't have to step along, can just shoot to
                // the end. [Fillet regression 2:237]
                if !curve_is_linear
                    && prev_uv_tan.dot(this_uv_tan) < 0.0
                    // Cusps happen.  In case of a turnaround in a very short interval,
                    // just continue on with the next parameter value.
                    && (this_param - prev_param).abs() > min_param_step
                {
                    this_param = (this_param + prev_param) / 2.0;
                    continue;
                }

                // Here we need to check whether this segment contains the test point.
                let mut check_on_curve = false;

                // If we do check on-curve, we will use dSegParam to find a guess
                // parameter for LocalPointSolve.
                let mut seg_param = 0.5; // just in case nothing else sets it...

                // First, if the current point equals the test point:
                if this_dist <= tol_uv {
                    check_on_curve = true;
                    seg_param = 1.0; // end of segment
                }

                // If the previous point was approaching and this point is receding:
                if !check_on_curve
                    && !did_local_solve_last_time
                    && (prev_cos < -0.001 && this_cos > 0.001)
                {
                    let (seg_dist, sp) = line_point_distance(prev_uv, this_uv - prev_uv, uv_pt);
                    seg_param = sp;

                    // Check whether we should check whether the pt is on the curve.
                    check_on_curve = seg_dist <= tol_uv;

                    // tidy up a projection exterior to the line segment
                    seg_param = seg_param.clamp(0.0, 1.0);

                    // Check whether this curve segment could possibly reach the test pt.
                    if !check_on_curve && !curve_is_linear {
                        // Extend start & end tangents to see whether they could reach
                        // as far as the test point.

                        // We know that dPrevCos < 0...
                        let chk_pt = prev_uv
                            + prev_uv_tan * prev_uv_tan_len * (this_param - prev_param).abs();
                        let (chk_pt_dist, _) =
                            line_point_distance(prev_uv, this_uv - prev_uv, chk_pt);
                        check_on_curve = chk_pt_dist >= seg_dist;

                        // ... and that dThisCos > 0.
                        if !check_on_curve {
                            // Try other end of segment.
                            let chk_pt = this_uv
                                - this_uv_tan * this_uv_tan_len * (this_param - prev_param).abs();
                            let (chk_pt_dist, _) =
                                line_point_distance(prev_uv, this_uv - prev_uv, chk_pt);
                            check_on_curve = chk_pt_dist >= seg_dist;
                        }
                    } // end more-detailed check for possible on-curve.
                } // end cosine-change check, for passing the test point.

                did_local_solve_last_time = false;

                if check_on_curve {
                    // We have a very good guess parameter for a local solve.
                    let guess_t = prev_param + seg_param * (this_param - prev_param);

                    // Interval: between prev pt and this pt.  Loosen up a bit.
                    let mut crv_ivl = Extent1d::default();
                    crv_ivl.add_value(prev_param);
                    crv_ivl.add_value(this_param);
                    crv_ivl.expand_absolute(EFF_ZERO);

                    if let Some(sol) = local_minimize(trim, &crv_ivl, uv_pt, tol_uv, guess_t)? {
                        if sol.start.solution_value <= tol_uv {
                            return Ok(UvContainment::on(this_eu.edge, sol.start.param(0)?));
                        }

                        // We have a local min/max to the curve: do another point at
                        // the solution value, to avoid the case where the test point
                        // is between the segment chord and the actual curve. The
                        // solution will have a cosine of zero: use that fact to avoid
                        // getting stuck.
                        let solve_t = sol.start.param(0)?;
                        if crv_ivl.contains_value(solve_t, 0.0001) {
                            this_param = solve_t;
                            did_local_solve_last_time = true;
                            continue;
                        }
                    } // end if bFoundAnswer
                } // end LocalPointSolve check for test pt on curve

                // Got the next sample point, find its quadrant.
                this_quad_num = calc_quad_num(uv_pt, this_uv);
                let delta = this_quad_num - prev_quad_num;

                // Check for and accumulate any change in quadrant.
                if delta == 0 {
                } else if delta == 1 || delta == -3 {
                    accum += 1;
                } else if delta == -1 || delta == 3 {
                    accum -= 1;
                } else {
                    // +- 2: diagonal. Just take a smaller step; is doesn't have to be
                    // much smaller.
                    this_param = prev_param + 0.75 * (this_param - prev_param);
                    continue;
                }
                break (this_uv_tan, this_uv_tan_len, this_dist, this_cos);
            };

            // Update This/Prev data.
            prev_param = this_param;
            prev_uv = this_uv;
            prev_uv_tan = this_uv_tan;
            prev_uv_tan_len = this_uv_tan_len;
            prev_dist = this_dist;
            prev_cos = this_cos;
            prev_quad_num = this_quad_num;
        } // end while stepping along this EU

        // Update the last-end stuff, for checking non-closed loops.
        last_end_eu = this_eu;
        last_end_param = this_param;
        last_end_uv = this_uv;
        last_end_quad_num = this_quad_num;
    } // end loop on all EU's

    // Check the result: iAccum
    if accum == 0 {
        return Ok(UvContainment::of(PointContainment::Outside));
    } else if accum == 4 || accum == -4 {
        return Ok(inside(accum));
    }

    // It's common for uv points to have 'exactly' the same u or v value as the input
    // point.  In that case, a single bit of noise can change the quadrant
    // classification by one. So we'll allow a one-quadrant error here.
    if accum == 1 || accum == -1 {
        return Ok(UvContainment::of(PointContainment::Outside));
    } else if accum == 3 || accum == -3 || accum == 5 || accum == -5 {
        return Ok(inside(accum));
    }

    // This is an error.  The only bit-noise error should be at the start/end of the
    // loop, and it can't be off by two.
    Ok(UvContainment::of(PointContainment::Unknown))
}

/// `SM_POC_INSIDE` with the loop orientation of the winding sign.
fn inside(accum: i32) -> UvContainment {
    UvContainment {
        containment: PointContainment::Inside,
        on_edge: None,
        edge_param: 0.0,
        orientation: if accum > 0 {
            OrientType::Same
        } else {
            OrientType::Opposite
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::math::Vec2;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::surface::plane::Plane;

    fn plane() -> Surface {
        Surface::from(Plane::new(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-20.0, -20.0, 20.0, 20.0),
        ))
    }

    fn trim(a: [f64; 2], b: [f64; 2]) -> Curve {
        let pw = vec![
            CPoint::new(a[0], a[1], 0.0, NOW),
            CPoint::new(b[0], b[1], 0.0, NOW),
        ];
        let cur = NurbCurve::new(pw, 1, vec![0.0, 0.0, 1.0, 1.0]).unwrap();
        Curve::from(BSplineCurve::from_nurb(cur, 2, false))
    }

    /// The square `[-5, 5]^2` counter-clockwise (or clockwise) as 4 line edges.
    fn square(ccw: bool) -> (Vec<Curve>, Vec<Curve>) {
        let mut v = [[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]];
        if !ccw {
            v.reverse();
        }
        let mut edges = Vec::new();
        let mut trims = Vec::new();
        for i in 0..4 {
            let (a, b) = (v[i], v[(i + 1) % 4]);
            edges.push(Curve::from(Line::from_points(
                Vec3::new(a[0], a[1], 0.0),
                Vec3::new(b[0], b[1], 0.0),
                3,
            )));
            trims.push(trim(a, b));
        }
        (edges, trims)
    }

    fn view<'a>(s: &'a Surface, edges: &'a [Curve], trims: &'a [Curve]) -> FaceLoop<'a> {
        FaceLoop {
            face: Some(LoopFace {
                surface: s,
                tolerance: 1e-6,
            }),
            edgeuses: edges
                .iter()
                .zip(trims)
                .enumerate()
                .map(|(i, (c, t))| LoopEdgeuse {
                    edge: EdgeId::from_index(i as u32),
                    curve: c,
                    interval: Extent1d::from_min_max(0.0, 1.0),
                    tolerance: 1e-6,
                    orientation: OrientType::Same,
                    uv_trim_curve: Some(t),
                })
                .collect(),
        }
    }

    #[test]
    fn square_inside_outside_on() {
        let k = Kernel::default();
        let s = plane();
        let (e, t) = square(true);
        let lp = view(&s, &e, &t);
        let r = contains_uv_point(&k, &lp, Point2::new(0.0, 0.0), 0.0).unwrap();
        assert_eq!(r.containment, PointContainment::Inside);
        assert_eq!(r.orientation, OrientType::Same);
        let r = contains_uv_point(&k, &lp, Point2::new(8.0, 1.0), 0.0).unwrap();
        assert_eq!(r.containment, PointContainment::Outside);
        assert_eq!(r.on_edge, None);
        let r = contains_uv_point(&k, &lp, Point2::new(5.0, 0.0), 0.0).unwrap();
        assert_eq!(r.containment, PointContainment::OnBoundary);
        assert_eq!(r.on_edge, Some(EdgeId::from_index(1)));
        assert!((r.edge_param - 0.5).abs() < 1e-12);

        let (e, t) = square(false);
        let lp = view(&s, &e, &t);
        let r = contains_uv_point(&k, &lp, Point2::new(1.0, -2.0), 0.0).unwrap();
        assert_eq!(r.containment, PointContainment::Inside);
        assert_eq!(r.orientation, OrientType::Opposite);
    }

    #[test]
    fn vertex_loop_face_and_missing_trim() {
        let k = Kernel::default();
        let s = plane();
        let empty = FaceLoop {
            face: None,
            edgeuses: Vec::new(),
        };
        let r = contains_uv_point(&k, &empty, Point2::new(0.0, 0.0), 0.0).unwrap();
        assert_eq!(r, UvContainment::of(PointContainment::Outside));

        let (e, t) = square(true);
        let mut lp = view(&s, &e, &t);
        lp.face = None;
        let r = contains_uv_point(&k, &lp, Point2::new(0.0, 0.0), 0.0).unwrap();
        assert_eq!(r.containment, PointContainment::Unknown);

        let mut lp = view(&s, &e, &t);
        if let Some(eu) = lp.edgeuses.last_mut() {
            eu.uv_trim_curve = None;
        }
        let err = contains_uv_point(&k, &lp, Point2::new(0.0, 0.0), 0.0).unwrap_err();
        assert!(matches!(err, SolidError::Unported { .. }));
    }

    #[test]
    fn corner_boundaries_and_poles() {
        let k = Kernel::default();
        let s = plane();
        let (e, t) = square(true);
        let lp = view(&s, &e, &t);
        let dom = Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0);
        let b = uv_corner_boundaries(&lp, &dom, 1e-9).unwrap();
        assert_eq!(b.boundaries, 15);
        assert_eq!(b.on_edges.len(), 8);
        assert_eq!(b.on_params.first(), Some(&0.0));

        // a point on a domain side with the loop on that side reaches GetSingularities
        let s2 = Surface::from(Plane::new(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &dom,
        ));
        let lp = view(&s2, &e, &t);
        // a plane has no singular sides, so GetSingularities is wired and no longer defers
        let r = contains_uv_point(&k, &lp, Point2::new(-5.0, 0.0), 0.0);
        assert!(!matches!(r, Err(SolidError::Unported { .. })));
    }

    #[test]
    fn quadrants() {
        let o = Point3::ZERO;
        assert_eq!(calc_quad_num(o, Point3::new(1.0, 1.0, 0.0)), 0);
        assert_eq!(calc_quad_num(o, Point3::new(-1.0, 1.0, 0.0)), 1);
        assert_eq!(calc_quad_num(o, Point3::new(-1.0, -1.0, 0.0)), 2);
        assert_eq!(calc_quad_num(o, Point3::new(1.0, -1.0, 0.0)), 3);
        assert_eq!(calc_quad_num(o, o), 0);
    }
}
