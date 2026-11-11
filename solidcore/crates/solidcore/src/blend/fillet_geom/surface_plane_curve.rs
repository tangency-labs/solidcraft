//! The curve where a surface meets a plane between two points known on both, traced on the
//! plane's equation.
//!
//! Upstream: the SolidCraft fork's `source/SMLib/src/SmFilletGeom.cpp`
//! (tangency-labs/smlib@5bdcddf, with @14a6daa's quarter points and start direction; WR-682).
//!
//! | Rust                  | Upstream               |
//! |-----------------------|------------------------|
//! | [`surface_plane_curve`] | `sm_SurfacePlaneCurve` |
//!
//! Deviations: out-params become return values; the context argument is dropped; `UVRate`'s
//! `(void)Eval` becomes `?`, where upstream would read an unset derivative array after a
//! failed evaluation. A blend by its definition is evaluated as its certified net where the
//! net's bound is within the tenth of `tol` the curve is held to (#2510). An end goes onto
//! the crossing by the least 3D step, not `OntoPlane`'s least uv step, and the curve ends
//! there, not at the given point (#2907): a point within `tol` of both surfaces is what the
//! vertices are, and the curve is held to the crossing itself. Not the fork's: a station off
//! the surface's domain in a direction the surface is not closed in is the curve leaving the
//! surface, and the trace refuses it (`SM_ERR`), where the fork marches on along the
//! B-spline's extrapolation (the goldens' generated `blend.fillet_geom_p2` case `gen0014`
//! reached u = -6e5 on a surface of domain [0, 1]). Bounds: the march
//! and the refinement together hold at most [`MAX_STATIONS`] stations; a curve whose spans
//! still miss the exact curve when refining would pass it can't be fitted (`SM_ERR`), as when
//! its 20 passes run out. Each pass splits every span that misses, so without the bound a
//! curve that never fits doubles its stations each pass (the goldens' generated
//! `blend.fillet_geom_p2` case `gen0014` ran out of memory).

#![allow(clippy::neg_cmp_op_on_partial_ord)] // a NaN must take the failing branch, as upstream

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::procedural_blend::NetScope;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;

const OP: &str = "blend::fillet_geom::surface_plane_curve::surface_plane_curve";

/// The most stations the trace holds, marched and refined.
const MAX_STATIONS: usize = 100_000;

/// The three curves of [`surface_plane_curve`].
#[derive(Debug)]
pub struct SurfacePlaneCurves {
    /// `rp3D`.
    pub curve_3d: Curve,
    /// `rpUVSurf`: its uv on the surface.
    pub uv_surface: Curve,
    /// `rpUVPlane`: its uv on the plane.
    pub uv_plane: Curve,
}

/// `smos_Max`.
fn smax(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `D`, `Du` and `Dv` of `surface` at `uv` (`Evaluate(uv, 1, 1, TRUE, TRUE, FALSE, .., FALSE)`).
fn first_derivs(k: &Kernel, surface: &Surface, uv: Point2) -> Result<(Vec3, Vec3, Vec3)> {
    let flags = EvaluateFlags {
        u_side: Side::Left,
        v_side: Side::Left,
        only_upper_half: false,
        non_zero_tangents: false,
    };
    let d = surface.evaluate(k, uv, 1, 1, flags)?;
    let at = |i: usize, j: usize| {
        d.get(i)
            .and_then(|row| row.get(j))
            .copied()
            .ok_or_else(|| SolidError::sm_err(OP))
    };
    Ok((at(0, 0)?, at(1, 0)?, at(0, 1)?))
}

/// What the routine solves on: the surface and the plane's point, axes and normal.
struct Tracer<'a> {
    k: &'a Kernel,
    surf: &'a Surface,
    origin: Point3,
    normal: Vec3,
    plane_u: Vec3,
    plane_v: Vec3,
    g11: f64,
    g12: f64,
    g22: f64,
    det_g: f64,
    scale: f64,
}

impl Tracer<'_> {
    /// `PlaneUV`: the plane's uv of a vector in it.
    fn plane_uv(&self, v: Vec3) -> Vec3 {
        let (a, b) = (v.dot(self.plane_u), v.dot(self.plane_v));
        Vec3::new(
            (self.g22 * a - self.g12 * b) / self.det_g,
            (self.g11 * b - self.g12 * a) / self.det_g,
            0.0,
        )
    }

    /// `Eval`: `D`, `Du`, `Dv` on the surface and the plane's equation there.
    fn eval(&self, uv: Point2) -> Result<(Vec3, Vec3, Vec3, f64)> {
        let (d, du, dv) = first_derivs(self.k, self.surf, uv)?;
        Ok((d, du, dv, (d - self.origin).dot(self.normal)))
    }

    /// `OntoPlane`: onto the plane's equation from a guess, by minimum-norm Newton steps.
    fn onto_plane(&self, uv: &mut Point2) -> Result<bool> {
        for _ in 0..40 {
            let (_, su, sv, g) = self.eval(*uv)?;
            let (gu, gv) = (su.dot(self.normal), sv.dot(self.normal));
            let gg = gu * gu + gv * gv;
            if !(gg > 0.0) {
                return Ok(false);
            }
            let (du, dv) = (-g * gu / gg, -g * gv / gg);
            *uv = Point2::new(uv.x + du, uv.y + dv);
            if g.abs() <= 1.0e-12 * self.scale
                && du.abs() + dv.abs() <= 1.0e-14 * (1.0 + uv.x.abs() + uv.y.abs())
            {
                return Ok(true);
            }
        }
        let (_, _, _, g) = self.eval(*uv)?;
        Ok(g.abs() <= 1.0e-10 * self.scale)
    }

    /// From `uv`, the crossing's point nearest in 3D: Newton steps of least 3D length, the
    /// surface's first fundamental form weighing the uv step. [`Tracer::onto_plane`]'s
    /// least uv step runs along the crossing wherever the parameters' speeds differ.
    fn onto_plane_nearest(&self, uv: &mut Point2) -> Result<bool> {
        for _ in 0..40 {
            let (_, su, sv, g) = self.eval(*uv)?;
            let (gu, gv) = (su.dot(self.normal), sv.dot(self.normal));
            let (e, f, h) = (su.dot(su), su.dot(sv), sv.dot(sv));
            let (mu, mv) = (h * gu - f * gv, e * gv - f * gu);
            let gg = gu * mu + gv * mv;
            if !(gg > 0.0) {
                return Ok(false);
            }
            let (du, dv) = (-g * mu / gg, -g * mv / gg);
            *uv = Point2::new(uv.x + du, uv.y + dv);
            if g.abs() <= 1.0e-12 * self.scale
                && du.abs() + dv.abs() <= 1.0e-14 * (1.0 + uv.x.abs() + uv.y.abs())
            {
                return Ok(true);
            }
        }
        let (_, _, _, g) = self.eval(*uv)?;
        Ok(g.abs() <= 1.0e-10 * self.scale)
    }

    /// `Tangent`: the unit 3D tangent at `uv` along the level line and its uv rate, oriented
    /// by `dir`.
    fn tangent(&self, uv: Point2, dir: Vec3) -> Result<(Vec3, Point2)> {
        let (_, su, sv, _) = self.eval(uv)?;
        let (gu, gv) = (su.dot(self.normal), sv.dot(self.normal));
        let t = su * gv - sv * gu;
        let len = t.length();
        if !(len > 0.0) {
            // the surface touches the plane
            return Err(SolidError::sm_err(OP));
        }
        let (mut a, mut b) = (gv / len, -gu / len);
        let mut t = t * (1.0 / len);
        if t.dot(dir) < 0.0 {
            t = -t;
            a = -a;
            b = -b;
        }
        Ok((t, Point2::new(a, b)))
    }

    /// `UVRate`: the uv rate of station `rate` per unit 3D length.
    fn uv_rate_per_length(&self, uv: Point2, rate: Point2) -> Result<Vec3> {
        let (_, su, sv, _) = self.eval(uv)?;
        let speed = (su * rate.x + sv * rate.y).length();
        Ok(Vec3::new(rate.x / speed, rate.y / speed, 0.0))
    }
}

/// One station of the trace.
#[derive(Clone, Copy)]
struct Station {
    uv: Point2,
    x: Point3,
    t: Vec3,
    rate: Point2,
}

/// A cubic B-spline with Bézier knots: `dim` 3 for the curve, 2 for its uv.
fn hermite(dim: u64, control: &[Point3], mult: &[usize], knots: &[f64]) -> Result<Curve> {
    Ok(Curve::from(BSplineCurve::create_canonical(
        &CanonicalData {
            dim,
            degree: 3,
            control_points: control,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: mult,
            knots,
            knot_type: KnotType::PiecewiseBezierKnots,
            weights: None,
            trim: None,
        },
    )?))
}

/// `sm_SurfacePlaneCurve`: the curve where `surf` meets `plane` between two points known on
/// both: the zero set of `g(u, v) = (S(u, v) - O) . N` on `surf`, traced from `start` to
/// `end`, its points solved on `surf` onto the plane's equation. A plane is unbounded: its
/// own domain does not limit the curve.
///
/// Each station is the surface's point nearest the predicted one on `g = 0`, the step along
/// the level line cut to keep the turn of its 3D tangent under `ang_tol`. The three curves are
/// cubic Hermite interpolants of the stations, a span split where a quarter point is off the
/// exact curve by more than a tenth of `tol`.
///
/// `start_dir`: the way the curve leaves `start`; `None`: towards `end`.
///
/// # Errors
///
/// `SM_ERR` for an end off the surface or the plane, a surface that touches the plane, a
/// curve that turns back before its end or leaves the surface's domain, a step that
/// vanishes, or curves that can't be fitted; those of the evaluations and the point drops.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one upstream routine
pub fn surface_plane_curve(
    k: &Kernel,
    surf: &Surface,
    plane: &Surface,
    start: Point3,
    end: Point3,
    start_dir: Option<Vec3>,
    tol: f64,
    ang_tol: f64,
) -> Result<SurfacePlaneCurves> {
    let fail = || SolidError::sm_err(OP);
    // the curve is held to a tenth of `tol` of the exact one: a blend by its definition
    // whose net's bound is within that evaluates as its net (#2510)
    let _scope = NetScope::within(k, 0.1 * tol);
    // the plane: O + u Pu + v Pv
    let (origin, plane_u, plane_v) = first_derivs(k, plane, Point2::new(0.0, 0.0))?;
    let normal = plane_u.cross(plane_v).unitize()?;
    let (g11, g12, g22) = (
        plane_u.dot(plane_u),
        plane_u.dot(plane_v),
        plane_v.dot(plane_v),
    );
    let det_g = g11 * g22 - g12 * g12;
    if !(det_g > 0.0) {
        return Err(fail());
    }
    let tr = Tracer {
        k,
        surf,
        origin,
        normal,
        plane_u,
        plane_v,
        g11,
        g12,
        g22,
        det_g,
        scale: smax(1.0, start.distance(end)),
    };

    // both ends on the surface
    let dom = surf.natural_uv_domain_with_nurb(k)?;
    let mut end_uv = [Point2::new(0.0, 0.0); 2];
    let mut end_x = [start, end];
    for ((uv, x), &p) in end_uv
        .iter_mut()
        .zip(end_x.iter_mut())
        .zip([start, end].iter())
    {
        let d = surf.drop_point::<NoObject>(k, p, &dom, &DropPointOptions::default())?;
        *uv = d.result_uv;
        let ok = if d.success {
            tr.onto_plane_nearest(uv)?
        } else {
            false
        };
        let on = surf.evaluate_point(k, *uv)?;
        if !ok || on.distance(p) > tol || (p - origin).dot(normal).abs() > tol {
            // an end is off the surface or the plane
            return Err(fail());
        }
        *x = on;
    }
    let [start_uv, stop_uv] = end_uv;
    let [start_x, stop_x] = end_x;
    // the curve stays on the surface: within its domain, but across a seam it is closed over
    let closed =
        |param| -> Result<bool> { Ok(surf.is_closed(k, &dom, param, Some(tol), false)?.0) };
    let (closed_u, closed_v) = (closed(SurfParamType::U)?, closed(SurfParamType::V)?);
    let on_domain = |uv: Point2| {
        let (lo, hi) = (dom.min(), dom.max());
        let pad = |a: f64, b: f64| 1.0e-12 * (1.0 + a.abs() + b.abs());
        let (pu, pv) = (pad(lo.x, hi.x), pad(lo.y, hi.y));
        (closed_u || (uv.x >= lo.x - pu && uv.x <= hi.x + pu))
            && (closed_v || (uv.y >= lo.y - pv && uv.y <= hi.y + pv))
    };

    // the stations: from the start, along the level line, to the end
    let (t0, r0) = tr.tangent(start_uv, start_dir.unwrap_or(end - start))?;
    let mut st = vec![Station {
        uv: start_uv,
        x: start_x,
        t: t0,
        rate: r0,
    }];
    let mut h = 0.0625 * start.distance(end);
    for _ in 0..MAX_STATIONS {
        let last = *st.last().ok_or_else(fail)?;
        if last.x.distance(end) <= 1.5 * h {
            // the last step: onto the end itself
            let (te, re) = tr.tangent(stop_uv, last.t)?;
            if te.dot(last.t) < 0.0 {
                // the curve turns back before its end
                return Err(fail());
            }
            st.push(Station {
                uv: stop_uv,
                x: stop_x,
                t: te,
                rate: re,
            });
            break;
        }
        // predict: a step of h along the tangent, in uv by its rate
        let (_, su, sv, _) = tr.eval(last.uv)?;
        let speed = (su * last.rate.x + sv * last.rate.y).length();
        if !(speed > 0.0) {
            return Err(fail());
        }
        let mut next = Point2::new(
            last.uv.x + h * last.rate.x / speed,
            last.uv.y + h * last.rate.y / speed,
        );
        let ok = tr.onto_plane(&mut next)?;
        let mut turned = None;
        if ok {
            let (tn, rn) = tr.tangent(next, last.t)?;
            turned = Some((tn, rn, tn.angle_between(last.t)?));
        }
        match turned {
            Some(_) if !on_domain(next) => {
                // the curve leaves the surface
                return Err(fail());
            }
            Some((tn, rn, turn)) if turn <= ang_tol => {
                let x = surf.evaluate_point(k, next)?;
                st.push(Station {
                    uv: next,
                    x,
                    t: tn,
                    rate: rn,
                });
                if turn < 0.25 * ang_tol {
                    h *= 1.5;
                }
            }
            _ => {
                h *= 0.5;
                if h < 1.0e-9 * tr.scale {
                    // the trace's step vanished
                    return Err(fail());
                }
            }
        }
    }

    // the Hermite spans, by arc length estimated by the chords, split until their quarter
    // points are on the exact curve
    for _ in 0..20 {
        let mut s = Vec::with_capacity(st.len());
        let mut along = 0.0;
        s.push(along);
        for pair in st.windows(2) {
            if let [a, b] = pair {
                along += b.x.distance(a.x);
                s.push(along);
            }
        }
        let last_index = st.len().saturating_sub(1);
        let mult: Vec<usize> = (0..st.len())
            .map(|j| if j == 0 || j == last_index { 4 } else { 3 })
            .collect();
        let (mut cp3, mut cp_uv, mut cp_pl) = (Vec::new(), Vec::new(), Vec::new());
        for (j, pair) in st.windows(2).enumerate() {
            let ([a, b], (Some(&sa), Some(&sb))) = (pair, (s.get(j), s.get(j + 1))) else {
                return Err(fail());
            };
            let third = (sb - sa) / 3.0;
            let (ra, rb) = (
                tr.uv_rate_per_length(a.uv, a.rate)?,
                tr.uv_rate_per_length(b.uv, b.rate)?,
            );
            let (ua, ub) = (
                Vec3::new(a.uv.x, a.uv.y, 0.0),
                Vec3::new(b.uv.x, b.uv.y, 0.0),
            );
            let (pa, pb) = (tr.plane_uv(a.x - origin), tr.plane_uv(b.x - origin));
            if j == 0 {
                cp3.push(a.x);
                cp_uv.push(ua);
                cp_pl.push(pa);
            }
            cp3.extend([a.x + a.t * third, b.x - b.t * third, b.x]);
            cp_uv.extend([ua + ra * third, ub - rb * third, ub]);
            cp_pl.extend([
                pa + tr.plane_uv(a.t) * third,
                pb - tr.plane_uv(b.t) * third,
                pb,
            ]);
        }
        let c3d = hermite(3, &cp3, &mult, &s)?;
        let uv_surface = hermite(2, &cp_uv, &mult, &s)?;
        let uv_plane = hermite(2, &cp_pl, &mult, &s)?;

        // each span at its quarters against the exact curve there, to a tenth of the tolerance;
        // a span off is split at its middle
        let mut off: Vec<(usize, Point2)> = Vec::new();
        for j in 0..st.len().saturating_sub(1) {
            let (Some(&sa), Some(&sb)) = (s.get(j), s.get(j + 1)) else {
                return Err(fail());
            };
            let mut span_ok = true;
            let mut mid_uv = Point2::new(0.0, 0.0);
            for q in 1..=3 {
                let at = sa + 0.25 * f64::from(q) * (sb - sa);
                let m = c3d.evaluate_point(k, at)?;
                let w = uv_surface.evaluate_point(k, at)?;
                let mut on_uv = Point2::new(w.x, w.y);
                let ok = tr.onto_plane(&mut on_uv)?;
                let exact = surf.evaluate_point(k, on_uv)?;
                let on_surface = surf.evaluate_point(k, Point2::new(w.x, w.y))?;
                if q == 2 {
                    mid_uv = on_uv;
                }
                if !ok || m.distance(exact) > 0.1 * tol || on_surface.distance(exact) > 0.1 * tol {
                    span_ok = false;
                }
            }
            if !span_ok {
                off.push((j, mid_uv));
            }
        }
        if off.is_empty() {
            return Ok(SurfacePlaneCurves {
                curve_3d: c3d,
                uv_surface,
                uv_plane,
            });
        }
        // insert those middles as stations
        if st.len() + off.len() > MAX_STATIONS {
            // the curves could not be fitted within the tolerance
            return Err(fail());
        }
        let mut refined = Vec::with_capacity(st.len() + off.len());
        let mut pending = off.iter().peekable();
        for (j, station) in st.iter().enumerate() {
            refined.push(*station);
            if let Some(&&(_, uv)) = pending.peek().filter(|&&&(at, _)| at == j) {
                let x = surf.evaluate_point(k, uv)?;
                let (t, rate) = tr.tangent(uv, station.t)?;
                refined.push(Station { uv, x, t, rate });
                pending.next();
            }
        }
        st = refined;
    }
    // the curves could not be fitted within the tolerance
    Err(fail())
}

/// Whether the two end points lie on both surfaces, within `tol`, and the surfaces cross at
/// both: their normals aren't parallel there (the condition of the call in
/// `sm_SrfSrfIntersection`).
///
/// # Errors
///
/// Those of the point drops and evaluations.
pub fn ends_on_both(
    k: &Kernel,
    surfaces: [&Surface; 2],
    ends: [Point3; 2],
    tol: f64,
) -> Result<bool> {
    let mut normals = [[Vec3::new(0.0, 0.0, 0.0); 2]; 2];
    for (surface, row) in surfaces.iter().zip(normals.iter_mut()) {
        let dom = surface.natural_uv_domain_with_nurb(k)?;
        for (&p, normal) in ends.iter().zip(row.iter_mut()) {
            let d = surface.drop_point::<NoObject>(k, p, &dom, &DropPointOptions::default())?;
            if !d.success {
                return Ok(false);
            }
            let on = surface.evaluate_point(k, d.result_uv)?;
            *normal = surface.evaluate_normal(k, d.result_uv, Side::Left, Side::Left)?;
            if on.distance(p) > tol {
                return Ok(false);
            }
        }
    }
    let [[a0, a1], [b0, b1]] = normals;
    for (a, b) in [(a0, b0), (a1, b1)] {
        if a.cross(b).length() <= crate::math::EFF_ZERO_SQRT * a.length() * b.length() {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::extent2d::Extent2d;
    use crate::geom::vector2d::Vec2;
    use crate::kernel::KernelOptions;
    use crate::surface::plane::Plane;

    /// A plane `z = 0` whose `u` runs 100 times faster than its `v`, cut by a plane at 45
    /// degrees along a line nearly along `u`, from a start 2e-6 off the crossing. The crossing's
    /// nearest point is about that far away. The least uv step, which is nearly equal in `u`
    /// and `v` here, lands about 1e-4 along the line instead, past the tolerance (#2907).
    #[test]
    fn an_end_off_the_crossing_goes_to_its_nearest_point_with_uneven_parameter_speeds() {
        let k = Kernel::new(&KernelOptions::default());
        let domain = Extent2d::from_bounds(-1.0, -50.0, 1.0, 50.0);
        let surf = Surface::from(Plane::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec2::new(100.0, 1.0),
            &domain,
        ));
        // the cutter: y + z + 0.01 x = 0
        let cut = |x: f64, z: f64| Point3::new(x, -z - 0.01 * x, z);
        let along = Vec3::new(1.0, -0.01, 0.0).unitize().unwrap();
        let cutter = Surface::from(Plane::new(
            Point3::new(0.0, 0.0, 0.0),
            along,
            along.cross(Vec3::new(0.01, 1.0, 1.0).unitize().unwrap()),
            Vec2::new(1.0, 1.0),
            &domain,
        ));
        let (start, end) = (cut(10.0, 2.0e-6), cut(20.0, 0.0));
        let c = surface_plane_curve(&k, &surf, &cutter, start, end, None, 4.0e-5, 0.1).unwrap();
        let first = c.curve_3d.evaluate_point(&k, 0.0).unwrap();
        // on the crossing, y = -0.01 x, z = 0, and as near the start as the geometry allows
        assert!(first.z.abs() <= 1.0e-11, "{first:?}");
        assert!((first.y + 0.01 * first.x).abs() <= 1.0e-11, "{first:?}");
        assert!(first.distance(start) <= 3.0e-6, "{}", first.distance(start));
    }
}
