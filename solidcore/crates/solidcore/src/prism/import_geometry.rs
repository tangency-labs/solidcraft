//! Scalar-unit conversion and ellipse admission for planar source geometry.
//! Not upstream. Preserves the reader's ordinary multiplication and hypot order.
use crate::math::Vec2;

/// Scale coordinates without changing the source reader's refusal policy.
pub fn scale_point(point: Vec2, scale: f64) -> Vec2 {
    Vec2::new(point.x * scale, point.y * scale)
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
