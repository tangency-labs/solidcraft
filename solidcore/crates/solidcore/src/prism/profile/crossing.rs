//! Whether a closed loop of lines and arcs crosses or touches itself: the check a profile built
//! from pieces (`crate::prism::fill`) passes, and the one a retry at a fit asks again.
//!
//! Two sides that join at a point meet there by construction. Their other meeting is found by
//! deflation from that join, not from the full quadratic: a line and a circle through a known
//! point meet again at the other root of the line's quadratic (`t2 = -B / A - t1`), two circles
//! through a known point meet again at its mirror image across the line of their centres. Each
//! is off by a few `f64::EPSILON` times the radius, where the full quadratic's near-double root
//! (two sides that leave their join at almost the same angle) is off by `f64::EPSILON R² / b`
//! for a meeting `b` from the join, which a large radius makes larger than the near-miss
//! tolerance.

use crate::math::det;
use std::f64::consts::TAU;

use crate::math::{Point2, Vec2};

use super::Seg2;

/// The near-miss tolerance of the crossing check for a section of conservative allowance `eps`:
/// two sides closer than this meet.
#[must_use]
pub fn near_miss(eps: f64) -> f64 {
    eps.max(1e-12) * 100.0
}

/// Whether the closed loop `segs` crosses or touches itself anywhere but where neighbours
/// join (`tol` round each join is not looked at). Neighbouring segments of one nonzero
/// `group` (a face's pieces) are one side: they are not checked against each other, and the
/// side joins its neighbours at its own ends. Lines and arcs only (other segments are not
/// looked at). It decides at `tol`, not at a proved bound: two sides within `tol` of each
/// other meet: where they cross, and where an end of one comes within `tol` of the other. The
/// caller passes finite segments (a NaN compares as no meeting), each starting where the one
/// before ends to within rounding (the last ending where the first starts): a join is taken as
/// a point of both sides, and their other meeting is read from it.
#[must_use]
pub fn loop_crosses(segs: &[Seg2], group: &[usize], tol: f64) -> bool {
    // each segment's side, and each side's two ends
    let mut side = Vec::new();
    let mut ends: Vec<(Point2, Point2)> = Vec::new();
    for (i, s) in segs.iter().enumerate() {
        let g = group.get(i).copied().unwrap_or(0);
        let joined = i > 0 && g != 0 && group.get(i - 1) == Some(&g);
        match ends.last_mut() {
            Some(last) if joined => last.1 = s.end(),
            _ => ends.push((s.start(), s.end())),
        }
        side.push(ends.len() - 1);
    }
    let n = ends.len();
    for (i, a) in segs.iter().enumerate() {
        for (j, b) in segs.iter().enumerate().skip(i + 1) {
            let (Some(&u), Some(&v)) = (side.get(i), side.get(j)) else {
                continue;
            };
            let Some(&(start, end)) = ends.get(u) else {
                continue;
            };
            if u == v {
                continue;
            }
            let next = v == u + 1;
            let wrap = u == 0 && v + 1 == n;
            let joins: Vec<Point2> = match (next, wrap) {
                (true, true) => vec![end, start],
                (true, false) => vec![end],
                (false, true) => vec![start],
                (false, false) => Vec::new(),
            };
            if meets(a, b, &joins, tol)
                .into_iter()
                .any(|p| joins.iter().all(|&v| p.distance(v) > tol))
            {
                return true;
            }
        }
    }
    false
}

/// Where two lines or arcs meet (where they overlap, the ends of the overlap), and their ends
/// within `tol` of the other. Where both end at one of `joins`, that join is one meeting and
/// the other comes from it.
fn meets(a: &Seg2, b: &Seg2, joins: &[Point2], tol: f64) -> Vec<Point2> {
    let ends_at = |s: &Seg2, j: Point2| s.start().distance(j) <= tol || s.end().distance(j) <= tol;
    let join = joins
        .iter()
        .copied()
        .find(|&j| ends_at(a, j) && ends_at(b, j));
    let candidates = match (*a, *b) {
        (Seg2::Line { a: p, b: q }, Seg2::Line { a: r, b: s }) => line_line(p, q, r, s, tol),
        (Seg2::Line { a: p, b: q }, Seg2::Arc { center, radius, .. })
        | (Seg2::Arc { center, radius, .. }, Seg2::Line { a: p, b: q }) => match join {
            Some(j) => line_circle_from(p, q, center, j, tol),
            None => line_circle(p, q, center, radius),
        },
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
            if c1.distance(c2) <= tol && (r1 - r2).abs() <= tol {
                // one circle: the arcs meet where an end of one lies on the other
                vec![a.start(), a.end(), b.start(), b.end()]
            } else if let Some(j) = join {
                circle_circle_from(c1, c2, j)
            } else {
                circle_circle(c1, r1, c2, r2)
            }
        }
        _ => Vec::new(),
    };
    // and an end of one within `tol` of the other (a corner that comes near a side without
    // crossing it: the closest two lines come, where they do not cross, is at an end)
    candidates
        .into_iter()
        .chain([a.start(), a.end(), b.start(), b.end()])
        .filter(|&p| on_seg(a, p, tol) && on_seg(b, p, tol))
        .collect()
}

/// Whether `p` lies on the line or arc `s`, within `tol`.
pub(crate) fn on_seg(s: &Seg2, p: Point2, tol: f64) -> bool {
    match *s {
        Seg2::Line { a, b } => {
            let d = b - a;
            let len2 = d.length_squared();
            if len2 == 0.0 {
                return p.distance(a) <= tol;
            }
            let t = (p - a).dot(d) / len2;
            let near = a + d * t.clamp(0.0, 1.0);
            p.distance(near) <= tol
        }
        Seg2::Arc { center, radius, .. } => {
            (p.distance(center) - radius).abs() <= tol && on_arc(s, p, tol)
        }
        _ => false,
    }
}

/// Whether the direction of `p` from an arc's centre lies within its sweep (`tol` along it).
pub(crate) fn on_arc(s: &Seg2, p: Point2, tol: f64) -> bool {
    let Seg2::Arc {
        center,
        radius,
        start,
        sweep,
    } = *s
    else {
        return false;
    };
    let d = p - center;
    let ang = det::atan2(d.y, d.x);
    let t = if sweep >= 0.0 {
        (ang - start).rem_euclid(TAU)
    } else {
        (start - ang).rem_euclid(TAU)
    };
    let slack = tol / radius;
    t <= sweep.abs() + slack || t >= TAU - slack
}

fn line_line(p: Point2, q: Point2, r: Point2, s: Point2, tol: f64) -> Vec<Point2> {
    let (d1, d2) = (q - p, s - r);
    let den = d1.x * d2.y - d1.y * d2.x;
    let scale = d1.length() * d2.length();
    if den.abs() <= 1e-14 * scale {
        // parallel: they meet only if they lie on one line, at the ends of the overlap
        let off = (r - p).x * d1.y - (r - p).y * d1.x;
        if off.abs() <= tol * d1.length() {
            return vec![p, q, r, s];
        }
        return Vec::new();
    }
    let w = r - p;
    let t = (w.x * d2.y - w.y * d2.x) / den;
    vec![p + d1 * t]
}

fn line_circle(p: Point2, q: Point2, c: Point2, r: f64) -> Vec<Point2> {
    let d = q - p;
    let f = p - c;
    let a = d.dot(d);
    if a == 0.0 {
        return Vec::new();
    }
    let b = 2.0 * f.dot(d);
    let cc = f.dot(f) - r * r;
    let disc = b * b - 4.0 * a * cc;
    if disc < 0.0 {
        // a near miss within rounding counts as touching at the foot of the centre
        let t = -b / (2.0 * a);
        return vec![p + d * t];
    }
    let root = disc.sqrt();
    vec![
        p + d * ((-b - root) / (2.0 * a)),
        p + d * ((-b + root) / (2.0 * a)),
    ]
}

/// Where the line from `p` to `q` meets the circle about `c` through `join`, an end of the
/// line: the join, and the other root of `|p + t d - c|² = r²` from the roots' sum `-B / A`.
fn line_circle_from(p: Point2, q: Point2, c: Point2, join: Point2, tol: f64) -> Vec<Point2> {
    let d = q - p;
    let a = d.dot(d);
    if a == 0.0 {
        return Vec::new();
    }
    let t1 = if p.distance(join) <= tol { 0.0 } else { 1.0 };
    let t2 = -2.0 * (p - c).dot(d) / a - t1;
    vec![p + d * t1, p + d * t2]
}

fn circle_circle(c1: Point2, r1: f64, c2: Point2, r2: f64) -> Vec<Point2> {
    let d = c2 - c1;
    let dist = d.length();
    if dist == 0.0 {
        return Vec::new();
    }
    let a = (dist * dist + r1 * r1 - r2 * r2) / (2.0 * dist);
    let h2 = r1 * r1 - a * a;
    let u = d * (1.0 / dist);
    let mid = c1 + u * a;
    if h2 < 0.0 {
        // a near miss within rounding counts as touching on the line of centres
        return vec![mid];
    }
    let h = h2.sqrt();
    let n = Vec2::new(-u.y, u.x);
    vec![mid + n * h, mid - n * h]
}

/// Where the circles about `c1` and `c2` that both pass through `join` meet: the join, and its
/// mirror image across the line of their centres.
fn circle_circle_from(c1: Point2, c2: Point2, join: Point2) -> Vec<Point2> {
    let d = c2 - c1;
    let len2 = d.length_squared();
    if len2 == 0.0 {
        return Vec::new();
    }
    let w = join - c1;
    let foot = c1 + d * (w.dot(d) / len2);
    vec![join, foot * 2.0 - join]
}
