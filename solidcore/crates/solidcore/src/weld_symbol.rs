//! The geometry of a welding symbol's contour mark and of a V groove (not upstream).
//!
//! A drawing's weld symbol is built from a few exact constructions that belong with the
//! kernel's geometry, not in the application that draws it: the finishing contour drawn over a
//! weld face (a flat line, or a sixty-degree arc bulging out of or into the weld) and the
//! height of an equilateral V groove. Everything here is a pure function of plane vectors and
//! sizes; conventions, sizes per standard and text stay with the caller.
//!
//! Hostile input is refused with [`SolidError::InvalidInput`], never a panic.

use std::f64::consts::FRAC_PI_3;

use crate::error::{Result, SolidError};
use crate::math::{Point2, Vec2};
use crate::prism::profile::Seg2;

/// How far a direction may stray from unit length, or two directions from a right angle.
const DIRECTION_TOLERANCE: f64 = 1e-9;

/// The finishing contour drawn over a weld face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeldContour {
    /// A flat face: the mark is the straight chord.
    Flush,
    /// A face bulging out of the joint: an arc bulging along the outward normal.
    Convex,
    /// A face sunk into the joint: an arc bulging against the outward normal.
    Concave,
}

/// The contour mark over the weld face at `face`, as one segment.
///
/// `tangent` runs along the face and `outward` leaves it, at a right angle; any length but zero,
/// as each is taken as its own direction. Their handedness fixes the side the arc turns to. `size` is the symbol's size `g` (the mark is
/// `1.5 g` long). The mark's middle stands `0.5 g` off the face for a flush contour; an arc has
/// radius `1.5 g`, spans sixty degrees, and its chord is that flush line, so the arc and the
/// line meet the same two ends.
///
/// # Errors
/// [`SolidError::InvalidInput`] when `face` or a direction is not finite, a direction is not
/// zero, the two are not perpendicular, or `size` is not finite and positive.
pub fn weld_contour_mark(
    face: Point2,
    tangent: Vec2,
    outward: Vec2,
    size: f64,
    contour: WeldContour,
) -> Result<Seg2> {
    const OP: &str = "weld_symbol::weld_contour_mark";
    let finite = |v: Vec2| v.x.is_finite() && v.y.is_finite();
    if !finite(face) || !finite(tangent) || !finite(outward) {
        return Err(SolidError::invalid_input(
            OP,
            "a point or direction is not finite",
        ));
    }
    if !size.is_finite() || size <= 0.0 {
        return Err(SolidError::invalid_input(
            OP,
            "the symbol size must be finite and positive",
        ));
    }
    // Each direction as its unit vector: each component over the length, so a fillet face's
    // diagonal (1, -1) gives exactly 1 / sqrt(2).
    let unit = |v: Vec2| {
        let length = v.length();
        (length > DIRECTION_TOLERANCE).then(|| Vec2::new(v.x / length, v.y / length))
    };
    let (Some(tangent), Some(outward)) = (unit(tangent), unit(outward)) else {
        return Err(SolidError::invalid_input(OP, "a direction is zero"));
    };
    if tangent.dot(outward).abs() > DIRECTION_TOLERANCE {
        return Err(SolidError::invalid_input(
            OP,
            "the directions are not perpendicular",
        ));
    }
    // Which way the outward normal turns from the tangent: +1 counter-clockwise, -1 clockwise.
    let sign = (tangent.x * outward.y - tangent.y * outward.x).signum();
    let radius = 1.5 * size;
    let offset = radius * 3.0_f64.sqrt() / 2.0;
    let middle = face + outward * (0.5 * size + radius - offset);
    let p0 = middle - tangent * (0.75 * size);
    let p3 = middle + tangent * (0.75 * size);
    Ok(match contour {
        WeldContour::Flush => Seg2::Line { a: p0, b: p3 },
        WeldContour::Convex | WeldContour::Concave => {
            let bulge = if contour == WeldContour::Convex {
                1.0
            } else {
                -1.0
            };
            let center = middle - outward * (bulge * offset);
            // The arc starts 120 degrees from the face tangent, on the side the outward normal
            // turns to; the tangent's own angle carries any tilt of the face.
            let start = sign * (bulge * 2.0 * FRAC_PI_3) + tangent.y.atan2(tangent.x);
            Seg2::Arc {
                center,
                radius,
                start,
                sweep: -sign * bulge * FRAC_PI_3,
            }
        }
    })
}

/// The depth of a sixty-degree (equilateral) V groove of the given opening `width`.
///
/// # Errors
/// [`SolidError::InvalidInput`] when `width` is not finite and positive.
pub fn v_groove_height(width: f64) -> Result<f64> {
    if !width.is_finite() || width <= 0.0 {
        return Err(SolidError::invalid_input(
            "weld_symbol::v_groove_height",
            "the groove width must be finite and positive",
        ));
    }
    Ok(width * 3.0_f64.sqrt() / 2.0)
}

#[cfg(test)]
mod tests;
