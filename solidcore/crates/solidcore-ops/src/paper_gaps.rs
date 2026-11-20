//! Bounded intersections for a paper stroke and its dimension obstacles.

/// A visible part and its distance from the original start, in paper units.
pub use solidcore::geom::paper_gaps::Piece;

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
