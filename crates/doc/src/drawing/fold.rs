//! The bases of views made from a parent view off its four sides: a pictorial view seen from one of
//! the parent's corners ([`axonometric`], [`pictorial_basis`]).
//!
//! This is drawing-only maths, so it lives in the drawing code and not in solidcore or geom (the
//! maintainers' ruling on #16038 and #14114: "solidcore holds what more than one client app needs").
//!
//! Sources: ISO 5456-3:1996, *Technical drawings, projection methods, part 3: axonometric
//! representations* (the isometric view's plane equally inclined to the three axes, the dimetric's
//! 1 : 1/2 : 1); Higham, *Accuracy and Stability of Numerical Algorithms* (2nd ed.), §27.8 (a length
//! scaled by its largest component before squaring, so that it can't overflow).

use solidcore_record::{Plane, Vec3};
use solidcraft_geom::cell::Projection;
use solidcraft_geom::prelude::*;
use solidcraft_geom::section::validate_plane;

use crate::{DocError, Result};

/// The least length a direction may have and still give a unit: a length at or below it is refused.
pub const MIN_DIRECTION: f64 = 1e-9;

/// The view of `frame` from its corner `corner = (k_x, k_y, k_n)`, along its x, its y and its
/// normal `n = x × y`: looking from `k_x x + k_y y + k_n n` toward the origin, paper x horizontal
/// in the frame (square to its y) and paper y up, so the view's normal points back at the corner.
/// `(±1, 1, ±1)` is isometric and `(±1, 1, ±√7)` the standard dimetric (ISO 5456-3). With `k` the
/// corner's unit, paper x is the unit of `k_n x - k_x n`, the view looks along
/// `t = -(k_x x + k_y y + k_n n)`, and paper y is `x × t`; the origin is kept. Each unit is
/// [`scaled_unit3`]'s, the module's one length rule. For a corner of components ±1 and 0 on a frame
/// of unit axes, every step is exact or one correctly rounded operation. The axes are orthonormal
/// to rounding, and as orthonormal as the frame's.
///
/// # Errors
///
/// [`DocError::Invalid`] for a frame [`validate_plane`] refuses, a corner not finite or of length
/// at or below [`MIN_DIRECTION`], or a corner within [`MIN_DIRECTION`] radians of the frame's y,
/// which has no horizontal.
pub fn axonometric(frame: &Plane, corner: [f64; 3]) -> Result<Plane> {
    let refuse = |reason: &str| DocError::Invalid(format!("no view from the corner: {reason}"));
    validate_plane(frame).map_err(|_| refuse("a frame not finite or not orthonormal within 1e-8"))?;
    let k = scaled_unit3(Vec3::from(corner)).ok_or_else(|| refuse("a corner not finite, or of length at or below 1e-9: a longer one fits"))?;
    let (x, y, n) = axes(frame);
    // `k_n x - k_x n` is as long as the sine of the corner's angle from y
    let across =
        scaled_unit3(x * k.z - n * k.x).ok_or_else(|| refuse("a corner within 1e-9 rad of the frame's y has no horizontal: one further fits"))?;
    let toward = -(x * k.x + y * k.y + n * k.z);
    Ok(Plane { origin: frame.origin, x: across, y: across.cross(toward) })
}

/// The basis of the pictorial view on `parent`'s diagonal `(right, up)` (each ±1: which way along
/// the parent's paper x and y it sits), in `projection`: an isometric view through the parent's
/// origin, its paper x horizontal in the parent. As a projected view does
/// (`solidcraft_geom::drawing_view::projected_basis`), it looks from the corner it sits by in third
/// angle and from the opposite one in first: the corner `(right, up, 1)` along the parent's paper
/// x, paper y and normal, each paper sign times 1 in third angle and -1 in first.
///
/// # Errors
///
/// [`DocError::Invalid`] for a parent basis [`validate_plane`] refuses.
pub fn pictorial_basis(parent: &Plane, (right, up): (f64, f64), projection: Projection) -> Result<Plane> {
    let from = facing(projection);
    let view = axonometric(parent, [right * from, up * from, 1.0])?;
    Ok(Plane { origin: view.origin, x: positive_zeros(view.x), y: positive_zeros(view.y) })
}

/// 1 when a view made from its parent in `projection` looks from the side it sits on (third angle),
/// -1 when from the side away from it (first angle), as
/// `solidcraft_geom::drawing_view::projected_basis` turns a side view.
fn facing(projection: Projection) -> f64 {
    match projection {
        Projection::Third => 1.0,
        Projection::First => -1.0,
    }
}

/// A frame's x and y axes and normal (`x × y`).
fn axes(frame: &Plane) -> (Vec3, Vec3, Vec3) {
    (frame.x, frame.y, frame.x.cross(frame.y))
}

/// `v` with any -0 made 0 (turning by a sign makes a zero component -0), so a view's axes write out
/// as `[0.0, 1.0, 0.0]`, never `[-0.0, 1.0, -0.0]`.
fn positive_zeros(v: Vec3) -> Vec3 {
    Vec3::new(positive_zero(v.x), positive_zero(v.y), positive_zero(v.z))
}

/// `c`, or 0 for -0.
fn positive_zero(c: f64) -> f64 {
    if c == 0.0 { 0.0 } else { c }
}

/// `v`'s length and unit, the length scaled by its largest component before the squares are summed
/// (Higham §27.8), so that no finite vector overflows to a zero "unit" or underflows to a length it
/// doesn't have: `m·sqrt(Σ(v/m)²)`. `None` for a component not finite or a zero vector. `sqrt` and
/// each division are correctly rounded, so the result is the same on every target. The length may
/// round to infinity for a finite vector near `f64::MAX`: still a length.
fn scaled3(v: Vec3) -> Option<(f64, Vec3)> {
    if !v.is_finite() {
        return None;
    }
    let m = v.x.abs().max(v.y.abs()).max(v.z.abs());
    if m == 0.0 {
        return None;
    }
    let (x, y, z) = (v.x / m, v.y / m, v.z / m);
    let s = (x * x + y * y + z * z).sqrt();
    Some((m * s, Vec3::new(x / s, y / s, z / s)))
}

/// `v`'s unit as [`scaled3`] gives it, or `None` for a length at or below [`MIN_DIRECTION`].
fn scaled_unit3(v: Vec3) -> Option<Vec3> {
    scaled3(v).and_then(|(len, unit)| (len > MIN_DIRECTION).then_some(unit))
}

#[cfg(test)]
#[path = "fold_tests.rs"]
mod tests;
