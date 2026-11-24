//! A frame member section's corners and how far an end reaches: the kernel's
//! [`solidcore::query::section_corners`] measures on the record's types, compared with
//! tolerances here.

use solidcore::query::section_corners as k;
use solidcore_convert::{loop2, vec3};
use solidcore_record::{Loop2, Vec3};

/// A side turning into the next by less than this (a cross product of side vectors times the
/// winding) is not an inside corner.
const TURN_TOL: f64 = 1e-9;

/// For each segment of `outline`, whether it is an inside corner's fillet: an arc turning against
/// the outline's winding (an angle's or a channel's root). Squaring one would cut into the
/// section's material.
#[must_use]
pub fn inside_corner_arcs(outline: &Loop2) -> Vec<bool> {
    k::measure_corner_turns(&loop2(outline)).inward_arcs
}

/// Whether `outline` is an open section's, like an angle, a channel or a beam: some corner turns
/// inward (a side meeting the next at an inside corner, or an inside corner's fillet). A tube's or
/// a bar's never does.
#[must_use]
pub fn is_open(outline: &Loop2) -> bool {
    let m = k::measure_corner_turns(&loop2(outline));
    m.inward_arcs.iter().any(|a| *a) || m.sharpest_turn < -TURN_TOL
}

/// How far past `end` along `toward` the furthest of `points` lies (mm); `None` for no points or
/// a zero `toward`.
#[must_use]
pub fn reach_along(end: Vec3, toward: Vec3, points: &[Vec3]) -> Option<f64> {
    let pts: Vec<solidcore::Vec3> = points.iter().copied().map(vec3).collect();
    k::reach_along(vec3(end), vec3(toward), &pts).ok().flatten()
}
