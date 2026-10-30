//! A box in the plane, the 2D [`crate::Aabb3`].

use crate::Vec2;

/// Axis-aligned box in the plane; [`Aabb2::EMPTY`] (low +∞, high −∞) holds nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb2 {
    pub min: Vec2,
    pub max: Vec2,
}

impl Default for Aabb2 {
    fn default() -> Self {
        Aabb2::EMPTY
    }
}

impl Aabb2 {
    pub const EMPTY: Aabb2 = Aabb2 { min: Vec2::new(f64::INFINITY, f64::INFINITY), max: Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY) };
    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y)
    }
    /// Grow to hold `p`; a point that is not finite is skipped.
    pub fn add(&mut self, p: Vec2) {
        if p.is_finite() {
            self.min = Vec2::new(self.min.x.min(p.x), self.min.y.min(p.y));
            self.max = Vec2::new(self.max.x.max(p.x), self.max.y.max(p.y));
        }
    }
    pub fn union(&self, o: &Aabb2) -> Aabb2 {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Aabb2 { min: Vec2::new(self.min.x.min(o.min.x), self.min.y.min(o.min.y)), max: Vec2::new(self.max.x.max(o.max.x), self.max.y.max(o.max.y)) }
    }
    pub fn center(&self) -> Vec2 {
        if self.is_empty() { Vec2::ZERO } else { (self.min + self.max) * 0.5 }
    }
    pub fn size(&self) -> Vec2 {
        if self.is_empty() { Vec2::ZERO } else { self.max - self.min }
    }
    pub fn diagonal(&self) -> f64 {
        self.size().len()
    }
    /// The box grown by `by` on every side; an empty box stays empty.
    pub fn inflated(&self, by: f64) -> Aabb2 {
        if self.is_empty() {
            return *self;
        }
        let d = Vec2::new(by, by);
        Aabb2 { min: self.min - d, max: self.max + d }
    }
    /// Whether `p` lies in the box, its sides included (never in an empty box, never for NaN).
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.y >= self.min.y && p.x <= self.max.x && p.y <= self.max.y
    }
    /// The box both hold: empty when they do not meet.
    pub fn intersection(&self, o: &Aabb2) -> Aabb2 {
        Aabb2 { min: Vec2::new(self.min.x.max(o.min.x), self.min.y.max(o.min.y)), max: Vec2::new(self.max.x.min(o.max.x), self.max.y.min(o.max.y)) }
    }
    /// Whether the two boxes share a point (never when either is empty).
    pub fn meets(&self, o: &Aabb2) -> bool {
        self.min.x <= o.max.x && o.min.x <= self.max.x && self.min.y <= o.max.y && o.min.y <= self.max.y
    }
    /// Whether the two boxes share some area, not only an edge or a corner (never when either is
    /// empty).
    pub fn overlaps(&self, o: &Aabb2) -> bool {
        self.min.x < o.max.x && o.min.x < self.max.x && self.min.y < o.max.y && o.min.y < self.max.y
    }
    /// Where `p` sits in the box: (0, 0) at `min`, (1, 1) at `max`, each side taken as at least
    /// 1e-9 long so a flat box still divides.
    pub fn relative(&self, p: Vec2) -> Vec2 {
        Vec2::new((p.x - self.min.x) / (self.max.x - self.min.x).max(1e-9), (p.y - self.min.y) / (self.max.y - self.min.y).max(1e-9))
    }
}

/// The box around the finite points (non-finite ones are skipped, as [`Aabb2::add`] does).
impl FromIterator<Vec2> for Aabb2 {
    fn from_iter<I: IntoIterator<Item = Vec2>>(points: I) -> Aabb2 {
        let mut b = Aabb2::EMPTY;
        b.extend(points);
        b
    }
}

impl Extend<Vec2> for Aabb2 {
    fn extend<I: IntoIterator<Item = Vec2>>(&mut self, points: I) {
        for p in points {
            self.add(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_from_points_holds_them_and_grows() {
        let b: Aabb2 = [Vec2::new(1.0, 2.0), Vec2::new(-1.0, 0.0), Vec2::new(f64::INFINITY, 0.0)].into_iter().collect();
        assert_eq!((b.min, b.max), (Vec2::new(-1.0, 0.0), Vec2::new(1.0, 2.0)), "the infinite point is skipped");
        assert!(b.contains(Vec2::new(0.0, 1.0)) && !b.contains(Vec2::new(0.0, 3.0)) && !b.contains(Vec2::new(f64::NAN, 1.0)));
        assert!(b.inflated(1.0).contains(Vec2::new(0.0, 3.0)));
        assert!(Aabb2::EMPTY.inflated(1e9).is_empty() && !Aabb2::EMPTY.contains(Vec2::ZERO));
        let far = Aabb2 { min: Vec2::new(2.0, 0.0), max: Vec2::new(3.0, 1.0) };
        assert!(!b.meets(&far) && b.inflated(1.0).meets(&far) && !b.meets(&Aabb2::EMPTY));
        let beside = Aabb2 { min: Vec2::new(1.0, 0.0), max: Vec2::new(2.0, 2.0) };
        assert!(b.meets(&beside) && !b.overlaps(&beside), "an edge shared is no overlap");
        assert!(b.inflated(0.5).overlaps(&beside) && !b.overlaps(&Aabb2::EMPTY) && !Aabb2::EMPTY.overlaps(&b));
        assert_eq!(b.union(&far).max, Vec2::new(3.0, 2.0));
        assert_eq!(b.relative(Vec2::new(0.0, 1.0)), Vec2::new(0.5, 0.5));
    }
}
