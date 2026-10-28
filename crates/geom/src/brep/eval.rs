//! Points of curve and surface records, in the records' own parameters (docs/native-brep.md,
//! "Geometry"), and the parameters of a point on a surface. Planes, splines and revolutions go
//! through the display evaluator ([`EvalSurface`], [`EvalCurve`], [`uv`]), which parameterises
//! them as their producer does; cylinders, cones, spheres and tori are evaluated and inverted in
//! closed form, in angle and height. A `surface_intersection` is the point of both surfaces in
//! the plane through its leader's point normal to the leader there, which truck's `subs`
//! converges to (within its 10⁻⁶ mm), found by Newton's method on the three planes.
//!
//! Call on validated records. Evaluation of a well-formed record never panics: a point that
//! cannot be had (a vanishing weight, a Newton step that does not converge) is `None`.
use super::pcurve::Error;
use super::{Curve, Frame, Surface};
use crate::Vec3;
use crate::bspline::MAX_SPLINE_DEGREE;
use crate::tess::partials::{Partials, tabulate, trig_derivative};
use crate::tess::uv::{self, Domain};
use crate::tess::{EvalCurve, EvalSurface, MAX_EVAL_KNOTS, MAX_EVAL_POINTS};
use std::f64::consts::{FRAC_PI_2, TAU};

/// Newton steps of one intersection point.
const MAX_NEWTON: usize = 64;

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}
fn unsupported(what: impl std::fmt::Display) -> Error {
    Error::Failed(format!("not supported yet: {what}"))
}

/// A frame's origin and axes, z = x × y.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axes {
    pub(crate) o: Vec3,
    pub(crate) x: Vec3,
    pub(crate) y: Vec3,
    pub(crate) z: Vec3,
}
impl Axes {
    pub(crate) fn new(f: &Frame) -> Self {
        let (x, y) = (v3(f.x_axis), v3(f.y_axis));
        Axes { o: v3(f.origin), x, y, z: x.cross(y) }
    }
    /// `p` in the frame: (x, y, z) coordinates.
    pub(crate) fn local(&self, p: Vec3) -> [f64; 3] {
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
    if u < 0.0 { u + TAU } else { u }
}

/// A surface record ready to evaluate.
#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceEval {
    /// A plane, spline or revolution, on the display evaluator.
    Eval(EvalSurface, Domain),
    Cylinder {
        axes: Axes,
        radius: f64,
    },
    Cone {
        axes: Axes,
        radius_at_origin: f64,
        slope: f64,
    },
    Sphere {
        axes: Axes,
        radius: f64,
    },
    Torus {
        axes: Axes,
        major: f64,
        minor: f64,
    },
}

impl SurfaceEval {
    pub fn new(s: &Surface) -> Result<Self, Error> {
        let eval = |e: EvalSurface| {
            let dom = uv::domain(&e);
            Ok(SurfaceEval::Eval(e, dom))
        };
        match s {
            Surface::Plane { origin, u_axis, v_axis } => eval(EvalSurface::Plane { origin: v3(*origin), du: v3(*u_axis), dv: v3(*v_axis) }),
            Surface::PlaneThrough { origin, u_point, v_point } => {
                let o = v3(*origin);
                eval(EvalSurface::Plane { origin: o, du: v3(*u_point) - o, dv: v3(*v_point) - o })
            }
            Surface::Cylinder { frame, radius } => Ok(SurfaceEval::Cylinder { axes: Axes::new(frame), radius: *radius }),
            Surface::Cone { frame, radius_at_origin, half_angle } => {
                Ok(SurfaceEval::Cone { axes: Axes::new(frame), radius_at_origin: *radius_at_origin, slope: half_angle.tan() })
            }
            Surface::Sphere { frame, radius } => Ok(SurfaceEval::Sphere { axes: Axes::new(frame), radius: *radius }),
            Surface::Torus { frame, major_radius, minor_radius } => {
                Ok(SurfaceEval::Torus { axes: Axes::new(frame), major: *major_radius, minor: *minor_radius })
            }
            Surface::Spline { u_knots, v_knots, points } => {
                let rows: Vec<Vec<[f64; 4]>> = points.iter().map(|r| r.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect()).collect();
                eval(nurbs(u_knots, v_knots, &rows)?)
            }
            Surface::RationalSpline { u_knots, v_knots, homogeneous } => eval(nurbs(u_knots, v_knots, homogeneous)?),
            Surface::PlacedRevolution { profile, origin, axis, matrix, forward } => {
                let profile = match CurveEval::new(profile)? {
                    CurveEval::Spline(c) => c,
                    CurveEval::Line { start, end } => EvalCurve {
                        degree: 1,
                        knots: vec![0.0, 0.0, 1.0, 1.0],
                        ctrl: vec![[start.x, start.y, start.z, 1.0], [end.x, end.y, end.z, 1.0]],
                    },
                    _ => return Err(unsupported("a revolution of a conic, polyline or intersection profile")),
                };
                let axis = v3(*axis).normalized().ok_or_else(|| Error::Invalid("revolution axis".into()))?;
                // Column-major: matrix[column][row].
                let transform = [0, 1, 2].map(|r| [matrix[0][r], matrix[1][r], matrix[2][r], matrix[3][r]]);
                eval(EvalSurface::Revolved { profile, origin: v3(*origin), axis, transform, swap: !forward })
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
        Domain { range: [ru, rv], period: [pu, pv] }
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
            SurfaceEval::Cone { axes: a, radius_at_origin, slope } => {
                let (r, dr) = a.radial(u);
                let rv = radius_at_origin + v * slope;
                (a.o + a.z * v + r * rv, dr * rv, a.z + r * *slope)
            }
            SurfaceEval::Sphere { axes: a, radius } => {
                let (r, dr) = a.radial(u);
                let (s, c) = v.sin_cos();
                (a.o + (r * c + a.z * s) * *radius, dr * (c * radius), (a.z * c - r * s) * *radius)
            }
            SurfaceEval::Torus { axes: a, major, minor } => {
                let (r, dr) = a.radial(u);
                let (s, c) = v.sin_cos();
                let ring = major + minor * c;
                (a.o + r * ring + a.z * (minor * s), dr * ring, (a.z * c - r * s) * *minor)
            }
        })
    }

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
            SurfaceEval::Cylinder { axes: a, radius } => (a.radial(u).0 * -*radius, Vec3::ZERO, Vec3::ZERO),
            SurfaceEval::Cone { axes: a, radius_at_origin, slope } => {
                let (r, dr) = a.radial(u);
                (r * -(radius_at_origin + v * slope), dr * *slope, Vec3::ZERO)
            }
            SurfaceEval::Sphere { axes: a, radius } => {
                let (r, dr) = a.radial(u);
                let (s, c) = v.sin_cos();
                (r * (-c * radius), dr * (-s * radius), (r * c + a.z * s) * -*radius)
            }
            SurfaceEval::Torus { axes: a, major, minor } => {
                let (r, dr) = a.radial(u);
                let (s, c) = v.sin_cos();
                (r * -(major + minor * c), dr * (-s * minor), (r * c + a.z * s) * -*minor)
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
        let lift = |a: &Axes, i: usize, j: usize, x: Vec3| if i == 0 && j == 0 { a.o + x } else { x };
        match self {
            SurfaceEval::Eval(e, _) => e.partials(u, v),
            SurfaceEval::Cylinder { axes: a, radius } => tabulate(|i, j| {
                let height = match j {
                    0 if i == 0 => a.z * v,
                    1 if i == 0 => a.z,
                    _ => Vec3::ZERO,
                };
                let ring = if j == 0 { radial(a, i) * *radius } else { Vec3::ZERO };
                Some(lift(a, i, j, ring + height))
            }),
            SurfaceEval::Cone { axes: a, radius_at_origin, slope } => tabulate(|i, j| {
                let (height, rv) = match j {
                    0 => (if i == 0 { a.z * v } else { Vec3::ZERO }, radius_at_origin + v * slope),
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
            SurfaceEval::Torus { axes: a, major, minor } => tabulate(|i, j| {
                let (c, s) = at_v(j);
                let ring = if j == 0 { major + minor * c } else { minor * c };
                let tube = if i == 0 { a.z * (minor * s) } else { Vec3::ZERO };
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
    pub fn project_with(&self, starts: &uv::Starts, p: Vec3, near: Option<[f64; 2]>, accept: f64) -> Option<[f64; 2]> {
        let q = match self {
            SurfaceEval::Eval(e, dom) => return uv::project_with(e, dom, starts, p, near, accept),
            SurfaceEval::Cylinder { axes, .. } => {
                let [a, b, h] = axes.local(p);
                [angle(a, b, near.map(|n| n[0])), h]
            }
            SurfaceEval::Cone { axes, radius_at_origin, slope } => {
                // The nearest point of the generator line through p's half-plane.
                let [a, b, h] = axes.local(p);
                [angle(a, b, near.map(|n| n[0])), ((a.hypot(b) - radius_at_origin) * slope + h) / (1.0 + slope * slope)]
            }
            SurfaceEval::Sphere { axes, .. } => {
                let [a, b, h] = axes.local(p);
                [angle(a, b, near.map(|n| n[0])), h.atan2(a.hypot(b))]
            }
            SurfaceEval::Torus { axes, major, .. } => {
                let [a, b, h] = axes.local(p);
                let v = h.atan2(a.hypot(b) - major);
                [angle(a, b, near.map(|n| n[0])), if v < 0.0 { v + TAU } else { v }]
            }
        };
        (q[0].is_finite() && q[1].is_finite() && self.point(q).is_some_and(|s| s.dist(p) <= accept)).then_some(q)
    }

    /// Whether moving along each direction at `uv` leaves the point where it is (a pole).
    pub fn pole(&self, uv: [f64; 2]) -> [bool; 2] {
        let Some((_, su, sv)) = self.derivs(uv[0], uv[1]) else { return [false; 2] };
        let (a, b) = (su.len(), sv.len());
        [a <= 1e-9 * b.max(1e-12), b <= 1e-9 * a.max(1e-12)]
    }

    /// [`Self::pole`], or within `reach` mm of one: moving along a direction across half its
    /// period (or its whole range) moves the point no more than `reach` (a boundary point that
    /// lies at a pole to within a tolerance, where its periodic parameter is free).
    pub fn pole_within(&self, uv: [f64; 2], reach: f64) -> [bool; 2] {
        let Some((_, su, sv)) = self.derivs(uv[0], uv[1]) else { return [false; 2] };
        uv::collapsed([su.len(), sv.len()], &self.domain(), reach)
    }
}

/// A tensor spline's rows (`rows[i][j]`, i along u) as the evaluator's u-fastest array.
fn nurbs(u_knots: &[f64], v_knots: &[f64], rows: &[Vec<[f64; 4]>]) -> Result<EvalSurface, Error> {
    let (nu, nv) = (rows.len(), rows.first().map_or(0, Vec::len));
    let degree = |k: &[f64], n: usize| k.len().checked_sub(n + 1).ok_or_else(|| Error::Invalid("spline degree".into()));
    let (pu, pv) = (degree(u_knots, nu)?, degree(v_knots, nv)?);
    if pu.max(pv) > MAX_SPLINE_DEGREE || pu.min(pv) == 0 {
        return Err(unsupported(format!("a spline surface of degree ({pu}, {pv})")));
    }
    if u_knots.len().max(v_knots.len()) > MAX_EVAL_KNOTS || nu.saturating_mul(nv) > MAX_EVAL_POINTS {
        return Err(Error::TooLarge(format!("a spline surface of {nu} × {nv} points")));
    }
    let mut ctrl = Vec::with_capacity(nu * nv);
    for j in 0..nv {
        for row in rows {
            ctrl.push(*row.get(j).ok_or_else(|| Error::Invalid("ragged surface".into()))?);
        }
    }
    Ok(EvalSurface::nurbs([pu, pv], [u_knots.to_vec(), v_knots.to_vec()], [nu, nv], ctrl))
}

/// A curve record ready to evaluate.
#[derive(Clone, Debug, PartialEq)]
pub enum CurveEval {
    Line {
        start: Vec3,
        end: Vec3,
    },
    /// A circle or an ellipse.
    Conic {
        axes: Axes,
        radii: [f64; 2],
    },
    Polyline(Vec<Vec3>),
    Spline(EvalCurve),
    Intersection(Box<Intersection>),
}

/// A `surface_intersection` ready to evaluate.
#[derive(Clone, Debug, PartialEq)]
pub struct Intersection {
    surfaces: [SurfaceEval; 2],
    leader: CurveEval,
}

impl CurveEval {
    pub fn new(c: &Curve) -> Result<Self, Error> {
        Ok(match c {
            Curve::Line { start, end } => CurveEval::Line { start: v3(*start), end: v3(*end) },
            Curve::Circle { frame, radius } => CurveEval::Conic { axes: Axes::new(frame), radii: [*radius; 2] },
            Curve::Ellipse { frame, radii } => CurveEval::Conic { axes: Axes::new(frame), radii: *radii },
            Curve::Polyline { points } => CurveEval::Polyline(points.iter().copied().map(v3).collect()),
            Curve::Spline { knots, points } => spline(knots, points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect())?,
            Curve::RationalSpline { knots, homogeneous } => spline(knots, homogeneous.clone())?,
            Curve::SurfaceIntersection { surfaces, leader, .. } => {
                let leader = CurveEval::new(leader)?;
                if !matches!(leader, CurveEval::Polyline(_) | CurveEval::Spline(_)) {
                    return Err(Error::Invalid("intersection leader".into()));
                }
                CurveEval::Intersection(Box::new(Intersection {
                    surfaces: [SurfaceEval::new(&surfaces[0])?, SurfaceEval::new(&surfaces[1])?],
                    leader,
                }))
            }
        })
    }

    /// The point at `t`.
    pub fn point(&self, t: f64) -> Option<Vec3> {
        if !t.is_finite() {
            return None;
        }
        match self {
            CurveEval::Line { start, end } => Some(*start + (*end - *start) * t),
            CurveEval::Conic { axes: a, radii } => {
                let (s, c) = t.sin_cos();
                Some(a.o + a.x * (radii[0] * c) + a.y * (radii[1] * s))
            }
            CurveEval::Polyline(_) | CurveEval::Spline(_) => self.leader(t).map(|d| d.0),
            CurveEval::Intersection(x) => x.point(t),
        }
    }

    /// Point and derivative of a polyline (truck's: the segment from ⌊t⌋, the last one at the
    /// end) or a spline.
    /// The point and first derivative at `t`; `None` for a traced intersection (its tangent
    /// is not held).
    pub fn derivs(&self, t: f64) -> Option<(Vec3, Vec3)> {
        if !t.is_finite() {
            return None;
        }
        match self {
            CurveEval::Line { start, end } => Some((*start + (*end - *start) * t, *end - *start)),
            CurveEval::Conic { axes: a, radii } => {
                let (s, c) = t.sin_cos();
                Some((a.o + a.x * (radii[0] * c) + a.y * (radii[1] * s), a.y * (radii[1] * c) - a.x * (radii[0] * s)))
            }
            CurveEval::Polyline(_) | CurveEval::Spline(_) => self.leader(t),
            CurveEval::Intersection(_) => None,
        }
    }

    fn leader(&self, t: f64) -> Option<(Vec3, Vec3)> {
        match self {
            CurveEval::Polyline(p) => {
                let n = p.len();
                let last = *p.last()?;
                if t <= 0.0 {
                    return Some((*p.first()?, *p.get(1)? - *p.first()?));
                }
                if t + 1.0 >= n as f64 {
                    return Some((last, last - *p.get(n.checked_sub(2)?)?));
                }
                let i = t as usize;
                let (a, b) = (*p.get(i)?, *p.get(i + 1)?);
                Some((a + (b - a) * (t - i as f64), b - a))
            }
            CurveEval::Spline(c) => c.eval(t),
            _ => None,
        }
    }
}

fn spline(knots: &[f64], ctrl: Vec<[f64; 4]>) -> Result<CurveEval, Error> {
    let degree = knots.len().checked_sub(ctrl.len() + 1).ok_or_else(|| Error::Invalid("spline degree".into()))?;
    if !(1..=MAX_SPLINE_DEGREE).contains(&degree) {
        return Err(unsupported(format!("a spline curve of degree {degree}")));
    }
    Ok(CurveEval::Spline(EvalCurve { degree, knots: knots.to_vec(), ctrl }))
}

impl Intersection {
    /// The point in the leader's normal plane at `t`, found from the leader's point: a function
    /// of `t` alone, whatever was evaluated before (and so the same on every thread).
    fn point(&self, t: f64) -> Option<Vec3> {
        let (lead, along) = self.leader.leader(t)?;
        if !(along.len2() > 0.0) {
            return None;
        }
        self.solve(lead, along)
    }

    /// Newton on the planes: each surface's tangent plane at the foot of the current point, and
    /// the leader's normal plane, until the point stops moving.
    fn solve(&self, lead: Vec3, along: Vec3) -> Option<Vec3> {
        let mut x = lead;
        let mut uv: Option<[[f64; 2]; 2]> = None;
        for _ in 0..MAX_NEWTON {
            let mut planes = [(Vec3::ZERO, 0.0); 2];
            let mut feet = [[0.0; 2]; 2];
            for (i, s) in self.surfaces.iter().enumerate() {
                let q = s.project(x, uv.and_then(|h| h.get(i).copied()), f64::INFINITY)?;
                let (f, su, sv) = s.derivs(q[0], q[1])?;
                let n = su.cross(sv).normalized()?;
                *planes.get_mut(i)? = (n, n.dot(f));
                *feet.get_mut(i)? = q;
            }
            uv = Some(feet);
            let [(n0, d0), (n1, d1)] = planes;
            let y = solve3([n0, n1, along], [d0, d1, along.dot(lead)])?;
            let step = y.dist(x);
            x = y;
            if step <= 1e-12 * (1.0 + x.len()) {
                return Some(x);
            }
        }
        None
    }
}

/// The point on the three planes `rows[i] · x = rhs[i]` (Cramer's rule), `None` when they meet
/// in no single point (tangent surfaces).
fn solve3(rows: [Vec3; 3], rhs: [f64; 3]) -> Option<Vec3> {
    let [a, b, c] = rows;
    let (bc, ca, ab) = (b.cross(c), c.cross(a), a.cross(b));
    let det = a.dot(bc);
    if !(det.abs() > 1e-12 * a.len() * b.len() * c.len()) {
        return None;
    }
    let x = (bc * rhs[0] + ca * rhs[1] + ab * rhs[2]) * (1.0 / det);
    x.is_finite().then_some(x)
}
