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

/// The signed Cartesian unit direction making the smallest angle with `direction`.
/// Equal absolute coordinates choose X before Y before Z. Zero and non-finite
/// directions have no answer. Comparing coordinates avoids normalisation and overflow.
#[must_use]
pub fn nearest_principal_direction(direction: Vec3) -> Option<Vec3> {
    if !direction.is_finite() || direction == Vec3::ZERO {
        return None;
    }
    let x = direction.x.abs();
    let y = direction.y.abs();
    let z = direction.z.abs();
    Some(if x >= y && x >= z {
        Vec3::new(direction.x.signum(), 0.0, 0.0)
    } else if y >= z {
        Vec3::new(0.0, direction.y.signum(), 0.0)
    } else {
        Vec3::new(0.0, 0.0, direction.z.signum())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_direction_uses_the_largest_signed_coordinate() {
        for (input, expected) in [
            ([4.0, 2.0, 1.0], [1.0, 0.0, 0.0]),
            ([-4.0, 2.0, 1.0], [-1.0, 0.0, 0.0]),
            ([1.0, 4.0, 2.0], [0.0, 1.0, 0.0]),
            ([1.0, -4.0, 2.0], [0.0, -1.0, 0.0]),
            ([1.0, 2.0, 4.0], [0.0, 0.0, 1.0]),
            ([1.0, 2.0, -4.0], [0.0, 0.0, -1.0]),
            ([-1.0, 1.0, 1.0], [-1.0, 0.0, 0.0]),
            ([0.0, -1.0, 1.0], [0.0, -1.0, 0.0]),
            ([f64::MAX, 1.0, 0.0], [1.0, 0.0, 0.0]),
            ([0.0, 0.0, f64::MIN_POSITIVE], [0.0, 0.0, 1.0]),
        ] {
            assert_eq!(nearest_principal_direction(v3(input)), Some(v3(expected)));
        }
        for input in [[0.0; 3], [f64::NAN, 1.0, 0.0], [0.0, f64::INFINITY, 1.0]] {
            assert!(nearest_principal_direction(v3(input)).is_none());
        }
    }
}
