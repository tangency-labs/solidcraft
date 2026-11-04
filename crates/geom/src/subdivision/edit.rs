//! Transactional topology edits. Invalid, nonmanifold results never replace the source cage.
use super::{Cage, Result, bad, key};
use crate::prelude::*;
use solidcore_record::Vec3;
use std::collections::{BTreeMap, BTreeSet};

impl Cage {
    pub(super) fn selected_faces(&self, selected: &[usize]) -> Result<BTreeSet<usize>> {
        let set: BTreeSet<_> = selected.iter().copied().collect();
        if set.is_empty() || set.len() != selected.len() || set.iter().any(|&i| i >= self.faces.len()) {
            return Err(bad("select distinct existing faces"));
        }
        Ok(set)
    }
    pub(super) fn finish_edit(self) -> Result<Self> {
        self.finish_edit_with_map().map(|(cage, _)| cage)
    }
    pub(super) fn finish_edit_with_map(mut self) -> Result<(Self, BTreeMap<usize, usize>)> {
        let used: BTreeSet<_> = self.faces.iter().flatten().copied().collect();
        let mut remap = BTreeMap::new();
        let mut points = Vec::with_capacity(used.len());
        for old in used {
            remap.insert(old, points.len());
            points.push(self.point(old)?);
        }
        for face in &mut self.faces {
            for id in face {
                *id = *remap.get(id).ok_or_else(|| bad("missing edited vertex"))?;
            }
        }
        let edges: BTreeSet<_> =
            self.faces.iter().flat_map(|f| f.iter().copied().zip(f.iter().copied().cycle().skip(1)).take(f.len()).map(|(a, b)| key(a, b))).collect();
        self.edge_creases.retain(|c| {
            remap.contains_key(&c.edge[0]) && remap.contains_key(&c.edge[1]) && edges.contains(&key(remap[&c.edge[0]], remap[&c.edge[1]]))
        });
        for c in &mut self.edge_creases {
            c.edge = c.edge.map(|v| remap[&v]);
        }
        self.vertex_creases.retain(|c| remap.contains_key(&c.vertex));
        for c in &mut self.vertex_creases {
            c.vertex = remap[&c.vertex];
        }
        self.points = points;
        self.validate()?;
        Ok((self, remap))
    }
    pub fn delete_faces(&self, selected: &[usize]) -> Result<Self> {
        let selected = self.selected_faces(selected)?;
        let mut out = self.clone();
        out.faces = self.faces.iter().enumerate().filter(|(i, _)| !selected.contains(i)).map(|(_, f)| f.clone()).collect();
        out.finish_edit()
    }
    /// Extrude a connected or disconnected face region, retaining its internal connectivity.
    pub fn extrude_faces(&self, selected: &[usize], delta: Vec3) -> Result<Self> {
        self.validate()?;
        let selected = self.selected_faces(selected)?;
        if !delta.is_finite() || delta.len() < 1e-9 {
            return Err(bad("extrusion needs a finite nonzero displacement"));
        }
        let mut out = self.clone();
        let mut duplicates = BTreeMap::new();
        let mut boundary = BTreeMap::new();
        for &fi in &selected {
            let face = &self.faces[fi];
            for &v in face {
                if let std::collections::btree_map::Entry::Vacant(entry) = duplicates.entry(v) {
                    entry.insert(out.points.len());
                    out.points.push(self.point(v)? + delta);
                }
            }
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                boundary.entry(key(a, b)).and_modify(|v: &mut Option<(usize, usize)>| *v = None).or_insert(Some((a, b)));
            }
        }
        for &fi in &selected {
            out.faces[fi] = self.faces[fi].iter().map(|v| duplicates[v]).collect();
        }
        for (a, b) in boundary.values().flatten().copied() {
            out.faces.push(vec![a, b, duplicates[&b], duplicates[&a]]);
        }
        out.finish_edit()
    }
    pub fn extrude_edges(&self, selected: &[[usize; 2]], delta: Vec3) -> Result<Self> {
        let topology = self.topology()?;
        if selected.is_empty() || selected.len() > self.points.len() || !delta.is_finite() || delta.len() < 1e-9 {
            return Err(bad("select boundary edges and a nonzero extrusion"));
        }
        let mut out = self.clone();
        let mut duplicates = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for &[a, b] in selected {
            let ei = *topology.lookup.get(&key(a, b)).ok_or_else(|| bad("invalid edge"))?;
            if !seen.insert(ei) {
                return Err(bad("duplicate edge"));
            }
            let edge = &topology.edges[ei];
            if edge.faces.len() != 1 {
                return Err(bad("edge extrusion requires a boundary"));
            }
            for v in [a, b] {
                if let std::collections::btree_map::Entry::Vacant(entry) = duplicates.entry(v) {
                    entry.insert(out.points.len());
                    out.points.push(self.point(v)? + delta);
                }
            }
            let face = &self.faces[edge.faces[0]];
            let (a, b) = face
                .iter()
                .copied()
                .zip(face.iter().copied().cycle().skip(1))
                .take(face.len())
                .find(|&(x, y)| key(x, y) == key(a, b))
                .ok_or_else(|| bad("invalid boundary"))?;
            out.faces.push(vec![b, a, duplicates[&a], duplicates[&b]]);
        }
        out.finish_edit()
    }
    /// Boundary loops follow the source face winding. The first vertex is not repeated.
    pub fn boundary_loops(&self) -> Result<Vec<Vec<usize>>> {
        self.boundary_loops_where(|_| true)
    }
    pub(super) fn boundary_loops_where(&self, active: impl Fn(usize) -> bool) -> Result<Vec<Vec<usize>>> {
        let t = self.topology()?;
        let mut next = BTreeMap::new();
        for e in &t.edges {
            let mut adjacent = e.faces.iter().copied().filter(|&face| active(face));
            let Some(face) = adjacent.next().filter(|_| adjacent.next().is_none()) else { continue };
            let face = self.faces.get(face).ok_or_else(|| bad("invalid boundary face"))?;
            let (a, b) = face
                .iter()
                .copied()
                .zip(face.iter().copied().cycle().skip(1))
                .take(face.len())
                .find(|&(a, b)| key(a, b) == key(e.a, e.b))
                .ok_or_else(|| bad("invalid boundary"))?;
            if next.insert(a, b).is_some() {
                return Err(bad("branching boundary"));
            }
        }
        let mut loops = Vec::new();
        while let Some((&start, _)) = next.first_key_value() {
            let mut path = vec![start];
            let mut at = start;
            loop {
                let to = next.remove(&at).ok_or_else(|| bad("open boundary chain"))?;
                if to == start {
                    break;
                }
                if path.len() > self.points.len() {
                    return Err(bad("cyclic boundary chain"));
                }
                path.push(to);
                at = to;
            }
            loops.push(path);
        }
        Ok(loops)
    }
    pub fn fill_hole(&self, vertex: usize) -> Result<Self> {
        let ring = self.boundary_loops()?.into_iter().find(|l| l.contains(&vertex)).ok_or_else(|| bad("pick a hole boundary"))?;
        let a = *ring.first().ok_or_else(|| bad("empty boundary"))?;
        let b = *ring.get(1).ok_or_else(|| bad("incomplete boundary"))?;
        self.fill_boundaries(&[[a, b]], super::topology_tools::FillMode::Automatic)
    }
    pub fn bridge(&self, vertices: [usize; 2]) -> Result<Self> {
        self.bridge_transition(vertices, false)
    }
    pub fn dissolve_edge(&self, edge: [usize; 2]) -> Result<Self> {
        let t = self.topology()?;
        let e = &t.edges[*t.lookup.get(&key(edge[0], edge[1])).ok_or_else(|| bad("invalid edge"))?];
        if e.faces.len() != 2 {
            return Err(bad("dissolve requires an interior edge"));
        }
        let mut next = BTreeMap::new();
        for &fi in &e.faces {
            let f = &self.faces[fi];
            for (&a, &b) in f.iter().zip(f.iter().cycle().skip(1)).take(f.len()) {
                if key(a, b) != key(e.a, e.b) && next.insert(a, b).is_some() {
                    return Err(bad("faces share more than the dissolved edge"));
                }
            }
        }
        let start = *next.first_key_value().ok_or_else(|| bad("empty dissolved boundary"))?.0;
        let mut ring = vec![start];
        let mut at = start;
        loop {
            let to = next.remove(&at).ok_or_else(|| bad("invalid dissolved boundary"))?;
            if to == start {
                break;
            }
            ring.push(to);
            at = to;
            if ring.len() > 32 {
                return Err(bad("dissolved face exceeds 32 vertices"));
            }
        }
        if !next.is_empty() {
            return Err(bad("dissolved boundary has multiple loops"));
        }
        let mut out = self.clone();
        out.faces = self.faces.iter().enumerate().filter(|(i, _)| !e.faces.contains(i)).map(|(_, f)| f.clone()).collect();
        out.faces.push(ring);
        out.finish_edit()
    }
    pub fn weld(&self, selected: &[usize]) -> Result<Self> {
        let set: BTreeSet<_> = selected.iter().copied().collect();
        if set.len() < 2 || set.len() != selected.len() {
            return Err(bad("weld requires at least two distinct vertices"));
        }
        let first = *set.first().ok_or_else(|| bad("empty weld"))?;
        let mut center = Vec3::ZERO;
        for &v in &set {
            center += self.point(v)?;
        }
        center = center / set.len() as f64;
        let mut out = self.clone();
        out.points[first] = center;
        for f in &mut out.faces {
            for v in f.iter_mut() {
                if set.contains(v) {
                    *v = first;
                }
            }
            f.dedup();
            if f.first() == f.last() {
                f.pop();
            }
        }
        out.faces.retain(|f| f.len() >= 3);
        out.finish_edit()
    }
    /// Detach a face region into a separate connected component, keeping its shape.
    pub fn unweld(&self, selected: &[usize]) -> Result<Self> {
        let selected = self.selected_faces(selected)?;
        let mut out = self.clone();
        let mut duplicates = BTreeMap::new();
        for fi in selected {
            let mut face = self.faces[fi].clone();
            for v in &mut face {
                let old = *v;
                *v = *duplicates.entry(old).or_insert_with(|| {
                    let id = out.points.len();
                    out.points.push(self.points[old]);
                    id
                });
            }
            out.faces[fi] = face;
        }
        out.finish_edit()
    }
    pub fn thicken(&self, distance: f64, sharp: bool) -> Result<Self> {
        self.validate()?;
        if self.faces.iter().any(|f| f.len() != 4) {
            return Err(bad("thicken requires a quad control cage"));
        }
        if !distance.is_finite() || distance.abs() < 1e-6 || distance.abs() > 10_000.0 {
            return Err(bad("thickness must be finite, nonzero and within 10000 mm"));
        }
        let loops = self.boundary_loops()?;
        if loops.is_empty() {
            return Err(bad("thicken requires an open sheet"));
        }
        let walls: usize = loops.iter().map(Vec::len).sum();
        if self.points.len().saturating_mul(2) > super::MAX_POINTS || self.faces.len().saturating_mul(2) + walls > super::MAX_FACES {
            return Err(bad("thickened cage exceeds the editing budget"));
        }
        let mut normals = vec![Vec3::ZERO; self.points.len()];
        for face in &self.faces {
            let origin = self.point(face[0])?;
            let mut n = Vec3::ZERO;
            for i in 1..face.len() - 1 {
                n += (self.point(face[i])? - origin).cross(self.point(face[i + 1])? - origin);
            }
            for &v in face {
                normals[v] += n;
            }
        }
        let mut out = self.clone();
        let n = self.points.len();
        out.vertex_creases.extend(self.vertex_creases.iter().map(|c| super::VertexCrease { vertex: c.vertex + n, sharpness: c.sharpness }));
        out.edge_creases.extend(self.edge_creases.iter().map(|c| super::EdgeCrease { edge: c.edge.map(|v| v + n), sharpness: c.sharpness }));
        for (&p, normal) in self.points.iter().zip(normals) {
            out.points.push(p + normal.normalized().ok_or_else(|| bad("undefined vertex normal"))? * distance);
        }
        // For positive thickness the source is the lower side.
        if distance > 0.0 {
            for f in &mut out.faces {
                f.reverse();
            }
        }
        for f in &self.faces {
            let mut f: Vec<_> = f.iter().map(|v| v + n).collect();
            if distance < 0.0 {
                f.reverse();
            }
            out.faces.push(f);
        }
        let mut rims = BTreeSet::new();
        for ring in loops {
            for (&a, &b) in ring.iter().zip(ring.iter().cycle().skip(1)).take(ring.len()) {
                let mut f = vec![a, b, b + n, a + n];
                if distance < 0.0 {
                    f.reverse();
                }
                out.faces.push(f);
                rims.insert(key(a, b));
                rims.insert(key(a + n, b + n));
            }
        }
        if sharp {
            out.edge_creases.retain(|c| !rims.contains(&key(c.edge[0], c.edge[1])));
            out.edge_creases.extend(rims.into_iter().map(|(a, b)| super::EdgeCrease { edge: [a, b], sharpness: 10.0 }));
        }
        out.finish_edit()
    }
}

#[cfg(test)]
mod tests {
    use super::super::primitives::Parameters;
    #[test]
    fn hierarchical_face_extrusion_replays_after_serialization_and_refinement() {
        use super::super::hierarchy::Hierarchy;
        use solidcore_record::Vec3;

        let plane = Parameters { kind: "face".into(), dimensions: [20.0, 20.0, 1.0], segments: [4, 4, 1], ..Default::default() }.build().unwrap();
        let roots = plane.faces.len();
        let source = Hierarchy::new(plane.clone()).unwrap().subdivide(&[5], [2, 2]).unwrap();
        let old_layout = source.layout().unwrap();
        let old_cage = source.evaluate().unwrap();
        let selected = roots;
        let displacement = Vec3::Z * 3.0;
        let mut hierarchy = source.extrude_faces(&[selected], displacement).unwrap();
        let layout = hierarchy.layout().unwrap();
        let cage = hierarchy.evaluate().unwrap();
        assert_eq!(hierarchy.base, plane);
        assert_eq!(hierarchy.depth, source.depth);
        assert_eq!(cage.faces.len(), old_cage.faces.len() + 4);
        assert_eq!(hierarchy.leaves.len(), source.leaves.len() + 4);
        for (id, owner) in old_layout.owners.iter().enumerate() {
            assert_eq!(&layout.owners[id], owner);
        }
        assert_eq!(layout.faces[15], old_layout.faces[15]);
        let cap = layout.faces[selected][0];
        assert!(cap >= old_cage.points.len());
        assert_eq!(cage.points[cap], old_cage.points[old_layout.faces[selected][0]] + displacement);

        let mut moved = hierarchy.clone();
        let base = old_layout.faces[15][0];
        let delta = Vec3::X * 0.2;
        let influence = hierarchy.basis(base).unwrap();
        moved.add_deltas(&[base], &[delta]).unwrap();
        let changed = moved.evaluate().unwrap();
        for (i, p) in cage.points.iter().enumerate() {
            assert!(changed.points[i].dist(*p + delta * influence.points[i].x) < 1e-10);
        }
        let influence = hierarchy.basis(cap).unwrap();
        hierarchy.add_deltas(&[cap], &[delta]).unwrap();
        let changed = hierarchy.evaluate().unwrap();
        for (i, p) in cage.points.iter().enumerate() {
            assert!(changed.points[i].dist(*p + delta * influence.points[i].x) < 1e-10);
        }

        let other = selected + 3;
        let before_other = hierarchy.layout().unwrap();
        let mut hierarchy = hierarchy.extrude_faces(&[other], displacement).unwrap();
        let latest = hierarchy.layout().unwrap();
        let other_cap = latest.faces[other][0];
        assert_ne!(other_cap, before_other.faces[other][0]);
        let influence = hierarchy.basis(other_cap).unwrap();
        let before = hierarchy.evaluate().unwrap();
        hierarchy.add_deltas(&[other_cap], &[delta]).unwrap();
        let after = hierarchy.evaluate().unwrap();
        for (i, p) in before.points.iter().enumerate() {
            assert!(after.points[i].dist(*p + delta * influence.points[i].x) < 1e-10);
        }

        let wall = source.leaves.len();
        let refined = hierarchy.subdivide(&[wall], [2, 1]).unwrap();
        assert_eq!(refined.depth, source.depth + 1);
        assert!(!refined.leaves[wall].active);
        assert_eq!(refined.evaluate().unwrap(), after.refined(1).unwrap());
        assert!(refined.layout().is_ok());
        let mut marked = hierarchy.with_evaluator(&after.with_creases(&[], &[other_cap], 10.0).unwrap()).unwrap().subdivide(&[wall], [2, 1]).unwrap();
        assert_ne!(marked.evaluate().unwrap(), refined.evaluate().unwrap());
        marked
            .restore_level_sharpness(
                &super::super::hierarchy::LevelSharpness {
                    level: hierarchy.depth,
                    edges: Vec::new(),
                    vertices: vec![super::super::VertexCrease { vertex: other_cap, sharpness: 0.0 }],
                },
                10.0,
            )
            .unwrap();
        assert_eq!(marked.evaluate().unwrap(), refined.evaluate().unwrap());
        let saved = serde_json::to_string(&refined).unwrap();
        let reopened: Hierarchy = serde_json::from_str(&saved).unwrap();
        assert_eq!(reopened, refined);
        assert_eq!(reopened.evaluate().unwrap(), refined.evaluate().unwrap());
    }

    #[test]
    fn hierarchical_free_edge_extrusion_owns_each_wall_and_rejects_hidden_stencil_edges() {
        use super::super::{fitting::RootBasis, hierarchy::Hierarchy, key};
        use crate::tess::EvalSurface;
        use solidcore_record::Vec3;

        let plane = Parameters { kind: "face".into(), dimensions: [20.0, 20.0, 1.0], segments: [4, 4, 1], ..Default::default() }.build().unwrap();
        let hierarchy = Hierarchy::new(plane).unwrap().subdivide(&[5], [2, 2]).unwrap();
        let source = hierarchy.evaluate().unwrap();
        let old_layout = hierarchy.layout().unwrap();
        let ring = source.boundary_loops().unwrap().remove(0);
        let free: std::collections::BTreeSet<_> =
            ring.iter().copied().zip(ring.iter().copied().cycle().skip(1)).take(ring.len()).map(|(a, b)| key(a, b)).collect();
        let (edge, path) = old_layout
            .paths
            .iter()
            .find(|(_, path)| {
                path.len() == 3 && path.iter().all(|&v| source.points[v].x < -8.0) && path.windows(2).all(|p| free.contains(&key(p[0], p[1])))
            })
            .unwrap();
        let delta = Vec3::Z * 3.0;
        let (mut extruded, front) = hierarchy.extrude_edges(&[*edge], delta).unwrap();
        let result = extruded.evaluate().unwrap();
        let layout = extruded.layout().unwrap();
        assert_eq!(front.len(), 2);
        assert_eq!(result.points.len(), source.points.len() + 3);
        assert_eq!(&result.points[..source.points.len()], source.points);
        assert_eq!(&result.faces[..source.faces.len()], source.faces);
        assert_eq!(&layout.owners[..source.faces.len()], old_layout.owners);
        assert_eq!(&layout.owners[source.faces.len()..], &[Some(hierarchy.leaves.len()), Some(hierarchy.leaves.len() + 1)]);
        for pair in front {
            let wall = layout.paths.iter().find(|(edge, _)| *edge == pair).unwrap();
            assert_eq!(wall.1, pair);
            for cap in pair {
                assert!(path.iter().any(|&old| result.points[cap] == source.points[old] + delta));
            }
        }

        let far = source.faces.iter().position(|f| f.iter().all(|&v| source.points[v].x > 7.0)).unwrap();
        let basis = RootBasis { level: 2, indices: (0..=4).collect() };
        let before = EvalSurface::from(&basis.fit(&source).unwrap()[far]);
        let after = EvalSurface::from(&basis.fit(&result).unwrap()[far]);
        for u in [0.0, 0.5, 1.0] {
            for v in [0.0, 0.5, 1.0] {
                let a = before.jet(u, v).unwrap();
                let b = after.jet(u, v).unwrap();
                for (p, q) in [a.point, a.du, a.dv, a.duu, a.duv, a.dvv].into_iter().zip([b.point, b.du, b.dv, b.duu, b.duv, b.dvv]) {
                    assert!(p.dist(q) < 1e-10);
                }
            }
        }

        let wall = hierarchy.leaves.len();
        let control = source.points.len();
        let influence = extruded.basis(control).unwrap();
        let detail = Vec3::Y * 0.2;
        extruded.add_deltas(&[control], &[detail]).unwrap();
        let after = extruded.evaluate().unwrap();
        for (i, p) in result.points.iter().enumerate() {
            assert!(after.points[i].dist(*p + detail * influence.points[i].x) < 1e-10);
        }
        let extruded = extruded.subdivide(&[wall], [2, 1]).unwrap();
        assert_eq!(extruded.evaluate().unwrap(), after.refined(1).unwrap());
        let saved = serde_json::to_string(&extruded).unwrap();
        let reopened: Hierarchy = serde_json::from_str(&saved).unwrap();
        assert_eq!(reopened, extruded);
        assert!(reopened.layout().is_ok());

        let trimmed = hierarchy.delete_faces(&[0]).unwrap();
        let boundary = trimmed.layout().unwrap();
        let edge = boundary
            .paths
            .iter()
            .find(|(edge, path)| {
                path.windows(2).all(|p| !free.contains(&key(p[0], p[1])))
                    && boundary
                        .faces
                        .iter()
                        .filter(|f| {
                            f.iter().copied().zip(f.iter().copied().cycle().skip(1)).take(f.len()).any(|(a, b)| key(a, b) == key(edge[0], edge[1]))
                        })
                        .count()
                        == 1
            })
            .unwrap()
            .0;
        assert!(trimmed.extrude_edges(&[edge], delta).unwrap_err().0.contains("true free evaluator edge"));
    }

    #[test]
    fn hierarchical_face_extrusion_reflects_separate_regions_and_keeps_joined_caps_on_the_plane() {
        use super::super::{hierarchy::Hierarchy, symmetry::Symmetry};
        use solidcore_record::Vec3;

        let plane = Parameters { kind: "face".into(), dimensions: [16.0, 12.0, 1.0], segments: [4, 3, 1], ..Default::default() }.build().unwrap();
        let base_symmetry = Symmetry::new(&plane, 0).unwrap();
        let seed = plane.faces.iter().position(|f| f.iter().all(|&v| plane.points[v].x >= 4.0)).unwrap();
        let roots = base_symmetry.mirror_face_selection(&plane.faces, &[seed]).unwrap();
        let root_count = plane.faces.len();
        let h = Hierarchy::new(plane).unwrap().subdivide(&roots, [2, 2]).unwrap();
        let cage = h.evaluate().unwrap();
        let symmetry = Symmetry::new(&cage, 0).unwrap();
        let old = h.layout().unwrap();
        let positive =
            old.faces.iter().enumerate().skip(root_count).position(|(_, f)| f.iter().all(|&v| cage.points[v].x > 3.5)).unwrap() + root_count;
        let pair = symmetry.mirror_face_selection(&old.faces, &[positive]).unwrap();
        assert_eq!(pair.len(), 2);
        let negative = *pair.iter().find(|&&id| id != positive).unwrap();
        let delta = Vec3::new(1.5, 0.0, 3.0);
        let edited = h.extrude_symmetric_faces(&[positive], delta, &symmetry).unwrap();
        let after = edited.evaluate().unwrap();
        let layout = edited.layout().unwrap();
        assert_eq!(edited.base, h.base);
        assert_eq!(after.faces.len(), cage.faces.len() + 8);
        assert_eq!(edited.leaves.len(), h.leaves.len() + 8);
        assert_eq!(&layout.owners[..old.owners.len()], old.owners);
        let mirrored = Symmetry::new(&after, 0).unwrap();
        mirrored.validate_reflection(&after).unwrap();
        for (a, b) in [(positive, delta), (negative, symmetry.reflect_vector(delta))] {
            let cell = old.owners.iter().position(|owner| *owner == Some(a)).unwrap();
            for (&new, &prior) in after.faces[cell].iter().zip(&cage.faces[cell]) {
                assert_eq!(after.points[new], cage.points[prior] + b);
            }
        }
        assert_eq!(&mirrored.partners[..cage.points.len()], symmetry.partners);
        let saved = serde_json::to_string(&edited).unwrap();
        assert!(saved.contains("face_offsets"));
        let reopened: Hierarchy = serde_json::from_str(&saved).unwrap();
        assert_eq!(reopened.evaluate().unwrap(), after);
        assert_eq!(reopened.layout().unwrap().faces, layout.faces);

        let touching = old
            .faces
            .iter()
            .position(|f| {
                !f.is_empty()
                    && f.iter().any(|&v| symmetry.partners[v] == v)
                    && f.iter().any(|&v| cage.points[v].x > 0.0)
                    && f.iter().all(|&v| cage.points[v].x >= 0.0)
            })
            .unwrap();
        let pair = symmetry.mirror_face_selection(&old.faces, &[touching]).unwrap();
        assert_eq!(pair.len(), 2);
        let seam = h.extrude_symmetric_faces(&[touching], delta, &symmetry).unwrap();
        assert_eq!(seam, h.extrude_faces(&pair, Vec3::Z * 3.0).unwrap());
        let seam_cage = seam.evaluate().unwrap();
        Symmetry::new(&seam_cage, 0).unwrap().validate_reflection(&seam_cage).unwrap();
        assert!(h.extrude_symmetric_faces(&[touching], Vec3::X, &symmetry).unwrap_err().0.contains("along the plane"));

        let odd = Parameters { kind: "face".into(), segments: [3, 2, 1], ..Default::default() }.build().unwrap();
        let symmetry = Symmetry::new(&odd, 0).unwrap();
        let face = odd
            .faces
            .iter()
            .enumerate()
            .find_map(|(id, f)| (f.iter().any(|&v| odd.points[v].x < 0.0) && f.iter().any(|&v| odd.points[v].x > 0.0)).then_some(id))
            .unwrap();
        assert_eq!(symmetry.mirror_face_selection(&odd.faces, &[face]).unwrap(), vec![face]);
        let h = Hierarchy::new(odd).unwrap();
        let edited = h.extrude_symmetric_faces(&[face], delta, &symmetry).unwrap();
        assert_eq!(edited, h.extrude_faces(&[face], Vec3::Z * 3.0).unwrap());
    }

    #[test]
    fn hierarchical_mirrored_edge_extrusion_reflects_the_rim_and_keeps_seam_paths_joined() {
        use super::super::{hierarchy::Hierarchy, symmetry::Symmetry};
        use std::collections::BTreeSet;

        let plane = Parameters { kind: "face".into(), dimensions: [16.0, 12.0, 1.0], segments: [4, 3, 1], ..Default::default() }.build().unwrap();
        let base_symmetry = Symmetry::new(&plane, 0).unwrap();
        let seed = plane.faces.iter().position(|f| f.iter().all(|&v| plane.points[v].x >= 4.0)).unwrap();
        let roots = base_symmetry.mirror_face_selection(&plane.faces, &[seed]).unwrap();
        let hierarchy = Hierarchy::new(plane).unwrap().subdivide(&roots, [2, 2]).unwrap();
        let before = hierarchy.evaluate().unwrap();
        let symmetry = Symmetry::new(&before, 0).unwrap();
        let layout = hierarchy.layout().unwrap();
        let ring = before.boundary_loops().unwrap().remove(0);
        let rim: BTreeSet<_> = ring.iter().zip(ring.iter().cycle().skip(1)).map(|(&a, &b)| key(a, b)).collect();
        let is_free = |path: &[usize]| path.windows(2).all(|p| rim.contains(&key(p[0], p[1])));
        let (edge, path) = layout.paths.iter().find(|(_, path)| is_free(path) && path.iter().all(|&v| before.points[v].x >= 3.5)).unwrap();
        let delta = Vec3::new(1.5, 0.0, 3.0);
        let (extruded, front) = hierarchy.extrude_symmetric_edges(&[*edge], delta, &symmetry).unwrap();
        let after = extruded.evaluate().unwrap();
        assert_eq!(&after.points[..before.points.len()], before.points);
        assert_eq!(&after.faces[..before.faces.len()], before.faces);
        assert_eq!(front.len(), 2 * (path.len() - 1));
        assert_eq!(after.faces.len(), before.faces.len() + front.len());
        assert_eq!(after.points.len(), before.points.len() + 2 * path.len());
        let new_symmetry = Symmetry::new(&after, 0).unwrap();
        new_symmetry.validate_reflection(&after).unwrap();
        for &v in path {
            let cap = after.points.iter().position(|&p| p == before.points[v] + delta).unwrap();
            assert_eq!(after.points[new_symmetry.partners[cap]], before.points[symmetry.partners[v]] + symmetry.reflect_vector(delta));
        }
        let saved = serde_json::to_string(&extruded).unwrap();
        assert!(saved.contains("edge_offsets"));
        let restored: Hierarchy = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.evaluate().unwrap(), after);
        assert!(hierarchy.extrude_symmetric_edges(&[*edge], Vec3::new(1.0, 0.0, 1e6), &symmetry).is_err());

        let (seam_edge, _) = layout
            .paths
            .iter()
            .find(|(_, path)| is_free(path) && path.iter().any(|&v| symmetry.partners[v] == v) && path.iter().any(|&v| before.points[v].x > 0.0))
            .unwrap();
        let reflected = seam_edge.map(|v| symmetry.partners[v]);
        assert!(hierarchy.extrude_symmetric_edges(&[*seam_edge], Vec3::X, &symmetry).is_err());
        let joined = hierarchy.extrude_symmetric_edges(&[*seam_edge], delta, &symmetry).unwrap();
        assert_eq!(joined, hierarchy.extrude_edges(&[*seam_edge, reflected], Vec3::Z * 3.0).unwrap());
    }

    #[test]
    fn trimmed_ribbon_uses_the_exact_active_boundary_and_leaves_the_stencil_intact() {
        use super::super::{hierarchy::Hierarchy, key};
        use crate::tess::EvalSurface;
        use solidcore_record::Vec3;

        let plane = Parameters { kind: "face".into(), dimensions: [20.0, 20.0, 1.0], segments: [4, 4, 1], ..Default::default() }.build().unwrap();
        let deleted = plane.faces.len();
        let mut h = Hierarchy::new(plane).unwrap().subdivide(&[5], [2, 2]).unwrap();
        let layout = h.layout().unwrap();
        let deleted_face = &layout.faces[deleted];
        let cage = h.evaluate().unwrap();
        let inserted = *deleted_face.iter().find(|&&v| h.controls[&v] == 1 && cage.points[v].x.abs() > 1.0 && cage.points[v].y.abs() > 1.0).unwrap();
        h.add_deltas(&[inserted], &[Vec3::Z * 0.6]).unwrap();
        let h = h.delete_faces(&[deleted]).unwrap();
        let cage = h.evaluate().unwrap();
        let layout = h.layout().unwrap();
        let fit = h.fit_root_patches(0.05).unwrap();
        let snapshot = h.clone();
        let attachment = deleted_face
            .iter()
            .copied()
            .zip(deleted_face.iter().copied().cycle().skip(1))
            .take(deleted_face.len())
            .find_map(|(a, b)| {
                h.trim_attachments(&[[a, b]])
                    .ok()?
                    .into_iter()
                    .find(|trace| trace.root == 5 && (0..2).any(|i| trace.uv[0][i] == 0.5 && trace.uv[1][i] == 0.5))
            })
            .unwrap();
        let source = &fit.patches[attachment.root];
        let delta = Vec3::new(0.25, -0.2, 2.0);
        let wall = attachment.ruled_patch(source, delta).unwrap();
        let [[a, b], _] = wall.domain();
        let [begin, end] = attachment.uv;
        let skin = EvalSurface::from(source);
        let surface = EvalSurface::from(&wall);
        let vary = usize::from(begin[0] == end[0]);
        let head = source.eval(begin[0], begin[1]).unwrap();
        let tail = source.eval(end[0], end[1]).unwrap();
        let mid = wall.eval((a + b) * 0.5, 0.0).unwrap();
        assert!(mid.dist(head.lerp(tail, 0.5)) > 1e-3, "fixture must exercise a curved trim");
        for i in 0..=16 {
            let fraction = i as f64 / 16.0;
            let u = a + (b - a) * fraction;
            let uv = [end[0] + (begin[0] - end[0]) * fraction, end[1] + (begin[1] - end[1]) * fraction];
            let parent = skin.jet(uv[0], uv[1]).unwrap();
            for v in [0.0, 0.25, 0.5, 1.0] {
                let child = surface.jet(u, v).unwrap();
                assert!(child.point.dist(parent.point + delta * v) < 1e-10);
                assert!(child.dv.dist(delta) < 1e-10);
                let tangent = if vary == 0 { parent.du } else { parent.dv };
                let ratio = (begin[vary] - end[vary]) / (b - a);
                assert!(child.du.dist(tangent * ratio) < 1e-10);
            }
        }
        let visible = &layout.faces[attachment.owner];
        assert!(visible.iter().copied().zip(visible.iter().copied().cycle().skip(1)).take(visible.len()).any(|(a, b)| [a, b] == attachment.edge));
        assert_eq!(h.trim_attachments(&[[attachment.edge[1], attachment.edge[0]]]).unwrap(), vec![attachment.clone()]);
        assert!(h.trim_attachments(&[[attachment.edge[1], attachment.edge[0]], attachment.edge]).is_err());
        let true_free = cage.boundary_loops().unwrap().remove(0);
        let edge = layout
            .paths
            .iter()
            .find(|(_, p)| {
                p.windows(2).all(|pair| {
                    true_free
                        .iter()
                        .copied()
                        .zip(true_free.iter().copied().cycle().skip(1))
                        .take(true_free.len())
                        .any(|(a, b)| key(a, b) == key(pair[0], pair[1]))
                })
            })
            .unwrap()
            .0;
        assert!(h.trim_attachments(&[edge]).is_err());
        assert!(attachment.ruled_patch(source, Vec3::Z * 200000.0).is_err());
        assert_eq!(h, snapshot);
        assert_eq!(h.evaluate().unwrap(), cage);
        assert_eq!(h.fit_root_patches(0.05).unwrap().patches, fit.patches);
    }

    #[test]
    fn trimmed_ribbon_retains_rational_weights_through_subranges_and_reversal() {
        use super::super::hierarchy::TrimAttachment;
        use crate::bspline::NurbsSurface;
        use solidcore_record::Vec3;
        let row = [Vec3::X, Vec3::X + Vec3::Y, Vec3::Y];
        let points: Vec<_> = row.into_iter().chain(row.map(|p| p + Vec3::Z)).collect();
        let weights = [1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0].repeat(2);
        let skin = NurbsSurface::new([2, 1], [vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]], &points, &weights).unwrap();
        let attach = TrimAttachment { edge: [0, 1], owner: 0, root: 0, uv: [[0.25, 0.5], [0.75, 0.5]] };
        let wall = attach.ruled_patch(&skin, Vec3::Z * 2.0).unwrap();
        let count = wall.count()[0];
        let (base, translated) = wall.homogeneous().split_at(count);
        assert_eq!(base.iter().map(|p| p.w).collect::<Vec<_>>(), translated.iter().map(|p| p.w).collect::<Vec<_>>());
        assert!(base.iter().any(|p| p.w < 1.0));
        for t in [0.25, 0.35, 0.5, 0.65, 0.75] {
            let same = skin.eval(1.0 - t, 0.5).unwrap();
            assert!(wall.eval(t, 0.0).unwrap().dist(same) < 1e-12);
            assert!(wall.eval(t, 1.0).unwrap().dist(same + Vec3::Z * 2.0) < 1e-12);
        }
    }

    #[test]
    fn persisted_trim_wall_has_editable_preview_rim_without_evaluator_changes() {
        use super::super::hierarchy::Hierarchy;
        use solidcore_record::Vec3;

        let cage = Parameters { kind: "face".into(), dimensions: [20.0, 20.0, 1.0], segments: [4, 4, 1], ..Default::default() }.build().unwrap();
        let deleted = cage.faces.len();
        let source = Hierarchy::new(cage).unwrap().subdivide(&[5], [2, 2]).unwrap();
        let face = source.layout().unwrap().faces[deleted].clone();
        let h = source.delete_faces(&[deleted]).unwrap();
        let edge = face
            .iter()
            .copied()
            .zip(face.iter().copied().cycle().skip(1))
            .take(face.len())
            .find_map(|(a, b)| h.trim_attachments(&[[a, b]]).ok()?.into_iter().find(|t| t.root == 5).map(|t| t.edge))
            .unwrap();
        let cage = h.evaluate().unwrap();
        let (skin, logical) = h.surface(&cage, 1).unwrap();
        let owner = logical.faces.len();
        let front = [cage.points.len(), cage.points.len() + 1];
        let delta = Vec3::Z * 2.0;
        let (edited, rim) = h.extend_edges(&[edge], delta).unwrap();
        assert_eq!(rim, vec![front]);
        assert_eq!(edited.evaluate().unwrap(), cage);
        let (wall, logical_wall) = edited.surface(&cage, 1).unwrap();
        assert_eq!(&wall.mesh.positions[..skin.mesh.positions.len()], skin.mesh.positions);
        assert_eq!(&wall.mesh.triangles[..skin.mesh.triangles.len()], skin.mesh.triangles);
        assert!(wall.mesh.tri_face[skin.mesh.tri_face.len()..].iter().all(|id| *id == owner as u32));
        assert_eq!(logical_wall.faces[owner], vec![edge[1], edge[0], front[0], front[1]]);
        for i in 0..2 {
            assert_eq!(wall.vertices[front[i]], skin.vertices[edge[i]] + delta);
        }

        let changed = edited.move_trim_rim(Vec3::Z).unwrap();
        let (moved, _) = changed.surface(&cage, 2).unwrap();
        for &vertex in &front {
            assert!(moved.vertices[vertex].dist(wall.vertices[vertex] + Vec3::Z) < 1e-10);
        }
        assert_eq!(changed.evaluate().unwrap(), cage);
        assert!(changed.subdivide(&[5], [2, 2]).is_err());
        let symmetry = super::super::symmetry::Symmetry::new(&cage, 0).unwrap();
        assert!(changed.extrude_symmetric_edges(&[edge], delta, &symmetry).unwrap_err().0.contains("source topology"));
        assert_eq!(changed.delete_faces(&[owner]).unwrap(), h);
        let saved = serde_json::to_string(&changed).unwrap();
        let reopened: Hierarchy = serde_json::from_str(&saved).unwrap();
        assert_eq!(reopened, changed);
        assert_eq!(reopened.surface(&cage, 1).unwrap().1, changed.surface(&cage, 1).unwrap().1);
        let mut tampered = serde_json::to_value(&changed).unwrap();
        tampered["trim_wall"]["attachment"]["root"] = serde_json::json!(999);
        let tampered: Hierarchy = serde_json::from_value(tampered).unwrap();
        assert!(tampered.validated_trim_wall().is_err());
    }

    #[test]
    fn stable_extrusion_of_refined_cells_preserves_face_slots_and_remote_surface() {
        use super::super::{fitting::RootBasis, hierarchy::Hierarchy, stable_extrusion};
        use crate::tess::EvalSurface;
        use solidcore_record::Vec3;

        let plane = Parameters { kind: "face".into(), dimensions: [40.0, 40.0, 1.0], segments: [8, 8, 1], ..Default::default() }.build().unwrap();
        let root_count = plane.faces.len();
        let mut hierarchy = Hierarchy::new(plane).unwrap().subdivide(&[27], [2, 1]).unwrap();
        let (&detail, _) = hierarchy.controls.iter().find(|(_, level)| **level == 1).unwrap();
        hierarchy.add_deltas(&[detail], &[Vec3::Z * 0.4]).unwrap();
        let original = hierarchy.clone();
        let source = hierarchy.evaluate().unwrap();
        let selected: Vec<_> = hierarchy
            .layout()
            .unwrap()
            .owners
            .iter()
            .enumerate()
            .filter_map(|(face, owner)| owner.is_some_and(|id| id >= root_count).then_some(face))
            .collect();
        assert_eq!(selected.len(), 4);

        let delta = Vec3::Z * 5.0;
        let result = stable_extrusion::faces(&source, &selected, delta).unwrap();
        assert_eq!(result.walls.len(), 8);
        assert_eq!(result.cage.points.len(), source.points.len() + 8);
        assert_eq!(result.cage.faces.len(), source.faces.len() + 8);
        assert_eq!(result.cap.iter().filter(|(old, cap)| old == cap).count(), 1);
        for (id, face) in source.faces.iter().enumerate() {
            if selected.contains(&id) {
                assert_eq!(result.cage.faces[id], face.iter().map(|v| result.cap[v]).collect::<Vec<_>>());
            } else {
                assert_eq!(result.cage.faces[id], *face);
            }
        }
        for (&old, &cap) in &result.cap {
            assert_eq!(result.cage.points[cap], source.points[old] + delta);
            if old != cap {
                assert_eq!(result.cage.points[old], source.points[old]);
            }
        }
        for (index, wall) in result.walls.iter().enumerate() {
            assert_eq!(wall.face, source.faces.len() + index);
            assert_eq!(wall.cap, wall.base.map(|v| result.cap[&v]));
            assert_eq!(result.cage.faces[wall.face], vec![wall.base[0], wall.base[1], wall.cap[1], wall.cap[0]]);
        }
        assert_eq!(hierarchy, original);

        let far = source.faces.iter().position(|f| f.iter().all(|&v| source.points[v].x < -14.0 && source.points[v].y < -14.0)).unwrap();
        let basis = RootBasis { level: 2, indices: (0..=4).collect() };
        let before = EvalSurface::from(&basis.fit(&source).unwrap()[far]);
        let after = EvalSurface::from(&basis.fit(&result.cage).unwrap()[far]);
        for u in [0.0, 0.3, 0.5, 0.7, 1.0] {
            for v in [0.0, 0.3, 0.5, 0.7, 1.0] {
                let a = before.jet(u, v).unwrap();
                let b = after.jet(u, v).unwrap();
                for (x, y) in [a.point, a.du, a.dv, a.duu, a.duv, a.dvv].into_iter().zip([b.point, b.du, b.dv, b.duu, b.duv, b.dvv]) {
                    assert!(x.dist(y) < 1e-10);
                }
            }
        }
        assert!(stable_extrusion::faces(&source, &[selected[0], selected[0]], delta).is_err());
    }

    #[test]
    fn stable_extrusion_of_free_edges_preserves_source_points_and_creases() {
        use super::super::{fitting::RootBasis, stable_extrusion};
        use solidcore_record::Vec3;

        let source = Parameters { kind: "face".into(), segments: [2, 2, 1], ..Default::default() }.build().unwrap();
        let ring = source.boundary_loops().unwrap().remove(0);
        let edge = [ring[0], ring[1]];
        let source = source.with_creases(&[edge], &[edge[0]], 4.0).unwrap();
        let delta = Vec3::Z * 5.0;
        let result = stable_extrusion::boundary(&source, &[edge], delta).unwrap();
        assert_eq!(&result.cage.faces[..source.faces.len()], source.faces);
        assert_eq!(&result.cage.points[..source.points.len()], source.points);
        assert_eq!(result.walls.len(), 1);
        let wall = result.walls[0];
        assert_eq!(wall.face, source.faces.len());
        assert_eq!(wall.base, edge);
        assert_eq!(wall.cap, edge.map(|v| result.cap[&v]));
        for e in [wall.base, wall.cap] {
            assert!(
                result
                    .cage
                    .edge_creases
                    .iter()
                    .any(|c| super::super::key(c.edge[0], c.edge[1]) == super::super::key(e[0], e[1]) && c.sharpness == 4.0)
            );
        }
        for v in [wall.base[0], wall.cap[0]] {
            assert!(result.cage.vertex_creases.iter().any(|c| c.vertex == v && c.sharpness == 4.0));
        }
        assert!(RootBasis { level: 2, indices: (0..=4).collect() }.fit(&result.cage).is_ok());
        assert!(stable_extrusion::boundary(&source, &[[usize::MAX, 0]], delta).is_err());
    }
    #[test]
    fn matched_openings_are_checked_between_constraints_and_keep_editable_topology() {
        use super::super::matching::{Config, Target, Tolerances, match_opening, quadratic_extension};
        use crate::tess::EvalSurface;
        for order in 0..=2 {
            let mut cage = if order == 2 {
                let box_cage = Parameters { dimensions: [2.0; 3], segments: [3; 3], ..Default::default() }.build().unwrap();
                let top: Vec<_> = box_cage
                    .faces
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| f.iter().all(|&i| (box_cage.points[i].z - 1.0).abs() < 1e-10))
                    .map(|(i, _)| i)
                    .collect();
                box_cage.delete_faces(&top).unwrap()
            } else {
                Parameters { kind: "face".into(), dimensions: [2.0; 3], segments: [2; 3], ..Default::default() }.build().unwrap()
            };
            let ring = cage.boundary_loops().unwrap().remove(0);
            let corners: Vec<_> = ring
                .iter()
                .copied()
                .filter(|&v| (cage.points[v].x.abs() - 1.0).abs() < 1e-10 && (cage.points[v].y.abs() - 1.0).abs() < 1e-10)
                .collect();
            cage = cage.with_creases(&[], &corners, 10.0).unwrap();
            let source = cage.clone();
            let z = 2.0;
            let plane = EvalSurface::Plane { origin: solidcore_record::Vec3::Z * z, du: solidcore_record::Vec3::X, dv: solidcore_record::Vec3::Y };
            let target = |fraction: f64| {
                let at = fraction * ring.len() as f64;
                let i = (at.floor() as usize).min(ring.len() - 1);
                let (a, b) = (source.points[ring[i]], source.points[ring[(i + 1) % ring.len()]]);
                let mut point = a.lerp(b, at - i as f64);
                point.z = z;
                Ok(Target { corner: false, point, tangent: b - a, normal: solidcore_record::Vec3::Z, support: plane.jet(point.x, point.y).unwrap() })
            };
            let config = Config {
                edge: [ring[0], ring[1]],
                coordinates: (0..ring.len()).map(|i| i as f64 / ring.len() as f64).collect(),
                reverse: false,
                order,
                tolerances: Tolerances::default(),
            };
            let result = match_opening(&cage, &config, target, 0.01).unwrap_or_else(|e| panic!("G{order}: {e}"));
            assert_eq!(result.cage.faces, source.faces);
            assert_eq!(result.cage.points.len(), source.points.len());
            assert!(result.report.position_mm <= 0.001);
            assert!(order == 0 || result.report.tangent_deg <= 0.1);
            assert!(order < 2 || result.report.curvature_relative <= 0.01);
            assert_eq!(cage, source);
            if order == 2 {
                let eval = quadratic_extension(&result.cage).unwrap();
                assert_eq!(eval, result.cage, "Pinned corners retain the original open-boundary rules");
            }
            let mut invalid = config.clone();
            invalid.coordinates.clear();
            assert!(
                match_opening(&cage, &invalid, |_| Err(super::super::bad("invalid configuration must be rejected before querying support")), 0.01)
                    .is_err()
            );
        }
    }
    #[test]
    fn smooth_g2_match_uses_a_noneditable_quadratic_ring_and_checks_the_fitted_surface_jets() {
        use super::super::matching::{Config, Target, Tolerances, match_opening, quadratic_extension};
        use crate::tess::EvalSurface;
        use solidcore_record::Vec3;
        let source = Parameters { kind: "cylinder".into(), dimensions: [2.0; 3], segments: [4, 3, 1], ..Default::default() }.build().unwrap();
        let top: Vec<_> =
            source.faces.iter().enumerate().filter(|(_, f)| f.iter().all(|&i| (source.points[i].z - 1.0).abs() < 1e-10)).map(|(i, _)| i).collect();
        let cage = source.delete_faces(&top).unwrap();
        let ring = cage.boundary_loops().unwrap().remove(0);
        assert_eq!(ring.len(), 32);
        let tau = std::f64::consts::TAU;
        let coordinates: Vec<_> = ring.iter().map(|&i| cage.points[i].y.atan2(cage.points[i].x).rem_euclid(tau) / tau).collect();
        let area = ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .take(ring.len())
            .map(|(&a, &b)| cage.points[a].x * cage.points[b].y - cage.points[a].y * cage.points[b].x)
            .sum::<f64>();
        let config = Config { edge: [ring[0], ring[1]], coordinates, reverse: area < 0.0, order: 2, tolerances: Tolerances::default() };
        for curved in [false, true] {
            let support = if curved {
                let mut ctrl = Vec::new();
                for j in 0..3 {
                    for i in 0..3 {
                        let z = 1.6 + if i == 2 { 3.2 } else { 0.0 } + if j == 2 { 3.2 } else { 0.0 } - 1.6 * (i + j) as f64;
                        ctrl.push([2.0 * i as f64 - 2.0, 2.0 * j as f64 - 2.0, z, 1.0]);
                    }
                }
                EvalSurface::Nurbs {
                    degree: [2, 2],
                    knots: [vec![0., 0., 0., 1., 1., 1.], vec![0., 0., 0., 1., 1., 1.]],
                    count: [3, 3],
                    ctrl,
                    closed: [false, false],
                }
            } else {
                EvalSurface::Plane { origin: Vec3::Z * 2.0, du: Vec3::X, dv: Vec3::Y }
            };
            let target = |fraction: f64| {
                let (s, c) = (fraction * tau).sin_cos();
                let point = Vec3::new(c, s, if curved { 0.2 } else { 2.0 });
                let jet = if curved { support.jet((c + 2.0) / 4.0, (s + 2.0) / 4.0) } else { support.jet(c, s) }.unwrap();
                Ok(Target { corner: false, point, tangent: Vec3::new(-s, c, 0.0), normal: jet.normal().unwrap(), support: jet })
            };
            let result = match_opening(&cage, &config, target, 0.01).unwrap_or_else(|e| panic!("curved={curved}: {e}"));
            eprintln!("G2 curved={curved}: {:?}", result.report);
            assert!(result.report.position_mm <= 0.001);
            assert!(result.report.tangent_deg <= 0.1);
            assert!(result.report.curvature_relative <= 0.01);
            assert_eq!(result.cage.faces, cage.faces);
            assert_eq!(result.cage.points.len(), cage.points.len());
            assert!(result.report.controls <= 256);
            let eval = quadratic_extension(&result.cage).unwrap();
            assert_eq!(eval.faces.len(), cage.faces.len() + 32);
            assert_eq!(eval.points.len(), cage.points.len() + 32);
            let preview = super::super::matching::preview(&result.cage, 2, 2).unwrap();
            assert_eq!(preview.vertices.len(), cage.points.len());
            assert_eq!(preview.boundaries.len(), cage.edges().unwrap().len());
            assert!(preview.mesh.tri_face.iter().all(|&face| (face as usize) < cage.faces.len()));
            let used: std::collections::BTreeSet<_> = preview.mesh.triangles.iter().flatten().copied().collect();
            assert_eq!(used.len(), preview.mesh.positions.len());
            let refined = super::super::matching::refine_g2(&result.cage, 1).unwrap();
            let before = eval.refined(1).unwrap();
            let after = quadratic_extension(&refined).unwrap();
            let basis = super::super::fitting::RootBasis { level: 2, indices: (0..=4).collect() };
            let before = basis.fit(&before).unwrap();
            let after = basis.fit(&after).unwrap();
            for (old, new) in before.iter().zip(&after).take(refined.faces.len()) {
                let old = EvalSurface::from(old);
                let new = EvalSurface::from(new);
                for u in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    for v in [0.0, 0.25, 0.5, 0.75, 1.0] {
                        let old = old.jet(u, v).unwrap();
                        let new = new.jet(u, v).unwrap();
                        for (a, b) in [old.point, old.du, old.dv, old.duu, old.duv, old.dvv]
                            .into_iter()
                            .zip([new.point, new.du, new.dv, new.duu, new.duv, new.dvv])
                        {
                            assert!(a.dist(b) < 1e-8, "refinement changed a G2 limit-surface jet: {a:?} versus {b:?}");
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn separating_unweld_and_boundary_weld_are_topological_inverses() {
        use solidcore_record::Vec3;
        let cage = Parameters { segments: [1; 3], dimensions: [2.0; 3], ..Default::default() }.build().unwrap();
        let face = &cage.faces[0];
        let seam: Vec<_> = face.iter().copied().zip(face.iter().copied().cycle().skip(1)).take(4).map(|(a, b)| [a, b]).collect();
        let cage = cage.with_creases(&[seam[0]], &[seam[0][0]], 2.5).unwrap();
        let [a, b] = cage.split_seam(&seam).unwrap();
        assert_eq!(a.faces.len() + b.faces.len(), 6);
        assert_eq!(a.points.len() + b.points.len(), 12);
        assert_eq!(a.boundary_loops().unwrap()[0].len(), 4);
        assert_eq!(b.boundary_loops().unwrap()[0].len(), 4);
        let edge = |c: &super::super::Cage| {
            let r = c.boundary_loops().unwrap().remove(0);
            [r[0], r[1]]
        };
        let welded = a.weld_openings(&b, edge(&a), edge(&b)).unwrap();
        assert_eq!((welded.points.len(), welded.faces.len()), (8, 6));
        assert!(welded.boundary_loops().unwrap().is_empty());
        assert_eq!(welded.edge_creases.len(), 1);
        assert_eq!(welded.edge_creases[0].sharpness, 2.5);
        assert!(welded.points.iter().all(|p| cage.points.contains(p)));
        let both = a.disjoint_union(&b).unwrap();
        assert_eq!((both.points.len(), both.faces.len()), (12, 6));
        assert_eq!(both.boundary_loops().unwrap().len(), 2);
        let mut moved = b.clone();
        for p in &mut moved.points {
            *p += Vec3::Z * 0.4;
        }
        let shifted = a.weld_openings(&moved, edge(&a), edge(&moved)).unwrap();
        let boundary = a.boundary_loops().unwrap().remove(0);
        assert!(boundary.iter().all(|&id| shifted.points.iter().any(|p| p.dist(a.points[id] + Vec3::Z * 0.2) < 1e-12)));
        assert!(cage.split_seam(&seam[..1]).is_err());
        assert!(a.split_seam(&seam).is_err());
        assert!(a.weld_openings(&b, [usize::MAX, 0], edge(&b)).is_err());
    }
    #[test]
    fn ring_insertion_keeps_quads_and_transfers_sharpness_to_split_edges() {
        let cage = Parameters { segments: [1; 3], dimensions: [2.0; 3], ..Default::default() }.build().unwrap();
        let seed = cage.edges().unwrap()[0];
        let cage = cage.with_creases(&[seed], &[seed[0]], 2.0).unwrap();
        let (inserted, seams) = cage.insert_edge_ring(seed, 0.5).unwrap();
        assert_eq!(seams.len(), 4);
        assert_eq!((inserted.points.len(), inserted.faces.len()), (12, 10));
        assert!(inserted.faces.iter().all(|f| f.len() == 4));
        assert_eq!(&inserted.points[..8], &cage.points);
        assert_eq!(inserted.edge_creases.len(), 2);
        assert!(inserted.edge_creases.iter().all(|c| c.sharpness == 2.0));
        assert!(inserted.vertex_creases.iter().any(|c| c.vertex >= 8 && c.sharpness == 1.0));
        assert!(inserted.boundary_loops().unwrap().is_empty());
        assert!(cage.insert_edge_ring(seed, f64::NAN).is_err());
        assert!(cage.insert_edge_ring([usize::MAX, 0], 0.5).is_err());
    }
    #[test]
    fn quad_fill_modes_and_erasing_an_extrusion_preserve_unrelated_openings() {
        use super::super::topology_tools::FillMode;
        let cylinder =
            Parameters { kind: "cylinder".into(), segments: [1; 3], dimensions: [10.0; 3], radii: [5.0, 2.0], ..Default::default() }.build().unwrap();
        let caps: Vec<_> =
            cylinder.faces.iter().enumerate().filter_map(|(id, f)| f.iter().all(|&v| cylinder.points[v].z > 4.9).then_some(id)).collect();
        let open = cylinder.delete_faces(&caps).unwrap();
        let ring = open.boundary_loops().unwrap().remove(0);
        assert_eq!(ring.len(), 8);
        let edge = [ring[0], ring[1]];
        let star = open.fill_boundaries(&[edge], FillMode::Automatic).unwrap();
        let minimal = open.fill_boundaries(&[edge], FillMode::Minimal).unwrap();
        assert_eq!(star.faces.len(), open.faces.len() + 4);
        assert_eq!(minimal.faces.len(), open.faces.len() + 3);
        assert_eq!(star.points.len(), open.points.len() + 1);
        assert!(star.faces.iter().chain(&minimal.faces).all(|f| f.len() == 4));
        assert!(star.boundary_loops().unwrap().is_empty());
        assert!(open.fill_boundaries(&[edge], FillMode::Single).is_err());

        let cube = Parameters { segments: [1; 3], dimensions: [10.0; 3], ..Default::default() }.build().unwrap();
        let bottom = cube.faces.iter().position(|f| f.iter().all(|&v| cube.points[v].z < -4.9)).unwrap();
        let cage = cube.delete_faces(&[bottom]).unwrap();
        let count = cage.faces.len();
        let extruded = cage.extrude_faces(&[0], solidcore_record::Vec3::Z * 3.0).unwrap();
        let mut region = vec![0];
        region.extend(count..extruded.faces.len());
        let restored = extruded.erase_and_fill(&region).unwrap();
        assert_eq!(restored.faces.len(), count);
        assert_eq!(restored.points.len(), cage.points.len());
        assert_eq!(restored.boundary_loops().unwrap().len(), 1);
        let positions =
            |c: &super::Cage| c.points.iter().map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(positions(&restored), positions(&cage));
    }
    #[test]
    fn sharp_thickening_preserves_the_two_skins_and_duplicates_source_creases() {
        let cage = Parameters { kind: "face".into(), segments: [2; 3], dimensions: [10.0; 3], ..Default::default() }.build().unwrap();
        let sharp = cage.thicken(2.0, true).unwrap();
        let smooth = cage.thicken(2.0, false).unwrap();
        assert_eq!((sharp.points.len(), sharp.faces.len()), (18, 16));
        assert_eq!(sharp.edge_creases.len(), 16);
        assert!(sharp.edge_creases.iter().all(|c| c.sharpness == 10.0));
        assert!(sharp.boundary_loops().unwrap().is_empty());
        assert_eq!(sharp.limit_vertices().unwrap()[0].z, 0.0);
        assert!(smooth.limit_vertices().unwrap()[0].z > 0.0);
        let crease = cage.edges().unwrap()[0];
        let cage = cage.with_creases(&[crease], &[0], 1.5).unwrap();
        let thick = cage.thicken(-2.0, false).unwrap();
        assert_eq!(thick.edge_creases.len(), 2);
        assert_eq!(thick.vertex_creases.len(), 2);
        assert!(thick.edge_creases.iter().all(|c| c.sharpness == 1.5));
        assert_eq!(thick.limit_vertices().unwrap().iter().map(|p| p.z).fold(f64::INFINITY, f64::min), -2.0);
    }
    #[test]
    fn unequal_boundary_bridge_is_a_closed_quad_transition() {
        let a = Parameters { kind: "face".into(), segments: [1; 3], dimensions: [10.0; 3], ..Default::default() }.build().unwrap();
        let b = Parameters { kind: "face".into(), segments: [2; 3], dimensions: [10.0; 3], ..Default::default() }.build().unwrap();
        let offset = a.points.len();
        let mut cage = a.clone();
        for f in &mut cage.faces {
            f.reverse();
        }
        cage.points.extend(b.points.iter().map(|p| *p + solidcore_record::Vec3::Z * 4.0));
        cage.faces.extend(b.faces.iter().map(|f| f.iter().map(|v| v + offset).collect()));
        assert_eq!(cage.boundary_loops().unwrap().iter().map(Vec::len).collect::<Vec<_>>(), vec![4, 8]);
        assert!(cage.bridge([0, offset]).is_err());
        let bridged = cage.bridge_transition([0, offset], true).unwrap();
        assert_eq!(bridged.faces.len(), 11);
        assert!(bridged.faces.iter().all(|f| f.len() == 4));
        assert!(bridged.boundary_loops().unwrap().is_empty());
        assert!(!bridged.surface(2).unwrap().mesh.triangles.is_empty());
    }
    use super::*;
    #[test]
    fn edits_preserve_manifold_orientation_and_undo_source() {
        let c = Parameters { segments: [1; 3], ..Default::default() }.build().unwrap();
        let raised = c.extrude_faces(&[0], Vec3::Z * 5.0).unwrap();
        assert_eq!(raised.faces.len(), 10);
        assert!(raised.boundary_loops().unwrap().is_empty());
        let hole = c.delete_faces(&[0]).unwrap();
        assert_eq!(hole.boundary_loops().unwrap().len(), 1);
        let filled = hole.fill_hole(hole.boundary_loops().unwrap()[0][0]).unwrap();
        assert!(filled.boundary_loops().unwrap().is_empty());
        let dissolved = c.dissolve_edge(c.edges().unwrap()[0]).unwrap();
        assert_eq!(dissolved.faces.len(), 5);
        assert_eq!(dissolved.faces.iter().map(Vec::len).max(), Some(6));
        assert!(!dissolved.mesh(2).unwrap().triangles.is_empty());
        let detached = c.unweld(&[0]).unwrap();
        assert_eq!(detached.boundary_loops().unwrap().len(), 2);
        let rings = detached.boundary_loops().unwrap();
        let joined = detached.bridge([rings[0][0], rings[1][0]]).unwrap();
        assert!(joined.boundary_loops().unwrap().is_empty());
        let plane = Parameters { kind: "face".into(), segments: [1; 3], ..Default::default() }.build().unwrap();
        let thick = plane.thicken(2.0, false).unwrap();
        assert_eq!(thick.faces.len(), 6);
        assert!(thick.boundary_loops().unwrap().is_empty());
        let extruded = plane.extrude_edges(&[plane.edges().unwrap()[0]], Vec3::Z * 5.0).unwrap();
        assert_eq!(extruded.faces.len(), 2);
        assert_eq!(c.faces.len(), 6);
        assert!(c.extrude_faces(&[999], Vec3::Z).is_err());
    }
}
