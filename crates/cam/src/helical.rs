//! Helical boring and thread milling: both follow a helix round a hole's axis (+Z), written as
//! helical arcs (`G2`/`G3` with Z) half a turn each.
//!
//! [`bore`] opens circular holes with an end mill: from the hole's centre a half-circle lead-in
//! inside the hole brings the tool tangentially onto its helix, so it never meets the wall
//! head-on; the helix goes down at a pitch (or a ramp angle, its pitch then capped), a full
//! flat circle cleans the bottom, and a lead-out half circle brings the tool back to the centre.
//! A hole more than two tool diameters across is opened in several helical rings, the first
//! within the tool's radius of the centre so no core is left, each further one at most a
//! stepover out. Finishing passes go round at the full diameter after roughing that left
//! `stock_to_leave` on the wall.
//!
//! [`thread_mill`] cuts internal and external threads with a thread mill, the helix at exactly
//! one pitch per turn in radial passes and an optional spring pass at full depth. Climb milling
//! with a clockwise spindle goes counter-clockwise inside a hole and clockwise round a boss; a
//! right-hand thread then rises when the tool goes counter-clockwise, so an internal right-hand
//! thread is climb milled from the bottom up and an external one from the top down. Z is the
//! tool's tip: a single-point thread mill's tooth is taken at its tip, and a multi-form tool's
//! rows of teeth stand a pitch apart above it, over its flute length, so each turn of it cuts
//! as many pitches of thread as it has rows. An internal thread's leads (half circles through
//! the axis) turn the tool a quarter turn about the axis, so they rise a quarter pitch with the
//! helix: wherever the teeth touch the wall they are on the thread, never notching its flanks.
//! No arc goes down faster than the plunge feed.
//!
//! The model does not show a thread, so the gouge check of thread milling takes the thread mill
//! as a flat end mill at the roots of its teeth ([`thread_check_tool`]): the thread's depth
//! inside its radius. That core must clear the part; the teeth beyond it cut the thread.

use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::check::verify;
use crate::ops::{Feeds, Heights, mills};
use crate::path::{MAX_MOVES, Move, Toolpath};
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

/// Most holes or threads in one operation.
pub const MAX_HOLES: usize = 10_000;
/// Most turns of one helix.
pub const MAX_TURNS: f64 = 20_000.0;
/// Smallest room between the tool and a hole's wall on each side for a helix (mm).
pub const MIN_WALL_GAP: f64 = 0.05;
/// A hole narrower than this many tool diameters leaves little room for the chips.
pub const CHIP_ROOM_RATIO: f64 = 1.2;
/// The radial depth of an ISO metric (60°) internal thread, per mm of pitch: 5H/8.
pub const ISO_INTERNAL_DEPTH: f64 = 0.541_265_877_365_273_1;
/// The radial depth of an ISO metric (60°) external thread, per mm of pitch: 17H/24.
pub const ISO_EXTERNAL_DEPTH: f64 = 0.613_434_661_013_976_4;
/// Most helical rings in one hole.
const MAX_RINGS: usize = 1_000;
/// Most radial or finishing passes.
const MAX_PASSES: u32 = 100;
/// The first ring of a wide hole, as a share of the tool radius from the centre: inside the
/// radius, so the centre is cut.
const FIRST_RING: f64 = 0.75;
/// How far outside the thread an external thread's lead-in starts, beyond its depth (mm).
const LEAD_MARGIN: f64 = 0.5;
/// Largest coordinate or size (mm).
const MAX_COORD: f64 = 1.0e6;
/// Smallest sweep of an arc written (radians); a helix's last piece shorter than this is
/// left out.
const MIN_SWEEP: f64 = 1e-6;

/// A circular hole: its centre in the operation's XY and its finished diameter (mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hole {
    pub center: Vec2,
    pub diameter: f64,
}

/// How a bore's helix goes down.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Descent {
    /// Down `pitch` mm each turn, whatever the ring's radius.
    Pitch { pitch: f64 },
    /// The helix the tool centre follows slopes at `angle` (radians), its pitch at most
    /// `max_pitch` (mm), which keeps wide rings from diving.
    Ramp { angle: f64, max_pitch: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoreParams {
    /// `top` is the hole's top, `bottom` its floor.
    pub heights: Heights,
    pub feeds: Feeds,
    pub descent: Descent,
    /// Climb milling (counter-clockwise inside the hole with a clockwise spindle).
    pub climb: bool,
    /// Most distance between the rings of a wide hole (mm), at most the tool diameter.
    pub stepover: f64,
    /// Material the roughing rings leave on the wall (mm).
    pub stock_to_leave: f64,
    /// Passes at the full diameter after roughing (0 to 100): the first takes the stock left,
    /// the others are spring passes.
    pub finish_passes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadParams {
    /// `top` and `bottom` are the thread's ends (its length is `top - bottom`).
    pub heights: Heights,
    pub feeds: Feeds,
    /// Advance per turn (mm).
    pub pitch: f64,
    /// The thread's major (nominal) diameter (mm), inside a hole or round a boss.
    pub diameter: f64,
    /// Inside a hole; otherwise round a boss.
    pub internal: bool,
    pub right_hand: bool,
    pub climb: bool,
    /// The thread's radial depth (mm); `None`: the ISO metric depth for the pitch
    /// ([`ISO_INTERNAL_DEPTH`], [`ISO_EXTERNAL_DEPTH`]).
    pub depth: Option<f64>,
    /// Passes the depth is split into, evenly (1 to 100).
    pub radial_passes: u32,
    /// One more pass at full depth, for the tool's spring.
    pub spring_pass: bool,
}

impl ThreadParams {
    /// The thread's radial depth (mm).
    pub fn thread_depth(&self) -> f64 {
        self.depth.unwrap_or(self.pitch * if self.internal { ISO_INTERNAL_DEPTH } else { ISO_EXTERNAL_DEPTH })
    }
}

/// A toolpath with what is worth knowing about it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HelicalPath {
    pub path: Toolpath,
    pub warnings: Vec<String>,
}

/// A number as messages show it: at most three decimals.
fn mm(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

fn finite_in(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && v >= lo && v <= hi
}

/// The point at `angle` on the circle round `c` of radius `rho`.
fn on(c: Vec2, rho: f64, angle: f64) -> Vec2 {
    c + Vec2::from_angle(angle) * rho
}

/// Collects moves, keeping the tool's position.
struct Builder {
    moves: Vec<Move>,
    pos: Option<Vec3>,
    /// The plunge feed: no arc goes down faster than this.
    plunge: f64,
}

impl Builder {
    fn push(&mut self, m: Move) -> Result<()> {
        if self.moves.len() >= MAX_MOVES {
            return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
        }
        if let Some(to) = m.target() {
            self.pos = Some(to);
        }
        self.moves.push(m);
        Ok(())
    }

    fn at(&self) -> Result<Vec3> {
        self.pos.ok_or_else(|| CamError::Unsafe("a move before the first positioning move".into()))
    }

    fn rapid(&mut self, to: Vec3) -> Result<()> {
        self.push(Move::Rapid { to })
    }

    fn feed(&mut self, to: Vec3, feed: f64) -> Result<()> {
        self.push(Move::Feed { to, feed })
    }

    /// Straight up or down to `z`: at feed going down, at rapid going up (through what is cut).
    fn vertical(&mut self, z: f64, plunge: f64) -> Result<()> {
        let p = self.at()?;
        if z < p.z - 1e-9 {
            self.feed(Vec3::new(p.x, p.y, z), plunge)
        } else if z > p.z + 1e-9 {
            self.rapid(Vec3::new(p.x, p.y, z))
        } else {
            Ok(())
        }
    }

    /// An arc to `to` round `center`, `sweep` radians long, at `feed`, slowed if need be so that
    /// it goes down no faster than the plunge feed: a steep helix close round the axis is
    /// nearly a plunge.
    fn arc(&mut self, to: Vec3, center: Vec2, ccw: bool, sweep: f64, feed: f64) -> Result<()> {
        let p = self.at()?;
        let drop = p.z - to.z;
        let feed = if drop > 1e-12 {
            let length = (sweep.abs() * Vec2::new(p.x, p.y).dist(center)).hypot(drop);
            feed.min(self.plunge * length / drop)
        } else {
            feed
        };
        self.push(Move::Arc { to, center, ccw, feed })
    }

    /// A half circle from where the tool is to `to` at height `z` (a helical one when that is
    /// not the tool's height), turning `ccw`.
    fn half_circle(&mut self, to: Vec2, z: f64, ccw: bool, feed: f64) -> Result<()> {
        let p = self.at()?;
        let from = Vec2::new(p.x, p.y);
        if from.dist(to) <= 1e-9 {
            return self.vertical(z, self.plunge);
        }
        self.arc(to.extend(z), (from + to) * 0.5, ccw, PI, feed)
    }

    /// A helix round `c` at radius `rho` from the tool's position (at `angle` on that circle) to
    /// `z_to`, `pitch` per turn, in half turns: the k-th half turn ends exactly `k × pitch / 2`
    /// from the start. Returns the angle it ends at.
    #[allow(clippy::too_many_arguments)]
    fn helix(&mut self, c: Vec2, rho: f64, angle: f64, z_to: f64, pitch: f64, ccw: bool, feed: f64) -> Result<f64> {
        let z_from = self.at()?.z;
        let dz = z_to - z_from;
        if !(pitch > 0.0) || !pitch.is_finite() {
            return invalid("the helix pitch must be above 0");
        }
        let turns = dz.abs() / pitch;
        if !(turns <= MAX_TURNS) {
            return Err(CamError::TooLarge(format!("a helix of more than {MAX_TURNS} turns: raise the pitch")));
        }
        let halves = turns * 2.0;
        if halves * PI < MIN_SWEEP {
            return Ok(angle);
        }
        let (turn, rise) = (if ccw { PI } else { -PI }, dz.signum() * pitch * 0.5);
        // Every half turn but the last ends on a whole half pitch; the last ends at `z_to`.
        let arcs = (halves - MIN_SWEEP / PI).ceil().max(1.0) as usize;
        for k in 1..arcs {
            let a = angle + turn * k as f64;
            self.arc(on(c, rho, a).extend(z_from + rise * k as f64), c, ccw, PI, feed)?;
        }
        let end = angle + turn * halves;
        self.arc(on(c, rho, end).extend(z_to), c, ccw, PI * (halves - (arcs - 1) as f64), feed)?;
        Ok(end)
    }

    /// A full flat circle round `c` from the tool's position at `angle`, in two half turns.
    fn circle(&mut self, c: Vec2, rho: f64, angle: f64, ccw: bool, feed: f64) -> Result<()> {
        let z = self.at()?.z;
        let turn = if ccw { PI } else { -PI };
        self.arc(on(c, rho, angle + turn).extend(z), c, ccw, PI, feed)?;
        self.arc(on(c, rho, angle + 2.0 * turn).extend(z), c, ccw, PI, feed)
    }
}

fn check_heights_and_feeds(h: &Heights, feeds: &Feeds) -> Result<()> {
    h.validate()?;
    feeds.validate()?;
    if !(h.top > h.bottom) {
        return invalid("the top must be above the bottom");
    }
    Ok(())
}

fn check_center(i: usize, c: Vec2) -> Result<()> {
    if c.x.is_finite() && c.y.is_finite() && c.x.abs() <= MAX_COORD && c.y.abs() <= MAX_COORD {
        Ok(())
    } else {
        invalid(format!("hole {}: its centre must be finite and within 1000 m", i + 1))
    }
}

/// The helix pitch at radius `rho`.
fn pitch_at(descent: &Descent, rho: f64) -> f64 {
    match *descent {
        Descent::Pitch { pitch } => pitch,
        Descent::Ramp { angle, max_pitch } => (TAU * rho * angle.tan()).min(max_pitch),
    }
}

fn check_descent(descent: &Descent, tool: &Tool) -> Result<()> {
    match *descent {
        Descent::Pitch { pitch } => {
            if !finite_in(pitch, 1e-3, tool.flute_length) {
                return invalid("the bore's pitch must be from 0.001 mm to the tool's flute length");
            }
        }
        Descent::Ramp { angle, max_pitch } => {
            if !finite_in(angle, 1e-3_f64.to_radians(), 45f64.to_radians()) {
                return invalid("the bore's ramp angle must be above 0 and at most 45°");
            }
            if !finite_in(max_pitch, 1e-3, tool.flute_length) {
                return invalid("the bore's largest pitch must be from 0.001 mm to the tool's flute length");
            }
        }
    }
    Ok(())
}

/// The radii (tool centre from the hole's axis) of the passes of one hole: the roughing rings
/// out to `rough`, then `finish` passes at `full`.
fn bore_radii(rough: f64, full: f64, tool_radius: f64, stepover: f64, finish: u32) -> Result<Vec<f64>> {
    let mut radii = if rough <= tool_radius {
        vec![rough]
    } else {
        let first = FIRST_RING * tool_radius;
        let gaps = ((rough - first) / stepover - 1e-9).ceil().max(1.0);
        if !(gaps < MAX_RINGS as f64) {
            return Err(CamError::TooLarge(format!("more than {MAX_RINGS} rings in one hole: raise the stepover")));
        }
        let gaps = gaps as usize;
        (0..=gaps).map(|i| if i == gaps { rough } else { first + (rough - first) * i as f64 / gaps as f64 }).collect()
    };
    radii.extend(std::iter::repeat_n(full, finish as usize));
    Ok(radii)
}

/// Helical bores of `holes` with `tool` (an end mill): see the module docs. Refuses a hole no
/// wider than the tool, naming it, and warns of one narrower than [`CHIP_ROOM_RATIO`] tool
/// diameters.
pub fn bore(holes: &[Hole], tool: &Tool, p: &BoreParams) -> Result<HelicalPath> {
    tool.validate()?;
    // A bore always goes down along a helix, never straight down, so the entry rule is met
    // for every tool that mills (a face mill included); a chamfer mill is refused.
    mills(tool, std::f64::consts::FRAC_PI_4)?;
    check_heights_and_feeds(&p.heights, &p.feeds)?;
    check_descent(&p.descent, tool)?;
    if holes.is_empty() || holes.len() > MAX_HOLES {
        return invalid(format!("give 1 to {MAX_HOLES} holes"));
    }
    if !finite_in(p.stepover, 1e-3, tool.diameter) {
        return invalid("the stepover must be above 0 and at most the tool diameter");
    }
    if !finite_in(p.stock_to_leave, 0.0, MAX_COORD) {
        return invalid("the stock to leave must be 0 or more");
    }
    if p.finish_passes > MAX_PASSES {
        return invalid(format!("at most {MAX_PASSES} finishing passes"));
    }
    let (h, r) = (p.heights, tool.radius());
    let mut warnings = Vec::new();
    let mut plans: Vec<(Vec2, Vec<f64>)> = Vec::with_capacity(holes.len());
    for (i, hole) in holes.iter().enumerate() {
        check_center(i, hole.center)?;
        let name = format!("hole {} at ({}, {}), ⌀{} mm", i + 1, mm(hole.center.x), mm(hole.center.y), mm(hole.diameter));
        if !finite_in(hole.diameter, 0.0, MAX_COORD) || !(hole.diameter > 0.0) {
            return invalid(format!("hole {}: its diameter must be above 0 and finite", i + 1));
        }
        let full = hole.diameter * 0.5 - r;
        if full < MIN_WALL_GAP {
            return invalid(format!(
                "{name}, is too small for the ⌀{} mm tool: a helical bore needs the hole at least {} mm wider than the tool",
                mm(tool.diameter),
                mm(2.0 * MIN_WALL_GAP)
            ));
        }
        let rough = full - p.stock_to_leave;
        if rough < MIN_WALL_GAP {
            return invalid(format!("{name}: the stock to leave ({} mm) leaves the tool no room to rough it", mm(p.stock_to_leave)));
        }
        if hole.diameter < CHIP_ROOM_RATIO * tool.diameter {
            warnings.push(format!(
                "{name}, is less than {CHIP_ROOM_RATIO} × the ⌀{} mm tool: little room for the chips to leave; a smaller tool cuts it more safely",
                mm(tool.diameter)
            ));
        }
        plans.push((hole.center, bore_radii(rough, full, r, p.stepover, p.finish_passes)?));
    }
    let mut b = Builder { moves: Vec::new(), pos: None, plunge: p.feeds.plunge };
    let ccw = p.climb;
    for (k, (c, radii)) in plans.iter().enumerate() {
        let c = *c;
        if k == 0 {
            b.rapid(c.extend(h.clearance))?;
        }
        b.rapid(c.extend(h.retract))?;
        b.feed(c.extend(h.top), p.feeds.plunge)?;
        for (j, &rho) in radii.iter().enumerate() {
            if j > 0 {
                b.vertical(h.top, p.feeds.plunge)?;
            }
            b.half_circle(on(c, rho, 0.0), h.top, ccw, p.feeds.cut)?;
            let end = b.helix(c, rho, 0.0, h.bottom, pitch_at(&p.descent, rho), ccw, p.feeds.cut)?;
            b.circle(c, rho, end, ccw, p.feeds.cut)?;
            b.half_circle(c, h.bottom, ccw, p.feeds.cut)?;
        }
        b.rapid(c.extend(h.retract))?;
    }
    let last = b.at()?;
    b.rapid(Vec3::new(last.x, last.y, h.clearance))?;
    let path = Toolpath { moves: b.moves };
    verify(&path, &h)?;
    Ok(HelicalPath { path, warnings })
}

/// The tool as the gouge check takes it for a thread cut by `p`: the flat core inside the
/// thread mill's teeth, as deep as the thread (a flat end mill of that diameter), under the
/// thread mill's own shank and holder. Only thread milling checks with it: anywhere else a
/// thread mill is checked whole ([`Tool::cutter`]), its teeth and all.
pub fn thread_check_tool(tool: &Tool, p: &ThreadParams) -> Result<Tool> {
    thread_core(tool, p.thread_depth())
}

/// [`thread_check_tool`] for a thread `depth` deep (radially, mm).
pub fn thread_core(tool: &Tool, depth: f64) -> Result<Tool> {
    if !(depth > 0.0) || !(depth < tool.radius()) {
        return invalid("the thread must be less deep than the thread mill's radius");
    }
    Ok(Tool {
        kind: ToolKind::FlatEndmill,
        diameter: tool.diameter - 2.0 * depth,
        corner_radius: None,
        shank_diameter: Some(tool.shank_diameter.unwrap_or(tool.diameter)),
        thread_pitch: None,
        tooth_height: None,
        description: format!("the core of the ⌀{} mm thread mill inside its teeth", mm(tool.diameter)),
        ..tool.clone()
    })
}

/// The tip heights the helix of each turn of the thread mill runs between, lowest first, and
/// how far the teeth cut beyond the thread's top: the thread's length less `lead` (the rise
/// of a lead) at each end for a single-point tool; for a multi-form one a turn per span of its
/// teeth from the bottom, the last as low as still reaches the top, all a whole number of
/// pitches apart so that they cut one helix.
fn thread_segments(tool: &Tool, p: &ThreadParams, lead: f64) -> Result<(Vec<(f64, f64)>, f64)> {
    let (lo, hi) = (p.heights.bottom + lead, p.heights.top - lead);
    if !(hi >= lo - 1e-9) {
        return invalid(format!("the thread must be at least {} mm long, for the leads in and out", mm(2.0 * lead)));
    }
    let len = (hi - lo).max(0.0);
    if !(len / p.pitch <= MAX_TURNS) {
        return Err(CamError::TooLarge(format!("a thread of more than {MAX_TURNS} turns")));
    }
    let Some(tp) = tool.thread_pitch else { return Ok((vec![(lo, lo + len)], 0.0)) };
    if (tp - p.pitch).abs() > 1e-6 * p.pitch.max(1.0) {
        return invalid(format!("the thread mill's teeth are {} mm apart: it cuts only a {} mm pitch, not {} mm", mm(tp), mm(tp), mm(p.pitch)));
    }
    let rows = (tool.flute_length / p.pitch + 1e-9).floor().max(1.0);
    let span = rows * p.pitch;
    let n = (len / span - 1e-9).ceil().max(1.0) as usize;
    // The last turn starts the fewest whole pitches up that its teeth still reach the top.
    let last = lo + p.pitch * ((len - span) / p.pitch - 1e-9).ceil().max(0.0);
    let start = |j: usize| if j + 1 == n { last } else { lo + span * j as f64 };
    Ok(((0..n).map(|j| (start(j), start(j) + p.pitch)).collect(), (last + span - hi).max(0.0)))
}

/// Thread milling round each of `centers` with `tool` (a thread mill): see the module docs.
pub fn thread_mill(centers: &[Vec2], tool: &Tool, p: &ThreadParams) -> Result<HelicalPath> {
    tool.validate()?;
    if tool.kind != ToolKind::ThreadMill {
        return invalid(format!("thread milling needs a thread mill, not a {}", tool.kind.label()));
    }
    check_heights_and_feeds(&p.heights, &p.feeds)?;
    if centers.is_empty() || centers.len() > MAX_HOLES {
        return invalid(format!("give 1 to {MAX_HOLES} threads"));
    }
    if !finite_in(p.pitch, 0.01, 100.0) {
        return invalid("the thread's pitch must be from 0.01 to 100 mm");
    }
    if !finite_in(p.diameter, 0.1, MAX_COORD) {
        return invalid("the thread's diameter must be from 0.1 mm to 1000 m");
    }
    if p.depth.is_some_and(|d| !(d > 0.0) || !(d < p.diameter * 0.5)) {
        return invalid("the thread's depth must be above 0 and less than its radius");
    }
    if !(1..=MAX_PASSES).contains(&p.radial_passes) {
        return invalid(format!("a thread is cut in 1 to {MAX_PASSES} radial passes"));
    }
    let depth = p.thread_depth();
    if tool.tooth_height.is_some_and(|t| t < depth - 1e-9) {
        return invalid(format!("the thread mill's teeth are shorter than the thread is deep ({} mm)", mm(depth)));
    }
    thread_check_tool(tool, p)?;
    // An internal thread's leads go round a quarter turn about the axis (a half circle through
    // it), so they rise a quarter pitch with the helix and its teeth stay on the thread.
    let lead_rise = if p.internal { p.pitch * 0.25 } else { 0.0 };
    let (segments, beyond) = thread_segments(tool, p, lead_rise)?;
    let mut warnings = Vec::new();
    if beyond > p.pitch + 1e-9 {
        warnings.push(format!("the thread mill's teeth are longer than the thread: they cut {} mm of thread beyond its top", mm(beyond)));
    }
    for (i, c) in centers.iter().enumerate() {
        check_center(i, *c)?;
    }
    let (h, r, big) = (p.heights, tool.radius(), p.diameter * 0.5);
    // The tool centre's distance from the axis before the first pass and at full depth.
    let (start, full) = if p.internal { (big - depth - r, big - r) } else { (big + r, big - depth + r) };
    if p.internal && start < MIN_WALL_GAP {
        return invalid(format!(
            "the ⌀{} mm thread mill does not fit the thread's minor diameter ⌀{} mm: it needs at least {} mm of room",
            mm(tool.diameter),
            mm(p.diameter - 2.0 * depth),
            mm(2.0 * MIN_WALL_GAP)
        ));
    }
    let n = p.radial_passes;
    let mut radii: Vec<f64> = (1..=n).map(|k| if k == n { full } else { start + (full - start) * f64::from(k) / f64::from(n) }).collect();
    if p.spring_pass {
        radii.push(full);
    }
    let ccw = p.internal == p.climb;
    let up = ccw == p.right_hand;
    let rise = if up { lead_rise } else { -lead_rise };
    let ordered: Vec<(f64, f64)> = if up { segments } else { segments.into_iter().rev().map(|(lo, hi)| (hi, lo)).collect() };
    // An external thread's lead-in starts this far outside the pass's circle: clear of the
    // thread by `LEAD_MARGIN`. Its leads stay level: they turn the tool only a few degrees
    // about the axis while its teeth are in the thread.
    let lead = (r * 0.5).max((depth + LEAD_MARGIN) * 0.5);
    let approach = |c: Vec2, rho: f64, angle: f64| if p.internal { c } else { on(c, rho + 2.0 * lead, angle) };
    let lead_ccw = if p.internal { ccw } else { !ccw };

    let mut b = Builder { moves: Vec::new(), pos: None, plunge: p.feeds.plunge };
    for c in centers {
        let c = *c;
        for &rho in &radii {
            for &(z_start, z_end) in &ordered {
                let s = approach(c, rho, 0.0);
                let z_in = z_start - rise;
                match b.pos {
                    None => {
                        b.rapid(s.extend(h.clearance))?;
                        b.rapid(s.extend(h.retract))?;
                        b.feed(s.extend(z_in), p.feeds.plunge)?;
                    }
                    Some(at) if Vec2::new(at.x, at.y).dist(s) > 1e-9 => {
                        b.vertical(h.retract.max(at.z), p.feeds.plunge)?;
                        b.rapid(s.extend(h.retract))?;
                        b.feed(s.extend(z_in), p.feeds.plunge)?;
                    }
                    Some(_) => b.vertical(z_in, p.feeds.plunge)?,
                }
                b.half_circle(on(c, rho, 0.0), z_start, lead_ccw, p.feeds.cut)?;
                let end = b.helix(c, rho, 0.0, z_end, p.pitch, ccw, p.feeds.cut)?;
                b.half_circle(approach(c, rho, end), z_end + rise, lead_ccw, p.feeds.cut)?;
            }
        }
        b.vertical(h.retract.max(b.at()?.z), p.feeds.plunge)?;
    }
    let last = b.at()?;
    b.rapid(Vec3::new(last.x, last.y, h.clearance))?;
    let path = Toolpath { moves: b.moves };
    verify(&path, &h)?;
    Ok(HelicalPath { path, warnings })
}
