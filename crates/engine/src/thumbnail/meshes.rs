//! Portable pictures reuse warm viewport geometry, including shared assembly placements.
//! Bound the complete scene buffers before copying, independently of design asset size.
use std::sync::Arc;

use solidcraft_doc::ModelBody;
use solidcraft_geom::Mesh;

pub(super) struct Budget(usize);

impl Default for Budget {
    fn default() -> Self {
        Self(256 << 20)
    }
}

impl Budget {
    pub(super) fn placed(&mut self, body: &ModelBody) -> Option<Arc<Mesh>> {
        let (mesh, transform) = body.cached_mesh_in_place()?;
        self.0 = self.0.checked_sub(crate::mesh_lines::buffer_bytes(&mesh))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;

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
