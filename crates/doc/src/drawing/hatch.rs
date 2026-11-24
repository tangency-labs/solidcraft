//! Hatch patterns: named sets of line families in paper millimetres at scale 1, with angles in
//! radians, and their original presets. A pattern travels with its drawing (`PatternSnapshot`);
//! PAT files read into it (`solidcraft_io::pat`). Its marks come from solidcore's line-family
//! clipping (`solidcore_ops::profile::clip_line_family`), which holds the family's geometry and
//! its limits; pattern lengths do not inherit a drawing view's scale.
//!
//! Validate after deserialization and before use: these records are mutable, and serde does not
//! call the validators. Readers must also bound encoded bytes and allocations before constructing
//! the records.

use std::f64::consts::FRAC_PI_4;

use serde::{Deserialize, Serialize};
use solidcore_ops::profile::{MAX_FAMILIES, Mark, clip_line_family};
use solidcore_record::profile::line_family::LineFamily;
use solidcore_record::{Region2, Vec2};

/// The longest pattern name, in ASCII bytes.
pub const MAX_NAME_BYTES: usize = 31;
/// The longest description, in Unicode scalar values.
pub const MAX_DESCRIPTION_CHARS: usize = 80;

/// A named pattern whose definition can travel with a drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    pub name: String,
    pub description: String,
    pub families: Vec<LineFamily>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("hatch pattern: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

fn continuous(angle: f64) -> LineFamily {
    LineFamily { angle, origin: Vec2::ZERO, delta_x: 0.0, delta_y: 1.0, dashes: Vec::new() }
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
        self.families.iter().try_for_each(|family| family.validate().map_err(|error| Invalid(error.0)))
    }

    /// The pattern's complete marks over a region in paper millimetres, or a refusal without
    /// partial geometry.
    ///
    /// # Errors
    /// Invalid patterns, unsupported boundaries, uncertain arithmetic or exhausted budgets.
    pub fn marks(&self, region: &Region2) -> Result<Vec<Mark>> {
        self.validate()?;
        clip_line_family(&self.families, region).map_err(|error| Invalid(error.0))
    }

    /// Original continuous 45-degree lines spaced 1 mm apart in the pattern's paper frame.
    pub fn lines_45() -> Self {
        Self { name: "LINES_45".into(), description: "Parallel lines at 45 degrees, 1 mm apart".into(), families: vec![continuous(FRAC_PI_4)] }
    }

    /// Original crossed 45- and 135-degree lines, each family spaced 1 mm apart.
    pub fn cross_hatch() -> Self {
        Self {
            name: "CROSS_HATCH".into(),
            description: "Crossed lines at 45 and 135 degrees, 1 mm apart".into(),
            families: vec![continuous(FRAC_PI_4), continuous(3.0 * FRAC_PI_4)],
        }
    }
}

#[cfg(test)]
#[path = "hatch_tests.rs"]
mod tests;
