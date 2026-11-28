//! A frame member section's corners and how far an end reaches: the kernel's
//! [`solidcore::query::section_corners`] measures on the record's types, compared with
//! tolerances here.

use solidcore::query::section_corners as k;
use solidcore_convert::{loop2, record_vec3, vec3};

use crate::error::status;
use crate::{Body, Error, Result};
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

/// How far past `end` along `toward` a member's end is built to leave the round bars `rounds`
/// (each its axis's ends and radius, mm) that `end` lies in: the farthest any point of its
/// section runs on before leaving such a bar's far side, the section bounded by `sections` (each
/// a rectangle's corners in order, placed at `end`), from 0 to `most`. `Ok(0.0)` where the end
/// meets no bar.
///
/// # Errors
///
/// The kernel's refusal ([`k::reach_through_rounds`]): a non-finite point or length, a zero
/// `toward`, or a bad radius for a bar the end lies in.
pub fn reach_through_rounds(
    end: Vec3,
    toward: Vec3,
    sections: &[[Vec3; 4]],
    rounds: &[(Vec3, Vec3, f64)],
    most: f64,
) -> Result<f64> {
    let sections: Vec<_> = sections.iter().map(|r| r.map(vec3)).collect();
    let rounds: Vec<_> = rounds
        .iter()
        .map(|(a, b, r)| (vec3(*a), vec3(*b), *r))
        .collect();
    k::reach_through_rounds(vec3(end), vec3(toward), &sections, &rounds, most).map_err(status)
}

/// How far `body` runs past `end` along `toward` (mm): its greatest level above the plane
/// through `end` square to `toward` ([`k::square_axes`], [`Body::levels_along`]), 0 when none of
/// it lies past.
///
/// # Errors
///
/// [`Error::Argument`] for a zero or non-finite `toward`; the refusals of
/// [`Body::levels_along`].
pub fn reach_past(body: &Body, end: Vec3, toward: Vec3) -> Result<f64> {
    let (x, y) = k::square_axes(vec3(toward)).map_err(|_| Error::Argument)?;
    let [_, high] = body.levels_along(end.into(), record_vec3(x).into(), record_vec3(y).into())?;
    Ok(high.max(0.0))
}
