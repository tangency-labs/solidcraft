//! The minimum distance between two bodies, on their exact faces and edges.
//!
//! The display meshes find where the bodies come nearest and prune pairs that are far apart:
//! the meshes lie within their chord tolerance of the faces, so a mesh distance bounds the exact
//! one from both sides. The answer itself comes from the exact geometry. Near the nearest mesh
//! points, each body offers the face there and the edges of that face that pass close by; the
//! points are moved in turn to the nearest point of one feature from the other (alternating
//! projection) until they settle, on analytic surfaces in closed form, on other surfaces through
//! the record's point inversion ([`SurfaceEval::project`]) and on curves by a bracketed search of
//! the record curve's own points. A point that settles off its face (past the
//! face's boundary) is dropped: the face's edges, offered too, then carry the minimum.

use std::collections::HashMap;
use std::sync::OnceLock;

use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::mesh_distance::{MeshClosest, TriangleSet};
use solidcraft_geom::{Aabb3, Mesh, Vec3};

use crate::body::Body;
use crate::surface_kind::{FaceSurface, surface_of};
use crate::{Result, guard};

/// The minimum distance between two bodies and the points where it is reached.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyDistance {
    pub distance: f64,
    /// On the first body.
    pub from: Vec3,
    /// On the second body.
    pub to: Vec3,
    /// Measured on the exact faces and edges (false: on the meshes, for a mesh body or where the
    /// exact geometry gave no answer).
    pub exact: bool,
}

/// A body made ready for distance queries: its mesh's triangles under a hierarchy, and its faces
/// and edges as they are first needed. Share one between the queries that involve its body.
pub struct DistanceShape<'a> {
    body: &'a Body,
    mesh: &'a Mesh,
    set: TriangleSet,
    chord: f64,
    faces: Vec<Option<SurfaceEval>>,
    edges: Vec<Option<(CurveEval, [f64; 2])>>,
    surfaces: Vec<OnceLock<Option<FaceSurface>>>,
    face_tris: OnceLock<HashMap<u32, Vec<u32>>>,
}

/// Most face pairs near the nearest mesh points whose exact distance is worked out.
const MAX_FACE_PAIRS: usize = 24;
/// Most projection rounds for one pair of features.
const MAX_ROUNDS: usize = 400;
/// A point less than this fraction of its distance from an edge off the line through it is
/// straight along the edge (see [`DistanceShape::on_face`]).
const ALONG: f64 = 1e-3;

impl<'a> DistanceShape<'a> {
    /// `mesh` is `body`'s mesh, within `chord` (mm) of its faces.
    pub fn new(body: &'a Body, mesh: &'a Mesh, chord: f64) -> DistanceShape<'a> {
        let record = body.record();
        let faces: Vec<Option<SurfaceEval>> = record.faces.iter().map(|f| SurfaceEval::new(&f.surface).ok()).collect();
        let edges = record.edges.iter().map(|e| CurveEval::new(&e.curve).ok().map(|c| (c, e.interval))).collect();
        let surfaces = faces.iter().map(|_| OnceLock::new()).collect();
        let chord = if chord.is_finite() && chord > 0.0 { chord } else { 0.0 };
        DistanceShape { body, mesh, set: TriangleSet::new(mesh), chord, faces, edges, surfaces, face_tris: OnceLock::new() }
    }

    /// The box around the body: its mesh's, grown by the chord tolerance.
    pub fn bounds(&self) -> Aabb3 {
        self.set.bounds().inflated(self.chord)
    }

    /// How far the mesh may lie from the faces (mm).
    pub fn chord(&self) -> f64 {
        self.chord
    }

    /// The nearest points of the two meshes, if they come within `limit`.
    pub fn mesh_closest(&self, other: &DistanceShape, limit: f64) -> Option<MeshClosest> {
        self.set.closest(&other.set, limit)
    }

    fn surface(&self, face: usize) -> Option<FaceSurface> {
        *self.surfaces.get(face)?.get_or_init(|| surface_of(self.body.record(), face))
    }

    fn face_triangles(&self, face: u32) -> &[u32] {
        let map = self.face_tris.get_or_init(|| {
            let mut m: HashMap<u32, Vec<u32>> = HashMap::new();
            for t in 0..self.set.len() as u32 {
                if let Some(f) = self.set.face(t) {
                    m.entry(f).or_default().push(t);
                }
            }
            m
        });
        map.get(&face).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Does `p`, a point on face `face`'s surface, lie on the face itself? Its mesh rules out
    /// points farther than twice the chord. Nearer the face's boundary than that, the exact edge
    /// nearest `p` decides: `p` must lie on the side of it where the face's mesh is.
    fn on_face(&self, face: u32, p: Vec3) -> bool {
        let tris = self.face_triangles(face);
        let tiny = 1e-9 * self.body.size().max(1.0);
        let reach = 2.0 * self.chord + tiny;
        if self.set.distance_to(p, tris) > reach {
            return false;
        }
        // The exact edges of the face near `p`: their nearest points and directions there.
        let mut near: Vec<(f64, Vec3, Vec3)> = Vec::new();
        for e in self.mesh.face_edges(face) {
            let Some(line) = self.mesh.edges.get(e) else { continue };
            let rough = match line.as_slice() {
                [one] => one.dist(p),
                _ => line.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min),
            };
            if rough > 2.0 * reach {
                continue;
            }
            if let Some((at, dir)) = self.edge_nearest(e, p, &mut Hint::default()) {
                near.push((at.dist(p), at, dir));
            }
        }
        let Some(least) = near.iter().map(|n| n.0).reduce(f64::min) else { return true };
        if least <= tiny {
            return true;
        }
        // The nearest edge (any of them, where several meet at a vertex) with the face's mesh on
        // `p`'s side. An edge with the face on both sides (a seam inside it) says nothing.
        let mut decided = false;
        for &(_, at, dir) in near.iter().filter(|n| n.0 <= least + tiny) {
            let Some(t) = dir.normalized() else { continue };
            let across = |x: Vec3| {
                let v = x - at;
                v - t * v.dot(t)
            };
            let sides: Vec<Vec3> = tris
                .iter()
                .filter_map(|&k| self.set.triangle(k))
                .filter(|[a, b, c]| solidcraft_geom::compare::closest_on_triangle(at, *a, *b, *c).dist(at) <= reach)
                .map(|[a, b, c]| across((a + b + c) / 3.0))
                .filter(|w| w.len() > tiny)
                .collect();
            let Some(inward) = sides.iter().copied().max_by(|x, y| x.len().total_cmp(&y.len())) else { continue };
            if sides.iter().any(|w| w.dot(inward) < 0.0) {
                continue;
            }
            // `p` straight along the edge (past its end, on the line through it) says nothing
            // about which side it is on: another edge meeting there decides. Straight along is
            // relative to how far `p` is: past the end, what is left across the edge is the
            // edge's own tilt and rounding, and its side is chance (#2283).
            let off = across(p);
            if off.len() <= tiny.max(ALONG * at.dist(p)) {
                continue;
            }
            decided = true;
            if off.dot(inward) >= 0.0 {
                return true;
            }
        }
        !decided
    }

    /// The features near mesh point `p` on face `face`: the face, and its edges within twice the chord.
    fn features(&self, face: u32, p: Vec3) -> Vec<Feature> {
        let mut out = vec![Feature::Face(face as usize)];
        let reach = 2.0 * self.chord + 1e-9 * self.body.size().max(1.0);
        for e in self.mesh.face_edges(face) {
            let Some(line) = self.mesh.edges.get(e) else { continue };
            let near = match line.as_slice() {
                [one] => one.dist(p),
                _ => line.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min),
            };
            if near <= reach && e < self.edges.len() {
                out.push(Feature::Edge(e));
            }
        }
        out
    }

    /// The point of `f` nearest `p`, with the parameter hint for the next call.
    fn project(&self, f: Feature, p: Vec3, hint: &mut Hint) -> Option<Vec3> {
        match f {
            Feature::Face(i) => {
                if let Some(q) = self.surface(i).and_then(|s| s.nearest(p)) {
                    return Some(q);
                }
                let s = self.faces.get(i)?.as_ref()?;
                let uv = s.project(p, hint.uv, f64::INFINITY).or_else(|| s.project(p, None, f64::INFINITY))?;
                hint.uv = Some(uv);
                s.point(uv).filter(|q| q.is_finite())
            }
            Feature::Edge(i) => self.edge_nearest(i, p, hint).map(|(q, _)| q),
        }
    }

    /// The point of edge `i` nearest `p`, with the edge's direction there: the nearest of evenly
    /// spaced points along it (or the last call's), narrowed between its neighbours by golden
    /// section on the record curve's own points.
    fn edge_nearest(&self, i: usize, p: Vec3, hint: &mut Hint) -> Option<(Vec3, Vec3)> {
        let (c, [t0, t1]) = self.edges.get(i)?.as_ref()?;
        let (t0, t1) = (*t0, *t1);
        let dist = |t: f64| c.point(t).map_or(f64::INFINITY, |q| q.dist(p));
        let n = 32;
        let step = (t1 - t0) / f64::from(n);
        let start = match hint.t {
            Some(t) if dist(t).is_finite() => t,
            _ => (0..=n).map(|k| t0 + step * f64::from(k)).min_by(|a, b| dist(*a).total_cmp(&dist(*b)))?,
        };
        let (mut lo, mut hi) = ((start - step).max(t0), (start + step).min(t1));
        let golden = 0.5 * (5f64.sqrt() - 1.0);
        for _ in 0..80 {
            let (x, y) = (hi - golden * (hi - lo), lo + golden * (hi - lo));
            if dist(x) <= dist(y) {
                hi = y;
            } else {
                lo = x;
            }
        }
        let t = [0.5 * (lo + hi), t0, t1].into_iter().min_by(|a, b| dist(*a).total_cmp(&dist(*b)))?;
        hint.t = Some(t);
        let q = c.point(t)?;
        let h = (t1 - t0) * 1e-6;
        let dir = match c.derivs(t) {
            Some((_, d)) => d,
            None => c.point((t + h).min(t1))? - c.point((t - h).max(t0))?,
        };
        q.is_finite().then_some((q, dir))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Feature {
    Face(usize),
    Edge(usize),
}

#[derive(Clone, Copy, Default)]
struct Hint {
    uv: Option<[f64; 2]>,
    t: Option<f64>,
}

/// Settle a pair of points on features `fa` of `a` and `fb` of `b`, starting from `pb` on `b`.
fn settle(a: &DistanceShape, fa: Feature, b: &DistanceShape, fb: Feature, pb: Vec3) -> Option<(f64, Vec3, Vec3)> {
    let (mut ha, mut hb) = (Hint::default(), Hint::default());
    let scale = a.body.size().max(b.body.size()).max(1.0);
    let mut q = b.project(fb, pb, &mut hb)?;
    let mut p = a.project(fa, q, &mut ha)?;
    for _ in 0..MAX_ROUNDS {
        let q2 = b.project(fb, p, &mut hb)?;
        let p2 = a.project(fa, q2, &mut ha)?;
        let moved = p2.dist(p).max(q2.dist(q));
        p = p2;
        q = q2;
        if moved <= 1e-12 * scale {
            break;
        }
    }
    let d = p.dist(q);
    d.is_finite().then_some((d, p, q))
}

/// The minimum distance between the bodies of `a` and `b`, when it may be within `limit` (mm;
/// `f64::INFINITY` for any): `None` when the meshes show it is not. Overlapping bodies are not
/// told apart from touching ones here (both give 0 or the distance between their surfaces):
/// an intersection says whether they share volume.
pub fn body_distance(a: &DistanceShape, b: &DistanceShape, limit: f64) -> Result<Option<BodyDistance>> {
    let slack = 2.0 * (a.chord + b.chord);
    let Some(m) = a.mesh_closest(b, if limit.is_finite() { limit + slack } else { f64::INFINITY }) else { return Ok(None) };
    let mesh = BodyDistance { distance: m.distance, from: m.a, to: m.b, exact: false };
    if a.body.is_mesh() || b.body.is_mesh() {
        return Ok(Some(mesh));
    }
    // The face pairs near the nearest mesh points, each with its nearest triangle pair.
    let mut seeds: HashMap<(u32, u32), MeshClosest> = HashMap::new();
    a.set.within(&b.set, m.distance + slack, |c| {
        let (Some(fa), Some(fb)) = (a.set.face(c.tri_a), b.set.face(c.tri_b)) else { return };
        let e = seeds.entry((fa, fb)).or_insert(c);
        if c.distance < e.distance {
            *e = c;
        }
    });
    let mut seeds: Vec<((u32, u32), MeshClosest)> = seeds.into_iter().collect();
    seeds.sort_by(|x, y| x.1.distance.total_cmp(&y.1.distance).then(x.0.cmp(&y.0)));
    seeds.truncate(MAX_FACE_PAIRS);
    let best = guard("body distance", || {
        let mut best: Option<BodyDistance> = None;
        let mut tried = std::collections::HashSet::new();
        for ((fa, fb), c) in &seeds {
            for xa in a.features(*fa, c.a) {
                for xb in b.features(*fb, c.b) {
                    if !tried.insert((xa, xb)) {
                        continue;
                    }
                    let Some((d, p, q)) = settle(a, xa, b, xb, c.b) else { continue };
                    let holds = |s: &DistanceShape, f: Feature, x: Vec3| match f {
                        Feature::Face(i) => s.on_face(i as u32, x),
                        Feature::Edge(_) => true,
                    };
                    if holds(a, xa, p) && holds(b, xb, q) && best.is_none_or(|bd| d < bd.distance) {
                        best = Some(BodyDistance { distance: d, from: p, to: q, exact: true });
                    }
                }
            }
        }
        Ok(best)
    })?;
    // The exact minimum lies within the chords of the mesh's; an answer outside that went astray.
    let chords = a.chord + b.chord + 1e-9 * a.body.size().max(b.body.size()).max(1.0);
    Ok(Some(match best {
        Some(e) if e.distance <= m.distance + chords && e.distance >= m.distance - chords => e,
        _ => mesh,
    }))
}
