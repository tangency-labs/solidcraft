//! Derived mesh feedback for a matched extrusion. Preparing the response uses
//! the command's topology operation and subdivision evaluator, without a seam
//! solve or a CAD fit. Sampling a prepared response only moves display arrays.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct ExtrusionPreview {
    pub mesh: crate::geom::Mesh,
    pub vertices: Vec<Vec3>,
    pub boundaries: Vec<([usize; 2], Vec<Vec3>)>,
}
#[derive(Debug)]
pub struct ExtrusionResponse {
    template: ExtrusionPreview,
    reference: Vec3,
    responses: Vec<[Vec3; 3]>,
    fixed: Vec<(usize, Vec3)>,
}
fn positions(preview: &ExtrusionPreview) -> Vec<Vec3> {
    preview.mesh.positions.iter().chain(&preview.vertices).chain(preview.boundaries.iter().flat_map(|(_, path)| path)).copied().collect()
}
fn point_key(point: Vec3) -> [u64; 3] {
    [point.x, point.y, point.z].map(|value| if value == 0.0 { 0 } else { value.to_bits() })
}
impl ExtrusionResponse {
    /// Sample the newest absolute displacement. This cannot update Form state.
    pub fn preview(&self, displacement: Vec3) -> Result<ExtrusionPreview> {
        if !displacement.is_finite() || displacement.x.abs().max(displacement.y.abs()).max(displacement.z.abs()) > 100_000.0 {
            return Err(error("extrusion feedback needs a finite displacement"));
        }
        let delta = displacement - self.reference;
        let mut preview = self.template.clone();
        let targets = preview.mesh.positions.iter_mut().chain(&mut preview.vertices).chain(preview.boundaries.iter_mut().flat_map(|(_, path)| path));
        for (point, response) in targets.zip(&self.responses) {
            *point += response[0] * delta.x + response[1] * delta.y + response[2] * delta.z;
            if !point.is_finite() {
                return Err(error("extrusion feedback exceeds finite display coordinates"));
            }
        }
        let mut fixed = self.fixed.iter().peekable();
        let targets = preview.mesh.positions.iter_mut().chain(&mut preview.vertices).chain(preview.boundaries.iter_mut().flat_map(|(_, path)| path));
        for (index, point) in targets.enumerate() {
            if let Some(&&(at, position)) = fixed.peek()
                && index == at
            {
                *point = position;
                fixed.next();
            }
        }
        preview.mesh.normals.fill(Vec3::ZERO);
        for triangle in &preview.mesh.triangles {
            let [a, b, c] = triangle.map(|i| preview.mesh.positions.get(i as usize).copied());
            let (Some(a), Some(b), Some(c)) = (a, b, c) else { return Err(error("extrusion feedback triangle is invalid")) };
            let normal = (b - a).cross(c - a);
            for &index in triangle {
                let target = preview.mesh.normals.get_mut(index as usize).ok_or_else(|| error("extrusion feedback normal is missing"))?;
                *target += normal;
            }
        }
        for normal in &mut preview.mesh.normals {
            *normal = normal.normalized().unwrap_or(Vec3::Z);
        }
        Ok(preview)
    }
}
impl Form {
    /// Prepare on a worker. For fixed topology and crease weights, subdivision
    /// positions are affine in the extrusion displacement. The sampled response
    /// is only display data and has no document, history or recovery codec.
    pub fn prepare_extrusion_preview(
        &self,
        faces: &[usize],
        edges: &[[usize; 2]],
        displacement: Vec3,
        checkpoint: bool,
    ) -> Result<ExtrusionResponse> {
        if !displacement.is_finite() || displacement.len() < 1e-9 {
            return Err(error("extrusion feedback needs a finite nonzero displacement"));
        }
        let mut display = self.clone();
        display.levels = display.levels.min(2);
        let evaluate = |delta| -> Result<(ExtrusionPreview, crate::doc::forms::Definition)> {
            let (candidate, original) = display.matched_extrusion_candidate(faces, edges, delta, checkpoint)?;
            let (surface, _) = Self::preview(&candidate.cage, candidate.hierarchy.as_ref(), original.matching.as_deref(), None, candidate.levels)?;
            let SurfacePreview { mesh, vertices, boundaries, .. } = surface;
            Ok((ExtrusionPreview { mesh, vertices, boundaries }, original))
        };
        let (mut template, original) = evaluate(displacement)?;
        let reference = positions(&template);
        if reference.len() > 65_536 || template.mesh.triangles.len() > 131_072 {
            return Err(error("extrusion feedback exceeds its interactive display budget"));
        }
        let step = (displacement.len() * 0.001).clamp(1e-8, 1.0);
        let mut changes = Vec::with_capacity(3);
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            let (preview, _) = evaluate(displacement + axis * step)?;
            if preview.mesh.triangles != template.mesh.triangles || preview.mesh.tri_face != template.mesh.tri_face {
                return Err(error("extrusion feedback changed its display topology"));
            }
            let values = positions(&preview);
            if values.len() != reference.len() {
                return Err(error("extrusion feedback changed its display layout"));
            }
            changes.push(values.into_iter().zip(&reference).map(|(next, &point)| self.placement.vector((next - point) / step)).collect::<Vec<_>>());
        }
        let [x, y, z]: [Vec<Vec3>; 3] = changes.try_into().map_err(|_| error("extrusion feedback response is incomplete"))?;
        let responses = x.into_iter().zip(y).zip(z).map(|((x, y), z)| [x, y, z]).collect();
        // Difference local samples before rigid placement so a distant origin
        // cannot erase a small response through coordinate cancellation.
        for point in template.mesh.positions.iter_mut().chain(&mut template.vertices).chain(template.boundaries.iter_mut().flat_map(|(_, path)| path))
        {
            *point = self.placement.point(*point);
        }
        for normal in &mut template.mesh.normals {
            *normal = self.placement.vector(*normal);
        }
        for point in template.mesh.edges.iter_mut().flatten() {
            *point = self.placement.point(*point);
        }
        let reference = positions(&template);
        let matching = original.matching.as_ref().ok_or_else(|| error("extrusion feedback requires a matched opening"))?;
        let ring = crate::doc::forms::match_openings(&original.cage, original.hierarchy.as_ref())?
            .into_iter()
            .find(|ring| matching.config.edge.iter().all(|v| ring.contains(v)))
            .ok_or_else(|| error("extrusion feedback lost its fixed opening"))?;
        let ring_edges: BTreeSet<_> = ring.iter().zip(ring.iter().cycle().skip(1)).take(ring.len()).map(|(&a, &b)| [a.min(b), a.max(b)]).collect();
        let seam: BTreeSet<_> = if let Some(hierarchy) = &original.hierarchy {
            hierarchy
                .layout()
                .map_err(error)?
                .paths
                .into_iter()
                .filter(|(_, path)| path.windows(2).all(|p| ring_edges.contains(&[p[0].min(p[1]), p[0].max(p[1])])))
                .map(|(edge, _)| edge)
                .collect()
        } else {
            ring_edges
        };
        let mut anchors = BTreeMap::new();
        for (edge, path) in template.boundaries.iter().filter(|(edge, _)| seam.contains(&[edge[0].min(edge[1]), edge[0].max(edge[1])])) {
            let (source_edge, source) = self
                .surface
                .boundaries
                .iter()
                .find(|(source, _)| [source[0].min(source[1]), source[0].max(source[1])] == [edge[0].min(edge[1]), edge[0].max(edge[1])])
                .ok_or_else(|| error("extrusion feedback opening has no accepted display source"))?;
            let intervals = path.len().checked_sub(1).filter(|&n| n > 0).ok_or_else(|| error("extrusion feedback opening is empty"))?;
            let source_intervals = source.len().saturating_sub(1);
            if source_intervals % intervals != 0 {
                return Err(error("extrusion feedback opening samples are incompatible"));
            }
            for (i, &point) in path.iter().enumerate() {
                let index = i * (source_intervals / intervals);
                let index = if source_edge == edge { index } else { source_intervals - index };
                let accepted = *source.get(index).ok_or_else(|| error("extrusion feedback opening sample is missing"))?;
                anchors.insert(point_key(point), accepted);
            }
        }
        if anchors.is_empty() {
            return Err(error("extrusion feedback could not retain the fixed opening"));
        }
        let fixed =
            reference.iter().enumerate().filter_map(|(index, &point)| anchors.get(&point_key(point)).copied().map(|point| (index, point))).collect();
        Ok(ExtrusionResponse { template, reference: displacement, responses, fixed })
    }
}
