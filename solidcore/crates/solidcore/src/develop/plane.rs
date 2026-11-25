//! The plane's vector measures and the box the development reads: dot and cross products,
//! lengths, directions, interpolation, and a box that grows round points.
//!
//! Not upstream (SolidCraft's sheet development, moved into the kernel, docs/layers.md §1).
//! Each formula is the one the development was written and tested with: a length is `hypot`, a
//! direction the vector over its length where that length is above [`DIRECTION_TOL`] and finite.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::math::Vec2;

/// Below this length (mm) a vector has no direction ([`Planar::normalized`]).
pub(crate) const DIRECTION_TOL: f64 = 1e-9;

/// The unit vector along x.
pub(crate) const X: Vec2 = Vec2::new(1.0, 0.0);
/// The unit vector along y.
pub(crate) const Y: Vec2 = Vec2::new(0.0, 1.0);

/// The vector measures of the plane on [`Vec2`].
#[allow(clippy::len_without_is_empty)] // a vector's length, not a count
pub(crate) trait Planar: Sized {
    /// The z component of the cross product.
    fn cross(self, o: Self) -> f64;
    /// The length.
    fn len(self) -> f64;
    /// The squared length.
    fn len2(self) -> f64;
    /// The distance to `o`.
    fn dist(self, o: Self) -> f64;
    /// The unit vector, `None` when not longer than [`DIRECTION_TOL`] or not finite.
    fn normalized(self) -> Option<Self>;
    /// Turned a quarter turn counter-clockwise.
    fn perp(self) -> Self;
    /// Its angle from x, in radians.
    fn angle(self) -> f64;
    /// The point a fraction `t` of the way to `o`.
    fn lerp(self, o: Self, t: f64) -> Self;
    /// Each coordinate divided by `d`.
    fn over(self, d: f64) -> Self;
    /// Every coordinate is finite.
    #[allow(clippy::wrong_self_convention)] // a copy type, as `f64::is_finite`
    fn is_finite(self) -> bool;
}

impl Planar for Vec2 {
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
        (l > DIRECTION_TOL && l.is_finite()).then(|| self.over(l))
    }
    fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }
    fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }
    fn lerp(self, o: Vec2, t: f64) -> Vec2 {
        self + (o - self) * t
    }
    fn over(self, d: f64) -> Vec2 {
        Vec2::new(self.x / d, self.y / d)
    }
    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// The unit vector at `a` radians from x.
pub(crate) fn from_angle(a: f64) -> Vec2 {
    Vec2::new(a.cos(), a.sin())
}

/// A box in the plane, grown round finite points; empty (its least corner above its greatest)
/// until it holds one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box2 {
    /// The least corner.
    pub min: Vec2,
    /// The greatest corner.
    pub max: Vec2,
}

impl Box2 {
    /// The box that holds nothing.
    pub const EMPTY: Box2 = Box2 {
        min: Vec2::new(f64::INFINITY, f64::INFINITY),
        max: Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    };

    /// Whether it holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y)
    }

    /// Grown to hold `p`; a point that is not finite is left out.
    pub fn add(&mut self, p: Vec2) {
        if p.is_finite() {
            self.min = Vec2::new(self.min.x.min(p.x), self.min.y.min(p.y));
            self.max = Vec2::new(self.max.x.max(p.x), self.max.y.max(p.y));
        }
    }

    /// The box that holds both.
    #[must_use]
    pub fn union(&self, o: &Box2) -> Box2 {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Box2 {
            min: Vec2::new(self.min.x.min(o.min.x), self.min.y.min(o.min.y)),
            max: Vec2::new(self.max.x.max(o.max.x), self.max.y.max(o.max.y)),
        }
    }

    /// Its sides, nothing for an empty box.
    #[must_use]
    pub fn size(&self) -> Vec2 {
        if self.is_empty() {
            Vec2::ZERO
        } else {
            self.max - self.min
        }
    }

    /// The length of its diagonal.
    #[must_use]
    pub fn diagonal(&self) -> f64 {
        self.size().len()
    }

    /// Grown by `by` every way; an empty box stays empty.
    #[must_use]
    pub fn inflated(&self, by: f64) -> Box2 {
        if self.is_empty() {
            return *self;
        }
        let d = Vec2::new(by, by);
        Box2 {
            min: self.min - d,
            max: self.max + d,
        }
    }

    /// Whether the two boxes share a point.
    #[must_use]
    pub fn meets(&self, o: &Box2) -> bool {
        self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
    }
}

impl FromIterator<Vec2> for Box2 {
    fn from_iter<I: IntoIterator<Item = Vec2>>(points: I) -> Box2 {
        let mut b = Box2::EMPTY;
        for p in points {
            b.add(p);
        }
        b
    }
}
