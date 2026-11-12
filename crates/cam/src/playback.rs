//! Playing a program move by move: the timeline (every move with its operation, tool, feed,
//! spindle speed and time, the operations and the tool changes) and a [`Playback`] that cuts the
//! simulated stock ([`crate::sim`]) to any move, or part of the way through one, forward or back,
//! finding what goes wrong as it goes.
//!
//! Going back restores the nearest copy of the stock kept at or before the target (a keyframe,
//! kept every so many cells cut, within a memory budget: when it is spent every other keyframe
//! is dropped and they are kept half as often) and cuts forward again, which is the same work,
//! so the stock is the same. A seek may be given a budget of cells to cut, for a view that does
//! a frame's worth at a time; it then says how far it got.
//!
//! What is found as the stock is cut, at the move and place it happens: a rapid in the stock,
//! a gouge (a cell cut below the part, measured again in 3D), cutting with the spindle stopped,
//! the shank rubbing (material above the flutes), the tool's shank or holder or a machine part
//! coming down on the stock, the cutter, holder or a machine part running into the workholding
//! or the table, and the spindle nose above its travel ([`crate::machine_model`]). Ported from
//! the author's own MIT-licensed G-code simulator: stepping the simulation, the holder and
//! machine checks against the stock as it is, and the findings they give.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::findings::{FindingEvent, FindingLog, Severity};
use crate::gcode::number;
use crate::gcode5::{JointWords, parse5};
use crate::kinematics::{bed_point, head_frame, work_turn_x};
use crate::machine_model::{MachineModel, ZoneOf, clearance, zones};
use crate::machine_time::{DwellUnit, MachineRates, block_times};
use crate::multiaxis::Pose;
use crate::obstacle::Turn;
use crate::sim::{Footprint, Grid, PartFloor, SimPlan, SimState, column_depth};
use crate::surface::Surface;
use crate::tool::Tool;
use crate::workholding::HoldScene;
use crate::{CamError, Result, invalid};

/// How far apart the checks along a move are (mm).
pub const CHECK_STEP: f64 = 0.5;
/// Most check positions along one move.
const MAX_CHECKS: usize = 4000;

/// What a move is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveKind {
    Rapid,
    Feed,
    Arc,
    /// A cut down the tool axis.
    Plunge,
}

/// One move of the timeline (a simulation step).
#[derive(Clone, Debug, PartialEq)]
pub struct ViewMove {
    pub line: usize,
    pub kind: MoveKind,
    /// Its run of operation ([`Timeline::ops`]).
    pub op: Option<usize>,
    pub tool: u32,
    pub slot: u8,
    /// The feed (mm/min, degrees a minute for a turn alone), for a cut.
    pub feed: Option<f64>,
    pub rpm: Option<f64>,
    /// When it starts and ends (seconds from the program's start).
    pub t0: f64,
    pub t1: f64,
}

/// A run of moves of one operation.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewOp {
    pub name: String,
    /// The operation's index in the job (or the tool change's, for a program alone).
    pub index: usize,
    pub tool: u32,
    /// Its moves: `first..end`.
    pub first: usize,
    pub end: usize,
    pub t0: f64,
    pub t1: f64,
}

/// A tool change: the first move with the new tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolChange {
    pub at_move: usize,
    pub line: usize,
    pub tool: u32,
}

/// Every move in time, the operations and the tool changes.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Timeline {
    pub moves: Vec<ViewMove>,
    pub ops: Vec<ViewOp>,
    pub tool_changes: Vec<ToolChange>,
    /// The whole program, seconds.
    pub duration: f64,
    /// Whether the moves were timed with acceleration.
    pub accelerated: bool,
}

/// The operations of a program written from a job: their names and each block's (as
/// [`parse5`] reads them, in order) operation index.
pub struct OpsGiven<'a> {
    pub names: &'a [String],
    pub owners: &'a [usize],
}

impl Timeline {
    /// The timeline of `text` as `plan` reads it, timed with `rates` (dwells in `dwell`); the
    /// operations from `ops`, or one per tool change (named `T<n>`).
    pub fn build(plan: &SimPlan, text: &str, rates: &MachineRates, dwell: DwellUnit, ops: Option<OpsGiven<'_>>) -> Result<Timeline> {
        let letters = plan.machine().letters();
        let blocks = parse5(text, &letters)?;
        let times = block_times(text, &letters, rates, dwell)?;
        let rapid_s = rates.rapid_xyz_mm_min / 60.0;
        let mut t = 0.0;
        let mut wi = 0usize;
        let mut moves: Vec<ViewMove> = Vec::with_capacity(plan.moves());
        let mut owners: Vec<Option<usize>> = Vec::with_capacity(plan.moves());
        for (b, block) in blocks.iter().enumerate() {
            let line = times.lines.get(b).copied().unwrap_or(0);
            while let Some(w) = times.waits.get(wi).filter(|w| w.line <= line) {
                t += w.seconds;
                wi += 1;
            }
            let mut secs = times.seconds.get(b).copied().unwrap_or(0.0);
            if let JointWords::Dwell { .. } = block {
                t += secs;
                continue;
            }
            let k = moves.len();
            let Some(sv) = plan.step(k) else { break };
            if secs <= 0.0 && sv.travel > 0.0 && rapid_s > 0.0 {
                // A move from an unknown position: as a rapid from above.
                secs = sv.travel / rapid_s;
            }
            let along = sv.end.tip - sv.start.tip;
            let kind = if sv.rapid {
                MoveKind::Rapid
            } else if sv.arc {
                MoveKind::Arc
            } else if along.dot(sv.end.axis) < -0.7 * along.len() {
                MoveKind::Plunge
            } else {
                MoveKind::Feed
            };
            let feed = match block {
                JointWords::Feed { f, inverse_time, mm_per_unit, .. } => Some(if *inverse_time {
                    sv.travel * f
                } else if along.len() > 1e-9 {
                    f * mm_per_unit
                } else {
                    *f
                }),
                JointWords::Arc { f, mm_per_unit, .. } | JointWords::PlaneArc { f, mm_per_unit, .. } => Some(f * mm_per_unit),
                _ => None,
            };
            moves.push(ViewMove { line: sv.line, kind, op: None, tool: sv.tool, slot: sv.slot, feed, rpm: sv.rpm, t0: t, t1: t + secs });
            owners.push(ops.as_ref().and_then(|o| o.owners.get(b).copied()));
            t += secs;
        }
        while let Some(w) = times.waits.get(wi) {
            t += w.seconds;
            wi += 1;
        }
        // Tool changes, and operations as runs of moves.
        let mut tool_changes = Vec::new();
        let mut runs: Vec<ViewOp> = Vec::new();
        let mut last_slot: Option<u8> = None;
        let mut by_tool = 0usize;
        for (k, m) in moves.iter_mut().enumerate() {
            if last_slot != Some(m.slot) {
                if last_slot.is_some() {
                    by_tool += 1;
                }
                tool_changes.push(ToolChange { at_move: k, line: m.line, tool: m.tool });
                last_slot = Some(m.slot);
            }
            let (index, name) = match (&ops, owners.get(k).copied().flatten()) {
                (Some(o), Some(i)) => (i, o.names.get(i).cloned().unwrap_or_else(|| format!("operation {}", i + 1))),
                _ => (by_tool, format!("T{}", m.tool)),
            };
            match runs.last_mut().filter(|r| r.index == index && r.name == name) {
                Some(r) => {
                    r.end = k + 1;
                    r.t1 = m.t1;
                }
                None => runs.push(ViewOp { name, index, tool: m.tool, first: k, end: k + 1, t0: m.t0, t1: m.t1 }),
            }
            m.op = Some(runs.len() - 1);
        }
        Ok(Timeline { moves, ops: runs, tool_changes, duration: t, accelerated: times.accelerated })
    }

    /// The time at fraction `f` of move `m` (seconds).
    pub fn time_at(&self, m: usize, f: f64) -> f64 {
        match self.moves.get(m) {
            Some(mv) => mv.t0 + (mv.t1 - mv.t0) * f.clamp(0.0, 1.0),
            None => self.duration,
        }
    }

    /// The move and fraction at time `t` (between moves: the end of the one before).
    pub fn at_time(&self, t: f64) -> (usize, f64) {
        let k = self.moves.partition_point(|m| m.t0 <= t);
        let Some(i) = k.checked_sub(1) else { return (0, 0.0) };
        match self.moves.get(i) {
            Some(m) if m.t1 > m.t0 && t < m.t1 => (i, ((t - m.t0) / (m.t1 - m.t0)).clamp(0.0, 1.0)),
            _ => (i, 1.0),
        }
    }

    /// The last move at or before G-code line `line`.
    pub fn at_line(&self, line: usize) -> Option<usize> {
        self.moves.partition_point(|m| m.line <= line).checked_sub(1)
    }
}

/// How much work and memory the keyframes may take.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    /// About how many cells are cut between two keyframes (more: a seek back cuts more again).
    pub keyframe_visits: f64,
    /// The most memory every keyframe together may take (bytes).
    pub keyframe_bytes: usize,
}

impl Default for Budget {
    fn default() -> Budget {
        #[cfg(target_arch = "wasm32")]
        let bytes = 128 << 20;
        #[cfg(not(target_arch = "wasm32"))]
        let bytes = 512 << 20;
        Budget { keyframe_visits: 20.0e6, keyframe_bytes: bytes }
    }
}

/// What a playback holds besides the program: the tools (all the job's), the machine model and
/// the workholding placed with the stock, and which operations cut into the part on purpose.
#[derive(Clone, Default)]
pub struct PlaybackSetup {
    pub tools: Vec<Tool>,
    pub model: Option<Arc<MachineModel>>,
    pub scene: Option<Arc<HoldScene>>,
    /// By operation index (as [`ViewOp::index`]): its gouges are warnings, not errors.
    pub deliberate: Vec<bool>,
    /// Whether the part has faces under its own material along Z, read on its exact faces: the
    /// stock is then kept along X and Y too ([`SimState::track_sides`]) and the view shows what
    /// those lines hold. `None`: found from the part's surface, as [`crate::sim::simulate`] does.
    pub hidden: Option<bool>,
}

/// How far a seek got.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reached {
    /// The move and the fraction of it the tool is at.
    pub at: (usize, f64),
    /// Whether it got to the target (else the budget ran out first).
    pub done: bool,
}

/// A program played on a simulated stock. See the module documentation.
pub struct Playback {
    plan: Arc<SimPlan>,
    timeline: Arc<Timeline>,
    part: Arc<Surface>,
    floor: Option<Arc<PartFloor>>,
    setup: PlaybackSetup,
    state: SimState,
    /// The tool's place: a move and the fraction of it done.
    cursor: (usize, f64),
    keyframes: Vec<SimState>,
    spacing: f64,
    since: f64,
    log: FindingLog,
    /// Moves whose findings are in the log: `0..checked`.
    checked: usize,
    budget: Budget,
    generation: u64,
    /// The stock is kept along X and Y too ([`PlaybackSetup::hidden`]).
    sides: bool,
}

fn before(a: (usize, f64), b: (usize, f64)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && a.1 < b.1)
}

impl Playback {
    /// A playback at the program's start. With `watch_gouges`, the part's height over every
    /// cell is found first (exact, on the part's surface), so gouges are found as they are cut.
    pub fn new(
        plan: Arc<SimPlan>,
        timeline: Arc<Timeline>,
        part: Arc<Surface>,
        setup: PlaybackSetup,
        budget: Budget,
        watch_gouges: bool,
    ) -> Playback {
        let floor = (watch_gouges && !part.is_empty()).then(|| Arc::new(plan.part_floor(&part)));
        let mut state = SimState::new(&plan);
        if let Some(f) = &floor {
            state.set_floor(f);
        }
        let sides = setup.hidden.unwrap_or_else(|| crate::sim::hides_faces(&part, plan.grid())) && state.track_sides(&plan);
        let mut log = FindingLog::default();
        if let Some(s) = &setup.scene {
            for f in &s.findings {
                log.push(f.clone());
            }
        }
        let spacing = if budget.keyframe_visits.is_finite() && budget.keyframe_visits > 1.0 { budget.keyframe_visits } else { 20.0e6 };
        Playback {
            plan,
            timeline,
            part,
            floor,
            setup,
            state,
            cursor: (0, 0.0),
            keyframes: Vec::new(),
            spacing,
            since: 0.0,
            log,
            checked: 0,
            budget,
            generation: 0,
            sides,
        }
    }

    pub fn plan(&self) -> &SimPlan {
        &self.plan
    }

    pub fn timeline(&self) -> &Timeline {
        &self.timeline
    }

    pub fn state(&self) -> &SimState {
        &self.state
    }

    pub fn setup(&self) -> &PlaybackSetup {
        &self.setup
    }

    /// The tool in slot `slot` of the plan.
    pub fn tool(&self, slot: u8) -> Option<&Tool> {
        let n = self.plan.tool_number(slot)?;
        self.setup.tools.iter().find(|t| t.number == n)
    }

    /// The move and fraction the tool is at.
    pub fn at(&self) -> (usize, f64) {
        self.cursor
    }

    /// Changes whenever the stock is put back to a keyframe (tile versions start again then).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// What was found so far (every move before [`Playback::checked`]).
    pub fn findings(&self) -> &FindingLog {
        &self.log
    }

    /// Moves whose findings are known.
    pub fn checked(&self) -> usize {
        self.checked
    }

    pub fn keyframes(&self) -> usize {
        self.keyframes.len()
    }

    pub fn keyframe_bytes(&self) -> usize {
        self.keyframes.iter().map(SimState::bytes).sum()
    }

    /// The tool's pose now.
    pub fn pose_now(&self) -> Option<Pose> {
        self.plan.pose(self.cursor.0, self.cursor.1)
    }

    /// The machine's rotary axes now (radians, as written; empty on a 3-axis machine).
    pub fn rotary_now(&self) -> Vec<f64> {
        self.plan.joints(self.cursor.0, self.cursor.1).map(|j| j.rotary).unwrap_or_default()
    }

    /// The spindle's own X, Y and Z now, in work coordinates ([`head_frame`]): how a part on
    /// the spindle is placed, as the collision check places it.
    pub fn head_frame_now(&self) -> [Vec3; 3] {
        head_frame(self.plan.machine(), &self.rotary_now())
    }

    /// Where a point fixed to the bed (work coordinates with the rotaries at zero) is now, in
    /// work coordinates ([`bed_point`]): the workholding turns against the work on a 4th axis.
    pub fn bed_point_now(&self, p: Vec3) -> Vec3 {
        bed_point(self.plan.machine(), &self.rotary_now(), p)
    }

    /// Where the tip is against the bed now (work coordinates with the rotaries at zero), as the
    /// collision check places the machine's carriage and frame parts.
    pub fn bed_tip_now(&self) -> Option<Vec3> {
        self.plan.joints(self.cursor.0, self.cursor.1).map(|j| j.linear - self.plan.machine().work_origin)
    }

    /// The bed's own X, Y and Z in work coordinates now: how a part fixed to the bed (or moving
    /// on it without turning: the carriage) stands against the work.
    pub fn bed_frame_now(&self) -> [Vec3; 3] {
        let o = self.bed_point_now(Vec3::ZERO);
        [Vec3::X, Vec3::Y, Vec3::Z].map(|e| self.bed_point_now(e) - o)
    }

    /// The time now (seconds from the start).
    pub fn time_now(&self) -> f64 {
        self.timeline.time_at(self.cursor.0, self.cursor.1)
    }

    /// Material removed up to now (mm³).
    pub fn removed_volume(&self) -> f64 {
        self.state.removed().iter().map(|r| r.volume).sum()
    }

    /// Moves the tool to fraction `f` of move `m` (past the last move: the end), cutting the
    /// stock to there; with `limit`, cutting about that many cells at most. See the module
    /// documentation.
    pub fn seek(&mut self, m: usize, f: f64, limit: Option<f64>) -> Result<Reached> {
        let n = self.plan.moves();
        let f = if f.is_finite() { f.clamp(0.0, 1.0) } else { 0.0 };
        let want = if n == 0 {
            (0, 0.0)
        } else if m >= n {
            (n - 1, 1.0)
        } else {
            (m, f)
        };
        // The same place in the stock's terms: the end of a move is the start of the next.
        let target = if want.1 >= 1.0 { (want.0 + 1, 0.0) } else { want };
        if before(target, self.state.at()) {
            self.restore(target);
        }
        let mut used = 0.0;
        loop {
            let (k, f0) = self.state.at();
            if !before((k, f0), target) {
                break;
            }
            let room = limit.map_or(f64::INFINITY, |l| (l - used).max(0.0));
            let (mut end, mut cost) = (k, 0.0);
            while end < target.0 {
                let c = self.plan.step_cost(end) * if end == k { 1.0 - f0 } else { 1.0 };
                if end > k && (cost + c > room || self.since + cost + c > self.spacing) {
                    break;
                }
                if end == k && c > room && limit.is_some() {
                    break;
                }
                cost += c;
                end += 1;
            }
            let (to, frac) = if end > k {
                (end, 0.0)
            } else if k < target.0 {
                // The step alone is over the budget: a part of it.
                let c = self.plan.step_cost(k).max(1.0);
                cost = room.max(1.0);
                (k, (f0 + room.max(1.0) / c).min(0.999_999))
            } else {
                cost = self.plan.step_cost(k) * (target.1 - f0).max(0.0);
                target
            };
            self.state.advance(&self.plan, to, frac)?;
            let done_to = self.state.at().0;
            for i in k..done_to {
                if i == self.checked {
                    self.check_step(i);
                    self.checked += 1;
                }
            }
            self.since += cost;
            used += cost;
            if self.state.at().1 == 0.0 && self.since >= self.spacing {
                self.keep();
            }
            if limit.is_some_and(|l| used >= l) && before(self.state.at(), target) {
                let (k, f) = self.state.at();
                self.cursor = if f == 0.0 && k > 0 { (k - 1, 1.0) } else { (k, f) };
                return Ok(Reached { at: self.cursor, done: false });
            }
        }
        self.cursor = want;
        Ok(Reached { at: want, done: true })
    }

    /// Plays on to the end, keeping keyframes and finding everything, then goes back to where
    /// the tool was.
    pub fn run_to_end(&mut self) -> Result<()> {
        let at = self.cursor;
        self.seek(usize::MAX, 1.0, None)?;
        self.seek(at.0, at.1, None)?;
        Ok(())
    }

    /// The first finding after move `m` (the setup's none), if the log has it.
    pub fn next_finding_after(&self, m: usize) -> Option<&FindingEvent> {
        self.log.events().iter().filter(|e| e.at_move.is_some_and(|x| x > m)).min_by_key(|e| e.at_move)
    }

    /// The stock put back to the latest keyframe at or before `target` (or to the start).
    fn restore(&mut self, target: (usize, f64)) {
        self.generation = self.generation.wrapping_add(1);
        self.since = 0.0;
        let k = self.keyframes.partition_point(|s| !before(target, s.at()));
        match k.checked_sub(1).and_then(|i| self.keyframes.get(i)) {
            Some(s) => self.state = s.clone(),
            None => {
                self.state = SimState::new(&self.plan);
                if let Some(f) = &self.floor {
                    self.state.set_floor(f);
                }
                if self.sides {
                    self.state.track_sides(&self.plan);
                }
            }
        }
    }

    /// Keeps a copy of the stock as it is, within the budget.
    fn keep(&mut self) {
        self.since = 0.0;
        let at = self.state.at();
        if self.keyframes.last().is_some_and(|s| !before(s.at(), at)) {
            return;
        }
        self.keyframes.push(self.state.clone());
        while self.keyframes.len() > 1 && self.keyframe_bytes() > self.budget.keyframe_bytes {
            let mut keep = false;
            self.keyframes.retain(|_| {
                keep = !keep;
                keep
            });
            self.spacing *= 2.0;
        }
        if self.keyframe_bytes() > self.budget.keyframe_bytes {
            self.keyframes.clear();
        }
    }

    /// What move `i` (just cut whole) did wrong.
    fn check_step(&mut self, i: usize) {
        let Some(sv) = self.plan.step(i) else { return };
        let r = self.state.removed().get(i).copied().unwrap_or_default();
        let tol = self.plan.tolerance();
        let op = self.timeline.moves.get(i).and_then(|m| m.op);
        let deliberate = op.and_then(|o| self.timeline.ops.get(o)).and_then(|o| self.setup.deliberate.get(o.index)).copied().unwrap_or(false);
        let mut found: Vec<FindingEvent> = Vec::new();
        let mut ev = |code: &str, severity: Severity, at: Vec3, depth: f64, message: String| {
            if !found.iter().any(|e| e.code == code) {
                found.push(FindingEvent { code: code.into(), severity, at_move: Some(i), line: sv.line, op, at: Some(at), depth, message });
            }
        };
        let mm = |v: f64| number(v, 3);
        if sv.rapid && r.depth > tol {
            ev("rapid-in-stock", Severity::Error, r.at, r.depth, format!("a rapid cuts {} mm into the stock (a crash on the machine)", mm(r.depth)));
        }
        if r.gouge > tol {
            let d = column_depth(&self.part, r.gouge_from, r.gouge_at, r.gouge);
            if d > tol {
                let (sev, why) = if deliberate {
                    (
                        Severity::Warning,
                        " (its operation cuts what the model need not show: `gouge_check: false`, or a chamfer, V-trace or spot its gouge check holds to its band)",
                    )
                } else {
                    (Severity::Error, "")
                };
                ev("gouge", sev, r.gouge_at, d, format!("the tool cuts {} mm into the part{why}", mm(d)));
            }
        }
        if !sv.rapid && r.volume > 0.0 && sv.rpm == Some(0.0) {
            ev("cut-spindle-off", Severity::Error, r.at, r.depth, "the tool cuts with the spindle stopped".into());
        }
        if r.above > tol {
            ev(
                "shank-rubs",
                Severity::Warning,
                r.above_at,
                r.above,
                format!("material stands {} mm above the flutes where the tool cuts: the shank rubs", mm(r.above)),
            );
        }
        let tool = self.tool(sv.slot).cloned();
        if let Some(tool) = tool {
            let model = self.setup.model.clone();
            let scene = self.setup.scene.clone();
            let table = scene.as_ref().map(|s| s.table);
            let (first, n, tips, turns) = step_tips(&self.plan, i, sv.known);
            // The workholding and the table, and the spindle's travel.
            if model.is_some() || scene.is_some() {
                let obstacles = scene.as_ref().map(|s| s.obstacles.as_slice()).unwrap_or(&[]);
                for h in clearance(&tool, model.as_deref(), table.as_ref(), obstacles, &tips, &turns, CHECK_STEP, tol) {
                    ev(&h.code, h.severity, h.at, 0.0, h.message);
                }
            }
            // The shank, the holder and the machine against the stock as it is now.
            match self.plan.grid() {
                Grid::Heights { .. } => {
                    let mut prev: Option<Vec3> = None;
                    let mut hits = 0usize;
                    'along: for &q in &tips {
                        let steps = prev.map_or(1, |a| ((a.dist(q) / CHECK_STEP).ceil() as usize).clamp(1, MAX_CHECKS));
                        for s in 1..=steps {
                            let p = prev.map_or(q, |a| a.lerp(q, s as f64 / steps as f64));
                            for zone in zones(&tool, model.as_deref(), table.as_ref(), p) {
                                if zone.of == ZoneOf::Cutter {
                                    continue;
                                }
                                if let Some(hit) = self.state.material_above(zone.fp, zone.z0 + tol) {
                                    let (code, what) = stock_hit(zone.of, &tool, model.as_deref());
                                    ev(&code, Severity::Error, hit, 0.0, format!("{what} comes down onto the stock"));
                                    hits += 1;
                                    if hits > 16 {
                                        break 'along;
                                    }
                                }
                            }
                        }
                        prev = Some(q);
                    }
                }
                Grid::Radii { .. } => {
                    // The tool's own envelopes, tilted with it, sampled round their bottoms.
                    let envelopes = tool.envelopes();
                    // The first envelope is the plain shank only when the tool stands out of
                    // its holder further than its flutes (as `zones` names them).
                    let shank = usize::from(tool.stickout.is_some_and(|s| s > tool.flute_length));
                    'poses: for s in first..=n {
                        let Some(pose) = self.plan.pose(i, s as f64 / n as f64) else { continue };
                        let side = pose.axis.cross(Vec3::new(1.0, 0.0, 0.0)).normalized().unwrap_or(Vec3::new(0.0, 1.0, 0.0));
                        let other = pose.axis.cross(side);
                        for (k, e) in envelopes.iter().enumerate() {
                            let c = pose.up(e.bottom + tol);
                            for a in 0..16 {
                                let (sa, ca) = (f64::from(a) * std::f64::consts::TAU / 16.0).sin_cos();
                                for rr in [0.0, 0.5, 1.0] {
                                    let p = c + side * (ca * e.radius * rr) + other * (sa * e.radius * rr);
                                    if self.state.contains(p) {
                                        let of = if k < shank { ZoneOf::Shank } else { ZoneOf::Holder(k - shank) };
                                        let (code, what) = stock_hit(of, &tool, None);
                                        ev(&code, Severity::Error, p, 0.0, format!("{what} comes down onto the stock"));
                                        continue 'poses;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        for e in found {
            self.log.push(e);
        }
    }
}

/// Where the tip goes along step `i` of `plan`, in work coordinates with the table rotaries at
/// zero, every [`CHECK_STEP`] mm: the first and last sample index and the tips. A move from an
/// unknown position (`known` false: down from above the stock) is checked at its end only.
fn step_tips(plan: &SimPlan, i: usize, known: bool) -> (usize, usize, Vec<Vec3>, Vec<Option<Turn>>) {
    let origin = plan.machine().work_origin;
    let n = plan.samples(i, CHECK_STEP, MAX_CHECKS).len().saturating_sub(1).max(1);
    let first = if known { 0 } else { n };
    let joints: Vec<_> = (first..=n).filter_map(|s| plan.joints(i, s as f64 / n as f64)).collect();
    let tips = joints.iter().map(|j| j.linear - origin).collect();
    // How the work (and what turns with it: a chuck's jaws) stands against the bed there.
    let turns = joints.iter().map(|j| work_turn_x(plan.machine(), &j.rotary)).collect();
    (first, n, tips, turns)
}

/// One kind of collision a program has with the workholding, the table or the machine's
/// travel, for one tool and one group of lines (an operation): where it first happens, how many
/// moves have it, and (for the spindle's travel) the highest the tip goes.
#[derive(Clone, Debug, PartialEq)]
pub struct HoldHit {
    /// The first time it happens (code, severity, move, line, place, message).
    pub first: FindingEvent,
    /// The tool's number.
    pub tool: u32,
    /// The group (`clearance_check`'s `group` of its first line).
    pub group: usize,
    /// Moves that have it.
    pub moves: usize,
    /// The highest the tip goes in those moves (work Z).
    pub highest: f64,
}

/// Every move of `plan` checked against the workholding (`scene`), the table and the spindle's
/// travel (`model`) the way a [`Playback`] checks it, without cutting the stock: the cutter,
/// shank, holder and the machine's parts against every obstacle, and the spindle nose against
/// the top of its travel. `tools` holds every tool of the program (with its holder). What a job
/// checks before its program is written; each code once per tool and `group` (of the move's
/// G-code line: the job's operation, so each operation that has it is named), at its first move.
pub fn clearance_check(
    plan: &SimPlan,
    tools: &[Tool],
    model: Option<&MachineModel>,
    scene: Option<&HoldScene>,
    group: &dyn Fn(usize) -> usize,
) -> Vec<HoldHit> {
    let mut out: Vec<HoldHit> = Vec::new();
    if model.is_none() && scene.is_none() {
        return out;
    }
    let tol = plan.tolerance();
    let table = scene.map(|s| s.table);
    let obstacles = scene.map(|s| s.obstacles.as_slice()).unwrap_or(&[]);
    for i in 0..plan.moves() {
        let Some(sv) = plan.step(i) else { continue };
        let Some(tool) = plan.tool_number(sv.slot).and_then(|n| tools.iter().find(|t| t.number == n)) else { continue };
        let (_, _, tips, turns) = step_tips(plan, i, sv.known);
        let high = tips.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max);
        let g = group(sv.line);
        for h in clearance(tool, model, table.as_ref(), obstacles, &tips, &turns, CHECK_STEP, tol) {
            match out.iter_mut().find(|o| o.first.code == h.code && o.tool == tool.number && o.group == g) {
                Some(o) => {
                    o.moves += 1;
                    o.highest = o.highest.max(high);
                }
                None => out.push(HoldHit {
                    first: FindingEvent {
                        code: h.code,
                        severity: h.severity,
                        at_move: Some(i),
                        line: sv.line,
                        op: None,
                        at: Some(h.at),
                        depth: 0.0,
                        message: h.message,
                    },
                    tool: tool.number,
                    group: g,
                    moves: 1,
                    highest: high,
                }),
            }
        }
    }
    out
}

/// A body the head check takes: its surface (work coordinates with the table rotaries at zero;
/// it turns with A), the code a hit ends with (`part`: `spindle-hits-part`) and what it is
/// called (`the part`).
pub struct HeadBody<'a> {
    pub surface: &'a Surface,
    pub code: &'a str,
    pub noun: &'a str,
}

// Tips [`head_check`] checked one at a time (the tests count its work).
#[cfg(test)]
thread_local! {
    pub(crate) static HEAD_TIPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The farthest any of `bodies` reaches from the A axis through `center` (Y, Z), whichever way
/// it is turned about it: the farthest corner of its box, across Y and Z.
fn turned_reach(bodies: &[HeadBody<'_>], center: [f64; 2]) -> f64 {
    bodies
        .iter()
        .map(|b| {
            let bb = b.surface.bounds();
            let y = (bb.min.y - center[0]).abs().max((bb.max.y - center[0]).abs());
            let z = (bb.min.z - center[1]).abs().max((bb.max.z - center[1]).abs());
            y.hypot(z)
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Every move of `plan` checked for the machine's parts that move with the tool (the spindle
/// head's body and the carriage: the machine model's exact parts) against `bodies` (the part,
/// the design's fixtures), as the holder check does for the tool's own holder; the workholding
/// and the table are [`clearance_check`]'s. Each code (`<part code>-hits-<body code>`) once per
/// tool and `group`, at its first move.
pub fn head_check(
    plan: &SimPlan,
    tools: &[Tool],
    model: &MachineModel,
    scene: Option<&HoldScene>,
    bodies: &[HeadBody<'_>],
    group: &dyn Fn(usize) -> usize,
) -> Vec<HoldHit> {
    use crate::machine_model::{Mount, zone_box, zone_meets_triangle};
    let mut out: Vec<HoldHit> = Vec::new();
    let table = scene.map(|s| s.table);
    let moving = |of: ZoneOf| match of {
        ZoneOf::Part(k) => model.parts.get(k).filter(|p| matches!(p.mount, Mount::Spindle | Mount::Carriage)),
        _ => None,
    };
    let top = bodies.iter().map(|b| b.surface.bounds().max.z).fold(f64::NEG_INFINITY, f64::max);
    for i in 0..plan.moves() {
        let Some(sv) = plan.step(i) else { continue };
        let Some(tool) = plan.tool_number(sv.slot).and_then(|n| tools.iter().find(|t| t.number == n)) else { continue };
        let (_, _, ends, end_turns) = step_tips(plan, i, sv.known);
        let high = ends.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max);
        let g = group(sv.line);
        // Quick reject: the lowest the moving parts come (at the lowest tip) is above every body,
        // as it stands or, the work turned, turned any way about A (no higher than the axis plus
        // the farthest the body's box reaches from it).
        let top = end_turns.iter().flatten().filter(|t| t.angle != 0.0).fold(top, |m, t| m.max(t.center[1] + turned_reach(bodies, t.center)));
        if let Some(low) = ends.iter().copied().reduce(|a, b| if b.z < a.z { b } else { a })
            && zones(tool, Some(model), table.as_ref(), low).iter().filter(|z| moving(z.of).is_some()).all(|z| z.z0 >= top)
        {
            continue;
        }
        // Every CHECK_STEP mm between the samples, each with the turn of the sample it leads to.
        let mut tips: Vec<(Vec3, Option<Turn>)> = Vec::new();
        for (k, &q) in ends.iter().enumerate() {
            let turn = if end_turns.len() == ends.len() { end_turns.get(k).copied().flatten().filter(|t| t.angle != 0.0) } else { None };
            let from = k.checked_sub(1).and_then(|j| ends.get(j)).copied();
            let steps = from.map_or(1, |a| ((a.dist(q) / CHECK_STEP).ceil() as usize).clamp(1, MAX_CHECKS));
            for s in 1..=steps {
                tips.push((from.map_or(q, |a| a.lerp(q, s as f64 / steps as f64)), turn));
            }
        }
        // Each code once a move.
        let mut found: Vec<(String, Vec3, String)> = Vec::new();
        for &(p, turn) in &tips {
            #[cfg(test)]
            HEAD_TIPS.with(|n| n.set(n.get() + 1));
            for zone in zones(tool, Some(model), table.as_ref(), p) {
                let Some(part) = moving(zone.of) else { continue };
                let (lo, hi) = zone_box(&zone);
                // The zone's box where the body is (turned back by A).
                let (qlo, qhi) = match turn {
                    Some(t) => {
                        let back = Turn { angle: -t.angle, center: t.center };
                        let mut a = Vec3::new(lo.x, f64::INFINITY, f64::INFINITY);
                        let mut b = Vec3::new(hi.x, f64::NEG_INFINITY, f64::NEG_INFINITY);
                        for (y, z) in [(lo.y, lo.z), (lo.y, hi.z), (hi.y, lo.z), (hi.y, hi.z)] {
                            let [ty, tz] = back.apply([y, z]);
                            a = Vec3::new(a.x, a.y.min(ty), a.z.min(tz));
                            b = Vec3::new(b.x, b.y.max(ty), b.z.max(tz));
                        }
                        (a, b)
                    }
                    None => (lo, hi),
                };
                for body in bodies {
                    let code = format!("{}-hits-{}", part.code, body.code);
                    if found.iter().any(|f| f.0 == code) {
                        continue;
                    }
                    let bb = body.surface.bounds();
                    let near = |b: &solidcraft_geom::Aabb3| {
                        b.min.x < qhi.x && b.max.x > qlo.x && b.min.y < qhi.y && b.max.y > qlo.y && b.min.z < qhi.z && b.max.z > qlo.z
                    };
                    if !near(&bb) {
                        continue;
                    }
                    // Turned, the body stays within its reach of the A axis: a zone farther
                    // from the axis (across Y and Z) cannot meet it.
                    if let Some(t) = turn {
                        let dy = (lo.y - t.center[0]).max(t.center[0] - hi.y).max(0.0);
                        let dz = (lo.z - t.center[1]).max(t.center[1] - hi.z).max(0.0);
                        if dy.hypot(dz) >= turned_reach(std::slice::from_ref(body), t.center) {
                            continue;
                        }
                    }
                    let hit = std::cell::Cell::new(false);
                    body.surface.visit(
                        |b| !hit.get() && near(b),
                        |_, t| {
                            if hit.get() {
                                return;
                            }
                            let t = match turn {
                                Some(tn) => t.map(|v| {
                                    let [y, z] = tn.apply([v.y, v.z]);
                                    Vec3::new(v.x, y, z)
                                }),
                                None => *t,
                            };
                            hit.set(zone_meets_triangle(&zone, &t));
                        },
                    );
                    if hit.get() {
                        found.push((code, Vec3::new(p.x, p.y, zone.z0), format!("{} runs into {}", part.name, body.noun)));
                    }
                }
            }
        }
        for (code, at, message) in found {
            match out.iter_mut().find(|o| o.first.code == code && o.tool == tool.number && o.group == g) {
                Some(o) => {
                    o.moves += 1;
                    o.highest = o.highest.max(high);
                }
                None => out.push(HoldHit {
                    first: FindingEvent {
                        code,
                        severity: Severity::Error,
                        at_move: Some(i),
                        line: sv.line,
                        op: None,
                        at: Some(at),
                        depth: 0.0,
                        message,
                    },
                    tool: tool.number,
                    group: g,
                    moves: 1,
                    highest: high,
                }),
            }
        }
    }
    out
}

/// The finding code and subject of zone `of` meeting the stock.
fn stock_hit(of: ZoneOf, tool: &Tool, model: Option<&MachineModel>) -> (String, String) {
    match of {
        ZoneOf::Cutter => ("cutter-hits-stock".into(), "the cutter".into()),
        ZoneOf::Shank => ("shank-hits-stock".into(), format!("the tool's shank (Ø{} mm)", number(tool.shank_diameter.unwrap_or(tool.diameter), 3))),
        ZoneOf::Holder(k) => {
            ("holder-hits-stock".into(), format!("the tool's holder (Ø{} mm)", number(tool.holder.get(k).map_or(0.0, |h| h.diameter), 3)))
        }
        ZoneOf::Part(k) => match model.and_then(|m| m.parts.get(k)) {
            Some(p) => (format!("{}-hits-stock", p.code), p.name.clone()),
            None => ("machine-hits-stock".into(), "a machine part".into()),
        },
    }
}

/// A footprint's check in one call, for tests and views: material above `z` inside `fp`.
pub fn stock_above(state: &mut SimState, fp: Footprint, z: f64) -> Option<Vec3> {
    state.material_above(fp, z)
}

/// Refuses a seek target that is not a whole number of moves within the program.
pub fn check_move(m: f64, moves: usize) -> Result<usize> {
    if !(m.is_finite() && m >= 0.0 && m.fract() == 0.0) {
        return invalid("a move is a whole number from 0");
    }
    if m as usize >= moves.max(1) {
        return Err(CamError::Invalid(format!("the program has {moves} moves (0 to {})", moves.saturating_sub(1))));
    }
    Ok(m as usize)
}
