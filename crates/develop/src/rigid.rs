//! Rigid motions of the plane, applied exactly to every kind of profile segment.

use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};

/// A rigid motion of the plane: a turn about the origin, then a shift. It never reflects, so a
/// panel keeps its handedness and an arc its sense of turning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rigid2 {
    pub cos: f64,
    pub sin: f64,
    pub shift: Vec2,
}

impl Rigid2 {
    pub const IDENTITY: Rigid2 = Rigid2 { cos: 1.0, sin: 0.0, shift: Vec2::ZERO };

    /// The turn by `angle` radians about the origin.
    pub fn turn(angle: f64) -> Rigid2 {
        Rigid2 { cos: angle.cos(), sin: angle.sin(), shift: Vec2::ZERO }
    }

    /// The motion that puts the point `from` on `to` and turns the direction `from_dir` to
    /// `to_dir`. `None` when either direction has no length.
    pub fn onto(from: Vec2, from_dir: Vec2, to: Vec2, to_dir: Vec2) -> Option<Rigid2> {
        let (f, t) = (from_dir.normalized()?, to_dir.normalized()?);
        let (cos, sin) = (f.dot(t), f.cross(t));
        let turned = Rigid2 { cos, sin, shift: Vec2::ZERO };
        Some(Rigid2 { shift: to - turned.vector(from), ..turned })
    }

    /// The turn this motion makes, in radians.
    pub fn angle(&self) -> f64 {
        self.sin.atan2(self.cos)
    }

    pub fn vector(&self, v: Vec2) -> Vec2 {
        Vec2::new(self.cos * v.x - self.sin * v.y, self.sin * v.x + self.cos * v.y)
    }

    pub fn point(&self, p: Vec2) -> Vec2 {
        self.vector(p) + self.shift
    }

    /// This motion, then `next`.
    pub fn then(&self, next: &Rigid2) -> Rigid2 {
        let turn = Rigid2 { shift: Vec2::ZERO, ..*next };
        Rigid2 {
            cos: next.cos * self.cos - next.sin * self.sin,
            sin: next.sin * self.cos + next.cos * self.sin,
            shift: turn.vector(self.shift) + next.shift,
        }
    }

    pub fn inverse(&self) -> Rigid2 {
        let back = Rigid2 { cos: self.cos, sin: -self.sin, shift: Vec2::ZERO };
        Rigid2 { shift: -back.vector(self.shift), ..back }
    }

    /// A segment moved: control points move, and an arc keeps its radius and sweep and turns
    /// its start angle. Exact for every kind of segment, since a rigid motion maps each one to
    /// one of its own kind.
    pub fn seg(&self, s: &Seg2) -> Seg2 {
        match *s {
            Seg2::Line { a, b } => Seg2::Line { a: self.point(a), b: self.point(b) },
            Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center: self.point(center), radius, start: start + self.angle(), sweep },
            Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: self.point(p0), p1: self.point(p1), p2: self.point(p2), p3: self.point(p3) },
            Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: self.point(a), apex: self.point(apex), b: self.point(b), w },
            Seg2::Bezier { n, mut p } => {
                // Only the first n + 1 points are the curve's; the unused tail stays zero.
                for q in p.iter_mut().take(usize::from(n) + 1) {
                    *q = self.point(*q);
                }
                Seg2::Bezier { n, p }
            }
        }
    }

    pub fn loop2(&self, l: &Loop2) -> Loop2 {
        Loop2 { segs: l.segs.iter().map(|s| self.seg(s)).collect() }
    }

    pub fn region(&self, r: &Region2) -> Region2 {
        Region2 { outer: self.loop2(&r.outer), holes: r.holes.iter().map(|h| self.loop2(h)).collect() }
    }
}
