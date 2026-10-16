//! Small conversions shared by the unit runners in [`crate::units`].

use solidcore::math::{Point2, Vec3};

/// A JSON `[x, y, z]` as a [`Vec3`].
#[must_use]
pub fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// A JSON `[u, v]` as a [`Point2`].
#[must_use]
pub fn p2(p: [f64; 2]) -> Point2 {
    Point2::new(p[0], p[1])
}

/// A boolean as an output value: `1.0` or `0.0`.
#[must_use]
pub fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}
