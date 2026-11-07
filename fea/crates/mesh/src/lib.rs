//! Mesh types for the SolidCraft FEA scaffold.
//!
//! Phase 0 only needs a structured hexahedral mesh for tests and the
//! CPU reference heat solver. Production meshing is delegated to Gmsh
//! (FFI) / `meshio` — see `docs/fea/CONTEXT.md`. No mesher is written
//! here.

/// A structured hexahedral mesh over a rectangular box.
///
/// Nodes are ordered lexicographically (`x` fastest) to match the
/// coalesced access pattern planned for the GPU kernels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredHexMesh {
    /// Elements along x.
    pub nx: usize,
    /// Elements along y.
    pub ny: usize,
    /// Elements along z.
    pub nz: usize,
}

impl StructuredHexMesh {
    /// Create a mesh, rejecting degenerate dimensions.
    pub fn new(nx: usize, ny: usize, nz: usize) -> Option<Self> {
        if nx == 0 || ny == 0 || nz == 0 {
            return None;
        }
        Some(Self { nx, ny, nz })
    }

    /// Number of nodes: `(nx + 1) * (ny + 1) * (nz + 1)`.
    pub fn num_nodes(&self) -> usize {
        (self.nx + 1) * (self.ny + 1) * (self.nz + 1)
    }

    /// Number of hexahedral elements: `nx * ny * nz`.
    pub fn num_elements(&self) -> usize {
        self.nx * self.ny * self.nz
    }

    /// Scalar degrees of freedom (one temperature per node).
    pub fn num_dofs(&self) -> usize {
        self.num_nodes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_nodes_and_elements() {
        let mesh = StructuredHexMesh::new(10, 10, 10).expect("non-zero mesh");
        assert_eq!(mesh.num_elements(), 1000);
        assert_eq!(mesh.num_nodes(), 1331);
        assert_eq!(mesh.num_dofs(), 1331);
    }

    #[test]
    fn rejects_degenerate_mesh() {
        assert_eq!(StructuredHexMesh::new(0, 1, 1), None);
    }
}
