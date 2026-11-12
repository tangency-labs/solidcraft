//! Places for labels along a line, as a drawing spaces the numbers of an ordinate dimension set:
//! one kernel call ([`solidcore::layout`]), which computes the spacing.

use crate::{Error, Result};

pub use solidcore::layout::MAX_ITEMS;

/// Places for items whose ideal places along a line are `ideal` (least first), at least `pitch`
/// apart and moved as little as possible in all ([`solidcore::layout::spread_along`]).
///
/// # Errors
/// [`Error::Argument`] for a pitch not finite or below zero, more than [`MAX_ITEMS`] places, a
/// place not finite or out of order, or a result not finite.
pub fn spread_along(ideal: &[f64], pitch: f64) -> Result<Vec<f64>> {
    solidcore::layout::spread_along(ideal, pitch).map_err(|_| Error::Argument)
}
