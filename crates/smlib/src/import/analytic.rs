//! Revolutions and extruded circles as SMLib's analytic surfaces.
//!
//! A `placed_revolution` record holds a surface the way truck does (a profile turned about an
//! axis, then placed); SMLib has no such surface, but every one truck makes for a plane, cylinder, cone,
//! sphere or torus is one of those exactly: a line square to the axis, a line along it, a line in a plane through it, a
//! circle centred on it, a circle in a plane through it. A rational spline that is a circular arc
//! swept straight along its axis (truck's extruded circles) is a cylinder. Each such face becomes
//! that analytic surface with the face's range on it, checked point by point over the face's
//! domain; where the analytic surface's natural normal is the other way round, the face is turned
//! over (its loops reversed, its uses in the shells flipped), which leaves the solid as it was.
//!
//! A revolution of any other profile, or one placed by a matrix that is not a rigid motion,
//! stays a revolution, which the import sends as its exact rational B-spline; any other spline
//! stays a spline.

use std::borrow::Cow;
use std::f64::consts::{FRAC_PI_2, TAU};

use solidcraft_geom::Vec3;
use solidcraft_geom::brep as b;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};

use super::{R, Unsupported};

const NOT_ANALYTIC: Unsupported = Unsupported::NotYet("revolved surfaces other than planes, cylinders, cones, spheres and tori");

/// `brep` with every revolution and extruded circle replaced by its analytic surface (borrowed
/// when it has none).
pub(super) fn analytic(brep: &b::Brep) -> R<Cow<'_, b::Brep>> {
    let size = extent(brep);
    let tol = brep.tolerance.max(1e-9 * size);
    let mut out: Option<b::Brep> = None;
    let mut turned = Vec::new();
    for (i, f) in brep.faces.iter().enumerate() {
        let found = match &f.surface {
            // One that is none of the analytic kinds goes as its exact rational B-spline
            // (`super::revolution`).
            b::Surface::PlacedRevolution { profile, origin, axis, matrix, forward } => {
                match Revolution::new(profile, *origin, *axis, matrix, *forward).and_then(|r| r.analytic(f.domain)) {
                    Ok(found) => Some(found),
                    Err(Unsupported::NotYet(_)) => None,
                    Err(e) => return Err(e),
                }
            }
            b::Surface::RationalSpline { u_knots, v_knots, homogeneous } => {
                extruded_circle(u_knots, v_knots, homogeneous, f.domain, 1e-9 * size.max(1.0))
            }
            _ => None,
        };
        let Some((mut surface, mut range)) = found else { continue };
        // The angle about the axis starts at 0: the frame turns to the face's first angle (SMLib
        // refuses an analytic range past 360°). A torus's tube angle can't turn so: it starts
        // within the first turn, where SMLib's degree domains are at home.
        if !matches!(surface, b::Surface::Plane { .. }) {
            turn_frame(&mut surface, range[0][0]);
            range[0] = [0.0, range[0][1] - range[0][0]];
        }
        if matches!(surface, b::Surface::Torus { .. }) {
            range[1] = first_turn(range[1]);
        }
        let flip = agree((&f.surface, f.domain), (&surface, range), tol)?;
        let face = out.get_or_insert_with(|| brep.clone()).faces.get_mut(i).ok_or(Unsupported::Invalid("face"))?;
        face.surface = surface;
        face.domain = range;
        if flip {
            turned.push(i);
        }
    }
    let Some(mut out) = out else { return Ok(Cow::Borrowed(brep)) };
    for i in turned {
        turn_over(&mut out, i)?;
    }
    Ok(Cow::Owned(out))
}

/// A rational spline that is a circular arc swept straight along the arc's axis, as a cylinder
/// with the face's range on it: one direction linear (two rows of control points), the second
/// row the first moved by one vector at right angles to the circle the first row runs on, the
/// weights alike. `None` where it is not.
fn extruded_circle(u_knots: &[f64], v_knots: &[f64], net: &[Vec<[f64; 4]>], domain: [[f64; 2]; 2], tol: f64) -> Option<(b::Surface, [[f64; 2]; 2])> {
    let linear = |k: &[f64]| k.len() == 4 && k[0] == k[1] && k[2] == k[3] && k[1] < k[2];
    let (rows, arc_knots, arc_range, line_knots, line_range): ([Vec<[f64; 4]>; 2], &[f64], [f64; 2], &[f64], [f64; 2]) =
        if net.iter().all(|r| r.len() == 2) && linear(v_knots) && !linear(u_knots) {
            ([net.iter().map(|r| r[0]).collect(), net.iter().map(|r| r[1]).collect()], u_knots, domain[0], v_knots, domain[1])
        } else if net.len() == 2 && linear(u_knots) && !linear(v_knots) {
            ([net.first()?.clone(), net.get(1)?.clone()], v_knots, domain[1], u_knots, domain[0])
        } else {
            return None;
        };
    let cart = |h: &[f64; 4]| Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
    let (first, second) = (&rows[0], &rows[1]);
    let d = cart(second.first()?) - cart(first.first()?);
    let alike = first.iter().zip(second).all(|(p, q)| (p[3] - q[3]).abs() <= 1e-12 * p[3].abs() && (cart(q) - cart(p) - d).len() <= tol);
    if !alike || first.len() != second.len() || !(d.len() > tol) {
        return None;
    }
    let arc = CurveEval::new(&b::Curve::RationalSpline { knots: arc_knots.to_vec(), homogeneous: first.clone() }).ok()?;
    let (center, radius, normal) = circle(&arc, arc_range)?;
    let z = d.normalized()?;
    if normal.cross(z).len() > 1e-9 {
        return None;
    }
    let x = (arc.point(arc_range[0])? - center).normalized()?;
    let y = z.cross(x);
    // The arc's angles from x, unwrapped along its range.
    let mut angle = 0.0;
    let mut prev = 0.0;
    for i in 1..=32 {
        let q = arc.point(arc_range[0] + (arc_range[1] - arc_range[0]) * f64::from(i) / 32.0)? - center;
        let mut a = q.dot(y).atan2(q.dot(x));
        while a - prev > std::f64::consts::PI {
            a -= TAU;
        }
        while a - prev < -std::f64::consts::PI {
            a += TAU;
        }
        prev = a;
        angle = a;
    }
    let height = |s: f64| (s - line_knots[1]) / (line_knots[2] - line_knots[1]) * d.len();
    let sorted = |a: f64, b: f64| if a <= b { [a, b] } else { [b, a] };
    let frame = b::Frame { origin: arr(center), x_axis: arr(x), y_axis: arr(y) };
    Some((b::Surface::Cylinder { frame, radius }, [sorted(0.0, angle), sorted(height(line_range[0]), height(line_range[1]))]))
}

/// An analytic surface's frame turned by `angle` about its axis: u then counts from there.
fn turn_frame(s: &mut b::Surface, angle: f64) {
    if let b::Surface::Cylinder { frame, .. } | b::Surface::Cone { frame, .. } | b::Surface::Sphere { frame, .. } | b::Surface::Torus { frame, .. } =
        s
    {
        let (x, y) = (v3(frame.x_axis), v3(frame.y_axis));
        let (sin, cos) = angle.sin_cos();
        frame.x_axis = arr(x * cos + y * sin);
        frame.y_axis = arr(y * cos - x * sin);
    }
}

/// An angle range moved by whole turns to start in [0, 2π).
fn first_turn([a, z]: [f64; 2]) -> [f64; 2] {
    let shift = (a / TAU).floor() * TAU;
    [a - shift, z - shift]
}

/// The body's size from its vertices (1 where it has none).
fn extent(brep: &b::Brep) -> f64 {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for v in &brep.vertices {
        for k in 0..3 {
            lo[k] = lo[k].min(v.point[k]);
            hi[k] = hi[k].max(v.point[k]);
        }
    }
    let d = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt();
    if d.is_finite() && d > 0.0 { d } else { 1.0 }
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}
fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// A revolution in world space: its profile placed, its axis placed, the turning sense.
struct Revolution {
    profile: CurveEval,
    line: Option<[Vec3; 2]>,
    /// A point on the axis and the axis, oriented so that the revolution's angle turns about it
    /// by the right-hand rule.
    o: Vec3,
    z: Vec3,
    /// The record's (u, v) is (profile, angle).
    forward: bool,
}

impl Revolution {
    fn new(profile: &b::Curve, origin: [f64; 3], axis: [f64; 3], m: &[[f64; 4]; 4], forward: bool) -> R<Revolution> {
        // Column-major: m[column][row]. A rigid motion only (a mirror reverses the turning).
        let col = |c: usize| Vec3::new(m[c][0], m[c][1], m[c][2]);
        let (c0, c1, c2, t) = (col(0), col(1), col(2), col(3));
        let unit = |v: Vec3| (v.len() - 1.0).abs() <= 1e-9;
        if !(m[0][3] == 0.0 && m[1][3] == 0.0 && m[2][3] == 0.0 && m[3][3] == 1.0)
            || !(unit(c0) && unit(c1) && unit(c2))
            || c0.dot(c1).abs() > 1e-9
            || c0.dot(c2).abs() > 1e-9
            || c1.dot(c2).abs() > 1e-9
        {
            return Err(Unsupported::NotYet("revolved surfaces placed by a matrix that is no rigid motion"));
        }
        let det = c0.dot(c1.cross(c2));
        let place = |p: Vec3| c0 * p.x + c1 * p.y + c2 * p.z + t;
        let turn = |d: Vec3| c0 * d.x + c1 * d.y + c2 * d.z;
        let z = turn(v3(axis)).normalized().ok_or(Unsupported::Invalid("revolution axis"))? * det.signum();
        let placed_curve = |c: &b::Curve| -> R<b::Curve> {
            let p = |q: [f64; 3]| arr(place(v3(q)));
            Ok(match c {
                b::Curve::Line { start, end } => b::Curve::Line { start: p(*start), end: p(*end) },
                b::Curve::Circle { frame, radius } => b::Curve::Circle {
                    frame: b::Frame { origin: p(frame.origin), x_axis: arr(turn(v3(frame.x_axis))), y_axis: arr(turn(v3(frame.y_axis))) },
                    radius: *radius,
                },
                b::Curve::Spline { knots, points } => b::Curve::Spline { knots: knots.clone(), points: points.iter().map(|q| p(*q)).collect() },
                b::Curve::RationalSpline { knots, homogeneous } => b::Curve::RationalSpline {
                    knots: knots.clone(),
                    homogeneous: homogeneous
                        .iter()
                        .map(|h| {
                            let w = h[3];
                            let q = place(Vec3::new(h[0], h[1], h[2]) * (1.0 / w));
                            [q.x * w, q.y * w, q.z * w, w]
                        })
                        .collect(),
                },
                _ => return Err(NOT_ANALYTIC),
            })
        };
        let world = placed_curve(profile)?;
        let line = match &world {
            b::Curve::Line { start, end } => Some([v3(*start), v3(*end)]),
            _ => None,
        };
        let profile = CurveEval::new(&world).map_err(|_| Unsupported::Invalid("revolution profile"))?;
        Ok(Revolution { profile, line, o: place(v3(origin)), z, forward })
    }

    /// The profile's parameter range and the angle range of a face's domain.
    fn ranges(&self, domain: [[f64; 2]; 2]) -> ([f64; 2], [f64; 2]) {
        if self.forward { (domain[0], domain[1]) } else { (domain[1], domain[0]) }
    }

    /// `p` relative to the axis: its foot on the axis, its height along z and its radial part.
    fn split(&self, p: Vec3) -> (Vec3, f64, Vec3) {
        let h = (p - self.o).dot(self.z);
        let foot = self.o + self.z * h;
        (foot, h, p - foot)
    }

    /// The analytic surface and the face's range on it: u the angle about z from the profile's
    /// half-plane, v as the surface's kind has it.
    fn analytic(&self, domain: [[f64; 2]; 2]) -> R<(b::Surface, [[f64; 2]; 2])> {
        let (t, angle) = self.ranges(domain);
        let at = |s: f64| self.profile.point(s).ok_or(Unsupported::Invalid("revolution profile"));
        let samples: Vec<Vec3> = (0..=16).map(|i| at(t[0] + (t[1] - t[0]) * f64::from(i) / 16.0)).collect::<R<_>>()?;
        // The profile's half-plane: the radial direction of its point farthest from the axis.
        let far = samples.iter().map(|p| self.split(*p).2).fold(Vec3::ZERO, |best, r| if r.len() > best.len() { r } else { best });
        let x = far.normalized().ok_or(NOT_ANALYTIC)?;
        let y = self.z.cross(x);
        let frame = |o: Vec3| b::Frame { origin: arr(o), x_axis: arr(x), y_axis: arr(y) };
        let sorted = |a: f64, b: f64| if a <= b { [a, b] } else { [b, a] };
        if let Some([s, e]) = self.line {
            let d = e - s;
            let len = d.len();
            if !(len > 0.0) {
                return Err(NOT_ANALYTIC);
            }
            // In a plane through the axis: no component out of the (z, x) plane.
            if (s - self.o).dot(y).abs() > 1e-9 * len.max(1.0) || d.dot(y).abs() > 1e-9 * len {
                return Err(NOT_ANALYTIC);
            }
            let (foot, _, _) = self.split(s);
            let height = |p: Vec3| (p - foot).dot(self.z);
            let radial = |p: Vec3| (p - foot).dot(x);
            let (h0, h1) = (height(at(t[0])?), height(at(t[1])?));
            let (dh, dr) = (d.dot(self.z), d.dot(x));
            if dh.abs() <= 1e-12 * len {
                // A disc or an annulus: the plane square to the axis, its range the box round the
                // sector the line sweeps (each end's radius at both end angles and every quarter
                // turn between: in the plane a point is linear in the radius).
                let (ra, rb) = (radial(at(t[0])?), radial(at(t[1])?));
                let mut angles = vec![angle[0], angle[1]];
                let mut q = (angle[0] / FRAC_PI_2).ceil() * FRAC_PI_2;
                while q < angle[1] && angles.len() < 16 {
                    angles.push(q);
                    q += FRAC_PI_2;
                }
                let corners: Vec<[f64; 2]> = angles.iter().flat_map(|a| [ra, rb].map(|r| [r * a.cos(), r * a.sin()])).collect();
                let lo = |k: usize| corners.iter().map(|c| c[k]).fold(f64::INFINITY, f64::min);
                let hi = |k: usize| corners.iter().map(|c| c[k]).fold(f64::NEG_INFINITY, f64::max);
                let plane = b::Surface::Plane { origin: arr(foot), u_axis: arr(x), v_axis: arr(y) };
                return Ok((plane, [[lo(0), hi(0)], [lo(1), hi(1)]]));
            }
            let r0 = radial(s) - dr / dh * height(s);
            let v = sorted(h0, h1);
            if dr.abs() <= 1e-12 * len {
                if !(r0 > 0.0) {
                    return Err(NOT_ANALYTIC);
                }
                return Ok((b::Surface::Cylinder { frame: frame(foot), radius: r0 }, [angle, v]));
            }
            // A cone on one side of its apex only.
            let r = |h: f64| r0 + h * dr / dh;
            if r(v[0]) < 0.0 || r(v[1]) < 0.0 {
                return Err(NOT_ANALYTIC);
            }
            let cone = b::Surface::Cone { frame: frame(foot), radius_at_origin: r0, half_angle: (dr / dh).atan() };
            return Ok((cone, [angle, v]));
        }
        // A circular profile.
        let (center, radius, normal) = circle(&self.profile, t).ok_or(NOT_ANALYTIC)?;
        let slack = 1e-9 * radius.max(1.0);
        if normal.dot(self.z).abs() > 1e-9 || normal.dot(x).abs() > 1e-9 || (center - self.o).dot(y).abs() > slack {
            return Err(NOT_ANALYTIC);
        }
        let (foot, _, off) = self.split(center);
        // The meridian angle of each sample about the circle's centre in the (x, z) plane, unwrapped.
        let meridian = |p: Vec3| {
            let q = p - center;
            q.dot(self.z).atan2(q.dot(x))
        };
        let mut angles = Vec::with_capacity(samples.len());
        for p in &samples {
            let mut a = meridian(*p);
            if let Some(prev) = angles.last().copied() {
                while a - prev > std::f64::consts::PI {
                    a -= TAU;
                }
                while a - prev < -std::f64::consts::PI {
                    a += TAU;
                }
            }
            angles.push(a);
        }
        let (first, last) = (angles.first().copied().ok_or(NOT_ANALYTIC)?, angles.last().copied().ok_or(NOT_ANALYTIC)?);
        if off.len() <= slack {
            // A sphere: the latitude is the meridian angle, within the half-plane's [-π/2, π/2].
            let v = sorted(first, last);
            if v[0] < -FRAC_PI_2 - 1e-9 || v[1] > FRAC_PI_2 + 1e-9 {
                return Err(NOT_ANALYTIC);
            }
            let v = [v[0].max(-FRAC_PI_2), v[1].min(FRAC_PI_2)];
            return Ok((b::Surface::Sphere { frame: frame(foot), radius }, [angle, v]));
        }
        let major = off.dot(x);
        if !(major > radius) {
            return Err(NOT_ANALYTIC);
        }
        let torus = b::Surface::Torus { frame: frame(foot), major_radius: major, minor_radius: radius };
        Ok((torus, [angle, sorted(first, last)]))
    }
}

/// The (u, v) of point `p` on an analytic surface, angles unwrapped to within half a turn of
/// `near`.
fn analytic_uv(s: &b::Surface, p: Vec3, near: [f64; 2]) -> Option<[f64; 2]> {
    let unwrap = |mut a: f64, n: f64| {
        while a - n > std::f64::consts::PI {
            a -= TAU;
        }
        while a - n < -std::f64::consts::PI {
            a += TAU;
        }
        a
    };
    if let b::Surface::Plane { origin, u_axis, v_axis } = s {
        let q = p - v3(*origin);
        return Some([q.dot(v3(*u_axis)), q.dot(v3(*v_axis))]);
    }
    let (frame, round) = match s {
        b::Surface::Cylinder { frame, .. } | b::Surface::Cone { frame, .. } | b::Surface::Sphere { frame, .. } => (frame, 0.0),
        b::Surface::Torus { frame, major_radius, .. } => (frame, *major_radius),
        _ => return None,
    };
    let (x, y) = (v3(frame.x_axis), v3(frame.y_axis));
    let z = x.cross(y);
    let q = p - v3(frame.origin);
    let (qx, qy, qz) = (q.dot(x), q.dot(y), q.dot(z));
    let u = unwrap(qy.atan2(qx), near[0]);
    let r = qx.hypot(qy);
    Some(match s {
        b::Surface::Sphere { .. } => [u, qz.atan2(r)],
        b::Surface::Torus { .. } => [u, unwrap(qz.atan2(r - round), near[1])],
        _ => [u, qz],
    })
}

/// Checks the analytic surface against the record's over the face's domain, 5 × 5 points within
/// `tol`, and says whether their natural normals are opposite (the face must be turned over),
/// read at the domain's middle.
fn agree((record, domain): (&b::Surface, [[f64; 2]; 2]), (analytic, range): (&b::Surface, [[f64; 2]; 2]), tol: f64) -> R<bool> {
    let bad = Unsupported::Invalid("a surface its analytic form does not match");
    let s = SurfaceEval::new(record).map_err(|_| bad.clone())?;
    let a = SurfaceEval::new(analytic).map_err(|_| bad.clone())?;
    let [[u0, u1], [v0, v1]] = domain;
    let near = [0.5 * (range[0][0] + range[0][1]), 0.5 * (range[1][0] + range[1][1])];
    let normals = |u: f64, w: f64| -> R<f64> {
        let (p, du, dv) = s.derivs(u, w).ok_or(bad.clone())?;
        let uv = analytic_uv(analytic, p, near).ok_or(bad.clone())?;
        let (q, au, av) = a.derivs(uv[0], uv[1]).ok_or(bad.clone())?;
        if !((p - q).len() <= tol) {
            return Err(bad.clone());
        }
        Ok(du.cross(dv).dot(au.cross(av)))
    };
    for i in 0..=4 {
        for j in 0..=4 {
            normals(u0 + (u1 - u0) * f64::from(i) / 4.0, v0 + (v1 - v0) * f64::from(j) / 4.0)?;
        }
    }
    let d = normals(0.5 * (u0 + u1), 0.5 * (v0 + v1))?;
    if !(d.abs() > 0.0) {
        return Err(bad);
    }
    Ok(d < 0.0)
}

/// The circle a curve runs along over `t`: centre, radius and unit normal (its turning sense),
/// checked at 65 points within 10⁻⁹ of the radius (at least 1 mm).
fn circle(c: &CurveEval, t: [f64; 2]) -> Option<(Vec3, f64, Vec3)> {
    let at = |s: f64| c.point(t[0] + (t[1] - t[0]) * s);
    let (p, q, r) = (at(0.0)?, at(1.0 / 3.0)?, at(2.0 / 3.0)?);
    let (u, w) = (q - p, r - p);
    let normal = u.cross(w);
    let nn = normal.dot(normal);
    if !(nn > 0.0) {
        return None;
    }
    let center = p + (normal.cross(u) * w.dot(w) + w.cross(normal) * u.dot(u)) * (0.5 / nn);
    let radius = (p - center).len();
    let k = normal.normalized()?;
    let slack = 1e-9 * radius.max(1.0);
    for i in 0..=64 {
        let s = at(f64::from(i) / 64.0)?;
        if ((s - center).len() - radius).abs() > slack || (s - center).dot(k).abs() > slack {
            return None;
        }
    }
    Some((center, radius, k))
}

/// Face `face` turned over: its loops run the other way, each use of it in a shell flips, and
/// each of its edges' uses names its new place. The solid is unchanged.
fn turn_over(brep: &mut b::Brep, face: usize) -> R<()> {
    use std::collections::HashMap;
    let index = |x: usize| u32::try_from(x).map_err(|_| Unsupported::Invalid("index"));
    let fi = index(face)?;
    let f = brep.faces.get_mut(face).ok_or(Unsupported::Invalid("face"))?;
    // (loop, old position) -> (new position, the use's new direction).
    let mut moved: HashMap<(u32, u32), (u32, bool)> = HashMap::new();
    for (li, l) in f.loops.iter_mut().enumerate() {
        if let b::Loop::Edges { edges } = l {
            let n = edges.len();
            edges.reverse();
            for (p, e) in edges.iter_mut().enumerate() {
                e.forward = !e.forward;
                moved.insert((index(li)?, index(n - 1 - p)?), (index(p)?, e.forward));
            }
        }
    }
    for e in &mut brep.edges {
        for u in e.uses.iter_mut().filter(|u| u.face == fi) {
            let &(position, forward) = moved.get(&(u.r#loop, u.position)).ok_or(Unsupported::Invalid("edge use"))?;
            u.position = position;
            u.upper = !forward;
        }
    }
    for s in &mut brep.shells {
        if let b::Shell::Faces { faces, .. } = s {
            for o in faces.iter_mut().filter(|o| o.face == fi) {
                o.forward = !o.forward;
            }
        }
    }
    Ok(())
}
