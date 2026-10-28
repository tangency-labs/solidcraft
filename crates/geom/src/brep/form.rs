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

/// The quarters an angle interval needs: the first one and how many. An end within `reach` (an
/// angle) of a quarter's end is on it: a face whose boundary lies within its tolerance of a
/// quarter's end needs no part of the quarter past it.
fn quarters([a0, a1]: [f64; 2], reach: f64) -> Option<(f64, usize)> {
    if !(a0.is_finite() && a1.is_finite() && a1 > a0 && a1 - a0 <= TAU * (1.0 + 1e-9)) {
        return None;
    }
    let on_end = |a: f64| {
        let end = (a / FRAC_PI_2).round() * FRAC_PI_2;
        if (a - end).abs() <= reach { end } else { a }
    };
    let (a0, a1) = (on_end(a0), on_end(a1));
    let q0 = quarter_of(a0);
    let q1 = (a1 / FRAC_PI_2 - SLACK).ceil();
    let n = (q1 - q0).max(1.0) as usize;
    (n <= 5).then_some((q0, n))
}

/// A face's quarters along an angle direction and its domain there: a whole turn is exactly
/// four quarters from the one it starts in (the form closes on itself, so it is periodic and the
/// face's seam uses lie a period apart), anything less the quarters it needs ([`quarters`], ends
/// within `reach` of a quarter's end on it).
fn face_quarters(a: [f64; 2], reach: f64) -> Option<((f64, usize), [f64; 2])> {
    if a[1] - a[0] >= TAU * (1.0 - 1e-9) {
        let q0 = quarter_of(a[0]);
        return Some(((q0, 4), [q0, q0 + 4.0]));
    }
    let grid = quarters(a, reach)?;
    Some((grid, params(a, grid)))
}

/// The form parameters of an angle interval on the quarters `quarters` chose for it (its ends kept
/// on them: an end within rounding past a quarter's end stays in that quarter).
fn params([a0, a1]: [f64; 2], (q0, n): (f64, usize)) -> [f64; 2] {
    let end = q0 + n as f64;
    [param(a0).clamp(q0, end), param(a1).clamp(q0, end)]
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
    let grid = quarters(t, 0.0)?;
    let (knots, pts) = arcs(grid.0, grid.1);
    let homogeneous = pts.iter().map(|([c, s], w)| hom(o + x * (rx * c) + y * (ry * s), *w)).collect();
    Some((Curve::RationalSpline { knots, homogeneous }, params(t, grid)))
}

/// The NURBS form of a cylinder, cone, sphere or torus face over `domain` (in the label's
/// parameters: angles, and the height along the axis of a cylinder or cone) and the domain in the
/// form's parameters; `None` for another kind, or a domain of more than one turn.
pub fn surface(s: &Surface, domain: [[f64; 2]; 2]) -> Option<(Surface, [[f64; 2]; 2])> {
    surface_within(s, domain, 0.0)
}

/// [`surface`] for a face whose boundary lies within `tolerance` (mm) of its surface: where the
/// domain ends within `tolerance` of a quarter's end (the angle that far along the widest circle
/// of the face in that direction), the form ends there too. A face's box is the extent of its
/// boundary's points ([`extent`]), which may run past a quarter's end by as much as its edges lie
/// off the surface; the form then covers no quarter for that alone (a torus face of half a tube
/// would otherwise go round the whole tube).
pub fn surface_within(s: &Surface, [u, v]: [[f64; 2]; 2], tolerance: f64) -> Option<(Surface, [[f64; 2]; 2])> {
    // The angle `tolerance` makes on a circle of `radius`.
    let reach = |radius: f64| if tolerance > 0.0 && radius > 0.0 && radius.is_finite() { tolerance / radius } else { 0.0 };
    let (reach_u, reach_v) = match s {
        Surface::Cylinder { radius, .. } => (reach(radius.abs()), 0.0),
        Surface::Cone { radius_at_origin, half_angle, .. } => {
            let slope = half_angle.tan();
            (reach(v.iter().map(|h| (radius_at_origin + h * slope).abs()).fold(0.0, f64::max)), 0.0)
        }
        Surface::Sphere { radius, .. } => (reach(radius.abs()), reach(radius.abs())),
        Surface::Torus { major_radius, minor_radius, .. } => (reach(major_radius.abs() + minor_radius.abs()), reach(minor_radius.abs())),
        _ => (0.0, 0.0),
    };
    let (grid, fu) = face_quarters(u, reach_u)?;
    let (u_knots, ring) = arcs(grid.0, grid.1);
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
            let vgrid = quarters(v, reach_v)?;
            let (v_knots, meridian) = arcs(vgrid.0, vgrid.1);
            let homogeneous = net(frame, &meridian, &|[c, s]| (radius * c, radius * s))?;
            Some((Surface::RationalSpline { u_knots, v_knots, homogeneous }, [fu, params(v, vgrid)]))
        }
        Surface::Torus { frame, major_radius, minor_radius } => {
            let (vgrid, fv) = face_quarters(v, reach_v)?;
            let (v_knots, tube) = arcs(vgrid.0, vgrid.1);
            let homogeneous = net(frame, &tube, &|[c, s]| (major_radius + minor_radius * c, minor_radius * s))?;
            Some((Surface::RationalSpline { u_knots, v_knots, homogeneous }, [fu, fv]))
        }
        Surface::PlacedRevolution { profile, origin, axis, matrix, forward } => {
            // The profile's control points turned about the axis on the quarter grid from angle
            // zero (the profile): (a + ib)(c + is) for each point's radial (a, b) in the frame and
            // each arc control point (c, s), weights multiplied; then the matrix.
            let (angles, along) = if *forward { (v, u) } else { (u, v) };
            let (ring_grid, fa) = face_quarters(angles, 0.0)?;
            let (a_knots, arcs_r) = arcs(ring_grid.0, ring_grid.1);
            let (p_knots, h) = profile_nurbs(profile, Some(along))?;
            let fp = match profile.as_ref() {
                Curve::Circle { .. } | Curve::Ellipse { .. } => curve(profile, along)?.1,
                _ => along,
            };
            let (o, x, y, k) = revolution_frame(&h, *origin, *axis)?;
            let net: Vec<Vec<[f64; 4]>> = arcs_r
                .iter()
                .map(|([c, s], wu)| {
                    h.iter()
                        .map(|hp| {
                            let d = Vec3::new(hp[0] / hp[3], hp[1] / hp[3], hp[2] / hp[3]) - o;
                            let (a, b, z) = (d.dot(x), d.dot(y), d.dot(k));
                            let q = o + k * z + x * (a * c - b * s) + y * (a * s + b * c);
                            hom(affine(matrix, q), wu * hp[3])
                        })
                        .collect()
                })
                .collect();
            if *forward {
                // u the profile, v the angle: the net turned over.
                let n = net.first()?.len();
                let t: Vec<Vec<[f64; 4]>> = (0..n).map(|j| net.iter().filter_map(|r| r.get(j).copied()).collect()).collect();
                Some((Surface::RationalSpline { u_knots: p_knots, v_knots: a_knots, homogeneous: t }, [fp, fa]))
            } else {
                Some((Surface::RationalSpline { u_knots: a_knots, v_knots: p_knots, homogeneous: net }, [fa, fp]))
            }
        }
        _ => None,
    }
}

/// A circle edge labelled on another frame about the same axis: the same circle, its angle
/// measured from `frame`'s x axis (its y axis taken so the circle keeps its own direction), and
/// its interval shifted to match, so its form lies on `frame`'s quarter grid. `None` when the
/// circle is not about `frame`'s axis (centre on it, normal along it, within `tol`).
pub fn on_frame(c: &Curve, [t0, t1]: [f64; 2], frame: &Frame, tol: f64) -> Option<(Curve, [f64; 2])> {
    let Curve::Circle { frame: own, radius } = c else { return None };
    let (o, x, _, z) = axes(own)?;
    let (fo, fx, _, fz) = axes(frame)?;
    let d = o - fo;
    if !((d - fz * d.dot(fz)).len() <= tol && (z.dot(fz).abs() - 1.0).abs() <= 1e-9) {
        return None;
    }
    let nx = (fx - z * fx.dot(z)).normalized()?;
    let ny = z.cross(nx);
    let shift = x.dot(ny).atan2(x.dot(nx));
    let relabel = Frame { origin: [o.x, o.y, o.z], x_axis: [nx.x, nx.y, nx.z], y_axis: [ny.x, ny.y, ny.z] };
    Some((Curve::Circle { frame: relabel, radius: *radius }, [t0 + shift, t1 + shift]))
}

/// The frame of a cylinder, cone, sphere or torus, or of a placed revolution (its axis, angle
/// zero at its profile: [`revolution_frame`]); `None` for another surface.
pub fn frame_of(s: &Surface) -> Option<Frame> {
    match s {
        Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } | Surface::Sphere { frame, .. } | Surface::Torus { frame, .. } => {
            Some(frame.clone())
        }
        Surface::PlacedRevolution { profile, origin, axis, matrix, .. } => {
            let (_, h) = profile_nurbs(profile, None)?;
            let (o, x, y, _) = revolution_frame(&h, *origin, *axis)?;
            let m = |p: Vec3| affine(matrix, p);
            let (mo, mx, my) = (m(o), m(o + x) - m(o), m(o + y) - m(o));
            Some(Frame { origin: [mo.x, mo.y, mo.z], x_axis: [mx.x, mx.y, mx.z], y_axis: [my.x, my.y, my.z] })
        }
        _ => None,
    }
}

/// `matrix` (column-major, `matrix[column][row]`) applied to `p`.
fn affine(matrix: &[[f64; 4]; 4], p: Vec3) -> Vec3 {
    let row = |r: usize| matrix[0][r] * p.x + matrix[1][r] * p.y + matrix[2][r] * p.z + matrix[3][r];
    Vec3::new(row(0), row(1), row(2))
}

/// A revolution's profile as a NURBS: its knots and homogeneous control points (a B-spline or
/// NURBS as it is, a line or conic as its form over `range`; `None` without a range).
fn profile_nurbs(profile: &Curve, range: Option<[f64; 2]>) -> Option<(Vec<f64>, Vec<[f64; 4]>)> {
    let formed;
    let c = match profile {
        Curve::Spline { .. } | Curve::RationalSpline { .. } => profile,
        other => {
            formed = curve(other, range?)?.0;
            &formed
        }
    };
    match c {
        Curve::Spline { knots, points } => Some((knots.clone(), points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect())),
        Curve::RationalSpline { knots, homogeneous } => Some((knots.clone(), homogeneous.clone())),
        _ => None,
    }
}

/// A revolution's frame: the origin and unit axis, x the radial direction of the profile's
/// first control point off the axis (angle zero), y = axis × x.
fn revolution_frame(h: &[[f64; 4]], origin: [f64; 3], axis: [f64; 3]) -> Option<(Vec3, Vec3, Vec3, Vec3)> {
    let (o, k) = (v3(origin), v3(axis).normalized()?);
    let x = h.iter().find_map(|h| {
        let d = Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]) - o;
        let r = d - k * d.dot(k);
        if r.len() > 1e-12 * (1.0 + d.len()) { r.normalized() } else { None }
    })?;
    Some((o, x, k.cross(x), k))
}

/// An edge's label and NURBS form: a circle about the axis of one of `frames` (the cylinder,
/// cone, sphere and torus faces it bounds) is labelled on that frame ([`on_frame`]), so its form
/// lies on that face's quarter grid; a circle or ellipse then has its form ([`curve`]). The label
/// with its interval, and the form with its own; `None` for an arc of more than one turn.
pub fn edge<'a>(
    c: &Curve,
    interval: [f64; 2],
    frames: impl IntoIterator<Item = &'a Frame>,
    tol: f64,
) -> Option<(Curve, [f64; 2], Option<(Curve, [f64; 2])>)> {
    let (label, iv) = frames.into_iter().find_map(|f| on_frame(c, interval, f, tol)).unwrap_or_else(|| (c.clone(), interval));
    let form = match label {
        Curve::Circle { .. } | Curve::Ellipse { .. } => Some(curve(&label, iv)?),
        _ => None,
    };
    Some((label, iv, form))
}

/// Points per edge along a loop taken for a face's extent.
const EXTENT_POINTS: usize = 32;

/// The largest value of `f` on `[a, b]`, where it rises to one peak and falls (golden section, to
/// rounding; the ends count).
fn golden_max(f: &dyn Fn(f64) -> f64, a: f64, b: f64) -> f64 {
    const R: f64 = 0.618_033_988_749_894_8;
    let (mut a, mut b) = (a, b);
    let best = f(a).max(f(b));
    let (mut c, mut d) = (b - R * (b - a), a + R * (b - a));
    let (mut fc, mut fd) = (f(c), f(d));
    while b - a > 1e-15 * (1.0 + a.abs().max(b.abs())) {
        if fc >= fd {
            b = d;
            d = c;
            fd = fc;
            c = b - R * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + R * (b - a);
            fd = f(d);
        }
    }
    best.max(fc).max(fd)
}

/// The parameter box of a face on `label` over its boundary: each loop's points (`loops`, in
/// order along each loop) projected onto the label, a periodic parameter unwrapped from one point
/// to the next, and each loop after the first a whole number of periods along where that keeps
/// the box smallest. What the face's NURBS form ([`surface`]) is built over; no pcurve is
/// involved.
pub fn extent(label: &Surface, loops: &[Vec<(&Curve, [f64; 2], bool)>]) -> Option<[[f64; 2]; 2]> {
    let eval = super::eval::SurfaceEval::new(label).ok()?;
    let period = eval.domain().period;
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for l in loops {
        // A loop is unwrapped from its own first point, so it can come out whole periods away
        // from the loops before it (a hole beside the seam projects to the seam's other side),
        // and a box over both is more than a turn wide.
        let (mut l_lo, mut l_hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        let mut prev: Option<[f64; 2]> = None;
        for (c, [t0, t1], forward) in l {
            let curve = super::eval::CurveEval::new(c).ok()?;
            // The point at `t` on the label, a periodic parameter unwrapped next to `near`.
            let at = |t: f64, near: Option<[f64; 2]>| -> Option<[f64; 2]> {
                let mut q = eval.project(curve.point(t)?, near, f64::INFINITY)?;
                if let Some(p) = near {
                    for d in 0..2 {
                        if let Some(Some(per)) = period.get(d) {
                            q[d] += ((p[d] - q[d]) / per).round() * per;
                        }
                    }
                }
                Some(q)
            };
            let mut samples: Vec<(f64, [f64; 2], [bool; 2])> = Vec::with_capacity(EXTENT_POINTS + 1);
            for k in 0..=EXTENT_POINTS {
                let f = k as f64 / EXTENT_POINTS as f64;
                let t = if *forward { t0 * (1.0 - f) + t1 * f } else { t1 * (1.0 - f) + t0 * f };
                let q = at(t, prev)?;
                // At a pole the collapsed parameter is anything: it bounds nothing.
                let pole = eval.pole(q);
                for d in 0..2 {
                    if !pole.get(d).copied().unwrap_or(false) {
                        l_lo[d] = l_lo[d].min(q[d]);
                        l_hi[d] = l_hi[d].max(q[d]);
                    }
                }
                if !pole.iter().any(|p| *p) {
                    prev = Some(q);
                }
                samples.push((t, q, pole));
            }
            // A sample is a point of the boundary, not its extreme: each sample higher (or lower)
            // than its neighbours brackets an extreme of the edge, which is found there.
            for k in 0..samples.len() {
                let (_, q, pole) = samples[k];
                let (a, b) = (samples[k.saturating_sub(1)].0, samples[(k + 1).min(samples.len() - 1)].0);
                for d in 0..2 {
                    // Next to a pole the parameter it collapses is no function of the point: a
                    // meridian's angle there is rounding, not an extreme.
                    let near_pole = |j: Option<usize>| j.and_then(|j| samples.get(j)).is_some_and(|s| s.2[d]);
                    if pole[d] || near_pole(k.checked_sub(1)) || near_pole(Some(k + 1)) {
                        continue;
                    }
                    let side = |j: usize| samples.get(j).map(|s| s.1[d]);
                    let (before, after) = (if k > 0 { side(k - 1) } else { None }, side(k + 1));
                    for sign in [1.0, -1.0] {
                        // Not lower than either neighbour and higher than one (a level run, a
                        // seam's constant angle, has no extreme between its samples).
                        let higher = |x: Option<f64>| x.is_none_or(|x| sign * q[d] >= sign * x);
                        let above = |x: Option<f64>| x.is_some_and(|x| sign * q[d] > sign * x);
                        if !(higher(before) && higher(after) && (above(before) || above(after))) {
                            continue;
                        }
                        let f = |t: f64| at(t, Some(q)).map_or(f64::NEG_INFINITY, |p| sign * p[d]);
                        let x = sign * golden_max(&f, a.min(b), a.max(b));
                        l_lo[d] = l_lo[d].min(x);
                        l_hi[d] = l_hi[d].max(x);
                    }
                }
            }
        }
        for d in 0..2 {
            if !(l_lo[d] <= l_hi[d]) {
                continue;
            }
            let shift = match period.get(d) {
                Some(Some(per)) if lo[d] <= hi[d] => {
                    // The whole periods around the ones that bring the loop's middle to the box's,
                    // the one giving the smallest box.
                    let k = ((lo[d] + hi[d] - l_lo[d] - l_hi[d]) / (2.0 * per)).round();
                    let span = |k: f64| hi[d].max(l_hi[d] + k * per) - lo[d].min(l_lo[d] + k * per);
                    [k, k - 1.0, k + 1.0].into_iter().min_by(|a, b| span(*a).total_cmp(&span(*b))).unwrap_or(k) * per
                }
                _ => 0.0,
            };
            lo[d] = lo[d].min(l_lo[d] + shift);
            hi[d] = hi[d].max(l_hi[d] + shift);
        }
    }
    // At most one turn round a periodic direction; a boundary that never goes round it (a face
    // bounded by its seam alone) covers the whole turn from there.
    for d in 0..2 {
        if let Some(Some(per)) = period.get(d) {
            hi[d] = if hi[d] - lo[d] <= 1e-9 * per { lo[d] + per } else { hi[d].min(lo[d] + per) };
        }
    }
    (lo[0] < hi[0] && lo[1] < hi[1]).then_some([[lo[0], hi[0]], [lo[1], hi[1]]])
}
