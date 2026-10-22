//! Topology counts comparable with kernels that use periodic faces and closed edges.
//!
//! A body may hold a surface in several faces (a full cylinder as two half faces, a circle as two
//! arcs). Each face's surface is the record's own ([`crate::offset::surf_of`]: plane, cylinder,
//! sphere, cone, torus; free-form faces by their surface record); adjacent faces on one surface
//! are merged, edges between merged faces disappear, and chains of edges through vertices that
//! only join two pieces of one edge are counted once.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;
use solidcraft_geom::brep::{Brep, Loop};
use solidcraft_geom::{Mesh, UnitVec3, Vec3};

use crate::Result;
use crate::body::Body;
use crate::heal::{edge_points, find, on_split_line};
use crate::history::edge_faces;
use crate::offset::{Surf, surf_of};

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct TopoCounts {
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub face_types: BTreeMap<String, usize>,
}

/// The kind of a face's surface, as counted.
fn name(s: Option<&Surf>) -> &'static str {
    match s {
        Some(Surf::Plane { .. }) => "plane",
        Some(Surf::Cylinder { .. }) => "cylinder",
        Some(Surf::Sphere { .. }) => "sphere",
        Some(Surf::Cone { .. }) => "cone",
        Some(Surf::Torus { .. }) | None => "other",
    }
}

/// The faces of a record with their surfaces, for telling which lie on one surface.
struct Faces<'a> {
    record: &'a Brep,
    surfs: Vec<Option<Surf>>,
    tol: f64,
}

impl<'a> Faces<'a> {
    fn new(record: &'a Brep, tol: f64) -> Faces<'a> {
        Faces { record, surfs: (0..record.faces.len()).map(|f| surf_of(record, f)).collect(), tol }
    }

    fn surf(&self, f: usize) -> Option<&Surf> {
        self.surfs.get(f).and_then(Option::as_ref)
    }

    fn planar(&self, f: usize) -> bool {
        matches!(self.surf(f), Some(Surf::Plane { .. }))
    }

    /// Do faces `a` and `b` lie on one surface: the same analytic surface, or the very same
    /// free-form surface record (pieces cut from one surface)?
    fn same(&self, a: usize, b: usize) -> bool {
        match (self.surf(a), self.surf(b)) {
            (Some(x), Some(y)) => x.same(y, self.tol),
            (None, None) => match (self.record.faces.get(a), self.record.faces.get(b)) {
                (Some(x), Some(y)) => x.surface == y.surface,
                _ => false,
            },
            _ => false,
        }
    }

    /// The record vertices on face `f`'s loops.
    fn vertices(&self, f: usize) -> Vec<Vec3> {
        let Some(face) = self.record.faces.get(f) else { return Vec::new() };
        face.loops
            .iter()
            .flat_map(|l| match l {
                Loop::Edges { edges } => edges.iter().filter_map(|u| self.record.edges.get(u.edge as usize)).flat_map(|e| e.vertices).collect(),
                Loop::Vertex { vertex } => vec![*vertex],
            })
            .filter_map(|v| self.record.vertices.get(v as usize).map(|v| Vec3::from(v.point)))
            .collect()
    }
}

/// Per edge (in `Body::edges` order): is it a seam between two pieces of one curved surface?
/// `mesh` is the body's tessellation, whose edges are the record's.
pub fn seam_flags(b: &Body, _mesh: &Mesh) -> Vec<bool> {
    let faces = Faces::new(b.record(), (b.size() * 1e-4).max(1e-6) * 10.0);
    edge_faces(b.record()).iter().map(|fs| matches!(fs[..], [x, y] if !faces.planar(x) && faces.same(x, y))).collect()
}

/// Merged face/edge/vertex counts (see module docs). `mesh` must be `b.mesh(..)`: faces of no
/// area in it don't count.
pub fn merged_topology(b: &Body, mesh: &Mesh) -> Result<TopoCounts> {
    let record = b.record();
    let nf = record.faces.len();
    let tol = (b.size() * 1e-4).max(1e-6);
    let faces = Faces::new(record, tol * 10.0);
    // Zero-area faces (a revolved profile edge lying on the axis) don't count.
    let mut area = vec![0.0f64; nf];
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if let (Some([p, q, r]), Some(a)) = (mesh.tri(t), area.get_mut(*f as usize)) {
            *a += (q - p).cross(r - p).len() * 0.5;
        }
    }
    let tiny = (b.size() * 1e-6).powi(2);
    let is_deg = |f: usize| area.get(f).is_some_and(|a| *a < tiny);

    // Edge → faces, edge → vertices. Vertices and edges lying on each other count once (two
    // tangent holes touch along a line that both bodies' pieces carry): welded by position,
    // edges by their ends and their middle point.
    let q = (b.size() * 1e-6).max(1e-9);
    let cell = |p: Vec3| ((p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64);
    let mut weld_v: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut vix = |v: u32| -> Option<usize> {
        let p = Vec3::from(record.vertices.get(v as usize)?.point);
        let n = weld_v.len();
        Some(*weld_v.entry(cell(p)).or_insert(n))
    };
    let mut weld_e: HashMap<(usize, usize, (i64, i64, i64)), usize> = HashMap::new();
    let mut welded_faces: Vec<Vec<usize>> = Vec::new();
    let mut welded_ends: Vec<(usize, usize)> = Vec::new();
    // A record edge standing for each welded edge (for its points).
    let mut rep: Vec<u32> = Vec::new();
    for (ei, (e, between)) in record.edges.iter().zip(edge_faces(record)).enumerate() {
        let (Some(a), Some(z)) = (vix(e.vertices[0]), vix(e.vertices[1])) else { continue };
        let pts = edge_points(record, ei as u32);
        let mid = pts.get(2).copied().unwrap_or(Vec3::ZERO);
        let n = weld_e.len();
        let we = *weld_e.entry((a.min(z), a.max(z), cell(mid))).or_insert(n);
        if we == welded_faces.len() {
            welded_faces.push(Vec::new());
            welded_ends.push((a, z));
            rep.push(ei as u32);
        }
        if let Some(fs) = welded_faces.get_mut(we) {
            for f in between {
                if !fs.contains(&f) {
                    fs.push(f);
                }
            }
        }
    }
    let points = |we: usize| rep.get(we).map(|e| edge_points(record, *e)).unwrap_or_default();
    // Pieces of a face split on purpose count apart.
    let keep = b.split_keep();
    let split = |we: usize| !keep.is_empty() && on_split_line(&points(we), keep);
    let mut fp: Vec<usize> = (0..nf).collect();
    let union = |fp: &mut Vec<usize>, a: usize, b: usize| {
        let (ra, rb) = (find(fp, a), find(fp, b));
        if let Some(s) = fp.get_mut(ra) {
            *s = rb;
        }
    };
    for (we, fs) in welded_faces.iter().enumerate() {
        if split(we) {
            continue;
        }
        match fs[..] {
            [a, b] if faces.same(a, b) => union(&mut fp, a, b),
            // Two tangent holes touch along a line that both carry: there four faces meet, two
            // on each hole's surface. Each surface's two pieces are one face.
            _ if fs.len() > 2 && fs.len() <= 8 => {
                for (i, &a) in fs.iter().enumerate() {
                    for &b in fs.iter().skip(i + 1) {
                        if faces.surf(a).is_some() && faces.same(a, b) {
                            union(&mut fp, a, b);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut groups: BTreeMap<usize, usize> = BTreeMap::new();
    let mut face_types: BTreeMap<String, usize> = BTreeMap::new();
    for f in 0..nf {
        if is_deg(f) {
            continue;
        }
        let r = find(&mut fp, f);
        if groups.insert(r, f).is_none() {
            *face_types.entry(name(faces.surf(f)).into()).or_insert(0) += 1;
        }
    }
    // Remaining edges, keyed by merged face pair.
    let mut remaining: Vec<((usize, usize), (usize, usize))> = Vec::new(); // (verts, face pair)
    let mut pinches: Vec<Vec3> = Vec::new();
    for (we, fs) in welded_faces.iter().enumerate() {
        if fs.iter().any(|f| is_deg(*f)) {
            continue;
        }
        let gs: Vec<usize> = fs.iter().map(|f| find(&mut fp, *f)).collect();
        // A free-form face pinched to a point (a horn torus at its axis): a zero-length edge
        // inside one face. A degenerate loop is kept there (two edges and a vertex).
        let pts = points(we);
        if let (Some(g), Some(p0)) = (gs.first(), pts.first())
            && gs.iter().all(|x| x == g)
            && faces.surf(*g).is_none()
            && pts.iter().all(|p| p.dist(*p0) < tol * 10.0)
        {
            if !pinches.iter().any(|q: &Vec3| q.dist(*p0) < tol * 10.0) {
                pinches.push(*p0);
            }
            continue;
        }
        let pair = match gs[..] {
            [a, b] if a == b => continue,
            [a, b] => (a.min(b), a.max(b)),
            [a] => (a, a),
            _ => (usize::MAX, usize::MAX),
        };
        if let Some(v) = welded_ends.get(we) {
            remaining.push((*v, pair));
        }
    }
    // Vertex degree among remaining edges.
    let mut deg: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, ((a, b), _)) in remaining.iter().enumerate() {
        deg.entry(*a).or_default().push(i);
        if b != a {
            deg.entry(*b).or_default().push(i);
        }
    }
    let mut ep: Vec<usize> = (0..remaining.len()).collect();
    let mut merge_vertex: HashSet<usize> = HashSet::new();
    for (v, es) in &deg {
        if let [i, j] = es[..]
            && let (Some(a), Some(b)) = (remaining.get(i), remaining.get(j))
            && a.1 == b.1
        {
            union(&mut ep, i, j);
            merge_vertex.insert(*v);
        }
    }
    let mut edge_groups: BTreeMap<usize, bool> = BTreeMap::new(); // root → has a real end vertex
    for (i, ((a, b), _)) in remaining.iter().enumerate() {
        let r = find(&mut ep, i);
        *edge_groups.entry(r).or_insert(false) |= !merge_vertex.contains(a) || !merge_vertex.contains(b);
    }
    let real_vertices = deg.keys().filter(|v| !merge_vertex.contains(v)).count();
    let closed_loops = edge_groups.values().filter(|r| !**r).count();
    // A cone that reaches its apex has a degenerate edge and a vertex there (as other kernels
    // count them).
    let mut apexes = 0;
    for (root, f) in &groups {
        if let Some(Surf::Cone { v: apex, .. }) = faces.surf(*f) {
            let reaches = (0..nf).filter(|g| find(&mut fp, *g) == *root).any(|g| faces.vertices(g).iter().any(|p| p.dist(*apex) < tol * 10.0));
            if reaches {
                apexes += 1;
            }
        }
    }
    let pinch = pinches.len();
    Ok(TopoCounts {
        faces: groups.len(),
        edges: edge_groups.len() + apexes + 2 * pinch,
        vertices: real_vertices + closed_loops + apexes + pinch,
        face_types,
    })
}

/// A cylindrical face of a body near a point: where its axis is, which way, its radius, the
/// extent of the face along the axis (from the axis point), and whether it is a hole (the
/// material outside the cylinder).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct CylinderFace {
    pub axis_point: Vec3,
    pub axis: UnitVec3,
    pub radius: f64,
    pub start: f64,
    pub end: f64,
    pub internal: bool,
}

/// A face's cylinder (its axis pointing the one way for every face of that cylinder), if it
/// lies on one.
fn cylinder_of(faces: &Faces, face: usize) -> Option<(UnitVec3, Vec3, f64, bool)> {
    let Some(Surf::Cylinder { o, a, r, convex }) = faces.surf(face) else { return None };
    let a = a.unit()?;
    let a = if a.x + a.y * 1e-3 + a.z * 1e-6 < 0.0 { -a } else { a };
    Some((a, *o, *r, !*convex))
}

pub fn cylinder_face_at(b: &Body, p: Vec3) -> Option<CylinderFace> {
    let size = b.size();
    let tol = (size * 1e-3).max(1e-3);
    let mesh = b.mesh(&crate::MeshTolerance::chord(tol)).ok()?;
    // Each face's distance from the point (its nearest triangle).
    let mut near: HashMap<u32, f64> = HashMap::new();
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let Some([a, bb, c]) = mesh.tri(t) else { continue };
        let d = solidcraft_geom::compare::closest_on_triangle(p, a, bb, c).dist(p);
        near.entry(*f).and_modify(|x| *x = x.min(d)).or_insert(d);
    }
    let nearest = near.values().copied().fold(f64::INFINITY, f64::min);
    if nearest > size * 0.05 {
        return None;
    }
    let faces = Faces::new(b.record(), (size * 1e-4).max(1e-6) * 10.0);
    // The facets of a curved face lie off it by up to the mesh's tolerance, so the triangle
    // nearest a point on a hole's wall can be one of the face round its rim (they share the
    // rim's points; in a thin plate the wall is a few long facets). Of the faces within that
    // tolerance of the nearest, the point lies on the one whose own surface it is nearest (a
    // cylinder: within its extent).
    let mut candidates: Vec<(u32, f64)> = near.into_iter().filter(|(_, d)| *d <= nearest + tol).collect();
    candidates.sort_by_key(|(f, _)| *f);
    let mut best: Option<(f64, Option<CylinderFace>)> = None;
    for (face, d) in candidates {
        let (off, cyl) = match (faces.surf(face as usize), cylinder_of(&faces, face as usize)) {
            (_, Some((axis, o, r, internal))) => {
                let c = CylinderFace { internal, ..cylinder_extent(&mesh, face, axis, o, r) };
                (d.min(c.off(p)), Some(c))
            }
            (Some(Surf::Plane { n, d: k }), None) => (d.min((n.dot(p) - k).abs()), None),
            _ => (d, None),
        };
        if best.as_ref().is_none_or(|(bo, _)| off < *bo) {
            best = Some((off, cyl));
        }
    }
    best?.1
}

/// Face `face` of a body as a cylinder: the axis and radius of its own surface, and its extent
/// along the axis and side read from `mesh` (the body's tessellation, whose faces are the
/// body's). `None` when the face is not part of a cylinder, or has no triangles.
pub fn cylinder_face(b: &Body, mesh: &Mesh, face: usize) -> Option<CylinderFace> {
    let crate::FaceSurface::Cylinder { origin, axis, radius } = crate::face_surface(b, face)? else { return None };
    let c = cylinder_extent(mesh, u32::try_from(face).ok()?, axis, origin, radius);
    (c.start.is_finite() && c.end.is_finite()).then_some(c)
}

impl CylinderFace {
    /// How far `p` is from the face: off its cylinder, and past its ends along the axis.
    pub fn off(&self, p: Vec3) -> f64 {
        let v = p - self.axis_point;
        let along = v.dot(self.axis);
        ((v - self.axis * along).len() - self.radius).abs() + (self.start - along).max(along - self.end).max(0.0)
    }
}

/// The cylindrical face one of whose end circles is centred at `p` (a circle given by its
/// centre, where no face lies), the nearest when several are.
pub fn cylinder_end_at(b: &Body, p: Vec3) -> Option<CylinderFace> {
    let size = b.size();
    let tol = (size * 1e-3).max(1e-3);
    let mesh = b.mesh(&crate::MeshTolerance::chord(tol)).ok()?;
    let faces = Faces::new(b.record(), (size * 1e-4).max(1e-6) * 10.0);
    let mut best: Option<(f64, CylinderFace)> = None;
    for face in 0..b.record().faces.len() {
        let Some((axis, o, r, internal)) = cylinder_of(&faces, face) else { continue };
        let c = CylinderFace { internal, ..cylinder_extent(&mesh, u32::try_from(face).ok()?, axis, o, r) };
        let v = p - c.axis_point;
        let along = v.dot(c.axis);
        let off = (v - c.axis * along).len() + (along - c.start).abs().min((c.end - along).abs());
        if off <= tol && best.is_none_or(|(bo, _)| off < bo) {
            best = Some((off, c));
        }
    }
    best.map(|(_, c)| c)
}

/// The extent along its axis of a cylindrical face of a tessellated body, and its side as the
/// mesh's normals have it.
fn cylinder_extent(mesh: &Mesh, face: u32, axis: UnitVec3, ap: Vec3, r: f64) -> CylinderFace {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut inward = 0.0;
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if *f != face {
            continue;
        }
        for k in t {
            let (Some(q), Some(n)) = (mesh.positions.get(*k as usize), mesh.normals.get(*k as usize)) else { continue };
            let s = (*q - ap).dot(axis);
            lo = lo.min(s);
            hi = hi.max(s);
            let radial = *q - ap - axis * s;
            inward += n.dot(radial);
        }
    }
    CylinderFace { axis_point: ap, axis, radius: r, start: lo, end: hi, internal: inward < 0.0 }
}
