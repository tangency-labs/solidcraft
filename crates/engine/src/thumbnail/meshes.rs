//! Portable pictures reuse warm viewport geometry, including shared assembly placements.
//! Bound the complete scene buffers before copying, independently of design asset size.
use std::{mem::size_of, sync::Arc};

use solidcraft_doc::ModelBody;
use solidcraft_geom::{Mesh, Vec3};

pub(super) struct Budget(usize);

impl Default for Budget {
    fn default() -> Self {
        Self(256 << 20)
    }
}

impl Budget {
    pub(super) fn placed(&mut self, body: &ModelBody) -> Option<Arc<Mesh>> {
        let (mesh, transform) = body.cached_mesh_in_place()?;
        self.0 = self.0.checked_sub(buffer_bytes(&mesh))?;
        if solidcraft_doc::is_identity(&transform) {
            return Some(mesh);
        }
        let mut moved = (*mesh).clone();
        for point in &mut moved.positions {
            *point = solidcraft_doc::apply_point(&transform, *point);
        }
        for normal in &mut moved.normals {
            *normal = solidcraft_doc::apply_vector(&transform, *normal);
        }
        for point in moved.edges.iter_mut().flatten() {
            *point = solidcraft_doc::apply_point(&transform, *point);
        }
        Some(Arc::new(moved))
    }
}

fn buffer_bytes(mesh: &Mesh) -> usize {
    let arrays = [
        (mesh.positions.len(), size_of::<Vec3>()),
        (mesh.normals.len(), size_of::<Vec3>()),
        (mesh.triangles.len(), size_of::<[u32; 3]>()),
        (mesh.tri_face.len(), size_of::<u32>()),
        (mesh.edges.len(), size_of::<Vec<Vec3>>()),
        (mesh.edge_faces.len(), size_of::<Vec<u32>>()),
        (mesh.seams.len(), size_of::<bool>()),
        (mesh.smooth_edges.len(), size_of::<bool>()),
        (mesh.sharp_edges.len(), size_of::<bool>()),
    ];
    let mut bytes = arrays.into_iter().fold(0usize, |sum, (n, size)| sum.saturating_add(n.saturating_mul(size)));
    for edge in &mesh.edges {
        bytes = bytes.saturating_add(edge.len().saturating_mul(size_of::<Vec3>()));
    }
    for faces in &mesh.edge_faces {
        bytes = bytes.saturating_add(faces.len().saturating_mul(size_of::<u32>()));
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_budget_never_meshes_or_copies_an_over_budget_body() {
        let body = ModelBody::new("Bracket".into(), solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(8., 6., 4.)).unwrap(), 1);
        assert!(Budget::default().placed(&body).is_none());
        assert!(body.cached_mesh().is_none(), "a cold body stays cold");
        let warm = body.mesh();
        assert!(Budget(1).placed(&body).is_none());
        let picture = Budget::default().placed(&body).unwrap();
        assert!(Arc::ptr_eq(&picture, &warm), "an unplaced warm mesh needs no copy");
    }
}
