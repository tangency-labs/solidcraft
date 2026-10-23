//! The NURBS forms of the record's analytic curves and surfaces: the exact B-spline each one is
//! (docs/native-brep.md, "Surfaces"), built the way SMLib holds its own, so that a circle and every
//! face it bounds share one parameterisation and their pcurves are exact by construction.
//!
//! A circle or ellipse is four rational quadratic quarter arcs from its frame's x axis, on
//! `[0, 4]`; the angle `θ` and the form's parameter `s` are related in closed form within each
//! quarter. A cylinder or cone is that circle times a degree-1 line along its axis; a sphere the
//! circle times a half circle of latitude (two quarters, on `[0, 2]`); a torus the circle times the
//! circle of its tube. A line is the degree-1 B-spline on its own parameter.
//!
//! The form covers one turn from the frame's x axis: an interval or domain outside `[0, 2π]` in
//! angle (an arc across the frame's seam) has no form here and gives `None`.

use super::{Curve, Frame, Surface};
use crate::Vec3;
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_8};

/// Rounding room on an angle at either end of a turn.
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

/// The form parameter of angle `theta` on quarter arcs from `base` (`quarters` of them), `None`
/// outside them.
fn param(theta: f64, base: f64, quarters: usize) -> Option<f64> {
    let span = quarters as f64 * FRAC_PI_2;
    let a = theta - base;
    if !(a >= -SLACK && a <= span + SLACK) {
        return None;
    }
    let a = a.clamp(0.0, span);
    let k = ((a / FRAC_PI_2).floor() as usize).min(quarters - 1);
    Some(k as f64 + quarter_u(a - k as f64 * FRAC_PI_2))
}

/// The angle at form parameter `s` on quarter arcs from `base`.
pub fn angle(s: f64, base: f64, quarters: usize) -> f64 {
    let s = s.clamp(0.0, quarters as f64);
    let k = (s.floor() as usize).min(quarters - 1);
    base + k as f64 * FRAC_PI_2 + quarter_psi(s - k as f64)
}

/// A turn's form parameter at angle `theta` (`[0, 2π]` to `[0, 4]`).
pub fn turn_param(theta: f64) -> Option<f64> {
    param(theta, 0.0, 4)
}

/// The control points (on the unit circle's plane, as `(cos, sin)` factors) and weights of
/// `quarters` quarter arcs from `base`, with their knot vector.
fn arcs(base: f64, quarters: usize) -> (Vec<f64>, Vec<([f64; 2], f64)>) {
    let at = |a: f64| [a.cos(), a.sin()];
    let mut pts = Vec::with_capacity(2 * quarters + 1);
    let mut knots = vec![0.0; 3];
    for k in 0..quarters {
        let (a, b) = (base + k as f64 * FRAC_PI_2, base + (k + 1) as f64 * FRAC_PI_2);
        let (p, q) = (at(a), at(b));
        if k == 0 {
            pts.push((p, 1.0));
        }
        pts.push(([p[0] + q[0], p[1] + q[1]], FRAC_1_SQRT_2));
        pts.push((q, 1.0));
        let end = (k + 1) as f64;
        if k + 1 < quarters {
            knots.extend([end, end]);
        } else {
            knots.extend([end; 3]);
        }
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
/// `None` for another kind, or an arc outside one turn from its frame's x axis.
pub fn curve(c: &Curve, [t0, t1]: [f64; 2]) -> Option<(Curve, [f64; 2])> {
    match c {
        Curve::Line { start, end } => {
            let (a, d) = (v3(*start), v3(*end) - v3(*start));
            let p = |t: f64| a + d * t;
            let (p0, p1) = (p(t0), p(t1));
            Some((Curve::Spline { knots: vec![t0, t0, t1, t1], points: vec![[p0.x, p0.y, p0.z], [p1.x, p1.y, p1.z]] }, [t0, t1]))
        }
        Curve::Circle { frame, radius } => conic(frame, [*radius; 2], [t0, t1]),
        Curve::Ellipse { frame, radii } => conic(frame, *radii, [t0, t1]),
        _ => None,
    }
}

fn conic(frame: &Frame, [rx, ry]: [f64; 2], [t0, t1]: [f64; 2]) -> Option<(Curve, [f64; 2])> {
    let (o, x, y, _) = axes(frame)?;
    let interval = [turn_param(t0)?, turn_param(t1)?];
    let (knots, pts) = arcs(0.0, 4);
    let homogeneous = pts.iter().map(|([c, s], w)| hom(o + x * (rx * c) + y * (ry * s), *w)).collect();
    Some((Curve::RationalSpline { knots, homogeneous }, interval))
}

/// The NURBS form of a cylinder, cone, sphere or torus face and its domain in the form's
/// parameters; `None` for another kind, or a domain outside one turn.
pub fn surface(s: &Surface, [[u0, u1], [v0, v1]]: [[f64; 2]; 2]) -> Option<(Surface, [[f64; 2]; 2])> {
    let u = [turn_param(u0)?, turn_param(u1)?];
    let (u_knots, ring) = arcs(0.0, 4);
    // Points of the u ring times a v profile `(radial, axial, weight)` about the frame.
    let net = |o: Vec3, x: Vec3, y: Vec3, z: Vec3, profile: &[([f64; 2], f64)], at: &dyn Fn([f64; 2]) -> (f64, f64)| -> Vec<Vec<[f64; 4]>> {
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
            .collect()
    };
    match s {
        Surface::Cylinder { frame, radius } => {
            let (o, x, y, z) = axes(frame)?;
            let profile = [([v0, 0.0], 1.0), ([v1, 0.0], 1.0)];
            let homogeneous = net(o, x, y, z, &profile, &|[h, _]| (*radius, h));
            Some((Surface::RationalSpline { u_knots, v_knots: vec![v0, v0, v1, v1], homogeneous }, [u, [v0, v1]]))
        }
        Surface::Cone { frame, radius_at_origin, half_angle } => {
            let (o, x, y, z) = axes(frame)?;
            let slope = half_angle.tan();
            let profile = [([v0, 0.0], 1.0), ([v1, 0.0], 1.0)];
            let homogeneous = net(o, x, y, z, &profile, &|[h, _]| (radius_at_origin + h * slope, h));
            Some((Surface::RationalSpline { u_knots, v_knots: vec![v0, v0, v1, v1], homogeneous }, [u, [v0, v1]]))
        }
        Surface::Sphere { frame, radius } => {
            let (o, x, y, z) = axes(frame)?;
            let v = [param(v0, -FRAC_PI_2, 2)?, param(v1, -FRAC_PI_2, 2)?];
            let (v_knots, meridian) = arcs(-FRAC_PI_2, 2);
            let homogeneous = net(o, x, y, z, &meridian, &|[c, s]| (radius * c, radius * s));
            Some((Surface::RationalSpline { u_knots, v_knots, homogeneous }, [u, v]))
        }
        Surface::Torus { frame, major_radius, minor_radius } => {
            let (o, x, y, z) = axes(frame)?;
            let v = [turn_param(v0)?, turn_param(v1)?];
            let (v_knots, tube) = arcs(0.0, 4);
            let homogeneous = net(o, x, y, z, &tube, &|[c, s]| (major_radius + minor_radius * c, minor_radius * s));
            Some((Surface::RationalSpline { u_knots, v_knots, homogeneous }, [u, v]))
        }
        _ => None,
    }
}

/// The angle on a turn at form parameter `s` (`[0, 4]` to `[0, 2π]`).
pub fn turn_angle(s: f64) -> f64 {
    angle(s, 0.0, 4)
}
