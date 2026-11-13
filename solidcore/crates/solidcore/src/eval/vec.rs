//! The vector measures the evaluator works with, on [`Vec3`].

use crate::math::Vec3;

/// Below this length a vector has no direction ([`VecExt::normalized`]).
const EPS: f64 = 1e-9;

/// Lengths, distances and directions of a [`Vec3`], as the evaluator takes them.
#[allow(clippy::len_without_is_empty)] // a vector's length, not a count
pub trait VecExt: Sized {
    /// The squared length.
    fn len2(self) -> f64;
    /// The length.
    fn len(self) -> f64;
    /// The distance to `o`.
    fn dist(self, o: Self) -> f64;
    /// The unit vector, `None` when shorter than 1e-9 or not finite.
    fn normalized(self) -> Option<Self>;
    /// Every coordinate is finite.
    #[allow(clippy::wrong_self_convention)] // a copy type, as `f64::is_finite`
    fn is_finite(self) -> bool;
}

impl VecExt for Vec3 {
    fn len2(self) -> f64 {
        self.dot(self)
    }
    fn len(self) -> f64 {
        self.len2().sqrt()
    }
    fn dist(self, o: Vec3) -> f64 {
        (self - o).len()
    }
    fn normalized(self) -> Option<Vec3> {
        let l = self.len();
        (l > EPS && l.is_finite()).then(|| self / l)
    }
    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

/// `[x, y, z]` as a [`Vec3`].
#[must_use]
pub fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}
