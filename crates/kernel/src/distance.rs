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

use solidcraft_geom::prelude::*;
use std::collections::HashMap;
use std::sync::OnceLock;

use solidcore_record::Vec3;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::mesh_distance::{MeshClosest, TriangleSet};
use solidcraft_geom::{Aabb3, Mesh};

use crate::body::Body;
use crate::surface_kind::{FaceSurface, surface_of};
use crate::{KernelError, Result, guard};

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
}

/// Most face pairs near the nearest mesh points whose exact distance is worked out.
const MAX_FACE_PAIRS: usize = 24;
/// Most projection rounds for one pair of features.
const MAX_ROUNDS: usize = 400;

impl<'a> DistanceShape<'a> {
    /// `mesh` is `body`'s mesh, within `chord` (mm) of its faces.
    pub fn new(body: &'a Body, mesh: &'a Mesh, chord: f64) -> DistanceShape<'a> {
        let record = body.record();
        let faces: Vec<Option<SurfaceEval>> = record.faces.iter().map(|f| SurfaceEval::new(f.parameters()).ok()).collect();
        let edges = record.edges.iter().map(|e| CurveEval::new(e.parameters()).ok().map(|c| (c, e.interval))).collect();
        let surfaces = faces.iter().map(|_| OnceLock::new()).collect();
        let chord = if chord.is_finite() && chord > 0.0 { chord } else { 0.0 };
        DistanceShape { body, mesh, set: TriangleSet::new(mesh), chord, faces, edges, surfaces }
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

    /// A projected point belongs to a face only when its distance to the trimmed face is
    /// within rounding tolerance. The kernel query respects the outer loop and every hole;
    /// nearby display triangles cannot establish membership at a concave corner.
    fn on_face(&self, face: u32, p: Vec3) -> Result<bool> {
        let form = self.body.ops_body().ok_or_else(|| crate::KernelError::Failed("body distance: exact face query unavailable".into()))?;
        let (body, order) = &*form;
        let distances = body
            .face_distances(order, face as usize, &[[p.x, p.y, p.z]])
            .map_err(|e| crate::KernelError::Failed(format!("body distance: trimmed face: {e}")))?;
        let tiny = 1e-9 * self.body.size().max(1.0);
        Ok(distances.first().copied().flatten().is_some_and(|d| d <= tiny))
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
                        Feature::Edge(_) => Ok(true),
                    };
                    if holds(a, xa, p)? && holds(b, xb, q)? && best.is_none_or(|bd| d < bd.distance) {
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

/// Faces or edges of a body, by their index in the body's face or edge order: what one item of a
/// measurement is on the body's exact B-rep ([`edges_closest`], [`point_closest`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Members<'a> {
    Faces(&'a [usize]),
    Edges(&'a [usize]),
}

/// The closest points of edges `edges` of `a` (over each edge's own interval) and the faces (as
/// they are trimmed) or edges `to` of `b`, on the exact curves and surfaces: the kernel's
/// minimum-distance solve of each edge's curve against `to`, never a tessellation. Between two
/// arcs that is the curve/curve minimum over both arcs, so a witness point lies on each arc, at
/// one of its ends where the curves come nearest past it. `None` when either body is a mesh body,
/// whose triangles are its geometry.
///
/// # Errors
///
/// [`KernelError::Failed`] for a B-rep body the kernel can't take, and when the kernel finds no
/// closest points or refuses the solve.
pub fn edges_closest(a: &Body, edges: &[usize], b: &Body, to: Members) -> Result<Option<BodyDistance>> {
    let (Some(form_a), Some(form_b)) = (exact_body(a)?, exact_body(b)?) else { return Ok(None) };
    let ((ops_a, order_a), (ops_b, order_b)) = (&*form_a, &*form_b);
    let (faces, to_edges) = members(to);
    let mut best: Option<BodyDistance> = None;
    for &edge in edges {
        let curve = ops_a.edge_curve(order_a, edge).map_err(|e| failed(&e))?;
        let c = ops_b.closest_to_curve(order_b, faces, to_edges, &curve).map_err(|e| failed(&e))?;
        let c = c.ok_or_else(|| KernelError::Failed("measure: the kernel found no closest points".into()))?;
        let found = BodyDistance { distance: c.distance, from: Vec3::from(c.from), to: Vec3::from(c.to), exact: true };
        if best.is_none_or(|bd| found.distance < bd.distance) {
            best = Some(found);
        }
    }
    Ok(best)
}

/// The closest points between trimmed faces of two B-rep bodies, including their exact
/// boundary curves and vertices. `None` for a mesh body, whose triangles are its geometry.
///
/// # Errors
/// [`KernelError::Failed`] when the kernel cannot take either body or answer the solve.
pub fn faces_closest(a: &Body, faces: &[usize], b: &Body, to: &[usize]) -> Result<Option<BodyDistance>> {
    if a.is_mesh() || b.is_mesh() {
        return Ok(None);
    }
    let (Some(form_a), Some(form_b)) = (exact_body(a)?, exact_body(b)?) else { return Ok(None) };
    let ((ops_a, order_a), (ops_b, order_b)) = (&*form_a, &*form_b);
    let c = ops_a.closest_faces(order_a, faces, ops_b, order_b, to).map_err(|e| failed(&e))?;
    Ok(Some(BodyDistance { distance: c.distance, from: Vec3::from(c.from), to: Vec3::from(c.to), exact: true }))
}

/// The closest points of `p` and the faces or edges `to` of `b`, as [`edges_closest`] measures
/// them; `from` is `p`. `None` for a mesh body.
///
/// # Errors
///
/// [`KernelError::Failed`] for a B-rep body the kernel can't take, and when the kernel finds no
/// closest point or refuses the solve.
pub fn point_closest(p: Vec3, b: &Body, to: Members) -> Result<Option<BodyDistance>> {
    let Some(form_b) = exact_body(b)? else { return Ok(None) };
    let (ops_b, order_b) = &*form_b;
    let (faces, edges) = members(to);
    let c = ops_b.closest_to_point(order_b, faces, edges, [p.x, p.y, p.z]).map_err(|e| failed(&e))?;
    let c = c.ok_or_else(|| KernelError::Failed("measure: the kernel found no closest point".into()))?;
    Ok(Some(BodyDistance { distance: c.distance, from: Vec3::from(c.from), to: Vec3::from(c.to), exact: true }))
}

/// `body`'s exact B-rep for a measurement: `None` for a mesh body, whose triangles are its
/// geometry, and an error for a B-rep body the kernel can't take, which is never measured on its
/// display mesh instead.
fn exact_body(body: &Body) -> Result<Option<crate::body::OpsForm>> {
    if body.is_mesh() {
        return Ok(None);
    }
    body.ops_body()
        .map(Some)
        .ok_or_else(|| KernelError::Failed("measure: exact measurement is unavailable: the kernel could not take the body".into()))
}

/// The face and edge lists of `to`.
fn members<'a>(to: Members<'a>) -> (&'a [usize], &'a [usize]) {
    match to {
        Members::Faces(faces) => (faces, &[]),
        Members::Edges(edges) => (&[], edges),
    }
}

fn failed(e: &solidcore_ops::Error) -> KernelError {
    KernelError::Failed(format!("measure: {e}"))
}
