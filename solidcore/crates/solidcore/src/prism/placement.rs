//! Where one planar loop lies against another: held clear inside it, clear outside it, along it
//! everywhere, or meeting it in places only. For loops of lines and circular arcs, within a
//! distance the caller derives from its model's tolerance.
//!
//! The nearest approach of two pieces is decided from their own geometry: zero where their lines
//! or circles cross at a point on both, else the least of the distances from the points where a
//! distance between a line and a circle, or two circles, can be least (each piece's ends, and on
//! an arc the points on the line from its centre to the other piece's centre or its foot on the
//! other line) to the other piece. A loop clear of the other is placed by
//! [`Loop2::holds`], which decides the hold of a point exactly on arcs (no chord stands in for
//! them). Loops that meet are along each other when the other's segments on the same line or
//! circle cover every segment of this one, end to end.
//!
//! Not upstream (SolidCraft's lip band, which asks whether the band's far edge stays inside the
//! rim or reaches past its other edge, moved into the kernel so the layers above compute no
//! geometry, docs/layers.md §1).
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests, kept as written

use crate::math::det;
use std::f64::consts::TAU;

use super::profile::{cross2, Loop2, Seg2, MAX_SEGMENTS};
use crate::math::{Point2, Vec2};

/// Where a loop lies against another ([`Loop2::placement`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopPlacement {
    /// Farther than the distance from the other loop everywhere, and held by it.
    Inside,
    /// Farther than the distance from the other loop everywhere, and not held by it.
    Outside,
    /// Along the other loop: every point of each of its segments within the distance of the
    /// other's segments that run along it.
    Coincident,
    /// Within the distance of the other loop in places, but not along it everywhere.
    Crossing,
}

impl Loop2 {
    /// Where this loop lies against `other` (both in one frame), two loops within `tol` of each
    /// other counting as meeting. `None` for a loop with a segment other than a line or an arc,
    /// an empty loop, more than [`MAX_SEGMENTS`] segments between them, a `tol` that is not
    /// positive and finite, or a hold [`Loop2::holds`] cannot decide.
    #[must_use]
    pub fn placement(&self, other: &Loop2, tol: f64) -> Option<LoopPlacement> {
        if !(tol.is_finite() && tol > 0.0)
            || self.segs.is_empty()
            || other.segs.is_empty()
            || self.segs.len().saturating_add(other.segs.len()) > MAX_SEGMENTS
            || !self
                .segs
                .iter()
                .chain(&other.segs)
                .all(Seg2::is_line_or_arc)
        {
            return None;
        }
        let mut nearest = f64::INFINITY;
        for a in &self.segs {
            for b in &other.segs {
                let d = seg_distance(a, b, tol);
                if d.is_nan() {
                    return None;
                }
                nearest = nearest.min(d);
            }
        }
        if nearest <= tol {
            let along = self.segs.iter().all(|a| covered(a, &other.segs, tol));
            return Some(if along {
                LoopPlacement::Coincident
            } else {
                LoopPlacement::Crossing
            });
        }
        let probe = self.segs.first()?.start();
        Some(if other.holds(probe)? {
            LoopPlacement::Inside
        } else {
            LoopPlacement::Outside
        })
    }
}

/// The angle `ang` measured the way `sweep` turns, in `[0, 2π)`.
fn turned(ang: f64, sweep: f64) -> f64 {
    (ang * sweep.signum()).rem_euclid(TAU)
}

fn angle_of(v: Vec2) -> f64 {
    det::atan2(v.y, v.x)
}

fn unit(v: Vec2) -> Option<Vec2> {
    let l = v.length();
    (l > 0.0 && l.is_finite()).then(|| v * (1.0 / l))
}

impl Seg2 {
    /// How far `p` is from the segment, exactly, for a line or a circular arc: the foot on a line
    /// clamped to its ends; on an arc the radial distance where `p` is within its angles (any
    /// point, at its centre), else the distance to the nearer end. `None` for a free-form
    /// segment, which has no closed form here, and for a point or segment that is not finite.
    #[must_use]
    pub fn distance_to(&self, p: Point2) -> Option<f64> {
        let d = self.is_line_or_arc().then(|| point_distance(p, self))?;
        d.is_finite().then_some(d)
    }
}

/// How far `p` is from the line or arc `s` (exact: the foot on a line clamped to its ends; on an
/// arc the radial distance where `p` is within its angles, else the nearer end).
fn point_distance(p: Point2, s: &Seg2) -> f64 {
    match *s {
        Seg2::Line { a, b } => {
            // The foot of `p` on the line, or the end it falls past.
            let d = b - a;
            let (along, l2) = ((p - a).dot(d), d.length_squared());
            if !(along > 0.0 && l2 > 0.0) {
                p.distance(a)
            } else if along >= l2 {
                p.distance(b)
            } else {
                p.distance(a + d * (along / l2))
            }
        }
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let v = p - center;
            // From the centre every point of the circle is as near.
            if v.length_squared() == 0.0 || turned(angle_of(v) - start, sweep) <= sweep.abs() {
                (v.length() - radius).abs()
            } else {
                p.distance(s.start()).min(p.distance(s.end()))
            }
        }
        _ => f64::NAN,
    }
}

/// Where the line through `p` and `q` crosses the circle about `c` of radius `r`.
fn line_circle(p: Point2, q: Point2, c: Point2, r: f64) -> Vec<Point2> {
    let d = q - p;
    let l2 = d.length_squared();
    if !(l2 > 0.0) {
        return Vec::new();
    }
    let foot = p + d * ((c - p).dot(d) / l2);
    let h2 = r * r - foot.distance_squared(c);
    if h2 < 0.0 {
        return Vec::new();
    }
    let along = d * (h2.sqrt() / l2.sqrt());
    vec![foot + along, foot - along]
}

/// The points where the lines or circles carrying `a` and `b` cross.
fn crossings(a: &Seg2, b: &Seg2) -> Vec<Point2> {
    match (*a, *b) {
        (Seg2::Line { a: p0, b: p1 }, Seg2::Line { a: q0, b: q1 }) => {
            let (d1, d2) = (p1 - p0, q1 - q0);
            let den = cross2(d1, d2);
            if den == 0.0 {
                return Vec::new();
            }
            vec![p0 + d1 * (cross2(q0 - p0, d2) / den)]
        }
        (Seg2::Line { a: p, b: q }, Seg2::Arc { center, radius, .. })
        | (Seg2::Arc { center, radius, .. }, Seg2::Line { a: p, b: q }) => {
            line_circle(p, q, center, radius)
        }
        (
            Seg2::Arc {
                center: c1,
                radius: r1,
                ..
            },
            Seg2::Arc {
                center: c2,
                radius: r2,
                ..
            },
        ) => {
            let d = c1.distance(c2);
            let Some(u) = unit(c2 - c1) else {
                return Vec::new();
            };
            if d > r1 + r2 || d < (r1 - r2).abs() {
                return Vec::new();
            }
            let x = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
            let h = (r1 * r1 - x * x).max(0.0).sqrt();
            let (m, across) = (c1 + u * x, Vec2::new(-u.y, u.x));
            vec![m + across * h, m - across * h]
        }
        _ => Vec::new(),
    }
}

/// The points of `x` where its distance to `y` can be least: its ends and, on an arc, the two
/// points of its circle on the line from its centre towards `y`'s centre or its foot on `y`'s
/// line.
fn candidates(x: &Seg2, y: &Seg2) -> Vec<Point2> {
    let mut pts = vec![x.start(), x.end()];
    if let Seg2::Arc { center, radius, .. } = *x {
        let target = match *y {
            Seg2::Arc { center: o, .. } => Some(o),
            Seg2::Line { a, b } => {
                let d = b - a;
                let l2 = d.length_squared();
                (l2 > 0.0).then(|| a + d * ((center - a).dot(d) / l2))
            }
            _ => None,
        };
        if let Some(u) = target.and_then(|t| unit(t - center)) {
            pts.push(center + u * radius);
            pts.push(center - u * radius);
        }
    }
    pts
}

/// How close the lines or arcs `a` and `b` come: zero where they cross (a crossing of their
/// carriers within `tol` of both), else the least distance from a [`candidates`] point on one
/// (within `tol` of it, so on it) to the other.
fn seg_distance(a: &Seg2, b: &Seg2, tol: f64) -> f64 {
    if crossings(a, b)
        .into_iter()
        .any(|x| point_distance(x, a) <= tol && point_distance(x, b) <= tol)
    {
        return 0.0;
    }
    let from = |x: &Seg2, y: &Seg2| -> f64 {
        candidates(x, y)
            .into_iter()
            .filter(|p| point_distance(*p, x) <= tol)
            .map(|p| point_distance(p, y))
            .fold(f64::INFINITY, f64::min)
    };
    from(a, b).min(from(b, a))
}

/// The stretch of `a` that `b` runs along, in `a`'s own measure from its start (length along a
/// line, angle turned on an arc) and possibly reaching past its ends: for two lines whose ends
/// are each within `tol` of the other's line (the distance to a line changes linearly along
/// another, so every point between is too), `b`'s ends projected on `a`; for two arcs whose
/// centres and radii together differ by at most `tol` (each point of one is then within `tol` of
/// the other's circle), `b`'s angles seen from `a`'s centre. `None` for any other pair.
fn span_on(a: &Seg2, b: &Seg2, tol: f64) -> Option<(f64, f64)> {
    match (*a, *b) {
        (Seg2::Line { a: p, b: q }, Seg2::Line { a: u, b: v }) => {
            let off = |x: Point2, s: Point2, d: Vec2| cross2(d, x - s).abs();
            let (da, db) = (unit(q - p)?, unit(v - u)?);
            let near = off(u, p, da) <= tol
                && off(v, p, da) <= tol
                && off(p, u, db) <= tol
                && off(q, u, db) <= tol;
            near.then(|| {
                let (tu, tv) = ((u - p).dot(da), (v - p).dot(da));
                (tu.min(tv), tu.max(tv))
            })
        }
        (
            Seg2::Arc {
                center: ca,
                radius: ra,
                start: sa,
                sweep: wa,
            },
            Seg2::Arc {
                center: cb,
                radius: rb,
                sweep: wb,
                ..
            },
        ) => {
            if !(ca.distance(cb) + (ra - rb).abs() <= tol) {
                return None;
            }
            let t0 = turned(angle_of(b.start() - ca) - sa, wa);
            Some(if wa.signum() == wb.signum() {
                (t0, t0 + wb.abs())
            } else {
                (t0 - wb.abs(), t0)
            })
        }
        _ => None,
    }
}

/// Whether `others` run along every point of `a`, to within `tol`: the stretches of `a` they run
/// along ([`span_on`]; on an arc each also a full turn either way, as angles wrap) leave no gap
/// wider than `tol` from its start to its end.
fn covered(a: &Seg2, others: &[Seg2], tol: f64) -> bool {
    let (len, slack, wraps) = match *a {
        Seg2::Line { a: p, b: q } => (p.distance(q), tol, false),
        Seg2::Arc { radius, sweep, .. } if radius > 0.0 => (sweep.abs(), tol / radius, true),
        _ => return false,
    };
    let mut spans: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in others.iter().filter_map(|b| span_on(a, b, tol)) {
        spans.push((lo, hi));
        if wraps {
            spans.push((lo - TAU, hi - TAU));
            spans.push((lo + TAU, hi + TAU));
        }
    }
    spans.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut reach = 0.0_f64;
    for (lo, hi) in spans {
        if lo > reach + slack {
            break;
        }
        reach = reach.max(hi);
    }
    reach >= len - slack
}
