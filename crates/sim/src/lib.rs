//! Simulation (analysis) for SolidCraft: the Fusion 360-style
//! simulation workspace, built clean-room and comparable in scope to
//! ANSYS. A study selects solid bodies from the document, assigns
//! materials (from `crates/doc`), loads and constraints (as
//! `crates/engine` commands), meshes the solid, solves, and returns
//! result fields for the viewport (`crates/render` / `crates/gpu`).
//!
//! MVP physics (see `docs/fea/ROADMAP.md`): linear heat first, then
//! linear elasticity (implicit PCG), explicit dynamics later. No
//! implicit contact/plasticity. No ANSYS/Fusion code is used or
//! embedded; commercial solvers are external validation oracles only.
//!
//! Units are the design's: mm, kg, s. So forces are mN, stiffness
//! N/m, stress N/mm² (MPa), conductivity W/(mm·K) scaled by the
//! document's unit handling — the solver itself works in model units.
//!
//! This crate is a scaffold: study types and validation only. The
//! kernel/doc/engine wiring, volume meshing (Gmsh), and WGSL kernels
//! are Phase 0–1 work and are not stubbed with invented APIs.
//! Every input is checked and every size is capped: a bad study is
//! an error, never a panic.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Caps, following the `crates/dynamics` convention: a bad model is
/// an error, never a panic or an unbounded allocation.
pub const MAX_STUDY_BODIES: usize = 256;
pub const MAX_LOADS: usize = 1024;
pub const MAX_CONSTRAINTS: usize = 1024;

/// Which physics a study solves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StudyKind {
    /// Steady linear heat conduction (`-div(k grad T) = f`).
    Thermal,
    /// Linear elasticity, implicit PCG ($k_e = \int B^T D B$).
    StaticStress,
    /// Explicit elastodynamics (Phase 3, not yet implemented).
    ExplicitDynamics,
}

/// A simulation study on selected document bodies. Scaffold form:
/// identities and physics kind only — materials, loads, and mesh
/// assignment arrive with the `doc`/`engine` wiring in Phase 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Study {
    /// Body identifiers in the document, in selection order.
    pub bodies: Vec<u64>,
    /// Physics to solve.
    pub kind: StudyKind,
}

/// Why a study could not be created or run.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SimError {
    /// No bodies selected: a study needs at least one solid.
    #[error("simulation study has no bodies")]
    NoBodies,
    /// Too many bodies, loads, or constraints.
    #[error("simulation study exceeds size caps ({what}: {count} > {max})")]
    TooLarge {
        /// Which collection exceeded its cap.
        what: &'static str,
        /// How many entries were supplied.
        count: usize,
        /// The cap that was exceeded.
        max: usize,
    },
}

impl Study {
    /// Create a study, rejecting empty or oversized selections.
    pub fn new(bodies: Vec<u64>, kind: StudyKind) -> Result<Self, SimError> {
        if bodies.is_empty() {
            return Err(SimError::NoBodies);
        }
        if bodies.len() > MAX_STUDY_BODIES {
            return Err(SimError::TooLarge {
                what: "bodies",
                count: bodies.len(),
                max: MAX_STUDY_BODIES,
            });
        }
        Ok(Self { bodies, kind })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn study_requires_bodies() {
        assert_eq!(
            Study::new(Vec::new(), StudyKind::Thermal),
            Err(SimError::NoBodies)
        );
    }

    #[test]
    fn study_rejects_oversized_selection() {
        let bodies = vec![1_u64; MAX_STUDY_BODIES + 1];
        assert!(matches!(
            Study::new(bodies, StudyKind::StaticStress),
            Err(SimError::TooLarge { .. })
        ));
    }

    #[test]
    fn study_accepts_valid_selection() {
        let study = Study::new(vec![7, 9], StudyKind::Thermal).expect("valid study");
        assert_eq!(study.bodies, vec![7, 9]);
        assert_eq!(study.kind, StudyKind::Thermal);
    }
}
