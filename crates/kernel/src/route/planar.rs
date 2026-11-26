//! The router's planar helpers ([`solidcraft_kernel_api::Planar`]), as `solidcraft_kernel::planar`:
//! the arcs a sketch tool draws from its picks, the turn between two path directions, a frame
//! section's corners, and paper strokes.

use solidcore_record::{Loop2, Plane, Vec3};
use solidcraft_kernel_api::{OpKind, Planar};

pub use solidcraft_kernel_api::planar::PlanarRefused;

use super::{Which, which};
use crate::backend::Solidcore;

/// The unit direction from `from` to `to`; `None` where they coincide or are not finite.
#[must_use]
pub fn unit_between(from: [f64; 2], to: [f64; 2]) -> Option<[f64; 2]> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.unit_between(from, to),
    }
}

/// The unit direction from `from` to `to` turned a quarter turn counter-clockwise; `None` where
/// they coincide or are not finite.
#[must_use]
pub fn left_normal_between(from: [f64; 2], to: [f64; 2]) -> Option<[f64; 2]> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.left_normal_between(from, to),
    }
}

/// The centre and radius of the circle through `from` and `other` that touches the unit direction
/// `dir` at `from`; `None` where the points coincide or `dir` runs along their chord.
#[must_use]
pub fn tangent_arc_centre(from: [f64; 2], other: [f64; 2], dir: [f64; 2]) -> Option<([f64; 2], f64)> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.tangent_arc_centre(from, other, dir),
    }
}

/// The point of the perpendicular bisector of the chord `start`-`end` nearest `p`: the centre of
/// the arc through both ends that has it; `None` where the ends coincide.
#[must_use]
pub fn bisector_centre(start: [f64; 2], end: [f64; 2], p: [f64; 2]) -> Option<[f64; 2]> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.bisector_centre(start, end, p),
    }
}

/// Whether the unit direction `dir` at `p` is square to the radius from `centre`, within a
/// millionth; `false` where `p` is on `centre`.
#[must_use]
pub fn is_square_to_radius(centre: [f64; 2], p: [f64; 2], dir: [f64; 2]) -> bool {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.is_square_to_radius(centre, p, dir),
    }
}

/// The centre of the circle through `a`, `b` and `c`; `None` for points on one line.
#[must_use]
pub fn circumcentre(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 2]> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.circumcentre(a, b, c),
    }
}

/// The circle through `s`, `e` and `t`: its centre, radius and the sweep (radians) of the arc from
/// `s` to `e` that passes through `t`; `None` for points on one line.
#[must_use]
pub fn three_point_circle(s: [f64; 2], e: [f64; 2], t: [f64; 2]) -> Option<([f64; 2], f64, f64)> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.three_point_circle(s, e, t),
    }
}

/// The third point of the arc from `s` to `e` for the cursor `h` with a radius or a sweep (radians;
/// the sweep wins when both are given): on the arc of that size whose middle is nearest the cursor.
/// `None`: no such arc.
#[must_use]
pub fn arc_third_point(s: [f64; 2], e: [f64; 2], h: [f64; 2], radius: Option<f64>, sweep: Option<f64>) -> Option<[f64; 2]> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.arc_third_point(s, e, h, radius, sweep),
    }
}

/// The angle in radians, in `[0, PI]`, between the planar directions `before` (the first curve's
/// end tangent) and `after` (the next curve's start tangent): 0 where the path runs straight on,
/// `PI` where it doubles back. The directions need not be unit.
///
/// # Errors
/// The kernel's refusal, in its words.
pub fn joint_turn(before: [f64; 2], after: [f64; 2]) -> Result<f64, PlanarRefused> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.joint_turn(before, after),
    }
}

/// For each segment of `outline`, whether it is an inside corner's fillet: an arc turning against
/// the outline's winding (an angle's or a channel's root).
#[must_use]
pub fn inside_corner_arcs(outline: &Loop2) -> Vec<bool> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.inside_corner_arcs(outline),
    }
}

/// Whether `outline` is an open section's, like an angle, a channel or a beam: some corner turns
/// inward. A tube's or a bar's never does.
#[must_use]
pub fn is_open(outline: &Loop2) -> bool {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.is_open(outline),
    }
}

/// How far along `toward` from `end` its line leaves the box round a section, `rect` its
/// rectangle's corners in order (mm). `None` for a zero `toward`, a degenerate `rect` or a line
/// along the box.
#[must_use]
pub fn exit_through_box(end: Vec3, toward: Vec3, rect: [Vec3; 4]) -> Option<f64> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.exit_through_box(end, toward, rect),
    }
}

/// The length of a straight paper stroke from `a` to `b`.
#[must_use]
pub fn straight_length(a: [f64; 2], b: [f64; 2]) -> f64 {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.straight_length(a, b),
    }
}

/// Whether a line's endpoints project to distinct points in the view plane `basis`.
#[must_use]
pub fn projected_endpoints_are_distinct(basis: Plane, ends: [Vec3; 2]) -> bool {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.projected_endpoints_are_distinct(basis, ends),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::{joint_turn as turn, paper_gaps, sketch_arc};

    #[test]
    fn routed_planar_helpers_are_solidcores() {
        let (a, b, c) = ([0.0, 0.0], [4.0, 0.0], [2.0, 2.0]);
        assert_eq!(circumcentre(a, b, c), sketch_arc::circumcentre(a, b, c));
        assert_eq!(three_point_circle(a, b, c), sketch_arc::three_point_circle(a, b, c));
        assert_eq!(arc_third_point(a, b, c, Some(3.0), None), sketch_arc::arc_third_point(a, b, c, Some(3.0), None));
        assert_eq!(tangent_arc_centre(a, b, [0.0, 1.0]), sketch_arc::tangent_arc_centre(a, b, [0.0, 1.0]));
        assert_eq!(joint_turn([1.0, 0.0], [0.0, 1.0]).ok(), turn::joint_turn([1.0, 0.0], [0.0, 1.0]).ok());
        assert_eq!(straight_length(a, b), paper_gaps::straight_length(a, b));
        // a refusal carries solidcore's words whole
        let words = turn::joint_turn([0.0, 0.0], [0.0, 1.0]).unwrap_err().to_string();
        assert_eq!(joint_turn([0.0, 0.0], [0.0, 1.0]), Err(PlanarRefused(words)));
    }
}
