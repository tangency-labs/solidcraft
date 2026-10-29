//! Transactional topology edits. Invalid, nonmanifold results never replace the source cage.
use super::{Cage, Result, bad, key};
use crate::Vec3;
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
        let t = self.topology()?;
        let mut next = BTreeMap::new();
        for e in t.edges.iter().filter(|e| e.faces.len() == 1) {
            let face = &self.faces[e.faces[0]];
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
            let plane = EvalSurface::Plane { origin: crate::Vec3::Z * z, du: crate::Vec3::X, dv: crate::Vec3::Y };
            let target = |fraction: f64| {
                let at = fraction * ring.len() as f64;
                let i = (at.floor() as usize).min(ring.len() - 1);
                let (a, b) = (source.points[ring[i]], source.points[ring[(i + 1) % ring.len()]]);
                let mut point = a.lerp(b, at - i as f64);
                point.z = z;
                Ok(Target { corner: false, point, tangent: b - a, normal: crate::Vec3::Z, support: plane.jet(point.x, point.y).unwrap() })
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
        use crate::{Vec3, tess::EvalSurface};
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
        }
    }
    #[test]
    fn separating_unweld_and_boundary_weld_are_topological_inverses() {
        use crate::Vec3;
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
        let extruded = cage.extrude_faces(&[0], crate::Vec3::Z * 3.0).unwrap();
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
        cage.points.extend(b.points.iter().map(|p| *p + crate::Vec3::Z * 4.0));
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
