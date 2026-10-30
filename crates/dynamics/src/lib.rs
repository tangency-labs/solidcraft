//! Multibody dynamics for SolidCraft (RFC #9719): rigid bodies with masses and inertias, held
//! together by joints, moved by gravity, springs, dampers and loads, integrated in time.
//!
//! The crate knows nothing of documents or occurrences: a [`Model`] lists bodies (mass,
//! inertia about the centre of mass, starting pose and velocities), joints between frames on
//! them ([`JointKind`]: rigid, revolute, slider, cylindrical, planar, ball) and force elements;
//! [`simulate`] returns a [`Trajectory`] of poses, joint values and rates, joint reactions and
//! energies at a fixed output interval. The assembly layer maps occurrences and joints to it.
//!
//! The formulation is the textbook one for constrained rigid bodies in maximal coordinates
//! (Lagrange multipliers, index-1 form, see `system.rs`), stepped by Runge–Kutta 4 or
//! Dormand–Prince 5(4) and projected back onto the joints after every step (`integrate.rs`).
//!
//! Units are the design's: mm, kg, s and radians, so forces are in mN (kg·mm/s²), torques in
//! mN·mm, stiffness in N/m (kg/s²) and gravity in mm/s² ([`STANDARD_GRAVITY`]).
//!
//! Every input is checked ([`Model::validate`], [`Settings::validate`]) and every size and loop
//! is capped ([`MAX_BODIES`], [`MAX_STEPS`], …): a bad model is an error, never a panic.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod integrate;
mod linalg;
pub mod math;
mod model;
mod system;

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

pub use integrate::simulate;
pub use math::{Mat3, Quat};
pub use model::{
    Anchor, Body, Force, Integrator, Joint, JointKind, MAX_BODIES, MAX_CONSTRAINT_ROWS, MAX_FORCES, MAX_JOINTS, MAX_MAGNITUDE,
    MAX_PROJECTION_ITERATIONS, MAX_SAMPLES, MAX_STEPS, Marker, Model, STANDARD_GRAVITY, Settings,
};

/// Why a simulation could not run or stopped.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum DynamicsError {
    #[error("invalid model: {0}")]
    InvalidModel(String),
    #[error("invalid settings: {0}")]
    InvalidSettings(String),
    #[error("too large: {0}")]
    TooLarge(String),
    #[error("the joint equations could not be solved at t = {time} s")]
    Singular { time: f64 },
    #[error("the motion diverged at t = {time} s")]
    Diverged { time: f64 },
    #[error("the step needed at t = {time} s is below the smallest allowed")]
    StepTooSmall { time: f64 },
    #[error("the run needs more than the step limit (stopped at t = {time} s)")]
    StepLimit { time: f64 },
}

pub type Result<T> = std::result::Result<T, DynamicsError>;

/// A body's state at one time (world).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyState {
    /// Centre of mass (mm).
    pub position: Vec3,
    /// The body's axes (body → world).
    pub orientation: Quat,
    /// Velocity of the centre of mass (mm/s).
    pub velocity: Vec3,
    /// Angular velocity (rad/s).
    pub angular_velocity: Vec3,
}

/// A joint's state at one time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointResult {
    /// The joint's values ([`JointKind::values`]; radians and mm). A ball joint gives B's turn
    /// from A as a rotation vector in A's frame.
    pub values: Vec<f64>,
    /// Their rates (rad/s, mm/s); a ball joint gives the relative angular velocity in A's frame.
    pub rates: Vec<f64>,
    /// The force the joint puts on `b`'s side (world, mN). Joints whose equations repeat
    /// others' (a planar linkage of revolutes) share their reactions in one of many valid ways.
    pub force: Vec3,
    /// The torque the joint puts on `b`'s side about its frame's origin (world, mN·mm).
    pub torque: Vec3,
}

/// Energies at one time (kg·mm²/s² = µJ).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Energy {
    pub kinetic: f64,
    /// Potential energy of gravity, zero at the world origin.
    pub gravity: f64,
    /// Energy stored in springs (preload included).
    pub spring: f64,
}

impl Energy {
    pub fn total(&self) -> f64 {
        self.kinetic + self.gravity + self.spring
    }
}

/// The system at one output time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Time (s).
    pub time: f64,
    pub bodies: Vec<BodyState>,
    pub joints: Vec<JointResult>,
    pub energy: Energy,
    /// The largest joint equation error (mm for points; directions are unitless).
    pub constraint_error: f64,
}

/// What a run did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    /// Accepted steps.
    pub steps: usize,
    /// Rejected steps (adaptive integration).
    pub rejected: usize,
    /// The largest joint equation error at an output sample.
    pub max_constraint_error: f64,
    /// Joint equations that repeat others (the most at any sample).
    pub redundant_equations: usize,
}

/// A run's result: the samples at t = 0, one output interval, two, … up to the duration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trajectory {
    pub samples: Vec<Sample>,
    pub stats: Stats,
}

#[cfg(test)]
mod tests;
