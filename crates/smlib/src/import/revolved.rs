//! Revolutions as SMLib's analytic surfaces. A `placed_revolution` record holds a surface the
//! way truck does (a profile turned about an axis, then placed); SMLib has no such surface, but
//! every one truck makes for a cylinder, cone, sphere or torus is one of those exactly: a line
//! along the axis, a line in a plane through it, a circle centred on it, a circle in a plane
//! through it. Each such face becomes that analytic surface, its range mapped from the
//! revolution's parameters, and checked point by point over the face's range; where the
//! analytic surface's natural normal is the other way round, the face is turned over (its loops
//! reversed, its uses in the shells flipped), which leaves the solid as it was.
//!
//! A revolution of any other profile, or one placed by a matrix that is not a rigid motion, is
//! refused.

use std::borrow::Cow;
use std::f64::consts::{FRAC_PI_2, TAU};

use solidcraft_geom::Vec3;
use solidcraft_geom::brep as b;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};

use super::{R, Unsupported};

const NOT_ANALYTIC: Unsupported = Unsupported::NotYet("revolved surfaces other than cylinders, cones, spheres and tori");

/// `brep` with every revolution replaced by its analytic surface (borrowed when it has none).
pub(super) fn analytic(brep: &b::Brep) -> R<Cow<'_, b::Brep>> {
    if !brep.faces.iter().any(|f| matches!(f.surface, b::Surface::PlacedRevolution { .. })) {
        return Ok(Cow::Borrowed(brep));
    }
    let mut out = brep.clone();
    let size = extent(brep);
    let mut turned = Vec::new();
    for (i, f) in out.faces.iter_mut().enumerate() {
        let b::Surface::PlacedRevolution { profile, origin, axis, matrix, forward } = &f.surface else { continue };
        let rev = Revolution::new(profile, *origin, *axis, matrix, *forward)?;
        let (surface, mut domain, v) = rev.analytic(f.domain)?;
        // Angles start within the first turn, where SMLib's degree domains are at home.
        domain[0] = first_turn(domain[0]);
        if matches!(surface, b::Surface::Torus { .. }) {
            domain[1] = first_turn(domain[1]);
        }
        let flip = agree(&f.surface, f.domain, &surface, domain, &rev, &v, brep.tolerance.max(1e-9 * size))?;
        f.surface = surface;
        f.domain = domain;
        if flip {
            turned.push(i);
        }
    }
    for i in turned {
        turn_over(&mut out, i)?;
    }
    Ok(Cow::Owned(out))
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
    fn analytic(&self, domain: [[f64; 2]; 2]) -> R<(b::Surface, [[f64; 2]; 2], V)> {
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
                return Err(NOT_ANALYTIC); // a disc: a plane, which a revolution face never is in the kernel
            }
            let r0 = radial(s) - dr / dh * height(s);
            let v = sorted(h0, h1);
            if dr.abs() <= 1e-12 * len {
                if !(r0 > 0.0) {
                    return Err(NOT_ANALYTIC);
                }
                return Ok((b::Surface::Cylinder { frame: frame(foot), radius: r0 }, [angle, v], V::Height { foot, z: self.z }));
            }
            // A cone on one side of its apex only.
            let r = |h: f64| r0 + h * dr / dh;
            if r(v[0]) < 0.0 || r(v[1]) < 0.0 {
                return Err(NOT_ANALYTIC);
            }
            let cone = b::Surface::Cone { frame: frame(foot), radius_at_origin: r0, half_angle: (dr / dh).atan() };
            return Ok((cone, [angle, v], V::Height { foot, z: self.z }));
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
            return Ok((b::Surface::Sphere { frame: frame(foot), radius }, [angle, v], V::Angle { center, x, z: self.z }));
        }
        let major = off.dot(x);
        if !(major > radius) {
            return Err(NOT_ANALYTIC);
        }
        let torus = b::Surface::Torus { frame: frame(foot), major_radius: major, minor_radius: radius };
        Ok((torus, [angle, sorted(first, last)], V::Angle { center, x, z: self.z }))
    }

    /// The analytic (u, v) of the record's (u, v): the angle is the revolution's own, v that of
    /// the profile's point (at angle 0), unwrapped to within half a turn of `near`.
    fn map(&self, v: &V, uv: [f64; 2], near: f64) -> Option<[f64; 2]> {
        let (t, angle) = if self.forward { (uv[0], uv[1]) } else { (uv[1], uv[0]) };
        let p = self.profile.point(t)?;
        Some([angle, v.of(p, near)])
    }
}

/// How the analytic v of a profile point is found.
enum V {
    /// Height along `z` above `foot` (cylinder, cone).
    Height { foot: Vec3, z: Vec3 },
    /// The angle about `center` from `x` towards `z` (sphere latitude, torus meridian).
    Angle { center: Vec3, x: Vec3, z: Vec3 },
}

impl V {
    fn of(&self, p: Vec3, near: f64) -> f64 {
        match self {
            V::Height { foot, z } => (p - *foot).dot(*z),
            V::Angle { center, x, z } => {
                let q = p - *center;
                let mut a = q.dot(*z).atan2(q.dot(*x));
                while a - near > std::f64::consts::PI {
                    a -= TAU;
                }
                while a - near < -std::f64::consts::PI {
                    a += TAU;
                }
                a
            }
        }
    }
}

/// Checks the analytic surface against the revolution over the face's range, 5 × 5 points
/// within `tol`, and says whether their natural normals are opposite (the face must be turned
/// over), read at the range's middle.
fn agree(record: &b::Surface, domain: [[f64; 2]; 2], analytic: &b::Surface, range: [[f64; 2]; 2], rev: &Revolution, v: &V, tol: f64) -> R<bool> {
    let bad = Unsupported::Invalid("a revolution its analytic surface does not match");
    let s = SurfaceEval::new(record).map_err(|_| bad.clone())?;
    let a = SurfaceEval::new(analytic).map_err(|_| bad.clone())?;
    let [[u0, u1], [v0, v1]] = domain;
    let near = 0.5 * (range[1][0] + range[1][1]);
    let normals = |u: f64, w: f64| -> R<f64> {
        let (p, du, dv) = s.derivs(u, w).ok_or(bad.clone())?;
        let uv = rev.map(v, [u, w], near).ok_or(bad.clone())?;
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
