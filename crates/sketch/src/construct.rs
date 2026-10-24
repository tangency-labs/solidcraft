//! Constructive seed for the solver.
//!
//! Before Levenberg–Marquardt runs, every point the constraints pin down relative to the given
//! points is placed exactly (Owen 1991, §3 and §5; Fudos and Hoffmann 1997), in two
//! interleaved steps repeated until neither places anything:
//!
//! - **propagation**: a point with two loci anchored on already placed points goes to their
//!   intersection;
//! - **decomposition** ([`crate::decompose`]): points no point-by-point order reaches but that
//!   are rigid relative to the placed ones (Owen's articulation-pair reduction) are placed by
//!   cluster merging, with the placed points as the ground.
//!
//! Then a rigid cluster that is free or pinned at a single placed point (nothing decides its
//! pose, and LM alone may land it mirrored) is placed by its reference pose
//! ([`Decomposition::floats`]). That is a guess at free motion ([`Placement::Float`]): the
//! solver starts from it, nothing is built on it.
//!
//! Of two mirror-image intersections, the one keeping the orientation the points had before
//! the solve is kept, so a dimension edit keeps each triangle's orientation, each arc on its
//! side of its tangent lines and each line's direction instead of letting the iteration fall
//! into a mirrored solution.
//!
//! The seed gives the iteration its start. Constructed values the constraints pin down one
//! after another, and whose constraints hold there, stay constants while LM solves the rest
//! (`solver::pinned`); every constraint is still verified, and the rank analysis of the solved
//! Jacobian still decides the degrees of freedom. A point with only one locus on placed points keeps its own position (projecting
//! it onto that locus would freeze a point that still has a degree of freedom). What neither
//! step places (triconnected remainders, under-constrained freedom, constraints read by
//! neither) is left to LM.
//!
//! What the constraints say about a point, and where two of them put it, is read by
//! [`crate::loci`] (the same reading drives the decomposition): coincident points; circles
//! (distances, line lengths, arc radii, points on circles of known radius, tangent circles);
//! rows and columns (horizontal and vertical lines and points, and offsets from them on the
//! side the point was on); lines through placed points along a known direction (a point on a
//! line, a line at a known angle, parallel or perpendicular to a placed line); lines at a
//! distance from a placed line (a point at a distance, a tangent circle's centre), on the side
//! the point was on; the radius to a touch point, normal to the line; midpoints and mirrored
//! points. A circle's radius set by its dimensions is seeded too.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use solidcraft_geom::Vec2;

use crate::decompose::{Budget, Decomposition, enqueue};
use crate::loci::{MAX_LOCI, Reading, Rel, View, place, radii, read};
use crate::model::{CurveIdx, PointIdx, Sketch, in_range};

/// Rounds of decomposition then propagation per seed. Each round after the first must follow a
/// propagation that placed something, so on real sketches the loop ends after a few; this
/// bounds a hostile chain that alternates.
const MAX_ROUNDS: usize = 16;

/// Start positions for the solver: `pos[i]` is the position of point `i` when it is given or
/// constructed, `placed_by[i]` how it was placed; `radius[k]` the radius of circle or arc `k`
/// when its dimensions set it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Seed {
    pub pos: Vec<Option<Vec2>>,
    pub placed_by: Vec<Option<Placement>>,
    pub radius: Vec<Option<f64>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    /// Fixed, fixed by a constraint, a point of a fixed curve, or held for this solve.
    Given,
    /// At the intersection of two loci anchored on the placed points `a` ≤ `b` (the same point
    /// when both loci come from it, or for a coincident point). `transversal` when the loci
    /// cross (the point is pinned down well); false when they touch.
    TwoLoci { a: usize, b: usize, transversal: bool },
    /// By cluster decomposition: rigid relative to the placed points, built from crossing
    /// intersections only.
    Cluster,
    /// A point of a rigid cluster whose pose nothing decides (free, or pinned at a single
    /// placed point), placed by the cluster's reference pose: a guess at free motion, so the
    /// solver starts from it but nothing is built on it.
    Float,
}

impl Placement {
    /// The solver starts from it. A touching placement is recorded but the solver starts the
    /// point where it is.
    pub(crate) fn anchors(self) -> bool {
        !matches!(self, Placement::TwoLoci { transversal: false, .. })
    }

    /// Later points may be built on it, and the solver may keep it (`solver::pinned`): it is
    /// pinned down well and not a guess.
    pub(crate) fn builds(self) -> bool {
        self.anchors() && self != Placement::Float
    }
}

/// Place the points the constraints pin down; `given[i]` marks the points that are constants
/// for this solve. Branches are chosen to match the orientation of `reference` (positions
/// before an interactive move, or the stored ones after linked geometry moved) where it covers
/// a point, of the sketch's current positions otherwise; a reference longer than the point list
/// is ignored. `edits`: the solve applies edits; a drag step solves against its reference and
/// moves only held points. `on_curve` holds the sketch's point-on-curve pairs
/// ([`crate::solver::points_on_curves`]); the decomposition works within what is left of
/// `budget`, and charges it.
pub(crate) fn seed(
    sk: &Sketch,
    given: &[bool],
    reference: Option<&[Vec2]>,
    edits: bool,
    on_curve: &[(PointIdx, CurveIdx)],
    budget: &mut Budget,
) -> Seed {
    let n = sk.points.len();
    let reference = reference.filter(|r| r.len() <= n);
    let before = |i: PointIdx| match reference.and_then(|r| r.get(i.0)) {
        Some(p) => Some(*p),
        None => sk.points.get(i).map(|p| p.pos),
    };
    let is_given = |i: usize| given.get(i).copied().unwrap_or(false);
    let pos: Vec<Option<Vec2>> = sk.points.iter().enumerate().map(|(i, p)| (is_given(i) && in_range(p.pos)).then_some(p.pos)).collect();
    let placed_by = pos.iter().map(|p| p.map(|_| Placement::Given)).collect();
    let mut out = Seed { pos, placed_by, radius: radii(sk) };
    let reading = read(sk, &before, on_curve, &out.radius);
    let mut prop = Propagation {
        reading: &reading,
        free: (0..n).map(|i| !is_given(i)).collect(),
        anchors: out.pos.clone(),
        placed_near: vec![0; n],
        queued: vec![false; n],
        queue: VecDeque::new(),
    };
    for i in 0..n {
        if prop.anchors.get(i).is_some_and(Option::is_some) {
            prop.anchored(i);
        }
    }
    // Every free point is examined once, and again when a neighbour placed later can complete
    // a pair of loci (see `Propagation::anchored`); there is nothing to examine when no free
    // point is related to a given one.
    if prop.next_to_free() {
        for i in (0..n).filter(|i| !is_given(*i)) {
            enqueue(&mut prop.queued, &mut prop.queue, i);
        }
    }
    let mut steps = n.saturating_add(reading.tie_count()).saturating_mul(2 * (MAX_LOCI + 1));
    prop.run(&mut out, &mut steps);
    // Where propagation stalls, the decomposition places what is rigid relative to the
    // anchors; propagation goes on from its placements, and the graph takes in what
    // propagation placed.
    if !switch::decompose_on() {
        return out;
    }
    let mut clusters = Decomposition::new(&reading, &prop.anchors, edits, budget.left());
    if !clusters.built() {
        return out;
    }
    for _ in 0..MAX_ROUNDS {
        let candidates: Vec<(usize, Vec2)> = (0..n).filter(|i| prop.is_free(*i)).filter_map(|i| Some((i, clusters.pos(i)?))).collect();
        let accepted = prop.consistent(&candidates);
        if accepted.is_empty() {
            break;
        }
        for (i, at) in accepted {
            prop.place(&mut out, i, at, Placement::Cluster);
        }
        let anchored = prop.run(&mut out, &mut steps);
        if anchored.is_empty() || clusters.exhausted() {
            break;
        }
        clusters.know(&anchored);
    }
    // What the constraints decide is placed. A rigid cluster they leave free to move goes where
    // its reference pose puts it, if that holds every relation to the anchors and no point of
    // it is placed already: a guess, so it anchors nothing and the next is checked against the
    // anchors alone.
    for float in clusters.floats(&|i| prop.anchors.get(i).copied().flatten()) {
        let open = float.iter().all(|(i, _)| out.pos.get(*i).is_some_and(Option::is_none));
        if open && prop.consistent(&float).len() == float.len() {
            for (i, at) in float {
                prop.place(&mut out, i, at, Placement::Float);
            }
        }
    }
    let (work, cut) = clusters.work();
    budget.charge(work, cut);
    out
}

/// Point-by-point placement state, kept across the decomposition rounds.
struct Propagation<'a> {
    reading: &'a Reading,
    /// Not given: may be placed.
    free: Vec<bool>,
    /// Points later points may be built on (see [`Placement::anchors`]).
    anchors: Vec<Option<Vec2>>,
    /// Anchoring neighbours (by a relation between two points) per point.
    placed_near: Vec<usize>,
    queued: Vec<bool>,
    queue: VecDeque<usize>,
}

impl Propagation<'_> {
    fn is_free(&self, i: usize) -> bool {
        self.free.get(i).copied().unwrap_or(false) && self.anchors.get(i).is_some_and(Option::is_none)
    }

    /// Some free point is related to an anchor.
    fn next_to_free(&self) -> bool {
        let anchored = |o: usize| self.anchors.get(o).is_some_and(Option::is_some);
        (0..self.free.len()).any(|i| self.is_free(i) && self.reading.neighbours(i).any(anchored))
    }

    /// The candidate positions of free points, in range, that hold every relation to the
    /// anchors and to the other candidates, as a propagated point must. One that misses is not
    /// placed, nor is any candidate related to it (the decomposition may have built it on the
    /// miss).
    fn consistent(&self, candidates: &[(usize, Vec2)]) -> Vec<(usize, Vec2)> {
        let by_point: BTreeMap<usize, Vec2> = candidates.iter().copied().filter(|(i, at)| self.is_free(*i) && in_range(*at)).collect();
        let at = |i: usize| self.anchors.get(i).copied().flatten().or(by_point.get(&i).copied());
        let view = View { at: &at, oriented: true };
        let mut dropped: BTreeSet<usize> = by_point.iter().filter(|(i, x)| !self.reading.holds(**i, **x, &view)).map(|(i, _)| *i).collect();
        let mut cascade: Vec<usize> = dropped.iter().copied().collect();
        while let Some(i) = cascade.pop() {
            for o in self.reading.neighbours(i) {
                if by_point.contains_key(&o) && dropped.insert(o) {
                    cascade.push(o);
                }
            }
        }
        by_point.into_iter().filter(|(i, _)| !dropped.contains(i)).collect()
    }

    /// Record `p` at `at`; an anchoring placement also anchors it and requeues the neighbours it
    /// may complete (true then). A given point is never placed.
    fn place(&mut self, out: &mut Seed, p: usize, at: Vec2, how: Placement) -> bool {
        if !self.is_free(p) {
            return false;
        }
        if let (Some(slot), Some(by)) = (out.pos.get_mut(p), out.placed_by.get_mut(p)) {
            *slot = Some(at);
            *by = Some(how);
        }
        if how.builds()
            && let Some(slot) = self.anchors.get_mut(p)
        {
            *slot = Some(at);
            self.anchored(p);
            return true;
        }
        false
    }

    /// Count the new anchor `p` at each neighbour, and requeue a free neighbour it can complete a
    /// pair of loci for (up to the MAX_LOCI-th anchor): a coincident neighbour, the second
    /// anchor, the first of a point with ties (they may give its other locus), and the points
    /// of the ties `p` may complete.
    fn anchored(&mut self, p: usize) {
        let Propagation { reading, free, anchors, placed_near, queued, queue } = self;
        let open = |o: usize| free.get(o).copied().unwrap_or(false) && anchors.get(o).is_some_and(Option::is_none);
        for e in reading.adj(p) {
            let o = e.other(p);
            let Some(count) = placed_near.get_mut(o) else { continue };
            *count += 1;
            if *count <= MAX_LOCI && (matches!(e.rel, Rel::Same) || *count >= 2 || reading.tie_count_of(o) > 0) && open(o) {
                enqueue(queued, queue, o);
            }
        }
        for o in reading.woken(p) {
            if open(o) {
                enqueue(queued, queue, o);
            }
        }
    }

    /// Place queued points until the queue or `steps` runs out; the points that became anchors.
    fn run(&mut self, out: &mut Seed, steps: &mut usize) -> Vec<(usize, Vec2)> {
        let mut anchored = Vec::new();
        while *steps > 0
            && let Some(p) = self.queue.pop_front()
        {
            *steps -= 1;
            if let Some(q) = self.queued.get_mut(p) {
                *q = false;
            }
            if !self.is_free(p) {
                continue;
            }
            let anchors = &self.anchors;
            let at = |i: usize| anchors.get(i).copied().flatten();
            let items = self.reading.items(p, &View { at: &at, oriented: true });
            let Some(fix) = place(&items, self.reading.before(p)) else { continue };
            let how = Placement::TwoLoci { a: fix.a, b: fix.b, transversal: fix.transversal };
            if self.place(out, p, fix.at, how) {
                anchored.push((p, fix.at));
            }
        }
        anchored
    }
}

#[cfg(test)]
pub(crate) mod switch {
    //! Test-only switches: the seed, or only its decomposition step, can be turned off, and
    //! every solve in this crate's tests is repeated without the seed to check that the
    //! outcome is the same.
    use std::cell::Cell;
    use std::thread::LocalKey;

    thread_local! {
        static SEED: Cell<bool> = const { Cell::new(true) };
        static DECOMPOSE: Cell<bool> = const { Cell::new(true) };
        static DIFFERENTIAL: Cell<bool> = const { Cell::new(true) };
    }

    pub(crate) fn seed_on() -> bool {
        SEED.with(Cell::get)
    }
    pub(crate) fn decompose_on() -> bool {
        DECOMPOSE.with(Cell::get)
    }
    pub(crate) fn differential_on() -> bool {
        DIFFERENTIAL.with(Cell::get)
    }

    /// Restores a switch when dropped, also when the test panics.
    struct Restore(&'static LocalKey<Cell<bool>>, bool);

    impl Drop for Restore {
        fn drop(&mut self) {
            self.0.with(|s| s.set(self.1));
        }
    }

    fn off<T>(key: &'static LocalKey<Cell<bool>>, f: impl FnOnce() -> T) -> T {
        let _restore = Restore(key, key.with(|s| s.replace(false)));
        f()
    }
    /// Run `f` with the seed off.
    pub(crate) fn without_seed<T>(f: impl FnOnce() -> T) -> T {
        off(&SEED, f)
    }
    /// Run `f` with the seed's propagation only (no decomposition).
    pub(crate) fn without_decomposition<T>(f: impl FnOnce() -> T) -> T {
        off(&DECOMPOSE, f)
    }
    /// Run `f` without the seed-on/seed-off comparison (for tests that measure the difference).
    pub(crate) fn without_differential<T>(f: impl FnOnce() -> T) -> T {
        off(&DIFFERENTIAL, f)
    }
}

#[cfg(not(test))]
pub(crate) mod switch {
    pub(crate) const fn seed_on() -> bool {
        true
    }
    pub(crate) const fn decompose_on() -> bool {
        true
    }
}
