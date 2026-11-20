//! Mirror selection is resolved through stable controls and logical face identities.
use super::*;
use solidcraft_geom::subdivision::{hierarchy::Hierarchy, symmetry::Symmetry};
use std::collections::{BTreeMap, BTreeSet};

impl Form {
    pub(super) fn validate_logical_symmetry(&self, symmetry: &Symmetry) -> Result<()> {
        validate_layout(&self.logical, self.hierarchy.as_ref(), symmetry)
    }
}
pub(super) fn validate_layout(logical: &Cage, hierarchy: Option<&Hierarchy>, symmetry: &Symmetry) -> Result<()> {
    let faces: BTreeSet<BTreeSet<usize>> = logical.faces.iter().filter(|f| !f.is_empty()).map(|f| f.iter().copied().collect()).collect();
    for face in &faces {
        let mirrored = face
            .iter()
            .map(|&v| symmetry.partners.get(v).copied().ok_or_else(|| error("invalid mirrored logical control")))
            .collect::<Result<BTreeSet<_>>>()?;
        if !faces.contains(&mirrored) {
            return Err(error("local face layout must be symmetric before enabling its mirror plane"));
        }
    }
    if let Some(h) = hierarchy {
        for (&id, &level) in &h.controls {
            let partner = symmetry.partners.get(id).ok_or_else(|| error("invalid mirrored control"))?;
            if h.controls.get(partner) != Some(&level) {
                return Err(error("mirrored controls must share their hierarchical level"));
            }
        }
    }
    Ok(())
}
impl Form {
    pub fn mirror_face_selection(&self, selected: &[usize]) -> Result<Vec<usize>> {
        let Some(symmetry) = &self.symmetry else { return Ok(selected.to_vec()) };
        symmetry.mirror_face_selection(&self.logical.faces, selected).map_err(error)
    }
    pub(super) fn refine_symmetric(&mut self, selected: &[usize], counts: [usize; 2]) -> Result<()> {
        let mut h = self.hierarchy.clone().map_or_else(|| Hierarchy::new(self.cage.clone()), Ok).map_err(error)?;
        if selected.is_empty() || selected.len() > 10_000 {
            return Err(error("select logical faces to refine"));
        }
        let mut plan: BTreeMap<usize, [usize; 2]> = selected.iter().map(|&id| (id, counts)).collect();
        if plan.len() != selected.len() {
            return Err(error("select distinct logical faces"));
        }
        if let Some(symmetry) = &self.symmetry {
            let corners = h.leaf_corners().map_err(error)?;
            for &id in selected {
                let pair = self.mirror_face_selection(&[id])?;
                let source = corners.get(id).copied().flatten().ok_or_else(|| error("missing source leaf corners"))?;
                for mirror in pair {
                    let other = corners.get(mirror).copied().flatten().ok_or_else(|| error("missing mirrored leaf corners"))?;
                    let a = *symmetry.partners.get(source[0]).ok_or_else(|| error("missing first mirror corner"))?;
                    let b = *symmetry.partners.get(source[1]).ok_or_else(|| error("missing second mirror corner"))?;
                    let reflected = source
                        .iter()
                        .map(|&v| symmetry.partners.get(v).copied().ok_or_else(|| error("missing mirror corner")))
                        .collect::<Result<Vec<_>>>()?;
                    if !reflected.iter().all(|v| other.contains(v)) {
                        continue;
                    }
                    let horizontal = [(other[0], other[1]), (other[2], other[3])].iter().any(|&(x, y)| (x == a && y == b) || (x == b && y == a));
                    let requested = if horizontal { counts } else { [counts[1], counts[0]] };
                    plan.entry(mirror)
                        .and_modify(|n| {
                            n[0] = n[0].max(requested[0]);
                            n[1] = n[1].max(requested[1]);
                        })
                        .or_insert(requested);
                }
            }
        }
        let mut groups: BTreeMap<[usize; 2], Vec<usize>> = BTreeMap::new();
        for (id, counts) in plan {
            groups.entry(counts).or_default().push(id);
        }
        for (counts, ids) in groups {
            h = h.subdivide(&ids, counts).map_err(error)?;
        }
        self.adopt_hierarchy(h, true)
    }
    pub fn hierarchical_creases(&mut self, edges: &[[usize; 2]], vertices: &[usize], sharpness: f64) -> Result<()> {
        if !sharpness.is_finite() || !(0.0..=10.0).contains(&sharpness) || (edges.is_empty() && vertices.is_empty()) {
            return Err(error("select crease elements and sharpness 0..10"));
        }
        let original = self.hierarchy.as_ref().ok_or_else(|| error("missing local topology"))?;
        if self.matching.is_some() {
            return Err(error("detach Match before changing a hierarchical crease"));
        }
        if original.has_trim_wall() {
            return Err(error("remove the derived trim wall before changing a hierarchical crease"));
        }
        let mut h = original.clone();
        let base_edges: BTreeSet<_> = h.base.edges().map_err(error)?.into_iter().collect();
        let (coarse_edges, local_edges): (Vec<_>, Vec<_>) =
            edges.iter().map(|&[a, b]| [a.min(b), a.max(b)]).partition(|edge| base_edges.contains(edge));
        let (coarse_vertices, local_vertices): (Vec<_>, Vec<_>) = vertices.iter().copied().partition(|&v| v < h.base.points.len());
        if !coarse_edges.is_empty() || !coarse_vertices.is_empty() {
            h.base = h.base.with_creases(&coarse_edges, &coarse_vertices, sharpness).map_err(error)?;
        }
        if !local_edges.is_empty() || !local_vertices.is_empty() {
            let layout = original.layout().map_err(error)?;
            let paths: BTreeMap<_, _> = layout.paths.into_iter().collect();
            let active: BTreeSet<_> = layout.faces.into_iter().flatten().collect();
            let mut segments = BTreeSet::new();
            for edge in local_edges {
                let path = paths
                    .get(&edge)
                    .filter(|path| path.len() >= 2 && path.first() == Some(&edge[0]) && path.last() == Some(&edge[1]))
                    .ok_or_else(|| error("select an active logical edge to change its crease"))?;
                for pair in path.windows(2) {
                    segments.insert([pair[0].min(pair[1]), pair[0].max(pair[1])]);
                }
            }
            if local_vertices.iter().any(|v| !active.contains(v)) {
                return Err(error("select an active logical point to change its crease"));
            }
            let cage = h.evaluate().map_err(error)?;
            let edited = cage.with_creases(&segments.into_iter().collect::<Vec<_>>(), &local_vertices, sharpness).map_err(error)?;
            if edited != cage {
                if let Some(symmetry) = &self.symmetry {
                    symmetry.validate_reflection(&edited).map_err(error)?;
                }
                h = h.with_evaluator(&edited).map_err(error)?;
            }
        }
        if h == *original {
            return Ok(());
        }
        self.adopt_hierarchy(h, true)
    }
}
