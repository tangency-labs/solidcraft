//! Bounded Catmull-Clark mesh previews for the local Forms experiment.
//! The control cage stays authoritative; subdivision creates a new quad mesh.
use std::collections::{BTreeMap, BTreeSet};

use crate::{Mesh, Vec3};
use serde::{Deserialize, Serialize};
pub mod edit;
pub mod fitting;
pub mod hierarchy;
pub mod matching;
pub mod pipe_network;
pub mod primitives;
pub mod projection;
pub mod symmetry;
pub mod topology_tools;

pub const MAX_FACES: usize = 30_000;
pub const MAX_POINTS: usize = 40_000;
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
