//! Profile loops whose segments carry the profile segments they come from, through every way the
//! builders reshape a loop before building (turning it round, re-seaming a circle, re-splitting
//! it to match another section). [`FaceOrigin::Profile`](crate::history::FaceOrigin::Profile)
//! numbers are defined here: the segments of the caller's regions in order, each region its outer
//! loop and then its holes ([`profile_loops`]).

use solidcraft_geom::prelude::*;
use std::collections::BTreeSet;

use solidcore_record::{Loop2, Region2, Seg2};

/// The loops of `regions` in profile order: each region's outer loop, then its holes.
pub(crate) fn profile_loops(regions: &[Region2]) -> impl Iterator<Item = &Loop2> {
    regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
}

/// A loop with, for each of its segments, the profile segments it runs over (one, until the loop
/// is re-split; a segment split in two leaves both pieces over it).
#[derive(Clone, Debug)]
pub(crate) struct NumberedLoop {
    pub lp: Loop2,
    pub from: Vec<Vec<usize>>,
}

/// A region of numbered loops (outer, then holes).
#[derive(Clone, Debug)]
pub(crate) struct NumberedRegion {
    pub outer: NumberedLoop,
    pub holes: Vec<NumberedLoop>,
}

/// `loops` numbered one after the other from 0, in the order given.
pub(crate) fn number_loops<'a>(loops: impl IntoIterator<Item = &'a Loop2>) -> Vec<NumberedLoop> {
    let mut base = 0;
    loops
        .into_iter()
        .map(|lp| {
            let numbered = NumberedLoop { lp: lp.clone(), from: (base..base + lp.segs.len()).map(|n| vec![n]).collect() };
            base += lp.segs.len();
            numbered
        })
        .collect()
}

/// `regions` numbered in profile order ([`profile_loops`]), as given (not yet oriented).
pub(crate) fn number_regions(regions: &[Region2]) -> Vec<NumberedRegion> {
    let mut loops = number_loops(profile_loops(regions)).into_iter();
    regions.iter().filter_map(|r| Some(NumberedRegion { outer: loops.next()?, holes: loops.by_ref().take(r.holes.len()).collect() })).collect()
}

impl NumberedLoop {
    /// The profile segments under segment `i`.
    pub(crate) fn numbers(&self, i: usize) -> &[usize] {
        self.from.get(i).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Run the other way round.
    pub(crate) fn reversed(&self) -> NumberedLoop {
        NumberedLoop { lp: self.lp.reversed(), from: self.from.iter().rev().cloned().collect() }
    }

    /// Counter-clockwise (as [`Loop2::ccw`]); clockwise when `hole`. Every builder orients its
    /// loops this way, through [`NumberedRegion::oriented`] or this.
    pub(crate) fn oriented(&self, hole: bool) -> NumberedLoop {
        let ccw = if self.lp.is_ccw() { self.clone() } else { self.reversed() };
        if hole { ccw.reversed() } else { ccw }
    }

    /// Each segment replaced by `f` of it (the same segment, moved).
    pub(crate) fn map_segs(&self, f: impl Fn(&Seg2) -> Seg2) -> NumberedLoop {
        NumberedLoop { lp: Loop2 { segs: self.lp.segs.iter().map(f).collect() }, from: self.from.clone() }
    }

    /// Start at segment `k`.
    pub(crate) fn rotate_left(&mut self, k: usize) {
        if k < self.lp.segs.len() && self.from.len() == self.lp.segs.len() {
            self.lp.segs.rotate_left(k);
            self.from.rotate_left(k);
        }
    }

    /// This loop (a full circle) re-split as `pieces` (arcs of the same circle): each piece runs
    /// over every arc of this loop it overlaps.
    pub(crate) fn resplit_circle(&self, pieces: Loop2) -> NumberedLoop {
        let from = arc_overlaps(&pieces.segs, &self.lp.segs).into_iter().map(|js| union(js.iter().map(|j| self.numbers(*j)))).collect();
        NumberedLoop { lp: pieces, from }
    }

    /// Re-split into exactly `n` segments to follow a loop of `n` vertices whose directions from
    /// its centre, seen in this loop's plane, are `angles`. A full circle is split once round (one
    /// turn, every arc a positive sweep): at `angles` when they go round in order (counter-
    /// clockwise, or clockwise when `clockwise`), otherwise from the first of them at the lead's
    /// perimeter `fractions` (the share of the lead's length before each vertex; evenly when
    /// those are unusable). Any other loop keeps its vertices, each paired with the followed
    /// vertex it lines up with ([`NumberedLoop::matched`]); the followed vertices left over split
    /// its segments, at their shares of the followed loop's perimeter `shares` (as `fractions`,
    /// for the loop followed). When that matching cannot be made, the longest segments are split
    /// in half.
    pub(crate) fn resample(&self, n: usize, angles: &[f64], fractions: &[f64], shares: &[f64], clockwise: bool) -> NumberedLoop {
        let lp = &self.lp;
        let circle = lp.segs.iter().all(|s| matches!(s, Seg2::Arc { .. }))
            && lp.segs.windows(2).all(|w| match (w.first(), w.get(1)) {
                (Some(Seg2::Arc { center: c1, radius: r1, .. }), Some(Seg2::Arc { center: c2, radius: r2, .. })) => {
                    c1.dist(*c2) < 1e-9 && (r1 - r2).abs() < 1e-9
                }
                _ => false,
            });
        if circle
            && n > 0
            && let Some(Seg2::Arc { center, radius, .. }) = lp.segs.first().copied()
        {
            let a = circle_breaks(n, angles, fractions, clockwise);
            let segs: Vec<Seg2> = (0..a.len())
                .filter_map(|i| {
                    let s = *a.get(i)?;
                    let e = if i + 1 < a.len() { *a.get(i + 1)? } else { a.first()? + std::f64::consts::TAU };
                    Some(Seg2::Arc { center, radius, start: s, sweep: e - s })
                })
                .collect();
            return self.resplit_circle(Loop2 { segs });
        }
        if let Some(out) = self.matched(n, angles, shares) {
            return out;
        }
        let mut out = self.clone();
        while out.lp.segs.len() < n {
            let Some((k, _)) = out.lp.segs.iter().enumerate().max_by(|a, b| a.1.length().total_cmp(&b.1.length())) else { break };
            out.split_half(k);
        }
        out
    }

    /// Re-split into `n` segments by pairing each of this loop's vertices with one of the `n`
    /// vertices of the loop it follows (their directions from its centre `angles`, its perimeter
    /// shares `shares`), in order round both loops, so that the directions from the centres agree
    /// best (the least sum of `1 - cos` of the angles between them); the followed vertices left
    /// between two paired ones split the segment between those, at their shares of the followed
    /// loop's length between them. Splitting the longest segments in half instead paired a
    /// symmetric loop asymmetrically when its longest segment was not on the axis (a slot lofted
    /// to a pointed shape: its long side split, not its round end, twisting the loft, #11849).
    /// None when there is nothing to add, the inputs are unusable or the matching is too big.
    fn matched(&self, n: usize, angles: &[f64], shares: &[f64]) -> Option<NumberedLoop> {
        let m = self.lp.segs.len();
        // The pairing tries every start and fills an m by n table for each.
        const BUDGET: usize = 4_000_000;
        if m == 0 || m >= n || angles.len() != n || shares.len() != n || self.from.len() != m || n.saturating_mul(n).saturating_mul(m) > BUDGET {
            return None;
        }
        if !angles.iter().chain(shares).all(|a| a.is_finite()) || shares.windows(2).any(|w| matches!(w, [a, b] if b <= a)) {
            return None;
        }
        let c = self.lp.centroid();
        let own: Vec<f64> = self.lp.segs.iter().map(|s| (s.start() - c).angle()).collect();
        if !own.iter().all(|a| a.is_finite()) {
            return None;
        }
        let cost = |i: usize, j: usize| own.get(i).zip(angles.get(j)).map_or(f64::INFINITY, |(a, b)| 1.0 - (a - b).cos());
        // For a start `s` (vertex 0 paired with followed vertex s), the least cost of pairing
        // vertices 0..=i with i at position p (followed vertex (s + p) mod n, 0 < p < n for i > 0),
        // and where vertex i - 1 was for it.
        let table = |s: usize, keep: bool| -> (Vec<f64>, Vec<Vec<usize>>) {
            let mut best = vec![f64::INFINITY; n];
            if let Some(b) = best.first_mut() {
                *b = cost(0, s);
            }
            let mut back: Vec<Vec<usize>> = Vec::new();
            for i in 1..m {
                let mut next = vec![f64::INFINITY; n];
                let mut came = vec![0usize; if keep { n } else { 0 }];
                // The best of the positions before p.
                let (mut low, mut at) = (f64::INFINITY, 0usize);
                for p in 1..n {
                    if let Some(&b) = best.get(p - 1)
                        && b < low
                    {
                        (low, at) = (b, p - 1);
                    }
                    if let Some(x) = next.get_mut(p) {
                        *x = low + cost(i, (s + p) % n);
                    }
                    if let Some(x) = came.get_mut(p) {
                        *x = at;
                    }
                }
                best = next;
                if keep {
                    back.push(came);
                }
            }
            (best, back)
        };
        let least = |row: &[f64]| row.iter().copied().enumerate().fold((0usize, f64::INFINITY), |(k, b), (p, x)| if x < b { (p, x) } else { (k, b) });
        // The first start that pairs best (a later one must be better by more than rounding).
        let mut start: Option<(usize, f64)> = None;
        for s in 0..n {
            let (_, total) = least(&table(s, false).0);
            if total.is_finite() && start.is_none_or(|(_, b)| total < b - 1e-12) {
                start = Some((s, total));
            }
        }
        let (s, _) = start?;
        let (row, back) = table(s, true);
        // Walk back from the best last position (followed positions in increasing order).
        let mut positions = vec![0usize; m];
        let mut p = least(&row).0;
        for i in (0..m).rev() {
            *positions.get_mut(i)? = p;
            if i > 0 {
                p = *back.get(i - 1)?.get(p)?;
            }
        }
        if positions.windows(2).any(|w| matches!(w, [a, b] if b <= a)) {
            return None;
        }
        // A position's share of the followed perimeter, unwrapped past the end.
        let share = |p: usize| shares.get((s + p) % n).map(|f| f + ((s + p) / n) as f64);
        let mut out = NumberedLoop { lp: Loop2 { segs: Vec::with_capacity(n) }, from: Vec::with_capacity(n) };
        for i in 0..m {
            let (a, b) = (*positions.get(i)?, positions.get(i + 1).copied().unwrap_or(n));
            let (fa, fb) = (share(a)?, share(b)?);
            let from = self.from.get(i)?;
            let mut rest = *self.lp.segs.get(i)?;
            // Where the remaining piece starts, as a share of this segment.
            let mut done = 0.0;
            for q in a + 1..b {
                let t = (share(q)? - fa) / (fb - fa);
                // Even shares when the followed loop's are unusable here.
                let t = if t.is_finite() && t > done && t < 1.0 { t } else { done + (1.0 - done) / (b - q + 1) as f64 };
                let (piece, after) = rest.split_at((t - done) / (1.0 - done));
                out.lp.segs.push(piece);
                out.from.push(from.clone());
                rest = after;
                done = t;
            }
            out.lp.segs.push(rest);
            out.from.push(from.clone());
        }
        // Start at the piece paired with the followed loop's first vertex.
        out.rotate_left((n - s) % n);
        (out.lp.segs.len() == n).then_some(out)
    }

    /// Each run of lines that `joins` says continue one another as a single line, over what
    /// they all ran over. (Adjacent profiles merged into one region keep a vertex where each
    /// shared edge met the outline.)
    pub(crate) fn merged_lines(&self, joins: impl Fn(&Seg2, &Seg2) -> bool) -> NumberedLoop {
        let n = self.lp.segs.len();
        let join = |i: usize, j: usize| match (self.lp.segs.get(i), self.lp.segs.get(j)) {
            (Some(s @ Seg2::Line { .. }), Some(t @ Seg2::Line { .. })) => joins(s, t),
            _ => false,
        };
        // Start where a run starts (a loop of nothing but one run stays as it is).
        let Some(k) = (0..n).find(|&i| !join((i + n - 1) % n, i)) else { return self.clone() };
        let mut out = NumberedLoop { lp: Loop2 { segs: Vec::new() }, from: Vec::new() };
        for j in 0..n {
            let i = (k + j) % n;
            let (Some(s), from) = (self.lp.segs.get(i).copied(), self.numbers(i)) else { continue };
            if j > 0
                && join((i + n - 1) % n, i)
                && let (Some(Seg2::Line { b, .. }), Some(prev)) = (out.lp.segs.last_mut(), out.from.last_mut())
            {
                *b = s.end();
                *prev = union([prev.as_slice(), from]);
                continue;
            }
            out.lp.segs.push(s);
            out.from.push(from.to_vec());
        }
        out
    }

    /// The same loop started at a segment `joins` does not join to the one before it, so no run of
    /// joined segments wraps round the loop's end (as it is when every segment joins the next).
    pub(crate) fn starting_a_run(&self, joins: impl Fn(&Seg2, &Seg2) -> bool) -> NumberedLoop {
        let segs = &self.lp.segs;
        let n = segs.len();
        let joined = |i: usize| matches!((segs.get((i + n - 1) % n), segs.get(i)), (Some(s), Some(t)) if joins(s, t));
        let mut out = self.clone();
        if let Some(k) = (0..n).find(|&i| !joined(i)) {
            out.rotate_left(k);
        }
        out
    }

    /// Split segment `k` in half, both halves over what it ran over.
    fn split_half(&mut self, k: usize) {
        if k >= self.lp.segs.len() || k >= self.from.len() {
            return;
        }
        let s = self.lp.segs.remove(k);
        let (p, q) = s.split_half();
        self.lp.segs.insert(k, q);
        self.lp.segs.insert(k, p);
        let from = self.from.get(k).cloned().unwrap_or_default();
        self.from.insert(k, from);
    }
}

impl NumberedRegion {
    /// The region as the builders build it: outer loop counter-clockwise, holes clockwise.
    pub(crate) fn oriented(&self) -> NumberedRegion {
        NumberedRegion { outer: self.outer.oriented(false), holes: self.holes.iter().map(|h| h.oriented(true)).collect() }
    }

    /// Each loop replaced by `f` of it.
    pub(crate) fn map_loops(&self, f: impl Fn(&NumberedLoop) -> NumberedLoop) -> NumberedRegion {
        NumberedRegion { outer: f(&self.outer), holes: self.holes.iter().map(f).collect() }
    }

    /// The outer loop, then the holes.
    pub(crate) fn loops(&self) -> impl Iterator<Item = &NumberedLoop> {
        std::iter::once(&self.outer).chain(&self.holes)
    }

    /// The region's geometry.
    pub(crate) fn region(&self) -> Region2 {
        Region2 { outer: self.outer.lp.clone(), holes: self.holes.iter().map(|h| h.lp.clone()).collect() }
    }
}

/// The smallest arc a circle is split into (radians).
const MIN_SWEEP: f64 = 1e-9;

/// `n` increasing break angles of one turn round a circle (each step and the closing step back
/// to the first, one turn on, positive) for a circle following a lead loop: `angles` (the lead's
/// vertex directions) when they go round once in order in the lead's direction (counter-clockwise,
/// or clockwise when `clockwise`), otherwise from the first of them at the lead's perimeter
/// `fractions`, or evenly. A non-star-shaped lead's directions turn back on themselves: forcing
/// them to increase wound the circle round many times (#2570).
fn circle_breaks(n: usize, angles: &[f64], fractions: &[f64], clockwise: bool) -> Vec<f64> {
    use std::f64::consts::TAU;
    let sign = if clockwise { -1.0 } else { 1.0 };
    // Angles measured in the lead's direction of travel.
    let along: Vec<f64> = angles.iter().map(|a| sign * a).collect();
    let run = (along.len() == n).then(|| one_turn(&along)).flatten().unwrap_or_else(|| {
        let start = along.first().copied().filter(|a| a.is_finite()).unwrap_or(0.0);
        let even = || (0..n).map(|i| i as f64 / n as f64).collect::<Vec<f64>>();
        let f = if usable_fractions(n, fractions) { fractions.to_vec() } else { even() };
        f.iter().map(|f| start + TAU * f).collect()
    });
    let mut breaks: Vec<f64> = run.into_iter().map(|a| sign * a).collect();
    if clockwise {
        breaks.reverse();
    }
    breaks
}

/// `along` unwrapped into one increasing turn (each step the shortest positive one), or `None`
/// when they do not go round once in order.
fn one_turn(along: &[f64]) -> Option<Vec<f64>> {
    let mut out: Vec<f64> = Vec::with_capacity(along.len());
    for &x in along {
        if !x.is_finite() {
            return None;
        }
        let next = match out.last() {
            Some(&prev) => {
                let step = (x - prev).rem_euclid(std::f64::consts::TAU);
                if step <= MIN_SWEEP {
                    return None;
                }
                prev + step
            }
            None => x,
        };
        out.push(next);
    }
    let (first, last) = (*out.first()?, *out.last()?);
    (last - first < std::f64::consts::TAU - MIN_SWEEP).then_some(out)
}

/// `fractions` are `n` finite, strictly increasing shares of a turn from 0, all under one.
fn usable_fractions(n: usize, fractions: &[f64]) -> bool {
    fractions.len() == n
        && fractions.first().is_some_and(|f| f.abs() < MIN_SWEEP)
        && fractions.windows(2).all(|w| matches!(w, [a, b] if b - a > MIN_SWEEP))
        && fractions.last().is_some_and(|f| f.is_finite() && *f < 1.0 - MIN_SWEEP)
}

/// Every number of `sets`, once each, in order.
fn union<'a>(sets: impl IntoIterator<Item = &'a [usize]>) -> Vec<usize> {
    sets.into_iter().flatten().copied().collect::<BTreeSet<usize>>().into_iter().collect()
}

/// Which arcs of `originals` (arcs of one circle) each arc of `pieces` (arcs of the same
/// circle) runs over: a circle re-split at other angles comes from every original arc it
/// overlaps.
fn arc_overlaps(pieces: &[Seg2], originals: &[Seg2]) -> Vec<Vec<usize>> {
    use std::f64::consts::TAU;
    // An arc as its counter-clockwise start angle and (positive) sweep.
    let span = |s: &Seg2| match *s {
        Seg2::Arc { start, sweep, .. } => Some(if sweep < 0.0 { (start + sweep, -sweep) } else { (start, sweep) }),
        _ => None,
    };
    let eps = 1e-9;
    pieces
        .iter()
        .map(|p| {
            let Some((p0, pw)) = span(p) else { return Vec::new() };
            (0..originals.len())
                .filter(|&j| {
                    originals.get(j).and_then(span).is_some_and(|(o0, ow)| {
                        // The original's start, seen from the piece's start going round.
                        let d = (o0 - p0).rem_euclid(TAU);
                        d < pw - eps || d + ow > TAU + eps || ow >= TAU - eps
                    })
                })
                .collect()
        })
        .collect()
}
