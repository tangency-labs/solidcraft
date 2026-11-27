//! solidcore's mesh rebuilding ([`Meshes`]): the solidcore-ops call this crate's callers made
//! before the interface existed, its refusal's words kept whole.

use solidcore_ops::mesh;
use solidcore_record::Vec3;
use solidcraft_kernel_api::mesh::{MeshRebuilt, MeshRefused, Meshes, ReconstructionKind};

use super::Solidcore;

impl Meshes for Solidcore {
    const FACE_ORDER_VERSION: u32 = mesh::FACE_ORDER_VERSION;

    fn reconstruct_mesh(&self, positions: &[Vec3], triangles: &[[u32; 3]], tolerance: f64) -> Result<MeshRebuilt, MeshRefused> {
        let made = mesh::reconstruct_mesh(positions, triangles, tolerance).map_err(|e| MeshRefused(e.to_string()))?;
        let kind = match made.kind {
            mesh::ReconstructionKind::Analytic => ReconstructionKind::Analytic,
            mesh::ReconstructionKind::Faceted => ReconstructionKind::Faceted,
        };
        Ok(MeshRebuilt { body: made.body, kind })
    }
}
