//! Which points of two neighbouring loft sections the loft's side faces join.
//!
//! A loft's side faces run from a point of one section to a point of the next, so every section
//! needs the same number of vertices, in the same order round. Each vertex of either section is
//! either paired with a vertex of the other or left alone, and a lone vertex then splits the
//! other section's segment where it lands. The pairing is the one that costs least, round both
//! loops in order ([`correspond`]), so a section gains a vertex for each one of the other it has
//! no match for: a corner pairs with the corner it lines up with, and a vertex where a side runs
//! straight on (or a curve turns smoothly) is cheap to leave alone.
//!
//! The cost of joining two points is how far apart they are, each section moved to its centroid
//! and scaled to unit area, plus how much their tangents differ on either side; a lone vertex
//! costs a little more ([`UNPAIRED`]), so that two points that line up pair. The cyclic
//! alignment is a shortest path through the grid of both loops' vertices, found for every start
//! by divide and conquer: two best paths from different starts can always be chosen not to cross,
//! so each start's path is searched between its neighbours' ([`best_walk`]).

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;

use solidcore_record::{Loop2, Seg2, Vec2};

use crate::numbered::NumberedLoop;
use crate::{KernelError, Result};

/// The weight of the tangent terms against the distance between the points.
const TURN: f64 = 1.0;
/// What a lone vertex costs on top of its distance and tangents: less than any corner's
/// mismatch, so a sharp corner pairs with a sharp corner, but enough that two points which line
/// up pair rather than both splitting the other section.
const UNPAIRED: f64 = 0.1;
/// The most cells the grid of a pairing may have ((m + 1) by (n + 1): two sections of about 2000
/// segments each). Each start's search stores one byte a cell, and the search over every start
/// visits about log2(n) times the grid.
const MAX_CELLS: usize = 4_000_000;
/// The deepest the divide and conquer over the starts goes (log2 of the vertex count: about 22
/// for any loop within `MAX_CELLS`).
const MAX_DEPTH: usize = 64;
/// Shares of a gap closer than this are the same point: two lone vertices landing on each other
/// pair.
const SAME_SHARE: f64 = 1e-9;

/// A section's plane coordinates in the frame the loft's sections share: the rotation (a
/// reflection where the section's plane faces back) from the plane's axes to the shared ones,
/// then the loop's centroid moved to the origin and its area scaled to one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    u: Vec2,
    v: Vec2,
    centre: Vec2,
    scale: f64,
}

impl Frame {
    /// The frame of `lp` whose shared axes are `u` and `v` (orthonormal, in plane coordinates).
    pub(crate) fn new(u: Vec2, v: Vec2, lp: &Loop2) -> Frame {
        let area = lp.signed_area().abs().sqrt();
        let perimeter: f64 = lp.segs.iter().map(Seg2::length).sum();
        // A loop with no area still gets a finite scale (its matching is then by position alone).
        let scale = if area.is_finite() && area > 0.0 {
            area
        } else if perimeter.is_finite() && perimeter > 0.0 {
            perimeter / 4.0
        } else {
            1.0
        };
        Frame { u, v, centre: lp.centroid(), scale }
    }

    fn point(&self, p: Vec2) -> Vec2 {
        self.dir(p - self.centre) * (1.0 / self.scale)
    }

    fn dir(&self, d: Vec2) -> Vec2 {
        Vec2::new(self.u.dot(d), self.v.dot(d))
    }

    /// The plane point of the shared point `q`.
    fn plane_point(&self, q: Vec2) -> Vec2 {
        self.centre + (self.u * q.x + self.v * q.y) * self.scale
    }
}

/// Where the cuts go that make two loops correspond: `a_cuts[i]` and `b_cuts[j]` are the
/// increasing parameters, inside (0, 1), at which segment `i` of the first and segment `j` of the
/// second split ([`NumberedLoop::cut`]); cut, both have the same number of segments, and the
/// second's segment `b_start` joins the first's segment 0.
#[derive(Clone, Debug)]
pub(crate) struct Correspondence {
    pub a_cuts: Vec<Vec<f64>>,
    pub b_cuts: Vec<Vec<f64>>,
    pub b_start: usize,
}

/// One loop as the pairing sees it: its segments, and its vertices' places and tangents in the
/// shared frame.
struct Side<'a> {
    segs: &'a [Seg2],
    frame: Frame,
    at: Vec<Vec2>,
    /// The unit tangent arriving at each vertex, and leaving it.
    before: Vec<Vec2>,
    after: Vec<Vec2>,
    len: Vec<f64>,
    /// For each curved segment, its points at even parameters ([`nearest`]).
    chords: Vec<Vec<Vec2>>,
}

impl<'a> Side<'a> {
    fn new(segs: &'a [Seg2], frame: Frame) -> Result<Side<'a>> {
        let n = segs.len();
        let at: Vec<Vec2> = segs.iter().map(|s| frame.point(s.start())).collect();
        let after: Vec<Vec2> = segs.iter().map(|s| frame.dir(s.start_tangent())).collect();
        let before: Vec<Vec2> = (0..n).filter_map(|i| segs.get((i + n - 1) % n)).map(|s| frame.dir(s.end_tangent())).collect();
        let len: Vec<f64> = segs.iter().map(Seg2::length).collect();
        let finite = |v: &[Vec2]| v.iter().all(|p| p.is_finite());
        let total: f64 = len.iter().sum();
        if n == 0 || before.len() != n || !finite(&at) || !finite(&before) || !finite(&after) || !(total.is_finite() && total > 0.0) {
            return Err(KernelError::Invalid("a loft section has no length or a point that is not finite".into()));
        }
        let chords = segs
            .iter()
            .map(|s| match s {
                Seg2::Line { .. } | Seg2::Arc { .. } => Vec::new(),
                _ => (0..=CHORDS).map(|k| s.point_at(k as f64 / CHORDS as f64)).collect(),
            })
            .collect();
        Ok(Side { segs, frame, at, before, after, len, chords })
    }

    fn count(&self) -> usize {
        self.segs.len()
    }

    /// Where the shared point `q` lands on segment `e`: the parameter of the segment's nearest
    /// point, that point and the unit tangent there (both in the shared frame).
    fn land(&self, e: usize, q: Vec2) -> Option<(f64, Vec2, Vec2)> {
        let s = self.segs.get(e)?;
        let chord = self.chords.get(e).map(Vec::as_slice).unwrap_or(&[]);
        let t = nearest(s, chord, self.frame.plane_point(q));
        let tangent = s.derivative(t).normalized().unwrap_or_else(|| if t < 0.5 { s.start_tangent() } else { s.end_tangent() });
        Some((t, self.frame.point(s.point_at(t)), self.frame.dir(tangent)))
    }

    /// The vertices from vertex `i` on round to vertex `k` (once round when they are the same),
    /// each with the length of the loop from `i` to it: `k` last, with the whole length.
    fn run(&self, i: usize, k: usize) -> Vec<(usize, f64)> {
        let n = self.count();
        let mut out = Vec::new();
        let (mut at, mut d) = (i % n, 0.0);
        for _ in 0..n {
            d += self.len.get(at).copied().unwrap_or(0.0);
            at = (at + 1) % n;
            out.push((at, d));
            if at == k % n {
                break;
            }
        }
        out
    }
}

/// A point moving forward round a loop from one of its vertices: [`Cursor::to`] finds the
/// segment and parameter a growing distance on, without going past the vertex `end`.
struct Cursor<'s, 'a> {
    side: &'s Side<'a>,
    seg: usize,
    end: usize,
    before: f64,
}

impl Cursor<'_, '_> {
    fn to(&mut self, d: f64) -> Option<(usize, f64)> {
        let n = self.side.count();
        loop {
            let l = *self.side.len.get(self.seg)?;
            let next = (self.seg + 1) % n;
            if d < self.before + l || next == self.end % n {
                let t = if l > 0.0 { (d - self.before) / l } else { 0.5 };
                return Some((self.seg, t.clamp(1e-9, 1.0 - 1e-9)));
            }
            self.before += l;
            self.seg = next;
        }
    }
}

/// How many chords a curved segment's nearest point is found on.
const CHORDS: usize = 8;

/// The parameter of the point of `s` nearest the plane point `p`; for a curve (neither a line nor
/// an arc), of the polyline through `chord`, its points at the parameters k / `CHORDS`.
fn nearest(s: &Seg2, chord: &[Vec2], p: Vec2) -> f64 {
    match *s {
        Seg2::Line { a, b } => {
            let d = b - a;
            let t = (p - a).dot(d) / d.len2();
            if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }
        }
        Seg2::Arc { center, start, sweep, .. } => {
            use std::f64::consts::TAU;
            let w = sweep.abs();
            if !(w > 0.0) || p.dist(center) == 0.0 {
                return 0.5;
            }
            // How far round the arc, in its direction of travel, the point's direction lies.
            let along = ((p - center).angle() - start) * sweep.signum();
            let u = along.rem_euclid(TAU);
            if u <= w {
                u / w
            } else if TAU - u < u - w {
                0.0
            } else {
                1.0
            }
        }
        // A curve: the nearest point of the polyline through its points at even parameters
        // (`CHORDS` pieces), as a cost needs it no closer and a search per grid cell is slow.
        _ => {
            let mut best = (f64::INFINITY, 0.0);
            for k in 0..CHORDS {
                let (t0, t1) = (k as f64 / CHORDS as f64, (k + 1) as f64 / CHORDS as f64);
                let (a, b) = (chord.get(k).copied().unwrap_or_else(|| s.point_at(t0)), chord.get(k + 1).copied().unwrap_or_else(|| s.point_at(t1)));
                let d = b - a;
                let u = ((p - a).dot(d) / d.len2()).clamp(0.0, 1.0);
                let u = if u.is_finite() { u } else { 0.0 };
                let dist = p.dist(a + d * u);
                if dist < best.0 {
                    best = (dist, t0 + (t1 - t0) * u);
                }
            }
            best.1
        }
    }
}

/// How much two unit tangents differ: 0 when they agree, 2 when they are opposite.
fn turn(a: Vec2, b: Vec2) -> f64 {
    1.0 - a.dot(b)
}

/// The two loops of a pairing: `a` along the grid's rows, `b` along its columns.
struct Grid<'a> {
    a: Side<'a>,
    b: Side<'a>,
}

impl Grid<'_> {
    /// Vertex `i` of `a` paired with vertex `j` of `b`.
    fn pair(&self, i: usize, j: usize) -> f64 {
        let g = |v: &Vec<Vec2>, k: usize| v.get(k).copied().unwrap_or(Vec2::ZERO);
        let (a, b) = (&self.a, &self.b);
        g(&a.at, i).dist(g(&b.at, j)) + TURN * 0.5 * (turn(g(&a.before, i), g(&b.before, j)) + turn(g(&a.after, i), g(&b.after, j)))
    }

    /// Vertex `i` of `x` alone, on segment `e` of `y`.
    fn alone(x: &Side, i: usize, y: &Side, e: usize) -> f64 {
        let (Some(&q), Some(&before), Some(&after)) = (x.at.get(i), x.before.get(i), x.after.get(i)) else { return f64::INFINITY };
        match y.land(e, q) {
            Some((_, p, t)) => q.dist(p) + TURN * 0.5 * (turn(before, t) + turn(after, t)) + UNPAIRED,
            None => f64::INFINITY,
        }
    }

    fn alone_a(&self, i: usize, e: usize) -> f64 {
        Grid::alone(&self.a, i, &self.b, e)
    }

    fn alone_b(&self, j: usize, e: usize) -> f64 {
        Grid::alone(&self.b, j, &self.a, e)
    }
}

/// A step of a walk through the grid: from node (i, j) (the first i vertices of `a` and the
/// vertices of `b` from the start up to column j behind it), vertex i of `a` paired with vertex
/// j of `b`, or either alone, landing on the other loop's segment between the vertex behind and
/// the one ahead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Pair,
    A,
    B,
}

/// A walk from node (0, `start`) to (m, `start` + n): one correspondence round both loops, `b`
/// started at its vertex `start`.
#[derive(Clone, Debug)]
struct Walk {
    start: usize,
    cost: f64,
    steps: Vec<Step>,
}

impl Walk {
    /// For each row, the least and the greatest column the walk visits there.
    fn bounds(&self, m: usize) -> (Vec<usize>, Vec<usize>) {
        let (mut lo, mut hi) = (vec![usize::MAX; m + 1], vec![0usize; m + 1]);
        let (mut i, mut j) = (0usize, self.start);
        let mut visit = |i: usize, j: usize| {
            if let (Some(l), Some(h)) = (lo.get_mut(i), hi.get_mut(i)) {
                *l = (*l).min(j);
                *h = (*h).max(j);
            }
        };
        visit(i, j);
        for s in &self.steps {
            match s {
                Step::Pair => (i, j) = (i + 1, j + 1),
                Step::A => i += 1,
                Step::B => j += 1,
            }
            visit(i, j);
        }
        (lo, hi)
    }
}

/// The least-cost walk from (0, `s`) to (m, `s` + n) through the nodes of row i at columns
/// `lo[i]..=hi[i]`. Ties go to a pair, then to `a`'s vertex alone.
fn walk(g: &Grid, s: usize, lo: &[usize], hi: &[usize]) -> Option<Walk> {
    let (m, n) = (g.a.count(), g.b.count());
    // The rows' column ranges, kept to the columns a walk from this start can reach.
    let ranges: Vec<(usize, usize)> = (0..=m).map(|i| (lo.get(i).copied().unwrap_or(0).max(s), hi.get(i).copied().unwrap_or(0).min(s + n))).collect();
    let width = |r: &(usize, usize)| if r.1 >= r.0 { r.1 - r.0 + 1 } else { 0 };
    let mut offsets = Vec::with_capacity(m + 2);
    let mut total = 0usize;
    for r in &ranges {
        offsets.push(total);
        total += width(r);
    }
    // How each node was reached: 0 a pair, 1 `a` alone, 2 `b` alone, 3 the start.
    let mut back = vec![3u8; total];
    let mut prev: Vec<f64> = Vec::new();
    let mut prev_range = (1usize, 0usize);
    for (i, &(l, h)) in ranges.iter().enumerate() {
        let mut cur = vec![f64::INFINITY; width(&(l, h))];
        let off = *offsets.get(i)?;
        for j in l..=h {
            let (mut best, mut how) = (if i == 0 && j == s { 0.0 } else { f64::INFINITY }, 3u8);
            if i > 0 {
                let within = |c: usize| c >= prev_range.0 && c <= prev_range.1;
                if j >= 1
                    && within(j - 1)
                    && let Some(&p) = prev.get(j - 1 - prev_range.0)
                {
                    let c = p + g.pair(i - 1, (j - 1) % n);
                    if c < best {
                        (best, how) = (c, 0);
                    }
                }
                if within(j)
                    && let Some(&p) = prev.get(j - prev_range.0)
                {
                    let c = p + g.alone_a(i - 1, (j + n - 1) % n);
                    if c < best {
                        (best, how) = (c, 1);
                    }
                }
            }
            if j > l
                && let Some(&p) = cur.get(j - 1 - l)
            {
                let c = p + g.alone_b((j - 1) % n, (i + m - 1) % m);
                if c < best {
                    (best, how) = (c, 2);
                }
            }
            if let Some(x) = cur.get_mut(j - l) {
                *x = best;
            }
            if let Some(x) = back.get_mut(off + j - l) {
                *x = how;
            }
        }
        prev = cur;
        prev_range = (l, h);
    }
    let end = s + n;
    if end < prev_range.0 || end > prev_range.1 {
        return None;
    }
    let cost = *prev.get(end - prev_range.0)?;
    if !cost.is_finite() {
        return None;
    }
    let mut steps = Vec::with_capacity(m + n);
    let (mut i, mut j) = (m, end);
    while (i, j) != (0, s) {
        let (l, _) = *ranges.get(i)?;
        let how = *back.get(offsets.get(i)? + j.checked_sub(l)?)?;
        match how {
            0 => {
                steps.push(Step::Pair);
                (i, j) = (i.checked_sub(1)?, j.checked_sub(1)?);
            }
            1 => {
                steps.push(Step::A);
                i = i.checked_sub(1)?;
            }
            2 => {
                steps.push(Step::B);
                j = j.checked_sub(1)?;
            }
            _ => return None,
        }
    }
    steps.reverse();
    Some(Walk { start: s, cost, steps })
}

/// Whether `w` is better than `best`: cheaper beyond rounding, or as cheap from an earlier start.
fn better(w: &Walk, best: &Walk) -> bool {
    let tol = 1e-12 * best.cost.abs().max(1.0);
    w.cost < best.cost - tol || (w.cost <= best.cost + tol && w.start < best.start)
}

/// The least-cost walk over every start. The best walks from two starts can be chosen not to
/// cross (where they would, the cheaper of the two pieces between serves both), so a start's
/// walk is searched only between the walks of the starts either side of it, halving the range
/// of starts each time: about m·n·log2(n) steps, not m·n².
fn best_walk(g: &Grid) -> Result<Walk> {
    let (m, n) = (g.a.count(), g.b.count());
    let none = || KernelError::Failed("loft sections could not be paired".into());
    let first = walk(g, 0, &vec![0; m + 1], &vec![n; m + 1]).ok_or_else(none)?;
    let (lo, hi) = first.bounds(m);
    // The walk from start n is the first one moved on by a turn.
    let hi_n: Vec<usize> = hi.iter().map(|c| c + n).collect();
    let mut best = first;
    between(g, (0, &lo), (n, &hi_n), &mut best, 0)?;
    Ok(best)
}

/// Every start strictly between `low.0` and `high.0`: the walks searched to the right of the
/// columns `low.1` (the leftmost the walk from `low.0` visits on each row) and to the left of
/// `high.1` (the rightmost of the walk from `high.0`).
fn between(g: &Grid, low: (usize, &[usize]), high: (usize, &[usize]), best: &mut Walk, depth: usize) -> Result<()> {
    if high.0 <= low.0 + 1 {
        return Ok(());
    }
    if depth > MAX_DEPTH {
        return Err(KernelError::Failed("loft section pairing went too deep".into()));
    }
    let m = g.a.count();
    let mid = low.0 + (high.0 - low.0) / 2;
    let Some(w) = walk(g, mid, low.1, high.1) else {
        return Err(KernelError::Failed("loft sections could not be paired".into()));
    };
    if better(&w, best) {
        *best = w.clone();
    }
    let (lo, hi) = w.bounds(m);
    between(g, low, (mid, &hi), best, depth + 1)?;
    between(g, (mid, &lo), high, best, depth + 1)
}

/// What an event of the correspondence is on one of the loops: one of its vertices, or a point
/// inside one of its segments.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    Vertex(usize),
    On(usize, f64),
}

/// The correspondence between loop `a` (in frame `fa`) and loop `b` (in frame `fb`): both wound
/// counter-clockwise in the shared frame. Between two pairs, the lone vertices of each loop land
/// on the other at the same share of the length between the pairs; with no pair at all, each
/// lands on the nearest point of the segment its walk puts it on.
pub(crate) fn correspond(a: &NumberedLoop, fa: Frame, b: &NumberedLoop, fb: Frame) -> Result<Correspondence> {
    let g = Grid { a: Side::new(&a.lp.segs, fa)?, b: Side::new(&b.lp.segs, fb)? };
    let (m, n) = (g.a.count(), g.b.count());
    if (m + 1).saturating_mul(n + 1) > MAX_CELLS {
        return Err(KernelError::Invalid(format!("loft sections of {m} and {n} segments are too many to pair")));
    }
    let w = best_walk(&g)?;
    // The events round both loops, in order, from the one at `a`'s vertex 0.
    let mut events: Vec<(Place, Place)> = Vec::with_capacity(m + n);
    let (mut i, mut j) = (0usize, w.start);
    for s in &w.steps {
        match s {
            Step::Pair => {
                events.push((Place::Vertex(i), Place::Vertex(j % n)));
                (i, j) = (i + 1, j + 1);
            }
            Step::A => {
                let e = (j + n - 1) % n;
                let t = g.b.land(e, *g.a.at.get(i).ok_or_else(bad)?).ok_or_else(bad)?.0;
                events.push((Place::Vertex(i), Place::On(e, t)));
                i += 1;
            }
            Step::B => {
                let e = (i + m - 1) % m;
                let t = g.a.land(e, *g.b.at.get(j % n).ok_or_else(bad)?).ok_or_else(bad)?.0;
                events.push((Place::On(e, t), Place::Vertex(j % n)));
                j += 1;
            }
        }
    }
    let first = events.iter().position(|(p, _)| *p == Place::Vertex(0)).ok_or_else(bad)?;
    events.rotate_left(first);
    let pairs: Vec<usize> = (0..events.len()).filter(|&k| matches!(events.get(k), Some((Place::Vertex(_), Place::Vertex(_))))).collect();
    if pairs.is_empty() {
        keep_in_order(&mut events);
    } else {
        events = by_share(&g, &events, &pairs)?;
        let first = events.iter().position(|(p, _)| *p == Place::Vertex(0)).ok_or_else(bad)?;
        events.rotate_left(first);
    }
    let mut a_cuts = vec![Vec::new(); m];
    let mut b_cuts = vec![Vec::new(); n];
    for (pa, pb) in &events {
        if let Place::On(e, t) = *pa {
            a_cuts.get_mut(e).ok_or_else(bad)?.push(t);
        }
        if let Place::On(e, t) = *pb {
            b_cuts.get_mut(e).ok_or_else(bad)?.push(t);
        }
    }
    for c in a_cuts.iter_mut().chain(b_cuts.iter_mut()) {
        c.sort_by(f64::total_cmp);
        if c.windows(2).any(|w| matches!(w, [x, y] if y <= x)) || c.iter().any(|t| !(*t > 0.0 && *t < 1.0)) {
            return Err(bad());
        }
    }
    // Where the event at `a`'s vertex 0 is on `b`, once `b` is cut.
    let before = |e: usize| e + b_cuts.iter().take(e).map(Vec::len).sum::<usize>();
    let b_start = match events.first().map(|(_, pb)| *pb).ok_or_else(bad)? {
        Place::Vertex(k) => before(k),
        Place::On(e, t) => before(e) + 1 + b_cuts.get(e).ok_or_else(bad)?.iter().take_while(|&&c| c < t).count(),
    };
    Ok(Correspondence { a_cuts, b_cuts, b_start })
}

fn bad() -> KernelError {
    KernelError::Failed("loft sections could not be paired".into())
}

/// The events with every lone vertex moved, between each two consecutive pairs (`pairs`, the
/// indices of the paired events), to the point of the other loop at the same share of the length
/// between the pairs. Two lone vertices that land on each other become a pair. The events start
/// at the first pair.
fn by_share(g: &Grid, events: &[(Place, Place)], pairs: &[usize]) -> Result<Vec<(Place, Place)>> {
    let vertex = |p: Place| match p {
        Place::Vertex(k) => Some(k),
        Place::On(..) => None,
    };
    let mut out = Vec::with_capacity(events.len());
    for (q, &k0) in pairs.iter().enumerate() {
        let k1 = *pairs.get((q + 1) % pairs.len()).ok_or_else(bad)?;
        let &(pa0, pb0) = events.get(k0).ok_or_else(bad)?;
        let &(pa1, pb1) = events.get(k1).ok_or_else(bad)?;
        let (i0, j0, i1, j1) =
            (vertex(pa0).ok_or_else(bad)?, vertex(pb0).ok_or_else(bad)?, vertex(pa1).ok_or_else(bad)?, vertex(pb1).ok_or_else(bad)?);
        out.push((pa0, pb0));
        // The lone vertices between, each with its share of its own loop's length between.
        let shares = |run: Vec<(usize, f64)>| -> Option<(f64, Vec<(usize, f64)>)> {
            let total = run.last()?.1;
            let lone = run.iter().take(run.len().saturating_sub(1)).map(|&(k, d)| (k, d / total)).collect();
            Some((total, lone))
        };
        let (la, lone_a) = shares(g.a.run(i0, i1)).ok_or_else(bad)?;
        let (lb, lone_b) = shares(g.b.run(j0, j1)).ok_or_else(bad)?;
        let mut on_a = Cursor { side: &g.a, seg: i0, end: i1, before: 0.0 };
        let mut on_b = Cursor { side: &g.b, seg: j0, end: j1, before: 0.0 };
        let (mut x, mut y) = (0usize, 0usize);
        while x < lone_a.len() || y < lone_b.len() {
            let (ka, sa) = lone_a.get(x).copied().unwrap_or((0, f64::INFINITY));
            let (kb, sb) = lone_b.get(y).copied().unwrap_or((0, f64::INFINITY));
            if (sa - sb).abs() <= SAME_SHARE {
                out.push((Place::Vertex(ka), Place::Vertex(kb)));
                (x, y) = (x + 1, y + 1);
            } else if sa < sb {
                let (e, t) = on_b.to(sa * lb).ok_or_else(bad)?;
                out.push((Place::Vertex(ka), Place::On(e, t)));
                x += 1;
            } else {
                let (e, t) = on_a.to(sb * la).ok_or_else(bad)?;
                out.push((Place::On(e, t), Place::Vertex(kb)));
                y += 1;
            }
        }
    }
    Ok(out)
}

/// The landings of a walk with no pair (each lone vertex on the nearest point of the segment the
/// walk puts it on) kept in their order along each segment and off its ends: two vertices can
/// have the same nearest point. The events start at `a`'s vertex 0; along the segment of `b` that
/// point lies inside, the landings the events end with come first.
fn keep_in_order(events: &mut [(Place, Place)]) {
    let count = events.len();
    let lead = events.iter().take_while(|e| matches!(e.1, Place::On(..))).count();
    for side in [0usize, 1] {
        let get = |e: &(Place, Place)| if side == 0 { e.0 } else { e.1 };
        let order: Vec<usize> = if side == 0 { (0..count).collect() } else { (lead..count).chain(0..lead).collect() };
        let mut runs: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for k in order {
            if let Some(Place::On(s, _)) = events.get(k).map(get) {
                runs.entry(s).or_default().push(k);
            }
        }
        for (s, at) in runs {
            let ts: Vec<f64> = at.iter().filter_map(|&k| if let Some(Place::On(_, t)) = events.get(k).map(get) { Some(t) } else { None }).collect();
            for (&k, t) in at.iter().zip(increasing(&ts)) {
                if let Some(e) = events.get_mut(k) {
                    let p = if side == 0 { &mut e.0 } else { &mut e.1 };
                    *p = Place::On(s, t);
                }
            }
        }
    }
}

/// `ts` made strictly increasing inside (0, 1), each moved as little as the order allows.
fn increasing(ts: &[f64]) -> Vec<f64> {
    let k = ts.len();
    let gap = (1e-3f64).min(0.5 / (k as f64 + 1.0));
    let mut out: Vec<f64> = Vec::with_capacity(k);
    for (q, &t) in ts.iter().enumerate() {
        let floor = out.last().map_or(gap, |p| p + gap);
        let ceil = 1.0 - gap * (k - q) as f64;
        out.push(t.max(floor).min(ceil));
    }
    for q in (0..k.saturating_sub(1)).rev() {
        let Some(&next) = out.get(q + 1) else { continue };
        if let Some(x) = out.get_mut(q) {
            *x = x.min(next - gap);
        }
    }
    let ok = out.iter().all(|t| *t > 0.0 && *t < 1.0) && out.windows(2).all(|w| matches!(w, [a, b] if b > a));
    if ok { out } else { (1..=k).map(|q| q as f64 / (k as f64 + 1.0)).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The next number of a fixed sequence, in [0, 1).
    fn next(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A counter-clockwise polygon of `n` vertices round `centre`, each at its own angle and
    /// distance (so the polygon is star-shaped but neither convex nor regular).
    fn polygon(seed: &mut u64, n: usize, centre: Vec2) -> Loop2 {
        let points: Vec<Vec2> = (0..n)
            .map(|k| {
                let angle = (k as f64 + 0.8 * next(seed)) * std::f64::consts::TAU / n as f64;
                centre + Vec2::from_angle(angle) * (5.0 + 10.0 * next(seed))
            })
            .collect();
        Loop2::polygon(&points)
    }

    /// The search between neighbouring starts' walks finds a walk as cheap as trying every start
    /// over the whole grid.
    #[test]
    fn the_search_over_starts_is_as_good_as_trying_every_start() {
        let mut seed = 11u64;
        for _ in 0..80 {
            let m = 3 + (next(&mut seed) * 10.0) as usize;
            let n = 3 + (next(&mut seed) * 10.0) as usize;
            let a = polygon(&mut seed, m, Vec2::ZERO);
            let offset = Vec2::new(next(&mut seed) * 4.0, next(&mut seed) * 4.0);
            let b = polygon(&mut seed, n, offset);
            fn side(l: &Loop2) -> Side<'_> {
                Side::new(&l.segs, Frame::new(Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), l)).unwrap()
            }
            let g = Grid { a: side(&a), b: side(&b) };
            let fast = best_walk(&g).unwrap();
            let slow = (0..n).filter_map(|s| walk(&g, s, &vec![0; m + 1], &vec![2 * n; m + 1])).map(|w| w.cost).fold(f64::INFINITY, f64::min);
            assert!((fast.cost - slow).abs() <= 1e-9 * slow.max(1.0), "{m} by {n}: {} against {slow}", fast.cost);
            // Its steps use every vertex of both loops once.
            let count = |s: Step| fast.steps.iter().filter(|x| **x == s).count();
            assert_eq!(count(Step::Pair) + count(Step::A), m);
            assert_eq!(count(Step::Pair) + count(Step::B), n);
        }
    }
}
