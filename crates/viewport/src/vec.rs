//! The renderer's own vector maths on [`Vec3`] and [`Vec2`], and its [`Bounds`]: the few
//! products, lengths, blends and boxes the camera, the CPU rasterizer, the shading models, the
//! section hatch and the GPU pipelines need. The renderer takes no geometry crate, so it keeps
//! these here rather than reach for an app's.

use solidcore_record::{Vec2, Vec3};

/// Lengths shorter than this (mm) have no direction ([`Vec3Ops::normalized`]).
const MIN_LENGTH: f64 = 1e-9;

/// Products, lengths and blends of a 3D vector or point.
// `len` is the vector's length, not a count.
#[allow(clippy::len_without_is_empty)]
pub trait Vec3Ops {
    /// The dot product.
    fn dot(self, o: Vec3) -> f64;
    /// The cross product, right-handed.
    fn cross(self, o: Vec3) -> Vec3;
    /// The length.
    fn len(self) -> f64;
    /// The distance to the point `o`.
    fn dist(self, o: Vec3) -> f64;
    /// This vector scaled to length 1, or `None` when it is too short or not finite.
    fn normalized(self) -> Option<Vec3>;
    /// The point a fraction `t` of the way to `o`.
    fn lerp(self, o: Vec3, t: f64) -> Vec3;
    /// The smaller of each component.
    fn min(self, o: Vec3) -> Vec3;
    /// The larger of each component.
    fn max(self, o: Vec3) -> Vec3;
    /// Any unit vector perpendicular to `self` (which should be non-zero); `Z` when it has none.
    fn any_perp(self) -> Vec3;
}

impl Vec3Ops for Vec3 {
    fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    fn dist(self, o: Vec3) -> f64 {
        (self - o).len()
    }
    fn normalized(self) -> Option<Vec3> {
        let l = self.len();
        (l > MIN_LENGTH && l.is_finite()).then(|| self / l)
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
}

/// Products, lengths and blends of a 2D vector or point (the section plane's).
// `len` is the vector's length, not a count.
#[allow(clippy::len_without_is_empty)]
pub trait Vec2Ops {
    /// The dot product.
    fn dot(self, o: Vec2) -> f64;
    /// The cross product's z: positive when `o` turns counter-clockwise from `self`.
    fn cross(self, o: Vec2) -> f64;
    /// The length.
    fn len(self) -> f64;
    /// The point a fraction `t` of the way to `o`.
    fn lerp(self, o: Vec2, t: f64) -> Vec2;
}

impl Vec2Ops for Vec2 {
    fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    fn lerp(self, o: Vec2, t: f64) -> Vec2 {
        self + (o - self) * t
    }
}

/// An axis-aligned box the renderer frames, clips and shades by: the camera's fit and depth range,
/// the ground's ambient occlusion. Plain data the host fills from its own bounds. An empty box
/// ([`Bounds::EMPTY`], or any with `min` past `max` or NaN) has no centre or size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl Default for Bounds {
    fn default() -> Self {
        Bounds::EMPTY
    }
}

impl Bounds {
    /// The box around nothing.
    pub const EMPTY: Bounds = Bounds {
        min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    };
    /// Whether the box holds no point.
    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z)
    }
    /// Grow the box to hold `p`; a point that is not finite is left out.
    pub fn add(&mut self, p: Vec3) {
        if p.is_finite() {
            self.min = self.min.min(p);
            self.max = self.max.max(p);
        }
    }
    /// The middle of the box; the origin for an empty box.
    pub fn center(&self) -> Vec3 {
        if self.is_empty() { Vec3::ZERO } else { (self.min + self.max) * 0.5 }
    }
    /// The length of the box's diagonal; zero for an empty box.
    pub fn diagonal(&self) -> f64 {
        if self.is_empty() { 0.0 } else { (self.max - self.min).len() }
    }
}

#[cfg(test)]
mod tests {
    use super::{Bounds, Vec2Ops, Vec3Ops};
    use solidcore_record::{Vec2, Vec3};

    #[test]
    fn products_lengths_and_blends() {
        let (a, b) = (Vec3::new(1.0, 2.0, 3.0), Vec3::new(-4.0, 5.0, 0.5));
        assert_eq!(a.dot(b), 7.5);
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
        assert_eq!(a.cross(b), Vec3::new(2.0 * 0.5 - 3.0 * 5.0, 3.0 * -4.0 - 0.5, 5.0 + 8.0));
        assert_eq!(Vec3::new(3.0, 4.0, 12.0).len(), 13.0);
        assert_eq!(Vec3::new(1.0, 1.0, 1.0).dist(Vec3::new(4.0, 5.0, 1.0)), 5.0);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
        assert_eq!(a.lerp(b, 0.5), (a + b) * 0.5);
        assert_eq!(a.min(b), Vec3::new(-4.0, 2.0, 0.5));
        assert_eq!(a.max(b), Vec3::new(1.0, 5.0, 3.0));
    }

    #[test]
    fn a_direction_only_from_a_finite_vector_with_length() {
        assert_eq!(Vec3::new(0.0, 0.0, 2.0).normalized(), Some(Vec3::Z));
        for v in [Vec3::ZERO, Vec3::new(1e-10, 0.0, 0.0), Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(f64::INFINITY, 0.0, 0.0)] {
            assert_eq!(v.normalized(), None, "{v:?}");
        }
        for v in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(1.0, 2.0, -3.0)] {
            let p = v.any_perp();
            assert!(p.dot(v).abs() < 1e-12 && (p.len() - 1.0).abs() < 1e-12, "{v:?} -> {p:?}");
        }
        assert_eq!(Vec3::ZERO.any_perp(), Vec3::Z);
    }

    #[test]
    fn a_box_has_a_centre_and_a_diagonal_unless_it_is_empty() {
        let b = Bounds { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(3.0, 4.0, 12.0) };
        assert!(!b.is_empty());
        assert_eq!(b.center(), Vec3::new(1.5, 2.0, 6.0));
        assert_eq!(b.diagonal(), 13.0);
        let mut grown = Bounds::EMPTY;
        for p in [Vec3::new(1.0, -2.0, 5.0), Vec3::new(f64::NAN, 9.0, 9.0), Vec3::new(-3.0, 4.0, f64::INFINITY), Vec3::new(2.0, 0.0, -1.0)] {
            grown.add(p);
        }
        assert_eq!(grown, Bounds { min: Vec3::new(1.0, -2.0, -1.0), max: Vec3::new(2.0, 0.0, 5.0) });
        let point = Bounds { min: Vec3::X, max: Vec3::X };
        assert!(!point.is_empty() && point.diagonal() == 0.0 && point.center() == Vec3::X);
        for empty in
            [Bounds::EMPTY, Bounds::default(), Bounds { min: Vec3::X, max: Vec3::ZERO }, Bounds { min: Vec3::new(f64::NAN, 0.0, 0.0), max: Vec3::X }]
        {
            assert!(empty.is_empty(), "{empty:?}");
            assert_eq!((empty.center(), empty.diagonal()), (Vec3::ZERO, 0.0));
        }
    }

    #[test]
    fn plane_products_lengths_and_blends() {
        let (a, b) = (Vec2::new(1.0, 2.0), Vec2::new(-3.0, 0.5));
        assert_eq!(a.dot(b), -2.0);
        assert_eq!(a.cross(b), 0.5 + 6.0);
        assert_eq!(Vec2::new(1.0, 0.0).cross(Vec2::new(0.0, 1.0)), 1.0);
        assert_eq!(Vec2::new(3.0, 4.0).len(), 5.0);
        assert_eq!(Vec2::new(1e200, 1e200).len(), 1e200 * std::f64::consts::SQRT_2);
        assert_eq!(a.lerp(b, 0.5), Vec2::new(-1.0, 1.25));
    }
}
