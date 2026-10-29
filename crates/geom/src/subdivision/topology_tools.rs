//! Bounded quad topology tools independent of document and viewport state.
use super::{Cage, Result, bad, key};
use crate::Vec3;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FillMode {
    Automatic,
    Single,
    Minimal,
    Star,
}

impl Cage {
    /// Split a closed base cage along a separating seam, with independent
    /// compact controls on each new free boundary.
    pub fn split_seam(&self, selected: &[[usize; 2]]) -> Result<[Self; 2]> {
        self.validate()?;
        if self.faces.iter().any(|f| f.len() != 4) || !self.boundary_loops()?.is_empty() {
            return Err(bad("Unweld requires a closed all-quad base cage"));
        }
        let seam: BTreeSet<_> = selected.iter().map(|e| key(e[0], e[1])).collect();
        let topology = self.topology()?;
        if seam.is_empty()
            || seam.len() != selected.len()
            || seam.iter().any(|e| topology.lookup.get(e).and_then(|&id| topology.edges.get(id)).is_none_or(|e| e.faces.len() != 2))
        {
            return Err(bad("select distinct interior seam edges to unweld"));
        }
        let mut adjacency = vec![Vec::new(); self.faces.len()];
        for edge in &topology.edges {
            if seam.contains(&key(edge.a, edge.b)) {
                continue;
            }
            if let [a, b] = edge.faces.as_slice() {
                adjacency[*a].push(*b);
                adjacency[*b].push(*a);
            }
        }
        let mut seen = BTreeSet::new();
        let mut parts = Vec::new();
        for first in 0..self.faces.len() {
            if seen.contains(&first) {
                continue;
            }
            let mut pending = vec![first];
            let mut faces = BTreeSet::new();
            while let Some(id) = pending.pop() {
                if !seen.insert(id) {
                    continue;
                }
                faces.insert(id);
                pending.extend(adjacency.get(id).ok_or_else(|| bad("missing seam adjacency"))?.iter().copied().filter(|id| !seen.contains(id)));
            }
            if parts.len() == 2 {
                return Err(bad("the selected seam must separate exactly two face regions"));
            }
            let mut part = self.clone();
            part.faces = faces.into_iter().map(|id| self.faces.get(id).cloned().ok_or_else(|| bad("invalid seam face"))).collect::<Result<_>>()?;
            parts.push(part.finish_edit()?);
        }
        if parts.len() != 2 {
            return Err(bad("the selected seam does not separate the cage"));
        }
        let second = parts.pop().ok_or_else(|| bad("missing second seam region"))?;
        let first = parts.pop().ok_or_else(|| bad("missing first seam region"))?;
        Ok([first, second])
    }
    /// Join equal free openings by identifying their controls; no extra faces
    /// are introduced. Both cages must already use the same coordinate frame.
    pub fn weld_openings(&self, other: &Self, first_edge: [usize; 2], second_edge: [usize; 2]) -> Result<Self> {
        self.validate()?;
        other.validate()?;
        if self.faces.iter().chain(&other.faces).any(|f| f.len() != 4) {
            return Err(bad("Weld requires all-quad base cages"));
        }
        let containing = |cage: &Cage, edge: [usize; 2]| -> Result<Vec<usize>> {
            cage.boundary_loops()?
                .into_iter()
                .find(|l| l.iter().copied().zip(l.iter().copied().cycle().skip(1)).take(l.len()).any(|(a, b)| key(a, b) == key(edge[0], edge[1])))
                .ok_or_else(|| bad("select a free boundary edge on each Form"))
        };
        let a = containing(self, first_edge)?;
        let mut b = containing(other, second_edge)?;
        if a.len() != b.len() || a.len() > 256 {
            return Err(bad("Weld requires equal openings within256 controls"));
        }
        b.reverse();
        let mut best = (0, f64::INFINITY);
        for offset in 0..b.len() {
            let mut score = 0.0;
            for (i, &v) in a.iter().enumerate() {
                score += (self.point(v)? - other.point(b[(i + offset) % b.len()])?).len2();
            }
            if score < best.1 {
                best = (offset, score);
            }
        }
        b.rotate_left(best.0);
        let mut out = self.clone();
        let mut remap: BTreeMap<_, _> = b.iter().copied().zip(a.iter().copied()).collect();
        for (&source, &target) in &remap {
            *out.points.get_mut(target).ok_or_else(|| bad("missing Weld control"))? = (self.point(target)? + other.point(source)?) * 0.5;
        }
        for (id, &p) in other.points.iter().enumerate() {
            if let std::collections::btree_map::Entry::Vacant(entry) = remap.entry(id) {
                entry.insert(out.points.len());
                out.points.push(p);
            }
        }
        let mut existing: BTreeSet<_> = self.faces.iter().map(|f| f.iter().copied().collect::<BTreeSet<_>>()).collect();
        for face in &other.faces {
            let face: Vec<_> = face.iter().map(|id| remap.get(id).copied().ok_or_else(|| bad("missing Weld remap"))).collect::<Result<_>>()?;
            if existing.insert(face.iter().copied().collect()) {
                out.faces.push(face);
            }
        }
        let mut edge_weights: BTreeMap<_, f64> = out.edge_creases.iter().map(|c| (key(c.edge[0], c.edge[1]), c.sharpness)).collect();
        for crease in &other.edge_creases {
            let mapped = crease.edge.map(|v| remap.get(&v).copied());
            let [Some(a), Some(b)] = mapped else { return Err(bad("missing Weld crease control")) };
            edge_weights.entry(key(a, b)).and_modify(|w| *w = w.max(crease.sharpness)).or_insert(crease.sharpness);
        }
        out.edge_creases = edge_weights.into_iter().map(|((a, b), sharpness)| super::EdgeCrease { edge: [a, b], sharpness }).collect();
        let mut vertex_weights: BTreeMap<_, f64> = out.vertex_creases.iter().map(|c| (c.vertex, c.sharpness)).collect();
        for crease in &other.vertex_creases {
            let mapped = *remap.get(&crease.vertex).ok_or_else(|| bad("missing Weld corner control"))?;
            vertex_weights.entry(mapped).and_modify(|w| *w = w.max(crease.sharpness)).or_insert(crease.sharpness);
        }
        out.vertex_creases = vertex_weights.into_iter().map(|(vertex, sharpness)| super::VertexCrease { vertex, sharpness }).collect();
        out.edit_budget()?;
        out.finish_edit()
    }
    pub fn disjoint_union(&self, other: &Self) -> Result<Self> {
        self.validate()?;
        other.validate()?;
        let mut out = self.clone();
        let offset = out.points.len();
        out.points.extend_from_slice(&other.points);
        out.faces.extend(other.faces.iter().map(|f| f.iter().map(|v| v + offset).collect()));
        out.edge_creases.extend(other.edge_creases.iter().map(|c| super::EdgeCrease { edge: c.edge.map(|v| v + offset), sharpness: c.sharpness }));
        out.vertex_creases.extend(other.vertex_creases.iter().map(|c| super::VertexCrease { vertex: c.vertex + offset, sharpness: c.sharpness }));
        out.edit_budget()?;
        out.finish_edit()
    }
    pub fn fill_boundaries(&self, selected: &[[usize; 2]], mode: FillMode) -> Result<Self> {
        let loops = self.boundary_loops()?;
        let requested: BTreeSet<_> = selected.iter().map(|e| key(e[0], e[1])).collect();
        let available: BTreeSet<_> = loops
            .iter()
            .flat_map(|ring| ring.iter().copied().zip(ring.iter().copied().cycle().skip(1)).take(ring.len()).map(|(a, b)| key(a, b)))
            .collect();
        if requested.is_empty() || !requested.is_subset(&available) {
            return Err(bad("select free boundary edges to fill"));
        }
        let mut out = self.clone();
        for ring in loops {
            if !ring.iter().copied().zip(ring.iter().copied().cycle().skip(1)).take(ring.len()).any(|(a, b)| requested.contains(&key(a, b))) {
                continue;
            }
            if ring.len() < 4 || ring.len() % 2 != 0 || ring.len() > 256 {
                return Err(bad("quad filling requires an even boundary within4..256 vertices"));
            }
            let mut fill = ring;
            fill.reverse();
            if fill.len() == 4 && mode != FillMode::Star {
                out.faces.push(fill);
                continue;
            }
            if mode == FillMode::Single {
                return Err(bad("single-face filling requires four boundary edges"));
            }
            if mode == FillMode::Minimal {
                for i in (1..fill.len() - 2).step_by(2) {
                    out.faces.push(vec![fill[0], fill[i], fill[i + 1], fill[i + 2]]);
                }
            } else {
                let center = fill.iter().map(|&v| self.point(v)).collect::<Result<Vec<_>>>()?.into_iter().fold(Vec3::ZERO, |sum, p| sum + p)
                    / fill.len() as f64;
                let id = out.points.len();
                out.points.push(center);
                for i in (0..fill.len()).step_by(2) {
                    out.faces.push(vec![fill[i], fill[i + 1], fill[(i + 2) % fill.len()], id]);
                }
            }
        }
        out.edit_budget()?;
        out.finish_edit()
    }

    pub fn erase_and_fill(&self, selected: &[usize]) -> Result<Self> {
        let topology = self.topology()?;
        let removed = self.selected_faces(selected)?;
        if removed.len() == self.faces.len() {
            return Err(bad("erasing every face would leave an empty Form"));
        }
        let interface: Vec<_> = topology
            .edges
            .iter()
            .filter(|e| e.faces.len() == 2 && e.faces.iter().filter(|f| removed.contains(f)).count() == 1)
            .map(|e| [e.a, e.b])
            .collect();
        if interface.is_empty() {
            return Err(bad("the erased region must border retained faces"));
        }
        let mut out = self.clone();
        out.faces = self.faces.iter().enumerate().filter(|(id, _)| !removed.contains(id)).map(|(_, f)| f.clone()).collect();
        let (out, remap) = out.finish_edit_with_map()?;
        let edges = interface
            .into_iter()
            .map(|e| {
                Ok([
                    *remap.get(&e[0]).ok_or_else(|| bad("missing retained boundary"))?,
                    *remap.get(&e[1]).ok_or_else(|| bad("missing retained boundary"))?,
                ])
            })
            .collect::<Result<Vec<_>>>()?;
        out.fill_boundaries(&edges, FillMode::Minimal)
    }

    pub fn insert_edge_ring(&self, seed: [usize; 2], fraction: f64) -> Result<(Self, Vec<[usize; 2]>)> {
        let topology = self.topology()?;
        if !fraction.is_finite() || !(1e-6..=1.0 - 1e-6).contains(&fraction) || self.faces.iter().any(|f| f.len() != 4) {
            return Err(bad("edge-ring insertion requires quads and an interior fraction"));
        }
        let start = key(seed[0], seed[1]);
        if !topology.lookup.contains_key(&start) {
            return Err(bad("select an existing control edge"));
        }
        let mut directions = BTreeMap::from([(start, [start.0, start.1])]);
        let mut pending = vec![start];
        let mut crossed = BTreeSet::new();
        while let Some(current) = pending.pop() {
            let direction = *directions.get(&current).ok_or_else(|| bad("missing ring direction"))?;
            let index = *topology.lookup.get(&current).ok_or_else(|| bad("missing ring edge"))?;
            for &id in &topology.edges.get(index).ok_or_else(|| bad("missing edge topology"))?.faces {
                crossed.insert(id);
                let face = self.faces.get(id).ok_or_else(|| bad("missing ring face"))?;
                let k = (0..4).find(|&k| key(face[k], face[(k + 1) % 4]) == current).ok_or_else(|| bad("edge is outside its ring face"))?;
                let next = if direction == [face[k], face[(k + 1) % 4]] {
                    [face[(k + 3) % 4], face[(k + 2) % 4]]
                } else {
                    [face[(k + 2) % 4], face[(k + 3) % 4]]
                };
                let next_key = key(next[0], next[1]);
                if let Some(previous) = directions.get(&next_key) {
                    if *previous != next {
                        return Err(bad("the ring reverses its parameter direction"));
                    }
                } else {
                    directions.insert(next_key, next);
                    pending.push(next_key);
                }
            }
        }
        if self.points.len() + directions.len() > super::MAX_POINTS || self.faces.len() + crossed.len() > super::MAX_FACES {
            return Err(bad("edge-ring insertion exceeds the editing budget"));
        }
        let mut out = self.clone();
        let mut inserted = BTreeMap::new();
        let corners: BTreeMap<_, _> = self.vertex_creases.iter().map(|c| (c.vertex, c.sharpness)).collect();
        for (&edge, &[a, b]) in &directions {
            let id = out.points.len();
            inserted.insert(edge, id);
            out.points.push(self.point(a)?.lerp(self.point(b)?, fraction));
            let sharpness = corners.get(&a).copied().unwrap_or(0.0) * (1.0 - fraction) + corners.get(&b).copied().unwrap_or(0.0) * fraction;
            if sharpness > 0.0 {
                out.vertex_creases.push(super::VertexCrease { vertex: id, sharpness });
            }
        }
        out.faces.clear();
        let mut seams = Vec::new();
        for face in &self.faces {
            let cuts: Vec<_> = (0..4).filter(|&k| inserted.contains_key(&key(face[k], face[(k + 1) % 4]))).collect();
            if cuts.is_empty() {
                out.faces.push(face.clone());
                continue;
            }
            let k = *cuts.first().ok_or_else(|| bad("missing ring crossing"))?;
            if cuts.len() != 2 || !cuts.contains(&((k + 2) % 4)) {
                return Err(bad("the ring crosses one face more than once"));
            }
            let p = *inserted.get(&key(face[k], face[(k + 1) % 4])).ok_or_else(|| bad("missing inserted endpoint"))?;
            let q = *inserted.get(&key(face[(k + 2) % 4], face[(k + 3) % 4])).ok_or_else(|| bad("missing opposite endpoint"))?;
            out.faces.push(vec![face[k], p, q, face[(k + 3) % 4]]);
            out.faces.push(vec![p, face[(k + 1) % 4], face[(k + 2) % 4], q]);
            seams.push([p.min(q), p.max(q)]);
        }
        out.edge_creases.clear();
        for crease in &self.edge_creases {
            if let Some(&id) = inserted.get(&key(crease.edge[0], crease.edge[1])) {
                out.edge_creases.extend(crease.edge.map(|v| super::EdgeCrease { edge: [v, id], sharpness: crease.sharpness }));
            } else {
                out.edge_creases.push(crease.clone());
            }
        }
        out.edit_budget()?;
        let (out, remap) = out.finish_edit_with_map()?;
        let seams = seams
            .into_iter()
            .map(|e| {
                Ok([*remap.get(&e[0]).ok_or_else(|| bad("inserted seam was lost"))?, *remap.get(&e[1]).ok_or_else(|| bad("inserted seam was lost"))?])
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((out, seams))
    }

    pub fn bridge_transition(&self, vertices: [usize; 2], allow_unequal: bool) -> Result<Self> {
        let loops = self.boundary_loops()?;
        let first = loops.iter().position(|l| l.contains(&vertices[0])).ok_or_else(|| bad("pick the first boundary"))?;
        let second = loops.iter().position(|l| l.contains(&vertices[1])).ok_or_else(|| bad("pick the second boundary"))?;
        let a = loops.get(first).ok_or_else(|| bad("missing first boundary"))?;
        let mut b = loops.get(second).ok_or_else(|| bad("missing second boundary"))?.clone();
        if first == second || a.len() > 256 || b.len() > 256 || (!allow_unequal && a.len() != b.len()) || (a.len() + b.len()) % 2 != 0 {
            return Err(bad("bridge requires separate compatible boundaries within256 controls"));
        }
        b.reverse();
        let mut best = None;
        for shift in 0..b.len() {
            let mut score = 0.0;
            for (i, &v) in a.iter().enumerate() {
                score += self.point(v)?.dist(self.point(b[(i * b.len() / a.len() + shift) % b.len()])?);
            }
            if best.is_none_or(|(_, old)| score < old) {
                best = Some((shift, score));
            }
        }
        b.rotate_left(best.ok_or_else(|| bad("empty bridge boundary"))?.0);
        let mut out = self.clone();
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            let (start_i, start_j) = (i, j);
            let mut steps = [false; 2];
            for step in &mut steps {
                *step = i < a.len() && (j == b.len() || (i + 1) * b.len() <= (j + 1) * a.len());
                if *step {
                    i += 1
                } else {
                    j += 1
                };
            }
            let face = match steps {
                [true, true] => vec![a[i % a.len()], a[(start_i + 1) % a.len()], a[start_i % a.len()], b[start_j % b.len()]],
                [false, false] => vec![a[start_i % a.len()], b[start_j % b.len()], b[(start_j + 1) % b.len()], b[j % b.len()]],
                _ => vec![a[i % a.len()], a[start_i % a.len()], b[start_j % b.len()], b[j % b.len()]],
            };
            out.faces.push(face);
        }
        out.edit_budget()?;
        out.finish_edit()
    }

    fn edit_budget(&self) -> Result<()> {
        if self.faces.len() > super::MAX_FACES || self.points.len() > super::MAX_POINTS {
            return Err(bad("edited cage exceeds the editing budget"));
        }
        Ok(())
    }
}
