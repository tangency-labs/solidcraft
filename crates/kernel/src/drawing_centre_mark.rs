//! Paper geometry for a centre mark on a face-on circle. The caller projects the circle into
//! the view first; both the radius and overshoot given here are paper millimetres. Styling,
//! reference resolution and gaps against other annotations belong to the scene builder.

use solidcore_record::{Seg2, Vec2};
use solidcraft_geom::paper::{MAX_COORDINATE_MM, MAX_SIDE_MM};

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
    solidcore_ops::paper_gaps::centre_cross([centre_mm.x, centre_mm.y], radius_mm, overshoot_mm)
        .map(|strokes| strokes.map(|[a, b]| Seg2::Line { a: Vec2::new(a[0], a[1]), b: Vec2::new(b[0], b[1]) }))
        .ok_or(Invalid("the mark must have length and fit within the paper scene's coordinate limit"))
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
    let (start, end) =
        solidcore_ops::paper_gaps::extend([a.x, a.y], [b.x, b.y], overshoot_mm).map_err(|_| Invalid("the endpoints must be distinct"))?;
    let (start, end) = (Vec2::new(start[0], start[1]), Vec2::new(end[0], end[1]));
    if !start.is_finite() || !end.is_finite() || [start.x, start.y, end.x, end.y].iter().any(|v| v.abs() > MAX_COORDINATE_MM) {
        return Err(Invalid("the line must fit within the paper scene's coordinate limit"));
    }
    Ok(Seg2::Line { a: start, b: end })
}

#[cfg(test)]
#[path = "drawing_centre_mark_tests.rs"]
mod tests;

/// Paper obstacles from dimension lines and measured text, in sheet coordinates.
#[derive(Clone, Debug, Default)]
pub struct GapObstacles {
    pub lines: Vec<(Vec2, Vec2)>,
    pub boxes: Vec<[Vec2; 4]>,
}

/// Split a straight centre stroke around dimension crossings and measured text boxes.
/// Returned start distances let the caller advance the dash phase without restarting it.
pub fn gaps(a: Vec2, b: Vec2, obstacles: &GapObstacles, margin: f64) -> Result<Vec<(Seg2, f64)>, Invalid> {
    if obstacles.lines.len().saturating_add(obstacles.boxes.len()) > 100_000 {
        return Err(Invalid("too many dimension obstacles for centre annotation gaps"));
    }
    let lines = obstacles.lines.iter().map(|(p, q)| ([p.x, p.y], [q.x, q.y])).collect::<Vec<_>>();
    let boxes = obstacles.boxes.iter().map(|c| c.map(|p| [p.x, p.y])).collect::<Vec<_>>();
    solidcore_ops::paper_gaps::cut([a.x, a.y], [b.x, b.y], &lines, &boxes, margin)
        .map(|pieces| {
            pieces.into_iter().map(|p| (Seg2::Line { a: Vec2::new(p.start[0], p.start[1]), b: Vec2::new(p.end[0], p.end[1]) }, p.distance)).collect()
        })
        .map_err(|_| Invalid("invalid or excessive dimension gap geometry"))
}
