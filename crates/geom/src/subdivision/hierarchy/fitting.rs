//! Root grids include sparse hierarchical displacements before limit sampling.
use super::*;
use crate::subdivision::fitting::{Fit, root_grid, sample_grids};

/// A logical region on one original fitted surface; holes run clockwise.
pub struct Region {
    pub root: usize,
    pub owner: usize,
    pub wires: Vec<Vec<[f64; 2]>>,
}

impl Hierarchy {
    /// Resolve one logical edge independently of display subdivision.
    pub fn edge_charts(&self, edge: [usize; 2], original_roots: bool) -> Result<Vec<crate::subdivision::fitting::EdgeChart>> {
        self.edge_charts_many(&[edge], original_roots)?.pop().ok_or_else(|| bad("source edge chart is missing"))
    }
    /// Batch incidence traversal shares the evaluator and chart tags.
    pub fn edge_charts_many(&self, edges: &[[usize; 2]], original_roots: bool) -> Result<Vec<Vec<crate::subdivision::fitting::EdgeChart>>> {
        if edges.is_empty() || edges.len() > 65_536 {
            return Err(bad("source edge chart request exceeds its topology budget"));
        }
        self.check_trim_wall_topology()?;
        let layout = self.layout()?;
        let available: BTreeMap<_, _> = layout.paths.iter().map(|([a, b], path)| (key(*a, *b), path)).collect();
        let paths = edges
            .iter()
            .map(|e| available.get(&key(e[0], e[1])).map(|path| (*path).clone()).ok_or_else(|| bad("select a visible logical edge")))
            .collect::<Result<Vec<_>>>()?;
        let cage = self.evaluate()?;
        let charts = if original_roots {
            Some(self.tags()?.1.into_iter().map(|(root, uv)| (root, uv.map(|p| p.map(|v| v as f64 / GRID as f64)))).collect::<Vec<_>>())
        } else {
            None
        };
        crate::subdivision::fitting::edge_charts(&cage, &paths, Some(&layout.owners), charts.as_deref())
    }

    pub fn fit_root_patches(&self, tolerance: f64) -> Result<Fit> {
        if !self.extrusions.is_empty() {
            return Err(bad("extruded hierarchy requires evaluator patch fitting"));
        }
        if !tolerance.is_finite() || !(1e-6..=1.0).contains(&tolerance) {
            return Err(bad("CAD fit tolerance must be 0.000001..1 mm"));
        }
        let mut last = f64::INFINITY;
        for level in self.depth.max(2)..=6 {
            let grids = self.patch_grids(level)?;
            let fine = self.patch_grids(level + 1)?;
            if let Some(fit) = root_grid::fit(&grids, &fine, level, tolerance, &mut last, None)? {
                return Ok(fit);
            }
        }
        Err(bad(&format!("hierarchical root fit deviation {last:.6} mm exceeds {tolerance:.6} mm")))
    }
    fn patch_grids(&self, level: usize) -> Result<Vec<Vec<Vec3>>> {
        if level < self.depth {
            return Err(bad("root sampling cannot discard hierarchical detail"));
        }
        self.base.refined_budget_at(level, 7, crate::subdivision::MAX_EVAL_FACES)?;
        let span = 1_usize << level;
        let (_, mut tags, _) = self.tags()?;
        for (_, uv) in &mut tags {
            for p in uv {
                *p = p.map(|c| c * span / GRID);
            }
        }
        sample_grids(self.evaluate()?, tags, level - self.depth, span, self.base.faces.len())
    }
    /// Boundaries are taken from logical leaves, so evaluator grid lines never
    /// become selectable B-rep edges. Shared T-points remain on neighboring wires.
    pub fn cad_regions(&self) -> Result<Vec<Region>> {
        let raw = self.raw_layout()?;
        let logical = self.layout()?;
        let mut owners = BTreeMap::new();
        for (leaf, owner) in raw.owners.iter().zip(&logical.owners) {
            if let (Some(leaf), Some(owner)) = (leaf, owner) {
                owners.insert(*leaf, *owner);
            }
        }
        let (locations, _, _) = self.locations()?;
        let uv: BTreeMap<_, _> =
            locations.into_iter().map(|((root, u, v), id)| ((root, id), [u as f64 / GRID as f64, v as f64 / GRID as f64])).collect();
        let mut groups: BTreeMap<(usize, usize), BTreeSet<(usize, usize)>> = BTreeMap::new();
        for (id, leaf) in self.leaves.iter().enumerate().filter(|(_, l)| l.active) {
            let owner = *owners.get(&id).ok_or_else(|| bad("missing CAD region owner"))?;
            let edges = groups.entry((owner, leaf.root)).or_default();
            let face = raw.faces.get(id).ok_or_else(|| bad("missing CAD leaf boundary"))?;
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                if !edges.remove(&(b, a)) && !edges.insert((a, b)) {
                    return Err(bad("duplicate CAD region boundary"));
                }
            }
        }
        let mut regions = Vec::with_capacity(groups.len());
        for ((owner, root), mut edges) in groups {
            let mut wires = Vec::new();
            while let Some(&(a, b)) = edges.first() {
                let mut wire = vec![*uv.get(&(root, a)).ok_or_else(|| bad("missing CAD boundary parameter"))?];
                edges.remove(&(a, b));
                let mut at = b;
                while at != a {
                    if wire.len() > 1024 {
                        return Err(bad("CAD region boundary budget exceeded"));
                    }
                    wire.push(*uv.get(&(root, at)).ok_or_else(|| bad("missing CAD boundary parameter"))?);
                    let next: Vec<_> = edges.range((at, 0)..=(at, usize::MAX)).copied().collect();
                    if next.len() != 1 {
                        return Err(bad("CAD region has an open or branching boundary"));
                    }
                    let edge = *next.first().ok_or_else(|| bad("missing CAD boundary edge"))?;
                    edges.remove(&edge);
                    at = edge.1;
                }
                wires.push(wire);
            }
            if wires.is_empty() {
                return Err(bad("empty CAD region"));
            }
            regions.push(Region { root, owner, wires });
        }
        Ok(regions)
    }
}
