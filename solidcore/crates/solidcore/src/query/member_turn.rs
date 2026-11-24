//! The turn of a member (a tube) about its own axis that makes one of its flat sides parallel
//! to a line, with the two measures a caller compares with tolerances.
//!
//! Upstream: none (not in SMLib).
//!
//! | Rust                    | Upstream |
//! |-------------------------|----------|
//! | [`measure_square_turn`] | (none)   |
//!
//! Deviations: no upstream counterpart.

use crate::error::{Result, SolidError};
use crate::math::{Vec3, PI};

/// The turn and what it rests on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SquareTurnMeasure {
    /// [`Vec3::square_turn`] of the axis, side normal and line, in radians. `0.0` when the
    /// line or the normal is along the axis (the turn means nothing then; the other two
    /// measures say so).
    pub turn: f64,
    /// `|n·a|` for the unit side normal `n` and unit axis `a`: 0 for a flat side of a member
    /// running along the axis.
    pub normal_along_axis: f64,
    /// The angle between the line and the axis, in [0, PI/2] (the line has no sense).
    pub line_off_axis: f64,
}

/// Measure the turn about `axis` that makes the side with normal `normal` parallel to `line`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a zero or non-finite vector.
pub fn measure_square_turn(axis: Vec3, normal: Vec3, line: Vec3) -> Result<SquareTurnMeasure> {
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    if !(finite(axis) && finite(normal) && finite(line)) {
        return Err(SolidError::invalid_input(
            "query::member_turn",
            "non-finite vector",
        ));
    }
    let axis = axis.unitize()?;
    let normal = normal.unitize()?;
    let line = line.unitize()?;
    let normal_along_axis = normal.dot(axis).abs();
    let angle = axis.angle_between(line)?;
    let line_off_axis = if angle > PI / 2.0 { PI - angle } else { angle };
    let turn = if line_off_axis < 1e-9 || normal_along_axis > 1.0 - 1e-9 {
        0.0
    } else {
        axis.square_turn(normal, line)?
    };
    Ok(SquareTurnMeasure {
        turn,
        normal_along_axis,
        line_off_axis,
    })
}
