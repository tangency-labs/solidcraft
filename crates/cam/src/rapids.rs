//! Rapid safety and travel of a written program: every `G0` block as the controller moves
//! through it, held against the part, the fixtures and the stock, and every position against
//! the machine's travel.
//!
//! The program is read with [`parse5`] and the machine is moved through each rapid block as the
//! controller would (every axis evenly from the block's start to its end), sampled every
//! `step` of the tip's travel and of a point at the tool's reach. At each sample, through
//! [`forward`] into work coordinates:
//!
//! - the cutter, as a cylinder of its radius without end above its tip (the shank and spindle
//!   are there too; a drill's end is its point), must not be in the part or a fixture by more
//!   than the tolerance (the gouge test of [`crate::collision`]), nor the tool's shank and
//!   holder touch them;
//! - with a stock, the cutter must not be in the stock (as given: what earlier cuts removed is
//!   not tracked, so a rapid below the stock's top over it fails), except on a retract straight
//!   up the tool's axis, which only passes through room the tool already fills. The caller
//!   may say what is left of it as each operation starts ([`check_rapids_staged`]: a box
//!   faced all over is lower after the facing). Nor does the stock count against a rapid
//!   back down the same line within the same operation, no deeper than that retract started
//!   (a peck drill going back into its hole). The part does not count against such a
//!   retract either when the block's operation cuts what the model does not show on purpose
//!   (`unchecked`, an operation without a gouge check: a deburr of a sharp-edged model): it
//!   leaves the cut where its own feed moves ended;
//! - a block that turns the rotary of a 4th axis (a table about X) must keep the cutter
//!   outside the radius the stock sweeps, all through the turn, wherever the stock is level with
//!   it along X. Without a stock the part is cut into [`TURN_SLICES`] slices along X, each held
//!   to the largest radius of the part's facets over it and the tolerance, so a slender section
//!   is not held to the radius of the part's widest one.
//!
//! A block after a return to the machine's home (`G28`, `G30`), a move in machine coordinates
//! (`G53`) or a tool change starts from where the reader cannot follow: only its end is
//! checked, which with the cutter endless above covers the way down from the machine's top.
//! Rotaries turned there (a post's safe rotate) are taken to turn at the machine's top, clear
//! of the work, and a tool change to be made where the new tool clears everything (at the
//! machine's top, where the post's safe retract takes it).
//!
//! The tool of each block is found by writing the program again with a marker at the start of
//! every operation: the marked program must move exactly as the real one, and the marker before
//! a block names its operation. Should that fail, every block is checked with every tool.

use serde::{Deserialize, Serialize};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::check::Swept;
use crate::collision::{Moves, gouges_in, holder_collisions_in};
use crate::cutter::Cutter;
use crate::gcode::words;
use crate::gcode5::{JointWords, MachineOperation, MachineProgram, Post5, emit_program, parse5};
use crate::kinematics::{Carrier, Joints, Machine, forward};
use crate::multiaxis::{Move5, Pose, Toolpath5};
use crate::stock::{Stock, axis_intrusion};
use crate::surface::Surface;
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// The default distance between the samples of a rapid (mm).
pub const RAPID_STEP: f64 = 0.5;
/// Most samples along the whole program, and along one block.
const MAX_SAMPLES: usize = 20_000_000;
const MAX_BLOCK_SAMPLES: usize = 2_000_000;
/// Most hits kept in full (all are counted).
const MAX_HITS_KEPT: usize = 1000;
/// The line written at the start of every operation of the marked program.
/// Slices of the part along X for the turn check without a stock.
pub const TURN_SLICES: usize = 128;
const MARK: &str = "(SOLIDCRAFT RAPID CHECK OPERATION START)";

/// What a rapid runs into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RapidHitKind {
    /// The cutter in the part or a fixture.
    Part,
    /// The tool's shank or holder touching the part or a fixture.
    Holder,
    /// The cutter in the stock.
    Stock,
    /// A rotary turning the work into the tool.
    Turn,
}

impl RapidHitKind {
    pub fn label(self) -> &'static str {
        match self {
            RapidHitKind::Part => "through the part or a fixture",
            RapidHitKind::Holder => "the shank or holder through the part or a fixture",
            RapidHitKind::Stock => "through the stock",
            RapidHitKind::Turn => "turns the work into the tool",
        }
    }
}

/// A rapid block that runs into something.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RapidHit {
    /// The program line (from 1).
    pub line: usize,
    /// The operation it belongs to (index into the program's operations), when known.
    pub operation: Option<usize>,
    pub kind: RapidHitKind,
    /// How deep, at its deepest sample (mm): how far the cutter is in the part or stock, how far
    /// the holder overlaps, how far inside the swept radius the cutter is during a turn.
    pub depth: f64,
    /// The tip there, work coordinates.
    pub at: Vec3,
}

/// What [`check_rapids`] found.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RapidCheck {
    /// Rapid blocks checked.
    pub checked: usize,
    /// Samples taken along them.
    pub samples: usize,
    /// Rapid blocks that run into something, counting each block once per kind.
    pub hit_count: usize,
    /// The first 1000 of them.
    pub hits: Vec<RapidHit>,
    /// The deepest of them (mm; 0 with none).
    pub worst: f64,
    /// Rapid blocks that start where the reader cannot follow the machine (after a return to
    /// its home, a move in machine coordinates or a tool change), of which only the end is checked.
    pub from_unknown: usize,
    /// Whether every block's tool was known (otherwise every block was checked with every tool).
    pub tools_known: bool,
}

impl RapidCheck {
    pub fn pass(&self) -> bool {
        self.hit_count == 0
    }

    fn hit(&mut self, hit: RapidHit) {
        self.hit_count += 1;
        self.worst = self.worst.max(hit.depth);
        if self.hits.len() < MAX_HITS_KEPT {
            self.hits.push(hit);
        }
    }
}

/// Where the axes are after a block.
#[derive(Clone, Debug)]
struct State {
    linear: Vec3,
    rotary: Vec<f64>,
    line: usize,
    /// The number of the tool in the spindle, when known.
    tool: Option<u32>,
}

fn tool_of(op: &MachineOperation) -> &Tool {
    match op {
        MachineOperation::Continuous(o) => &o.tool,
        MachineOperation::Indexed(o) => &o.tool,
    }
}

fn pose_at(machine: &Machine, linear: Vec3, rotary: &[f64]) -> Pose {
    forward(machine, &Joints { linear, rotary: rotary.to_vec() })
}

/// Whether line `line` returns to the machine's home or moves in machine coordinates.
fn homes(line: &str) -> bool {
    words(line, 0).is_ok_and(|ws| ws.iter().any(|&(c, v)| c == 'G' && matches!((v * 10.0).round() as i64, 280 | 300 | 530)))
}

fn same_motion(a: &JointWords, b: &JointWords) -> bool {
    match (a, b) {
        (JointWords::Rapid { linear: l1, rotary: r1, .. }, JointWords::Rapid { linear: l2, rotary: r2, .. })
        | (JointWords::Feed { linear: l1, rotary: r1, .. }, JointWords::Feed { linear: l2, rotary: r2, .. })
        | (JointWords::Arc { linear: l1, rotary: r1, .. }, JointWords::Arc { linear: l2, rotary: r2, .. })
        | (JointWords::PlaneArc { linear: l1, rotary: r1, .. }, JointWords::PlaneArc { linear: l2, rotary: r2, .. }) => l1 == l2 && r1 == r2,
        (JointWords::Dwell { seconds: s1, .. }, JointWords::Dwell { seconds: s2, .. }) => s1 == s2,
        _ => false,
    }
}

fn line_of(b: &JointWords) -> usize {
    b.line()
}

/// The operation of every block of `blocks` (read from the program as written), from the
/// program written again with a marker before every operation; `None` when the two do not move
/// alike. A view uses it to put every block of a job's program under its operation.
pub fn owners(program: &MachineProgram, post: &Post5, machine: &Machine, blocks: &[JointWords]) -> Option<Vec<usize>> {
    let mut marked = post.clone();
    marked.base.operation_start.insert(0, MARK.into());
    let text = emit_program(program, &marked, machine).ok()?;
    let again = parse5(&text, &machine.letters()).ok()?;
    if again.len() != blocks.len() || !again.iter().zip(blocks).all(|(a, b)| same_motion(a, b)) {
        return None;
    }
    let marks: Vec<usize> = text.lines().enumerate().filter(|(_, l)| l.contains(MARK)).map(|(i, _)| i + 1).collect();
    if marks.len() != program.operations.len() {
        return None;
    }
    Some(again.iter().map(|b| marks.partition_point(|&m| m < line_of(b)).saturating_sub(1)).collect())
}

/// What a 4th axis turn must keep the cutter out of: the line of the table about X (its Y and Z
/// in work coordinates), the radius it sweeps and its extent along X.
#[derive(Clone, Copy, Debug)]
struct TurnZone {
    center: Vec2,
    radius: f64,
    x: (f64, f64),
}

fn turn_zones(machine: &Machine, part: &Surface, stock: Option<&Stock>, tol: f64) -> Vec<TurnZone> {
    let [axis] = machine.rotaries.as_slice() else { return Vec::new() };
    if axis.carrier != Carrier::Table || axis.direction.normalized().map_or(0.0, |d| d.x.abs()) < 1.0 - 1e-9 {
        return Vec::new();
    }
    let c = axis.center - machine.work_origin;
    let center = Vec2::new(c.y, c.z);
    if let Some(s) = stock {
        return vec![TurnZone { center, radius: s.radius_about_x(center) - tol, x: s.x_range() }];
    }
    let b = part.bounds();
    if b.is_empty() || !b.min.x.is_finite() || !b.max.x.is_finite() {
        return Vec::new();
    }
    // Each facet's farthest point from the axis is one of its corners (the distance from a
    // line is convex), and it counts in every slice its X range meets.
    let (x0, x1) = (b.min.x, b.max.x);
    let slices = if x1 - x0 > tol { TURN_SLICES } else { 1 };
    let width = (x1 - x0) / slices as f64;
    let slice_of = |x: f64| if width > 0.0 { (((x - x0) / width).floor().max(0.0) as usize).min(slices - 1) } else { 0 };
    let mut radius = vec![0.0f64; slices];
    for t in part.triangles() {
        let (lo, hi) = t.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
        let far = t.iter().map(|p| (p.y - center.x).hypot(p.z - center.y)).fold(0.0, f64::max);
        let (i, j) = (slice_of(lo), slice_of(hi));
        for r in radius.iter_mut().skip(i).take(j.saturating_sub(i) + 1) {
            *r = r.max(far);
        }
    }
    radius
        .iter()
        .enumerate()
        .filter(|(_, r)| **r > 0.0)
        .map(|(k, r)| {
            let lo = x0 + width * k as f64;
            let hi = if k + 1 == slices { x1 } else { x0 + width * (k + 1) as f64 };
            TurnZone { center, radius: r + tol, x: (lo - tol, hi + tol) }
        })
        .collect()
}

/// The farthest any point of `tool` that is checked lies from its tip (mm).
fn reach(tool: &Tool) -> f64 {
    tool.envelopes().iter().map(|e| e.top).fold(tool.flute_length, f64::max) + tool.radius()
}

/// The cutter a rapid is checked with: the tool's, or for a drill its point (a cone of its
/// point angle up to its diameter), so a drill rapided out of and back into the cone floor it
/// cut is held to its own shape there, not to a flat end of its diameter.
fn rapid_cutter(tool: &Tool) -> Result<Cutter> {
    if crate::drill::tip_length(tool) > 0.0
        && let Ok(c) = Cutter::tapered(tool.radius(), 0.0, crate::drill::point_angle(tool) * 0.5)
    {
        return Ok(c);
    }
    tool.cutter().or_else(|_| Cutter::flat(tool.radius()))
}

/// Checks every rapid of `text` (written from `program` through `post` for `machine`) against
/// `obstacles` (the part and the fixtures), `stock` and, for 4th-axis turns, the radius of the
/// stock or of `part`; see the module notes. `tol` is how deep the cutter may be before it
/// counts, `step` the distance between samples.
#[allow(clippy::too_many_arguments)]
pub fn check_rapids(
    text: &str,
    program: &MachineProgram,
    post: &Post5,
    machine: &Machine,
    obstacles: &Surface,
    part: &Surface,
    stock: Option<&Stock>,
    tol: f64,
    step: f64,
) -> Result<RapidCheck> {
    check_rapids_staged(text, program, post, machine, obstacles, part, stock, &[], &[], tol, step)
}

/// [`check_rapids`], with what is left of the stock as each operation starts: `stages[k]` is
/// the stock the rapids of operation `k` are held against (one per operation, or none at all
/// for `stock` throughout). A block of no known operation, and every 4th-axis turn, is held
/// against `stock`, the stock as the job starts. `unchecked[k]` says operation `k` cuts what
/// the model does not show on purpose (one per operation, or none at all): its retracts
/// straight up the tool's axis are not held against the part.
#[allow(clippy::too_many_arguments)]
pub fn check_rapids_staged(
    text: &str,
    program: &MachineProgram,
    post: &Post5,
    machine: &Machine,
    obstacles: &Surface,
    part: &Surface,
    stock: Option<&Stock>,
    stages: &[Option<Stock>],
    unchecked: &[bool],
    tol: f64,
    step: f64,
) -> Result<RapidCheck> {
    post.validate()?;
    machine.validate()?;
    if !(tol >= 0.0) || !(tol <= 1.0) {
        return invalid("the rapid check's tolerance must be from 0 to 1 mm");
    }
    if !(step >= 0.01) || !(step <= 100.0) {
        return invalid("the rapid check's sample step must be from 0.01 to 100 mm");
    }
    if let Some(s) = stock {
        s.validate()?;
    }
    if !stages.is_empty() && stages.len() != program.operations.len() {
        return invalid("the stock's stages must be one per operation");
    }
    if !unchecked.is_empty() && unchecked.len() != program.operations.len() {
        return invalid("the operations without a gouge check must be given one per operation");
    }
    for s in stages.iter().flatten() {
        s.validate()?;
    }
    if program.operations.is_empty() {
        return Ok(RapidCheck { tools_known: true, ..RapidCheck::default() });
    }
    let blocks = solidcraft_profile::scope("cam", "rapids.parse", || parse5(text, &machine.letters()))?;
    let owner = solidcraft_profile::scope("cam", "rapids.owners", || owners(program, post, machine, &blocks));
    let mut out = RapidCheck { tools_known: owner.is_some(), ..RapidCheck::default() };
    let tools: Vec<&Tool> = program.operations.iter().map(tool_of).collect();
    let cutters: Vec<Cutter> = tools.iter().map(|t| rapid_cutter(t)).collect::<Result<_>>()?;
    // Every operation's index, but one per tool number: what a block of no known operation is
    // checked with.
    let mut every: Vec<usize> = Vec::new();
    for (i, t) in tools.iter().enumerate() {
        if !every.iter().any(|&k| tools.get(k).is_some_and(|u| u.number == t.number)) {
            every.push(i);
        }
    }
    let zones = turn_zones(machine, part, stock, tol);
    let lever = machine.work_origin.len() + machine.rotaries.iter().map(|r| r.center.len()).sum::<f64>();
    // Lines that return home, counted up to each line, so a gap between two blocks is seen at once.
    let mut homed = vec![0usize];
    for l in text.lines() {
        let last = homed.last().copied().unwrap_or(0);
        homed.push(last + usize::from(homes(l)));
    }
    let homes_between = |a: usize, b: usize| homed.get(b.saturating_sub(1)).zip(homed.get(a)).is_some_and(|(x, y)| x > y);

    // The tool number of block `i`, when its operation is known.
    let tool_at = |i: usize| owner.as_ref().and_then(|o| o.get(i)).and_then(|&k| tools.get(k)).map(|t| t.number);
    let walk = solidcraft_profile::span("cam", "rapids.walk");
    let mut state: Option<State> = None;
    // The operation and the start of its last retract straight up the tool's axis, which a
    // rapid may go back down along as far as it started (a peck drill's way back into its hole).
    let mut withdrawn: Option<(usize, Pose)> = None;
    // Each operation's straight cutting moves so far with the tool upright, in work
    // coordinates: a rapid straight down to above where one of them went through that point
    // comes down through what that operation has already cut (a level of a pocket or a
    // roughing coming down through the air the levels before cleared).
    // Filed in a frame whose Z is the tool's axis of the operation's first such move (an
    // indexed operation at any angle, not only at A0).
    let mut cut: Vec<Option<(AxisFrame, Swept)>> = program.operations.iter().map(|_| None).collect();
    // What the blocks found, in their order: the blocks' own checks are made after the walk
    // through the program, side by side ([`crate::par`]), and their hits taken in this order.
    let mut found: Vec<Found<'_>> = Vec::new();
    // With the tool always along machine Z (rotaries on the table only), a rapid from an unknown
    // place (after a return home) comes down its own axis to where it ends.
    let table_only = machine.rotaries.iter().all(|r| r.carrier == Carrier::Table);
    for (i, b) in blocks.iter().enumerate() {
        let (line, linear, rotary, rapid) = match b {
            JointWords::Rapid { line, linear, rotary } => (*line, *linear, rotary, true),
            JointWords::Feed { line, linear, rotary, .. }
            | JointWords::Arc { line, linear, rotary, .. }
            | JointWords::PlaneArc { line, linear, rotary, .. } => (*line, *linear, rotary, false),
            JointWords::Dwell { .. } => continue,
        };
        let to = State { linear, rotary: rotary.clone(), line, tool: tool_at(i) };
        if rapid {
            // A tool change may leave the tool anywhere (the post's own lines take it to the
            // machine's top, where a post has them).
            let from = state.as_ref().filter(|s| !homes_between(s.line, line) && s.tool == to.tool);
            if from.is_none() && state.is_some() {
                out.from_unknown += 1;
            }
            let ops: Vec<usize> = match owner.as_ref().and_then(|o| o.get(i)) {
                Some(&k) => vec![k],
                None => every.clone(),
            };
            out.checked += 1;
            for k in ops {
                let (Some(tool), Some(cutter)) = (tools.get(k), cutters.get(k)) else { continue };
                let poses = samples(machine, from, &to, reach(tool) + lever, step, &mut out.samples)?;
                // What is left of the stock as the block's operation starts, where that is known.
                let left = match (&owner, stages.get(k)) {
                    (Some(_), Some(s)) => s.as_ref(),
                    _ => stock,
                };
                let deliberate = owner.is_some() && unchecked.get(k).copied().unwrap_or(false);
                // Back down the way it went out: also after a return home and a whole turn of a
                // rotary in between (a rewind), where only the end of the way down is known.
                let back_in = owner.is_some()
                    && withdrawn.as_ref().is_some_and(|(w, bottom)| {
                        *w == k && (back_down(&poses, bottom) || (from.is_none() && table_only && on_way_out(&poses, bottom)))
                    });
                let through_cut = owner.is_some() && cut.get(k).and_then(Option::as_ref).is_some_and(|(f, c)| down_into(&poses, f, c));
                let left = if back_in || through_cut { None } else { left };
                if owner.is_some() && retract(&poses) {
                    withdrawn = poses.first().map(|p| (k, *p));
                }
                let turns = from.is_some_and(|f| f.rotary.iter().zip(&to.rotary).any(|(a, b)| (a - b).abs() > 1e-9));
                if turns {
                    let deepest = zones
                        .iter()
                        .flat_map(|z| {
                            poses.iter().filter_map(move |p| axis_intrusion(p, tool.radius(), z.center, z.radius, z.x.0, z.x.1).map(|d| (d, p.tip)))
                        })
                        .reduce(|a, b| if b.0 > a.0 { b } else { a });
                    found.push(Found::Block(Block { poses, at: (line, owner.as_ref().map(|_| k)), tool, cutter, stock: left, deliberate }));
                    if let Some((depth, at)) = deepest {
                        found.push(Found::Hit(RapidHit { line, operation: owner.as_ref().map(|_| k), kind: RapidHitKind::Turn, depth, at }));
                    }
                } else {
                    found.push(Found::Block(Block { poses, at: (line, owner.as_ref().map(|_| k)), tool, cutter, stock: left, deliberate }));
                }
            }
        }
        if !rapid
            && let (Some(&k), Some(from)) = (owner.as_ref().and_then(|o| o.get(i)), state.as_ref())
            && let Some(c) = cut.get_mut(k)
        {
            // An arc is filed by its ends: its chord is not where it cut.
            let arc = matches!(b, JointWords::Arc { .. } | JointWords::PlaneArc { .. });
            let (a, end) = (pose_at(machine, from.linear, &from.rotary), pose_at(machine, to.linear, &to.rotary));
            let (f, c) = c.get_or_insert_with(|| (AxisFrame::along(a.axis), Swept::default()));
            if f.holds(&a) && f.holds(&end) {
                let (p, q) = (f.local(a.tip), f.local(end.tip));
                if arc {
                    c.add(p, p);
                    c.add(q, q);
                } else {
                    c.add(p, q);
                }
            }
        }
        state = Some(to);
    }
    drop(walk);
    let _span = solidcraft_profile::span("cam", "rapids.blocks");
    let blocks: Vec<&Block<'_>> = found.iter().filter_map(|f| if let Found::Block(b) = f { Some(b) } else { None }).collect();
    let weights: Vec<usize> = blocks.iter().map(|b| b.poses.len()).collect();
    let ranges = crate::par::split(&weights, crate::par::threads_for(weights.iter().sum(), BLOCK_SAMPLES_PER_THREAD));
    let checked: Vec<Vec<Vec<RapidHit>>> =
        crate::par::map(ranges, |r| blocks.get(r).unwrap_or(&[]).iter().map(|b| check_block(b, obstacles, tol, step)).collect::<Result<Vec<_>>>())?;
    let mut checked = checked.into_iter().flatten();
    for f in found {
        match f {
            Found::Block(_) => checked.next().into_iter().flatten().for_each(|h| out.hit(h)),
            Found::Hit(h) => out.hit(h),
        }
    }
    Ok(out)
}

/// A frame whose Z is a tool axis, for the cutting moves along that axis.
struct AxisFrame {
    x: Vec3,
    y: Vec3,
    z: Vec3,
}

impl AxisFrame {
    fn along(axis: Vec3) -> AxisFrame {
        // Work X and Y where the axis is work Z, so an upright operation is filed as seen from
        // above.
        let z = axis.normalized().unwrap_or(Vec3::Z);
        let x =
            if z.x.abs() < 0.9 { (Vec3::X - z * z.x).normalized().unwrap_or(Vec3::X) } else { (Vec3::Y - z * z.y).normalized().unwrap_or(Vec3::Y) };
        AxisFrame { x, y: z.cross(x), z }
    }

    /// Whether the tool's axis at `p` is this frame's Z.
    fn holds(&self, p: &Pose) -> bool {
        p.axis.dot(self.z) >= 1.0 - 1e-9
    }

    fn local(&self, v: Vec3) -> Vec3 {
        Vec3::new(v.dot(self.x), v.dot(self.y), v.dot(self.z))
    }
}

/// Whether `poses` (a rapid's samples) run straight down the tool axis of `frame`, ending above
/// where a cutting move of `cut` (filed in `frame`) went through that point.
fn down_into(poses: &[Pose], frame: &AxisFrame, cut: &Swept) -> bool {
    let (Some(a), Some(b)) = (poses.first(), poses.last()) else { return false };
    let (la, lb) = (frame.local(a.tip), frame.local(b.tip));
    poses.len() > 1
        && lb.z < la.z
        && poses.iter().all(|p| {
            let l = frame.local(p.tip);
            frame.holds(p) && (l.x - la.x).hypot(l.y - la.y) <= 1e-6
        })
        && cut.below(Vec2::new(lb.x, lb.y), lb.z)
}

/// Whether `poses` (the known end of a rapid from an unknown place) stand on the line a retract
/// from `bottom` went out along, with its axis, no lower than where it started.
fn on_way_out(poses: &[Pose], bottom: &Pose) -> bool {
    !poses.is_empty()
        && poses.iter().all(|p| {
            p.axis_angle(bottom) <= 1e-9 && (p.tip - bottom.tip).cross(bottom.axis).len() <= 1e-6 && (p.tip - bottom.tip).dot(bottom.axis) >= -1e-9
        })
}

/// The poses along a rapid from `from` (the block's start; `None` where it is not known: only
/// its end then) to `to`, at most `step` apart for the tip and for a point `reach` up the tool.
fn samples(machine: &Machine, from: Option<&State>, to: &State, reach: f64, step: f64, total: &mut usize) -> Result<Vec<Pose>> {
    let end = pose_at(machine, to.linear, &to.rotary);
    let Some(from) = from else {
        *total = total.saturating_add(1);
        return Ok(vec![end]);
    };
    let start = pose_at(machine, from.linear, &from.rotary);
    let a = Joints { linear: from.linear, rotary: from.rotary.clone() };
    let b = Joints { linear: to.linear, rotary: to.rotary.clone() };
    let turn = a.rotary_change(&b);
    let lever = start.tip.len().max(end.tip.len()) + reach;
    let length = from.linear.dist(to.linear) + start.tip.dist(end.tip) + turn * lever;
    let n = (length / step).ceil().max(1.0);
    if !(n <= MAX_BLOCK_SAMPLES as f64) {
        return Err(CamError::TooLarge(format!("a rapid of the program needs more than {MAX_BLOCK_SAMPLES} samples")));
    }
    let n = n as usize;
    *total = total.saturating_add(n + 1);
    if *total > MAX_SAMPLES {
        return Err(CamError::TooLarge(format!("the program's rapids need more than {MAX_SAMPLES} samples")));
    }
    Ok((0..=n)
        .map(|k| {
            let j = a.lerp(&b, k as f64 / n as f64);
            pose_at(machine, j.linear, &j.rotary)
        })
        .collect())
}

/// Samples of rapids below which one thread checks them all (a sample of a turning rapid holds
/// the tool against the part along any axis, which is slow).
const BLOCK_SAMPLES_PER_THREAD: usize = 64;

/// A rapid block to check: its samples, its line and operation, the tool, and what is left of
/// the stock (`None`: not checked against it).
struct Block<'a> {
    poses: Vec<Pose>,
    at: (usize, Option<usize>),
    tool: &'a Tool,
    cutter: &'a Cutter,
    stock: Option<&'a Stock>,
    deliberate: bool,
}

/// What the walk through a program finds, in order: a block to check, or a hit already found.
enum Found<'a> {
    Block(Block<'a>),
    Hit(RapidHit),
}

/// The hits of one rapid block (`b.at`: its line and the operation whose tool it is, when known)
/// against `obstacles` and the stock, in the order they count.
fn check_block(b: &Block<'_>, obstacles: &Surface, tol: f64, step: f64) -> Result<Vec<RapidHit>> {
    let mut out = RapidCheck::default();
    let (poses, tool, cutter, stock, deliberate) = (b.poses.as_slice(), b.tool, b.cutter, b.stock, b.deliberate);
    let (line, op) = b.at;
    let path = Toolpath5 { moves: poses.iter().map(|&to| Move5::Rapid { to }).collect() };
    // A retract out of a cut the model does not show leaves it the way it went in.
    if !(deliberate && retract(poses)) {
        let hits = solidcraft_profile::scope("cam", "rapids.block_gouge", || gouges_in(obstacles, cutter, &path, tol, step, Moves::All))?;
        if let Some(h) = hits.iter().max_by(|a, b| a.distance.total_cmp(&b.distance)) {
            out.hit(RapidHit { line, operation: op, kind: RapidHitKind::Part, depth: h.distance, at: h.pose.tip });
        }
    }
    if !tool.envelopes().is_empty() {
        // A shank no wider than the cutter may come as close as the cutter may (`tol`).
        let hits =
            solidcraft_profile::scope("cam", "rapids.block_holder", || holder_collisions_in(obstacles, tool, &path, 0.0, tol, step, Moves::All))?;
        if let Some(h) = hits.iter().min_by(|a, b| a.distance.total_cmp(&b.distance)) {
            out.hit(RapidHit { line, operation: op, kind: RapidHitKind::Holder, depth: (-h.distance).max(0.0), at: h.pose.tip });
        }
    }
    // Straight up the tool's axis out of where it is (a retract out of the cut), the tool only
    // passes through room it already fills: the stock does not count against it.
    if let Some(s) = stock.filter(|_| !retract(poses)) {
        let deepest =
            poses.iter().filter_map(|p| s.intrusion(p, tool.radius(), tol).map(|d| (d, p.tip))).reduce(|a, b| if b.0 > a.0 { b } else { a });
        if let Some((depth, at)) = deepest {
            out.hit(RapidHit { line, operation: op, kind: RapidHitKind::Stock, depth, at });
        }
    }
    Ok(out.hits)
}

/// Whether `poses` (a rapid's samples) run straight back down the tool axis of `bottom` (the
/// start of a retract), along its line and no deeper than it.
fn back_down(poses: &[Pose], bottom: &Pose) -> bool {
    let (Some(a), Some(b)) = (poses.first(), poses.last()) else { return false };
    poses.len() > 1
        && poses.iter().all(|p| p.axis_angle(bottom) <= 1e-9 && (p.tip - bottom.tip).cross(bottom.axis).len() <= 1e-6)
        && (b.tip - a.tip).dot(bottom.axis) < 0.0
        && (b.tip - bottom.tip).dot(bottom.axis) >= -1e-9
}

/// Whether `poses` (a rapid's samples) run straight out along the tool axis of the first,
/// the axis held.
fn retract(poses: &[Pose]) -> bool {
    let (Some(a), Some(b)) = (poses.first(), poses.last()) else { return false };
    let d = b.tip - a.tip;
    poses.len() > 1 && poses.iter().all(|p| p.axis_angle(a) <= 1e-9 && (p.tip - a.tip).cross(a.axis).len() <= 1e-6) && d.dot(a.axis) > 0.0
}

/// A position beyond the machine's travel.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TravelViolation {
    /// The program line (from 1).
    pub line: usize,
    /// `X`, `Y` or `Z`.
    pub axis: char,
    /// Where the axis would be (mm, machine coordinates).
    pub value: f64,
    pub low: f64,
    pub high: f64,
}

/// What [`check_travel`] found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Travel {
    /// Motion blocks checked.
    pub checked: usize,
    /// The first position beyond the travel.
    pub violation: Option<TravelViolation>,
}

/// Every position the linear axes of `machine` reach in `text` (the end of every block, and
/// along every arc) against its linear limits; `None` for a machine without them. With tool
/// centre point control the axes are found from the tip through the kinematics.
pub fn check_travel(text: &str, machine: &Machine) -> Result<Option<Travel>> {
    machine.validate()?;
    let Some((lo, hi)) = machine.linear_limits else { return Ok(None) };
    check_travel_within(text, machine, lo, hi, Vec3::ZERO).map(Some)
}

/// Every position the linear axes of `machine` reach in `text`, moved by `shift` (where work
/// zero is in machine coordinates), against `lo` to `hi` (machine coordinates): a machine's soft
/// limits. The violation's value is in machine coordinates.
pub fn check_travel_within(text: &str, machine: &Machine, lo: Vec3, hi: Vec3, shift: Vec3) -> Result<Travel> {
    machine.validate()?;
    let fin = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    if !(fin(lo) && fin(hi) && fin(shift)) || lo.x > hi.x || lo.y > hi.y || lo.z > hi.z {
        return invalid("the travel limits must be finite, each low at most its high");
    }
    let blocks = parse5(text, &machine.letters())?;
    let mut out = Travel::default();
    let mut last: Option<Vec3> = None;
    for b in &blocks {
        let (line, points) = match b {
            JointWords::Rapid { line, linear, .. } | JointWords::Feed { line, linear, .. } => (*line, vec![*linear]),
            JointWords::Arc { line, linear, .. } | JointWords::PlaneArc { line, linear, .. } => {
                let mut pts = Vec::new();
                if let Some(from) = last
                    && let Some(arc) = b.arc_from(from)
                {
                    let start = arc.plane.split(from).0 - arc.plane.split(arc.center).0;
                    let sweep = arc.sweep();
                    for k in 1..64 {
                        pts.push(arc.at(k as f64 / 64.0));
                    }
                    // Where the arc reaches farthest along its plane's axes (it may between the
                    // samples): wherever it passes a quarter turn.
                    if sweep.abs() > 1e-12 {
                        for k in -6..=6 {
                            let t = (f64::from(k) * std::f64::consts::FRAC_PI_2 - start.angle()) / sweep;
                            if t > 0.0 && t < 1.0 {
                                pts.push(arc.at(t));
                            }
                        }
                    }
                }
                pts.push(*linear);
                (*line, pts)
            }
            JointWords::Dwell { .. } => continue,
        };
        out.checked += 1;
        if out.violation.is_none() {
            'points: for p in &points {
                let p = *p + shift;
                for (axis, v, a, b) in [('X', p.x, lo.x, hi.x), ('Y', p.y, lo.y, hi.y), ('Z', p.z, lo.z, hi.z)] {
                    if !(v >= a - 1e-6 && v <= b + 1e-6) {
                        out.violation = Some(TravelViolation { line, axis, value: v, low: a, high: b });
                        break 'points;
                    }
                }
            }
        }
        last = points.last().copied();
    }
    Ok(out)
}
