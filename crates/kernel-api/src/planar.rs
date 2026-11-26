//! Planar helpers ([`Planar`]): the arcs a sketch tool draws from its picks, the turn between
//! two path directions, a frame section's corners, and paper strokes. Plain numbers and the
//! record's values in, plain numbers out.

use solidcore_record::{Loop2, Plane, Vec3};

use crate::Backend;

/// A planar helper the kernel refuses, in its words: a direction not finite or of no length.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PlanarRefused(pub String);

/// The planar helpers of one kernel ([`crate::OpKind::Planar`]).
pub trait Planar: Backend {
    /// The unit direction from `from` to `to`; `None` where they coincide or are not finite.
    fn unit_between(&self, from: [f64; 2], to: [f64; 2]) -> Option<[f64; 2]>;
    /// The unit direction from `from` to `to` turned a quarter turn counter-clockwise; `None` where
    /// they coincide or are not finite.
    fn left_normal_between(&self, from: [f64; 2], to: [f64; 2]) -> Option<[f64; 2]>;
    /// The centre and radius of the circle through `from` and `other` that touches the unit
    /// direction `dir` at `from`; `None` where the points coincide or `dir` runs along their chord.
    fn tangent_arc_centre(&self, from: [f64; 2], other: [f64; 2], dir: [f64; 2]) -> Option<([f64; 2], f64)>;
    /// The point of the perpendicular bisector of the chord `start`-`end` nearest `p`: the centre
    /// of the arc through both ends that has it; `None` where the ends coincide.
    fn bisector_centre(&self, start: [f64; 2], end: [f64; 2], p: [f64; 2]) -> Option<[f64; 2]>;
    /// Whether the unit direction `dir` at `p` is square to the radius from `centre`, within a
    /// millionth; `false` where `p` is on `centre`.
    fn is_square_to_radius(&self, centre: [f64; 2], p: [f64; 2], dir: [f64; 2]) -> bool;
    /// The centre of the circle through `a`, `b` and `c`; `None` for points on one line.
    fn circumcentre(&self, a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 2]>;
    /// The circle through `s`, `e` and `t`: its centre, radius and the sweep (radians) of the arc
    /// from `s` to `e` that passes through `t`; `None` for points on one line.
    fn three_point_circle(&self, s: [f64; 2], e: [f64; 2], t: [f64; 2]) -> Option<([f64; 2], f64, f64)>;
    /// The third point of the arc from `s` to `e` for the cursor `h` with a radius or a sweep
    /// (radians; the sweep wins when both are given): on the arc of that size whose middle is
    /// nearest the cursor. `None`: no such arc.
    fn arc_third_point(&self, s: [f64; 2], e: [f64; 2], h: [f64; 2], radius: Option<f64>, sweep: Option<f64>) -> Option<[f64; 2]>;
    /// The angle in radians, in `[0, PI]`, between the planar directions `before` (the first
    /// curve's end tangent) and `after` (the next curve's start tangent): 0 where the path runs
    /// straight on, `PI` where it doubles back. The directions need not be unit.
    fn joint_turn(&self, before: [f64; 2], after: [f64; 2]) -> Result<f64, PlanarRefused>;
    /// For each segment of `outline`, whether it is an inside corner's fillet: an arc turning
    /// against the outline's winding (an angle's or a channel's root).
    fn inside_corner_arcs(&self, outline: &Loop2) -> Vec<bool>;
    /// Whether `outline` is an open section's, like an angle, a channel or a beam: some corner
    /// turns inward. A tube's or a bar's never does.
    fn is_open(&self, outline: &Loop2) -> bool;
    /// How far along `toward` from `end` its line leaves the box round a section, `rect` its
    /// rectangle's corners in order (mm). `None` for a zero `toward`, a degenerate `rect` or a line
    /// along the box.
    fn exit_through_box(&self, end: Vec3, toward: Vec3, rect: [Vec3; 4]) -> Option<f64>;
    /// The length of a straight paper stroke from `a` to `b`.
    fn straight_length(&self, a: [f64; 2], b: [f64; 2]) -> f64;
    /// Whether a line's endpoints project to distinct points in the view plane `basis`.
    fn projected_endpoints_are_distinct(&self, basis: Plane, ends: [Vec3; 2]) -> bool;
}
