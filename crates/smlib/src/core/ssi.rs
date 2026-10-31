//! Surface/surface intersection on solidcore (`smlib_ssi.cpp`'s `smlib_ssi`, `smlib_surface_domain`,
//! `smlib_surface_evaluate`), on the surfaces [`crate::core::split`] makes from a [`Spec`]:
//! `SmSurface::GlobalSurfaceIntersect` and `SmAdvSurfaceIntersector`, whose results are read as the
//! capi reads them.

use std::borrow::Cow;

use solidcore::curve::curve::Curve;
use solidcore::geom::vector2d::Point2;
use solidcore::intersect::surface_intersector::p5::do_intersection;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::surface::surface::p2::{SurfaceIntersection, TsectCurveType};

use super::convert::bspline_curve_part;
use super::run;
use super::split::{Tool, tool};
use crate::ffi::ssi::Nurbs;
use crate::ssi::{CurveType, Method, Spec, SsiCurve, SsiPoint, SsiResult, curve2, curve3};
use crate::{Error, Result};

/// The surface of `spec` with its natural UV domain (`construct`).
pub(crate) fn surface(spec: &Spec) -> Result<Tool> {
    run("smlib_surface", |k| tool(k, spec))?
}

/// `[u0, u1, v0, v1]`.
pub(crate) fn domain(s: &Tool) -> [f64; 4] {
    let (lo, hi) = (s.domain.min(), s.domain.max());
    [lo.x, hi.x, lo.y, hi.y]
}

/// The surface's point at (u, v): `smlib_surface_evaluate`, a non-finite point being `SM_ERR`.
pub(crate) fn evaluate(s: &Tool, u: f64, v: f64) -> Result<[f64; 3]> {
    if !u.is_finite() || !v.is_finite() {
        return Err(Error::Argument);
    }
    let p = run("smlib_surface_evaluate", |k| s.surface.evaluate_point(k, Point2::new(u, v)))?;
    let r = [p.x, p.y, p.z];
    if r.iter().all(|x| x.is_finite()) { Ok(r) } else { Err(Error::Solidcore("smlib_surface_evaluate: not a finite point".into())) }
}

fn curve_type(t: TsectCurveType) -> CurveType {
    match t {
        TsectCurveType::Unknown => CurveType::Unknown,
        TsectCurveType::Touching => CurveType::Touching,
        TsectCurveType::Crossing => CurveType::Crossing,
        TsectCurveType::Tangent => CurveType::Tangent,
        TsectCurveType::Coincident => CurveType::Coincident,
        TsectCurveType::NearTangent => CurveType::NearTangent,
        TsectCurveType::RegionBoundary => CurveType::RegionBoundary,
    }
}

/// The capi's `nurbs`: `c` as its B-spline, control points made cartesian in `dim` coordinates;
/// `None` for a curve that is not one, which is never approximated here.
fn nurbs(k: &Kernel, c: &Curve, dim: usize) -> Result<Option<Nurbs>> {
    let Some(part): Option<Cow<'_, BSplineCurve>> = bspline_curve_part(k, c)? else { return Ok(None) };
    let Some(n) = part.gw_nurb() else { return Ok(None) };
    let mut out = Nurbs { points: Vec::with_capacity(n.cpts().len() * dim), weights: Vec::with_capacity(n.cpts().len()), knots: n.knots().to_vec() };
    for p in n.cpts() {
        let w = if p.w != solidcore::nurbs::math::NOW { p.w } else { 1.0 };
        let q = [p.x / w, p.y / w, p.z / w];
        out.points.extend_from_slice(q.get(..dim).unwrap_or(&q));
        out.weights.push(w);
    }
    Ok(Some(out))
}

/// The capi's `start`: where `c` starts, NaN where there is no curve or it can't be evaluated.
fn start(k: &Kernel, c: Option<&Curve>) -> [f64; 3] {
    c.and_then(|c| c.evaluate_point(k, c.natural_interval().min()).ok()).map_or([f64::NAN; 3], |p| [p.x, p.y, p.z])
}

fn read(k: &Kernel, r: &SurfaceIntersection) -> Result<SsiResult> {
    let mut out = SsiResult::default();
    for (i, c3) in r.curves_3d.iter().enumerate() {
        let (ua, ub) = (r.surface1_uv_curves.get(i).and_then(Option::as_ref), r.surface2_uv_curves.get(i).and_then(Option::as_ref));
        let ty = r.curve_types.get(i).copied();
        let ivl = c3.natural_interval();
        let uv = |c: Option<&Curve>| -> Result<_> { Ok(c.map(|c| nurbs(k, c, 2)).transpose()?.flatten().map(curve2)) };
        out.curves.push(SsiCurve {
            curve_type: ty.map_or(CurveType::None, curve_type),
            deviation: r.deviations.get(i).copied().unwrap_or(f64::NAN),
            interval: [ivl.min(), ivl.max()],
            // A curve that is no B-spline is the one thing the result can't hold.
            curve: curve3(nurbs(k, c3, 3)?.ok_or(Error::Unsupported)?),
            uv_a: uv(ua)?,
            uv_b: uv(ub)?,
        });
        if ty == Some(TsectCurveType::Touching) {
            let (a, b) = (start(k, ua), start(k, ub));
            out.points.push(SsiPoint { xyz: start(k, Some(c3)), uv_a: [a[0], a[1]], uv_b: [b[0], b[1]] });
        }
    }
    Ok(out)
}

/// `smlib_ssi`: `a` and `b` within their domains, at approximation tolerance `approx_tol` (<= 0:
/// the kernel's default).
pub(crate) fn intersect(a: &Tool, b: &Tool, method: Method, approx_tol: f64) -> Result<SsiResult> {
    if !approx_tol.is_finite() {
        return Err(Error::Argument);
    }
    let tol = (approx_tol > 0.0).then_some(approx_tol);
    let edges = [true, true];
    run("smlib_ssi", |k| {
        let found = match method {
            Method::Global => a.surface.global_surface_intersect(k, &a.domain, &b.surface, &b.domain, edges, tol, None),
            Method::Advanced => do_intersection(k, &a.surface, &a.domain, &b.surface, &b.domain, edges, tol, None),
        };
        // An SMLib status is an answer, as the C API returns it.
        match found {
            Ok(found) => Ok(read(k, &found)),
            Err(e) => match e.sm_status() {
                Some(status) => Ok(Err(Error::Status(status))),
                None => Err(e),
            },
        }
    })?
}
