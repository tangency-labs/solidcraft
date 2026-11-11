//! The vector measures the mesher works with, on the record's plain [`Vec3`] and the kernel's: the formulas, bit
//! for bit, the planner was built and measured with (SolidCraft's geometry traits), so its plans
//! do not move with the move into solidcore.

use solidcore_record::Vec3;

/// Below this length a vector has no direction ([`VecExt::normalized`]).
const EPS: f64 = 1e-9;

/// Products, lengths, distances and directions of a [`Vec3`].
// `len` is the vector's length, not a count.
#[allow(clippy::len_without_is_empty)]
pub(crate) trait VecExt: Sized {
    fn dot(self, o: Self) -> f64;
    #[cfg_attr(not(test), allow(dead_code))] // the simplifier's tests
    fn cross(self, o: Self) -> Self;
    fn len2(self) -> f64;
    fn len(self) -> f64;
    fn dist(self, o: Self) -> f64;
    /// The unit vector, `None` when shorter than 1e-9 or not finite.
    fn normalized(self) -> Option<Self>;
    fn min(self, o: Self) -> Self;
    fn max(self, o: Self) -> Self;
    /// Distance from this point to the segment `a`–`b`.
    fn dist_to_segment(self, a: Self, b: Self) -> f64;
    /// Whether every coordinate is finite.
    #[allow(clippy::wrong_self_convention)] // a copy type, as `f64::is_finite`
    fn is_finite(self) -> bool;
}

macro_rules! vec_ext {
    ($v:ty) => {
        impl VecExt for $v {
            fn dot(self, o: $v) -> f64 {
                self.x * o.x + self.y * o.y + self.z * o.z
            }
            fn cross(self, o: $v) -> $v {
                <$v>::new(
                    self.y * o.z - self.z * o.y,
                    self.z * o.x - self.x * o.z,
                    self.x * o.y - self.y * o.x,
                )
            }
            fn len2(self) -> f64 {
                self.dot(self)
            }
            fn len(self) -> f64 {
                self.len2().sqrt()
            }
            fn dist(self, o: $v) -> f64 {
                (self - o).len()
            }
            fn normalized(self) -> Option<$v> {
                let l = self.len();
                (l > EPS && l.is_finite()).then(|| self / l)
            }
            fn min(self, o: $v) -> $v {
                <$v>::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
            }
            fn max(self, o: $v) -> $v {
                <$v>::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
            }
            fn is_finite(self) -> bool {
                self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
            }
            fn dist_to_segment(self, a: $v, b: $v) -> f64 {
                let ab = b - a;
                let l2 = ab.len2();
                let t = if l2 > 0.0 {
                    // Proven: the clamp bounds are the constants 0.0 <= 1.0, neither NaN.
                    ((self - a).dot(ab) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                self.dist(a + ab * t)
            }
        }
    };
}

// The record's vector and the kernel's: the same formulas on each.
vec_ext!(Vec3);
vec_ext!(solidcore::Vec3);
