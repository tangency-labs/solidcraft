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
//! surface, and the trace reports leaving its domain (`SM_ERR`), where the fork marches on
//! along the B-spline's extrapolation (the goldens' generated `blend.fillet_geom_p2` case
//! `gen0014` reached u = -6e5 on a surface of domain [0, 1]). Bounds: the march
//! and the refinement together hold at most [`MAX_STATIONS`] stations; a curve whose spans
//! still miss the exact curve when refining would pass it can't be fitted (`SM_ERR`), as when
//! its 20 passes run out. Each pass splits every span that misses, so without the bound a
//! curve that never fits doubles its stations each pass (the goldens' generated
//! `blend.fillet_geom_p2` case `gen0014` ran out of memory). Not the fork's: a span off the
//! exact curve is split at a point of the crossing in the middle half of its chord, not at the
//! uv cubic's middle put back on the plane. Where the surface's speed varies steeply along the
//! crossing (a chamfer's ruled surface between rails whose weights differ 58-fold, #19967), a
//! station's uv rate is large, the uv cubic leaves the span, and its middle put back on the
//! plane lies outside the span: every pass split the same spans again until the passes ran
//! out. A span with no such point (none in the middle half of its chord, or one nearer the other
//! station than its own) is left as it is, and the fit ends at its cap as before if it still
//! misses.
//!
//! Failures (#15034): three reviewed defects report a successful evaluation missing its
//! promised first derivatives, a non-positive plane Gram determinant after its axes' cross
//! product was unitized, or no speed on re-evaluating an accepted unit-speed station. The
//! named causes are [`BlendFailure::PlaneCurveTouches`], [`BlendFailure::PlaneCurveEndOff`],
//! [`BlendFailure::PlaneCurveLeavesDomain`], [`BlendFailure::PlaneCurveStepVanished`] (a
//! stopped march, a defect), and [`BlendFailure::PlaneCurveFitCap`]. All keep `SM_ERR`.
//! Four unreachable checks are deleted: no last station (the trace starts with one and only
//! grows), a reversed end tangent (the tangent is oriented to the previous one), and two
//! missing span arc lengths (one arc-length entry is built for every station).

#![allow(clippy::neg_cmp_op_on_partial_ord)] // a NaN must take the failing branch, as upstream

use crate::curve::curve::Curve;
use crate::error::blend::BlendFailure;
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
        d.get(i).and_then(|row| row.get(j)).copied().ok_or_else(|| {
            SolidError::blend(OP, BlendFailure::Defect {
                why: "the surface's evaluation succeeded but returned no first derivative at the requested order, which the evaluator promises for the orders asked",
            })
        })
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
            return Err(SolidError::blend(
                OP,
                BlendFailure::PlaneCurveTouches { u: uv.x, v: uv.y },
            ));
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

    /// A point of the crossing between two stations, to split their span at: in the middle
    /// half of their chord (its side of the plane through the chord's middle, normal to it, at
    /// most a quarter of the chord's length from that plane), so each new span keeps at least a
    /// quarter of the old one, and nearer each station than the other is (a point of the same
    /// stretch of the crossing, not of another branch passing by). Found by bisection on `l`
    /// in [0, 1] over the crossing's points [`Tracer::onto_plane`] reaches from
    /// `(1 - l) a + l b` in uv, by that side (behind the middle at `a`, past it at `b`).
    /// `None` where a candidate is not on the plane, its side is not a number, 52 halvings
    /// (the last bit of `l`) find none in the middle half, or the one found is not nearer
    /// each station than the other.
    fn between(&self, a: &Station, b: &Station) -> Result<Option<Point2>> {
        let chord = b.x - a.x;
        let mid = a.x + chord * 0.5;
        let length = chord.length();
        if !(length > 0.0) {
            return Ok(None);
        }
        let (mut behind, mut past) = (0.0_f64, 1.0_f64);
        for _ in 0..52 {
            let l = 0.5 * (behind + past);
            let mut uv = Point2::new(
                (1.0 - l) * a.uv.x + l * b.uv.x,
                (1.0 - l) * a.uv.y + l * b.uv.y,
            );
            if !self.onto_plane(&mut uv)? {
                return Ok(None);
            }
            let x = self.eval(uv)?.0;
            // the signed distance from the middle plane, along the chord
            let side = (x - mid).dot(chord) / length;
            if !side.is_finite() {
                return Ok(None);
            }
            if side.abs() <= 0.25 * length {
                let near = x.distance(a.x) < length && x.distance(b.x) < length;
                return Ok(near.then_some(uv));
            }
            if side < 0.0 {
                behind = l;
            } else {
                past = l;
            }
        }
        Ok(None)
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
/// exact curve by more than a tenth of `tol`, at a point of the crossing in the middle half of
/// the span's chord.
///
/// `start_dir`: the way the curve leaves `start`; `None`: towards `end`.
///
/// # Errors
///
/// Typed blend causes with status `SM_ERR` for an end off the crossing, a surface that
/// touches the plane, a trace leaving the surface's domain or whose step vanishes, and a
/// fit past its work cap; reviewed defects with the same status for broken evaluation or
/// tracing invariants; those of the evaluations and the point drops.
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
        return Err(SolidError::blend(OP, BlendFailure::Defect {
            why: "the cutting plane's Gram determinant is not positive though its axes' cross product was just unitized, so the axes are independent and only the determinant's arithmetic can have failed",
        }));
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
    for (((uv, x), &p), at_end) in end_uv
        .iter_mut()
        .zip(end_x.iter_mut())
        .zip([start, end].iter())
        .zip([false, true])
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
            return Err(SolidError::blend(
                OP,
                BlendFailure::PlaneCurveEndOff {
                    at_end,
                    distance: if ok {
                        Some(smax(on.distance(p), (p - origin).dot(normal).abs()))
                    } else {
                        None
                    },
                },
            ));
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
    let mut last = Station {
        uv: start_uv,
        x: start_x,
        t: t0,
        rate: r0,
    };
    let mut st = vec![last];
    let mut h = 0.0625 * start.distance(end);
    for _ in 0..MAX_STATIONS {
        if last.x.distance(end) <= 1.5 * h {
            // the last step: onto the end itself; tangent already orients te to last.t,
            // so its dot with last.t cannot be negative (nor can a NaN compare less than 0)
            let (te, re) = tr.tangent(stop_uv, last.t)?;
            last = Station {
                uv: stop_uv,
                x: stop_x,
                t: te,
                rate: re,
            };
            st.push(last);
            break;
        }
        // predict: a step of h along the tangent, in uv by its rate
        let (_, su, sv, _) = tr.eval(last.uv)?;
        let speed = (su * last.rate.x + sv * last.rate.y).length();
        if !(speed > 0.0) {
            return Err(SolidError::blend(OP, BlendFailure::Defect {
                why: "re-evaluating an accepted station found no speed along the uv rate the tracer built there to have unit 3D speed on the same surface",
            }));
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
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::PlaneCurveLeavesDomain {
                        u: next.x,
                        v: next.y,
                    },
                ));
            }
            Some((tn, rn, turn)) if turn <= ang_tol => {
                let x = surf.evaluate_point(k, next)?;
                last = Station {
                    uv: next,
                    x,
                    t: tn,
                    rate: rn,
                };
                st.push(last);
                if turn < 0.25 * ang_tol {
                    h *= 1.5;
                }
            }
            _ => {
                h *= 0.5;
                if h < 1.0e-9 * tr.scale {
                    return Err(SolidError::blend(
                        OP,
                        BlendFailure::PlaneCurveStepVanished {
                            u: last.uv.x,
                            v: last.uv.y,
                            step: h,
                            minimum: 1.0e-9 * tr.scale,
                        },
                    ));
                }
            }
        }
    }

    // the Hermite spans, by arc length estimated by the chords, split until their quarter
    // points are on the exact curve
    for pass in 0..20 {
        // one arc-length entry per station: neither span loop below can lack its pair
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
        for (j, ((a, b), (&sa, &sb))) in st
            .iter()
            .zip(st.iter().skip(1))
            .zip(s.iter().zip(s.iter().skip(1)))
            .enumerate()
        {
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
        // a span off is split at the crossing's point between its stations
        let mut off: Vec<(usize, Point2)> = Vec::new();
        // a span that misses with no point to split at stays as it is: the passes go on
        let mut missed = false;
        for (j, ((&sa, &sb), (a, b))) in s
            .iter()
            .zip(s.iter().skip(1))
            .zip(st.iter().zip(st.iter().skip(1)))
            .enumerate()
        {
            let mut span_ok = true;
            for q in 1..=3 {
                let at = sa + 0.25 * f64::from(q) * (sb - sa);
                let m = c3d.evaluate_point(k, at)?;
                let w = uv_surface.evaluate_point(k, at)?;
                let mut on_uv = Point2::new(w.x, w.y);
                let ok = tr.onto_plane(&mut on_uv)?;
                let exact = surf.evaluate_point(k, on_uv)?;
                let on_surface = surf.evaluate_point(k, Point2::new(w.x, w.y))?;
                if !ok || m.distance(exact) > 0.1 * tol || on_surface.distance(exact) > 0.1 * tol {
                    span_ok = false;
                }
            }
            if !span_ok {
                match tr.between(a, b)? {
                    Some(mid_uv) => off.push((j, mid_uv)),
                    None => missed = true,
                }
            }
        }
        if off.is_empty() && !missed {
            return Ok(SurfacePlaneCurves {
                curve_3d: c3d,
                uv_surface,
                uv_plane,
            });
        }
        // insert those middles as stations
        if st.len() + off.len() > MAX_STATIONS {
            return Err(SolidError::blend(
                OP,
                BlendFailure::PlaneCurveFitCap {
                    stations: st.len(),
                    passes: pass + 1,
                },
            ));
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
    Err(SolidError::blend(
        OP,
        BlendFailure::PlaneCurveFitCap {
            stations: st.len(),
            passes: 20,
        },
    ))
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
            // Not the fork's: a plane goes on as
            // itself past the face it was made for, so a point is on it by its distance to the
            // plane, not to the plane's domain, which ends at the face's edge (an end past a
            // shallow concave crease lies on the side face's plane continued)
            let off = if matches!(
                surface.kind(),
                crate::surface::surface::p2::SurfaceKind::Plane(_)
            ) && normal.length() > crate::math::EFF_ZERO
            {
                ((p - on).dot(*normal) / normal.length()).abs()
            } else {
                on.distance(p)
            };
            if off > tol {
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
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::surface::plane::Plane;

    /// The blossom of `x^k` in degree `n` at `i` copies of `hi` and `n - i` of `lo`: the
    /// elementary symmetric sum of degree `k` over its arguments, divided by `C(n, k)`.
    fn blossom(n: u32, k: u32, i: u32, lo: f64, hi: f64) -> f64 {
        let choose = |n: u32, k: u32| -> f64 {
            (0..k).fold(1.0, |c, j| c * f64::from(n - j) / f64::from(j + 1))
        };
        let power = |x: f64, e: u32| (0..e).fold(1.0, |p, _| p * x);
        let sum: f64 = (0..=k.min(i))
            .filter(|&r| k - r <= n - i)
            .map(|r| choose(i, r) * choose(n - i, k - r) * power(hi, r) * power(lo, k - r))
            .sum();
        sum / choose(n, k)
    }

    /// The surface `(u, v, 1e8 (v - u^2) (v - u^2 - 5e-5))` over `[-0.02, 0.02] x
    /// [-2e-4, 4e-4]`, one Bézier patch of degrees 4 and 2 (its control points the polynomial's
    /// blossoms, so it is the polynomial exactly). The plane `z = 0` crosses it on two branches,
    /// `v = u^2` and `v = u^2 + 5e-5`.
    fn two_parabolas() -> Surface {
        let (u0, u1, v0, v1) = (-0.02, 0.02, -2.0e-4, 4.0e-4);
        let (a, d) = (1.0e8, 5.0e-5);
        // f = a (v^2 - 2 u^2 v + u^4 - d v + d u^2), as (coefficient, power of u, power of v)
        let terms = [
            (a, 0, 2),
            (-2.0 * a, 2, 1),
            (a, 4, 0),
            (-a * d, 0, 1),
            (a * d, 2, 0),
        ];
        let net: Vec<Vec<CPoint>> = (0..=4_u32)
            .map(|i| {
                (0..=2_u32)
                    .map(|j| {
                        let x = blossom(4, 1, i, u0, u1);
                        let y = blossom(2, 1, j, v0, v1);
                        let z = terms
                            .iter()
                            .map(|&(c, p, q)| {
                                c * blossom(4, p, i, u0, u1) * blossom(2, q, j, v0, v1)
                            })
                            .sum();
                        CPoint::new(x, y, z, NOW)
                    })
                    .collect()
            })
            .collect();
        let uk = [u0, u0, u0, u0, u0, u1, u1, u1, u1, u1];
        let vk = [v0, v0, v0, v1, v1, v1];
        Surface::from(BSplineSurface::from_data(4, 2, 4, 2, 9, 5, &net, &uk, &vk).unwrap())
    }

    /// A span is split on the branch its stations trace. Stations at `u = -0.01` and `0.01` on
    /// `v = u^2` (a turn of 0.04 rad between them) have their chord's middle at `v = 1e-4`,
    /// above the other branch `v = u^2 + 5e-5`: the crossing's point reached from there lies on
    /// that other branch, 5e-5 off the stations' one, in the middle of the chord and nearer
    /// each station than the other is.
    #[test]
    fn a_span_is_split_on_the_branch_its_stations_trace_not_on_one_passing_between_them() {
        let k = Kernel::new(&KernelOptions::default());
        let surf = two_parabolas();
        let tr = Tracer {
            k: &k,
            surf: &surf,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vec3::new(0.0, 0.0, 1.0),
            plane_u: Vec3::new(1.0, 0.0, 0.0),
            plane_v: Vec3::new(0.0, 1.0, 0.0),
            g11: 1.0,
            g12: 0.0,
            g22: 1.0,
            det_g: 1.0,
            scale: 1.0,
        };
        let station = |u: f64| {
            let mut uv = Point2::new(u, u * u);
            assert!(tr.onto_plane(&mut uv).unwrap());
            assert!((uv.y - uv.x * uv.x).abs() <= 1.0e-12, "{uv:?}");
            let x = surf.evaluate_point(&k, uv).unwrap();
            let (t, rate) = tr.tangent(uv, Vec3::new(1.0, 0.0, 0.0)).unwrap();
            Station { uv, x, t, rate }
        };
        let (a, b) = (station(-0.01), station(0.01));
        let uv = tr.between(&a, &b).unwrap().unwrap();
        // on v = u^2, not on v = u^2 + 5e-5
        assert!((uv.y - uv.x * uv.x).abs() <= 1.0e-9, "{uv:?}");
        assert!(uv.x.abs() <= 0.005, "{uv:?}");
    }

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

    /// A plane goes on past the face it was made for: an end on the plane `z = 0` but past its
    /// domain (made for a face 2 wide) is on it, by its distance to the plane. Upstream
    /// dropped the end onto the domain, whose edge
    /// lies 2 from it, and answered `false`, so `sm_SurfacePlaneCurve` was never made for a
    /// corner's side curve running past its side face's edge.
    #[test]
    fn an_end_past_a_planes_domain_is_on_the_plane() {
        let k = Kernel::new(&KernelOptions::default());
        let small = Extent2d::from_bounds(-1.0, -1.0, 1.0, 1.0);
        let large = Extent2d::from_bounds(-50.0, -50.0, 50.0, 50.0);
        let floor = Surface::from(Plane::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec2::new(1.0, 1.0),
            &small,
        ));
        // the wall: x = 0
        let wall = Surface::from(Plane::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec2::new(1.0, 1.0),
            &large,
        ));
        let ends = [Point3::new(0.0, 0.5, 0.0), Point3::new(0.0, 3.0, 0.0)];
        assert!(ends_on_both(&k, [&floor, &wall], ends, 4.0e-5).unwrap());
        // an end off the plane is still off it
        let off = [ends[0], Point3::new(0.0, 3.0, 1.0e-3)];
        assert!(!ends_on_both(&k, [&floor, &wall], off, 4.0e-5).unwrap());
    }

    /// An end off the plane: a typed blend failure that keeps the bare `Upstream`'s status
    /// (#13790).
    #[test]
    fn an_end_off_the_plane_is_a_typed_failure() {
        let k = Kernel::new(&KernelOptions::default());
        let domain = Extent2d::from_bounds(-50.0, -50.0, 50.0, 50.0);
        let plane = |o: Point3, u: Vec3, v: Vec3| {
            Surface::from(Plane::new(o, u, v, Vec2::new(1.0, 1.0), &domain))
        };
        let surf = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        // the cutter: x = 0
        let cutter = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let (start, end) = (Point3::new(5.0, 0.0, 0.0), Point3::new(0.0, 10.0, 0.0));
        let e = surface_plane_curve(&k, &surf, &cutter, start, end, None, 1.0e-6, 0.1).unwrap_err();
        assert!(matches!(
            e.blend_failure(),
            Some(BlendFailure::PlaneCurveEndOff { at_end: false, .. })
        ));
        assert_eq!(e.operation(), OP);
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR));
    }
}
