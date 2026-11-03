//! Paper geometry for a centre mark on a face-on circle. The caller projects the circle into
//! the view first; both the radius and overshoot given here are paper millimetres. Styling,
//! reference resolution and gaps against other annotations belong to the scene builder.

use crate::paper::{MAX_COORDINATE_MM, MAX_SIDE_MM};
use crate::{EPS, Seg2, Vec2};

/// A centre mark that cannot be represented on the paper scene.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("centre mark: {0}")]
pub struct Invalid(pub &'static str);

/// Two perpendicular, view-aligned strokes through `centre_mm`. The strokes each reach
/// `overshoot_mm` beyond the circle on both sides. A caller applies view scale to the circle,
/// not to the overshoot. Each returned segment is a complete path for the paper scene's line
/// style; no dashes or interruption gaps are baked into the geometry.
pub fn strokes(centre_mm: Vec2, radius_mm: f64, overshoot_mm: f64) -> Result<[Seg2; 2], Invalid> {
    if !centre_mm.is_finite() {
        return Err(Invalid("the centre must be finite"));
    }
    if !radius_mm.is_finite() || radius_mm <= 0.0 {
        return Err(Invalid("the paper radius must be finite and above zero"));
    }
    if !overshoot_mm.is_finite() || !(0.0..=MAX_SIDE_MM).contains(&overshoot_mm) {
        return Err(Invalid("the paper overshoot must be finite and between zero and the maximum sheet side"));
    }
    let half = radius_mm + overshoot_mm;
    if !half.is_finite() || half <= EPS || centre_mm.x.abs() + half > MAX_COORDINATE_MM || centre_mm.y.abs() + half > MAX_COORDINATE_MM {
        return Err(Invalid("the mark must have length and fit within the paper scene's coordinate limit"));
    }
    let x = Vec2::X * half;
    let y = Vec2::Y * half;
    Ok([Seg2::Line { a: centre_mm - x, b: centre_mm + x }, Seg2::Line { a: centre_mm - y, b: centre_mm + y }])
}

/// A centre line between two already projected paper points, extended by `overshoot_mm` at each
/// end. Selection and associativity are the caller's responsibility; this does not choose what
/// model references establish the endpoints.
pub fn centre_line(a: Vec2, b: Vec2, overshoot_mm: f64) -> Result<Seg2, Invalid> {
    if !a.is_finite() || !b.is_finite() {
        return Err(Invalid("the endpoints must be finite"));
    }
    if !overshoot_mm.is_finite() || !(0.0..=MAX_SIDE_MM).contains(&overshoot_mm) {
        return Err(Invalid("the paper overshoot must be finite and between zero and the maximum sheet side"));
    }
    let along = (b - a).normalized().ok_or(Invalid("the endpoints must be distinct"))?;
    let start = a - along * overshoot_mm;
    let end = b + along * overshoot_mm;
    if !start.is_finite() || !end.is_finite() || [start.x, start.y, end.x, end.y].iter().any(|v| v.abs() > MAX_COORDINATE_MM) {
        return Err(Invalid("the line must fit within the paper scene's coordinate limit"));
    }
    Ok(Seg2::Line { a: start, b: end })
}

#[cfg(test)]
#[path = "drawing_centre_mark_tests.rs"]
mod tests;
