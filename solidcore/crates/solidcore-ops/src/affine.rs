//! Affine maps from an application's numbers, as the matrices [`crate::Body::transform`] and
//! [`crate::curve::transform`] take: rows 0-2 the images of x, y and z, row 3 the translation
//! (`p' = p·A + t`), the fourth column `(0, 0, 0, 1)`. A move, a turn about a line, a scale
//! about a point, a mirror in a plane, one map after another, and the map that undoes one. The maps are the kernel's
//! ([`solidcore::Transform::translation`], [`solidcore::Transform::rotation`],
//! [`solidcore::Transform::uniform_scaling`], [`solidcore::Transform::reflection`],
//! [`solidcore::Transform::concat`], [`solidcore::Transform::inverse`]); ops checks the
//! numbers and passes them on.

use solidcore::geom::certified::Verdict;
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
