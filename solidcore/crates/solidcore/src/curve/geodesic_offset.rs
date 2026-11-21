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
//!   `(u, v)` and the parameter direction of `w`. The step count doubles from 16 until two
//!   successive end points lie within a tenth of the tolerance of each other (at most
//!   [`MAX_STEPS`]). A geodesic crosses a periodic surface's seam; one leaving the surface's
//!   parameter box elsewhere is refused.
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
/// not found or where the surface is singular, a curve with no tangent at a station, or a
/// geodesic leaving the surface's parameter box; `NotConverged` when a geodesic's end does not
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
    let shoot = Shoot {
        k,
        surface,
        domain,
        periods: (period(SurfParamType::U)?, period(SurfParamType::V)?),
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
        for &t in &params {
            ends.push(shoot.end_at(t)?);
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
            let truth = shoot.end_at(mid)?;
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

/// One offset's geodesics: what each shot needs.
struct Shoot<'a> {
    k: &'a Kernel,
    surface: &'a Surface,
    domain: &'a Extent2d,
    /// The surface's period along u and v over `domain` (0: not periodic).
    periods: (f64, f64),
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

    /// The geodesic equation's right-hand side at state `(u, v, a, b)`: `(a, b, a', b')`.
    fn rate(&self, s: [f64; 4]) -> Result<[f64; 4]> {
        let [u, v, a, b] = s;
        let d = self.derivs(self.inside(Point2::new(u, v))?, (a, b))?;
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
    /// the classical Runge-Kutta rule.
    fn integrate(&self, start: [f64; 4], steps: usize) -> Result<Point2> {
        #[allow(clippy::cast_precision_loss)] // at most MAX_STEPS
        let h = self.distance / steps as f64;
        let add = |s: [f64; 4], r: [f64; 4], f: f64| {
            let mut out = s;
            for (x, r) in out.iter_mut().zip(r) {
                *x += r * f;
            }
            out
        };
        let mut s = start;
        for _ in 0..steps {
            let k1 = self.rate(s)?;
            let k2 = self.rate(add(s, k1, 0.5 * h))?;
            let k3 = self.rate(add(s, k2, 0.5 * h))?;
            let k4 = self.rate(add(s, k3, h))?;
            let mut next = s;
            for (((x, a), (b, c)), d) in next.iter_mut().zip(k1).zip(k2.into_iter().zip(k3)).zip(k4)
            {
                *x += h / 6.0 * (a + 2.0 * b + 2.0 * c + d);
            }
            s = next;
        }
        let [u, v, _, _] = s;
        self.inside(Point2::new(u, v))
    }

    /// The end of the geodesic of length `distance` leaving the curve at its parameter `t`,
    /// square to it, to the side asked.
    fn end_at(&self, t: f64) -> Result<Point3> {
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
        let across = n
            .cross(tangent - n * tangent.dot(n))
            .unitize()
            .map_err(|_| invalid("a curve with no tangent along the surface at a station"))?;
        let w = match self.side {
            Side::Left => across,
            Side::Right => -across,
        };
        let (a, b) = Self::parameter_direction(&d, w)?;
        let start = [uv.x, uv.y, a, b];
        let mut steps = FIRST_STEPS;
        let mut last = self.derivs(self.integrate(start, steps)?, (0.0, 0.0))?.p;
        while steps < MAX_STEPS {
            steps *= 2;
            let next = self.derivs(self.integrate(start, steps)?, (0.0, 0.0))?.p;
            if next.distance(last) <= self.settle {
                return Ok(next);
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
