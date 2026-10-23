//! The NURBS forms of the record's analytic curves and surfaces: the exact B-spline each one is
//! (docs/native-brep.md, "Surfaces"), built the way SMLib holds its own, so that a circle and every
//! face it bounds share one parameterisation and their pcurves are exact by construction.
//!
//! Angles are on rational quadratic quarter arcs laid on their frame's quarter grid: quarter `q`
//! runs from `q·π/2` to `(q+1)·π/2` from the frame's x axis, and the form parameter of an angle
//! `θ` in it is `q + u`, `u` the arc's own parameter (closed form, [`quarter_u`]). Every form on one
//! frame therefore has the same parameter at the same angle, whatever quarters it spans: a circle
//! edge's form and its cylinder's form agree along it, so its pcurve is a straight line in (u, v).
//! A form spans the quarters its interval needs (at most five for one turn), so an arc across the
//! frame's x axis has a form like any other.
//!
//! A circle or ellipse is the arcs (an ellipse scaled by its radii, which keeps it exact: its
//! parameter is the eccentric angle). A cylinder or cone is the arcs times a degree-1 line along the
//! axis; a sphere the arcs times arcs of latitude (from `−π/2` to `π/2`); a torus the arcs times
//! the arcs of its tube. A line is the degree-1 B-spline on its own parameter.

use super::{Curve, Frame, Surface};
use crate::Vec3;
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_8, TAU};

/// Rounding room on an angle at a quarter's end.
const SLACK: f64 = 1e-12;

/// The quarter arc's parameter `u` in `[0, 1]` at angle `ψ` in `[0, π/2]` from its start: the
/// arc with ends `±π/4` from its bisector and middle weight `cos π/4` has
/// `tan((ψ − π/4)/2) = tan(π/8)(2u − 1)`.
fn quarter_u(psi: f64) -> f64 {
    0.5 * (1.0 + (0.5 * psi - FRAC_PI_8).tan() / FRAC_PI_8.tan())
}

fn quarter_psi(u: f64) -> f64 {
    2.0 * (((2.0 * u - 1.0) * FRAC_PI_8.tan()).atan() + FRAC_PI_8)
}

/// The quarter an angle lies in (an angle on a quarter's end lies in the one after it).
fn quarter_of(theta: f64) -> f64 {
    (theta / FRAC_PI_2 + SLACK).floor()
}

/// The form parameter of angle `theta`: its quarter plus the arc's parameter there.
pub fn param(theta: f64) -> f64 {
    let q = quarter_of(theta);
    q + quarter_u((theta - q * FRAC_PI_2).clamp(0.0, FRAC_PI_2))
}

/// The angle at form parameter `s`.
pub fn angle(s: f64) -> f64 {
    let q = s.floor();
    q * FRAC_PI_2 + quarter_psi(s - q)
}

/// The quarters an angle interval needs: the first one and how many.
fn quarters([a0, a1]: [f64; 2]) -> Option<(f64, usize)> {
    if !(a0.is_finite() && a1.is_finite() && a1 > a0 && a1 - a0 <= TAU * (1.0 + 1e-9)) {
        return None;
    }
    let q0 = quarter_of(a0);
    let q1 = (a1 / FRAC_PI_2 - SLACK).ceil();
    let n = (q1 - q0).max(1.0) as usize;
    (n <= 5).then_some((q0, n))
}

/// Quarter arcs from quarter `q0`, `n` of them: their knot vector (in form parameter) and their
/// control points (as `(cos, sin)` factors on the unit circle's plane) with weights.
fn arcs(q0: f64, n: usize) -> (Vec<f64>, Vec<([f64; 2], f64)>) {
    let at = |a: f64| [a.cos(), a.sin()];
    let mut pts = Vec::with_capacity(2 * n + 1);
    let mut knots = vec![q0; 3];
    for k in 0..n {
        let q = q0 + k as f64;
        let (p, r) = (at(q * FRAC_PI_2), at((q + 1.0) * FRAC_PI_2));
        if k == 0 {
            pts.push((p, 1.0));
        }
        pts.push(([p[0] + r[0], p[1] + r[1]], FRAC_1_SQRT_2));
        pts.push((r, 1.0));
        knots.extend(if k + 1 < n { vec![q + 1.0; 2] } else { vec![q + 1.0; 3] });
    }
    (knots, pts)
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn hom(p: Vec3, w: f64) -> [f64; 4] {
    [p.x * w, p.y * w, p.z * w, w]
}

/// The frame's origin and unit axes (z = x × y).
fn axes(f: &Frame) -> Option<(Vec3, Vec3, Vec3, Vec3)> {
    let (x, y) = (v3(f.x_axis).normalized()?, v3(f.y_axis).normalized()?);
    Some((v3(f.origin), x, y, x.cross(y).normalized()?))
}

/// The NURBS form of a line, circle or ellipse edge and its interval in the form's parameter;
/// `None` for another kind, or an arc of more than one turn.
pub fn curve(c: &Curve, [t0, t1]: [f64; 2]) -> Option<(Curve, [f64; 2])> {
    match c {
        Curve::Line { start, end } => {
            let (a, d) = (v3(*start), v3(*end) - v3(*start));
            let (p0, p1) = (a + d * t0, a + d * t1);
            Some((Curve::Spline { knots: vec![t0, t0, t1, t1], points: vec![[p0.x, p0.y, p0.z], [p1.x, p1.y, p1.z]] }, [t0, t1]))
        }
        Curve::Circle { frame, radius } => conic(frame, [*radius; 2], [t0, t1]),
        Curve::Ellipse { frame, radii } => conic(frame, *radii, [t0, t1]),
        _ => None,
    }
}

fn conic(frame: &Frame, [rx, ry]: [f64; 2], t: [f64; 2]) -> Option<(Curve, [f64; 2])> {
    let (o, x, y, _) = axes(frame)?;
    let (q0, n) = quarters(t)?;
    let (knots, pts) = arcs(q0, n);
    let homogeneous = pts.iter().map(|([c, s], w)| hom(o + x * (rx * c) + y * (ry * s), *w)).collect();
    Some((Curve::RationalSpline { knots, homogeneous }, [param(t[0]), param(t[1])]))
}

/// The NURBS form of a cylinder, cone, sphere or torus face over `domain` (in the label's
/// parameters: angles, and the height along the axis of a cylinder or cone) and the domain in the
/// form's parameters; `None` for another kind, or a domain of more than one turn.
pub fn surface(s: &Surface, [u, v]: [[f64; 2]; 2]) -> Option<(Surface, [[f64; 2]; 2])> {
    let (q0, n) = quarters(u)?;
    let (u_knots, ring) = arcs(q0, n);
    let fu = [param(u[0]), param(u[1])];
    // The ring of arcs times a profile `(control, weight)` mapped to (radial, axial) about the
    // frame: a rational tensor product, its weights the products.
    let net = |f: &Frame, profile: &[([f64; 2], f64)], at: &dyn Fn([f64; 2]) -> (f64, f64)| -> Option<Vec<Vec<[f64; 4]>>> {
        let (o, x, y, z) = axes(f)?;
        Some(
            ring.iter()
                .map(|([c, s], wu)| {
                    let r = x * *c + y * *s;
                    profile
                        .iter()
                        .map(|(q, wv)| {
                            let (radial, axial) = at(*q);
                            hom(o + r * radial + z * axial, wu * wv)
                        })
                        .collect()
                })
                .collect(),
        )
    };
    let line = [([v[0], 0.0], 1.0), ([v[1], 0.0], 1.0)];
    match s {
        Surface::Cylinder { frame, radius } => {
            let homogeneous = net(frame, &line, &|[h, _]| (*radius, h))?;
            Some((Surface::RationalSpline { u_knots, v_knots: vec![v[0], v[0], v[1], v[1]], homogeneous }, [fu, v]))
        }
        Surface::Cone { frame, radius_at_origin, half_angle } => {
            let slope = half_angle.tan();
            let homogeneous = net(frame, &line, &|[h, _]| (radius_at_origin + h * slope, h))?;
            Some((Surface::RationalSpline { u_knots, v_knots: vec![v[0], v[0], v[1], v[1]], homogeneous }, [fu, v]))
        }
        Surface::Sphere { frame, radius } => {
            let (p0, m) = quarters(v)?;
            let (v_knots, meridian) = arcs(p0, m);
            let homogeneous = net(frame, &meridian, &|[c, s]| (radius * c, radius * s))?;
            Some((Surface::RationalSpline { u_knots, v_knots, homogeneous }, [fu, [param(v[0]), param(v[1])]]))
        }
        Surface::Torus { frame, major_radius, minor_radius } => {
            let (p0, m) = quarters(v)?;
            let (v_knots, tube) = arcs(p0, m);
            let homogeneous = net(frame, &tube, &|[c, s]| (major_radius + minor_radius * c, minor_radius * s))?;
            Some((Surface::RationalSpline { u_knots, v_knots, homogeneous }, [fu, [param(v[0]), param(v[1])]]))
        }
        _ => None,
    }
}
