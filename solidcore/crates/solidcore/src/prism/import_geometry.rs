//! Scalar-unit conversion, chord coordinates and ellipse admission for planar source geometry.
//! Not upstream. Preserves the reader's ordinary multiplication and hypot order.
use crate::math::Vec2;

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
            "prism::import_geometry::chord_coordinates",
            "coordinates must be finite and the chord must have finite positive length",
        ));
    }
    let normalized = Vec2::new((point.x - leading_x) / chord, point.y / chord);
    if !normalized.x.is_finite() || !normalized.y.is_finite() {
        return Err(crate::SolidError::invalid_input(
            "prism::import_geometry::chord_coordinates",
            "coordinates in chord units must be finite",
        ));
    }
    Ok(normalized)
}
/// Scale a stored scalar with ordinary binary64 multiplication.
pub fn scale_value(value: f64, scale: f64) -> f64 {
    value * scale
}
/// Whether a nonzero source coordinate vanished when scaled.
pub fn collapsed_coordinate(source: Vec2, scaled: Vec2) -> bool {
    (source.x != 0.0 && scaled.x == 0.0) || (source.y != 0.0 && scaled.y == 0.0)
}
/// The support field that cannot be represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EllipseSupport {
    /// Finite, nondegenerate supporting axes.
    Valid,
    /// Invalid centre, major axis or ratio.
    InvalidMajor,
    /// The minor axis collapses in represented units.
    CollapsedMinor,
}
/// Admit the original major/minor support without repairing a collapsed axis.
pub fn ellipse_support(center: Vec2, major: Vec2, ratio: f64) -> EllipseSupport {
    let radius = major.x.hypot(major.y);
    let finite = |p: Vec2| p.x.is_finite() && p.y.is_finite();
    if !finite(center)
        || !finite(major)
        || major == Vec2::ZERO
        || !radius.is_finite()
        || radius <= 0.0
        || !ratio.is_finite()
        || ratio <= 0.0
        || ratio > 1.0
    {
        return EllipseSupport::InvalidMajor;
    }
    let minor = Vec2::new(-major.y * ratio, major.x * ratio);
    let minor_radius = radius * ratio;
    if !finite(minor) || minor == Vec2::ZERO || !minor_radius.is_finite() || minor_radius <= 0.0 {
        return EllipseSupport::CollapsedMinor;
    }
    EllipseSupport::Valid
}

/// Transform retained source anchors with one rounding per component.
/// The owning reader/writer retains its existing validation and error order.
pub fn transform_anchor(point: Vec2, scale: f64, translation: Vec2) -> Vec2 {
    Vec2::new(
        point.x.mul_add(scale, translation.x),
        point.y.mul_add(scale, translation.y),
    )
}
