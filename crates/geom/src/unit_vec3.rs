//! A unit vector: a 3D vector of length 1.

use std::ops::{Add, Deref, Mul, Neg, Sub};

use serde::{Deserialize, Serialize};

use crate::Vec3;

/// A unit vector: length 1 up to rounding. Made only from a vector with a length
/// ([`UnitVec3::new`], [`Vec3::unit`]), so a function taking one need not normalise it or
/// handle a zero vector again. It reads as the [`Vec3`] it holds (`d.dot(v)`, `*d`), and
/// [`Vec3::dot`] and [`Vec3::cross`] take one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "Vec3", try_from = "Vec3")]
pub struct UnitVec3(Vec3);

impl UnitVec3 {
    pub const X: UnitVec3 = UnitVec3(Vec3::X);
    pub const Y: UnitVec3 = UnitVec3(Vec3::Y);
    pub const Z: UnitVec3 = UnitVec3(Vec3::Z);

    /// `v` scaled to length 1; `None` when it has no length (or is not finite).
    pub fn new(v: Vec3) -> Option<UnitVec3> {
        v.normalized().map(UnitVec3)
    }
    pub fn vec(self) -> Vec3 {
        self.0
    }
    /// Any unit vector square to this one.
    pub fn any_perp(self) -> UnitVec3 {
        UnitVec3(self.0.any_perp())
    }
    /// Two unit vectors square to this one and to each other, `(u, v)` with `u × v` along it.
    pub fn perp_pair(self) -> (UnitVec3, UnitVec3) {
        let u = self.any_perp();
        (u, UnitVec3(self.0.cross(u.0)))
    }
    /// The angle to `o`, 0 to π.
    pub fn angle_to(self, o: UnitVec3) -> f64 {
        self.0.dot(o.0).clamp(-1.0, 1.0).acos()
    }
}

impl Vec3 {
    /// This vector scaled to length 1 ([`UnitVec3::new`]).
    pub fn unit(self) -> Option<UnitVec3> {
        UnitVec3::new(self)
    }
    /// This vector turned by `angle` about `axis` (right hand; Rodrigues' formula).
    pub fn rotated_about(self, axis: UnitVec3, angle: f64) -> Vec3 {
        let (s, c) = angle.sin_cos();
        self * c + axis.cross(self) * s + axis * (axis.dot(self) * (1.0 - c))
    }
}

impl Deref for UnitVec3 {
    type Target = Vec3;
    fn deref(&self) -> &Vec3 {
        &self.0
    }
}

impl From<UnitVec3> for Vec3 {
    fn from(d: UnitVec3) -> Vec3 {
        d.0
    }
}

impl TryFrom<Vec3> for UnitVec3 {
    type Error = &'static str;
    fn try_from(v: Vec3) -> Result<UnitVec3, Self::Error> {
        UnitVec3::new(v).ok_or("a unit vector needs a finite vector with a length")
    }
}

impl From<&UnitVec3> for Vec3 {
    fn from(d: &UnitVec3) -> Vec3 {
        d.0
    }
}

impl Neg for UnitVec3 {
    type Output = UnitVec3;
    fn neg(self) -> UnitVec3 {
        UnitVec3(-self.0)
    }
}

/// `+` and `-` with a [`Vec3`] or another unit vector, giving a [`Vec3`].
macro_rules! sum_ops {
    ($($l:ty, $r:ty);*) => {$(
        impl Add<$r> for $l {
            type Output = Vec3;
            fn add(self, o: $r) -> Vec3 {
                Vec3::from(self) + Vec3::from(o)
            }
        }
        impl Sub<$r> for $l {
            type Output = Vec3;
            fn sub(self, o: $r) -> Vec3 {
                Vec3::from(self) - Vec3::from(o)
            }
        }
    )*};
}
sum_ops!(UnitVec3, Vec3; Vec3, UnitVec3; UnitVec3, UnitVec3);

impl Mul<f64> for UnitVec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        self.0 * s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unit_vector_has_length_one() {
        let d = Vec3::new(0.0, 3.0, 4.0).unit().unwrap();
        assert_eq!(*d, Vec3::new(0.0, 0.6, 0.8));
        assert_eq!(d * 5.0, Vec3::new(0.0, 3.0, 4.0));
        assert_eq!(*-d, Vec3::new(0.0, -0.6, -0.8));
        assert_eq!(d.dot(Vec3::Z), 0.8);
        assert_eq!(UnitVec3::Z.cross(UnitVec3::X), Vec3::Y);
        assert_eq!(Vec3::X.dot(d), 0.0);
        assert_eq!(Vec3::X + d, Vec3::new(1.0, 0.6, 0.8));
        assert!((d - UnitVec3::Z).dist(Vec3::new(0.0, 0.6, -0.2)) < 1e-15);
    }

    #[test]
    fn frames_turns_and_angles() {
        let n = Vec3::new(1.0, 2.0, 2.0).unit().unwrap();
        assert!(n.any_perp().dot(n).abs() < 1e-12);
        let (u, v) = n.perp_pair();
        assert!(u.dot(n).abs() < 1e-12 && v.dot(n).abs() < 1e-12 && u.dot(v).abs() < 1e-12);
        assert!((u.len() - 1.0).abs() < 1e-12 && (v.len() - 1.0).abs() < 1e-12);
        assert!(u.cross(v).dist(*n) < 1e-12);
        let q = Vec3::X.rotated_about(UnitVec3::Z, std::f64::consts::FRAC_PI_2);
        assert!(q.dist(Vec3::Y) < 1e-12);
        assert!(n.rotated_about(n, 1.0).dist(*n) < 1e-12);
        assert_eq!(UnitVec3::X.angle_to(UnitVec3::Y), std::f64::consts::FRAC_PI_2);
        assert_eq!(n.angle_to(n), 0.0);
        assert_eq!(n.angle_to(-n), std::f64::consts::PI);
    }

    #[test]
    fn no_length_no_unit_vector() {
        assert_eq!(Vec3::ZERO.unit(), None);
        assert_eq!(Vec3::new(f64::NAN, 0.0, 1.0).unit(), None);
        assert_eq!(Vec3::new(f64::INFINITY, 0.0, 0.0).unit(), None);
    }
}
