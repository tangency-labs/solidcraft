//! Assemble swept chains and shared junctions into one editable cage.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SegmentOverride {
    pub diameter: Option<f64>,
    pub samples: Option<usize>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PipeSettings {
    pub diameter: f64,
    pub section_density: usize,
    pub path_samples: usize,
    pub overrides: BTreeMap<String, SegmentOverride>,
}
impl Default for PipeSettings {
    fn default() -> Self {
        Self { diameter: 10.0, section_density: 1, path_samples: 3, overrides: BTreeMap::new() }
    }
}
impl PipeSettings {
    pub fn validate(&self) -> Result<()> {
        let diameter = |d: f64| d.is_finite() && (0.1..=10_000.0).contains(&d);
        if !diameter(self.diameter)
            || !(1..=12).contains(&self.section_density)
            || !(1..=20).contains(&self.path_samples)
            || self.overrides.len() > 128
            || self.overrides.keys().map(String::len).sum::<usize>() > 131_072
            || self.overrides.iter().any(|(key, value)| {
                key.is_empty()
                    || key.contains('\0')
                    || value.diameter.is_some_and(|d| !diameter(d))
                    || value.samples.is_some_and(|s| !(1..=20).contains(&s))
            })
        {
            return Err(bad("pipe diameter must be 0.1..10000 mm, section density 1..12 and path samples 1..20, with bounded segment overrides"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SegmentGeometry {
    pub key: String,
    pub diameter: f64,
    pub samples: usize,
    pub path: Vec<Vec3>,
    pub faces: Vec<usize>,
}
pub struct BuiltPipe {
    pub cage: Cage,
    pub segments: Vec<SegmentGeometry>,
}
impl Network {
    pub fn build(&self, settings: &PipeSettings) -> Result<BuiltPipe> {
        settings.validate()?;
        let effective: Vec<_> = self
            .chains
            .iter()
            .map(|chain| {
                let given = settings.overrides.get(&chain.key);
                (given.and_then(|v| v.diameter).unwrap_or(settings.diameter), given.and_then(|v| v.samples).unwrap_or(settings.path_samples))
            })
            .collect();
        let paths = effective.iter().enumerate().map(|(i, &(_, samples))| self.sample(i, samples)).collect::<Result<Vec<_>>>()?;
        let mut clearance = vec![0.0f64; self.nodes.len()];
        let mut directions = vec![Vec::<Vec3>::new(); self.nodes.len()];
        for ((chain, path), &(diameter, _)) in self.chains.iter().zip(&paths).zip(&effective) {
            if chain.closed {
                continue;
            }
            for (end, &node) in chain.nodes.iter().enumerate() {
                if self.degree[node] > 2 {
                    clearance[node] = clearance[node].max(diameter * 1.1);
                    let direction = if end == 0 { path[1] - path[0] } else { path[path.len() - 2] - path[path.len() - 1] };
                    directions[node].push(direction.normalized().ok_or_else(|| bad("pipe junction has no branch direction"))?);
                }
            }
        }
        for (node, directions) in directions.iter().enumerate() {
            let mut sine = 1.0f64;
            for (i, a) in directions.iter().enumerate() {
                for b in directions.iter().skip(i + 1) {
                    let dot = a.dot(*b).clamp(-1.0, 1.0);
                    if dot > (10.0f64).to_radians().cos() {
                        return Err(bad("pipe branches meet at an angle too small for a junction"));
                    }
                    sine = sine.min(((1.0 - dot) * 0.5).sqrt());
                }
            }
            clearance[node] *= (0.75 / sine).max(1.0);
        }
        let mut cage = Cage::default();
        let mut ports = BTreeMap::<usize, Vec<junction::Port>>::new();
        let mut segments = Vec::with_capacity(self.chains.len());
        for ((chain, source_path), &(diameter, samples)) in self.chains.iter().zip(&paths).zip(&effective) {
            let radius = diameter * 0.5;
            let path = if chain.closed { source_path.clone() } else { trim(source_path, clearance[chain.nodes[0]], clearance[chain.nodes[1]])? };
            let caps = chain.nodes.map(|node| self.degree[node] == 1);
            let sweep = sweep::tube(&path, radius, settings.section_density, chain.closed, caps)?;
            let offset = cage.points.len();
            let first_face = cage.faces.len();
            cage.points.extend(sweep.cage.points);
            cage.faces.extend(sweep.cage.faces.into_iter().map(|face| face.into_iter().map(|v| v + offset).collect()));
            budget(&cage)?;
            if !chain.closed {
                for end in 0..2 {
                    let node = chain.nodes[end];
                    if self.degree[node] > 2 {
                        ports.entry(node).or_default().push(junction::Port {
                            direction: sweep.tangents[end] * if end == 0 { 1.0 } else { -1.0 },
                            ring: sweep.ends[end].iter().map(|v| v + offset).collect(),
                            radius,
                            reverse: end == 1,
                        });
                    }
                }
            }
            segments.push(SegmentGeometry { key: chain.key.clone(), diameter, samples, path, faces: (first_face..cage.faces.len()).collect() });
        }
        for (node, ports) in ports {
            if ports.len() != self.degree[node] {
                return Err(bad("pipe junction lost a branch"));
            }
            junction::attach(&mut cage, self.nodes[node], &ports, settings.section_density)?;
            budget(&cage)?;
        }
        cage.validate()?;
        if !cage.boundary_loops()?.is_empty() {
            return Err(bad("pipe network did not close every branch"));
        }
        Ok(BuiltPipe { cage, segments })
    }
}
fn trim(points: &[Vec3], start: f64, end: f64) -> Result<Vec<Vec3>> {
    let total = length(points);
    if start + end >= total * 0.9 {
        return Err(bad("pipe branches are too short for this diameter; reduce the diameter or lengthen the path"));
    }
    let mut out = vec![at_length(points, start)?];
    let mut walked = 0.0;
    for pair in points.windows(2) {
        walked += pair[0].dist(pair[1]);
        if walked > start + 1e-9 && walked < total - end - 1e-9 {
            out.push(pair[1]);
        }
    }
    out.push(at_length(points, total - end)?);
    Ok(out)
}
