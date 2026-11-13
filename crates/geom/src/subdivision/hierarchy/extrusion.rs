//! Replay local extrusion without replacing the original editable hierarchy.
use super::*;
use crate::subdivision::{
    stable_extrusion::{self, Extrusion, Wall},
    symmetry::Symmetry,
};
mod mirror;

const MAX_EXTRUSIONS: usize = 32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LevelExtrusion {
    pub level: usize,
    pub first_vertex: usize,
    /// Evaluator face indices grouped by original logical face, in face ID order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cells: Vec<Vec<usize>>,
    /// True free evaluator edges expanded from selected logical edge paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    edges: Vec<[usize; 2]>,
    delta: Vec3,
    /// Actual displacement of each cap chart when the selected components move differently.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    face_offsets: Vec<Vec3>,
    /// Actual displacement of each evaluator edge when mirrored strips move differently.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    edge_offsets: Vec<Vec3>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<Detail>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharpness: Option<LevelSharpness>,
}

impl LevelExtrusion {
    pub(super) fn wall_count_bound(&self) -> Option<usize> {
        // A selected quad can contribute at most four wall sides. A selected
        // free edge contributes one. Shared sides can only reduce the count.
        self.cells.iter().try_fold(self.edges.len(), |count, chart| count.checked_add(chart.len().checked_mul(4)?))
    }

    pub(super) fn is_single_face_for_preview_compaction(&self) -> bool {
        self.level == 1
            && self.cells.len() == 1
            && self.edges.is_empty()
            && self.face_offsets.is_empty()
            && self.edge_offsets.is_empty()
            && self.sharpness.is_none()
    }
    pub(super) fn replay(&self, cage: &Cage, influence: bool) -> Result<Extrusion> {
        if self.first_vertex != cage.points.len()
            || self.cells.is_empty() == self.edges.is_empty()
            || (!self.face_offsets.is_empty() && self.face_offsets.len() != self.cells.len())
            || (!self.edge_offsets.is_empty() && self.edge_offsets.len() != self.edges.len())
        {
            return Err(bad("invalid hierarchical extrusion topology"));
        }
        let result = if self.cells.is_empty() {
            if influence {
                stable_extrusion::boundary_influence(cage, &self.edges)
            } else if self.edge_offsets.is_empty() {
                stable_extrusion::boundary(cage, &self.edges, self.delta)
            } else {
                let mut result = stable_extrusion::boundary_influence(cage, &self.edges)?;
                mirror::apply_edge_offsets(&self.edges, &self.edge_offsets, &mut result)?;
                Ok(result)
            }
        } else {
            if self.cells.len() > MAX_LEAVES
                || self.cells.iter().any(Vec::is_empty)
                || self.cells.iter().map(Vec::len).sum::<usize>() > cage.faces.len()
            {
                return Err(bad("invalid hierarchical extrusion topology"));
            }
            let selected: Vec<_> = self.cells.iter().flatten().copied().collect();
            if influence {
                stable_extrusion::face_influence(cage, &selected)
            } else if self.face_offsets.is_empty() {
                stable_extrusion::faces(cage, &selected, self.delta)
            } else {
                let mut result = stable_extrusion::face_influence(cage, &selected)?;
                mirror::apply_offsets(cage, &self.cells, &self.face_offsets, &mut result)?;
                Ok(result)
            }
        }?;
        if result.walls.is_empty() {
            return Err(bad("extrusion needs a region with a boundary"));
        }
        Ok(result)
    }

    pub(super) fn rechart(&self, tags: &mut Vec<Tag>, charts: &mut usize, walls: &[Wall]) -> Result<()> {
        for cells in &self.cells {
            let first = *cells.first().ok_or_else(|| bad("empty extrusion chart"))?;
            let source = tags.get(first).ok_or_else(|| bad("missing extrusion source chart"))?.0;
            for &cell in cells {
                let tag = tags.get_mut(cell).ok_or_else(|| bad("missing extrusion source cell"))?;
                if tag.0 != source {
                    return Err(bad("extrusion chart crosses distinct source patches"));
                }
                tag.0 = *charts;
            }
            *charts += 1;
        }
        let step = GRID >> self.level;
        for wall in walls {
            if wall.face != tags.len() {
                return Err(bad("extrusion wall order disagrees with evaluator"));
            }
            tags.push((*charts, [[0, 0], [step, 0], [step, step], [0, step]]));
            *charts += 1;
        }
        Ok(())
    }
}

impl Hierarchy {
    pub(super) fn validate_extrusion_levels(&self) -> Result<()> {
        if self.depth > MAX_DEPTH
            || self.extrusions.len() > MAX_EXTRUSIONS
            || self.extrusions.iter().any(|op| op.level > self.depth || op.sharpness.as_ref().is_some_and(|s| s.level != op.level))
            || self.extrusions.windows(2).any(|pair| pair[0].level > pair[1].level)
        {
            return Err(bad("invalid hierarchical extrusion levels"));
        }
        Ok(())
    }

    pub(super) fn extrusions_at(&self, level: usize) -> impl Iterator<Item = &LevelExtrusion> {
        self.extrusions.iter().filter(move |operation| operation.level == level)
    }

    /// Affine remapping of authored offsets, alongside the base control coordinates.
    pub fn map_displacements(&mut self, mut transform: impl FnMut(Vec3) -> Vec3) {
        if let Some(wall) = &mut self.trim_wall {
            wall.delta = transform(wall.delta);
        }
        for detail in self.details.iter_mut().flatten() {
            detail.delta = transform(detail.delta);
        }
        for operation in &mut self.extrusions {
            operation.delta = transform(operation.delta);
            for offset in &mut operation.face_offsets {
                *offset = transform(*offset);
            }
            for offset in &mut operation.edge_offsets {
                *offset = transform(*offset);
            }
            for detail in &mut operation.details {
                detail.delta = transform(detail.delta);
            }
        }
    }

    /// Extrude active logical faces while retaining their cap IDs, base controls,
    /// and the previously authored detail levels. Wall charts start at this level.
    pub fn extrude_faces(&self, selected: &[usize], delta: Vec3) -> Result<Self> {
        self.extrude_face_regions(selected, delta, None)
    }

    /// Extrude each selected face and its mirror in one connected topology edit.
    /// The first picked side of a paired region supplies the displacement.
    pub fn extrude_symmetric_faces(&self, selected: &[usize], delta: Vec3, symmetry: &Symmetry) -> Result<Self> {
        self.extrude_face_regions(selected, delta, Some(symmetry))
    }

    fn extrude_face_regions(&self, selected: &[usize], delta: Vec3, symmetry: Option<&Symmetry>) -> Result<Self> {
        self.check_trim_wall_topology()?;
        if self.extrusions.len() >= MAX_EXTRUSIONS || !self.dissolved_edges.is_empty() {
            return Err(bad("hierarchical extrusion requires undissolved faces within the operation budget"));
        }
        if selected.is_empty() || selected.len() > self.leaves.len() || selected.iter().copied().collect::<BTreeSet<_>>().len() != selected.len() {
            return Err(bad("select distinct active logical faces to extrude"));
        }
        let layout = self.raw_layout()?;
        let expanded;
        let faces = if let Some(symmetry) = symmetry {
            expanded = symmetry.mirror_face_selection(&layout.faces, selected)?;
            &expanded
        } else {
            selected
        };
        let mut cells = BTreeMap::new();
        for &id in faces {
            if self.leaves.get(id).is_none_or(|leaf| !leaf.active) || cells.insert(id, Vec::new()).is_some() {
                return Err(bad("select distinct active logical faces to extrude"));
            }
        }
        for (cell, owner) in layout.owners.into_iter().enumerate() {
            if let Some(group) = owner.and_then(|id| cells.get_mut(&id)) {
                group.push(cell);
            }
        }
        let cage = self.evaluate()?;
        let (delta, face_offsets) = if let Some(symmetry) = symmetry {
            symmetry.validate_reflection(&cage)?;
            mirror::face_offsets(&cage, &cells, selected, symmetry, delta)?
        } else {
            (delta, Vec::new())
        };
        let operation = LevelExtrusion {
            level: self.depth,
            first_vertex: cage.points.len(),
            cells: cells.values().cloned().collect(),
            edges: Vec::new(),
            delta,
            face_offsets,
            edge_offsets: Vec::new(),
            details: Vec::new(),
            sharpness: None,
        };
        self.install_extrusion(&cage, operation, cells.into_keys()).map(|(hierarchy, _)| hierarchy)
    }

    /// Extend selected true free boundaries. A logical edge may span multiple
    /// evaluator edges. Returns the translated boundary edges on the new walls.
    pub fn extrude_edges(&self, selected: &[[usize; 2]], delta: Vec3) -> Result<(Self, Vec<[usize; 2]>)> {
        self.extrude_edge_regions(selected, delta, None)
    }

    /// Extend selected free paths and their mirrored partners in one topology edit.
    pub fn extrude_symmetric_edges(&self, selected: &[[usize; 2]], delta: Vec3, symmetry: &Symmetry) -> Result<(Self, Vec<[usize; 2]>)> {
        self.extrude_edge_regions(selected, delta, Some(symmetry))
    }

    fn extrude_edge_regions(&self, selected: &[[usize; 2]], delta: Vec3, symmetry: Option<&Symmetry>) -> Result<(Self, Vec<[usize; 2]>)> {
        self.check_trim_wall_topology()?;
        if self.extrusions.len() >= MAX_EXTRUSIONS || selected.is_empty() || selected.len() > MAX_LEAVES {
            return Err(bad("select free logical edges within the extrusion operation budget"));
        }
        let layout = self.layout()?;
        let paths: BTreeMap<_, _> = layout.paths.into_iter().map(|([a, b], path)| (key(a, b), path)).collect();
        let mut requested = Vec::new();
        let mut unique = BTreeSet::new();
        let mut mirrored = BTreeSet::new();
        for &[a, b] in selected {
            let path = paths.get(&key(a, b)).ok_or_else(|| bad("select existing logical boundary edges to extrude"))?;
            for pair in path.windows(2) {
                let edge = key(pair[0], pair[1]);
                if !unique.insert(edge) {
                    return Err(bad("duplicate logical edge extrusion"));
                }
                requested.push(edge);
            }
            if let Some(symmetry) = symmetry {
                let reflected = path
                    .iter()
                    .map(|&v| symmetry.partners.get(v).copied().ok_or_else(|| bad("missing edge mirror partner")))
                    .collect::<Result<Vec<_>>>()?;
                let first = *reflected.first().ok_or_else(|| bad("empty mirrored edge"))?;
                let last = *reflected.last().ok_or_else(|| bad("empty mirrored edge"))?;
                let partner = paths.get(&key(first, last)).ok_or_else(|| bad("the reflected edge is not a logical boundary"))?;
                if partner != &reflected && !partner.iter().eq(reflected.iter().rev()) {
                    return Err(bad("the reflected path has a different subdivision"));
                }
                mirrored.extend(reflected.windows(2).map(|pair| key(pair[0], pair[1])));
            }
        }
        let cage = self.evaluate()?;
        unique.append(&mut mirrored);
        let edges: Vec<_> = unique.into_iter().map(|(a, b)| [a, b]).collect();
        let (delta, edge_offsets) = if let Some(symmetry) = symmetry {
            symmetry.validate_reflection(&cage)?;
            let seeds = requested
                .into_iter()
                .map(|(a, b)| edges.binary_search(&[a, b]).map_err(|_| bad("missing selected evaluator edge")))
                .collect::<Result<Vec<_>>>()?;
            mirror::edge_offsets(&edges, &seeds, symmetry, delta)?
        } else {
            (delta, Vec::new())
        };
        let operation = LevelExtrusion {
            level: self.depth,
            first_vertex: cage.points.len(),
            cells: Vec::new(),
            edges,
            delta,
            face_offsets: Vec::new(),
            edge_offsets,
            details: Vec::new(),
            sharpness: None,
        };
        self.install_extrusion(&cage, operation, std::iter::empty())
    }

    fn install_extrusion(
        &self,
        cage: &Cage,
        operation: LevelExtrusion,
        cap_leaves: impl IntoIterator<Item = usize>,
    ) -> Result<(Self, Vec<[usize; 2]>)> {
        let (_, mut tags, _) = self.tags()?;
        let charts = tags.iter().map(|(root, _)| root + 1).max().unwrap_or(0);
        let mut next_chart = charts;
        let result = operation.replay(cage, false)?;
        if self.leaves.len().saturating_add(result.walls.len()) > MAX_LEAVES {
            return Err(bad("hierarchical extrusion leaf budget exceeded"));
        }
        operation.rechart(&mut tags, &mut next_chart, &result.walls)?;
        let mut out = self.clone();
        for (chart, id) in (charts..).zip(cap_leaves) {
            out.leaves.get_mut(id).ok_or_else(|| bad("missing extrusion cap leaf"))?.root = chart;
        }
        let step = GRID >> self.depth;
        for chart in charts + operation.cells.len()..next_chart {
            out.leaves.push(Leaf { root: chart, domain: [0, 0, step, step], active: true });
        }
        out.extrusions.push(operation);
        let (locations, _, levels) = out.locations()?;
        for leaf in out.leaves.iter().filter(|leaf| leaf.active) {
            let [u, v, x, y] = leaf.domain;
            for (u, v) in [(u, v), (x, v), (x, y), (u, y)] {
                let vertex = *locations.get(&(leaf.root, u, v)).ok_or_else(|| bad("missing extruded logical corner"))?;
                let level = levels.iter().position(|&n| vertex < n).ok_or_else(|| bad("missing extrusion control level"))?;
                out.controls.entry(vertex).or_insert(level);
            }
        }
        out.layout()?;
        let front = result
            .walls
            .iter()
            .map(|wall| {
                let [a, b] = wall.cap;
                [a.min(b), a.max(b)]
            })
            .collect();
        Ok((out, front))
    }
}
