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

    /// Re-split into exactly `n` segments, other loops split their longest segments in half. A
    /// full circle is split once round (one turn, every arc a positive sweep) to follow a lead
    /// loop: at the lead's vertex directions `angles` when they go round in order (counter-
    /// clockwise, or clockwise when `clockwise`), otherwise from the first of them at the lead's
    /// perimeter `fractions` (the share of the lead's length before each vertex; evenly when
    /// those are unusable).
    pub(crate) fn resample(&self, n: usize, angles: &[f64], fractions: &[f64], clockwise: bool) -> NumberedLoop {
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
        let mut out = self.clone();
        while out.lp.segs.len() < n {
            let Some((k, _)) = out.lp.segs.iter().enumerate().max_by(|a, b| a.1.length().total_cmp(&b.1.length())) else { break };
            out.split_half(k);
        }
        out
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
