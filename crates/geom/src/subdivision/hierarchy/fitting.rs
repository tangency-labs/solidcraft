//! Root grids include sparse hierarchical displacements before limit sampling.
use super::*;
use crate::subdivision::fitting::{Fit, root_grid, sample_grids};

/// A logical region on one original fitted surface; holes run clockwise.
pub struct Region {
    pub root: usize,
    pub owner: usize,
    pub wires: Vec<Vec<[f64; 2]>>,
}

/// One active rectangular leaf's centre in the oriented fitted source chart.
pub struct FaceFrameSite {
    pub owner: usize,
    pub chart: usize,
    pub uv: [f64; 2],
    /// Chart parameter change along the logical root's positive U direction.
    pub along_u: [f64; 2],
}

impl Hierarchy {
    pub fn face_frame_sites(&self, original_roots: bool) -> Result<Vec<FaceFrameSite>> {
        if self.leaves.len() > MAX_LEAVES
            || self.leaves.iter().filter(|leaf| leaf.active).any(|leaf| {
                let [u, v, x, y] = leaf.domain;
                u >= x || v >= y || x > GRID || y > GRID || (original_roots && leaf.root >= self.base.faces.len())
            })
        {
            return Err(bad("invalid Form frame leaf parameters"));
        }
        let root_site = |owner: usize, leaf: &Leaf| FaceFrameSite {
            owner,
            chart: leaf.root,
            uv: [(leaf.domain[0] + leaf.domain[2]) as f64 / (2 * GRID) as f64, (leaf.domain[1] + leaf.domain[3]) as f64 / (2 * GRID) as f64],
            along_u: [1.0, 0.0],
        };
        if original_roots && self.dissolved_edges.is_empty() {
            return Ok(self.leaves.iter().enumerate().filter(|(_, leaf)| leaf.active).map(|(owner, leaf)| root_site(owner, leaf)).collect());
        }
        let Layout { owners, .. } = self.layout()?;
        let (_, tags, _) = self.tags()?;
        if owners.len() != tags.len() {
            return Err(bad("Form frame chart ownership disagrees with its evaluator"));
        }
        let mut sites = BTreeMap::new();
        let mut merged = BTreeSet::new();
        for (chart, (owner, (root, uv))) in owners.into_iter().zip(tags).enumerate() {
            let Some(owner) = owner else { continue };
            let leaf = self.leaves.get(owner).filter(|leaf| leaf.active).ok_or_else(|| bad("missing active Form frame leaf"))?;
            let [u, v, x, y] = leaf.domain;
            if root != leaf.root || uv.iter().any(|&[a, b]| a < u || a > x || b < v || b > y) {
                merged.insert(owner);
                continue;
            }
            if original_roots {
                sites.entry(owner).or_insert_with(|| root_site(owner, leaf));
                continue;
            }
            let centre = [u + x, v + y];
            // At an evaluator seam use the cell on the positive root-U/V
            // side. A logical leaf's centre is strictly inside its rectangle.
            if (0..2).any(|axis| {
                let a = 2 * uv[0][axis].min(uv[2][axis]);
                let b = 2 * uv[0][axis].max(uv[2][axis]);
                !(a..b).contains(&centre[axis])
            }) {
                continue;
            }
            let delta = |p: [usize; 2]| std::array::from_fn::<_, 2, _>(|axis| p[axis] as f64 - uv[0][axis] as f64);
            let (a, b) = (delta(uv[1]), delta(uv[3]));
            let c = std::array::from_fn::<_, 2, _>(|axis| centre[axis] as f64 * 0.5 - uv[0][axis] as f64);
            let det = a[0] * b[1] - a[1] * b[0];
            if det <= 0.0 {
                return Err(bad("invalid oriented evaluator frame chart"));
            }
            let local = [(c[0] * b[1] - c[1] * b[0]) / det, (a[0] * c[1] - a[1] * c[0]) / det];
            let site = FaceFrameSite { owner, chart, uv: local, along_u: [b[1] / det, -a[1] / det] };
            if sites.insert(owner, site).is_some() {
                return Err(bad("ambiguous evaluator frame chart"));
            }
        }
        Ok(sites.into_iter().filter_map(|(owner, site)| (!merged.contains(&owner)).then_some(site)).collect())
    }

    pub(super) fn has_local_sharpness(&self) -> bool {
        self.sharpness.iter().any(|level| level.level > 0)
    }

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
        self.skin_edge_charts_many(edges, original_roots)
    }
    /// Read-only source charts for active skin edges, also while a derived wall
    /// is present. The wall's virtual rim has its own ruled-patch charts.
    pub fn skin_edge_charts_many(&self, edges: &[[usize; 2]], original_roots: bool) -> Result<Vec<Vec<crate::subdivision::fitting::EdgeChart>>> {
        if edges.is_empty() || edges.len() > 65_536 {
            return Err(bad("source edge chart request exceeds its topology budget"));
        }
        let layout = self.layout()?;
        if layout.collapse.is_some() && !original_roots {
            return Err(bad("a retained Collapse requires its original fitted charts"));
        }
        let available: BTreeMap<_, _> = layout.paths.iter().map(|([a, b], path)| (key(*a, *b), path)).collect();
        let mut paths = Vec::new();
        let mut derived = Vec::with_capacity(edges.len());
        for &edge in edges {
            let chart = layout.collapse.and_then(|site| site.diagonal_chart(edge));
            if chart.is_none() {
                let path = available.get(&key(edge[0], edge[1])).ok_or_else(|| bad("select a visible logical edge"))?;
                paths.push((*path).clone());
            }
            derived.push(chart);
        }
        let mut regular = if paths.is_empty() {
            Vec::new().into_iter()
        } else {
            let cage = self.evaluate()?;
            let charts = if original_roots {
                Some(self.tags()?.1.into_iter().map(|(root, uv)| (root, uv.map(|p| p.map(|v| v as f64 / GRID as f64)))).collect::<Vec<_>>())
            } else {
                None
            };
            crate::subdivision::fitting::edge_charts(&cage, &paths, Some(&layout.owners), charts.as_deref())?.into_iter()
        };
        derived
            .into_iter()
            .map(|chart| match chart {
                Some(chart) => Ok(vec![chart]),
                None => regular.next().ok_or_else(|| bad("missing fitted logical edge charts")),
            })
            .collect()
    }

    /// Four oriented evaluator cells of each intact original root eligible
    /// for exact planar preview-sheet compaction. Cap and wall charts retain
    /// their normal evaluator route.
    pub fn extrusion_preview_sheet_groups(&self) -> Result<Vec<[usize; 4]>> {
        let [operation] = self.extrusions.as_slice() else { return Ok(Vec::new()) };
        if self.depth != 1
            || !operation.is_single_face_for_preview_compaction()
            || self.has_local_sharpness()
            || self.has_trim_wall()
            || self.has_quad_cap()
            || !self.dissolved_edges.is_empty()
        {
            return Ok(Vec::new());
        }
        let layout = self.layout()?;
        let (_, tags, _) = self.tags()?;
        if tags.len() != layout.owners.len() {
            return Err(bad("preview sheet cells lost their logical ownership"));
        }
        let mut by_owner = BTreeMap::<usize, Vec<usize>>::new();
        for (cell, owner) in layout.owners.into_iter().enumerate() {
            if let Some(owner) = owner {
                by_owner.entry(owner).or_default().push(cell);
            }
        }
        let half = GRID / 2;
        let oriented = [
            [[0, 0], [half, 0], [half, half], [0, half]],
            [[GRID, 0], [GRID, half], [half, half], [half, 0]],
            [[GRID, GRID], [half, GRID], [half, half], [GRID, half]],
            [[0, GRID], [0, half], [half, half], [half, GRID]],
        ];
        let mut groups = Vec::new();
        for (owner, cells) in by_owner {
            let Ok(cells) = <[usize; 4]>::try_from(cells) else { continue };
            let leaf = self.leaves.get(owner).ok_or_else(|| bad("missing preview sheet owner"))?;
            if !leaf.active || leaf.root >= self.base.faces.len() || leaf.domain != [0, 0, GRID, GRID] {
                continue;
            }
            if cells.iter().zip(oriented).all(|(&cell, uv)| tags.get(cell) == Some(&(leaf.root, uv))) {
                groups.push(cells);
            }
        }
        Ok(groups)
    }
    pub fn fit_root_patches(&self, tolerance: f64) -> Result<Fit> {
        if !self.extrusions.is_empty() {
            return Err(bad("extruded hierarchy requires evaluator patch fitting"));
        }
        if self.has_local_sharpness() {
            // A smooth original-root tensor spans the new seam. Terminal
            // evaluator cells put every authored crease on a fitted boundary.
            return Err(bad("local sharpness requires evaluator patch fitting"));
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
