//! Drilled holes on a printed circuit board: a slot's direction and whether a hole reaches the
//! board's edge or a cutout.

/// The direction from `from` to `to`, in radians anticlockwise from +X; `0` when the points
/// coincide or a coordinate is not finite.
#[must_use]
pub fn direction_angle(from: [f64; 2], to: [f64; 2]) -> f64 {
    solidcore::geom::board_drill::direction_angle(from, to)
}

/// Whether a drilled hole touches or crosses any of the closed `loops`. The hole is a stadium
/// centred at `center`, `size[0]` wide along `angle` (radians anticlockwise from +X) and `size[1]`
/// across; a non-finite or negative input reaches nothing.
#[must_use]
pub fn hole_reaches_loops(
    center: [f64; 2],
    size: [f64; 2],
    angle: f64,
    loops: &[Vec<[f64; 2]>],
) -> bool {
    solidcore::geom::board_drill::hole_reaches_loops(center, size, angle, loops)
}
