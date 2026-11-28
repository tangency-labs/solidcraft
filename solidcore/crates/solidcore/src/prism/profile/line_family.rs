//! Families of parallel lines with a repeating dash sequence, clipped to a planar region within
//! one work budget: the geometry under a drawing's hatch pattern, with lengths in millimetres
//! and angles in radians.
//!
//! A family's origin is in the region's XY frame. With tangent `u` at its angle and left normal
//! `n`, member `k` passes through `origin + k * (delta_x * u + delta_y * n)`. The signed deltas
//! preserve both the spacing and the dash phase between members.
//!
//! Empty dashes mean a continuous line. Otherwise positive lengths draw, negative
//! lengths leave gaps, and zero places a dot without advancing. The sequence repeats
//! after the sum of its absolute lengths. A negative-only sequence is valid and draws
//! nothing; an all-zero sequence is invalid because it cannot advance.
//!
//! The limits bound the values [`clip`] admits, not numerical accuracy or the number of marks
//! a region needs; [`clip`] bounds that with its budget. A named pattern of families, its
//! presets and its file format belong to the application that draws it.
//!
//! Not upstream (SolidCraft's drawing hatcher, its pattern model left in SolidCraft,
//! docs/layers.md §1).

mod budget;
mod clip;
mod generate;
mod phase;
mod support;

use crate::math::Vec2;

pub use generate::{clip, Mark};
pub use support::dot_region;

/// Maximum input boundary pieces admitted before converting or preparing a region.
pub const MAX_INPUT_PIECES: usize = 8192;

/// The most line families one [`clip`] admits.
pub const MAX_FAMILIES: usize = 64;
/// The most entries in one family's repeating dash sequence.
pub const MAX_DASHES: usize = 6;
/// The largest coordinate, delta or dash magnitude, in millimetres.
pub const MAX_LENGTH_MM: f64 = 1e5;
/// The largest angle magnitude, in radians.
pub const MAX_ANGLE_RAD: f64 = 1e6;

/// One family of parallel lines and its repeating dash sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct LineFamily {
    /// Counterclockwise direction of each line, in radians.
    pub angle: f64,
    /// A point on member zero, in the region's XY frame.
    pub origin: Vec2,
    /// Signed shift along the line from one member to the next.
    pub delta_x: f64,
    /// Signed perpendicular spacing from one member to the next.
    pub delta_y: f64,
    /// Signed draw/gap lengths and zero dots; empty means continuous.
    pub dashes: Vec<f64>,
}

/// Why [`clip`] or [`LineFamily::validate`] refused its input.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("line family: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Invalid(message))
    }
}

fn length(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_LENGTH_MM
}

impl LineFamily {
    /// Check the values without normalizing angles, changing signs or discarding dots.
    /// Nonzero spacing has no lower cutoff here; [`clip`] bounds the resulting work.
    pub fn validate(&self) -> Result<()> {
        require(self.dashes.len() <= MAX_DASHES, "more than 6 dash entries")?;
        require(
            self.angle.is_finite() && self.angle.abs() <= MAX_ANGLE_RAD,
            "angle must be finite and within +/-1000000 radians",
        )?;
        require(
            length(self.origin.x) && length(self.origin.y),
            "origin must be finite and within +/-100000 mm",
        )?;
        require(
            length(self.delta_x) && length(self.delta_y),
            "deltas must be finite and within +/-100000 mm",
        )?;
        require(self.delta_y != 0.0, "perpendicular spacing must be nonzero")?;
        require(
            self.dashes.iter().copied().all(length),
            "dash lengths must be finite and within +/-100000 mm",
        )?;
        if !self.dashes.is_empty() {
            let period: f64 = self.dashes.iter().map(|d| d.abs()).sum();
            require(
                period.is_finite() && period > 0.0,
                "a nonempty dash sequence must have a positive finite period",
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod clip_tests;
#[cfg(test)]
mod generate_tests;
#[cfg(test)]
mod phase_tests;
