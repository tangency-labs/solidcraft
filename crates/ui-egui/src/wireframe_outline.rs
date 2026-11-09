//! Outlines of curved surfaces in the Wireframe visual style (#13347).
//!
//! Wireframe draws a body's edges and no faces. A curved surface's outline is not an edge, so a
//! sphere (whose only edge is a hidden seam) drew nothing, and a cylinder showed its end circles
//! without its sides. `solidcore_tess` finds the outline on the display mesh; this keeps each
//! mesh's folds, so a view change only checks which way they face.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

use solidcore_tess::Fold;
use solidcraft_engine::geom::Mesh;

/// The folds of `mesh` ([`Mesh::folds`]), kept while the mesh is.
pub(crate) fn folds_of(mesh: &Arc<Mesh>) -> Arc<[Fold]> {
    type Kept = HashMap<usize, (Weak<Mesh>, Arc<[Fold]>)>;
    static KEPT: LazyLock<Mutex<Kept>> = LazyLock::new(Default::default);
    let at = Arc::as_ptr(mesh) as usize;
    if let Some((_, found)) =
        KEPT.lock().unwrap_or_else(|e| e.into_inner()).get(&at).filter(|(kept, _)| kept.as_ptr() == Arc::as_ptr(mesh) && kept.strong_count() > 0)
    {
        return found.clone();
    }
    let made: Arc<[Fold]> = mesh.folds().into();
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    // Meshes that are gone (an edit, another design) free their folds.
    kept.retain(|_, (w, _)| w.strong_count() > 0);
    kept.insert(at, (Arc::downgrade(mesh), made.clone()));
    made
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_are_kept_with_their_mesh() {
        let square = |z: f64| {
            let p = |x: f64, y: f64| solidcore_record::Vec3::new(x, y, z);
            Mesh {
                positions: vec![p(0., 0.), p(1., 0.), p(1., 1.), p(0., 1.)],
                normals: vec![solidcore_record::Vec3::Z; 4],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                tri_face: vec![0, 0],
                ..Default::default()
            }
        };
        let mesh = Arc::new(square(0.0));
        let first = folds_of(&mesh);
        assert_eq!(first.len(), 1, "the square's two triangles meet flat along its diagonal");
        assert!(Arc::ptr_eq(&first, &folds_of(&mesh)), "the same mesh finds its folds again");
        let other = Arc::new(square(1.0));
        assert!(!Arc::ptr_eq(&first, &folds_of(&other)), "another mesh has its own");
    }
}
