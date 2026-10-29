//! Exact closed support boundaries, sampled only to choose an arc-length parameter.
use super::{Definition, Placement};
use crate::{DocError, Result, kernel};
use solidcraft_geom::{
    Aabb3, Plane, Vec3,
    subdivision::{
        Cage,
        matching::{self, Config, Solution, Target, Tolerances},
    },
    tess::SurfaceJet,
};

/// A normalized parameter on one exact support edge. Unlike a whole-wire
/// arc-length fraction, this stays on the same corner when a face changes aspect.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryCoordinate {
    pub curve: usize,
    pub fraction: f64,
}

#[derive(Clone)]
struct Sample {
    arc: f64,
    curve: usize,
    fraction: f64,
    point: Vec3,
    uv: Option<[f64; 2]>,
    neighbor_uv: Option<[f64; 2]>,
}
#[derive(Clone)]
pub struct BoundarySupport {
    adjacent: Vec<std::sync::Arc<kernel::FaceGeometry>>,
    face: Option<kernel::FaceGeometry>,
    plane: Option<Plane>,
    curves: Vec<kernel::CurveGeometry>,
    samples: Vec<Sample>,
    length: f64,
    corners: Vec<(f64, Vec3)>,
}
fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}
impl BoundarySupport {
    pub fn from_face(body: &kernel::Body, face: usize) -> Result<Self> {
        let geometry = kernel::face_geometry(body, face)?;
        let curves = geometry.loops.get(geometry.outer).cloned().ok_or_else(|| invalid("Match support has no outer boundary"))?;
        Self::new(curves, Some(geometry))
    }
    pub fn from_adjacent_faces(body: &kernel::Body, face: usize) -> Result<Self> {
        let mut support = Self::from_face(body, face)?;
        let outer = support.face.as_ref().ok_or_else(|| invalid("adjacent Match requires a support face"))?.outer;
        let boundaries = kernel::face_boundaries(body, face)?;
        let edges = boundaries.get(outer).ok_or_else(|| invalid("support boundary adjacency is missing"))?;
        if edges.len() != support.curves.len() {
            return Err(invalid("support boundary adjacency disagrees with its exact curves"));
        }
        let mut faces = std::collections::BTreeMap::<usize, std::sync::Arc<kernel::FaceGeometry>>::new();
        for edge in edges {
            let neighbor = edge
                .neighbor
                .ok_or_else(|| invalid("each support edge needs one neighboring face; choose Selected face for a free surface boundary"))?;
            let geometry = match faces.entry(neighbor) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.get().clone(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(std::sync::Arc::new(kernel::face_geometry(body, neighbor)?)).clone()
                }
            };
            support.adjacent.push(geometry);
        }
        let mut hints = vec![None; support.curves.len()];
        for sample in &mut support.samples {
            let face = support.adjacent.get(sample.curve).ok_or_else(|| invalid("support edge has no neighboring surface"))?;
            let hint = hints.get_mut(sample.curve).ok_or_else(|| invalid("invalid adjacent surface seed"))?;
            let uv = face.parameters(sample.point, *hint, 1e-7)?;
            sample.neighbor_uv = Some(uv);
            *hint = Some(uv);
        }
        Ok(support)
    }
    /// Selected edges must make exactly one closed, unbranched wire.
    pub fn from_edges(body: &kernel::Body, edges: &[usize]) -> Result<Self> {
        let unique: std::collections::BTreeSet<_> = edges.iter().copied().collect();
        if edges.is_empty() || edges.len() > 256 || unique.len() != edges.len() {
            return Err(invalid("Match requires 1..256 distinct wire edges"));
        }
        let mut remaining = kernel::edge_geometry(body, edges)?;
        let first = remaining.first().cloned().ok_or_else(|| invalid("empty Match wire"))?;
        remaining.remove(0);
        let mut ordered = vec![first];
        while !remaining.is_empty() {
            let point = ordered.last().and_then(|c| c.at(1.0)).map(|p| p.0).ok_or_else(|| invalid("invalid Match wire endpoint"))?;
            let mut candidates = Vec::new();
            for (i, curve) in remaining.iter().enumerate() {
                if curve.at(0.0).is_some_and(|p| p.0.dist(point) < 1e-7) {
                    candidates.push((i, false));
                } else if curve.at(1.0).is_some_and(|p| p.0.dist(point) < 1e-7) {
                    candidates.push((i, true));
                }
            }
            let [(index, reverse)] = candidates.as_slice() else {
                return Err(invalid("Match edges must form one connected, unbranched wire"));
            };
            let mut curve = remaining.remove(*index);
            if *reverse {
                curve.range.swap(0, 1);
            }
            ordered.push(curve);
        }
        Self::new(ordered, None)
    }
    fn new(curves: Vec<kernel::CurveGeometry>, face: Option<kernel::FaceGeometry>) -> Result<Self> {
        if curves.is_empty() || curves.len() > 1024 {
            return Err(invalid("Match boundary curve budget exceeded"));
        }
        for (curve, next) in curves.iter().zip(curves.iter().cycle().skip(1)) {
            let (a, b) = (
                curve.at(1.0).ok_or_else(|| invalid("invalid support endpoint"))?.0,
                next.at(0.0).ok_or_else(|| invalid("invalid next support endpoint"))?.0,
            );
            if a.dist(b) > 1e-7 {
                return Err(invalid("Match support boundary is not closed"));
            }
        }
        let mut samples = Vec::with_capacity(curves.len() * 33);
        let mut length = 0.0;
        let mut previous = None;
        let mut near = None;
        for (index, curve) in curves.iter().enumerate() {
            for i in 0..=32 {
                let fraction = i as f64 / 32.0;
                let point = curve.at(fraction).ok_or_else(|| invalid("Match support curve is not evaluable"))?.0;
                if let Some(p) = previous.filter(|_| i > 0) {
                    length += point.dist(p);
                }
                let uv = face.as_ref().map(|face| face.parameters(point, near, 1e-7)).transpose()?;
                near = uv;
                samples.push(Sample { arc: length, curve: index, fraction, point, uv, neighbor_uv: None });
                previous = Some(point);
            }
        }
        if !length.is_finite() || length < 1e-7 {
            return Err(invalid("Match support wire has no length"));
        }
        let origin = samples.first().map(|p| p.point).ok_or_else(|| invalid("empty support samples"))?;
        let area = samples.windows(2).fold(Vec3::ZERO, |a, p| a + (p[0].point - origin).cross(p[1].point - origin));
        let bounds: Aabb3 = samples.iter().map(|p| p.point).collect();
        let plane = Plane::from_normal(origin, area)
            .filter(|plane| samples.iter().all(|p| (p.point - origin).dot(plane.normal()).abs() < 1e-7 * bounds.size().len().max(1.0)));
        let mut corners = Vec::new();
        for (i, (curve, next)) in curves.iter().zip(curves.iter().cycle().skip(1)).enumerate() {
            let (point, a) = curve.at(1.0).ok_or_else(|| invalid("invalid boundary corner"))?;
            let (_, b) = next.at(0.0).ok_or_else(|| invalid("invalid next boundary tangent"))?;
            if a.normalized().zip(b.normalized()).is_none_or(|(a, b)| a.dot(b) < 0.999999) {
                let arc = samples.get(i * 33 + 32).ok_or_else(|| invalid("missing corner coordinate"))?.arc;
                corners.push(((arc / length).rem_euclid(1.0), point));
            }
        }
        Ok(Self { face, plane, curves, samples, length, corners, adjacent: Vec::new() })
    }
    pub fn supports_curvature(&self) -> bool {
        self.face.is_some() || self.plane.is_some()
    }
    pub fn coordinate(&self, fraction: f64) -> Result<BoundaryCoordinate> {
        if !fraction.is_finite() {
            return Err(invalid("support coordinate must be finite"));
        }
        let goal = fraction.rem_euclid(1.0) * self.length;
        let next = self.samples.partition_point(|s| s.arc < goal);
        if next == 0 {
            return Ok(BoundaryCoordinate { curve: 0, fraction: 0.0 });
        }
        let a = self.samples.get(next - 1).ok_or_else(|| invalid("invalid support sample"))?;
        let b = self.samples.get(next).ok_or_else(|| invalid("invalid next support sample"))?;
        if a.curve != b.curve || b.arc <= a.arc {
            return Err(invalid("disconnected support coordinate interval"));
        }
        let t = (goal - a.arc) / (b.arc - a.arc);
        Ok(BoundaryCoordinate { curve: a.curve, fraction: a.fraction + (b.fraction - a.fraction) * t })
    }
    pub fn fraction(&self, coordinate: &BoundaryCoordinate) -> Result<f64> {
        if coordinate.curve >= self.curves.len() || !coordinate.fraction.is_finite() || !(0.0..=1.0).contains(&coordinate.fraction) {
            return Err(invalid("saved support coordinate no longer names a bounded edge parameter"));
        }
        let at = coordinate.fraction * 32.0;
        let i = (at.floor() as usize).min(31);
        let a = self.samples.get(coordinate.curve * 33 + i).ok_or_else(|| invalid("missing saved support sample"))?;
        let b = self.samples.get(coordinate.curve * 33 + i + 1).ok_or_else(|| invalid("missing next saved support sample"))?;
        Ok(((a.arc + (b.arc - a.arc) * (at - i as f64)) / self.length).rem_euclid(1.0))
    }
    /// Exact point and tangent; the arc-length table only determines a curve parameter.
    pub fn at(&self, fraction: f64) -> Result<(Vec3, Vec3)> {
        if !fraction.is_finite() {
            return Err(invalid("support coordinate must be finite"));
        }
        let goal = fraction.rem_euclid(1.0) * self.length;
        let next = self.samples.partition_point(|s| s.arc < goal);
        if next == 0 {
            return self.curves.first().and_then(|c| c.at(0.0)).ok_or_else(|| invalid("invalid first support curve"));
        }
        let a = self.samples.get(next - 1).ok_or_else(|| invalid("invalid support sample"))?;
        let b = self.samples.get(next).ok_or_else(|| invalid("support coordinate is outside its sample table"))?;
        if a.curve != b.curve || b.arc <= a.arc {
            return Err(invalid("disconnected support parameter interval"));
        }
        let t = (goal - a.arc) / (b.arc - a.arc);
        self.curves
            .get(a.curve)
            .and_then(|c| c.at(a.fraction + (b.fraction - a.fraction) * t))
            .ok_or_else(|| invalid("support parameter is not evaluable"))
    }
    fn fraction_near(&self, point: Vec3) -> Result<f64> {
        self.samples
            .windows(2)
            .filter_map(|p| {
                let delta = p[1].point - p[0].point;
                if delta.len2() < 1e-20 {
                    return None;
                }
                let t = ((point - p[0].point).dot(delta) / delta.len2()).clamp(0.0, 1.0);
                let distance = (p[0].point + delta * t).dist(point);
                Some((distance, (p[0].arc + (p[1].arc - p[0].arc) * t) / self.length))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|p| p.1.rem_euclid(1.0))
            .ok_or_else(|| invalid("support boundary has no usable segments"))
    }
    fn parameter_seed(&self, fraction: f64, adjacent: bool) -> Option<[f64; 2]> {
        let goal = fraction.rem_euclid(1.0) * self.length;
        let next = self.samples.partition_point(|s| s.arc < goal);
        if next == 0 {
            return if adjacent { self.samples.first()?.neighbor_uv } else { self.samples.first()?.uv };
        }
        let (a, b) = (self.samples.get(next - 1)?, self.samples.get(next)?);
        let (u, v) = if adjacent { (a.neighbor_uv?, b.neighbor_uv?) } else { (a.uv?, b.uv?) };
        let t = (goal - a.arc) / (b.arc - a.arc);
        Some([u[0] + (v[0] - u[0]) * t, u[1] + (v[1] - u[1]) * t])
    }
    /// Initialize one stable, normalized coordinate per boundary vertex.
    pub fn initialize(&self, cage: &Cage, placement: &Placement, edge: [usize; 2], order: usize, tolerances: Tolerances) -> Result<Config> {
        let ring = matching::opening(cage, edge).map_err(|e| invalid(e.to_string()))?;
        let points = ring
            .iter()
            .map(|&v| cage.points.get(v).copied().map(|p| placement.point(p)).ok_or_else(|| invalid("invalid opening vertex")))
            .collect::<Result<Vec<_>>>()?;
        if let Some((coordinates, reverse)) = self.corner_mapping(&points, cage, &ring) {
            return Ok(Config { edge, coordinates, reverse, order, tolerances });
        }
        let phase = self.fraction_near(*points.first().ok_or_else(|| invalid("empty opening"))?)?;
        let mut distances = vec![0.0];
        for p in points.windows(2) {
            distances.push(distances.last().copied().unwrap_or(0.0) + p[0].dist(p[1]));
        }
        let length = distances.last().copied().unwrap_or(0.0) + points.first().zip(points.last()).map_or(0.0, |(a, b)| a.dist(*b));
        if length < 1e-7 {
            return Err(invalid("opening has no length"));
        }
        let mut best = None;
        for reverse in [false, true] {
            let sign = if reverse { -1.0 } else { 1.0 };
            let coordinates = distances.iter().map(|d| (phase + sign * d / length).rem_euclid(1.0)).collect::<Vec<_>>();
            let mut error = 0.0;
            for (point, &fraction) in points.iter().zip(&coordinates) {
                error += point.dist(self.at(fraction)?.0).powi(2);
            }
            if best.as_ref().is_none_or(|(old, _, _)| error < *old) {
                best = Some((error, reverse, coordinates));
            }
        }
        let (_, reverse, mut coordinates) = best.ok_or_else(|| invalid("could not map the Match opening"))?;
        let mut pinned = std::collections::BTreeSet::new();
        for &(corner, _) in &self.corners {
            let distance = |a: f64| {
                let d = (a - corner).abs();
                d.min(1.0 - d)
            };
            let i = coordinates
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| distance(**a).total_cmp(&distance(**b)))
                .map(|p| p.0)
                .ok_or_else(|| invalid("no boundary control for support corner"))?;
            if !pinned.insert(i) {
                return Err(invalid("add boundary control resolution to represent every support corner"));
            }
            *coordinates.get_mut(i).ok_or_else(|| invalid("invalid support corner control"))? = corner;
        }
        Ok(Config { edge, coordinates, reverse, order, tolerances })
    }
    /// Preserve a polygon's actual corners when its initial pose is oblique to
    /// the support. Snapping a global arc phase can otherwise pin a straight
    /// side's midpoint and introduce an artificial singular corner.
    fn corner_mapping(&self, points: &[Vec3], cage: &Cage, ring: &[usize]) -> Option<(Vec<f64>, bool)> {
        if self.corners.len() < 3 || points.len() != ring.len() {
            return None;
        }
        let mut corners = Vec::new();
        for (i, &vertex) in ring.iter().enumerate() {
            let incoming = (*points.get(i)? - *points.get((i + points.len() - 1) % points.len())?).normalized()?;
            let outgoing = (*points.get((i + 1) % points.len())? - *points.get(i)?).normalized()?;
            if incoming.dot(outgoing) < 0.95 || cage.vertex_creases.iter().any(|c| c.vertex == vertex && c.sharpness >= 10.0) {
                corners.push(i);
            }
        }
        if corners.len() != self.corners.len() {
            return None;
        }
        let count = corners.len();
        let mut best = None;
        for reverse in [false, true] {
            for offset in 0..count {
                let mut score = 0.0;
                for (k, &i) in corners.iter().enumerate() {
                    let target = if reverse { (offset + count - k) % count } else { (offset + k) % count };
                    score += points.get(i)?.dist(self.corners.get(target)?.1).powi(2);
                }
                if best.as_ref().is_none_or(|&(old, _, _)| score < old) {
                    best = Some((score, offset, reverse));
                }
            }
        }
        let (_, offset, reverse) = best?;
        let mut coordinates = vec![0.0; points.len()];
        for (k, &start) in corners.iter().enumerate() {
            let end = *corners.get((k + 1) % count)?;
            let steps = (end + points.len() - start) % points.len();
            let length = (0..steps).map(|step| points[(start + step) % points.len()].dist(points[(start + step + 1) % points.len()])).sum::<f64>();
            if length < 1e-10 {
                return None;
            }
            let target = if reverse { (offset + count - k) % count } else { (offset + k) % count };
            let next = if reverse { (target + count - 1) % count } else { (target + 1) % count };
            let a = self.corners.get(target)?.0;
            let b = self.corners.get(next)?.0;
            let interval = if reverse { -(a - b).rem_euclid(1.0) } else { (b - a).rem_euclid(1.0) };
            let mut distance = 0.0;
            for step in 0..steps {
                let i = (start + step) % points.len();
                *coordinates.get_mut(i)? = (a + interval * distance / length).rem_euclid(1.0);
                distance += points.get(i)?.dist(*points.get((i + 1) % points.len())?);
            }
        }
        Some((coordinates, reverse))
    }
    pub fn target(&self, fraction: f64, placement: &Placement, order: usize) -> Result<Target> {
        let (point, tangent) = self.at(fraction)?;
        let adjacent = if self.adjacent.is_empty() { None } else { self.adjacent.get(self.coordinate(fraction)?.curve) };
        let face = adjacent.map(std::sync::Arc::as_ref).or(self.face.as_ref());
        let (support, normal) = if let Some(face) = face {
            let uv = face.parameters(point, self.parameter_seed(fraction, adjacent.is_some()), 1e-7)?;
            let jet = face.surface.jet(uv[0], uv[1]).ok_or_else(|| invalid("Match support has no analytic surface jet"))?;
            let normal = jet.normal().ok_or_else(|| invalid("Match support is singular at its boundary"))? * if face.outward { 1.0 } else { -1.0 };
            (jet, normal)
        } else if let Some(plane) = self.plane {
            (SurfaceJet { point, du: plane.x, dv: plane.y, duu: Vec3::ZERO, duv: Vec3::ZERO, dvv: Vec3::ZERO }, plane.normal())
        } else {
            if order > 0 {
                return Err(invalid("G1/G2 require a surface support or a planar closed wire"));
            }
            (SurfaceJet { point, du: Vec3::ZERO, dv: Vec3::ZERO, duu: Vec3::ZERO, duv: Vec3::ZERO, dvv: Vec3::ZERO }, Vec3::ZERO)
        };
        let local = SurfaceJet {
            point: placement.local_point(support.point),
            du: placement.local_vector(support.du),
            dv: placement.local_vector(support.dv),
            duu: placement.local_vector(support.duu),
            duv: placement.local_vector(support.duv),
            dvv: placement.local_vector(support.dvv),
        };
        Ok(Target {
            corner: self.corners.iter().any(|(_, corner)| corner.dist(point) < (self.length * 1e-8).max(1e-7)),
            point: placement.local_point(point),
            tangent: placement.local_vector(tangent),
            normal: placement.local_vector(normal),
            support: local,
        })
    }
    pub fn solve(&self, definition: &Definition, config: &Config, tolerance: f64) -> Result<Solution> {
        definition.placement.validate()?;
        if definition.hierarchy.is_some() {
            return Err(invalid("Match on a locally refined cage is not supported yet"));
        }
        if definition.symmetry.is_some() {
            return Err(invalid("disable symmetry before matching an external support"));
        }
        let ring = matching::opening(&definition.cage, config.edge).map_err(|e| invalid(e.to_string()))?;
        let mut corners = Vec::new();
        for (&id, &fraction) in ring.iter().zip(&config.coordinates) {
            let point = self.at(fraction)?.0;
            if self.corners.iter().any(|(_, corner)| corner.dist(point) < (self.length * 1e-8).max(1e-7)) {
                corners.push(id);
            }
        }
        let cage = if corners.is_empty() {
            definition.cage.clone()
        } else {
            definition.cage.with_creases(&[], &corners, 10.0).map_err(|e| invalid(e.to_string()))?
        };
        matching::match_opening(
            &cage,
            config,
            |f| self.target(f, &definition.placement, config.order).map_err(|e| solidcraft_geom::subdivision::SubdivisionError(e.to_string())),
            tolerance,
        )
        .map_err(|e| invalid(e.to_string()))
    }
}
