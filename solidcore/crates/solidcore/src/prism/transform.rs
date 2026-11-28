//! Planar scaling and translation: a unit scale of points and scalars, chord coordinates, and a
//! uniform scale plus translation with one rounding per component (FMA).
//! Not upstream. Moved from SolidCraft's DXF reader and writer with their ordinary
//! multiplication and FMA order unchanged.
use super::arc_conversion::PathRefusal;
use super::bounds::{finite_point, positive};
use crate::math::Vec2;
type Result<T> = std::result::Result<T, PathRefusal>;

/// Scale coordinates without changing the source reader's refusal policy.
pub fn scale_point(point: Vec2, scale: f64) -> Vec2 {
    Vec2::new(point.x * scale, point.y * scale)
}
/// Express a planar point in units of an axis-aligned chord, with its nose at x = 0.
/// The same divisor scales both coordinates, preserving the section's aspect ratio.
/// This is the affine map ((x - `leading_x`) / c, y / c), c = `trailing_x` - `leading_x`.
///
/// # Errors
/// Non-finite coordinates, a non-positive or non-finite chord, or an unrepresentable result.
pub fn chord_coordinates(point: Vec2, leading_x: f64, trailing_x: f64) -> crate::Result<Vec2> {
    let chord = trailing_x - leading_x;
    if !point.x.is_finite()
        || !point.y.is_finite()
        || !leading_x.is_finite()
        || !trailing_x.is_finite()
        || !chord.is_finite()
        || chord <= 0.0
    {
        return Err(crate::SolidError::invalid_input(
            "prism::transform::chord_coordinates",
            "coordinates must be finite and the chord must have finite positive length",
        ));
    }
    let normalized = Vec2::new((point.x - leading_x) / chord, point.y / chord);
    if !normalized.x.is_finite() || !normalized.y.is_finite() {
        return Err(crate::SolidError::invalid_input(
            "prism::transform::chord_coordinates",
            "coordinates in chord units must be finite",
        ));
    }
    Ok(normalized)
}
/// Scale a stored scalar with ordinary binary64 multiplication.
pub fn scale_value(value: f64, scale: f64) -> f64 {
    value * scale
}
/// Transform retained source anchors with one rounding per component.
/// The owning reader/writer retains its existing validation and error order.
pub fn transform_anchor(point: Vec2, scale: f64, translation: Vec2) -> Vec2 {
    Vec2::new(
        point.x.mul_add(scale, translation.x),
        point.y.mul_add(scale, translation.y),
    )
}
/// Scale and translate with the original one-rounding FMA semantics.
pub fn transformed_point(point: Vec2, scale: f64, translation: Vec2) -> Result<Vec2> {
    finite_point(point)?;
    finite_point(Vec2::new(
        point.x.mul_add(scale, translation.x),
        point.y.mul_add(scale, translation.y),
    ))
}

/// Scale a dimension, rejecting nonpositive or nonfinite results.
pub fn scaled_positive(value: f64, scale: f64) -> Result<f64> {
    positive(value * scale)
}
/// Scale an axis without translating it, retaining ordinary multiplication rounding.
pub fn scaled_axis(axis: Vec2, scale: f64) -> Result<Vec2> {
    finite_point(Vec2::new(axis.x * scale, axis.y * scale))
}
