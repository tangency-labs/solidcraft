//! Whether a path seen in a sketch plane runs through the inside of a profile, not only along its
//! outline or through one of its corners.

/// Whether the polyline `path` runs through the inside of one of `regions` (each its outer loop
/// and then its holes, each loop a list of corners that closes back to its first), not only along
/// their outline or through one of its corners: a piece of it lies in a region farther than
/// `margin` from every loop, or it crosses a profile thinner than that from one side to the other.
#[must_use]
pub fn path_runs_inside(path: &[[f64; 2]], regions: &[Vec<Vec<[f64; 2]>>], margin: f64) -> bool {
    solidcore::geom::footprint::path_runs_inside(path, regions, margin)
}
