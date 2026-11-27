//! Sketch block members the solve leaves out.
//!
//! A block holds each member point P at its place in the frame of its axis from O to X by two
//! rows, `P = O + u·d + v·perp(d)` with `d = X − O`, and each member radius at its share of the
//! axis length by one. When nothing else reads P (no constraint on it, no other block, no curve
//! relation but one among such points of the same block), those rows only say where P goes once
//! O and X are known: P and its rows are left out of the solve, and P is placed from the solved
//! axis. A block's members then no longer join one connected group of unknowns, so a block of
//! any size solves and drags as fast as its axis does.
//!
//! The analysis means the same. A left-out point's two rows are the only ones that read its two
//! unknowns, and they are the identity there: the rank rises by exactly two for it, so it adds
//! no freedom, and it is determined exactly when the freedoms the rest leaves move it by
//! nothing. The checks capped at a group's size (redundancy, `Nearby`) run on the smaller group,
//! so they now reach blocks the cap kept them from. Every null vector of the rows left in extends to one of all rows (the point follows
//! the axis), so that is read from the null space on the axis ends' columns. A left-out radius
//! is the same with one row and one unknown. An arc's or a closed spline's own relation among
//! left-out points of one block holds wherever the frame puts them (a block keeps its shape up
//! to a similarity, which keeps both): it is a combination of their frame rows, adds no rank,
//! and is left out with them and checked once they are placed.
//!
//! LM damps a step by how far it moves the unknowns, so of the steps that fit equally well it
//! takes the one that moves least. Left out, the members would no longer weigh on a move of the
//! axis, and an under-constrained block dragged by one point would turn about its axis instead
//! of its members' middle. [`Moments`] gives the axis ends' columns the damping the members'
//! moves added.

use super::{Block, Layout, State, max_abs, set_point, system};
use crate::linalg::Reduced;
use crate::model::Sketch;
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

/// What moving a block's axis ends costs LM's damping for the members left out of the solve.
/// With `w = u + iv` (complex) a point's place in the frame, the point moves `(1 − w)·δO + w·δX`,
/// so the sum of the points' squared moves is `outer·|δO|² + inner·|δX|² + 2·Re(cross·conj(δO)·δX)`.
/// A radius of a block of free scale moves `k·δ|d|`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Moments {
    pub(super) o: usize,
    pub(super) x: usize,
    /// `Σ|1 − w|²`.
    outer: f64,
    /// `Σ|w|²`.
    inner: f64,
    /// `Σ conj(1 − w)·w`, real and imaginary parts.
    cross: [f64; 2],
    /// `Σk²` over the left-out radii of a block of free scale.
    radii: f64,
    /// The most a left-out point moves for a unit move of each axis end, `max(|1 − w| + |w|)`:
    /// how much an error of the axis grows at the members.
    reach: f64,
}

impl Moments {
    /// See the field.
    pub(super) fn reach(&self) -> f64 {
        self.reach
    }
}

/// One block's members left out of the solve.
#[derive(Debug)]
struct Frame {
    o: usize,
    x: usize,
    /// Each point: its index and its place (u, v) in the frame.
    points: Vec<(usize, f64, f64)>,
    /// Each radius: its curve, its share of the axis length, and the length of a block of fixed
    /// scale.
    radii: Vec<(usize, f64, Option<f64>)>,
}

impl Frame {
    fn moments(&self) -> Moments {
        let mut m = Moments { o: self.o, x: self.x, outer: 0.0, inner: 0.0, cross: [0.0; 2], radii: 0.0, reach: 1.0 };
        for &(_, u, v) in &self.points {
            m.reach = m.reach.max((1.0 - u).hypot(v) + u.hypot(v));
            // conj(1 − w)·w = (1 − u + iv)(u + iv) = u − u² − v² + iv.
            m.outer += (1.0 - u) * (1.0 - u) + v * v;
            m.inner += u * u + v * v;
            m.cross[0] += u - u * u - v * v;
            m.cross[1] += v;
        }
        m.radii = self.radii.iter().filter(|(_, _, l0)| l0.is_none()).map(|(_, k, _)| k * k).sum();
        m
    }

    fn units(&self) -> impl Iterator<Item = Unit> + '_ {
        let radii = self.radii.iter().map(|&(r, k, l0)| Unit::Radius { r, k, fixed: l0.is_some() });
        self.points.iter().map(|&(p, u, v)| Unit::Point { p, u, v }).chain(radii)
    }
}

/// The members of every block that the solve leaves out (see the module documentation).
#[derive(Debug, Default)]
pub(super) struct Carried<'a> {
    frames: Vec<Frame>,
    /// The curves' own relations among left-out points, checked once those are placed.
    implied: Blocks<'a>,
}

/// The rank analysis' pivot tolerance: a null vector moving a member by less moves it by nothing.
const STILL: f64 = 1e-7;

/// The solve's blocks with the variables each reads.
type Blocks<'a> = Vec<(Block<'a>, Vec<usize>)>;

/// `blocks` (from `build_blocks`, with whether each reads a constant) without the rows of the
/// block members nothing else reads, and a [`Block::Carried`] in their place for each block
/// whose axis has an unknown end.
pub(super) fn carry<'a>(sk: &Sketch, lay: &Layout, blocks: Blocks<'a>, reads_constant: Vec<bool>) -> (Blocks<'a>, Vec<bool>, Carried<'a>) {
    if sk.blocks.is_empty() || !switch::on() {
        return (blocks, reads_constant, Carried::default());
    }
    const MANY: usize = usize::MAX;
    let pv = |p: usize| lay.pvar.get(p).copied().flatten();
    let rv = |c: usize| lay.rvar.get(c).copied().flatten();
    // Per unknown (by its first variable): the block whose one frame row places it, or MANY
    // when more than one row does.
    let mut owner: Vec<Option<usize>> = vec![None; lay.n];
    for (b, _) in &blocks {
        let (u, f) = match *b {
            Block::Frame { p, f, .. } => (pv(p), f),
            Block::FrameRadius { r, f, .. } => (rv(r), f),
            _ => continue,
        };
        if let Some(slot) = u.and_then(|u| owner.get_mut(u)) {
            *slot = Some(if slot.is_some() { MANY } else { f });
        }
    }
    let mut open: Vec<bool> = owner.iter().map(|o| o.is_some_and(|f| f != MANY)).collect();
    if !open.contains(&true) {
        return (blocks, reads_constant, Carried::default());
    }
    let units: Vec<Vec<usize>> = blocks
        .iter()
        .map(|(_, vars)| {
            let mut u: Vec<usize> = vars.iter().map(|v| lay.unit(*v)).collect();
            u.dedup();
            u
        })
        .collect();
    let mut readers: Vec<Vec<usize>> = vec![Vec::new(); lay.n];
    for (i, us) in units.iter().enumerate() {
        for u in us {
            if let Some(r) = readers.get_mut(*u) {
                r.push(i);
            }
        }
    }
    // An unknown still open to leave out, placed by block `f`.
    let mine = |open: &[bool], u: usize, f: usize| open.get(u).copied().unwrap_or(false) && owner.get(u).copied().flatten() == Some(f);
    // The block whose left-out unknowns row `i` only places or relates, if it does.
    let carries = |open: &[bool], i: usize| -> Option<usize> {
        let (b, _) = blocks.get(i)?;
        let us = units.get(i)?;
        match *b {
            Block::Frame { p, f, .. } => pv(p).filter(|u| mine(open, *u, f)).map(|_| f),
            Block::FrameRadius { r, o, x, f, .. } => {
                let axis = [pv(o), pv(x)];
                (rv(r).is_some_and(|u| mine(open, u, f)) && us.iter().all(|u| axis.contains(&Some(*u)) || mine(open, *u, f))).then_some(f)
            }
            // A fixed point (a fixed centre) does not move with the frame, so the row still binds.
            Block::Arc { .. } | Block::Closure { .. } if reads_constant.get(i).copied().unwrap_or(false) => None,
            Block::Arc { .. } | Block::Closure { .. } => {
                let f = us.first().and_then(|u| owner.get(*u).copied().flatten())?;
                us.iter().all(|u| mine(open, *u, f)).then_some(f)
            }
            _ => None,
        }
    };
    // Close every unknown a row reads that it does not leave out with it, until nothing changes:
    // each unknown closes once and then rechecks the rows that read it.
    let mut stack: Vec<usize> = (0..blocks.len()).rev().collect();
    while let Some(i) = stack.pop() {
        let fate = carries(&open, i);
        let close: Vec<usize> = units.get(i).into_iter().flatten().copied().filter(|u| !fate.is_some_and(|f| mine(&open, *u, f))).collect();
        for u in close {
            if let Some(o) = open.get_mut(u).filter(|o| **o) {
                *o = false;
                stack.extend(readers.get(u).into_iter().flatten().copied());
            }
        }
    }
    let fates: Vec<Option<usize>> = (0..blocks.len()).map(|i| carries(&open, i)).collect();
    // Per block (its index in `frame_rows`): where it is in `frames`.
    let mut slot: Vec<Option<usize>> = vec![None; sk.blocks.len()];
    let mut frames: Vec<Frame> = Vec::new();
    let mut implied = Vec::new();
    let mut kept = Vec::with_capacity(blocks.len());
    let mut kept_constant = Vec::with_capacity(blocks.len());
    for ((b, vars), (fate, constant)) in blocks.into_iter().zip(fates.into_iter().zip(reads_constant)) {
        let Some(f) = fate else {
            kept.push((b, vars));
            kept_constant.push(constant);
            continue;
        };
        let (o, x) = match b {
            Block::Frame { o, x, .. } | Block::FrameRadius { o, x, .. } => (o, x),
            _ => {
                implied.push((b, vars));
                continue;
            }
        };
        let Some(at) = slot.get_mut(f) else { continue };
        let at = *at.get_or_insert_with(|| {
            frames.push(Frame { o, x, points: Vec::new(), radii: Vec::new() });
            frames.len() - 1
        });
        let Some(frame) = frames.get_mut(at) else { continue };
        match b {
            Block::Frame { p, u, v, .. } => frame.points.push((p, u, v)),
            Block::FrameRadius { r, k, l0, .. } => frame.radii.push((r, k, l0)),
            _ => {}
        }
    }
    for frame in &frames {
        let mut vars: Vec<usize> = [frame.o, frame.x].into_iter().filter_map(pv).flat_map(|v| [v, v + 1]).collect();
        if vars.is_empty() {
            continue;
        }
        vars.sort_unstable();
        kept_constant.push(vars.len() < 4);
        kept.push((Block::Carried(frame.moments()), vars));
    }
    (kept, kept_constant, Carried { frames, implied })
}

/// The damping the [`Block::Carried`] among `blocks` add to LM's matrix, at `x`: each entry's
/// row and column (in `col`, of `n`) and its weight, to be scaled by the damping factor.
pub(super) fn damping(sk: &Sketch, lay: &Layout, blocks: &[(Block, Vec<usize>)], x: &[f64], col: &[usize], n: usize) -> Vec<(usize, usize, f64)> {
    let mut out = Vec::new();
    for (b, _) in blocks {
        let Block::Carried(m) = b else { continue };
        let at = |p: usize, k: usize| lay.pvar.get(p).copied().flatten().and_then(|v| col.get(v + k).copied()).filter(|c| *c < n);
        let cols = [at(m.o, 0), at(m.o, 1), at(m.x, 0), at(m.x, 1)];
        let mut w = [[0.0; 4]; 4];
        for k in 0..2 {
            w[k][k] = m.outer;
            w[k + 2][k + 2] = m.inner;
        }
        // The (O, X) block is the 2 × 2 matrix of multiplying by `cross`.
        let [re, im] = m.cross;
        let ox = [[re, -im], [im, re]];
        for (i, row) in ox.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                w[i][j + 2] = *v;
                w[j + 2][i] = *v;
            }
        }
        if m.radii > 0.0 {
            let s = State { sk, lay, x };
            let d = s.p(m.x) - s.p(m.o);
            let len = d.len();
            if len > 0.0 && len.is_finite() {
                let g = [d.x / len, d.y / len];
                for i in 0..2 {
                    for j in 0..2 {
                        let gg = m.radii * g[i] * g[j];
                        w[i][j] += gg;
                        w[i + 2][j + 2] += gg;
                        w[i][j + 2] -= gg;
                        w[i + 2][j] -= gg;
                    }
                }
            }
        }
        for (i, ci) in cols.iter().enumerate() {
            for (j, cj) in cols.iter().enumerate() {
                if let (Some(ci), Some(cj), Some(v)) = (ci, cj, w.get(i).and_then(|r| r.get(j)))
                    && *v != 0.0
                {
                    out.push((*ci, *cj, *v));
                }
            }
        }
    }
    out
}

/// A pose the analysis reads the freedoms at: the reduced Jacobian of the unknowns analysed
/// there (columns from `col`, of `cols`) and the values of all unknowns.
pub(super) struct Pose<'p> {
    pub(super) sk: &'p Sketch,
    pub(super) reduced: &'p Reduced,
    pub(super) x: &'p [f64],
}

impl Carried<'_> {
    /// Put every left-out member where its block's axis puts it in `x`.
    pub(super) fn place(&self, sk: &Sketch, lay: &Layout, x: &mut [f64]) {
        for frame in &self.frames {
            let (o, d, length) = {
                let s = State { sk, lay, x };
                let (o, end) = (s.p(frame.o), s.p(frame.x));
                (o, end - o, o.dist(end))
            };
            for &(p, u, v) in &frame.points {
                if let Some(var) = lay.pvar.get(p).copied().flatten() {
                    set_point(x, var, o + d * u + d.perp() * v);
                }
            }
            for &(r, k, l0) in &frame.radii {
                if let Some(slot) = lay.rvar.get(r).copied().flatten().and_then(|v| x.get_mut(v)) {
                    *slot = k * l0.unwrap_or(length);
                }
            }
        }
    }

    /// Per point of `n`: left out.
    pub(super) fn point_mask(&self, n: usize) -> Vec<bool> {
        let mut mask = if self.frames.is_empty() { Vec::new() } else { vec![false; n] };
        for (p, _, _) in self.frames.iter().flat_map(|f| &f.points) {
            if let Some(m) = mask.get_mut(*p) {
                *m = true;
            }
        }
        mask
    }

    /// The points left out, ascending.
    #[cfg(test)]
    pub(super) fn points(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.frames.iter().flat_map(|f| f.points.iter().map(|(p, _, _)| *p)).collect();
        out.sort_unstable();
        out
    }

    /// The largest residual of the curves' own relations among left-out points, at `x`.
    pub(super) fn implied_residual(&self, sk: &Sketch, lay: &Layout, x: &mut [f64]) -> f64 {
        if self.implied.is_empty() { 0.0 } else { max_abs(&system(sk, lay, &self.implied, x, false).0) }
    }

    /// The blocks with an axis end among the sorted variables `vars` (of one component).
    pub(super) fn in_component(&self, lay: &Layout, vars: &[usize]) -> Vec<usize> {
        if self.frames.is_empty() {
            return Vec::new();
        }
        let var = |p: usize| lay.pvar.get(p).copied().flatten();
        (0..self.frames.len())
            .filter(|f| self.frames.get(*f).is_some_and(|fr| [fr.o, fr.x].into_iter().filter_map(var).any(|v| vars.binary_search(&v).is_ok())))
            .collect()
    }

    /// Mark the left-out unknowns of the blocks whose axis ends are both constants determined,
    /// and return the rank of their rows.
    pub(super) fn settle_constant(&self, lay: &Layout, det: &mut [bool]) -> usize {
        let var = |p: usize| lay.pvar.get(p).copied().flatten();
        let mut rank = 0;
        for frame in self.frames.iter().filter(|fr| var(fr.o).is_none() && var(fr.x).is_none()) {
            rank += self.mark(lay, frame, det, |_| true);
        }
        rank
    }

    /// For the blocks `frames` of one analysed component: mark each left-out unknown determined
    /// when the freedoms at `pose` move it by nothing (none is in a component free to translate,
    /// `floating`), and return the rank of their rows and whether one so marked lies where a
    /// freedom of its axis leaves it (a turn about it).
    pub(super) fn settle(
        &self,
        lay: &Layout,
        frames: &[usize],
        floating: bool,
        pose: &Pose,
        col: &[usize],
        cols: usize,
        det: &mut [bool],
    ) -> (usize, bool) {
        let null = self.null(lay, frames, pose, col, cols);
        let mut rank = 0;
        let mut still_while_turning = false;
        for (f, motions) in frames.iter().zip(&null) {
            let Some(frame) = self.frames.get(*f) else { continue };
            rank += self.mark(lay, frame, det, |unit| {
                let still = !(floating && unit.is_point()) && motions.iter().all(|m| unit.moved(m, pose, frame, lay) <= STILL);
                still_while_turning |= still && !motions.is_empty();
                still
            });
        }
        (rank, still_while_turning)
    }

    /// Clear the flags of the left-out unknowns of `frames` that the freedoms at a second pose
    /// move (see `Nearby` in the solver).
    pub(super) fn unsettle(&self, lay: &Layout, frames: &[usize], pose: &Pose, col: &[usize], cols: usize, det: &mut [bool]) {
        let null = self.null(lay, frames, pose, col, cols);
        for (f, motions) in frames.iter().zip(&null) {
            let Some(frame) = self.frames.get(*f) else { continue };
            for unit in frame.units() {
                if motions.iter().any(|m| unit.moved(m, pose, frame, lay) > STILL) {
                    for v in unit.vars(lay) {
                        if let Some(d) = det.get_mut(v) {
                            *d = false;
                        }
                    }
                }
            }
        }
    }

    /// Per block of `frames`: the moves of its axis ends `[δOx, δOy, δXx, δXy]` along each
    /// null vector of `pose`'s Jacobian that moves them (an end that is not among the columns
    /// stays put).
    fn null(&self, lay: &Layout, frames: &[usize], pose: &Pose, col: &[usize], cols: usize) -> Vec<Vec<[f64; 4]>> {
        let slots: Vec<[Option<usize>; 4]> = frames
            .iter()
            .map(|f| {
                let at = |p: usize, k: usize| lay.pvar.get(p).copied().flatten().and_then(|v| col.get(v + k).copied()).filter(|c| *c < cols);
                self.frames.get(*f).map_or([None; 4], |fr| [at(fr.o, 0), at(fr.o, 1), at(fr.x, 0), at(fr.x, 1)])
            })
            .collect();
        let mut wanted: Vec<usize> = slots.iter().flatten().flatten().copied().collect();
        wanted.sort_unstable();
        wanted.dedup();
        let vectors = if wanted.is_empty() { Vec::new() } else { pose.reduced.null_on(&wanted) };
        slots
            .iter()
            .map(|s| {
                let entry =
                    |n: &[f64], c: Option<usize>| c.and_then(|c| wanted.binary_search(&c).ok()).and_then(|k| n.get(k)).copied().unwrap_or(0.0);
                vectors
                    .iter()
                    .map(|n| [entry(n, s[0]), entry(n, s[1]), entry(n, s[2]), entry(n, s[3])])
                    .filter(|m| m.iter().any(|v| *v != 0.0))
                    .collect()
            })
            .collect()
    }

    /// Set `det` for `frame`'s left-out unknowns as `still` says; the rank of their rows.
    fn mark(&self, lay: &Layout, frame: &Frame, det: &mut [bool], mut still: impl FnMut(&Unit) -> bool) -> usize {
        let mut rank = 0;
        for unit in frame.units() {
            let s = still(&unit);
            let vars = unit.vars(lay);
            rank += vars.len();
            for v in vars {
                if let Some(d) = det.get_mut(v) {
                    *d = s;
                }
            }
        }
        rank
    }
}

/// A left-out unknown: a point at (u, v) in its block's frame, or a radius at `k` times its
/// block's length.
enum Unit {
    Point { p: usize, u: f64, v: f64 },
    Radius { r: usize, k: f64, fixed: bool },
}

impl Unit {
    fn is_point(&self) -> bool {
        matches!(self, Unit::Point { .. })
    }

    fn vars(&self, lay: &Layout) -> Vec<usize> {
        match *self {
            Unit::Point { p, .. } => lay.pvar.get(p).copied().flatten().map(|v| vec![v, v + 1]).unwrap_or_default(),
            Unit::Radius { r, .. } => lay.rvar.get(r).copied().flatten().into_iter().collect(),
        }
    }

    /// How far it moves (largest coordinate) when the axis ends move by `m` at `pose`.
    fn moved(&self, m: &[f64; 4], pose: &Pose, frame: &Frame, lay: &Layout) -> f64 {
        let (d_o, d_x) = (Vec2::new(m[0], m[1]), Vec2::new(m[2], m[3]));
        let e = d_x - d_o;
        match *self {
            Unit::Point { u, v, .. } => {
                let moved = d_o + e * u + e.perp() * v;
                moved.x.abs().max(moved.y.abs())
            }
            Unit::Radius { fixed: true, .. } => 0.0,
            Unit::Radius { k, .. } => {
                let s = State { sk: pose.sk, lay, x: pose.x };
                let d = s.p(frame.x) - s.p(frame.o);
                let len = d.len();
                if len > 0.0 { (k * d.dot(e) / len).abs() } else { (k * e.len()).abs() }
            }
        }
    }
}

/// Whether blocks' members are left out of the solve: always, except in this crate's tests,
/// which compare with the solve that keeps them.
#[cfg(test)]
pub(crate) mod switch {
    use std::cell::Cell;

    thread_local! {
        static OFF: Cell<bool> = const { Cell::new(false) };
    }

    pub(crate) fn on() -> bool {
        !OFF.with(Cell::get)
    }

    /// Run `f` with every block member kept in the solve.
    pub(crate) fn without<T>(f: impl FnOnce() -> T) -> T {
        let was = OFF.with(|o| o.replace(true));
        let out = f();
        OFF.with(|o| o.set(was));
        out
    }
}

#[cfg(not(test))]
mod switch {
    pub(super) fn on() -> bool {
        true
    }
}
