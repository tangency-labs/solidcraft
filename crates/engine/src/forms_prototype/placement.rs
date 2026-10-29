//! World-space edit targets are mapped into a stable local control frame.
use super::*;
use crate::doc::{Mat, forms::Placement};
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
        self.cage.points.iter().map(|&p| self.placement.point(p)).collect()
    }
    pub fn world_point(&self, p: Vec3) -> Vec3 {
        self.placement.point(p)
    }
    pub fn local_vector(&self, v: Vec3) -> Vec3 {
        self.placement.local_vector(v)
    }
    pub(super) fn install_surface(&mut self, surface: SurfacePreview) {
        self.local_vertices = surface.vertices.clone();
        self.surface = placed_surface(surface, &self.placement);
    }
    pub fn transform(&mut self, matrix: &Mat, checkpoint: bool) -> Result<()> {
        if self.profile.is_some() {
            return Err(error("move Form Surface interior guides; its CAD rim is fixed"));
        }
        Placement::from_matrix(matrix)?;
        let placement = Placement::from_matrix(&crate::doc::mat_mul(matrix, &self.placement.matrix()))?;
        if self.cage.points.iter().any(|&p| {
            let p = placement.point(p);
            !p.is_finite() || p.x.abs().max(p.y.abs()).max(p.z.abs()) > 100_000.0
        }) {
            return Err(error("placed Form controls must remain within 100000 mm"));
        }
        if placement.matrix().iter().flatten().zip(self.placement.matrix().iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-12) {
            return Ok(());
        }
        let delta = Placement::from_matrix(matrix)?;
        let surface = placed_surface(self.surface.clone(), &delta);
        if checkpoint {
            self.checkpoint();
        }
        self.placement = placement;
        self.surface = surface;
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
        if let Some(matrix) = rigid_between(&from, &to) {
            self.transform(&matrix, checkpoint)?;
            return Ok(true);
        }
        let Some(affine) = affine_between(&from, &to) else { return Ok(false) };
        self.apply_affine(&affine, checkpoint)?;
        Ok(true)
    }
    fn apply_affine(&mut self, affine: &Affine, checkpoint: bool) -> Result<()> {
        let columns = self.placement.axes.map(|axis| affine.vector(axis));
        let x = columns[0].normalized().ok_or_else(|| error("Form scale collapses an axis"))?;
        let z = x.cross(columns[1]).normalized().ok_or_else(|| error("Form scale collapses a plane"))?;
        let y = z.cross(x);
        let placement = Placement { origin: affine.point(self.placement.origin), axes: [x, y, z] };
        placement.validate()?;
        let local = |v| placement.local_vector(affine.vector(self.placement.vector(v)));
        let mut definition = self.definition();
        if let Some(h) = &mut definition.hierarchy {
            for p in &mut h.base.points {
                *p = local(*p);
            }
            for detail in h.details.iter_mut().flatten() {
                detail.delta = local(detail.delta);
            }
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
        if definition.cage.points.iter().any(|&p| {
            let p = placement.point(p);
            !p.is_finite() || p.x.abs().max(p.y.abs()).max(p.z.abs()) > 100_000.0
        }) {
            return Err(error("scaled Form controls must remain within 100000 mm"));
        }
        definition.placement = placement;
        self.adopt_definition(definition, checkpoint)
    }
}
struct Affine {
    columns: [Vec3; 3],
    shift: Vec3,
}
impl Affine {
    fn vector(&self, v: Vec3) -> Vec3 {
        self.columns[0] * v.x + self.columns[1] * v.y + self.columns[2] * v.z
    }
    fn point(&self, p: Vec3) -> Vec3 {
        self.vector(p) + self.shift
    }
}
/// A whole-selection affine edit acts directly on subdivision controls and details.
/// Its cost does not grow with the number of selected inverse-solve constraints.
fn affine_between(from: &[Vec3], to: &[Vec3]) -> Option<Affine> {
    if from.len() != to.len() || from.len() < 3 || from.iter().chain(to).any(|p| !p.is_finite()) {
        return None;
    }
    let a = *from.first()?;
    let b = *to.first()?;
    let second = from.iter().enumerate().max_by(|(_, p), (_, q)| p.dist(a).total_cmp(&q.dist(a)))?.0;
    let e1 = *from.get(second)? - a;
    let third = from.iter().enumerate().max_by(|(_, p), (_, q)| (**p - a).cross(e1).len2().total_cmp(&(**q - a).cross(e1).len2()))?.0;
    let e2 = *from.get(third)? - a;
    let normal = e1.cross(e2).normalized()?;
    let fourth = from.iter().enumerate().max_by(|(_, p), (_, q)| (**p - a).dot(normal).abs().total_cmp(&(**q - a).dot(normal).abs()))?.0;
    let d1 = *to.get(second)? - b;
    let d2 = *to.get(third)? - b;
    let (e3, d3) = if (*from.get(fourth)? - a).dot(normal).abs() > 1e-8 {
        (*from.get(fourth)? - a, *to.get(fourth)? - b)
    } else {
        let cross = d1.cross(d2);
        (normal, cross.normalized()? * (cross.len() / e1.cross(e2).len()).sqrt())
    };
    let determinant = e1.dot(e2.cross(e3));
    if determinant.abs() < 1e-12 * e1.len() * e2.len() * e3.len() {
        return None;
    }
    let [g1, g2, g3] = [e2.cross(e3), e3.cross(e1), e1.cross(e2)].map(|v| v / determinant);
    let vector = |v: Vec3| d1 * v.dot(g1) + d2 * v.dot(g2) + d3 * v.dot(g3);
    let columns = [Vec3::X, Vec3::Y, Vec3::Z].map(vector);
    if columns.iter().any(|p| !p.is_finite()) || columns[0].dot(columns[1].cross(columns[2])) <= 1e-12 {
        return None;
    }
    let affine = Affine { columns, shift: b - vector(a) };
    from.iter().zip(to).all(|(p, q)| affine.point(*p).dist(*q) < 1e-8).then_some(affine)
}
fn rigid_between(from: &[Vec3], to: &[Vec3]) -> Option<Mat> {
    if from.len() != to.len() || from.is_empty() || from.iter().chain(to).any(|p| !p.is_finite()) {
        return None;
    }
    let a = *from.first()?;
    let b = *to.first()?;
    let translation = b - a;
    if from.iter().zip(to).all(|(p, q)| (*q - *p).dist(translation) < 1e-9) {
        return Some(crate::doc::rigid(translation, Vec3::ZERO, Vec3::Z, 0.0));
    }
    let second = from.iter().enumerate().max_by(|(_, p), (_, q)| p.dist(a).total_cmp(&q.dist(a)))?.0;
    let x = (*from.get(second)? - a).normalized()?;
    let third = from.iter().enumerate().max_by(|(_, p), (_, q)| (**p - a).cross(x).len2().total_cmp(&(**q - a).cross(x).len2()))?.0;
    let z = x.cross(*from.get(third)? - a).normalized()?;
    let y = z.cross(x);
    let u = (*to.get(second)? - b).normalized()?;
    let w = u.cross(*to.get(third)? - b).normalized()?;
    let v = w.cross(u);
    let rotate = |p: Vec3| u * p.dot(x) + v * p.dot(y) + w * p.dot(z);
    let shift = b - rotate(a);
    if !from.iter().zip(to).all(|(p, q)| (rotate(*p) + shift).dist(*q) < 1e-8) {
        return None;
    }
    let [rx, ry, rz] = [rotate(Vec3::X), rotate(Vec3::Y), rotate(Vec3::Z)];
    Some([[rx.x, rx.y, rx.z, 0.0], [ry.x, ry.y, ry.z, 0.0], [rz.x, rz.y, rz.z, 0.0], [shift.x, shift.y, shift.z, 1.0]])
}
