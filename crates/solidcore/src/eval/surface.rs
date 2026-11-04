//! Points of surfaces in their own parameters, and the parameters of a point on a surface.
//! Planes, splines and revolutions go through [`EvalSurface`]; cylinders, cones, spheres and
//! tori are evaluated and inverted in closed form, in angle and height.
//!
//! Evaluation never panics: a point that cannot be had (a vanishing weight, a parameter that
//! is not finite) is `None`.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use super::geometry::{Curve, Curve2, Frame, Surface};
use super::spline::{
    tabulate, trig_derivative, EvalCurve, EvalSurface, Partials, MAX_EVAL_KNOTS, MAX_EVAL_POINTS,
    MAX_SPLINE_DEGREE,
};
use super::uv::{self, Domain};
use super::vec::{v3, VecExt};
use super::{Collapse, CurveEval, EvalError};
use crate::math::Vec3;
use std::f64::consts::{FRAC_PI_2, TAU};

fn unsupported(what: impl std::fmt::Display) -> EvalError {
    EvalError::Unsupported(what.to_string())
}

/// A frame's origin and axes, z = x × y.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axes {
    /// The origin.
    pub o: Vec3,
    /// The x axis.
    pub x: Vec3,
    /// The y axis.
    pub y: Vec3,
    /// `x × y`.
    pub z: Vec3,
}
impl Axes {
    /// The frame's origin and axes.
    #[must_use]
    pub fn new(f: &Frame) -> Self {
        let (x, y) = (v3(f.x_axis), v3(f.y_axis));
        Axes {
            o: v3(f.origin),
            x,
            y,
            z: x.cross(y),
        }
    }
    /// `p` in the frame: (x, y, z) coordinates.
    #[must_use]
    pub fn local(&self, p: Vec3) -> [f64; 3] {
        let q = p - self.o;
        [q.dot(self.x), q.dot(self.y), q.dot(self.z)]
    }
    /// The radial direction at angle `u` and its derivative.
    fn radial(&self, u: f64) -> (Vec3, Vec3) {
        let (s, c) = u.sin_cos();
        (self.x * c + self.y * s, self.y * c - self.x * s)
    }
}

/// The angle of (a, b) in [0, 2π), `near`'s (or 0) on the axis, where it is undefined.
fn angle(a: f64, b: f64, near: Option<f64>) -> f64 {
    if a == 0.0 && b == 0.0 {
        return near.unwrap_or(0.0);
    }
    let u = b.atan2(a);
    if u < 0.0 {
        u + TAU
    } else {
        u
    }
}

/// A surface ready to evaluate.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)] // evaluated in place: a box would cost every evaluation a load
pub enum SurfaceEval {
    /// A plane, spline or revolution, with its parameter box.
    Eval(EvalSurface, Domain),
    /// [`Surface::Cylinder`].
    Cylinder {
        /// The frame.
        axes: Axes,
        /// The radius.
        radius: f64,
    },
    /// [`Surface::Cone`].
    Cone {
        /// The frame.
        axes: Axes,
        /// The radius at v = 0.
        radius_at_origin: f64,
        /// The radius's change per unit height, `tan(half_angle)`.
        slope: f64,
    },
    /// [`Surface::Sphere`].
    Sphere {
        /// The frame.
        axes: Axes,
        /// The radius.
        radius: f64,
    },
    /// [`Surface::Torus`].
    Torus {
        /// The frame.
        axes: Axes,
        /// The ring's radius.
        major: f64,
        /// The tube's radius.
        minor: f64,
        /// The tube angle at v = 0 (`Surface::Torus::tube_start`).
        tube: f64,
    },
}

impl SurfaceEval {
    /// The evaluator of `s`.
    pub fn new(s: &Surface) -> Result<Self, EvalError> {
        let eval = |e: EvalSurface| {
            let dom = uv::domain(&e);
            Ok(SurfaceEval::Eval(e, dom))
        };
        match s {
            Surface::Plane {
                origin,
                u_axis,
                v_axis,
            } => eval(EvalSurface::Plane {
                origin: v3(*origin),
                du: v3(*u_axis),
                dv: v3(*v_axis),
            }),
            Surface::PlaneThrough {
                origin,
                u_point,
                v_point,
            } => {
                let o = v3(*origin);
                eval(EvalSurface::Plane {
                    origin: o,
                    du: v3(*u_point) - o,
                    dv: v3(*v_point) - o,
                })
            }
            Surface::Cylinder { frame, radius } => Ok(SurfaceEval::Cylinder {
                axes: Axes::new(frame),
                radius: *radius,
            }),
            Surface::Cone {
                frame,
                radius_at_origin,
                half_angle,
            } => Ok(SurfaceEval::Cone {
                axes: Axes::new(frame),
                radius_at_origin: *radius_at_origin,
                slope: half_angle.tan(),
            }),
            Surface::Sphere { frame, radius } => Ok(SurfaceEval::Sphere {
                axes: Axes::new(frame),
                radius: *radius,
            }),
            Surface::Torus {
                frame,
                major_radius,
                minor_radius,
                tube_start,
            } => Ok(SurfaceEval::Torus {
                axes: Axes::new(frame),
                major: *major_radius,
                minor: *minor_radius,
                tube: *tube_start,
            }),
            // A label with no (u, v) of its own: its face evaluates its NURBS form.
            Surface::BallPatch { .. } => Err(unsupported(
                "a ball patch's label (evaluate its NURBS form)",
            )),
            Surface::Spline {
                u_knots,
                v_knots,
                points,
            } => {
                let rows: Vec<Vec<[f64; 4]>> = points
                    .iter()
                    .map(|r| r.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect())
                    .collect();
                eval(nurbs(u_knots, v_knots, &rows)?)
            }
            Surface::RationalSpline {
                u_knots,
                v_knots,
                homogeneous,
            } => eval(nurbs(u_knots, v_knots, homogeneous)?),
            Surface::PlacedRevolution {
                profile,
                origin,
                axis,
                matrix,
                forward,
            } => {
                // An intersection profile is refused on the record, before descending into it:
                // its surfaces may be revolutions of intersections in turn, nested without end.
                if matches!(**profile, Curve::SurfaceIntersection { .. }) {
                    return Err(unsupported(
                        "a revolution of a conic, polyline or intersection profile",
                    ));
                }
                let profile = match CurveEval::new(profile)? {
                    CurveEval::Spline(c) => c,
                    CurveEval::Line { start, end } => EvalCurve::new(
                        1,
                        vec![0.0, 0.0, 1.0, 1.0],
                        vec![[start.x, start.y, start.z, 1.0], [end.x, end.y, end.z, 1.0]],
                    ),
                    _ => {
                        return Err(unsupported(
                            "a revolution of a conic, polyline or intersection profile",
                        ));
                    }
                };
                let axis = v3(*axis)
                    .normalized()
                    .ok_or_else(|| EvalError::Invalid("revolution axis".into()))?;
                // Column-major: matrix[column][row].
                let [c0, c1, c2, c3] = *matrix;
                let transform = [0, 1, 2].map(|r| {
                    let at = |c: [f64; 4]| c.get(r).copied().unwrap_or(0.0);
                    [at(c0), at(c1), at(c2), at(c3)]
                });
                eval(EvalSurface::Revolved {
                    profile,
                    origin: v3(*origin),
                    axis,
                    transform,
                    swap: !forward,
                })
            }
        }
    }

    /// The parameter box and periods.
    pub fn domain(&self) -> Domain {
        let turn = (Some((0.0, TAU)), Some(TAU));
        let open = (None, None);
        let ((ru, pu), (rv, pv)) = match self {
            SurfaceEval::Eval(_, dom) => return *dom,
            SurfaceEval::Cylinder { .. } | SurfaceEval::Cone { .. } => (turn, open),
            SurfaceEval::Sphere { .. } => (turn, (Some((-FRAC_PI_2, FRAC_PI_2)), None)),
            SurfaceEval::Torus { .. } => (turn, turn),
        };
        Domain {
            range: [ru, rv],
            period: [pu, pv],
        }
    }

    /// Point and first partial derivatives at (u, v).
    pub fn derivs(&self, u: f64, v: f64) -> Option<(Vec3, Vec3, Vec3)> {
        if !(u.is_finite() && v.is_finite()) {
            return None;
        }
        Some(match self {
            SurfaceEval::Eval(e, _) => return e.derivs(u, v),
            SurfaceEval::Cylinder { axes: a, radius } => {
                let (r, dr) = a.radial(u);
                (a.o + a.z * v + r * *radius, dr * *radius, a.z)
            }
            SurfaceEval::Cone {
                axes: a,
                radius_at_origin,
                slope,
            } => {
                let (r, dr) = a.radial(u);
                let rv = radius_at_origin + v * slope;
                (a.o + a.z * v + r * rv, dr * rv, a.z + r * *slope)
            }
            SurfaceEval::Sphere { axes: a, radius } => {
                let (r, dr) = a.radial(u);
                let (s, c) = v.sin_cos();
                (
                    a.o + (r * c + a.z * s) * *radius,
                    dr * (c * radius),
                    (a.z * c - r * s) * *radius,
                )
            }
            SurfaceEval::Torus {
                axes: a,
                major,
                minor,
                tube,
            } => {
                let (r, dr) = a.radial(u);
                let (s, c) = (v + tube).sin_cos();
                let ring = major + minor * c;
                (
                    a.o + r * ring + a.z * (minor * s),
                    dr * ring,
                    (a.z * c - r * s) * *minor,
                )
            }
        })
    }

    /// The point at `uv`.
    #[must_use]
    pub fn point(&self, uv: [f64; 2]) -> Option<Vec3> {
        self.derivs(uv[0], uv[1]).map(|d| d.0)
    }

    /// The second partial derivatives `(S_uu, S_uv, S_vv)` at (u, v): in closed form on the
    /// analytic surfaces, from the basis functions' own second derivatives on a spline; `None`
    /// for a revolution ([`EvalSurface::second`]).
    pub fn second(&self, u: f64, v: f64) -> Option<(Vec3, Vec3, Vec3)> {
        if !(u.is_finite() && v.is_finite()) {
            return None;
        }
        Some(match self {
            SurfaceEval::Eval(e, _) => return e.second(u, v),
            SurfaceEval::Cylinder { axes: a, radius } => {
                (a.radial(u).0 * -*radius, Vec3::ZERO, Vec3::ZERO)
            }
            SurfaceEval::Cone {
                axes: a,
                radius_at_origin,
                slope,
            } => {
                let (r, dr) = a.radial(u);
                (r * -(radius_at_origin + v * slope), dr * *slope, Vec3::ZERO)
            }
            SurfaceEval::Sphere { axes: a, radius } => {
                let (r, dr) = a.radial(u);
                let (s, c) = v.sin_cos();
                (
                    r * (-c * radius),
                    dr * (-s * radius),
                    (r * c + a.z * s) * -*radius,
                )
            }
            SurfaceEval::Torus {
                axes: a,
                major,
                minor,
                tube,
            } => {
                let (r, dr) = a.radial(u);
                let (s, c) = (v + tube).sin_cos();
                (
                    r * -(major + minor * c),
                    dr * (-s * minor),
                    (r * c + a.z * s) * -*minor,
                )
            }
        })
    }

    /// The partial derivatives at (u, v) up to the fourth order ([`Partials`]), exactly: in closed
    /// form on the analytic surfaces (angles differentiate as `cos(t + kπ/2)`), on the display
    /// evaluator otherwise ([`EvalSurface::partials`]).
    pub fn partials(&self, u: f64, v: f64) -> Option<Partials> {
        if !(u.is_finite() && v.is_finite()) {
            return None;
        }
        // The i-th derivative of the radial direction at angle u, and the k-th of cos and sin at v.
        let radial = |a: &Axes, i: usize| {
            let (c, s) = trig_derivative(u, i);
            a.x * c + a.y * s
        };
        let at_v = |k: usize| trig_derivative(v, k);
        let lift =
            |a: &Axes, i: usize, j: usize, x: Vec3| if i == 0 && j == 0 { a.o + x } else { x };
        match self {
            SurfaceEval::Eval(e, _) => e.partials(u, v),
            SurfaceEval::Cylinder { axes: a, radius } => tabulate(|i, j| {
                let height = match j {
                    0 if i == 0 => a.z * v,
                    1 if i == 0 => a.z,
                    _ => Vec3::ZERO,
                };
                let ring = if j == 0 {
                    radial(a, i) * *radius
                } else {
                    Vec3::ZERO
                };
                Some(lift(a, i, j, ring + height))
            }),
            SurfaceEval::Cone {
                axes: a,
                radius_at_origin,
                slope,
            } => tabulate(|i, j| {
                let (height, rv) = match j {
                    0 => (
                        if i == 0 { a.z * v } else { Vec3::ZERO },
                        radius_at_origin + v * slope,
                    ),
                    1 => (if i == 0 { a.z } else { Vec3::ZERO }, *slope),
                    _ => (Vec3::ZERO, 0.0),
                };
                Some(lift(a, i, j, radial(a, i) * rv + height))
            }),
            SurfaceEval::Sphere { axes: a, radius } => tabulate(|i, j| {
                let (c, s) = at_v(j);
                let pole = if i == 0 { a.z * s } else { Vec3::ZERO };
                Some(lift(a, i, j, (radial(a, i) * c + pole) * *radius))
            }),
            SurfaceEval::Torus {
                axes: a,
                major,
                minor,
                tube,
            } => tabulate(|i, j| {
                let (c, s) = trig_derivative(v + tube, j);
                let ring = if j == 0 { major + minor * c } else { minor * c };
                let tube = if i == 0 {
                    a.z * (minor * s)
                } else {
                    Vec3::ZERO
                };
                Some(lift(a, i, j, radial(a, i) * ring + tube))
            }),
        }
    }

    /// The parameters of `p`, a point within `accept` of the surface, from `near` when given
    /// (not unwrapped to it).
    pub fn project(&self, p: Vec3, near: Option<[f64; 2]>, accept: f64) -> Option<[f64; 2]> {
        self.project_with(&uv::Starts::default(), p, near, accept)
    }

    /// [`SurfaceEval::project`] with the search grid of a spline or revolution kept in `starts`
    /// ([`uv::project_with`]), which must serve this surface only.
    pub fn project_with(
        &self,
        starts: &uv::Starts,
        p: Vec3,
        near: Option<[f64; 2]>,
        accept: f64,
    ) -> Option<[f64; 2]> {
        let q = match self {
            SurfaceEval::Eval(e, dom) => return uv::project_with(e, dom, starts, p, near, accept),
            SurfaceEval::Cylinder { axes, .. } => {
                let [a, b, h] = axes.local(p);
                [angle(a, b, near.map(|n| n[0])), h]
            }
            SurfaceEval::Cone {
                axes,
                radius_at_origin,
                slope,
            } => {
                // The nearest point of the generator line through p's half-plane.
                let [a, b, h] = axes.local(p);
                [
                    angle(a, b, near.map(|n| n[0])),
                    ((a.hypot(b) - radius_at_origin) * slope + h) / (1.0 + slope * slope),
                ]
            }
            SurfaceEval::Sphere { axes, .. } => {
                let [a, b, h] = axes.local(p);
                [angle(a, b, near.map(|n| n[0])), h.atan2(a.hypot(b))]
            }
            SurfaceEval::Torus {
                axes, major, tube, ..
            } => {
                let [a, b, h] = axes.local(p);
                let v = (h.atan2(a.hypot(b) - major) - tube).rem_euclid(TAU);
                [angle(a, b, near.map(|n| n[0])), v]
            }
        };
        (q[0].is_finite() && q[1].is_finite() && self.point(q).is_some_and(|s| s.dist(p) <= accept))
            .then_some(q)
    }

    /// Which directions are collapsed at `uv`, exactly (to rounding), or within `reach` mm
    /// ([`super::collapse`]): a spline or revolution by its iso-lines, an analytic form by the
    /// radius of its circle along the angle in closed form (a cone's apex, a sphere's pole, a
    /// torus's ring or tube of radius zero). Refused with a typed error where that cannot be
    /// decided.
    pub fn collapse(&self, uv: [f64; 2], reach: f64) -> Result<Collapse, EvalError> {
        super::collapse::check(uv, reach)?;
        let v = uv[1];
        let (g, unit, circle) = (
            super::collapse::gamma,
            super::collapse::UNIT,
            super::collapse::circle,
        );
        // The circle along u of radius `r`, rounded within `err`; v moves the point along a
        // straight line or a circle of radius `along_v` (a cylinder's and cone's generator never
        // stands still; a torus's tube does when its radius is zero).
        let (u_flag, v_flag) = match self {
            SurfaceEval::Eval(e, _) => return super::collapse::collapse(e, uv, reach),
            SurfaceEval::Cylinder { radius, .. } => (circle(*radius, 0.0, reach)?, false),
            SurfaceEval::Cone {
                radius_at_origin,
                slope,
                ..
            } => {
                let r = radius_at_origin + v * slope;
                let err = g(2) * (radius_at_origin.abs() + (v * slope).abs());
                (circle(r, err, reach)?, false)
            }
            SurfaceEval::Sphere { radius, .. } => {
                // R cos v, its cosine rounded, and the pole's own latitude (π/2) not being a
                // float: the parameter given for it is off by up to its rounding.
                let err = radius.abs() * (g(3) + 2.0 * unit * v.abs());
                (
                    circle(radius * v.cos(), err, reach)?,
                    circle(*radius, 0.0, reach)?,
                )
            }
            SurfaceEval::Torus {
                major, minor, tube, ..
            } => {
                let w = v + tube;
                let ring = major + minor * w.cos();
                let err = g(4) * (major.abs() + minor.abs()) + minor.abs() * 2.0 * unit * w.abs();
                (circle(ring, err, reach)?, circle(*minor, 0.0, reach)?)
            }
        };
        Ok(Collapse::of([u_flag, v_flag]))
    }

    /// Whether moving along each direction at `uv` leaves the point where it is (a pole):
    /// [`Self::collapse`] at no reach.
    pub fn pole(&self, uv: [f64; 2]) -> Result<[bool; 2], EvalError> {
        Ok(self.collapse(uv, 0.0)?.flags())
    }

    /// [`Self::pole`], or within `reach` mm of one: moving along a direction across its whole
    /// range moves the point no more than `reach`, certified (a boundary point that lies at a
    /// pole to within a tolerance, where its periodic parameter is free).
    pub fn pole_within(&self, uv: [f64; 2], reach: f64) -> Result<[bool; 2], EvalError> {
        Ok(self.collapse(uv, reach)?.flags())
    }
}

/// A tensor spline's rows (`rows[i][j]`, i along u) as the evaluator's u-fastest array.
fn nurbs(
    u_knots: &[f64],
    v_knots: &[f64],
    rows: &[Vec<[f64; 4]>],
) -> Result<EvalSurface, EvalError> {
    let (nu, nv) = (rows.len(), rows.first().map_or(0, Vec::len));
    let degree = |k: &[f64], n: usize| {
        k.len()
            .checked_sub(n + 1)
            .ok_or_else(|| EvalError::Invalid("spline degree".into()))
    };
    let (pu, pv) = (degree(u_knots, nu)?, degree(v_knots, nv)?);
    if pu.max(pv) > MAX_SPLINE_DEGREE || pu.min(pv) == 0 {
        return Err(unsupported(format!(
            "a spline surface of degree ({pu}, {pv})"
        )));
    }
    if u_knots.len().max(v_knots.len()) > MAX_EVAL_KNOTS || nu.saturating_mul(nv) > MAX_EVAL_POINTS
    {
        return Err(EvalError::TooLarge(format!(
            "a spline surface of {nu} × {nv} points"
        )));
    }
    let mut ctrl = Vec::with_capacity(nu * nv);
    for j in 0..nv {
        for row in rows {
            ctrl.push(
                *row.get(j)
                    .ok_or_else(|| EvalError::Invalid("ragged surface".into()))?,
            );
        }
    }
    Ok(EvalSurface::nurbs(
        [pu, pv],
        [u_knots.to_vec(), v_knots.to_vec()],
        [nu, nv],
        ctrl,
    ))
}

/// Four-point Gauss–Legendre nodes and weights on `[−1, 1]`.
const GAUSS4: [(f64, f64); 4] = [
    (-0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
    (-0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
];

/// Pieces each pcurve is integrated in, and how many Gauss panels cover a v range.
const FLUX_PIECES: usize = 32;
const FLUX_PANELS: usize = 4;

/// The flux of `field` through a face, `∫∫ field(S)·(S_u × S_v) du dv` over the (u, v) region its
/// loops (record convention, each use its pcurve and direction) bound, by Green's theorem:
/// `∮ −G du` with `G(u, v) = ∫ field·N ds` from `v0` to `v`. A quadrature for the sign of a
/// shell's volume (`field = p / 3`) or a winding number, which only has to be right to well
/// under a half: no geometry is made from it. Loops round a periodic direction need no seam
/// edge, as a seam's two runs at constant u add nothing to `∮ G du`.
pub fn face_flux(
    s: &SurfaceEval,
    v0: f64,
    loops: &[Vec<(&Curve2, bool)>],
    field: &dyn Fn(Vec3) -> Vec3,
) -> Option<f64> {
    let g = |u: f64, v: f64| -> Option<f64> {
        let (mut sum, h) = (0.0, (v - v0) / FLUX_PANELS as f64);
        for panel in 0..FLUX_PANELS {
            let a = v0 + h * panel as f64;
            for (x, w) in GAUSS4 {
                let t = a + 0.5 * h * (x + 1.0);
                let (p, su, sv) = s.derivs(u, t)?;
                sum += 0.5 * h * w * field(p).dot(su.cross(sv));
            }
        }
        sum.is_finite().then_some(sum)
    };
    let mut total = 0.0;
    for (c, forward) in loops.iter().flatten() {
        let [t0, t1] = c.interval()?;
        let at = |k: f64| {
            let f = k / FLUX_PIECES as f64;
            // Exactly the ends at f = 0 and 1, whatever the interval's rounding.
            c.eval(if *forward {
                t0 * (1.0 - f) + t1 * f
            } else {
                t1 * (1.0 - f) + t0 * f
            })
        };
        for k in 0..FLUX_PIECES {
            let (a, m, z) = (at(k as f64)?, at(k as f64 + 0.5)?, at(k as f64 + 1.0)?);
            total -= g(m[0], m[1])? * (z[0] - a[0]);
        }
    }
    total.is_finite().then_some(total)
}
