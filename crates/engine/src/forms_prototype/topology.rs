//! Topology operations that intentionally materialize a local evaluator do so explicitly.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

impl Form {
    pub fn insert_loop(&mut self, edge: [usize; 2], fraction: f64) -> Result<()> {
        if self.hierarchy.is_some() {
            return Err(error("whole edge-loop insertion requires a base cage"));
        }
        let (cage, _) = self.cage.insert_edge_ring(edge, fraction).map_err(error)?;
        self.replace_cage(cage)
    }
    pub fn bridge_boundaries(&mut self, edges: &[[usize; 2]], vertices: &[usize]) -> Result<()> {
        let local = self.hierarchy.is_some();
        let (cage, remap) = if let Some(h) = &self.hierarchy {
            h.materialize().map_err(error)?
        } else {
            (self.cage.clone(), (0..self.cage.points.len()).map(|i| (i, i)).collect())
        };
        let seeds = if edges.is_empty() {
            if vertices.len() != 2 {
                return Err(error("select one edge on each of two free boundaries"));
            }
            vertices.to_vec()
        } else {
            let mut uses: BTreeMap<(usize, usize), usize> = BTreeMap::new();
            for face in &self.logical.faces {
                for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                    *uses.entry((a.min(b), a.max(b))).or_default() += 1;
                }
            }
            if edges.iter().any(|e| uses.get(&(e[0].min(e[1]), e[0].max(e[1]))) != Some(&1)) {
                return Err(error("bridge requires free logical boundary edges"));
            }
            edges.iter().map(|e| e[0]).collect()
        };
        let loops = cage.boundary_loops().map_err(error)?;
        let mut selected = BTreeSet::new();
        for seed in seeds {
            let vertex = *remap.get(&seed).ok_or_else(|| error("selected boundary did not survive materialization"))?;
            let id = loops.iter().position(|l| l.contains(&vertex)).ok_or_else(|| error("select a free boundary"))?;
            selected.insert(id);
        }
        if selected.len() != 2 {
            return Err(error("select exactly two distinct boundary loops"));
        }
        let mut seeds = Vec::new();
        for id in selected {
            seeds.push(*loops.get(id).and_then(|l| l.first()).ok_or_else(|| error("empty selected boundary"))?);
        }
        let a = *seeds.first().ok_or_else(|| error("missing first boundary"))?;
        let b = *seeds.get(1).ok_or_else(|| error("missing second boundary"))?;
        self.adopt_base(cage.bridge_transition([a, b], local).map_err(error)?)
    }
}
