//! solidcore's placement maths ([`Affine`]): each the solidcore-ops call this crate's callers
//! made before the interface existed, its refusal's words kept whole.

use solidcore_ops::{affine as ops, prism};
use solidcore_record::{Loop2, Seg2, Vec2};
use solidcraft_kernel_api::{Affine, AffineRefused, AxisRefusal, Matrix, OffsetQuad};

use super::Solidcore;

/// solidcore's refusal, in its words.
fn refused(e: solidcore_ops::Error) -> AffineRefused {
    AffineRefused(e.to_string())
}

/// solidcore's refusal of an axis query, by the reason it states. Those queries refuse only with
/// these three reasons; any other refusal is the numbers' (not finite).
fn axis_refused(e: solidcore_ops::Error) -> AxisRefusal {
    match e {
        solidcore_ops::Error::Refused { reason: ops::REFUSED_ZERO_AXIS, .. } => AxisRefusal::ZeroAxis,
        solidcore_ops::Error::Refused { reason: ops::REFUSED_FLAT_MAP, .. } => AxisRefusal::FlatMap,
        _ => AxisRefusal::NotFinite,
    }
}

impl Affine for Solidcore {
    fn linear_determinant(&self, matrix: &Matrix) -> f64 {
        ops::linear_determinant(matrix)
    }

    fn directed_unit(&self, axis: [f64; 3]) -> Option<[f64; 3]> {
        ops::directed_unit(axis)
    }

    fn translated_point(&self, point: [f64; 3], by: [f64; 3]) -> [f64; 3] {
        ops::translated_point(point, by)
    }

    fn compose_turn(&self, first: [f64; 3], radians: f64, world: [f64; 3], turn: f64) -> Option<([f64; 3], f64)> {
        ops::compose_turn(first, radians, world, turn)
    }

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

    fn frame_axes(&self, d: [f64; 3], v: [f64; 3]) -> Result<[[f64; 3]; 2], AffineRefused> {
        ops::frame_axes(d, v).map_err(refused)
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

    fn plane_onto_line(&self, origin: [f64; 3], x: [f64; 3], y: [f64; 3], at: [f64; 3], dir: [f64; 3]) -> Option<([f64; 3], [[f64; 3]; 2])> {
        ops::plane_onto_line(origin, x, y, at, dir)
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

    fn loop_holds(&self, lp: &Loop2, p: Vec2) -> Option<bool> {
        prism::loop_holds(lp, p)
    }

    fn segment_distance(&self, seg: &Seg2, p: Vec2) -> Option<f64> {
        prism::segment_distance(seg, p)
    }

    fn same_circle(&self, a: (Vec2, f64), b: (Vec2, f64), tol: f64) -> bool {
        prism::same_circle(a, b, tol)
    }

    fn note_beside(&self, at: (Vec2, f64), height: f64) -> Vec2 {
        prism::note_beside(at, height)
    }

    fn extent_along(&self, min: [f64; 3], max: [f64; 3], placement: &Matrix, axis: [f64; 3]) -> Result<f64, AxisRefusal> {
        ops::extent_along(min, max, placement, axis).map_err(axis_refused)
    }

    fn along_own_axes(&self, placement: &Matrix, axis: [f64; 3]) -> Result<bool, AxisRefusal> {
        ops::along_own_axes(placement, axis).map_err(axis_refused)
    }

    fn square_plane_offset(&self, placement: &Matrix, origin: [f64; 3], normal: [f64; 3], axis: [f64; 3]) -> Result<Option<f64>, AxisRefusal> {
        ops::square_plane_offset(placement, origin, normal, axis).map_err(axis_refused)
    }
}
