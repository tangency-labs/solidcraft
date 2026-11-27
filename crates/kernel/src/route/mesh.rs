//! The router's mesh rebuilding ([`solidcraft_kernel_api::mesh::Meshes`]), as
//! `solidcraft_kernel::mesh`: a source triangle mesh converted to a solid.

use solidcore_record::Vec3;
use solidcraft_kernel_api::OpKind;
use solidcraft_kernel_api::mesh::Meshes;

pub use solidcraft_kernel_api::mesh::{MeshRebuilt, MeshRefused, ReconstructionKind};

use super::{Which, which};
use crate::backend::Solidcore;

/// The version of the face order a rebuilt mesh comes back in: the kernel's. This build holds
/// solidcore only.
pub const FACE_ORDER_VERSION: u32 = <Solidcore as Meshes>::FACE_ORDER_VERSION;

/// `triangles` over `positions` rebuilt as a solid within `tolerance` (mm), keeping its facets
/// where no analytic form is certified.
///
/// # Errors
/// The kernel's refusal, in its words: a malformed or too large mesh, a tolerance it refuses, or a
/// failure while building.
pub fn reconstruct_mesh(positions: &[Vec3], triangles: &[[u32; 3]], tolerance: f64) -> Result<MeshRebuilt, MeshRefused> {
    match which(OpKind::Mesh) {
        Which::Solidcore => Solidcore.reconstruct_mesh(positions, triangles, tolerance),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::mesh;

    #[test]
    fn routed_mesh_rebuilding_is_solidcores() {
        assert_eq!(FACE_ORDER_VERSION, mesh::FACE_ORDER_VERSION);
        let p = Vec3::new;
        let positions = [
            p(0.0, 0.0, 0.0),
            p(2.0, 0.0, 0.0),
            p(2.0, 3.0, 0.0),
            p(0.0, 3.0, 0.0),
            p(0.0, 0.0, 4.0),
            p(2.0, 0.0, 4.0),
            p(2.0, 3.0, 4.0),
            p(0.0, 3.0, 4.0),
        ];
        let triangles =
            [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        let routed = reconstruct_mesh(&positions, &triangles, 0.05).unwrap();
        let direct = mesh::reconstruct_mesh(&positions, &triangles, 0.05).unwrap();
        assert_eq!(routed.body, direct.body);
        assert_eq!(direct.kind, mesh::ReconstructionKind::Analytic);
        assert_eq!(routed.kind, ReconstructionKind::Analytic);
        // a refusal carries solidcore's words whole
        let words = mesh::reconstruct_mesh(&positions, &[[0, 1, 99]], 1e-6).unwrap_err().to_string();
        assert_eq!(reconstruct_mesh(&positions, &[[0, 1, 99]], 1e-6).unwrap_err(), MeshRefused(words));
    }
}
