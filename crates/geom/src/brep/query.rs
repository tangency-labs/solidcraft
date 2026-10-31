//! Exact queries on a record: points and tangents along its edges, how far a point is from an
//! edge, where a point lies on its faces and which way they face there, and which faces a ray
//! meets. Every answer comes from the record's own curves, surfaces and pcurves; nothing here
//! meshes the body. Build a [`Query`] once per record: it keeps each curve and surface ready to
//! evaluate.

use std::cell::OnceCell;

use super::eval::{CurveEval, SurfaceEval};
use super::{Brep, Loop, Shell, Surface};
use crate::Vec3;

/// Most pieces an edge is cut into by [`Query::edge_points`].
const MAX_EDGE_POINTS: usize = 4096;
/// Most halvings of one piece of an edge in [`Query::edge_points`].
const MAX_DEPTH: u32 = 16;
/// Points each pcurve is drawn with in a face's parameters, to tell inside from outside.
const PCURVE_POINTS: usize = 64;
/// Newton steps of [`Query::edge_distance`].
const MAX_NEWTON: usize = 32;

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// A record ready for exact queries (see the module docs).
pub struct Query<'a> {
    record: &'a Brep,
    curves: Vec<Option<CurveEval>>,
    surfaces: Vec<Option<SurfaceEval>>,
    /// Whether each face's shell uses it along its natural normal (`None` for a face no shell
    /// lists, which a validated record has none of).
    forward: Vec<Option<bool>>,
    /// Each face's loops in its own parameters, drawn when first asked for.
    loops: Vec<OnceCell<Vec<Vec<[f64; 2]>>>>,
}

impl<'a> Query<'a> {
    pub fn new(record: &'a Brep) -> Query<'a> {
        let mut forward = vec![None; record.faces.len()];
        for shell in &record.shells {
            if let Shell::Faces { faces, .. } = shell {
                for of in faces {
                    if let Some(slot) = forward.get_mut(of.face as usize) {
                        *slot = Some(of.forward);
                    }
                }
            }
        }
        Query {
            record,
            curves: record.edges.iter().map(|e| CurveEval::new(e.parameters()).ok()).collect(),
            surfaces: record.faces.iter().map(|f| SurfaceEval::new(f.parameters()).ok()).collect(),
            forward,
            loops: record.faces.iter().map(|_| OnceCell::new()).collect(),
        }
    }

    pub fn record(&self) -> &'a Brep {
        self.record
    }

    /// Edge `e`'s point and derivative at `t` on its parameter. `None` for a traced
    /// intersection curve (its tangent is not held).
    pub fn edge_at(&self, e: usize, t: f64) -> Option<(Vec3, Vec3)> {
        self.curves.get(e)?.as_ref()?.derivs(t)
    }

    /// Edge `e`'s interval on its parameter, from its start vertex to its end vertex.
    pub fn edge_interval(&self, e: usize) -> Option<[f64; 2]> {
        Some(self.record.edges.get(e)?.interval)
    }

    /// The faces edge `e` bounds, once for each time a loop uses it (a periodic face's seam twice).
    pub fn edge_faces(&self, e: usize) -> Vec<usize> {
        self.record.edges.get(e).map_or_else(Vec::new, |edge| edge.uses.iter().map(|u| u.face as usize).collect())
    }

    /// Points of edge `e` from its start to its end, each with its parameter: the ends, and
    /// between them as many as keep the curve within `sag` mm of the chord between two
    /// neighbours and turning less than a twentieth of a turn between them. A line is its two
    /// ends.
    pub fn edge_points(&self, e: usize, sag: f64) -> Option<Vec<(f64, Vec3)>> {
        let curve = self.curves.get(e)?.as_ref()?;
        let [t0, t1] = self.edge_interval(e)?;
        let at = |t: f64| curve.point(t);
        let (p0, p1) = (at(t0)?, at(t1)?);
        if matches!(curve, CurveEval::Line { .. }) {
            return Some(vec![(t0, p0), (t1, p1)]);
        }
        let sag = sag.max(1e-12);
        let turn = (std::f64::consts::TAU / 20.0).cos();
        let mut out = vec![(t0, p0)];
        // Pieces still to look at; the last pushed is looked at first, so pushing the right half
        // before the left keeps the output in order. A closed edge is halved before it is looked at.
        let mut todo = vec![(t0, p0, t1, p1, 0u32)];
        while let Some((a, pa, b, pb, depth)) = todo.pop() {
            let m = 0.5 * (a + b);
            let pm = at(m)?;
            let chord = pb - pa;
            let off = if chord.len() > 0.0 { (pm - pa).cross(chord).len() / chord.len() } else { pm.dist(pa) };
            let bent = match (curve.derivs(a), curve.derivs(b)) {
                (Some((_, da)), Some((_, db))) => da.normalized().zip(db.normalized()).is_none_or(|(x, y)| x.dot(y) < turn),
                _ => false,
            };
            if (off > sag || bent || (depth == 0 && pa.dist(pb) <= sag)) && depth < MAX_DEPTH && out.len() + todo.len() < MAX_EDGE_POINTS {
                todo.push((m, pm, b, pb, depth + 1));
                todo.push((a, pa, m, pm, depth + 1));
            } else {
                out.push((b, pb));
            }
        }
        Some(out)
    }

    /// How far `p` is from edge `e`: from its nearest point, found from the nearest of its
    /// [`Query::edge_points`] and Newton's method on the curve itself.
    pub fn edge_distance(&self, e: usize, p: Vec3, sag: f64) -> Option<f64> {
        let points = self.edge_points(e, sag)?;
        let [t0, t1] = self.edge_interval(e)?;
        let (mut t, mut best) = points.iter().map(|(t, q)| (*t, q.dist(p))).min_by(|x, y| x.1.total_cmp(&y.1))?;
        for _ in 0..MAX_NEWTON {
            let Some((q, d)) = self.edge_at(e, t) else { break };
            best = best.min(q.dist(p));
            let dd = d.dot(d);
            if !(dd > 1e-300) {
                break;
            }
            let next = (t - (q - p).dot(d) / dd).clamp(t0, t1);
            if (next - t).abs() <= 1e-15 * (1.0 + t.abs()) {
                break;
            }
            t = next;
        }
        Some(best)
    }

    /// The parameters on face `f` of `p`, a point within `accept` mm of its surface, brought
    /// into the face's own domain where the surface turns round.
    pub fn face_uv(&self, f: usize, p: Vec3, accept: f64) -> Option<[f64; 2]> {
        let s = self.surfaces.get(f)?.as_ref()?;
        let face = self.record.faces.get(f)?;
        let mut uv = s.project(p, None, accept)?;
        // A plane's projection takes any point: this one must lie on the surface.
        if !s.point(uv).is_some_and(|q| q.dist(p) <= accept) {
            return None;
        }
        let periods = s.domain().period;
        for (k, slot) in uv.iter_mut().enumerate() {
            if let (Some(period), Some([lo, hi])) = (periods.get(k).copied().flatten(), face.domain.get(k)) {
                let middle = 0.5 * (lo + hi);
                *slot += ((middle - *slot) / period).round() * period;
            }
        }
        Some(uv)
    }

    /// Face `f`'s unit normal at its parameters `uv`, out of the material: the natural normal,
    /// turned where the face's shell uses the face against it.
    pub fn face_normal(&self, f: usize, uv: [f64; 2]) -> Option<Vec3> {
        let (_, su, sv) = self.surfaces.get(f)?.as_ref()?.derivs(uv[0], uv[1])?;
        let n = su.cross(sv).normalized()?;
        Some(if self.forward.get(f).copied().flatten()? { n } else { -n })
    }

    /// Whether `p` lies on face `f`: within `accept` mm of its surface, and inside its outer
    /// loop and none of its holes, or on one of its loops (a face holds its own edges).
    pub fn face_contains(&self, f: usize, p: Vec3, accept: f64) -> bool {
        self.face_uv(f, p, accept).is_some_and(|uv| self.inside(f, uv) || self.on_loops(f, uv, accept))
    }

    /// The faces `p` lies on (see [`Query::face_contains`]), nearest first, each with how far
    /// `p` is from it.
    pub fn faces_at(&self, p: Vec3, accept: f64) -> Vec<(f64, usize)> {
        let mut out: Vec<(f64, usize)> = (0..self.record.faces.len())
            .filter_map(|f| {
                let uv = self.face_uv(f, p, accept)?;
                let q = self.surfaces.get(f)?.as_ref()?.point(uv)?;
                (self.inside(f, uv) || self.on_loops(f, uv, accept)).then_some((q.dist(p), f))
            })
            .collect();
        out.sort_by(|x, y| x.0.total_cmp(&y.0));
        out
    }

    /// Where the ray from `o` along `d` meets the faces ahead of `o` (further than `accept`):
    /// each meeting's distance along `d` (a unit vector), face and point, nearest first. Faces on
    /// planes, cylinders, cones and spheres are met exactly; faces on other surfaces are not
    /// looked at yet.
    pub fn ray(&self, o: Vec3, d: Vec3, accept: f64) -> Vec<(f64, usize, Vec3)> {
        let Some(d) = d.normalized() else { return Vec::new() };
        let mut out = Vec::new();
        for (f, face) in self.record.faces.iter().enumerate() {
            for s in ray_surface(&face.surface, o, d) {
                let q = o + d * s;
                if s > accept && self.face_contains(f, q, accept) {
                    out.push((s, f, q));
                }
            }
        }
        out.sort_by(|x, y| x.0.total_cmp(&y.0));
        out
    }

    /// Whether `uv` lies inside face `f`'s outer loop and outside its holes, by winding number on
    /// its loops drawn from their pcurves.
    fn inside(&self, f: usize, uv: [f64; 2]) -> bool {
        let Some(cell) = self.loops.get(f) else { return false };
        let loops = cell.get_or_init(|| self.draw_loops(f));
        let mut it = loops.iter();
        let Some(outer) = it.next() else { return false };
        winding(outer, uv) != 0 && it.all(|hole| winding(hole, uv) == 0)
    }

    /// Whether `uv` lies on one of face `f`'s loops: within `accept` mm of it, read in the
    /// face's parameters through the surface's slowest rate there.
    fn on_loops(&self, f: usize, uv: [f64; 2], accept: f64) -> bool {
        let (Some(cell), Some(Some(s))) = (self.loops.get(f), self.surfaces.get(f)) else { return false };
        let Some((_, su, sv)) = s.derivs(uv[0], uv[1]) else { return false };
        let rate = su.len().min(sv.len());
        if !(rate > 0.0) {
            return false;
        }
        let reach = accept / rate;
        let loops = cell.get_or_init(|| self.draw_loops(f));
        loops.iter().any(|ring| {
            let n = ring.len();
            (0..n).any(|i| match (ring.get(i), ring.get((i + 1) % n)) {
                (Some(a), Some(b)) => to_segment(uv, *a, *b) <= reach,
                _ => false,
            })
        })
    }

    /// Face `f`'s edge loops in its parameters, each run as the loop runs, the outer one first.
    fn draw_loops(&self, f: usize) -> Vec<Vec<[f64; 2]>> {
        let Some(face) = self.record.faces.get(f) else { return Vec::new() };
        face.loops
            .iter()
            .filter_map(|l| match l {
                Loop::Edges { edges } => Some(edges),
                Loop::Vertex { .. } => None,
            })
            .map(|edges| {
                let mut ring = Vec::new();
                for oe in edges {
                    let Some([t0, t1]) = oe.pcurve.interval() else { continue };
                    let mut pts: Vec<[f64; 2]> =
                        (0..PCURVE_POINTS).filter_map(|k| oe.pcurve.eval(t0 + (t1 - t0) * k as f64 / PCURVE_POINTS as f64)).collect();
                    pts.extend(oe.pcurve.eval(t1));
                    if !oe.forward {
                        pts.reverse();
                    }
                    // Each pcurve's last point is the next one's first.
                    pts.pop();
                    ring.extend(pts);
                }
                ring
            })
            .collect()
    }
}

/// How far `p` is from the segment `a`–`b`, in the plane of the parameters.
fn to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - a[0] - dx * t).hypot(p[1] - a[1] - dy * t)
}

/// How many times `ring` winds round `p` (0 outside).
fn winding(ring: &[[f64; 2]], p: [f64; 2]) -> i32 {
    let n = ring.len();
    let mut w = 0;
    for i in 0..n {
        let (Some(a), Some(b)) = (ring.get(i), ring.get((i + 1) % n)) else { continue };
        let side = (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1]);
        if a[1] <= p[1] {
            if b[1] > p[1] && side > 0.0 {
                w += 1;
            }
        } else if b[1] <= p[1] && side < 0.0 {
            w -= 1;
        }
    }
    w
}

/// Where the line `o + s d` (unit `d`) meets `surface`: the values of `s`. Planes, cylinders,
/// cones and spheres; nothing for other surfaces.
fn ray_surface(surface: &Surface, o: Vec3, d: Vec3) -> Vec<f64> {
    let roots = |a: f64, b: f64, c: f64| -> Vec<f64> {
        if a.abs() <= 1e-300 {
            return if b.abs() > 1e-300 { vec![-c / b] } else { Vec::new() };
        }
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return Vec::new();
        }
        // The root that doesn't cancel first, the other from their product.
        let q = -0.5 * (b + b.signum() * disc.sqrt());
        let mut r = vec![q / a];
        if q.abs() > 1e-300 {
            r.push(c / q);
        }
        r
    };
    let plane = |origin: Vec3, n: Vec3| -> Vec<f64> {
        let den = d.dot(n);
        if den.abs() <= 1e-15 * n.len() { Vec::new() } else { vec![(origin - o).dot(n) / den] }
    };
    match surface {
        Surface::Plane { origin, u_axis, v_axis } => plane(v3(*origin), v3(*u_axis).cross(v3(*v_axis))),
        Surface::PlaneThrough { origin, u_point, v_point } => {
            let c = v3(*origin);
            plane(c, (v3(*u_point) - c).cross(v3(*v_point) - c))
        }
        Surface::Cylinder { frame, radius } => {
            let (c, z) = (v3(frame.origin), v3(frame.x_axis).cross(v3(frame.y_axis)));
            let (w, dd) = ((o - c) - z * (o - c).dot(z), d - z * d.dot(z));
            roots(dd.dot(dd), 2.0 * w.dot(dd), w.dot(w) - radius * radius)
        }
        Surface::Cone { frame, radius_at_origin, half_angle } => {
            // r(v) = r0 + v tan(half), v along z; the radial distance squared is r(v)².
            let (c, z) = (v3(frame.origin), v3(frame.x_axis).cross(v3(frame.y_axis)));
            let k = half_angle.tan();
            let (w, dd) = ((o - c) - z * (o - c).dot(z), d - z * d.dot(z));
            let (h, dh) = ((o - c).dot(z), d.dot(z));
            let (r, dr) = (radius_at_origin + h * k, dh * k);
            roots(dd.dot(dd) - dr * dr, 2.0 * (w.dot(dd) - r * dr), w.dot(w) - r * r)
                .into_iter()
                .filter(|s| radius_at_origin + (h + dh * s) * k >= 0.0)
                .collect()
        }
        Surface::Sphere { frame, radius } => {
            let w = o - v3(frame.origin);
            roots(1.0, 2.0 * w.dot(d), w.dot(w) - radius * radius)
        }
        _ => Vec::new(),
    }
}
