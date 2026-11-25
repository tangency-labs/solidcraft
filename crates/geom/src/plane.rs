use solidcore_record::{Plane, Vec2, Vec3};

use crate::Vec3Geom;

/// The geometry of a sketch plane ([`Plane`]): made from directions, and mapping points to and
/// from it.
// `new` makes a plane, or none from degenerate axes.
#[allow(clippy::new_ret_no_self)]
pub trait PlaneGeom {
    /// Plane through `origin` with the given axes, re-orthonormalised. `None` if degenerate.
    fn new(origin: Vec3, x: Vec3, y: Vec3) -> Option<Plane>;
    /// Plane through `origin` with normal `n` (in-plane axes chosen deterministically).
    fn from_normal(origin: Vec3, n: impl Into<Vec3>) -> Option<Plane>;
    /// The same plane seen from its other side: a half turn about the sketch x axis (y
    /// reversed), or about the y axis (x reversed) when `about_y`. Still right-handed.
    fn flipped(&self, about_y: bool) -> Plane;
    fn normal(&self) -> Vec3;
    fn to_world(&self, p: Vec2) -> Vec3;
    fn dir_to_world(&self, d: Vec2) -> Vec3;
    fn to_local(&self, p: Vec3) -> Vec2;
    /// Signed distance of `p` above the plane (along the normal).
    fn height(&self, p: Vec3) -> f64;
    /// The plane moved along its normal by `d`.
    fn offset(&self, d: f64) -> Plane;
    /// Intersection of the ray `o + t·dir` with the plane (t may be negative).
    fn intersect_ray(&self, o: Vec3, dir: Vec3) -> Option<Vec3>;
}

impl PlaneGeom for Plane {
    fn new(origin: Vec3, x: Vec3, y: Vec3) -> Option<Plane> {
        let x = x.normalized()?;
        let n = x.cross(y).normalized()?;
        let y = n.cross(x).normalized()?;
        origin.is_finite().then_some(Plane { origin, x, y })
    }
    fn from_normal(origin: Vec3, n: impl Into<Vec3>) -> Option<Plane> {
        let n = n.into().normalized()?;
        // Horizontal planes keep world X as sketch x; others use the horizontal direction Z × n.
        let cand = if n.dot(Vec3::Z).abs() > 0.9 { Vec3::X } else { Vec3::Z.cross(n) };
        let x = (cand - n * cand.dot(n)).normalized()?;
        Plane::new(origin, x, n.cross(x))
    }
    fn flipped(&self, about_y: bool) -> Plane {
        if about_y { Plane { x: -self.x, ..*self } } else { Plane { y: -self.y, ..*self } }
    }
    fn normal(&self) -> Vec3 {
        self.x.cross(self.y)
    }
    fn to_world(&self, p: Vec2) -> Vec3 {
        self.origin + self.x * p.x + self.y * p.y
    }
    fn dir_to_world(&self, d: Vec2) -> Vec3 {
        self.x * d.x + self.y * d.y
    }
    fn to_local(&self, p: Vec3) -> Vec2 {
        let d = p - self.origin;
        Vec2::new(d.dot(self.x), d.dot(self.y))
    }
    fn height(&self, p: Vec3) -> f64 {
        (p - self.origin).dot(self.normal())
    }
    fn offset(&self, d: f64) -> Plane {
        Plane { origin: self.origin + self.normal() * d, ..*self }
    }
    fn intersect_ray(&self, o: Vec3, dir: Vec3) -> Option<Vec3> {
        let n = self.normal();
        let den = dir.dot(n);
        if den.abs() < 1e-12 {
            return None;
        }
        let t = (self.origin - o).dot(n) / den;
        t.is_finite().then(|| o + dir * t)
    }
}
