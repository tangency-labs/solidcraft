//! Stable path chains and all-quad sweeps for editable network pipes.
use super::{Cage, MAX_FACES, MAX_POINTS, Result, bad};
use crate::Vec3;
use std::collections::{BTreeMap, BTreeSet};

mod build;
mod junction;
mod sweep;
pub use build::{BuiltPipe, PipeSettings, SegmentGeometry, SegmentOverride};
pub use sweep::swept_path;

const JOIN_TOLERANCE: f64 = 1e-7;

/// An already evaluated source curve. Keys are persistent CAD/sketch identities;
/// polyline samples describe its arc length, rather than editable cage vertices.
#[derive(Clone, Debug)]
pub struct PathEdge {
    pub key: String,
    pub points: Vec<Vec3>,
}
#[derive(Clone, Debug)]
pub struct Chain {
    pub key: String,
    pub edges: Vec<(usize, bool)>,
    pub nodes: [usize; 2],
    pub closed: bool,
}
#[derive(Clone, Debug)]
pub struct Network {
    edges: Vec<PathEdge>,
    nodes: Vec<Vec3>,
    degree: Vec<usize>,
    chains: Vec<Chain>,
}

impl Network {
    pub fn edges(&self) -> &[PathEdge] {
        &self.edges
    }
    pub fn nodes(&self) -> &[Vec3] {
        &self.nodes
    }
    pub fn degree(&self) -> &[usize] {
        &self.degree
    }
    pub fn chains(&self) -> &[Chain] {
        &self.chains
    }
    pub fn new(mut edges: Vec<PathEdge>) -> Result<Self> {
        if edges.is_empty()
            || edges.len() > 128
            || edges.iter().any(|e| e.key.is_empty() || e.key.len() > 4096 || e.key.contains('\0') || !(2..=32_768).contains(&e.points.len()))
            || edges.iter().map(|e| e.points.len()).sum::<usize>() > 32_768
            || edges.iter().map(|e| e.key.len()).sum::<usize>() > 65_536
        {
            return Err(bad("a pipe needs 1..128 named source edges and bounded curve samples"));
        }
        edges.sort_by(|a, b| a.key.cmp(&b.key));
        if edges.windows(2).any(|e| e[0].key == e[1].key) {
            return Err(bad("pipe source edge identities must be distinct"));
        }
        let mut nodes = Vec::<Vec3>::new();
        let mut ends = Vec::with_capacity(edges.len());
        let mut adjacency = BTreeMap::<usize, Vec<usize>>::new();
        for (id, edge) in edges.iter_mut().enumerate() {
            if edge.points.iter().any(|p| !p.is_finite() || p.len() > 100_000.0) {
                return Err(bad("pipe source points must be finite and bounded"));
            }
            edge.points.dedup_by(|a, b| a.dist(*b) <= 1e-10);
            let (Some(&a), Some(&b)) = (edge.points.first(), edge.points.last()) else {
                return Err(bad("empty pipe source edge"));
            };
            if edge.points.len() < 2 || length(&edge.points) < JOIN_TOLERANCE {
                return Err(bad("pipe source edges must have positive length"));
            }
            let mut indices = [0; 2];
            for (slot, point) in [a, b].into_iter().enumerate() {
                let node = nodes.iter().position(|p| p.dist(point) <= JOIN_TOLERANCE).unwrap_or_else(|| {
                    nodes.push(point);
                    nodes.len() - 1
                });
                indices[slot] = node;
                adjacency.entry(node).or_default().push(id);
            }
            // Shared endpoints use one coordinate, including a one-edge closed path.
            edge.points[0] = nodes[indices[0]];
            if let Some(last) = edge.points.last_mut() {
                *last = nodes[indices[1]];
            }
            ends.push(indices);
        }
        let degree: Vec<_> = (0..nodes.len()).map(|i| adjacency.get(&i).map_or(0, Vec::len)).collect();
        if degree.iter().any(|n| *n > 6) {
            return Err(bad("a pipe junction supports at most six branches"));
        }
        let mut used = BTreeSet::new();
        let mut chains = Vec::new();
        for start in (0..nodes.len()).filter(|&n| degree[n] != 2) {
            for &edge in adjacency.get(&start).into_iter().flatten() {
                if !used.contains(&edge) {
                    chains.push(trace(start, edge, &edges, &ends, &adjacency, &mut used)?);
                }
            }
        }
        for edge in 0..edges.len() {
            if !used.contains(&edge) {
                chains.push(trace(ends[edge][0], edge, &edges, &ends, &adjacency, &mut used)?);
            }
        }
        chains.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(Self { edges, nodes, degree, chains })
    }

    /// `samples` is the average number of intervals per original source edge.
    /// Allocate the total by arc length, retaining at least one on every edge.
    pub fn sample(&self, chain: usize, samples: usize) -> Result<Vec<Vec3>> {
        let chain = self.chains.get(chain).ok_or_else(|| bad("pipe chain index is outside the path"))?;
        if !(1..=20).contains(&samples) {
            return Err(bad("pipe path samples must be 1..20"));
        }
        let lengths = chain
            .edges
            .iter()
            .map(|&(i, _)| self.edges.get(i).map(|e| length(&e.points)))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| bad("pipe chain lost a source edge"))?;
        let total = lengths.iter().sum::<f64>();
        if !total.is_finite() || total <= JOIN_TOLERANCE {
            return Err(bad("pipe chain has no finite length"));
        }
        let target = (samples * lengths.len()).max(if chain.nodes[0] == chain.nodes[1] { 3 } else { 1 });
        let mut counts = vec![1usize; lengths.len()];
        // Split the widest remaining interval. Unlike proportional rounding with
        // a reserved minimum, this does not oversample short source edges.
        for _ in lengths.len()..target {
            let mut widest = 0;
            for i in 1..lengths.len() {
                if lengths[i] / counts[i] as f64 > lengths[widest] / counts[widest] as f64 {
                    widest = i;
                }
            }
            counts[widest] += 1;
        }
        let mut out = Vec::with_capacity(target + 1);
        for ((&(id, forward), &count), &len) in chain.edges.iter().zip(&counts).zip(&lengths) {
            let points = &self.edges[id].points;
            for i in usize::from(!out.is_empty())..=count {
                let t = i as f64 / count as f64;
                out.push(at_length(points, if forward { t * len } else { (1.0 - t) * len })?);
            }
        }
        Ok(out)
    }
}

fn trace(
    start: usize,
    mut edge: usize,
    edges: &[PathEdge],
    ends: &[[usize; 2]],
    adjacency: &BTreeMap<usize, Vec<usize>>,
    used: &mut BTreeSet<usize>,
) -> Result<Chain> {
    let mut node = start;
    let mut path = Vec::new();
    for _ in 0..=edges.len() {
        if !used.insert(edge) {
            return Err(bad("pipe chain repeats a source edge"));
        }
        let pair = ends.get(edge).ok_or_else(|| bad("pipe chain lost an endpoint"))?;
        let forward = pair[0] == node;
        if !forward && pair[1] != node {
            return Err(bad("pipe chain is disconnected"));
        }
        path.push((edge, forward));
        node = pair[usize::from(forward)];
        let incident = adjacency.get(&node).ok_or_else(|| bad("pipe path lost a node"))?;
        if node == start || incident.len() != 2 {
            let mut identities: Vec<_> = path.iter().map(|&(i, _)| edges[i].key.as_str()).collect();
            identities.sort_unstable();
            let key = identities.iter().map(|id| format!("{}:{id}", id.len())).collect::<Vec<_>>().join("|");
            return Ok(Chain { key, edges: path, nodes: [start, node], closed: node == start && incident.len() == 2 });
        }
        edge = incident.iter().copied().find(|i| !used.contains(i)).ok_or_else(|| bad("pipe chain has no unused continuation"))?;
    }
    Err(bad("pipe chain traversal exceeded its budget"))
}
fn length(points: &[Vec3]) -> f64 {
    points.windows(2).map(|p| p[0].dist(p[1])).sum()
}
fn at_length(points: &[Vec3], mut distance: f64) -> Result<Vec3> {
    for pair in points.windows(2) {
        let len = pair[0].dist(pair[1]);
        if distance <= len && len > 0.0 {
            return Ok(pair[0] + (pair[1] - pair[0]) * (distance / len).clamp(0.0, 1.0));
        }
        distance -= len;
    }
    points.last().copied().ok_or_else(|| bad("pipe source edge has no endpoint"))
}

fn budget(cage: &Cage) -> Result<()> {
    if cage.points.len() > MAX_POINTS || cage.faces.len() > MAX_FACES {
        return Err(bad("pipe control cage exceeds its editing budget"));
    }
    Ok(())
}
