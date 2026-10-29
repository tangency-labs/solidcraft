//! Logical deletion and region merging preserve all evaluator control identities.
use super::*;

fn edge_uses(faces: &[Vec<usize>]) -> BTreeMap<(usize, usize), Vec<usize>> {
    let mut uses: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for (id, face) in faces.iter().enumerate() {
        for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
            uses.entry(key(a, b)).or_default().push(id);
        }
    }
    uses
}

pub(super) fn dissolved_layout(mut layout: Layout, edges: &[[usize; 2]]) -> Result<Layout> {
    if edges.is_empty() {
        return Ok(layout);
    }
    if edges.len() > MAX_LEAVES * 4 {
        return Err(bad("dissolved seam budget exceeded"));
    }
    let uses = edge_uses(&layout.faces);
    let mut adjacency: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for &[a, b] in edges {
        if !seen.insert(key(a, b)) {
            return Err(bad("duplicate dissolved seam"));
        }
        let pair = uses.get(&key(a, b)).filter(|ids| ids.len() == 2).ok_or_else(|| bad("dissolve requires interior logical edges"))?;
        let (&first, &second) = (pair.first().ok_or_else(|| bad("missing first region"))?, pair.get(1).ok_or_else(|| bad("missing second region"))?);
        adjacency.entry(first).or_default().insert(second);
        adjacency.entry(second).or_default().insert(first);
    }
    let mut representatives = BTreeMap::new();
    for &start in adjacency.keys() {
        if representatives.contains_key(&start) {
            continue;
        }
        let mut group = BTreeSet::new();
        let mut pending = vec![start];
        while let Some(id) = pending.pop() {
            if group.insert(id) {
                pending.extend(adjacency.get(&id).into_iter().flatten().copied().filter(|n| !group.contains(n)));
            }
        }
        let mut next = BTreeMap::new();
        let mut incoming = BTreeSet::new();
        for &id in &group {
            let face = layout.faces.get(id).ok_or_else(|| bad("missing dissolved face"))?;
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                let interior = uses.get(&key(a, b)).is_some_and(|ids| ids.len() == 2 && ids.iter().all(|i| group.contains(i)));
                if !interior && (next.insert(a, b).is_some() || !incoming.insert(b)) {
                    return Err(bad("dissolved region has a branching boundary"));
                }
            }
        }
        if !(3..=256).contains(&next.len()) {
            return Err(bad("dissolved region requires one boundary within 256 corners"));
        }
        let begin = *next.keys().next().ok_or_else(|| bad("dissolved region has no boundary"))?;
        let mut boundary = Vec::with_capacity(next.len());
        let mut vertex = begin;
        for _ in 0..next.len() {
            boundary.push(vertex);
            vertex = *next.get(&vertex).ok_or_else(|| bad("dissolved boundary is open"))?;
            if vertex == begin {
                break;
            }
        }
        if vertex != begin || boundary.len() != next.len() {
            return Err(bad("dissolved region cannot contain holes"));
        }
        for &id in &group {
            representatives.insert(id, start);
            layout.faces.get_mut(id).ok_or_else(|| bad("missing region slot"))?.clear();
        }
        *layout.faces.get_mut(start).ok_or_else(|| bad("missing merged region"))? = boundary;
    }
    for id in layout.owners.iter_mut().flatten() {
        *id = representatives.get(id).copied().unwrap_or(*id);
    }
    let visible: BTreeSet<_> = edge_uses(&layout.faces).into_keys().collect();
    layout.paths.retain(|(edge, _)| visible.contains(&key(edge[0], edge[1])));
    Ok(layout)
}

impl Hierarchy {
    /// Bridge explicitly turns local evaluator cells into ordinary editable quads.
    /// Deleted cells stay absent; source control IDs are returned for boundary selection.
    pub fn materialize(&self) -> Result<(Cage, BTreeMap<usize, usize>)> {
        let mut cage = self.evaluate()?;
        let layout = self.layout()?;
        cage.faces = cage.faces.into_iter().zip(layout.owners).filter_map(|(face, owner)| owner.map(|_| face)).collect();
        cage.finish_edit_with_map()
    }
    pub fn leaf_corners(&self) -> Result<Vec<Option<[usize; 4]>>> {
        let (locations, _, _) = self.locations()?;
        self.leaves
            .iter()
            .map(|leaf| {
                if !leaf.active {
                    return Ok(None);
                };
                let [u, v, x, y] = leaf.domain;
                let mut corners = [0; 4];
                for (slot, (u, v)) in corners.iter_mut().zip([(u, v), (x, v), (x, y), (u, y)]) {
                    *slot = *locations.get(&(leaf.root, u, v)).ok_or_else(|| bad("missing parametric leaf corner"))?;
                }
                Ok(Some(corners))
            })
            .collect()
    }
    pub fn delete_faces(&self, selected: &[usize]) -> Result<Self> {
        let layout = self.layout()?;
        let count = selected.len();
        let selected: BTreeSet<_> = selected.iter().copied().collect();
        if selected.is_empty() || selected.len() != count || selected.iter().any(|&id| layout.faces.get(id).is_none_or(Vec::is_empty)) {
            return Err(bad("select existing logical faces to delete"));
        }
        let raw = self.raw_layout()?;
        let removed: BTreeSet<_> =
            raw.owners.iter().zip(&layout.owners).filter_map(|(raw, owner)| owner.filter(|id| selected.contains(id)).and(*raw)).collect();
        let mut out = self.clone();
        for id in removed {
            out.leaves.get_mut(id).ok_or_else(|| bad("missing deleted leaf"))?.active = false;
        }
        if !out.leaves.iter().any(|leaf| leaf.active) {
            return Err(bad("deleting every face would leave an empty Form"));
        }
        let remaining = out.raw_layout()?;
        let uses = edge_uses(&remaining.faces);
        out.dissolved_edges.retain(|e| uses.get(&key(e[0], e[1])).is_some_and(|ids| ids.len() == 2));
        out.retain_visible_controls()?;
        Ok(out)
    }

    pub fn dissolve_edges(&self, edges: &[[usize; 2]]) -> Result<Self> {
        let layout = self.layout()?;
        let uses = edge_uses(&layout.faces);
        if edges.is_empty() || edges.iter().any(|e| uses.get(&key(e[0], e[1])).is_none_or(|ids| ids.len() != 2)) {
            return Err(bad("select interior logical edges to dissolve"));
        }
        let mut out = self.clone();
        out.dissolved_edges.extend(edges.iter().map(|e| [e[0].min(e[1]), e[0].max(e[1])]));
        out.dissolved_edges.sort_unstable();
        out.dissolved_edges.dedup();
        out.retain_visible_controls()?;
        Ok(out)
    }

    fn retain_visible_controls(&mut self) -> Result<()> {
        let used: BTreeSet<_> = self.layout()?.faces.into_iter().flatten().collect();
        self.controls.retain(|id, _| used.contains(id));
        Ok(())
    }
}
