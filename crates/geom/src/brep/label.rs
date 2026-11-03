//! What a record's labels mean in their own parameters, computed: the natural (u, v) ranges of
//! a surface as it is named (a cone's apex), an edge's interval on its circle or ellipse, and
//! whether a face's NURBS form turns its natural normal against its surface's. The record holds
//! these as data; working them out is the kernel's.
use solidcore_record::brep::{Curve, Edge, Face, Frame, Natural, Surface, UNBOUNDED, knot_domain};

use super::eval;

/// The ranges of u and v of `s` as it is named (its own (u, v), not its NURBS form's). Call on a
/// validated record.
pub fn surface_natural(s: &Surface) -> [Natural; 2] {
    use std::f64::consts::FRAC_PI_2;
    match s {
        Surface::Plane { .. } | Surface::PlaneThrough { .. } => [UNBOUNDED; 2],
        Surface::Cylinder { .. } => [Natural::Periodic, UNBOUNDED],
        Surface::Cone { radius_at_origin, half_angle, .. } => {
            let apex = -radius_at_origin / half_angle.tan();
            let v = if *half_angle > 0. { Natural::Range(apex, f64::INFINITY) } else { Natural::Range(f64::NEG_INFINITY, apex) };
            [Natural::Periodic, v]
        }
        Surface::Sphere { .. } => [Natural::Periodic, Natural::Range(-FRAC_PI_2, FRAC_PI_2)],
        Surface::Torus { .. } => [Natural::Periodic; 2],
        // A label: the face's (u, v) are its NURBS form's.
        Surface::BallPatch { .. } => [UNBOUNDED; 2],
        Surface::Spline { u_knots, v_knots, points } => {
            [knot_domain(u_knots, points.len()), knot_domain(v_knots, points.first().map_or(0, Vec::len))]
        }
        Surface::RationalSpline { u_knots, v_knots, homogeneous } => {
            [knot_domain(u_knots, homogeneous.len()), knot_domain(v_knots, homogeneous.first().map_or(0, Vec::len))]
        }
        Surface::PlacedRevolution { profile, forward: true, .. } => [profile.natural(), Natural::Periodic],
        Surface::PlacedRevolution { profile, forward: false, .. } => [Natural::Periodic, profile.natural()],
    }
}

/// The edge's interval on its `curve` itself, for code that reads what the edge is (a circle's
/// angles): its `interval` when it has no NURBS form; otherwise the angles of its ends on the
/// NURBS, unwrapped forward along the edge (a whole turn for a closed edge). A form's circle or
/// ellipse has its frame turned so its angle grows along the NURBS (the SMLib conversion makes
/// it so). `None` where the NURBS can't be evaluated.
pub fn edge_curve_interval(edge: &Edge) -> Option<[f64; 2]> {
    curve_interval(&edge.curve, edge.nurbs.as_ref(), edge.interval)
}

/// [`edge_curve_interval`] of an edge given by its parts: its `curve`, its NURBS form and its
/// interval on that form.
pub fn curve_interval(curve: &Curve, nurbs: Option<&Curve>, interval: [f64; 2]) -> Option<[f64; 2]> {
    use std::f64::consts::TAU;
    let Some(n) = nurbs else { return Some(interval) };
    let (frame, radii) = match curve {
        Curve::Circle { frame, radius } => (frame, [*radius, *radius]),
        Curve::Ellipse { frame, radii } => (frame, *radii),
        _ => return Some(interval),
    };
    let eval = eval::CurveEval::new(n).ok()?;
    let [t0, t1] = interval;
    let o = crate::Vec3::new(frame.origin[0], frame.origin[1], frame.origin[2]);
    let (x, y) = (crate::Vec3::from(frame.x_axis), crate::Vec3::from(frame.y_axis));
    let angle = |t: f64| eval.point(t).map(|p| ((p - o).dot(y) / radii[1]).atan2((p - o).dot(x) / radii[0]).rem_euclid(TAU));
    let (a0, a1) = (angle(t0)?, angle(t1)?);
    let sweep = (a1 - a0).rem_euclid(TAU);
    Some([a0, a0 + if sweep <= 1e-12 { TAU } else { sweep }])
}

/// Whether the face's NURBS form turns its natural normal against `surface`'s (away from the
/// axis of a cylinder or cone, the centre of a sphere, the tube's centre circle of a torus), at
/// the middle of `domain`. A shell's `forward` is about [`Face::parameters`], so the side
/// `surface` is used on is `forward != form_turns(face)`. `Some(false)` without a form; `None`
/// where the form does not evaluate there.
pub fn form_turns(face: &Face) -> Option<bool> {
    let Some(form) = &face.nurbs else { return Some(false) };
    let eval = eval::SurfaceEval::new(form).ok()?;
    let [[u0, u1], [v0, v1]] = face.domain;
    let (p, su, sv) = eval.derivs(0.5 * (u0 + u1), 0.5 * (v0 + v1))?;
    let v3 = |a: [f64; 3]| crate::Vec3::new(a[0], a[1], a[2]);
    let radial = |f: &Frame| {
        let (o, z) = (v3(f.origin), v3(f.x_axis).cross(v3(f.y_axis)));
        let d = p - o;
        (o, d - z * (d.dot(z) / z.dot(z)))
    };
    let outward = match &face.surface {
        Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } => radial(frame).1,
        Surface::Sphere { frame, .. } => p - v3(frame.origin),
        Surface::BallPatch { center, .. } => p - v3(*center),
        Surface::Torus { frame, major_radius, .. } => {
            let (o, r) = radial(frame);
            p - (o + r.normalized()? * *major_radius)
        }
        _ => return None,
    };
    let along = su.cross(sv).dot(outward);
    (along.abs() > 0.0).then_some(along < 0.0)
}
