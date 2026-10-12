//! Small `f64` linear-algebra types: [`Vec3`]/[`Point3`], [`Vec2`]/[`Point2`],
//! [`Transform`], and the numeric constants of `SmMath.h`.
//!
//! Upstream: `source/SMLib/inc/SmVector3d.h`, `SmVector2d.h`, `SmCoreTypes.h`,
//! `SmMath.h`, `SmTransform.h` / `src/SmTransform.cpp`, and `smgu_4x4Mult` in
//! `src/SmGeomUtility.cpp`.
//!
//! | Rust                         | Upstream                                       |
//! |------------------------------|------------------------------------------------|
//! | [`Vec3`]                     | `SmVector3d`                                   |
//! | [`Point3`] (alias of `Vec3`) | `SmPoint3d` (`#define SmPoint3d SmVector3d`)   |
//! | [`Vec2`] / [`Point2`]        | `SmVector2d` / `SmPoint2d` (re-exported from [`crate::geom::vector2d`]) |
//! | [`Vec3::dot`]                | `SmVector3d::Dot`                              |
//! | [`Vec3::cross`]              | `operator*(const SmVector3d&, const SmVector3d&)` |
//! | [`Vec3::length_squared`]     | `SmVector3d::LengthSquared`                    |
//! | [`Vec3::length`]             | `SmVector3d::Length`                           |
//! | [`Vec3::unitize`]            | `SmVector3d::Unitize`                          |
//! | [`Vec3::distance`]           | `SmVector3d::DistanceBetween`                  |
//! | [`Transform`]                | `SmTransform` (`m_adT[4][4]`)                  |
//! | [`Transform::concat`]        | `SmTransform::ConcatTransform` / `smgu_4x4Mult`|
//! | [`Transform::transform_point`]  | `SmTransform::TransformPoint`               |
//! | [`Transform::transform_vector`] | `SmTransform::TransformVector`              |
//! | [`EFF_ZERO`] etc.            | `SM_EFF_ZERO` etc.                             |
//!
//! ## Why not `glam`/`nalgebra`
//!
//! The correctness gate compares against the C++ to `1e-9` relative, and the port keeps
//! upstream operation order. Third-party vector crates may reassociate sums, use SIMD
//! lanes or fused multiply-add, and do not match upstream degenerate-case behaviour
//! (e.g. `Unitize` of a zero vector). These hand-written types spell every expression
//! in upstream order, so results are bit-identical to the C++ (built with
//! `-ffp-contract=off`).

use core::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use crate::error::{Result, SolidError};

pub use crate::geom::vector2d::{Point2, Vec2};

/// `SM_PI` (the upstream literal rounds to exactly this `f64`).
pub const PI: f64 = core::f64::consts::PI;
/// `SM_HALFPI`.
pub const HALF_PI: f64 = core::f64::consts::FRAC_PI_2;
/// `SM_3HALFPI`.
pub const THREE_HALF_PI: f64 = 4.7123889803846897576939650749192543262957530990626;
/// `SM_2PI`.
pub const TWO_PI: f64 = core::f64::consts::TAU;
/// `SM_SQRT3`.
pub const SQRT3: f64 = 1.7320508075688772935274463415058723669428052538104;
/// `SM_EFF_ZERO`: numeric zero tolerance.
pub const EFF_ZERO: f64 = 1.0e-12;
/// `SM_EFF_ZERO_SQ`: `EFF_ZERO` squared.
pub const EFF_ZERO_SQ: f64 = 1.0e-24;
/// `SM_EFF_ZERO_SQRT`: square root of `EFF_ZERO`.
pub const EFF_ZERO_SQRT: f64 = 1.0e-6;
/// `SM_EFF_ZERO_PARAM`: numeric zero tolerance for knot/parameter values.
pub const EFF_ZERO_PARAM: f64 = 1.2e-8;
/// `SM_EFF_ZERO_DEG`: numeric zero tolerance for angles in degrees.
pub const EFF_ZERO_DEG: f64 = 1.0e-4;
/// `SM_EFF_ZERO_RAD`: numeric zero tolerance for angles in radians.
pub const EFF_ZERO_RAD: f64 = EFF_ZERO_DEG * PI / 180.0;
/// `SM_UNDEF_DOUBLE`: upstream's sentinel for an unset double.
pub const UNDEF_DOUBLE: f64 = 1.23456e-61;

/// A 3D vector (`SmVector3d`). Also used for points, see [`Point3`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

/// A 3D point. Upstream uses one type for points and vectors
/// (`#define SmPoint3d SmVector3d`), and so does the port.
pub type Point3 = Vec3;

impl Vec3 {
    /// The zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    /// Unit X axis.
    pub const X: Self = Self::new(1.0, 0.0, 0.0);
    /// Unit Y axis.
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);
    /// Unit Z axis.
    pub const Z: Self = Self::new(0.0, 0.0, 1.0);

    /// Creates a vector from its components.
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Components as an array `[x, y, z]`.
    #[must_use]
    pub const fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    /// Dot product, `(x*o.x) + (y*o.y) + (z*o.z)` (`SmVector3d::Dot`).
    #[must_use]
    pub fn dot(self, o: Self) -> f64 {
        (self.x * o.x) + (self.y * o.y) + (self.z * o.z)
    }

    /// Cross product `self × o` (upstream `operator*` on two vectors).
    #[must_use]
    pub fn cross(self, o: Self) -> Self {
        Self::new(
            (self.y * o.z) - (self.z * o.y),
            (self.z * o.x) - (self.x * o.z),
            (self.x * o.y) - (self.y * o.x),
        )
    }

    /// Squared Euclidean length (`SmVector3d::LengthSquared`).
    #[must_use]
    pub fn length_squared(self) -> f64 {
        (self.x * self.x) + (self.y * self.y) + (self.z * self.z)
    }

    /// Euclidean length (`SmVector3d::Length`).
    #[must_use]
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    /// Distance between two points (`SmVector3d::DistanceBetween`).
    #[must_use]
    pub fn distance(self, o: Self) -> f64 {
        (self - o).length()
    }

    /// True when the length is below `tol` (`SmVector3d::IsZero`).
    #[must_use]
    pub fn is_zero(self, tol: f64) -> bool {
        self.length_squared() < tol * tol
    }

    /// Unit vector in the same direction (`SmVector3d::Unitize`).
    ///
    /// Mirrors upstream exactly: a vector whose squared length is within
    /// [`EFF_ZERO_SQ`] of 1 is returned unchanged; a vector with squared length below
    /// [`EFF_ZERO_SQ`] is an error. Upstream additionally overwrites such a vector with
    /// `(0, 0, 1)`; callers that rely on that use [`Vec3::unitize_or_z`].
    pub fn unitize(self) -> Result<Self> {
        let len_sq = self.length_squared();
        if len_sq < EFF_ZERO_SQ {
            return Err(SolidError::Degenerate {
                operation: "math::Vec3::unitize",
                entity: None,
            });
        }
        Ok(self.unitize_nonzero(len_sq))
    }

    /// Like [`Vec3::unitize`], but returns upstream's fallback `(0, 0, 1)` for a zero
    /// vector (`Unitize(FALSE)` with its status ignored).
    #[must_use]
    pub fn unitize_or_z(self) -> Self {
        let len_sq = self.length_squared();
        if len_sq < EFF_ZERO_SQ {
            Self::Z
        } else {
            self.unitize_nonzero(len_sq)
        }
    }

    fn unitize_nonzero(self, len_sq: f64) -> Self {
        if (len_sq - 1.0).abs() > EFF_ZERO_SQ {
            let len = len_sq.sqrt();
            Self::new(self.x / len, self.y / len, self.z / len)
        } else {
            self
        }
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, o: Self) {
        *self = *self - o;
    }
}

impl Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, d: f64) -> Self {
        Self::new(self.x * d, self.y * d, self.z * d)
    }
}

impl Mul<Vec3> for f64 {
    type Output = Vec3;
    fn mul(self, v: Vec3) -> Vec3 {
        Vec3::new(v.x * self, v.y * self, v.z * self)
    }
}

impl MulAssign<f64> for Vec3 {
    fn mul_assign(&mut self, d: f64) {
        *self = *self * d;
    }
}

/// An affine 3D transform stored as upstream's row-vector 4×4 matrix
/// (`SmTransform::m_adT`): `out = [p.x p.y p.z 1] * m`.
///
/// Rows 0..3 are the images of the X, Y, Z axes; row 3 is the translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// The 4×4 matrix, `m[row][col]`.
    pub m: [[f64; 4]; 4],
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// A pure translation by `d`.
    #[must_use]
    pub const fn translation(d: Vec3) -> Self {
        let mut t = Self::IDENTITY;
        t.m[3] = [d.x, d.y, d.z, 1.0];
        t
    }

    /// Builds a transform from the images of the axes and the origin
    /// (`SetToXAxis`/`SetToYAxis`/`SetToZAxis`/`SetToDisp`).
    #[must_use]
    pub const fn from_axes(x: Vec3, y: Vec3, z: Vec3, origin: Point3) -> Self {
        Self {
            m: [
                [x.x, x.y, x.z, 0.0],
                [y.x, y.y, y.z, 0.0],
                [z.x, z.y, z.z, 0.0],
                [origin.x, origin.y, origin.z, 1.0],
            ],
        }
    }

    /// Image of the X axis (`GetToXAxis`).
    #[must_use]
    pub const fn x_axis(&self) -> Vec3 {
        Vec3::new(self.m[0][0], self.m[0][1], self.m[0][2])
    }

    /// Image of the Y axis (`GetToYAxis`).
    #[must_use]
    pub const fn y_axis(&self) -> Vec3 {
        Vec3::new(self.m[1][0], self.m[1][1], self.m[1][2])
    }

    /// Image of the Z axis (`GetToZAxis`).
    #[must_use]
    pub const fn z_axis(&self) -> Vec3 {
        Vec3::new(self.m[2][0], self.m[2][1], self.m[2][2])
    }

    /// Translation part (`GetToDisp` / `GetToOrigin`).
    #[must_use]
    pub const fn translation_part(&self) -> Vec3 {
        Vec3::new(self.m[3][0], self.m[3][1], self.m[3][2])
    }

    /// `self * next`: apply `self`, then `next` (`ConcatTransform`, `smgu_4x4Mult`,
    /// same summation order).
    #[must_use]
    pub fn concat(&self, next: &Self) -> Self {
        let mut r = [[0.0; 4]; 4];
        // Loop order is kk-outer instead of upstream's jj-outer, but each r[ii][jj] still
        // accumulates 0.0 + a[ii][0]*b[0][jj] + ... + a[ii][3]*b[3][jj] in kk order,
        // so the result is bit-identical.
        for (r_row, a_row) in r.iter_mut().zip(self.m.iter()) {
            for (a_val, b_row) in a_row.iter().zip(next.m.iter()) {
                for (r_val, b_val) in r_row.iter_mut().zip(b_row.iter()) {
                    *r_val += a_val * b_val;
                }
            }
        }
        Self { m: r }
    }

    /// Maps a point (`SmTransform::TransformPoint`).
    #[must_use]
    pub fn transform_point(&self, p: Point3) -> Point3 {
        let (tx, ty, tz, td) = (
            self.x_axis(),
            self.y_axis(),
            self.z_axis(),
            self.translation_part(),
        );
        Vec3::new(
            td.x + p.x * tx.x + p.y * ty.x + p.z * tz.x,
            td.y + p.x * tx.y + p.y * ty.y + p.z * tz.y,
            td.z + p.x * tx.z + p.y * ty.z + p.z * tz.z,
        )
    }

    /// Maps a vector, ignoring translation (`SmTransform::TransformVector`).
    #[must_use]
    pub fn transform_vector(&self, v: Vec3) -> Vec3 {
        let (tx, ty, tz) = (self.x_axis(), self.y_axis(), self.z_axis());
        Vec3::new(
            v.x * tx.x + v.y * ty.x + v.z * tz.x,
            v.x * tx.y + v.y * ty.y + v.z * tz.y,
            v.x * tx.z + v.y * ty.z + v.z * tz.z,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::approx_constant)]
    fn constants_match_upstream_literals() {
        // Upstream SmMath.h literals round to the std constants bit for bit.
        assert_eq!(PI, 3.1415926535897932384626433832795028841971693993751);
        assert_eq!(HALF_PI, 1.5707963267948965192313216916397514420985846996875);
        assert_eq!(TWO_PI, 6.2831853071795864769252867665590057683943387987502);
    }

    #[test]
    fn dot_cross_length() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(-4.0, 5.0, 0.5);
        assert_eq!(a.dot(b), -4.0 + 10.0 + 1.5);
        assert_eq!(
            a.cross(b),
            Vec3::new(2.0 * 0.5 - 3.0 * 5.0, 3.0 * -4.0 - 0.5, 5.0 + 8.0)
        );
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
        assert_eq!(Vec3::new(3.0, 4.0, 12.0).length(), 13.0);
        assert_eq!(a.cross(b).dot(a), 0.0);
    }

    #[test]
    fn unitize_matches_upstream_cases() {
        let u = Vec3::new(3.0, 0.0, 4.0).unitize().unwrap();
        assert_eq!(u, Vec3::new(0.6, 0.0, 0.8));
        // Already-unit vectors are returned bit-for-bit.
        let n = Vec3::new(0.6, 0.0, 0.8);
        assert_eq!(n.unitize().unwrap(), n);
        // Zero vector: error, or upstream's (0,0,1) fallback.
        assert!(matches!(
            Vec3::ZERO.unitize(),
            Err(SolidError::Degenerate { .. })
        ));
        assert_eq!(Vec3::new(1e-13, 0.0, 0.0).unitize_or_z(), Vec3::Z);
    }

    #[test]
    fn transform_point_vector_concat() {
        let rot_z = Transform::from_axes(Vec3::Y, -Vec3::X, Vec3::Z, Vec3::ZERO);
        let tr = Transform::translation(Vec3::new(1.0, 2.0, 3.0));
        let p = Vec3::new(1.0, 0.0, 0.0);
        assert_eq!(rot_z.transform_point(p), Vec3::new(0.0, 1.0, 0.0));
        // concat applies self first, then the argument.
        let both = rot_z.concat(&tr);
        assert_eq!(both.transform_point(p), Vec3::new(1.0, 3.0, 3.0));
        assert_eq!(both.transform_vector(p), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(Transform::IDENTITY.concat(&both), both);
        assert_eq!(Transform::default(), Transform::IDENTITY);
    }

    #[test]
    fn vec2_basics() {
        let a = Vec2::new(3.0, 4.0);
        assert_eq!(a.length(), 5.0);
        assert_eq!((a - a + a * 2.0).dot(-a), -50.0);
    }
}
