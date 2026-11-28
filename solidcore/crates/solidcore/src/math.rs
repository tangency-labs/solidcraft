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

    /// Maps a list of point coordinates in place, preserving their order.
    /// Each point uses [`Self::transform_point`]'s row-vector convention and rounding.
    pub fn transform_points(&self, points: &mut [[f64; 3]]) {
        for point in points {
            let [x, y, z] = *point;
            let image = self.transform_point(Point3::new(x, y, z));
            *point = [image.x, image.y, image.z];
        }
    }

    /// The axis-aligned box around the eight corners of the box `lo`..`hi` mapped by
    /// [`Self::transform_point`]: it holds the exact image of every point of the box, and for
    /// a map that turns it is larger than the image itself. An empty box (`lo` above `hi` on
    /// any axis) maps to itself.
    ///
    /// Rounding: each mapped coordinate `t + x a + y b + z c` is four terms summed in order,
    /// within `gamma_4` of the sum of their sizes; the sizes' sum is itself rounded, so each
    /// side moves out by `gamma_5` of it, plus the smallest normal number for products that
    /// underflow, and then by one more double for the subtraction or addition.
    #[must_use]
    pub fn transformed_box(&self, lo: Point3, hi: Point3) -> (Point3, Point3) {
        use crate::geom::certified::{next_down, next_up};
        use crate::nurbs::math::gamma;
        if lo.x > hi.x || lo.y > hi.y || lo.z > hi.z {
            return (lo, hi);
        }
        let (tx, ty, tz, td) = (
            self.x_axis(),
            self.y_axis(),
            self.z_axis(),
            self.translation_part(),
        );
        let g = gamma(5);
        let mut min = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut max = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for i in 0..8 {
            let corner = Point3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let p = self.transform_point(corner);
            let size = |t: f64, a: f64, b: f64, c: f64| {
                g * (t.abs() + (corner.x * a).abs() + (corner.y * b).abs() + (corner.z * c).abs())
                    + f64::MIN_POSITIVE
            };
            let e = Vec3::new(
                size(td.x, tx.x, ty.x, tz.x),
                size(td.y, tx.y, ty.y, tz.y),
                size(td.z, tx.z, ty.z, tz.z),
            );
            min = Vec3::new(
                min.x.min(next_down(p.x - e.x)),
                min.y.min(next_down(p.y - e.y)),
                min.z.min(next_down(p.z - e.z)),
            );
            max = Vec3::new(
                max.x.max(next_up(p.x + e.x)),
                max.y.max(next_up(p.y + e.y)),
                max.z.max(next_up(p.z + e.z)),
            );
        }
        (min, max)
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

    /// Whether this finite affine map is a rotation or reflection and a translation.
    /// Axis lengths and mutual dot products use `axis_tol`; the homogeneous entries use
    /// `affine_tol`. Both tolerances must be finite and positive.
    #[must_use]
    pub fn is_rigid(&self, axis_tol: f64, affine_tol: f64) -> bool {
        let [x, y, z, t] = &self.m;
        let (a, b, c) = (self.x_axis(), self.y_axis(), self.z_axis());
        axis_tol.is_finite()
            && axis_tol > 0.0
            && affine_tol.is_finite()
            && affine_tol > 0.0
            && self.m.iter().flatten().all(|v| v.is_finite())
            && [x, y, z].iter().all(|r| r[3].abs() < affine_tol)
            && (t[3] - 1.0).abs() < affine_tol
            && [a, b, c]
                .iter()
                .all(|v| (v.length() - 1.0).abs() < axis_tol)
            && a.dot(b).abs() < axis_tol
            && b.dot(c).abs() < axis_tol
            && c.dot(a).abs() < axis_tol
    }

    /// The orientation-preserving 2D map induced on an orthonormal plane frame.
    /// Translation normal to the plane is discarded, as for an orthogonal projection.
    /// Returns none for an identity within `identity_tol`, a changed normal, or a 2D
    /// determinant differing from one by at least `tol`. The caller establishes rigidity.
    #[must_use]
    pub fn plane_motion(
        &self,
        origin: Point3,
        x: Vec3,
        y: Vec3,
        tol: f64,
        identity_tol: f64,
    ) -> Option<([[f64; 2]; 2], Vec2)> {
        if !(tol.is_finite()
            && tol > 0.0
            && identity_tol.is_finite()
            && identity_tol > 0.0
            && finite3(origin)
            && finite3(x)
            && finite3(y)
            && self.m.iter().flatten().all(|v| v.is_finite()))
            || self
                .m
                .iter()
                .flatten()
                .zip(Self::IDENTITY.m.iter().flatten())
                .all(|(a, b)| (a - b).abs() < identity_tol)
        {
            return None;
        }
        let n = x.cross(y);
        if (self.transform_vector(n) - n).length() > tol {
            return None;
        }
        let (rx, ry) = (self.transform_vector(x), self.transform_vector(y));
        let shift = self.transform_point(origin) - origin;
        let a = [[rx.dot(x), ry.dot(x)], [rx.dot(y), ry.dot(y)]];
        let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
        let b = Vec2::new(shift.dot(x), shift.dot(y));
        ((det - 1.0).abs() < tol && b.x.is_finite() && b.y.is_finite()).then_some((a, b))
    }

    /// The unit, orthogonal frame axes nearest to `x` and `y`: `x` normalized, and `y` made
    /// perpendicular to it and normalized (the kernel picks a perpendicular when `y` is parallel to
    /// `x` or has no length). Restores a frame carried through a rigid motion to exactly unit and
    /// orthogonal axes.
    ///
    /// # Errors
    /// Non-finite coordinates or an `x` of no length.
    pub fn orthonormal_frame(x: Vec3, y: Vec3) -> crate::error::Result<[Vec3; 2]> {
        let [x, y, _] = x.make_unit_ortho_vectors(Some(y))?;
        Ok([x, y])
    }

    /// Not upstream: plane axes `x` and `y` with each coordinate smaller than `noise` dropped,
    /// each made unit again. A flat face's plane read back from a body comes tilted by rounding
    /// (about 2e-15); a cut by that plane meets the face only nearly, where the same plane with
    /// the rounding dropped meets it exactly. Axes with nothing to drop come back unit and
    /// otherwise unchanged. `None` when a coordinate or `noise` is not finite, `noise` is
    /// negative, an axis has no length once cleaned, or the cleaned axes are parallel: the
    /// caller then keeps the axes it has.
    #[must_use]
    pub fn axis_squared_frame(x: Vec3, y: Vec3, noise: f64) -> Option<[Vec3; 2]> {
        if !(finite3(x) && finite3(y) && noise.is_finite() && noise >= 0.0) {
            return None;
        }
        let clean = |v: Vec3| {
            let c = |a: f64| if a.abs() < noise { 0.0 } else { a };
            Vec3::new(c(v.x), c(v.y), c(v.z)).unitize().ok()
        };
        let (x, y) = (clean(x)?, clean(y)?);
        x.cross(y).unitize().ok()?;
        Some([x, y])
    }

    /// Not upstream: the axes `x` and `y` of a frame whose normal runs along `d`: `x` is the unit
    /// cross product of `v`'s part square to `d` with `d`, and `y = d × x`; so `x`, `y` and `d` are
    /// unit and square to each other within a few ulps for any `v` taken, `x × y` runs along `d`,
    /// and `y` runs along `v`'s part square to `d`. `d` and `v` are made unit first, at any finite
    /// length (nothing is squared before it is scaled). `None` when a coordinate is not finite, `d`
    /// or `v` has no length, or `v`'s part square to `d` is no longer than its own rounding
    /// ([`FRAME_LEAN`]): `y` would then follow the rounding, not `v`. Otherwise `y`'s direction is
    /// off `v`'s part square to `d` by about `4ε / sin`, `sin` the sine of the angle between `v`
    /// and `d`; a caller that needs it closer compares that sine itself.
    #[must_use]
    pub fn frame_axes(d: Vec3, v: Vec3) -> Option<[Vec3; 2]> {
        if !(finite3(d) && finite3(v)) {
            return None;
        }
        let (_, d) = d.length_and_unit()?;
        let (_, v) = v.length_and_unit()?;
        let (square, y) = (v - d * v.dot(d)).length_and_unit()?;
        if square <= FRAME_LEAN {
            return None;
        }
        let (_, x) = y.cross(d).length_and_unit()?;
        Some([x, d.cross(x)])
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

    /// Map a point on a line and its direction. Translation affects only the point.
    #[must_use]
    pub fn transform_line(&self, point: Point3, direction: Vec3) -> (Point3, Vec3) {
        (
            self.transform_point(point),
            self.transform_vector(direction),
        )
    }

    /// Map an undirected axis to this frame and orient its unit direction here.
    ///
    /// # Errors
    /// Refuses a non-finite point or a direction with length at most 1e-9.
    pub fn transform_canonical_axis(
        &self,
        point: Point3,
        direction: Vec3,
    ) -> crate::Result<(Point3, Vec3)> {
        let (point, direction) = self.transform_line(point, direction);
        let length = direction.length();
        if !finite3(point) || !length.is_finite() || length <= 1e-9 {
            return Err(crate::SolidError::InvalidInput {
                operation: OP_MAPS,
                reason: "the axis has no finite point and direction",
            });
        }
        Ok((point, (direction / length).canonical_axis_direction()))
    }

    /// Not upstream: how far the linear part of this map (its rows 0 to 2, no translation) takes
    /// the point `pivot` from where it was, as the move it adds to the map's turn about `pivot`:
    /// `pivot - transform_vector(pivot)`. A turn about the origin followed by this move is the
    /// same turn about `pivot`.
    #[must_use]
    pub fn pivot_displacement(&self, pivot: Vec3) -> Vec3 {
        pivot - self.transform_vector(pivot)
    }
}

/// The operation name of the refusals of the maps built from their numbers.
const OP_MAPS: &str = "math::Transform";

/// The remainder [`Transform::frame_axes`] refuses: taking the unit `v`'s part along the unit `d`
/// out leaves a remainder of length `sin` (the sine of the angle between them) but for its
/// rounding. For a `v` along `d` (or against it) the computed remainder is at most about 10 ulps:
/// `|1 - |d|²|` for `d` unit within 2 ulps (4), the three-term `v · d` (`γ₃`, 3), and the product,
/// the difference and the length (an ulp each). So a remainder no longer than 16 ulps is
/// rounding, not `v`, and gives no direction (brett19 on #22066).
pub const FRAME_LEAN: f64 = 16.0 * f64::EPSILON;

/// Whether every coordinate of `v` is finite.
fn finite3(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// The sine of the angle to a plane's normal below which [`plane_onto_line`] takes a line as
/// square to the plane: nearer than that, which way the plane tilts would come from rounding in
/// the line's direction.
pub const PLANE_ONTO_LINE_SQUARE_SIN: f64 = 1e-6;

/// Not upstream: the plane through `origin` with the unit, square axes `x` and `y` brought onto
/// the line through `at` along the unit direction `dir`, as a plane at an angle through an axis
/// starts from its base: tilted by the least turn that makes it parallel to the line (about the
/// axis square to both its normal `x × y` and the line), then moved along its normal until it
/// holds the line. A plane that already holds the line comes back unchanged. Returns the new
/// origin and axes; `None` for a number not finite or a line within
/// [`PLANE_ONTO_LINE_SQUARE_SIN`] of the normal, which no tilt makes the plane hold.
#[must_use]
pub fn plane_onto_line(
    origin: Point3,
    x: Vec3,
    y: Vec3,
    at: Point3,
    dir: Vec3,
) -> Option<(Point3, Vec3, Vec3)> {
    if ![origin, x, y, at, dir].into_iter().all(finite3) {
        return None;
    }
    let n = x.cross(y);
    let along = n - dir * dir.dot(n);
    if along.length() <= PLANE_ONTO_LINE_SQUARE_SIN {
        return None;
    }
    let to = along.unitize().ok()?;
    let k = n.cross(to);
    let (sin, cos) = (k.length(), n.dot(to));
    let (x, y) = match k.unitize() {
        Ok(k) if sin > 1e-12 => {
            let tilt = |v: Vec3| v * cos + k.cross(v) * sin + k * (k.dot(v) * (1.0 - cos));
            (tilt(x), tilt(y))
        }
        _ => (x, y),
    };
    Some((origin - to * to.dot(origin - at), x, y))
}

/// Not upstream: the corners of a parallelogram on a plane, such as a reference image pinned to
/// points of a sketch. Its bottom edge runs from `bl` to `br`; its top-left corner is
/// `top_left` when the shape may stretch, and otherwise lies `aspect` (height over
/// width) times the bottom edge's length from `bl`, square to the edge and on its left looking
/// down `normal` (a unit normal of the plane). Returns bottom-left, bottom-right, top-right and
/// top-left; `None` when the corners give no area: an edge not finite or shorter than a
/// nanometre, or the two edges within a microradian of one line.
#[must_use]
pub fn rect_of_aspect(
    bl: Point3,
    br: Point3,
    top_left: Option<Point3>,
    normal: Vec3,
    aspect: f64,
) -> Option<[Point3; 4]> {
    let along = br - bl;
    let up = match top_left {
        Some(tl) => tl - bl,
        None => normal.cross(along) * aspect,
    };
    let (width, height) = (along.length(), up.length());
    let ok = width > 1e-9
        && height > 1e-9
        && width.is_finite()
        && height.is_finite()
        && along.cross(up).length() > 1e-6 * width * height;
    if !ok {
        return None;
    }
    Some([bl, br, br + up, bl + up])
}

/// Not upstream: how a rectangle `aspect` (height over width) times as high as wide fits a frame
/// pinned at `bl`, `br` and `tl`: its width (the bottom edge's length), and whether `tl` lies off
/// the rectangle's own top-left corner, square to the bottom edge on the frame's plane, by more
/// than a millionth of (1 + its larger side): the frame stretches it. `None` when the frame gives
/// no area, as [`rect_of_aspect`] refuses it.
#[must_use]
pub fn fit_aspect(bl: Point3, br: Point3, tl: Point3, aspect: f64) -> Option<(f64, bool)> {
    let (along, up) = (br - bl, tl - bl);
    let normal = along.cross(up).unitize().ok()?;
    rect_of_aspect(bl, br, Some(tl), normal, aspect)?;
    let width = along.length();
    let height = width * aspect;
    let own = bl + normal.cross(along) * aspect;
    let stretched = (tl - own).length() > 1e-6 * (1.0 + width.max(height));
    Some((width, stretched))
}

/// Not upstream: a quad offset from its own plane on both sides, such as a decal drawn just off
/// the face it lies on so the face never hides it. `corners` are bottom-left, bottom-right,
/// top-right and top-left; the normal is the unit cross product of the bottom edge with the left
/// one. Returns that normal, the corners moved `lift` along it and the corners moved `lift`
/// against it; `None` for a coordinate or lift not finite, or corners whose edges give no normal.
#[must_use]
pub fn offset_quad(corners: [Point3; 4], lift: f64) -> Option<(Vec3, [Point3; 4], [Point3; 4])> {
    if !(corners.iter().all(|p| finite3(*p)) && lift.is_finite()) {
        return None;
    }
    let [bl, br, _, tl] = corners;
    let normal = (br - bl).cross(tl - bl).unitize().ok()?;
    let off = normal * lift;
    Some((normal, corners.map(|p| p + off), corners.map(|p| p - off)))
}

/// Not upstream: the point `s` along the edge `u` and `t` along the edge `v` of the
/// parallelogram with a corner at `corner`, `corner + s u + t v`, as a picture's texture
/// coordinates place a point on the quad it is drawn on.
#[must_use]
pub fn parallelogram_point(corner: Point3, u: Vec3, v: Vec3, s: f64, t: f64) -> Point3 {
    corner + u * s + v * t
}

/// Why [`box_extent_along`], [`axis_along_own_axes`] and [`plane_offset_square_to`] refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisRefusal {
    /// A number of the map, the axis or the box is not finite.
    NotFinite,
    /// The axis has no direction (it is zero, or too short to unitize).
    ZeroAxis,
    /// The map flattens space, so it has no inverse.
    FlatMap,
}

/// `axis` (a direction in the outer frame) as the placed body's own frame sees it, unit: the
/// linear part of the inverse of `placement` applied to it, unitized.
fn own_direction(placement: &Transform, axis: Vec3) -> Result<Vec3, AxisRefusal> {
    if !placement.m.iter().flatten().all(|v| v.is_finite()) || !finite3(axis) {
        return Err(AxisRefusal::NotFinite);
    }
    let inverse = placement.inverse().map_err(|_| AxisRefusal::FlatMap)?;
    let own = inverse
        .transform_vector(axis)
        .unitize()
        .map_err(|_| AxisRefusal::ZeroAxis)?;
    if finite3(own) {
        Ok(own)
    } else {
        Err(AxisRefusal::NotFinite)
    }
}

/// Not upstream: how far a body's box `min`..`max` (in the body's own frame), placed in the outer
/// frame by `placement`, reaches along the outer direction `axis` (any length): the box's
/// [`crate::geom::extent3d::Extent3d::span_along`] along the unit direction that `axis` is in the
/// body's own frame, high less low. Tight when `axis` runs along one of the body's own axes
/// ([`axis_along_own_axes`]).
///
/// # Errors
/// [`AxisRefusal`]: a number not finite (also the box's, or an extent not finite), a zero
/// `axis`, or a `placement` that flattens space.
pub fn box_extent_along(
    min: Point3,
    max: Point3,
    placement: &Transform,
    axis: Vec3,
) -> Result<f64, AxisRefusal> {
    let own = own_direction(placement, axis)?;
    if !finite3(min) || !finite3(max) {
        return Err(AxisRefusal::NotFinite);
    }
    let [low, high] =
        crate::geom::extent3d::Extent3d::from_bounds(min.x, min.y, min.z, max.x, max.y, max.z)
            .span_along(Point3::ZERO, own);
    let extent = high - low;
    if extent.is_finite() {
        Ok(extent)
    } else {
        Err(AxisRefusal::NotFinite)
    }
}

/// Not upstream: whether the outer direction `axis` runs along one of the own axes of a body
/// placed by `placement` (so a box of the body is tight along it): the unit direction `axis` is
/// in the body's own frame has a component of at least `1 - 1e-9` in size.
///
/// # Errors
/// [`AxisRefusal`] for a number not finite, a zero `axis`, or a `placement` that flattens space.
pub fn axis_along_own_axes(placement: &Transform, axis: Vec3) -> Result<bool, AxisRefusal> {
    let own = own_direction(placement, axis)?;
    Ok([own.x, own.y, own.z].iter().any(|c| c.abs() >= 1.0 - 1e-9))
}

/// Not upstream: where a flat face, a plane through `origin` with normal `normal` in a body's own
/// frame, cuts the outer direction `axis` once the body is placed by `placement`, if it is square
/// to it: `Some` of the offset of the placed `origin` along the unit `axis` from the outer
/// origin when the placed normal is parallel to `axis` (the unit vectors' cross product no
/// longer than `1e-9`), `None` when it is not, or when `origin` or `normal` is not finite or the
/// placed normal has no direction.
///
/// # Errors
/// [`AxisRefusal`] for a `placement` or `axis` with a number not finite, or a zero `axis`.
pub fn plane_offset_square_to(
    placement: &Transform,
    origin: Point3,
    normal: Vec3,
    axis: Vec3,
) -> Result<Option<f64>, AxisRefusal> {
    if !placement.m.iter().flatten().all(|v| v.is_finite()) || !finite3(axis) {
        return Err(AxisRefusal::NotFinite);
    }
    let axis = axis.unitize().map_err(|_| AxisRefusal::ZeroAxis)?;
    if !finite3(origin) || !finite3(normal) {
        return Ok(None);
    }
    let Ok(placed) = placement.transform_vector(normal).unitize() else {
        return Ok(None);
    };
    if placed.cross(axis).length() > 1e-9 {
        return Ok(None);
    }
    Ok(Some(
        placement
            .transform_point(origin)
            .distance_along(Point3::ZERO, axis),
    ))
}

/// The operation name of the refusals of [`centroid_and_mean_radius`].
const OP_CENTROID: &str = "math::centroid_and_mean_radius";

/// A point set's arithmetic centroid and mean Euclidean distance from that centroid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointSetSummary {
    /// The arithmetic mean of the input points.
    pub centroid: Point2,
    /// The arithmetic mean of each point's distance from `centroid`.
    pub mean_distance: f64,
}

/// Not upstream: the arithmetic centroid and mean radial distance of a non-empty point set.
///
/// # Errors
/// [`crate::error::SolidError::InvalidInput`] for an empty set, a non-finite point, or a
/// non-finite intermediate or result.
pub fn centroid_and_mean_radius(points: &[Point2]) -> crate::error::Result<PointSetSummary> {
    use crate::error::SolidError;
    if points.is_empty() {
        return Err(SolidError::invalid_input(OP_CENTROID, "empty point set"));
    }
    if points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err(SolidError::invalid_input(OP_CENTROID, "non-finite point"));
    }

    let (mut x, mut y) = (0.0, 0.0);
    for point in points {
        x += point.x;
        y += point.y;
        if !x.is_finite() || !y.is_finite() {
            return Err(SolidError::invalid_input(
                OP_CENTROID,
                "point centroid overflows",
            ));
        }
    }
    let count = points.len() as f64;
    let centroid = Point2::new(x / count, y / count);
    if !centroid.x.is_finite() || !centroid.y.is_finite() {
        return Err(SolidError::invalid_input(
            OP_CENTROID,
            "point centroid is not finite",
        ));
    }

    let mut total_distance = 0.0;
    for point in points {
        let distance = point.distance(centroid);
        if !distance.is_finite() {
            return Err(SolidError::invalid_input(
                OP_CENTROID,
                "point distance overflows",
            ));
        }
        total_distance += distance;
        if !total_distance.is_finite() {
            return Err(SolidError::invalid_input(
                OP_CENTROID,
                "mean point distance overflows",
            ));
        }
    }
    let mean_distance = total_distance / count;
    if !mean_distance.is_finite() {
        return Err(SolidError::invalid_input(
            OP_CENTROID,
            "mean point distance is not finite",
        ));
    }
    Ok(PointSetSummary {
        centroid,
        mean_distance,
    })
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

    /// The rigid map from world coordinates into the frame of the plane through `origin` with
    /// x axis along `x` and y axis on `y`'s side in the plane of the two (each any length):
    /// the unit axes `x̂`, `n = unit(x̂ × y)` and `ŷ = n × x̂` become X, Z and Y, and `origin`
    /// the origin. A point's Z after the map is its level above the plane along its normal.
    /// The linear part's columns are `x̂`, `ŷ` and `n` (the transpose of the frame's rows),
    /// its translation `-(origin·x̂, origin·ŷ, origin·n)`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for an origin or axis not finite, or an `x` or `x̂ × y` whose squared
    /// length is below [`EFF_ZERO_SQ`] (an axis of no length, or axes along one line).
    pub fn into_plane_frame(origin: Point3, x: Vec3, y: Vec3) -> crate::error::Result<Self> {
        use crate::error::SolidError;
        if !(finite3(origin) && finite3(x) && finite3(y)) {
            return Err(SolidError::invalid_input(
                OP_MAPS,
                "a plane's numbers not finite",
            ));
        }
        let none =
            |_| SolidError::invalid_input(OP_MAPS, "a plane's axes of no length or along one line");
        let x = x.unitize().map_err(none)?;
        let n = x.cross(y).unitize().map_err(none)?;
        let y = n.cross(x);
        Ok(Self::from_axes(
            Vec3::new(x.x, y.x, n.x),
            Vec3::new(x.y, y.y, n.y),
            Vec3::new(x.z, y.z, n.z),
            Vec3::new(-origin.dot(x), -origin.dot(y), -origin.dot(n)),
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

// ---- Not upstream: the 2D arithmetic of a sketch's arc snaps -------------------------------
//
// Plain `[f64; 2]` points and directions, for the applications' arc tools: the circle through
// three points, the arc a typed radius or sweep gives, the tangent arc, the perpendicular
// bisector. Each is written in the order the application wrote it, so a move changes no result.

/// `a - b`.
fn sub_2d([ax, ay]: [f64; 2], [bx, by]: [f64; 2]) -> [f64; 2] {
    [ax - bx, ay - by]
}

/// The dot product.
fn dot_2d([ax, ay]: [f64; 2], [bx, by]: [f64; 2]) -> f64 {
    ax * bx + ay * by
}

/// The length.
fn length_2d([x, y]: [f64; 2]) -> f64 {
    x.hypot(y)
}

/// `v` turned a quarter turn counter-clockwise.
fn left_2d([x, y]: [f64; 2]) -> [f64; 2] {
    [-y, x]
}

/// `at + along * k`.
fn along_2d([x, y]: [f64; 2], [dx, dy]: [f64; 2], k: f64) -> [f64; 2] {
    [x + dx * k, y + dy * k]
}

/// The point halfway between `a` and `b`.
fn midpoint_2d([ax, ay]: [f64; 2], [bx, by]: [f64; 2]) -> [f64; 2] {
    [(ax + bx) * 0.5, (ay + by) * 0.5]
}

/// Not upstream: `v` made unit, `None` for a vector no longer than `1e-9` or not finite.
#[must_use]
pub fn unit_2d(v: [f64; 2]) -> Option<[f64; 2]> {
    let [x, y] = v;
    let length = length_2d(v);
    (length > 1e-9 && length.is_finite()).then_some([x / length, y / length])
}

/// Not upstream: the unit direction from `from` to `to`, `None` where they coincide.
#[must_use]
pub fn unit_between_2d(from: [f64; 2], to: [f64; 2]) -> Option<[f64; 2]> {
    unit_2d(sub_2d(to, from))
}

/// Not upstream: the unit direction from `from` to `to` turned a quarter turn counter-clockwise,
/// `None` where they coincide.
#[must_use]
pub fn left_normal_between_2d(from: [f64; 2], to: [f64; 2]) -> Option<[f64; 2]> {
    unit_between_2d(from, to).map(left_2d)
}

/// Not upstream: the centre and radius of the circle through `from` and `other` that touches the
/// unit direction `dir` at `from`. `None` when the points are within `1e-9` or `dir` runs along the
/// chord (no such circle).
#[must_use]
pub fn tangent_arc_centre_2d(
    from: [f64; 2],
    other: [f64; 2],
    dir: [f64; 2],
) -> Option<([f64; 2], f64)> {
    let w = sub_2d(from, other);
    let length = length_2d(w);
    if length <= 1e-9 {
        return None;
    }
    // The centre lies on the normal at `from`, as far from it as it is from `other`:
    // |from + n t - other|^2 = t^2.
    let n = left_2d(dir);
    let k = dot_2d(n, w);
    if k.abs() <= 1e-9 * length {
        return None;
    }
    let t = -dot_2d(w, w) / (2.0 * k);
    Some((along_2d(from, n, t), t.abs()))
}

/// Not upstream: the point of the perpendicular bisector of the chord `start`-`end` nearest `p`
/// (the centre of the arc through both ends that has it). `None` when the ends coincide.
#[must_use]
pub fn bisector_centre_2d(start: [f64; 2], end: [f64; 2], p: [f64; 2]) -> Option<[f64; 2]> {
    let mid = midpoint_2d(start, end);
    let n = left_2d(unit_between_2d(start, end)?);
    Some(along_2d(mid, n, dot_2d(n, sub_2d(p, mid))))
}

/// Not upstream: whether the unit direction `dir` at `p` is square to the radius from `centre`
/// (within a millionth). `false` where `p` is on `centre`.
#[must_use]
pub fn is_square_to_radius_2d(centre: [f64; 2], p: [f64; 2], dir: [f64; 2]) -> bool {
    unit_between_2d(centre, p).is_some_and(|radial| dot_2d(dir, radial).abs() <= 1e-6)
}

/// Not upstream: whether `p` lies on the line through `origin` along `dir`, within `tol` of it.
/// `false` where `dir` is of no length or not finite.
#[must_use]
pub fn on_line_2d(p: [f64; 2], origin: [f64; 2], dir: [f64; 2], tol: f64) -> bool {
    unit_2d(dir).is_some_and(|u| dot_2d(left_2d(u), sub_2d(p, origin)).abs() <= tol)
}

/// Not upstream: the centre of the circle through `a`, `b` and `c`. `None` for points on one
/// line (or not finite).
#[must_use]
pub fn circumcentre_2d(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 2]> {
    let [abx, aby] = sub_2d(b, a);
    let [acx, acy] = sub_2d(c, a);
    let ab2 = abx * abx + aby * aby;
    let ac2 = acx * acx + acy * acy;
    let d = 2.0 * (abx * acy - aby * acx);
    if !d.is_finite() || d.abs() < 1e-12 {
        return None;
    }
    let [ax, ay] = a;
    let centre = [
        ax + (acy * ab2 - aby * ac2) / d,
        ay + (abx * ac2 - acx * ab2) / d,
    ];
    centre.iter().all(|v| v.is_finite()).then_some(centre)
}

/// Not upstream: the circle through `s`, `e` and `t`: its centre, radius and the sweep (radians)
/// of the arc from `s` to `e` that passes through `t`. `None` for points on one line, a radius
/// under `1e-9` or coincident ends.
#[must_use]
pub fn three_point_circle_2d(
    s: [f64; 2],
    e: [f64; 2],
    t: [f64; 2],
) -> Option<([f64; 2], f64, f64)> {
    let centre = circumcentre_2d(s, t, e)?;
    let r = length_2d(sub_2d(centre, s));
    let chord = sub_2d(e, s);
    let n = left_2d(unit_2d(chord)?);
    if !r.is_finite() || r <= 1e-9 {
        return None;
    }
    let mid = midpoint_2d(s, e);
    let minor = 2.0 * (length_2d(chord) / (2.0 * r)).clamp(-1.0, 1.0).asin();
    // The arc through `t` is the long one when `t` is on the centre's side of the chord.
    let long = dot_2d(n, sub_2d(t, mid)) * dot_2d(n, sub_2d(centre, mid)) > 0.0;
    Some((
        centre,
        r,
        if long {
            core::f64::consts::TAU - minor
        } else {
            minor
        },
    ))
}

/// Not upstream: the third point of the arc from `s` to `e` for the cursor `h` with a radius or a
/// sweep (radians; the sweep wins when both are given): on the arc of that size whose middle is
/// nearest the cursor. A radius picks among the minor and major arcs on either side of the chord;
/// a sweep takes the cursor's side. `None`: no such arc (a radius under half the chord, a sweep
/// outside `(0, 2 pi)`, neither given, or coincident ends).
#[must_use]
pub fn arc_third_point_2d(
    s: [f64; 2],
    e: [f64; 2],
    h: [f64; 2],
    radius: Option<f64>,
    sweep: Option<f64>,
) -> Option<[f64; 2]> {
    let chord = sub_2d(e, s);
    let len = length_2d(chord);
    let n = left_2d(unit_2d(chord)?);
    let mid = midpoint_2d(s, e);
    if let Some(sweep) = sweep {
        if !(sweep > 1e-9 && sweep < core::f64::consts::TAU - 1e-9) {
            return None;
        }
        let half = sweep * 0.5;
        let r = len / (2.0 * half.sin());
        let side = if dot_2d(n, sub_2d(h, mid)) < 0.0 {
            -1.0
        } else {
            1.0
        };
        return Some(along_2d(
            mid,
            n,
            side * r * 2.0 * (half * 0.5).sin().powi(2),
        ));
    }
    let r = radius?;
    if !r.is_finite() || r * 2.0 < len - 1e-9 {
        return None;
    }
    let k = (r * r - len * len * 0.25).max(0.0).sqrt();
    [k + r, k - r, r - k, -k - r]
        .into_iter()
        .map(|d| along_2d(mid, n, d))
        .filter(|p| p.iter().all(|v| v.is_finite()))
        .min_by(|x, y| length_2d(sub_2d(*x, h)).total_cmp(&length_2d(sub_2d(*y, h))))
}

/// Not upstream: the distance between two points.
#[must_use]
pub fn distance_2d(a: [f64; 2], b: [f64; 2]) -> f64 {
    length_2d(sub_2d(b, a))
}

/// Not upstream: the side of the line from `p` to `q` the region lies on that is on the left
/// of the piece from `a` to `b` (a piece of a counter-clockwise loop lying on that line):
/// `Some(true)` left of the line seen from `p` towards `q`, `Some(false)` right. `None` when
/// an end of the piece lies further from the line than `tol` times the line's size, the line
/// or the piece has no length, or a value is not finite.
#[must_use]
pub fn left_side_along_line_2d(
    a: [f64; 2],
    b: [f64; 2],
    p: [f64; 2],
    q: [f64; 2],
    tol: f64,
) -> Option<bool> {
    let u = sub_2d(q, p);
    let len = length_2d(u);
    let size = 1.0 + len + length_2d(p);
    // Whether the line has a length is its own matter; how far a piece may lie off it grows
    // with where it is, as rounding does.
    if !(tol.is_finite()
        && tol >= 0.0
        && len.is_finite()
        && size.is_finite()
        && len > tol * (1.0 + len))
    {
        return None;
    }
    let off = |x: [f64; 2]| {
        let [dx, dy] = sub_2d(x, p);
        (dx * u[1] - dy * u[0]).abs()
    };
    if !(off(a) <= tol * size * len && off(b) <= tol * size * len) {
        return None;
    }
    let along = dot_2d(sub_2d(b, a), u);
    if along > 0.0 {
        Some(true)
    } else if along < 0.0 {
        Some(false)
    } else {
        None
    }
}

/// Not upstream: whether the region on the left of an arc piece (a piece of a counter-clockwise
/// loop, about `centre` with `radius`, sweeping `sweep` radians, positive counter-clockwise)
/// lies inside the circle about `c` of radius `r` the piece lies on: `Some(true)` inside,
/// `Some(false)` outside. `None` when the piece's centre or radius is further from the circle's
/// than `tol` times its size, the piece has no sweep, or a value is not finite.
#[must_use]
pub fn left_side_inside_circle_2d(
    centre: [f64; 2],
    radius: f64,
    sweep: f64,
    c: [f64; 2],
    r: f64,
    tol: f64,
) -> Option<bool> {
    let size = 1.0 + r.abs() + length_2d(c);
    if !(tol.is_finite() && tol >= 0.0 && size.is_finite() && sweep.is_finite() && sweep != 0.0) {
        return None;
    }
    let on = distance_2d(centre, c) <= tol * size && (radius.abs() - r.abs()).abs() <= tol * size;
    on.then_some(sweep > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rounding below `noise` along a world axis is dropped and the axes come back unit; a plane
    /// tilted for real, and one with nothing to drop, keep their axes; parallel, empty or
    /// non-finite axes and a bad `noise` give `None`.
    #[test]
    fn an_axis_squared_frame_drops_only_the_rounding() {
        let tilted = Transform::axis_squared_frame(
            Vec3::new(1.0, 2e-15, 0.0),
            Vec3::new(-3e-16, 1.0, 1e-15),
            1e-12,
        );
        assert_eq!(tilted, Some([Vec3::X, Vec3::Y]));
        let (s, c) = (0.3_f64.sin(), 0.3_f64.cos());
        let real = [Vec3::new(c, s, 0.0), Vec3::new(-s, c, 0.0)];
        assert_eq!(
            Transform::axis_squared_frame(real[0], real[1], 1e-12),
            Some(real)
        );
        assert_eq!(
            Transform::axis_squared_frame(Vec3::X, Vec3::new(1.0, 1e-15, 0.0), 1e-12),
            None,
            "parallel once cleaned"
        );
        assert_eq!(
            Transform::axis_squared_frame(Vec3::new(1e-13, 0.0, 0.0), Vec3::Y, 1e-12),
            None,
            "no length once cleaned"
        );
        assert_eq!(
            Transform::axis_squared_frame(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Y, 1e-12),
            None
        );
        for noise in [-1.0, f64::INFINITY, f64::NAN] {
            assert_eq!(Transform::axis_squared_frame(Vec3::X, Vec3::Y, noise), None);
        }
    }

    #[test]
    fn rigid_maps_include_reflections_but_reject_scale_shear_and_hostile_entries() {
        let rigid = |m: Transform| m.is_rigid(1e-6, 1e-12);
        assert!(rigid(Transform::IDENTITY));
        assert!(rigid(Transform::translation(Vec3::new(3.0, -4.0, 5.0))));
        assert!(rigid(
            Transform::rotation(Point3::ZERO, Vec3::Z, 0.7).unwrap()
        ));
        assert!(rigid(Transform::reflection(Point3::ZERO, Vec3::X).unwrap()));
        for (row, col, value) in [
            (0, 0, 2.0),
            (1, 0, 0.01),
            (0, 3, 1e-6),
            (3, 3, 0.0),
            (3, 0, f64::INFINITY),
            (2, 1, f64::NAN),
        ] {
            let mut m = Transform::IDENTITY;
            m.m[row][col] = value;
            assert!(!rigid(m), "{row} {col} {value}");
        }
        assert!(!Transform::IDENTITY.is_rigid(f64::NAN, 1e-12));
        assert!(!Transform::IDENTITY.is_rigid(1e-6, 0.0));
    }

    #[test]
    fn plane_motion_uses_the_plane_origin_and_axes_and_rejects_tilts_and_reflections() {
        // Plane normal X, with local axes Y and Z, away from the world origin.
        // A quarter turn about X maps (10, 20, 30) to (10, -30, 20).
        let origin = Point3::new(10.0, 20.0, 30.0);
        let motion = |m: Transform| m.plane_motion(origin, Vec3::Y, Vec3::Z, 1e-9, 1e-12);
        let turn = Transform::from_axes(Vec3::X, Vec3::Z, -Vec3::Y, Vec3::ZERO);
        let (a, b) = motion(turn).unwrap();
        assert_eq!(a, [[0.0, -1.0], [1.0, 0.0]]);
        assert_eq!(b, Vec2::new(-50.0, -10.0));
        assert!(motion(Transform::IDENTITY).is_none());
        assert!(motion(Transform::rotation(Point3::ZERO, Vec3::Y, 0.2).unwrap()).is_none());
        assert!(motion(Transform::reflection(Point3::ZERO, Vec3::Y).unwrap()).is_none());
        // A normal displacement leaves every projected coordinate unchanged.
        let (a, b) = motion(Transform::translation(Vec3::X * 5.0)).unwrap();
        assert_eq!(a, [[1.0, 0.0], [0.0, 1.0]]);
        assert_eq!(b, Vec2::ZERO);
        assert!(motion(Transform::translation(Vec3::new(f64::NAN, 0.0, 0.0))).is_none());
    }

    #[test]
    fn point_arrays_keep_their_order_under_an_affine_map() {
        let map = Transform {
            m: [
                [2.0, 0.0, 0.0, 0.0],
                [1.0, -1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [10.0, 20.0, 30.0, 1.0],
            ],
        };
        let mut points = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        map.transform_points(&mut points);
        assert_eq!(points, [[14.0, 18.0, 33.0], [23.0, 15.0, 36.0]]);
        map.transform_points(&mut []);
    }

    #[test]
    fn transformed_box_holds_its_moved_corners_and_an_empty_box_stays_empty() {
        // A quarter turn about z, then a move by (10, 20, 30): x goes to y, y to -x.
        let map = Transform {
            m: [
                [0.0, 1.0, 0.0, 0.0],
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [10.0, 20.0, 30.0, 1.0],
            ],
        };
        let (lo, hi) = map.transformed_box(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 2.0, 3.0));
        let (want_lo, want_hi) = (Point3::new(8.0, 20.0, 30.0), Point3::new(10.0, 21.0, 33.0));
        for (got, want, out) in [
            (lo.x, want_lo.x, -1.0),
            (lo.y, want_lo.y, -1.0),
            (lo.z, want_lo.z, -1.0),
            (hi.x, want_hi.x, 1.0),
            (hi.y, want_hi.y, 1.0),
            (hi.z, want_hi.z, 1.0),
        ] {
            // outside the exact image, by no more than its rounding
            assert!((got - want) * out > 0.0 && (got - want).abs() < 1e-13);
        }
        let empty = (Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 1.0));
        assert_eq!(map.transformed_box(empty.0, empty.1), empty);
        // 1 + 2^-53 rounds to 1: the box still holds it.
        let tiny = 2f64.powi(-53);
        let shift = Transform::translation(Vec3::new(1.0, 0.0, 0.0));
        let p = Point3::new(tiny, 0.0, 0.0);
        let (lo, hi) = shift.transformed_box(p, p);
        assert!(lo.x < 1.0 && hi.x > 1.0 && hi.x - lo.x < 1e-14);
    }

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
        assert!(bad(Transform::into_plane_frame(
            Point3::ZERO,
            Vec3::ZERO,
            Vec3::Y
        )));
        assert!(bad(Transform::into_plane_frame(
            Point3::ZERO,
            Vec3::X,
            Vec3::new(-3.0, 0.0, 0.0)
        )));
        assert!(bad(Transform::into_plane_frame(
            Point3::new(f64::NAN, 0.0, 0.0),
            Vec3::X,
            Vec3::Y
        )));
    }

    /// Into a plane's frame: the plane's origin goes to the origin, a point along its x axis
    /// to X, along its normal to Z (its level above the plane), and the map is rigid. The
    /// plane through (3, 4, 5) with x = 2Z and y = (1, -1, 0) has the unit normal
    /// x̂ × ŷ = (1, 1, 0)/√2; a skew `y` in that plane gives the same frame.
    #[test]
    fn into_a_planes_frame_its_normal_is_z() {
        let o = Point3::new(3.0, 4.0, 5.0);
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let n = Vec3::new(s, s, 0.0);
        let skew = Vec3::new(1.0, -1.0, 7.0);
        for y in [Vec3::new(1.0, -1.0, 0.0), skew] {
            let t = Transform::into_plane_frame(o, Vec3::new(0.0, 0.0, 2.0), y).unwrap();
            let near = |a: Point3, b: Point3| (a - b).length() < 1e-12;
            assert!(near(t.transform_point(o), Point3::ZERO));
            assert!(near(
                t.transform_point(o + Vec3::Z * 2.5),
                Point3::new(2.5, 0.0, 0.0)
            ));
            assert!(near(
                t.transform_point(o + n * 4.0),
                Point3::new(0.0, 0.0, 4.0)
            ));
            assert!(near(
                t.transform_point(o + Vec3::new(s, -s, 0.0)),
                Point3::new(0.0, 1.0, 0.0)
            ));
            assert!(t.is_regular(0.0).is_yes());
        }
    }

    /// Onto a line: the top plane brought onto a line 30° above it through (1, 2, 3) along
    /// (0, cos 30°, sin 30°) tilts 30° about X (its x axis stays) and holds the line; onto a
    /// line it lies parallel to it only moves; a line square to it, or within
    /// [`PLANE_ONTO_LINE_SQUARE_SIN`] of square, and numbers not finite are refused.
    #[test]
    fn a_plane_brought_onto_a_line_tilts_the_least_and_holds_it() {
        let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-12;
        let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
        let (at, dir) = (Point3::new(1.0, 2.0, 3.0), Vec3::new(0.0, c, s));
        let (o, x, y) = plane_onto_line(Point3::ZERO, Vec3::X, Vec3::Y, at, dir).unwrap();
        let n = x.cross(y);
        assert!(near(x, Vec3::X), "{x:?}");
        assert!(near(y, dir), "{y:?}");
        assert!((n.dot(Vec3::Z) - c).abs() < 1e-12 && n.dot(dir).abs() < 1e-12);
        assert!((at - o).dot(n).abs() < 1e-12 && (at + dir * 7.0 - o).dot(n).abs() < 1e-12);

        let (o, x, y) = plane_onto_line(
            Point3::ZERO,
            Vec3::X,
            Vec3::Y,
            Point3::new(0.0, 3.0, 2.0),
            Vec3::X,
        )
        .unwrap();
        assert_eq!((x, y), (Vec3::X, Vec3::Y));
        assert!(near(o, Point3::new(0.0, 0.0, 2.0)), "{o:?}");

        let tilt = 1e-8;
        let steep = Vec3::new(0.0, tilt, (1.0 - tilt * tilt).sqrt());
        for dir in [Vec3::Z, -Vec3::Z, steep] {
            assert_eq!(
                plane_onto_line(Point3::ZERO, Vec3::X, Vec3::Y, at, dir),
                None
            );
        }
        assert_eq!(
            plane_onto_line(
                Point3::new(f64::NAN, 0.0, 0.0),
                Vec3::X,
                Vec3::Y,
                at,
                Vec3::X
            ),
            None
        );
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
    fn pivot_displacement_makes_a_turn_about_the_origin_a_turn_about_the_pivot() {
        let rot_z = Transform::from_axes(Vec3::Y, -Vec3::X, Vec3::Z, Vec3::ZERO);
        let pivot = Vec3::new(0.0, 10.0, 0.0);
        let d = rot_z.pivot_displacement(pivot);
        assert_eq!(d, Vec3::new(10.0, 10.0, 0.0));
        // The pivot stays where it is under the turn followed by the displacement.
        assert_eq!(rot_z.transform_vector(pivot) + d, pivot);
        // No turn, no displacement; a translation in the map is not read as a turn.
        assert_eq!(Transform::IDENTITY.pivot_displacement(pivot), Vec3::ZERO);
        let tr = Transform::translation(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(tr.pivot_displacement(pivot), Vec3::ZERO);
    }

    #[test]
    fn centroid_and_mean_radius_uses_the_arithmetic_mean_radius() {
        let points = [
            Point2::new(0.0, 0.0),
            Point2::new(6.0, 0.0),
            Point2::new(0.0, 6.0),
        ];
        let summary = centroid_and_mean_radius(&points).unwrap();
        assert_eq!(summary.centroid, Point2::new(2.0, 2.0));
        let expected = (2.0 * 2.0_f64.sqrt() + 4.0 * 5.0_f64.sqrt()) / 3.0;
        assert!((summary.mean_distance - expected).abs() <= 2.0 * f64::EPSILON * expected);
        assert_ne!(
            summary.mean_distance, 4.0,
            "the RMS radius is not the mean radius"
        );

        let moved = [
            Point2::new(0.0, 0.0),
            Point2::new(6.0, 0.0),
            Point2::new(0.0, 12.0),
        ];
        let moved_summary = centroid_and_mean_radius(&moved).unwrap();
        assert_eq!(moved_summary.centroid, Point2::new(2.0, 4.0));
        let moved_expected =
            (2.0 * 5.0_f64.sqrt() + 4.0 * 2.0_f64.sqrt() + 2.0 * 17.0_f64.sqrt()) / 3.0;
        assert!(
            (moved_summary.mean_distance - moved_expected).abs()
                <= 2.0 * f64::EPSILON * moved_expected
        );
    }

    #[test]
    fn centroid_and_mean_radius_accepts_single_and_duplicate_points() {
        let one = Point2::new(3.0, -4.0);
        assert_eq!(
            centroid_and_mean_radius(&[one]).unwrap(),
            PointSetSummary {
                centroid: one,
                mean_distance: 0.0,
            }
        );
        assert_eq!(
            centroid_and_mean_radius(&[one, one, one]).unwrap(),
            PointSetSummary {
                centroid: one,
                mean_distance: 0.0,
            }
        );
    }

    #[test]
    fn centroid_and_mean_radius_refuses_empty_non_finite_and_overflowing_sets() {
        assert!(centroid_and_mean_radius(&[]).is_err());
        assert!(centroid_and_mean_radius(&[Point2::new(f64::NAN, 0.0)]).is_err());
        assert!(centroid_and_mean_radius(&[Point2::new(0.0, f64::INFINITY)]).is_err());
        assert!(centroid_and_mean_radius(&[
            Point2::new(f64::MAX, 0.0),
            Point2::new(f64::MAX, 0.0),
        ])
        .is_err());
        assert!(centroid_and_mean_radius(&[
            Point2::new(-f64::MAX, 0.0),
            Point2::new(f64::MAX, 0.0),
        ])
        .is_err());
    }

    /// A piece running along its line has the region on the line's left, one running against it
    /// on its right; a piece off the line, a line or piece with no length and bad numbers give
    /// none.
    #[test]
    fn a_piece_on_a_line_tells_the_side_its_region_is_on() {
        let (p, q) = ([0.0, 1.0], [0.0, 21.0]);
        assert_eq!(
            left_side_along_line_2d([0.0, 5.0], [0.0, 9.0], p, q, 1e-9),
            Some(true)
        );
        assert_eq!(
            left_side_along_line_2d([0.0, 9.0], [0.0, 5.0], p, q, 1e-9),
            Some(false)
        );
        // Past the line's ends, on its carrier, still on it.
        assert_eq!(
            left_side_along_line_2d([0.0, 30.0], [0.0, 25.0], p, q, 1e-9),
            Some(false)
        );
        assert_eq!(
            left_side_along_line_2d([1e-6, 5.0], [0.0, 9.0], p, q, 1e-9),
            None
        );
        assert_eq!(
            left_side_along_line_2d([0.0, 5.0], [0.0, 5.0], p, q, 1e-9),
            None
        );
        assert_eq!(
            left_side_along_line_2d([0.0, 5.0], [0.0, 9.0], p, p, 1e-9),
            None
        );
        assert_eq!(
            left_side_along_line_2d([0.0, 5.0], [0.0, 9.0], p, q, f64::NAN),
            None
        );
        assert_eq!(
            left_side_along_line_2d([f64::NAN, 5.0], [0.0, 9.0], p, q, 1e-9),
            None
        );
        assert_eq!(distance_2d([1.0, 1.0], [4.0, 5.0]), 5.0);
        // A short line far from the origin tells the same side as at the origin.
        for x in [0.0, 5e5] {
            let (p, q) = ([x, 0.0], [x, 4e-4]);
            assert_eq!(
                left_side_along_line_2d([x, 1e-4], [x, 3e-4], p, q, 1e-9),
                Some(true),
                "{x}"
            );
            assert_eq!(
                left_side_along_line_2d([x, 3e-4], [x, 1e-4], p, q, 1e-9),
                Some(false),
                "{x}"
            );
        }
    }

    /// A counter-clockwise arc piece has the region inside its circle, a clockwise one outside;
    /// a piece on another circle, with no sweep or bad numbers gives none.
    #[test]
    fn an_arc_piece_tells_whether_its_region_is_inside_the_circle() {
        let c = [0.0, 19.4];
        assert_eq!(
            left_side_inside_circle_2d(c, 3.04, 1.2, c, 3.04, 1e-9),
            Some(true)
        );
        assert_eq!(
            left_side_inside_circle_2d(c, 3.04, -1.2, c, 3.04, 1e-9),
            Some(false)
        );
        assert_eq!(
            left_side_inside_circle_2d([0.0, 20.0], 3.04, 1.2, c, 3.04, 1e-9),
            None
        );
        assert_eq!(
            left_side_inside_circle_2d(c, 3.05, 1.2, c, 3.04, 1e-9),
            None
        );
        assert_eq!(
            left_side_inside_circle_2d(c, 3.04, 0.0, c, 3.04, 1e-9),
            None
        );
        assert_eq!(
            left_side_inside_circle_2d(c, 3.04, f64::NAN, c, 3.04, 1e-9),
            None
        );
    }

    #[test]
    fn vec2_basics() {
        let a = Vec2::new(3.0, 4.0);
        assert_eq!(a.length(), 5.0);
        assert_eq!((a - a + a * 2.0).dot(-a), -50.0);
    }
}
