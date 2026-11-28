//! Coarsening an outline for packing ([`coarsen`]): fewer corners, so a layout has less to try,
//! grown outward only, so whatever is kept clear of the coarse outline is clear of the true one.
//!
//! Each step takes one corner off: a corner turning inward is dropped and its neighbours joined,
//! or an edge goes and the edges either side of it are extended until they meet. Both add a
//! triangle outside the outline. The step taken is the one whose triangle loses the least area
//! other parts could use; a step that would make the outline cross itself is not taken. Since
//! parts keep the spacing apart, a dent too narrow for a disc of the spacing's radius to enter
//! holds no part anyway: filling it changes no layout (the outline's closing by that disc,
//! grown by the spacing, is the outline grown by the spacing), so it costs nothing. A gear's
//! tooth gaps narrower than the spacing allows go first; an arc's inside, a notch or a slot a
//! part fits in cost their area and stay unless the budget allows. Each step costs more where
//! the outline is coarse already, so the outline is coarsened evenly, not in one spot.
//!
//! Where a disc reaches is judged on a grid of cells (a quarter of the radius, or finer), not on
//! the exact outline: it only ranks the steps. The coarse outline covers the true one exactly.

use std::collections::BinaryHeap;

use crate::math::Vec2;

use super::{is_simple, Shape2, MAX_COORD, MAX_CORNERS};

/// How much area other parts could use a coarsened outline may lose, over its own area.
pub const COARSEN_BUDGET: f64 = 1e-3;

/// Most cells along a side of the grid where a disc's reach is judged.
const GRID: usize = 1024;

/// A triangle more cells than this across in its box is costed by its whole area.
const MAX_SCAN: usize = 1 << 14;

/// `shape`'s outer ring with corners taken off while that loses no more than `budget` of its
/// area that other parts could use, kept `reach` (the spacing) away. The result covers `shape`
/// and stays within its box; its holes are kept as they are. A ring that is not finite, too
/// large, too detailed for a layout or has no area comes back as it is.
pub fn coarsen(shape: &Shape2, reach: f64, budget: f64) -> Shape2 {
    let ring = &shape.outer;
    if ring.len() <= 3
        || ring.len() > MAX_CORNERS
        || ring
            .iter()
            .any(|p| !(p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD))
    {
        return shape.clone();
    }
    let area = signed_area(ring);
    if !(area.is_finite() && area != 0.0) {
        return shape.clone();
    }
    let ccw: Vec<Vec2> = if area > 0.0 {
        ring.clone()
    } else {
        ring.iter().rev().copied().collect()
    };
    let room = Room::new(&ccw, reach);
    let allowed = if budget.is_finite() && budget > 0.0 {
        budget * area.abs()
    } else {
        0.0
    };
    let Some(mut out) = shrink(ccw.clone(), &room, allowed) else {
        return shape.clone();
    };
    // Each step only adds area outside, but rounding is checked too: an outline that crosses
    // itself or leaves out a corner of the true one is not used.
    if out.len() == ccw.len() || !is_simple(&out) || !surrounds(&out, &ccw) {
        return shape.clone();
    }
    if area < 0.0 {
        out.reverse();
    }
    Shape2 {
        outer: out,
        holes: shape.holes.clone(),
    }
}

/// Whether every corner of `fine` is inside `coarse` or on its edge (to within rounding).
fn surrounds(coarse: &[Vec2], fine: &[Vec2]) -> bool {
    let (lo, hi) = bounds(fine);
    let tol = 1e-9 * (hi.x - lo.x).hypot(hi.y - lo.y).max(1e-9);
    fine.iter().all(|q| {
        let mut inside = false;
        for (a, b) in sides(coarse) {
            if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
        }
        inside || sides(coarse).any(|(a, b)| touch(*q, *q, a, b, tol))
    })
}

/// The box of `ring`'s corners.
fn bounds(ring: &[Vec2]) -> (Vec2, Vec2) {
    let first = ring.first().copied().unwrap_or(Vec2::new(0.0, 0.0));
    ring.iter().fold((first, first), |(lo, hi), p| {
        (
            Vec2::new(lo.x.min(p.x), lo.y.min(p.y)),
            Vec2::new(hi.x.max(p.x), hi.y.max(p.y)),
        )
    })
}

fn cross(o: Vec2, a: Vec2, b: Vec2) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Each side of the closed ring `r`: a corner and the next, the last back to the first.
fn sides(r: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    r.iter().copied().zip(r.iter().copied().cycle().skip(1))
}

fn signed_area(ring: &[Vec2]) -> f64 {
    sides(ring)
        .map(|(a, b)| cross(Vec2::new(0.0, 0.0), a, b))
        .sum::<f64>()
        / 2.0
}

/// Where other parts can't reach: the cells a disc of the spacing's radius can't enter from
/// outside without touching the outline (the outline's closing by that disc).
struct Room {
    grid: Option<Grid>,
}

struct Grid {
    lo: Vec2,
    h: f64,
    nx: usize,
    ny: usize,
    shut: Vec<bool>,
}

impl Room {
    fn new(ring: &[Vec2], r: f64) -> Room {
        Room {
            grid: Grid::new(ring, r),
        }
    }

    /// The area of triangle `t` (outside the outline) other parts could use.
    fn lost(&self, t: [Vec2; 3]) -> f64 {
        let [t0, t1, t2] = t;
        let area = cross(t0, t1, t2).abs() / 2.0;
        let Some(g) = &self.grid else { return area };
        let lo = Vec2::new(t0.x.min(t1.x).min(t2.x), t0.y.min(t1.y).min(t2.y));
        let hi = Vec2::new(t0.x.max(t1.x).max(t2.x), t0.y.max(t1.y).max(t2.y));
        let cell = |v: f64, from: f64, n: usize| {
            ((v - from) / g.h).floor().max(0.0).min(n as f64) as usize
        };
        let (x0, x1) = (
            cell(lo.x, g.lo.x, g.nx),
            cell(hi.x, g.lo.x, g.nx).saturating_add(1).min(g.nx),
        );
        let (y0, y1) = (
            cell(lo.y, g.lo.y, g.ny),
            cell(hi.y, g.lo.y, g.ny).saturating_add(1).min(g.ny),
        );
        if x1.saturating_sub(x0).saturating_mul(y1.saturating_sub(y0)) > MAX_SCAN {
            return area;
        }
        let side = cross(t0, t1, t2).signum();
        let shut_at = |c: Vec2| {
            let (x, y) = (
                ((c.x - g.lo.x) / g.h).floor(),
                ((c.y - g.lo.y) / g.h).floor(),
            );
            x >= 0.0
                && y >= 0.0
                && g.shut
                    .get(y as usize * g.nx + x as usize)
                    .copied()
                    .unwrap_or(false)
                && (x as usize) < g.nx
        };
        // The share of the cells whose centre is in the triangle that are shut; a triangle too
        // small to hold a centre is judged at its own centre.
        let (mut cells, mut shut) = (0usize, 0usize);
        for y in y0..y1 {
            let cy = g.lo.y + (y as f64 + 0.5) * g.h;
            for x in x0..x1 {
                let c = Vec2::new(g.lo.x + (x as f64 + 0.5) * g.h, cy);
                if [(t0, t1), (t1, t2), (t2, t0)]
                    .iter()
                    .all(|(a, b)| cross(*a, *b, c) * side >= 0.0)
                {
                    cells += 1;
                    shut += usize::from(g.shut.get(y * g.nx + x).copied().unwrap_or(false));
                }
            }
        }
        if cells == 0 {
            let mid = Vec2::new((t0.x + t1.x + t2.x) / 3.0, (t0.y + t1.y + t2.y) / 3.0);
            return if shut_at(mid) { 0.0 } else { area };
        }
        area * (1.0 - shut as f64 / cells as f64)
    }
}

impl Grid {
    fn new(ring: &[Vec2], r: f64) -> Option<Grid> {
        if !(r.is_finite() && r > 0.0) {
            return None;
        }
        let (lo, hi) = ring.iter().fold(
            (ring.first().copied()?, ring.first().copied()?),
            |(lo, hi), p| {
                (
                    Vec2::new(lo.x.min(p.x), lo.y.min(p.y)),
                    Vec2::new(hi.x.max(p.x), hi.y.max(p.y)),
                )
            },
        );
        let extent = (hi.x - lo.x).max(hi.y - lo.y) + 2.0 * r;
        let h = (r / 4.0).max(extent / GRID as f64);
        // Cells as large as the disc can't tell a dent it fits from one it doesn't.
        if !(h.is_finite() && h > 0.0 && h < r / 2.0) {
            return None;
        }
        let pad = r + 2.0 * h;
        let lo = Vec2::new(lo.x - pad, lo.y - pad);
        let nx = ((hi.x - lo.x + pad) / h).ceil() as usize + 1;
        let ny = ((hi.y - lo.y + pad) / h).ceil() as usize + 1;
        // Cells whose centre is inside the outline (crossings along each row).
        let mut inside = vec![false; nx * ny];
        let mut xs = Vec::new();
        for y in 0..ny {
            let cy = lo.y + (y as f64 + 0.5) * h;
            xs.clear();
            for (a, b) in sides(ring) {
                if (a.y > cy) != (b.y > cy) {
                    xs.push(a.x + (cy - a.y) * (b.x - a.x) / (b.y - a.y));
                }
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks(2) {
                let [x0, x1] = pair else { continue };
                let from = ((x0 - lo.x) / h - 0.5).ceil().max(0.0) as usize;
                let to = (((x1 - lo.x) / h - 0.5).floor() + 1.0)
                    .max(0.0)
                    .min(nx as f64) as usize;
                for c in inside
                    .iter_mut()
                    .skip(y * nx)
                    .take(nx)
                    .skip(from)
                    .take(to.saturating_sub(from))
                {
                    *c = true;
                }
            }
        }
        // Grown by the radius, then shrunk by it: what a disc from outside can't enter. Each with
        // a cell's width to spare (more than a cell's half diagonal), so a dent counts as shut only
        // when it surely is.
        let near = distances(&inside, nx, ny);
        let grown: Vec<bool> = near.iter().map(|d| d.sqrt() * h <= r - h).collect();
        let outside: Vec<bool> = grown.iter().map(|g| !g).collect();
        let away = distances(&outside, nx, ny);
        let shut = away
            .iter()
            .zip(&inside)
            .map(|(d, i)| !i && d.sqrt() * h >= r + h)
            .collect();
        Some(Grid {
            lo,
            h,
            nx,
            ny,
            shut,
        })
    }
}

/// Each cell's squared distance (in cells) to the nearest cell set in `mask`.
fn distances(mask: &[bool], nx: usize, ny: usize) -> Vec<f64> {
    const FAR: f64 = 1e20;
    let mut d: Vec<f64> = mask.iter().map(|m| if *m { 0.0 } else { FAR }).collect();
    let mut f = Vec::new();
    let mut out = Vec::new();
    for y in 0..ny {
        f.clear();
        f.extend(d.iter().skip(y * nx).take(nx).copied());
        line_distances(&f, &mut out);
        for (c, v) in d.iter_mut().skip(y * nx).take(nx).zip(&out) {
            *c = *v;
        }
    }
    for x in 0..nx {
        f.clear();
        f.extend((0..ny).map(|y| d.get(y * nx + x).copied().unwrap_or(FAR)));
        line_distances(&f, &mut out);
        for (y, v) in out.iter().enumerate() {
            if let Some(c) = d.get_mut(y * nx + x) {
                *c = *v;
            }
        }
    }
    d
}

/// The squared distance transform of one line of samples `f`: for each `q`, the least of
/// `(q - p)^2 + f[p]` (the lower envelope of parabolas).
fn line_distances(f: &[f64], out: &mut Vec<f64>) {
    let n = f.len();
    out.clear();
    if n == 0 {
        return;
    }
    let at = |p: usize| f.get(p).copied().unwrap_or(f64::INFINITY);
    let meet = |q: usize, p: usize| {
        ((at(q) + (q * q) as f64) - (at(p) + (p * p) as f64)) / (2.0 * (q as f64 - p as f64))
    };
    // The parabolas of the lower envelope, each with where it starts being the lowest; the
    // first starts at minus infinity, so the walk back stops there.
    let mut hull: Vec<(usize, f64)> = vec![(0, f64::NEG_INFINITY)];
    for q in 1..n {
        while let Some(&(p, start)) = hull.last() {
            let s = meet(q, p);
            if hull.len() > 1 && s <= start {
                hull.pop();
            } else {
                hull.push((q, s));
                break;
            }
        }
    }
    let mut k = 0usize;
    for q in 0..n {
        while hull.get(k + 1).is_some_and(|h| h.1 < q as f64) {
            k += 1;
        }
        let p = hull.get(k).map_or(q, |h| h.0);
        let d = q as f64 - p as f64;
        out.push(d * d + at(p));
    }
}

/// One way of taking a corner off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    /// The corner goes and its neighbours are joined: for a corner turning inward.
    Drop,
    /// The edge from the corner to the next goes, the edges either side extended to meet.
    Merge,
}

/// A step's cost, ordered so that a max-heap gives the smallest first.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cost(f64);

impl Eq for Cost {}

impl PartialOrd for Cost {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cost {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.total_cmp(&self.0)
    }
}

/// The links of a ring being coarsened.
struct Links {
    /// The true outline's box.
    bounds: (Vec2, Vec2),
    pos: Vec<Vec2>,
    prev: Vec<usize>,
    next: Vec<usize>,
}

impl Links {
    /// The corner before `i`.
    fn prev_of(&self, i: usize) -> Option<usize> {
        self.prev.get(i).copied()
    }

    /// The corner after `i`.
    fn next_of(&self, i: usize) -> Option<usize> {
        self.next.get(i).copied()
    }

    /// Where corner `i` is.
    fn at(&self, i: usize) -> Option<Vec2> {
        self.pos.get(i).copied()
    }

    /// Links `a` to `b` as the corner after it.
    fn join(&mut self, a: usize, b: usize) {
        if let Some(n) = self.next.get_mut(a) {
            *n = b;
        }
        if let Some(p) = self.prev.get_mut(b) {
            *p = a;
        }
    }

    /// A step at `i`: the corner it adds (for a merge) and the triangle it adds outside.
    fn plan(&self, i: usize, step: Step, tol: f64) -> Option<(Option<Vec2>, [Vec2; 3])> {
        let (p, j) = (*self.prev.get(i)?, *self.next.get(i)?);
        let (a, b, c) = (*self.pos.get(p)?, *self.pos.get(i)?, *self.pos.get(j)?);
        match step {
            // The corner turns inward (or not at all): the triangle it cuts is outside.
            Step::Drop => (cross(a, b, c) <= 0.0).then_some((None, [a, b, c])),
            Step::Merge => {
                let q = *self.next.get(j)?;
                if q == p || q == i {
                    return None;
                }
                let d = *self.pos.get(q)?;
                let (d1, d2) = (
                    Vec2::new(b.x - a.x, b.y - a.y),
                    Vec2::new(c.x - d.x, c.y - d.y),
                );
                let den = d1.x * d2.y - d1.y * d2.x;
                if den.abs() <= tol * tol {
                    return None;
                }
                let w = Vec2::new(c.x - b.x, c.y - b.y);
                let (t, u) = (
                    (w.x * d2.y - w.y * d2.x) / den,
                    (w.x * d1.y - w.y * d1.x) / den,
                );
                if !(t > 0.0 && u > 0.0 && t.is_finite() && u.is_finite()) {
                    return None;
                }
                let x = Vec2::new(b.x + t * d1.x, b.y + t * d1.y);
                // The edges meet outside the edge they replace, and within the true outline's
                // box: a part that fits a space exactly still fits it.
                let (lo, hi) = self.bounds;
                let boxed = x.x >= lo.x - tol
                    && x.x <= hi.x + tol
                    && x.y >= lo.y - tol
                    && x.y <= hi.y + tol;
                (boxed && cross(b, c, x) < 0.0).then_some((Some(x), [b, x, c]))
            }
        }
    }
}

/// The ring `ring` (counter-clockwise) coarsened as [`coarsen`] says.
fn shrink(ring: Vec<Vec2>, room: &Room, allowed: f64) -> Option<Vec<Vec2>> {
    let n = ring.len();
    let (lo, hi) = bounds(&ring);
    let tol = 1e-9 * (hi.x - lo.x).hypot(hi.y - lo.y).max(1e-9);
    let mut ring = Links {
        bounds: (lo, hi),
        pos: ring,
        prev: (0..n).map(|i| (i + n - 1) % n.max(1)).collect(),
        next: (0..n).map(|i| (i + 1) % n.max(1)).collect(),
    };
    let mut alive = vec![true; n];
    let mut stamp = vec![0u32; n];
    let mut heap: BinaryHeap<(Cost, Step, usize, u32)> = BinaryHeap::new();
    let push =
        |heap: &mut BinaryHeap<(Cost, Step, usize, u32)>, ring: &Links, stamp: &[u32], i: usize| {
            for step in [Step::Drop, Step::Merge] {
                if let (Some((_, t)), Some(s)) = (ring.plan(i, step, tol), stamp.get(i)) {
                    let cost = room.lost(t);
                    if cost.is_finite() {
                        heap.push((Cost(cost), step, i, *s));
                    }
                }
            }
        };
    for i in 0..n {
        push(&mut heap, &ring, &stamp, i);
    }
    let (mut left, mut spent) = (n, 0.0);
    while left > 3 {
        let Some((Cost(cost), step, i, st)) = heap.pop() else {
            break;
        };
        if !alive.get(i).copied().unwrap_or(false) || stamp.get(i) != Some(&st) {
            continue;
        }
        // The cheapest step left is over the budget: stop.
        if spent + cost > allowed {
            break;
        }
        let Some((x, _)) = ring.plan(i, step, tol) else {
            continue;
        };
        let (Some(p), Some(j)) = (ring.prev_of(i), ring.next_of(i)) else {
            continue;
        };
        let (Some(pp), Some(pi), Some(pj), Some(before)) =
            (ring.at(p), ring.at(i), ring.at(j), ring.prev_of(p))
        else {
            continue;
        };
        // The new edges, and the stretch of the rest of the ring they must stay clear of.
        let (new, to) = match x {
            None => (vec![(pp, pj)], before),
            Some(x) => (vec![(pi, x), (x, pj)], p),
        };
        let (blo, bhi) = new.iter().fold(
            (
                Vec2::new(f64::INFINITY, f64::INFINITY),
                Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
            ),
            |(lo, hi), (a, b)| {
                (
                    Vec2::new(lo.x.min(a.x).min(b.x) - tol, lo.y.min(a.y).min(b.y) - tol),
                    Vec2::new(hi.x.max(a.x).max(b.x) + tol, hi.y.max(a.y).max(b.y) + tol),
                )
            },
        );
        let mut crosses = false;
        let mut k = ring.next_of(j);
        while let Some(at) = k.filter(|k| *k != to && !crosses) {
            let next = ring.next_of(at);
            let (Some(e0), Some(e1)) = (ring.at(at), next.and_then(|e| ring.at(e))) else {
                // A broken link: refuse the step rather than guess.
                crosses = true;
                break;
            };
            let missed = e0.x.max(e1.x) < blo.x
                || e0.x.min(e1.x) > bhi.x
                || e0.y.max(e1.y) < blo.y
                || e0.y.min(e1.y) > bhi.y;
            crosses = !missed && new.iter().any(|(a, b)| touch(*a, *b, e0, e1, tol));
            k = next;
        }
        if crosses {
            continue;
        }
        match x {
            None => {
                if let Some(a) = alive.get_mut(i) {
                    *a = false;
                }
                ring.join(p, j);
            }
            Some(x) => {
                let Some(q) = ring.next_of(j) else { continue };
                if let Some(a) = alive.get_mut(j) {
                    *a = false;
                }
                if let Some(at) = ring.pos.get_mut(i) {
                    *at = x;
                }
                ring.join(i, q);
            }
        }
        spent += cost;
        left -= 1;
        // The steps near the change are worked out again.
        let mut k = p;
        for _ in 0..2 {
            k = ring.prev_of(k).unwrap_or(k);
        }
        for _ in 0..6 {
            if let Some(s) = stamp.get_mut(k) {
                *s = s.wrapping_add(1);
            }
            push(&mut heap, &ring, &stamp, k);
            k = ring.next_of(k).unwrap_or(k);
        }
    }
    let mut k = alive.iter().position(|a| *a)?;
    let mut out = Vec::new();
    for _ in 0..left {
        out.push(ring.at(k)?);
        k = ring.next_of(k)?;
    }
    Some(out)
}

/// Whether segments `a0 a1` and `b0 b1` cross or come within `tol` of each other.
fn touch(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2, tol: f64) -> bool {
    let (d1, d2, d3, d4) = (
        cross(b0, b1, a0),
        cross(b0, b1, a1),
        cross(a0, a1, b0),
        cross(a0, a1, b1),
    );
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        return true;
    }
    let near = |p: Vec2, a: Vec2, b: Vec2| {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 {
            (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (p.x - a.x - t * dx).hypot(p.y - a.y - t * dy) <= tol
    };
    near(a0, b0, b1) || near(a1, b0, b1) || near(b0, a0, a1) || near(b1, a0, a1)
}
