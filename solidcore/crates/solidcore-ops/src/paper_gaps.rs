//! Bounded intersections for a paper stroke and its dimension obstacles.

/// A visible part and its distance from the original start, in paper units.
pub use solidcore::geom::paper_gaps::Piece;

/// The length of a straight paper stroke.
pub fn straight_length(a: [f64; 2], b: [f64; 2]) -> f64 {
    solidcore::geom::paper_gaps::straight_length(a, b)
}

/// Whether a line's endpoints project to distinct points in the given view plane.
pub fn projected_endpoints_are_distinct(
    basis: solidcore_record::Plane,
    ends: [solidcore_record::Vec3; 2],
) -> bool {
    solidcore::geom::paper_gaps::projected_endpoints_are_distinct(
        basis.origin.into(),
        [basis.x.into(), basis.y.into()],
        ends.map(Into::into),
    )
}

/// Two orthogonal strokes through a circle, extending this far past its radius on either side.
pub fn centre_cross(centre: [f64; 2], radius: f64, overshoot: f64) -> Option<[[[f64; 2]; 2]; 2]> {
    solidcore::geom::paper_gaps::centre_cross(centre, radius, overshoot)
}

/// Cut a straight paper stroke around line crossings and convex text boxes.
///
/// # Errors
/// Returns an error for non-finite, out-of-range, degenerate or excessive input.
pub fn cut(
    a: [f64; 2],
    b: [f64; 2],
    lines: &[([f64; 2], [f64; 2])],
    boxes: &[[[f64; 2]; 4]],
    margin: f64,
) -> crate::Result<Vec<Piece>> {
    solidcore::geom::paper_gaps::cut(a, b, lines, boxes, margin).map_err(|_| crate::Error::Argument)
}

/// Lengthen a straight paper stroke by `overshoot` past each end, along its own direction; with
/// no `overshoot`, whether its ends are apart. The ends may lie anywhere finite.
///
/// # Errors
/// Returns an error for non-finite input or output, an out-of-range `overshoot`, or ends that
/// coincide.
pub fn extend(a: [f64; 2], b: [f64; 2], overshoot: f64) -> crate::Result<([f64; 2], [f64; 2])> {
    solidcore::geom::paper_gaps::extend(a, b, overshoot).map_err(|_| crate::Error::Argument)
}
