//! Stock-removal simulation of the session: the setup `sim.setup` made, and the stock as far as its
//! playhead has cut it. Runtime state, like `render_jobs`: outside the design and its undo history.
//! A design that replaces this session (`Session::replace_design`) takes this state with it.
//!
//! The playhead runs on machine time. A move takes its path length over its feed (a rapid, or a move
//! with no feed, at the setup's rapid rate). A run cuts the whole moves that end by the playhead, and
//! the steps of the move in progress that are done by then. A backward seek restores the nearest
//! checkpoint, a copy of the stock every few moves, and cuts forward from it.
//!
//! Checkpoints are costed by the tiles they do not share with the checkpoint before them, so a few
//! of them fit in a quarter of the budget.
//!
//! shortcut: the first checkpoint is costed as a whole stock, since nothing earlier shares its tiles,
//! so a stock over a quarter of the budget keeps no checkpoints and a backward seek replays from the
//! start. Upgrade: keep the uncut box and cost the first checkpoint against it.
//!
//! shortcut: a checkpoint is not kept for a forward seek past it, so the moves after a backward seek are
//! cut again. Upgrade: keep the later checkpoints, sorted, and skip them on the way forward.

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_machining::SimError;
use solidcraft_machining::dexel::Stock;
use solidcraft_machining::sim::{Cut, Cutter, Progress, step_count};
use solidcraft_machining::sweep::{ArcFixedAxis, FlatTool};
use solidcraft_machining::toolpath::{Diag, MoveKind, NO_TOOL, Program};
use std::sync::atomic::{AtomicU64, Ordering};

use web_time::Instant;

use crate::Session;

/// Moves between checkpoints at first. Each time the checkpoints are thinned, the spacing doubles.
const CHECKPOINT_EVERY: usize = 256;

/// What `sim.setup` was given. Kept so a backward seek can rebuild the stock from the start.
pub(crate) struct Setup {
    pub min: Vec3,
    pub max: Vec3,
    pub h: f64,
    pub budget_bytes: u64,
    /// The rapid rate, mm/min.
    pub rapid_mm_min: f64,
    pub program: Program,
}

/// The stock after `cursor` whole moves, with the cutter's reports so far: where a backward seek
/// starts from. `bytes` is what it adds to the checkpoints: the tiles it does not share with the one
/// before (see `Job::recount`).
struct Checkpoint {
    cursor: usize,
    stock: Stock,
    cutter: Cutter,
    bytes: u64,
}

/// One simulation: the stock, how many moves of the program it has had cut, and what was reported.
pub(crate) struct Job {
    setup: Setup,
    stock: Stock,
    /// Moves completed; the stock has all of them and none after.
    cursor: usize,
    cutter: Cutter,
    /// The box's volume, from which removed volume is counted.
    start_volume: f64,
    /// The move in progress, part-cut: how many of its steps the stock has.
    partial: Option<Cut>,
    /// The failure that stopped the run. A forward run refuses until a backward seek.
    error: Option<SimError>,
    /// Changes when the stock is rebuilt from the setup (see `rebuild`). A restore from a checkpoint
    /// keeps it: the UI redraws the chunks that changed.
    revision: u64,
    /// Where each move starts on the machine clock, s, and where the last one ends: move `i` runs from
    /// `starts[i]` to `starts[i + 1]`.
    starts: Vec<f64>,
    /// Steps each move takes when cut (see `step_count`), so a playhead maps onto steps without a cut.
    steps: Vec<u32>,
    /// The playhead: the machine time the stock is cut to.
    time: f64,
    checkpoints: Vec<Checkpoint>,
    /// Moves between checkpoints now (see `CHECKPOINT_EVERY`).
    every: usize,
    /// Most bytes the checkpoints may take: a quarter of the budget.
    cap: u64,
    /// Checkpoint restores so far, for the tests that check a backward seek used one.
    #[cfg(test)]
    restores: usize,
}

#[derive(Default)]
pub(crate) struct Sim {
    job: Option<Job>,
}

/// Revisions are handed out by the process, not by a session, so one never repeats: not after
/// `file.new`, not in another session.
static REVISIONS: AtomicU64 = AtomicU64::new(0);

/// A revision no stock has had before.
pub(crate) fn next_revision() -> u64 {
    REVISIONS.fetch_add(1, Ordering::Relaxed) + 1
}

/// The move's way from its start, the previous tip, as the cutter's arc, so the tool follows the cut.
/// `None` for a straight move, the first move, or an arc the cutter would not take.
fn arc_of(program: &Program, i: usize) -> Option<ArcFixedAxis> {
    let m = program.moves.get(i)?;
    let MoveKind::Arc { centre, normal, ccw, turns } = &m.kind else { return None };
    let start = program.moves.get(i.checked_sub(1)?)?.tip;
    // The tool only fills the arc's own fields: its geometry does not depend on the tool.
    let tool = FlatTool::new(1.0, 1.0, Vec3::Z)?;
    ArcFixedAxis::new(Vec3::from(start), Vec3::from(m.tip), Vec3::from(*centre), Vec3::from(*normal), *ccw, *turns, tool)
}

/// The time move `i` takes, s: its path length over its feed. A rapid, or a move with no feed, runs at
/// `rapid` mm/s. The first move has no start point and a jump has no known path, so both take none.
fn move_time(program: &Program, i: usize, rapid: f64) -> f64 {
    let Some(m) = program.moves.get(i) else { return 0.0 };
    let Some(start) = i.checked_sub(1).and_then(|p| program.moves.get(p)).map(|p| Vec3::from(p.tip)) else { return 0.0 };
    if matches!(m.kind, MoveKind::Jump) {
        return 0.0;
    }
    let len = match arc_of(program, i) {
        Some(arc) => arc.length(),
        None => (Vec3::from(m.tip) - start).len(),
    };
    let rate = if matches!(m.kind, MoveKind::Rapid) || !(m.feed > 0.0) { rapid } else { m.feed / 60.0 };
    let t = len / rate;
    if t.is_finite() { t } else { 0.0 }
}

/// Where each move starts on the machine clock, then where the last one ends (see `Job::starts`).
fn move_starts(program: &Program, rapid_mm_min: f64) -> Vec<f64> {
    let rapid = rapid_mm_min / 60.0;
    let mut starts = Vec::with_capacity(program.moves.len() + 1);
    let mut t = 0.0;
    starts.push(t);
    for i in 0..program.moves.len() {
        t += move_time(program, i, rapid);
        starts.push(t);
    }
    starts
}

impl Job {
    /// The stock of `setup`, uncut. Nothing is dirty: the UI draws the uncut box itself. Refused, never
    /// a panic, when the box or the memory is bad, or when the program takes more time than a number holds.
    pub fn new(setup: Setup, revision: u64) -> Result<Job, SimError> {
        let stock = Stock::new_box(setup.min, setup.max, setup.h, setup.budget_bytes)?;
        let start_volume = stock.volume();
        let starts = move_starts(&setup.program, setup.rapid_mm_min);
        if !starts.last().is_some_and(|t| t.is_finite()) {
            return Err(SimError::BadInput("the program takes too long to time".into()));
        }
        let steps = (0..setup.program.moves.len()).map(|i| u32::try_from(step_count(&setup.program, i)).unwrap_or(u32::MAX)).collect();
        let cap = setup.budget_bytes / 4;
        Ok(Job {
            setup,
            stock,
            cursor: 0,
            cutter: Cutter::new(),
            start_volume,
            partial: None,
            error: None,
            revision,
            starts,
            steps,
            time: 0.0,
            checkpoints: Vec::new(),
            every: CHECKPOINT_EVERY,
            cap,
            #[cfg(test)]
            restores: 0,
        })
    }

    /// Runs to move `to` (a move count), cutting forward or going back. The playhead then sits at the
    /// end of move `to - 1`, or where the cut got to when the run stopped early. Stops early, with
    /// `false`, when `until` has passed, but always after one step, so a call makes progress. A step that
    /// fails stops the run: the error is kept, and a forward run then returns it until a backward seek.
    pub fn run_to(&mut self, to: usize, until: Option<Instant>) -> Result<bool, SimError> {
        let reached = self.goto(to, 0, until);
        self.time = match reached {
            Ok(true) => self.starts.get(to).copied().unwrap_or(0.0),
            _ => self.position_time(),
        };
        reached
    }

    /// Runs to the playhead `t` (s, within the program): the whole moves that end by then, and the steps
    /// of the move in progress that are done by then. Otherwise as `run_to`.
    pub fn run_at(&mut self, t: f64, until: Option<Instant>) -> Result<bool, SimError> {
        let t = t.clamp(0.0, self.total_time());
        let (to, part) = self.target_at(t);
        let reached = self.goto(to, part, until);
        self.time = match reached {
            Ok(true) => t,
            _ => self.position_time(),
        };
        reached
    }

    /// Moves the stock to the position (`to`, `part`): `to` moves cut whole, then `part` steps of move
    /// `to`. Behind the position it goes back: a move part-cut is rewound in place, and anything earlier
    /// restores the nearest checkpoint and cuts forward. Stops early as `run_to` does.
    fn goto(&mut self, to: usize, part: usize, until: Option<Instant>) -> Result<bool, SimError> {
        if (to, part) < self.position() {
            if self.cursor == to {
                // The move in progress is taken back in place, not replayed from a checkpoint.
                if let Some(mut cut) = self.partial.take() {
                    cut.revert(&mut self.stock);
                }
            } else {
                self.back(to)?;
            }
        }
        if let Some(e) = self.error.as_ref().filter(|_| self.position() < (to, part)) {
            return Err(e.clone());
        }
        let mut first = true;
        while self.position() < (to, part) {
            if !first && until.is_some_and(|t| Instant::now() >= t) {
                return Ok(false);
            }
            first = false;
            if let Err(e) = self.step() {
                self.error = Some(e.clone());
                return Err(e);
            }
        }
        Ok(true)
    }

    /// The position the playhead `t` cuts to: the whole moves that end by then, and the steps of the
    /// next move that are done by then. A move with one step (or none, when it cannot be simulated) is
    /// cut whole only at its end.
    fn target_at(&self, t: f64) -> (usize, usize) {
        let ends = self.starts.get(1..).unwrap_or(&[]);
        let whole = ends.partition_point(|&e| e <= t);
        let (Some(&from), Some(&to)) = (self.starts.get(whole), self.starts.get(whole + 1)) else {
            return (whole, 0);
        };
        if to <= from {
            return (whole, 0);
        }
        let steps = self.steps.get(whole).copied().unwrap_or(0) as usize;
        if steps < 2 {
            return (whole, 0);
        }
        let fraction = ((t - from) / (to - from)).clamp(0.0, 1.0);
        let part = ((fraction * steps as f64).floor() as usize).min(steps - 1);
        (whole, part)
    }

    /// Where the stock is cut to: the moves completed, and the steps done of the move in progress.
    fn position(&self) -> (usize, usize) {
        (self.cursor, self.partial.as_ref().map_or(0, Cut::done))
    }

    /// The machine time the stock is cut to: the start of the move in progress, plus its share done (a
    /// lower bound, since a step counts once the playhead has passed its share).
    fn position_time(&self) -> f64 {
        let Some(&begin) = self.starts.get(self.cursor) else { return self.total_time() };
        match (&self.partial, self.starts.get(self.cursor + 1)) {
            (Some(cut), Some(&end)) if cut.steps() > 0 => begin + (end - begin) * cut.done() as f64 / cut.steps() as f64,
            _ => begin,
        }
    }

    /// Restores the nearest checkpoint at or before move `to`, or the setup, uncut, when there is none.
    /// The checkpoints after it go: they are cut again on the way forward.
    fn back(&mut self, to: usize) -> Result<(), SimError> {
        let found = self
            .checkpoints
            .iter()
            .rposition(|c| c.cursor <= to)
            .and_then(|i| self.checkpoints.get(i).map(|cp| (i, cp.stock.clone(), cp.cursor, cp.cutter.clone())));
        match found {
            Some((i, stock, cursor, cutter)) => {
                self.checkpoints.truncate(i + 1);
                #[cfg(test)]
                {
                    self.restores += 1;
                }
                self.replace(stock, cursor, cutter, false);
                Ok(())
            }
            None => self.rebuild(),
        }
    }

    /// Builds the stock again from the setup, uncut, for a backward seek with no checkpoint before it.
    fn rebuild(&mut self) -> Result<(), SimError> {
        let stock = Stock::new_box(self.setup.min, self.setup.max, self.setup.h, self.setup.budget_bytes)?;
        self.checkpoints.clear();
        self.every = CHECKPOINT_EVERY;
        self.replace(stock, 0, Cutter::new(), true);
        Ok(())
    }

    /// Puts `stock` in place of the current one, at `cursor` with `cutter`. The chunks either stock has
    /// reached are dirty, since the surface may differ wherever they are. The revision changes only when
    /// `new_revision` says the stock was built again: a restore from a checkpoint keeps it.
    fn replace(&mut self, mut stock: Stock, cursor: usize, cutter: Cutter, new_revision: bool) {
        let mut dirty = self.stock.touched_chunks();
        // An undone step can leave a chunk dirty but no longer touched; its old mesh must still go.
        dirty.extend(self.stock.dirty_chunks());
        dirty.extend(stock.touched_chunks());
        stock.clear_dirty();
        stock.mark_dirty(dirty);
        self.stock = stock;
        self.cursor = cursor;
        self.cutter = cutter;
        self.partial = None;
        self.error = None;
        if new_revision {
            self.revision = next_revision();
        }
    }

    /// Cuts one step: the next piece of the move in progress, or the first step of the next move.
    fn step(&mut self) -> Result<(), SimError> {
        let mut cut = match self.partial.take() {
            Some(cut) => cut,
            None => match self.cutter.start(&self.setup.program, self.cursor)? {
                Some(cut) => cut,
                None => {
                    self.finish_move();
                    return Ok(());
                }
            },
        };
        match self.cutter.step(&mut cut, &mut self.stock)? {
            Progress::More => self.partial = Some(cut),
            Progress::Cut | Progress::Skipped => self.finish_move(),
        }
        Ok(())
    }

    /// Counts the move at the cursor as cut. Every `every` moves it keeps a checkpoint of the stock, and
    /// over the cap it thins the checkpoints.
    fn finish_move(&mut self) {
        self.cursor += 1;
        let last = self.checkpoints.last().map_or(0, |c| c.cursor);
        if self.cursor < last.saturating_add(self.every) {
            return;
        }
        let bytes = match self.checkpoints.last() {
            Some(prev) => self.stock.unshared_bytes(&prev.stock),
            None => self.stock.memory().0,
        };
        if bytes > self.cap {
            // Wait longer before costing the next one, so a refused checkpoint is not recounted every move.
            self.every = self.every.saturating_mul(2);
            return;
        }
        self.checkpoints.push(Checkpoint { cursor: self.cursor, stock: self.stock.clone(), cutter: self.cutter.clone(), bytes });
        // Over the cap, drop every other checkpoint but the first and the newest, and space the rest
        // twice as far. A pair drops its first, the newest stays.
        while self.checkpoint_bytes() > self.cap {
            if self.checkpoints.is_empty() {
                break;
            }
            if self.checkpoints.len() <= 2 {
                self.checkpoints.remove(0);
            } else {
                let newest = self.checkpoints.len() - 1;
                let mut i = 0;
                self.checkpoints.retain(|_| {
                    let keep = i % 2 == 0 || i == newest;
                    i += 1;
                    keep
                });
            }
            self.recount();
            self.every = self.every.saturating_mul(2);
        }
    }

    /// Costs each checkpoint again: the tiles it does not share with the one before it. Dropping a
    /// checkpoint changes what the next one adds, so this runs after the checkpoints change.
    fn recount(&mut self) {
        for i in 0..self.checkpoints.len() {
            let bytes = match i.checked_sub(1).and_then(|p| self.checkpoints.get(p)) {
                Some(prev) => self.checkpoints.get(i).map_or(0, |c| c.stock.unshared_bytes(&prev.stock)),
                None => self.checkpoints.get(i).map_or(0, |c| c.stock.memory().0),
            };
            if let Some(c) = self.checkpoints.get_mut(i) {
                c.bytes = bytes;
            }
        }
    }

    /// Bytes the checkpoints take, by their costs.
    fn checkpoint_bytes(&self) -> u64 {
        self.checkpoints.iter().fold(0u64, |sum, c| sum.saturating_add(c.bytes))
    }

    /// The tool at the playhead: its tool's id, its tip and its axis. The tip moves along the move the
    /// playhead is on, from the previous tip (on the arc, when the move is one), and the axis turns
    /// linearly and is normalised. `None` without moves or when that move has no tool.
    pub fn tool_pose(&self) -> Option<(u32, [f64; 3], [f64; 3])> {
        let program = &self.setup.program;
        let n = program.moves.len();
        if n == 0 {
            return None;
        }
        let k = self.starts.partition_point(|&s| s <= self.time).saturating_sub(1).min(n - 1);
        let m = program.moves.get(k)?;
        if m.tool == NO_TOOL {
            return None;
        }
        let (begin, end) = (*self.starts.get(k)?, *self.starts.get(k + 1)?);
        let f = if end > begin { ((self.time - begin) / (end - begin)).clamp(0.0, 1.0) } else { 1.0 };
        let prev = k.checked_sub(1).and_then(|p| program.moves.get(p));
        let tip = Vec3::from(m.tip);
        let from = prev.map_or(tip, |p| Vec3::from(p.tip));
        let at = match arc_of(program, k) {
            Some(arc) => arc.point(f),
            None => from + (tip - from) * f,
        };
        let from_axis = Vec3::from(prev.map_or(m.axis, |p| p.axis));
        let to_axis = Vec3::from(m.axis);
        let axis = (from_axis + (to_axis - from_axis) * f).normalized().unwrap_or(to_axis);
        Some((m.tool, <[f64; 3]>::from(at), <[f64; 3]>::from(axis)))
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn total(&self) -> usize {
        self.setup.program.moves.len()
    }

    /// The machine time of the whole program, s.
    pub fn total_time(&self) -> f64 {
        self.starts.last().copied().unwrap_or(0.0)
    }

    /// The playhead, s: the machine time the stock is cut to.
    pub fn playhead(&self) -> f64 {
        self.time
    }

    /// Hands out the chunks whose surface changed since the last call, and forgets them.
    pub fn take_dirty(&mut self) -> Vec<[usize; 3]> {
        let dirty = self.stock.dirty_chunks();
        self.stock.clear_dirty();
        dirty
    }

    /// The step count of the move in progress: `(done, steps)`, when a run stopped part-way through.
    pub fn partial(&self) -> Option<(usize, usize)> {
        self.partial.as_ref().map(|c| (c.done(), c.steps()))
    }

    /// Why the run stopped, when a step failed.
    pub fn error(&self) -> Option<&SimError> {
        self.error.as_ref()
    }

    /// The box's volume before any cut, mm^3.
    pub fn start_volume(&self) -> f64 {
        self.start_volume
    }

    /// Material left, mm^3.
    pub fn remaining_volume(&self) -> f64 {
        self.stock.volume()
    }

    /// Material taken away so far, mm^3.
    pub fn removed_volume(&self) -> f64 {
        self.start_volume - self.stock.volume()
    }

    /// Bytes the stock takes, against its budget.
    pub fn memory(&self) -> (u64, u64) {
        self.stock.memory()
    }

    pub fn bounds(&self) -> (Vec3, Vec3) {
        (self.setup.min, self.setup.max)
    }

    pub fn h(&self) -> f64 {
        self.setup.h
    }

    /// The G-code's own problems, then what cutting could not simulate.
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diag> + '_ {
        self.setup.program.diags.iter().chain(self.cutter.diags())
    }
}

impl Sim {
    /// Makes `job` the simulation, dropping the one there was.
    pub fn set(&mut self, job: Job) {
        self.job = Some(job);
    }

    pub fn job(&self) -> Option<&Job> {
        self.job.as_ref()
    }

    pub fn job_mut(&mut self) -> Option<&mut Job> {
        self.job.as_mut()
    }

    /// Drops the simulation; `true` when there was one.
    pub fn clear(&mut self) -> bool {
        self.job.take().is_some()
    }
}

/// Read access for the UI (drawing the stock and the toolpath): the simulation's stock as far as
/// its cursor has cut it, and the program it runs. `None` before `sim.setup`.
impl Session {
    pub fn sim_stock(&self) -> Option<&Stock> {
        self.sim.job().map(|j| &j.stock)
    }

    pub fn sim_program(&self) -> Option<&Program> {
        self.sim.job().map(|j| &j.setup.program)
    }

    /// The stock's revision: it changes when the stock is rebuilt from the setup (setup, a backward seek
    /// with no checkpoint before it, clear). 0 without a simulation.
    pub fn sim_revision(&self) -> u64 {
        self.sim.job().map_or(0, Job::revision)
    }

    /// The chunks whose surface changed since the last call, each once, and forgets them: what the UI
    /// remeshes. Empty without a simulation.
    pub fn sim_take_dirty(&mut self) -> Vec<[usize; 3]> {
        self.sim.job_mut().map_or_else(Vec::new, Job::take_dirty)
    }

    /// Moves completed and moves in the program, for the simulation's progress. `None` without one.
    pub fn sim_progress(&self) -> Option<(usize, usize)> {
        self.sim.job().map(|j| (j.cursor(), j.total()))
    }

    /// The playhead and the program's machine time, both s. `None` without a simulation.
    pub fn sim_time(&self) -> Option<(f64, f64)> {
        self.sim.job().map(|j| (j.playhead(), j.total_time()))
    }

    /// The tool at the playhead: its tool's id, tip and axis. `None` without a simulation, or when the
    /// move there has no tool.
    pub fn sim_tool_pose(&self) -> Option<(u32, [f64; 3], [f64; 3])> {
        self.sim.job().and_then(Job::tool_pose)
    }

    /// Moves between checkpoints now, for the tests. `None` without a simulation.
    #[cfg(test)]
    pub fn sim_checkpoint_every(&self) -> Option<usize> {
        self.sim.job().map(|j| j.every)
    }

    /// Checkpoint restores so far in this simulation, for the tests. `None` without a simulation.
    #[cfg(test)]
    pub fn sim_checkpoint_restores(&self) -> Option<usize> {
        self.sim.job().map(|j| j.restores)
    }
}
