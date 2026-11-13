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

/// One free-edge collapse expressed on the unchanged source chart.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryCollapse {
    pub face: usize,
    pub root: usize,
    pub retained_neighbor: usize,
    pub point: usize,
    /// Triangle corners in retained face winding, starting at the collapsed point.
    pub boundary: [usize; 3],
    /// The same corners in the fitted original root's normalized parameters.
    pub parameters: [[f64; 2]; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RetainedBoundaryCollapse {
    endpoints: [usize; 2],
    site: BoundaryCollapse,
}

impl BoundaryCollapse {
    pub fn diagonal_edges(self) -> [[usize; 2]; 2] {
        [self.boundary[1], self.boundary[2]].map(|end| [self.point.min(end), self.point.max(end)])
    }

    pub(super) fn diagonal_chart(self, edge: [usize; 2]) -> Option<crate::subdivision::fitting::EdgeChart> {
        let canonical = [edge[0].min(edge[1]), edge[0].max(edge[1])];
        let at = self.diagonal_edges().iter().position(|&candidate| candidate == canonical)? + 1;
        let end = self.boundary[at];
        let start = self.parameters[0];
        let stop = self.parameters[at];
        let uv = if self.point < end { [start, stop] } else { [stop, start] };
        Some(crate::subdivision::fitting::EdgeChart { face: self.root, uv, range: [0.0, 1.0] })
    }
}

impl Hierarchy {
    pub fn has_boundary_collapse(&self) -> bool {
        self.boundary_collapse.is_some()
    }

    pub fn collapsed_face(&self) -> Option<usize> {
        self.boundary_collapse.map(|record| record.site.face)
    }

    pub fn is_retired_collapse_control(&self, id: usize) -> bool {
        self.boundary_collapse.is_some_and(|record| record.endpoints.contains(&id))
    }

    fn collapse_source_supported(&self) -> bool {
        self.depth > 0
            && self.trim_wall.is_none()
            && self.extrusions.is_empty()
            && !self.has_quad_cap()
            && !self.has_detached_material()
            && self.dissolved_edges.is_empty()
    }

    pub fn supports_boundary_collapse(&self) -> bool {
        !self.has_boundary_collapse() && self.collapse_source_supported()
    }

    /// Resolve a complete free side of one logical quad. Both chosen endpoints
    /// must belong exclusively to that quad. The opposite edge retains its
    /// one existing neighbor, with no change to that neighbor's chart or rim.
    pub fn boundary_collapse(&self, vertices: [usize; 2]) -> Result<BoundaryCollapse> {
        if !self.supports_boundary_collapse() {
            return Err(bad("refined Collapse needs independent original leaf charts"));
        }
        resolve_boundary_collapse(&self.skin_layout()?, &self.leaves, vertices)
    }

    pub fn reserve_boundary_collapse(&mut self, mut endpoints: [usize; 2]) -> Result<()> {
        endpoints.sort_unstable();
        let site = self.boundary_collapse(endpoints)?;
        let (_, _, levels) = self.tags()?;
        let level = levels.iter().position(|&count| site.point < count).ok_or_else(|| bad("missing collapsed control level"))?;
        self.controls.entry(site.point).or_insert(level);
        self.boundary_collapse = Some(RetainedBoundaryCollapse { endpoints, site });
        Ok(())
    }

    pub fn validated_boundary_collapse(&self) -> Result<Option<BoundaryCollapse>> {
        if !self.has_boundary_collapse() {
            return Ok(None);
        }
        Ok(self.replay_boundary_collapse(&self.skin_layout()?)?.map(|(site, _)| site))
    }

    pub(super) fn replay_boundary_collapse(&self, layout: &Layout) -> Result<Option<(BoundaryCollapse, [usize; 2])>> {
        let Some(record) = self.boundary_collapse else { return Ok(None) };
        if !self.collapse_source_supported() || record.endpoints[0] >= record.endpoints[1] {
            return Err(bad("invalid retained boundary Collapse source"));
        }
        let site = resolve_boundary_collapse(layout, &self.leaves, record.endpoints)?;
        if site != record.site || !self.controls.contains_key(&site.point) {
            return Err(bad("the retained Collapse changed its exact source ownership"));
        }
        Ok(Some((site, record.endpoints)))
    }
}

fn resolve_boundary_collapse(layout: &Layout, leaves: &[Leaf], [a, b]: [usize; 2]) -> Result<BoundaryCollapse> {
    if a == b {
        return Err(bad("select two distinct points on one complete free side"));
    }
    let mut owners = layout.faces.iter().enumerate().filter(|(_, face)| face.contains(&a) || face.contains(&b));
    let (owner, face) = owners
        .next()
        .filter(|(_, face)| face.contains(&a) && face.contains(&b))
        .filter(|_| owners.next().is_none())
        .ok_or_else(|| bad("both Collapse endpoints must belong only to the same free leaf"))?;
    let [q0, q1, q2, q3] = face.as_slice() else {
        return Err(bad("refined Collapse needs one quad with four complete logical sides"));
    };
    let corners = [*q0, *q1, *q2, *q3];
    let side =
        (0..4).find(|&i| key(corners[i], corners[(i + 1) % 4]) == key(a, b)).ok_or_else(|| bad("select adjacent points on the same free side"))?;
    let end = (side + 1) % 4;
    let next = (side + 2) % 4;
    let last = (side + 3) % 4;
    let opposite = key(corners[next], corners[last]);
    let mut adjacent = layout
        .faces
        .iter()
        .enumerate()
        .filter(|(id, face)| *id != owner && face.iter().zip(face.iter().cycle().skip(1)).take(face.len()).any(|(&x, &y)| key(x, y) == opposite));
    let (retained_neighbor, _) = adjacent
        .next()
        .filter(|_| adjacent.next().is_none())
        .ok_or_else(|| bad("refined Collapse needs one unchanged neighbor across the opposite edge"))?;
    let [lo, hi] = [a.min(b), a.max(b)];
    let path = layout
        .paths
        .binary_search_by_key(&[lo, hi], |(edge, _)| *edge)
        .ok()
        .and_then(|index| layout.paths.get(index))
        .map(|(_, path)| path)
        .filter(|path| path.len() >= 3 && !path.len().is_multiple_of(2))
        .ok_or_else(|| bad("the free Collapse side needs an existing midpoint station"))?;
    let point = *path.get(path.len() / 2).ok_or_else(|| bad("missing exact Collapse midpoint"))?;
    if layout.faces.iter().any(|face| face.contains(&point)) {
        return Err(bad("the Collapse midpoint is already owned by visible material"));
    }
    let leaf = leaves.get(owner).filter(|leaf| leaf.active).ok_or_else(|| bad("missing active Collapse chart"))?;
    let [u0, v0, u1, v1] = leaf.domain;
    let uv = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
    let midpoint = std::array::from_fn(|axis| (uv[side][axis] + uv[end][axis]) as f64 / (2 * GRID) as f64);
    let normalized = |index: usize| uv[index].map(|value| value as f64 / GRID as f64);
    Ok(BoundaryCollapse {
        face: owner,
        root: leaf.root,
        retained_neighbor,
        point,
        boundary: [point, corners[next], corners[last]],
        parameters: [midpoint, normalized(next), normalized(last)],
    })
}

/// Two independent skins over the original refined evaluator.
pub struct UnweldSplit {
    pub halves: [Hierarchy; 2],
    /// Complete paired cut sides, each directed in its retained skin winding.
    pub rim: Vec<[FreeRimSide; 2]>,
}

impl Hierarchy {
    pub fn has_detached_material(&self) -> bool {
        !self.detached_leaves.is_empty()
    }

    pub fn supports_refined_unweld(&self) -> bool {
        self.supports_hole_restoration()
            && !self.has_quad_cap()
            && self.dissolved_edges.is_empty()
            && !self.has_detached_material()
            && !self.has_boundary_collapse()
    }

    pub(super) fn validate_detached_leaves(&self) -> Result<()> {
        if self.detached_leaves.len() > MAX_LEAVES
            || self.detached_leaves.windows(2).any(|w| w[0] >= w[1])
            || self
                .detached_leaves
                .iter()
                .any(|&id| self.leaves.get(id).is_none_or(|leaf| leaf.active || leaf.domain[0] >= leaf.domain[2] || leaf.domain[1] >= leaf.domain[3]))
        {
            return Err(bad("invalid detached Unweld leaf ownership"));
        }
        Ok(())
    }

    /// Separate only logical material ownership, retaining the original
    /// evaluator and every surviving face, edge, control and detail index.
    pub fn split_logical_seam(&self, edges: &[[usize; 2]]) -> Result<UnweldSplit> {
        if !self.supports_refined_unweld() {
            return Err(bad("refined Unweld requires original undivided skin charts without caps, trim walls or extrusions"));
        }
        let source = self.layout()?;
        let RimPartition { groups: [first, second], rim } = split_logical_rim(&source, edges)?;
        let half = |detached: Vec<usize>| -> Result<Self> {
            let mut next = self.clone();
            for &id in &detached {
                let leaf = next.leaves.get_mut(id).ok_or_else(|| bad("missing Unweld leaf"))?;
                leaf.active = false;
            }
            next.detached_leaves = detached;
            next.retain_visible_controls()?;
            let layout = next.layout()?;
            if layout.faces.iter().zip(&source.faces).any(|(after, before)| !after.is_empty() && after != before) {
                return Err(bad("refined Unweld changed surviving logical face boundaries"));
            }
            Ok(next)
        };
        Ok(UnweldSplit { halves: [half(second)?, half(first)?], rim })
    }

    /// Bridge explicitly turns local evaluator cells into ordinary editable quads.
    /// Deleted cells stay absent; source control IDs are returned for boundary selection.
    pub fn materialize(&self) -> Result<(Cage, BTreeMap<usize, usize>)> {
        self.check_trim_wall_topology()?;
        if self.has_detached_material() || self.has_boundary_collapse() {
            return Err(bad("Bridge cannot rebase a retained Unweld or Collapse source"));
        }
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
        if let Some(cap) = self.quad_cap_faces().find(|id| selected.contains(id)) {
            if selected != [cap] {
                return Err(bad("select only the quad cap face to delete it"));
            }
            return self.retire_quad_cap(cap);
        }
        if self.trim_wall.is_some() && selected == [self.leaves.len()] {
            let mut out = self.clone();
            out.trim_wall = None;
            return Ok(out);
        }
        self.check_trim_wall_topology()?;
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
        self.check_trim_wall_topology()?;
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

pub(super) fn read_detached_leaves<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Vec<usize>, D::Error> {
    struct Detached;
    impl<'de> serde::de::Visitor<'de> for Detached {
        type Value = Vec<usize>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("ordered detached Form leaf IDs")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
            let mut ids = Vec::new();
            while let Some(id) = seq.next_element::<usize>()? {
                if id >= MAX_LEAVES || ids.last().is_some_and(|&previous| previous >= id) {
                    return Err(serde::de::Error::custom("invalid detached Form leaf IDs"));
                }
                ids.push(id);
            }
            Ok(ids)
        }
    }
    d.deserialize_seq(Detached)
}

struct RimPartition {
    groups: [Vec<usize>; 2],
    rim: Vec<[FreeRimSide; 2]>,
}

fn split_logical_rim(layout: &Layout, edges: &[[usize; 2]]) -> Result<RimPartition> {
    if !(3..=256).contains(&edges.len()) || layout.owners.is_empty() || layout.owners.iter().any(Option::is_none) {
        return Err(bad("refined Unweld needs one bounded separating seam on a closed skin"));
    }
    let selected: BTreeSet<_> = edges.iter().map(|&[a, b]| key(a, b)).collect();
    if selected.len() != edges.len() || selected.iter().any(|(a, b)| a == b) {
        return Err(bad("select distinct logical seam edges"));
    }
    let mut uses = BTreeMap::<_, Vec<_>>::new();
    for (owner, face) in layout.faces.iter().enumerate() {
        for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
            let row = uses.entry(key(a, b)).or_default();
            if row.len() == 2 {
                return Err(bad("refined Unweld found a nonmanifold logical seam"));
            }
            row.push((owner, a < b));
        }
    }
    let mut adjacency = vec![Vec::new(); layout.faces.len()];
    for (&edge, row) in &uses {
        let [(a, ab), (b, ba)] = row.as_slice() else { return Err(bad("refined Unweld requires a closed logical skin")) };
        if a == b || ab == ba {
            return Err(bad("refined Unweld found inconsistent logical winding"));
        }
        if !selected.contains(&edge) {
            adjacency[*a].push(*b);
            adjacency[*b].push(*a);
        }
    }
    let mut groups = [Vec::new(), Vec::new()];
    let mut owners = vec![None; layout.faces.len()];
    let mut count = 0;
    for (id, face) in layout.faces.iter().enumerate() {
        if face.is_empty() || owners[id].is_some() {
            continue;
        }
        let group = groups.get_mut(count).ok_or_else(|| bad("refined Unweld seam separates more than two regions"))?;
        let mut pending = vec![id];
        owners[id] = Some(count);
        while let Some(owner) = pending.pop() {
            group.push(owner);
            for &neighbor in &adjacency[owner] {
                if owners[neighbor].is_none() {
                    owners[neighbor] = Some(count);
                    pending.push(neighbor);
                }
            }
        }
        group.sort_unstable();
        count += 1;
    }
    if count != 2 {
        return Err(bad("the logical seam does not separate the Form into two regions"));
    }
    let mut by_start = BTreeMap::new();
    for (a, b) in selected {
        let edge = [a, b];
        let [(first, forward), (second, reverse)] = uses.get(&(a, b)).map(Vec::as_slice).unwrap_or_default() else {
            return Err(bad("select existing interior logical seam edges"));
        };
        let first_group = owners[*first].ok_or_else(|| bad("missing first Unweld region"))?;
        let second_group = owners[*second].ok_or_else(|| bad("missing second Unweld region"))?;
        if first_group == second_group {
            return Err(bad("the selected seam includes a side inside one region"));
        }
        let index = layout.paths.binary_search_by_key(&edge, |(id, _)| *id).map_err(|_| bad("missing exact logical seam path"))?;
        let (_, path) = layout.paths.get(index).filter(|(_, path)| path.len() >= 2).ok_or_else(|| bad("empty logical seam path"))?;
        let side = |owner, forward| FreeRimSide {
            edge: if forward { edge } else { [b, a] },
            owner,
            path: if forward { path.clone() } else { path.iter().rev().copied().collect() },
        };
        let mut pair = [side(*first, *forward), side(*second, *reverse)];
        if first_group == 1 {
            pair.swap(0, 1);
        }
        if by_start.insert(pair[0].edge[0], pair).is_some() {
            return Err(bad("refined Unweld requires one simple closed seam"));
        }
    }
    let (&start, _) = by_start.first_key_value().ok_or_else(|| bad("missing logical seam cycle"))?;
    let mut at = start;
    let mut rim = Vec::with_capacity(by_start.len());
    while !by_start.is_empty() {
        let pair = by_start.remove(&at).ok_or_else(|| bad("refined Unweld requires one complete seam cycle"))?;
        at = pair[0].edge[1];
        rim.push(pair);
    }
    if at != start {
        return Err(bad("the logical Unweld seam does not close"));
    }
    Ok(RimPartition { groups, rim })
}

/// A canonical logical edge on one of a rectangular leaf's four exact sides.
#[derive(Clone, Debug)]
pub struct ParameterEdge {
    pub edge: [usize; 2],
    /// Integer root coordinates oriented from the lower logical vertex ID.
    pub uv: [[usize; 2]; 2],
}
#[derive(Clone, Debug)]
pub struct ParameterLeaf {
    pub id: usize,
    pub root: usize,
    pub domain: [usize; 4],
    pub sides: [Vec<ParameterEdge>; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafCut {
    pub parent: usize,
    pub axis: usize,
    pub coordinate: usize,
}
#[derive(Clone, Debug)]
pub struct LoopPoint {
    pub root: usize,
    pub uv: [usize; 2],
    pub existing: Option<usize>,
    pub source_edge: [usize; 2],
    pub fraction: f64,
}
#[derive(Clone, Debug)]
pub struct AlignedLoop {
    pub cuts: Vec<LeafCut>,
    pub points: Vec<LoopPoint>,
    pub existing_edges: Vec<[usize; 2]>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum StationKey {
    Vertex(usize),
    Edge([usize; 2], usize, usize),
}
type StationPair = (StationKey, StationKey);
struct LoopGraph {
    leaves: Vec<ParameterLeaf>,
    bounds: BTreeMap<usize, [usize; 4]>,
    points: BTreeMap<StationKey, LoopPoint>,
    edges: BTreeMap<StationPair, Option<LeafCut>>,
    pending: BTreeSet<(usize, usize, usize, usize)>,
    visited: BTreeSet<(usize, usize, usize, usize)>,
    remaining: usize,
}
impl Hierarchy {
    /// Exact logical sides; a T-junction adds segments, never a fifth side.
    pub fn parameter_sides(&self) -> Result<Vec<ParameterLeaf>> {
        self.check_trim_wall_topology()?;
        if !self.dissolved_edges.is_empty() {
            return Err(bad("parameter-side insertion requires undissolved rectangular leaves"));
        }
        let layout = self.layout()?;
        let (locations, _, _) = self.locations()?;
        let mut reverse = BTreeMap::new();
        for ((root, u, v), vertex) in locations {
            if reverse.insert((root, vertex), [u, v]).is_some_and(|previous| previous != [u, v]) {
                return Err(bad("a source vertex has ambiguous coordinates in its root chart"));
            }
        }
        let mut result = Vec::new();
        for (id, leaf) in self.leaves.iter().enumerate().filter(|(_, leaf)| leaf.active) {
            let face = layout.faces.get(id).ok_or_else(|| bad("missing logical leaf boundary"))?;
            let mut sides: [Vec<ParameterEdge>; 4] = std::array::from_fn(|_| Vec::new());
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                let uv = [a.min(b), a.max(b)].map(|vertex| reverse.get(&(leaf.root, vertex)).copied());
                let [Some(a_uv), Some(b_uv)] = uv else { return Err(bad("logical edge has no exact root coordinate")) };
                let [u, v, x, y] = leaf.domain;
                let side = if a_uv[1] == v && b_uv[1] == v {
                    0
                } else if a_uv[0] == x && b_uv[0] == x {
                    1
                } else if a_uv[1] == y && b_uv[1] == y {
                    2
                } else if a_uv[0] == u && b_uv[0] == u {
                    3
                } else {
                    return Err(bad("logical boundary leaves its rectangular parameter side"));
                };
                sides[side].push(ParameterEdge { edge: [a.min(b), a.max(b)], uv: [a_uv, b_uv] });
            }
            if sides.iter().any(Vec::is_empty) {
                return Err(bad("logical leaf is missing a parameter side"));
            }
            result.push(ParameterLeaf { id, root: leaf.root, domain: leaf.domain, sides });
        }
        Ok(result)
    }
    /// Add precisely one dyadic seam per requested leaf, without balancing or
    /// exposing any unrelated uniform evaluator grid lines as logical edges.
    pub fn split_at(&self, cuts: &[LeafCut]) -> Result<Self> {
        self.parameter_sides()?;
        if cuts.len() > 512 {
            return Err(bad("aligned Insert Loop exceeds its leaf or cut budget"));
        }
        let mut plan = BTreeMap::new();
        for &cut in cuts {
            if cut.axis > 1 {
                return Err(bad("cut axis must be U or V"));
            }
            if plan.insert(cut.parent, cut).is_some_and(|previous| previous != cut) {
                return Err(bad("aligned Insert Loop revisits a parent with incompatible cuts"));
            }
        }
        if self.leaves.len().saturating_add(plan.len().saturating_mul(2)) > MAX_LEAVES {
            return Err(bad("aligned Insert Loop exceeds its leaf budget"));
        }
        let mut out = self.clone();
        for cut in plan.into_values() {
            let leaf = out.leaves.get_mut(cut.parent).filter(|leaf| leaf.active).ok_or_else(|| bad("select an active cut parent"))?;
            if cut.coordinate <= leaf.domain[cut.axis] || cut.coordinate >= leaf.domain[cut.axis + 2] {
                return Err(bad("cut coordinate must be strictly inside its parameter extent"));
            }
            let mut low = leaf.clone();
            let mut high = leaf.clone();
            low.domain[cut.axis + 2] = cut.coordinate;
            high.domain[cut.axis] = cut.coordinate;
            leaf.active = false;
            out.leaves.extend([low, high]);
        }
        out.update_parameter_depth()?;
        out.details.resize_with(out.depth + 1, Vec::new);
        let (locations, _, levels) = out.locations()?;
        for leaf in out.leaves.iter().filter(|leaf| leaf.active) {
            let [u, v, x, y] = leaf.domain;
            for (u, v) in [(u, v), (x, v), (x, y), (u, y)] {
                let vertex = *locations.get(&(leaf.root, u, v)).ok_or_else(|| bad("cut endpoint is outside the evaluator grid"))?;
                let birth = levels.iter().position(|&count| vertex < count).ok_or_else(|| bad("missing cut control birth"))?;
                out.controls.entry(vertex).or_insert(birth);
            }
        }
        out.layout()?;
        Ok(out)
    }
    pub fn aligned_loop_vertices(&self, points: &[LoopPoint]) -> Result<Vec<usize>> {
        if points.len() > 256 {
            return Err(bad("aligned Insert Loop exceeds 256 stations"));
        }
        let (locations, _, _) = self.locations()?;
        points
            .iter()
            .map(|point| {
                let vertex = *locations.get(&(point.root, point.uv[0], point.uv[1])).ok_or_else(|| bad("missing aligned endpoint identity"))?;
                if point.existing.is_some_and(|id| id != vertex) || !self.controls.contains_key(&vertex) {
                    return Err(bad("aligned insertion changed an existing control identity"));
                }
                Ok(vertex)
            })
            .collect()
    }

    /// Identify the actual cut sides of one completed Insert Loop transaction.
    /// The two children of each retired parent are appended in parent order.
    /// A cut can reuse either endpoint, so vertex age is immaterial.
    pub fn inserted_loop_edges(&self, before: &Self) -> Result<Vec<[usize; 2]>> {
        let prefix = self.leaves.get(..before.leaves.len()).ok_or_else(|| bad("Insert Loop discarded original leaf slots"))?;
        let mut retired = Vec::new();
        for (old, current) in before.leaves.iter().zip(prefix) {
            if old.root != current.root || old.domain != current.domain || !old.active && current.active {
                return Err(bad("Insert Loop changed an original leaf identity"));
            }
            if old.active && !current.active {
                retired.push(old);
            }
        }
        let appended = self.leaves.get(before.leaves.len()..).ok_or_else(|| bad("missing inserted loop children"))?;
        if retired.len() > 512 || appended.len() != retired.len() * 2 {
            return Err(bad("Insert Loop did not append two children per retired parent"));
        }
        if retired.is_empty() {
            return if self == before { Ok(Vec::new()) } else { Err(bad("Insert Loop changed source without adding a seam")) };
        }
        let sides: BTreeMap<_, _> = self.parameter_sides()?.into_iter().map(|leaf| (leaf.id, leaf)).collect();
        let mut seams = BTreeSet::new();
        for (index, (parent, [low, high])) in retired.into_iter().zip(appended.as_chunks::<2>().0).enumerate() {
            if !low.active || !high.active || low.root != parent.root || high.root != parent.root {
                return Err(bad("Insert Loop children lost their parent chart"));
            }
            let axis = (0..2)
                .find(|&axis| {
                    let cut = low.domain[axis + 2];
                    if cut <= parent.domain[axis] || cut >= parent.domain[axis + 2] {
                        return false;
                    }
                    let (mut a, mut b) = (parent.domain, parent.domain);
                    a[axis + 2] = cut;
                    b[axis] = cut;
                    low.domain == a && high.domain == b
                })
                .ok_or_else(|| bad("Insert Loop children do not partition their parent's parameter rectangle"))?;
            let first = before.leaves.len() + 2 * index;
            let a = sides.get(&first).ok_or_else(|| bad("missing lower inserted loop side"))?;
            let b = sides.get(&(first + 1)).ok_or_else(|| bad("missing upper inserted loop side"))?;
            let a = a.sides[axis + 1].iter().map(|segment| segment.edge).collect::<BTreeSet<_>>();
            let b = b.sides[if axis == 0 { 3 } else { 0 }].iter().map(|segment| segment.edge).collect::<BTreeSet<_>>();
            if a.is_empty() || a != b {
                return Err(bad("Insert Loop children disagree on their common seam"));
            }
            seams.extend(a);
            if seams.len() > 4096 {
                return Err(bad("Insert Loop preview exceeds its seam budget"));
            }
        }
        Ok(seams.into_iter().collect())
    }

    /// Follow an exact grid-aligned contour through coarse cuts and already
    /// present fine seams. Arbitrary non-grid fractions need a later inverse
    /// solve; they are refused rather than silently snapped to a grid station.
    pub fn aligned_loop(&self, edge: [usize; 2], fraction: f64) -> Result<AlignedLoop> {
        if !fraction.is_finite() || !(1e-6..=1.0 - 1e-6).contains(&fraction) || edge[0] == edge[1] {
            return Err(bad("insert positions must be inside a nondegenerate logical edge"));
        }
        let canonical = [edge[0].min(edge[1]), edge[0].max(edge[1])];
        let leaves = self.parameter_sides()?;
        let source = leaves
            .iter()
            .flat_map(|leaf| leaf.sides.iter().flatten().map(move |segment| (leaf, segment)))
            .find(|(_, segment)| segment.edge == canonical)
            .ok_or_else(|| bad("select a visible source logical edge"))?;
        let (leaf, segment) = source;
        let varying = usize::from(segment.uv[0][0] == segment.uv[1][0]);
        // Check the caller's direction before reducing to an integer station.
        // Complementing a rounded 2/3 would perturb the corresponding 1/3.
        let endpoints = if edge == canonical { segment.uv } else { [segment.uv[1], segment.uv[0]] };
        let [a, b] = endpoints.map(|uv| uv[varying] as f64);
        let value = a + (b - a) * fraction;
        if value.fract() != 0.0 || !(0.0..=GRID as f64).contains(&value) || ((value - a) / (b - a)) != fraction {
            return Err(bad(
                "mixed-depth Insert Loop currently needs an exact aligned dyadic station; arbitrary-fraction corridor solving is not implemented yet",
            ));
        }
        let root = leaf.root;
        let entry = segment.uv[0][1 - varying];
        self.aligned_loop_from(leaves, root, varying, value as usize, entry)
    }

    /// Start a midpoint cut in a specific leaf, including when both entry
    /// stations are existing T-controls on its transverse sides.
    pub fn aligned_loop_from_leaf(&self, face: usize, axis: usize) -> Result<AlignedLoop> {
        if axis > 1 {
            return Err(bad("cut axis must be U or V"));
        }
        let leaves = self.parameter_sides()?;
        let leaf = leaves.iter().find(|leaf| leaf.id == face).ok_or_else(|| bad("select an active rectangular logical face"))?;
        let extent = leaf.domain[axis + 2] - leaf.domain[axis];
        if extent < 2 || !extent.is_multiple_of(2) {
            return Err(bad("the requested midpoint is outside the aligned dyadic depth budget"));
        }
        let coordinate = leaf.domain[axis] + extent / 2;
        let (root, entry) = (leaf.root, leaf.domain[1 - axis]);
        let plan = self.aligned_loop_from(leaves, root, axis, coordinate, entry)?;
        if !plan.cuts.contains(&LeafCut { parent: face, axis, coordinate }) {
            return Err(bad("the aligned contour did not cross its requested leaf"));
        }
        Ok(plan)
    }

    fn aligned_loop_from(&self, leaves: Vec<ParameterLeaf>, root: usize, axis: usize, coordinate: usize, entry: usize) -> Result<AlignedLoop> {
        let mut uv = [entry; 2];
        uv[axis] = coordinate;
        let mut graph = LoopGraph {
            leaves,
            bounds: BTreeMap::new(),
            points: BTreeMap::new(),
            edges: BTreeMap::new(),
            pending: BTreeSet::from([(root, axis, coordinate, entry)]),
            visited: BTreeSet::new(),
            remaining: 4_000_000,
        };
        for leaf in &self.leaves {
            graph
                .bounds
                .entry(leaf.root)
                .and_modify(|bounds| {
                    bounds[0] = bounds[0].min(leaf.domain[0]);
                    bounds[1] = bounds[1].min(leaf.domain[1]);
                    bounds[2] = bounds[2].max(leaf.domain[2]);
                    bounds[3] = bounds[3].max(leaf.domain[3]);
                })
                .or_insert(leaf.domain);
        }
        let seed = graph.station(root, uv)?;
        while let Some(state) = graph.pending.pop_first() {
            if graph.visited.insert(state) {
                if graph.visited.len() > 512 {
                    return Err(bad("aligned Insert Loop exceeds its root-state budget"));
                }
                graph.expand(state)?;
            }
        }
        let mut adjacency = BTreeMap::<StationKey, BTreeSet<StationKey>>::new();
        for (a, b) in graph.edges.keys() {
            adjacency.entry(a.clone()).or_default().insert(b.clone());
            adjacency.entry(b.clone()).or_default().insert(a.clone());
        }
        let mut component = BTreeSet::new();
        let mut pending = BTreeSet::from([seed]);
        while let Some(point) = pending.pop_first() {
            if !component.insert(point.clone()) {
                continue;
            }
            if component.len() > 256 {
                return Err(bad("aligned Insert Loop exceeds 256 stations"));
            }
            let adjacent = adjacency.get(&point).ok_or_else(|| bad("aligned station has no seam continuation"))?;
            if adjacent.len() > 2 {
                return Err(bad("aligned Insert Loop has branching continuation at a source pole"));
            }
            pending.extend(adjacent.iter().filter(|point| !component.contains(*point)).cloned());
        }
        let mut cuts = BTreeMap::new();
        let mut existing_edges = BTreeSet::new();
        for ((a, b), cut) in &graph.edges {
            if !component.contains(a) || !component.contains(b) {
                continue;
            }
            if let Some(cut) = cut {
                if cuts.insert(cut.parent, *cut).is_some_and(|previous| previous != *cut) {
                    return Err(bad("aligned Insert Loop crosses one leaf more than once"));
                }
            } else if let (StationKey::Vertex(a), StationKey::Vertex(b)) = (a, b) {
                existing_edges.insert([(*a).min(*b), (*a).max(*b)]);
            }
        }
        let points = component
            .iter()
            .map(|key| graph.points.get(key).cloned().ok_or_else(|| bad("missing aligned station provenance")))
            .collect::<Result<Vec<_>>>()?;
        Ok(AlignedLoop { cuts: cuts.into_values().collect(), points, existing_edges: existing_edges.into_iter().collect() })
    }
}
impl LoopGraph {
    fn spend(&mut self, work: usize) -> Result<()> {
        self.remaining = self.remaining.checked_sub(work).ok_or_else(|| bad("aligned Insert Loop exceeds its shared traversal budget"))?;
        Ok(())
    }
    fn station(&mut self, root: usize, uv: [usize; 2]) -> Result<StationKey> {
        let work = self
            .leaves
            .iter()
            .filter(|leaf| leaf.root == root)
            .try_fold(self.leaves.len() * 2, |n, leaf| n.checked_add(leaf.sides.iter().map(Vec::len).sum::<usize>() * 2))
            .ok_or_else(|| bad("source side scan size overflowed"))?;
        self.spend(work)?;
        let mut found = None;
        for leaf in self.leaves.iter().filter(|leaf| leaf.root == root) {
            for segment in leaf.sides.iter().flatten() {
                let varying = usize::from(segment.uv[0][0] == segment.uv[1][0]);
                let fixed = 1 - varying;
                if uv[fixed] != segment.uv[0][fixed]
                    || uv[varying] < segment.uv[0][varying].min(segment.uv[1][varying])
                    || uv[varying] > segment.uv[0][varying].max(segment.uv[1][varying])
                {
                    continue;
                }
                let offset = uv[varying].abs_diff(segment.uv[0][varying]);
                let span = segment.uv[0][varying].abs_diff(segment.uv[1][varying]);
                if span == 0 {
                    return Err(bad("degenerate source parameter segment"));
                }
                let key = if offset == 0 {
                    StationKey::Vertex(segment.edge[0])
                } else if offset == span {
                    StationKey::Vertex(segment.edge[1])
                } else {
                    let mut a = offset;
                    let mut b = span;
                    while b != 0 {
                        let r = a % b;
                        a = b;
                        b = r;
                    }
                    StationKey::Edge(segment.edge, offset / a, span / a)
                };
                let point = LoopPoint {
                    root,
                    uv,
                    existing: match &key {
                        StationKey::Vertex(id) => Some(*id),
                        _ => None,
                    },
                    source_edge: segment.edge,
                    fraction: offset as f64 / span as f64,
                };
                if found.as_ref().is_some_and(|(other, _)| other != &key) {
                    return Err(bad("parameter side station has conflicting source identities"));
                }
                found = Some((key, point));
            }
        }
        let (key, point) = found.ok_or_else(|| bad("aligned station is not on an original logical boundary"))?;
        if self.points.len() >= 4096 && !self.points.contains_key(&key) {
            return Err(bad("aligned Insert Loop exceeds its station traversal budget"));
        }
        self.points.entry(key.clone()).or_insert(point);
        Ok(key)
    }
    fn connect(&mut self, a: StationKey, b: StationKey, cut: Option<LeafCut>) -> Result<()> {
        if a == b {
            return Err(bad("aligned seam has coincident endpoint identities"));
        }
        let pair = if a < b { (a, b) } else { (b, a) };
        if self.edges.len() >= 4096 && !self.edges.contains_key(&pair) {
            return Err(bad("aligned Insert Loop exceeds its seam traversal budget"));
        }
        if self.edges.insert(pair, cut).is_some_and(|previous| previous != cut) {
            return Err(bad("aligned seam has ambiguous source ownership"));
        }
        Ok(())
    }
    fn expand(&mut self, (root, axis, coordinate, entry): (usize, usize, usize, usize)) -> Result<()> {
        let bounds = *self.bounds.get(&root).ok_or_else(|| bad("missing root parameter extent"))?;
        if coordinate <= bounds[axis] || coordinate >= bounds[axis + 2] {
            return Err(bad("aligned continuation reaches an ambiguous root corner"));
        }
        let work = self
            .leaves
            .iter()
            .filter(|leaf| leaf.root == root)
            .try_fold(self.leaves.len() * 4, |n, leaf| n.checked_add(4 + leaf.sides.iter().map(Vec::len).sum::<usize>() * 2))
            .ok_or_else(|| bad("source root scan overflowed"))?;
        self.spend(work)?;
        let varying = 1 - axis;
        let mut intervals: Vec<_> = self
            .leaves
            .iter()
            .filter(|leaf| leaf.root == root && coordinate >= leaf.domain[axis] && coordinate <= leaf.domain[axis + 2])
            .map(|leaf| [leaf.domain[varying], leaf.domain[varying + 2]])
            .collect();
        let sort_work = intervals.len().saturating_mul((usize::BITS - intervals.len().max(1).leading_zeros()) as usize);
        self.spend(sort_work)?;
        intervals.sort_unstable();
        let mut merged: Vec<[usize; 2]> = Vec::new();
        for interval in intervals {
            if let Some(last) = merged.last_mut().filter(|last| last[1] >= interval[0]) {
                last[1] = last[1].max(interval[1]);
            } else {
                merged.push(interval);
            }
        }
        let extent = merged
            .iter()
            .find(|extent| entry >= extent[0] && entry <= extent[1])
            .copied()
            .ok_or_else(|| bad("aligned entry has no active continuation"))?;
        let leaves: Vec<_> = self
            .leaves
            .iter()
            .filter(|leaf| leaf.root == root && leaf.domain[varying] >= extent[0] && leaf.domain[varying + 2] <= extent[1])
            .cloned()
            .collect();
        let mut transitions = BTreeSet::new();
        for leaf in leaves {
            self.spend(1)?;
            let low = leaf.domain[axis];
            let high = leaf.domain[axis + 2];
            let varying = 1 - axis;
            if coordinate < low || coordinate > high {
                continue;
            }
            if coordinate > low && coordinate < high {
                let mut a = [0; 2];
                a[axis] = coordinate;
                a[varying] = leaf.domain[varying];
                let mut b = a;
                b[varying] = leaf.domain[varying + 2];
                let first = self.station(root, a)?;
                let second = self.station(root, b)?;
                self.connect(first.clone(), second.clone(), Some(LeafCut { parent: leaf.id, axis, coordinate }))?;
                transitions.insert((a, first));
                transitions.insert((b, second));
            } else {
                for segment in leaf.sides.iter().flatten().filter(|segment| segment.uv[0][axis] == coordinate && segment.uv[1][axis] == coordinate) {
                    self.spend(1)?;
                    let a = self.station(root, segment.uv[0])?;
                    let b = self.station(root, segment.uv[1])?;
                    self.connect(a.clone(), b.clone(), None)?;
                    transitions.insert((segment.uv[0], a));
                    transitions.insert((segment.uv[1], b));
                }
            }
        }
        for (uv, key) in transitions {
            let varying = 1 - axis;
            if uv[varying] != bounds[varying] && uv[varying] != bounds[varying + 2] {
                continue;
            }
            self.transition(root, &key)?;
        }
        Ok(())
    }
    fn transition(&mut self, root: usize, key: &StationKey) -> Result<()> {
        let work = self
            .leaves
            .iter()
            .filter(|leaf| leaf.root != root)
            .try_fold(self.leaves.len() * 2, |n, leaf| n.checked_add(leaf.sides.iter().map(Vec::len).sum::<usize>() * 2))
            .ok_or_else(|| bad("adjacent chart scan overflowed"))?;
        self.spend(work)?;
        let mut next = BTreeSet::new();
        for leaf in self.leaves.iter().filter(|leaf| leaf.root != root) {
            for segment in leaf.sides.iter().flatten() {
                let uv = match key {
                    StationKey::Vertex(id) => {
                        let Some(i) = segment.edge.iter().position(|v| v == id) else {
                            continue;
                        };
                        segment.uv[i]
                    }
                    StationKey::Edge(edge, n, d) => {
                        if segment.edge != *edge {
                            continue;
                        }
                        let mut uv = [0; 2];
                        for (axis, slot) in uv.iter_mut().enumerate() {
                            let a = segment.uv[0][axis] as i64;
                            let delta = segment.uv[1][axis] as i64 - a;
                            let scaled = delta * (*n as i64);
                            if scaled % (*d as i64) != 0 {
                                return Err(bad("aligned station exceeds the root dyadic depth budget"));
                            }
                            let value = a + scaled / (*d as i64);
                            if !(0..=GRID as i64).contains(&value) {
                                return Err(bad("invalid adjacent source chart station"));
                            }
                            *slot = value as usize;
                        }
                        uv
                    }
                };
                let bounds = self.bounds.get(&leaf.root).ok_or_else(|| bad("missing adjacent root extent"))?;
                let boundary = [uv[0] == bounds[0] || uv[0] == bounds[2], uv[1] == bounds[1] || uv[1] == bounds[3]];
                let axis = match boundary {
                    [true, false] => 1,
                    [false, true] => 0,
                    [true, true] => return Err(bad("aligned continuation reaches an ambiguous root corner")),
                    [false, false] => continue,
                };
                next.insert((leaf.root, axis, uv[axis], uv[1 - axis]));
            }
        }
        if next.len() > 1 {
            return Err(bad("aligned source boundary has branching root continuation"));
        }
        self.pending.extend(next);
        Ok(())
    }
}
