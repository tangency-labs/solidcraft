//! The vector measures hidden lines work with, on the kernel's [`Vec3`]: as SolidCraft's geometry
//! took them, where the code came from.

use crate::math::Vec3;

/// Below this length a vector has no direction (mm).
const EPS: f64 = 1e-9;

/// Lengths, distances, directions and bounds of a [`Vec3`].
#[allow(clippy::len_without_is_empty)] // a vector's length, not a count
pub(crate) trait V3: Sized {
    /// The squared length.
    fn len2(self) -> f64;
    /// The length.
    fn len(self) -> f64;
    /// The distance to `o`.
    fn dist(self, o: Self) -> f64;
    /// The unit vector, `None` when shorter than 1e-9 or not finite.
    fn normalized(self) -> Option<Self>;
    /// The point a fraction `t` of the way to `o`.
    fn lerp(self, o: Self, t: f64) -> Self;
    /// The lesser coordinates.
    fn min(self, o: Self) -> Self;
    /// The greater coordinates.
    fn max(self, o: Self) -> Self;
    /// A unit vector square to this one.
    fn any_perp(self) -> Self;
    /// The distance to the segment from `a` to `b`.
    fn dist_to_segment(self, a: Self, b: Self) -> f64;
    /// Every coordinate is finite.
    #[allow(clippy::wrong_self_convention)] // a copy type, as `f64::is_finite`
    fn is_finite(self) -> bool;
}

impl V3 for Vec3 {
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
    fn lerp(self, o: Vec3, t: f64) -> Vec3 {
        self + (o - self) * t
    }
    fn min(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    fn max(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
    fn any_perp(self) -> Vec3 {
        let a = if self.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        self.cross(a).normalized().unwrap_or(Vec3::Z)
    }
    fn dist_to_segment(self, a: Vec3, b: Vec3) -> f64 {
        let ab = b - a;
        let l2 = ab.len2();
        let t = if l2 > 0.0 {
            ((self - a).dot(ab) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.dist(a + ab * t)
    }
    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

/// Entry `k` of a small fixed array (a coordinate, a direction's bounds), the default past its
/// end: the indices hidden lines use are 0 to N - 1 by construction.
pub(crate) fn at<T: Copy + Default, const N: usize>(a: [T; N], k: usize) -> T {
    a.get(k).copied().unwrap_or_default()
}

/// Sets entry `k` of a small fixed array, when it has one.
pub(crate) fn set<T, const N: usize>(a: &mut [T; N], k: usize, v: T) {
    if let Some(slot) = a.get_mut(k) {
        *slot = v;
    }
}

/// `x` held to `[lo, hi]`, as `f64::clamp` holds it (a NaN stays one), without its panic when
/// `lo > hi`: then `lo` wins below it and `hi` above it.
pub(crate) fn clamp_to(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}
