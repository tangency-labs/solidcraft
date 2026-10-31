//! The minimum distance between two bodies, on their exact faces and edges.
//!
//! The display meshes find where the bodies come nearest and prune pairs that are far apart:
//! the meshes lie within their chord tolerance of the faces, so a mesh distance bounds the exact
//! one from both sides. The answer itself comes from the exact geometry. Near the nearest mesh
//! points, each body offers the face there and the edges of that face that pass close by; the
//! points are moved in turn to the nearest point of one feature from the other (alternating
//! projection) until they settle, on analytic surfaces in closed form and on other surfaces and
//! curves through the kernel's point inversion. A point that settles off its face (past the
//! face's boundary) is dropped: the face's edges, offered too, then carry the minimum.

use std::collections::HashMap;
use std::sync::OnceLock;

use solidcraft_geom::mesh_distance::{MeshClosest, TriangleSet};
use solidcraft_geom::{Aabb3, Mesh, Vec3};
use truck_modeling as mt;

use crate::body::{Body, from_p3, p3};
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
    faces: Vec<mt::Face>,
    edges: Vec<mt::Edge>,
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
        let exact = body.mesh.is_none();
        let faces: Vec<mt::Face> = if exact { body.solid.face_iter().cloned().collect() } else { Vec::new() };
        let edges = if exact { Body::unique_edges(&body.solid) } else { Vec::new() };
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
        *self.surfaces.get(face)?.get_or_init(|| self.faces.get(face).and_then(|f| surface_of(f, self.body.size())))
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
        use mt::{ParametricSurface, SearchNearestParameter};
        match f {
            Feature::Face(i) => {
                if let Some(q) = self.surface(i).and_then(|s| s.nearest(p)) {
                    return Some(q);
                }
                let s = self.faces.get(i)?.oriented_surface();
                let uv = s.search_nearest_parameter(p3(p), hint.uv, 100).or_else(|| s.search_nearest_parameter(p3(p), None, 100))?;
                hint.uv = Some(uv);
                Some(from_p3(s.subs(uv.0, uv.1))).filter(|q| q.is_finite())
            }
            Feature::Edge(i) => self.edge_nearest(i, p, hint).map(|(q, _)| q),
        }
    }

    /// The point of edge `i` nearest `p`, with the edge's direction there.
    fn edge_nearest(&self, i: usize, p: Vec3, hint: &mut Hint) -> Option<(Vec3, Vec3)> {
        use mt::{BoundedCurve, ParametricCurve, SearchNearestParameter};
        let c = self.edges.get(i)?.oriented_curve();
        let (t0, t1) = c.range_tuple();
        let (lo, hi) = (t0.min(t1), t0.max(t1));
        let at = |t: f64| from_p3(c.subs(t));
        let start = match hint.t {
            Some(t) => t,
            None => {
                // A coarse look along the curve, then the inversion from the best sample.
                let n = 32;
                (0..=n).map(|k| t0 + (t1 - t0) * f64::from(k) / f64::from(n)).min_by(|a, b| at(*a).dist(p).total_cmp(&at(*b).dist(p)))?
            }
        };
        let mut best = (start.clamp(lo, hi), f64::INFINITY);
        best.1 = at(best.0).dist(p);
        for t in [c.search_nearest_parameter(p3(p), Some(start), 64).map(|t| t.clamp(lo, hi)), Some(t0), Some(t1)].into_iter().flatten() {
            let d = at(t).dist(p);
            if d < best.1 {
                best = (t, d);
            }
        }
        hint.t = Some(best.0);
        let d = c.der(best.0);
        let q = at(best.0);
        q.is_finite().then_some((q, Vec3::new(d.x, d.y, d.z)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Feature {
    Face(usize),
    Edge(usize),
}

#[derive(Clone, Copy, Default)]
struct Hint {
    uv: Option<(f64, f64)>,
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
    if a.faces.is_empty() || b.faces.is_empty() {
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

/// How two bodies meet, from their exact faces: see [`contact`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Contact {
    /// Their faces stay at least this far apart (mm), more than round-off: they share no point.
    Apart(f64),
    /// Two faces cross each other at a point inside both, at an angle: each passes into the
    /// other body there, so they share volume however little.
    Crossing,
    /// Neither, or not known: they touch, or the exact geometry could not tell.
    Touching,
}

/// Faces meeting at less than this angle (radians) are taken for tangent.
const CROSSING_ANGLE: f64 = 5e-3;
/// How far from square to a feature a gap may lean and count as its least (radians): along a
/// round face the gap could close by no more than its radius times half this squared, a few
/// tenths of a nanometre on a part tens of millimetres across.
const SQUARE: f64 = 1e-4;
/// Most places near where the bodies meet that [`contact`] looks at.
const MAX_CONTACT_SEEDS: usize = 256;

/// A face's surface or an edge's curve (with its parameter range), for [`nearest_points`].
enum Geo {
    Surface(mt::Surface),
    Curve(mt::Curve, f64, f64),
}

impl Geo {
    fn dims(&self) -> usize {
        match self {
            Geo::Surface(_) => 2,
            Geo::Curve(..) => 1,
        }
    }

    fn point(&self, x: &[f64]) -> Option<Vec3> {
        use mt::{ParametricCurve, ParametricSurface};
        let p = match self {
            Geo::Surface(s) => from_p3(s.subs(*x.first()?, *x.get(1)?)),
            Geo::Curve(c, ..) => from_p3(c.subs(*x.first()?)),
        };
        p.is_finite().then_some(p)
    }

    /// The derivatives along each parameter, by central differences of [`Geo::point`]: the
    /// steps then follow the points exactly as they are evaluated, whatever the surface's own
    /// derivatives make of a reversed or reparametrized face.
    fn tangents(&self, x: &[f64]) -> Option<Vec<Vec3>> {
        let mut out = Vec::with_capacity(x.len());
        for i in 0..x.len() {
            let h = 1e-6 * x.get(i)?.abs().max(1.0);
            let (mut lo, mut hi) = (x.to_vec(), x.to_vec());
            *lo.get_mut(i)? -= h;
            *hi.get_mut(i)? += h;
            out.push((self.point(&hi)? - self.point(&lo)?) * (0.5 / h));
        }
        Some(out)
    }

    /// Keeps a curve's parameter on the edge; says whether it was held at an end.
    fn clamp(&self, x: &mut [f64]) -> bool {
        match (self, x.first_mut()) {
            (Geo::Curve(_, lo, hi), Some(t)) => {
                let held = *t <= *lo || *t >= *hi;
                *t = t.clamp(*lo, *hi);
                held
            }
            _ => false,
        }
    }
}

/// The nearest points of two features (faces' surfaces or edges' curves) from parameters `x`
/// (those of `ga`, then those of `gb`): least squares on the gap between them (Levenberg-Marquardt,
/// an edge's parameter held on the edge). Alternate projections creep where faces meet at a
/// shallow angle or nearly touch (each round takes off a fraction of the gap that goes to nothing
/// with the angle) and stop short with a gap that is not there; these steps close it, or settle
/// where it is least. Returns the gap, both points, and whether it settled: the gap is gone
/// (within `eps`), or it is square to both features (an edge held at an end excepted).
fn nearest_points(ga: &Geo, gb: &Geo, mut x: Vec<f64>, eps: f64) -> Option<(f64, Vec3, Vec3, bool)> {
    let na = ga.dims();
    let n = x.len();
    if n != na + gb.dims() || n > 4 {
        return None;
    }
    let eval = |x: &[f64]| -> Option<(Vec3, Vec3)> { Some((ga.point(x.get(..na)?)?, gb.point(x.get(na..)?)?)) };
    // Square to a feature: the gap runs across each of its directions (an edge held at an end
    // excepted).
    let square = |g: &Geo, x: &mut [f64], r: Vec3| -> bool {
        let held = g.clamp(x);
        held || g.tangents(x).is_some_and(|ts| ts.iter().all(|t| t.normalized().is_none_or(|t| r.dot(t).abs() <= SQUARE)))
    };
    let settled = |x: &[f64], p: Vec3, q: Vec3| -> bool {
        let d = p.dist(q);
        d <= eps || {
            let r = (p - q) * (1.0 / d);
            let mut x = x.to_vec();
            let (xa, xb) = x.split_at_mut(na);
            square(ga, xa, r) && square(gb, xb, r)
        }
    };
    let (mut p, mut q) = eval(&x)?;
    let mut r2 = (p - q).dot(p - q);
    let mut mu = 0.0;
    for _ in 0..200 {
        // Settled: no further step, which where the gap is the same along a direction (coaxial
        // cylinders) could only wander along it.
        if r2.sqrt() <= eps * 0.1 || settled(&x, p, q) {
            break;
        }
        let r = p - q;
        let mut cols = ga.tangents(x.get(..na)?)?;
        cols.extend(gb.tangents(x.get(na..)?)?.into_iter().map(|t| -t));
        // Normal equations (J^T J + mu) dx = -J^T r.
        let mut m = [[0.0; 5]; 4];
        for (i, ci) in cols.iter().enumerate() {
            for (j, cj) in cols.iter().enumerate() {
                *m.get_mut(i)?.get_mut(j)? = ci.dot(*cj);
            }
            *m.get_mut(i)?.get_mut(4)? = -ci.dot(r);
        }
        let trace: f64 = (0..n).filter_map(|i| m.get(i).and_then(|row| row.get(i))).sum();
        if !(trace > 0.0 && trace.is_finite()) {
            return None;
        }
        // Damped at least a little: where the gap is the same along a direction (coaxial
        // cylinders, a plane over a plane) the system says nothing about it, and undamped the
        // round-off in it sends the points off along it.
        let floor = trace * 1e-8;
        if mu == 0.0 {
            mu = trace * 1e-6;
        }
        let mut stepped = false;
        for _ in 0..30 {
            let mut a = m;
            for (i, row) in a.iter_mut().enumerate().take(n) {
                if let Some(d) = row.get_mut(i) {
                    *d += mu;
                }
            }
            let Some(dx) = solve(a, n) else { break };
            let mut y: Vec<f64> = x.iter().zip(dx.iter()).map(|(v, d)| v + d).collect();
            ga.clamp(y.get_mut(..na)?);
            gb.clamp(y.get_mut(na..)?);
            let Some((p2, q2)) = eval(&y) else { break };
            let r2n = (p2 - q2).dot(p2 - q2);
            if r2n < r2 * (1.0 - 1e-12) {
                (x, p, q, r2) = (y, p2, q2, r2n);
                mu = (mu / 3.0).max(floor);
                stepped = true;
                break;
            }
            mu *= 4.0;
        }
        if !stepped {
            break;
        }
    }
    let d = r2.sqrt();
    d.is_finite().then(|| (d, p, q, settled(&x, p, q)))
}

/// Solves the `n` by `n` system (at most 4) given as rows `[a0, a1, a2, a3, rhs]` (Gauss with
/// partial pivoting).
fn solve(mut m: [[f64; 5]; 4], n: usize) -> Option<Vec<f64>> {
    if n > 4 {
        return None;
    }
    for c in 0..n {
        let pivot = (c..n).max_by(|&i, &j| m[i][c].abs().total_cmp(&m[j][c].abs()))?;
        m.swap(c, pivot);
        let d = m[c][c];
        if !(d.abs() > 0.0 && d.is_finite()) {
            return None;
        }
        let row = m[c];
        for below in m.iter_mut().take(n).skip(c + 1) {
            let f = below[c] / d;
            for (v, r) in below.iter_mut().zip(row).skip(c) {
                *v -= f * r;
            }
        }
    }
    let mut x = vec![0.0; n];
    for c in (0..n).rev() {
        let s: f64 = ((c + 1)..n).map(|j| m[c][j] * x[j]).sum();
        x[c] = (m[c][4] - s) / m[c][c];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Face `face` of `s` and the edges of it that pass within `reach` of `p` (as meshed), the
/// nearest few.
fn near_features(s: &DistanceShape, face: u32, p: Vec3, reach: f64) -> Vec<Feature> {
    let mut edges: Vec<(f64, usize)> = s
        .mesh
        .face_edges(face)
        .into_iter()
        .filter(|e| *e < s.edges.len())
        .filter_map(|e| {
            let line = s.mesh.edges.get(e)?;
            let d = match line.as_slice() {
                [one] => one.dist(p),
                _ => line.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min),
            };
            (d <= reach).then_some((d, e))
        })
        .collect();
    edges.sort_by(|x, y| x.0.total_cmp(&y.0));
    std::iter::once(Feature::Face(face as usize)).chain(edges.into_iter().take(6).map(|(_, e)| Feature::Edge(e))).collect()
}

/// How the bodies of `a` and `b` meet ([`Contact`]), from their exact faces near where their
/// meshes come within both chords of each other (elsewhere the faces are farther apart than the
/// meshes' least distance, which bounds them). At each such place, for each pair of faces, the
/// nearest points of the faces and of their edges there ([`nearest_points`]):
/// - crossing when two faces close the gap (within `eps`, mm) at a point inside both faces
///   (clear of their edges) where their surfaces cross at more than [`CROSSING_ANGLE`];
/// - apart when at every place the least of those gaps, on the faces and edges themselves, is
///   more than `eps`: whether one body lies inside the other the caller tells from points inside
///   them;
/// - touching otherwise (a place where nothing settled on the faces counts as one that could
///   touch).
pub(crate) fn contact(a: &DistanceShape, b: &DistanceShape, eps: f64) -> Result<Contact> {
    use mt::{ParametricSurface3D, SearchNearestParameter};
    let slack = 2.0 * (a.chord + b.chord);
    let Some(m) = a.mesh_closest(b, slack + eps) else {
        // The meshes, each within its chord of the faces, are farther apart than both chords.
        return Ok(Contact::Apart(slack / 2.0));
    };
    if a.faces.is_empty() || b.faces.is_empty() {
        return Ok(Contact::Touching);
    }
    // The nearest triangle pair of each pair of faces in each cell of a grid, the cells several
    // chords across: one place per stretch of contact.
    let cell = (10.0 * slack).max(1e-6 * a.body.size().max(b.body.size()).max(1.0));
    let key = |p: Vec3| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64, (p.z / cell).floor() as i64);
    let mut seeds: HashMap<(u32, u32, (i64, i64, i64)), MeshClosest> = HashMap::new();
    a.set.within(&b.set, m.distance + slack, |c| {
        let (Some(fa), Some(fb)) = (a.set.face(c.tri_a), b.set.face(c.tri_b)) else { return };
        let e = seeds.entry((fa, fb, key((c.a + c.b) * 0.5))).or_insert(c);
        if c.distance < e.distance {
            *e = c;
        }
    });
    let mut seeds: Vec<((u32, u32, (i64, i64, i64)), MeshClosest)> = seeds.into_iter().collect();
    seeds.sort_by(|x, y| x.1.distance.total_cmp(&y.1.distance).then(x.0.cmp(&y.0)));
    let complete = seeds.len() <= MAX_CONTACT_SEEDS;
    seeds.truncate(MAX_CONTACT_SEEDS);
    let slip = 1e-6 * a.body.size().max(b.body.size()).max(1.0);
    guard("body contact", || {
        // Inside face `f` of `s` (a point [`DistanceShape::on_face`] puts on it): clear of each
        // of its edges, measured on the edges themselves, by more than `slip` many times over.
        let interior = |s: &DistanceShape, f: u32, p: Vec3| {
            s.mesh.face_edges(f).into_iter().all(|e| s.edge_nearest(e, p, &mut Hint::default()).is_some_and(|(at, _)| at.dist(p) > 10.0 * slip))
        };
        // A feature near `p` as geometry, with its parameters at `p`.
        let geo = |s: &DistanceShape, f: Feature, p: Vec3| -> Option<(Geo, Vec<f64>)> {
            use mt::BoundedCurve;
            match f {
                Feature::Face(i) => {
                    let surface = s.faces.get(i)?.oriented_surface();
                    let (u, v) = surface.search_nearest_parameter(p3(p), None, 100)?;
                    Some((Geo::Surface(surface), vec![u, v]))
                }
                Feature::Edge(i) => {
                    let mut hint = Hint::default();
                    s.edge_nearest(i, p, &mut hint)?;
                    let curve = s.edges.get(i)?.oriented_curve();
                    let (t0, t1) = curve.range_tuple();
                    Some((Geo::Curve(curve, t0.min(t1), t0.max(t1)), vec![hint.t?]))
                }
            }
        };
        // Within `slip` of an edge of face `f`: a least gap on a face's boundary settles a little
        // way past it (a minimum is flat), and is that face's all the same.
        let at_boundary = |s: &DistanceShape, f: u32, x: Vec3| {
            s.mesh.face_edges(f).into_iter().any(|e| s.edge_nearest(e, x, &mut Hint::default()).is_some_and(|(at, _)| at.dist(x) <= slip))
        };
        let mut least = f64::INFINITY;
        let mut unsettled = !complete;
        for ((fa, fb, _), c) in &seeds {
            let mut here = f64::INFINITY;
            let (near_a, near_b) = (near_features(a, *fa, c.a, 10.0 * slack), near_features(b, *fb, c.b, 10.0 * slack));
            for &xa in &near_a {
                for &xb in &near_b {
                    let (Some((ga, pa)), Some((gb, pb))) = (geo(a, xa, c.a), geo(b, xb, c.b)) else { continue };
                    let Some((d, p, q, settled)) = nearest_points(&ga, &gb, pa.into_iter().chain(pb).collect(), eps) else { continue };
                    let on = |s: &DistanceShape, f: Feature, x: Vec3| match f {
                        Feature::Face(i) => s.on_face(i as u32, x) || at_boundary(s, i as u32, x),
                        Feature::Edge(_) => true,
                    };
                    if !(settled && on(a, xa, p) && on(b, xb, q)) {
                        continue;
                    }
                    if let (Feature::Face(i), Feature::Face(j), Geo::Surface(sa), Geo::Surface(sb)) = (xa, xb, &ga, &gb)
                        && d <= eps
                        && interior(a, i as u32, p)
                        && interior(b, j as u32, q)
                    {
                        let normal = |s: &mt::Surface, x: Vec3| -> Option<Vec3> {
                            let (u, v) = s.search_nearest_parameter(p3(x), None, 100)?;
                            let n = s.normal(u, v);
                            Vec3::new(n.x, n.y, n.z).normalized()
                        };
                        if let (Some(na), Some(nb)) = (normal(sa, p), normal(sb, q))
                            && na.dot(nb).abs() < CROSSING_ANGLE.cos()
                        {
                            return Ok(Contact::Crossing);
                        }
                    }
                    here = here.min(d);
                }
            }
            if here.is_finite() {
                least = least.min(here);
            } else {
                unsettled = true;
            }
        }
        Ok(if !unsettled && least.is_finite() && least > eps + slip { Contact::Apart(least) } else { Contact::Touching })
    })
}
