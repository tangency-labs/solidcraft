//! World-space edit targets are mapped into a stable local control frame.
use super::*;
use crate::doc::{Mat, forms::Placement};
use crate::kernel::affine::{self, Fit};
use std::collections::BTreeSet;

pub(super) fn placed_surface(mut surface: SurfacePreview, placement: &Placement) -> SurfacePreview {
    if placement.is_identity() {
        return surface;
    }
    for p in &mut surface.mesh.positions {
        *p = placement.point(*p);
    }
    for n in &mut surface.mesh.normals {
        *n = placement.vector(*n);
    }
    for curve in &mut surface.mesh.edges {
        for p in curve {
            *p = placement.point(*p);
        }
    }
    for p in &mut surface.vertices {
        *p = placement.point(*p);
    }
    for (_, curve) in &mut surface.boundaries {
        for p in curve {
            *p = placement.point(*p);
        }
    }
    surface.index = crate::geom::MeshIndex::new(&surface.mesh);
    surface
}
impl Form {
    pub fn control_positions(&self) -> Vec<Vec3> {
        self.logical.points.iter().map(|&p| self.placement.point(p)).collect()
    }
    pub fn world_point(&self, p: Vec3) -> Vec3 {
        self.placement.point(p)
    }
    pub fn local_vector(&self, v: Vec3) -> Vec3 {
        self.placement.local_vector(v)
    }
    pub(super) fn install_surface(&mut self, surface: SurfacePreview) {
        self.local_vertices = Arc::from(surface.vertices.as_slice());
        self.surface = Arc::new(placed_surface(surface, &self.placement));
    }
    pub fn transform(&mut self, matrix: &Mat, checkpoint: bool) -> Result<()> {
        if self.profile.is_some() {
            return Err(error("move Form Surface interior guides; its CAD rim is fixed"));
        }
        let delta = Placement::from_matrix(matrix)?;
        let placement = Placement::from_matrix(&affine::then(&self.placement.matrix(), matrix)?)?;
        if self.logical.points.iter().any(|&p| {
            let p = placement.point(p);
            !p.is_finite() || p.x.abs().max(p.y.abs()).max(p.z.abs()) > 100_000.0
        }) {
            return Err(error("placed Form controls must remain within 100000 mm"));
        }
        if placement.matrix().iter().flatten().zip(self.placement.matrix().iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-12) {
            return Ok(());
        }
        let surface = placed_surface(self.surface.as_ref().clone(), &delta);
        if checkpoint {
            self.checkpoint();
        }
        self.placement = placement;
        self.surface = Arc::new(surface);
        self.match_report = None;
        self.extrusion = None;
        self.redo.clear();
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    fn local_targets(&self, positions: &[[f64; 3]]) -> Vec<[f64; 3]> {
        positions
            .iter()
            .map(|p| {
                let p = self.placement.local_point(Vec3::new(p[0], p[1], p[2]));
                [p.x, p.y, p.z]
            })
            .collect()
    }
    pub fn set(&mut self, indices: &[usize], positions: &[[f64; 3]], checkpoint: bool) -> Result<()> {
        if self.try_rigid(indices, positions, checkpoint, false)? {
            return Ok(());
        }
        self.set_local(indices, &self.local_targets(positions), checkpoint)
    }
    pub fn set_surface(&mut self, indices: &[usize], positions: &[[f64; 3]], checkpoint: bool) -> Result<()> {
        if self.try_rigid(indices, positions, checkpoint, true)? {
            return Ok(());
        }
        self.set_surface_local(indices, &self.local_targets(positions), checkpoint)
    }
    fn try_rigid(&mut self, indices: &[usize], positions: &[[f64; 3]], checkpoint: bool, surface: bool) -> Result<bool> {
        if self.profile.is_some() {
            return Ok(false);
        }
        if indices.is_empty() || indices.len() != positions.len() {
            return Ok(false);
        }
        let ids: BTreeSet<_> = indices.iter().copied().collect();
        let all: BTreeSet<_> = self.logical.faces.iter().flatten().copied().collect();
        if ids.len() != indices.len() || ids != all {
            return Ok(false);
        }
        let controls;
        let source = if surface {
            &self.surface.vertices
        } else {
            controls = self.control_positions();
            &controls
        };
        let from = indices.iter().map(|&i| source.get(i).copied()).collect::<Option<Vec<_>>>().ok_or_else(|| error("invalid placed Form control"))?;
        let to: Vec<_> = positions.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
        match affine::fit_between(&from, &to) {
            Some(Fit::Rigid(matrix)) => self.transform(&matrix, checkpoint)?,
            Some(Fit::Affine(matrix)) => self.apply_affine(&matrix, checkpoint)?,
            None => return Ok(false),
        }
        Ok(true)
    }
    fn apply_affine(&mut self, matrix: &Mat, checkpoint: bool) -> Result<()> {
        let axes = affine::mapped_axes(matrix, self.placement.axes).ok_or_else(|| error("Form scale collapses an axis or plane"))?;
        let placement = Placement { origin: affine::point(matrix, self.placement.origin), axes };
        placement.validate()?;
        let local = |v| placement.local_vector(affine::vector(matrix, self.placement.vector(v)));
        let mut definition = self.definition();
        if let Some(h) = &mut definition.hierarchy {
            for p in &mut h.base.points {
                *p = local(*p);
            }
            h.map_displacements(local);
            definition.cage = h.evaluate().map_err(error)?;
        } else {
            for p in &mut definition.cage.points {
                *p = local(*p);
            }
        }
        if let Some(symmetry) = &mut definition.symmetry {
            let axis = [Vec3::X, Vec3::Y, Vec3::Z].get(symmetry.axis).copied().ok_or_else(|| error("invalid symmetry axis"))?;
            symmetry.origin *= local(axis).dot(axis);
            for (i, &j) in symmetry.partners.iter().enumerate() {
                if symmetry.reflect(definition.cage.point(i).map_err(error)?).dist(definition.cage.point(j).map_err(error)?) > 2e-6 {
                    return Err(error("this scale would skew the mirror plane; disable symmetry before applying it"));
                }
            }
        }
        if self.logical.points.iter().any(|&p| {
            let p = affine::point(matrix, self.placement.point(p));
            !p.is_finite() || p.x.abs().max(p.y.abs()).max(p.z.abs()) > 100_000.0
        }) {
            return Err(error("scaled Form controls must remain within 100000 mm"));
        }
        definition.placement = placement;
        self.adopt_definition(definition, checkpoint)
    }
}
