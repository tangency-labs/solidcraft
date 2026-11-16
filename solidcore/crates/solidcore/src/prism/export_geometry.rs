//! Conservative bounds and positive affine transforms for planar export geometry.
//!
//! Not upstream. Relocated from the DXF owned model with the original outward bounds,
//! FMA transform order and explicit refusal domains unchanged. No recursion.

use super::path_export::PathRefusal;
use super::recognition::RecognitionVector;
use crate::geom::certified::next_up;
use crate::math::Vec2;
mod bulges;
pub use bulges::bounds as bulged_bounds;
type Result<T> = std::result::Result<T, PathRefusal>;
fn invalid(message: &str) -> PathRefusal {
    PathRefusal {
        piece: None,
        reason: message.into(),
    }
}

/// Bounds with conservative extrema in planar coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// Minimum coordinates.
    pub min: Vec2,
    /// Maximum coordinates.
    pub max: Vec2,
}
impl Bounds {
    /// Validate bounds without changing either endpoint.
    pub fn new(min: Vec2, max: Vec2) -> Result<Self> {
        if !min.is_finite() || !max.is_finite() || min.x > max.x || min.y > max.y {
            return Err(invalid("bounds must be finite and ordered"));
        }
        Ok(Self { min, max })
    }
    /// The enclosure of two boxes.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: Vec2::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Vec2::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }
}
fn finite_point(point: Vec2) -> Result<Vec2> {
    if point.is_finite() {
        Ok(point)
    } else {
        Err(invalid("nonfinite point"))
    }
}

fn positive(value: f64) -> Result<f64> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(invalid("dimension must be finite and positive"))
    }
}

/// Enclose a complete circular support, rounded outwards.
pub fn supporting_circle(c: Vec2, r: f64) -> Result<Bounds> {
    finite_point(c)?;
    positive(r)?;
    // One outward ULP encloses each correctly rounded sum/difference, including
    // subnormal results. Overflow/infinite outward bounds are explicitly refused.
    Bounds::new(
        Vec2::new(-next_up(-(c.x - r)), -next_up(-(c.y - r))),
        Vec2::new(next_up(c.x + r), next_up(c.y + r)),
    )
}

/// Enclose stored points without reconstructing geometry.
pub fn point_bounds(points: &[Vec2]) -> Result<Bounds> {
    let first = finite_point(
        *points
            .first()
            .ok_or_else(|| invalid("point array is empty"))?,
    )?;
    let mut bounds = Bounds::new(first, first)?;
    for point in points.iter().skip(1) {
        let point = finite_point(*point)?;
        bounds = bounds.union(Bounds::new(point, point)?);
    }
    Ok(bounds)
}

/// Enclose the supporting ellipse using outward L1 axis bounds.
pub fn ellipse_bounds(c: Vec2, major: Vec2, ratio: f64, t0: f64, t1: f64) -> Result<Bounds> {
    finite_point(c)?;
    finite_point(major)?;
    if major == Vec2::ZERO
        || !ratio.is_finite()
        || ratio <= 0.0
        || ratio > 1.0
        || !t0.is_finite()
        || !t1.is_finite()
    {
        return Err(invalid("invalid ellipse support or parameters"));
    }
    // |x-cx| <= |major.x| + ratio*|major.y|, likewise for y.
    // Each nonnegative product and sum is rounded outward independently.
    // abs is exact for finite f64. No hypot/trig accuracy assumption is used.
    let x = next_up(major.x.abs() + next_up(ratio * major.y.abs()));
    let y = next_up(major.y.abs() + next_up(ratio * major.x.abs()));
    Bounds::new(
        Vec2::new(-next_up(-(c.x - x)), -next_up(-(c.y - y))),
        Vec2::new(next_up(c.x + x), next_up(c.y + y)),
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
/// Whether the original degree-valued arc fields can be sampled finitely.
pub fn arc_fields_finite(a0: f64, a1: f64) -> bool {
    a0.is_finite()
        && a1.is_finite()
        && (a1 - a0).is_finite()
        && a0.to_radians().is_finite()
        && a1.to_radians().is_finite()
}
/// Whether an axis has a finite nonzero Euclidean length.
pub fn axis_nonzero(axis: Vec2) -> bool {
    axis.is_finite() && axis.length_hypot().is_finite() && axis.length_hypot() > 0.0
}
/// Whether the interval difference is finite.
pub fn finite_span(start: f64, end: f64) -> bool {
    (end - start).is_finite()
}
/// Whether a degree value has a finite radian representation.
pub fn finite_degrees(angle: f64) -> bool {
    angle.is_finite() && angle.to_radians().is_finite()
}
/// Whether rounded degree endpoints remain distinct modulo one turn.
pub fn distinct_degrees(start: f64, end: f64) -> bool {
    let span = end - start;
    span.is_finite() && span.rem_euclid(360.0) != 0.0
}
