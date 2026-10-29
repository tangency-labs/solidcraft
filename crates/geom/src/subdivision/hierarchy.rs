//! Local rectangular leaves over nested uniform Catmull-Clark control levels.
//! Logical T-junctions never become polygon faces in the subdivision evaluator.
use super::{Cage, Result, SurfacePreview, bad, key};
use crate::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod fitting;
mod topology;

pub use fitting::Region;

pub const MAX_DEPTH: usize = 4;
const GRID: usize = 1 << MAX_DEPTH;
const MAX_LEAVES: usize = 10_000;
type Tag = (usize, [[usize; 2]; 4]);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Leaf {
    pub root: usize,
    pub domain: [usize; 4],
    pub active: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detail {
    pub vertex: usize,
    pub delta: Vec3,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hierarchy {
    pub base: Cage,
    pub depth: usize,
    pub leaves: Vec<Leaf>,
    /// Sparse additive details at each level, including coarse level zero.
    pub details: Vec<Vec<Detail>>,
    /// Evaluator vertex index -> the level at which this selectable control is edited.
    pub controls: BTreeMap<usize, usize>,
    /// Logical seams removed without changing the subdivision evaluator.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dissolved_edges: Vec<[usize; 2]>,
}
pub struct Layout {
    /// Retired leaf slots stay empty, so surviving IDs never change.
    pub faces: Vec<Vec<usize>>,
    /// Deleted regions leave their evaluator cells intact but have no output owner.
    pub owners: Vec<Option<usize>>,
    pub paths: Vec<([usize; 2], Vec<usize>)>,
}

impl Hierarchy {
    pub fn new(base: Cage) -> Result<Self> {
        base.validate()?;
        if base.faces.iter().any(|f| f.len() != 4) {
            return Err(bad("local refinement requires quad base patches"));
        }
        let leaves = (0..base.faces.len()).map(|root| Leaf { root, domain: [0, 0, GRID, GRID], active: true }).collect();
        let controls = (0..base.points.len()).map(|v| (v, 0)).collect();
        Ok(Self { base, depth: 0, leaves, details: vec![Vec::new()], controls, dissolved_edges: Vec::new() })
    }
    pub fn evaluate(&self) -> Result<Cage> {
        self.base.validate()?;
        if self.base.faces.iter().any(|f| f.len() != 4) {
            return Err(bad("hierarchical base patches must be quads"));
        }
        if self.depth > MAX_DEPTH || self.details.len() != self.depth + 1 || self.leaves.len() > MAX_LEAVES {
            return Err(bad("hierarchical refinement budget exceeded"));
        }
        let mut cage = self.base.clone();
        for (level, details) in self.details.iter().enumerate() {
            if details.len() > cage.points.len() {
                return Err(bad("hierarchical detail budget exceeded"));
            }
            let mut seen = BTreeSet::new();
            for detail in details {
                if !seen.insert(detail.vertex) || !detail.delta.is_finite() {
                    return Err(bad("invalid hierarchical detail"));
                }
                *cage.points.get_mut(detail.vertex).ok_or_else(|| bad("invalid hierarchical control"))? += detail.delta;
            }
            cage.validate()?;
            if level < self.depth {
                cage = cage.refined(1)?;
            }
        }
        Ok(cage)
    }
    /// The linear influence of an edit at its original coarse/detail level.
    pub fn basis(&self, vertex: usize) -> Result<Cage> {
        let birth = *self.controls.get(&vertex).ok_or_else(|| bad("vertex is not a selectable hierarchical control"))?;
        if birth > self.depth {
            return Err(bad("invalid hierarchical control level"));
        }
        let mut cage = self.base.clone();
        cage.points.fill(Vec3::ZERO);
        for level in 0..=self.depth {
            if level == birth {
                cage.points.get_mut(vertex).ok_or_else(|| bad("invalid hierarchical control"))?.x = 1.0;
            }
            if level < self.depth {
                cage = cage.refined(1)?;
            }
        }
        Ok(cage)
    }
    pub fn add_deltas(&mut self, vertices: &[usize], deltas: &[Vec3]) -> Result<()> {
        if vertices.len() != deltas.len() || vertices.len() > 256 {
            return Err(bad("hierarchical edits require at most 256 matching controls"));
        }
        for (&vertex, &delta) in vertices.iter().zip(deltas) {
            let birth = *self.controls.get(&vertex).ok_or_else(|| bad("invalid hierarchical selection"))?;
            let details = self.details.get_mut(birth).ok_or_else(|| bad("invalid detail level"))?;
            if let Some(detail) = details.iter_mut().find(|d| d.vertex == vertex) {
                detail.delta += delta;
            } else {
                details.push(Detail { vertex, delta });
            }
        }
        Ok(())
    }
    fn tags(&self) -> Result<(Cage, Vec<Tag>, Vec<usize>)> {
        let mut cage = self.base.clone();
        let mut counts = vec![cage.points.len()];
        let mut tags: Vec<_> = (0..cage.faces.len()).map(|f| (f, [[0, 0], [GRID, 0], [GRID, GRID], [0, GRID]])).collect();
        for _ in 0..self.depth {
            let mut next = Vec::with_capacity(tags.len() * 4);
            for (root, uv) in tags {
                let center = [(uv[0][0] + uv[2][0]) / 2, (uv[0][1] + uv[2][1]) / 2];
                for i in 0..4 {
                    let a = uv[i];
                    let b = uv[(i + 1) % 4];
                    let c = uv[(i + 3) % 4];
                    next.push((root, [a, [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], center, [(a[0] + c[0]) / 2, (a[1] + c[1]) / 2]]));
                }
            }
            cage = cage.refined(1)?;
            counts.push(cage.points.len());
            tags = next;
        }
        Ok((cage, tags, counts))
    }
    fn locations(&self) -> Result<(BTreeMap<(usize, usize, usize), usize>, Vec<Tag>, Vec<usize>)> {
        let (cage, tags, counts) = self.tags()?;
        let mut locations = BTreeMap::new();
        for (face, (root, uv)) in cage.faces.iter().zip(&tags) {
            for (&vertex, uv) in face.iter().zip(uv) {
                locations.insert((*root, uv[0], uv[1]), vertex);
            }
        }
        Ok((locations, tags, counts))
    }
    /// Split selected leaves independently in U/V. Counts must be powers of two.
    pub fn subdivide(&self, selected: &[usize], counts: [usize; 2]) -> Result<Self> {
        self.layout()?;
        if !self.dissolved_edges.is_empty() {
            return Err(bad("subdivision of dissolved logical regions requires a new parameter decomposition"));
        }
        if counts.iter().any(|&n| n == 0 || n > GRID || !n.is_power_of_two()) || counts == [1, 1] {
            return Err(bad("local subdivision counts must be powers of two in 1..16"));
        }
        let unique: BTreeSet<_> = selected.iter().copied().collect();
        if unique.is_empty()
            || unique.len() != selected.len()
            || selected.len().saturating_mul(counts[0] * counts[1]) + self.leaves.len() > MAX_LEAVES
        {
            return Err(bad("select distinct active leaves within the refinement budget"));
        }
        let mut out = self.clone();
        for &id in selected {
            let leaf = out.leaves.get_mut(id).filter(|l| l.active).ok_or_else(|| bad("select an active logical face"))?;
            let [u, v, x, y] = leaf.domain;
            let du = (x - u) / counts[0];
            let dv = (y - v) / counts[1];
            if du == 0 || dv == 0 {
                return Err(bad("local refinement depth budget exceeded"));
            }
            let root = leaf.root;
            leaf.active = false;
            for j in 0..counts[1] {
                for i in 0..counts[0] {
                    out.leaves.push(Leaf { root, domain: [u + i * du, v + j * dv, u + (i + 1) * du, v + (j + 1) * dv], active: true });
                }
            }
        }
        let spacing = out
            .leaves
            .iter()
            .filter(|l| l.active)
            .flat_map(|l| [l.domain[2] - l.domain[0], l.domain[3] - l.domain[1]])
            .min()
            .ok_or_else(|| bad("empty hierarchy"))?;
        out.depth = self.depth.max((GRID / spacing).ilog2() as usize);
        if out.depth > MAX_DEPTH {
            return Err(bad("local refinement depth budget exceeded"));
        }
        out.details.resize_with(out.depth + 1, Vec::new);
        let (locations, _, levels) = out.locations()?;
        for leaf in out.leaves.iter().filter(|l| l.active) {
            let [u, v, x, y] = leaf.domain;
            for (u, v) in [(u, v), (x, v), (x, y), (u, y)] {
                let vertex = *locations.get(&(leaf.root, u, v)).ok_or_else(|| bad("missing inserted control"))?;
                let level = levels.iter().position(|&n| vertex < n).ok_or_else(|| bad("missing control level"))?;
                out.controls.entry(vertex).or_insert(level);
            }
        }
        out.layout()?;
        Ok(out)
    }
    pub fn insert_target(&self, first: [usize; 2], second: [usize; 2]) -> Result<(usize, usize)> {
        let layout = self.layout()?;
        let (locations, _, _) = self.locations()?;
        let first = key(first[0], first[1]);
        let second = key(second[0], second[1]);
        let mut found = Vec::new();
        for (id, face) in layout.faces.iter().enumerate().filter(|(_, f)| !f.is_empty()) {
            let edges: BTreeSet<_> =
                face.iter().copied().zip(face.iter().copied().cycle().skip(1)).take(face.len()).map(|(a, b)| key(a, b)).collect();
            if !edges.contains(&first) || !edges.contains(&second) {
                continue;
            }
            let leaf = &self.leaves[id];
            let side = |edge: (usize, usize)| -> Option<usize> {
                let a = locations.iter().find_map(|(&(root, u, v), &i)| (root == leaf.root && i == edge.0).then_some([u, v]))?;
                let b = locations.iter().find_map(|(&(root, u, v), &i)| (root == leaf.root && i == edge.1).then_some([u, v]))?;
                let [u, v, x, y] = leaf.domain;
                if a[1] == v && b[1] == v {
                    Some(0)
                } else if a[0] == x && b[0] == x {
                    Some(1)
                } else if a[1] == y && b[1] == y {
                    Some(2)
                } else if a[0] == u && b[0] == u {
                    Some(3)
                } else {
                    None
                }
            };
            if let (Some(a), Some(b)) = (side(first), side(second))
                && (a + 2) % 4 == b
            {
                found.push((id, a % 2));
            }
        }
        if found.len() != 1 {
            return Err(bad("Insert Point needs opposite edges of one unambiguous logical face"));
        }
        Ok(found[0])
    }
    pub fn seam_endpoints(&self, parent: usize, axis: usize) -> Result<[usize; 2]> {
        let leaf = self.leaves.get(parent).ok_or_else(|| bad("missing insertion parent"))?;
        let [u, v, x, y] = leaf.domain;
        let uv = if axis == 0 {
            [[(u + x) / 2, v], [(u + x) / 2, y]]
        } else if axis == 1 {
            [[u, (v + y) / 2], [x, (v + y) / 2]]
        } else {
            return Err(bad("insert axis must be U or V"));
        };
        let (locations, _, _) = self.locations()?;
        Ok([
            *locations.get(&(leaf.root, uv[0][0], uv[0][1])).ok_or_else(|| bad("missing inserted endpoint"))?,
            *locations.get(&(leaf.root, uv[1][0], uv[1][1])).ok_or_else(|| bad("missing inserted endpoint"))?,
        ])
    }
    pub fn layout(&self) -> Result<Layout> {
        topology::dissolved_layout(self.raw_layout()?, &self.dissolved_edges)
    }
    fn raw_layout(&self) -> Result<Layout> {
        if self.depth > MAX_DEPTH || self.leaves.is_empty() || self.leaves.len() > MAX_LEAVES || !self.leaves.iter().any(|l| l.active) {
            return Err(bad("invalid hierarchical topology budget"));
        }
        let (locations, tags, levels) = self.locations()?;
        if self.controls.len() > *levels.last().ok_or_else(|| bad("missing hierarchy level"))?
            || self.controls.iter().any(|(&v, &level)| levels.get(level).is_none_or(|&n| v >= n))
        {
            return Err(bad("invalid selectable hierarchical controls"));
        }
        let mut exposed = vec![BTreeSet::new(); self.base.faces.len()];
        for leaf in self.leaves.iter().filter(|l| l.active) {
            let [u, v, x, y] = leaf.domain;
            if leaf.root >= self.base.faces.len() || u >= x || v >= y || x > GRID || y > GRID {
                return Err(bad("invalid leaf parameter domain"));
            }
            for (u, v) in [(u, v), (x, v), (x, y), (u, y)] {
                let vertex = *locations.get(&(leaf.root, u, v)).ok_or_else(|| bad("leaf corner is outside the evaluator grid"))?;
                // Share T-junctions across base-patch boundaries by evaluator identity.
                exposed[leaf.root].insert((u, v, vertex));
            }
        }
        let all_ids: BTreeSet<_> = exposed.iter().flatten().map(|&(_, _, v)| v).collect();
        for (&(root, u, v), &vertex) in &locations {
            if all_ids.contains(&vertex) {
                exposed[root].insert((u, v, vertex));
            }
        }
        let mut faces = Vec::with_capacity(self.leaves.len());
        let mut paths = BTreeMap::new();
        let step = GRID >> self.depth;
        for leaf in &self.leaves {
            if !leaf.active {
                faces.push(Vec::new());
                continue;
            }
            let [u, v, x, y] = leaf.domain;
            let mut face = Vec::new();
            for (a, b) in [([u, v], [x, v]), ([x, v], [x, y]), ([x, y], [u, y]), ([u, y], [u, v])] {
                let axis = usize::from(a[0] == b[0]);
                let fixed = 1 - axis;
                let mut nodes: Vec<_> = exposed[leaf.root]
                    .iter()
                    .copied()
                    .filter(|&(s, t, _)| {
                        let uv = [s, t];
                        uv[fixed] == a[fixed] && uv[axis] >= a[axis].min(b[axis]) && uv[axis] <= a[axis].max(b[axis])
                    })
                    .collect();
                nodes.sort_by_key(|&(s, t, _)| [s, t][axis]);
                if a[axis] > b[axis] {
                    nodes.reverse();
                }
                for pair in nodes.windows(2) {
                    let (s, t, first) = pair[0];
                    let (p, q, last) = pair[1];
                    face.push(first);
                    let start = [s, t];
                    let end = [p, q];
                    let n = start[axis].abs_diff(end[axis]) / step;
                    let mut path = Vec::with_capacity(n + 1);
                    for i in 0..=n {
                        let mut uv = start;
                        uv[axis] = if end[axis] > start[axis] { start[axis] + i * step } else { start[axis] - i * step };
                        path.push(*locations.get(&(leaf.root, uv[0], uv[1])).ok_or_else(|| bad("missing logical boundary point"))?);
                    }
                    if first > last {
                        path.reverse();
                    }
                    paths.insert(key(first, last), path);
                }
            }
            if face.len() < 4 || face.iter().any(|v| !self.controls.contains_key(v)) {
                return Err(bad("logical leaf lost a selectable corner"));
            }
            faces.push(face);
        }
        let mut by_root = vec![Vec::new(); self.base.faces.len()];
        for (id, leaf) in self.leaves.iter().enumerate().filter(|(_, l)| l.active) {
            by_root[leaf.root].push((id, leaf));
        }
        let mut owners = Vec::with_capacity(tags.len());
        for (root, uv) in tags {
            let center = [uv.iter().map(|p| p[0]).sum::<usize>(), uv.iter().map(|p| p[1]).sum::<usize>()];
            let matches: Vec<_> = by_root[root]
                .iter()
                .filter(|(_, l)| {
                    l.root == root
                        && center[0] > 4 * l.domain[0]
                        && center[0] < 4 * l.domain[2]
                        && center[1] > 4 * l.domain[1]
                        && center[1] < 4 * l.domain[3]
                })
                .map(|(i, _)| *i)
                .collect();
            if matches.len() > 1 {
                return Err(bad("logical leaves overlap"));
            }
            owners.push(matches.first().copied());
        }
        Ok(Layout { faces, owners, paths: paths.into_iter().map(|((a, b), p)| ([a, b], p)).collect() })
    }
    pub fn surface(&self, cage: &Cage, level: usize) -> Result<(SurfacePreview, Cage)> {
        let layout = self.layout()?;
        let mut surface = cage.surface(level)?;
        let mut triangles = Vec::with_capacity(surface.mesh.triangles.len());
        let mut faces = Vec::with_capacity(surface.mesh.tri_face.len());
        for (triangle, &id) in surface.mesh.triangles.iter().zip(&surface.mesh.tri_face) {
            if let Some(owner) = *layout.owners.get(id as usize).ok_or_else(|| bad("missing leaf owner"))? {
                triangles.push(*triangle);
                faces.push(owner as u32);
            }
        }
        surface.mesh.triangles = triangles;
        surface.mesh.tri_face = faces;
        surface.index = crate::MeshIndex::new(&surface.mesh);
        let curves: BTreeMap<_, _> = surface.boundaries.into_iter().map(|(edge, path)| (key(edge[0], edge[1]), (edge, path))).collect();
        surface.boundaries = layout
            .paths
            .into_iter()
            .map(|(edge, path)| {
                let mut curve = Vec::new();
                for pair in path.windows(2) {
                    let (source, points) = curves.get(&key(pair[0], pair[1])).ok_or_else(|| bad("missing evaluator curve"))?;
                    let points: Vec<_> = if source[0] == pair[0] { points.clone() } else { points.iter().rev().copied().collect() };
                    curve.extend(points.into_iter().skip(usize::from(!curve.is_empty())));
                }
                Ok((edge, curve))
            })
            .collect::<Result<_>>()?;
        let logical = Cage { points: cage.points.clone(), faces: layout.faces, ..Default::default() };
        Ok((surface, logical))
    }
}
