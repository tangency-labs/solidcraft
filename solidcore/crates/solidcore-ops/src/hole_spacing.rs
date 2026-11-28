//! The spacing of round items that share one axis direction: holes along a row or round a
//! bolt circle, by their centres and axes. Solidcore reads the pattern and measures the gaps
//! ([`solidcore::geom::hole_spacing::spacing`]); ops only adapts the application's plain
//! numbers and the kernel's refusal.

use crate::Result;

pub use solidcore::geom::hole_spacing::{Pattern, PitchCircle, Spacing, MAX_ITEMS};

/// The spacing of round items, each its centre and its axis direction: the pattern their
/// centres make in the plane square to the axis, their order along or round it, each
/// neighbouring pair's centre-to-centre gap, the span, and the pitch where the gaps are even.
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Refused`], in its words) for fewer than two or more
/// than [`MAX_ITEMS`] items, a value that is not finite, axes that are not parallel, and two
/// items that share one centre.
pub fn spacing(items: &[([f64; 3], [f64; 3])]) -> Result<Spacing> {
    solidcore::geom::hole_spacing::spacing(items).map_err(crate::error::status)
}
