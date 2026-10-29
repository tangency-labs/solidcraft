//! Bounded matching of an opening, with analytic CAD jets and independent seam checks.
use super::{Cage, Result, bad, fitting::RootBasis, key};
use crate::{
    Vec3,
    tess::{EvalSurface, SurfaceJet},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod solve;

/// Evaluation-only continuation used by smooth G2 openings. Ghost elements never
/// become selectable elements or contribute to the displayed bounds.
pub fn preview(cage: &Cage, order: usize, levels: usize) -> Result<super::SurfacePreview> {
    let evaluator = if order == 2 { quadratic_extension(cage)? } else { cage.clone() };
    let mut surface = evaluator.surface(levels)?;
    surface.vertices.truncate(cage.points.len());
    let edges: BTreeSet<_> = cage.edges()?.into_iter().map(|e| key(e[0], e[1])).collect();
    surface.boundaries.retain(|(e, _)| edges.contains(&key(e[0], e[1])));
    let mut mesh = crate::Mesh::default();
    let mut remap = vec![None; surface.mesh.positions.len()];
    for (triangle, &face) in surface.mesh.triangles.iter().zip(&surface.mesh.tri_face) {
        if face as usize >= cage.faces.len() {
            continue;
        }
        let mut indices = [0; 3];
        for (slot, &old) in triangle.iter().enumerate() {
            let entry = remap.get_mut(old as usize).ok_or_else(|| bad("invalid Match preview triangle"))?;
            indices[slot] = match *entry {
                Some(index) => index,
                None => {
                    let index = mesh.positions.len() as u32;
                    mesh.positions.push(*surface.mesh.positions.get(old as usize).ok_or_else(|| bad("missing Match preview point"))?);
                    mesh.normals.push(*surface.mesh.normals.get(old as usize).ok_or_else(|| bad("missing Match preview normal"))?);
                    *entry = Some(index);
                    index
                }
            };
        }
        mesh.triangles.push(indices);
        mesh.tri_face.push(face);
    }
    surface.mesh = mesh;
    surface.index = crate::MeshIndex::new(&surface.mesh);
    Ok(surface)
}

pub fn evaluated_vertices(cage: &Cage, order: usize, levels: usize) -> Result<Vec<Vec3>> {
    let evaluator = if order == 2 { quadratic_extension(cage)? } else { cage.clone() };
    let mut vertices = evaluator.refined(levels)?.limit_vertices()?;
    vertices.truncate(cage.points.len());
    Ok(vertices)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tolerances {
    pub position_mm: f64,
    pub tangent_deg: f64,
    pub curvature_relative: f64,
}
impl Default for Tolerances {
    fn default() -> Self {
        Self { position_mm: 0.001, tangent_deg: 0.1, curvature_relative: 0.01 }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub edge: [usize; 2],
    pub coordinates: Vec<f64>,
    #[serde(default)]
    pub reverse: bool,
    pub order: usize,
    #[serde(default)]
    pub tolerances: Tolerances,
}
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub position_mm: f64,
    pub tangent_deg: f64,
    pub curvature_relative: f64,
    pub iterations: usize,
    pub controls: usize,
}
pub struct Solution {
    pub cage: Cage,
    pub basis: RootBasis,
    pub report: Report,
}
/// An exact point on the target boundary and the analytic jet of its support there.
#[derive(Clone, Copy)]
pub struct Target {
    /// A genuine corner of the support curve has no single differential seam condition.
    pub corner: bool,
    pub point: Vec3,
    pub tangent: Vec3,
    pub normal: Vec3,
    pub support: SurfaceJet,
}
pub fn opening(cage: &Cage, edge: [usize; 2]) -> Result<Vec<usize>> {
    cage.boundary_loops()?
        .into_iter()
        .find(|ring| ring.iter().copied().zip(ring.iter().copied().cycle().skip(1)).take(ring.len()).any(|(a, b)| key(a, b) == key(edge[0], edge[1])))
        .ok_or_else(|| bad("Match requires a complete free opening"))
}
impl Config {
    pub fn validate_topology(&self, cage: &Cage) -> Result<()> {
        cage.validate()?;
        let openings = cage.boundary_loops()?;
        if cage.faces.iter().any(|f| f.len() != 4) || openings.is_empty() || (self.order == 2 && openings.len() != 1) {
            return Err(bad("Match requires quad topology and a valid opening"));
        }
        self.validate(&opening(cage, self.edge)?)
    }
    fn validate(&self, ring: &[usize]) -> Result<()> {
        let t = &self.tolerances;
        if self.order > 2
            || !(4..=64).contains(&ring.len())
            || self.coordinates.len() != ring.len()
            || self.coordinates.iter().any(|f| !f.is_finite() || !(0.0..1.0).contains(f))
            || !t.position_mm.is_finite()
            || !(1e-6..=1.0).contains(&t.position_mm)
            || !t.tangent_deg.is_finite()
            || !(0.0001..=10.0).contains(&t.tangent_deg)
            || !t.curvature_relative.is_finite()
            || !(0.000001..=1.0).contains(&t.curvature_relative)
        {
            return Err(bad("Match requires G0..G2, 4..64 boundary controls, normalized coordinates and bounded tolerances"));
        }
        let mut winding = 0.0;
        for i in 0..ring.len() {
            let step = self.step(i)?;
            if step.abs() < 1e-10 {
                return Err(bad("Match coordinates must be distinct"));
            }
            winding += step.abs();
        }
        if (winding - 1.0).abs() > 1e-8 {
            return Err(bad("Match coordinates must describe exactly one complete boundary"));
        }
        Ok(())
    }
    fn step(&self, i: usize) -> Result<f64> {
        let a = *self.coordinates.get(i).ok_or_else(|| bad("missing Match coordinate"))?;
        let b = *self.coordinates.get((i + 1) % self.coordinates.len()).ok_or_else(|| bad("missing next Match coordinate"))?;
        Ok(if self.reverse { -(a - b).rem_euclid(1.0) } else { (b - a).rem_euclid(1.0) })
    }
    fn coordinate(&self, i: usize, t: f64) -> Result<f64> {
        Ok((self.coordinates.get(i).ok_or_else(|| bad("missing Match coordinate"))? + self.step(i)? * t).rem_euclid(1.0))
    }
}
/// Add only an evaluation ring. Its quadratic extension is linear in the editable
/// controls; none of its faces or points belongs to the editable/output topology.
pub fn quadratic_extension(cage: &Cage) -> Result<Cage> {
    let loops = cage.boundary_loops()?;
    let [ring] = loops.as_slice() else {
        return Err(bad("G2 Match requires exactly one opening"));
    };
    if ring.len() > 64 || cage.faces.iter().any(|f| f.len() != 4) {
        return Err(bad("G2 Match requires bounded quad topology"));
    }
    let topology = cage.topology()?;
    let boundary: BTreeSet<_> = ring.iter().copied().collect();
    // An explicitly pinned corner keeps the ordinary open-boundary evaluation
    // rules. Extending such a loop would let inner controls round its straight
    // runs even though the corner point itself remained pinned.
    if cage.vertex_creases.iter().any(|c| c.sharpness >= 10.0 && boundary.contains(&c.vertex)) {
        return Ok(cage.clone());
    }
    let neighbors = |vertex: usize, excluded: &BTreeSet<usize>| -> Result<usize> {
        let mut candidates = BTreeSet::new();
        for &id in topology.incident.get(vertex).ok_or_else(|| bad("invalid Match control"))? {
            let e = topology.edges.get(id).ok_or_else(|| bad("invalid Match edge"))?;
            let next = if e.a == vertex { e.b } else { e.a };
            if !excluded.contains(&next) {
                candidates.insert(next);
            }
        }
        if candidates.len() != 1 {
            return Err(bad("G2 Match needs two regular inward control rows; add control resolution"));
        }
        candidates.into_iter().next().ok_or_else(|| bad("missing inward control"))
    };
    let inner = ring.iter().map(|&v| neighbors(v, &boundary)).collect::<Result<Vec<_>>>()?;
    let excluded: BTreeSet<_> = boundary.iter().copied().chain(inner.iter().copied()).collect();
    let second = inner.iter().map(|&v| neighbors(v, &excluded)).collect::<Result<Vec<_>>>()?;
    let mut out = cage.clone();
    let start = out.points.len();
    for ((&v, &a), &b) in ring.iter().zip(&inner).zip(&second) {
        out.points.push(cage.point(v)? * 3.0 - cage.point(a)? * 3.0 + cage.point(b)?);
    }
    for i in 0..ring.len() {
        let next = (i + 1) % ring.len();
        out.faces.push(vec![ring[next], ring[i], start + i, start + next]);
    }
    out.validate()?;
    Ok(out)
}
fn evaluator(cage: &Cage, order: usize) -> Result<Cage> {
    if order == 2 { quadratic_extension(cage) } else { Ok(cage.clone()) }
}
#[derive(Clone)]
struct Station {
    root: usize,
    uv: [f64; 2],
    across: [f64; 2],
    fraction: f64,
    differential: bool,
    direction: f64,
    interval: [f64; 2],
    vertex: bool,
}
fn stations(cage: &Cage, ring: &[usize], config: &Config, samples: &[f64]) -> Result<Vec<Station>> {
    let mut out = Vec::with_capacity(ring.len() * samples.len());
    for (i, (&a, &b)) in ring.iter().zip(ring.iter().cycle().skip(1)).take(ring.len()).enumerate() {
        let (root, edge) = cage
            .faces
            .iter()
            .enumerate()
            .find_map(|(root, f)| (0..4).find(|&j| f.get(j) == Some(&a) && f.get((j + 1) % 4) == Some(&b)).map(|edge| (root, edge)))
            .ok_or_else(|| bad("Match opening lost its source face"))?;
        for &t in samples {
            let (uv, across) = match edge {
                0 => ([t, 0.0], [0.0, 1.0]),
                1 => ([1.0, t], [-1.0, 0.0]),
                2 => ([1.0 - t, 1.0], [0.0, -1.0]),
                _ => ([0.0, 1.0 - t], [1.0, 0.0]),
            };
            out.push(Station {
                root,
                uv,
                across,
                fraction: config.coordinate(i, t)?,
                differential: t > 0.0,
                direction: if config.reverse { -1.0 } else { 1.0 },
                interval: [*config.coordinates.get(i).ok_or_else(|| bad("missing Match coordinate"))?, config.step(i)?],
                vertex: t == 0.0,
            });
        }
    }
    Ok(out)
}
fn jets(cage: &Cage, order: usize, basis: &RootBasis, stations: &[Station]) -> Result<Vec<SurfaceJet>> {
    let eval = evaluator(cage, order)?;
    let surfaces: Vec<_> = basis.fit(&eval)?.iter().map(EvalSurface::from).collect();
    stations
        .iter()
        .map(|s| surfaces.get(s.root).and_then(|surface| surface.jet(s.uv[0], s.uv[1])).ok_or_else(|| bad("Match has no analytic boundary jet")))
        .collect()
}
fn nearby(cage: &Cage, ring: &[usize], order: usize) -> Result<Vec<usize>> {
    let topology = cage.topology()?;
    let mut selected: BTreeSet<_> = ring.iter().copied().collect();
    for _ in 0..order {
        let previous = selected.clone();
        for v in previous {
            for &id in topology.incident.get(v).ok_or_else(|| bad("invalid Match control"))? {
                let e = topology.edges.get(id).ok_or_else(|| bad("invalid Match edge"))?;
                selected.extend([e.a, e.b]);
            }
        }
        if selected.len() > 256 {
            return Err(bad("Match is limited to 256 nearby controls"));
        }
    }
    let chosen: BTreeSet<_> = ring.iter().copied().collect();
    for other in cage.boundary_loops()? {
        if other.iter().any(|v| !chosen.contains(v)) {
            for v in other {
                selected.remove(&v);
            }
        }
    }
    Ok(selected.into_iter().collect())
}
fn qualifies(r: &Report, c: &Config) -> bool {
    r.position_mm <= c.tolerances.position_mm
        && (c.order == 0 || r.tangent_deg <= c.tolerances.tangent_deg)
        && (c.order < 2 || r.curvature_relative <= c.tolerances.curvature_relative)
}
fn score(r: &Report, c: &Config) -> f64 {
    let mut score = (r.position_mm / c.tolerances.position_mm).powi(2);
    if c.order > 0 {
        score += (r.tangent_deg / c.tolerances.tangent_deg).powi(2);
    }
    if c.order > 1 {
        score += (r.curvature_relative / c.tolerances.curvature_relative).powi(2);
    }
    score.sqrt()
}
fn closest(point: Vec3, station: &Station, at: impl Fn(f64) -> Result<Target>) -> Result<Target> {
    let sample = |t: f64| at((station.interval[0] + station.interval[1] * t).rem_euclid(1.0));
    let (mut a, mut b) = (0.0, 1.0);
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    let (mut c, mut d) = (b - ratio * (b - a), a + ratio * (b - a));
    let (mut pc, mut pd) = (sample(c)?, sample(d)?);
    for _ in 0..48 {
        if pc.point.dist(point) <= pd.point.dist(point) {
            b = d;
            d = c;
            pd = pc;
            c = b - ratio * (b - a);
            pc = sample(c)?;
        } else {
            a = c;
            c = d;
            pc = pd;
            d = a + ratio * (b - a);
            pd = sample(d)?;
        }
    }
    [sample(0.0)?, sample(1.0)?, pc, pd]
        .into_iter()
        .min_by(|a, b| a.point.dist(point).total_cmp(&b.point.dist(point)))
        .ok_or_else(|| bad("no target boundary station"))
}
struct CheckContext<'a, F> {
    stations: &'a [Station],
    targets: &'a [Target],
    at: &'a F,
    config: &'a Config,
    diagonal: f64,
    controls: usize,
}
fn measure<F: Fn(f64) -> Result<Target>>(jets: &[SurfaceJet], check: &CheckContext<'_, F>, iterations: usize) -> Result<Report> {
    let config = check.config;
    let mut out = Report { position_mm: 0.0, tangent_deg: 0.0, curvature_relative: 0.0, iterations, controls: check.controls };
    for ((jet, original_target), station) in jets.iter().zip(check.targets).zip(check.stations) {
        let target = if station.vertex || jet.point.dist(original_target.point) <= config.tolerances.position_mm * 0.1 {
            *original_target
        } else {
            closest(jet.point, station, check.at)?
        };
        out.position_mm = out.position_mm.max(jet.point.dist(target.point));
        if config.order == 0 || target.corner {
            continue;
        }
        let normal = jet.normal().ok_or_else(|| {
            bad(&format!("Match produced a singular boundary surface at face {} ({:.4}, {:.4})", station.root + 1, station.uv[0], station.uv[1]))
        })?;
        let target_normal = target.normal.normalized().ok_or_else(|| bad("Match support has no normal"))?;
        out.tangent_deg = out.tangent_deg.max(normal.dot(target_normal).abs().clamp(0.0, 1.0).acos().to_degrees());
        if config.order < 2 {
            continue;
        }
        let a = target.tangent.normalized().ok_or_else(|| bad("Match support has no boundary tangent"))?;
        let b = target_normal.cross(a).normalized().ok_or_else(|| bad("Match support has no tangent plane"))?;
        let mut error = 0.0;
        let mut reference = 0.0;
        let source_sign = if normal.dot(target_normal) < 0.0 { -1.0 } else { 1.0 };
        let target_sign = if target.support.normal().ok_or_else(|| bad("singular Match support"))?.dot(target_normal) < 0.0 { -1.0 } else { 1.0 };
        for (u, v, weight) in [(a, a, 1.0), (a, b, 2.0), (b, b, 1.0)] {
            let source = jet.curvature(u, v).ok_or_else(|| bad("invalid matched curvature"))? * source_sign;
            let target = target.support.curvature(u, v).ok_or_else(|| bad("support curvature is unavailable"))? * target_sign;
            error += (source - target).powi(2) * weight;
            reference += target.powi(2) * weight;
        }
        out.curvature_relative = out.curvature_relative.max(error.sqrt() / reference.sqrt().max(1.0 / check.diagonal.max(1e-6)));
    }
    Ok(out)
}
/// Solve only nearby editable controls. The callback supplies exact support geometry;
/// acceptance is measured at stations that are absent from the constraint system.
pub fn match_opening(cage: &Cage, config: &Config, target: impl Fn(f64) -> Result<Target>, fit_tolerance: f64) -> Result<Solution> {
    cage.validate()?;
    if !fit_tolerance.is_finite() || !(1e-6..=1.0).contains(&fit_tolerance) {
        return Err(bad("Match fit tolerance must be 0.000001..1 mm"));
    }
    let openings = cage.boundary_loops()?;
    if cage.faces.iter().any(|f| f.len() != 4) || openings.is_empty() || (config.order == 2 && openings.len() != 1) {
        return Err(bad("Match requires connected all-quad topology; G2 requires exactly one opening"));
    }
    let ring = opening(cage, config.edge)?;
    config.validate(&ring)?;
    let topology = cage.topology()?;
    let mut seen = BTreeSet::new();
    let mut pending = vec![*ring.first().ok_or_else(|| bad("empty Match opening"))?];
    while let Some(v) = pending.pop() {
        if !seen.insert(v) {
            continue;
        }
        for &id in topology.incident.get(v).ok_or_else(|| bad("invalid Match topology"))? {
            let edge = topology.edges.get(id).ok_or_else(|| bad("invalid Match edge"))?;
            pending.extend([edge.a, edge.b].into_iter().filter(|i| !seen.contains(i)));
        }
    }
    if seen.len() != cage.points.len() {
        return Err(bad("Match requires one connected cage"));
    }
    let boundary: BTreeSet<_> = ring.iter().copied().collect();
    let corners: BTreeSet<_> = cage.vertex_creases.iter().filter(|c| c.sharpness >= 10.0 && boundary.contains(&c.vertex)).map(|c| c.vertex).collect();
    let corner_edges: Vec<_> = if config.order > 0 {
        topology
            .edges
            .iter()
            .filter(|e| boundary.contains(&e.a) != boundary.contains(&e.b) && (corners.contains(&e.a) || corners.contains(&e.b)))
            .map(|e| [e.a, e.b])
            .collect()
    } else {
        Vec::new()
    };
    let prepared = if corner_edges.is_empty() { None } else { Some(cage.with_creases(&corner_edges, &[], 10.0)?) };
    let cage = prepared.as_ref().unwrap_or(cage);
    let controls = nearby(cage, &ring, config.order)?;
    // The CAD-to-cage approximation and the actual CAD-to-support seam are separate
    // measurements. Seam qualification below uses the requested Match tolerances.
    let eval = evaluator(cage, config.order)?;
    let fit = eval.fit_root_patches_at(fit_tolerance, 3)?;
    let basis = fit.root_basis.ok_or_else(|| bad("Match needs a compact common root basis"))?;
    if controls.len().saturating_mul(eval.faces.len()).saturating_mul(1usize << (2 * basis.level)) > 40_000_000 {
        return Err(bad("Match analytic response budget exceeded; reduce nearby control density"));
    }
    let training = stations(cage, &ring, config, &[0.0, 0.25, 0.5, 0.75])?;
    let checks = stations(cage, &ring, config, &[0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875])?;
    let targets = training.iter().map(|s| target(s.fraction)).collect::<Result<Vec<_>>>()?;
    let checked_targets = checks.iter().map(|s| target(s.fraction)).collect::<Result<Vec<_>>>()?;
    for t in targets.iter().chain(&checked_targets) {
        if !t.point.is_finite()
            || t.point.x.abs().max(t.point.y.abs()).max(t.point.z.abs()) > 100_000.0
            || !t.tangent.is_finite()
            || !t.normal.is_finite()
        {
            return Err(bad("Match target geometry must be finite and bounded"));
        }
    }
    let bounds: crate::Aabb3 = targets.iter().map(|t| t.point).collect();
    let diagonal = bounds.size().len();
    if diagonal < 1e-6 {
        return Err(bad("Match target has no boundary extent"));
    }
    let original = jets(cage, config.order, &basis, &training)?;
    let mut current = cage.clone();
    let context = CheckContext { stations: &checks, targets: &checked_targets, at: &target, config, diagonal, controls: controls.len() };
    let mut report = measure(&jets(&current, config.order, &basis, &checks)?, &context, 0)?;
    if qualifies(&report, config) {
        basis.fit_checked(&eval, fit_tolerance)?;
        return Ok(Solution { cage: current, basis, report });
    }
    let response = solve::responses(cage, config.order, &basis, &training, &controls)?;
    for iteration in 0..12 {
        if qualifies(&report, config) {
            basis.fit_checked(&evaluator(&current, config.order)?, fit_tolerance)?;
            return Ok(Solution { cage: current, basis, report });
        }
        let current_jets = jets(&current, config.order, &basis, &training)?;
        // Stored coordinates bind the boundary vertices. Between them, match the
        // geometric curve rather than imposing a piecewise-linear parameter speed
        // that can contradict the smooth subdivision curve's tangent at a knot.
        let sliding_targets = training
            .iter()
            .zip(&current_jets)
            .zip(&targets)
            .map(|((station, jet), bound)| if station.vertex || iteration == 0 { Ok(*bound) } else { closest(jet.point, station, &target) })
            .collect::<Result<Vec<_>>>()?;
        let delta = solve::step(&training, &sliding_targets, &original, &current_jets, &response, config, diagonal)?;
        let mut accepted = None;
        for scale in [1.0, 0.5, 0.25, 0.125, 0.0625, 0.03125] {
            let mut candidate = current.clone();
            for (&id, &d) in controls.iter().zip(&delta) {
                *candidate.points.get_mut(id).ok_or_else(|| bad("invalid Match control"))? += d * scale;
            }
            if candidate.validate().is_err() {
                continue;
            }
            let Ok(candidate_jets) = jets(&candidate, config.order, &basis, &checks) else { continue };
            let Ok(candidate_report) = measure(&candidate_jets, &context, iteration + 1) else { continue };
            if qualifies(&candidate_report, config) || score(&candidate_report, config) < score(&report, config) * (1.0 - 1e-6) {
                accepted = Some((candidate, candidate_report));
                break;
            }
        }
        let Some((candidate, candidate_report)) = accepted else { break };
        current = candidate;
        report = candidate_report;
    }
    if qualifies(&report, config) {
        basis.fit_checked(&evaluator(&current, config.order)?, fit_tolerance)?;
        return Ok(Solution { cage: current, basis, report });
    }
    Err(bad(&format!(
        "Match did not meet the sampled tolerances: position {:.6} mm, tangent {:.6} degrees, curvature {:.6}; add control resolution or adjust the tolerances",
        report.position_mm, report.tangent_deg, report.curvature_relative
    )))
}
