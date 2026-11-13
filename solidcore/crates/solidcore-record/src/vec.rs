//! Vectors and points as data: [`Vec2`] (sketch space) and [`Vec3`] (model space, Z up), with
//! their component-wise operators (`+`, `-`, scalar `*` and `/`, negation). Every geometric
//! quantity of them (dot and cross products, lengths, distances, directions) is the kernel's.
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// 2D vector / point (sketch space).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f64; 2]", into = "[f64; 2]")]
pub struct Vec2 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
}

/// 3D vector / point (model space, Z up).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f64; 3]", into = "[f64; 3]")]
pub struct Vec3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl From<[f64; 2]> for Vec2 {
    fn from(a: [f64; 2]) -> Self {
        Vec2::new(a[0], a[1])
    }
}
impl From<Vec2> for [f64; 2] {
    fn from(v: Vec2) -> Self {
        [v.x, v.y]
    }
}
impl From<[f64; 3]> for Vec3 {
    fn from(a: [f64; 3]) -> Self {
        Vec3::new(a[0], a[1], a[2])
    }
}
impl From<Vec3> for [f64; 3] {
    fn from(v: Vec3) -> Self {
        [v.x, v.y, v.z]
    }
}

impl Vec2 {
    /// The origin.
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    /// Unit X.
    pub const X: Vec2 = Vec2 { x: 1.0, y: 0.0 };
    /// Unit Y.
    pub const Y: Vec2 = Vec2 { x: 0.0, y: 1.0 };
    /// `(x, y)`.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Vec2 { x, y }
    }
    /// Both components finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
    /// `(x, y, z)`.
    #[must_use]
    pub fn extend(self, z: f64) -> Vec3 {
        Vec3::new(self.x, self.y, z)
    }
}

impl Vec3 {
    /// The origin.
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    /// Unit X.
    pub const X: Vec3 = Vec3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    /// Unit Y.
    pub const Y: Vec3 = Vec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    /// Unit Z.
    pub const Z: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };
    /// `(x, y, z)`.
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }
    /// Every component finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    /// The components in single precision (for drawing).
    pub fn to_f32(self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }
}

macro_rules! ops {
    ($t:ident, $($f:ident),+) => {
        impl Add for $t { type Output = $t; fn add(self, o: $t) -> $t { $t { $($f: self.$f + o.$f),+ } } }
        impl Sub for $t { type Output = $t; fn sub(self, o: $t) -> $t { $t { $($f: self.$f - o.$f),+ } } }
        impl Mul<f64> for $t { type Output = $t; fn mul(self, k: f64) -> $t { $t { $($f: self.$f * k),+ } } }
        impl Div<f64> for $t { type Output = $t; fn div(self, k: f64) -> $t { $t { $($f: self.$f / k),+ } } }
        impl Neg for $t { type Output = $t; fn neg(self) -> $t { $t { $($f: -self.$f),+ } } }
        impl AddAssign for $t { fn add_assign(&mut self, o: $t) { $(self.$f += o.$f;)+ } }
        impl SubAssign for $t { fn sub_assign(&mut self, o: $t) { $(self.$f -= o.$f;)+ } }
    };
}
ops!(Vec2, x, y);
ops!(Vec3, x, y, z);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_are_component_wise_and_serde_is_an_array() {
        let (a, b) = (Vec3::new(1., 2., 3.), Vec3::new(-4., 0.5, 8.));
        assert_eq!(a + b, Vec3::new(-3., 2.5, 11.));
        assert_eq!(a - b, Vec3::new(5., 1.5, -5.));
        assert_eq!(a * 2., Vec3::new(2., 4., 6.));
        assert_eq!(b / 2., Vec3::new(-2., 0.25, 4.));
        assert_eq!(-a, Vec3::new(-1., -2., -3.));
        let mut c = a;
        c += b;
        c -= a;
        assert_eq!(c, b);
        assert_eq!(serde_json::to_string(&a).unwrap(), "[1.0,2.0,3.0]");
        assert_eq!(
            serde_json::from_str::<Vec2>("[0.5,-1.0]").unwrap(),
            Vec2::new(0.5, -1.)
        );
        assert_eq!(Vec2::new(1., 2.).extend(3.), a);
        assert!(!Vec3::new(f64::NAN, 0., 0.).is_finite());
    }
}
