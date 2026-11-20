//! Local rectangular leaves over nested uniform Catmull-Clark control levels.
//! Logical T-junctions never become polygon faces in the subdivision evaluator.
use super::{Cage, EdgeCrease, Result, SurfacePreview, VertexCrease, bad, key};
use crate::prelude::*;
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use std::collections::{BTreeMap, BTreeSet};

mod extrusion;
mod fill;
mod fitting;
mod topology;
mod trim_attachment;
mod trim_wall;

pub use fill::{FreeRimSide, QuadCapRim, QuadCapSide};
pub use fitting::{FaceFrameSite, Region};
pub use topology::{AlignedLoop, BoundaryCollapse, LeafCut, LoopPoint, ParameterEdge, ParameterLeaf, UnweldSplit};
pub use trim_attachment::TrimAttachment;
pub use trim_wall::TrimWall;

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
pub struct LevelSharpness {
    pub level: usize,
    pub edges: Vec<EdgeCrease>,
    pub vertices: Vec<VertexCrease>,
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
    #[serde(deserialize_with = "read_control_levels")]
    pub controls: BTreeMap<usize, usize>,
    /// Absolute crease overrides at their authoring level. Zero removes an inherited crease.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sharpness: Vec<LevelSharpness>,
    /// Logical seams removed without changing the subdivision evaluator.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dissolved_edges: Vec<[usize; 2]>,
    /// Topology operations replayed in authoring order at their original levels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    extrusions: Vec<extrusion::LevelExtrusion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trim_wall: Option<TrimWall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quad_cap: Option<fill::QuadCap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "fill::read_additional_quad_caps")]
    additional_quad_caps: Vec<fill::QuadCap>,
    /// Permanently empty face slots left by explicit cap removal.
    #[serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "fill::read_retired_quad_caps")]
    retired_quad_caps: Vec<usize>,
    /// Ownership retired by a split. Its original corners still delimit
    /// surviving logical sides, and its evaluator stencils stay intact.
    #[serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "topology::read_detached_leaves")]
    detached_leaves: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boundary_collapse: Option<topology::RetainedBoundaryCollapse>,
}

// The design JSON has always stored control indices as object keys. Read
// those strings explicitly because an internally tagged feature buffers them
// as strings before deserializing its Form definition.
fn read_control_levels<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<BTreeMap<usize, usize>, D::Error> {
    struct Controls;
    impl<'de> serde::de::Visitor<'de> for Controls {
        type Value = BTreeMap<usize, usize>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a map of Form control indices to their authoring levels")
        }

        fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> std::result::Result<Self::Value, M::Error> {
            use serde::de::Error;
            let mut controls = BTreeMap::new();
            while let Some((key, level)) = map.next_entry::<String, usize>()? {
                let vertex = key
                    .parse::<usize>()
                    .ok()
                    .filter(|id| *id < super::MAX_EVAL_POINTS && id.to_string() == key)
                    .ok_or_else(|| M::Error::custom("invalid Form control index"))?;
                if level > MAX_DEPTH || controls.insert(vertex, level).is_some() {
                    return Err(M::Error::custom("invalid or duplicate Form control level"));
                }
            }
            Ok(controls)
        }
    }
    deserializer.deserialize_map(Controls)
}

pub struct Layout {
    /// Retired leaf slots stay empty, so surviving IDs never change.
    pub faces: Vec<Vec<usize>>,
    /// Deleted regions leave their evaluator cells intact but have no output owner.
    pub owners: Vec<Option<usize>>,
    pub paths: Vec<([usize; 2], Vec<usize>)>,
    pub collapse: Option<BoundaryCollapse>,
}

impl Layout {
    pub fn visible_edges(&self) -> impl Iterator<Item = [usize; 2]> + '_ {
        self.paths.iter().map(|(edge, _)| *edge).chain(self.collapse.into_iter().flat_map(BoundaryCollapse::diagonal_edges))
    }
}

impl Hierarchy {
    pub fn new(base: Cage) -> Result<Self> {
        base.validate()?;
        if base.faces.iter().any(|f| f.len() != 4) {
            return Err(bad("local refinement requires quad base patches"));
        }
        let leaves = (0..base.faces.len()).map(|root| Leaf { root, domain: [0, 0, GRID, GRID], active: true }).collect();
        let controls = (0..base.points.len()).map(|v| (v, 0)).collect();
        Ok(Self {
            base,
            depth: 0,
            leaves,
            details: vec![Vec::new()],
            controls,
            sharpness: Vec::new(),
            dissolved_edges: Vec::new(),
            extrusions: Vec::new(),
            trim_wall: None,
            quad_cap: None,
            additional_quad_caps: Vec::new(),
            retired_quad_caps: Vec::new(),
            detached_leaves: Vec::new(),
            boundary_collapse: None,
        })
    }
    pub fn evaluate(&self) -> Result<Cage> {
        self.evaluate_at_depth(self.depth)
    }
    /// Upper bound on evaluator faces at any step, before allocating or
    /// replaying. Every subdivision replaces one quad with four, and each
    /// extrusion only appends walls.
    pub fn replay_face_bound(&self) -> Option<usize> {
        self.validate_extrusion_levels().ok()?;
        let mut faces = self.base.faces.len();
        for level in 0..=self.depth {
            if level != 0 {
                faces = faces.checked_mul(4)?;
            }
            for operation in self.extrusions_at(level) {
                faces = faces.checked_add(operation.wall_count_bound()?)?;
            }
        }
        Some(faces)
    }
    /// Replay the authored coefficients and topology through one stored level.
    pub fn evaluate_at_depth(&self, depth: usize) -> Result<Cage> {
        self.base.validate()?;
        if self.base.faces.iter().any(|f| f.len() != 4) {
            return Err(bad("hierarchical base patches must be quads"));
        }
        if self.depth > MAX_DEPTH || depth > self.depth || self.details.len() != self.depth + 1 || self.leaves.len() > MAX_LEAVES {
            return Err(bad("hierarchical refinement budget exceeded"));
        }
        self.validate_quad_cap_slots()?;
        self.validate_detached_leaves()?;
        if self.sharpness.len() > self.depth + 1
            || self.sharpness.iter().any(|s| s.level > self.depth)
            || self.sharpness.windows(2).any(|s| s[0].level >= s[1].level)
        {
            return Err(bad("invalid hierarchical crease levels"));
        }
        self.validate_extrusion_levels()?;
        let mut cage = self.base.clone();
        for (level, details) in self.details.iter().enumerate().take(depth + 1) {
            Self::apply_details(details, &mut cage)?;
            Self::apply_sharpness(self.sharpness.iter().find(|s| s.level == level), &mut cage)?;
            for operation in self.extrusions_at(level) {
                cage = operation.replay(&cage, false)?.cage;
                Self::apply_details(&operation.details, &mut cage)?;
                Self::apply_sharpness(operation.sharpness.as_ref(), &mut cage)?;
            }
            if level < depth {
                cage = cage.refined(1)?;
            }
        }
        Ok(cage)
    }
    fn apply_details(details: &[Detail], cage: &mut Cage) -> Result<()> {
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
        cage.validate()
    }
    fn apply_sharpness(sharpness: Option<&LevelSharpness>, cage: &mut Cage) -> Result<()> {
        let Some(sharpness) = sharpness else { return Ok(()) };
        let edge_keys = cage.edges()?.into_iter().map(|e| key(e[0], e[1])).collect::<BTreeSet<_>>();
        let valid = |s: f64| s.is_finite() && (0.0..=10.0).contains(&s);
        let mut edges = BTreeMap::new();
        let mut vertices = BTreeMap::new();
        for c in &sharpness.edges {
            let e = key(c.edge[0], c.edge[1]);
            if !edge_keys.contains(&e) || !valid(c.sharpness) || edges.insert(e, c.sharpness).is_some() {
                return Err(bad("invalid hierarchical edge crease"));
            }
        }
        for c in &sharpness.vertices {
            if c.vertex >= cage.points.len() || !valid(c.sharpness) || vertices.insert(c.vertex, c.sharpness).is_some() {
                return Err(bad("invalid hierarchical vertex crease"));
            }
        }
        for c in &cage.edge_creases {
            edges.entry(key(c.edge[0], c.edge[1])).or_insert(c.sharpness);
        }
        for c in &cage.vertex_creases {
            vertices.entry(c.vertex).or_insert(c.sharpness);
        }
        cage.edge_creases = edges.into_iter().filter(|&(_, s)| s > 0.0).map(|((a, b), sharpness)| EdgeCrease { edge: [a, b], sharpness }).collect();
        cage.vertex_creases = vertices.into_iter().filter(|&(_, s)| s > 0.0).map(|(vertex, sharpness)| VertexCrease { vertex, sharpness }).collect();
        Ok(())
    }
    /// Install changes to the terminal evaluator as details without changing any
    /// authored control level, leaf identity, or subdivision topology.
    pub fn with_evaluator(&self, cage: &Cage) -> Result<Self> {
        cage.validate()?;
        let before = self.evaluate()?;
        if cage.faces != before.faces || cage.points.len() != before.points.len() {
            return Err(bad("hierarchical evaluator topology changed"));
        }
        let mut out = self.clone();
        let terminal = out.extrusions.iter().rposition(|operation| operation.level == out.depth);
        let details = if let Some(id) = terminal {
            &mut out.extrusions[id].details
        } else {
            out.details.last_mut().ok_or_else(|| bad("missing terminal detail level"))?
        };
        let mut deltas: BTreeMap<_, _> = details.iter().map(|d| (d.vertex, d.delta)).collect();
        for (vertex, (&after, &old)) in cage.points.iter().zip(&before.points).enumerate() {
            if after != old {
                *deltas.entry(vertex).or_insert(Vec3::ZERO) += after - old;
            }
        }
        *details = deltas.into_iter().filter(|&(_, delta)| delta != Vec3::ZERO).map(|(vertex, delta)| Detail { vertex, delta }).collect();
        if let Some(id) = terminal {
            out.extrusions[id].sharpness = None;
        } else {
            out.sharpness.retain(|s| s.level != self.depth);
        }
        let inherited = out.evaluate()?;
        let mut edges: BTreeMap<_, _> = inherited.edge_creases.iter().map(|c| (key(c.edge[0], c.edge[1]), (c.sharpness, 0.0))).collect();
        let mut vertices: BTreeMap<_, _> = inherited.vertex_creases.iter().map(|c| (c.vertex, (c.sharpness, 0.0))).collect();
        for c in &cage.edge_creases {
            edges.entry(key(c.edge[0], c.edge[1])).or_default().1 = c.sharpness;
        }
        for c in &cage.vertex_creases {
            vertices.entry(c.vertex).or_default().1 = c.sharpness;
        }
        let edges: Vec<_> =
            edges.into_iter().filter_map(|((a, b), (old, sharpness))| (old != sharpness).then_some(EdgeCrease { edge: [a, b], sharpness })).collect();
        let vertices: Vec<_> =
            vertices.into_iter().filter_map(|(vertex, (old, sharpness))| (old != sharpness).then_some(VertexCrease { vertex, sharpness })).collect();
        if !edges.is_empty() || !vertices.is_empty() {
            let sharpness = LevelSharpness { level: self.depth, edges, vertices };
            if let Some(id) = terminal {
                out.extrusions[id].sharpness = Some(sharpness);
            } else {
                out.sharpness.push(sharpness);
            }
        }
        Ok(out)
    }
    /// Restore generated sharpness at the level where it was authored. Keep
    /// any crease that the user has since changed to a different weight.
    pub fn restore_level_sharpness(&mut self, previous: &LevelSharpness, generated: f64) -> Result<()> {
        if previous.level > self.depth {
            return Err(bad("invalid crease history level"));
        }
        let mut level = self.clone();
        level.depth = previous.level;
        level.details.truncate(previous.level + 1);
        level.sharpness.retain(|s| s.level <= previous.level);
        level.extrusions.retain(|operation| operation.level <= previous.level);
        let mut cage = level.evaluate()?;
        let edges = cage.edges()?.into_iter().map(|e| key(e[0], e[1])).collect::<BTreeSet<_>>();
        let valid = |s: f64| s.is_finite() && (0.0..=10.0).contains(&s);
        let mut seen = BTreeSet::new();
        for old in &previous.edges {
            let edge = key(old.edge[0], old.edge[1]);
            if !edges.contains(&edge) || !valid(old.sharpness) || !seen.insert(edge) {
                return Err(bad("invalid hierarchical edge history"));
            }
            if let Some(crease) = cage.edge_creases.iter_mut().find(|c| key(c.edge[0], c.edge[1]) == edge && c.sharpness == generated) {
                crease.sharpness = old.sharpness;
            }
        }
        seen.clear();
        for old in &previous.vertices {
            if old.vertex >= cage.points.len() || !valid(old.sharpness) || !seen.insert((old.vertex, old.vertex)) {
                return Err(bad("invalid hierarchical vertex history"));
            }
            if let Some(crease) = cage.vertex_creases.iter_mut().find(|c| c.vertex == old.vertex && c.sharpness == generated) {
                crease.sharpness = old.sharpness;
            }
        }
        cage.edge_creases.retain(|c| c.sharpness > 0.0);
        cage.vertex_creases.retain(|c| c.sharpness > 0.0);
        let edited = level.with_evaluator(&cage)?;
        if let Some(index) = self.extrusions.iter().rposition(|op| op.level == previous.level) {
            self.extrusions[index].sharpness = edited.extrusions.get(index).and_then(|op| op.sharpness.clone());
        } else {
            self.sharpness.retain(|s| s.level != previous.level);
            self.sharpness.extend(edited.sharpness.into_iter().filter(|s| s.level == previous.level));
            self.sharpness.sort_by_key(|s| s.level);
        }
        Ok(())
    }
    /// The linear influence of an edit at its original coarse/detail level.
    pub fn basis(&self, vertex: usize) -> Result<Cage> {
        let birth = *self.controls.get(&vertex).ok_or_else(|| bad("vertex is not a selectable hierarchical control"))?;
        if birth > self.depth {
            return Err(bad("invalid hierarchical control level"));
        }
        self.validate_extrusion_levels()?;
        let mut cage = self.base.clone();
        cage.points.fill(Vec3::ZERO);
        for level in 0..=self.depth {
            let mut injected = false;
            if level == birth && vertex < cage.points.len() {
                cage.points.get_mut(vertex).ok_or_else(|| bad("invalid hierarchical control"))?.x = 1.0;
                injected = true;
            }
            Self::apply_sharpness(self.sharpness.iter().find(|s| s.level == level), &mut cage)?;
            for operation in self.extrusions_at(level) {
                cage = operation.replay(&cage, true)?.cage;
                if level == birth && !injected && vertex < cage.points.len() {
                    cage.points.get_mut(vertex).ok_or_else(|| bad("invalid extruded control"))?.x = 1.0;
                    injected = true;
                }
                Self::apply_sharpness(operation.sharpness.as_ref(), &mut cage)?;
            }
            if level == birth && !injected {
                return Err(bad("invalid hierarchical control birth"));
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
        if vertices.iter().any(|&id| self.is_retired_collapse_control(id)) {
            return Err(bad("a collapsed endpoint no longer has a selectable control"));
        }
        // Stage only the affected sparse rows. The base cage and unrelated
        // details can be large, and an invalid later control must not keep
        // any changes from earlier controls in the same drag update.
        let mut staged = BTreeMap::new();
        for (&vertex, &delta) in vertices.iter().zip(deltas) {
            let birth = *self.controls.get(&vertex).ok_or_else(|| bad("invalid hierarchical selection"))?;
            let extrusion = self.extrusions.iter().rposition(|op| op.level == birth && op.first_vertex <= vertex);
            let original = match extrusion {
                Some(index) => self.extrusions.get(index).map(|op| &op.details),
                None => self.details.get(birth),
            }
            .ok_or_else(|| bad("invalid detail level"))?;
            let details = staged.entry((extrusion, birth)).or_insert_with(|| original.clone());
            if let Some(detail) = details.iter_mut().find(|d| d.vertex == vertex) {
                let next = detail.delta + delta;
                if !next.is_finite() {
                    return Err(bad("hierarchical detail delta must be finite"));
                }
                detail.delta = next;
            } else {
                if !delta.is_finite() {
                    return Err(bad("hierarchical detail delta must be finite"));
                }
                details.push(Detail { vertex, delta });
            }
        }
        for ((extrusion, birth), details) in staged {
            // Every slot was read above. This method never changes row counts.
            if let Some(index) = extrusion {
                self.extrusions[index].details = details;
            } else {
                self.details[birth] = details;
            }
        }
        Ok(())
    }
    fn tags(&self) -> Result<(Cage, Vec<Tag>, Vec<usize>)> {
        self.validate_extrusion_levels()?;
        let mut cage = self.base.clone();
        cage.points.fill(Vec3::ZERO);
        let mut counts = Vec::with_capacity(self.depth + 1);
        let mut charts = cage.faces.len();
        let mut tags: Vec<_> = (0..charts).map(|f| (f, [[0, 0], [GRID, 0], [GRID, GRID], [0, GRID]])).collect();
        for level in 0..=self.depth {
            for operation in self.extrusions_at(level) {
                let result = operation.replay(&cage, true)?;
                operation.rechart(&mut tags, &mut charts, &result.walls)?;
                cage = result.cage;
            }
            counts.push(cage.points.len());
            if level == self.depth {
                break;
            }
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
            tags = next;
        }
        Ok((cage, tags, counts))
    }
    fn locations(&self) -> Result<(BTreeMap<(usize, usize, usize), usize>, Vec<Tag>, Vec<usize>)> {
        let (cage, tags, counts) = self.tags()?;
        let mut locations = BTreeMap::new();
        for (face, (root, uv)) in cage.faces.iter().zip(&tags) {
            for (&vertex, uv) in face.iter().zip(uv) {
                if locations.insert((*root, uv[0], uv[1]), vertex).is_some_and(|previous| previous != vertex) {
                    return Err(bad("incompatible evaluator vertices share a chart coordinate"));
                }
            }
        }
        Ok((locations, tags, counts))
    }
    /// Split selected leaves independently in U/V. Counts must be powers of two.
    pub fn subdivide(&self, selected: &[usize], counts: [usize; 2]) -> Result<Self> {
        self.check_trim_wall_topology()?;
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
            let extent = [x - u, y - v];
            if extent[0] < counts[0] || extent[1] < counts[1] {
                return Err(bad("local refinement depth budget exceeded"));
            }
            if !extent[0].is_multiple_of(counts[0]) || !extent[1].is_multiple_of(counts[1]) {
                return Err(bad("local subdivision does not divide the parameter extent exactly"));
            }
            let du = extent[0] / counts[0];
            let dv = extent[1] / counts[1];
            let root = leaf.root;
            leaf.active = false;
            for j in 0..counts[1] {
                for i in 0..counts[0] {
                    out.leaves.push(Leaf { root, domain: [u + i * du, v + j * dv, u + (i + 1) * du, v + (j + 1) * dv], active: true });
                }
            }
        }
        out.update_parameter_depth()?;
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
    fn update_parameter_depth(&mut self) -> Result<()> {
        for leaf in self.leaves.iter().filter(|leaf| leaf.active) {
            let [u, v, x, y] = leaf.domain;
            if u >= x || v >= y || x > GRID || y > GRID {
                return Err(bad("invalid leaf parameter extent"));
            }
            for coordinate in leaf.domain {
                let level = if coordinate == 0 { 0 } else { MAX_DEPTH.saturating_sub(coordinate.trailing_zeros() as usize) };
                self.depth = self.depth.max(level);
            }
        }
        if self.depth > MAX_DEPTH {
            return Err(bad("local refinement depth budget exceeded"));
        }
        Ok(())
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
        let mut layout = self.skin_layout()?;
        for (face, rim) in self.replay_quad_caps(&layout)? {
            let slot = layout.faces.get_mut(face).ok_or_else(|| bad("missing quad cap face slot"))?;
            *slot = rim.sides.into_iter().map(|side| side.edge[0]).collect();
        }
        if let Some((site, retired)) = self.replay_boundary_collapse(&layout)? {
            let slot = layout.faces.get_mut(site.face).ok_or_else(|| bad("missing collapsed face owner"))?;
            *slot = site.boundary.to_vec();
            layout.paths.retain(|(edge, _)| !edge.iter().any(|id| retired.contains(id)));
            layout.collapse = Some(site);
        }
        if layout.faces.iter().flatten().any(|vertex| !self.controls.contains_key(vertex)) {
            return Err(bad("logical boundary lost a selectable corner"));
        }
        Ok(layout)
    }
    fn skin_layout(&self) -> Result<Layout> {
        topology::dissolved_layout(self.raw_layout()?, &self.dissolved_edges)
    }
    fn exposes_leaf(&self, id: usize, leaf: &Leaf) -> bool {
        leaf.active || self.detached_leaves.binary_search(&id).is_ok()
    }
    fn raw_layout(&self) -> Result<Layout> {
        if self.depth > MAX_DEPTH || self.leaves.is_empty() || self.leaves.len() > MAX_LEAVES || !self.leaves.iter().any(|l| l.active) {
            return Err(bad("invalid hierarchical topology budget"));
        }
        self.validate_quad_cap_slots()?;
        self.validate_detached_leaves()?;
        let (locations, tags, levels) = self.locations()?;
        if self.controls.len() > *levels.last().ok_or_else(|| bad("missing hierarchy level"))?
            || self.controls.iter().any(|(&v, &level)| levels.get(level).is_none_or(|&n| v >= n))
        {
            return Err(bad("invalid selectable hierarchical controls"));
        }
        let charts = tags.iter().map(|(root, _)| root + 1).max().unwrap_or(0);
        let mut exposed = vec![BTreeSet::new(); charts];
        for (_, leaf) in self.leaves.iter().enumerate().filter(|(id, leaf)| self.exposes_leaf(*id, leaf)) {
            let [u, v, x, y] = leaf.domain;
            if leaf.root >= charts || u >= x || v >= y || x > GRID || y > GRID {
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
            if face.len() < 4 {
                return Err(bad("logical leaf lost an evaluator corner"));
            }
            faces.push(face);
        }
        let mut by_root = vec![Vec::new(); charts];
        for (id, leaf) in self.leaves.iter().enumerate().filter(|(id, leaf)| self.exposes_leaf(*id, leaf)) {
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
            owners.push(matches.first().copied().filter(|&id| self.leaves.get(id).is_some_and(|leaf| leaf.active)));
        }
        Ok(Layout { faces, owners, paths: paths.into_iter().map(|((a, b), p)| ([a, b], p)).collect(), collapse: None })
    }
    pub fn surface(&self, cage: &Cage, level: usize) -> Result<(SurfacePreview, Cage)> {
        self.layout_surface(cage, cage.surface(level)?)
    }
    pub fn layout_surface(&self, cage: &Cage, mut surface: SurfacePreview) -> Result<(SurfacePreview, Cage)> {
        let layout = self.layout()?;
        let mut triangles = Vec::with_capacity(surface.mesh.triangles.len());
        let mut faces = Vec::with_capacity(surface.mesh.tri_face.len());
        for (triangle, &id) in surface.mesh.triangles.iter().zip(&surface.mesh.tri_face) {
            if let Some(owner) = *layout.owners.get(id as usize).ok_or_else(|| bad("missing leaf owner"))?
                && layout.collapse.is_none_or(|site| site.face != owner)
            {
                // The retained rectangular evaluator does not delimit the
                // triangle. Only the fitted trim renders this face.
                triangles.push(*triangle);
                faces.push(owner as u32);
            }
        }
        surface.mesh.triangles = triangles;
        surface.mesh.tri_face = faces;
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
        if let Some(site) = layout.collapse {
            // Keep logical IDs in the overlay. Exact chart guides fill these
            // edges when the fitted preview is ready.
            surface.boundaries.extend(site.diagonal_edges().map(|edge| (edge, Vec::new())));
        }
        let mut logical = Cage { points: cage.points.clone(), faces: layout.faces, ..Default::default() };
        if let Some(wall) = self.validated_trim_wall()? {
            wall.append_preview(&mut surface, &mut logical)?;
        }
        surface.index = crate::MeshIndex::new(&surface.mesh);
        Ok((surface, logical))
    }
}
