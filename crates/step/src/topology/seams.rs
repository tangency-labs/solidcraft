//! Seam edges for faces that wind around a periodic surface without one. STEP lets a face on a
//! cylinder, cone or sphere be bounded by loops that go once around the axis (a band between
//! two circles, a cap bounded by one circle, a cone bounded by its base and a vertex loop at its
//! apex); schema 4 keeps every loop closed in the surface's (u, v), so such a face gets a seam:
//! a ruling line on a cylinder or cone, a meridian arc on a sphere, from a vertex of one loop to
//! the other loop (split there when it has no vertex at that angle) or to the pole the face
//! covers; on a surface of revolution, its profile turned to the vertex's angle. The loops merge into one: the first, the seam out, the second (or nothing at a pole),
//! the seam back. A torus face winding without a seam is refused for now.

use std::f64::consts::{PI, TAU};

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::brep::{Curve as BCurve, Frame, Surface as BSurface, Vertex};

use super::{Builder, EdgeB, LoopB, R, index, unsupported};

/// Samples per edge when following a loop around the axis.
const SAMPLES: usize = 16;
/// Edges split to place seams, per solid.
const MAX_SPLITS: usize = 10_000;

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn wrap(x: f64) -> f64 {
    x - (x / TAU).round() * TAU
}

/// The axis frame of a surface of revolution: origin, unit axis, and the pole points at the
/// ends of its v range that have one (min v, max v).
struct Axis {
    origin: Vec3,
    z: Vec3,
    kind: Kind,
    poles: [Option<Vec3>; 2],
}

#[derive(Clone, PartialEq)]
enum Kind {
    Ruled,
    Sphere(f64),
    /// A placed revolution with u the angle: its profile.
    Revolved(BCurve),
}

fn axis(s: &BSurface) -> Option<Axis> {
    let frame = |f: &Frame| {
        let (o, x, y) = (Vec3::from(f.origin), Vec3::from(f.x_axis), Vec3::from(f.y_axis));
        (o, x.cross(y))
    };
    match s {
        BSurface::Cylinder { frame: f, .. } => {
            let (origin, z) = frame(f);
            Some(Axis { origin, z, kind: Kind::Ruled, poles: [None, None] })
        }
        BSurface::Cone { frame: f, radius_at_origin, half_angle } => {
            let (origin, z) = frame(f);
            let apex = origin + z * (-radius_at_origin / half_angle.tan());
            let poles = if *half_angle > 0.0 { [Some(apex), None] } else { [None, Some(apex)] };
            Some(Axis { origin, z, kind: Kind::Ruled, poles })
        }
        BSurface::Sphere { frame: f, radius } => {
            let (origin, z) = frame(f);
            Some(Axis { origin, z, kind: Kind::Sphere(*radius), poles: [Some(origin - z * *radius), Some(origin + z * *radius)] })
        }
        BSurface::PlacedRevolution { profile, origin, axis, matrix, forward: false } if *matrix == IDENTITY => {
            let (origin, z) = (Vec3::from(*origin), Vec3::from(*axis));
            let c = CurveEval::new(profile).ok()?;
            let solidcraft_geom::brep::Natural::Range(a, b) = profile.natural() else { return None };
            // A profile end on the axis is a pole.
            let pole = |t: f64| {
                c.point(t).filter(|p| {
                    let d = *p - origin;
                    (d - z * d.dot(z)).len() <= 1e-9 * (1.0 + d.len())
                })
            };
            Some(Axis { origin, z, kind: Kind::Revolved((**profile).clone()), poles: [pole(a), pole(b)] })
        }
        _ => None,
    }
}

const IDENTITY: [[f64; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// `p` turned by `angle` about the axis through `o` along unit `k` (right-handed).
fn turned(p: Vec3, o: Vec3, k: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    let x = p - o;
    o + x * c + k.cross(x) * s + k * (k.dot(x) * (1.0 - c))
}

/// A revolution's profile turned by `angle`: the meridian there, with the profile's parameter.
fn meridian(profile: &BCurve, o: Vec3, k: Vec3, angle: f64) -> R<BCurve> {
    let turn = |h: &[f64; 4]| {
        let w = h[3];
        let p = turned(Vec3::new(h[0] / w, h[1] / w, h[2] / w), o, k, angle);
        [p.x * w, p.y * w, p.z * w, w]
    };
    Ok(match profile {
        BCurve::Spline { knots, points } => {
            BCurve::Spline { knots: knots.clone(), points: points.iter().map(|p| arr(turned(Vec3::from(*p), o, k, angle))).collect() }
        }
        BCurve::RationalSpline { knots, homogeneous } => {
            BCurve::RationalSpline { knots: knots.clone(), homogeneous: homogeneous.iter().map(turn).collect() }
        }
        BCurve::Line { start, end } => {
            BCurve::Line { start: arr(turned(Vec3::from(*start), o, k, angle)), end: arr(turned(Vec3::from(*end), o, k, angle)) }
        }
        _ => return Err(unsupported("a revolution of this profile")),
    })
}

impl Builder<'_, '_> {
    /// Give every face that winds around its surface without a seam one. Returns how many
    /// seams were added.
    pub(super) fn add_seams(&mut self) -> R<usize> {
        let mut added = 0;
        let mut splits = 0;
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            let surface = face.surface.clone();
            let periodic = super::natural_periods(&surface);
            if !periodic.iter().any(|p| *p) {
                continue;
            }
            let eval = SurfaceEval::new(&surface).map_err(|e| e.to_string())?;
            let mut up = Vec::new();
            let mut down = Vec::new();
            for (li, l) in face.loops.iter().enumerate() {
                let LoopB::Edges(es) = l else { continue };
                let w = self.winding(&eval, es, periodic)?;
                if w[1] != 0 || w[0].abs() > 1 {
                    return Err(format!("face #{}: {}", face.step, unsupported("a loop winding around a surface more than once or in v")));
                }
                match w[0] {
                    1 => up.push(li),
                    -1 => down.push(li),
                    _ => {}
                }
            }
            if up.is_empty() && down.is_empty() {
                continue;
            }
            let step = face.step;
            let Some(ax) = axis(&surface) else {
                return Err(format!("face #{step}: {}", unsupported("a face winding around a torus without a seam edge")));
            };
            match (up.as_slice(), down.as_slice()) {
                // The face lies to the left of its loops in (u, v): above a loop running +u.
                ([l], []) => self.pole_seam(fi, *l, &ax, ax.poles[1], &eval)?,
                ([], [l]) => self.pole_seam(fi, *l, &ax, ax.poles[0], &eval)?,
                ([a], [b]) => self.band_seam(fi, *a, *b, &ax, &eval, &mut splits)?,
                _ => return Err(format!("face #{step}: {}", unsupported("a face with several loops around its axis"))),
            }
            added += 1;
        }
        Ok(added)
    }

    /// Turns (in u, in v) a loop makes around a periodic surface.
    fn winding(&self, eval: &SurfaceEval, es: &[(u32, bool)], periodic: [bool; 2]) -> R<[i64; 2]> {
        let mut last: Option<[f64; 2]> = None;
        let mut total = [0.0; 2];
        for &(e, forward) in es {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
            let [lo, hi] = edge.interval;
            for k in 0..=SAMPLES {
                let s = k as f64 / SAMPLES as f64;
                let t = if forward { lo + (hi - lo) * s } else { hi - (hi - lo) * s };
                let p = c.point(t).ok_or("edge point")?;
                let Some(q) = eval.project(p, last, f64::INFINITY) else { continue };
                if let Some(l) = last {
                    for d in 0..2 {
                        if periodic[d] {
                            total[d] += wrap(q[d] - l[d]);
                        }
                    }
                }
                last = Some(q);
            }
        }
        Ok(total.map(|x| (x / TAU).round() as i64))
    }

    /// The vertex where use `k` of a loop starts.
    fn start(&self, (e, forward): (u32, bool)) -> R<u32> {
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        Ok(if forward { edge.vertices[0] } else { edge.vertices[1] })
    }

    fn vpoint(&self, v: u32) -> R<Vec3> {
        self.vertices.get(v as usize).map(|v| Vec3::from(v.point)).ok_or_else(|| "vertex reference".to_string())
    }

    /// Edge loop `li` of face `fi`, rotated to start at its use `k`.
    fn rotated(&self, fi: usize, li: usize, k: usize) -> R<Vec<(u32, bool)>> {
        let Some(LoopB::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
        Ok(es.iter().skip(k).chain(es.iter().take(k)).copied().collect())
    }

    /// A seam edge from vertex `a` to vertex `b` on the surface: a ruling line, a meridian arc
    /// of a sphere, or a revolution's profile turned to their angle. Returns the edge and
    /// whether it runs from `a` to `b`.
    fn seam_edge(&mut self, a: u32, b: u32, ax: &Axis, eval: &SurfaceEval, check: bool) -> R<(u32, bool)> {
        let (pa, pb) = (self.vpoint(a)?, self.vpoint(b)?);
        if pa.dist(pb) <= self.tol {
            return Err("a seam of no length".into());
        }
        let (curve, interval, along) = match &ax.kind {
            Kind::Ruled => (BCurve::Line { start: arr(pa), end: arr(pb) }, [0.0, 1.0], true),
            Kind::Sphere(r) => {
                let x = (pa - ax.origin).normalized().ok_or("a seam from the sphere's centre")?;
                let to = pb - ax.origin;
                let y = (to - x * to.dot(x)).normalized().ok_or("a seam between opposite points")?;
                let theta = to.dot(y).atan2(to.dot(x));
                if !(theta > 0.0 && theta <= PI) {
                    return Err("a degenerate seam".into());
                }
                (BCurve::Circle { frame: Frame { origin: arr(ax.origin), x_axis: arr(x), y_axis: arr(y) }, radius: *r }, [0.0, theta], true)
            }
            Kind::Revolved(profile) => {
                // a's angle; each vertex's profile parameter (a pole's from the profile's end).
                let qa = eval.project(pa, None, f64::INFINITY).ok_or("a seam vertex off its surface")?;
                let qb = eval.project(pb, Some(qa), f64::INFINITY).ok_or("a seam vertex off its surface")?;
                let solidcraft_geom::brep::Natural::Range(lo, hi) = profile.natural() else { return Err("a periodic profile".into()) };
                let vb = match ax.poles {
                    [Some(p), _] if p.dist(pb) <= self.tol => lo,
                    [_, Some(p)] if p.dist(pb) <= self.tol => hi,
                    _ => qb[1],
                };
                let curve = meridian(profile, ax.origin, ax.z, qa[0])?;
                let along = qa[1] < vb;
                (curve, if along { [qa[1], vb] } else { [vb, qa[1]] }, along)
            }
        };
        if !(interval[0] < interval[1]) {
            return Err("a degenerate seam".into());
        }
        // On the surface: its middle within the tolerance (a ruling, a meridian).
        let c = CurveEval::new(&curve).map_err(|m| m.to_string())?;
        if check {
            let m = c.point(0.5 * (interval[0] + interval[1])).ok_or("seam point")?;
            let off = eval.project(m, None, f64::INFINITY).and_then(|q| eval.point(q)).map_or(f64::INFINITY, |s| s.dist(m));
            if !(off <= self.tol) {
                return Err(unsupported("a seam off its surface: loops with no vertices at one angle"));
            }
        }
        let vertices = if along { [a, b] } else { [b, a] };
        for (t, v) in interval.into_iter().zip(vertices) {
            let gap = c.point(t).ok_or("seam point")?.dist(self.vpoint(v)?);
            if let Some(x) = self.vertices.get_mut(v as usize) {
                x.tolerance = x.tolerance.max(1.5 * gap);
            }
        }
        let i = index(self.edges.len())?;
        self.edges.push(EdgeB { curve, interval, vertices, tolerance: self.tol });
        Ok((i, along))
    }

    /// Join loop `li` of face `fi` to the pole the face covers: seam out and back.
    fn pole_seam(&mut self, fi: usize, li: usize, ax: &Axis, pole: Option<Vec3>, eval: &SurfaceEval) -> R<()> {
        let step = self.faces.get(fi).map_or(0, |f| f.step);
        let Some(p) = pole else {
            return Err(format!("face #{step}: a face around an open surface of revolution with one bound"));
        };
        let Some(LoopB::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
        let es = es.clone();
        // Start at a vertex off the pole.
        let mut k = None;
        for (i, u) in es.iter().enumerate() {
            if self.vpoint(self.start(*u)?)?.dist(p) > self.tol {
                k = Some(i);
                break;
            }
        }
        let k = k.ok_or_else(|| format!("face #{step}: a loop around the axis on its pole"))?;
        let ring = self.rotated(fi, li, k)?;
        let a = self.start(*ring.first().ok_or("empty loop")?)?;
        // The face's vertex loop at the pole, if it has one, else a new vertex there.
        let face = self.faces.get(fi).ok_or("face reference")?;
        let at_pole = face.loops.iter().enumerate().find_map(|(i, l)| match l {
            LoopB::Vertex(v) => self.vertices.get(*v as usize).filter(|x| Vec3::from(x.point).dist(p) <= self.tol.max(x.tolerance)).map(|_| (i, *v)),
            LoopB::Edges(_) => None,
        });
        let b = match at_pole {
            Some((_, v)) => v,
            None => {
                let v = index(self.vertices.len())?;
                self.vertices.push(Vertex { point: arr(p), tolerance: self.tol });
                v
            }
        };
        let (seam, along) = self.seam_edge(a, b, ax, eval, false)?;
        let mut merged = ring;
        merged.push((seam, along));
        merged.push((seam, !along));
        let face = self.faces.get_mut(fi).ok_or("face reference")?;
        if let Some(l) = face.loops.get_mut(li) {
            *l = LoopB::Edges(merged);
        }
        if let Some((i, _)) = at_pole {
            remove_loop(face, i, li);
        }
        Ok(())
    }

    /// Join loops `la` (running +u) and `lb` (running −u) of face `fi` by a seam.
    fn band_seam(&mut self, fi: usize, la: usize, lb: usize, ax: &Axis, eval: &SurfaceEval, splits: &mut usize) -> R<()> {
        let angle = |s: &Self, v: u32| -> R<f64> {
            let p = s.vpoint(v)?;
            eval.project(p, None, f64::INFINITY).map(|q| q[0]).ok_or_else(|| "a vertex off its surface".to_string())
        };
        let (Some(LoopB::Edges(a_es)), Some(LoopB::Edges(b_es))) =
            (self.faces.get(fi).and_then(|f| f.loops.get(la)), self.faces.get(fi).and_then(|f| f.loops.get(lb)))
        else {
            return Err("loop reference".into());
        };
        let (a_es, b_es) = (a_es.clone(), b_es.clone());
        // Angles agree within the tolerance at the radius they are at.
        let close = |s: &Self, va: u32, vb: u32| -> R<bool> {
            let (ua, ub) = (angle(s, va)?, angle(s, vb)?);
            let r = {
                let p = s.vpoint(va)? - ax.origin;
                (p - ax.z * p.dot(ax.z)).len()
            };
            Ok(wrap(ua - ub).abs() * r <= s.tol)
        };
        let mut pair = None;
        'search: for (i, ua) in a_es.iter().enumerate().take(100) {
            for (j, ub) in b_es.iter().enumerate().take(100) {
                if close(self, self.start(*ua)?, self.start(*ub)?)? {
                    pair = Some((i, j));
                    break 'search;
                }
            }
        }
        let (i, j) = match pair {
            Some(p) => p,
            None => {
                // Split loop B where the ruling (or meridian) through A's first vertex meets it.
                *splits += 1;
                if *splits > MAX_SPLITS {
                    return Err(format!("more than {MAX_SPLITS} edges split for seams"));
                }
                let target = angle(self, self.start(*a_es.first().ok_or("empty loop")?)?)?;
                let mut found = None;
                for &(e, _) in &b_es {
                    if let Some(t) = self.crossing(e, target, eval)? {
                        found = Some((e, t));
                        break;
                    }
                }
                let (e, t) = found.ok_or_else(|| unsupported("a band whose bounds share no angle"))?;
                let v = self.split_edge(e, t)?;
                let Some(LoopB::Edges(b_now)) = self.faces.get(fi).and_then(|f| f.loops.get(lb)) else { return Err("loop reference".into()) };
                let mut j = None;
                for (k, u) in b_now.iter().enumerate() {
                    if self.start(*u)? == v {
                        j = Some(k);
                        break;
                    }
                }
                (0, j.ok_or("the split vertex is not on its loop")?)
            }
        };
        let a_ring = self.rotated(fi, la, i)?;
        let b_ring = self.rotated(fi, lb, j)?;
        let va = self.start(*a_ring.first().ok_or("empty loop")?)?;
        let vb = self.start(*b_ring.first().ok_or("empty loop")?)?;
        let (seam, along) = self.seam_edge(va, vb, ax, eval, true)?;
        let mut merged = a_ring;
        merged.push((seam, along));
        merged.extend(b_ring);
        merged.push((seam, !along));
        let face = self.faces.get_mut(fi).ok_or("face reference")?;
        if let Some(l) = face.loops.get_mut(la) {
            *l = LoopB::Edges(merged);
        }
        remove_loop(face, lb, la);
        Ok(())
    }

    /// The parameter inside edge `e`'s interval where its curve reaches the angle `target`
    /// about the axis, if it does.
    fn crossing(&self, e: u32, target: f64, eval: &SurfaceEval) -> R<Option<f64>> {
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
        let [lo, hi] = edge.interval;
        let d = |t: f64| c.point(t).and_then(|p| eval.project(p, None, f64::INFINITY)).map(|q| wrap(q[0] - target));
        let n = 64;
        let mut prev: Option<(f64, f64)> = None;
        for k in 0..=n {
            let t = lo + (hi - lo) * k as f64 / n as f64;
            let Some(x) = d(t) else { continue };
            if let Some((t0, x0)) = prev
                && x0.signum() != x.signum()
                && x0.abs() < 1.0
                && x.abs() < 1.0
            {
                let (mut a, mut b, mut fa) = (t0, t, x0);
                for _ in 0..100 {
                    let m = 0.5 * (a + b);
                    let Some(fm) = d(m) else { break };
                    if fm.signum() == fa.signum() {
                        a = m;
                        fa = fm;
                    } else {
                        b = m;
                    }
                }
                let t = 0.5 * (a + b);
                let eps = 1e-9 * (hi - lo);
                return Ok((t > lo + eps && t < hi - eps).then_some(t));
            }
            prev = Some((t, x));
        }
        Ok(None)
    }

    /// Split edge `e` at `t` inside its interval: it keeps [min, t], a new edge takes
    /// [t, max], and every loop using it uses both. Returns the new vertex.
    fn split_edge(&mut self, e: u32, t: f64) -> R<u32> {
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
        let p = c.point(t).ok_or("split point")?;
        let (curve, [lo, hi], [a, b], tolerance) = (edge.curve.clone(), edge.interval, edge.vertices, edge.tolerance);
        if !(t > lo && t < hi) {
            return Err("a split outside its edge".into());
        }
        let v = index(self.vertices.len())?;
        self.vertices.push(Vertex { point: arr(p), tolerance: self.tol });
        let e2 = index(self.edges.len())?;
        if let Some(x) = self.edges.get_mut(e as usize) {
            x.interval = [lo, t];
            x.vertices = [a, v];
        }
        self.edges.push(EdgeB { curve, interval: [t, hi], vertices: [v, b], tolerance });
        for f in &mut self.faces {
            for l in &mut f.loops {
                let LoopB::Edges(es) = l else { continue };
                if !es.iter().any(|u| u.0 == e) {
                    continue;
                }
                let mut out = Vec::with_capacity(es.len() + 1);
                for &(x, forward) in es.iter() {
                    if x != e {
                        out.push((x, forward));
                    } else if forward {
                        out.extend([(e, true), (e2, true)]);
                    } else {
                        out.extend([(e2, false), (e, false)]);
                    }
                }
                *es = out;
            }
        }
        Ok(v)
    }
}

/// Remove loop `gone` from a face whose loop `kept` absorbed it, keeping its outer loop marked.
fn remove_loop(face: &mut super::FaceB, gone: usize, kept: usize) {
    if gone >= face.loops.len() {
        return;
    }
    face.loops.remove(gone);
    face.outer = match face.outer {
        Some(o) if o == gone => Some(if kept > gone { kept - 1 } else { kept }),
        Some(o) if o > gone => Some(o - 1),
        o => o,
    };
}
