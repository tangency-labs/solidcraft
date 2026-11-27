//! Placement maths ([`Affine`]): affine maps as row-vector matrices ([`Matrix`]: rows 0-2 the
//! images of x, y and z, row 3 the translation, `p' = p·A + t`), points, axes and lines carried
//! through them, how far points and boxes reach along a direction, parallelograms on a plane,
//! and the planar angle and curve helpers placement reads. Every value is plain numbers or the
//! record's (`solidcore_record`), so the application holds no kernel type.

use solidcore_record::{Loop2, Seg2, Vec2};

use crate::Backend;

/// An affine map in the row-vector convention: rows 0-2 the images of x, y and z, row 3 the
/// translation, the fourth column `(0, 0, 0, 1)`.
pub type Matrix = [[f64; 4]; 4];

/// A quad offset from its own plane on both sides: its unit normal, its corners moved along the
/// normal and its corners moved against it ([`Affine::offset_quad`]).
pub type OffsetQuad = ([f64; 3], [[f64; 3]; 4], [[f64; 3]; 4]);

/// Numbers a placement call refuses, in the kernel's words: a number not finite, an axis of no
/// length, a map that is not regular.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct AffineRefused(pub String);

/// Why an axis query on a placed body ([`Affine::extent_along`], [`Affine::along_own_axes`],
/// [`Affine::square_plane_offset`]) is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisRefusal {
    /// A number is not finite.
    NotFinite,
    /// The axis is zero.
    ZeroAxis,
    /// The placement flattens space.
    FlatMap,
}

/// A planar face square to a direction ([`crate::OpKind::Affine`]): its level and tolerance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SquareFaceLevel {
    /// The signed distance from the origin to the face's plane, along the direction.
    pub level: f64,
    /// The face's own tolerance: levels closer than this are the same level on the face.
    pub tolerance: f64,
}

/// The placement maths of one kernel ([`crate::OpKind::Affine`]).
pub trait Affine: Backend {
    /// The turn by `angle` radians, right-handed about `axis` (any length but none), about the
    /// line through `point` along it.
    fn rotation(&self, point: [f64; 3], axis: [f64; 3], angle: f64) -> Result<Matrix, AffineRefused>;
    /// `first`, then `second`: steps stacked into one map.
    fn then(&self, first: &Matrix, second: &Matrix) -> Result<Matrix, AffineRefused>;
    /// The map that undoes `matrix`.
    fn inverse(&self, matrix: &Matrix) -> Result<Matrix, AffineRefused>;
    /// `point` under `matrix`.
    fn apply_point(&self, matrix: &Matrix, point: [f64; 3]) -> [f64; 3];
    /// Whether `matrix` is a rotation or reflection and a translation, with the axis and
    /// homogeneous-entry tolerances given. A map not finite, or a tolerance not valid, is not.
    fn is_rigid(&self, matrix: &Matrix, axis_tol: f64, affine_tol: f64) -> bool;
    /// The change from placement `seen` to placement `now` in the orthonormal plane frame
    /// `origin`, `x`, `y`, as an in-plane 2 × 2 map and a move. `None` for an unchanged map, a
    /// singular `seen`, a tilt or an in-plane reflection.
    fn projection_motion(&self, seen: &Matrix, now: &Matrix, origin: [f64; 3], x: [f64; 3], y: [f64; 3]) -> Option<([[f64; 2]; 2], [f64; 2])>;
    /// The orthonormal axes along `x` and in the plane of `x` and `y`, on `y`'s side (a direction
    /// the kernel chooses when `y` runs along `x` or has no length).
    fn orthonormal_axes(&self, x: [f64; 3], y: [f64; 3]) -> Result<[[f64; 3]; 2], AffineRefused>;
    /// The axes `x` and `y` of a frame whose normal runs along `d`: `y` along `v`'s part square
    /// to `d` and `x = y × unit(d)`, both unit. Refused for numbers not finite, no length, or a `v`
    /// whose part square to `d` is within its own rounding.
    fn frame_axes(&self, d: [f64; 3], v: [f64; 3]) -> Result<[[f64; 3]; 2], AffineRefused>;
    /// A plane's axes with the coordinates smaller than `noise` dropped, each unit again. `None`
    /// when nothing sound is left; the caller keeps its own axes.
    fn axis_squared_axes(&self, x: [f64; 3], y: [f64; 3], noise: f64) -> Option<[[f64; 3]; 2]>;
    /// The signed angle about unit `normal` from unit `start` to unit `end`, in radians (a half
    /// turn is positive).
    fn signed_unit_angle(&self, normal: [f64; 3], start: [f64; 3], end: [f64; 3]) -> Result<f64, AffineRefused>;
    /// The line through `origin` along `direction`, carried by `frame`.
    fn line_in_frame(&self, frame: &Matrix, origin: [f64; 3], direction: [f64; 3]) -> Result<([f64; 3], [f64; 3]), AffineRefused>;
    /// An undirected axis oriented by its largest component.
    fn canonical_axis_direction(&self, direction: [f64; 3]) -> [f64; 3];
    /// `direction` as a unit axis direction, oriented by its largest component; `None` shorter
    /// than `min_length` or not finite.
    fn unit_axis_direction(&self, direction: [f64; 3], min_length: f64) -> Option<[f64; 3]>;
    /// A definition's axis carried into world coordinates by `definition_to_world`, oriented
    /// there, and expressed in the authoring frame by `world_to_author`.
    fn placed_axis(
        &self,
        definition_to_world: &Matrix,
        world_to_author: &Matrix,
        origin: [f64; 3],
        direction: [f64; 3],
    ) -> Result<([f64; 3], [f64; 3]), AffineRefused>;
    /// The dot product of `point - origin` with `direction`: the distance from `origin` along a
    /// unit `direction` (a longer one scales it by its length); negative behind `origin`.
    fn distance_along(&self, origin: [f64; 3], direction: [f64; 3], point: [f64; 3]) -> f64;
    /// How far the box from `min` to `max` reaches from `origin` along `direction`, `[low,
    /// high]`.
    fn span_along(&self, min: [f64; 3], max: [f64; 3], origin: [f64; 3], direction: [f64; 3]) -> Result<[f64; 2], AffineRefused>;
    /// How far the turn in `matrix` (a translation in it ignored) moves `pivot`: the move to add
    /// to the turn about the origin to make it the turn about `pivot`.
    fn pivot_displacement(&self, matrix: &Matrix, pivot: [f64; 3]) -> Result<[f64; 3], AffineRefused>;
    /// `direction`, reversed when it points more than a right angle away from `like`. `None`
    /// when either has no length or a number not finite.
    fn same_sense(&self, direction: [f64; 3], like: [f64; 3]) -> Option<[f64; 3]>;
    /// The corners (bottom-left, bottom-right, top-right, top-left) of a parallelogram on a plane:
    /// its bottom edge from `bl` to `br`, its top-left corner `top_left` when it may stretch, else
    /// `aspect` (height over width) times as high as wide. `None` for no area.
    fn rect_of_aspect(&self, bl: [f64; 2], br: [f64; 2], top_left: Option<[f64; 2]>, aspect: f64) -> Option<[[f64; 2]; 4]>;
    /// How a rectangle `aspect` times as high as wide fits a frame pinned at `bl`, `br` and `tl`:
    /// its width, and whether the frame stretches it. `None` for no area.
    fn fit_aspect(&self, bl: [f64; 3], br: [f64; 3], tl: [f64; 3], aspect: f64) -> Option<(f64, bool)>;
    /// The quad `corners` offset `lift` from its own plane on both sides. `None` for numbers not
    /// finite or no area.
    fn offset_quad(&self, corners: [[f64; 3]; 4], lift: f64) -> Option<OffsetQuad>;
    /// The point `s` along edge `u` and `t` along edge `v` of the parallelogram at `corner`.
    fn parallelogram_point(&self, corner: [f64; 3], u: [f64; 3], v: [f64; 3], s: f64, t: f64) -> [f64; 3];
    /// A planar point in units of the axis-aligned chord from `leading_x` to `trailing_x`, its
    /// nose at x = 0.
    fn chord_coordinates(&self, point: Vec2, leading_x: f64, trailing_x: f64) -> Result<Vec2, AffineRefused>;
    /// The counter-clockwise angle from ray `start` to ray `end` about `center`, in radians
    /// modulo one turn. `None` for a ray not finite or of no length.
    fn counterclockwise_sweep(&self, center: Vec2, start: Vec2, end: Vec2) -> Option<f64>;
    /// Whether the cubic `piece` is a trimmed interval of the cubic `source`, either way round.
    /// Other segment kinds are not.
    fn cubic_contains(&self, source: &Seg2, piece: &Seg2) -> bool;
    /// How far the box from `min` to `max` (a body's own box) reaches along `axis` (a direction
    /// of the outer frame, any length) once the body is placed by `placement`. Tight when `axis`
    /// runs along one of the body's own axes.
    fn extent_along(&self, min: [f64; 3], max: [f64; 3], placement: &Matrix, axis: [f64; 3]) -> Result<f64, AxisRefusal>;
    /// Whether `axis`, a direction of the outer frame, runs along one of the own axes of a body
    /// placed by `placement`.
    fn along_own_axes(&self, placement: &Matrix, axis: [f64; 3]) -> Result<bool, AxisRefusal>;
    /// Where a flat face (through `origin`, normal `normal`, in a body's own frame) of a body placed
    /// by `placement` cuts the unit `axis`, as its offset from the outer origin; `None` when it is
    /// not square to `axis`.
    fn square_plane_offset(&self, placement: &Matrix, origin: [f64; 3], normal: [f64; 3], axis: [f64; 3]) -> Result<Option<f64>, AxisRefusal>;
    /// The circle of a loop of arcs making one directed turn on one stored support: its centre
    /// and radius.
    fn loop_circle(&self, lp: &Loop2) -> Option<(Vec2, f64)>;
    /// Whether `lp` holds `p` by the even-odd rule, decided exactly: `None` when `p` is on the
    /// loop or within rounding of it.
    fn loop_holds(&self, lp: &Loop2, p: Vec2) -> Option<bool>;
    /// How far `p` is from `seg`, exactly, for a line or a circular arc: the foot clamped to a
    /// line's ends; on an arc the radial distance where `p` is within its angles, else the
    /// distance to the nearer end. `None` for a free-form segment or a point that is no number.
    fn segment_distance(&self, seg: &Seg2, p: Vec2) -> Option<f64>;
    /// Whether the circles `a` and `b` (centre and radius) are one hole: their centres and their
    /// radii each less than `tol` apart.
    fn same_circle(&self, a: (Vec2, f64), b: (Vec2, f64), tol: f64) -> bool;
    /// Where a note `height` high stands to name the circle `at` (centre and radius), level with
    /// its centre and clear of its rim on the right: its lower left corner.
    fn note_beside(&self, at: (Vec2, f64), height: f64) -> Vec2;
}
