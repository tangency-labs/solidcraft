//! Contract and result types for `motion.simulate` (`cmd/motion_simulate.rs`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Simulate the assembly's moving rigid groups as rigid bodies under gravity, through its joints
/// and motion links, from where they are now and at rest. Read-only: the design does not move.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SimulateParams {
    /// Simulated time, s (> 0, at most 3600).
    pub duration: f64,
    /// Integration step, s (default 0.001).
    pub step: Option<f64>,
    /// Time between the samples returned, s (default: duration / 100, at least the step).
    pub output_interval: Option<f64>,
    /// Gravity, mm/s² (default [0, 0, −9806.65]: standard gravity along −Z).
    pub gravity: Option<[f64; 3]>,
}

/// A rigid group's frame at one time (world, mm): its origin and unit axes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GroupPose {
    pub group: usize,
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
}

/// A joint's values at one time, in the units `joint.list` gives (degrees and mm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JointValues {
    pub joint: u64,
    pub values: Vec<f64>,
}

/// Energies at one time, J.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SimulationEnergy {
    pub kinetic: f64,
    /// Gravity's potential energy, zero at the world origin.
    pub potential: f64,
}

/// The assembly at one time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SimulationSample {
    /// s.
    pub time: f64,
    /// Every moving rigid group's frame.
    pub groups: Vec<GroupPose>,
    pub joints: Vec<JointValues>,
    pub energy: SimulationEnergy,
}

/// A moving rigid group of the run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SimulatedGroup {
    pub group: usize,
    /// Its occurrences (as `assembly.mass_properties` lists them).
    pub occurrences: Vec<u64>,
    pub mass_kg: f64,
}

/// What `motion.simulate` returns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SimulateResult {
    pub groups: Vec<SimulatedGroup>,
    /// The joints simulated (between two rigid groups, one at least moving), by id.
    pub joints: Vec<u64>,
    /// The motion links simulated, by index.
    pub links: Vec<usize>,
    pub samples: Vec<SimulationSample>,
    /// Integration steps taken.
    pub steps: usize,
    /// The largest joint error at a sample, mm.
    pub max_constraint_error_mm: f64,
    /// Joint equations that repeat others (a planar linkage of revolutes).
    pub redundant_equations: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
