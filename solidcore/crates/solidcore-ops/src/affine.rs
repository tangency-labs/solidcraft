//! Affine maps from an application's numbers, as the matrices [`crate::Body::transform`] and
//! [`crate::curve::transform`] take: rows 0-2 the images of x, y and z, row 3 the translation
//! (`p' = p·A + t`), the fourth column `(0, 0, 0, 1)`. A move, a turn about a line, a scale
//! about a point, a mirror in a plane, one map after another, and the map that undoes one. The maps are the kernel's
//! ([`solidcore::Transform::translation`], [`solidcore::Transform::rotation`],
//! [`solidcore::Transform::uniform_scaling`], [`solidcore::Transform::reflection`],
//! [`solidcore::Transform::concat`], [`solidcore::Transform::inverse`]); ops checks the
//! numbers and passes them on. Beside them, how far a point or a box reaches along a direction
//! ([`distance_along`], [`span_along`]), as the kernel measures it, a placed body's box along a
//! direction ([`extent_along`], [`along_own_axes`]), a placed flat face's offset
//! ([`square_plane_offset`]), a point under a map ([`apply_point`]), the quads a pinned picture is
//! drawn on ([`picture_frame`], [`lifted_quad`], [`parallelogram_point`]), and a direction turned
//! to the sense of another ([`same_sense`]).

use solidcore::geom::certified::Verdict;
use solidcore::geom::extent3d::Extent3d;
use solidcore::math::AxisRefusal;
use solidcore::{Transform, Vec3};

use crate::body::REGULAR_RATIO;
use crate::{Error, Result};

/// An affine map in the row-vector convention of [`crate::Body::transform`].
pub type Matrix = [[f64; 4]; 4];

/// Whether every number of `v` is finite.
fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// `v` as the kernel's vector.
fn vec3([x, y, z]: [f64; 3]) -> Vec3 {
    Vec3::new(x, y, z)
}

/// The kernel's map, or [`Error::Argument`] for the numbers it refused.
fn taken(map: solidcore::Result<Transform>) -> Result<Matrix> {
    map.map(|t| t.m).map_err(|_| Error::Argument)
}

/// The move by `by`.
///
/// # Errors
/// [`Error::Argument`] for a number not finite.
pub fn translation(by: [f64; 3]) -> Result<Matrix> {
    if !finite(&by) {
        return Err(Error::Argument);
    }
    Ok(Transform::translation(vec3(by)).m)
}

/// The turn by `angle` radians, right-handed about `axis`, about the line through `point`
/// along `axis` (any length but none).
///
/// # Errors
/// [`Error::Argument`] for a point, axis or angle not finite, or an axis of no length.
pub fn rotation(point: [f64; 3], axis: [f64; 3], angle: f64) -> Result<Matrix> {
    taken(Transform::rotation(vec3(point), vec3(axis), angle))
}

/// The orthonormal axes along `x` and in the plane of `x` and `y` (on `y`'s side),
/// using the kernel's [`Transform::orthonormal_frame`]. If `y` is parallel to `x` or has
/// no length, the kernel chooses a perpendicular direction. This restores a unit frame after
/// carrying its axes through a rigid motion.
///
/// # Errors
/// [`Error::Argument`] for non-finite axes or an axis the kernel refuses.
pub fn orthonormal_axes(x: [f64; 3], y: [f64; 3]) -> Result<[[f64; 3]; 2]> {
    if !finite(&x) || !finite(&y) {
        return Err(Error::Argument);
    }
    let [x, y] = Transform::orthonormal_frame(vec3(x), vec3(y)).map_err(|_| Error::Argument)?;
    Ok([[x.x, x.y, x.z], [y.x, y.y, y.z]])
}

/// Carry planar axes by a rigid `matrix`, keeping UV scales and keeping square source
/// axes square, through [`Transform::rigid_plane_axes`]. The caller establishes that the
/// matrix is a rigid motion; skew source axes retain their parametrisation.
///
/// # Errors
/// [`Error::Argument`] for axes or a map the kernel refuses.
pub fn rigid_plane_axes(matrix: &Matrix, u: [f64; 3], v: [f64; 3]) -> Result<[[f64; 3]; 2]> {
    let [u, v] = Transform { m: *matrix }
        .rigid_plane_axes(vec3(u), vec3(v))
        .map_err(|_| Error::Argument)?;
    Ok([[u.x, u.y, u.z], [v.x, v.y, v.z]])
}

/// Carry a plane given by its origin and two points through a rigid motion, as an origin
/// and axes, through [`Transform::rigid_plane_through`]. The caller establishes a rigid map.
///
/// # Errors
/// [`Error::Argument`] for coordinates or a map the kernel refuses.
pub fn rigid_plane_through(
    matrix: &Matrix,
    origin: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
) -> Result<([f64; 3], [[f64; 3]; 2])> {
    let (o, [u, v]) = Transform { m: *matrix }
        .rigid_plane_through(vec3(origin), vec3(u), vec3(v))
        .map_err(|_| Error::Argument)?;
    Ok(([o.x, o.y, o.z], [[u.x, u.y, u.z], [v.x, v.y, v.z]]))
}

/// The scale by `factor` in every direction about `centre`, which it keeps.
///
/// # Errors
/// [`Error::Argument`] for a centre not finite, or a factor not finite and positive (a mirror
/// is [`reflection`]).
pub fn scaling(centre: [f64; 3], factor: f64) -> Result<Matrix> {
    if factor.is_nan() || factor <= 0.0 {
        return Err(Error::Argument);
    }
    taken(Transform::uniform_scaling(vec3(centre), factor))
}

/// The mirror in the plane through `origin` square to `normal` (any length but none).
///
/// # Errors
/// [`Error::Argument`] for an origin or normal not finite, or a normal of no length.
pub fn reflection(origin: [f64; 3], normal: [f64; 3]) -> Result<Matrix> {
    taken(Transform::reflection(vec3(origin), vec3(normal)))
}

/// `first`, then `second` (`p·A1 + t1`, then that `·A2 + t2`): steps stacked into one map.
///
/// # Errors
/// [`Error::Argument`] for a map with a number not finite, given or made.
pub fn then(first: &Matrix, second: &Matrix) -> Result<Matrix> {
    if !finite(first.as_flattened()) || !finite(second.as_flattened()) {
        return Err(Error::Argument);
    }
    let both = Transform { m: *first }.concat(&Transform { m: *second }).m;
    if !finite(both.as_flattened()) {
        return Err(Error::Argument);
    }
    Ok(both)
}

/// The map that undoes `matrix` (same convention): `then(matrix, inverse(matrix))` is the
/// identity to rounding.
///
/// # Errors
/// [`Error::Argument`] for a matrix [`solidcore::Transform::is_regular`] does not prove regular at
/// [`REGULAR_RATIO`] (as [`crate::Body::transform`] refuses it).
pub fn inverse(matrix: &Matrix) -> Result<Matrix> {
    let map = Transform { m: *matrix };
    if map.is_regular(REGULAR_RATIO) != Verdict::Yes {
        return Err(Error::Argument);
    }
    taken(map.inverse())
}

/// How far `point` stands from `origin` along `direction`: the dot product of `point - origin`
/// with `direction`, which is as many lengths as `direction` is long (a unit direction gives
/// the distance itself). Negative behind `origin`.
pub fn distance_along(origin: [f64; 3], direction: [f64; 3], point: [f64; 3]) -> f64 {
    Vec3::from(point).distance_along(Vec3::from(origin), Vec3::from(direction))
}

/// `point` under `matrix` (the row-vector convention above: `p·A + t`), through
/// [`Transform::transform_points`].
pub fn apply_point(matrix: &Matrix, point: [f64; 3]) -> [f64; 3] {
    let mut points = [point];
    Transform { m: *matrix }.transform_points(&mut points);
    let [p] = points;
    p
}

/// The corners of a picture pinned to points of a plane, in that plane's coordinates, through
/// [`solidcore::math::picture_frame`]: its bottom edge from `bl` to `br`, its top-left corner
/// `top_left` when it may stretch, else `aspect` (height over width) times as high as wide with
/// its top on the left of the bottom edge. Bottom-left, bottom-right, top-right, top-left;
/// `None` for corners that give no area.
pub fn picture_frame(
    bl: [f64; 2],
    br: [f64; 2],
    top_left: Option<[f64; 2]>,
    aspect: f64,
) -> Option<[[f64; 2]; 4]> {
    let on = |[x, y]: [f64; 2]| Vec3::new(x, y, 0.0);
    let corners = solidcore::math::picture_frame(
        on(bl),
        on(br),
        top_left.map(on),
        Vec3::new(0.0, 0.0, 1.0),
        aspect,
    )?;
    Some(corners.map(|p| [p.x, p.y]))
}

/// How a picture `aspect` (height over width) times as high as wide fits a frame pinned at `bl`,
/// `br` and `tl`, through [`solidcore::math::picture_fit`]: its width, and whether the frame
/// stretches it. `None` for a frame with no area.
pub fn picture_fit(bl: [f64; 3], br: [f64; 3], tl: [f64; 3], aspect: f64) -> Option<(f64, bool)> {
    solidcore::math::picture_fit(vec3(bl), vec3(br), vec3(tl), aspect)
}

/// A quad's unit normal, its corners lifted along that normal and its corners lifted against it,
/// as [`lifted_quad`] returns them.
pub type LiftedQuad = ([f64; 3], [[f64; 3]; 4], [[f64; 3]; 4]);

/// A quad lifted `lift` clear of its own plane on both sides, through
/// [`solidcore::math::lifted_quad`]: the quad's unit normal (bottom edge cross left edge), its
/// corners moved `lift` along that normal and its corners moved `lift` against it. `None` for
/// numbers not finite or corners with no area.
pub fn lifted_quad(corners: [[f64; 3]; 4], lift: f64) -> Option<LiftedQuad> {
    let (n, front, back) = solidcore::math::lifted_quad(corners.map(vec3), lift)?;
    let arr = |p: Vec3| [p.x, p.y, p.z];
    Some((arr(n), front.map(arr), back.map(arr)))
}

/// The point `s` along the edge `u` and `t` along the edge `v` of the parallelogram with a
/// corner at `corner`, through [`solidcore::math::parallelogram_point`].
pub fn parallelogram_point(corner: [f64; 3], u: [f64; 3], v: [f64; 3], s: f64, t: f64) -> [f64; 3] {
    let p = solidcore::math::parallelogram_point(vec3(corner), vec3(u), vec3(v), s, t);
    [p.x, p.y, p.z]
}

/// How far the box from `min` to `max` (a body's box, say) reaches from `origin` along
/// `direction`, `[low, high]`: the least and the greatest [`distance_along`] of its points,
/// through [`Extent3d::span_along`].
///
/// # Errors
/// [`Error::Argument`] for a number not finite, or a reach not.
pub fn span_along(
    min: [f64; 3],
    max: [f64; 3],
    origin: [f64; 3],
    direction: [f64; 3],
) -> Result<[f64; 2]> {
    if !finite(&min) || !finite(&max) || !finite(&origin) || !finite(&direction) {
        return Err(Error::Argument);
    }
    let span = Extent3d::from_bounds(min[0], min[1], min[2], max[0], max[1], max[2])
        .span_along(vec3(origin), vec3(direction));
    if !finite(&span) {
        return Err(Error::Argument);
    }
    Ok(span)
}

/// The operation name of the refusals of [`extent_along`], [`along_own_axes`] and
/// [`square_plane_offset`].
const OP_AXIS: &str = "affine";

/// The reason of [`Error::Refused`] for a number not finite.
pub const REFUSED_NOT_FINITE: &str = "a number is not finite";
/// The reason of [`Error::Refused`] for an axis with no direction.
pub const REFUSED_ZERO_AXIS: &str = "the axis is zero";
/// The reason of [`Error::Refused`] for a placement that flattens space.
pub const REFUSED_FLAT_MAP: &str = "the placement flattens space";

/// The refusal as ops states it.
fn refused(why: AxisRefusal) -> Error {
    let reason = match why {
        AxisRefusal::NotFinite => REFUSED_NOT_FINITE,
        AxisRefusal::ZeroAxis => REFUSED_ZERO_AXIS,
        AxisRefusal::FlatMap => REFUSED_FLAT_MAP,
    };
    Error::Refused {
        operation: OP_AXIS,
        reason,
    }
}

/// How far the box from `min` to `max` (a body's own box) reaches along `axis`, a direction of
/// the outer frame (any length), once the body is placed by `placement`: the kernel's
/// [`solidcore::math::box_extent_along`]. Tight when `axis` runs along one of the body's own
/// axes ([`along_own_axes`]).
///
/// # Errors
/// [`Error::Refused`] with [`REFUSED_NOT_FINITE`] for a number not finite, [`REFUSED_ZERO_AXIS`]
/// for a zero `axis`, [`REFUSED_FLAT_MAP`] for a `placement` that flattens space.
pub fn extent_along(
    min: [f64; 3],
    max: [f64; 3],
    placement: &Matrix,
    axis: [f64; 3],
) -> Result<f64> {
    solidcore::math::box_extent_along(
        vec3(min),
        vec3(max),
        &Transform { m: *placement },
        vec3(axis),
    )
    .map_err(refused)
}

/// Whether `axis`, a direction of the outer frame, runs along one of the own axes of a body placed
/// by `placement`, through [`solidcore::math::axis_along_own_axes`].
///
/// # Errors
/// As [`extent_along`].
pub fn along_own_axes(placement: &Matrix, axis: [f64; 3]) -> Result<bool> {
    solidcore::math::axis_along_own_axes(&Transform { m: *placement }, vec3(axis)).map_err(refused)
}

/// Where a flat face (through `origin`, normal `normal`, in a body's own frame) of a body placed
/// by `placement` cuts `axis`, if square to it: `Some` of its offset along the unit `axis` from
/// the outer origin, `None` when it is not square to `axis` (or `origin` or `normal` is not
/// finite or the placed normal has no direction), through
/// [`solidcore::math::plane_offset_square_to`].
///
/// # Errors
/// As [`extent_along`], less the flat placement (it is not inverted).
pub fn square_plane_offset(
    placement: &Matrix,
    origin: [f64; 3],
    normal: [f64; 3],
    axis: [f64; 3],
) -> Result<Option<f64>> {
    solidcore::math::plane_offset_square_to(
        &Transform { m: *placement },
        vec3(origin),
        vec3(normal),
        vec3(axis),
    )
    .map_err(refused)
}

/// The axis-aligned box around the corners of the box `lo`..`hi` moved by `matrix`, through
/// [`Transform::moved_box`]: it holds the moved box, a little larger for a turn. An empty box
/// (`lo` above `hi` on an axis) comes back as it is.
///
/// # Errors
/// [`Error::Argument`] for a matrix with a number not finite, a NaN bound (an empty box's
/// too), or a corner of a non-empty box that is not finite or does not map to finite numbers.
pub fn moved_box(matrix: &Matrix, lo: [f64; 3], hi: [f64; 3]) -> Result<([f64; 3], [f64; 3])> {
    if !finite(matrix.as_flattened()) || lo.iter().chain(&hi).any(|v| v.is_nan()) {
        return Err(Error::Argument);
    }
    if lo.iter().zip(&hi).any(|(l, h)| l > h) {
        return Ok((lo, hi));
    }
    if !finite(&lo) || !finite(&hi) {
        return Err(Error::Argument);
    }
    let (a, b) = Transform { m: *matrix }.moved_box(vec3(lo), vec3(hi));
    let (a, b) = ([a.x, a.y, a.z], [b.x, b.y, b.z]);
    if !finite(&a) || !finite(&b) {
        return Err(Error::Argument);
    }
    Ok((a, b))
}

/// How far the turn in `matrix` (its rows 0 to 2; a translation in it is ignored) moves the
/// point `pivot` that it is about, as the move to add to the turn about the origin to make it
/// the turn about `pivot`, through [`Transform::pivot_displacement`].
///
/// # Errors
/// [`Error::Argument`] for a matrix or pivot with a number not finite, or a displacement not.
pub fn pivot_displacement(matrix: &Matrix, pivot: [f64; 3]) -> Result<[f64; 3]> {
    if !finite(matrix.as_flattened()) || !finite(&pivot) {
        return Err(Error::Argument);
    }
    let d = Transform { m: *matrix }.pivot_displacement(vec3(pivot));
    let d = [d.x, d.y, d.z];
    if !finite(&d) {
        return Err(Error::Argument);
    }
    Ok(d)
}

/// `direction`, reversed when it points more than a right angle away from `like`, through
/// [`Vec3::with_sense_of`]: a line's direction given the sense of another (a turn about an edge
/// found again keeps the sense it was picked with). `None` when either has no length or a
/// number not finite.
pub fn same_sense(direction: [f64; 3], like: [f64; 3]) -> Option<[f64; 3]> {
    let d = vec3(direction).with_sense_of(vec3(like))?;
    Some([d.x, d.y, d.z])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_direction_takes_the_sense_of_another_and_none_without_length() {
        assert_eq!(
            same_sense([0.0, -2.0, 0.0], [0.1, 1.0, 0.0]),
            Some([0.0, 2.0, 0.0])
        );
        assert_eq!(
            same_sense([0.0, 2.0, 0.0], [0.1, 1.0, 0.0]),
            Some([0.0, 2.0, 0.0])
        );
        // Not parallel at all: still the sense of the nearer half.
        assert_eq!(
            same_sense([1.0, -1.0, 0.0], [0.0, 1.0, 0.0]),
            Some([-1.0, 1.0, 0.0])
        );
        assert_eq!(same_sense([0.0; 3], [0.0, 1.0, 0.0]), None);
        assert_eq!(same_sense([0.0, 1.0, 0.0], [0.0; 3]), None);
        assert_eq!(same_sense([f64::NAN, 1.0, 0.0], [0.0, 1.0, 0.0]), None);
    }

    /// The mirror in the plane y = 0.
    const MIRROR_Y: Matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    const IDENTITY: Matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    /// `p` under `m`, in the row-vector convention.
    fn apply(m: &Matrix, p: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|j| p[0] * m[0][j] + p[1] * m[1][j] + p[2] * m[2][j] + m[3][j])
    }

    fn near(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
    }

    fn regular(m: &Matrix) -> bool {
        Transform { m: *m }.is_regular(REGULAR_RATIO) == Verdict::Yes
    }

    #[test]
    fn a_box_reaches_its_own_length_along_its_own_axis() {
        let (lo, hi) = ([0.0, 0.0, 0.0], [4.0, 2.0, 1.0]);
        assert_eq!(
            extent_along(lo, hi, &IDENTITY, [1.0, 0.0, 0.0]).unwrap(),
            4.0
        );
        assert_eq!(
            extent_along(lo, hi, &IDENTITY, [0.0, 5.0, 0.0]).unwrap(),
            2.0
        );
        assert!(along_own_axes(&IDENTITY, [0.0, 0.0, -3.0]).unwrap());
        // a move does not change how far it reaches
        let moved = translation([7.0, -3.0, 2.0]).unwrap();
        assert_eq!(extent_along(lo, hi, &moved, [1.0, 0.0, 0.0]).unwrap(), 4.0);
    }

    #[test]
    fn a_turned_box_is_read_along_the_axis_it_has() {
        let (lo, hi) = ([0.0, 0.0, 0.0], [4.0, 2.0, 1.0]);
        // a quarter turn about z takes the body's x to the outer y
        let turn = rotation([0.0; 3], [0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2).unwrap();
        assert!((extent_along(lo, hi, &turn, [0.0, 1.0, 0.0]).unwrap() - 4.0).abs() < 1e-12);
        assert!((extent_along(lo, hi, &turn, [1.0, 0.0, 0.0]).unwrap() - 2.0).abs() < 1e-12);
        assert!(along_own_axes(&turn, [0.0, 1.0, 0.0]).unwrap());
        // a slanted axis runs along none of the body's axes
        assert!(!along_own_axes(&turn, [1.0, 1.0, 0.0]).unwrap());
        assert!(!along_own_axes(&IDENTITY, [1.0, 1.0, 1.0]).unwrap());
    }

    #[test]
    fn a_zero_not_finite_axis_or_singular_placement_is_refused_by_the_extent_ops() {
        let (lo, hi) = ([0.0; 3], [1.0; 3]);
        let refusal = |e: Error| match e {
            Error::Refused { reason, .. } => reason,
            _ => "other",
        };
        let flat: Matrix = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(
            refusal(extent_along(lo, hi, &IDENTITY, [0.0; 3]).unwrap_err()),
            REFUSED_ZERO_AXIS
        );
        assert_eq!(
            refusal(along_own_axes(&IDENTITY, [0.0; 3]).unwrap_err()),
            REFUSED_ZERO_AXIS
        );
        assert_eq!(
            refusal(extent_along(lo, hi, &flat, [1.0, 0.0, 0.0]).unwrap_err()),
            REFUSED_FLAT_MAP
        );
        assert_eq!(
            refusal(along_own_axes(&flat, [1.0, 0.0, 0.0]).unwrap_err()),
            REFUSED_FLAT_MAP
        );
        assert_eq!(
            refusal(extent_along(lo, hi, &IDENTITY, [f64::NAN, 0.0, 1.0]).unwrap_err()),
            REFUSED_NOT_FINITE
        );
        assert_eq!(
            refusal(
                extent_along([f64::NAN, 0.0, 0.0], hi, &IDENTITY, [1.0, 0.0, 0.0]).unwrap_err()
            ),
            REFUSED_NOT_FINITE
        );
        let mut bad = IDENTITY;
        bad[3][0] = f64::INFINITY;
        assert!(along_own_axes(&bad, [1.0, 0.0, 0.0]).is_err());
    }

    #[test]
    fn a_plane_square_to_the_axis_gives_its_offset_and_one_that_is_not_gives_none() {
        let z = [0.0, 0.0, 1.0];
        let at = |m: &Matrix, origin, normal, axis| {
            square_plane_offset(m, origin, normal, axis).unwrap()
        };
        // a face at z = 3 with a normal along z, any length of axis
        assert_eq!(
            at(
                &IDENTITY,
                [1.0, 1.0, 3.0],
                [0.0, 0.0, -2.0],
                [0.0, 0.0, 4.0]
            ),
            Some(3.0)
        );
        // sideways to the axis: not square
        assert_eq!(at(&IDENTITY, [0.0; 3], [1.0, 0.0, 0.0], z), None);
        assert_eq!(at(&IDENTITY, [0.0; 3], [0.0, 1.0, 1.0], z), None);
        // a placement that moves and turns: the face z = 1 of a body turned a quarter about x
        // faces along -y, and the body moved by 5 along y puts it at y = 5 - 1
        let turn = rotation([0.0; 3], [1.0, 0.0, 0.0], std::f64::consts::FRAC_PI_2).unwrap();
        let placed = then(&turn, &translation([0.0, 5.0, 0.0]).unwrap()).unwrap();
        let off = at(&placed, [0.0, 0.0, 1.0], z, [0.0, 1.0, 0.0]).unwrap();
        assert!((off - 4.0).abs() < 1e-12);
        assert_eq!(at(&placed, [0.0, 0.0, 1.0], z, [0.0, 0.0, 1.0]), None);
        // no direction to the normal, or not finite: not a face to take
        assert_eq!(at(&IDENTITY, [0.0; 3], [0.0; 3], z), None);
        assert_eq!(at(&IDENTITY, [f64::NAN, 0.0, 0.0], z, z), None);
        // a zero or not finite axis is refused
        assert!(square_plane_offset(&IDENTITY, [0.0; 3], z, [0.0; 3]).is_err());
        assert!(square_plane_offset(&IDENTITY, [0.0; 3], z, [f64::NAN, 0.0, 1.0]).is_err());
    }

    #[test]
    fn a_mirror_in_y_0_is_the_mirror_matrix_and_twice_is_the_identity() {
        assert_eq!(reflection([0.0; 3], [0.0, 1.0, 0.0]).unwrap(), MIRROR_Y);
        // any length of an axis normal, any origin on its plane's side: exact
        for (origin, normal) in [
            ([3.0, 5.0, -2.0], [0.0, 4.0, 0.0]),
            ([1.0, 2.0, 7.5], [0.0, 0.0, -1.0]),
            ([-6.0, 0.0, 1.0], [2.0, 0.0, 0.0]),
        ] {
            let m = reflection(origin, normal).unwrap();
            assert!(regular(&m));
            assert_eq!(then(&m, &m).unwrap(), IDENTITY);
        }
    }

    #[test]
    fn the_displacement_of_a_pivot_is_what_a_turn_about_it_adds() {
        let q = std::f64::consts::FRAC_PI_2;
        let about_origin = rotation([0.0; 3], [0.0, 0.0, 1.0], q).unwrap();
        let pivot = [0.0, 10.0, 0.0];
        let d = pivot_displacement(&about_origin, pivot).unwrap();
        // The turn about the pivot is the turn about the origin, then this move.
        let about_pivot = rotation(pivot, [0.0, 0.0, 1.0], q).unwrap();
        for p in [[1.0, 0.5, 0.5], [-3.0, 2.0, 7.0]] {
            let turned = apply(&about_origin, p);
            assert!(near(
                [turned[0] + d[0], turned[1] + d[1], turned[2] + d[2]],
                apply(&about_pivot, p),
                1e-12
            ));
        }
        assert_eq!(pivot_displacement(&IDENTITY, pivot).unwrap(), [0.0; 3]);
        assert_eq!(
            pivot_displacement(&about_origin, [f64::NAN, 0.0, 0.0]),
            Err(Error::Argument)
        );
    }

    #[test]
    fn a_moved_box_holds_the_moved_box_and_refuses_numbers_not_finite() {
        let turn = then(
            &rotation([0.0; 3], [0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2).unwrap(),
            &translation([10.0, 20.0, 30.0]).unwrap(),
        )
        .unwrap();
        let (lo, hi) = moved_box(&turn, [0.0; 3], [1.0, 2.0, 3.0]).unwrap();
        assert!(near(lo, [8.0, 20.0, 30.0], 1e-12) && near(hi, [10.0, 21.0, 33.0], 1e-12));
        let empty = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        assert_eq!(moved_box(&turn, empty.0, empty.1).unwrap(), empty);
        assert!(moved_box(&turn, [f64::NAN, 0.0, 0.0], [1.0; 3]).is_err());
        // a NaN on one axis is refused even when another axis is reversed (empty)
        assert!(moved_box(&IDENTITY, [1.0, f64::NAN, 0.0], [0.0, 1.0, 1.0]).is_err());
        assert!(moved_box(&IDENTITY, [1.0, 0.0, 0.0], [0.0, 1.0, f64::NAN]).is_err());
        let mut bad = IDENTITY;
        bad[3][0] = f64::NAN;
        assert!(moved_box(&bad, [0.0; 3], [1.0; 3]).is_err());
    }

    #[test]
    fn a_quarter_turn_about_z_takes_x_to_y_and_a_turn_keeps_its_line() {
        let q = std::f64::consts::FRAC_PI_2;
        let m = rotation([0.0; 3], [0.0, 0.0, 1.0], q).unwrap();
        assert!(regular(&m));
        assert!(near(apply(&m, [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0], 1e-15));
        assert!(near(apply(&m, [0.0, 1.0, 0.0]), [-1.0, 0.0, 0.0], 1e-15));
        // about the line through (10, 0, 0) along a slanted axis: the line's points stay
        let m = rotation([10.0, 0.0, 0.0], [1.0, 2.0, 2.0], 0.7).unwrap();
        assert!(regular(&m));
        assert!(near(apply(&m, [10.0, 0.0, 0.0]), [10.0, 0.0, 0.0], 1e-13));
        assert!(near(apply(&m, [13.0, 6.0, 6.0]), [13.0, 6.0, 6.0], 1e-12));
    }

    #[test]
    fn a_scale_keeps_its_centre_and_moves_stack() {
        let c = [1.0, -2.0, 3.0];
        let m = scaling(c, 2.5).unwrap();
        assert!(regular(&m));
        assert!(near(apply(&m, c), c, 1e-14));
        assert!(near(apply(&m, [2.0, -2.0, 3.0]), [3.5, -2.0, 3.0], 1e-14));
        let (a, b) = ([1.5, -2.0, 3.0], [0.25, 4.0, -1.0]);
        let sum = [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
        let stacked = then(&translation(a).unwrap(), &translation(b).unwrap()).unwrap();
        assert_eq!(stacked, translation(sum).unwrap());
        assert!(regular(&stacked));
    }

    #[test]
    fn a_map_then_its_inverse_is_the_identity_and_a_mirror_is_its_own() {
        let turn = rotation([1.0, 2.0, 3.0], [1.0, -2.0, 2.0], 0.7).unwrap();
        let map = then(&turn, &translation([4.0, -5.0, 6.0]).unwrap()).unwrap();
        let size = map.iter().flatten().fold(1.0_f64, |a, x| a.max(x.abs()));
        for m in [
            map,
            then(&map, &scaling([-1.0, 0.5, 2.0], 2.5).unwrap()).unwrap(),
            reflection([0.5, -1.0, 3.0], [1.0, 1.0, 1.0]).unwrap(),
        ] {
            let inv = inverse(&m).unwrap();
            assert!(regular(&inv));
            for back in [then(&m, &inv).unwrap(), then(&inv, &m).unwrap()] {
                let off = back
                    .iter()
                    .flatten()
                    .zip(IDENTITY.iter().flatten())
                    .fold(0.0_f64, |a, (x, y)| a.max((x - y).abs()));
                assert!(off <= 1e-15 * size * 4.0, "{back:?}");
            }
        }
        let mirror = reflection([3.0, 5.0, -2.0], [0.0, 4.0, 0.0]).unwrap();
        assert_eq!(inverse(&mirror).unwrap(), mirror);
        let mut flat = IDENTITY;
        flat[1] = [0.0; 4];
        assert_eq!(inverse(&flat), Err(Error::Argument));
        let mut nan = IDENTITY;
        nan[3][0] = f64::NAN;
        assert_eq!(inverse(&nan), Err(Error::Argument));
    }

    #[test]
    fn numbers_not_finite_or_of_no_size_are_refused() {
        assert_eq!(translation([f64::NAN, 0.0, 0.0]), Err(Error::Argument));
        assert_eq!(rotation([0.0; 3], [0.0; 3], 1.0), Err(Error::Argument));
        assert_eq!(
            rotation([0.0; 3], [0.0, 0.0, 1.0], f64::INFINITY),
            Err(Error::Argument)
        );
        assert_eq!(scaling([0.0; 3], 0.0), Err(Error::Argument));
        assert_eq!(scaling([0.0; 3], -1.0), Err(Error::Argument));
        assert_eq!(scaling([0.0; 3], f64::NAN), Err(Error::Argument));
        assert_eq!(scaling([f64::NAN, 0.0, 0.0], 2.0), Err(Error::Argument));
        assert_eq!(reflection([0.0; 3], [0.0; 3]), Err(Error::Argument));
        assert_eq!(
            reflection([0.0, f64::NAN, 0.0], [0.0, 1.0, 0.0]),
            Err(Error::Argument)
        );
        // a stack whose numbers overflow
        let far = translation([1e308, 0.0, 0.0]).unwrap();
        assert_eq!(then(&far, &far), Err(Error::Argument));
    }

    #[test]
    fn distance_along_is_signed_and_ignores_the_sideways_offset() {
        let (o, d) = ([1.0, 2.0, 3.0], [0.0, 1.0, 0.0]);
        assert!((distance_along(o, d, [9.0, 6.0, -4.0]) - 4.0).abs() < 1e-12);
        assert!((distance_along(o, d, [1.0, -1.0, 3.0]) + 3.0).abs() < 1e-12);
        assert_eq!(distance_along(o, d, o), 0.0);
    }

    /// A picture 2:1 on a 40 mm bottom edge is 20 mm high, on the edge's left; a top-left point
    /// stretches it; corners on one line give none.
    #[test]
    fn a_picture_frame_follows_its_bottom_edge_and_its_shape_or_top_corner() {
        assert_eq!(
            picture_frame([0.0, 0.0], [40.0, 0.0], None, 0.5),
            Some([[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]])
        );
        assert_eq!(
            picture_frame([0.0, 0.0], [40.0, 0.0], Some([0.0, 30.0]), 0.5),
            Some([[0.0, 0.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0]])
        );
        assert_eq!(picture_frame([0.0, 0.0], [0.0, 0.0], None, 0.5), None);
        assert_eq!(
            picture_frame([0.0, 0.0], [40.0, 0.0], Some([20.0, 0.0]), 0.5),
            None
        );
        assert_eq!(picture_frame([f64::NAN, 0.0], [40.0, 0.0], None, 0.5), None);
    }

    /// A 2:1 picture on a 40 mm edge in the XZ plane fits a frame 20 mm high, and one 30 mm high
    /// stretches it; a top-left corner on the bottom edge's line gives no frame.
    #[test]
    fn a_picture_fits_a_frame_of_its_shape_and_a_taller_one_stretches_it() {
        let (bl, br) = ([0.0, 0.0, 0.0], [40.0, 0.0, 0.0]);
        assert_eq!(
            picture_fit(bl, br, [0.0, 0.0, 20.0], 0.5),
            Some((40.0, false))
        );
        assert_eq!(
            picture_fit(bl, br, [0.0, 0.0, 30.0], 0.5),
            Some((40.0, true))
        );
        assert_eq!(picture_fit(bl, br, [20.0, 0.0, 0.0], 0.5), None);
    }

    /// A 10 mm square at z = 5 lifted 0.2 mm: the front 0.2 mm above, the back 0.2 mm below.
    #[test]
    fn a_lifted_quad_lies_either_side_of_its_plane() {
        let square = [
            [0.0, 0.0, 5.0],
            [10.0, 0.0, 5.0],
            [10.0, 10.0, 5.0],
            [0.0, 10.0, 5.0],
        ];
        let (n, front, back) = lifted_quad(square, 0.2).unwrap();
        assert_eq!(n, [0.0, 0.0, 1.0]);
        assert!(front
            .iter()
            .zip(square)
            .all(|(f, s)| near(*f, [s[0], s[1], 5.2], 1e-12)));
        assert!(back
            .iter()
            .zip(square)
            .all(|(b, s)| near(*b, [s[0], s[1], 4.8], 1e-12)));
        assert_eq!(lifted_quad([[1.0, 1.0, 1.0]; 4], 0.2), None);
        assert_eq!(lifted_quad(square, f64::NAN), None);
        assert_eq!(
            parallelogram_point([1.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 2.0, 0.0], 0.5, 0.25),
            [3.0, 0.5, 0.0]
        );
        let moved = translation([1.0, 2.0, 3.0]).unwrap();
        assert_eq!(apply_point(&moved, [1.0, 1.0, 1.0]), [2.0, 3.0, 4.0]);
    }

    #[test]
    fn span_along_takes_the_box_corners_either_way_round_and_refuses_what_is_not_finite() {
        let (o, d) = ([1.0, 1.0, 1.0], [1.0, 2.0, 2.0]);
        assert_eq!(
            span_along([-1.0, 0.0, 2.0], [3.0, 5.0, 4.0], o, d),
            Ok([-2.0, 16.0])
        );
        assert_eq!(
            span_along([3.0, 5.0, 4.0], [-1.0, 0.0, 2.0], o, d),
            Ok([-2.0, 16.0])
        );
        assert_eq!(
            span_along([0.0; 3], [1.0; 3], o, [0.0, f64::NAN, 1.0]),
            Err(Error::Argument)
        );
        assert_eq!(
            span_along([0.0; 3], [f64::INFINITY; 3], o, d),
            Err(Error::Argument)
        );
        assert_eq!(
            span_along([0.0; 3], [f64::MAX; 3], o, d),
            Err(Error::Argument)
        );
    }
}
