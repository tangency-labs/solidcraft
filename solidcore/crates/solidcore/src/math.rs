//! Small `f64` linear-algebra types: [`Vec3`]/[`Point3`], [`Vec2`]/[`Point2`],
//! [`Transform`], and the numeric constants of `SmMath.h`.
//!
//! Upstream: `source/SMLib/inc/SmVector3d.h`, `SmVector2d.h`, `SmCoreTypes.h`,
//! `SmMath.h`, `SmTransform.h` / `src/SmTransform.cpp`, and `smgu_4x4Mult` in
//! `src/SmGeomUtility.cpp`.
//!
//! | Rust                         | Upstream                                       |
//! |------------------------------|------------------------------------------------|
//! | [`Vec3`] / [`Point3`]        | `SmVector3d` / `SmPoint3d` (re-exported from [`crate::geom::vector3d`]) |
//! | [`Vec2`] / [`Point2`]        | `SmVector2d` / `SmPoint2d` (re-exported from [`crate::geom::vector2d`]) |
//! | [`Transform`]                | `SmTransform` (`m_adT[4][4]`)                  |
//! | [`Transform::concat`]        | `SmTransform::ConcatTransform` / `smgu_4x4Mult`|
//! | [`Transform::transform_point`]  | `SmTransform::TransformPoint`               |
//! | [`Transform::transform_vector`] | `SmTransform::TransformVector`              |
//! | [`Transform::rotation`], [`Transform::uniform_scaling`], [`Transform::reflection`] | (not upstream) |
//! | [`EFF_ZERO`] etc.            | `SM_EFF_ZERO` etc.                             |
//! | [`is_infinite_param`]        | `SM_IS_INFINITE`                               |
//! | [`max2`], [`min2`], [`max3`], [`min3`], [`max4`], [`min4`] | `smos_Max`, `smos_Min`, `smos_3Max`, `smos_3Min`, `smos_4Max`, `smos_4Min` |
//! | [`rad_to_deg`]               | `SM_RAD2DEG`                                   |
//!
//! ## Why not `glam`/`nalgebra`
//!
//! The correctness gate compares against the C++ to `1e-9` relative, and the port keeps
//! upstream operation order. Third-party vector crates may reassociate sums, use SIMD
//! lanes or fused multiply-add, and do not match upstream degenerate-case behaviour
//! (e.g. `Unitize` of a zero vector). These hand-written types spell every expression
//! in upstream order, so results are bit-identical to the C++ (built with
//! `-ffp-contract=off`).

pub use crate::geom::vector2d::{Point2, Vec2};
pub use crate::geom::vector3d::{Point3, Vec3};

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
/// `SM_BIG_DOUBLE`: a value larger than any coordinate (used for "inside out" extents).
pub const BIG_DOUBLE: f64 = 1.0e20;
/// `SM_INFINITE_PARAMETER`: the value that marks an unbounded interval end.
pub const INFINITE_PARAMETER: f64 = 1234567.0;
/// `SM_INFINITE_PARAMETER_SQRT`: unbounded marker for representations that square it.
pub const INFINITE_PARAMETER_SQRT: f64 = 1111.111;
/// `SM_BOUNDED_INFINITE_PARAM`: half size used to approximate an unbounded interval.
pub const BOUNDED_INFINITE_PARAM: f64 = 33.0;

/// `SM_IS_INFINITE(a)`: true when `a` is `±INFINITE_PARAMETER` or
/// `±INFINITE_PARAMETER_SQRT` (exact comparison, as upstream).
#[must_use]
pub fn is_infinite_param(a: f64) -> bool {
    a == INFINITE_PARAMETER
        || a == -INFINITE_PARAMETER
        || a == INFINITE_PARAMETER_SQRT
        || a == -INFINITE_PARAMETER_SQRT
}

/// `smos_Max(a, b)`: `a > b ? a : b`.
///
/// Unlike [`f64::max`], the result for a NaN operand follows the comparison
/// (`b` when either is NaN), as upstream's macro does.
#[must_use]
pub fn max2(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `smos_Min(a, b)`: `a < b ? a : b` (NaN handling as [`max2`]).
#[must_use]
pub fn min2(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `smos_3Max(a, b, c)`, with upstream's comparison order.
#[must_use]
pub fn max3(a: f64, b: f64, c: f64) -> f64 {
    if a > b {
        if a > c {
            a
        } else {
            c
        }
    } else if b > c {
        b
    } else {
        c
    }
}

/// `smos_3Min(a, b, c)`, with upstream's comparison order.
#[must_use]
pub fn min3(a: f64, b: f64, c: f64) -> f64 {
    if a < b {
        if a < c {
            a
        } else {
            c
        }
    } else if b < c {
        b
    } else {
        c
    }
}

/// `smos_4Max(a, b, c, d)` = `smos_Max(smos_Max(a, b), smos_Max(c, d))`.
#[must_use]
pub fn max4(a: f64, b: f64, c: f64, d: f64) -> f64 {
    max2(max2(a, b), max2(c, d))
}

/// `smos_4Min(a, b, c, d)` = `smos_Min(smos_Min(a, b), smos_Min(c, d))`.
#[must_use]
pub fn min4(a: f64, b: f64, c: f64, d: f64) -> f64 {
    min2(min2(a, b), min2(c, d))
}

/// `SM_RAD2DEG(a)` = `a * (180.0 / SM_PI)`: radians to degrees.
#[must_use]
pub fn rad_to_deg(a: f64) -> f64 {
    a * (180.0 / PI)
}

/// The determinant of the 3×3 matrix of the first three entries of `m`'s rows, expanded
/// along the first row.
#[must_use]
pub(crate) fn det3(m: &[[f64; 4]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
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

    /// The rigid placement `p` as a transform: its axes' images and its origin (the
    /// z axis as the placement derives it).
    #[must_use]
    pub fn of_placement(p: &crate::geom::axis2_placement::Axis2Placement) -> Self {
        Self::from_axes(p.x_axis(), p.y_axis(), p.z_axis(), p.origin())
    }

    /// The lengths of the linear part's rows (the images of the axes), each within
    /// `gamma_4` of exact, relatively.
    #[must_use]
    pub fn row_lengths(&self) -> [f64; 3] {
        let len = |c: &[f64; 4]| (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        [len(&self.m[0]), len(&self.m[1]), len(&self.m[2])]
    }

    /// Whether the transform is a regular affine map, proved: every entry finite, the
    /// last column `(0, 0, 0, 1)`, and its linear part's conditioning, the ratio of its
    /// smallest scale to its largest (singular values `s_min / s_max`), above `ratio`.
    /// `ratio` of 0 asks only for a non-zero determinant; a uniform scale of any size has
    /// conditioning 1, a map that flattens one direction against the others (a scale of
    /// `(1e-300, 1, 1)`) conditioning that small.
    ///
    /// The conditioning is bounded below without a solve. With `r_i` the rows (the images
    /// of the axes), `F = sum |r_i|^2` and `C = sum_{i<j} |r_i|^2 |r_j|^2`:
    /// `s_max <= |A|_F = sqrt F`, and `s_max s_mid` is the largest singular value of the
    /// cofactor matrix, whose rows are the cross products `r_i x r_j` with
    /// `|r_i x r_j| <= |r_i| |r_j|`, so `s_max s_mid <= sqrt C`. Then
    /// `s_min = |det| / (s_max s_mid) >= |det| / sqrt C`, and
    /// `s_min / s_max >= |det| / sqrt(F C)`: the map is proved regular when `|det|` is
    /// proved above `ratio sqrt(F C)`. (The bound is within a factor 3 of the conditioning
    /// for a rigid motion; a stretch `(2, 3, 4)`, of conditioning 0.5, proves 0.28.)
    ///
    /// Rounding: the linear part is first scaled by one power of two, so its largest entry
    /// is in `[1, 2)` (exact, the ratio unchanged; a subnormal largest entry in two exact
    /// steps). The determinant's six products round within `gamma_5` of the sum of their
    /// sizes; `F` and `C` are sums of non-negative terms within `gamma_9` each, the root
    /// and the product with `ratio` two more roundings (`gamma_15` in all). Entries far
    /// below the largest underflow:
    /// - a scaled entry below `1e-100` (judged from the source entry, so an entry flushed
    ///   to 0 by the scaling, in a row spanning more than `2^1074`, is counted too) moves
    ///   each product of the determinant by a few subnormal units at most, which an absolute
    ///   term of 256 of them covers (the cofactors are at most 8, an entry moves by half a
    ///   unit);
    /// - a 2x2 minor that cancels to a subnormal, times an entry of at least `1e-100`, rounds
    ///   by at most half a subnormal unit, far inside `gamma_5 size` (that term of `size` is at
    ///   least `1e-300`);
    /// - `size` is itself computed: the exact one is within `(1 - u)^-5` of it, and the bound's
    ///   own sum rounds, which `gamma_5 size (1 + gamma_8)` covers;
    /// - squares below the smallest normal double move `F` and `C` by under `1e-320`
    ///   absolutely, so `sqrt(F C)` by under `1e-159` (`F` is at most 12), which the
    ///   absolute term `ratio * 1e-150` covers.
    ///
    /// Not upstream (SolidCraft's station-map check, `maps_ok`).
    #[must_use]
    pub fn is_regular(&self, ratio: f64) -> crate::geom::certified::Verdict {
        use crate::geom::certified::{gamma, Bounded, Verdict};
        let finite = self.m.iter().flatten().all(|x| x.is_finite());
        let affine = self.m[0][3] == 0.0
            && self.m[1][3] == 0.0
            && self.m[2][3] == 0.0
            && self.m[3][3] == 1.0;
        if !finite || !affine || ratio.is_nan() || ratio < 0.0 || ratio.is_infinite() {
            return Verdict::No;
        }
        let big = self
            .m
            .iter()
            .take(3)
            .flat_map(|r| r.iter().take(3))
            .fold(0.0_f64, |b, v| b.max(v.abs()));
        if big == 0.0 {
            return Verdict::No;
        }
        // One power of two for the whole linear part (exact; two steps for a subnormal
        // largest entry, whose inverse power alone would overflow).
        let pre = if big < f64::MIN_POSITIVE {
            2.0_f64.powi(600)
        } else {
            1.0
        };
        #[allow(clippy::cast_possible_truncation)] // an exponent of a finite double
        let scale = 2.0_f64.powi(-((big * pre).log2().floor() as i32));
        let mut m = [[0.0_f64; 3]; 3];
        let mut tiny_entry = false;
        for (row, src) in m.iter_mut().zip(&self.m) {
            for (r, &v) in row.iter_mut().zip(src) {
                *r = (v * pre) * scale;
                // from the source entry: one the scaling flushes to 0 counts too
                tiny_entry |= v != 0.0 && (v * pre).abs() < 1e-100 / scale;
            }
        }
        let det = m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2])
            - m[1][0] * (m[0][1] * m[2][2] - m[2][1] * m[0][2])
            + m[2][0] * (m[0][1] * m[1][2] - m[1][1] * m[0][2]);
        let size = m[0][0].abs() * ((m[1][1] * m[2][2]).abs() + (m[2][1] * m[1][2]).abs())
            + m[1][0].abs() * ((m[0][1] * m[2][2]).abs() + (m[2][1] * m[0][2]).abs())
            + m[2][0].abs() * ((m[0][1] * m[1][2]).abs() + (m[1][1] * m[0][2]).abs());
        let q = |c: &[f64; 3]| c[0] * c[0] + c[1] * c[1] + c[2] * c[2];
        let (q0, q1, q2) = (q(&m[0]), q(&m[1]), q(&m[2]));
        let f = q0 + q1 + q2;
        let c = q0 * q1 + q0 * q2 + q1 * q2;
        let floor = ratio * (f * c).sqrt();
        let value = det.abs() - floor;
        let tiny = if tiny_entry {
            256.0 * f64::from_bits(1)
        } else {
            0.0
        };
        let lower = Bounded {
            value,
            bound: gamma(5) * size * (1.0 + gamma(8))
                + gamma(15) * floor
                + ratio * 1e-150
                + gamma(1) * value.abs()
                + tiny,
        }
        .above(0.0);
        if lower == Verdict::Yes {
            return Verdict::Yes;
        }
        // Proved not regular: every product of the determinant exactly 0 (no entry
        // underflowed), or the conditioning's upper bound `|r_min| / |r_max|` (each row is
        // the image of a unit vector, between `s_min` and `s_max` long) at most `ratio`,
        // the squares' rounding and underflow counted against it.
        let (q_min, q_max) = (q0.min(q1).min(q2), q0.max(q1).max(q2));
        let flat = size == 0.0 && !tiny_entry;
        let thin =
            ratio * ratio * q_max * (1.0 - gamma(8)) - 1e-320 > q_min * (1.0 + gamma(4)) + 1e-320;
        if flat || thin {
            Verdict::No
        } else {
            Verdict::Undecided
        }
    }

    /// Carry a plane's axes by a rigid motion, preserving its UV scales and orthogonality.
    /// Source axes square to the same dot-product rounding as the plane constructor are
    /// reconstructed as an orthogonal pair after the move. Genuinely skew axes keep their
    /// parametrisation and are mapped directly. Call this only for a rigid motion.
    ///
    /// This prevents the rounding of successive rotations from accumulating into a skew
    /// plane. It does not widen the plane constructor's rounding bound.
    ///
    /// # Errors
    /// Non-finite coordinates, axes of no length, or a degenerate mapped pair.
    pub fn rigid_plane_axes(&self, u: Vec3, v: Vec3) -> crate::error::Result<[Vec3; 2]> {
        use crate::error::SolidError;
        use crate::nurbs::math::gamma;
        let (a, b) = (u.length(), v.length());
        if !(finite3(u)
            && finite3(v)
            && a > 0.0
            && b > 0.0
            && (a * b).is_finite()
            && self.m.iter().flatten().all(|x| x.is_finite()))
        {
            return Err(SolidError::invalid_input(
                OP_MAPS,
                "invalid plane axes or motion",
            ));
        }
        let (mu, mv) = (self.transform_vector(u), self.transform_vector(v));
        if !(finite3(mu) && finite3(mv)) {
            return Err(SolidError::invalid_input(
                OP_MAPS,
                "non-finite mapped plane axes",
            ));
        }
        if u.dot(v).abs() > gamma(3) * a * b {
            return Ok([mu, mv]);
        }
        let [x, y, _] = mu.make_unit_ortho_vectors(Some(mv.unitize()?))?;
        Ok([x * a, y * b])
    }

    /// Carry a plane given by three points through a rigid motion, returning its origin
    /// and axes. Keeping axes instead of reconstructing endpoints avoids cancellation when
    /// the plane's parametrisation is read back.
    ///
    /// # Errors
    /// Those of [`Self::rigid_plane_axes`], or a non-finite origin.
    pub fn rigid_plane_through(
        &self,
        origin: Point3,
        u: Point3,
        v: Point3,
    ) -> crate::error::Result<(Point3, [Vec3; 2])> {
        let axes = self.rigid_plane_axes(u - origin, v - origin)?;
        let origin = self.transform_point(origin);
        if !finite3(origin) {
            return Err(crate::error::SolidError::invalid_input(
                OP_MAPS,
                "non-finite plane origin",
            ));
        }
        Ok((origin, axes))
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

/// The operation name of the refusals of the maps built from their numbers.
const OP_MAPS: &str = "math::Transform";

/// Whether every coordinate of `v` is finite.
fn finite3(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// Not upstream: the rigid motions and scalings an application asks for by their numbers
/// (a turn about a line, a scale about a point, a mirror in a plane), built here so the
/// layers above the kernel only pass the numbers on. Each is in the row-vector convention
/// of [`Transform`]: rows 0 to 2 the images of x, y and z, row 3 the translation.
impl Transform {
    /// The linear map whose rows are `x`, `y` and `z`, about the point `fixed`, which it
    /// keeps: `p' = (p - fixed) A + fixed`, its translation `fixed - fixed A`.
    fn about(fixed: Point3, x: Vec3, y: Vec3, z: Vec3) -> Self {
        let image = x * fixed.x + y * fixed.y + z * fixed.z;
        Self::from_axes(x, y, z, fixed - image)
    }

    /// The turn by `angle` radians, right-handed about `axis`, about the line through
    /// `point` along `axis` (any length, made unit here): Rodrigues' rotation formula,
    /// `R = c I + s [k]x + (1 - c) k kᵀ` with `k` the unit axis, `c` and `s` the cosine and
    /// sine of the angle, transposed for the row-vector convention.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a point, axis or angle not finite, or an axis whose squared
    /// length is below [`EFF_ZERO_SQ`].
    pub fn rotation(point: Point3, axis: Vec3, angle: f64) -> crate::error::Result<Self> {
        use crate::error::SolidError;
        if !(finite3(point) && finite3(axis) && angle.is_finite()) {
            return Err(SolidError::invalid_input(
                OP_MAPS,
                "a turn's numbers not finite",
            ));
        }
        let k = axis
            .unitize()
            .map_err(|_| SolidError::invalid_input(OP_MAPS, "a turn's axis of no length"))?;
        let (s, c) = angle.sin_cos();
        let v = 1.0 - c;
        let (x, y, z) = (k.x, k.y, k.z);
        Ok(Self::about(
            point,
            Vec3::new(c + x * x * v, x * y * v + z * s, x * z * v - y * s),
            Vec3::new(x * y * v - z * s, c + y * y * v, y * z * v + x * s),
            Vec3::new(x * z * v + y * s, y * z * v - x * s, c + z * z * v),
        ))
    }

    /// The scale by `factor` in every direction about `centre`, which it keeps. A negative
    /// factor turns the scale through the centre as well.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a centre or factor not finite, or a factor of zero (a map with
    /// no inverse).
    pub fn uniform_scaling(centre: Point3, factor: f64) -> crate::error::Result<Self> {
        use crate::error::SolidError;
        if !(finite3(centre) && factor.is_finite()) || factor == 0.0 {
            return Err(SolidError::invalid_input(
                OP_MAPS,
                "a scale's numbers not finite, or a factor of zero",
            ));
        }
        Ok(Self::about(
            centre,
            Vec3::new(factor, 0.0, 0.0),
            Vec3::new(0.0, factor, 0.0),
            Vec3::new(0.0, 0.0, factor),
        ))
    }

    /// The mirror in the plane through `origin` square to `normal` (any length, made unit
    /// here): `p' = p - 2 ((p - origin)·n) n`, its linear part `I - 2 n nᵀ` (symmetric, so
    /// the same in either convention) and its translation `2 (origin·n) n`. For a normal
    /// along an axis every entry is exact, so the mirror twice is the identity exactly.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for an origin or normal not finite, or a normal whose squared length
    /// is below [`EFF_ZERO_SQ`].
    pub fn reflection(origin: Point3, normal: Vec3) -> crate::error::Result<Self> {
        use crate::error::SolidError;
        if !(finite3(origin) && finite3(normal)) {
            return Err(SolidError::invalid_input(
                OP_MAPS,
                "a mirror's numbers not finite",
            ));
        }
        let n = normal
            .unitize()
            .map_err(|_| SolidError::invalid_input(OP_MAPS, "a mirror's normal of no length"))?;
        let row = |e: Vec3, ni: f64| e - n * (2.0 * ni);
        Ok(Self::from_axes(
            row(Vec3::X, n.x),
            row(Vec3::Y, n.y),
            row(Vec3::Z, n.z),
            n * (2.0 * origin.dot(n)),
        ))
    }

    /// The map that undoes this one: `p = (p' - t) A⁻¹`, its linear part `A⁻¹` (the adjugate
    /// over the determinant: with `a`, `b`, `c` the rows of `A`, row `i` holds the `i`-th
    /// components of `b × c`, `c × a` and `a × b`, over `a · (b × c)`) and its translation
    /// `-t A⁻¹`. A mirror in a plane through the origin square to an axis is its own inverse
    /// exactly.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a map not proved regular ([`Transform::is_regular`] at ratio 0: an
    /// entry not finite, a fourth column that is not `(0, 0, 0, 1)`, a determinant not
    /// proved apart from 0).
    pub fn inverse(&self) -> crate::error::Result<Self> {
        if !self.is_regular(0.0).is_yes() {
            return Err(crate::error::SolidError::invalid_input(
                OP_MAPS,
                "a map not proved regular has no inverse",
            ));
        }
        let (a, b, c) = (self.x_axis(), self.y_axis(), self.z_axis());
        let (bc, ca, ab) = (b.cross(c), c.cross(a), a.cross(b));
        let s = 1.0 / a.dot(bc);
        let linear = Self::from_axes(
            Vec3::new(bc.x, ca.x, ab.x) * s,
            Vec3::new(bc.y, ca.y, ab.y) * s,
            Vec3::new(bc.z, ca.z, ab.z) * s,
            Point3::ZERO,
        );
        let t = linear.transform_vector(self.translation_part());
        Ok(Self::from_axes(
            linear.x_axis(),
            linear.y_axis(),
            linear.z_axis(),
            -t,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_rigid_turns_keep_plane_axes_square_and_keep_uv_scales() {
        use crate::nurbs::math::gamma;
        let tilt = Transform::rotation(Point3::ZERO, Vec3::X, 1.5e-6).unwrap();
        let turn = Transform::rotation(Point3::ZERO, Vec3::Z, 1_f64.to_radians()).unwrap();
        let [mut u, mut v] = tilt.rigid_plane_axes(Vec3::X * 2.0, Vec3::Y * 3.0).unwrap();
        for _ in 0..360 {
            [u, v] = turn.rigid_plane_axes(u, v).unwrap();
            assert!(u.dot(v).abs() <= gamma(3) * u.length() * v.length());
            assert!((u.length() - 2.0).abs() < 1e-12);
            assert!((v.length() - 3.0).abs() < 1e-12);
        }
    }

    #[test]
    fn a_rigid_turn_preserves_a_skew_plane_parametrisation() {
        let turn =
            Transform::rotation(Point3::ZERO, Vec3::Z, core::f64::consts::FRAC_PI_2).unwrap();
        let [u, v] = turn
            .rigid_plane_axes(Vec3::new(2.0, 0.0, 0.0), Vec3::new(1.0, 3.0, 0.0))
            .unwrap();
        assert!((u - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-14);
        assert!((v - Vec3::new(-3.0, 1.0, 0.0)).length() < 1e-14);
        assert!((u.dot(v) - 2.0).abs() < 1e-14);
    }

    /// The largest difference between two matrices' entries.
    fn apart(a: &Transform, b: &Transform) -> f64 {
        a.m.iter()
            .flatten()
            .zip(b.m.iter().flatten())
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_quarter_turn_about_z_takes_x_to_y_and_keeps_its_line() {
        use crate::geom::certified::Verdict;
        let q = core::f64::consts::FRAC_PI_2;
        let turn = Transform::rotation(Point3::ZERO, Vec3::new(0.0, 0.0, 2.0), q).unwrap();
        let want = Transform::from_axes(Vec3::Y, -Vec3::X, Vec3::Z, Point3::ZERO);
        assert!(apart(&turn, &want) <= 1e-15, "{turn:?}");
        // about the line through (10, 0, 0) along z: that point stays, (11, 0, 0) goes to
        // (10, 1, 0)
        let p = Point3::new(10.0, 0.0, 0.0);
        let turn = Transform::rotation(p, Vec3::Z, q).unwrap();
        assert!(turn.transform_point(p).distance(p) <= 1e-14);
        let image = turn.transform_point(Point3::new(11.0, 0.0, 0.0));
        assert!(
            image.distance(Point3::new(10.0, 1.0, 0.0)) <= 1e-14,
            "{image:?}"
        );
        // a turn about a skew axis keeps lengths and its axis
        let axis = Vec3::new(1.0, 2.0, 2.0);
        let turn = Transform::rotation(p, axis, 0.7).unwrap();
        assert!(
            turn.transform_point(p + axis * 3.0)
                .distance(p + axis * 3.0)
                <= 1e-12
        );
        for r in turn.row_lengths() {
            assert!((r - 1.0).abs() <= 1e-14);
        }
        assert_eq!(turn.is_regular(1e-9), Verdict::Yes);
    }

    #[test]
    fn a_scale_keeps_its_centre_and_a_mirror_twice_is_the_identity() {
        let c = Point3::new(1.0, -2.0, 3.0);
        let scale = Transform::uniform_scaling(c, 2.5).unwrap();
        assert!(scale.transform_point(c).distance(c) <= 1e-14);
        let p = scale.transform_point(c + Vec3::X);
        assert!(p.distance(c + Vec3::X * 2.5) <= 1e-14, "{p:?}");
        // the mirror in y = 5, an axis normal of any length: every entry exact
        let mirror =
            Transform::reflection(Point3::new(7.0, 5.0, -1.0), Vec3::new(0.0, 3.0, 0.0)).unwrap();
        assert_eq!(
            mirror.transform_point(Point3::new(1.0, 2.0, 3.0)),
            Point3::new(1.0, 8.0, 3.0)
        );
        assert_eq!(mirror.concat(&mirror), Transform::IDENTITY);
        // a slanted mirror keeps its plane's points and reverses its normal
        let n = Vec3::new(1.0, 1.0, 0.0);
        let mirror = Transform::reflection(c, n).unwrap();
        assert!(mirror.transform_point(c).distance(c) <= 1e-14);
        let back = mirror.transform_point(c + n);
        assert!(back.distance(c - n) <= 1e-14, "{back:?}");
    }

    #[test]
    fn a_map_after_its_inverse_is_the_identity_and_a_singular_one_has_none() {
        // a turn about a line off the origin, then a move and a scale
        let turn = Transform::rotation(Point3::new(1.0, 2.0, 3.0), Vec3::new(1.0, -2.0, 2.0), 0.7)
            .unwrap();
        let map = turn
            .concat(&Transform::translation(Vec3::new(4.0, -5.0, 6.0)))
            .concat(&Transform::uniform_scaling(Point3::new(-1.0, 0.5, 2.0), 2.5).unwrap());
        let inv = map.inverse().unwrap();
        let size = map.m.iter().flatten().fold(1.0_f64, |a, x| a.max(x.abs()));
        for (p, q) in [(map, inv), (inv, map)] {
            let back = p.concat(&q);
            assert!(
                apart(&back, &Transform::IDENTITY) <= 1e-15 * size,
                "{back:?}"
            );
        }
        // a mirror in a plane square to an axis is its own inverse, exactly
        let mirror = Transform::reflection(Point3::new(7.0, 5.0, -1.0), Vec3::Y).unwrap();
        assert_eq!(mirror.inverse().unwrap(), mirror);
        // a flattening and a projective map have none
        let mut flat = Transform::IDENTITY;
        flat.m[2] = [0.0; 4];
        let mut projective = Transform::IDENTITY;
        projective.m[0][3] = 1.0;
        for m in [flat, projective] {
            assert!(matches!(
                m.inverse(),
                Err(crate::error::SolidError::InvalidInput { operation, .. }) if operation == OP_MAPS
            ));
        }
    }

    #[test]
    fn maps_from_numbers_refuse_what_is_not_finite_or_has_no_size() {
        let bad = |r: crate::error::Result<Transform>| matches!(r, Err(crate::error::SolidError::InvalidInput { operation, .. }) if operation == OP_MAPS);
        assert!(bad(Transform::rotation(Point3::ZERO, Vec3::ZERO, 1.0)));
        assert!(bad(Transform::rotation(Point3::ZERO, Vec3::Z, f64::NAN)));
        assert!(bad(Transform::rotation(
            Point3::new(f64::INFINITY, 0.0, 0.0),
            Vec3::Z,
            1.0
        )));
        assert!(bad(Transform::uniform_scaling(Point3::ZERO, 0.0)));
        assert!(bad(Transform::uniform_scaling(Point3::ZERO, f64::NAN)));
        assert!(bad(Transform::reflection(Point3::ZERO, Vec3::ZERO)));
        assert!(bad(Transform::reflection(
            Point3::ZERO,
            Vec3::new(f64::NAN, 1.0, 0.0)
        )));
    }

    #[test]
    fn a_rigid_motion_is_regular_and_a_flattening_is_not() {
        use crate::geom::certified::Verdict;
        let turn = Transform::from_axes(
            Vec3::new(0.6, 0.8, 0.0),
            Vec3::new(-0.8, 0.6, 0.0),
            Vec3::Z,
            Vec3::new(1.0, 2.0, 3.0),
        );
        assert_eq!(turn.is_regular(1e-9), Verdict::Yes);
        // Scaled by 2, 3, 4: conditioning 0.5; the lower bound proves 24 / sqrt(29 * 244)
        // = 0.285, the rows' lengths cap it at 2 / 4.
        let stretch = Transform::from_axes(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::new(0.0, 0.0, 4.0),
            Vec3::ZERO,
        );
        assert_eq!(stretch.is_regular(0.28), Verdict::Yes);
        assert_eq!(stretch.is_regular(0.29), Verdict::Undecided);
        assert_eq!(stretch.is_regular(0.5), Verdict::Undecided);
        assert_eq!(stretch.is_regular(0.51), Verdict::No);
        // The identity's bound is 1 / 3 exactly: not proved above it.
        assert_eq!(
            Transform::IDENTITY.is_regular(1.0 / 3.0),
            Verdict::Undecided
        );
        // Every product of its determinant is exactly 0: proved singular.
        let flat = Transform::from_axes(Vec3::X, Vec3::Y, Vec3::new(1.0, 1.0, 0.0), Vec3::ZERO);
        assert_eq!(flat.is_regular(0.0), Verdict::No);
        assert_eq!(flat.is_regular(1e-9), Verdict::No);
        // Nearly flat: a determinant of 1e-300 rounds relatively, so it is proved non-zero.
        let thin = Transform::from_axes(Vec3::X, Vec3::Y, Vec3::new(0.3, 0.7, 1e-300), Vec3::ZERO);
        assert_eq!(thin.is_regular(0.0), Verdict::Yes);
        let mut nan = Transform::IDENTITY;
        nan.m[3][0] = f64::NAN;
        assert_eq!(nan.is_regular(0.0), Verdict::No);
        let mut projective = Transform::IDENTITY;
        projective.m[0][3] = 1.0;
        assert_eq!(projective.is_regular(0.0), Verdict::No);
        let mut zero_row = Transform::IDENTITY;
        zero_row.m[1] = [0.0; 4];
        assert_eq!(zero_row.is_regular(0.0), Verdict::No);
    }

    /// A map that flattens one direction against the others is not regular however its
    /// determinant compares with its rows' lengths (each row's scale apart, `(1e-300, 1, 1)`
    /// was proved regular); one that flattens by less than the ratio is.
    #[test]
    fn a_flattening_against_the_other_directions_is_refused() {
        use crate::geom::certified::Verdict;
        let diag = |a: f64, b: f64, c: f64| {
            Transform::from_axes(
                Vec3::new(a, 0.0, 0.0),
                Vec3::new(0.0, b, 0.0),
                Vec3::new(0.0, 0.0, c),
                Vec3::ZERO,
            )
        };
        assert_eq!(diag(1e-300, 1.0, 1.0).is_regular(1e-9), Verdict::No);
        assert_eq!(diag(1.0, 1e-10, 1.0).is_regular(1e-9), Verdict::No);
        assert_eq!(diag(1.0, 1.0, 1e-5).is_regular(1e-9), Verdict::Yes);
        // A row spanning more than 2^1022: the scaling flushes its small entry and the other
        // rows' products underflow; never a wrong answer (conditioning about 1e-300).
        let wide =
            Transform::from_axes(Vec3::new(1e300, 1e-300, 0.0), Vec3::Y, Vec3::Z, Vec3::ZERO);
        assert_eq!(wide.is_regular(1e-9), Verdict::No);
        assert_ne!(wide.is_regular(0.0), Verdict::No);
    }

    /// A uniform scale is regular however small or large, subnormal included: its
    /// conditioning is 1, which no underflow or overflow of the products may hide (a
    /// subnormal scale was `Undecided`: its row scale overflowed).
    #[test]
    fn a_tiny_or_huge_uniform_scale_is_regular() {
        for s in [1e-110, 1e110, 1e-300, 1e300, 1e-310, f64::from_bits(1)] {
            let mut m = Transform::IDENTITY;
            for (i, row) in m.m.iter_mut().enumerate().take(3) {
                row[i] = s;
            }
            assert_eq!(
                m.is_regular(1e-9),
                crate::geom::certified::Verdict::Yes,
                "{s}"
            );
        }
    }

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
