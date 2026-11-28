//! Planar helpers ([`Planar`]): the arcs a sketch tool draws from its picks, the side of a sketch
//! curve a region lies on, the turn between two path directions, a frame section's corners, paper
//! strokes and the runs a round-joined line is stroked in, where a board's drilled hole reaches,
//! whether a path runs through a profile's inside, and a sketch plane's normal and points as a sketch solver holds them. Plain numbers and the
//! record's values in, plain numbers out.

use solidcore_record::{Loop2, Plane, Vec2, Vec3};

use crate::Backend;

/// A planar helper the kernel refuses, in its words: a direction not finite or of no length.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PlanarRefused(pub String);

/// One run of a stroked line ([`Planar::stroke_runs`]): the points from one sharp turn or line end
/// to the next, or a whole loop that has no sharp turn.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeRun {
    /// The run's points in order. A sharp turn is the last point of one run and the first of the
    /// next.
    pub points: Vec<[f64; 2]>,
    /// Whether the run is a loop without ends: its last point joins its first.
    pub closed: bool,
}

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
    /// Whether `p` lies on the line through `origin` along `dir`, within `tol` of it; `false`
    /// where `dir` is of no length.
    fn on_line(&self, p: [f64; 2], origin: [f64; 2], dir: [f64; 2], tol: f64) -> bool;
    /// The centre of the circle through `a`, `b` and `c`; `None` for points on one line.
    fn circumcentre(&self, a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 2]>;
    /// The circle through `s`, `e` and `t`: its centre, radius and the sweep (radians) of the arc
    /// from `s` to `e` that passes through `t`; `None` for points on one line.
    fn three_point_circle(&self, s: [f64; 2], e: [f64; 2], t: [f64; 2]) -> Option<([f64; 2], f64, f64)>;
    /// The third point of the arc from `s` to `e` for the cursor `h` with a radius or a sweep
    /// (radians; the sweep wins when both are given): on the arc of that size whose middle is
    /// nearest the cursor. `None`: no such arc.
    fn arc_third_point(&self, s: [f64; 2], e: [f64; 2], h: [f64; 2], radius: Option<f64>, sweep: Option<f64>) -> Option<[f64; 2]>;
    /// The distance between two points (an arc's radius, from its centre and an end).
    fn distance(&self, a: [f64; 2], b: [f64; 2]) -> f64;
    /// Whether the region on the left of the piece from `a` to `b` of a counter-clockwise loop
    /// lies left of the line from `p` to `q` (`Some(true)`) or right of it (`Some(false)`);
    /// `None` when the piece is not on that line, within `tol` times its size.
    fn left_side_along_line(&self, a: [f64; 2], b: [f64; 2], p: [f64; 2], q: [f64; 2], tol: f64) -> Option<bool>;
    /// Whether the region on the left of an arc piece of a counter-clockwise loop (about `centre`
    /// with `radius`, sweeping `sweep` radians) lies inside the circle about `c` of radius `r`
    /// (`Some(true)`) or outside it (`Some(false)`); `None` when the piece is not on that circle,
    /// within `tol` times its size.
    fn left_side_inside_circle(&self, centre: [f64; 2], radius: f64, sweep: f64, c: [f64; 2], r: f64, tol: f64) -> Option<bool>;
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
    /// How far past `end` along `toward` any point of a member's end leaves the box round a
    /// section, `rect` its rectangle's corners in order: the farthest over `end` and the rectangles
    /// `sections` placed square to the line at `end` (mm). `None` for a zero `toward`, a degenerate
    /// `rect` or every line along the box.
    fn reach_through_box(&self, end: Vec3, toward: Vec3, sections: &[[Vec3; 4]], rect: [Vec3; 4]) -> Option<f64>;
    /// How far past `end` along `toward` a member's end is built to leave the round bars `rounds`
    /// (each its axis's ends and radius, mm) that `end` lies in: the farthest any point of its
    /// section runs on before leaving such a bar's far side, the section bounded by `sections` (each a
    /// rectangle's corners in order, placed at `end`), from 0 to `most`; `Ok(0.0)` where the end meets
    /// no bar.
    ///
    /// # Errors
    /// The kernel's refusal, in its words: a non-finite point or length, a zero `toward`, or a bad
    /// radius for a bar the end lies in.
    fn reach_through_rounds(
        &self,
        end: Vec3,
        toward: Vec3,
        sections: &[[Vec3; 4]],
        rounds: &[(Vec3, Vec3, f64)],
        most: f64,
    ) -> Result<f64, PlanarRefused>;
    /// The length of a straight paper stroke from `a` to `b`.
    fn straight_length(&self, a: [f64; 2], b: [f64; 2]) -> f64;
    /// The runs a paper line through `points` is stroked in so that its sharp turns get round
    /// joins: cut at every turn sharper than the angle whose cosine is `smooth_turn_cos`. Points
    /// closer than `min_step` are one point, and so are the two ends of a `closed` line that
    /// repeats its start; a `closed` line without a sharp turn is one closed run.
    fn stroke_runs(&self, points: &[[f64; 2]], closed: bool, min_step: f64, smooth_turn_cos: f64) -> Vec<StrokeRun>;
    /// Whether a line's endpoints project to distinct points in the view plane `basis`.
    fn projected_endpoints_are_distinct(&self, basis: Plane, ends: [Vec3; 2]) -> bool;
    /// The direction from `from` to `to`, in radians anticlockwise from +X; `0` where the points
    /// coincide or a coordinate is not finite.
    fn direction_angle(&self, from: [f64; 2], to: [f64; 2]) -> f64;
    /// Whether a drilled hole touches or crosses any of the closed `loops`: a stadium centred at
    /// `center`, `size[0]` wide along `angle` (radians anticlockwise from +X) and `size[1]`
    /// across; a non-finite or negative input reaches nothing.
    fn hole_reaches_loops(&self, center: [f64; 2], size: [f64; 2], angle: f64, loops: &[Vec<[f64; 2]>]) -> bool;
    /// Whether a side of a board's outline lies on the wall of the drilled hole of
    /// `hole_reaches_loops`: every one of `points` is, within `chord` (the sag of a wall drawn as
    /// chords) or 0.6 % of the hole's radius, whichever is larger. No points, or a non-finite or
    /// negative input, is on no wall.
    fn side_on_hole_wall(&self, center: [f64; 2], size: [f64; 2], angle: f64, points: &[[f64; 2]], chord: f64) -> bool;
    /// Whether the polyline `path` runs through the inside of one of `regions` (each its outer
    /// loop and then its holes, each loop a list of corners that closes back to its first), not
    /// only along their outline or through one of its corners: a piece of it lies in a region
    /// farther than `margin` from every loop, or it crosses a profile thinner than that from one
    /// side to the other.
    fn path_runs_inside(&self, path: &[[f64; 2]], regions: &[Vec<Vec<[f64; 2]>>], margin: f64) -> bool;
    /// The three points a sketch solver holds a circle by, in the orthonormal frame `x`, `y` about
    /// `c`, at `radius` and starting `from` radians.
    fn circle_points(&self, c: Vec3, x: Vec3, y: Vec3, radius: f64, from: f64) -> [Vec3; 3];
    /// The three points a sketch solver holds a 2D arc's circle by, through its sketch `plane`:
    /// its centre `c` and the arc's start `start`, in sketch coordinates.
    fn arc_circle_points(&self, c: [f64; 2], start: [f64; 2], plane: &Plane) -> [Vec3; 3];
    /// Whether two planes lie on the same support, whichever way each faces.
    fn same_plane(&self, a: &Plane, b: &Plane) -> bool;
    /// The normal of an orthonormal plane.
    fn plane_normal(&self, plane: &Plane) -> Vec3;
    /// Sketch point `point` in space, through its orthonormal plane.
    fn plane_point(&self, plane: &Plane, point: Vec2) -> Vec3;
}
