//! Drilling: holes at points, fed straight down the tool axis with a drill (or plunged with a
//! flat end mill), in one of four cycles.
//!
//! Every cycle is written out as plain rapid and feed moves (and a dwell), which every
//! controller reads and the read-back follows; no canned cycle (G81, G82, G83, G73) is written.
//! The moves are those cycles' own:
//! - [`Cycle::Simple`]: feed to depth, rapid out to the retract (R) height.
//! - [`Cycle::Dwell`]: the same with a pause at depth.
//! - [`Cycle::Peck`]: feed a peck deeper at a time, rapid right out to the R height after each
//!   peck (clearing the chips), rapid back down to a little above the depth reached, and feed on.
//! - [`Cycle::ChipBreak`]: feed a peck deeper at a time, backing off a short way at rapid
//!   after each to break the chip.
//!
//! Holes are visited in a greedy nearest-neighbour order from the hole lowest in X (then Y),
//! deterministic for the same points. Between holes the tool rises to the clearance height,
//! or with [`DrillParams::stay_at_retract`] travels at the R height, which is allowed only when
//! the R height is above everything in the way ([`DrillParams::part_top`]).
//!
//! A drill's point is a cone below its full diameter: [`tip_length`] is how far. A hole's
//! depth is to the tip, or with [`DrillParams::to_full_diameter`] to where the drill reaches
//! its full diameter. The gouge check ([`tool_gouges`]) holds the drill's real shape against
//! the part: the cylinder above the point and the cone, inscribed by stepped cylinders.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use serde::{Deserialize, Serialize};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::check::verify;
use crate::collision::{GougeTest, Hit, Moves as Sampled, depth_at, for_each_sample, gouges};
use crate::cutter::Cutter;
use crate::dropcutter::drop_z;
use crate::multiaxis::{Move5, Pose, Toolpath5};
use crate::ops::Heights;
use crate::path::{MAX_MOVES, Move, Toolpath};
use crate::surface::Surface;
use crate::tool::{MAX_TOOL_DIAMETER, Tool, ToolKind};
use crate::{CamError, Result, invalid};

/// Most holes one drilling operation drills.
pub const MAX_HOLES: usize = 10_000;
/// The point angle of a drill that gives none (degrees).
pub const DEFAULT_POINT_ANGLE_DEG: f64 = 118.0;
/// Holes closer together than this (mm) are the same hole, drilled once.
pub const SAME_HOLE: f64 = 1e-6;
/// Most pecks into one hole.
const MAX_PECKS: usize = 10_000;
/// Most stepped cylinders a drill's point is checked with.
const MAX_POINT_STEPS: usize = 64;
/// Largest coordinate, height or distance accepted (mm).
const MAX_COORD: f64 = 1.0e6;
/// Longest dwell (seconds).
const MAX_DWELL: f64 = 3600.0;

/// How each hole is drilled.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Cycle {
    /// Feed to depth, then rapid out to the R height.
    Simple,
    /// Feed to depth, pause `seconds`, rapid out.
    Dwell { seconds: f64 },
    /// Feed `peck` deeper at a time (from the hole's top); after each peck rapid out to the R
    /// height, then rapid back down to `clearance` above the depth reached.
    Peck { peck: f64, clearance: f64 },
    /// Feed `peck` deeper at a time; after each peck rapid back `back_off` (staying in the
    /// hole) to break the chip.
    ChipBreak { peck: f64, back_off: f64 },
}

impl Cycle {
    pub fn label(&self) -> &'static str {
        match self {
            Cycle::Simple => "simple",
            Cycle::Dwell { .. } => "dwell",
            Cycle::Peck { .. } => "peck",
            Cycle::ChipBreak { .. } => "chip_break",
        }
    }

    fn validate(&self) -> Result<()> {
        let distance = |v: f64| v > 0.0 && v <= MAX_COORD;
        match *self {
            Cycle::Simple => Ok(()),
            Cycle::Dwell { seconds } if (0.0..=MAX_DWELL).contains(&seconds) => Ok(()),
            Cycle::Dwell { .. } => invalid("the dwell must be from 0 to 3600 seconds"),
            Cycle::Peck { peck, clearance } => {
                if !distance(peck) {
                    return invalid("the peck depth must be above 0");
                }
                if !distance(clearance) {
                    return invalid("the peck clearance (how far above the depth reached the tool comes back down to) must be above 0");
                }
                Ok(())
            }
            Cycle::ChipBreak { peck, back_off } => {
                if !distance(peck) {
                    return invalid("the peck depth must be above 0");
                }
                if !distance(back_off) {
                    return invalid("the chip-breaking back-off must be above 0");
                }
                Ok(())
            }
        }
    }
}

/// A drilling operation's settings, in the operation's frame (its Z the tool axis).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrillParams {
    pub cycle: Cycle,
    /// `clearance`: where the tool travels between holes; `retract`: the R height, where rapid
    /// motion towards a hole stops and feed begins; `top`: the hole's top, where pecks are
    /// counted from; `bottom`: the hole's depth (to the tip, or see `to_full_diameter`).
    pub heights: Heights,
    /// The feed into the hole (mm/min).
    pub feed: f64,
    /// Take `bottom` as where the drill reaches its full diameter: the tip goes
    /// [`tip_length`] deeper. A flat end mill has no point and goes to `bottom` either way.
    pub to_full_diameter: bool,
    /// Travel between holes at the R height rather than the clearance height. Allowed only when
    /// the R height is above `part_top`.
    pub stay_at_retract: bool,
    /// The highest point of everything the tool could meet travelling between holes (the part
    /// and its fixtures, in this frame); `None`: not known, and the tool does not stay at the R
    /// height.
    pub part_top: Option<f64>,
}

/// The drill's point angle (radians): its own, or [`DEFAULT_POINT_ANGLE_DEG`].
pub fn point_angle(tool: &Tool) -> f64 {
    tool.point_angle_deg.unwrap_or(DEFAULT_POINT_ANGLE_DEG).to_radians()
}

/// How far below its full diameter a drill's tip is (mm): `r / tan(point angle / 2)`. 0 for
/// every other kind of tool (and for a drill with a 180° point).
pub fn tip_length(tool: &Tool) -> f64 {
    if tool.kind != ToolKind::Drill {
        return 0.0;
    }
    let half = point_angle(tool) * 0.5;
    if !(half > 1e-6) || half >= std::f64::consts::FRAC_PI_2 - 1e-12 {
        return 0.0;
    }
    tool.radius() / half.tan()
}

/// How far above its tip a tool cuts its full diameter (mm), so how far below a hole's bottom
/// its tip must go for the hole to be its full diameter all the way down: a drill's point
/// ([`tip_length`]), a bull nose's or face mill's corner radius, a ball's radius, nothing for a
/// flat end mill. `None` for a tool that never cuts a round wall to its full diameter (a chamfer
/// mill, a thread mill).
pub fn full_diameter_rise(tool: &Tool) -> Option<f64> {
    match tool.kind {
        ToolKind::Drill => Some(tip_length(tool)),
        ToolKind::FlatEndmill => Some(0.0),
        ToolKind::BullNose | ToolKind::FaceMill => Some(tool.corner_radius.unwrap_or(0.0).max(0.0)),
        ToolKind::BallEndmill => Some(tool.radius()),
        ToolKind::Chamfer | ToolKind::ThreadMill => None,
    }
}

/// Why `tool` cannot cut `depth` mm down into a hole (or the feature `what` names) to its
/// floor: its flutes are shorter, or it stands out of its holder less than that. `None`: it
/// reaches. The one reach rule both a job's suggestions and an operation's tool picks apply.
pub fn reach_shortfall(tool: &Tool, depth: f64, what: &str) -> Option<String> {
    if !depth.is_finite() {
        return None;
    }
    if tool.flute_length + 1e-9 < depth {
        return Some(format!("its flutes ({} mm) are shorter than the {} mm deep {what}", mm4(tool.flute_length), mm4(depth)));
    }
    match tool.stickout {
        Some(s) if s + 1e-9 < depth => Some(format!("it stands out {} mm from its holder, less than the {} mm deep {what}", mm4(s), mm4(depth))),
        _ => None,
    }
}

/// How far above a hole's top (mm) a tool's flutes must end when its shank is wider than the
/// hole: the shank stays out of the hole, clear of its rim.
pub const SHANK_CLEAR: f64 = 0.5;

/// [`reach_shortfall`] into a hole (or the feature `what` names) `width` mm across: a tool whose
/// shank is wider than that cannot follow its flutes in, so its flutes must also end
/// [`SHANK_CLEAR`] above the top, or the shank sits on the rim at the floor. `None`: it reaches.
pub fn reach_shortfall_into(tool: &Tool, depth: f64, width: f64, what: &str) -> Option<String> {
    if let Some(short) = reach_shortfall(tool, depth, what) {
        return Some(short);
    }
    let shank = tool.shank_diameter?;
    if !(depth.is_finite() && width.is_finite()) || shank <= width + 1e-9 || tool.flute_length + 1e-9 >= depth + SHANK_CLEAR {
        return None;
    }
    Some(format!(
        "its shank ({} mm) is wider than the {} mm {what}, so its flutes ({} mm) must be at least {} mm long for it to reach the {} mm deep floor clear of the rim",
        mm4(shank),
        mm4(width),
        mm4(tool.flute_length),
        mm4(depth + SHANK_CLEAR),
        mm4(depth)
    ))
}

/// How far a blind, flat-floored hole tapped down to `thread_bottom` would have to go below its
/// modelled floor at `floor` for `tool`, its point (or corner) at the floor, to cut its full
/// diameter down to the thread's bottom: `None` when it already does (an end mill, or a thread
/// ending a point's length above the floor). Infinite for a tool that never cuts a round wall
/// to its full diameter.
pub fn below_floor_for_thread(tool: &Tool, floor: f64, thread_bottom: f64, tol: f64) -> Option<f64> {
    let rise = full_diameter_rise(tool).unwrap_or(f64::INFINITY);
    let short = floor + rise - thread_bottom;
    (short > tol).then_some(short)
}

/// A length as messages show it: at most four decimals.
fn mm4(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// A drilling operation takes a drill, a flat end mill to plunge with, or a chamfer mill (a
/// spot drill) to spot holes with: its point cuts the cone a drill then starts in.
pub fn check_drilling_tool(tool: &Tool) -> Result<()> {
    match tool.kind {
        ToolKind::Drill | ToolKind::FlatEndmill | ToolKind::Chamfer => Ok(()),
        other => invalid(format!(
            "a drilling operation needs a drill, a flat end mill to plunge with, or a chamfer mill to spot with; tool {} is a {}",
            tool.number,
            other.label()
        )),
    }
}

/// A last peck shorter than this share of the peck is merged into the one before it: a sliver
/// of a peck (0.002 mm where the depth is a hair over a whole number of pecks) is a retract and
/// a re-entry for nothing.
pub const SHORT_PECK: f64 = 0.1;

/// The depths (operation Z) a hole from `top` to `bottom` is fed to, one per peck: `peck`
/// apart from the top, the last at `bottom`. A last peck shorter than [`SHORT_PECK`] of `peck`
/// is merged into the one before (that one goes a little deeper than `peck`). `None` for a
/// peck: one feed to `bottom`.
pub fn peck_depths(top: f64, bottom: f64, peck: Option<f64>) -> Result<Vec<f64>> {
    if !(top.is_finite() && bottom.is_finite() && top > bottom) {
        return invalid("the hole has no depth: its bottom must be below its top");
    }
    let Some(peck) = peck else { return Ok(vec![bottom]) };
    if !(peck > 0.0) || !(peck <= MAX_COORD) {
        return invalid("the peck depth must be above 0");
    }
    let depth = top - bottom;
    let mut n = (depth / peck - 1e-9).ceil().max(1.0);
    if n > 1.0 && depth - peck * (n - 1.0) < SHORT_PECK * peck {
        n -= 1.0;
    }
    if !(n <= MAX_PECKS as f64) {
        return Err(CamError::TooLarge(format!("more than {MAX_PECKS} pecks into one hole; use a larger peck")));
    }
    let n = n as usize;
    Ok((1..=n).map(|k| if k == n { bottom } else { top - peck * k as f64 }).collect())
}

/// `points` without repeats (holes within [`SAME_HOLE`] of an earlier one), in the order they
/// are drilled: from the one lowest in X (then Y), each time on to the nearest hole not yet
/// drilled (the first in `points` among equally near ones). Returns the order and how many
/// repeats were dropped.
pub fn order_holes(points: &[Vec2]) -> Result<(Vec<Vec2>, usize)> {
    if points.is_empty() {
        return invalid("there are no holes to drill");
    }
    if points.len() > MAX_HOLES {
        return Err(CamError::TooLarge(format!("more than {MAX_HOLES} holes in one drilling operation")));
    }
    if points.iter().any(|p| !p.is_finite() || p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD) {
        return invalid("the hole positions must be finite numbers within 1000 m");
    }
    let mut done = vec![false; points.len()];
    let first = points.iter().enumerate().min_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.1.y.total_cmp(&b.1.y))).map_or(0, |(i, _)| i);
    let mut order = Vec::with_capacity(points.len());
    let mut repeats = 0;
    let mut current = Some(first);
    while let Some(i) = current {
        let Some(p) = points.get(i).copied() else { break };
        if let Some(d) = done.get_mut(i) {
            *d = true;
        }
        order.push(p);
        let mut next: Option<(usize, f64)> = None;
        for (j, q) in points.iter().enumerate() {
            if done.get(j).copied().unwrap_or(true) {
                continue;
            }
            let d = p.dist(*q);
            if d <= SAME_HOLE {
                if let Some(x) = done.get_mut(j) {
                    *x = true;
                }
                repeats += 1;
                continue;
            }
            if next.is_none_or(|(_, best)| d < best) {
                next = Some((j, d));
            }
        }
        current = next.map(|(j, _)| j);
    }
    Ok((order, repeats))
}

/// Collects moves, refusing more than [`MAX_MOVES`].
struct Moves(Vec<Move>);

impl Moves {
    fn push(&mut self, m: Move) -> Result<()> {
        if self.0.len() >= MAX_MOVES {
            return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
        }
        self.0.push(m);
        Ok(())
    }

    fn rapid(&mut self, p: Vec2, z: f64) -> Result<()> {
        self.push(Move::Rapid { to: p.extend(z) })
    }

    fn feed(&mut self, p: Vec2, z: f64, feed: f64) -> Result<()> {
        self.push(Move::Feed { to: p.extend(z), feed })
    }
}

/// Drills a hole at each of `points` (the operation's XY; repeats are drilled once) with
/// `tool`, in the order of [`order_holes`]. The path is checked with [`verify`] before it is
/// returned.
pub fn drill(points: &[Vec2], tool: &Tool, params: &DrillParams) -> Result<Toolpath> {
    tool.validate()?;
    check_drilling_tool(tool)?;
    params.heights.validate()?;
    params.cycle.validate()?;
    if !(params.feed > 0.0) || !(params.feed <= 1.0e6) {
        return invalid("the drilling feed must be above 0");
    }
    let h = params.heights;
    let tip = if params.to_full_diameter { h.bottom - tip_length(tool) } else { h.bottom };
    if !(tip < h.top) {
        return invalid("the hole has no depth: its bottom must be below its top");
    }
    if h.top - tip > tool.flute_length + 1e-9 {
        return invalid(format!(
            "the hole is {} mm deep, deeper than tool {}'s flutes ({} mm): the chips could not get out",
            h.top - tip,
            tool.number,
            tool.flute_length
        ));
    }
    if tool.kind == ToolKind::Chamfer {
        crate::chamfer::within_cone(tool, h.top - tip, "spot")?;
    }
    if params.stay_at_retract && !params.part_top.is_some_and(|t| t.is_finite() && h.retract > t) {
        return invalid(
            "`stay_at_retract` needs the retract (R) height above the top of the part and its fixtures; travel at the clearance height instead",
        );
    }
    let travel = if params.stay_at_retract { h.retract } else { h.clearance };
    let peck = match params.cycle {
        Cycle::Peck { peck, .. } | Cycle::ChipBreak { peck, .. } => Some(peck),
        Cycle::Simple | Cycle::Dwell { .. } => None,
    };
    let depths = peck_depths(h.top, tip, peck)?;
    let (holes, _) = order_holes(points)?;

    let mut m = Moves(Vec::new());
    for (i, &p) in holes.iter().enumerate() {
        let over = if i == 0 { h.clearance } else { travel };
        m.rapid(p, over)?;
        if over > h.retract {
            m.rapid(p, h.retract)?;
        }
        for (k, &z) in depths.iter().enumerate() {
            m.feed(p, z, params.feed)?;
            let last = k + 1 == depths.len();
            match params.cycle {
                Cycle::Dwell { seconds } if last => m.push(Move::Dwell { seconds })?,
                Cycle::Peck { clearance, .. } if !last => {
                    m.rapid(p, h.retract)?;
                    let back = (z + clearance).min(h.retract);
                    if back < h.retract {
                        m.rapid(p, back)?;
                    }
                }
                Cycle::ChipBreak { back_off, .. } if !last => m.rapid(p, (z + back_off).min(h.retract))?,
                _ => {}
            }
        }
        m.rapid(p, h.retract)?;
        if (i + 1 == holes.len() || !params.stay_at_retract) && h.clearance > h.retract {
            m.rapid(p, h.clearance)?;
        }
    }
    let path = Toolpath { moves: m.0 };
    verify(&path, &Heights { bottom: tip, ..h })?;
    Ok(path)
}

/// The poses of `path` moved `h` up their tool axes.
pub(crate) fn raised(path: &Toolpath5, h: f64) -> Toolpath5 {
    let up = |p: Pose| Pose::new(p.up(h), p.axis);
    let moves = path
        .moves
        .iter()
        .map(|m| match *m {
            Move5::Rapid { to } => Move5::Rapid { to: up(to) },
            Move5::Feed { to, feed } => Move5::Feed { to: up(to), feed },
            Move5::Dwell { seconds } => Move5::Dwell { seconds },
        })
        .collect();
    Toolpath5 { moves }
}

#[cfg(test)]
thread_local! {
    /// What [`tool_gouges`] did on this thread for a drill: the poses it tested, and the hits it
    /// merged into the deepest of each move (one lookup each).
    pub(crate) static GOUGE_WORK: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// The gouges of `tool` along the cutting moves of `path`, as [`gouges`] finds them for a
/// milling cutter. A drill is held against the surface as its real shape: the full diameter
/// from [`tip_length`] up, and its point as stepped cylinders inscribed in the cone, each
/// within the larger of `tol` and 1/64 of the tip length of the cone along the axis. Every step
/// is tested quickly at the samples of each move (how far it would have to back out along its
/// axis); the deepest step of each move is then measured as [`gouges`] measures a cutter (the
/// deepest point of the surface inside it, to its own surface, see [`depth_at`]), and so is the
/// full diameter where it meets the part on that move: a wall it cuts into is deepest there,
/// though a narrower step lower down has further to back out (a 6 mm drill down a 5 mm hole
/// cuts 0.5 mm into its wall, where the narrowest step past the wall measures a few hundredths).
/// So a part with many gouging pecks costs at most two exact measures per move, not one per
/// step. One hit per move, at its deepest; the pose is the tip's.
pub fn tool_gouges(surface: &Surface, tool: &Tool, path: &Toolpath5, tol: f64, sample_step: f64) -> Result<Vec<Hit>> {
    if tool.kind != ToolKind::Drill {
        return gouges(surface, &tool.cutter()?, path, tol, sample_step);
    }
    tool.validate()?;
    let (r, length) = (tool.radius(), tip_length(tool));
    if !(length > 1e-9) {
        return gouges(surface, &Cutter::flat(r)?, path, tol, sample_step);
    }
    let steps = ((length / tol.max(1e-6)).ceil().max(1.0) as usize).min(MAX_POINT_STEPS);
    // The deepest quick value of every step on each move, keyed by the move: one lookup per
    // hit, not a search through every hit found so far (which made a part with many gouging
    // pecks quadratic in them). With it, the step and the pose of its cylinder.
    let mut deepest: BTreeMap<usize, (f64, usize, Pose)> = BTreeMap::new();
    // The full diameter's own deepest pose on each move it meets the part.
    let mut full: BTreeMap<usize, (f64, Pose)> = BTreeMap::new();
    for k in 1..=steps {
        let t = k as f64 / steps as f64;
        let cutter = Cutter::flat(r * t)?;
        let test = GougeTest::new(&cutter, tol)?;
        let lifted = raised(path, length * t);
        for_each_sample(&lifted, cutter.radius, sample_step, Sampled::Cutting, |index, pose| {
            #[cfg(test)]
            GOUGE_WORK.with(|w| w.set((w.get().0 + 1, w.get().1)));
            if let Some(d) = test.depth(surface, &pose)? {
                #[cfg(test)]
                GOUGE_WORK.with(|w| w.set((w.get().0, w.get().1 + 1)));
                if k == steps && full.get(&index).is_none_or(|(e, _)| d > *e) {
                    full.insert(index, (d, pose));
                }
                match deepest.entry(index) {
                    Entry::Occupied(mut x) if d > x.get().0 => {
                        x.insert((d, k, pose));
                    }
                    Entry::Occupied(_) => {}
                    Entry::Vacant(x) => {
                        x.insert((d, k, pose));
                    }
                }
            }
            Ok(())
        })?;
    }
    let mut hits = Vec::with_capacity(deepest.len());
    for (index, (_, k, pose)) in deepest {
        let t = k as f64 / steps as f64;
        let mut best = (depth_at(surface, &Cutter::flat(r * t)?, &pose)?, t, pose);
        if k != steps
            && let Some(&(_, at)) = full.get(&index)
        {
            let d = depth_at(surface, &Cutter::flat(r)?, &at)?;
            if d > best.0 {
                best = (d, 1.0, at);
            }
        }
        let (depth, t, pose) = best;
        if depth > tol {
            hits.push(Hit { index, pose: Pose::new(pose.up(-length * t), pose.axis), envelope: None, distance: depth });
        }
    }
    Ok(hits)
}

/// How far past a hole's edge, as a share of the tool's radius, [`check_approach`] looks for
/// the part: far enough to rest on the rim of a hole of the tool's own size.
const RIM: f64 = 0.1;

/// Refuses holes the tool cannot come down to at rapid: at each of `holes` it comes straight
/// down from the clearance height to `retract` (the R height), so nothing of `surface` (the part
/// and fixtures, in the operation's frame) may be above the R height, by more than `tol`, within
/// the tool's radius and a tenth more of the hole's axis. The gouge check holds the tool against
/// the finished part, where the hole is already there; this is the check that catches a hole in
/// a boss drilled with the heights of the plate under it, where the drill would rapid into the
/// boss.
pub fn check_approach(surface: &Surface, tool: &Tool, holes: &[Vec2], retract: f64, tol: f64) -> Result<()> {
    tool.validate()?;
    if !(retract.is_finite() && retract.abs() <= MAX_COORD) {
        return invalid("the retract height must be a finite number");
    }
    if !(0.0..=1.0).contains(&tol) {
        return invalid("the tolerance must be from 0 to 1 mm");
    }
    if holes.len() > MAX_HOLES {
        return Err(CamError::TooLarge(format!("more than {MAX_HOLES} holes in one drilling operation")));
    }
    if holes.iter().any(|p| !p.is_finite() || p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD) {
        return invalid("the hole positions must be finite numbers within 1000 m");
    }
    let ring = Cutter::flat((tool.radius() * (1.0 + RIM) + tol).min(MAX_TOOL_DIAMETER * 0.5))?;
    for p in holes {
        if let Some(z) = drop_z(surface, &ring, p.x, p.y)
            && z > retract + tol
        {
            return invalid(format!(
                "the part round the hole at ({}, {}) reaches Z {}, above the retract (R) height {}: the tool would rapid down into it; give \
                 `top` and `retract` at or above the part round every hole, or drill holes at different heights in separate operations",
                mm(p.x),
                mm(p.y),
                mm(z),
                mm(retract)
            ));
        }
    }
    Ok(())
}

/// `v` to the micrometre, for messages (no `-0`).
fn mm(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0 + 0.0
}

/// The tip positions of the holes in `path` (for reports): where each feed into a new hole ends
/// deepest, in drilling order.
pub fn hole_bottoms(path: &Toolpath) -> Vec<Vec3> {
    let mut out: Vec<Vec3> = Vec::new();
    let mut at: Option<Vec2> = None;
    for m in &path.moves {
        if let Move::Feed { to, .. } = *m {
            let xy = Vec2::new(to.x, to.y);
            match out.last_mut() {
                Some(last) if at.is_some_and(|a| a.dist(xy) <= SAME_HOLE) => last.z = last.z.min(to.z),
                _ => out.push(to),
            }
            at = Some(xy);
        }
    }
    out
}
