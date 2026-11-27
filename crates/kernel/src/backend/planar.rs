//! solidcore's planar helpers ([`Planar`]): each the solidcore-ops call this crate's callers made
//! before the interface existed, its refusal's words kept whole.

use solidcore_ops::{board_drill, constraint, footprint, joint_turn, paper_gaps, prism, section_corners, sketch_arc, stroke_runs};
use solidcore_record::{Loop2, Plane, Seg2, Vec2, Vec3};
use solidcraft_kernel_api::planar::{Planar, PlanarRefused, StrokeRun};

use super::Solidcore;

impl Planar for Solidcore {
    fn unit_between(&self, from: [f64; 2], to: [f64; 2]) -> Option<[f64; 2]> {
        sketch_arc::unit_between(from, to)
    }

    fn left_normal_between(&self, from: [f64; 2], to: [f64; 2]) -> Option<[f64; 2]> {
        sketch_arc::left_normal_between(from, to)
    }

    fn tangent_arc_centre(&self, from: [f64; 2], other: [f64; 2], dir: [f64; 2]) -> Option<([f64; 2], f64)> {
        sketch_arc::tangent_arc_centre(from, other, dir)
    }

    fn bisector_centre(&self, start: [f64; 2], end: [f64; 2], p: [f64; 2]) -> Option<[f64; 2]> {
        sketch_arc::bisector_centre(start, end, p)
    }

    fn is_square_to_radius(&self, centre: [f64; 2], p: [f64; 2], dir: [f64; 2]) -> bool {
        sketch_arc::is_square_to_radius(centre, p, dir)
    }

    fn on_line(&self, p: [f64; 2], origin: [f64; 2], dir: [f64; 2], tol: f64) -> bool {
        sketch_arc::on_line(p, origin, dir, tol)
    }

    fn circumcentre(&self, a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 2]> {
        sketch_arc::circumcentre(a, b, c)
    }

    fn three_point_circle(&self, s: [f64; 2], e: [f64; 2], t: [f64; 2]) -> Option<([f64; 2], f64, f64)> {
        sketch_arc::three_point_circle(s, e, t)
    }

    fn arc_third_point(&self, s: [f64; 2], e: [f64; 2], h: [f64; 2], radius: Option<f64>, sweep: Option<f64>) -> Option<[f64; 2]> {
        sketch_arc::arc_third_point(s, e, h, radius, sweep)
    }

    fn distance(&self, a: [f64; 2], b: [f64; 2]) -> f64 {
        sketch_arc::distance(a, b)
    }

    fn left_side_along_line(&self, a: [f64; 2], b: [f64; 2], p: [f64; 2], q: [f64; 2], tol: f64) -> Option<bool> {
        sketch_arc::left_side_along_line(a, b, p, q, tol)
    }

    fn left_side_inside_circle(&self, centre: [f64; 2], radius: f64, sweep: f64, c: [f64; 2], r: f64, tol: f64) -> Option<bool> {
        sketch_arc::left_side_inside_circle(centre, radius, sweep, c, r, tol)
    }

    fn joint_turn(&self, before: [f64; 2], after: [f64; 2]) -> Result<f64, PlanarRefused> {
        joint_turn::joint_turn(before, after).map_err(|e| PlanarRefused(e.to_string()))
    }

    fn inside_corner_arcs(&self, outline: &Loop2) -> Vec<bool> {
        section_corners::inside_corner_arcs(outline)
    }

    fn is_open(&self, outline: &Loop2) -> bool {
        section_corners::is_open(outline)
    }

    fn exit_through_box(&self, end: Vec3, toward: Vec3, rect: [Vec3; 4]) -> Option<f64> {
        section_corners::exit_through_box(end, toward, rect)
    }

    fn reach_through_rounds(
        &self,
        end: Vec3,
        toward: Vec3,
        sections: &[[Vec3; 4]],
        rounds: &[(Vec3, Vec3, f64)],
        most: f64,
    ) -> Result<f64, PlanarRefused> {
        section_corners::reach_through_rounds(end, toward, sections, rounds, most).map_err(|e| PlanarRefused(e.to_string()))
    }

    fn straight_length(&self, a: [f64; 2], b: [f64; 2]) -> f64 {
        paper_gaps::straight_length(a, b)
    }

    fn stroke_runs(&self, points: &[[f64; 2]], closed: bool, min_step: f64, smooth_turn_cos: f64) -> Vec<StrokeRun> {
        stroke_runs::runs(points, closed, min_step, smooth_turn_cos)
            .into_iter()
            .map(|run| StrokeRun { points: run.points, closed: run.closed })
            .collect()
    }

    fn projected_endpoints_are_distinct(&self, basis: Plane, ends: [Vec3; 2]) -> bool {
        paper_gaps::projected_endpoints_are_distinct(basis, ends)
    }

    fn direction_angle(&self, from: [f64; 2], to: [f64; 2]) -> f64 {
        board_drill::direction_angle(from, to)
    }

    fn hole_reaches_loops(&self, center: [f64; 2], size: [f64; 2], angle: f64, loops: &[Vec<[f64; 2]>]) -> bool {
        board_drill::hole_reaches_loops(center, size, angle, loops)
    }

    fn side_on_hole_wall(&self, center: [f64; 2], size: [f64; 2], angle: f64, points: &[[f64; 2]], chord: f64) -> bool {
        board_drill::side_on_hole_wall(center, size, angle, points, chord)
    }

    fn path_runs_inside(&self, path: &[[f64; 2]], regions: &[Vec<Vec<[f64; 2]>>], margin: f64) -> bool {
        footprint::path_runs_inside(path, regions, margin)
    }

    fn circle_points(&self, c: Vec3, x: Vec3, y: Vec3, radius: f64, from: f64) -> [Vec3; 3] {
        constraint::circle_points(c, x, y, radius, from)
    }

    fn arc_circle_points(&self, c: [f64; 2], start: [f64; 2], plane: &Plane) -> [Vec3; 3] {
        constraint::arc_circle_points(c, start, plane)
    }

    fn same_plane(&self, a: &Plane, b: &Plane) -> bool {
        constraint::same_plane(a, b)
    }

    fn plane_normal(&self, plane: &Plane) -> Vec3 {
        constraint::plane_normal(plane)
    }

    fn plane_point(&self, plane: &Plane, point: Vec2) -> Vec3 {
        constraint::plane_point(plane, point)
    }

    fn translate_point(&self, point: Vec2, displacement: Vec2, fraction: f64) -> Vec2 {
        prism::translate_point(point, displacement, fraction)
    }

    fn chord(&self, a: Vec2, b: Vec2) -> (Vec2, f64) {
        prism::chord(a, b)
    }

    fn chord_point(&self, a: Vec2, b: Vec2, fraction: f64) -> Vec2 {
        prism::chord_point(a, b, fraction)
    }

    fn left_unit_normal(&self, a: Vec2, b: Vec2, minimum_length: f64) -> Option<Vec2> {
        prism::left_unit_normal(a, b, minimum_length)
    }

    fn tangent_guide_tip(
        &self,
        at: Vec2,
        tangent: Vec2,
        min_length: f64,
        angle_tolerance: f64,
        fallback_lengths: &[f64],
        coord_limit: f64,
    ) -> Option<Vec2> {
        prism::tangent_guide_tip(at, tangent, min_length, angle_tolerance, fallback_lengths, coord_limit)
    }

    fn lengthen_direction(&self, direction: Vec2, min_length: f64) -> Vec2 {
        prism::lengthen_direction(direction, min_length)
    }

    fn stored_arc(&self, center: Vec2, start: Vec2, end: Vec2, reversed: bool) -> Seg2 {
        prism::stored_arc(center, start, end, reversed)
    }

    fn arc_contacts_match(&self, expected: &Seg2, stored: &Seg2, angle_tolerance: f64) -> bool {
        prism::arc_contacts_match(expected, stored, angle_tolerance)
    }

    fn points_agree(&self, actual: Vec2, expected: Vec2, absolute_tolerance: f64) -> bool {
        prism::points_agree(actual, expected, absolute_tolerance)
    }

    fn circle_curvature(&self, center: Vec2, point: Vec2, radius: f64) -> Vec2 {
        prism::circle_curvature(center, point, radius)
    }

    fn ray_chord_parameter(&self, plane: &Plane, origin: Vec3, direction: Vec3, endpoints: [Vec2; 2]) -> Option<f64> {
        prism::ray_chord_parameter(plane, origin, direction, endpoints)
    }
}
