//! solidcore's planar helpers ([`Planar`]): each the solidcore-ops call this crate's callers made
//! before the interface existed, its refusal's words kept whole.

use solidcore_ops::{joint_turn, paper_gaps, section_corners, sketch_arc};
use solidcore_record::{Loop2, Plane, Vec3};
use solidcraft_kernel_api::planar::{Planar, PlanarRefused};

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

    fn circumcentre(&self, a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 2]> {
        sketch_arc::circumcentre(a, b, c)
    }

    fn three_point_circle(&self, s: [f64; 2], e: [f64; 2], t: [f64; 2]) -> Option<([f64; 2], f64, f64)> {
        sketch_arc::three_point_circle(s, e, t)
    }

    fn arc_third_point(&self, s: [f64; 2], e: [f64; 2], h: [f64; 2], radius: Option<f64>, sweep: Option<f64>) -> Option<[f64; 2]> {
        sketch_arc::arc_third_point(s, e, h, radius, sweep)
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

    fn straight_length(&self, a: [f64; 2], b: [f64; 2]) -> f64 {
        paper_gaps::straight_length(a, b)
    }

    fn projected_endpoints_are_distinct(&self, basis: Plane, ends: [Vec3; 2]) -> bool {
        paper_gaps::projected_endpoints_are_distinct(basis, ends)
    }
}
