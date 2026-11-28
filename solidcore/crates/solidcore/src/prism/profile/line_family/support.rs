//! Exact scalar operations used by hatch admission. In particular, normalization
//! uses component division, never Vec2's legacy negligible-divisor convention.
use crate::math::det;
use crate::math::Vec2;
use crate::prism::profile::{Loop2, Seg2};
use std::f64::consts::{PI, TAU};

pub(super) trait HatchVec {
    fn is_finite(&self) -> bool;
    fn len(self) -> f64;
    fn perp(self) -> Vec2;
    fn from_angle(angle: f64) -> Vec2;
    #[cfg(test)]
    fn dist(self, other: Vec2) -> f64;
}
impl HatchVec for Vec2 {
    fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
    fn len(self) -> f64 {
        det::hypot(self.x, self.y)
    }
    fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }
    fn from_angle(angle: f64) -> Vec2 {
        Vec2::new(det::cos(angle), det::sin(angle))
    }
    #[cfg(test)]
    fn dist(self, other: Vec2) -> f64 {
        det::hypot(self.x - other.x, self.y - other.y)
    }
}

pub(super) trait HatchSeg {
    fn check(&self, max_abs: f64) -> Result<(), &'static str>;
}
impl HatchSeg for Seg2 {
    fn check(&self, max_abs: f64) -> Result<(), &'static str> {
        let valid = |p: Vec2| p.is_finite() && p.x.abs() <= max_abs && p.y.abs() <= max_abs;
        match *self {
            Seg2::Line { a, b } => {
                if !valid(a) || !valid(b) {
                    return Err("non-finite or excessive coordinate");
                }
                if a == b {
                    return Err("collapsed curve");
                }
            }
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                if !valid(center) {
                    return Err("non-finite or excessive coordinate");
                }
                if !radius.is_finite() || !(1e-12..=max_abs).contains(&radius) {
                    return Err("arc radius");
                }
                if !start.is_finite() || start.abs() > 1e6 {
                    return Err("arc start angle");
                }
                if !sweep.is_finite() || !(1e-12..=TAU + 1e-12).contains(&sweep.abs()) {
                    return Err("arc sweep");
                }
            }
            _ => return Err("not supported yet: free-form hatch boundary"),
        }
        if !self.start().is_finite() || !self.end().is_finite() {
            return Err("non-finite curve endpoint");
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) trait HatchLoop {
    fn circle(center: Vec2, radius: f64) -> Loop2;
    fn circle_from(center: Vec2, radius: f64, start: f64) -> Loop2;
}
#[cfg(test)]
impl HatchLoop for Loop2 {
    fn circle(center: Vec2, radius: f64) -> Loop2 {
        Self::circle_from(center, radius, 0.0)
    }
    fn circle_from(center: Vec2, radius: f64, start: f64) -> Loop2 {
        Loop2 {
            segs: vec![
                Seg2::Arc {
                    center,
                    radius,
                    start,
                    sweep: PI,
                },
                Seg2::Arc {
                    center,
                    radius,
                    start: start + PI,
                    sweep: PI,
                },
            ],
        }
    }
}

/// A finite, representable disk for a hatch dot's physical pen diameter.
pub fn dot_region(at: Vec2, width: f64) -> Option<crate::prism::profile::Region2> {
    let radius = width * 0.5;
    if !at.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
        || at.x + radius <= at.x
        || at.x - radius >= at.x
        || at.y + radius <= at.y
        || at.y - radius >= at.y
    {
        return None;
    }
    Some(crate::prism::profile::Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Arc {
                    center: at,
                    radius,
                    start: 0.0,
                    sweep: PI,
                },
                Seg2::Arc {
                    center: at,
                    radius,
                    start: PI,
                    sweep: PI,
                },
            ],
        },
        holes: Vec::new(),
    })
}
