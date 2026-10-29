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
        let lookup: BTreeMap<BTreeSet<usize>, usize> =
            self.logical.faces.iter().enumerate().filter(|(_, f)| !f.is_empty()).map(|(id, f)| (f.iter().copied().collect(), id)).collect();
        let mut out: BTreeSet<_> = selected.iter().copied().collect();
        for &id in selected {
            let face = self.logical.faces.get(id).filter(|f| !f.is_empty()).ok_or_else(|| error("select an existing logical face"))?;
            let reflected = face
                .iter()
                .map(|&v| symmetry.partners.get(v).copied().ok_or_else(|| error("invalid mirrored corner")))
                .collect::<Result<BTreeSet<_>>>()?;
            out.insert(*lookup.get(&reflected).ok_or_else(|| error("logical face has no matching mirrored region"))?);
        }
        Ok(out.into_iter().collect())
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
        let mut h = self.hierarchy.clone().ok_or_else(|| error("missing local topology"))?;
        let base_edges: BTreeSet<_> = h.base.edges().map_err(error)?.into_iter().map(|e| (e[0].min(e[1]), e[0].max(e[1]))).collect();
        if sharpness > 0.0
            && (edges.iter().any(|e| !base_edges.contains(&(e[0].min(e[1]), e[0].max(e[1])))) || vertices.iter().any(|&v| v >= h.base.points.len()))
        {
            return Err(error("sharpness on newly inserted local seams is not supported by smooth root surfaces"));
        }
        let base = h.base.with_creases(edges, vertices, sharpness).map_err(error)?;
        if base == h.base {
            return Ok(());
        };
        h.base = base;
        self.adopt_hierarchy(h, true)
    }
}
