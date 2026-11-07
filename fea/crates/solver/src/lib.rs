//! Physics kernels for the SolidCraft FEA scaffold.
//!
//! Phase 0 (see `docs/fea/ROADMAP.md`): CPU reference solver for
//! steady linear heat conduction on a structured hex mesh. This
//! crate currently provides only the problem definition and a stub
//! residual evaluation — the assembled sparse solve and PCG loop are
//! not implemented yet. The GPU matrix-free PCG path (Phase 1) must
//! reproduce this reference.

use fea_mesh::StructuredHexMesh;

/// Steady linear heat problem: `-div(k grad T) = f` with
/// `k > 0` constant and homogeneous Dirichlet boundaries (stub).
#[derive(Debug, Clone, PartialEq)]
pub struct HeatProblem {
    /// Thermal conductivity `k`.
    pub conductivity: f64,
    /// Uniform volumetric source `f`.
    pub source: f64,
}

impl HeatProblem {
    /// Create a problem, rejecting non-physical conductivity.
    pub fn new(conductivity: f64, source: f64) -> Option<Self> {
        if conductivity.is_finite() && conductivity > 0.0 && source.is_finite() {
            Some(Self {
                conductivity,
                source,
            })
        } else {
            None
        }
    }
}

/// Residual `r = f - A T` for a zero initial guess (`T = 0`).
///
/// Stub for Phase 0: with `T = 0` the residual is simply the source
/// term at every DOF. The full assembled operator `A` is not built
/// yet, so this intentionally does not claim to solve anything.
pub fn initial_residual(problem: &HeatProblem, mesh: &StructuredHexMesh) -> Vec<f64> {
    let _ = mesh;
    vec![problem.source; mesh.num_dofs()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_residual_is_source_at_zero_guess() {
        let problem = HeatProblem::new(1.0, 2.5).expect("valid problem");
        let mesh = StructuredHexMesh::new(2, 2, 2).expect("valid mesh");
        let residual = initial_residual(&problem, &mesh);
        assert_eq!(residual.len(), mesh.num_dofs());
        assert!(residual.iter().all(|&value| value == 2.5));
    }

    #[test]
    fn rejects_non_positive_conductivity() {
        assert_eq!(HeatProblem::new(0.0, 1.0), None);
        assert_eq!(HeatProblem::new(-1.0, 1.0), None);
    }
}
