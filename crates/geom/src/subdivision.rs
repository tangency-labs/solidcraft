//! Bounded Catmull-Clark mesh previews for the local Forms experiment.
//! The control cage stays authoritative; subdivision creates a new quad mesh.
use crate::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

use crate::Mesh;
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
pub mod edit;
pub mod fitting;
pub mod hierarchy;
pub mod matching;
pub mod pipe_network;
pub mod primitives;
pub mod projection;
pub mod stable_extrusion;
pub mod symmetry;
pub mod topology_tools;

pub use solidcore_ops::form_cage::{MAX_FACES, MAX_POINTS};
pub const MAX_LEVELS: usize = 4;
// CAD fitting evaluates a denser grid without exposing it as an editable cage or GPU preview.
const MAX_EVAL_FACES: usize = 500_000;
const MAX_EVAL_POINTS: usize = 600_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeCrease {
    pub edge: [usize; 2],
    pub sharpness: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VertexCrease {
    pub vertex: usize,
    pub sharpness: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cage {
    pub points: Vec<Vec3>,
    pub faces: Vec<Vec<usize>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_creases: Vec<EdgeCrease>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vertex_creases: Vec<VertexCrease>,
}

#[derive(Debug, thiserror::Error)]
#[error("subdivision: {0}")]
pub struct SubdivisionError(pub String);
type Result<T> = std::result::Result<T, SubdivisionError>;
fn bad(message: &str) -> SubdivisionError {
    SubdivisionError(message.into())
}

#[derive(Default)]
struct Edge {
    a: usize,
    b: usize,
    faces: Vec<usize>,
    forward: bool,
}
struct Topology {
    edges: Vec<Edge>,
    lookup: BTreeMap<(usize, usize), usize>,
    incident: Vec<Vec<usize>>,
}

/// Evaluated logical elements on the displayed surface, independent of the cage display.
#[derive(Clone, Debug)]
pub struct SurfacePreview {
    pub mesh: Mesh,
    pub vertices: Vec<Vec3>,
    pub boundaries: Vec<([usize; 2], Vec<Vec3>)>,
    pub index: crate::MeshIndex,
}
fn key(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

impl Cage {
    pub fn with_creases(&self, edges: &[[usize; 2]], vertices: &[usize], sharpness: f64) -> Result<Self> {
        if !sharpness.is_finite() || !(0.0..=10.0).contains(&sharpness) || edges.len() + vertices.len() == 0 {
            return Err(bad("select crease elements and sharpness 0..10"));
        }
        let mut out = self.clone();
        let keys: BTreeSet<_> = edges.iter().map(|e| key(e[0], e[1])).collect();
        let vertices: BTreeSet<_> = vertices.iter().copied().collect();
        out.edge_creases.retain(|c| !keys.contains(&key(c.edge[0], c.edge[1])));
        out.vertex_creases.retain(|c| !vertices.contains(&c.vertex));
        if sharpness > 0.0 {
            out.edge_creases.extend(keys.into_iter().map(|(a, b)| EdgeCrease { edge: [a, b], sharpness }));
            out.vertex_creases.extend(vertices.into_iter().map(|vertex| VertexCrease { vertex, sharpness }));
        }
        out.validate()?;
        Ok(out)
    }
    pub fn point(&self, i: usize) -> Result<Vec3> {
        self.points.get(i).copied().ok_or_else(|| bad("invalid vertex"))
    }

    fn topology(&self) -> Result<Topology> {
        if self.points.is_empty() || self.points.len() > MAX_EVAL_POINTS || self.faces.is_empty() || self.faces.len() > MAX_EVAL_FACES {
            return Err(bad("empty cage or mesh budget exceeded"));
        }
        if self.points.iter().any(|p| !p.is_finite() || p.x.abs().max(p.y.abs()).max(p.z.abs()) > 100_000.0) {
            return Err(bad("coordinates must be finite and within 100000 mm"));
        }
        let mut edges: Vec<Edge> = Vec::new();
        let mut lookup: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        let mut incident = vec![Vec::new(); self.points.len()];
        let mut unique = BTreeSet::new();
        for (fi, face) in self.faces.iter().enumerate() {
            if !(3..=32).contains(&face.len()) {
                return Err(bad("faces require 3..32 distinct vertices"));
            }
            let mut sorted = face.clone();
            sorted.sort_unstable();
            if sorted.windows(2).any(|w| w[0] == w[1]) || !unique.insert(sorted) {
                return Err(bad("repeated vertex or duplicate face"));
            }
            for &v in face {
                self.point(v)?;
                let list = incident.get_mut(v).ok_or_else(|| bad("invalid vertex"))?;
                list.push(fi);
                if list.len() > 64 {
                    return Err(bad("vertex valence exceeds 64"));
                }
            }
            for (a, b) in face.iter().copied().zip(face.iter().copied().cycle().skip(1)).take(face.len()) {
                let k = key(a, b);
                if let Some(&ei) = lookup.get(&k) {
                    let e = edges.get_mut(ei).ok_or_else(|| bad("invalid edge"))?;
                    if e.faces.len() != 1 || e.forward == (a < b) {
                        return Err(bad("nonmanifold edge or inconsistent winding"));
                    }
                    e.faces.push(fi);
                } else {
                    lookup.insert(k, edges.len());
                    edges.push(Edge { a: k.0, b: k.1, faces: vec![fi], forward: a < b });
                }
            }
        }
        let mut vedges = vec![Vec::new(); self.points.len()];
        for (ei, e) in edges.iter().enumerate() {
            for v in [e.a, e.b] {
                vedges.get_mut(v).ok_or_else(|| bad("invalid vertex"))?.push(ei);
            }
        }
        for (v, faces) in incident.iter().enumerate() {
            let es = vedges.get(v).ok_or_else(|| bad("invalid vertex"))?;
            let boundaries = es.iter().filter(|&&i| edges.get(i).is_some_and(|e| e.faces.len() == 1)).count();
            if faces.is_empty() || !matches!(boundaries, 0 | 2) {
                return Err(bad("unused or nonmanifold vertex"));
            }
            let mut reached = BTreeSet::new();
            let mut pending = vec![*faces.first().ok_or_else(|| bad("unused vertex"))?];
            while let Some(f) = pending.pop() {
                if !reached.insert(f) {
                    continue;
                }
                for &ei in es {
                    let e = edges.get(ei).ok_or_else(|| bad("invalid edge"))?;
                    if e.faces.contains(&f) {
                        pending.extend(e.faces.iter().copied().filter(|n| !reached.contains(n)));
                    }
                }
            }
            if reached.len() != faces.len() {
                return Err(bad("disconnected vertex fan"));
            }
        }
        let mut seen = BTreeSet::new();
        for c in &self.edge_creases {
            if !c.sharpness.is_finite()
                || !(0.0..=10.0).contains(&c.sharpness)
                || !lookup.contains_key(&key(c.edge[0], c.edge[1]))
                || !seen.insert(key(c.edge[0], c.edge[1]))
            {
                return Err(bad("invalid or duplicate edge crease"));
            }
        }
        let mut seen = BTreeSet::new();
        for c in &self.vertex_creases {
            if !c.sharpness.is_finite() || !(0.0..=10.0).contains(&c.sharpness) || c.vertex >= self.points.len() || !seen.insert(c.vertex) {
                return Err(bad("invalid or duplicate vertex crease"));
            }
        }
        Ok(Topology { edges, lookup, incident: vedges })
    }

    pub fn validate(&self) -> Result<()> {
        if self.points.len() > MAX_POINTS || self.faces.len() > MAX_FACES {
            return Err(bad("editable cage budget exceeded"));
        }
        self.topology().map(|_| ())
    }

    pub fn edges(&self) -> Result<Vec<[usize; 2]>> {
        Ok(self.topology()?.edges.iter().map(|e| [e.a, e.b]).collect())
    }

    pub fn refined(&self, levels: usize) -> Result<Cage> {
        self.refined_budget(levels)?;
        let mut out = self.clone();
        for _ in 0..levels {
            out = out.step()?;
        }
        Ok(out)
    }
    pub fn preview_level(&self, preferred: usize) -> Result<usize> {
        if preferred > MAX_LEVELS {
            return Err(bad("preview level budget exceeded"));
        }
        self.validate()?;
        (0..=preferred).rev().find(|&level| self.refined_budget(level).is_ok()).ok_or_else(|| bad("no supported preview level"))
    }

    fn step(&self) -> Result<Cage> {
        let t = self.topology()?;
        let sharp: BTreeMap<_, _> = self.edge_creases.iter().map(|c| (key(c.edge[0], c.edge[1]), c.sharpness)).collect();
        let vertex_sharp: BTreeMap<_, _> = self.vertex_creases.iter().map(|c| (c.vertex, c.sharpness)).collect();
        let centers: Vec<Vec3> = self
            .faces
            .iter()
            .map(|f| {
                let mut sum = Vec3::ZERO;
                for &i in f {
                    sum += self.point(i)?;
                }
                Ok(sum / f.len() as f64)
            })
            .collect::<Result<_>>()?;
        let mut points = Vec::with_capacity(self.points.len() + t.edges.len() + centers.len());
        for (v, &p) in self.points.iter().enumerate() {
            let es = t.incident.get(v).ok_or_else(|| bad("invalid vertex"))?;
            let mut boundary = Vec::new();
            let mut fs = BTreeSet::new();
            let mut midpoint = Vec3::ZERO;
            for &ei in es {
                let e = t.edges.get(ei).ok_or_else(|| bad("invalid edge"))?;
                let q = self.point(if e.a == v { e.b } else { e.a })?;
                midpoint += (p + q) / 2.0;
                fs.extend(e.faces.iter().copied());
                if e.faces.len() == 1 {
                    boundary.push(q);
                }
            }
            let smooth = if boundary.len() == 2 {
                (p * 6.0 + boundary[0] + boundary[1]) / 8.0
            } else {
                let mut average = Vec3::ZERO;
                for &f in &fs {
                    average += *centers.get(f).ok_or_else(|| bad("invalid face"))?;
                }
                let n = es.len() as f64;
                (average / fs.len() as f64 + midpoint * (2.0 / n) + p * (n - 3.0)) / n
            };
            let mut creases: Vec<_> = es
                .iter()
                .map(|&i| {
                    let e = &t.edges[i];
                    let s = if e.faces.len() == 1 { 10.0 } else { sharp.get(&key(e.a, e.b)).copied().unwrap_or(0.0) };
                    (s, if e.a == v { e.b } else { e.a })
                })
                .filter(|(s, _)| *s > 0.0)
                .collect();
            creases.sort_by(|a, b| b.0.total_cmp(&a.0));
            let corner = vertex_sharp.get(&v).copied().unwrap_or(0.0);
            let surviving: Vec<_> = creases.iter().copied().filter(|(s, _)| *s > 1.0).collect();
            let rule = |corner: bool, count: usize| if corner { 3 } else { count.min(3) };
            let parent = rule(corner > 0.0, creases.len());
            let child = rule(corner > 1.0, surviving.len());
            let mask = |rule, edges: &[(f64, usize)]| -> Result<Vec3> {
                match rule {
                    0 | 1 => Ok(smooth),
                    2 => Ok((p * 6.0 + self.point(edges[0].1)? + self.point(edges[1].1)?) / 8.0),
                    _ => Ok(p),
                }
            };
            let point = if parent < 2 {
                smooth
            } else if parent == child {
                mask(parent, &creases)?
            } else {
                // Uniform semi-sharp subdivision interpolates the two discrete masks by
                // the mean sharpness of the constraints that disappear at this step.
                let mut transitions: Vec<_> = creases.iter().filter(|(s, _)| *s <= 1.0).map(|(s, _)| *s).collect();
                if corner > 0.0 && corner <= 1.0 {
                    transitions.push(corner);
                }
                if transitions.is_empty() {
                    return Err(bad("missing crease rule transition"));
                }
                let weight = transitions.iter().sum::<f64>() / transitions.len() as f64;
                mask(parent, &creases)? * weight + mask(child, &surviving)? * (1.0 - weight)
            };
            points.push(point);
        }
        let edge_start = points.len();
        for e in &t.edges {
            let mut p = self.point(e.a)? + self.point(e.b)?;
            if e.faces.len() == 2 {
                for &f in &e.faces {
                    p += *centers.get(f).ok_or_else(|| bad("invalid face"))?;
                }
                p = p / 4.0;
            } else {
                p = p / 2.0;
            }
            let s = sharp.get(&key(e.a, e.b)).copied().unwrap_or(0.0).min(1.0);
            p = p * (1.0 - s) + (self.point(e.a)? + self.point(e.b)?) * (s / 2.0);
            points.push(p);
        }
        let face_start = points.len();
        points.extend(centers);
        if points.len() > MAX_EVAL_POINTS {
            return Err(bad("vertex budget exceeded"));
        }
        let mut faces = Vec::with_capacity(self.faces.len() * 4);
        for (fi, f) in self.faces.iter().enumerate() {
            for i in 0..f.len() {
                let v = f[i];
                let next = f[(i + 1) % f.len()];
                let prev = f[(i + f.len() - 1) % f.len()];
                let a = *t.lookup.get(&key(v, next)).ok_or_else(|| bad("invalid edge"))?;
                let b = *t.lookup.get(&key(prev, v)).ok_or_else(|| bad("invalid edge"))?;
                faces.push(vec![v, edge_start + a, face_start + fi, edge_start + b]);
            }
        }
        let decay = |s: f64| if s >= 10.0 { 10.0 } else { (s - 1.0).max(0.0) };
        let mut edge_creases = Vec::new();
        for c in &self.edge_creases {
            let s = decay(c.sharpness);
            if s > 0.0 {
                let mid = edge_start + *t.lookup.get(&key(c.edge[0], c.edge[1])).ok_or_else(|| bad("invalid crease"))?;
                for v in c.edge {
                    edge_creases.push(EdgeCrease { edge: [v, mid], sharpness: s });
                }
            }
        }
        let vertex_creases = self
            .vertex_creases
            .iter()
            .filter_map(|c| {
                let s = decay(c.sharpness);
                (s > 0.0).then_some(VertexCrease { vertex: c.vertex, sharpness: s })
            })
            .collect();
        Ok(Cage { points, faces, edge_creases, vertex_creases })
    }

    pub fn mesh(&self, levels: usize) -> Result<Mesh> {
        Ok(self.surface(levels)?.mesh)
    }

    pub fn surface(&self, levels: usize) -> Result<SurfacePreview> {
        // Validate the growth budget before carrying any per-edge ancestry forward.
        self.refined_budget(levels)?;
        let mut smooth = self.clone();
        let mut roots: Vec<usize> = (0..self.faces.len()).collect();
        let mut paths: Vec<([usize; 2], Vec<usize>)> = self.edges()?.into_iter().map(|e| (e, e.to_vec())).collect();
        for _ in 0..levels {
            let topology = smooth.topology()?;
            for (_, path) in &mut paths {
                let mut next = Vec::with_capacity(path.len() * 2 - 1);
                if let Some(&first) = path.first() {
                    next.push(first);
                }
                for pair in path.windows(2) {
                    let edge = topology.lookup.get(&key(pair[0], pair[1])).copied().ok_or_else(|| bad("missing descendant edge"))?;
                    next.push(smooth.points.len() + edge);
                    next.push(pair[1]);
                }
                *path = next;
            }
            let mut next_roots = Vec::new();
            for (face, &root) in smooth.faces.iter().zip(&roots) {
                next_roots.extend(std::iter::repeat_n(root, face.len()));
            }
            smooth = smooth.step()?;
            roots = next_roots;
        }
        let limit = smooth.limit_vertices()?;
        let vertices = limit.get(..self.points.len()).ok_or_else(|| bad("missing evaluated vertices"))?.to_vec();
        let boundaries = paths
            .into_iter()
            .map(|(edge, path)| {
                Ok((
                    edge,
                    path.into_iter().map(|i| limit.get(i).copied().ok_or_else(|| bad("missing edge limit point"))).collect::<Result<Vec<_>>>()?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut mesh = Mesh { normals: vec![Vec3::ZERO; limit.len()], positions: limit, ..Default::default() };
        for (fi, f) in smooth.faces.iter().enumerate() {
            for corner in 1..f.len() - 1 {
                let tri = [f[0], f[corner], f[corner + 1]];
                let [a, b, c] = tri.map(|i| mesh.positions.get(i).copied().unwrap_or(Vec3::ZERO));
                let normal = (b - a).cross(c - a);
                for i in tri {
                    let n = mesh.normals.get_mut(i).ok_or_else(|| bad("invalid normal"))?;
                    *n += normal;
                }
                mesh.triangles.push(tri.map(|v| v as u32));
                mesh.tri_face.push(roots.get(fi).copied().ok_or_else(|| bad("missing logical face"))? as u32);
            }
        }
        for n in &mut mesh.normals {
            *n = n.normalized().unwrap_or(Vec3::Z);
        }
        let index = crate::MeshIndex::new(&mesh);
        Ok(SurfacePreview { mesh, vertices, boundaries, index })
    }

    fn refined_budget(&self, levels: usize) -> Result<()> {
        self.refined_budget_at(levels, MAX_LEVELS, MAX_FACES)
    }
    fn refined_budget_at(&self, levels: usize, limit: usize, face_budget: usize) -> Result<()> {
        if levels > limit {
            return Err(bad("subdivision level budget exceeded"));
        }
        self.validate()?;
        let mut faces = self.faces.len();
        for level in 0..levels {
            faces = if level == 0 {
                self.faces.iter().map(|f| f.len()).sum()
            } else {
                faces.checked_mul(4).ok_or_else(|| bad("mesh budget exceeded"))?
            };
            if faces > face_budget {
                return Err(bad("mesh budget exceeded"));
            }
        }
        Ok(())
    }
}

/// Original all-quad primitives, with shared control vertices along the box's seams.
pub fn primitive(kind: &str, size: f64, spans: usize) -> Result<Cage> {
    if !size.is_finite() || !(0.1..=10_000.0).contains(&size) || !(1..=4).contains(&spans) {
        return Err(bad("size must be 0.1..10000 mm and spans 1..4"));
    }
    if !matches!(kind, "box" | "plane" | "sphere") {
        return Err(bad("unknown primitive"));
    }
    let n = spans as i32;
    let mut lookup = BTreeMap::new();
    let mut points = Vec::new();
    let mut faces = Vec::new();
    // Each frame has u cross v pointing outwards.
    let frames = if kind == "plane" {
        vec![([0, 0, 0], [1, 0, 0], [0, 1, 0])]
    } else {
        vec![
            ([0, 0, n], [1, 0, 0], [0, 1, 0]),
            ([0, n, 0], [1, 0, 0], [0, 0, -1]),
            ([n, 0, 0], [0, 1, 0], [0, 0, 1]),
            ([0, 0, 0], [0, 1, 0], [1, 0, 0]),
            ([0, 0, 0], [1, 0, 0], [0, 0, 1]),
            ([0, 0, 0], [0, 0, 1], [0, 1, 0]),
        ]
    };
    for (origin, u, v) in frames {
        for j in 0..n {
            for i in 0..n {
                let mut face = [0; 4];
                for (slot, (s, t)) in [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)].into_iter().enumerate() {
                    // The +Y face is parameterised from its upper Z corner.
                    let mut o = origin;
                    if v[2] < 0 {
                        o[2] = n;
                    }
                    let k = [o[0] + s * u[0] + t * v[0], o[1] + s * u[1] + t * v[1], o[2] + s * u[2] + t * v[2]];
                    let id = *lookup.entry(k).or_insert_with(|| {
                        let mut p = Vec3::new(k[0] as f64 / n as f64 - 0.5, k[1] as f64 / n as f64 - 0.5, k[2] as f64 / n as f64 - 0.5) * size;
                        if kind == "plane" {
                            p.z = 0.0;
                        }
                        if kind == "sphere" {
                            p = p.normalized().unwrap_or(Vec3::Z) * (size * 0.5);
                        }
                        let id = points.len();
                        points.push(p);
                        id
                    });
                    face[slot] = id;
                }
                faces.push(face.to_vec());
            }
        }
    }
    let cage = Cage { points, faces, ..Default::default() };
    cage.validate()?;
    Ok(cage)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hierarchy_replay_bound_covers_early_walls_and_later_subdivision() {
        use hierarchy::Hierarchy;
        let cage = primitive("plane", 10.0, 2).unwrap();
        let rim = cage.boundary_loops().unwrap().remove(0);
        let base = Hierarchy::new(cage).unwrap();
        assert_eq!(base.replay_face_bound(), Some(4));

        let (wall, _) = base.extrude_edges(&[[rim[0], rim[1]]], Vec3::Z).unwrap();
        assert_eq!(wall.replay_face_bound(), Some(5));
        let refined = wall.subdivide(&[0], [2, 2]).unwrap();
        assert_eq!(refined.replay_face_bound(), Some(20));
        assert_eq!(refined.evaluate().unwrap().faces.len(), 20);

        let cap = base.subdivide(&[0], [2, 2]).unwrap().extrude_faces(&[4], Vec3::Z).unwrap();
        assert_eq!(cap.replay_face_bound(), Some(20));
        assert_eq!(cap.evaluate().unwrap().faces.len(), 20);
        let mut forged = serde_json::to_value(&cap).unwrap();
        forged["depth"] = serde_json::json!(usize::MAX);
        assert!(serde_json::from_value::<Hierarchy>(forged).unwrap().replay_face_bound().is_none());
    }

    #[test]
    fn quad_cap_rim_reverses_retained_winding_and_covers_every_evaluator_segment() {
        use hierarchy::Hierarchy;
        let cage = primitive("box", 10.0, 1).unwrap();
        let removed = &cage.faces[0];
        let opposite = cage.faces.iter().position(|face| face.iter().all(|v| !removed.contains(v))).unwrap();
        let h = Hierarchy::new(cage).unwrap().subdivide(&[opposite], [2, 2]).unwrap();
        let before = h.layout().unwrap();
        let mut expected = before.faces[0].clone();
        let minimum = expected.iter().enumerate().min_by_key(|(_, v)| *v).unwrap().0;
        expected.rotate_left(minimum);
        let trimmed = h.delete_faces(&[0]).unwrap();
        let rim = trimmed.quad_cap_rim(&[[expected[0], expected[1]]], &[]).unwrap();
        assert!(rim.closes_skin);
        let layout = trimmed.layout().unwrap();
        for (i, side) in rim.sides.iter().enumerate() {
            let [a, b] = [expected[i], expected[(i + 1) % 4]];
            assert_eq!(side.edge, [a, b]);
            assert!(
                layout.faces[side.owner]
                    .iter()
                    .copied()
                    .zip(layout.faces[side.owner].iter().copied().cycle().skip(1))
                    .take(layout.faces[side.owner].len())
                    .any(|pair| pair == (b, a))
            );
            let (_, path) = layout.paths.iter().find(|(edge, _)| *edge == [a.min(b), a.max(b)]).unwrap();
            assert_eq!(path.len(), 3, "each logical side owns two evaluator segments");
        }
        assert_eq!(rim, trimmed.quad_cap_rim(&[], &[expected[3]]).unwrap());
        let interior = layout
            .paths
            .iter()
            .find(|(edge, _)| !rim.sides.iter().any(|side| key(side.edge[0], side.edge[1]) == key(edge[0], edge[1])))
            .map(|(edge, _)| *edge)
            .unwrap();
        assert!(trimmed.quad_cap_rim(&[[expected[0], expected[1]], interior], &[]).is_err());
        assert!(trimmed.quad_cap_rim(&[], &[usize::MAX]).is_err());
        assert_eq!(trimmed.evaluate().unwrap(), h.evaluate().unwrap());

        let original_opening = Hierarchy::new(h.base.delete_faces(&[0]).unwrap()).unwrap().subdivide(&[opposite - 1], [2, 2]).unwrap();
        let cage = original_opening.evaluate().unwrap();
        let opening = cage.boundary_loops().unwrap().remove(0);
        let vertex = *opening.iter().find(|id| original_opening.controls.contains_key(id)).unwrap();
        assert!(original_opening.restore_deleted_holes(&[], &[vertex], false).is_err());
        assert!(original_opening.quad_cap_rim(&[], &[vertex]).unwrap().closes_skin);
    }

    #[test]
    fn separate_quad_caps_close_all_box_openings_and_keep_retired_slots() {
        use hierarchy::Hierarchy;
        let base = primitive("box", 10.0, 1).unwrap();
        let opposite = base.faces.iter().position(|face| face.iter().all(|v| !base.faces[0].contains(v))).unwrap();
        let [a, b] = [0, opposite].map(|face| [base.faces[face][0], base.faces[face][1]]);
        let mut hierarchy = Hierarchy::new(base).unwrap().delete_faces(&[0, opposite]).unwrap();
        let first = hierarchy.reserve_quad_cap(&[a], &[]).unwrap();
        assert!(!hierarchy.validated_quad_caps().unwrap()[0].1.closes_skin);
        let second = hierarchy.reserve_quad_cap(&[b], &[]).unwrap();
        let both = hierarchy.validated_quad_caps().unwrap();
        assert_eq!([both[0].0, both[1].0], [first, second]);
        assert!(both.iter().all(|(_, rim)| rim.closes_skin));
        assert!(hierarchy.reserve_quad_cap(&[b], &[]).is_err());
        let mut reopened = hierarchy.delete_faces(&[second]).unwrap();
        assert!(!reopened.validated_quad_caps().unwrap()[0].1.closes_skin);
        let third = reopened.reserve_quad_cap(&[b], &[]).unwrap();
        assert!(third > second);
        let reopened = reopened.delete_faces(&[first]).unwrap();
        let layout = reopened.layout().unwrap();
        assert!(layout.faces[first].is_empty());
        assert!(layout.faces[second].is_empty());
        assert_eq!(layout.faces[third].len(), 4);
        let saved: Hierarchy = serde_json::from_value(serde_json::to_value(&reopened).unwrap()).unwrap();
        assert_eq!(saved, reopened);

        let valid = serde_json::to_value(&hierarchy).unwrap();
        let mut no_primary = valid.clone();
        no_primary.as_object_mut().unwrap().remove("quad_cap");
        let invalid: Hierarchy = serde_json::from_value(no_primary).unwrap();
        assert!(invalid.layout().is_err());
        let mut too_many = valid;
        let sample = too_many["additional_quad_caps"][0].clone();
        too_many["additional_quad_caps"] = serde_json::json!(
            (0..8)
                .map(|face| {
                    let mut cap = sample.clone();
                    cap["face"] = serde_json::json!(face + 10);
                    cap
                })
                .collect::<Vec<_>>()
        );
        assert!(serde_json::from_value::<Hierarchy>(too_many).is_err());
    }

    #[test]
    fn refined_boundary_collapse_selects_only_exclusive_endpoints_and_keeps_leaf_ids() {
        use hierarchy::Hierarchy;
        let base = Hierarchy::new(primitive("plane", 8.0, 1).unwrap()).unwrap();
        let plane = base.subdivide(&[0], [2, 1]).unwrap().subdivide(&[2], [2, 1]).unwrap();
        let layout = plane.layout().unwrap();
        let [a, b, c, d] = layout.faces[1].as_slice() else { panic!("left leaf quad") };
        let collapse = plane.boundary_collapse([*d, *a]).unwrap();
        assert_eq!(plane.boundary_collapse([*a, *d]).unwrap(), collapse);
        assert_eq!([collapse.face, collapse.retained_neighbor], [1, 3]);
        assert_eq!(collapse.boundary, [collapse.point, *b, *c]);
        assert_eq!(collapse.parameters, [[0.0, 0.5], [0.5, 0.0], [0.5, 1.0]]);
        assert!(layout.faces.iter().all(|face| !face.contains(&collapse.point)));
        assert!(plane.boundary_collapse([*b, *c]).is_err());
        assert!(plane.boundary_collapse([*a, usize::MAX]).is_err());
        assert!(plane.boundary_collapse([*a, *a]).is_err());
        let split_neighbor = base.subdivide(&[0], [2, 1]).unwrap().subdivide(&[2], [1, 2]).unwrap();
        assert!(split_neighbor.boundary_collapse([*a, *d]).is_err(), "a T-junction on the opposite edge needs a separate chart policy");
    }

    #[test]
    fn saved_boundary_collapse_preserves_evaluator_and_exposes_only_exact_diagonals() {
        use hierarchy::Hierarchy;
        let mut source =
            Hierarchy::new(primitive("plane", 8.0, 1).unwrap()).unwrap().subdivide(&[0], [2, 1]).unwrap().subdivide(&[2], [2, 1]).unwrap();
        let raw = source.layout().unwrap();
        let quad = &raw.faces[1];
        let endpoints = [quad[3], quad[0]];
        let cage = source.evaluate().unwrap();
        source.reserve_boundary_collapse(endpoints).unwrap();
        let layout = source.layout().unwrap();
        let site = layout.collapse.unwrap();
        assert_eq!(layout.faces[site.face], site.boundary);
        assert_eq!(layout.faces[site.retained_neighbor], raw.faces[site.retained_neighbor]);
        assert_eq!(source.evaluate().unwrap(), cage);
        assert!(source.controls.contains_key(&site.point));
        assert!(endpoints.iter().all(|&id| source.is_retired_collapse_control(id)));
        assert!(layout.visible_edges().all(|edge| !edge.iter().any(|id| endpoints.contains(id))));
        for edge in site.diagonal_edges() {
            assert!(layout.visible_edges().any(|candidate| candidate == edge));
            assert!(!layout.paths.iter().any(|(candidate, _)| *candidate == edge));
            let charts = source.edge_charts(edge, true).unwrap();
            assert_eq!(charts.len(), 1);
            assert_eq!(charts[0].face, site.root);
            assert!(charts[0].uv[0][0] != charts[0].uv[1][0] && charts[0].uv[0][1] != charts[0].uv[1][1]);
            assert!(source.edge_charts(edge, false).is_err());
        }
        let (surface, logical) = source.surface(&cage, 1).unwrap();
        assert_eq!(logical.faces[site.face], site.boundary);
        assert!(!surface.mesh.tri_face.contains(&(site.face as u32)));
        assert!(surface.mesh.tri_face.contains(&(site.retained_neighbor as u32)));
        let frozen = source.clone();
        assert!(source.add_deltas(&endpoints, &[Vec3::X, Vec3::X]).is_err());
        assert_eq!(source, frozen);
        let reopened: Hierarchy = serde_json::from_value(serde_json::to_value(&source).unwrap()).unwrap();
        assert_eq!(reopened, source);
        assert_eq!(reopened.validated_boundary_collapse().unwrap(), Some(site));
    }

    #[test]
    fn refined_unweld_keeps_mixed_depth_seam_stations_and_disables_automatic_restoration() {
        use hierarchy::Hierarchy;
        let source = Hierarchy::new(primitive("box", 10.0, 1).unwrap()).unwrap().subdivide(&[0], [2, 2]).unwrap();
        let layout = source.layout().unwrap();
        let fine: Vec<_> = source.leaves.iter().enumerate().filter_map(|(id, leaf)| (leaf.active && leaf.root == 0).then_some(id)).collect();
        let mut uses = BTreeMap::new();
        for &id in &fine {
            let face = &layout.faces[id];
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                *uses.entry(key(a, b)).or_insert(0) += 1;
            }
        }
        let seam = uses.into_iter().filter_map(|((a, b), count)| (count == 1).then_some([a, b])).collect::<Vec<_>>();
        assert_eq!(seam.len(), 8);
        assert!(source.split_logical_seam(&seam[..7]).is_err());
        let split = source.split_logical_seam(&seam).unwrap();
        let mut combined = BTreeSet::new();
        for (index, half) in split.halves.iter().enumerate() {
            assert!(half.has_detached_material());
            assert_eq!(half.evaluate().unwrap(), source.evaluate().unwrap());
            let kept = half.layout().unwrap();
            for (id, face) in kept.faces.iter().enumerate().filter(|(_, face)| !face.is_empty()) {
                assert!(combined.insert(id));
                assert_eq!(face, &layout.faces[id]);
            }
            for pair in &split.rim {
                let side = &pair[index];
                let [a, b] = side.edge;
                let sorted = [a.min(b), a.max(b)];
                let path = kept.paths.iter().find(|(edge, _)| *edge == sorted).unwrap();
                assert_eq!(path.1, if a < b { side.path.clone() } else { side.path.iter().rev().copied().collect() });
            }
            let seed = split.rim[0][index].edge;
            let error = half.restore_deleted_holes(&[seed], &[], false).unwrap_err().to_string();
            assert!(error.contains("detached by Unweld"), "{error}");
            let saved: Hierarchy = serde_json::from_value(serde_json::to_value(half).unwrap()).unwrap();
            assert_eq!(&saved, half);
            assert_eq!(saved.layout().unwrap().faces, kept.faces);
            let mut forged = serde_json::to_value(half).unwrap();
            forged["detached_leaves"] = serde_json::json!([split.rim[0][index].owner]);
            let forged: Hierarchy = serde_json::from_value(forged).unwrap();
            assert!(forged.layout().is_err());
        }
        assert_eq!(combined.len(), layout.faces.iter().filter(|face| !face.is_empty()).count());
    }

    #[test]
    fn refined_weld_rims_pair_opposite_owner_winding_without_rebasing_sculpt() {
        use hierarchy::Hierarchy;

        fn half(low: f64, high: f64, top: bool, count: usize) -> Hierarchy {
            let mut points = Vec::new();
            for z in [low, high] {
                for (x, y) in [(-4.0, -4.0), (4.0, -4.0), (4.0, 4.0), (-4.0, 4.0)] {
                    points.push(Vec3::new(x, y, z));
                }
            }
            let mut faces = (0..4)
                .map(|a| {
                    let b = (a + 1) % 4;
                    vec![a, b, b + 4, a + 4]
                })
                .collect::<Vec<_>>();
            faces.push(if top { vec![4, 5, 6, 7] } else { vec![3, 2, 1, 0] });
            let mut h = Hierarchy::new(Cage { points, faces, ..Default::default() }).unwrap().subdivide(&[4], [count, count]).unwrap();
            let cage = h.evaluate().unwrap();
            let center = h
                .controls
                .iter()
                .find(|&(&v, &level)| {
                    let p = cage.points[v];
                    level == 1 && p.x == 0.0 && p.y == 0.0
                })
                .map(|(&v, _)| v)
                .unwrap();
            h.add_deltas(&[center], &[Vec3::Z * 0.25]).unwrap();
            assert_ne!(h.evaluate().unwrap(), cage);
            assert_eq!(h.details[1].len(), 1);
            h
        }

        let lower = half(-5.0, 0.0, false, 2);
        let upper = half(0.0, 5.0, true, 4);
        let old_lower = lower.clone();
        let old_upper = upper.clone();
        let pairs = lower.refined_weld_rims(&upper, [4, 5], [0, 1]).unwrap();
        let expected = [[[5, 4], [0, 1]], [[4, 7], [3, 0]], [[7, 6], [2, 3]], [[6, 5], [1, 2]]];
        assert_eq!(pairs.len(), expected.len());
        let lower_eval = lower.evaluate().unwrap().refined(1).unwrap();
        let upper_eval = upper.evaluate().unwrap();
        let first_path = &pairs[0][0].path;
        assert_eq!(
            first_path.iter().map(|&v| lower_eval.points[v]).collect::<Vec<_>>(),
            vec![Vec3::new(2.75, -2.75, 0.0), Vec3::new(0.0, -3.75, 0.0), Vec3::new(-2.75, -2.75, 0.0)]
        );
        for ([a, b], edges) in pairs.iter().zip(expected) {
            assert_eq!([a.edge, b.edge], edges);
            assert_eq!(a.path.len(), 3);
            assert_eq!(b.path.len(), 5);
            assert_eq!(a.owner, b.owner);
            assert_eq!(lower_eval.points[a.path[0]], upper_eval.points[b.path[4]]);
            assert_eq!(lower_eval.points[a.path[1]], upper_eval.points[b.path[2]]);
            assert_eq!(lower_eval.points[a.path[2]], upper_eval.points[b.path[0]]);
        }
        assert_eq!(lower.refined_weld_rims(&upper, [5, 4], [1, 0]).unwrap(), pairs);
        assert!(lower.refined_weld_rims(&upper, [0, 4], [0, 1]).is_err());
        assert!(lower.refined_weld_rims(&upper, [usize::MAX, 4], [0, 1]).is_err());
        let split = upper.subdivide(&[0], [2, 1]).unwrap();
        assert!(lower.refined_weld_rims(&split, [5, 6], [1, 2]).is_err());
        let closed = Hierarchy::new(primitive("box", 10.0, 1).unwrap()).unwrap().subdivide(&[0], [2, 2]).unwrap();
        let leaf = closed.leaf_corners().unwrap().iter().rposition(Option::is_some).unwrap();
        let cut = closed.delete_faces(&[leaf]).unwrap();
        let corners = closed.leaf_corners().unwrap()[leaf].unwrap();
        let error = lower.refined_weld_rims(&cut, [4, 5], [corners[0], corners[1]]).unwrap_err();
        assert!(error.to_string().contains("original free rim"));
        assert_eq!(lower, old_lower);
        assert_eq!(upper, old_upper);
    }

    #[test]
    fn retired_quad_cap_slots_stay_empty_across_refinement_restoration_and_recapping() {
        use hierarchy::Hierarchy;
        let cage = primitive("box", 10.0, 1).unwrap();
        let opposite = cage.faces.iter().position(|face| face.iter().all(|v| !cage.faces[0].contains(v))).unwrap();
        let hierarchy = Hierarchy::new(cage).unwrap().subdivide(&[opposite], [2, 2]).unwrap();
        let raw = hierarchy.layout().unwrap();
        let edge: [usize; 2] = raw.faces[0][..2].try_into().unwrap();
        let hole = hierarchy.delete_faces(&[0]).unwrap();
        let mut capped = hole.clone();
        let first = capped.leaves.len();
        capped.reserve_quad_cap(&[edge], &[]).unwrap();
        let (_, rim) = capped.validated_quad_cap().unwrap().unwrap();
        assert!(capped.delete_faces(&[first, opposite]).is_err());
        assert_eq!(capped.quad_cap_face(), Some(first));
        let removed = capped.delete_faces(&[first]).unwrap();
        let layout = removed.layout().unwrap();
        assert_eq!(layout.faces[..first], hole.layout().unwrap().faces);
        assert!(layout.faces[first].is_empty());
        assert_eq!(layout.owners, hole.layout().unwrap().owners);
        assert_eq!(removed.evaluate().unwrap(), capped.evaluate().unwrap());
        assert_eq!(removed.controls, capped.controls);
        assert_eq!(removed.details, capped.details);
        assert!(removed.delete_faces(&[first]).is_err());
        assert_eq!(removed.quad_cap_rim(&[edge], &[]).unwrap(), rim);
        let restored = removed.restore_deleted_holes(&[edge], &[], true).unwrap();
        assert_eq!(restored.layout().unwrap().faces[0], raw.faces[0]);
        assert!(restored.layout().unwrap().faces[first].is_empty());

        let mut saved: Hierarchy = serde_json::from_value(serde_json::to_value(&removed).unwrap()).unwrap();
        assert_eq!(saved, removed);
        let child = saved.leaves.iter().position(|leaf| leaf.active && leaf.root == opposite).unwrap();
        saved = saved.subdivide(&[child], [2, 2]).unwrap();
        let second = saved.leaves.len();
        assert!(second > first + 1);
        saved.reserve_quad_cap(&[edge], &[]).unwrap();
        assert_eq!(saved.validated_quad_cap().unwrap(), Some((second, rim)));
        assert!(saved.layout().unwrap().faces[first].is_empty());
        assert_eq!(saved.layout().unwrap().faces[second].len(), 4);

        let mut invalid = serde_json::to_value(&removed).unwrap();
        invalid["retired_quad_caps"] = serde_json::json!([first, first]);
        assert!(serde_json::from_value::<Hierarchy>(invalid).is_err());
        let mut invalid = serde_json::to_value(&removed).unwrap();
        invalid["retired_quad_caps"] = serde_json::json!([0]);
        let invalid: Hierarchy = serde_json::from_value(invalid).unwrap();
        assert!(invalid.layout().is_err());
        assert!(invalid.evaluate().is_err());
    }

    #[test]
    fn refined_erase_and_cap_uses_the_complete_new_interface_and_preserves_older_openings() {
        use hierarchy::Hierarchy;
        let parameters = primitives::Parameters { kind: "face".into(), dimensions: [12.0; 3], segments: [6, 6, 1], ..Default::default() };
        let old = Hierarchy::new(parameters.build().unwrap()).unwrap();
        let mut source = old.subdivide(&[28], [4, 4]).unwrap().delete_faces(&[7]).unwrap();
        let selected: Vec<_> = source.leaves.iter().enumerate().filter_map(|(id, leaf)| (leaf.active && leaf.root == 28).then_some(id)).collect();
        assert_eq!(selected.len(), 16);
        let corners = source.leaf_corners().unwrap();
        let center = corners[selected[5]].unwrap()[2];
        source.add_deltas(&[center], &[Vec3::Z * 0.25]).unwrap();
        let old_skin = source.evaluate().unwrap();
        let layout = source.layout().unwrap();
        let old_openings = old_skin.boundary_loops_where(|cell| layout.owners[cell].is_some()).unwrap();
        assert_eq!(old_openings.len(), 2, "the sheet perimeter and an earlier hole are both free");
        let mut disconnected = selected.clone();
        disconnected.push(10);
        for invalid in [vec![0], vec![8], vec![15, 16], vec![usize::MAX], vec![selected[0]; 2], disconnected] {
            assert!(source.erase_and_cap(&invalid).is_err(), "{invalid:?}");
        }
        let replaced = source.erase_and_cap(&selected).unwrap();
        let (cap, rim) = replaced.validated_quad_cap().unwrap().unwrap();
        assert_eq!(cap, source.leaves.len());
        assert!(!rim.closes_skin);
        assert_eq!(replaced.evaluate().unwrap(), old_skin);
        assert_eq!(replaced.details, source.details);
        assert_eq!(replaced.base, source.base);
        let next = replaced.layout().unwrap();
        assert!(next.faces[7].is_empty());
        assert!(selected.iter().all(|&id| next.faces[id].is_empty()));
        assert!(next.faces[cap].iter().all(|v| source.base.faces[28].contains(v)));
        for (cell, owner) in layout.owners.iter().enumerate() {
            assert_eq!(next.owners[cell], owner.filter(|id| !selected.contains(id)));
        }
        let reopened = replaced.delete_faces(&[cap]).unwrap();
        let skin = reopened.layout().unwrap();
        let openings = old_skin.boundary_loops_where(|cell| skin.owners[cell].is_some()).unwrap();
        assert_eq!(openings.len(), old_openings.len() + 1);
        assert!(old_openings.iter().all(|rim| openings.contains(rim)));
        let restored = reopened.restore_deleted_holes(&[rim.sides[0].edge], &[], false).unwrap();
        assert_eq!(restored.evaluate().unwrap(), old_skin);
        assert_eq!(restored.layout().unwrap().owners, layout.owners);
        assert!(restored.layout().unwrap().faces[cap].is_empty());
    }

    #[test]
    fn refined_fill_recovers_terminal_children_and_preserves_an_unrelated_dissolve() {
        use hierarchy::Hierarchy;
        let parameters = primitives::Parameters { kind: "face".into(), segments: [4, 4, 1], ..Default::default() };
        let h = Hierarchy::new(parameters.build().unwrap()).unwrap().subdivide(&[5], [2, 2]).unwrap();
        let parent = h.leaves.len() - 4;
        let first = h.leaves.len();
        let h = h.subdivide(&[parent], [2, 2]).unwrap();
        let layout = h.layout().unwrap();
        let has = |face: &[usize], edge: [usize; 2]| {
            face.iter().copied().zip(face.iter().copied().cycle().skip(1)).take(face.len()).any(|(a, b)| key(a, b) == key(edge[0], edge[1]))
        };
        let seam = layout.paths.iter().find(|(edge, _)| has(&layout.faces[14], *edge) && has(&layout.faces[15], *edge)).unwrap().0;
        let h = h.dissolve_edges(&[seam]).unwrap();
        assert!(h.leaves[15].active && h.layout().unwrap().faces[15].is_empty(), "dissolved owners remain active material");
        let all_children: Vec<_> = (first..h.leaves.len()).collect();
        let deleted = h.delete_faces(&all_children).unwrap();
        let cage = deleted.evaluate().unwrap();
        let layout = deleted.layout().unwrap();
        let rims = cage.boundary_loops_where(|cell| layout.owners[cell].is_some()).unwrap();
        assert_eq!(rims.len(), 2);
        let hole = rims.iter().min_by_key(|rim| rim.len()).unwrap();
        let outer = rims.iter().max_by_key(|rim| rim.len()).unwrap();
        let seed = *hole.iter().find(|v| deleted.controls.contains_key(v)).unwrap();
        let outside = *outer.iter().find(|v| deleted.controls.contains_key(v)).unwrap();
        assert!(deleted.restore_deleted_holes(&[], &[outside], false).is_err(), "the original open sheet rim has no retained material");
        assert!(deleted.restore_deleted_holes(&[], &[seed, outside], false).is_err(), "a mixed valid/unsupported fill is atomic");
        let hole_edges: BTreeSet<_> =
            hole.iter().copied().zip(hole.iter().copied().cycle().skip(1)).take(hole.len()).map(|(a, b)| key(a, b)).collect();
        let edge = layout.paths.iter().find(|(_, path)| path.windows(2).all(|w| hole_edges.contains(&key(w[0], w[1])))).unwrap().0;
        assert!(
            deleted.restore_deleted_holes(&[edge], &[outside], false).is_err(),
            "an edge pick cannot hide an explicitly selected unsupported point"
        );
        let filled = deleted.restore_deleted_holes(&[edge], &[seed], false).unwrap();
        assert_eq!(filled, h);
        assert!(!filled.leaves[parent].active, "the old parent must stay retired even when all its children were deleted");
        let mut ambiguous = deleted.clone();
        ambiguous.leaves.push(ambiguous.leaves[first].clone());
        assert!(ambiguous.restore_deleted_holes(&[], &[seed], false).is_err(), "duplicate inactive domains cannot supply material");
        let layout = h.layout().unwrap();
        let id = *all_children.last().unwrap();
        let point = layout.faces[id][0];
        assert_eq!(layout.faces[id].len(), 4);
        let one = h.delete_faces(&[id]).unwrap();
        assert_eq!(one.restore_deleted_holes(&[], &[point], true).unwrap(), h);
    }
    #[test]
    fn forms_logical_deletion_and_dissolve_preserve_the_sculpted_evaluator() {
        use hierarchy::Hierarchy;
        let cage = primitives::Parameters::default().build().unwrap();
        let mut h = Hierarchy::new(cage).unwrap();
        h.add_deltas(&[0], &[Vec3::new(0.5, 0.0, 0.3)]).unwrap();
        let before = h.evaluate().unwrap();
        let edge = h.base.edges().unwrap().into_iter().find(|e| h.base.faces[0].windows(2).any(|p| key(p[0], p[1]) == key(e[0], e[1]))).unwrap();
        let dissolved = h.dissolve_edges(&[edge]).unwrap();
        let layout = dissolved.layout().unwrap();
        assert_eq!(layout.faces.iter().filter(|f| !f.is_empty()).count(), 23);
        assert!(layout.faces.iter().any(|f| f.len() == 6));
        assert_eq!(dissolved.evaluate().unwrap(), before);
        let a = h.surface(&before, 2).unwrap().0;
        let b = dissolved.surface(&before, 2).unwrap().0;
        assert_eq!(a.mesh.positions, b.mesh.positions);
        assert_eq!(a.mesh.triangles, b.mesh.triangles);
        assert!(!b.boundaries.iter().any(|(e, _)| key(e[0], e[1]) == key(edge[0], edge[1])));
        let representative = layout.faces.iter().position(|f| f.len() == 6).unwrap();
        let removed = dissolved.delete_faces(&[representative]).unwrap();
        assert_eq!(removed.evaluate().unwrap(), before);
        assert_eq!(removed.layout().unwrap().faces.iter().filter(|f| !f.is_empty()).count(), 22);
        assert!(removed.dissolved_edges.is_empty());
        assert_eq!(serde_json::from_str::<Hierarchy>(&serde_json::to_string(&removed).unwrap()).unwrap(), removed);

        let local = h.subdivide(&[0], [2, 1]).unwrap();
        let leaf = local.leaves.iter().position(|l| l.active && l.root == 0).unwrap();
        let removed = local.delete_faces(&[leaf]).unwrap();
        assert_eq!(removed.evaluate().unwrap(), local.evaluate().unwrap());
        let layout = removed.layout().unwrap();
        assert_eq!(layout.faces.iter().filter(|f| !f.is_empty()).count(), 24);
        let missing = layout.owners.iter().filter(|o| o.is_none()).count();
        assert_eq!(missing, 2);
        let a = local.surface(&local.evaluate().unwrap(), 1).unwrap().0;
        let b = removed.surface(&removed.evaluate().unwrap(), 1).unwrap().0;
        assert_eq!(a.mesh.positions, b.mesh.positions);
        assert_eq!(a.mesh.triangles.len() - b.mesh.triangles.len(), missing * 8);
        let all: Vec<_> = layout.faces.iter().enumerate().filter_map(|(id, f)| (!f.is_empty()).then_some(id)).collect();
        assert!(removed.delete_faces(&all).is_err());
        assert!(removed.delete_faces(&[usize::MAX]).is_err());
    }

    #[test]
    fn dissolved_leaf_corners_can_remain_evaluator_only() {
        use hierarchy::Hierarchy;
        let parameters = primitives::Parameters { kind: "face".into(), segments: [2, 2, 1], ..Default::default() };
        let base = parameters.build().unwrap();
        let center = base.points.iter().position(|point| *point == Vec3::ZERO).unwrap();
        let seams = base.edges().unwrap().into_iter().filter(|edge| edge.contains(&center)).collect::<Vec<_>>();
        let hierarchy = Hierarchy::new(base.clone()).unwrap().dissolve_edges(&seams).unwrap();
        assert!(!hierarchy.controls.contains_key(&center));
        let layout = hierarchy.layout().unwrap();
        assert_eq!(layout.faces.iter().filter(|face| !face.is_empty()).count(), 1);
        assert!(!layout.faces.iter().flatten().any(|&vertex| vertex == center));
        assert_eq!(hierarchy.evaluate().unwrap(), base);
    }
    #[test]
    fn ordinary_box_and_quadball_face_extrusions_fit_the_default_cad_tolerance() {
        for kind in ["box", "quadball"] {
            let c = primitives::Parameters { kind: kind.into(), ..Default::default() }.build().unwrap();
            let extruded = c.extrude_faces(&[0], Vec3::X * 8.0).unwrap();
            assert_eq!(extruded.faces.len(), c.faces.len() + 4);
            let fit = extruded.fit_patches(0.01).unwrap();
            assert!(fit.sampled_deviation <= 0.01, "{kind}: {}", fit.sampled_deviation);
            assert_eq!(fit.patches.len(), fit.patch_roots.len());
            assert_eq!(fit.patch_roots.iter().copied().collect::<BTreeSet<_>>(), (0..extruded.faces.len()).collect());
            assert!(fit.patches.iter().all(|p| p.count().into_iter().all(|n| n <= 17)));
        }
    }
    #[test]
    fn hierarchical_leaves_preserve_surface_and_nested_detail_levels() {
        use hierarchy::Hierarchy;
        let cage = primitive("box", 10.0, 1).unwrap();
        let h = Hierarchy::new(cage.clone()).unwrap().subdivide(&[0], [2, 1]).unwrap();
        assert_eq!(h.depth, 1);
        let layout = h.layout().unwrap();
        assert_eq!(layout.faces.iter().filter(|f| !f.is_empty()).count(), 7);
        assert!(layout.faces[0].is_empty());
        assert!(layout.faces[1..6].iter().any(|f| f.len() > 4));
        assert_eq!(h.evaluate().unwrap(), cage.refined(1).unwrap());
        let (surface, _) = h.surface(&h.evaluate().unwrap(), 2).unwrap();
        let before = cage.surface(3).unwrap();
        assert_eq!(surface.mesh.positions, before.mesh.positions);
        let a = h.seam_endpoints(0, 0).unwrap();
        let h = h.subdivide(&[6], [2, 2]).unwrap();
        assert_eq!(h.depth, 2);
        assert!(h.leaves[7].active);
        assert_eq!(h.evaluate().unwrap(), cage.refined(2).unwrap());
        let mut edited = h.clone();
        edited.add_deltas(&[a[0]], &[Vec3::Z]).unwrap();
        let coarse = edited.evaluate().unwrap();
        let basis = h.basis(a[0]).unwrap();
        for ((p, q), b) in coarse.points.iter().zip(h.evaluate().unwrap().points).zip(basis.points) {
            assert!((*p - q - Vec3::Z * b.x).len() < 1e-10);
        }
        let saved = serde_json::to_string(&edited).unwrap();
        assert_eq!(serde_json::from_str::<Hierarchy>(&saved).unwrap(), edited);
        assert!(h.subdivide(&[0], [2, 2]).is_err());
        assert!(h.subdivide(&[7], [3, 2]).is_err());
        let mut hostile = h;
        hostile.leaves[7].domain = [10, 0, 0, 16];
        assert!(hostile.layout().is_err());
    }
    #[test]
    fn least_squares_projection_is_geometric_and_rejects_ambiguous_directions() {
        use projection::{Projection, project};
        let center = Vec3::new(7.0, -2.0, 5.0);
        let u = Vec3::new(1.0, 2.0, 0.0).normalized().unwrap();
        let normal = Vec3::new(2.0, -1.0, 3.0).normalized().unwrap();
        let v = normal.cross(u);
        let points: Vec<_> = [-2.0, 2.0]
            .into_iter()
            .flat_map(|x| [-1.0, 1.0].into_iter().flat_map(move |y| [-0.1, 0.1].map(|z| center + u * x + v * y + normal * z)))
            .collect();
        let line = project(&points, Projection::Line, None, None).unwrap();
        let plane = project(&points, Projection::Plane, None, None).unwrap();
        for ((p, l), f) in points.iter().zip(line).zip(plane) {
            assert!(l.dist(center + u * (*p - center).dot(u)) < 1e-10);
            assert!(f.dist(*p - normal * (*p - center).dot(normal)) < 1e-10);
        }
        let xy = project(&points, Projection::Plane, Some(Vec3::Z), None).unwrap();
        assert!(xy.iter().all(|p| (p.z - center.z).abs() < 1e-12));
        let x = project(&points, Projection::Line, Some(Vec3::X), Some(Vec3::ZERO)).unwrap();
        assert!(x.iter().all(|p| p.y == 0.0 && p.z == 0.0));
        let cube = primitive("box", 2.0, 1).unwrap();
        assert!(project(&cube.points, Projection::Line, None, None).is_err());
        assert!(project(&cube.points, Projection::Plane, None, None).is_err());
        assert!(project(&points, Projection::Plane, Some(Vec3::ZERO), None).is_err());
        assert!(project(&[Vec3::ZERO; 3], Projection::Plane, None, None).is_err());
        assert!(project(&[Vec3::ZERO, Vec3::X, Vec3::X * 2.0], Projection::Plane, None, None).is_err());
        assert!(project(&points, Projection::Line, None, Some(Vec3::new(f64::NAN, 0.0, 0.0))).is_err());
    }
    #[test]
    fn finite_creases_decay_and_infinite_corners_remain_fixed() {
        let c = primitive("box", 2.0, 1).unwrap();
        let edges = c.edges().unwrap();
        let finite = c.with_creases(&edges[..1], &[], 1.5).unwrap();
        let refined = finite.refined(1).unwrap();
        assert_eq!(refined.edge_creases.len(), 2);
        assert!(refined.edge_creases.iter().all(|e| e.sharpness == 0.5));
        assert!(finite.refined(2).unwrap().edge_creases.is_empty());
        let limit = finite.limit_vertices().unwrap();
        assert!(limit.iter().all(|p| p.is_finite()));
        let sharp = c.with_creases(&edges, &[], 10.0).unwrap();
        assert_eq!(&sharp.refined(3).unwrap().points[..8], &c.points);
        assert_eq!(sharp.limit_vertices().unwrap(), c.points);
        assert!(sharp.fit_patches(1e-5).unwrap().sampled_deviation < 1e-8);
        assert!(c.with_creases(&[[0, usize::MAX]], &[], 2.0).is_err());
        let incident: Vec<_> = edges.iter().copied().filter(|e| e.contains(&0)).collect();
        let half = c.with_creases(&incident, &[], 0.5).unwrap().refined(1).unwrap();
        let smooth = c.refined(1).unwrap();
        assert!(half.points[0].dist((smooth.points[0] + c.points[0]) / 2.0) < 1e-12);
        let dart = c.with_creases(&incident[..1], &[], 10.0).unwrap();
        let a = dart.limit_vertices().unwrap();
        let b = dart.refined(1).unwrap().limit_vertices().unwrap();
        for (p, q) in a.iter().zip(b) {
            assert!(p.dist(q) < 1e-10);
        }
    }
    #[test]
    fn cage_topology_edits_keep_creases_on_their_duplicated_geometry() {
        let cage = primitive("box", 2.0, 1).unwrap();
        let [a, b] = [cage.faces[0][0], cage.faces[0][1]];
        let cage = cage.with_creases(&[[a, b]], &[a], 2.0).unwrap();
        let delta = Vec3::Z * 5.0;
        for (edited, offset) in [(cage.extrude_faces(&[0], delta).unwrap(), delta), (cage.unweld(&[0]).unwrap(), Vec3::ZERO)] {
            assert_eq!(edited.edge_creases.len(), 2);
            assert_eq!(edited.vertex_creases.len(), 2);
            assert!(edited.vertex_creases.iter().any(|c| edited.points[c.vertex] == cage.points[a] + offset && c.sharpness == 2.0));
        }
        let mut invalid = cage;
        invalid.faces[0][0] = usize::MAX;
        assert!(invalid.unweld(&[0]).is_err());
    }
    #[test]
    fn cube_stencil_and_closed_topology() {
        let c = primitive("box", 2.0, 1).unwrap();
        assert_eq!((c.points.len(), c.faces.len(), c.edges().unwrap().len()), (8, 6, 12));
        let r = c.refined(1).unwrap();
        r.validate().unwrap();
        assert_eq!((r.points.len(), r.faces.len()), (26, 24));
        for p in &r.points[..8] {
            assert!((p.x.abs() - 5.0 / 9.0).abs() < 1e-12);
            assert!((p.y.abs() - 5.0 / 9.0).abs() < 1e-12);
            assert!((p.z.abs() - 5.0 / 9.0).abs() < 1e-12);
        }
        let mesh = c.mesh(3).unwrap();
        for tri in &mesh.triangles {
            let [a, b, d] = tri.map(|i| mesh.positions[i as usize]);
            assert!((b - a).cross(d - a).dot((a + b + d) / 3.0) > 0.0);
        }
    }
    #[test]
    fn boundary_and_affine_equivariance() {
        let c = primitive("plane", 2.0, 1).unwrap();
        let r = c.refined(1).unwrap();
        assert!(r.points[0].dist(Vec3::new(-0.75, -0.75, 0.0)) < 1e-12);
        for kind in ["box", "sphere", "plane"] {
            for spans in 1..=4 {
                let c = primitive(kind, 30.0, spans).unwrap();
                let moved = Cage {
                    points: c.points.iter().map(|p| *p * 2.0 + Vec3::new(3.0, 4.0, 5.0)).collect(),
                    faces: c.faces.clone(),
                    ..Default::default()
                };
                for (a, b) in c.refined(2).unwrap().points.iter().zip(moved.refined(2).unwrap().points) {
                    assert!((*a * 2.0 + Vec3::new(3.0, 4.0, 5.0)).dist(b) < 1e-10);
                }
            }
        }
    }
    fn mixed_depth_loop_fixture() -> hierarchy::Hierarchy {
        let cage =
            primitives::Parameters { kind: "face".into(), dimensions: [8.0, 4.0, 1.0], segments: [2, 1, 1], ..Default::default() }.build().unwrap();
        let mut h = hierarchy::Hierarchy::new(cage).unwrap().subdivide(&[1], [2, 2]).unwrap();
        let vertex = *h.controls.iter().find(|(_, birth)| **birth == 1).unwrap().0;
        h.add_deltas(&[vertex], &[Vec3::new(0.0, 0.0, 0.08)]).unwrap();
        h
    }
    #[test]
    fn hierarchical_parameter_sides_trace_a_t_control_and_reuse_existing_fine_seams() {
        let h = mixed_depth_loop_fixture();
        let sides = h.parameter_sides().unwrap();
        let coarse = sides.iter().find(|leaf| leaf.id == 0).unwrap();
        assert_eq!(coarse.sides.iter().map(Vec::len).collect::<Vec<_>>(), vec![1, 2, 1, 1]);
        let seed = coarse.sides[3][0].edge;
        let plan = h.aligned_loop(seed, 0.5).unwrap();
        assert_eq!(plan.cuts, vec![hierarchy::LeafCut { parent: 0, axis: 1, coordinate: 8 }]);
        assert_eq!(plan.existing_edges.len(), 2);
        assert_eq!(plan.points.len(), 4);
        assert_eq!(plan.points.iter().filter(|point| point.existing.is_some()).count(), 3);
        let after = h.split_at(&plan.cuts).unwrap();
        assert_eq!(after.depth, h.depth);
        assert_eq!(after.evaluate().unwrap(), h.evaluate().unwrap(), "aligned topology must not displace the evaluator");
        assert_eq!(after.details, h.details);
        for (&id, &birth) in &h.controls {
            assert_eq!(after.controls.get(&id), Some(&birth));
        }
        let vertices = after.aligned_loop_vertices(&plan.points).unwrap();
        for (point, &id) in plan.points.iter().zip(&vertices) {
            if let Some(old) = point.existing {
                assert_eq!(old, id);
            }
        }
        let layout = after.layout().unwrap();
        for edge in &plan.existing_edges {
            assert!(layout.paths.iter().any(|(other, _)| other == edge));
        }
        assert_eq!(after.leaves.iter().filter(|leaf| leaf.active).count(), h.leaves.iter().filter(|leaf| leaf.active).count() + 1);
        let reversed = h.aligned_loop([seed[1], seed[0]], 0.5).unwrap();
        assert_eq!(plan.cuts, reversed.cuts);
        let mut unchanged = h.clone();
        assert!(unchanged.aligned_loop(seed, 0.3).is_err());
        assert_eq!(unchanged, h);
        // Empty cut plans are genuine no-ops, not another logical seam.
        unchanged = unchanged.split_at(&[]).unwrap();
        assert_eq!(unchanged, h);
    }
    #[test]
    fn hierarchical_nonmidpoint_cuts_use_coordinate_depth_and_preserve_exact_parameter_area() {
        let h = mixed_depth_loop_fixture();
        let sides = h.parameter_sides().unwrap();
        let seed = sides.iter().find(|leaf| leaf.id == 0).unwrap().sides[3][0].edge;
        let point_y = |f: f64| {
            let side = &sides.iter().find(|leaf| leaf.id == 0).unwrap().sides[3][0];
            side.uv[0][1] as f64 + (side.uv[1][1] as f64 - side.uv[0][1] as f64) * f
        };
        // Canonical ID orientation can reverse the left geometric boundary.
        let fraction = if point_y(0.25) == 4.0 { 0.25 } else { 0.75 };
        let plan = h.aligned_loop(seed, fraction).unwrap();
        assert_eq!(plan.cuts.len(), 3, "one coarse cut plus exactly two fine-cell cuts");
        let after = h.split_at(&plan.cuts).unwrap();
        assert_eq!(after.depth, 2);
        assert_eq!(
            after.evaluate().unwrap(),
            h.evaluate().unwrap().refined(1).unwrap(),
            "new zero-detail evaluation preserves the actual sculpted limit source"
        );
        assert_eq!(&after.details[..h.details.len()], h.details.as_slice());
        assert_eq!(after.leaves.iter().filter(|leaf| leaf.active).count(), h.leaves.iter().filter(|leaf| leaf.active).count() + 3);
        for root in 0..2 {
            let area: usize = after
                .leaves
                .iter()
                .filter(|leaf| leaf.active && leaf.root == root)
                .map(|leaf| (leaf.domain[2] - leaf.domain[0]) * (leaf.domain[3] - leaf.domain[1]))
                .sum();
            assert_eq!(area, 256);
        }
        let parent = after.leaves.iter().position(|leaf| leaf.active && leaf.root == 0 && leaf.domain[1] == 4 && leaf.domain[3] == 16).unwrap();
        let split = after.subdivide(&[parent], [1, 2]).unwrap();
        assert_eq!(split.depth, 3);
        assert!(split.leaves.iter().any(|leaf| leaf.active && leaf.root == 0 && leaf.domain[1] == 10));
        let odd = h.split_at(&[hierarchy::LeafCut { parent: 0, axis: 1, coordinate: 3 }]).unwrap();
        assert_eq!(odd.depth, 4);
        let parent = odd.leaves.iter().position(|leaf| leaf.active && leaf.root == 0 && leaf.domain[1] == 0 && leaf.domain[3] == 3).unwrap();
        assert!(odd.subdivide(&[parent], [1, 2]).is_err());
        assert!(h.split_at(&[hierarchy::LeafCut { parent: 0, axis: 2, coordinate: 8 }]).is_err());
        assert!(h.split_at(&[hierarchy::LeafCut { parent: 0, axis: 1, coordinate: 0 }]).is_err());
        // Existing serialized domain/control fields already carry nonuniform
        // dyadic rectangles; no private cache or new persistent handle is used.
        let decoded: hierarchy::Hierarchy = serde_json::from_slice(&serde_json::to_vec(&after).unwrap()).unwrap();
        assert_eq!(decoded, after);
        assert_eq!(decoded.layout().unwrap().faces, after.layout().unwrap().faces);
    }

    #[test]
    fn aligned_loop_validates_nonbinary_fractions_in_the_callers_edge_direction() {
        let cage = primitives::Parameters { kind: "face".into(), segments: [1; 3], ..Default::default() }.build().unwrap();
        let h = hierarchy::Hierarchy::new(cage).unwrap().split_at(&[hierarchy::LeafCut { parent: 0, axis: 1, coordinate: 3 }]).unwrap();
        let sides = h.parameter_sides().unwrap();
        let leaf = sides.iter().find(|leaf| leaf.id == 1).unwrap();
        let segment = &leaf.sides[3][0];
        assert_eq!(segment.uv.map(|uv| uv[1]), [0, 3]);
        let edge = segment.edge;
        let forward = h.aligned_loop(edge, 1.0 / 3.0).unwrap();
        let reversed = h.aligned_loop([edge[1], edge[0]], 2.0 / 3.0).unwrap();
        assert_eq!(forward.cuts, [hierarchy::LeafCut { parent: 1, axis: 1, coordinate: 1 }]);
        assert_eq!(forward.cuts, reversed.cuts);
        assert_eq!(h.split_at(&forward.cuts).unwrap(), h.split_at(&reversed.cuts).unwrap());
        assert!(h.aligned_loop_from_leaf(1, 1).is_err(), "an odd extent has no exact midpoint on the stored grid");
    }

    #[test]
    fn hierarchical_aligned_closed_quarter_itinerary_is_independent_of_the_plan_counts() {
        let cage = primitives::Parameters { segments: [1; 3], ..Default::default() }.build().unwrap();
        let h = hierarchy::Hierarchy::new(cage).unwrap().subdivide(&[0], [2, 2]).unwrap();
        let c = h.leaf_corners().unwrap()[4].unwrap();
        let seed = [c[0], c[1]];
        let plan = h.aligned_loop(seed, 0.25).unwrap();
        let reversed = h.aligned_loop([seed[1], seed[0]], 0.75).unwrap();
        // From the six documented box frames, x=one-quarter runs top(0),
        // back(1), bottom(3), front(4), and never left/right(5/2). Root0
        // has two crossed fine rows: children6 and8, both at root U=4.
        let expected: std::collections::BTreeSet<_> = [(6, 0, 4), (8, 0, 4), (1, 0, 4), (3, 1, 4), (4, 0, 4)].into_iter().collect();
        let cuts = |plan: &hierarchy::AlignedLoop| {
            plan.cuts.iter().map(|cut| (cut.parent, cut.axis, cut.coordinate)).collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(cuts(&plan), expected);
        assert_eq!(cuts(&reversed), expected);
        assert!(plan.existing_edges.is_empty());
        assert_eq!(plan.points.len(), 5);
        let after = h.split_at(&plan.cuts).unwrap();
        let reverse = h.split_at(&reversed.cuts).unwrap();
        assert_eq!(after, reverse);
        assert_eq!(after.leaves.iter().filter(|leaf| leaf.active).count(), 14);
        assert_eq!(after.evaluate().unwrap(), h.evaluate().unwrap().refined(1).unwrap());
        // Five different seam stations with degree2 incidence form one closed
        // component, not two open strips accidentally producing the same count.
        let ids: std::collections::BTreeSet<_> = after.aligned_loop_vertices(&plan.points).unwrap().into_iter().collect();
        assert_eq!(ids.len(), 5);
        let layout = after.layout().unwrap();
        let mut degree = std::collections::BTreeMap::<usize, usize>::new();
        for (edge, _) in &layout.paths {
            if edge.iter().all(|id| ids.contains(id)) {
                for id in edge {
                    *degree.entry(*id).or_default() += 1;
                }
            }
        }
        assert_eq!(degree.len(), 5);
        assert!(degree.values().all(|&degree| degree == 2));
    }
    #[test]
    fn hierarchical_aligned_deleted_region_separates_components_without_balancing_other_roots() {
        let cage =
            primitives::Parameters { kind: "face".into(), dimensions: [16.0, 4.0, 1.0], segments: [4, 1, 1], ..Default::default() }.build().unwrap();
        let h = hierarchy::Hierarchy::new(cage).unwrap().subdivide(&[3], [2, 2]).unwrap().delete_faces(&[1]).unwrap();
        let c = h.leaf_corners().unwrap()[0].unwrap();
        let seed = [c[3], c[0]];
        let plan = h.aligned_loop(seed, 0.5).unwrap();
        assert_eq!(plan.cuts, vec![hierarchy::LeafCut { parent: 0, axis: 1, coordinate: 8 }]);
        let after = h.split_at(&plan.cuts).unwrap();
        assert_eq!(&after.leaves[1..h.leaves.len()], &h.leaves[1..]);
        assert_eq!(after.evaluate().unwrap(), h.evaluate().unwrap());
        let right: Vec<_> = after.leaves.iter().filter(|leaf| leaf.root >= 2).collect();
        let old: Vec<_> = h.leaves.iter().filter(|leaf| leaf.root >= 2).collect();
        assert_eq!(right, old);
        assert!(h.aligned_loop(seed, 1.0 / 32.0).unwrap_err().to_string().contains("aligned dyadic"));
        assert!(
            h.split_at(&[hierarchy::LeafCut { parent: 0, axis: 1, coordinate: 8 }, hierarchy::LeafCut { parent: 0, axis: 1, coordinate: 4 }])
                .is_err()
        );
    }

    #[test]
    fn invalid_cages_and_budgets_are_refused() {
        let c = primitive("box", 40.0, 2).unwrap();
        let mut broken = c.clone();
        broken.faces[0][0] = usize::MAX;
        assert!(broken.validate().is_err());
        let mut broken = c.clone();
        broken.faces.push(c.faces[0].clone());
        assert!(broken.validate().is_err());
        let mut broken = c.clone();
        broken.faces[0].reverse();
        assert!(broken.validate().is_err());
        let mut broken = c.clone();
        broken.points[0].x = f64::NAN;
        assert!(broken.validate().is_err());
        assert!(c.refined(5).is_err());
        assert_eq!(c, primitive("box", 40.0, 2).unwrap());
        let mut bow = primitive("plane", 2.0, 1).unwrap();
        bow.points.extend([Vec3::new(2., 0., 0.), Vec3::new(2., 2., 0.), Vec3::new(0., 2., 0.)]);
        bow.faces.push(vec![0, 4, 5, 6]);
        assert!(bow.validate().is_err());
    }
}
#[test]
fn hierarchical_delta_batch_is_atomic_on_invalid_controls_and_overflow() {
    use hierarchy::Hierarchy;
    let mut hierarchy = Hierarchy::new(primitive("box", 10.0, 1).unwrap()).unwrap();
    let before = hierarchy.clone();
    assert!(hierarchy.add_deltas(&[0, usize::MAX], &[Vec3::Z, Vec3::X]).is_err());
    assert_eq!(hierarchy, before);
    assert!(hierarchy.add_deltas(&[0, 1], &[Vec3::Z, Vec3::new(f64::NAN, 0.0, 0.0)]).is_err());
    assert_eq!(hierarchy, before);
    let large = Vec3::new(f64::MAX, 0.0, 0.0);
    assert!(hierarchy.add_deltas(&[0, 0], &[large, large]).is_err());
    assert_eq!(hierarchy, before);
    hierarchy.add_deltas(&[0, 0], &[Vec3::X, Vec3::Y]).unwrap();
    assert_eq!(hierarchy.evaluate().unwrap().points[0], before.base.points[0] + Vec3::X + Vec3::Y);
}
