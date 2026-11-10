//! The geometry of vectors and points ([`Vec2`], [`Vec3`], plain data in the record): dot and
//! cross products, lengths, distances, directions and interpolation.
use solidcore_record::{Vec2, Vec3};

use crate::UnitVec3;

/// The geometry of a 2D vector or point.
// `len` is the vector's length, not a count.
#[allow(clippy::len_without_is_empty)]
pub trait Vec2Geom {
    fn dot(self, o: Vec2) -> f64;
    /// z component of the 3D cross product.
    fn cross(self, o: Vec2) -> f64;
    fn len(self) -> f64;
    fn len2(self) -> f64;
    fn dist(self, o: Vec2) -> f64;
    fn normalized(self) -> Option<Vec2>;
    /// Rotated +90°.
    fn perp(self) -> Vec2;
    fn angle(self) -> f64;
    fn from_angle(a: f64) -> Vec2;
    fn lerp(self, o: Vec2, t: f64) -> Vec2;
}

impl Vec2Geom for Vec2 {
    fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    fn len2(self) -> f64 {
        self.dot(self)
    }
    fn dist(self, o: Vec2) -> f64 {
        (self - o).len()
    }
    fn normalized(self) -> Option<Vec2> {
        let l = self.len();
        (l > crate::EPS && l.is_finite()).then(|| self / l)
    }
    fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }
    fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }
    fn from_angle(a: f64) -> Vec2 {
        Vec2::new(a.cos(), a.sin())
    }
    fn lerp(self, o: Vec2, t: f64) -> Vec2 {
        self + (o - self) * t
    }
}

/// The geometry of a 3D vector or point.
// `len` is the vector's length, not a count.
#[allow(clippy::len_without_is_empty)]
pub trait Vec3Geom {
    /// Takes a [`Vec3`] or a [`UnitVec3`](crate::UnitVec3).
    fn dot(self, o: impl Into<Vec3>) -> f64;
    /// Takes a [`Vec3`] or a [`UnitVec3`](crate::UnitVec3).
    fn cross(self, o: impl Into<Vec3>) -> Vec3;
    fn len(self) -> f64;
    fn len2(self) -> f64;
    fn dist(self, o: Vec3) -> f64;
    fn normalized(self) -> Option<Vec3>;
    fn lerp(self, o: Vec3, t: f64) -> Vec3;
    fn min(self, o: Vec3) -> Vec3;
    fn max(self, o: Vec3) -> Vec3;
    /// Any unit vector perpendicular to `self` (which should be non-zero).
    fn any_perp(self) -> Vec3;
    /// Distance from this point to the segment `a`–`b`.
    fn dist_to_segment(self, a: Vec3, b: Vec3) -> f64;
    /// This vector scaled to length 1 ([`UnitVec3::new`]).
    fn unit(self) -> Option<UnitVec3>;
}

impl Vec3Geom for Vec3 {
    fn dot(self, o: impl Into<Vec3>) -> f64 {
        let o = o.into();
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    fn cross(self, o: impl Into<Vec3>) -> Vec3 {
        let o = o.into();
        Vec3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    fn len(self) -> f64 {
        self.len2().sqrt()
    }
    fn len2(self) -> f64 {
        self.dot(self)
    }
    fn dist(self, o: Vec3) -> f64 {
        (self - o).len()
    }
    fn normalized(self) -> Option<Vec3> {
        let l = self.len();
        (l > crate::EPS && l.is_finite()).then(|| self / l)
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
        let t = if l2 > 0.0 { ((self - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
        self.dist(a + ab * t)
    }
    fn unit(self) -> Option<UnitVec3> {
        UnitVec3::new(self)
    }
}
