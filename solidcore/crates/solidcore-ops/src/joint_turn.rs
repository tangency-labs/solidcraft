//! How far a path turns where two curves meet: the angle between the direction the first
//! leaves in and the direction the second continues in. Solidcore measures it
//! ([`solidcore::query::edge_turn::joint_turn`]); ops only adapts the application's plain
//! numbers and its refusal.

use crate::{Error, Result};

/// The angle in radians, in `[0, PI]`, between the planar directions `before` (the first curve's
/// end tangent) and `after` (the next curve's start tangent): 0 where the path runs straight
/// on, `PI` where it doubles back. The directions need not be unit.
///
/// # Errors
/// [`Error::Argument`] for a direction that is not finite or is zero-length.
pub fn joint_turn(before: [f64; 2], after: [f64; 2]) -> Result<f64> {
    solidcore::query::edge_turn::joint_turn(before, after).map_err(|_| Error::Argument)
}
