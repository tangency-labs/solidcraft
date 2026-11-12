//! 4th-axis programs as written, read back through the machine's kinematics and held against
//! the toolpaths they were written from, so what leaves for the machine is checked rather than
//! what was planned.
//!
//! The written blocks are read with [`parse5`], and the machine is moved through each one as
//! the controller would: every axis evenly from the block's start to its end (around the arc
//! for `G2`/`G3`). Points along the way, through [`forward`], give the tip and tool axis against
//! the work, and these are compared both ways round with the planned cutting moves: the tip
//! with the planned line, the axis with the planned turn of the nearest one. Rounding to the
//! post's decimals, the pieces the cuts were broken into, and anything lost between the two
//! show up as deviation. The draw out of the cut and the way back in of a post's rewinds are not
//! in the toolpaths: of a move that may be one (straight along Z at the rewind's retract feed or
//! a plunge feed of the program), only its end in the cut is held against them (the planned
//! moves are still all looked for in what was written), and it counts in the cut length and
//! time as written like any other feed.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::hash::{BuildHasherDefault, Hasher};

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::gcode5::{JointWords, MachineOperation, MachineProgram, Post5, four_axis_only, parse5};
use crate::kinematics::{Carrier, Joints, Machine, arc_distance, forward};
use crate::machine_time::{DwellUnit, joint_dwells_in};
use crate::multiaxis::{Move5, Pose, Program5, Toolpath5, slerp};
use crate::{CamError, Result};

/// Most grid entries the comparison builds.
const MAX_ENTRIES: usize = 20_000_000;
/// Most points the written program is followed at.
const MAX_SAMPLES: usize = 10_000_000;
/// Largest rotary turn between two points a written block is followed at (radians).
const SAMPLE_TURN: f64 = 0.002;
/// Most points one written block is followed at.
const MAX_BLOCK_SAMPLES: usize = 512;
/// Chord error a written arc is followed within (mm).
const ARC_SAMPLE: f64 = 1e-4;

/// The written program against its toolpaths.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Readback5 {
    /// Largest distance between the tip as written and the planned cutting moves, either way
    /// round (mm). Distances beyond [`Readback5::reach`] are reported as `reach`.
    pub max_tip_deviation: f64,
    /// Largest angle between the tool axis as written and the planned one at the nearest
    /// cutting move (radians); π where no planned move is within reach.
    pub max_axis_deviation: f64,
    /// How far the post's precision and tolerances let the tips differ (mm): see [`allowance5`].
    pub allowed_tip: f64,
    /// How far they let the axes differ (radians).
    pub allowed_axis: f64,
    /// Farthest the comparison looks (mm).
    pub reach: f64,
    /// Distance the tip travels at feed as written and as planned (mm).
    pub cut_length: f64,
    pub planned_cut_length: f64,
    /// Time at feed as written (as the controller times it) and as planned, dwells included (minutes).
    pub cut_minutes: f64,
    pub planned_cut_minutes: f64,
}

impl Readback5 {
    /// Whether the written program stays within the post's precision and tolerances of its toolpaths.
    pub fn within(&self) -> bool {
        self.max_tip_deviation <= self.allowed_tip && self.max_axis_deviation <= self.allowed_axis
    }
}

/// The planned moves of every operation in work coordinates.
fn planned_paths(program: &MachineProgram, post: &Post5, machine: &Machine) -> Result<Vec<Toolpath5>> {
    program
        .operations
        .iter()
        .map(|op| match op {
            MachineOperation::Continuous(o) => Ok(o.path.clone()),
            MachineOperation::Indexed(o) => o.poses(machine, post.base.arc_tolerance),
        })
        .collect()
}

/// The largest deviation of tip (mm) and axis (radians) between a written and a planned
/// cutting move that `post`'s precision and tolerances account for, on `machine` for
/// `program`: half the last written digit on each linear axis (three times over, for an arc's
/// end and centre), half the last angle digit on the rotary times the farthest the tip gets
/// from its line, half again the linearization tolerances (the deviation between a piece's ends
/// peaks near, not exactly at, its middle), and the chord error of arcs twice.
pub fn allowance5(post: &Post5, machine: &Machine, program: &MachineProgram) -> Result<(f64, f64)> {
    let paths = planned_paths(program, post, machine)?;
    let reach = paths.iter().flat_map(Toolpath5::poses).map(|p| p.tip.len()).fold(0.0, f64::max);
    let lever = reach + machine.work_origin.len() + machine.rotaries.iter().map(|r| r.center.len()).sum::<f64>() + 1.0;
    let digit = 0.5 * 10f64.powi(-i32::from(post.base.decimals)) * post.base.units.mm();
    let angle_digit = (0.5 * 10f64.powi(-i32::from(post.angle_decimals))).to_radians();
    let n = machine.rotaries.len() as f64;
    let tip = 3.0 * digit * 3f64.sqrt() + n * angle_digit * lever + 1.5 * post.tolerance + 2.0 * post.base.arc_tolerance + 1e-6;
    let axis = 1.5 * post.angle_tolerance + 2.0 * n * angle_digit + 1e-9;
    Ok((tip, axis))
}

/// Reads `text` (written by [`crate::gcode5::emit5`] from `program` through `post` for
/// `machine`) back and measures how far its cutting moves are from the program's toolpaths.
pub fn readback5(text: &str, program: &Program5, post: &Post5, machine: &Machine) -> Result<Readback5> {
    readback_program(text, &MachineProgram::from(program), post, machine)
}

/// Where the axes are after a written block.
#[derive(Clone)]
struct State {
    linear: Vec3,
    rotary: Vec<f64>,
}

/// Whether a cut from `from` to `to` at `feed` (mm/min) is the draw out of the cut or the way
/// back in of one of the post's rewinds ([`crate::gcode5::Rewind`]): on a machine whose rotary
/// turns the table (so the tool is along Z), straight along Z by at most the rewind's distance
/// with every other axis held, at one of `feeds` (the rewind's and the operations' plunge feeds)
/// as written. Such a move only passes through the room the tool leaves above a point of the
/// cut, so that point is all that is checked of it against the toolpaths.
fn rewind_move(post: &Post5, machine: &Machine, from: &State, to: &State, feed: f64, feeds: &[f64]) -> bool {
    let Some(rewind) = post.rewind else { return false };
    if machine.rotaries.iter().any(|r| r.carrier == Carrier::Head) {
        return false;
    }
    let digit = 0.5 * 10f64.powi(-i32::from(post.base.decimals)) * post.base.units.mm();
    let feed_digit = 0.5 * 10f64.powi(-i32::from(post.base.feed_decimals)) * post.base.units.mm() + 1e-9;
    from.rotary == to.rotary
        && from.linear.x == to.linear.x
        && from.linear.y == to.linear.y
        && (to.linear.z - from.linear.z).abs() <= rewind.distance + 2.0 * digit
        && feeds.iter().any(|r| (feed - r).abs() <= feed_digit)
}

fn pose_at(machine: &Machine, linear: Vec3, rotary: &[f64]) -> Pose {
    forward(machine, &Joints { linear, rotary: rotary.to_vec() })
}

/// [`readback5`] for a program that may hold indexed operations
/// ([`crate::gcode5::emit_program`]).
pub fn readback_program(text: &str, program: &MachineProgram, post: &Post5, machine: &Machine) -> Result<Readback5> {
    post.validate()?;
    machine.validate()?;
    four_axis_only(machine)?;
    let span = solidcraft_profile::span("cam", "readback.written");
    let written = joint_dwells_in(DwellUnit::of(&post.base)?, parse5(text, &machine.letters())?);
    let (allowed_tip, allowed_axis) = allowance5(post, machine, program)?;
    let reach = (allowed_tip * 10.0).max(1.0);
    // The feeds a rewind's moves are written at.
    let rewind_feeds: Vec<f64> = match post.rewind {
        Some(r) => [r.retract_feed, r.plunge_feed]
            .into_iter()
            .chain(program.operations.iter().filter_map(|op| match op {
                MachineOperation::Continuous(o) => o.plunge_feed.or(o.tool.plunge_feed),
                MachineOperation::Indexed(_) => None,
            }))
            .collect(),
        None => Vec::new(),
    };

    // The written cutting moves, followed point by point.
    let mut samples: Vec<Pose> = Vec::new();
    let mut written_segments: Vec<(Pose, Pose)> = Vec::new();
    let (mut cut_length, mut cut_minutes) = (0.0, 0.0);
    let mut state: Option<State> = None;
    for w in &written {
        let next = match w {
            JointWords::Rapid { linear, rotary, .. } => Some(State { linear: *linear, rotary: rotary.clone() }),
            JointWords::Feed { linear, rotary, f, inverse_time, mm_per_unit, .. } => {
                let to = State { linear: *linear, rotary: rotary.clone() };
                if let Some(from) = state.as_ref() {
                    // Of a move that may be a rewind's, only its end in the cut is held
                    // against the toolpaths.
                    let rewind = !*inverse_time && rewind_move(post, machine, from, &to, *f * *mm_per_unit, &rewind_feeds);
                    if rewind {
                        let low = if from.linear.z <= to.linear.z { from } else { &to };
                        samples.push(pose_at(machine, low.linear, &low.rotary));
                    }
                    let a = Joints { linear: from.linear, rotary: from.rotary.clone() };
                    let b = Joints { linear: to.linear, rotary: to.rotary.clone() };
                    let turn = a.rotary_change(&b);
                    let lin = from.linear.dist(to.linear);
                    cut_minutes += if *inverse_time {
                        1.0 / f
                    } else if lin > 1e-9 {
                        lin / (f * mm_per_unit)
                    } else {
                        turn.to_degrees() / f
                    };
                    let n = ((turn / SAMPLE_TURN).ceil() as usize).clamp(1, MAX_BLOCK_SAMPLES);
                    let mut prev = pose_at(machine, a.linear, &a.rotary);
                    for k in 1..=n {
                        let j = a.lerp(&b, k as f64 / n as f64);
                        let p = pose_at(machine, j.linear, &j.rotary);
                        cut_length += prev.tip.dist(p.tip);
                        written_segments.push((prev, p));
                        if !rewind {
                            samples.push(p);
                        }
                        prev = p;
                    }
                }
                Some(to)
            }
            JointWords::Arc { linear, rotary, f, mm_per_unit, .. } | JointWords::PlaneArc { linear, rotary, f, mm_per_unit, .. } => {
                let to = State { linear: *linear, rotary: rotary.clone() };
                if let Some(from) = state.as_ref()
                    && let Some(arc) = w.arc_from(from.linear)
                {
                    cut_minutes += arc.length() / (f * mm_per_unit);
                    let mut prev = pose_at(machine, from.linear, rotary);
                    for q in arc.points(ARC_SAMPLE) {
                        let p = pose_at(machine, q, rotary);
                        cut_length += prev.tip.dist(p.tip);
                        written_segments.push((prev, p));
                        samples.push(p);
                        prev = p;
                    }
                }
                Some(to)
            }
            JointWords::Dwell { seconds, .. } => {
                cut_minutes += seconds / 60.0;
                None
            }
        };
        if let Some(s) = next {
            state = Some(s);
        }
        if samples.len() > MAX_SAMPLES {
            return Err(CamError::TooLarge("the program is too large to read back and check".into()));
        }
    }

    drop(span);
    // The planned cutting moves.
    let span = solidcraft_profile::span("cam", "readback.planned");
    let mut planned_segments: Vec<(Pose, Pose)> = Vec::new();
    let mut planned_points: Vec<Pose> = Vec::new();
    let (mut planned_cut_length, mut planned_cut_minutes) = (0.0, 0.0);
    for (op, path) in program.operations.iter().zip(planned_paths(program, post, machine)?) {
        let mut pos: Option<Pose> = None;
        for m in &path.moves {
            if let (Move5::Feed { to, .. }, Some(from)) = (m, pos) {
                planned_segments.push((from, *to));
                planned_points.extend([from, Pose { tip: from.tip.lerp(to.tip, 0.5), axis: slerp(from.axis, to.axis, 0.5) }, *to]);
            }
            if let Some(to) = m.target() {
                pos = Some(to);
            }
        }
        let (length, minutes) = match op {
            MachineOperation::Continuous(_) => {
                let s = path.stats();
                (s.cut_length, s.cut_minutes)
            }
            MachineOperation::Indexed(o) => {
                let s = o.path.stats();
                (s.cut_length, s.cut_minutes)
            }
        };
        planned_cut_length += length;
        planned_cut_minutes += minutes;
    }

    drop(span);
    let (t1, a1) =
        solidcraft_profile::scope("cam", "readback.written_to_planned", || directed(&samples, &planned_segments, reach, allowed_tip, allowed_axis))?;
    let (t2, a2) = solidcraft_profile::scope("cam", "readback.planned_to_written", || {
        directed(&planned_points, &written_segments, reach, allowed_tip, allowed_axis)
    })?;
    Ok(Readback5 {
        max_tip_deviation: t1.max(t2),
        max_axis_deviation: a1.max(a2),
        allowed_tip,
        allowed_axis,
        reach,
        cut_length,
        planned_cut_length,
        cut_minutes,
        planned_cut_minutes,
    })
}

/// Points below which one thread holds them all against the pieces.
const POINTS_PER_THREAD: usize = 8192;
/// How many times finer than the far grid's the cells of the near grid are.
const NEAR_CELLS: f64 = 8.0;
/// Entries in a cell's 27 of the far grid past which its points look in the near grid first.
const CROWDED: usize = 256;

/// Pieces filed by the cells of space they cross (each split into bits at most a cell long,
/// filed by the cells of the bit's box, so a long diagonal files about its length in cells, not
/// its box's volume); a point looks in its cell and the 26 around it, so any piece within
/// `cell` of it is seen. In space, not seen from above: the levels of a pocket or the turns of
/// a wrapped path lie over one another, and filed by XY alone each point would be held against
/// all of them.
struct Grid {
    cell: f64,
    lo: Vec3,
    cells: HashMap<(i64, i64, i64), Vec<usize>, BuildHasherDefault<CellHasher>>,
}

/// A hash of a cell's index for the grids every point looks up 27 cells of: a multiply (the map
/// only needs its cells spread), not the default's keyed hash.
#[derive(Default)]
struct CellHasher(u64);

impl Hasher for CellHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(*b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }

    fn write_i64(&mut self, n: i64) {
        self.0 = (self.0.rotate_left(5) ^ n as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl Grid {
    /// The pieces of `to` that `keep` names filed in cells `cell` wide from `lo`; `None` past
    /// `cap` entries.
    fn file(to: &[(Pose, Pose)], keep: impl Fn(usize) -> bool, lo: Vec3, cell: f64, cap: usize) -> Option<Grid> {
        let mut grid = Grid { cell, lo, cells: HashMap::default() };
        let mut entries = 0usize;
        for (k, (a, b)) in to.iter().enumerate().filter(|(k, _)| keep(*k)) {
            let (pa, pb) = (a.tip, b.tip);
            // The extent bounds the length, so at most about 4096 × NEAR_CELLS × √3 bits.
            let bits = ((pa.dist(pb) / cell).ceil() as usize).clamp(1, 1 << 16);
            for s in 0..bits {
                let (p, q) = (pa.lerp(pb, s as f64 / bits as f64), pa.lerp(pb, (s + 1) as f64 / bits as f64));
                let ((i0, j0, l0), (i1, j1, l1)) = (grid.at(p.min(q)), grid.at(p.max(q)));
                for i in i0..=i1 {
                    for j in j0..=j1 {
                        for l in l0..=l1 {
                            let filed = grid.cells.entry((i, j, l)).or_default();
                            if filed.last() != Some(&k) {
                                filed.push(k);
                                entries += 1;
                            }
                        }
                    }
                }
                if entries > cap {
                    return None;
                }
            }
        }
        Some(grid)
    }

    /// The cells around `c` and `c` itself.
    fn around((i, j, l): (i64, i64, i64)) -> impl Iterator<Item = (i64, i64, i64)> {
        (i - 1..=i + 1).flat_map(move |i| (j - 1..=j + 1).flat_map(move |j| (l - 1..=l + 1).map(move |l| (i, j, l))))
    }

    fn at(&self, p: Vec3) -> (i64, i64, i64) {
        let index = |v: f64, o: f64| ((v - o) / self.cell).floor() as i64;
        (index(p.x, self.lo.x), index(p.y, self.lo.y), index(p.z, self.lo.z))
    }
}

/// The largest deviation of a pose of `from` from the piece of `to` that explains it best (the
/// least of tip distance over `allowed_tip` and axis angle over `allowed_axis`, the larger of
/// the two counting; then the nearer, then the less turned): its tip from the piece's line and
/// its axis from the piece's turn. Tip distances are capped at `reach`.
fn directed(from: &[Pose], to: &[(Pose, Pose)], reach: f64, allowed_tip: f64, allowed_axis: f64) -> Result<(f64, f64)> {
    directed_counted(from, to, reach, allowed_tip, allowed_axis).map(|(worst, _)| worst)
}

/// [`directed`], and how many pieces it measured a point against.
pub(crate) fn directed_counted(from: &[Pose], to: &[(Pose, Pose)], reach: f64, allowed_tip: f64, allowed_axis: f64) -> Result<((f64, f64), usize)> {
    if from.is_empty() {
        return Ok(((0.0, 0.0), 0));
    }
    if to.is_empty() {
        return Ok(((reach, PI), 0));
    }
    let (mut lo, mut hi) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in to.iter().flat_map(|(a, b)| [a.tip, b.tip]).chain(from.iter().map(|p| p.tip)) {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    if !(lo.is_finite() && hi.is_finite()) {
        return Err(CamError::Unsafe("a toolpath has a coordinate that is not a finite number".into()));
    }
    // The far grid's cells are the reach wide, so a point's 27 hold every piece in reach. Where
    // the pieces crowd (the turns of a small helix, a written arc's many chords) those 27 hold
    // thousands, so a point in a crowded cell looks first in the near grid, cells a fraction of
    // that, of the pieces filed around crowded cells: whatever it does not hold is farther than
    // its cell, and a best piece whose score already says it is nearer than that is the best of
    // all. Only the points it cannot settle look in the far grid.
    let cell = reach.max((hi.x - lo.x).max(hi.y - lo.y).max(hi.z - lo.z) / 4096.0);
    let far = Grid::file(to, |_| true, lo, cell, MAX_ENTRIES)
        .ok_or_else(|| CamError::TooLarge("the program is too large to read back and check".into()))?;
    // How many entries each cell's 27 hold, and the crowded cells.
    let mut held: HashMap<(i64, i64, i64), usize, BuildHasherDefault<CellHasher>> = HashMap::default();
    for (&c, filed) in &far.cells {
        for x in Grid::around(c) {
            *held.entry(x).or_default() += filed.len();
        }
    }
    let crowded: Vec<(i64, i64, i64)> = held.iter().filter(|&(_, &n)| n > CROWDED).map(|(&c, _)| c).collect();
    let near = (!crowded.is_empty())
        .then(|| {
            solidcraft_profile::scope("cam", "readback.near_grid", || {
                let mut keep = vec![false; to.len()];
                for k in crowded.iter().flat_map(|&c| Grid::around(c)).filter_map(|c| far.cells.get(&c)).flatten() {
                    if let Some(x) = keep.get_mut(*k) {
                        *x = true;
                    }
                }
                Grid::file(to, |k| keep.get(k).copied().unwrap_or(false), lo, cell / NEAR_CELLS, MAX_ENTRIES / 4)
            })
        })
        .flatten();
    // The best piece for `p` among those a grid files around it, and the pieces measured.
    let best_in = |grid: &Grid, p: &Pose, measured: &mut usize| -> Option<(f64, f64, f64)> {
        let (pi, pj, pl) = grid.at(p.tip);
        let mut best: Option<(f64, f64, f64)> = None;
        for i in pi - 1..=pi + 1 {
            for j in pj - 1..=pj + 1 {
                for l in pl - 1..=pl + 1 {
                    for &k in grid.cells.get(&(i, j, l)).map(Vec::as_slice).unwrap_or_default() {
                        let Some((a, b)) = to.get(k) else { continue };
                        *measured += 1;
                        let d = p.tip.dist_to_segment(a.tip, b.tip);
                        if d > reach {
                            continue;
                        }
                        // A piece whose tip distance alone scores worse than the best so far
                        // cannot replace it (its score is at least that), so its axis is not
                        // measured.
                        if best.is_some_and(|(s, _, _)| d / allowed_tip > s) {
                            continue;
                        }
                        let e = arc_distance(p.axis, a.axis, b.axis);
                        let score = (d / allowed_tip).max(e / allowed_axis);
                        if best.is_none_or(|b| (score, d, e) < b) {
                            best = Some((score, d, e));
                        }
                    }
                }
            }
        }
        best
    };
    // Each point against the pieces near it, the points side by side.
    let check = |p: &Pose| -> (f64, f64, usize) {
        let mut measured = 0;
        if let Some(near) = &near
            && held.get(&far.at(p.tip)).is_some_and(|&n| n > CROWDED)
            && let Some((score, d, e)) = best_in(near, p, &mut measured)
            && score * allowed_tip <= near.cell * (1.0 - 1e-6)
        {
            return (d, e, measured);
        }
        match best_in(&far, p, &mut measured) {
            Some((_, d, e)) => (d, e, measured),
            None => (reach, PI, measured),
        }
    };
    let ranges = crate::par::split(&vec![1; from.len()], crate::par::threads_for(from.len(), POINTS_PER_THREAD));
    let worst = crate::par::map(ranges, |r| {
        from.get(r).unwrap_or(&[]).iter().try_fold((0.0f64, 0.0f64, 0usize), |(t, a, n), p| {
            let (d, e, m) = check(p);
            Ok::<_, CamError>((t.max(d), a.max(e), n + m))
        })
    })?;
    let (t, a, n) = worst.into_iter().fold((0.0f64, 0.0f64, 0usize), |(t, a, n), (d, e, m)| (t.max(d), a.max(e), n + m));
    Ok(((t, a), n))
}
