//! Contracts and result types for the commands in `cmd/mass_properties.rs`.
//!
//! The reply types are also the Rust API a dynamics solver reads (`Session::mass_properties`):
//! units are kg, mm and kg·mm² throughout.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Mass, centre of mass and inertia tensor of every occurrence of the design and of every rigid
/// group (occurrences joined by rigid joints, which move as one body), from each body's material
/// density. Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AssemblyMassParams {}

/// A placed coordinate frame in world coordinates (mm): its origin and its unit axes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MassFrame {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
}

/// Mass, centre of mass and inertia tensor of one rigid body (or of several taken as one).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RigidMass {
    /// Mass in kg.
    pub mass_kg: f64,
    /// Centre of mass in world coordinates, mm.
    pub center_of_mass_mm: [f64; 3],
    /// Centre of mass in the frame's coordinates, mm.
    pub center_of_mass_local_mm: [f64; 3],
    /// Inertia tensor about the centre of mass, world axes, kg·mm², row by row: the moments on
    /// the diagonal (Ixx = ∫ (y² + z²) dm …), the negated products of inertia off it
    /// (−∫ x y dm …), as URDF and most dynamics libraries take it. `null` when a closed body's
    /// mesh does not close (a warning names it).
    pub inertia_world_kg_mm2: Option<[[f64; 3]; 3]>,
    /// The same tensor in the frame's axes, kg·mm². `null` as above, or when the frame is
    /// scaled (its axes are not a rotation).
    pub inertia_local_kg_mm2: Option<[[f64; 3]; 3]>,
}

/// One body of an occurrence.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BodyMassEntry {
    pub name: String,
    /// Its physical material (the default material when none is assigned).
    pub material: String,
    pub density_g_cm3: f64,
    /// A closed solid: an open body (a surface) has no volume and adds no mass.
    pub closed: bool,
    /// Volume in mm³ and mass in kg, for a closed body only.
    pub volume_mm3: Option<f64>,
    pub mass_kg: Option<f64>,
}

/// One occurrence (0: the design's own bodies, which never move).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OccurrenceMass {
    pub occurrence: u64,
    pub name: String,
    /// The component it places.
    pub component: u64,
    /// Occurrence ids from the top of the assembly down (empty for 0).
    pub path: Vec<u64>,
    /// Index of its rigid group in `groups`.
    pub group: usize,
    /// Grounded, or the design itself.
    pub fixed: bool,
    /// The occurrence's frame (its component's origin and axes as placed).
    pub frame: MassFrame,
    pub bodies: Vec<BodyMassEntry>,
    /// Bodies left out because they are not closed solids.
    pub open_bodies_skipped: usize,
    /// Its own bodies' totals in its frame; `null` when it holds no closed body.
    pub mass: Option<RigidMass>,
}

/// Occurrences that move as one body.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RigidGroupMass {
    /// Its index in `groups`.
    pub group: usize,
    /// The occurrences it holds, ascending (0: the design's own bodies).
    pub occurrences: Vec<u64>,
    /// Holds the design itself or a grounded occurrence: it does not move.
    pub fixed: bool,
    /// The occurrence whose frame `frame` is: 0 (world) for a fixed group holding the design,
    /// else the group's lowest occurrence id.
    pub frame_occurrence: u64,
    pub frame: MassFrame,
    /// Totals of all its occurrences in `frame` (parallel-axis theorem); `null` when massless.
    pub mass: Option<RigidMass>,
}

/// What `assembly.mass_properties` returns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AssemblyMassResult {
    /// Every occurrence, the design's own bodies (0) first, then by occurrence id.
    pub occurrences: Vec<OccurrenceMass>,
    /// The rigid groups, in order of their lowest occurrence id.
    pub groups: Vec<RigidGroupMass>,
    /// Approximations made (open bodies, repeated subassemblies, scaled frames).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "mass_properties_tests.rs"]
mod tests;
