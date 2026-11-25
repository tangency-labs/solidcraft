//! Holes in a sheet's flat pattern: whether two circles are one hole, and where a note naming a hole
//! goes beside it.
//!
//! Not upstream (SolidCraft's press-in hardware, which keeps a hole's hardware by the hole's circle
//! in the flat and writes its name beside it in the flat pattern's drawing, moved into the kernel so
//! the layers above compute no geometry, docs/layers.md §1).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::math::Vec2;

/// A circle in the flat: its centre and radius.
pub type Circle = (Vec2, f64);

/// Whether `a` and `b` are the same hole: their centres less than `tol` apart and their radii less
/// than `tol` apart. A tolerance, a centre or a radius that is not a number (or a tolerance of
/// nothing) makes no hole the other's.
#[must_use]
pub fn same_circle(a: Circle, b: Circle, tol: f64) -> bool {
    a.0.distance(b.0) < tol && (a.1 - b.1).abs() < tol
}

/// Where the lower left corner of a note `height` high, read left to right, goes to stand beside
/// the circle `at`: level with its centre, just clear of its rim on the right, half a height from
/// the rim. The note's own middle then lies on the line through the centre.
#[must_use]
pub fn note_beside(at: Circle, height: f64) -> Vec2 {
    Vec2::new(at.0.x + at.1 + height * 0.5, at.0.y - height * 0.5)
}
