//! The runs a paper line is stroked in when its sharp turns get round joins.

/// One run: the points from one sharp turn or line end to the next, or a whole loop without a
/// sharp turn (`closed`).
pub use solidcore::geom::stroke_runs::Run;

/// The runs of the polyline through `points`, cut at every turn sharper than the angle whose
/// cosine is `smooth_turn_cos`. Points closer than `min_step` are one point, and so are the two
/// ends of a `closed` line that repeats its start; a `closed` line without a sharp turn is one
/// closed run.
pub fn runs(points: &[[f64; 2]], closed: bool, min_step: f64, smooth_turn_cos: f64) -> Vec<Run> {
    solidcore::geom::stroke_runs::runs(points, closed, min_step, smooth_turn_cos)
}
