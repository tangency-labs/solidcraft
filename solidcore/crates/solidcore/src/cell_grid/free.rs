//! Exact clipping of one cell to the field as drawn (`free` mode): `cell ∩ region`, with the arcs
//! of the boundary kept as arcs.
//!
//! The boundary of the cell and the boundary of the region are cut at every crossing. A piece of
//! the cell's boundary stays when it lies inside the region, a piece of the region's boundary
//! stays when it lies inside the cell, and where the two run along each other the cell's piece
//! stays when both run the same way. The kept pieces chain into loops: outer loops counter-clockwise,
//! holes clockwise.

use crate::math::Vec2;

use super::vec::ring_next;
use crate::prism::profile::cross2;

use super::region::{circle_segment_points, Bounds, Convex, Loop, Prim, Region, EPS};

/// Distance (mm) within which two boundaries count as running along each other.
const ON: f64 = 1e-7;
/// Smallest area (mm²) of a loop kept.
const MIN_AREA: f64 = 1e-9;

/// The cell to clip.
pub(crate) enum Cell<'a> {
    Polygon(&'a Convex),
    Circle { c: Vec2, r: f64 },
}

impl Cell<'_> {
    fn bounds(&self) -> Bounds {
        match self {
            Cell::Polygon(p) => p.bounds,
            Cell::Circle { c, r } => Bounds {
                x0: c.x - r,
                y0: c.y - r,
                x1: c.x + r,
                y1: c.y + r,
            },
        }
    }

    fn prims(&self) -> Vec<Prim> {
        match self {
            Cell::Polygon(p) => {
                let n = p.pts.len();
                (0..n)
                    .filter_map(|k| Some(Prim::line(*p.pts.get(k)?, *p.pts.get(ring_next(k, n))?)))
                    .collect()
            }
            Cell::Circle { c, r } => {
                let start = Vec2::new(c.x + r, c.y);
                vec![Prim::Arc {
                    c: *c,
                    r: *r,
                    a0: 0.0,
                    sweep: std::f64::consts::TAU,
                    p0: start,
                    p1: start,
                }]
            }
        }
    }

    fn strictly_contains(&self, p: Vec2) -> bool {
        match self {
            Cell::Polygon(poly) => poly.contains(p, 0.0),
            Cell::Circle { c, r } => p.distance(*c) < *r,
        }
    }
}

/// Where a point lies along a piece: the fraction of its length from the start.
fn fraction(p: &Prim, x: Vec2) -> f64 {
    match *p {
        Prim::Line { a, b, .. } => {
            let d = b - a;
            let l2 = d.dot(d);
            if l2 > 0.0 {
                (x - a).dot(d) / l2
            } else {
                0.0
            }
        }
        Prim::Arc { c, a0, sweep, .. } => {
            let a = (x.y - c.y).atan2(x.x - c.x);
            let along = if sweep >= 0.0 {
                (a - a0).rem_euclid(std::f64::consts::TAU)
            } else {
                (a0 - a).rem_euclid(std::f64::consts::TAU)
            };
            if sweep == 0.0 {
                0.0
            } else {
                along / sweep.abs()
            }
        }
    }
}

/// Up to four points where two pieces meet (the ends of the overlap where they run along each other).
fn intersections(a: &Prim, b: &Prim) -> Vec<Vec2> {
    let mut out = Vec::new();
    match (*a, *b) {
        (Prim::Line { a: p, b: q, .. }, Prim::Line { a: r, b: s, .. }) => {
            let (d1, d2) = (q - p, s - r);
            let den = cross2(d1, d2);
            let scale = d1.length() * d2.length();
            if den.abs() > 1e-12 * scale && scale > 0.0 {
                let t = cross2(r - p, d2) / den;
                let u = cross2(r - p, d1) / den;
                let (tt, uu) = (1e-9, 1e-9);
                if (-tt..=1.0 + tt).contains(&t) && (-uu..=1.0 + uu).contains(&u) {
                    out.push(p + d1 * t.clamp(0.0, 1.0));
                }
            } else if scale > 0.0 && cross2(r - p, d1).abs() <= 1e-9 * d1.length() {
                // Parallel and on one line: the ends of each that lie on the other.
                for (pt, host) in [(r, a), (s, a), (p, b), (q, b)] {
                    let f = fraction(host, pt);
                    if (0.0..=1.0).contains(&f) {
                        out.push(pt);
                    }
                }
            }
        }
        (Prim::Line { a: p, b: q, .. }, Prim::Arc { c, r, .. })
        | (Prim::Arc { c, r, .. }, Prim::Line { a: p, b: q, .. }) => {
            let arc = if matches!(a, Prim::Arc { .. }) { a } else { b };
            let (pts, n) = circle_segment_points(c, r, p, q);
            for &x in pts.iter().take(n) {
                if arc.contains_angle((x.y - c.y).atan2(x.x - c.x), 1e-9) {
                    out.push(x);
                }
            }
        }
        (
            Prim::Arc {
                c: c1,
                r: r1,
                p0: a0,
                p1: a1,
                ..
            },
            Prim::Arc {
                c: c2,
                r: r2,
                p0: b0,
                p1: b1,
                ..
            },
        ) => {
            let d = c1.distance(c2);
            if d < 1e-12 {
                if (r1 - r2).abs() < 1e-9 {
                    for (pt, host) in [(b0, a), (b1, a), (a0, b), (a1, b)] {
                        if host.contains_angle(
                            (pt.y - host_center(host).y).atan2(pt.x - host_center(host).x),
                            1e-9,
                        ) {
                            out.push(pt);
                        }
                    }
                }
            } else if d <= r1 + r2 + 1e-9 && d >= (r1 - r2).abs() - 1e-9 {
                let u = (c2 - c1) / d;
                let along = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
                let h = (r1 * r1 - along * along).max(0.0).sqrt();
                let mid = c1 + u * along;
                let perp = Vec2::new(-u.y, u.x);
                for x in [mid + perp * h, mid - perp * h] {
                    let on_a = a.contains_angle((x.y - c1.y).atan2(x.x - c1.x), 1e-9);
                    let on_b = b.contains_angle((x.y - c2.y).atan2(x.x - c2.x), 1e-9);
                    if on_a && on_b && !out.iter().any(|q: &Vec2| q.distance(x) < 1e-9) {
                        out.push(x);
                    }
                }
            }
        }
    }
    out
}

fn host_center(p: &Prim) -> Vec2 {
    match *p {
        Prim::Arc { c, .. } => c,
        Prim::Line { a, .. } => a,
    }
}

/// The piece cut at the given `(fraction, point)` crossings.
fn split(p: &Prim, mut cuts: Vec<(f64, Vec2)>) -> Vec<Prim> {
    let len = p.length();
    let tol = 1e-7;
    cuts.retain(|&(f, _)| f * len > tol && (1.0 - f) * len > tol);
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    cuts.dedup_by(|b, a| (b.0 - a.0) * len <= tol);
    let mut fs = vec![0.0];
    let mut pts = vec![p.start()];
    for (f, x) in cuts {
        fs.push(f);
        pts.push(x);
    }
    fs.push(1.0);
    pts.push(p.end());
    let mut out = Vec::new();
    for k in 0..pts.len().saturating_sub(1) {
        let (Some(&s), Some(&e), Some(&f0), Some(&f1)) =
            (pts.get(k), pts.get(k + 1), fs.get(k), fs.get(k + 1))
        else {
            continue;
        };
        match *p {
            Prim::Line { lattice, .. } => {
                if s.distance(e) > 1e-9 {
                    out.push(Prim::Line {
                        a: s,
                        b: e,
                        lattice,
                    });
                }
            }
            Prim::Arc {
                c, r, a0, sweep, ..
            } => {
                if (f1 - f0) * sweep.abs() > 1e-9 {
                    out.push(Prim::Arc {
                        c,
                        r,
                        a0: a0 + f0 * sweep,
                        sweep: (f1 - f0) * sweep,
                        p0: s,
                        p1: e,
                    });
                }
            }
        }
    }
    out
}

/// `cell ∩ region` as loops (outer counter-clockwise, holes clockwise), and the signed area
/// of each. `None` when the pieces do not chain into closed loops (a degenerate field).
pub(crate) fn clip_cell(region: &Region, cell: &Cell) -> Option<Vec<(Vec<Prim>, f64)>> {
    let cprims = cell.prims();
    let near: Vec<Prim> = region
        .near(&cell.bounds().grown(EPS))
        .into_iter()
        .filter_map(|i| region.prims.get(i).copied())
        .collect();
    let mut csplit: Vec<Vec<(f64, Vec2)>> = vec![Vec::new(); cprims.len()];
    let mut rsplit: Vec<Vec<(f64, Vec2)>> = vec![Vec::new(); near.len()];
    for (ci, cp) in cprims.iter().enumerate() {
        let cb = cp.bounds();
        for (ri, rp) in near.iter().enumerate() {
            if !cb.overlaps(&rp.bounds(), 1e-9) {
                continue;
            }
            for x in intersections(cp, rp) {
                if let Some(s) = csplit.get_mut(ci) {
                    s.push((fraction(cp, x), x));
                }
                if let Some(s) = rsplit.get_mut(ri) {
                    s.push((fraction(rp, x), x));
                }
            }
        }
    }
    let mut kept: Vec<Prim> = Vec::new();
    for (cp, cuts) in cprims.iter().zip(csplit) {
        for sub in split(cp, cuts) {
            let mid = sub.mid();
            // The nearest piece of the field's boundary, if one runs along this one.
            let along = near
                .iter()
                .filter(|r| r.distance(mid) <= ON)
                .min_by(|a, b| a.distance(mid).total_cmp(&b.distance(mid)));
            let keep = match along {
                Some(r) => sub.direction_at(mid).dot(r.direction_at(mid)) > 0.0,
                None => region.contains(mid),
            };
            if keep {
                kept.push(sub);
            }
        }
    }
    for (rp, cuts) in near.iter().zip(rsplit) {
        for sub in split(rp, cuts) {
            let mid = sub.mid();
            if cprims.iter().any(|c| c.distance(mid) <= ON) {
                continue; // along the cell: the cell's own piece decided it
            }
            if cell.strictly_contains(mid) {
                kept.push(sub);
            }
        }
    }
    chain(&kept)
}

/// Chains pieces into closed loops. `None` when one does not close.
fn chain(subs: &[Prim]) -> Option<Vec<(Vec<Prim>, f64)>> {
    let tol = 1e-6;
    let mut used = vec![false; subs.len()];
    let mut loops = Vec::new();
    for first in 0..subs.len() {
        if used.get(first).copied().unwrap_or(true) {
            continue;
        }
        if let Some(u) = used.get_mut(first) {
            *u = true;
        }
        let start = subs.get(first)?.start();
        let mut cur = vec![*subs.get(first)?];
        let mut end = subs.get(first)?.end();
        while end.distance(start) > tol {
            let next = subs
                .iter()
                .enumerate()
                .filter(|&(k, s)| {
                    !used.get(k).copied().unwrap_or(true) && s.start().distance(end) <= tol
                })
                .min_by(|a, b| {
                    a.1.start()
                        .distance(end)
                        .total_cmp(&b.1.start().distance(end))
                })?;
            if let Some(u) = used.get_mut(next.0) {
                *u = true;
            }
            end = next.1.end();
            cur.push(*next.1);
        }
        let lp = Loop::new(cur, None)?;
        let area = lp.signed_area();
        if area.abs() > MIN_AREA {
            loops.push((lp.prims, area));
        }
    }
    Some(loops)
}
