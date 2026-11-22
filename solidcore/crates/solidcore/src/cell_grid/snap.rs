//! Snapping a field to the lattice: each straight edge moves onto the nearest lattice line of
//! its direction on the solid side, so fields shrink and keep-outs grow, never the other way.

use std::f64::consts::TAU;

use crate::math::Vec2;
use crate::prism::profile::cross2;

use super::vec::Vec2Ext;

use super::vec::{ring_next, ring_prev};

use super::lattice::Lattice;
use super::region::{circle_segment_points, line_intersection, Loop, Prim, Region, EPS};

/// A snapped edge: lattice direction index, direction vector, left normal and offset.
#[derive(Clone, Copy)]
struct Line {
    k: usize,
    d: Vec2,
    nl: Vec2,
    c: f64,
}

fn direction(a: Vec2, b: Vec2) -> f64 {
    (b.y - a.y).atan2(b.x - a.x)
}

/// Whether the shared vertex of two consecutive segments can go: the second runs on the same way
/// and the vertex lies on the straight line between the far ends, to within [`EPS`]. A position
/// test, not an angle: on a very long edge a tiny angle is a visible step.
fn removable(first: (Vec2, Vec2), second: (Vec2, Vec2)) -> bool {
    let (start, vertex, end) = (first.0, second.0, second.1);
    let span = end - start;
    let len = span.length();
    if len <= EPS || (first.1 - first.0).dot(second.1 - second.0) <= 0.0 {
        return false;
    }
    (span.x * (vertex.y - start.y) - span.y * (vertex.x - start.x)).abs() / len <= EPS
}

/// Merges consecutive segments that run the same way on the same line, and drops tiny ones, in
/// one pass round the ring (a run that wraps from the last segment to the first included).
pub(super) fn merge_collinear(segs: Vec<(Vec2, Vec2)>) -> Vec<(Vec2, Vec2)> {
    let mut out = merge_forward(segs);
    while out.len() > 2 {
        let (Some(&last), Some(&first)) = (out.last(), out.first()) else {
            break;
        };
        if !removable(last, first) {
            break;
        }
        out.pop();
        if let Some(slot) = out.first_mut() {
            *slot = (last.0, first.1);
        }
    }
    out
}

/// [`merge_collinear`] along an open run: no wrap from the last segment to the first.
fn merge_forward(segs: Vec<(Vec2, Vec2)>) -> Vec<(Vec2, Vec2)> {
    let mut out: Vec<(Vec2, Vec2)> = Vec::new();
    for seg in segs.into_iter().filter(|&(a, b)| a.distance(b) > EPS) {
        out.push(seg);
        while out.len() >= 2 {
            let (Some(&last), Some(&before)) = (out.last(), out.get(out.len() - 2)) else {
                break;
            };
            if !removable(before, last) {
                break;
            }
            out.truncate(out.len() - 2);
            out.push((before.0, last.1));
        }
    }
    out
}

/// The points where two pieces of a loop meet, exactly: line and line, line and circle, circle
/// and circle, each hit kept only where it lies on the piece (within an arc's sweep). Pieces
/// that overlap along a stretch give the ends of the stretch.
pub(super) fn piece_hits(p: &Prim, q: &Prim) -> Vec<Vec2> {
    let mut hits = Vec::new();
    match (*p, *q) {
        (Prim::Line { a, b, .. }, Prim::Line { a: c, b: d, .. }) => {
            let (r, s) = (b - a, d - c);
            let den = cross2(r, s);
            let len = r.length() * s.length();
            if den.abs() > 1e-12 * len.max(1.0) {
                let t = cross2(c - a, s) / den;
                let u = cross2(c - a, r) / den;
                if (-1e-12..=1.0 + 1e-12).contains(&t) && (-1e-12..=1.0 + 1e-12).contains(&u) {
                    hits.push(a + r * t);
                }
            } else if cross2(c - a, r).abs() <= EPS * r.length() {
                // Parallel on one line: the ends of each lying on the other.
                let on = |x: Vec2, from: Vec2, to: Vec2| {
                    let along = (x - from).dot(to - from) / (to - from).dot(to - from).max(1e-300);
                    (-1e-12..=1.0 + 1e-12).contains(&along)
                };
                hits.extend([a, b].into_iter().filter(|&x| on(x, c, d)));
                hits.extend([c, d].into_iter().filter(|&x| on(x, a, b)));
            }
        }
        (Prim::Line { a, b, .. }, Prim::Arc { c, r, .. })
        | (Prim::Arc { c, r, .. }, Prim::Line { a, b, .. }) => {
            let arc = if matches!(p, Prim::Arc { .. }) { p } else { q };
            hits.extend(
                {
                    let (pts, n) = circle_segment_points(c, r, a, b);
                    pts.into_iter().take(n)
                }
                .filter(|&x| on_arc(arc, x)),
            );
        }
        (Prim::Arc { c: c0, r: r0, .. }, Prim::Arc { c: c1, r: r1, .. }) => {
            let d = c1 - c0;
            let dist = d.length();
            if dist <= EPS && (r0 - r1).abs() <= EPS {
                // The same circle: the ends of each arc that lie on the other.
                hits.extend([p.start(), p.end()].into_iter().filter(|&x| on_arc(q, x)));
                hits.extend([q.start(), q.end()].into_iter().filter(|&x| on_arc(p, x)));
            } else if dist > EPS && dist <= r0 + r1 + EPS && dist >= (r0 - r1).abs() - EPS {
                let along = (dist * dist + r0 * r0 - r1 * r1) / (2.0 * dist);
                let off_sq = r0 * r0 - along * along;
                let off = if off_sq > 0.0 { off_sq.sqrt() } else { 0.0 };
                let (u, w) = (d / dist, Vec2::new(-d.y, d.x) / dist);
                let mid = c0 + u * along;
                hits.extend(
                    [mid + w * off, mid - w * off]
                        .into_iter()
                        .filter(|&x| on_arc(p, x) && on_arc(q, x)),
                );
            }
        }
    }
    hits
}

/// Whether two pieces share a stretch of positive length (more than [`EPS`]): collinear lines
/// that overlap, or arcs of one circle that overlap. Touching at a point is not an overlap.
fn overlap(p: &Prim, q: &Prim) -> bool {
    match (*p, *q) {
        (Prim::Line { a, b, .. }, Prim::Line { a: c, b: d, .. }) => {
            let Some(u) = (b - a).normalized() else {
                return false;
            };
            let n = Vec2::new(-u.y, u.x);
            if n.dot(c - a).abs() > EPS || n.dot(d - a).abs() > EPS {
                return false;
            }
            let (sc, sd) = ((c - a).dot(u), (d - a).dot(u));
            let lo = sc.min(sd).max(0.0);
            let hi = sc.max(sd).min(a.distance(b));
            hi - lo > EPS
        }
        (
            Prim::Arc {
                c: c0,
                r: r0,
                a0: s0,
                sweep: w0,
                ..
            },
            Prim::Arc {
                c: c1,
                r: r1,
                a0: s1,
                sweep: w1,
                ..
            },
        ) => {
            if c0.distance(c1) > EPS || (r0 - r1).abs() > EPS {
                return false;
            }
            // Each arc as a counter-clockwise run: its start angle and length.
            let ccw = |a0: f64, w: f64| if w >= 0.0 { (a0, w) } else { (a0 + w, -w) };
            let ((sp, lp), (sq, lq)) = (ccw(s0, w0), ccw(s1, w1));
            let d = (sq - sp).rem_euclid(TAU);
            let shared = (lp.min(d + lq) - d).max(0.0) + (lp.min(d + lq - TAU)).max(0.0);
            shared * r0 > EPS
        }
        _ => false,
    }
}

/// Whether `x`, a point of the arc's circle, lies within the arc's sweep.
fn on_arc(arc: &Prim, x: Vec2) -> bool {
    match *arc {
        Prim::Arc { c, r, .. } => {
            let v = x - c;
            v.length() > EPS && arc.contains_angle(v.y.atan2(v.x), EPS / r.max(EPS))
        }
        Prim::Line { .. } => false,
    }
}

/// True when no two pieces of the closed loop meet except neighbours at their shared joint.
/// Exact: lines and arcs are intersected as lines and circles. Pieces that are not neighbours
/// must not meet at all, a touch at an end included: a loop that touches itself at a point is not
/// a valid region boundary either.
pub(super) fn simple(prims: &[Prim]) -> bool {
    let n = prims.len();
    for a in 0..n {
        for b in a + 1..n {
            let (Some(p), Some(q)) = (prims.get(a), prims.get(b)) else {
                return false;
            };
            if overlap(p, q) {
                return false;
            }
            let adjacent = b == a + 1 || (a == 0 && b == n - 1);
            // The joints the pair really shares: a hit there is the loop passing from one piece
            // to the next.
            let joints: Vec<Vec2> = if adjacent {
                [(p.end(), q.start()), (q.end(), p.start())]
                    .into_iter()
                    .filter(|(x, y)| x.distance(*y) <= EPS)
                    .map(|(x, _)| x)
                    .collect()
            } else {
                Vec::new()
            };
            if piece_hits(p, q)
                .into_iter()
                .any(|x| !joints.iter().any(|j| j.distance(x) <= EPS))
            {
                return false;
            }
        }
    }
    true
}

/// One piece of a loop being snapped: an edge moved onto a lattice line, or a piece kept as
/// drawn (an arc, or a straight edge off the lattice directions). `start` is where the piece
/// began before snapping.
#[derive(Clone, Copy)]
struct Item {
    snapped: Option<Line>,
    prim: Prim,
    start: Vec2,
}

const COLLAPSED: &str = "collapses when snapped";

/// Where the snapped line meets a piece kept as drawn, near `near`: the crossing with its line,
/// or the crossing with its circle that lies nearest to `near`.
fn meet_fixed(line: &Line, fixed: &Prim, near: Vec2) -> Option<Vec2> {
    match *fixed {
        Prim::Line { a, b, .. } => {
            let d = (b - a).normalized()?;
            let n = Vec2::new(-d.y, d.x);
            line_intersection(line.nl, line.c, n, n.dot(a))
        }
        Prim::Arc { c, r, .. } => {
            let foot = c + line.nl * (line.c - line.nl.dot(c));
            let off = (foot - c).length();
            let half_sq = r * r - off * off;
            if !(half_sq >= 0.0) {
                return None;
            }
            let half = half_sq.sqrt();
            let (p, q) = (foot + line.d * half, foot - line.d * half);
            Some(if p.distance(near) <= q.distance(near) {
                p
            } else {
                q
            })
        }
    }
}

/// The vertex between two neighbouring pieces; `near` is where it was before snapping.
fn joint(prev: &Item, next: &Item, near: Vec2) -> Option<Vec2> {
    match (&prev.snapped, &next.snapped) {
        (Some(a), Some(b)) => line_intersection(a.nl, a.c, b.nl, b.c),
        (Some(a), None) => meet_fixed(a, &next.prim, near),
        (None, Some(b)) => meet_fixed(b, &prev.prim, near),
        (None, None) => Some(near),
    }
}

/// Runs of neighbouring snapped edges on one line (the same lattice direction and offset) as
/// one, a run that wraps round the ring included.
fn merge_same_line(items: Vec<Item>) -> Vec<Item> {
    let same = |a: &Item, b: &Item| match (&a.snapped, &b.snapped) {
        (Some(x), Some(y)) => x.k == y.k && (x.c - y.c).abs() <= EPS,
        _ => false,
    };
    let mut out: Vec<Item> = Vec::new();
    for item in items {
        if out.last().is_some_and(|last| same(last, &item)) {
            continue;
        }
        out.push(item);
    }
    while out.len() > 2 {
        if !matches!((out.first(), out.last()), (Some(f), Some(l)) if same(l, f)) {
            break;
        }
        out.pop();
    }
    out
}

/// The vertices of the loop bounded by the snapped pieces, in order.
///
/// Snapping can land neighbouring edges on the same line, or shrink an edge to nothing (a small
/// step between two edges that snap to one line). Those are merged or dropped, and the rest
/// meet again, until the loop is stable. `Err` when the loop really collapses: an edge reverses,
/// or two neighbours cannot meet.
fn snapped_vertices(mut items: Vec<Item>) -> Result<(Vec<Item>, Vec<Vec2>), &'static str> {
    loop {
        items = merge_same_line(items);
        let n = items.len();
        let all_lines = items.iter().all(|i| i.snapped.is_some());
        if n < 2 || (all_lines && n < 3) {
            return Err(COLLAPSED);
        }
        let mut verts = Vec::new();
        for k in 0..n {
            let (Some(a), Some(b)) = (items.get(ring_prev(k, n)), items.get(k)) else {
                return Err(COLLAPSED);
            };
            verts.push(joint(a, b, b.start).ok_or(COLLAPSED)?);
        }
        let mut zero = None;
        for k in 0..n {
            let (Some(&v0), Some(&v1), Some(item)) =
                (verts.get(k), verts.get(ring_next(k, n)), items.get(k))
            else {
                return Err(COLLAPSED);
            };
            let Some(l) = item.snapped else { continue };
            let len = (v1 - v0).dot(l.d);
            if len < -EPS {
                return Err(COLLAPSED);
            }
            if len <= EPS && zero.is_none() {
                zero = Some(k);
            }
        }
        match zero {
            None => return Ok((items, verts)),
            // Its neighbours now meet directly; go round again.
            Some(k) => items = without(items, k),
        }
    }
}

/// `items` without the one at `k`.
fn without<T>(items: Vec<T>, k: usize) -> Vec<T> {
    items
        .into_iter()
        .enumerate()
        .filter_map(|(i, x)| (i != k).then_some(x))
        .collect()
}

/// A piece kept as drawn, with its ends moved to `a` and `b` (they lie on it).
fn refit(prim: &Prim, a: Vec2, b: Vec2) -> Option<Prim> {
    match *prim {
        Prim::Line { lattice, .. } => Some(Prim::Line { a, b, lattice }),
        Prim::Arc {
            c,
            r,
            sweep,
            p0,
            p1,
            ..
        } => {
            if a.distance(p0) <= EPS && b.distance(p1) <= EPS {
                return Some(*prim);
            }
            let (s, e) = ((a - c).y.atan2((a - c).x), (b - c).y.atan2((b - c).x));
            let turn = if sweep >= 0.0 {
                (e - s).rem_euclid(TAU)
            } else {
                -(s - e).rem_euclid(TAU)
            };
            (turn.abs() > EPS && turn.abs() < TAU - EPS).then_some(Prim::Arc {
                c,
                r,
                a0: s,
                sweep: turn,
                p0: a,
                p1: b,
            })
        }
    }
}

/// The loop with each straight edge along a lattice direction moved onto the nearest lattice
/// line on the region's side, and every other piece (an arc, an edge off the lattice directions)
/// kept as drawn, its ends moved to meet the snapped edges. The loop must run with the region on
/// its left. Each snapped edge then moves `inset` further lattice lines to the region's side (a
/// field gets smaller). `Err` says why a loop cannot snap.
pub(crate) fn snap_loop(lp: &Loop, lat: &Lattice, inset: u32) -> Result<Loop, &'static str> {
    let prims = merge_straight_runs(&lp.prims);
    if prims.len() < 2 {
        return Err("degenerate");
    }
    let mut items = Vec::new();
    for p in &prims {
        let snapped = match *p {
            Prim::Line { a, b, .. } => lat.direction_index(direction(a, b)).map(|k| {
                let nl = lat.normal(k); // the left normal points into the region
                let mid = (a + b) * 0.5;
                let mut c = lat.snap_up(k, nl.dot(mid), EPS);
                for _ in 0..inset {
                    // The next line in: the nearest one more than EPS further.
                    c = lat.snap_up(k, c, -EPS);
                }
                Line {
                    k,
                    d: Vec2::new(nl.y, -nl.x),
                    nl,
                    c,
                }
            }),
            Prim::Arc { .. } => None,
        };
        items.push(Item {
            snapped,
            prim: *p,
            start: p.start(),
        });
    }
    if items.iter().all(|i| i.snapped.is_none()) {
        return Err(if prims.iter().any(|p| matches!(p, Prim::Arc { .. })) {
            "has curves"
        } else {
            "edge off the lattice directions"
        });
    }
    let (items, verts) = snapped_vertices(items)?;
    let n = verts.len();
    let mut out_prims = Vec::new();
    for (k, item) in items.iter().enumerate() {
        let (Some(&a), Some(&b)) = (verts.get(k), verts.get(ring_next(k, n))) else {
            return Err(COLLAPSED);
        };
        out_prims.push(match item.snapped {
            Some(_) => Prim::Line {
                a,
                b,
                lattice: true,
            },
            None => refit(&item.prim, a, b).ok_or(COLLAPSED)?,
        });
    }
    if !simple(&out_prims) {
        return Err("self-intersects when snapped");
    }
    let out = Loop::new(out_prims, Some(false)).ok_or(COLLAPSED)?;
    if (out.signed_area() > 0.0) != (lp.signed_area() > 0.0) {
        return Err(COLLAPSED);
    }
    Ok(out)
}

/// The pieces with neighbouring straight ones on one line merged (see [`merge_collinear`]);
/// arcs stay as they are. A ring without arcs is merged as one ring.
fn merge_straight_runs(prims: &[Prim]) -> Vec<Prim> {
    let Some(first_arc) = prims.iter().position(|p| matches!(p, Prim::Arc { .. })) else {
        let segs = prims.iter().map(|p| (p.start(), p.end())).collect();
        return merge_collinear(segs)
            .into_iter()
            .map(|(a, b)| Prim::line(a, b))
            .collect();
    };
    let mut out: Vec<Prim> = Vec::new();
    let mut run: Vec<(Vec2, Vec2)> = Vec::new();
    let flush = |run: &mut Vec<(Vec2, Vec2)>, out: &mut Vec<Prim>| {
        out.extend(
            merge_forward(std::mem::take(run))
                .into_iter()
                .map(|(a, b)| Prim::line(a, b)),
        );
    };
    // Start at an arc, so a run of straight pieces is never split by the start of the ring.
    for p in prims.iter().cycle().skip(first_arc).take(prims.len()) {
        match *p {
            Prim::Line { a, b, .. } => run.push((a, b)),
            Prim::Arc { .. } => {
                flush(&mut run, &mut out);
                out.push(*p);
            }
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Flags the straight edges of an unsnapped loop that already lie on lattice lines.
fn mark_lattice_segments(lp: &Loop, lat: &Lattice) -> Loop {
    let prims = lp
        .prims
        .iter()
        .map(|p| match *p {
            Prim::Line { a, b, .. } => {
                let on = lat.direction_index(direction(a, b)).is_some_and(|k| {
                    let nl = lat.normal(k);
                    lat.on_line(k, nl.dot(a), EPS) && lat.on_line(k, nl.dot(b), EPS)
                });
                Prim::Line { a, b, lattice: on }
            }
            Prim::Arc { .. } => *p,
        })
        .collect();
    Loop::new(prims, Some(lp.flattened)).unwrap_or_else(|| lp.clone())
}

/// Snaps every loop that can (the fields, the keep-outs, or both, as asked). Returns the region and a warning for each loop that collapsed.
pub(crate) fn snap_region(
    region: &Region,
    lat: &Lattice,
    (snap_fields, snap_keepouts): (bool, bool),
    inset: u32,
) -> (Region, Vec<String>) {
    let mut warnings = Vec::new();
    let mut parts = Vec::new();
    for (outer, holes) in &region.parts {
        let mut new = Vec::new();
        let loops = std::iter::once((outer, false)).chain(holes.iter().map(|h| (h, true)));
        for (lp, keepout) in loops {
            if keepout && !snap_keepouts || !keepout && !snap_fields {
                new.push(mark_lattice_segments(lp, lat));
                continue;
            }
            // A loop with a flattened curve is followed, never snapped.
            let tried = if lp.flattened {
                Err("has curves")
            } else {
                snap_loop(lp, lat, if keepout { 0 } else { inset })
            };
            let snapped = match tried {
                Ok(s) => s,
                Err(why) => {
                    if why != "has curves" && why != "edge off the lattice directions" {
                        warnings.push(format!("a loop {why}; it is kept as drawn"));
                    }
                    mark_lattice_segments(lp, lat)
                }
            };
            new.push(snapped);
        }
        let mut it = new.into_iter();
        if let Some(first) = it.next() {
            parts.push((first, it.collect()));
        }
    }
    (Region::new(parts, lat.p), warnings)
}
