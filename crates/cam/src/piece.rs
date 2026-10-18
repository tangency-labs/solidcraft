//! Planar boundary pieces: straight lines and circular arcs, with the distance and crossing
//! queries the offsetting needs.

use std::f64::consts::TAU;

use solidcraft_geom::{Seg2, Vec2};

/// A parameter within this of 0 or 1 counts as on the piece.
const PARAM_SLACK: f64 = 1e-9;

/// Angle wrapped into `[0, 2π)`.
pub(crate) fn wrap(a: f64) -> f64 {
    let r = a.rem_euclid(TAU);
    if r.is_finite() { r } else { 0.0 }
}

/// Signed angle from `a` to `b` in `(-π, π]`.
pub(crate) fn turn(a: Vec2, b: Vec2) -> f64 {
    a.cross(b).atan2(a.dot(b))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Piece {
    Line {
        a: Vec2,
        b: Vec2,
    },
    /// Arc about `c` from angle `a0` sweeping `sweep` radians (positive = counter-clockwise).
    Arc {
        c: Vec2,
        r: f64,
        a0: f64,
        sweep: f64,
    },
}

impl Piece {
    pub fn start(&self) -> Vec2 {
        self.point_at(0.0)
    }
    pub fn end(&self) -> Vec2 {
        self.point_at(1.0)
    }
    pub fn point_at(&self, t: f64) -> Vec2 {
        match *self {
            Piece::Line { a, b } => a.lerp(b, t),
            Piece::Arc { c, r, a0, sweep } => c + Vec2::from_angle(a0 + sweep * t) * r,
        }
    }
    pub fn length(&self) -> f64 {
        match *self {
            Piece::Line { a, b } => a.dist(b),
            Piece::Arc { r, sweep, .. } => r * sweep.abs(),
        }
    }
    /// Unit direction of travel (zero for a degenerate line).
    pub fn tangent_at(&self, t: f64) -> Vec2 {
        match *self {
            Piece::Line { a, b } => (b - a).normalized().unwrap_or_default(),
            Piece::Arc { a0, sweep, .. } => Vec2::from_angle(a0 + sweep * t).perp() * sweep.signum(),
        }
    }
    pub fn reversed(&self) -> Piece {
        match *self {
            Piece::Line { a, b } => Piece::Line { a: b, b: a },
            Piece::Arc { c, r, a0, sweep } => Piece::Arc { c, r, a0: a0 + sweep, sweep: -sweep },
        }
    }
    /// The part between parameters `t0` and `t1`.
    pub fn sub(&self, t0: f64, t1: f64) -> Piece {
        match *self {
            Piece::Line { a, b } => Piece::Line { a: a.lerp(b, t0), b: a.lerp(b, t1) },
            Piece::Arc { c, r, a0, sweep } => Piece::Arc { c, r, a0: a0 + sweep * t0, sweep: sweep * (t1 - t0) },
        }
    }
    /// Distance from `p` to the nearest point of the piece.
    pub fn dist(&self, p: Vec2) -> f64 {
        match *self {
            Piece::Line { a, b } => {
                let d = b - a;
                let l2 = d.len2();
                let t = if l2 > 0.0 { ((p - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
                p.dist(a + d * t)
            }
            Piece::Arc { c, r, a0, sweep } => match arc_param(a0, sweep, (p - c).angle()) {
                Some(_) => (p.dist(c) - r).abs(),
                None => p.dist(self.start()).min(p.dist(self.end())),
            },
        }
    }
    /// A box that contains the piece (an arc's whole circle).
    pub fn bounds(&self) -> (Vec2, Vec2) {
        match *self {
            Piece::Line { a, b } => (Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y))),
            Piece::Arc { c, r, .. } => (Vec2::new(c.x - r, c.y - r), Vec2::new(c.x + r, c.y + r)),
        }
    }
    /// Half of ∮(x dy − y dx) along the piece: the pieces of a closed loop sum to its signed area.
    pub fn area_term(&self) -> f64 {
        match *self {
            Piece::Line { a, b } => 0.5 * a.cross(b),
            Piece::Arc { c, r, sweep, .. } => 0.5 * (r * r * sweep + c.cross(self.end() - self.start())),
        }
    }
    pub fn is_finite(&self) -> bool {
        match *self {
            Piece::Line { a, b } => a.is_finite() && b.is_finite(),
            Piece::Arc { c, r, a0, sweep } => c.is_finite() && r.is_finite() && a0.is_finite() && sweep.is_finite(),
        }
    }
    pub fn seg(self) -> Seg2 {
        match self {
            Piece::Line { a, b } => Seg2::Line { a, b },
            Piece::Arc { c, r, a0, sweep } => Seg2::Arc { center: c, radius: r, start: a0, sweep },
        }
    }
    /// Where the two pieces cross: `(parameter on self, parameter on o)` pairs. Pieces lying
    /// along each other (collinear lines, arcs of one circle) report nothing.
    pub fn intersections(&self, o: &Piece, out: &mut Vec<(f64, f64)>) {
        match (*self, *o) {
            (Piece::Line { a: a1, b: b1 }, Piece::Line { a: a2, b: b2 }) => {
                let (d1, d2) = (b1 - a1, b2 - a2);
                let den = d1.cross(d2);
                if !(den.abs() > 1e-12 * d1.len() * d2.len()) {
                    return;
                }
                let w = a2 - a1;
                let (t, u) = (w.cross(d2) / den, w.cross(d1) / den);
                if on_piece(t) && on_piece(u) {
                    out.push((t.clamp(0.0, 1.0), u.clamp(0.0, 1.0)));
                }
            }
            (Piece::Line { a, b }, Piece::Arc { c, r, a0, sweep }) => {
                for (t, p) in line_circle(a, b, c, r) {
                    if let Some(u) = arc_param(a0, sweep, (p - c).angle()) {
                        out.push((t, u));
                    }
                }
            }
            (Piece::Arc { .. }, Piece::Line { .. }) => {
                let mut swapped = Vec::new();
                o.intersections(self, &mut swapped);
                out.extend(swapped.into_iter().map(|(u, t)| (t, u)));
            }
            (Piece::Arc { c: c1, r: r1, a0: s1, sweep: w1 }, Piece::Arc { c: c2, r: r2, a0: s2, sweep: w2 }) => {
                let between = c2 - c1;
                let d = between.len();
                if !(d > 1e-12) || d > r1 + r2 + 1e-9 || d < (r1 - r2).abs() - 1e-9 {
                    return;
                }
                let along = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
                let h = (r1 * r1 - along * along).max(0.0).sqrt();
                let dir = between / d;
                let base = c1 + dir * along;
                let pts = if h > 1e-12 { vec![base + dir.perp() * h, base - dir.perp() * h] } else { vec![base] };
                for p in pts {
                    if let (Some(t), Some(u)) = (arc_param(s1, w1, (p - c1).angle()), arc_param(s2, w2, (p - c2).angle())) {
                        out.push((t, u));
                    }
                }
            }
        }
    }
}

fn on_piece(t: f64) -> bool {
    (-PARAM_SLACK..=1.0 + PARAM_SLACK).contains(&t)
}

/// The parameter at which an arc from `a0` sweeping `sweep` passes angle `theta`.
fn arc_param(a0: f64, sweep: f64, theta: f64) -> Option<f64> {
    let span = sweep.abs();
    if !(span > 0.0) {
        return None;
    }
    let delta = if sweep > 0.0 { wrap(theta - a0) } else { wrap(a0 - theta) };
    if delta <= span + PARAM_SLACK {
        Some((delta / span).min(1.0))
    } else if delta >= TAU - PARAM_SLACK {
        // Just before the start: the angle wrapped to nearly a full turn.
        Some(0.0)
    } else {
        None
    }
}

/// Where the segment `a`–`b` meets the circle: `(parameter, point)`.
fn line_circle(a: Vec2, b: Vec2, c: Vec2, r: f64) -> Vec<(f64, Vec2)> {
    let d = b - a;
    let len = d.len();
    if !(len > 1e-12) {
        return Vec::new();
    }
    let f = a - c;
    let h = d.cross(f).abs() / len;
    if h > r + 1e-9 {
        return Vec::new();
    }
    let mid = -f.dot(d) / (len * len);
    let half = (r * r - h * h).max(0.0).sqrt() / len;
    let ts = if half * len > 1e-12 { vec![mid - half, mid + half] } else { vec![mid] };
    ts.into_iter().filter(|t| on_piece(*t)).map(|t| (t.clamp(0.0, 1.0), a + d * t.clamp(0.0, 1.0))).collect()
}
