//! The tool as the simulation cuts with it: its cutting end's height over its tip at each
//! distance from its axis, with an endless cylinder of its diameter above.

use super::*;

/// Iterations of the bisections (each halves the interval).
pub(super) const BISECTIONS: usize = 60;
/// A slope taken as vertical (a tool's flank at its rim).
const STEEP: f64 = 1.0e12;

/// The cutting end's shape for the simulation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Shape {
    Flat,
    Ball,
    Bull,
    /// A point: height `slope·d` at distance `d` from the axis.
    Cone(f64),
    /// A flat tip of radius `tip`, then a cone: height `slope·(d - tip)` beyond it (a chamfer
    /// mill).
    Taper {
        slope: f64,
        tip: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Profile {
    pub(crate) radius: f64,
    pub(crate) corner: f64,
    pub(super) shape: Shape,
}

impl Profile {
    pub(crate) fn of(tool: &Tool) -> Result<Profile> {
        tool.validate()?;
        let r = tool.radius();
        Ok(match tool.kind {
            // A thread mill is checked as its whole radius, as the gouge check does.
            ToolKind::FlatEndmill | ToolKind::ThreadMill => Profile { radius: r, corner: 0.0, shape: Shape::Flat },
            ToolKind::BallEndmill => Profile { radius: r, corner: r, shape: Shape::Ball },
            ToolKind::BullNose | ToolKind::FaceMill => {
                let c = tool.corner_radius.unwrap_or(0.0).clamp(0.0, r);
                if c <= 0.0 {
                    Profile { radius: r, corner: 0.0, shape: Shape::Flat }
                } else if c >= r {
                    Profile { radius: r, corner: r, shape: Shape::Ball }
                } else {
                    Profile { radius: r, corner: c, shape: Shape::Bull }
                }
            }
            ToolKind::Drill => Profile { radius: r, corner: 0.0, shape: Shape::Cone(1.0 / (crate::drill::point_angle(tool) * 0.5).tan()) },
            ToolKind::Chamfer => {
                let half = tool.angle_deg.unwrap_or(90.0).to_radians() * 0.5;
                let tip = (tool.tip_diameter.unwrap_or(0.0) * 0.5).clamp(0.0, r);
                Profile { radius: r, corner: 0.0, shape: Shape::Taper { slope: 1.0 / half.tan(), tip } }
            }
        })
    }

    /// Height of the cutting end above the tip at distance `d` from the axis (`d` taken within
    /// the radius).
    pub(crate) fn height(&self, d: f64) -> f64 {
        let d = d.clamp(0.0, self.radius);
        match self.shape {
            Shape::Flat => 0.0,
            Shape::Cone(k) => k * d,
            Shape::Taper { slope, tip } => slope * (d - tip).max(0.0),
            Shape::Ball | Shape::Bull => {
                let a = self.radius - self.corner;
                if d <= a {
                    return 0.0;
                }
                let e = (d - a).min(self.corner);
                self.corner - (self.corner * self.corner - e * e).max(0.0).sqrt()
            }
        }
    }

    /// The tool's radius at height `h` above its tip (its endless cylinder above included):
    /// the farthest from the axis [`Profile::height`] is at most `h`; `None` below the tip.
    pub(crate) fn section(&self, h: f64) -> Option<f64> {
        if !(h >= 0.0) {
            return None;
        }
        let r = self.radius;
        Some(match self.shape {
            Shape::Flat => r,
            Shape::Cone(k) => {
                if k > 0.0 {
                    (h / k).min(r)
                } else {
                    r
                }
            }
            Shape::Taper { slope, tip } => {
                if slope > 0.0 {
                    (tip + h / slope).min(r)
                } else {
                    r
                }
            }
            Shape::Ball | Shape::Bull => {
                let c = self.corner;
                if h >= c { r } else { r - c + (c * c - (c - h) * (c - h)).max(0.0).sqrt() }
            }
        })
    }

    /// The slope of [`Profile::height`] at `d`, [`STEEP`] at the rim.
    pub(super) fn slope(&self, d: f64) -> f64 {
        match self.shape {
            Shape::Flat => 0.0,
            Shape::Cone(k) => k,
            Shape::Taper { slope, tip } => {
                if d <= tip {
                    0.0
                } else {
                    slope
                }
            }
            Shape::Ball | Shape::Bull => {
                let e = d - (self.radius - self.corner);
                if e <= 0.0 {
                    return 0.0;
                }
                let q = self.corner * self.corner - e * e;
                if q <= 0.0 { STEEP } else { (e / q.sqrt()).min(STEEP) }
            }
        }
    }

    /// Largest distance between tool positions on a 4th axis that leaves cusps of at most half
    /// of `tol` (module documentation).
    pub(crate) fn step(&self, tol: f64) -> f64 {
        let s = match self.shape {
            Shape::Flat => (4.0 * self.radius * tol).sqrt(),
            Shape::Ball | Shape::Bull => (4.0 * self.corner * tol).sqrt(),
            Shape::Cone(k) | Shape::Taper { slope: k, .. } => tol / k.max(1e-9),
        };
        s.clamp(1e-3, self.radius.max(1e-3))
    }

    /// The least height of the tool's lower envelope over a cell as the tip moves straight from
    /// `z0` (at 0) to `z0 + dz` (at `len`) along a line, the cell at `u` along it and `v` (≥ 0)
    /// to its side; `None` when the tool never covers the cell. Exact: the height is convex
    /// along the move, its least value on the covered stretch is where its slope is zero.
    pub(crate) fn along_line(&self, u: f64, v: f64, len: f64, z0: f64, dz: f64) -> Option<f64> {
        let r = self.radius;
        if v > r {
            return None;
        }
        if len <= 1e-12 {
            let d = hyp(u, v);
            return (d <= r).then(|| z0.min(z0 + dz) + self.height(d));
        }
        let w = (r * r - v * v).max(0.0).sqrt();
        let (lo, hi) = ((u - w).max(0.0), (u + w).min(len));
        if lo > hi {
            return None;
        }
        let m = dz / len;
        let s = if hi - lo <= 1e-12 {
            lo
        } else {
            match self.shape {
                Shape::Flat => {
                    if m > 0.0 {
                        lo
                    } else {
                        hi
                    }
                }
                Shape::Ball => (u - dz * w / hyp(len, dz)).clamp(lo, hi),
                Shape::Cone(k) => {
                    if m.abs() >= k {
                        if m > 0.0 { lo } else { hi }
                    } else {
                        let c = -m / k;
                        (u + c * v / (1.0 - c * c).sqrt()).clamp(lo, hi)
                    }
                }
                Shape::Bull | Shape::Taper { .. } => {
                    let slope = |s: f64| {
                        let t = s - u;
                        let d = hyp(t, v);
                        if d <= 1e-15 { m } else { m + self.slope(d) * t / d }
                    };
                    if slope(lo) >= 0.0 {
                        lo
                    } else if slope(hi) <= 0.0 {
                        hi
                    } else {
                        let (mut a, mut b) = (lo, hi);
                        for _ in 0..BISECTIONS {
                            let mid = 0.5 * (a + b);
                            if slope(mid) < 0.0 {
                                a = mid;
                            } else {
                                b = mid;
                            }
                        }
                        0.5 * (a + b)
                    }
                }
            }
        };
        Some(z0 + m * s + self.height(hyp(s - u, v)))
    }
}
