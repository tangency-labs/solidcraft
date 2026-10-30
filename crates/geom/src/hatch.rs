//! Plain hatch patterns in paper millimetres at scale 1, with angles in radians.
//!
//! A family's origin is in the pattern's XY frame. With tangent `u` at its angle and
//! left normal `n`, member `k` passes through `origin + k * (delta_x * u + delta_y * n)`.
//! The signed deltas preserve both the spacing and the dash phase between members.
//! Pattern lengths do not inherit a drawing view's scale.
//!
//! Empty dashes mean a continuous line. Otherwise positive lengths draw, negative
//! lengths leave gaps, and zero places a dot without advancing. The sequence repeats
//! after the sum of its absolute lengths. A negative-only sequence is valid and draws
//! nothing; an all-zero sequence is invalid because it cannot advance.
//!
//! Validate after deserialization and before use: these records are mutable, and serde
//! does not call the validators. The limits bound the stored values, not numerical
//! accuracy or the number of marks a region needs. Clipping, its work/output budgets,
//! drawing records and paper-scene construction are separate from this model. Readers
//! must also bound encoded bytes and allocations before constructing the records.

mod budget;
mod clip;
mod generate;
mod phase;

use std::f64::consts::FRAC_PI_4;

use serde::{Deserialize, Serialize};

use crate::Vec2;

pub use generate::{Mark, generate};

/// The most parallel-line families in one pattern.
pub const MAX_FAMILIES: usize = 64;
/// The most entries in one family's repeating dash sequence.
pub const MAX_DASHES: usize = 6;
/// The longest pattern name, in ASCII bytes.
pub const MAX_NAME_BYTES: usize = 31;
/// The longest description, in Unicode scalar values.
pub const MAX_DESCRIPTION_CHARS: usize = 80;
/// The largest stored coordinate, delta or dash magnitude, in paper millimetres.
pub const MAX_LENGTH_MM: f64 = 1e5;
/// The largest stored angle magnitude, in radians.
pub const MAX_ANGLE_RAD: f64 = 1e6;

/// A named pattern whose definition can travel with a drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    pub name: String,
    pub description: String,
    pub families: Vec<Family>,
}

/// One family of parallel lines and its repeating dash sequence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    /// Counterclockwise direction of each line, in radians.
    pub angle: f64,
    /// A point on member zero, in the global pattern XY frame, in paper millimetres.
    pub origin: Vec2,
    /// Signed shift along the line from one member to the next, in paper millimetres.
    pub delta_x: f64,
    /// Signed perpendicular spacing from one member to the next, in paper millimetres.
    pub delta_y: f64,
    /// Signed draw/gap lengths and zero dots; empty means continuous.
    pub dashes: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("hatch pattern: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

fn length(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_LENGTH_MM
}

impl Family {
    /// Check stored values without normalizing angles, changing signs or discarding dots.
    /// Nonzero spacing has no lower cutoff here; a generator must bound its resulting work.
    pub fn validate(&self) -> Result<()> {
        require(self.dashes.len() <= MAX_DASHES, "more than 6 dash entries")?;
        require(self.angle.is_finite() && self.angle.abs() <= MAX_ANGLE_RAD, "angle must be finite and within +/-1000000 radians")?;
        require(length(self.origin.x) && length(self.origin.y), "origin must be finite and within +/-100000 mm")?;
        require(length(self.delta_x) && length(self.delta_y), "deltas must be finite and within +/-100000 mm")?;
        require(self.delta_y != 0.0, "perpendicular spacing must be nonzero")?;
        require(self.dashes.iter().copied().all(length), "dash lengths must be finite and within +/-100000 mm")?;
        if !self.dashes.is_empty() {
            let period: f64 = self.dashes.iter().map(|d| d.abs()).sum();
            require(period.is_finite() && period > 0.0, "a nonempty dash sequence must have a positive finite period")?;
        }
        Ok(())
    }

    fn continuous(angle: f64) -> Self {
        Self { angle, origin: Vec2::ZERO, delta_x: 0.0, delta_y: 1.0, dashes: Vec::new() }
    }
}

impl Pattern {
    /// Check metadata and every family. A solid fill is a separate paint, not an empty pattern.
    pub fn validate(&self) -> Result<()> {
        require(!self.families.is_empty() && self.families.len() <= MAX_FAMILIES, "a pattern needs 1 to 64 families")?;
        require(!self.name.is_empty() && self.name.len() <= MAX_NAME_BYTES, "name must have 1 to 31 ASCII characters")?;
        let valid_name = self.name.as_bytes().split_first().is_some_and(|(first, rest)| {
            first.is_ascii_alphanumeric() && rest.iter().all(|b| b.is_ascii_alphanumeric() || matches!(*b, b'_' | b'-' | b'$'))
        });
        require(valid_name, "name must start with an ASCII letter or digit and contain only ASCII letters, digits, _, - or $")?;
        // A Unicode scalar takes at most four UTF-8 bytes. Bound traversal before counting it.
        require(self.description.len() <= MAX_DESCRIPTION_CHARS * 4, "description exceeds 80 characters")?;
        require(self.description.chars().count() <= MAX_DESCRIPTION_CHARS, "description exceeds 80 characters")?;
        require(!self.description.chars().any(char::is_control), "description contains a control character")?;
        self.families.iter().try_for_each(Family::validate)
    }

    /// Original continuous 45-degree lines spaced 1 mm apart in the pattern's paper frame.
    pub fn lines_45() -> Self {
        Self {
            name: "LINES_45".into(),
            description: "Parallel lines at 45 degrees, 1 mm apart".into(),
            families: vec![Family::continuous(FRAC_PI_4)],
        }
    }

    /// Original crossed 45- and 135-degree lines, each family spaced 1 mm apart.
    pub fn cross_hatch() -> Self {
        Self {
            name: "CROSS_HATCH".into(),
            description: "Crossed lines at 45 and 135 degrees, 1 mm apart".into(),
            families: vec![Family::continuous(FRAC_PI_4), Family::continuous(3.0 * FRAC_PI_4)],
        }
    }
}

#[cfg(test)]
mod clip_tests;
#[cfg(test)]
mod generate_tests;
#[cfg(test)]
mod phase_tests;
#[cfg(test)]
#[path = "hatch_tests.rs"]
mod tests;
