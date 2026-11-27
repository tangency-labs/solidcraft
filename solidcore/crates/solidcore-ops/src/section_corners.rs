//! A frame member section's corners and how far an end reaches: the kernel's
//! [`solidcore::query::section_corners`] measures on the record's types, compared with
//! tolerances here.

use solidcore::query::section_corners as k;
use solidcore_convert::{loop2, vec3};
use solidcore_record::{Loop2, Vec3};

/// A side turning into the next by more than this the wrong way (a cross product of side vectors
/// times the winding, below it) is an inside corner.
const INWARD: f64 = -1e-9;

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
    m.inward_arcs.iter().any(|a| *a) || m.sharpest_turn < INWARD
}

/// How far along `toward` from `end` its line leaves the box round a section, `rect` its
/// rectangle's corners in order (mm): the nearest side it crosses, its sides square to the
/// rectangle's plane without end. `None` for a zero `toward`, a degenerate `rect` or a
/// line along the box.
#[must_use]
pub fn exit_through_box(end: Vec3, toward: Vec3, rect: [Vec3; 4]) -> Option<f64> {
    k::exit_through_box(vec3(end), vec3(toward), &rect.map(vec3))
        .ok()
        .flatten()
}

/// How far along `toward` from `start` its line leaves the round bar of `radius` (mm) about the
/// line `axis`, without end. `None` for a zero `toward`, a radius not finite and positive, a
/// degenerate axis, or a line that misses the bar or runs along it.
#[must_use]
pub fn exit_through_round(
    start: Vec3,
    toward: Vec3,
    axis: (Vec3, Vec3),
    radius: f64,
) -> Option<f64> {
    k::exit_through_round(
        vec3(start),
        vec3(toward),
        (vec3(axis.0), vec3(axis.1)),
        radius,
    )
    .ok()
    .flatten()
}
