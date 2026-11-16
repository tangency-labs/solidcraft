//! What a record's labels mean in their own parameters, computed: the natural (u, v) ranges of
//! a surface as it is named (a cone's apex), an edge's interval on its circle or ellipse, and
//! whether a face's NURBS form turns its natural normal against its surface's. The record holds
//! these as data; working them out is the kernel's.
//!
//! Origin: SolidCraft's `crates/geom/src/brep/label.rs` (MIT OR Apache-2.0), moved here with its
//! history (RFC 0003). It takes the kernel's evaluation types; the record's edges and faces are
//! read by `solidcore-ops`' `label`, which converts them.
use super::vec::{v3, VecExt};
use super::{Curve, CurveEval, Frame, Surface, SurfaceEval};

/// A parameter's natural range: periodic with period 2π, or between two bounds (infinite for
/// an unbounded direction). The record's `Natural`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Natural {
    /// Periodic, period 2π.
    Periodic,
    /// Between the two bounds.
    Range(f64, f64),
}
/// The natural range of an unbounded direction.
pub const UNBOUNDED: Natural = Natural::Range(f64::NEG_INFINITY, f64::INFINITY);

/// The active interval of a spline's knots: `[knots[degree], knots[count]]` for `count` control
/// points.
#[must_use]
pub fn knot_domain(k: &[f64], count: usize) -> Natural {
    let degree = k.len().saturating_sub(count + 1);
    match (k.get(degree), k.get(count)) {
        (Some(a), Some(b)) => Natural::Range(*a, *b),
        _ => Natural::Range(0., 0.),
    }
}

/// The range of a curve's `t`.
#[must_use]
pub fn curve_natural(c: &Curve) -> Natural {
    match c {
        Curve::Line { .. } => UNBOUNDED,
        Curve::Circle { .. } | Curve::Ellipse { .. } => Natural::Periodic,
        Curve::Polyline { points } => Natural::Range(0., points.len().saturating_sub(1) as f64),
        Curve::Spline { knots, points } => knot_domain(knots, points.len()),
        Curve::RationalSpline { knots, homogeneous } => knot_domain(knots, homogeneous.len()),
        Curve::SurfaceIntersection { leader, .. } => curve_natural(leader),
    }
}

/// The ranges of u and v of `s` as it is named (its own (u, v), not its NURBS form's). Call on a
/// validated record.
#[must_use]
pub fn surface_natural(s: &Surface) -> [Natural; 2] {
    use std::f64::consts::FRAC_PI_2;
    match s {
        // A ball patch is a label: the face's (u, v) are its NURBS form's.
        Surface::Plane { .. } | Surface::PlaneThrough { .. } | Surface::BallPatch { .. } => {
            [UNBOUNDED; 2]
        }
        Surface::Cylinder { .. } => [Natural::Periodic, UNBOUNDED],
        Surface::Cone {
            radius_at_origin,
            half_angle,
            ..
        } => {
            let apex = -radius_at_origin / half_angle.tan();
            let v = if *half_angle > 0. {
                Natural::Range(apex, f64::INFINITY)
            } else {
                Natural::Range(f64::NEG_INFINITY, apex)
            };
            [Natural::Periodic, v]
        }
        Surface::Sphere { .. } => [Natural::Periodic, Natural::Range(-FRAC_PI_2, FRAC_PI_2)],
        Surface::Torus { .. } => [Natural::Periodic; 2],
        Surface::Spline {
            u_knots,
            v_knots,
            points,
        } => [
            knot_domain(u_knots, points.len()),
            knot_domain(v_knots, points.first().map_or(0, Vec::len)),
        ],
        Surface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous,
        } => [
            knot_domain(u_knots, homogeneous.len()),
            knot_domain(v_knots, homogeneous.first().map_or(0, Vec::len)),
        ],
        Surface::PlacedRevolution {
            profile,
            forward: true,
            ..
        } => [curve_natural(profile), Natural::Periodic],
        Surface::PlacedRevolution {
            profile,
            forward: false,
            ..
        } => [Natural::Periodic, curve_natural(profile)],
    }
}

/// The edge's interval on its `curve` itself, for code that reads what the edge is (a circle's
/// angles), given by its parts: its `curve`, its NURBS form and its `interval` on that form. Its
/// `interval` when it has no NURBS form; otherwise the angles of its ends on the NURBS, unwrapped
/// forward along the edge (a whole turn for a closed edge). A form's circle or ellipse has its
/// frame turned so its angle grows along the NURBS (the SMLib conversion makes it so). `None`
/// where the NURBS can't be evaluated.
#[must_use]
pub fn curve_interval(
    curve: &Curve,
    nurbs: Option<&Curve>,
    interval: [f64; 2],
) -> Option<[f64; 2]> {
    use std::f64::consts::TAU;
    let Some(n) = nurbs else {
        return Some(interval);
    };
    let (frame, radii) = match curve {
        Curve::Circle { frame, radius } => (frame, [*radius, *radius]),
        Curve::Ellipse { frame, radii } => (frame, *radii),
        _ => return Some(interval),
    };
    let eval = CurveEval::new(n).ok()?;
    let [t0, t1] = interval;
    let o = v3(frame.origin);
    let (x, y) = (v3(frame.x_axis), v3(frame.y_axis));
    let angle = |t: f64| {
        eval.point(t).map(|p| {
            ((p - o).dot(y) / radii[1])
                .atan2((p - o).dot(x) / radii[0])
                .rem_euclid(TAU)
        })
    };
    let (a0, a1) = (angle(t0)?, angle(t1)?);
    let sweep = (a1 - a0).rem_euclid(TAU);
    Some([a0, a0 + if sweep <= 1e-12 { TAU } else { sweep }])
}

/// Whether a face's NURBS form (`nurbs`) turns its natural normal against `surface`'s (away from
/// the axis of a cylinder or cone, the centre of a sphere, the tube's centre circle of a torus),
/// at the middle of the face's `domain`. A shell's `forward` is about the face's parameters, so
/// the side `surface` is used on is `forward != form_turns(..)`. `Some(false)` without a form;
/// `None` where the form does not evaluate there.
#[must_use]
pub fn form_turns(
    surface: &Surface,
    nurbs: Option<&Surface>,
    domain: [[f64; 2]; 2],
) -> Option<bool> {
    let Some(form) = nurbs else {
        return Some(false);
    };
    let eval = SurfaceEval::new(form).ok()?;
    let [[u0, u1], [v0, v1]] = domain;
    let (p, su, sv) = eval.derivs(0.5 * (u0 + u1), 0.5 * (v0 + v1))?;
    let radial = |f: &Frame| {
        let (o, z) = (v3(f.origin), v3(f.x_axis).cross(v3(f.y_axis)));
        let d = p - o;
        (o, d - z * (d.dot(z) / z.dot(z)))
    };
    let outward = match surface {
        Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } => radial(frame).1,
        Surface::Sphere { frame, .. } => p - v3(frame.origin),
        Surface::BallPatch { center, .. } => p - v3(*center),
        Surface::Torus {
            frame,
            major_radius,
            ..
        } => {
            let (o, r) = radial(frame);
            p - (o + r.normalized()? * *major_radius)
        }
        _ => return None,
    };
    let along = su.cross(sv).dot(outward);
    (along.abs() > 0.0).then_some(along < 0.0)
}
