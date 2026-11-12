//! Profile loops whose segments carry the profile segments they come from, through every way the
//! builders reshape a loop before building (turning it round, re-seaming a circle, matching it to
//! another section). [`FaceOrigin::Profile`](crate::history::FaceOrigin::Profile)
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

    /// The loop as matched for a loft (`solidcore_ops::sweep::match_loft_sections`): `segs`, each
    /// over the numbers of the segments of this loop `from` names.
    pub(crate) fn matched(&self, segs: Vec<Seg2>, from: &[Vec<usize>]) -> NumberedLoop {
        let from = from
            .iter()
            .map(|ix| if let [i] = ix.as_slice() { self.numbers(*i).to_vec() } else { union(ix.iter().map(|i| self.numbers(*i))) })
            .collect();
        NumberedLoop { lp: Loop2 { segs }, from }
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

/// Every number of `sets`, once each, in order.
fn union<'a>(sets: impl IntoIterator<Item = &'a [usize]>) -> Vec<usize> {
    sets.into_iter().flatten().copied().collect::<BTreeSet<usize>>().into_iter().collect()
}
