//! Snapping a field to the lattice: each straight edge moves onto the nearest lattice line of
//! its direction on the solid side, so fields shrink and keep-outs grow, never the other way.

use crate::math::Vec2;

use super::vec::{ring_next, ring_prev};

use super::lattice::Lattice;
use super::region::{line_intersection, Loop, Prim, Region, EPS};

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

fn segments_meet(p0: Vec2, p1: Vec2, q0: Vec2, q1: Vec2) -> bool {
    let orient = |a: Vec2, b: Vec2, c: Vec2| {
        let v = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if v.abs() < 1e-12 {
            0
        } else if v > 0.0 {
            1
        } else {
            -1
        }
    };
    orient(p0, p1, q0) != orient(p0, p1, q1) && orient(q0, q1, p0) != orient(q0, q1, p1)
}

/// True when no two non-adjacent edges of the closed polygon meet.
fn simple(poly: &[Vec2]) -> bool {
    let n = poly.len();
    for a in 0..n {
        let (Some(&p0), Some(&p1)) = (poly.get(a), poly.get(ring_next(a, n))) else {
            return false;
        };
        for b in a + 2..n {
            if a == 0 && b == n - 1 {
                continue;
            }
            let (Some(&q0), Some(&q1)) = (poly.get(b), poly.get(ring_next(b, n))) else {
                return false;
            };
            if segments_meet(p0, p1, q0, q1) {
                return false;
            }
        }
    }
    true
}

/// The vertices of the loop bounded by the snapped lines, in order.
///
/// Snapping can land neighbouring edges on the same line, or shrink an edge to nothing (a small
/// step between two edges that snap to one line). Those are merged or dropped, and the rest
/// meet again, until the loop is stable. `Err` when the loop really collapses: an edge reverses,
/// or two neighbours end up parallel on different lines.
fn snapped_vertices(mut lines: Vec<Line>) -> Result<Vec<Vec2>, &'static str> {
    const COLLAPSED: &str = "collapses when snapped";
    loop {
        // Neighbours on the same line become one edge, in one pass round the ring.
        lines = merge_same_line(lines);
        if lines.len() < 3 {
            return Err(COLLAPSED);
        }
        let n = lines.len();
        let mut verts = Vec::new();
        for k in 0..n {
            let (Some(a), Some(b)) = (lines.get(ring_prev(k, n)), lines.get(k)) else {
                return Err(COLLAPSED);
            };
            verts.push(line_intersection(a.nl, a.c, b.nl, b.c).ok_or(COLLAPSED)?);
        }
        let mut zero = None;
        for k in 0..n {
            let (Some(&v0), Some(&v1), Some(l)) =
                (verts.get(k), verts.get(ring_next(k, n)), lines.get(k))
            else {
                return Err(COLLAPSED);
            };
            let len = (v1 - v0).dot(l.d);
            if len < -EPS {
                return Err(COLLAPSED);
            }
            if len <= EPS && zero.is_none() {
                zero = Some(k);
            }
        }
        match zero {
            None => return Ok(verts),
            // Its neighbours now meet directly; go round again.
            Some(k) => {
                lines = without(lines, k);
            }
        }
    }
}

/// Runs of neighbouring lines that are one line (the same lattice direction and offset) as one,
/// a run that wraps round the ring included.
fn merge_same_line(lines: Vec<Line>) -> Vec<Line> {
    let same = |a: &Line, b: &Line| a.k == b.k && (a.c - b.c).abs() <= EPS;
    let mut out: Vec<Line> = Vec::new();
    for line in lines {
        if out.last().is_some_and(|last| same(last, &line)) {
            continue;
        }
        out.push(line);
    }
    while out.len() > 2 {
        if !matches!((out.first(), out.last()), (Some(f), Some(l)) if same(l, f)) {
            break;
        }
        out.pop();
    }
    out
}

/// `items` without the one at `k`.
fn without<T>(items: Vec<T>, k: usize) -> Vec<T> {
    items
        .into_iter()
        .enumerate()
        .filter_map(|(i, x)| (i != k).then_some(x))
        .collect()
}

/// The loop with each edge moved onto the nearest lattice line on the region's side. The loop
/// must run with the region on its left. `Err` says why a loop cannot snap.
pub(crate) fn snap_loop(lp: &Loop, lat: &Lattice) -> Result<Loop, &'static str> {
    if lp.curved {
        return Err("has curves");
    }
    let mut segs = Vec::new();
    for p in &lp.prims {
        match *p {
            Prim::Line { a, b, .. } => segs.push((a, b)),
            Prim::Arc { .. } => return Err("has curves"),
        }
    }
    let segs = merge_collinear(segs);
    if segs.len() < 3 {
        return Err("degenerate");
    }
    let mut lines = Vec::new();
    for &(a, b) in &segs {
        let k = lat
            .direction_index(direction(a, b))
            .ok_or("edge off the lattice directions")?;
        let nl = lat.normal(k); // the left normal points into the region
        let d = Vec2::new(nl.y, -nl.x);
        let mid = (a + b) * 0.5;
        lines.push(Line {
            k,
            d,
            nl,
            c: lat.snap_up(k, nl.dot(mid), EPS),
        });
    }
    let verts = snapped_vertices(lines)?;
    let n = verts.len();
    let prims: Vec<Prim> = (0..n)
        .filter_map(|k| {
            Some(Prim::Line {
                a: *verts.get(k)?,
                b: *verts.get(ring_next(k, n))?,
                lattice: true,
            })
        })
        .collect();
    if !simple(&verts) {
        return Err("self-intersects when snapped");
    }
    let out = Loop::new(prims, Some(false)).ok_or("collapses when snapped")?;
    if (out.signed_area() > 0.0) != (lp.signed_area() > 0.0) {
        return Err("collapses when snapped");
    }
    Ok(out)
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
    Loop::new(prims, Some(lp.curved)).unwrap_or_else(|| lp.clone())
}

/// Snaps every loop that can. Returns the region and a warning for each loop that collapsed.
pub(crate) fn snap_region(region: &Region, lat: &Lattice) -> (Region, Vec<String>) {
    let mut warnings = Vec::new();
    let mut parts = Vec::new();
    for (outer, holes) in &region.parts {
        let mut new = Vec::new();
        for lp in std::iter::once(outer).chain(holes) {
            let snapped = match snap_loop(lp, lat) {
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
