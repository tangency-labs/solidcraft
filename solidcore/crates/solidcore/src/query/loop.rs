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
//! | `calc_quad_num` (private)    | `sm_CalcQuadNum` (file static, unit `topology.loopuse`) |
//! | `WindingHull` (crate)        | none (a shortcut for `sm_AssertTestFace37`)    |
//! | `WalkExclusion` (crate)      | none (a shortcut for `SmFace::PointClassify`)  |
//!
//! Deviations:
//! - **Topology.** The loop is a [`LoopId`] of a [`Brep`], taken mutably: upstream's
//!   `SmEdgeuse::GetOrCreateUVTrimCurve` (`topology.edgeuse_p3`) attaches the missing
//!   UV trim curves to the B-rep while the walk goes on, and edits the edge (interval,
//!   tolerance) it dropped the curve for. Every edge value is read from the B-rep where
//!   upstream reads it, after the creation. `SmLoop::GetUVCornerBoundaries` is
//!   [`crate::topology::loopuse::uv_corner_boundaries`]. `surface` is the caller's copy
//!   of the face's surface (`SmFace::PointClassify` evaluates with out-of-bounds
//!   evaluation enabled on it), else the face's own; the zone tolerance is the face's
//!   (`SmTol::GetZoneTol3d(pSurface)` of the face's own surface, whose owner is the
//!   face; the arena surfaces carry no owner). A face without surface gives
//!   [`PointContainment::Unknown`], as upstream.
//!   The same walk accepts prepared, read-only topology internally; a missing trim
//!   reports [`SolidError::Unported`] only when the walk needs it, without eager work.
//!   Temporary surface copies retain exact evaluation data and owner tolerances.
//! - **UV trim curves.** A failing status of `GetOrCreateUVTrimCurve` (a recorded drop
//!   failure of the existing curve, or a failed creation) or a missing curve gives
//!   `SM_POC_UNKNOWN`, as upstream.
//! - **Poles.** `SmSurface::GetSingularities` (unit `surface.surface_p3`) is only
//!   reached when the point lies on a side of the surface's natural domain and an edge
//!   use end of the loop does too.
//! - **Ignored statuses.** Upstream ignores the status of `EvaluatePoint` /
//!   `Evaluate` of the surface and the trim curves and then reads the outputs they did
//!   not write. For the trim curves (B-splines, whose failed evaluation writes
//!   nothing) the port does the same: a failed `EvaluatePoint` of the last edge use's
//!   trim leaves `sLastEndUV` at its `SM_UNDEF_DOUBLE` default, and a failed
//!   `Evaluate(.., 1, ..)` leaves `sPtDer` as it was (its `SM_UNDEF_DOUBLE` defaults
//!   for the edge use's first evaluation, else the previous evaluation's point and
//!   derivative). A trim curve whose basis fails to evaluate (degree 3 with 2 control
//!   points, from a STEP cylinder band) reaches it. A failed surface `EvaluatePoint`
//!   still returns its error (no input reaches it). With the opt-in
//!   `Kernel::fixes.query_statuses_pass_on` (WR-663) a failed evaluation or local solve
//!   answers `SM_POC_UNKNOWN`, as the fork (tangency-labs/smlib 7a35f33).
//!   `DropPoint` and `LocalPointSolve` failures are "no answer" (both reset their
//!   success flag first upstream). [`SolidError::Unported`] from any callee is
//!   returned.
//! - `dTol3d` is unused upstream and is kept as `_tol_3d`.
//! - **Winding hulls.** Not upstream: `WindingHull` bounds every UV point the walk can
//!   evaluate on a loop, so a caller that only asks whether a point is inside can leave
//!   out the walk for a point outside it (`SmFace::AssertValid` test 37, which tests every
//!   inner loop against every other). `WalkExclusion` adds what rules out an answer on
//!   the loop too (the point off the hull by the walk's UV tolerance, its surface point
//!   off every edge curve's box by the edge's), so a face's point classification, which
//!   stops only on those answers, can leave out its inner loops' walks.
//! - **Loops.** Both walk loops keep upstream's bounds (1000 steps per edge use and
//!   1000 re-evaluations per step, then `SM_POC_UNKNOWN`); the `goto Got_dThisParam`
//!   re-evaluations are an inner loop. No recursion.

use crate::curve::curve::p3::PointSolveOptions;
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::geom_utility::p1::line_point_distance;
use crate::geom::global_solver::SolverOperation;
use crate::geom::point_set::PointContainment;
use crate::geom::solution_array::Solution;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::p1::map_to_2d;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::loopuse::{uv_corner_boundaries_with_topology, QueryTopology};
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, LoopId};

/// Upper bound of the steps along one edge use and of the re-evaluations of one step
/// (upstream's infinite-loop guards).
const MAX_LOOP_COUNT: u32 = 1000;

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

/// `sm_CalcQuadNum` (unit `topology.loopuse`): the quadrant of `test` in a frame
/// centred on `origin` (`1 | 0` above, `2 | 3` below).
fn calc_quad_num(origin: Point3, test: Point3) -> i32 {
    crate::topology::loopuse::calc_quad_num(origin, test) as i32
}

fn null(reason: &'static str) -> SolidError {
    SolidError::invalid_input("query::loop::contains_uv_point", reason)
}

/// The edge of an edge use (`GetEdge`, dereferenced by `ContainsUVPoint`).
fn edge_of(b: &Brep, eu: EdgeuseId) -> Result<EdgeId> {
    eu1::edge(b, eu)?.ok_or_else(|| null("ContainsUVPoint: edge use without edge"))
}

/// `SmSurface::GetSingularities`: the sides of the natural domain that are poles.
fn surface_singularities(k: &Kernel, surface: &Surface) -> Result<u32> {
    Ok(surface.get_singularities(k, false, false)?.singular)
}

/// `DropPoint` of the test point onto an edge curve (`SM_SO_INTERSECT`, no snapping
/// vector, no guess): the parameter when it lies within `tol`. A failing drop is no
/// drop (its status is ignored upstream).
fn drop_on_edge(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    point: Point3,
    tol: f64,
) -> Result<Option<f64>> {
    match curve.drop_point(
        k,
        interval,
        point,
        None,
        tol,
        None,
        SolverOperation::Intersect,
    ) {
        Ok(d) => Ok(d.success.then_some(d.parameter)),
        Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => Err(e),
        Err(_) => Ok(None),
    }
}

/// The value of a call whose failure the port returns, or `None` when, with the opt-in
/// [`crate::kernel::UpstreamFixes::query_statuses_pass_on`], the point is not classified
/// (`SM_POC_UNKNOWN`, tangency-labs/smlib 7a35f33). [`SolidError::Unported`] propagates.
fn or_unknown<T>(k: &Kernel, r: Result<T>) -> Result<Option<T>> {
    match r {
        Err(e) if k.fixes.query_statuses_pass_on && !e.is_unported_or_cancelled() => Ok(None),
        r => r.map(Some),
    }
}

/// Not upstream: whether `point` may lie within `reach` of `curve` over `interval`, so
/// that [`drop_on_edge`] there can find it (a drop succeeds only for a solution closer than
/// its tolerance). `false` only when the point lies outside the box of the curve's points
/// over the interval, grown by `reach` and by an evaluation's rounding (`1e-6` of the box's
/// size and of the coordinates, as [`WindingHull::excludes`]), in some coordinate. That box
/// is, for a line, the box of its points at the interval's ends (its points between lie
/// between them); for a B-spline with positive weights whose basis is well made (as many
/// control points as its knots and degree call for, the knots in order) over an interval
/// inside its domain, the box of its control points (each point is a convex combination of
/// them). Every other curve may reach every point. Neither curve's copy for evaluation can
/// fail, so leaving out its copy and drop changes no result.
fn curve_box_reaches(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    point: Point3,
    reach: f64,
) -> bool {
    use crate::curve::curve::CurveKind;
    use crate::nurbs::math::NOW;
    let mut lo = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut add = |e: Point3| {
        lo = Point3::new(lo.x.min(e.x), lo.y.min(e.y), lo.z.min(e.z));
        hi = Point3::new(hi.x.max(e.x), hi.y.max(e.y), hi.z.max(e.z));
    };
    match curve.kind() {
        CurveKind::Line(_) => {
            for t in [interval.min(), interval.max()] {
                let Ok(e) = curve.evaluate_point(k, t) else {
                    return true;
                };
                add(e);
            }
        }
        CurveKind::BSpline(c) => {
            let Some(nurb) = c.nurb.as_ref() else {
                return true;
            };
            let p = nurb.degree();
            let cpts = nurb.cpts();
            let well_made = p >= 1
                && cpts.len() > p
                && nurb.knots().len() == cpts.len() + p + 1
                && nurb
                    .knots()
                    .windows(2)
                    .all(|w| matches!(w, [a, b] if a <= b));
            let domain = c.natural_interval();
            if !(well_made && interval.min() >= domain.min() && interval.max() <= domain.max()) {
                return true;
            }
            for c in cpts {
                if !(c.w == NOW || (c.w > 0.0 && c.w.is_finite())) {
                    return true;
                }
                add(c.to_euclid());
            }
        }
        _ => return true,
    }
    if ![lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]
        .iter()
        .all(|v| v.is_finite())
    {
        return true;
    }
    let apart = |lo: f64, hi: f64, x: f64| {
        let m = reach + 1e-6 * (1.0 + (hi - lo) + lo.abs().max(hi.abs()) + x.abs());
        x < lo - m || x > hi + m
    };
    !(apart(lo.x, hi.x, point.x) || apart(lo.y, hi.y, point.y) || apart(lo.z, hi.z, point.z))
}

/// `LocalPointSolve(SM_SO_MINIMIZE)` of the test point on a UV trim curve: whether it
/// converged and the solution. A failing solve has not converged (its status is
/// ignored upstream and the flag is reset first); with the opt-in
/// [`crate::kernel::UpstreamFixes::query_statuses_pass_on`] it classifies nothing
/// (`Err(())`, the fork's `SM_POC_UNKNOWN`).
fn local_minimize(
    k: &Kernel,
    trim: &Curve,
    interval: &Extent1d,
    point: Point3,
    tol_uv: f64,
    guess: f64,
) -> Result<std::result::Result<Option<Solution<()>>, ()>> {
    let opts = PointSolveOptions {
        distance_tolerance: Some(tol_uv),
        ..PointSolveOptions::default()
    };
    match trim.local_point_solve::<()>(k, interval, SolverOperation::Minimize, point, &opts, guess)
    {
        Ok((true, sol)) => Ok(Ok(Some(sol))),
        Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => Err(e),
        Err(_) if k.fixes.query_statuses_pass_on => Ok(Err(())),
        Ok((false, _)) | Err(_) => Ok(Ok(None)),
    }
}

/// The point and unit tangent (in the edge use's direction) of a trim curve at `t`,
/// with the tangent's length (`1` when below `SM_EFF_ZERO`), evaluated into `pt_der`
/// (upstream `sPtDer`). A failed evaluation leaves `pt_der` as it was, as upstream
/// ignores the status of `Evaluate`; with `query_statuses_pass_on` it is the error.
fn eval_with_tangent(
    k: &Kernel,
    trim: &Curve,
    t: f64,
    orient: OrientType,
    pt_der: &mut [Vec3; 2],
) -> Result<(Point3, Vec3, f64)> {
    match trim.evaluate(k, t, 1, false, true) {
        Ok(der) => {
            for (slot, v) in pt_der.iter_mut().zip(der) {
                *slot = v;
            }
        }
        Err(e) if e.is_unported_or_cancelled() || k.fixes.query_statuses_pass_on => return Err(e),
        // status ignored upstream: sPtDer keeps its values
        Err(_) => {}
    }
    let [pt, mut tan] = *pt_der;
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

/// `SmLoop::ContainsUVPoint`: classifies the parameter-space point `uv` against loop
/// `l` of the B-rep only (other loops of the face are not looked at) by counting the
/// quadrant changes of the loop's UV trim curves around it: `±4` is inside
/// (counter-clockwise [`OrientType::Same`], clockwise [`OrientType::Opposite`]), `0`
/// outside, with one quadrant of noise allowed. The point is on the boundary when its
/// surface point lies on an edge curve within the edge's zone tolerance, when a trim
/// curve passes within the face's zone tolerance mapped to UV space, or when it lies
/// on a gap between consecutive edge uses (a pole); a point on a pole side shared by
/// the loop is on the boundary too. A vertex loop contains nothing. A UV trim curve
/// the edge use does not have is made (`GetOrCreateUVTrimCurve`) and attached to the
/// B-rep. `_tol_3d` is unused, as upstream.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] where upstream dereferences null (a loop without loop
/// use, a loop use without face use, a face use without face, an edge use without
/// edge, an edge without curve); [`SolidError::Unported`] from the callees (module
/// docs); the errors of the surface and trim curve evaluations (see the module
/// Deviations).
pub fn contains_uv_point(
    k: &Kernel,
    b: &mut Brep,
    l: LoopId,
    surface: Option<&Surface>,
    uv: Point2,
    tol_3d: f64,
) -> Result<UvContainment> {
    contains_uv_point_with_topology(k, &mut QueryTopology::Mutable(b), l, surface, uv, tol_3d)
}

/// Shared containment walk, with lazy trim creation limited to mutable topology.
pub(crate) fn contains_uv_point_with_topology(
    k: &Kernel,
    b: &mut QueryTopology<'_>,
    l: LoopId,
    surface: Option<&Surface>,
    uv: Point2,
    _tol_3d: f64,
) -> Result<UvContainment> {
    k.work_counters.add_loop_walk();
    // Work from Loopuses, because they will give us an ordered list of Edgeuses.
    let lu = b
        .loop_(l)?
        .loopuse
        .ok_or_else(|| null("ContainsUVPoint: loop without loop use"))?;
    let eus = eu1::loopuse_edgeuses(b, lu)?;
    let Some(&last_eu) = eus.last() else {
        // could be a vertex loopuse
        return Ok(UvContainment::of(PointContainment::Outside));
    };

    // check state - must have a Face and surface
    let fu = b
        .loopuse(lu)?
        .faceuse
        .ok_or_else(|| null("ContainsUVPoint: loop use without face use"))?;
    let f = b
        .faceuse(fu)?
        .face
        .ok_or_else(|| null("ContainsUVPoint: face use without face"))?;
    let face_rec = b.face(f)?;
    let own_surface;
    let surface: &Surface = match surface {
        Some(s) => s,
        None => match face_rec.surface.as_ref() {
            Some(s) => {
                // Keep the exact evaluation data and owner tolerance while the
                // mutable walk may attach trim curves to the B-rep.
                own_surface = s.clone_for_evaluation()?;
                &own_surface
            }
            None => return Ok(UvContainment::of(PointContainment::Unknown)),
        },
    };
    // `SmTol::GetZoneTol3d(pSurface)`: the surface is the face's (the face is its owner
    // upstream), so the face's tolerance
    let zone_tol = face_rec.tolerance;

    let srf_uv_domain = surface.natural_uv_domain_with_nurb(k)?;
    let Some(point_3d) = or_unknown(k, surface.evaluate_point(k, uv))? else {
        return Ok(UvContainment::of(PointContainment::Unknown));
    };
    let uv_pt = Point3::new(uv.x, uv.y, 0.0);

    // Get a tolerance in uv space.
    // Scale the input tolerance by the UV deriv size.
    let mut tol_uv = map_to_2d(k, zone_tol, uv, surface)?;

    // minimize pole checking - if UVPnt on boundary -then-> if Loop on boundary -then->
    // if GetSingularities -then-> expensive check

    // Is crUVPoint on Surface Natural UVBoundary (where poles are allowed)
    let uv_boundaries = srf_uv_domain.point_boundaries(uv, tol_uv);

    // When crUVPoint is on Surface UVBoundary - does Loop have any edgeuse ends on
    // Surface Natural UVBoundary
    let corners = if uv_boundaries != 0 {
        uv_corner_boundaries_with_topology(k, b, l, &srf_uv_domain, tol_uv)?
    } else {
        crate::topology::loopuse::UvCorners::default()
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
                out.on_edge = *edge;
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
    let mut last_end_edge = edge_of(b, last_eu)?;
    let mut last_end_param = {
        let ivl = b.edge(last_end_edge)?.interval;
        if eu1::orientation(b, last_eu)? == OrientType::Same {
            ivl.max()
        } else {
            ivl.min()
        }
    };
    if !b.ensure_trim_curve(k, last_eu)? {
        // stat != SM_SUCCESS || pThisUVCurve == NULL
        return Ok(UvContainment::of(PointContainment::Unknown));
    }
    let mut last_end_uv = {
        let last_trim = eu1::uv_trim_curve(b, last_eu)?
            .0
            .ok_or_else(|| null("ContainsUVPoint: edge use without UV trim curve"))?;
        // (status ignored upstream: a failed evaluation leaves sLastEndUV undefined)
        match last_trim.evaluate_point(k, last_end_param) {
            Ok(p) => p,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) if k.fixes.query_statuses_pass_on => {
                return Ok(UvContainment::of(PointContainment::Unknown));
            }
            Err(_) => Vec3::UNDEF,
        }
    };
    let mut last_end_quad_num = calc_quad_num(uv_pt, last_end_uv);

    // Now start the outer loop, on Edgeuses.
    for &this_eu in &eus {
        let this_edge = edge_of(b, this_eu)?;

        // low work: see if TgtPoint is on this EdgeCurve
        // (not upstream) only where the curve's box, grown by the edge's tolerance, reaches
        // the point: the drop finds the point only within that tolerance of the curve
        // ([`curve_box_reaches`]). Upstream copies and drops onto every edge of the loop, so
        // a face of n edges classified at n points made n^2 drops.
        let on_edge_curve = {
            let edge = b.edge(this_edge)?;
            let curve = edge
                .curve
                .as_ref()
                .ok_or_else(|| null("ContainsUVPoint: edge without curve"))?;
            if curve_box_reaches(k, curve, &edge.interval, point_3d, edge.tolerance) {
                k.work_counters.add_containment_edge_drop();
                // the drop builds the edge curve's cache at the edge's tolerance
                // (SmTol::GetApproxTol3d(&m_crCurve) reads pEdge->GetTolerance() through the
                // owner)
                let mut curve = curve.clone_for_evaluation()?;
                curve.set_owner_zone_tol_3d(Some(edge.tolerance));
                drop_on_edge(k, &curve, &edge.interval, point_3d, edge.tolerance)?
            } else {
                None
            }
        };
        if let Some(param) = on_edge_curve {
            // TgtPoint is on EdgeCurve - set output and return
            return Ok(UvContainment::on(this_edge, param));
        }

        if !b.ensure_trim_curve(k, this_eu)? {
            // we can't work
            return Ok(UvContainment::of(PointContainment::Unknown));
        }
        let trim = eu1::uv_trim_curve(b, this_eu)?
            .0
            .ok_or_else(|| null("ContainsUVPoint: edge use without UV trim curve"))?;

        // Grab some info about this EU and its uv curve (the edge as the creation left
        // it).
        let orient = eu1::orientation(b, this_eu)?;
        let domain = b.edge(this_edge)?.interval;
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
        let curve_is_linear = trim.is_linear(k, tol_uv, false)?.linear;

        // Eval info for start point of this EU.
        let mut pt_der = [Vec3::UNDEF; 2];
        let Some((mut prev_uv, mut prev_uv_tan, mut prev_uv_tan_len)) = or_unknown(
            k,
            eval_with_tangent(k, trim, prev_param, orient, &mut pt_der),
        )?
        else {
            return Ok(UvContainment::of(PointContainment::Unknown));
        };
        let (prev_vec, mut prev_dist) = unit_from(uv_pt, prev_uv);
        let mut prev_cos = prev_vec.dot(prev_uv_tan);

        // Note: if dPrevDist < tol, then this uv pt is on the edge.
        if prev_dist < tol_uv {
            // We should refine the point.
            let Ok(sol) = local_minimize(k, trim, &domain, uv_pt, tol_uv, prev_param)? else {
                return Ok(UvContainment::of(PointContainment::Unknown));
            };
            if let Some(sol) = sol {
                // since the guess was good enough - we expect the refined solution to
                // be as good or better
                if sol.start.solution_value <= tol_uv {
                    // return the refined answer.
                    return Ok(UvContainment::on(this_edge, sol.start.param(0)?));
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
                    UvContainment::on(last_end_edge, last_end_param)
                } else {
                    UvContainment::on(this_edge, prev_param)
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

                let Some((uv_at, this_uv_tan, this_uv_tan_len)) = or_unknown(
                    k,
                    eval_with_tangent(k, trim, this_param, orient, &mut pt_der),
                )?
                else {
                    return Ok(UvContainment::of(PointContainment::Unknown));
                };
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

                    let Ok(sol) = local_minimize(k, trim, &crv_ivl, uv_pt, tol_uv, guess_t)? else {
                        return Ok(UvContainment::of(PointContainment::Unknown));
                    };
                    if let Some(sol) = sol {
                        if sol.start.solution_value <= tol_uv {
                            return Ok(UvContainment::on(this_edge, sol.start.param(0)?));
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
        last_end_edge = this_edge;
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

/// Not upstream: the UV box that holds every point [`contains_uv_point`] evaluates on a
/// loop's trim curves, for a loop whose walk cannot fail or change the B-rep away from
/// the test point. For a test point strictly outside the box in `u` or `v` the walk's
/// answer is not [`PointContainment::Inside`] ([`WindingHull::excludes`]), so a caller
/// that only asks "inside?" need not walk; outside it by more than the walk's UV
/// tolerance, the answer is not [`PointContainment::OnBoundary`] by a trim either
/// ([`WalkExclusion`]).
///
/// Why: every point the walk evaluates (`sLastEndUV`, each step, each local solve's point)
/// is on a trim curve at a parameter of its edge's interval, widened by the local solves'
/// slack ([`with_solve_slack`]: a solve's interval is the edge's, or two steps' widened by
/// `SM_EFF_ZERO`, and its answer stays in it). A loop is given a hull only when each of its
/// trims is either
/// - a B-spline with positive weights whose evaluation cannot fail (as many control points
///   as its knots and degree call for, a degree the evaluators take, finite knots and no
///   positive knot gap short enough for their `1e-24` divisor guards, which only ever
///   divide by knot differences spanning the evaluated span), without out-of-bounds
///   evaluation (so a parameter past its domain is clamped into it) and with its edge's
///   interval, widened, in `[U[p], U[n]]` (where the convex hull property holds; an
///   unclamped curve extrapolates past it): its points lie in the box of its control
///   points; or
/// - a line, whose points over the widened interval lie between its points at the
///   widened interval's ends (a line extends past them);
///
/// so no evaluation reads an undefined point and every evaluated point lies in the box.
/// Every edge's interval is longer than `SM_EFF_ZERO`, so the walk steps along every edge
/// use and each chord it tests between edge uses joins two of those points (an edge use
/// it did not step along would hand the next chord the previous edge use's last point, or
/// the undefined point on the first).
/// With the test point outside that box in one coordinate, every evaluated point is in
/// one open half-plane through it, so the walk's quadrant numbers take two adjacent values
/// and its winding count ends at `0` or `±1`: [`PointContainment::Outside`]. Every earlier
/// return of the walk (on an edge, on a pole side, `Unknown`) is not `Inside` either.
///
/// A hull is also given only when the walk's point-independent steps cannot fail or edit
/// the B-rep: the loop, its loop use, face use and face exist; every edge use has an edge
/// with a curve that copies for evaluation, an orientation, and an existing UV trim whose
/// edge use records no drop failure (so `GetOrCreateUVTrimCurve` builds nothing and
/// succeeds), and every edge curve is a B-spline, a line or an ellipse (whose drops return
/// no [`SolidError::Unported`]: the walk gives the curve its edge's tolerance, so an
/// ellipse's NURB never asks its owner for one). The walk's point-dependent errors (the
/// drops, the trims' evaluations and local solves) are not returned, except the surface's
/// evaluation at the point, which is the caller's to have made first; a test point on a
/// side of the surface's natural domain (where the walk reads the loop's corners) is the
/// caller's to leave to the walk ([`WindingHull::excludes`] takes that side test).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WindingHull {
    lo: Point2,
    hi: Point2,
    /// The face the walk reads its zone tolerance from (the loop use's face use's).
    face: FaceId,
}

impl WindingHull {
    /// The hull of loop `l` of `b`, or `None` where the conditions above do not hold.
    pub(crate) fn of(k: &Kernel, b: &Brep, l: LoopId) -> Option<Self> {
        use crate::curve::curve::CurveKind;
        use crate::nurbs::math::NOW;
        let lu = b.loop_(l).ok()?.loopuse?;
        let fu = b.loopuse(lu).ok()?.faceuse?;
        let f = b.faceuse(fu).ok()?.face?;
        b.face(f).ok()?.surface.as_ref()?;
        let eus = eu1::loopuse_edgeuses(b, lu).ok()?;
        let mut lo = Point2::new(f64::INFINITY, f64::INFINITY);
        let mut hi = Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for &eu in &eus {
            let edge = b.edge(edge_of(b, eu).ok()?).ok()?;
            // the walk steps along an edge use only while its parameters are more than
            // SM_EFF_ZERO apart (its own test, computed as it computes it); one that does
            // not step carries the previous `this_uv` (undefined on the first) into the
            // next edge use's chord, a point outside the box
            if (edge.interval.max() - edge.interval.min()).abs() <= EFF_ZERO {
                return None;
            }
            let curve = edge.curve.as_ref()?;
            if !matches!(
                curve.kind(),
                CurveKind::BSpline(_) | CurveKind::Line(_) | CurveKind::Ellipse(_)
            ) {
                return None;
            }
            curve.clone_for_evaluation().ok()?;
            eu1::orientation(b, eu).ok()?;
            let failed = b
                .edgeuse(eu)
                .ok()?
                .drop_curve_fail
                .as_deref()
                .is_some_and(|f| f.fail_status != crate::error::SM_SUCCESS);
            if failed {
                return None;
            }
            let trim_curve = eu1::uv_trim_curve(b, eu).ok()?.0?;
            let trim = match trim_curve.kind() {
                CurveKind::BSpline(trim) => trim,
                CurveKind::Line(_) => {
                    // a line's points over the widened interval lie between its points
                    // at the widened interval's ends
                    let widened = with_solve_slack(&edge.interval);
                    for t in [widened.min(), widened.max()] {
                        let e = trim_curve.evaluate_point(k, t).ok()?;
                        if !(e.x.is_finite() && e.y.is_finite()) {
                            return None;
                        }
                        lo = Point2::new(lo.x.min(e.x), lo.y.min(e.y));
                        hi = Point2::new(hi.x.max(e.x), hi.y.max(e.y));
                    }
                    continue;
                }
                _ => return None,
            };
            let nurb = trim.nurb.as_ref()?;
            let p = nurb.degree();
            let cpts = nurb.cpts();
            let well_made = p >= 1
                && cpts.len() > p
                && nurb.knots().len() == cpts.len() + p + 1
                && nurb
                    .knots()
                    .windows(2)
                    .all(|w| matches!(w, [a, b] if a <= b));
            let domain = trim.natural_interval();
            let within = edge.interval.min() >= domain.min()
                && edge.interval.max() <= domain.max()
                && in_hull_domain(nurb, &edge.interval);
            if !(well_made && within && evaluates_everywhere(nurb.knots(), p))
                || trim.is_out_of_bounds_enabled()
            {
                return None;
            }
            for c in cpts {
                if !(c.w == NOW || (c.w > 0.0 && c.w.is_finite())) {
                    return None;
                }
                let e = c.to_euclid();
                if !(e.x.is_finite() && e.y.is_finite()) {
                    return None;
                }
                lo = Point2::new(lo.x.min(e.x), lo.y.min(e.y));
                hi = Point2::new(hi.x.max(e.x), hi.y.max(e.y));
            }
        }
        (lo.x <= hi.x && lo.y <= hi.y).then_some(Self { lo, hi, face: f })
    }

    /// Whether `uv` lies outside the hull in `u` or `v` by more than an evaluation's
    /// rounding (`1e-6` of the hull's size and of the coordinates), so that the walk of
    /// [`contains_uv_point`] from it cannot answer [`PointContainment::Inside`], given
    /// `on_domain_side`: whether `uv` lies on a side of the face's surface's natural
    /// domain at the walk's UV tolerance (`SmExtent2d::GetPointBoundaries`, the walk's
    /// first test; such a point is never excluded).
    pub(crate) fn excludes(&self, uv: Point2, on_domain_side: bool) -> bool {
        !on_domain_side && self.excludes_by(uv, 0.0)
    }

    /// Whether `uv` lies outside the hull in `u` or `v` by more than `margin` and an
    /// evaluation's rounding (`1e-6` of the hull's size and of the coordinates).
    fn excludes_by(&self, uv: Point2, margin: f64) -> bool {
        if !(uv.x.is_finite() && uv.y.is_finite() && margin.is_finite()) {
            return false;
        }
        let pad =
            |lo: f64, hi: f64, x: f64| 1e-6 * (1.0 + (hi - lo) + lo.abs().max(hi.abs()) + x.abs());
        let out = |lo: f64, hi: f64, x: f64| {
            let m = margin + pad(lo, hi, x);
            x < lo - m || x > hi + m
        };
        out(self.lo.x, self.hi.x, uv.x) || out(self.lo.y, self.hi.y, uv.y)
    }
}

/// Not upstream: the parameters a curve may be evaluated at by the local solves of
/// [`contains_uv_point`] and the drops it makes, for an interval `ivl` they are given: a
/// solve keeps its answer in its interval, which is the edge's or two steps' widened by
/// `SM_EFF_ZERO` (the walk's), and a drop's curve cache keeps a span whose interval lies
/// in `ivl` to `SM_EFF_ZERO` (`IsContainedBy`). So `ivl` widened by `SM_EFF_ZERO` and
/// the rounding of those sums and comparisons (a few units in the last place of the
/// interval's ends).
fn with_solve_slack(ivl: &Extent1d) -> Extent1d {
    let mut widened = *ivl;
    widened.expand_absolute(EFF_ZERO + 4.0 * f64::EPSILON * (1.0 + ivl.max_dimension()));
    widened
}

/// Not upstream: whether every parameter the walk or a drop may evaluate the NURB at for
/// interval `ivl` ([`with_solve_slack`], clamped into the knot range `[U[0], U[m]]` as
/// the evaluators clamp it) lies in `[U[p], U[n]]` (`n` its number of control points),
/// where the curve is a combination of its control points with nonnegative weights and
/// so lies in their box (the convex hull property). Past those knots an unclamped curve
/// extrapolates out of the box. A knot vector that decreases (a file can hold one) has
/// no such range: the property is stated for nondecreasing knots.
fn in_hull_domain(nurb: &crate::nurbs::crv_basic::Curve, ivl: &Extent1d) -> bool {
    let (p, knots) = (nurb.degree(), nurb.knots());
    let n = nurb.cpts().len();
    if !knots.windows(2).all(|w| matches!(w, [a, b] if a <= b)) {
        return false;
    }
    let (Some(&first), Some(&last), Some(&lo), Some(&hi)) =
        (knots.first(), knots.last(), knots.get(p), knots.get(n))
    else {
        return false;
    };
    let widened = with_solve_slack(ivl);
    widened.min().max(first) >= lo && widened.max().min(last) <= hi
}

/// Not upstream: whether a B-spline of degree `p` on `knots` (nondecreasing, as many as
/// its control points call for) evaluates at every finite parameter, points and
/// derivatives (`sm_CrvEval`, `sm_CrvDerivs`): a degree below the evaluators' scratch
/// bounds, finite knots, and every positive knot gap far above the `1e-24` (basis) and
/// `SM_EFF_ZERO_SQ` (derivative) divisor guards. Each divisor they test is a difference
/// of two knots on either side of the evaluated span (which `FindSpan` returns nonempty),
/// so it is at least that span's length less a few roundings of the knots' size.
fn evaluates_everywhere(knots: &[f64], p: usize) -> bool {
    let size = knots.iter().fold(0.0_f64, |m, u| m.max(u.abs()));
    let shortest = 1e-12 * (1.0 + size);
    p < crate::nurbs::knots::MAX_DEGREE
        && size.is_finite()
        && knots
            .windows(2)
            .all(|w| matches!(w, [a, b] if b == a || b - a >= shortest))
}

/// Not upstream: what a point classification of face `f` at `uv` needs to leave out an
/// inner loop's walk ([`contains_uv_point`]) that cannot answer
/// [`PointContainment::Inside`] or [`PointContainment::OnBoundary`]: the face's surface
/// point at `uv`, the walk's UV tolerance there, and that `uv` lies off the sides of the
/// surface's natural domain (where the walk reads the loop's corners and the surface's
/// poles). Each is computed as the walk computes it, on the same copy of the surface, so a
/// walk left out would have computed the same values without failing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WalkExclusion {
    face: FaceId,
    uv: Point2,
    point: Point3,
    /// The walk's UV tolerance (the face's zone tolerance mapped to UV at `uv`, at least
    /// `100 SM_EFF_ZERO`).
    tol_uv: f64,
}

impl WalkExclusion {
    /// The values at `uv` for loops of face `f` walked on `surface` (the copy the walk is
    /// given), or `None` where one of them fails or `uv` lies on a side of the natural
    /// domain: every loop is then walked.
    pub(crate) fn at(
        k: &Kernel,
        b: &Brep,
        f: FaceId,
        surface: &Surface,
        uv: Point2,
    ) -> Option<Self> {
        if !(uv.x.is_finite() && uv.y.is_finite()) {
            return None;
        }
        let zone_tol = b.face(f).ok()?.tolerance;
        let domain = surface.natural_uv_domain_with_nurb(k).ok()?;
        let point = surface.evaluate_point(k, uv).ok()?;
        let tol_uv = map_to_2d(k, zone_tol, uv, surface).ok()?;
        let finite = point.x.is_finite() && point.y.is_finite() && point.z.is_finite();
        if !(finite && tol_uv.is_finite()) || domain.point_boundaries(uv, tol_uv) != 0 {
            return None;
        }
        Some(Self {
            face: f,
            uv,
            point,
            tol_uv: tol_uv.max(EFF_ZERO * 100.0),
        })
    }

    /// Whether the walk of loop `l` of `b` (a loop of this face) from the point cannot
    /// answer [`PointContainment::Inside`] or [`PointContainment::OnBoundary`], cannot
    /// fail and cannot edit the B-rep, so a classification that only stops on those
    /// answers need not walk it.
    ///
    /// Why: the loop has a [`WindingHull`] on this face (no failure, no edit, not
    /// `Inside`; the surface steps are this value's), and the walk's other answers are
    /// not on its boundary:
    /// - On a pole side: `uv` is off every side of the natural domain.
    /// - On a trim (`dPrevDist < tol`, the chord between edge uses `<= tol`, a step's
    ///   point or a local solve's distance `<= tol`): each measures the distance from
    ///   `uv` to an evaluated trim point or a chord between two of them, all in the hull,
    ///   and `uv` lies outside it by more than the floored UV tolerance in one coordinate.
    /// - On an edge curve (the 3D drop): the drop succeeds only with a distance below the
    ///   edge's tolerance, measured from the surface point to the curve at a parameter of
    ///   the edge's interval (a line's drop clamps into it) or of that interval widened
    ///   by the solves' slack (an ellipse's, through its NURB, which clamps into its
    ///   domain), or to the undefined point a failed evaluation leaves. The surface point
    ///   lies farther than the tolerance from every edge curve's box over those
    ///   parameters ([`drop_cannot_reach`]) and from the undefined point. Edge curves of
    ///   other kinds (whose drops may answer from an analytic replacement) keep the walk.
    pub(crate) fn excludes(&self, k: &Kernel, b: &Brep, l: LoopId) -> bool {
        let Some(hull) = WindingHull::of(k, b, l) else {
            return false;
        };
        if hull.face != self.face || !hull.excludes_by(self.uv, self.tol_uv) {
            return false;
        }
        let Some(lu) = b.loop_(l).ok().and_then(|lp| lp.loopuse) else {
            return false;
        };
        let Ok(eus) = eu1::loopuse_edgeuses(b, lu) else {
            return false;
        };
        eus.iter().all(|&eu| {
            edge_of(b, eu)
                .and_then(|e| b.edge(e))
                .is_ok_and(|edge| drop_cannot_reach(edge, self.point))
        })
    }
}

/// Not upstream: whether the walk's drop of `point` on `edge`'s curve (`DropPoint`,
/// [`SolverOperation::Intersect`] at the edge's tolerance) cannot succeed, decided from
/// the curve's own data (not `CalculateBoundingBox`, whose Bézier refinement of a single
/// span assumes a clamped curve):
/// - a line: no box. Its drop (`DropPointFast`) measures the distance to its infinite
///   line, and keeps it when the foot's parameter, converted from STEP to NURB, lies in
///   the interval: a conversion that need not invert the evaluation's (found in review: a
///   line whose NURB domain is within `SM_EFF_ZERO` maps NURB parameters to themselves
///   but STEP parameters back with its scale, so a point far past the segment dropped at
///   distance 0). No geometric bound is taken (reviews found a direction shorter than
///   `SM_EFF_ZERO`, which `line_point_distance` takes for a point, and directions whose
///   norm underflows or overflows): the drop's distance is one of three values, its
///   `line_point_distance` or the distance to its point at either end of the interval,
///   and each is computed here with the same functions on the same inputs, bit for bit,
///   so the drop fails exactly when all three are at least the tolerance;
/// - an ellipse: it evaluates and solves through one NURB, which clamps a parameter into
///   its knot range; with positive weights and every parameter the drop can evaluate in
///   `[U[p], U[n]]` ([`in_hull_domain`]), its points lie in the box of all its control
///   points (the convex hull property), so `point` must lie farther than the tolerance
///   (and an evaluation's rounding) from that box, and from [`Vec3::UNDEF`] (where a
///   failed evaluation leaves it).
///
/// Other curves give `false`.
fn drop_cannot_reach(edge: &crate::topology::Edge, point: Point3) -> bool {
    use crate::curve::curve::CurveKind;
    use crate::nurbs::math::NOW;
    let Some(curve) = edge.curve.as_ref() else {
        return false;
    };
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut add = |p: Vec3| {
        lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    };
    match curve.kind() {
        CurveKind::Line(line) => {
            // the line's drop is `Line::drop_point_fast` alone (an intersect never
            // errors there, so the global solve returns its answer), and its distance is
            // one of three values, computed here as it computes them: its
            // `line_point_distance` from the line's point and vector when the foot's
            // parameter is not clamped, else the distance to its point at the interval's
            // start or end (`clamp_value` gives an end exactly). The drop succeeds only
            // with that distance below the edge's tolerance.
            let c = line.canonical_unit();
            let (unclamped, _) = line_point_distance(c.point, c.vector, point);
            let ends = [edge.interval.min(), edge.interval.max()];
            return [unclamped]
                .into_iter()
                .chain(ends.map(|t| line.evaluate_point(t).distance(point)))
                .all(|d| d >= edge.tolerance);
        }
        CurveKind::Ellipse(e) => {
            let base = e.base();
            let Some(nurb) = base.nurb.as_ref() else {
                return false;
            };
            let positive = nurb
                .cpts()
                .iter()
                .all(|c| c.w == NOW || (c.w > 0.0 && c.w.is_finite()));
            if !positive || base.is_out_of_bounds_enabled() || !in_hull_domain(nurb, &edge.interval)
            {
                return false;
            }
            for c in nurb.cpts() {
                add(c.to_euclid());
            }
        }
        _ => return false,
    }
    let coords = [
        lo.x, lo.y, lo.z, hi.x, hi.y, hi.z, point.x, point.y, point.z,
    ];
    if !(coords.iter().all(|c| c.is_finite()) && edge.tolerance.is_finite()) {
        return false;
    }
    let size = coords.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
    let reach = edge.tolerance + 1e-6 * (1.0 + (hi - lo).length() + size);
    let out = |lo: f64, hi: f64, x: f64| x < lo - reach || x > hi + reach;
    let off_box = out(lo.x, hi.x, point.x) || out(lo.y, hi.y, point.y) || out(lo.z, hi.z, point.z);
    off_box && point.distance(Vec3::UNDEF) > reach
}

/// Not upstream: whether some edge use of loop `l` has no UV trim curve (or the loop's
/// edge uses cannot be listed), so that a walk or sample of the loop may make one and
/// edit its edge; a loop whose edge uses all have their trims makes none
/// (`GetOrCreateUVTrimCurve` then builds nothing).
pub(crate) fn lacks_uv_trim(b: &Brep, l: LoopId) -> bool {
    let Some(lu) = b.loop_(l).ok().and_then(|lp| lp.loopuse) else {
        return true;
    };
    let Ok(eus) = eu1::loopuse_edgeuses(b, lu) else {
        return true;
    };
    eus.iter().any(|&eu| match eu1::uv_trim_curve(b, eu) {
        Ok((trim, _)) => trim.is_none(),
        Err(_) => true,
    })
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
    use crate::geom::extent1d::Extent1d;
    use crate::geom::extent2d::Extent2d;
    use crate::math::Vec2;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::surface::plane::Plane;
    use crate::topology::{
        Edge, Edgeuse, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId, Loop, Loopuse, LoopuseId,
        LoopuseStart, Region, RegionId, Shell, ShellId, Vertex, VertexId,
    };

    /// A degree-3 trim with only 2 control points: its basis fails to evaluate.
    fn bad_trim() -> Curve {
        let pw = vec![
            CPoint::new(1.0, 2.0, 0.0, NOW),
            CPoint::new(3.0, 4.0, 0.0, NOW),
        ];
        let cur = NurbCurve::new(pw, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0]).unwrap();
        Curve::from(BSplineCurve::from_nurb(cur, 2, false))
    }

    #[test]
    fn a_failed_trim_evaluation_is_returned() {
        // the failure is returned (SM_POC_UNKNOWN at the caller), where upstream keeps the
        // previous point and derivative, or the SM_UNDEF_DOUBLE defaults on a first
        // evaluation
        let k = Kernel::default();
        let trim = bad_trim();
        assert!(trim.evaluate(&k, 0.0, 1, false, true).is_err());
        let mut pt_der = [Vec3::UNDEF; 2];
        assert!(eval_with_tangent(&k, &trim, 0.0, OrientType::Same, &mut pt_der).is_err());
        let mut pt_der = [Vec3::new(1.0, 2.0, 0.0), Vec3::new(0.0, 2.0, 0.0)];
        assert!(eval_with_tangent(&k, &trim, 0.5, OrientType::Opposite, &mut pt_der).is_err());
    }

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

    /// A one-face B-rep on `surface` whose loop is the square `[-5, 5]^2`
    /// counter-clockwise (or clockwise) as 4 line edges, each edge use with its UV
    /// trim curve (except the ones in `without_trim`).
    fn square(surface: Surface, ccw: bool, without_trim: &[usize]) -> Brep {
        let mut v = [[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]];
        if !ccw {
            v.reverse();
        }
        let mut b = Brep::new();
        b.add_region_record(Region {
            in_brep: true,
            ..Region::default()
        })
        .unwrap();
        b.add_shell_record(Shell {
            faces: vec![FaceId::from_index(0)],
            region: Some(RegionId::from_index(0)),
            ..Shell::default()
        })
        .unwrap();
        b.add_face_record(Face {
            loops: vec![LoopId::from_index(0)],
            surface: Some(surface),
            faceuse: Some(FaceuseId::from_index(0)),
            tolerance: 1e-6,
            ..Face::new(ShellId::from_index(0))
        })
        .unwrap();
        b.add_loop_record(Loop {
            face: FaceId::from_index(0),
            edges: Vec::new(),
            loopuse: Some(LoopuseId::from_index(0)),
        })
        .unwrap();
        b.add_faceuse_record(Faceuse {
            shell: Some(ShellId::from_index(0)),
            face: Some(FaceId::from_index(0)),
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
            orientation: OrientType::Same,
        })
        .unwrap();
        for i in 0..4_u32 {
            let (p, q) = (v[i as usize], v[(i as usize + 1) % 4]);
            b.add_vertex_record(Vertex {
                point: Vec3::new(p[0], p[1], 0.0),
                tolerance: 1e-6,
                ..Vertex::default()
            })
            .unwrap();
            b.add_edge_record(Edge {
                vertices: [VertexId::from_index(i), VertexId::from_index((i + 1) % 4)],
                edgeuses: vec![EdgeuseId::from_index(i)],
                interval: Extent1d::from_min_max(0.0, 1.0),
                tolerance: 1e-6,
                curve: Some(Curve::from(Line::from_points(
                    Vec3::new(p[0], p[1], 0.0),
                    Vec3::new(q[0], q[1], 0.0),
                    3,
                ))),
            })
            .unwrap();
            b.add_edgeuse_record(Edgeuse {
                edge: Some(EdgeId::from_index(i)),
                owner: EdgeuseOwner::Loopuse(Some(LoopuseId::from_index(0))),
                ccw: Some(EdgeuseId::from_index((i + 1) % 4)),
                cw: Some(EdgeuseId::from_index((i + 3) % 4)),
                orientation: OrientType::Same,
                uv_trim_curve: (!without_trim.contains(&(i as usize))).then(|| trim(p, q)),
                ..Edgeuse::default()
            })
            .unwrap();
        }
        b
    }

    fn contains(b: &mut Brep, uv: [f64; 2]) -> Result<UvContainment> {
        contains_uv_point(
            &Kernel::default(),
            b,
            LoopId::from_index(0),
            None,
            Point2::new(uv[0], uv[1]),
            0.0,
        )
    }

    #[test]
    fn issue_2009_unevaluable_trim_is_unknown() {
        // tangency-labs/smlib 7a35f33 (SmLoop::ContainsUVPoint): a trim point that
        // cannot be evaluated is not classified (SM_POC_UNKNOWN); without the flag the
        // port ignores the failed evaluation as upstream does (its outputs keep their
        // values) and classifies on.
        let mut b = square(plane(), true, &[]);
        b.edgeuse_mut(EdgeuseId::from_index(3))
            .unwrap()
            .uv_trim_curve = Some(Curve::from(BSplineCurve::new(2)));
        assert!(contains(&mut b.clone_standing_for_self().unwrap(), [0.0, 0.0]).is_ok());
        let mut k = Kernel::default();
        k.fixes.query_statuses_pass_on = true;
        let r = contains_uv_point(
            &k,
            &mut b,
            LoopId::from_index(0),
            None,
            Point2::new(0.0, 0.0),
            0.0,
        )
        .unwrap();
        assert_eq!(r, UvContainment::of(PointContainment::Unknown));
    }

    #[test]
    fn square_inside_outside_on() {
        let mut b = square(plane(), true, &[]);
        let r = contains(&mut b, [0.0, 0.0]).unwrap();
        assert_eq!(r.containment, PointContainment::Inside);
        assert_eq!(r.orientation, OrientType::Same);
        let r = contains(&mut b, [8.0, 1.0]).unwrap();
        assert_eq!(r.containment, PointContainment::Outside);
        assert_eq!(r.on_edge, None);
        let r = contains(&mut b, [5.0, 0.0]).unwrap();
        assert_eq!(r.containment, PointContainment::OnBoundary);
        assert_eq!(r.on_edge, Some(EdgeId::from_index(1)));
        assert!((r.edge_param - 0.5).abs() < 1e-12);

        let mut b = square(plane(), false, &[]);
        let r = contains(&mut b, [1.0, -2.0]).unwrap();
        assert_eq!(r.containment, PointContainment::Inside);
        assert_eq!(r.orientation, OrientType::Opposite);
    }

    /// The square's winding hull is its box. Every point the hull excludes on a grid
    /// around it walks to an answer other than inside, and a point inside is not
    /// excluded; a point on a side of the surface's domain is never excluded.
    #[test]
    fn a_winding_hull_excludes_only_points_the_walk_finds_not_inside() {
        let b = square(plane(), true, &[]);
        let hull = WindingHull::of(&Kernel::default(), &b, LoopId::from_index(0)).unwrap();
        assert!(hull.excludes(Point2::new(8.0, 1.0), false));
        assert!(!hull.excludes(Point2::new(8.0, 1.0), true));
        assert!(!hull.excludes(Point2::new(0.0, 0.0), false));
        assert!(!hull.excludes(Point2::new(5.0, 0.0), false));
        let mut excluded = 0;
        for i in -17..=17 {
            for j in -17..=17 {
                let uv = [f64::from(i) * 0.7, f64::from(j) * 0.7];
                let r = contains(&mut b.clone_standing_for_self().unwrap(), uv).unwrap();
                if hull.excludes(Point2::new(uv[0], uv[1]), false) {
                    excluded += 1;
                    assert_ne!(r.containment, PointContainment::Inside, "{uv:?}");
                }
                if r.containment == PointContainment::Inside {
                    assert!(!hull.excludes(Point2::new(uv[0], uv[1]), false));
                }
            }
        }
        assert!(excluded > 500, "{excluded}");
    }

    /// The same with line trims (a line's points over its edge's interval lie between
    /// its ends): the hull is their ends' box.
    #[test]
    fn a_winding_hull_of_line_trims_excludes_only_not_inside_points() {
        let mut b = square(plane(), true, &[]);
        let v = [[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]];
        for i in 0..4_usize {
            let (p, q) = (v[i], v[(i + 1) % 4]);
            b.edgeuse_mut(EdgeuseId::from_index(i as u32))
                .unwrap()
                .uv_trim_curve = Some(Curve::from(Line::from_points(
                Vec3::new(p[0], p[1], 0.0),
                Vec3::new(q[0], q[1], 0.0),
                2,
            )));
        }
        let k = Kernel::default();
        let Some(hull) = WindingHull::of(&k, &b, LoopId::from_index(0)) else {
            return; // a line trim the walk cannot evaluate over [0, 1]: no hull, nothing to check
        };
        for i in -17..=17 {
            for j in -17..=17 {
                let uv = [f64::from(i) * 0.7, f64::from(j) * 0.7];
                if hull.excludes(Point2::new(uv[0], uv[1]), false) {
                    let r = contains(&mut b.clone_standing_for_self().unwrap(), uv).unwrap();
                    assert_ne!(r.containment, PointContainment::Inside, "{uv:?}");
                }
            }
        }
        assert!(hull.excludes(Point2::new(8.0, 1.0), false));
        assert!(!hull.excludes(Point2::new(0.0, 0.0), false));
    }

    /// No hull where the walk could fail, build a trim or read past what the box holds.
    #[test]
    fn no_winding_hull_where_the_walk_could_do_more() {
        let l = LoopId::from_index(0);
        // a missing trim (the walk would make it)
        assert_eq!(
            WindingHull::of(&Kernel::default(), &square(plane(), true, &[3]), l),
            None
        );
        // a trim whose basis does not evaluate
        let mut b = square(plane(), true, &[]);
        b.edgeuse_mut(EdgeuseId::from_index(3))
            .unwrap()
            .uv_trim_curve = Some(bad_trim());
        assert_eq!(WindingHull::of(&Kernel::default(), &b, l), None);
        // a face without surface, and a vertex loop
        let mut b = square(plane(), true, &[]);
        b.face_mut(FaceId::from_index(0)).unwrap().surface = None;
        assert_eq!(WindingHull::of(&Kernel::default(), &b, l), None);
        let mut b = square(plane(), true, &[]);
        b.loopuse_mut(LoopuseId::from_index(0)).unwrap().start = LoopuseStart::Vertexuse(None);
        assert_eq!(WindingHull::of(&Kernel::default(), &b, l), None);
        // an edge interval past its trim's domain
        let mut b = square(plane(), true, &[]);
        b.edge_mut(EdgeId::from_index(2)).unwrap().interval = Extent1d::from_min_max(0.0, 1.5);
        assert_eq!(WindingHull::of(&Kernel::default(), &b, l), None);
    }

    /// No hull for a trim evaluated past its domain (it may extrapolate out of its
    /// control points' box) or one with a positive knot gap short enough for the
    /// evaluators' divisor guards (its evaluation may fail and read an undefined point).
    #[test]
    fn no_winding_hull_where_a_trim_may_leave_its_box() {
        let l = LoopId::from_index(0);
        let k = Kernel::default();
        let mut b = square(plane(), true, &[]);
        let mut out_of_bounds = BSplineCurve::from_nurb(
            NurbCurve::new(
                vec![
                    CPoint::new(-5.0, 5.0, 0.0, NOW),
                    CPoint::new(-5.0, -5.0, 0.0, NOW),
                ],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap(),
            2,
            false,
        );
        out_of_bounds.set_out_of_bounds_enabled(true);
        b.edgeuse_mut(EdgeuseId::from_index(3))
            .unwrap()
            .uv_trim_curve = Some(Curve::from(out_of_bounds));
        assert_eq!(WindingHull::of(&k, &b, l), None);

        let mut b = square(plane(), true, &[]);
        let nurb = NurbCurve::new(
            vec![
                CPoint::new(-5.0, 5.0, 0.0, NOW),
                CPoint::new(-5.0, 0.0, 0.0, NOW),
                CPoint::new(-5.0, -5.0, 0.0, NOW),
            ],
            1,
            vec![0.0, 0.0, 0.5, 1.0, 1.0],
        )
        .unwrap();
        let mut short_gap = BSplineCurve::from_nurb(nurb, 2, false);
        // (set on the curve as built, whose construction may tidy so close a knot)
        let knots = short_gap.nurb.as_mut().unwrap().knots_mut();
        knots[2] = 1e-15;
        assert_eq!(*knots, [0.0, 0.0, 1e-15, 1.0, 1.0]);
        b.edgeuse_mut(EdgeuseId::from_index(3))
            .unwrap()
            .uv_trim_curve = Some(Curve::from(short_gap));
        assert_eq!(WindingHull::of(&k, &b, l), None);
    }

    /// A copy of loop `l`'s face's surface (as the face's point classification gives
    /// the walk) and the face.
    fn surface_of(b: &Brep, l: LoopId) -> (Surface, FaceId) {
        let f = b.loop_(l).unwrap().face;
        let surface = b.face(f).unwrap().surface.as_ref().unwrap();
        (surface.clone_for_evaluation().unwrap(), f)
    }

    /// The walk exclusion's values at `uv` for loop `l`'s face.
    fn exclusion_at(k: &Kernel, b: &Brep, l: LoopId, uv: [f64; 2]) -> Option<WalkExclusion> {
        let (surface, f) = surface_of(b, l);
        WalkExclusion::at(k, b, f, &surface, Point2::new(uv[0], uv[1]))
    }

    /// Whether loop `l`'s walk from `uv` is left out, checking that the walk it leaves
    /// out answers neither inside nor on the loop; returns that and the walk's answer.
    fn left_out_and_walk(b: &Brep, l: LoopId, uv: [f64; 2]) -> (bool, PointContainment) {
        let k = Kernel::default();
        let left_out = exclusion_at(&k, b, l, uv).is_some_and(|x| x.excludes(&k, b, l));
        let (surface, _) = surface_of(b, l);
        let mut walked = b.clone_standing_for_self().unwrap();
        let r = contains_uv_point(
            &k,
            &mut walked,
            l,
            Some(&surface),
            Point2::new(uv[0], uv[1]),
            0.0,
        )
        .unwrap();
        if left_out {
            assert!(
                !matches!(
                    r.containment,
                    PointContainment::Inside | PointContainment::OnBoundary
                ),
                "{uv:?}: {r:?}"
            );
        }
        (left_out, r.containment)
    }

    /// The square of line edges with B-spline trims, on a plane whose UV is its (x, y):
    /// every walk the exclusion leaves out on a grid around it, and at points across its
    /// right side by fractions and multiples of the walk's tolerance, answers neither
    /// inside nor on it. A point just outside the hull's padding (the walk's UV tolerance
    /// `1e-6` and the rounding pad `1e-6 (1 + 10 + 5 + 5)`) is left out; a point within
    /// the tolerance of the side is walked and is on it.
    #[test]
    fn a_walk_exclusion_leaves_out_only_walks_off_a_square() {
        let b = square(plane(), true, &[]);
        let l = LoopId::from_index(0);
        let mut uvs = Vec::new();
        for i in -17..=17 {
            for j in -17..=17 {
                uvs.push([f64::from(i) * 0.7, f64::from(j) * 0.7]);
            }
        }
        for d in [
            0.0, 1e-7, 5e-7, 1e-6, 2e-6, 1e-5, 2.1e-5, 2.3e-5, 1e-4, 1e-2,
        ] {
            uvs.extend([[5.0 + d, 1.3], [5.0 - d, 1.3], [-0.4, -5.0 - d]]);
        }
        let mut left_out = 0;
        for &uv in &uvs {
            left_out += usize::from(left_out_and_walk(&b, l, uv).0);
        }
        assert!(left_out > 500, "{left_out}");
        assert!(left_out_and_walk(&b, l, [5.0 + 3e-5, 1.3]).0);
        assert_eq!(
            left_out_and_walk(&b, l, [5.0 + 5e-7, 1.3]),
            (false, PointContainment::OnBoundary)
        );
        assert_eq!(
            left_out_and_walk(&b, l, [0.0, 0.0]),
            (false, PointContainment::Inside)
        );
    }

    /// Puts loop 0 of the square B-rep `b` (face 0) as the inner loop of the outer
    /// square `[-30, 10]^2` (loop 1), with vertex uses on every edge use.
    fn hold_in_outer_square(b: &mut Brep) {
        let l = LoopId::from_index(0);
        let o = [[-30.0, -30.0], [10.0, -30.0], [10.0, 10.0], [-30.0, 10.0]];
        b.add_loop_record(Loop {
            face: FaceId::from_index(0),
            edges: Vec::new(),
            loopuse: Some(LoopuseId::from_index(1)),
        })
        .unwrap();
        b.add_loopuse_record(Loopuse {
            faceuse: Some(FaceuseId::from_index(0)),
            loop_: Some(LoopId::from_index(1)),
            mate: None,
            start: LoopuseStart::Edgeuse(Some(EdgeuseId::from_index(4))),
            orientation: OrientType::Same,
        })
        .unwrap();
        for i in 0..4_u32 {
            let ix = 4 + i;
            let p = Vec3::new(o[i as usize][0], o[i as usize][1], 0.0);
            let q = Vec3::new(
                o[((i + 1) % 4) as usize][0],
                o[((i + 1) % 4) as usize][1],
                0.0,
            );
            b.add_vertex_record(Vertex {
                point: p,
                tolerance: 1e-6,
                ..Vertex::default()
            })
            .unwrap();
            b.add_edge_record(Edge {
                vertices: [
                    VertexId::from_index(ix),
                    VertexId::from_index(4 + (i + 1) % 4),
                ],
                edgeuses: vec![EdgeuseId::from_index(ix)],
                interval: Extent1d::from_min_max(0.0, 1.0),
                tolerance: 1e-6,
                curve: Some(Curve::from(Line::from_points(p, q, 3))),
            })
            .unwrap();
            b.add_edgeuse_record(Edgeuse {
                edge: Some(EdgeId::from_index(ix)),
                owner: EdgeuseOwner::Loopuse(Some(LoopuseId::from_index(1))),
                ccw: Some(EdgeuseId::from_index(4 + (i + 1) % 4)),
                cw: Some(EdgeuseId::from_index(4 + (i + 3) % 4)),
                orientation: OrientType::Same,
                uv_trim_curve: Some(Curve::from(Line::from_points(p, q, 2))),
                ..Edgeuse::default()
            })
            .unwrap();
        }
        let f = FaceId::from_index(0);
        b.face_mut(f).unwrap().loops = vec![LoopId::from_index(1), l];
        b.faceuse_mut(FaceuseId::from_index(0)).unwrap().loopuses =
            vec![LoopuseId::from_index(1), LoopuseId::from_index(0)];
        b.face_mut(f).unwrap().uv_domain = Extent2d::from_bounds(-30.0, -30.0, 10.0, 10.0);
        for i in 0..8_u32 {
            let vu = b
                .add_vertexuse_record(crate::topology::Vertexuse {
                    vertex: Some(VertexId::from_index(i)),
                    owner: crate::topology::VertexuseOwner::Edgeuse(Some(EdgeuseId::from_index(i))),
                    ..crate::topology::Vertexuse::default()
                })
                .unwrap();
            b.edgeuse_mut(EdgeuseId::from_index(i)).unwrap().vertexuse = Some(vu);
        }
    }

    /// Face 0's point classification at `uv` (`SmFace::PointClassify`, every loop) with
    /// the shortcuts on and off, as debug text.
    fn classified_on_and_off(b: &Brep, uv: [f64; 2]) -> (String, String) {
        let classify = |memo: bool| {
            let mut k = Kernel::new(&crate::kernel::KernelOptions::default());
            k.memoize_curve_caches = memo;
            let mut copy = b.clone_standing_for_self().unwrap();
            let mut pc =
                crate::intersect::point_class::PointClassification::<crate::EntityRef>::new(1e-6);
            let r = crate::query::face::p2::point_classify(
                &k,
                &mut copy,
                FaceId::from_index(0),
                Point2::new(uv[0], uv[1]),
                &mut pc,
                crate::query::face::p2::PointClassifyOptions::default(),
            );
            format!("{r:?} {pc:?}")
        };
        (classify(true), classify(false))
    }

    /// A loop with an edge whose interval is within `SM_EFF_ZERO` (the square
    /// `[-20, -10]^2` clockwise, its first 10-long side parameterised over `[0, 5e-13]`):
    /// the walk does not step along it and tests the next chord from the undefined
    /// point, finding `(-5, -5)`, far outside the square, on the first edge. Such a loop
    /// has no hull, so it is walked, and a face holding it as an inner loop of an outer
    /// square classifies the point the same with the shortcut on and off.
    #[test]
    fn a_loop_with_an_edge_the_walk_does_not_step_along_has_no_hull() {
        let mut b = square(plane(), true, &[]);
        let v = [
            [-20.0, -10.0],
            [-10.0, -10.0],
            [-10.0, -20.0],
            [-20.0, -20.0],
        ];
        for i in 0..4_usize {
            let p = Vec3::new(v[i][0], v[i][1], 0.0);
            let q = Vec3::new(v[(i + 1) % 4][0], v[(i + 1) % 4][1], 0.0);
            let (end, max) = if i == 0 {
                (p + (q - p) / 5e-13, 5e-13)
            } else {
                (q, 1.0)
            };
            b.vertex_mut(VertexId::from_index(i as u32)).unwrap().point = p;
            let e = b.edge_mut(EdgeId::from_index(i as u32)).unwrap();
            e.interval = Extent1d::from_min_max(0.0, max);
            e.curve = Some(Curve::from(Line::from_points(p, end, 3)));
            b.edgeuse_mut(EdgeuseId::from_index(i as u32))
                .unwrap()
                .uv_trim_curve = Some(Curve::from(Line::from_points(p, end, 2)));
        }
        let (k, l, uv) = (Kernel::default(), LoopId::from_index(0), [-5.0, -5.0]);
        assert_eq!(WindingHull::of(&k, &b, l), None);
        assert!(!exclusion_at(&k, &b, l, uv).is_some_and(|x| x.excludes(&k, &b, l)));
        let walk = contains(&mut b.clone_standing_for_self().unwrap(), uv).unwrap();
        assert_eq!(walk.containment, PointContainment::OnBoundary);
        hold_in_outer_square(&mut b);
        let (on, off) = classified_on_and_off(&b, uv);
        assert_eq!(on, off);
    }

    /// A loop whose trims are unclamped B-splines (degree 1, knots `[-1, 0.25, 0.75, 2]`,
    /// so the convex hull property holds only over `[0.25, 0.75]`) over edge intervals
    /// `[0, 1]`: they extrapolate past their control points to the diamond
    /// `(-20, -15) (-15, -10) (-10, -15) (-15, -20)`, the hole. `(-11, -15)` is inside it
    /// and outside the control points' box `[-18.75, -11.25]^2`. Such a loop has no hull,
    /// so it is walked (inside), and the face holding it as a hole of the outer square
    /// classifies the point the same (outside the face) with the shortcut on and off. The
    /// plane is turned an eighth of a turn so the 3D edges' boxes do not reach the point
    /// either: only the hull's domain decides.
    #[test]
    fn a_loop_of_unclamped_trims_past_their_hull_domain_has_no_hull() {
        let k = Kernel::default();
        let mut b = square(plane(), true, &[]);
        let v = [
            [-20.0, -15.0],
            [-15.0, -10.0],
            [-10.0, -15.0],
            [-15.0, -20.0],
        ];
        for i in 0..4_usize {
            let p = Vec3::new(v[i][0], v[i][1], 0.0);
            let q = Vec3::new(v[(i + 1) % 4][0], v[(i + 1) % 4][1], 0.0);
            b.vertex_mut(VertexId::from_index(i as u32)).unwrap().point = p;
            let e = b.edge_mut(EdgeId::from_index(i as u32)).unwrap();
            e.interval = Extent1d::from_min_max(0.0, 1.0);
            e.curve = Some(Curve::from(Line::from_points(p, q, 3)));
            let (a, c) = (p * 0.75 + q * 0.25, p * 0.25 + q * 0.75);
            let nurb = NurbCurve::new(
                vec![
                    CPoint::new(a.x, a.y, 0.0, NOW),
                    CPoint::new(c.x, c.y, 0.0, NOW),
                ],
                1,
                vec![-1.0, 0.25, 0.75, 2.0],
            )
            .unwrap();
            let trim = Curve::from(BSplineCurve::from_nurb(nurb, 2, false));
            // the trim is the edge's line in UV over [0, 1] (as the walk evaluates it)
            for j in 0..=20 {
                let t = f64::from(j) / 20.0;
                let at = trim.evaluate(&k, t, 1, false, true).unwrap()[0];
                assert!(at.distance(p + (q - p) * t) < 1e-12, "edge {i} at {t}");
            }
            b.edgeuse_mut(EdgeuseId::from_index(i as u32))
                .unwrap()
                .uv_trim_curve = Some(trim);
        }
        let (l, uv) = (LoopId::from_index(0), [-11.0, -15.0]);
        assert_eq!(WindingHull::of(&k, &b, l), None);
        let walk = contains(&mut b.clone_standing_for_self().unwrap(), uv).unwrap();
        assert_eq!(walk.containment, PointContainment::Inside);
        hold_in_outer_square(&mut b);
        // the turned plane, with every vertex and edge moved onto it
        let turned = Surface::from(Plane::new(
            Vec3::ZERO,
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-40.0, -40.0, 40.0, 40.0),
        ));
        let on_plane = |p: Vec3| turned.evaluate_point(&k, Point2::new(p.x, p.y)).unwrap();
        for i in 0..8_u32 {
            let vertex = b.vertex_mut(VertexId::from_index(i)).unwrap();
            vertex.point = on_plane(vertex.point);
            let edge = b.edge_mut(EdgeId::from_index(i)).unwrap();
            let line = edge.curve.as_ref().unwrap();
            let (p, q) = (
                on_plane(line.evaluate_point(&k, 0.0).unwrap()),
                on_plane(line.evaluate_point(&k, 1.0).unwrap()),
            );
            edge.curve = Some(Curve::from(Line::from_points(p, q, 3)));
        }
        b.face_mut(FaceId::from_index(0)).unwrap().surface = Some(turned);
        let (on, off) = classified_on_and_off(&b, uv);
        assert_eq!(on, off);
        assert!(off.contains("point_class: Unknown"), "{off}");
    }

    /// A loop with an unclamped circle edge, as a file can hold one (a quarter arc read
    /// back with its NURB replaced by a degree-2 rational one on knots
    /// `[-2, -1, 0, 1, 2, 3]`, the edge over `[0.1, 0.9]`, which reaches `x = 1`):
    /// `CalculateBoundingBox` refines its single span as if clamped and ends its box at
    /// `x = 0.7071`, so its box does not hold the curve. The drop's box is the circle's
    /// control points', which does: `(1.00001, 0)`, within the edge's tolerance `2e-5` of
    /// the circle, is walked and on it, and the face classifies it the same (on the edge)
    /// with the shortcut on and off.
    #[test]
    fn a_loop_with_an_unclamped_circle_edge_is_walked_near_it() {
        use crate::curve::ellipse::Ellipse;
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::topology::database_io::{DatabaseReader, DatabaseWriter, FileType, InputStream};
        let k = Kernel::default();
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let frame = Axis2Placement::new(Vec3::ZERO, Vec3::X, Vec3::Y);
        let arc = Ellipse::create_canonical_circle(
            &k,
            &frame,
            1.0,
            Some(&Extent1d::from_min_max(-45.0, 45.0)),
            None,
        )
        .unwrap();
        let nurb = NurbCurve::new(
            vec![
                CPoint::new(2.0 * s - 1.0, -2.0 * s, 0.0, 2.0 - s),
                CPoint::new(1.0, 0.0, 0.0, s),
                CPoint::new(2.0 * s - 1.0, 2.0 * s, 0.0, 2.0 - s),
            ],
            2,
            vec![-2.0, -1.0, 0.0, 1.0, 2.0, 3.0],
        )
        .unwrap();
        // the arc as written, its base NURB swapped for the unclamped one, read back
        let mut full = DatabaseWriter::open_for_write(FileType::Binary);
        arc.write_to_db(&mut full, 30).unwrap();
        let mut base = DatabaseWriter::open_for_write(FileType::Binary);
        arc.base().write_to_db(&mut base, 30).unwrap();
        let mut replacement = DatabaseWriter::open_for_write(FileType::Binary);
        BSplineCurve::from_nurb(nurb, 3, false)
            .write_to_db(&mut replacement, 30)
            .unwrap();
        let mut bytes = full.into_bytes();
        bytes.truncate(bytes.len() - base.bytes().len());
        bytes.extend_from_slice(replacement.bytes());
        let mut reader = DatabaseReader::with_stream(FileType::Binary, InputStream::new(bytes));
        let curve = Curve::from(Ellipse::read_from_db(&k, &mut reader, 3, 30, true).unwrap());
        let trim_nurb = NurbCurve::new(
            vec![
                CPoint::new(s, -s, 0.0, 1.0),
                CPoint::new((s + 1.0) / 2.0, -s / 2.0, 0.0, (1.0 + s) / 2.0),
                CPoint::new((s + 1.0) / 2.0, 0.0, 0.0, (1.0 + s) / 2.0),
                CPoint::new((s + 1.0) / 2.0, s / 2.0, 0.0, (1.0 + s) / 2.0),
                CPoint::new(s, s, 0.0, 1.0),
            ],
            2,
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let trim_curve = || Curve::from(BSplineCurve::from_nurb(trim_nurb.clone(), 2, false));
        let ivl = Extent1d::from_min_max(0.1, 0.9);
        // the circle and its trim agree over the interval and reach x = 1
        for j in 0..=20 {
            let t = 0.1 + 0.8 * f64::from(j) / 20.0;
            let p = curve.evaluate_point(&k, t).unwrap();
            assert!(p.distance(trim_curve().evaluate_point(&k, t).unwrap()) < 1e-12);
            assert!((p.x * p.x + p.y * p.y - 1.0).abs() < 1e-12);
        }
        assert!((curve.evaluate_point(&k, 0.5).unwrap().x - 1.0).abs() < 1e-12);
        // a quad closed by three lines past the arc's ends
        let p = curve.evaluate_point(&k, 0.1).unwrap();
        let q = curve.evaluate_point(&k, 0.9).unwrap();
        let v = [p, q, q * 0.67 + p * 0.33, q * 0.33 + p * 0.67];
        let mut b = square(plane(), true, &[]);
        let mut edge_curve = Some(curve);
        for i in 0..4 {
            let (p, q) = (v[i], v[(i + 1) % 4]);
            b.vertex_mut(VertexId::from_index(i as u32)).unwrap().point = p;
            let e = b.edge_mut(EdgeId::from_index(i as u32)).unwrap();
            e.tolerance = 2e-5;
            let trim = if i == 0 {
                e.interval = ivl;
                e.curve = edge_curve.take();
                trim_curve()
            } else {
                e.curve = Some(Curve::from(Line::from_points(p, q, 3)));
                Curve::from(Line::from_points(p, q, 2))
            };
            b.edgeuse_mut(EdgeuseId::from_index(i as u32))
                .unwrap()
                .uv_trim_curve = Some(trim);
        }
        let (l, uv) = (LoopId::from_index(0), [1.00001, 0.0]);
        assert!(!exclusion_at(&k, &b, l, uv).is_some_and(|x| x.excludes(&k, &b, l)));
        let walk = contains(&mut b.clone_standing_for_self().unwrap(), uv).unwrap();
        assert_eq!(walk.containment, PointContainment::OnBoundary);
        hold_in_outer_square(&mut b);
        let (on, off) = classified_on_and_off(&b, uv);
        assert_eq!(on, off);
        assert!(off.contains("point_class: Edge"), "{off}");
    }

    /// A line edge whose NURB domain is within `SM_EFF_ZERO` (`[0, 1e-13]`, analytic
    /// domain `[0, 1]`, scale 10, from `(-5, -5)` along X): it evaluates between
    /// `x = -5` and `5`, but its drop converts the infinite line's foot to a NURB
    /// parameter that need not invert the evaluation and accepts `(8, -5)`, past the
    /// segment on its line, at distance 0. The exclusion mirrors that drop, so the loop
    /// is walked for such a point (the walk makes the drop only where the edge's box
    /// reaches, `curve_box_reaches`, so it is the walk that answers), and the face
    /// classifies it the same with the shortcut on and off.
    #[test]
    fn a_point_on_a_line_edges_infinite_line_is_walked() {
        let k = Kernel::default();
        let mut b = square(plane(), true, &[]);
        let nurb = NurbCurve::new(
            vec![
                CPoint::new(-5.0, -5.0, 0.0, NOW),
                CPoint::new(5.0, -5.0, 0.0, NOW),
            ],
            1,
            vec![0.0, 0.0, 1e-13, 1e-13],
        )
        .unwrap();
        let line = Line::from_parts(
            BSplineCurve::from_nurb(nurb, 3, false),
            Vec3::new(-5.0, -5.0, 0.0),
            Vec3::X,
            Extent1d::from_min_max(0.0, 1.0),
            10.0,
            false,
        );
        let (uv, l) = ([8.0, -5.0], LoopId::from_index(0));
        let point = Vec3::new(uv[0], uv[1], 0.0);
        let e = b.edge_mut(EdgeId::from_index(0)).unwrap();
        e.curve = Some(Curve::from(line));
        let dropped = e
            .curve
            .as_ref()
            .unwrap()
            .drop_point(
                &k,
                &e.interval,
                point,
                None,
                e.tolerance,
                None,
                SolverOperation::Intersect,
            )
            .unwrap();
        assert!(
            dropped.success,
            "the drop accepts the point past the segment"
        );
        assert!(!drop_cannot_reach(e, point));
        assert!(!exclusion_at(&k, &b, l, uv).is_some_and(|x| x.excludes(&k, &b, l)));
        hold_in_outer_square(&mut b);
        let (on, off) = classified_on_and_off(&b, uv);
        assert_eq!(on, off);
    }

    /// A line edge whose direction is shorter than `SM_EFF_ZERO` (`(1e-13, 0, 0)`, scale
    /// `1.3e14`, analytic domain `[1, 2]`, from `(-5, -5)`): `line_point_distance` takes
    /// such a vector for a point and reports 13 from `(8, -5)`, but the drop clamps its
    /// foot and evaluates a point of the line at `(8, -5)`, distance 0. The point is on
    /// the line, so it is walked (on the edge), and the face classifies it the same with
    /// the shortcut on and off.
    #[test]
    fn a_line_edge_with_a_tiny_direction_bounds_by_its_line() {
        let k = Kernel::default();
        let mut b = square(plane(), true, &[]);
        let nurb = NurbCurve::new(
            vec![
                CPoint::new(-5.0, -5.0, 0.0, NOW),
                CPoint::new(5.0, -5.0, 0.0, NOW),
            ],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let line = Line::from_parts(
            BSplineCurve::from_nurb(nurb, 3, false),
            Vec3::new(-5.0, -5.0, 0.0),
            Vec3::new(1e-13, 0.0, 0.0),
            Extent1d::from_min_max(1.0, 2.0),
            1.3e14,
            false,
        );
        let (uv, l) = ([8.0, -5.0], LoopId::from_index(0));
        let point = Vec3::new(uv[0], uv[1], 0.0);
        let e = b.edge_mut(EdgeId::from_index(0)).unwrap();
        e.curve = Some(Curve::from(line));
        let dropped = e
            .curve
            .as_ref()
            .unwrap()
            .drop_point(
                &k,
                &e.interval,
                point,
                None,
                e.tolerance,
                None,
                SolverOperation::Intersect,
            )
            .unwrap();
        assert!(dropped.success, "the drop finds the point on the line");
        assert!(!drop_cannot_reach(e, point));
        assert!(!exclusion_at(&k, &b, l, uv).is_some_and(|x| x.excludes(&k, &b, l)));
        let walk = contains(&mut b.clone_standing_for_self().unwrap(), uv).unwrap();
        assert_eq!(walk.containment, PointContainment::OnBoundary);
        hold_in_outer_square(&mut b);
        let (on, off) = classified_on_and_off(&b, uv);
        assert_eq!(on, off);
    }

    /// Line edges whose directions underflow or overflow a norm (`(d, 0, 0)` with
    /// `d = 1e-13, 1e-160, 1e-161, 1e160`, scale `13 / d`, analytic domain `[1, 2]`,
    /// either way round, written to a file and read back): wherever the exclusion says
    /// the drop of `(8, -5)` cannot succeed, it does not, and the face classifies the
    /// point the same with the shortcut on and off.
    #[test]
    fn a_line_edges_exclusion_holds_for_extreme_directions() {
        use crate::topology::database_io::{DatabaseReader, DatabaseWriter, FileType, InputStream};
        let k = Kernel::default();
        let uv = [8.0, -5.0];
        let point = Vec3::new(uv[0], uv[1], 0.0);
        for d in [1e-13_f64, 1e-160, 1e-161, 1e160] {
            for inside_out in [false, true] {
                let mut b = square(plane(), true, &[]);
                let nurb = NurbCurve::new(
                    vec![
                        CPoint::new(-5.0, -5.0, 0.0, NOW),
                        CPoint::new(5.0, -5.0, 0.0, NOW),
                    ],
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                )
                .unwrap();
                let line = Line::from_parts(
                    BSplineCurve::from_nurb(nurb, 3, false),
                    Vec3::new(-5.0, -5.0, 0.0),
                    Vec3::new(d, 0.0, 0.0),
                    Extent1d::from_min_max(1.0, 2.0),
                    13.0 / d,
                    inside_out,
                );
                let mut writer = DatabaseWriter::open_for_write(FileType::Binary);
                line.write_to_db(&mut writer, 30).unwrap();
                let mut reader = DatabaseReader::with_stream(
                    FileType::Binary,
                    InputStream::new(writer.into_bytes()),
                );
                let line = Line::read_from_db(&mut reader, 3, 30).unwrap();
                let e = b.edge_mut(EdgeId::from_index(0)).unwrap();
                e.curve = Some(Curve::from(line));
                let dropped = e
                    .curve
                    .as_ref()
                    .unwrap()
                    .drop_point(
                        &k,
                        &e.interval,
                        point,
                        None,
                        e.tolerance,
                        None,
                        SolverOperation::Intersect,
                    )
                    .unwrap();
                let case = format!("d {d:e}, inside out {inside_out}");
                assert!(!(drop_cannot_reach(e, point) && dropped.success), "{case}");
                hold_in_outer_square(&mut b);
                let (on, off) = classified_on_and_off(&b, uv);
                assert_eq!(on, off, "{case}");
            }
        }
    }

    /// A point on a side of the surface's natural domain is always walked (the walk then
    /// reads the loop's corners and the surface's poles): the same point off the sides of
    /// a larger domain is left out.
    #[test]
    fn a_walk_exclusion_walks_a_point_on_a_side_of_the_domain() {
        let k = Kernel::default();
        let l = LoopId::from_index(0);
        let b = square(plane(), true, &[]);
        assert!(exclusion_at(&k, &b, l, [8.0, 1.0]).is_some_and(|x| x.excludes(&k, &b, l)));
        let on_side = Surface::from(Plane::new(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-8.0, -8.0, 8.0, 8.0),
        ));
        let b = square(on_side, true, &[]);
        assert_eq!(exclusion_at(&k, &b, l, [8.0, 1.0]), None);
    }

    /// Edge curves that are B-splines keep the walk (a B-spline's drop may answer from an
    /// analytic replacement the edge's box does not hold), though the hull rules the
    /// point out.
    #[test]
    fn a_walk_exclusion_walks_loops_of_b_spline_edges() {
        let k = Kernel::default();
        let l = LoopId::from_index(0);
        let mut b = square(plane(), true, &[]);
        let v = [[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]];
        for i in 0..4_usize {
            b.edge_mut(EdgeId::from_index(i as u32)).unwrap().curve =
                Some(trim(v[i], v[(i + 1) % 4]));
        }
        assert!(
            WindingHull::of(&k, &b, l).is_some_and(|h| h.excludes(Point2::new(8.0, 1.0), false))
        );
        for uv in [[8.0, 1.0], [0.0, 30.0], [-12.0, -12.0]] {
            assert!(!left_out_and_walk(&b, l, uv).0, "{uv:?}");
        }
    }

    /// A circle's loop (an ellipse edge, as a hole's loop is made): every walk left out
    /// on a grid over and around the face's domain answers neither inside nor on it, and
    /// most walks off the circle's box are left out.
    #[test]
    fn a_walk_exclusion_leaves_out_only_walks_off_a_circle() {
        use crate::curve::ellipse::Ellipse;
        use crate::geom::axis2_placement::Axis2Placement;
        let mut k = Kernel::default();
        let frame = Axis2Placement::new(Vec3::new(3.0, -2.0, 0.0), Vec3::X, Vec3::Y);
        let circle = Ellipse::create_canonical_circle(&k, &frame, 2.0, None, None).unwrap();
        let b = crate::primitive::api_trim_ops::create_planar_faces(&mut k, &[Curve::from(circle)])
            .unwrap();
        let f = b.face_list()[0];
        let l = b.face(f).unwrap().loops[0];
        let lu = b.loop_(l).unwrap().loopuse.unwrap();
        for eu in eu1::loopuse_edgeuses(&b, lu).unwrap() {
            let edge = b.edge(edge_of(&b, eu).unwrap()).unwrap();
            assert!(matches!(
                edge.curve.as_ref().unwrap().kind(),
                crate::curve::curve::CurveKind::Ellipse(_)
            ));
        }
        let domain = b.face(f).unwrap().uv_domain;
        let (lo, hi) = (domain.min(), domain.max());
        let mut left_out = 0;
        for i in -20..=60 {
            for j in -20..=60 {
                let uv = [
                    lo.x + (hi.x - lo.x) * f64::from(i) / 40.0,
                    lo.y + (hi.y - lo.y) * f64::from(j) / 40.0,
                ];
                left_out += usize::from(left_out_and_walk(&b, l, uv).0);
            }
        }
        assert!(left_out > 3000, "{left_out}");
    }

    /// A circle's loop whose ellipse edge has a NURB with decreasing knots, as a file
    /// can hold one: the convex hull property does not hold for it, so its control
    /// points' box certifies nothing, and a point far from the box, which is left out
    /// with the knots in order, is walked.
    #[test]
    fn a_loop_with_an_ellipse_edge_of_decreasing_knots_is_walked() {
        use crate::curve::ellipse::Ellipse;
        use crate::geom::axis2_placement::Axis2Placement;
        let mut k = Kernel::default();
        let frame = Axis2Placement::new(Vec3::new(3.0, -2.0, 0.0), Vec3::X, Vec3::Y);
        let circle = Ellipse::create_canonical_circle(&k, &frame, 2.0, None, None).unwrap();
        let mut b = crate::primitive::api_trim_ops::create_planar_faces(
            &mut k,
            &[Curve::from(circle.clone())],
        )
        .unwrap();
        let f = b.face_list()[0];
        let l = b.face(f).unwrap().loops[0];
        let lu = b.loop_(l).unwrap().loopuse.unwrap();
        let e = edge_of(&b, eu1::loopuse_edgeuses(&b, lu).unwrap()[0]).unwrap();
        let domain = b.face(f).unwrap().uv_domain;
        let (lo, hi) = (domain.min(), domain.max());
        let uv = [hi.x + 10.0 * (hi.x - lo.x), hi.y + 10.0 * (hi.y - lo.y)];
        let point = surface_of(&b, l)
            .0
            .evaluate_point(&k, Point2::new(uv[0], uv[1]))
            .unwrap();
        assert!(drop_cannot_reach(b.edge(e).unwrap(), point));
        assert!(exclusion_at(&k, &b, l, uv).is_some_and(|x| x.excludes(&k, &b, l)));

        let mut unordered = circle;
        let knots = unordered.base_mut().nurb.as_mut().unwrap().knots_mut();
        let i = (3..knots.len() - 4)
            .find(|&i| knots[i] < knots[i + 1])
            .unwrap();
        knots.swap(i, i + 1);
        b.edge_mut(e).unwrap().curve = Some(Curve::from(unordered));
        assert!(!drop_cannot_reach(b.edge(e).unwrap(), point));
        assert!(!exclusion_at(&k, &b, l, uv).is_some_and(|x| x.excludes(&k, &b, l)));
    }

    #[test]
    fn vertex_loop_face_and_missing_trim() {
        // a loop use without edge uses (a vertex loop) contains nothing
        let mut b = square(plane(), true, &[]);
        b.loopuse_mut(LoopuseId::from_index(0)).unwrap().start = LoopuseStart::Vertexuse(None);
        let r = contains(&mut b, [0.0, 0.0]).unwrap();
        assert_eq!(r, UvContainment::of(PointContainment::Outside));

        // a face without surface cannot classify
        let mut b = square(plane(), true, &[]);
        b.face_mut(FaceId::from_index(0)).unwrap().surface = None;
        let r = contains(&mut b, [0.0, 0.0]).unwrap();
        assert_eq!(r.containment, PointContainment::Unknown);

        // a missing trim curve is made (GetOrCreateUVTrimCurve): the lines drop onto the
        // plane or the creation defers on a callee that is not ported yet, but the
        // missing curve itself is not a stub
        let mut b = square(plane(), true, &[3]);
        let r = contains(&mut b, [0.0, 0.0]);
        assert!(!matches!(
            r,
            Err(SolidError::Unported {
                unit: "topology.edgeuse_p3",
                ..
            })
        ));
    }

    #[test]
    fn corner_boundaries_and_poles() {
        // a point on a domain side with the loop on that side reaches GetSingularities
        let dom = Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0);
        let s2 = Surface::from(Plane::new(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &dom,
        ));
        let mut b = square(s2, true, &[]);
        // a plane has no singular sides, so GetSingularities is wired and no longer defers
        let r = contains(&mut b, [-5.0, 0.0]);
        assert!(!matches!(r, Err(SolidError::Unported { .. })));
    }

    #[test]
    fn prepared_walk_keeps_lazy_trim_order() {
        let k = Kernel::default();
        // The final trim is read first. The first edge's 3D drop then exits before
        // needing its missing trim or the missing trims of later edge uses.
        let mut b = square(plane(), true, &[0, 1, 2]);
        let prepared = contains_uv_point_with_topology(
            &k,
            &mut QueryTopology::Prepared(&b),
            LoopId::from_index(0),
            None,
            Point2::new(0.0, -5.0),
            0.0,
        )
        .unwrap();
        assert_eq!(prepared.containment, PointContainment::OnBoundary);
        assert_eq!(prepared.on_edge, Some(EdgeId::from_index(0)));
        assert_eq!(prepared, contains(&mut b, [0.0, -5.0]).unwrap());
        for i in 0..3 {
            assert!(b
                .edgeuse(EdgeuseId::from_index(i))
                .unwrap()
                .uv_trim_curve
                .is_none());
        }

        // Conversely, a missing final trim is needed even for that same 3D drop.
        let b = square(plane(), true, &[3]);
        assert!(matches!(
            contains_uv_point_with_topology(
                &k,
                &mut QueryTopology::Prepared(&b),
                LoopId::from_index(0),
                None,
                Point2::new(0.0, -5.0),
                0.0,
            ),
            Err(SolidError::Unported {
                unit: "topology.edgeuse_p3",
                ..
            })
        ));
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
