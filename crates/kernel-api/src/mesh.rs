//! Meshes rebuilt as solids ([`Meshes`]): a source triangle mesh converted within a length
//! tolerance, as certified planes and cylinders where the kernel can prove them, else as the
//! mesh's own facets. Positions and triangles in, the record out.

use solidcore_record::Vec3;
use solidcore_record::brep::Brep;

use crate::Backend;

/// What a rebuilt mesh is made of, as the kernel certified it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReconstructionKind {
    /// Certified planes and circular cylinders, including planar shoulders.
    Analytic,
    /// The source mesh's planar facets, with adjacent coplanar triangles joined.
    Faceted,
}

/// A source mesh rebuilt as a solid.
#[derive(Clone, Debug)]
pub struct MeshRebuilt {
    /// The solid, as the record.
    pub body: Brep,
    /// What it is made of.
    pub kind: ReconstructionKind,
}

/// A mesh the kernel would not rebuild, in its words: a malformed or too large mesh, a tolerance
/// it refuses, or a failure while building.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct MeshRefused(pub String);

/// The mesh rebuilding of one kernel ([`crate::OpKind::Mesh`]).
pub trait Meshes: Backend {
    /// The version of the face order a rebuilt mesh comes back in: a rebuild cached under another
    /// version is stale.
    const FACE_ORDER_VERSION: u32;
    /// `triangles` over `positions` rebuilt as a solid within `tolerance` (mm), keeping its facets
    /// where no analytic form is certified. Source normals and display settings are not used.
    fn reconstruct_mesh(&self, positions: &[Vec3], triangles: &[[u32; 3]], tolerance: f64) -> Result<MeshRebuilt, MeshRefused>;
}
