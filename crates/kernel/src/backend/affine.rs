//! solidcore's placement maths ([`Affine`]): each the solidcore-ops call this crate's callers
//! made before the interface existed, its refusal's words kept whole.

use solidcore_ops::{affine as ops, prism};
use solidcore_record::{Loop2, Seg2, Vec2};
use solidcraft_kernel_api::{Affine, AffineRefused, Matrix, OffsetQuad};

use super::Solidcore;

/// solidcore's refusal, in its words.
fn refused(e: solidcore_ops::Error) -> AffineRefused {
    AffineRefused(e.to_string())
}

impl Affine for Solidcore {
    fn rotation(&self, point: [f64; 3], axis: [f64; 3], angle: f64) -> Result<Matrix, AffineRefused> {
        ops::rotation(point, axis, angle).map_err(refused)
    }

    fn then(&self, first: &Matrix, second: &Matrix) -> Result<Matrix, AffineRefused> {
        ops::then(first, second).map_err(refused)
    }

    fn inverse(&self, matrix: &Matrix) -> Result<Matrix, AffineRefused> {
        ops::inverse(matrix).map_err(refused)
    }

    fn apply_point(&self, matrix: &Matrix, point: [f64; 3]) -> [f64; 3] {
        ops::apply_point(matrix, point)
    }

    fn is_rigid(&self, matrix: &Matrix, axis_tol: f64, affine_tol: f64) -> bool {
        ops::is_rigid(matrix, axis_tol, affine_tol)
    }

    fn projection_motion(&self, seen: &Matrix, now: &Matrix, origin: [f64; 3], x: [f64; 3], y: [f64; 3]) -> Option<([[f64; 2]; 2], [f64; 2])> {
        ops::projection_motion(seen, now, origin, x, y)
    }

    fn orthonormal_axes(&self, x: [f64; 3], y: [f64; 3]) -> Result<[[f64; 3]; 2], AffineRefused> {
        ops::orthonormal_axes(x, y).map_err(refused)
    }

    fn axis_squared_axes(&self, x: [f64; 3], y: [f64; 3], noise: f64) -> Option<[[f64; 3]; 2]> {
        ops::axis_squared_axes(x, y, noise)
    }

    fn signed_unit_angle(&self, normal: [f64; 3], start: [f64; 3], end: [f64; 3]) -> Result<f64, AffineRefused> {
        ops::signed_unit_angle(normal, start, end).map_err(refused)
    }

    fn line_in_frame(&self, frame: &Matrix, origin: [f64; 3], direction: [f64; 3]) -> Result<([f64; 3], [f64; 3]), AffineRefused> {
        ops::line_in_frame(frame, origin, direction).map_err(refused)
    }

    fn canonical_axis_direction(&self, direction: [f64; 3]) -> [f64; 3] {
        ops::canonical_axis_direction(direction)
    }

    fn unit_axis_direction(&self, direction: [f64; 3], min_length: f64) -> Option<[f64; 3]> {
        ops::unit_axis_direction(direction, min_length)
    }

    fn placed_axis(
        &self,
        definition_to_world: &Matrix,
        world_to_author: &Matrix,
        origin: [f64; 3],
        direction: [f64; 3],
    ) -> Result<([f64; 3], [f64; 3]), AffineRefused> {
        ops::placed_axis(definition_to_world, world_to_author, origin, direction).map_err(refused)
    }

    fn distance_along(&self, origin: [f64; 3], direction: [f64; 3], point: [f64; 3]) -> f64 {
        ops::distance_along(origin, direction, point)
    }

    fn span_along(&self, min: [f64; 3], max: [f64; 3], origin: [f64; 3], direction: [f64; 3]) -> Result<[f64; 2], AffineRefused> {
        ops::span_along(min, max, origin, direction).map_err(refused)
    }

    fn pivot_displacement(&self, matrix: &Matrix, pivot: [f64; 3]) -> Result<[f64; 3], AffineRefused> {
        ops::pivot_displacement(matrix, pivot).map_err(refused)
    }

    fn same_sense(&self, direction: [f64; 3], like: [f64; 3]) -> Option<[f64; 3]> {
        ops::same_sense(direction, like)
    }

    fn rect_of_aspect(&self, bl: [f64; 2], br: [f64; 2], top_left: Option<[f64; 2]>, aspect: f64) -> Option<[[f64; 2]; 4]> {
        ops::rect_of_aspect(bl, br, top_left, aspect)
    }

    fn fit_aspect(&self, bl: [f64; 3], br: [f64; 3], tl: [f64; 3], aspect: f64) -> Option<(f64, bool)> {
        ops::fit_aspect(bl, br, tl, aspect)
    }

    fn offset_quad(&self, corners: [[f64; 3]; 4], lift: f64) -> Option<OffsetQuad> {
        ops::offset_quad(corners, lift)
    }

    fn parallelogram_point(&self, corner: [f64; 3], u: [f64; 3], v: [f64; 3], s: f64, t: f64) -> [f64; 3] {
        ops::parallelogram_point(corner, u, v, s, t)
    }

    fn chord_coordinates(&self, point: Vec2, leading_x: f64, trailing_x: f64) -> Result<Vec2, AffineRefused> {
        ops::chord_coordinates(point, leading_x, trailing_x).map_err(refused)
    }

    fn counterclockwise_sweep(&self, center: Vec2, start: Vec2, end: Vec2) -> Option<f64> {
        prism::counterclockwise_sweep(center, start, end)
    }

    fn cubic_contains(&self, source: &Seg2, piece: &Seg2) -> bool {
        prism::cubic_contains(source, piece)
    }

    fn loop_circle(&self, lp: &Loop2) -> Option<(Vec2, f64)> {
        prism::loop_circle(lp)
    }
}
