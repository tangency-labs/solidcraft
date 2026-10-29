//! A CAD face's fixed rim and an editable grid of interior guide positions.
use super::{Definition, Placement, SupportRef};
use crate::{DocError, ModelBody, ModelState, Result, kernel};
use serde::{Deserialize, Serialize};
use solidcraft_geom::{Vec3, subdivision::Cage};
use std::collections::BTreeSet;

mod partition;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default, skip_serializing_if = "is_false")]
    pub partition: bool,
    pub source: SupportRef,
    pub segments: [usize; 2],
    /// Position (0) or tangent (1) continuity at the original CAD boundary.
    pub continuity: usize,
    /// Undeformed samples in the Form's local frame. Recompute preserves offsets.
    pub rest: Vec<Vec3>,
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
        let mut mesh = patch.display_mesh(tolerance.min(0.05))?;
        let mut maximum = fit.report.gap.max(fit.report.point_gap);
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
        let edges = cage.edges().map_err(|e| invalid(e.to_string()))?;
        mesh.edges = edges
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
            .collect::<Result<_>>()?;
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
                patch_roots: roots.unwrap_or_else(|| vec![0]),
                root_surfaces: 1,
                trimmed_roots: true,
                solid: false,
            },
        ))
    }
    pub fn validate(&self, cage: &Cage) -> Result<()> {
        self.source.validate()?;
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
        let (index, _, _) = crate::naming::face_on_body(body, name, *pick).ok_or_else(|| invalid("the Form Surface source face is missing"))?;
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
        let profile = Self { partition: true, source, segments, continuity, rest };
        profile.validate(&cage)?;
        Ok((profile, cage))
    }
    pub fn recompute(&self, definition: &Definition, state: &ModelState) -> Result<(Self, Cage)> {
        self.validate(&definition.cage)?;
        let (body, index) = self.resolve(state)?;
        let rest: Vec<_> = sample(&body.body, index, self.segments)?.into_iter().map(|p| definition.placement.local_point(p)).collect();
        let boundary = self.boundary();
        let mut cage = definition.cage.clone();
        for (i, point) in cage.points.iter_mut().enumerate() {
            let (Some(old), Some(new)) = (self.rest.get(i), rest.get(i)) else {
                return Err(invalid("Form Surface lost a guide sample"));
            };
            *point = if boundary.contains(&i) { *new } else { *new + (*point - *old) };
        }
        let mut profile = self.clone();
        profile.rest = rest;
        profile.validate(&cage)?;
        Ok((profile, cage))
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
            let fitted = kernel::face_geometry(&patch, 0)?;
            let missing: Vec<_> =
                guides.iter().enumerate().filter_map(|(i, p)| fitted.parameters(*p, None, tolerance).is_err().then_some(i)).collect();
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
