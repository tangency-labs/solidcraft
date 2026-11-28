//! The router's planar helpers ([`solidcraft_kernel_api::Planar`]), as `solidcraft_kernel::planar`:
//! the arcs a sketch tool draws from its picks, the side of a sketch curve a region lies on, the turn
//! between two path directions, a frame section's corners, paper strokes and the runs a round-joined
//! line is stroked in, where a board's drilled hole reaches, whether a path runs through a
//! profile's inside, and a sketch plane's normal and points
//! as a sketch solver holds them.

use solidcore_record::{Loop2, Plane, Vec2, Vec3};
use solidcraft_kernel_api::{OpKind, Planar};

pub use solidcraft_kernel_api::planar::{PlanarRefused, StrokeRun};

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

/// Whether `p` lies on the line through `origin` along `dir`, within `tol` of it; `false` where
/// `dir` is of no length.
#[must_use]
pub fn on_line(p: [f64; 2], origin: [f64; 2], dir: [f64; 2], tol: f64) -> bool {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.on_line(p, origin, dir, tol),
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

/// The distance between two points (an arc's radius, from its centre and an end).
#[must_use]
pub fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.distance(a, b),
    }
}

/// Whether the region on the left of the piece from `a` to `b` of a counter-clockwise loop lies
/// left of the line from `p` to `q` (`Some(true)`) or right of it (`Some(false)`); `None` when the
/// piece is not on that line, within `tol` times its size.
#[must_use]
pub fn left_side_along_line(a: [f64; 2], b: [f64; 2], p: [f64; 2], q: [f64; 2], tol: f64) -> Option<bool> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.left_side_along_line(a, b, p, q, tol),
    }
}

/// Whether the region on the left of an arc piece of a counter-clockwise loop (about `centre` with
/// `radius`, sweeping `sweep` radians) lies inside the circle about `c` of radius `r` (`Some(true)`)
/// or outside it (`Some(false)`); `None` when the piece is not on that circle, within `tol` times its
/// size.
#[must_use]
pub fn left_side_inside_circle(centre: [f64; 2], radius: f64, sweep: f64, c: [f64; 2], r: f64, tol: f64) -> Option<bool> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.left_side_inside_circle(centre, radius, sweep, c, r, tol),
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

/// How far past `end` along `toward` any point of a member's end leaves the box round a
/// section, `rect` its rectangle's corners in order: the farthest over `end` and the rectangles
/// `sections` placed square to the line at `end` (mm). `None` for a zero `toward`, a degenerate
/// `rect` or every line along the box.
#[must_use]
pub fn reach_through_box(end: Vec3, toward: Vec3, sections: &[[Vec3; 4]], rect: [Vec3; 4]) -> Option<f64> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.reach_through_box(end, toward, sections, rect),
    }
}

/// How far past `end` along `toward` a member's end is built to leave the round bars `rounds`
/// (each its axis's ends and radius, mm) that `end` lies in: the farthest any point of its
/// section runs on before leaving such a bar's far side, the section bounded by `sections` (each a
/// rectangle's corners in order, placed at `end`), from 0 to `most`; `Ok(0.0)` where the end meets
/// no bar.
///
/// # Errors
/// The kernel's refusal, in its words: a non-finite point or length, a zero `toward`, or a bad
/// radius for a bar the end lies in.
pub fn reach_through_rounds(end: Vec3, toward: Vec3, sections: &[[Vec3; 4]], rounds: &[(Vec3, Vec3, f64)], most: f64) -> Result<f64, PlanarRefused> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.reach_through_rounds(end, toward, sections, rounds, most),
    }
}

/// The length of a straight paper stroke from `a` to `b`.
#[must_use]
pub fn straight_length(a: [f64; 2], b: [f64; 2]) -> f64 {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.straight_length(a, b),
    }
}

/// The runs a paper line through `points` is stroked in so that its sharp turns get round joins:
/// cut at every turn sharper than the angle whose cosine is `smooth_turn_cos`. Points closer than
/// `min_step` are one point, and so are the two ends of a `closed` line that repeats its start; a
/// `closed` line without a sharp turn is one closed run.
#[must_use]
pub fn stroke_runs(points: &[[f64; 2]], closed: bool, min_step: f64, smooth_turn_cos: f64) -> Vec<StrokeRun> {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.stroke_runs(points, closed, min_step, smooth_turn_cos),
    }
}

/// Whether a line's endpoints project to distinct points in the view plane `basis`.
#[must_use]
pub fn projected_endpoints_are_distinct(basis: Plane, ends: [Vec3; 2]) -> bool {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.projected_endpoints_are_distinct(basis, ends),
    }
}

/// The direction from `from` to `to`, in radians anticlockwise from +X; `0` where the points
/// coincide or a coordinate is not finite.
#[must_use]
pub fn direction_angle(from: [f64; 2], to: [f64; 2]) -> f64 {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.direction_angle(from, to),
    }
}

/// Whether a drilled hole touches or crosses any of the closed `loops`: a stadium centred at
/// `center`, `size[0]` wide along `angle` (radians anticlockwise from +X) and `size[1]` across; a
/// non-finite or negative input reaches nothing.
#[must_use]
pub fn hole_reaches_loops(center: [f64; 2], size: [f64; 2], angle: f64, loops: &[Vec<[f64; 2]>]) -> bool {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.hole_reaches_loops(center, size, angle, loops),
    }
}

/// Whether a side of a board's outline lies on the wall of the drilled hole of
/// [`hole_reaches_loops`]: every one of `points` is, within `chord` (the sag of a wall drawn as
/// chords) or 0.6 % of the hole's radius, whichever is larger. No points, or a non-finite or
/// negative input, is on no wall.
#[must_use]
pub fn side_on_hole_wall(center: [f64; 2], size: [f64; 2], angle: f64, points: &[[f64; 2]], chord: f64) -> bool {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.side_on_hole_wall(center, size, angle, points, chord),
    }
}

/// Whether the polyline `path` runs through the inside of one of `regions` (each its outer
/// loop and then its holes, each loop a list of corners that closes back to its first), not
/// only along their outline or through one of its corners: a piece of it lies in a region
/// farther than `margin` from every loop, or it crosses a profile thinner than that from one
/// side to the other.
#[must_use]
pub fn path_runs_inside(path: &[[f64; 2]], regions: &[Vec<Vec<[f64; 2]>>], margin: f64) -> bool {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.path_runs_inside(path, regions, margin),
    }
}

/// The three points a sketch solver holds a circle by, in the orthonormal frame `x`, `y` about
/// `c`, at `radius` and starting `from` radians.
#[must_use]
pub fn circle_points(c: Vec3, x: Vec3, y: Vec3, radius: f64, from: f64) -> [Vec3; 3] {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.circle_points(c, x, y, radius, from),
    }
}

/// The three points a sketch solver holds a 2D arc's circle by, through its sketch `plane`: its
/// centre `c` and the arc's start `start`, in sketch coordinates.
#[must_use]
pub fn arc_circle_points(c: [f64; 2], start: [f64; 2], plane: &Plane) -> [Vec3; 3] {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.arc_circle_points(c, start, plane),
    }
}

/// Whether two planes lie on the same support, whichever way each faces.
#[must_use]
pub fn same_plane(a: &Plane, b: &Plane) -> bool {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.same_plane(a, b),
    }
}

/// The normal of an orthonormal plane.
#[must_use]
pub fn plane_normal(plane: &Plane) -> Vec3 {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.plane_normal(plane),
    }
}

/// Sketch point `point` in space, through its orthonormal plane.
#[must_use]
pub fn plane_point(plane: &Plane, point: Vec2) -> Vec3 {
    match which(OpKind::Planar) {
        Which::Solidcore => Solidcore.plane_point(plane, point),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::{board_drill, constraint, footprint, joint_turn as turn, paper_gaps, sketch_arc, stroke_runs as stroke};

    #[test]
    fn routed_planar_helpers_are_solidcores() {
        let (a, b, c) = ([0.0, 0.0], [4.0, 0.0], [2.0, 2.0]);
        assert_eq!(circumcentre(a, b, c), sketch_arc::circumcentre(a, b, c));
        assert_eq!(three_point_circle(a, b, c), sketch_arc::three_point_circle(a, b, c));
        assert_eq!(arc_third_point(a, b, c, Some(3.0), None), sketch_arc::arc_third_point(a, b, c, Some(3.0), None));
        assert_eq!(tangent_arc_centre(a, b, [0.0, 1.0]), sketch_arc::tangent_arc_centre(a, b, [0.0, 1.0]));
        assert_eq!(joint_turn([1.0, 0.0], [0.0, 1.0]).ok(), turn::joint_turn([1.0, 0.0], [0.0, 1.0]).ok());
        assert_eq!(straight_length(a, b), paper_gaps::straight_length(a, b));
        assert_eq!(on_line(c, a, [1.0, 1.0], 1e-9), sketch_arc::on_line(c, a, [1.0, 1.0], 1e-9));
        assert_eq!(on_line(c, a, b, 1e-9), sketch_arc::on_line(c, a, b, 1e-9));
        assert_eq!(distance(a, b), sketch_arc::distance(a, b));
        assert_eq!(left_side_along_line(a, [2.0, 0.0], a, b, 1e-9), sketch_arc::left_side_along_line(a, [2.0, 0.0], a, b, 1e-9));
        assert_eq!(left_side_along_line(a, [2.0, 0.0], a, b, 1e-9), Some(true));
        let quarter = std::f64::consts::FRAC_PI_2;
        assert_eq!(left_side_inside_circle(a, 2.0, quarter, a, 2.0, 1e-9), sketch_arc::left_side_inside_circle(a, 2.0, quarter, a, 2.0, 1e-9));
        assert_eq!(left_side_inside_circle(a, 2.0, quarter, a, 2.0, 1e-9), Some(true));
        assert_eq!(direction_angle([0.0, 0.0], [1.0, 1.0]), board_drill::direction_angle([0.0, 0.0], [1.0, 1.0]));
        let square = vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]];
        for c in [[5.0, 5.0], [9.5, 5.0], [0.0, 5.0]] {
            assert_eq!(hole_reaches_loops(c, [2.0, 1.0], 0.0, &square), board_drill::hole_reaches_loops(c, [2.0, 1.0], 0.0, &square));
            let side = [[c[0] + 1.0, c[1]], [c[0], c[1] + 0.5]];
            assert_eq!(side_on_hole_wall(c, [2.0, 1.0], 0.0, &side, 0.02), board_drill::side_on_hole_wall(c, [2.0, 1.0], 0.0, &side, 0.02));
        }
        let region = std::slice::from_ref(&square);
        for path in [vec![[5.0, 5.0]], vec![[-5.0, 5.0], [15.0, 5.0]], vec![[0.0, -5.0], [0.0, 15.0]]] {
            assert_eq!(path_runs_inside(&path, region, 0.1), footprint::path_runs_inside(&path, region, 0.1));
        }
        let xy = Plane::XY;
        let tilted = Plane { origin: Vec3::new(1.0, 2.0, 3.0), x: Vec3::new(0.0, 1.0, 0.0), y: Vec3::new(0.0, 0.0, 1.0) };
        for plane in [&xy, &tilted] {
            assert_eq!(plane_normal(plane), constraint::plane_normal(plane));
            assert_eq!(plane_point(plane, Vec2::new(2.0, -1.0)), constraint::plane_point(plane, Vec2::new(2.0, -1.0)));
            assert_eq!(arc_circle_points([1.0, 1.0], [3.0, 1.0], plane), constraint::arc_circle_points([1.0, 1.0], [3.0, 1.0], plane));
            assert_eq!(same_plane(plane, &tilted), constraint::same_plane(plane, &tilted));
        }
        let (o, x, y) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(circle_points(o, x, y, 2.5, 0.3), constraint::circle_points(o, x, y, 2.5, 0.3));
        // a line's stroke runs are solidcore's, bit for bit
        let lines: [(&[[f64; 2]], bool); 6] = [
            (&[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0], [0.0, 0.0]], true),
            (&[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]], false),
            (&[[0.0, 0.0], [1.0, 0.1], [2.0, 0.3], [3.0, 0.6], [3.0, 0.6], [2.0, 2.0]], false),
            (&[[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0], [1.0, 0.0]], true),
            (&[[0.0, 0.0], [0.0, 0.0]], false),
            (&[], false),
        ];
        let bits = |runs: Vec<(Vec<[f64; 2]>, bool)>| -> Vec<(Vec<[u64; 2]>, bool)> {
            runs.into_iter().map(|(p, c)| (p.iter().map(|q| [q[0].to_bits(), q[1].to_bits()]).collect(), c)).collect()
        };
        for (points, closed) in lines {
            let routed = stroke_runs(points, closed, 1e-3, 0.866).into_iter().map(|r| (r.points, r.closed)).collect();
            let ops = stroke::runs(points, closed, 1e-3, 0.866).into_iter().map(|r| (r.points, r.closed)).collect();
            assert_eq!(bits(routed), bits(ops));
        }
        // a refusal carries solidcore's words whole
        let words = turn::joint_turn([0.0, 0.0], [0.0, 1.0]).unwrap_err().to_string();
        assert_eq!(joint_turn([0.0, 0.0], [0.0, 1.0]), Err(PlanarRefused(words)));
    }
}
