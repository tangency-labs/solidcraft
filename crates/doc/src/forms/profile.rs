//! A CAD face's fixed rim and an editable grid of interior guide positions.
use super::{
    Definition, Placement, SupportRef,
    support::{BoundaryBinding, BoundaryCoordinate, BoundarySupport},
};
use crate::{DocError, ModelBody, ModelState, Result, kernel};
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_geom::subdivision::Cage;
use std::collections::BTreeSet;

mod partition;
pub(super) mod quilt;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default, skip_serializing_if = "is_false")]
    pub partition: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart: Option<ProfileChart>,
    pub source: SupportRef,
    pub segments: [usize; 2],
    /// Position (0) or tangent (1) continuity at the original CAD boundary.
    pub continuity: usize,
    /// Undeformed samples in the Form's local frame. Recompute preserves offsets.
    pub rest: Vec<Vec3>,
}
/// A logical UV rectangle tied to exact named boundary curves, independent of
/// the kernel's current surface parameter axes and trim-loop seam.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileChart {
    pub binding: BoundaryBinding,
    /// Exact rim stations expressed in the saved logical normalized UV frame.
    pub anchors: [ProfileAnchor; 16],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileAnchor {
    pub coordinate: BoundaryCoordinate,
    pub uv: [f64; 2],
}
pub struct ProfileFit {
    pub patch_roots: Option<Vec<usize>>,
    pub open: kernel::Body,
    pub patch: kernel::Body,
    pub solid: kernel::Body,
    pub report: kernel::PatchReport,
}
fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}
fn is_false(value: &bool) -> bool {
    !*value
}

impl Profile {
    /// Guide geometry while an exact CAD fit is pending. These are interpolation
    /// guides, so subdivision shrinkage must never move the boundary handles.
    pub fn preview(&self, cage: &Cage) -> Result<solidcraft_geom::subdivision::SurfacePreview> {
        self.validate(cage)?;
        let mut mesh = solidcraft_geom::Mesh { positions: cage.points.clone(), ..Default::default() };
        for (id, face) in cage.faces.iter().enumerate() {
            let &[a, b, c, d] = face.as_slice() else {
                return Err(invalid("invalid Form Surface guide quad"));
            };
            for triangle in [[a as u32, b as u32, c as u32], [a as u32, c as u32, d as u32]] {
                mesh.triangles.push(triangle);
                mesh.tri_face.push(if self.partition { id as u32 } else { u32::MAX });
            }
        }
        mesh.normals = vec![Vec3::ZERO; mesh.positions.len()];
        for triangle in &mesh.triangles {
            let Some([a, b, c]) = mesh.tri(triangle) else {
                continue;
            };
            let normal = (b - a).cross(c - a);
            for &i in triangle {
                if let Some(n) = mesh.normals.get_mut(i as usize) {
                    *n += normal;
                }
            }
        }
        for normal in &mut mesh.normals {
            *normal = normal.normalized().unwrap_or(Vec3::Z);
        }
        let boundaries = cage
            .edges()
            .map_err(|e| invalid(e.to_string()))?
            .into_iter()
            .map(|edge| (edge, vec![cage.points[edge[0]], cage.points[edge[1]]]))
            .collect();
        let index = solidcraft_geom::MeshIndex::new(&mesh);
        Ok(solidcraft_geom::subdivision::SurfacePreview { mesh, vertices: cage.points.clone(), boundaries, index })
    }
    /// CAD display and editable guide edges evaluated on the same filled surface.
    pub fn display(
        &self,
        state: &ModelState,
        cage: &Cage,
        placement: &Placement,
        tolerance: f64,
    ) -> Result<(solidcraft_geom::Mesh, super::ConversionReport)> {
        let fit = self.fit(state, cage, placement, tolerance)?;
        let roots = fit.patch_roots.clone();
        let patch = fit.patch;
        let surface = kernel::face_geometry(&patch, 0)?;
        let mut mesh = patch.mesh(&kernel::MeshTolerance::chord(tolerance.min(0.05)))?;
        let (display_mesh, shared_guides) = partition::display_mesh(&patch, roots.as_deref(), self, cage, placement, tolerance, mesh)?;
        mesh = display_mesh;
        let mut maximum = fit.report.gap.max(fit.report.point_gap);
        let edges = cage.edges().map_err(|e| invalid(e.to_string()))?;
        mesh.edges = if let Some(guides) = shared_guides {
            for &point in &cage.points {
                let point = placement.point(point);
                let uv = surface.parameters(point, None, tolerance)?;
                maximum = maximum.max(surface.surface.eval(uv[0], uv[1]).0.dist(point));
            }
            guides
        } else if roots.is_none() && patch.face_count() > 1 {
            let (source, face) = self.resolve(state)?;
            let rim = QuiltRim::new(self, cage, placement, &source.body, face)?;
            let original = quilt_guide_edges(&patch, cage, placement, &edges, &rim)?;
            match quilt::display(&patch, &source.body, &edges, &rim, &original, tolerance) {
                Ok(sampled) => {
                    mesh = sampled.mesh;
                    sampled.guides
                }
                Err(reason) => {
                    log::warn!("Form Surface quilt retains independent display guides: {reason}");
                    original
                }
            }
        } else {
            let parameters = cage
                .points
                .iter()
                .map(|&p| {
                    let point = placement.point(p);
                    let uv = surface.parameters(point, None, tolerance)?;
                    maximum = maximum.max(surface.surface.eval(uv[0], uv[1]).0.dist(point));
                    Ok(uv)
                })
                .collect::<Result<Vec<_>>>()?;
            edges
                .iter()
                .map(|&[a, b]| {
                    let (Some(a), Some(b)) = (parameters.get(a), parameters.get(b)) else {
                        return Err(invalid("a guide edge lost an endpoint"));
                    };
                    let curve = (0..=24)
                        .map(|i| {
                            let t = i as f64 / 24.0;
                            surface.surface.eval(a[0] * (1.0 - t) + b[0] * t, a[1] * (1.0 - t) + b[1] * t).0
                        })
                        .collect::<Vec<_>>();
                    if curve.iter().any(|p| !p.is_finite()) {
                        return Err(invalid("the filled guide edge is not finite"));
                    }
                    Ok(curve)
                })
                .collect::<Result<_>>()?
        };
        mesh.edge_faces = if roots.is_some() {
            edges
                .iter()
                .map(|edge| {
                    cage.faces
                        .iter()
                        .enumerate()
                        .filter(|(_, face)| {
                            face.iter().zip(face.iter().cycle().skip(1)).take(face.len()).any(|(&a, &b)| [a.min(b), a.max(b)] == *edge)
                        })
                        .map(|(id, _)| id as u32)
                        .collect()
                })
                .collect()
        } else {
            vec![vec![u32::MAX]; mesh.edges.len()]
        };
        mesh.seams = vec![false; mesh.edges.len()];
        if let Some(roots) = &roots {
            for face in &mut mesh.tri_face {
                *face = *roots.get(*face as usize).ok_or_else(|| invalid("missing Surface CAD cell identity"))? as u32;
            }
        } else {
            mesh.tri_face.fill(u32::MAX);
        }
        Ok((
            mesh,
            super::ConversionReport {
                sampled_deviation_mm: maximum,
                subdivision_level: 0,
                patches: patch.face_count(),
                patch_roots: roots.unwrap_or_else(|| vec![0; patch.face_count()]),
                root_surfaces: 1,
                trimmed_roots: true,
                solid: false,
            },
        ))
    }
    pub fn validate(&self, cage: &Cage) -> Result<()> {
        self.source.validate()?;
        if let Some(chart) = &self.chart {
            chart.binding.validate()?;
            if chart.anchors.iter().any(|anchor| {
                !anchor.coordinate.fraction.is_finite()
                    || !(0.0..=1.0).contains(&anchor.coordinate.fraction)
                    || anchor.uv.iter().any(|v| !v.is_finite() || !(-1e-7..=1.0 + 1e-7).contains(v))
            }) {
                return Err(invalid("invalid Form Surface chart anchor"));
            }
        }
        if !matches!(self.source, SupportRef::Face { .. })
            || self.continuity > 1
            || self.segments.iter().any(|n| !(1..=16).contains(n))
            || self.rest.len() != (self.segments[0] + 1) * (self.segments[1] + 1)
            || self.rest.len() != cage.points.len()
            || self.rest.iter().any(|p| !p.is_finite() || p.len() > 100_000.0)
        {
            return Err(invalid("a Form Surface requires one face, G0/G1 and a bounded regular guide grid"));
        }
        let faces = grid_faces(self.segments, false);
        let reversed = grid_faces(self.segments, true);
        if cage.faces != faces && cage.faces != reversed {
            return Err(invalid("the Form Surface guide grid changed topology"));
        }
        cage.validate().map_err(|e| invalid(e.to_string()))
    }
    pub fn resolve<'a>(&self, state: &'a ModelState) -> Result<(&'a ModelBody, usize)> {
        let SupportRef::Face { body, name, pick } = &self.source else {
            return Err(invalid("Form Surface requires a face reference"));
        };
        let body = state.body(body).ok_or_else(|| invalid("the Form Surface source body is missing"))?;
        let (index, _) = crate::naming::face_on_body(body, name, *pick).ok_or_else(|| invalid("the Form Surface source face is missing"))?;
        Ok((body, index))
    }
    pub fn boundary(&self) -> BTreeSet<usize> {
        if self.segments.iter().any(|n| !(1..=16).contains(n)) {
            return BTreeSet::new();
        }
        let [u, v] = self.segments;
        (0..=v).flat_map(|j| (0..=u).filter(move |&i| i == 0 || i == u || j == 0 || j == v).map(move |i| j * (u + 1) + i)).collect()
    }
    pub fn seed(body: &ModelBody, face: usize, segments: [usize; 2], continuity: usize) -> Result<(Self, Cage)> {
        if segments.iter().any(|n| !(1..=16).contains(n)) || continuity > 1 {
            return Err(invalid("invalid Form Surface resolution or continuity"));
        }
        let source = SupportRef::face(body, face)?;
        let geometry = kernel::face_geometry(&body.body, face)?;
        if body.body.is_surface() || body.body.shell_count() != 1 || geometry.loops.len() != 1 {
            return Err(invalid("Form Surface needs one closed solid and a face with one boundary loop"));
        }
        let rest = sample(&body.body, face, segments)?;
        let cage = Cage { points: rest.clone(), faces: grid_faces(segments, !geometry.outward), ..Default::default() };
        let mut state = ModelState::default();
        state.bodies.push(body.clone());
        let (support, binding) = source.resolve_bound(&state, false, None)?;
        let chart = Some(ProfileChart::capture(binding, &support, &body.body, face, ChartOrientation::default())?);
        let profile = Self { partition: true, chart, source, segments, continuity, rest };
        profile.validate(&cage)?;
        Ok((profile, cage))
    }
    pub fn recompute(&self, definition: &Definition, state: &ModelState) -> Result<(Self, Cage)> {
        self.validate(&definition.cage)?;
        definition.placement.validate()?;
        let (body, index) = self.resolve(state)?;
        let (support, binding) = self.source.resolve_bound(state, false, self.chart.as_ref().map(|chart| &chart.binding))?;
        let candidates = chart_samples(&body.body, index, self.segments)?;
        let (chart, mut rest) = if let Some(chart) = &self.chart {
            let geometry = kernel::face_geometry(&body.body, index)?;
            let span = kernel::face_parameter_span(&body.body, index)?;
            let anchors = chart.anchors.iter().map(|anchor| support.at_coordinate(&anchor.coordinate).map(|p| p.0)).collect::<Result<Vec<_>>>()?;
            let (_, world) = unique_grid(candidates, |orientation, _| {
                chart.anchors.iter().zip(&anchors).all(|(anchor, point)| {
                    let uv = orientation.map(anchor.uv);
                    let a = span[0][0] + (span[0][1] - span[0][0]) * uv[0];
                    let b = span[1][0] + (span[1][1] - span[1][0]) * uv[1];
                    geometry.surface.eval(a, b).0.dist(*point) <= 1e-7
                })
            })?;
            let rest = world.into_iter().map(|p| definition.placement.local_point(p)).collect();
            (ProfileChart { binding, anchors: chart.anchors.clone() }, rest)
        } else {
            // Legacy samples certify their own logical chart only while the
            // physical source is unchanged. An already moved support is not a
            // licence to guess a seam or rotate an asymmetric sculpt.
            let (orientation, _) =
                unique_grid(candidates, |_, grid| grid.iter().zip(&self.rest).all(|(a, b)| definition.placement.local_point(*a).dist(*b) <= 1e-7))?;
            let chart = ProfileChart::capture(binding, &support, &body.body, index, orientation)?;
            (chart, self.rest.clone())
        };
        // Reordering a chart may introduce a few arithmetic ulps (for example
        // 1 - 2/3 versus 1/3). Do not rewrite an unchanged authored grid for
        // that roundoff; this bound is unrelated to the CAD fit tolerance.
        for (point, old) in rest.iter_mut().zip(&self.rest) {
            let roundoff = 64.0 * f64::EPSILON * point.len().max(old.len()).max(1.0);
            if point.dist(*old) <= roundoff {
                *point = *old;
            }
        }
        let boundary = self.boundary();
        let mut cage = definition.cage.clone();
        if rest != self.rest {
            for (i, point) in cage.points.iter_mut().enumerate() {
                let (Some(old), Some(new)) = (self.rest.get(i), rest.get(i)) else {
                    return Err(invalid("Form Surface lost a guide sample"));
                };
                *point = if boundary.contains(&i) {
                    *new
                } else {
                    let delta = *point - *old;
                    let before = grid_frame(&self.rest, self.segments, i)?;
                    let after = grid_frame(&rest, self.segments, i)?;
                    // Preserve sculpted lengths in millimetres. Stretch and
                    // shear rotate the material frame without scaling details.
                    *new + after[0] * delta.dot(before[0]) + after[1] * delta.dot(before[1]) + after[2] * delta.dot(before[2])
                };
            }
        }
        let mut profile = self.clone();
        profile.chart = Some(chart);
        profile.rest = rest;
        profile.validate(&cage)?;
        Ok((profile, cage))
    }
    pub fn transform_picks(&mut self, matrix: &crate::Mat) {
        self.source.transform_picks(matrix);
        if let Some(chart) = &mut self.chart {
            chart.binding.transform_picks(matrix);
        }
    }
    /// Explicitly replace an unavailable association using the user's chosen
    /// logical U/V correspondence. Keep the existing grid and sculpted lengths.
    pub fn reattach(
        &self,
        definition: &Definition,
        state: &ModelState,
        source: SupportRef,
        [swap, reverse_u, reverse_v]: [bool; 3],
    ) -> Result<(Self, Cage)> {
        self.validate(&definition.cage)?;
        definition.placement.validate()?;
        let mut selected = self.clone();
        selected.source = source;
        let (body, face) = selected.resolve(state)?;
        let (mut profile, _) = Self::seed(body, face, self.segments, self.continuity)?;
        profile.partition = self.partition;
        let orientation = ChartOrientation { swap, reverse_u, reverse_v };
        let (_, world) = chart_samples(&body.body, face, self.segments)?
            .into_iter()
            .find(|(candidate, _)| *candidate == orientation)
            .ok_or_else(|| invalid("invalid explicit Form Surface chart mapping"))?;
        let rest: Vec<_> = world.into_iter().map(|p| definition.placement.local_point(p)).collect();
        let (support, binding) = profile.source.resolve_bound(state, false, None)?;
        profile.chart = Some(ProfileChart::capture(binding, &support, &body.body, face, orientation)?);
        let boundary = self.boundary();
        let mut cage = definition.cage.clone();
        for (index, point) in cage.points.iter_mut().enumerate() {
            let old = self.rest.get(index).ok_or_else(|| invalid("missing retained Surface guide"))?;
            let new = rest.get(index).ok_or_else(|| invalid("missing reattached Surface guide"))?;
            *point = if boundary.contains(&index) {
                *new
            } else {
                let delta = *point - *old;
                let before = grid_frame(&self.rest, self.segments, index)?;
                let after = grid_frame(&rest, self.segments, index)?;
                *new + after[0] * delta.dot(before[0]) + after[1] * delta.dot(before[1]) + after[2] * delta.dot(before[2])
            };
        }
        profile.rest = rest;
        profile.validate(&cage)?;
        Ok((profile, cage))
    }
    pub(crate) fn remap_names(&mut self, features: &std::collections::BTreeMap<u64, u64>, bodies: &std::collections::BTreeMap<String, String>) {
        self.source.remap_names(features, bodies);
        if let Some(chart) = &mut self.chart {
            chart.binding.remap_names(features, bodies);
        }
    }
    /// Fit the original rim through the interior guides; leave the input untouched on failure.
    /// History of `open` is over `[source]`, `patch` over `[open]`, `solid` over `[open, patch]`.
    pub fn fit(&self, state: &ModelState, cage: &Cage, placement: &Placement, tolerance: f64) -> Result<ProfileFit> {
        self.validate(cage)?;
        placement.validate()?;
        if !tolerance.is_finite() || !(1e-6..=1.0).contains(&tolerance) {
            return Err(invalid("invalid Form Surface tolerance"));
        }
        let (source, index) = self.resolve(state)?;
        let geometry = kernel::face_geometry(&source.body, index)?;
        if geometry.loops.len() != 1 || source.body.is_surface() || source.body.shell_count() != 1 {
            return Err(invalid("Form Surface currently requires one closed source solid and a face with one boundary loop"));
        }
        let picks = geometry
            .loops
            .iter()
            .flatten()
            .map(|edge| edge.at(0.5).map(|p| p.0).ok_or_else(|| invalid("invalid Form Surface rim")))
            .collect::<Result<Vec<_>>>()?;
        let open = kernel::remove_faces(&source.body, &[index])?;
        let boundary = self.boundary();
        let guides: Vec<_> = cage.points.iter().enumerate().filter(|(i, _)| !boundary.contains(i)).map(|(_, p)| placement.point(*p)).collect();
        let maximum = solidcraft_geom::fill_general::MAX_FILL_POINTS;
        let mut selected: BTreeSet<usize> = (0..guides.len().min(maximum)).map(|i| i * guides.len() / guides.len().min(maximum).max(1)).collect();
        for _ in 0..4 {
            let spec = kernel::PatchSpec {
                continuity: if self.continuity == 0 { kernel::Continuity::G0 } else { kernel::Continuity::G1 },
                points: selected.iter().filter_map(|&i| guides.get(i).copied()).collect(),
                ..Default::default()
            };
            let (patch, report) = kernel::patch_edges_report(&open, &picks, &spec)?;
            if report.gap > tolerance || report.point_gap > tolerance {
                return Err(invalid("the Form Surface fit exceeds its requested tolerance"));
            }
            let fitted = (0..patch.face_count()).map(|face| kernel::face_geometry(&patch, face)).collect::<kernel::Result<Vec<_>>>()?;
            let missing: Vec<_> = guides
                .iter()
                .enumerate()
                .filter_map(|(i, p)| (!fitted.iter().any(|face| face.parameters(*p, None, tolerance).is_ok())).then_some(i))
                .collect();
            if missing.is_empty() {
                if let Ok(Some(partition)) = partition::build(&patch, self, cage, placement, tolerance)
                    && let Ok(solid) = kernel::stitch_split(&[&open, &partition.body], tolerance)
                    && !solid.is_surface()
                    && solid.shell_count() == 1
                    && solid.validity().is_empty()
                {
                    return Ok(ProfileFit { open, patch: partition.body, solid, report, patch_roots: Some(partition.roots) });
                }
                let solid = kernel::stitch(&[&open, &patch], tolerance)?;
                if solid.is_surface() || solid.shell_count() != 1 || !solid.validity().is_empty() {
                    return Err(invalid("the Form Surface did not sew into one valid solid"));
                }
                return Ok(ProfileFit { open, patch, solid, report, patch_roots: None });
            }
            let previous = selected.len();
            for i in missing {
                if selected.len() == maximum {
                    break;
                }
                selected.insert(i);
            }
            if selected.len() == previous {
                return Err(invalid("the Form Surface could not meet every interior guide within tolerance"));
            }
        }
        Err(invalid("the Form Surface guide fit did not converge"))
    }
}
/// Display guide curves on a filled quilt's exact bounded charts. A trimmed
/// source's rectangular guide box can extend outside its rim; only interior
/// authored guides are fit constraints. Their strict fit is checked above.
pub(super) fn quilt_guide_edges(
    patch: &kernel::Body,
    cage: &Cage,
    placement: &Placement,
    edges: &[[usize; 2]],
    rim: &QuiltRim,
) -> Result<Vec<Vec<Vec3>>> {
    use solidcraft_geom::tess::uv;
    let surfaces = (0..patch.face_count()).map(|face| kernel::face_geometry(patch, face)).collect::<kernel::Result<Vec<_>>>()?;
    let domains: Vec<_> = surfaces.iter().map(|face| uv::domain(&face.surface)).collect();
    let starts: Vec<_> = surfaces.iter().map(|_| uv::Starts::default()).collect();
    edges
        .iter()
        .map(|&edge| {
            let [a, b] = edge;
            let (Some(a), Some(b)) = (cage.points.get(a), cage.points.get(b)) else {
                return Err(invalid("a guide edge lost an endpoint"));
            };
            let (a, b) = (placement.point(*a), placement.point(*b));
            let a = rim.sample(edge, 0.0)?.unwrap_or(a);
            let b = rim.sample(edge, 1.0)?.unwrap_or(b);
            let mut hints = vec![None; surfaces.len()];
            rim.stations(edge)?
                .into_iter()
                .map(|t| {
                    if let Some(point) = rim.sample(edge, t)? {
                        return Ok(point);
                    }
                    let point = a * (1.0 - t) + b * t;
                    let mut nearest: Option<(f64, Vec3)> = None;
                    for (((face, domain), starts), hint) in surfaces.iter().zip(&domains).zip(&starts).zip(&mut hints) {
                        let Some(parameters) = uv::project_with(&face.surface, domain, starts, point, *hint, f64::INFINITY) else {
                            continue;
                        };
                        *hint = Some(parameters);
                        let candidate = face.surface.eval(parameters[0], parameters[1]).0;
                        let distance = candidate.dist(point);
                        if candidate.is_finite() && distance.is_finite() && nearest.is_none_or(|(best, _)| distance < best) {
                            nearest = Some((distance, candidate));
                        }
                    }
                    nearest.map(|(_, p)| p).ok_or_else(|| invalid("the filled guide edge has no finite point on its CAD quilt"))
                })
                .collect()
        })
        .collect()
}
/// Coordinates belong to the immutable undeformed guide topology and the source
/// rim, never to the edited cage or to a nearest point on the deformed quilt.
pub(super) struct QuiltRim {
    support: BoundarySupport,
    ring: Vec<usize>,
    coordinates: std::collections::BTreeMap<usize, f64>,
    reverse: bool,
}
impl QuiltRim {
    pub(super) fn new(profile: &Profile, cage: &Cage, placement: &Placement, source: &kernel::Body, face: usize) -> Result<Self> {
        let support = BoundarySupport::from_face(source, face)?;
        let mut rest = cage.clone();
        rest.points = profile.rest.clone();
        let mut rings = rest.boundary_loops().map_err(|e| invalid(e.to_string()))?;
        if rings.len() != 1 {
            return Err(invalid("Form Surface display requires one logical guide perimeter"));
        }
        let ring = rings.pop().ok_or_else(|| invalid("Form Surface display lost its logical perimeter"))?;
        let points = ring
            .iter()
            .map(|&id| rest.points.get(id).copied().map(|p| placement.point(p)).ok_or_else(|| invalid("missing undeformed guide boundary point")))
            .collect::<Result<Vec<_>>>()?;
        let phase = support.fraction_near(*points.first().ok_or_else(|| invalid("empty undeformed guide perimeter"))?)?;
        let mut distances = vec![0.0];
        for pair in points.windows(2) {
            distances.push(distances.last().copied().unwrap_or(0.0) + pair[0].dist(pair[1]));
        }
        let length = distances.last().copied().unwrap_or(0.0) + points.first().zip(points.last()).map_or(0.0, |(a, b)| a.dist(*b));
        if !length.is_finite() || length < 1e-7 {
            return Err(invalid("undeformed Form Surface guide perimeter has no finite length"));
        }
        // Surface's rectangular scaffold need not lie on the trimmed source.
        // Unlike Match, displaying it must not pin one control per source corner.
        let mut best = None;
        for reverse in [false, true] {
            let sign = if reverse { -1.0 } else { 1.0 };
            let coordinates: Vec<_> = distances.iter().map(|d| (phase + sign * d / length).rem_euclid(1.0)).collect();
            let mut error = 0.0;
            for (point, &fraction) in points.iter().zip(&coordinates) {
                error += point.dist(support.at(fraction)?.0).powi(2);
            }
            if !error.is_finite() {
                return Err(invalid("Form Surface display has invalid fixed rim coordinates"));
            }
            if best.as_ref().is_none_or(|(old, _, _)| error < *old) {
                best = Some((error, reverse, coordinates));
            }
        }
        let (_, reverse, coordinates) = best.ok_or_else(|| invalid("could not orient the fixed Form Surface rim"))?;
        Ok(Self { support, coordinates: ring.iter().copied().zip(coordinates).collect(), ring, reverse })
    }
    pub(super) fn sample(&self, [a, b]: [usize; 2], t: f64) -> Result<Option<Vec3>> {
        let at = |fraction| self.support.at(fraction).map(|p| p.0);
        if t == 0.0 {
            return self.coordinates.get(&a).copied().map(at).transpose();
        }
        if t == 1.0 {
            return self.coordinates.get(&b).copied().map(at).transpose();
        }
        let Some((start, span)) = self.arc([a, b])? else { return Ok(None) };
        at((start + t * span).rem_euclid(1.0)).map(Some)
    }
    pub(super) fn stations(&self, edge: [usize; 2]) -> Result<Vec<f64>> {
        let mut stations: Vec<_> = (0..=24).map(|i| i as f64 / 24.0).collect();
        if let Some((start, span)) = self.arc(edge)? {
            for corner in self.support.corner_fractions() {
                let distance = if span > 0.0 { (corner - start).rem_euclid(1.0) } else { -(start - corner).rem_euclid(1.0) };
                let t = distance / span;
                if t.is_finite() && t > 0.0 && t < 1.0 {
                    stations.push(t);
                }
            }
            stations.sort_by(f64::total_cmp);
            stations.dedup();
        }
        Ok(stations)
    }
    fn arc(&self, [a, b]: [usize; 2]) -> Result<Option<(f64, f64)>> {
        let Some(index) = self.ring.iter().position(|&id| id == a) else { return Ok(None) };
        let forward = self.ring.get((index + 1) % self.ring.len()).is_some_and(|&id| id == b);
        let backward = self.ring.get((index + self.ring.len() - 1) % self.ring.len()).is_some_and(|&id| id == b);
        // Membership of the actual boundary loop, rather than two fixed IDs,
        // distinguishes a perimeter edge from an interior guide chord.
        if !forward && !backward {
            return Ok(None);
        }
        let (Some(&start), Some(&end)) = (self.coordinates.get(&a), self.coordinates.get(&b)) else {
            return Err(invalid("Form Surface display lost a fixed rim coordinate"));
        };
        let increasing = forward != self.reverse;
        let span = if increasing { (end - start).rem_euclid(1.0) } else { -(start - end).rem_euclid(1.0) };
        Ok(Some((start, span)))
    }
}
fn grid_faces([u, v]: [usize; 2], reverse: bool) -> Vec<Vec<usize>> {
    (0..v)
        .flat_map(|j| {
            (0..u).map(move |i| {
                let a = j * (u + 1) + i;
                let mut face = vec![a, a + 1, a + u + 2, a + u + 1];
                if reverse {
                    face.reverse();
                }
                face
            })
        })
        .collect()
}
fn sample(body: &kernel::Body, face: usize, [u, v]: [usize; 2]) -> Result<Vec<Vec3>> {
    let geometry = kernel::face_geometry(body, face)?;
    let span = kernel::face_parameter_span(body, face)?;
    let mut points = Vec::with_capacity((u + 1) * (v + 1));
    for j in 0..=v {
        for i in 0..=u {
            let a = span[0][0] + (span[0][1] - span[0][0]) * i as f64 / u as f64;
            let b = span[1][0] + (span[1][1] - span[1][0]) * j as f64 / v as f64;
            let p = geometry.surface.eval(a, b).0;
            if !p.is_finite() || p.len() > 100_000.0 {
                return Err(invalid("the source face has no finite bounded guide grid"));
            }
            points.push(p);
        }
    }
    Ok(points)
}

impl ProfileChart {
    fn capture(binding: BoundaryBinding, support: &BoundarySupport, body: &kernel::Body, face: usize, orientation: ChartOrientation) -> Result<Self> {
        let geometry = kernel::face_geometry(body, face)?;
        let span = kernel::face_parameter_span(body, face)?;
        let center = Some([(span[0][0] + span[0][1]) * 0.5, (span[1][0] + span[1][1]) * 0.5]);
        let mut anchors = Vec::with_capacity(16);
        for index in 0..16 {
            let coordinate = support.coordinate(index as f64 / 16.0)?;
            let point = support.at_coordinate(&coordinate)?.0;
            let parameters = geometry.parameters(point, center, 1e-7)?;
            let uv = [(parameters[0] - span[0][0]) / (span[0][1] - span[0][0]), (parameters[1] - span[1][0]) / (span[1][1] - span[1][0])];
            anchors.push(ProfileAnchor { coordinate, uv: orientation.inverse(uv) });
        }
        let anchors = anchors.try_into().map_err(|_| invalid("missing Form Surface chart anchors"))?;
        Ok(Self { binding, anchors })
    }
}
#[derive(Clone, Copy, Default, Eq, PartialEq)]
struct ChartOrientation {
    swap: bool,
    reverse_u: bool,
    reverse_v: bool,
}
impl ChartOrientation {
    fn map(self, [u, v]: [f64; 2]) -> [f64; 2] {
        let [a, b] = if self.swap { [v, u] } else { [u, v] };
        [if self.reverse_u { 1.0 - a } else { a }, if self.reverse_v { 1.0 - b } else { b }]
    }
    fn inverse(self, [u, v]: [f64; 2]) -> [f64; 2] {
        let [a, b] = [if self.reverse_u { 1.0 - u } else { u }, if self.reverse_v { 1.0 - v } else { v }];
        if self.swap { [b, a] } else { [a, b] }
    }
}
fn unique_grid(
    candidates: Vec<(ChartOrientation, Vec<Vec3>)>,
    accepts: impl Fn(ChartOrientation, &[Vec3]) -> bool,
) -> Result<(ChartOrientation, Vec<Vec3>)> {
    let mut matches = candidates.into_iter().filter(|(orientation, grid)| accepts(*orientation, grid));
    let grid =
        matches.next().ok_or_else(|| invalid("Form Surface source chart cannot be certified; retain the saved source and reattach explicitly"))?;
    if matches.next().is_some() {
        return Err(invalid("Form Surface source chart is ambiguous; retain the saved source and reattach explicitly"));
    }
    Ok(grid)
}
/// Enumerate the eight logical orientations. Evaluating normalized logical UV
/// directly also handles unequal U/V sampling when native axes are exchanged.
fn chart_samples(body: &kernel::Body, face: usize, segments: [usize; 2]) -> Result<Vec<(ChartOrientation, Vec<Vec3>)>> {
    let geometry = kernel::face_geometry(body, face)?;
    let span = kernel::face_parameter_span(body, face)?;
    let [u, v] = segments;
    let mut grids = Vec::with_capacity(8);
    for swap in [false, true] {
        for reverse_u in [false, true] {
            for reverse_v in [false, true] {
                let orientation = ChartOrientation { swap, reverse_u, reverse_v };
                let mut grid = Vec::with_capacity((u + 1) * (v + 1));
                for j in 0..=v {
                    for i in 0..=u {
                        let uv = orientation.map([i as f64 / u as f64, j as f64 / v as f64]);
                        let a = span[0][0] + (span[0][1] - span[0][0]) * uv[0];
                        let b = span[1][0] + (span[1][1] - span[1][0]) * uv[1];
                        let p = geometry.surface.eval(a, b).0;
                        if !p.is_finite() || p.len() > 100_000.0 {
                            return Err(invalid("the source face has no finite bounded guide grid"));
                        }
                        grid.push(p);
                    }
                }
                grids.push((orientation, grid));
            }
        }
    }
    Ok(grids)
}

fn grid_frame(points: &[Vec3], [u, v]: [usize; 2], index: usize) -> Result<[Vec3; 3]> {
    let i = index % (u + 1);
    let j = index / (u + 1);
    if i == 0 || i >= u || j == 0 || j >= v {
        return Err(invalid("an interior Form Surface guide needs two logical tangent directions"));
    }
    let at = |id| points.get(id).copied().ok_or_else(|| invalid("missing Form Surface tangent sample"));
    let x = (at(index + 1)? - at(index - 1)?).normalized().ok_or_else(|| invalid("singular Form Surface U tangent"))?;
    let across = at(index + u + 1)? - at(index - u - 1)?;
    let z = x.cross(across).normalized().ok_or_else(|| invalid("singular Form Surface guide frame"))?;
    let y = z.cross(x);
    Ok([x, y, z])
}
impl crate::walk::Walk for Profile {
    fn walk_refs<'a>(&'a self, f: &mut dyn FnMut(crate::walk::RefItem<'a>)) {
        crate::walk::Walk::walk_refs(&self.source, f);
        if let Some(chart) = &self.chart {
            crate::walk::Walk::walk_refs(&chart.binding, f);
        }
    }
    fn walk_refs_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::RefItemMut<'a>)) {
        crate::walk::Walk::walk_refs_mut(&mut self.source, f);
        if let Some(chart) = &mut self.chart {
            crate::walk::Walk::walk_refs_mut(&mut chart.binding, f);
        }
    }
    fn walk_geometry<'a>(&'a self, f: &mut dyn FnMut(crate::walk::GeoItem<'a>)) {
        crate::walk::Walk::walk_geometry(&self.source, f);
        if let Some(chart) = &self.chart {
            crate::walk::Walk::walk_geometry(&chart.binding, f);
        }
    }
    fn walk_geometry_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::GeoItemMut<'a>)) {
        crate::walk::Walk::walk_geometry_mut(&mut self.source, f);
        if let Some(chart) = &mut self.chart {
            crate::walk::Walk::walk_geometry_mut(&mut chart.binding, f);
        }
    }
}
