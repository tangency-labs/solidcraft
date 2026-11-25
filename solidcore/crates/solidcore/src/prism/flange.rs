//! How far a flange reaches from its bend: in a flat pattern, the least distance, measured square
//! to the bend line, from the edge of the bend's zone across a panel to the panel's far boundary,
//! taken over the length of the bend (#20113).
//!
//! A press brake cannot form a flange shorter than the shop's minimum, so the number a shop's rule
//! is held against is the shortest of the reaches from every point of the bend: a ray from the
//! zone's edge, square to the bend line, runs through the panel's material until it leaves the
//! panel's outer boundary. Cut-outs inside the panel (its holes) do not stop it; a cut-out that
//! reaches the edge is part of the outer boundary, and does.
//!
//! The least over the bend is a closed form: the shortest reach is the lowest point of the
//! boundary above the zone's edge within the bend's length, so every line contributes at the ends
//! of the part of it over the bend and every arc at its ends, its lowest point and where it
//! crosses the bend's ends. A boundary of free-form curves has none (`None`, never a guess).
//!
//! A ray square to the bend that starts where the zone's edge has no material (a notch cut into the
//! bend, a relief) is not counted: only the stretches of the zone's edge the panel stands on are.
//! A wall slanting in over the panel, or a rounded or chamfered corner, shortens the reaches near
//! it to nothing, so `inset` leaves that much of each end of the bend out.
//!
//! Not upstream: SolidCraft's sheet metal shop check, in the kernel so the layers above compute no
//! geometry (docs/layers.md §1). The panels (the flat less the bends' zones) are the planar
//! boolean `solidcore-ops` makes.
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests, kept as written

use std::f64::consts::TAU;

use super::profile::{Region2, Seg2};
use crate::math::{Point2, Vec2};

/// How near a point is to the zone's edge line to lie on it (mm).
const ON_EDGE: f64 = 1e-6;

/// The most segments of a boundary the query reads.
const MAX_SEGMENTS: usize = 20_000;

/// A flange's reach from its bend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlangeReach {
    /// From the edge of the bend's zone (where the flat sheet ends and the bend begins) (mm).
    pub from_edge: f64,
    /// From the bend's centre line (mm): the reach from the edge and the zone's half width.
    pub from_line: f64,
}

/// The bend's frame: its centre line starts at `origin` and runs along `u`; `n` is the side the
/// panel is on; `edge` is the distance from the centre line to the zone's edge on that side.
struct Bend {
    origin: Point2,
    u: Vec2,
    n: Vec2,
    edge: f64,
}

impl Bend {
    /// Where along the bend `p` is.
    fn along(&self, p: Point2) -> f64 {
        (p - self.origin).dot(self.u)
    }

    /// How far `p` is above the zone's edge, toward the panel.
    fn above(&self, p: Point2) -> f64 {
        (p - self.origin).dot(self.n) - self.edge
    }
}

/// The point of the unit circle about the origin at `theta` in the bend's frame, as an angle in the
/// plane.
fn plane_angle(b: &Bend, theta: f64) -> f64 {
    let v = b.u * theta.cos() + b.n * theta.sin();
    v.y.atan2(v.x)
}

fn on_arc(ang: f64, start: f64, sweep: f64) -> bool {
    ((ang - start) * sweep.signum()).rem_euclid(TAU) <= sweep.abs()
}

/// Whether a boundary reached at `s`, running on in the direction whose `u` component is `ds`
/// (zero when it is not at an end), is over the stretch `[x, y]` of the zone's edge and not only
/// touching its end from outside.
fn over(s: f64, ds: f64, x: f64, y: f64) -> bool {
    if s < x - ON_EDGE || s > y + ON_EDGE {
        return false;
    }
    y - x <= ON_EDGE
        || (s > x + ON_EDGE && s < y - ON_EDGE)
        || (s <= x + ON_EDGE && ds > 0.0)
        || (s >= y - ON_EDGE && ds < 0.0)
}

/// How far the flange that `panel` is reaches from the bend whose zone is `zone`, whose centre
/// line runs from `start` to `end`, on its left (`left`) or its right, measured over the bend
/// less `inset` at each end (the whole bend for 0; the middle point when the insets meet).
///
/// `panel` is the panel's material (its outer loop counter-clockwise; its holes do not count) and
/// lies against the zone's edge on that side; the zone is the bend's region in the flat.
///
/// `None` when it has no value: a bend with no length, a zone that has no extent on that side, a
/// panel that does not stand on the zone's edge there, a boundary that is free-form, an `inset`
/// that is no length, or an input that is no number.
#[must_use]
pub fn flange_reach(
    panel: &Region2,
    zone: &Region2,
    start: Point2,
    end: Point2,
    left: bool,
    inset: f64,
) -> Option<FlangeReach> {
    let span = end - start;
    let length = span.length();
    if !(length > 0.0 && length.is_finite() && inset.is_finite() && inset >= 0.0) {
        return None;
    }
    if panel.outer.segs.len() > MAX_SEGMENTS || zone.outer.segs.len() > MAX_SEGMENTS {
        return None;
    }
    let u = span * (1.0 / length);
    let perp = Vec2::new(-u.y, u.x);
    let (n, sign) = if left { (perp, 1.0) } else { (-perp, -1.0) };
    let edge = zone
        .outer
        .segs
        .iter()
        .map(|g| (g.start() - start).dot(n))
        .fold(f64::NEG_INFINITY, f64::max);
    if !(edge > 0.0 && edge.is_finite()) {
        return None;
    }
    let bend = Bend {
        origin: start,
        u,
        n,
        edge,
    };
    let (mut lo, mut hi) = (inset, length - inset);
    if lo > hi {
        lo = length / 2.0;
        hi = lo;
    }
    // The stretches of the zone's edge the panel stands on: its boundary on the edge line, running
    // with the material on its left (the direction that has `n` on its left).
    let mut stands: Vec<(f64, f64)> = Vec::new();
    for g in &panel.outer.segs {
        if let Seg2::Line { a, b } = *g {
            let level = bend.above(a).abs().max(bend.above(b).abs());
            if level <= ON_EDGE && (b - a).dot(u) * sign > 0.0 {
                let (sa, sb) = (bend.along(a), bend.along(b));
                let (x, y) = (sa.min(sb).max(lo), sa.max(sb).min(hi));
                if x <= y {
                    stands.push((x, y));
                }
            }
        }
    }
    let mut least: Option<f64> = None;
    let mut take = |above: f64| {
        if above >= -ON_EDGE {
            let above = above.max(0.0);
            least = Some(least.map_or(above, |l: f64| l.min(above)));
        }
    };
    for g in &panel.outer.segs {
        match *g {
            Seg2::Line { a, b } => {
                let (sa, sb, ta, tb) = (bend.along(a), bend.along(b), bend.above(a), bend.above(b));
                // The edge itself, and a wall square to the bend, which no ray meets.
                if (sb - sa).abs() <= ON_EDGE || (ta.abs() <= ON_EDGE && tb.abs() <= ON_EDGE) {
                    continue;
                }
                let at = |s: f64| ta + (tb - ta) * (s - sa) / (sb - sa);
                for &(x, y) in &stands {
                    let (c, d) = (sa.min(sb).max(x), sa.max(sb).min(y));
                    if c <= d && (d - c > ON_EDGE || y - x <= ON_EDGE) {
                        take(at(c));
                        take(at(d));
                    }
                }
            }
            Seg2::Arc {
                center,
                radius,
                start: from,
                sweep,
            } => {
                if !(radius > 0.0 && radius.is_finite() && sweep.is_finite() && from.is_finite()) {
                    return None;
                }
                let cs = bend.along(center);
                // Where it can be lowest: its ends, its lowest point, and where it crosses each
                // stretch's ends; each with the way the arc runs on from it when it is an end.
                let into = |ang: f64, forward: bool| {
                    let t = Vec2::new(-ang.sin(), ang.cos())
                        * (sweep.signum() * if forward { 1.0 } else { -1.0 });
                    t.dot(u)
                };
                for (ang, forward) in [(from, true), (from + sweep, false)] {
                    let p = center + Vec2::new(ang.cos(), ang.sin()) * radius;
                    let (s, t) = (bend.along(p), bend.above(p));
                    for &(x, y) in &stands {
                        if (t > ON_EDGE && s >= x - ON_EDGE && s <= y + ON_EDGE)
                            || (t <= ON_EDGE && over(s, into(ang, forward), x, y))
                        {
                            take(t);
                        }
                    }
                }
                let mut angles = vec![(-n.y).atan2(-n.x)];
                for &(x, y) in &stands {
                    for s in [x, y] {
                        let cos = (s - cs) / radius;
                        if cos.abs() <= 1.0 {
                            let sin = (1.0 - cos * cos).sqrt();
                            angles.push(plane_angle(&bend, sin.atan2(cos)));
                            angles.push(plane_angle(&bend, (-sin).atan2(cos)));
                        }
                    }
                }
                for ang in angles {
                    if !on_arc(ang, from, sweep) {
                        continue;
                    }
                    let p = center + Vec2::new(ang.cos(), ang.sin()) * radius;
                    let (s, t) = (bend.along(p), bend.above(p));
                    if stands
                        .iter()
                        .any(|&(x, y)| s >= x - ON_EDGE && s <= y + ON_EDGE)
                    {
                        take(t);
                    }
                }
            }
            _ => return None,
        }
    }
    let from_edge = least?;
    Some(FlangeReach {
        from_edge,
        from_line: from_edge + edge,
    })
}
