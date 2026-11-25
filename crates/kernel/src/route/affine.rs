//! The router's placement maths ([`solidcraft_kernel_api::Affine`]), as `solidcraft_kernel::affine`:
//! affine maps as row-vector matrices, points, axes and lines through them, reaches along a
//! direction, parallelograms on a plane, and the planar angle and curve helpers placement reads.

use solidcore_record::{Loop2, Seg2, Vec2};
use solidcraft_kernel_api::{Affine, OpKind};

pub use solidcraft_kernel_api::affine::{AffineRefused, Matrix, OffsetQuad};

use super::{Which, which};
use crate::backend::Solidcore;

/// The turn by `angle` radians, right-handed about `axis` (any length but none), about the
/// line through `point` along it.
pub fn rotation(point: [f64; 3], axis: [f64; 3], angle: f64) -> Result<Matrix, AffineRefused> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.rotation(point, axis, angle),
    }
}

/// `first`, then `second`: steps stacked into one map.
pub fn then(first: &Matrix, second: &Matrix) -> Result<Matrix, AffineRefused> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.then(first, second),
    }
}

/// The map that undoes `matrix`.
pub fn inverse(matrix: &Matrix) -> Result<Matrix, AffineRefused> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.inverse(matrix),
    }
}

/// `point` under `matrix`.
pub fn apply_point(matrix: &Matrix, point: [f64; 3]) -> [f64; 3] {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.apply_point(matrix, point),
    }
}

/// Whether `matrix` is a rotation or reflection and a translation, with the axis and
/// homogeneous-entry tolerances given. A map not finite, or a tolerance not valid, is not.
pub fn is_rigid(matrix: &Matrix, axis_tol: f64, affine_tol: f64) -> bool {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.is_rigid(matrix, axis_tol, affine_tol),
    }
}

/// The change from placement `seen` to placement `now` in the orthonormal plane frame
/// `origin`, `x`, `y`, as an in-plane 2 × 2 map and a move. `None` for an unchanged map, a
/// singular `seen`, a tilt or an in-plane reflection.
pub fn projection_motion(seen: &Matrix, now: &Matrix, origin: [f64; 3], x: [f64; 3], y: [f64; 3]) -> Option<([[f64; 2]; 2], [f64; 2])> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.projection_motion(seen, now, origin, x, y),
    }
}

/// The orthonormal axes along `x` and in the plane of `x` and `y`, on `y`'s side (a direction
/// the kernel chooses when `y` runs along `x` or has no length).
pub fn orthonormal_axes(x: [f64; 3], y: [f64; 3]) -> Result<[[f64; 3]; 2], AffineRefused> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.orthonormal_axes(x, y),
    }
}

/// A plane's axes with the coordinates smaller than `noise` dropped, each unit again. `None`
/// when nothing sound is left; the caller keeps its own axes.
pub fn axis_squared_axes(x: [f64; 3], y: [f64; 3], noise: f64) -> Option<[[f64; 3]; 2]> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.axis_squared_axes(x, y, noise),
    }
}

/// The signed angle about unit `normal` from unit `start` to unit `end`, in radians (a half
/// turn is positive).
pub fn signed_unit_angle(normal: [f64; 3], start: [f64; 3], end: [f64; 3]) -> Result<f64, AffineRefused> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.signed_unit_angle(normal, start, end),
    }
}

/// The line through `origin` along `direction`, carried by `frame`.
pub fn line_in_frame(frame: &Matrix, origin: [f64; 3], direction: [f64; 3]) -> Result<([f64; 3], [f64; 3]), AffineRefused> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.line_in_frame(frame, origin, direction),
    }
}

/// An undirected axis oriented by its largest component.
pub fn canonical_axis_direction(direction: [f64; 3]) -> [f64; 3] {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.canonical_axis_direction(direction),
    }
}

/// `direction` as a unit axis direction, oriented by its largest component; `None` shorter
/// than `min_length` or not finite.
pub fn unit_axis_direction(direction: [f64; 3], min_length: f64) -> Option<[f64; 3]> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.unit_axis_direction(direction, min_length),
    }
}

/// A definition's axis carried into world coordinates by `definition_to_world`, oriented
/// there, and expressed in the authoring frame by `world_to_author`.
pub fn placed_axis(
    definition_to_world: &Matrix,
    world_to_author: &Matrix,
    origin: [f64; 3],
    direction: [f64; 3],
) -> Result<([f64; 3], [f64; 3]), AffineRefused> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.placed_axis(definition_to_world, world_to_author, origin, direction),
    }
}

/// How far `point` stands from `origin` along `direction`, in lengths of `direction`;
/// negative behind `origin`.
pub fn distance_along(origin: [f64; 3], direction: [f64; 3], point: [f64; 3]) -> f64 {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.distance_along(origin, direction, point),
    }
}

/// How far the box from `min` to `max` reaches from `origin` along `direction`, `[low,
/// high]`.
pub fn span_along(min: [f64; 3], max: [f64; 3], origin: [f64; 3], direction: [f64; 3]) -> Result<[f64; 2], AffineRefused> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.span_along(min, max, origin, direction),
    }
}

/// How far the turn in `matrix` (a translation in it ignored) moves `pivot`: the move to add
/// to the turn about the origin to make it the turn about `pivot`.
pub fn pivot_displacement(matrix: &Matrix, pivot: [f64; 3]) -> Result<[f64; 3], AffineRefused> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.pivot_displacement(matrix, pivot),
    }
}

/// `direction`, reversed when it points more than a right angle away from `like`. `None`
/// when either has no length or a number not finite.
pub fn same_sense(direction: [f64; 3], like: [f64; 3]) -> Option<[f64; 3]> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.same_sense(direction, like),
    }
}

/// The corners (bottom-left, bottom-right, top-right, top-left) of a parallelogram on a plane:
/// its bottom edge from `bl` to `br`, its top-left corner `top_left` when it may stretch, else
/// `aspect` (height over width) times as high as wide. `None` for no area.
pub fn rect_of_aspect(bl: [f64; 2], br: [f64; 2], top_left: Option<[f64; 2]>, aspect: f64) -> Option<[[f64; 2]; 4]> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.rect_of_aspect(bl, br, top_left, aspect),
    }
}

/// How a rectangle `aspect` times as high as wide fits a frame pinned at `bl`, `br` and `tl`:
/// its width, and whether the frame stretches it. `None` for no area.
pub fn fit_aspect(bl: [f64; 3], br: [f64; 3], tl: [f64; 3], aspect: f64) -> Option<(f64, bool)> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.fit_aspect(bl, br, tl, aspect),
    }
}

/// The quad `corners` offset `lift` from its own plane on both sides. `None` for numbers not
/// finite or no area.
pub fn offset_quad(corners: [[f64; 3]; 4], lift: f64) -> Option<OffsetQuad> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.offset_quad(corners, lift),
    }
}

/// The point `s` along edge `u` and `t` along edge `v` of the parallelogram at `corner`.
pub fn parallelogram_point(corner: [f64; 3], u: [f64; 3], v: [f64; 3], s: f64, t: f64) -> [f64; 3] {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.parallelogram_point(corner, u, v, s, t),
    }
}

/// A planar point in units of the axis-aligned chord from `leading_x` to `trailing_x`, its
/// nose at x = 0.
pub fn chord_coordinates(point: Vec2, leading_x: f64, trailing_x: f64) -> Result<Vec2, AffineRefused> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.chord_coordinates(point, leading_x, trailing_x),
    }
}

/// The counter-clockwise angle from ray `start` to ray `end` about `center`, in radians
/// modulo one turn. `None` for a ray not finite or of no length.
pub fn counterclockwise_sweep(center: Vec2, start: Vec2, end: Vec2) -> Option<f64> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.counterclockwise_sweep(center, start, end),
    }
}

/// Whether the cubic `piece` is a trimmed interval of the cubic `source`, either way round.
/// Other segment kinds are not.
pub fn cubic_contains(source: &Seg2, piece: &Seg2) -> bool {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.cubic_contains(source, piece),
    }
}

/// The circle of a loop of arcs making one directed turn on one stored support: its centre
/// and radius.
pub fn loop_circle(lp: &Loop2) -> Option<(Vec2, f64)> {
    match which(OpKind::Affine) {
        Which::Solidcore => Solidcore.loop_circle(lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::{affine as ops, prism};

    #[test]
    fn routed_placement_maths_is_solidcores() {
        let turn = rotation([1.0, 2.0, 3.0], [0.0, 0.0, 2.0], 0.7).unwrap();
        assert_eq!(turn, ops::rotation([1.0, 2.0, 3.0], [0.0, 0.0, 2.0], 0.7).unwrap());
        let back = inverse(&turn).unwrap();
        assert_eq!(back, ops::inverse(&turn).unwrap());
        assert_eq!(then(&turn, &back).unwrap(), ops::then(&turn, &back).unwrap());
        assert_eq!(apply_point(&turn, [4.0, 5.0, 6.0]), ops::apply_point(&turn, [4.0, 5.0, 6.0]));
        assert!(is_rigid(&turn, 1e-6, 1e-12));
        assert_eq!(pivot_displacement(&turn, [1.0, 2.0, 3.0]).unwrap(), ops::pivot_displacement(&turn, [1.0, 2.0, 3.0]).unwrap());
        assert_eq!(distance_along([0.0; 3], [0.0, 0.0, 2.0], [1.0, 1.0, 3.0]), 6.0);
        assert_eq!(span_along([0.0; 3], [1.0, 2.0, 3.0], [0.0; 3], [0.0, 0.0, 1.0]).unwrap(), [0.0, 3.0]);
        let (c, s, e) = (Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0));
        assert_eq!(counterclockwise_sweep(c, s, e), prism::counterclockwise_sweep(c, s, e));
        // a refusal carries solidcore's words whole
        let words = ops::rotation([0.0; 3], [0.0; 3], 1.0).unwrap_err().to_string();
        assert_eq!(rotation([0.0; 3], [0.0; 3], 1.0), Err(AffineRefused(words)));
        let words = ops::chord_coordinates(Vec2::new(0.5, 0.0), 1.0, 1.0).unwrap_err().to_string();
        assert_eq!(chord_coordinates(Vec2::new(0.5, 0.0), 1.0, 1.0), Err(AffineRefused(words)));
    }
}
