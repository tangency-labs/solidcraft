//! A curve on a surface offset along the surface: the curve whose points lie a given geodesic
//! distance from the curve, each reached by the geodesic that leaves the curve square to it in
//! the surface's tangent plane. A band of constant width on the surface, as a chamfer's or a
//! shoulder's boundary needs.
//!
//! Not upstream. The construction, every step the surface's own derivatives:
//! - Stations at even steps of the curve's interval asked, its ends included. At each, the
//!   curve's point dropped onto the surface (its foot `(u, v)`), the unit normal
//!   `n = S_u x S_v / |S_u x S_v|`, the curve's tangent taken into the tangent plane, and the
//!   direction square to it there, `w = n x t` (to the left about `n`) or `-w` (to the right).
//! - From the foot along `w`, the geodesic of length `distance`: in arc length `s`,
//!   `x'' = S_u u'' + S_v v'' + Q` with `Q = S_uu u'^2 + 2 S_uv u'v' + S_vv v'^2` normal to the
//!   surface, so `[E F; F G] (u'', v'') = -(S_u . Q, S_v . Q)` (`E`, `F`, `G` the first
//!   fundamental form; each derivative read on the spans the geodesic runs into, as a rational
//!   sphere's quarters differ at their knots), integrated by the classical fourth-order
//!   Runge-Kutta rule from
//!   `(u, v)` and the parameter direction of `w`, each step cut where it meets one of the
//!   surface's knot lines (its seam included) so that every step reads one span, where the
//!   equation is smooth (a rational circle's second derivatives jump at its quarters). The
//!   step count doubles from 16 until two
//!   successive end points lie within a tenth of the tolerance of each other (at most
//!   [`MAX_STEPS`]). A geodesic crosses a periodic surface's seam; one leaving the surface's
//!   parameter box elsewhere is refused.
//! - Where neighbouring ends run back against the curve's direction carried along the
//!   geodesics to them (the offset's own direction there), the offset folds over itself and is
//!   refused.
//! - The offset is the cubic B-spline interpolating the geodesics' end points at the stations'
//!   own parameters. It is measured at the middle of every span: the geodesic shot from the
//!   curve there against the spline's point there. While the worst is past the tolerance the
//!   stations double (from 17, at most [`MAX_STATIONS`]).
//!
//! What holds: at every station the offset passes through the end of the geodesic of length
//! `distance` (to the integration's tolerance); between stations its distance from the shot
//! geodesics' ends is measured at the span middles and returned ([`GeodesicOffset::worst`]).
//! Measured, not proved.

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::crv_fit::p1::ParamMethod;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;

const OP: &str = "curve::geodesic_offset";

/// The most Runge-Kutta steps along one geodesic.
pub const MAX_STEPS: usize = 4096;
/// The most stations along the curve.
pub const MAX_STATIONS: usize = 1025;
/// The stations the construction starts from.
const FIRST_STATIONS: usize = 17;
/// The steps the integration starts from.
const FIRST_STEPS: usize = 16;
/// The most steps, cuts at knot lines included, per step asked.
const MAX_CUTS: usize = 8;
/// The Newton corrections of a step's length to land on a knot line.
const NEWTON_CUTS: usize = 3;
/// A knot line this small a fraction of a step from either end is not cut at.
const CUT_MARGIN: f64 = 1e-9;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// Which side of a curve on a surface the offset lies on, about the surface's own normal
/// (`S_u x S_v`): to the left of the curve's direction of travel (`n x t`), or to the right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// `n x t`.
    Left,
    /// `-(n x t)`.
    Right,
}

/// A curve offset along a surface ([`geodesic_offset`]).
#[derive(Clone, Debug)]
pub struct GeodesicOffset {
    /// The offset, a cubic B-spline on the curve's own parameters.
    pub curve: Curve,
    /// The largest distance found, at the middle of each span, between the offset and the end
    /// of the geodesic shot from the curve there.
    pub worst: f64,
    /// The stations the offset was built through.
    pub stations: usize,
}

/// `curve` over `interval` of its parameter offset `distance` along `surface` (over its parameter box `domain`) to `side`,
/// within `tolerance` where the measure allows (module notes).
///
/// # Errors
/// `InvalidInput` for a distance or a tolerance not finite and positive, a station whose foot is
/// not found or where the surface is singular, a curve with no tangent at a station, a
/// geodesic leaving the surface's parameter box, or ends that run back against the curve (the
/// distance past its radius of curvature on the surface, where the offset folds over itself); `NotConverged` when a geodesic's end does not
/// settle within [`MAX_STEPS`] steps; those of the evaluations, the drops and the fit.
pub fn geodesic_offset(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    distance: f64,
    side: Side,
    tolerance: f64,
) -> Result<GeodesicOffset> {
    if !(distance.is_finite() && distance > 0.0) {
        return Err(invalid("a distance finite and positive"));
    }
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(invalid("a tolerance finite and positive"));
    }
    // a periodic surface's geodesics cross its seam: the period along each parameter
    let period = |along: SurfParamType| -> Result<f64> {
        let side = if along == SurfParamType::U {
            domain.u_interval()
        } else {
            domain.v_interval()
        };
        Ok(if surface.is_periodic(k, domain, along)? {
            side.length()
        } else {
            0.0
        })
    };
    // the knot lines (a surface with none to read is integrated without cuts)
    let knots = |along: SurfParamType| -> Vec<f64> {
        surface
            .knots_with_nurb(k, along, None)
            .map(|(knots, _)| knots)
            .unwrap_or_default()
    };
    let shoot = Shoot {
        k,
        surface,
        domain,
        periods: (period(SurfParamType::U)?, period(SurfParamType::V)?),
        knots: (knots(SurfParamType::U), knots(SurfParamType::V)),
        curve,
        distance,
        side,
        settle: tolerance * 0.1,
    };
    let ivl = *interval;
    if !(ivl.min().is_finite() && ivl.max().is_finite() && ivl.min() < ivl.max()) {
        return Err(invalid("an interval of the curve finite and increasing"));
    }
    let mut count = FIRST_STATIONS;
    loop {
        let steps = count - 1;
        #[allow(clippy::cast_precision_loss)] // at most MAX_STATIONS
        let at = |i: usize| ivl.evaluate(i as f64 / steps as f64);
        let params: Vec<f64> = (0..count).map(at).collect();
        let mut ends = Vec::new();
        let mut tangents = Vec::new();
        for &t in &params {
            let (end, tangent) = shoot.end_at(t)?;
            ends.push(end);
            tangents.push(tangent);
        }
        // Past the curve's radius of curvature on the surface the geodesics cross and the ends
        // run back against the curve's direction carried to them: the offset folds over itself.
        // (Each chord against the carried tangents at its own ends, in the tangent planes there:
        // on a curved surface the source's tangent can point anywhere at the far end.)
        let back = ends
            .windows(2)
            .zip(tangents.windows(2))
            .any(|(e, t)| match (e, t) {
                ([p, q], [a, b]) => {
                    let chord = *q - *p;
                    chord.dot(*a) < 0.0 || chord.dot(*b) < 0.0
                }
                _ => false,
            });
        if back {
            return Err(invalid(
                "the offset folds over itself: the distance is past the curve's radius of curvature on the surface",
            ));
        }
        let fit = Curve::from(BSplineCurve::create_interpolating_curve_with_params(
            ParamMethod::ChordLength,
            3,
            3,
            &ends,
            &[],
            None,
            false,
            Some(&params),
        )?);
        let mut worst = 0.0_f64;
        for (a, b) in params.iter().zip(params.iter().skip(1)) {
            let mid = 0.5 * (a + b);
            let (truth, _) = shoot.end_at(mid)?;
            worst = worst.max(fit.evaluate_point(k, mid)?.distance(truth));
        }
        if worst <= tolerance || count >= MAX_STATIONS {
            return Ok(GeodesicOffset {
                curve: fit,
                worst,
                stations: count,
            });
        }
        count = (2 * count - 1).min(MAX_STATIONS);
    }
}

/// The fraction of the way from `x0` to `x1` (up to `reach` times the way, past a sliver at
/// `x0`) where the first of `knots` lies, and its value there: on a periodic parameter
/// (`period` more than 0) the knots' copies a whole period on count too.
fn knot_on_way(x0: f64, x1: f64, reach: f64, knots: &[f64], period: f64) -> Option<(f64, f64)> {
    let dx = x1 - x0;
    if dx == 0.0 || !dx.is_finite() {
        return None;
    }
    let far = x0 + reach * dx;
    let (lo, hi) = (x0.min(far), x0.max(far));
    let mut best: Option<(f64, f64)> = None;
    for &kn in knots {
        let (first, last) = if period > 0.0 {
            (((lo - kn) / period).floor(), ((hi - kn) / period).ceil())
        } else {
            (0.0, 0.0)
        };
        let mut m = first;
        while m <= last && m - first < 8.0 {
            let at = kn + m * period;
            let f = (at - x0) / dx;
            if f > CUT_MARGIN && f <= reach && best.is_none_or(|(b, _)| f < b) {
                best = Some((f, at));
            }
            m += 1.0;
        }
    }
    best
}

/// One offset's geodesics: what each shot needs.
struct Shoot<'a> {
    k: &'a Kernel,
    surface: &'a Surface,
    domain: &'a Extent2d,
    /// The surface's period along u and v over `domain` (0: not periodic).
    periods: (f64, f64),
    /// The surface's distinct knots along u and v, its ends included.
    knots: (Vec<f64>, Vec<f64>),
    curve: &'a Curve,
    distance: f64,
    side: Side,
    /// How close two successive end points must lie for the integration to have settled.
    settle: f64,
}

/// The surface's derivatives to second order at a point.
struct Derivs {
    p: Point3,
    su: Vec3,
    sv: Vec3,
    suu: Vec3,
    suv: Vec3,
    svv: Vec3,
}

impl Shoot<'_> {
    /// `uv` taken into the parameter box, across a periodic surface's seam where it runs over
    /// one; refused where it lies outside the box.
    fn inside(&self, uv: Point2) -> Result<Point2> {
        let (inside, at) =
            self.domain
                .contains_periodic_point(uv, self.periods.0, self.periods.1, 0.0);
        if inside {
            Ok(at)
        } else {
            Err(invalid(
                "the offset leaves the surface: a geodesic runs out of its parameter box",
            ))
        }
    }

    /// The derivatives at `uv`, read on the spans a line moving along `(a, b)` runs into (at
    /// a knot a surface's derivatives differ from one span to the next, as a rational
    /// circle's quarters do).
    fn derivs(&self, uv: Point2, (a, b): (f64, f64)) -> Result<Derivs> {
        // On a periodic surface's seam, moving out of the box: read the spans across the seam
        // (past the box's end there is no span to read).
        let across = |x: f64, d: f64, lo: f64, hi: f64, period: f64| {
            if period > 0.0 && d < 0.0 && x <= lo {
                x + period
            } else if period > 0.0 && d > 0.0 && x >= hi {
                x - period
            } else {
                x
            }
        };
        let (ui, vi) = (self.domain.u_interval(), self.domain.v_interval());
        let uv = Point2::new(
            across(uv.x, a, ui.min(), ui.max(), self.periods.0),
            across(uv.y, b, vi.min(), vi.max(), self.periods.1),
        );
        let side = |d: f64| {
            if d < 0.0 {
                crate::nurbs::knots::Side::Right
            } else {
                crate::nurbs::knots::Side::Left
            }
        };
        let d = self.surface.evaluate(
            self.k,
            uv,
            2,
            2,
            EvaluateFlags {
                u_side: side(a),
                v_side: side(b),
                non_zero_tangents: false,
                ..EvaluateFlags::default()
            },
        )?;
        let at = |i: usize, j: usize| -> Result<Vec3> {
            d.get(i)
                .and_then(|r| r.get(j))
                .copied()
                .ok_or_else(|| invalid("a surface's derivatives"))
        };
        Ok(Derivs {
            p: at(0, 0)?,
            su: at(1, 0)?,
            sv: at(0, 1)?,
            suu: at(2, 0)?,
            suv: at(1, 1)?,
            svv: at(0, 2)?,
        })
    }

    /// The parameter direction `(a, b)` with `S_u a + S_v b = w`, by its normal equations.
    fn parameter_direction(d: &Derivs, w: Vec3) -> Result<(f64, f64)> {
        let (e, f, g) = (d.su.dot(d.su), d.su.dot(d.sv), d.sv.dot(d.sv));
        let det = e * g - f * f;
        #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
        if !(det > 0.0) {
            return Err(invalid("a surface singular on the offset's way"));
        }
        let (wu, wv) = (w.dot(d.su), w.dot(d.sv));
        Ok(((g * wu - f * wv) / det, (e * wv - f * wu) / det))
    }

    /// The geodesic equation's right-hand side at state `(u, v, a, b)`: `(a, b, a', b')`, for a
    /// step from state `from`. A step lives in one span (it is cut at knot lines): a stage
    /// rounded past a knot line from `from` is read on the line, on `from`'s side.
    fn rate(&self, s: [f64; 4], from: [f64; 4]) -> Result<[f64; 4]> {
        let [u, v, a, b] = s;
        let read = |x: f64, x0: f64, velocity: f64, knots: &[f64], period: f64| {
            knot_on_way(x0, x, 1.0, knots, period).map_or((x, velocity), |(_, at)| (at, x0 - x))
        };
        let (ue, ua) = read(u, from[0], a, &self.knots.0, self.periods.0);
        let (ve, vb) = read(v, from[1], b, &self.knots.1, self.periods.1);
        let d = self.derivs(self.inside(Point2::new(ue, ve))?, (ua, vb))?;
        let q = d.suu * (a * a) + d.suv * (2.0 * a * b) + d.svv * (b * b);
        // [E F; F G] (a', b') = -(S_u . Q, S_v . Q)
        let (e, f, g) = (d.su.dot(d.su), d.su.dot(d.sv), d.sv.dot(d.sv));
        let det = e * g - f * f;
        #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
        if !(det > 0.0) {
            return Err(invalid("a surface singular on the offset's way"));
        }
        let (ru, rv) = (-d.su.dot(q), -d.sv.dot(q));
        Ok([a, b, (g * ru - f * rv) / det, (e * rv - f * ru) / det])
    }

    /// The end of the geodesic of length `distance` from state `start` in `steps` equal steps of
    /// the classical Runge-Kutta rule, and its parameter direction there.
    fn integrate(&self, start: [f64; 4], steps: usize) -> Result<(Point2, (f64, f64))> {
        #[allow(clippy::cast_precision_loss)] // at most MAX_STEPS
        let h = self.distance / steps as f64;
        let add = |s: [f64; 4], r: [f64; 4], f: f64| {
            let mut out = s;
            for (x, r) in out.iter_mut().zip(r) {
                *x += r * f;
            }
            out
        };
        let step = |s: [f64; 4], h: f64| -> Result<[f64; 4]> {
            let k1 = self.rate(s, s)?;
            let k2 = self.rate(add(s, k1, 0.5 * h), s)?;
            let k3 = self.rate(add(s, k2, 0.5 * h), s)?;
            let k4 = self.rate(add(s, k3, h), s)?;
            let mut next = s;
            for (((x, a), (b, c)), d) in next.iter_mut().zip(k1).zip(k2.into_iter().zip(k3)).zip(k4)
            {
                *x += h / 6.0 * (a + 2.0 * b + 2.0 * c + d);
            }
            Ok(next)
        };
        // A step across a knot line (where the surface's second derivatives jump, as at a
        // rational circle's quarters and a periodic surface's seam) is cut there and the state
        // put on the line itself, so every step reads the derivatives of one span only: the
        // rule's order holds only where the geodesic equation is smooth over the step. A knot
        // just past a step's end (up to half a step on) is reached in one longer step, so no
        // step ends a rounding's width short of a knot and reads the span behind it.
        let mut s = start;
        let mut left = self.distance;
        let most = steps.saturating_mul(MAX_CUTS);
        for _ in 0..most {
            #[allow(clippy::neg_cmp_op_on_partial_ord)] // stops on NaN too
            if !(left > CUT_MARGIN * h) {
                break;
            }
            let hh = h.min(left);
            let next = step(s, hh)?;
            if let Some((f, axis, at)) = self.first_knot(s, next, (left / hh).min(1.5)) {
                // where the step meets the knot: Newton's rule on the parameter along the
                // geodesic, its rate the state's own (du/ds, dv/ds)
                let mut cut = (f * hh).min(left);
                let mut part = step(s, cut)?;
                for _ in 0..NEWTON_CUTS {
                    let (Some(x), Some(rate)) = (part.get(axis), part.get(axis + 2)) else {
                        break;
                    };
                    let fix = (at - x) / rate;
                    if !fix.is_finite() || fix == 0.0 {
                        break;
                    }
                    cut = (cut + fix).clamp(0.0, left);
                    part = step(s, cut)?;
                }
                s = part;
                if let Some(x) = s.get_mut(axis) {
                    *x = at;
                }
                left -= cut;
            } else {
                s = next;
                left -= hh;
            }
        }
        if left > CUT_MARGIN * h {
            s = step(s, left)?;
        }
        let [u, v, a, b] = s;
        Ok((self.inside(Point2::new(u, v))?, (a, b)))
    }

    /// The fraction of the way from state `s` to state `next` (by the parameters, linearly, up
    /// to `reach` times the way) where the first knot line after `s` lies, if any, with its
    /// parameter (0: u, 1: v) and its value on the way (a periodic surface's knot a whole
    /// period on as needed).
    fn first_knot(&self, s: [f64; 4], next: [f64; 4], reach: f64) -> Option<(f64, usize, f64)> {
        let [u0, v0, _, _] = s;
        let [u1, v1, _, _] = next;
        let u = knot_on_way(u0, u1, reach, &self.knots.0, self.periods.0).map(|(f, at)| (f, 0, at));
        let v = knot_on_way(v0, v1, reach, &self.knots.1, self.periods.1).map(|(f, at)| (f, 1, at));
        match (u, v) {
            (Some(a), Some(b)) => Some(if b.0 < a.0 { b } else { a }),
            (a, b) => a.or(b),
        }
    }

    /// The end of the geodesic of length `distance` leaving the curve at its parameter `t`,
    /// square to it, to the side asked, and the curve's unit tangent in the surface carried
    /// along the geodesic to that end (the offset's own direction there, in the tangent plane at
    /// the end). The tangent leaves as `w x n` to the left (`-(w x n)` to the right) of the
    /// geodesic's direction `w`; carried along a geodesic it keeps that angle to the geodesic's
    /// direction, so at the end it is the same expression in the end's `w` and `n`.
    fn end_at(&self, t: f64) -> Result<(Point3, Vec3)> {
        let k = self.k;
        let c = self.curve.evaluate(k, t, 1, false, true)?;
        let (Some(&p), Some(&tangent)) = (c.first(), c.get(1)) else {
            return Err(invalid("a curve's derivatives"));
        };
        let foot =
            self.surface
                .drop_point::<NoObject>(k, p, self.domain, &DropPointOptions::default())?;
        if !foot.success {
            return Err(invalid(
                "a station of the curve with no foot on the surface",
            ));
        }
        let uv = foot.result_uv;
        let d = self.derivs(uv, (0.0, 0.0))?;
        let n =
            d.su.cross(d.sv)
                .unitize()
                .map_err(|_| invalid("a surface singular on the offset's way"))?;
        let along = (tangent - n * tangent.dot(n))
            .unitize()
            .map_err(|_| invalid("a curve with no tangent along the surface at a station"))?;
        let across = n
            .cross(along)
            .unitize()
            .map_err(|_| invalid("a curve with no tangent along the surface at a station"))?;
        let w = match self.side {
            Side::Left => across,
            Side::Right => -across,
        };
        let (a, b) = Self::parameter_direction(&d, w)?;
        let start = [uv.x, uv.y, a, b];
        let mut steps = FIRST_STEPS;
        let mut last = self.derivs(self.integrate(start, steps)?.0, (0.0, 0.0))?.p;
        while steps < MAX_STEPS {
            steps *= 2;
            let (end, (a, b)) = self.integrate(start, steps)?;
            // read on the spans the geodesic arrives from
            let d = self.derivs(end, (-a, -b))?;
            let next = d.p;
            if next.distance(last) <= self.settle {
                let n =
                    d.su.cross(d.sv)
                        .unitize()
                        .map_err(|_| invalid("a surface singular on the offset's way"))?;
                let w = d.su * a + d.sv * b;
                let carried = match self.side {
                    Side::Left => w.cross(n),
                    Side::Right => n.cross(w),
                }
                .unitize()
                .map_err(|_| invalid("a surface singular on the offset's way"))?;
                return Ok((next, carried));
            }
            last = next;
        }
        Err(SolidError::NotConverged {
            operation: OP,
            iterations: u32::try_from(MAX_STEPS).unwrap_or(u32::MAX),
        })
    }
}

#[cfg(test)]
mod tests;
