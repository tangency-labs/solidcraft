//! Affine maps from an application's numbers, as the matrices [`crate::Body::transform`] and
//! [`crate::curve::transform`] take: rows 0-2 the images of x, y and z, row 3 the translation
//! (`p' = p·A + t`), the fourth column `(0, 0, 0, 1)`. A move, a turn about a line, a scale
//! about a point, a mirror in a plane, one map after another, and the map that undoes one. The maps are the kernel's
//! ([`solidcore::Transform::translation`], [`solidcore::Transform::rotation`],
//! [`solidcore::Transform::uniform_scaling`], [`solidcore::Transform::reflection`],
//! [`solidcore::Transform::concat`], [`solidcore::Transform::inverse`]); ops checks the
//! numbers and passes them on.

use solidcore::geom::affine_fit;
use solidcore::geom::certified::Verdict;
use solidcore::{Transform, Vec3};
use solidcore_convert::{record_vec3, vec3 as kernel_vec3};
use solidcore_record::Vec3 as RecordVec3;

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

/// The rigid or affine map that takes every source point to its supplied target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// A translation or rotation preserves all pair distances.
    Rigid(Matrix),
    /// A proper affine map includes a non-rigid stretch.
    Affine(Matrix),
}

/// Match ordered points to a proper rigid or affine map, when one fits all the pairs.
pub fn fit_between(from: &[RecordVec3], to: &[RecordVec3]) -> Option<Fit> {
    if from.len() != to.len() {
        return None;
    }
    let pairs = from
        .iter()
        .zip(to)
        .map(|(&p, &q)| (kernel_vec3(p), kernel_vec3(q)));
    Some(match affine_fit::between(pairs)? {
        affine_fit::Fit::Rigid(map) => Fit::Rigid(map.m),
        affine_fit::Fit::Affine(map) => Fit::Affine(map.m),
    })
}

/// Rotate about the line through `origin`, then translate, with editor axis tolerances.
pub fn rigid_motion(
    translation: RecordVec3,
    origin: RecordVec3,
    axis: RecordVec3,
    angle: f64,
) -> Result<Matrix> {
    affine_fit::rigid_motion(
        kernel_vec3(translation),
        kernel_vec3(origin),
        kernel_vec3(axis),
        angle,
        1e-12,
    )
    .map(|map| map.m)
    .ok_or(Error::Argument)
}

/// Check the axes of a proper orthonormal frame at a caller's tolerance.
pub fn proper_axes(axes: [RecordVec3; 3], tolerance: f64) -> bool {
    affine_fit::proper_axes(axes.map(kernel_vec3), tolerance)
}

/// Orthonormalise the first two directions of an affine image of a frame.
pub fn mapped_axes(matrix: &Matrix, axes: [RecordVec3; 3]) -> Option<[RecordVec3; 3]> {
    affine_fit::mapped_axes(&Transform { m: *matrix }, axes.map(kernel_vec3))
        .map(|axes| axes.map(record_vec3))
}

/// Build a frame's matrix from its supplied axes and origin.
pub fn from_frame(origin: RecordVec3, axes: [RecordVec3; 3]) -> Matrix {
    affine_fit::from_frame(kernel_vec3(origin), axes.map(kernel_vec3)).m
}

/// Map a point using a qualified affine matrix.
pub fn point(matrix: &Matrix, point: RecordVec3) -> RecordVec3 {
    let map = Transform { m: *matrix };
    record_vec3(affine_fit::point(&map, kernel_vec3(point)))
}

/// Map a displacement using a qualified affine matrix.
pub fn vector(matrix: &Matrix, vector: RecordVec3) -> RecordVec3 {
    record_vec3(affine_fit::vector(
        &Transform { m: *matrix },
        kernel_vec3(vector),
    ))
}

/// Resolve a vector's components in a qualified orthonormal frame.
pub fn frame_vector(axes: [RecordVec3; 3], vector: RecordVec3) -> RecordVec3 {
    record_vec3(affine_fit::frame_vector(
        axes.map(kernel_vec3),
        kernel_vec3(vector),
    ))
}

/// Resolve a point relative to a qualified orthonormal frame and its origin.
pub fn frame_point(origin: RecordVec3, axes: [RecordVec3; 3], point: RecordVec3) -> RecordVec3 {
    record_vec3(affine_fit::frame_point(
        kernel_vec3(origin),
        axes.map(kernel_vec3),
        kernel_vec3(point),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
