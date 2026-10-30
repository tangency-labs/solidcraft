//! Rotations for the solver: a 3×3 matrix and a unit quaternion.

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

/// A 3×3 matrix, row-major (`m[row][col]`); acts on column vectors.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mat3(pub [[f64; 3]; 3]);

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    pub const ZERO: Mat3 = Mat3([[0.0; 3]; 3]);

    /// The matrix whose columns are `a`, `b` and `c`.
    pub fn from_cols(a: Vec3, b: Vec3, c: Vec3) -> Mat3 {
        Mat3([[a.x, b.x, c.x], [a.y, b.y, c.y], [a.z, b.z, c.z]])
    }
    pub fn diagonal(d: Vec3) -> Mat3 {
        Mat3([[d.x, 0.0, 0.0], [0.0, d.y, 0.0], [0.0, 0.0, d.z]])
    }
    pub fn col(&self, i: usize) -> Vec3 {
        let [r0, r1, r2] = self.0;
        match i {
            0 => Vec3::new(r0[0], r1[0], r2[0]),
            1 => Vec3::new(r0[1], r1[1], r2[1]),
            _ => Vec3::new(r0[2], r1[2], r2[2]),
        }
    }
    pub fn row(&self, i: usize) -> Vec3 {
        let [a, b, c] = match i {
            0 => self.0[0],
            1 => self.0[1],
            _ => self.0[2],
        };
        Vec3::new(a, b, c)
    }
    pub fn mul_vec(&self, v: Vec3) -> Vec3 {
        Vec3::new(self.row(0).dot(v), self.row(1).dot(v), self.row(2).dot(v))
    }
    pub fn mul(&self, o: &Mat3) -> Mat3 {
        Mat3::from_cols(self.mul_vec(o.col(0)), self.mul_vec(o.col(1)), self.mul_vec(o.col(2)))
    }
    pub fn transpose(&self) -> Mat3 {
        Mat3::from_cols(self.row(0), self.row(1), self.row(2))
    }
    pub fn is_finite(&self) -> bool {
        self.0.iter().flatten().all(|x| x.is_finite())
    }
    /// The largest difference between `m[i][j]` and `m[j][i]`.
    pub fn asymmetry(&self) -> f64 {
        let m = &self.0;
        (m[0][1] - m[1][0]).abs().max((m[0][2] - m[2][0]).abs()).max((m[1][2] - m[2][1]).abs())
    }
    pub fn determinant(&self) -> f64 {
        self.col(0).dot(self.col(1).cross(self.col(2)))
    }
    /// The inverse of a symmetric positive definite matrix (an inertia tensor); `None` when it is
    /// not one (by its leading principal minors, scaled to its size).
    pub fn spd_inverse(&self) -> Option<Mat3> {
        let m = &self.0;
        let scale = m[0][0].abs().max(m[1][1].abs()).max(m[2][2].abs());
        if !(scale > 0.0) || !self.is_finite() || self.asymmetry() > 1e-9 * scale {
            return None;
        }
        let rel = 1e-12;
        let m1 = m[0][0];
        let m2 = m[0][0] * m[1][1] - m[0][1] * m[1][0];
        let det = self.determinant();
        if !(m1 > rel * scale) || !(m2 > rel * scale * scale) || !(det > rel * scale * scale * scale) {
            return None;
        }
        // Adjugate over determinant: the cross products of the columns are the adjugate's rows.
        let (a, b, c) = (self.col(0), self.col(1), self.col(2));
        let inv = Mat3::from_cols(b.cross(c), c.cross(a), a.cross(b)).transpose();
        let inv = Mat3(inv.0.map(|r| r.map(|x| x / det)));
        inv.is_finite().then_some(inv)
    }
}

/// `a × (b × c)`-style helper: the vector `a × (a × v)`.
pub fn cross2(a: Vec3, v: Vec3) -> Vec3 {
    a.cross(a.cross(v))
}

/// A rotation as a unit quaternion `w + xi + yj + zk`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Quat {
    fn default() -> Self {
        Quat::IDENTITY
    }
}

impl Quat {
    pub const IDENTITY: Quat = Quat { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };

    pub fn vec(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
    /// The turn by `angle` (radians) about `axis` (any length; the identity for a zero axis).
    pub fn from_axis_angle(axis: Vec3, angle: f64) -> Quat {
        match axis.normalized() {
            Some(a) => {
                let (s, c) = (angle * 0.5).sin_cos();
                Quat { w: c, x: a.x * s, y: a.y * s, z: a.z * s }
            }
            None => Quat::IDENTITY,
        }
    }
    /// The turn by the rotation vector `v` (axis times angle).
    pub fn from_rotation_vector(v: Vec3) -> Quat {
        // sin(θ/2)/θ by its series for small turns: a projection's corrections are far below
        // the length a direction can be normalised from.
        let angle = v.len();
        let (s, c) = (angle * 0.5).sin_cos();
        let k = if angle < 1e-4 { 0.5 - angle * angle / 48.0 } else { s / angle };
        if !k.is_finite() || !v.is_finite() {
            return Quat::IDENTITY;
        }
        Quat { w: c, x: v.x * k, y: v.y * k, z: v.z * k }
    }
    /// The rotation whose matrix has these orthonormal columns (a right-handed frame).
    pub fn from_mat3(m: &Mat3) -> Option<Quat> {
        let r = &m.0;
        let tr = r[0][0] + r[1][1] + r[2][2];
        let q = if tr > 0.0 {
            let s = (tr + 1.0).sqrt() * 2.0;
            Quat { w: 0.25 * s, x: (r[2][1] - r[1][2]) / s, y: (r[0][2] - r[2][0]) / s, z: (r[1][0] - r[0][1]) / s }
        } else if r[0][0] > r[1][1] && r[0][0] > r[2][2] {
            let s = (1.0 + r[0][0] - r[1][1] - r[2][2]).sqrt() * 2.0;
            Quat { w: (r[2][1] - r[1][2]) / s, x: 0.25 * s, y: (r[0][1] + r[1][0]) / s, z: (r[0][2] + r[2][0]) / s }
        } else if r[1][1] > r[2][2] {
            let s = (1.0 + r[1][1] - r[0][0] - r[2][2]).sqrt() * 2.0;
            Quat { w: (r[0][2] - r[2][0]) / s, x: (r[0][1] + r[1][0]) / s, y: 0.25 * s, z: (r[1][2] + r[2][1]) / s }
        } else {
            let s = (1.0 + r[2][2] - r[0][0] - r[1][1]).sqrt() * 2.0;
            Quat { w: (r[1][0] - r[0][1]) / s, x: (r[0][2] + r[2][0]) / s, y: (r[1][2] + r[2][1]) / s, z: 0.25 * s }
        };
        q.normalized()
    }
    /// The Hamilton product `self ⊗ o` (turn by `o`, then by `self`).
    pub fn compose(self, o: Quat) -> Quat {
        Quat {
            w: self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
            x: self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            y: self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            z: self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
        }
    }
    pub fn conjugate(self) -> Quat {
        Quat { w: self.w, x: -self.x, y: -self.y, z: -self.z }
    }
    pub fn norm(self) -> f64 {
        (self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    pub fn is_finite(self) -> bool {
        self.w.is_finite() && self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    /// Scaled to length 1; `None` for a zero or non-finite quaternion.
    pub fn normalized(self) -> Option<Quat> {
        let n = self.norm();
        (n > 1e-300 && n.is_finite()).then(|| Quat { w: self.w / n, x: self.x / n, y: self.y / n, z: self.z / n })
    }
    pub fn to_mat3(self) -> Mat3 {
        let Quat { w, x, y, z } = self;
        Mat3([
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
            [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
            [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
        ])
    }
    pub fn rotate(self, v: Vec3) -> Vec3 {
        // v + 2 q×(q×v + w v)
        let q = self.vec();
        v + q.cross(q.cross(v) + v * self.w) * 2.0
    }
    /// The rate of change of this orientation when the body turns at `omega` (world frame):
    /// `q̇ = ½ (0, ω) ⊗ q`.
    pub fn rate(self, omega: Vec3) -> Quat {
        let o = Quat { w: 0.0, x: omega.x, y: omega.y, z: omega.z }.compose(self);
        Quat { w: 0.5 * o.w, x: 0.5 * o.x, y: 0.5 * o.y, z: 0.5 * o.z }
    }
    /// `self + k·o`, component-wise (an integrator's stage; normalise afterwards).
    pub fn add_scaled(self, o: Quat, k: f64) -> Quat {
        Quat { w: self.w + k * o.w, x: self.x + k * o.x, y: self.y + k * o.y, z: self.z + k * o.z }
    }
    /// The rotation vector (axis times angle, angle in [0, π]).
    pub fn rotation_vector(self) -> Vec3 {
        let q = if self.w < 0.0 { Quat { w: -self.w, x: -self.x, y: -self.y, z: -self.z } } else { self };
        let s = q.vec().len();
        if s < 1e-12 {
            return q.vec() * 2.0;
        }
        q.vec() * (2.0 * s.atan2(q.w) / s)
    }
}
