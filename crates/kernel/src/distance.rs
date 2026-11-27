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
use solidcore_record::brep::Curve;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::mesh_distance::{MeshClosest, TriangleSet};
use solidcraft_geom::{Aabb3, Mesh};

use crate::body::{Body, PointClass};
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
/// and edges initialized lazily as individual exact queries need them. Share one between the
/// queries that involve its body.
pub struct DistanceShape<'a> {
    body: &'a Body,
    mesh: &'a Mesh,
    /// Optional column-major rigid transform mapping `mesh` into `body`'s world frame (`None`:
    /// identity), so placed occurrences share one definition mesh without cloning it.
    mesh_transform: Option<[[f64; 4]; 4]>,
    set: TriangleSet,
    chord: f64,
    faces: OnceLock<Vec<OnceLock<Option<SurfaceEval>>>>,
    edges: OnceLock<Vec<OnceLock<Option<(CurveEval, [f64; 2])>>>>,
    surfaces: OnceLock<Vec<OnceLock<Option<FaceSurface>>>>,
}

/// Most face pairs near the nearest mesh points whose exact distance is worked out.
const MAX_FACE_PAIRS: usize = 128;
/// Most projection rounds for one pair of features.
const MAX_ROUNDS: usize = 400;

/// Column-major 4x4 affine point transform.
fn apply_point(m: &[[f64; 4]; 4], p: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
        m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
        m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
    )
}

impl<'a> DistanceShape<'a> {
    /// `mesh` is `body`'s mesh, within `chord` (mm) of its faces.
    pub fn new(body: &'a Body, mesh: &'a Mesh, chord: f64) -> DistanceShape<'a> {
        Self::with_triangle_set(body, mesh, None, TriangleSet::new(mesh), chord)
    }

    /// Creates a [`DistanceShape`] from a prebuilt [`TriangleSet`] (for example one built fallibly
    /// in world coordinates from a shared definition mesh and rigid placement `mesh_transform`).
    /// Does not touch `body.record()` until an exact face or edge query runs.
    pub fn with_triangle_set(
        body: &'a Body,
        mesh: &'a Mesh,
        mesh_transform: Option<[[f64; 4]; 4]>,
        set: TriangleSet,
        chord: f64,
    ) -> DistanceShape<'a> {
        let chord = if chord.is_finite() && chord > 0.0 { chord } else { 0.0 };
        DistanceShape { body, mesh, mesh_transform, set, chord, faces: OnceLock::new(), edges: OnceLock::new(), surfaces: OnceLock::new() }
    }

    /// The box around the body: its mesh's, grown by the chord tolerance.
    pub fn bounds(&self) -> Aabb3 {
        self.set.bounds().inflated(self.chord)
    }

    /// The uninflated box around the mesh's finite triangles in world space.
    pub fn mesh_bounds(&self) -> Aabb3 {
        self.set.bounds()
    }

    /// Rough heap footprint of this shape's triangle hierarchy and lazy evaluator tables in bytes.
    pub fn estimated_bytes(&self) -> usize {
        let face_slots = self.faces.get().map_or(0, |v| v.capacity().saturating_mul(std::mem::size_of::<OnceLock<Option<SurfaceEval>>>()));
        let edge_slots = self.edges.get().map_or(0, |v| v.capacity().saturating_mul(std::mem::size_of::<OnceLock<Option<(CurveEval, [f64; 2])>>>()));
        let surf_slots = self.surfaces.get().map_or(0, |v| v.capacity().saturating_mul(std::mem::size_of::<OnceLock<Option<FaceSurface>>>()));
        self.set.estimated_bytes().saturating_add(face_slots).saturating_add(edge_slots).saturating_add(surf_slots)
    }

    /// How far the mesh may lie from the faces (mm).
    pub fn chord(&self) -> f64 {
        self.chord
    }

    /// The nearest points of the two meshes, if they come within `limit`.
    pub fn mesh_closest(&self, other: &DistanceShape, limit: f64) -> Option<MeshClosest> {
        self.set.closest(&other.set, limit)
    }

    fn face_eval(&self, face: usize) -> Option<&SurfaceEval> {
        let slots = self.faces.get_or_init(|| (0..self.body.face_count()).map(|_| OnceLock::new()).collect());
        slots.get(face)?.get_or_init(|| self.body.record().faces.get(face).and_then(|f| SurfaceEval::new(f.parameters()).ok())).as_ref()
    }

    fn edge_eval(&self, edge: usize) -> Option<&(CurveEval, [f64; 2])> {
        let slots = self.edges.get_or_init(|| (0..self.body.edge_count()).map(|_| OnceLock::new()).collect());
        slots
            .get(edge)?
            .get_or_init(|| self.body.record().edges.get(edge).and_then(|e| CurveEval::new(e.parameters()).ok().map(|c| (c, e.interval))))
            .as_ref()
    }

    fn surface(&self, face: usize) -> Option<FaceSurface> {
        let slots = self.surfaces.get_or_init(|| (0..self.body.face_count()).map(|_| OnceLock::new()).collect());
        *slots.get(face)?.get_or_init(|| surface_of(self.body.record(), face))
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
        let tiny = (1e-9 * self.body.size().max(1.0)).max(self.body.record().tolerance);
        Ok(distances.first().copied().flatten().is_some_and(|d| d <= tiny))
    }

    /// The features near mesh point `p` on face `face`: the face, and its edges within twice the chord.
    fn features(&self, face: u32, p: Vec3) -> Vec<Feature> {
        let mut out = vec![Feature::Face(face as usize)];
        let reach = 2.0 * self.chord + 1e-9 * self.body.size().max(1.0);
        let edge_count = self.body.edge_count();
        let pt = |v: Vec3| match self.mesh_transform {
            Some(m) => apply_point(&m, v),
            None => v,
        };
        for e in self.mesh.face_edges(face) {
            let Some(line) = self.mesh.edges.get(e) else { continue };
            let near = match line.as_slice() {
                [one] => pt(*one).dist(p),
                _ => line.windows(2).map(|w| p.dist_to_segment(pt(w[0]), pt(w[1]))).fold(f64::INFINITY, f64::min),
            };
            if near <= reach && e < edge_count {
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
                let s = self.face_eval(i)?;
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
        let (c, [t0, t1]) = self.edge_eval(i)?;
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
    let scale = a.body.size().max(b.body.size()).max(pb.len()).max(1.0);
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
    let contact_tol = 1e-12 * a.body.size().max(b.body.size()).max(1.0);
    let best = guard("body distance", || {
        let mut best: Option<BodyDistance> = None;
        let mut tried = std::collections::HashSet::new();
        'search: for ((fa, fb), c) in &seeds {
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
                        if d <= contact_tol {
                            break 'search;
                        }
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

/// Exact point witness proving two closed solids share interior volume, for catching a boolean
/// intersection that falsely returned empty (for example, a rib piercing a curved wall without
/// placing any vertex strictly inside the other solid).
///
/// Every returned point is verified by exact kernel point classification: either an exact vertex
/// or edge point of one solid that classifies strictly [`PointClass::Inside`] the other, or a
/// candidate point that classifies strictly [`PointClass::Inside`] both solids. Touching-only
/// solids never produce a witness.
pub fn overlap_witness(a: &DistanceShape, b: &DistanceShape) -> Result<Option<Vec3>> {
    if a.body.is_mesh() || b.body.is_mesh() || a.body.is_surface() || b.body.is_surface() {
        return Ok(None);
    }
    let box_a = a.body.exact_bounds()?;
    let box_b = b.body.exact_bounds()?;
    let shared = box_a.intersection(&box_b);
    if shared.is_empty() {
        return Ok(None);
    }

    // Pass 1: exact boundary points (vertices and edge samples) of one solid strictly inside the
    // other. Any boundary point of a closed solid strictly away from the other solid's boundary
    // guarantees positive shared 3D volume (never triggered by coplanar/touching faces).
    const MAX_BOUNDARY_SAMPLES: usize = 512;
    const MAX_VERIFY_HITS: usize = 32;
    for (source, other) in [(a, b), (b, a)] {
        let samples = boundary_samples_in(source, &shared, MAX_BOUNDARY_SAMPLES);
        if samples.is_empty() {
            continue;
        }
        let classes = other.body.classify_points(&samples)?;
        for (&point, _) in samples.iter().zip(classes).filter(|(_, c)| *c == PointClass::Inside).take(MAX_VERIFY_HITS) {
            if away_from_boundary(point, other.body)? {
                return Ok(Some(point));
            }
        }
    }

    // Pass 2: interior candidates near crossing/touching mesh triangles and in the shared box,
    // requiring strict `Inside` and positive exact boundary distance on BOTH bodies (catches
    // face-interior-only penetrations with zero false positives on touching faces or chord error).
    const MAX_INTERIOR_SAMPLES: usize = 512;
    let candidates = interior_candidates(a, b, &shared, MAX_INTERIOR_SAMPLES);
    if candidates.is_empty() {
        return Ok(None);
    }
    let classes_a = a.body.classify_points(&candidates)?;
    let inside_a: Vec<Vec3> = candidates.into_iter().zip(classes_a).filter_map(|(p, c)| (c == PointClass::Inside).then_some(p)).collect();
    if inside_a.is_empty() {
        return Ok(None);
    }
    let classes_b = b.body.classify_points(&inside_a)?;
    for (point, _) in inside_a.into_iter().zip(classes_b).filter(|(_, c)| *c == PointClass::Inside).take(MAX_VERIFY_HITS) {
        if away_from_boundary(point, a.body)? && away_from_boundary(point, b.body)? {
            return Ok(Some(point));
        }
    }
    Ok(None)
}

/// Whether `p` lies strictly away from `body`'s exact boundary (beyond tolerance), preventing
/// touching-face points whose raw point classification fluctuates near a boundary from being
/// misread as interior overlap witnesses.
fn away_from_boundary(p: Vec3, body: &Body) -> Result<bool> {
    let faces: Vec<usize> = (0..body.face_count()).collect();
    if faces.is_empty() {
        return Ok(false);
    }
    let tol = (body.record().tolerance * 4.0).max(body.size() * 1e-7).max(1e-6);
    Ok(point_closest(p, body, Members::Faces(&faces))?.is_some_and(|d| d.distance > tol))
}

/// Exact vertices and exact edge curve samples of `shape` that lie inside `region`.
fn boundary_samples_in(shape: &DistanceShape, region: &Aabb3, cap: usize) -> Vec<Vec3> {
    let record = shape.body.record();
    let mut out = Vec::new();
    for v in &record.vertices {
        if out.len() >= cap {
            return out;
        }
        let p = Vec3::from(v.point);
        if p.is_finite() && region.contains(p) {
            out.push(p);
        }
    }
    for (i, edge) in record.edges.iter().enumerate() {
        if out.len() >= cap {
            break;
        }
        let Some((curve, [t0, t1])) = shape.edge_eval(i) else {
            continue;
        };
        if matches!(edge.curve, Curve::Line { .. }) {
            let (Some(p0), Some(p1)) = (curve.point(*t0), curve.point(*t1)) else {
                continue;
            };
            if p0.is_finite()
                && p1.is_finite()
                && let Some((u0, u1)) = clip_segment_to_box(p0, p1, region)
            {
                let d = p1 - p0;
                for frac in [0.25, 0.5, 0.75] {
                    if out.len() >= cap {
                        break;
                    }
                    let u = u0 + (u1 - u0) * frac;
                    let p = p0 + d * u;
                    if p.is_finite() && region.contains(p) {
                        out.push(p);
                    }
                }
            }
            continue;
        }
        const CURVE_STEPS: u8 = 8;
        for step in 1..CURVE_STEPS {
            if out.len() >= cap {
                break;
            }
            let t = *t0 + (*t1 - *t0) * (f64::from(step) / f64::from(CURVE_STEPS));
            if let Some(p) = curve.point(t)
                && p.is_finite()
                && region.contains(p)
            {
                out.push(p);
            }
        }
    }
    out
}

/// Parameter sub-interval `[t0, t1]` in `[0, 1]` where segment `p0..p1` lies inside `bx`.
fn clip_segment_to_box(p0: Vec3, p1: Vec3, bx: &Aabb3) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    let axes = [(p0.x, p1.x - p0.x, bx.min.x, bx.max.x), (p0.y, p1.y - p0.y, bx.min.y, bx.max.y), (p0.z, p1.z - p0.z, bx.min.z, bx.max.z)];
    for (o, d, lo, hi) in axes {
        if d.abs() <= 1e-15 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d;
        let (a, b) = ((lo - o) * inv, (hi - o) * inv);
        let (enter, exit) = if a <= b { (a, b) } else { (b, a) };
        t0 = t0.max(enter);
        t1 = t1.min(exit);
        if t0 > t1 {
            return None;
        }
    }
    Some((t0, t1))
}

/// Candidate points in `region` near close/crossing mesh triangles and at the shared box centres.
fn interior_candidates(a: &DistanceShape, b: &DistanceShape, region: &Aabb3, cap: usize) -> Vec<Vec3> {
    let mut out = Vec::new();
    let push = |out: &mut Vec<Vec3>, p: Vec3| {
        if out.len() < cap && p.is_finite() && region.contains(p) {
            out.push(p);
        }
    };
    let center = region.center();
    push(&mut out, center);
    let quarter = region.size() * 0.25;
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            for sz in [-1.0, 1.0] {
                push(&mut out, Vec3::new(center.x + sx * quarter.x, center.y + sy * quarter.y, center.z + sz * quarter.z));
            }
        }
    }
    let slack = 2.0 * (a.chord + b.chord);
    let mut pairs = 0usize;
    const MAX_TRI_PAIRS: usize = 64;
    a.set.within(&b.set, slack, |c| {
        if pairs >= MAX_TRI_PAIRS || out.len() >= cap {
            return;
        }
        pairs += 1;
        let mid = (c.a + c.b) * 0.5;
        push(&mut out, mid);
        if let Some([p0, p1, p2]) = a.set.triangle(c.tri_a) {
            push(&mut out, (p0 + p1 + p2) * (1.0 / 3.0));
            push(&mut out, (mid + p0) * 0.5);
            push(&mut out, (mid + p1) * 0.5);
            push(&mut out, (mid + p2) * 0.5);
        }
        if let Some([q0, q1, q2]) = b.set.triangle(c.tri_b) {
            push(&mut out, (q0 + q1 + q2) * (1.0 / 3.0));
            push(&mut out, (mid + q0) * 0.5);
            push(&mut out, (mid + q1) * 0.5);
            push(&mut out, (mid + q2) * 0.5);
        }
    });
    out
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

/// The closest points of a bounded line segment and the selected faces or edges of a body.
/// `from` lies on the segment, including its ends. A mesh body gives `None`.
///
/// # Errors
///
/// [`KernelError::Failed`] when the kernel cannot take the body or measure the segment.
pub fn segment_closest(start: Vec3, end: Vec3, body: &Body, to: Members) -> Result<Option<BodyDistance>> {
    let Some(form) = exact_body(body)? else { return Ok(None) };
    let (ops, order) = &*form;
    let (faces, edges) = members(to);
    let c = ops.closest_to_segment(order, faces, edges, start.into(), end.into()).map_err(|e| failed(&e))?;
    let c = c.ok_or_else(|| KernelError::Failed("measure: the kernel found no closest points".into()))?;
    Ok(Some(BodyDistance { distance: c.distance, from: Vec3::from(c.from), to: Vec3::from(c.to), exact: true }))
}

/// Certified non-boolean test for whether two solid B-rep bodies share no interior volume
/// (pure contact or disjoint, never penetrating).
///
/// Returns `Some(true)` when certified disjoint or flush contact with no interior overlap,
/// `Some(false)` when real interior overlap is proved, and `None` for mesh bodies, conversion
/// errors, or undecided geometry.
pub fn interiors_disjoint(a: &Body, b: &Body) -> Option<bool> {
    let (Some(form_a), Some(form_b)) = (exact_body(a).ok()?, exact_body(b).ok()?) else {
        return None;
    };
    let ((ops_a, _), (ops_b, _)) = (&*form_a, &*form_b);
    ops_a.interiors_disjoint(ops_b).ok().flatten()
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
