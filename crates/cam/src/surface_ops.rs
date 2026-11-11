//! 3-axis surface operations on a [`crate::Surface`]: raster finishing ([`parallel`]), layered
//! roughing of a stock box ([`roughing`]) and z-level finishing of steep walls ([`waterline`]).
//! The tool axis is +Z of whatever coordinates the surface is given in, so an indexed (3+2 or
//! 3+1) setup runs these on the surface turned into its frame ([`crate::Surface::in_frame`]),
//! with the boundary and heights given in that frame too. Nothing here assumes where the part
//! sits.
//!
//! Every point the tool cuts at is placed with the drop-cutter, so no cutting point is below
//! where the cutter rests on the surface (raised by the stock to leave). Passes are sampled at a
//! base step (at most the cutter's radius) and refined where the surface bends: wherever the
//! dropped height half way along a stretch, or at either of two points near its thirds, is
//! more than the tolerance from the straight line between its ends, the stretch is halved,
//! down to stretches shorter than the tolerance. No stretch longer than a cap tied to the
//! cutter is taken as straight: under a corner radius short enough that the corner's bend
//! keeps it within the tolerance, under a flat cutter its radius. A stretch still out of
//! tolerance then (a vertical wall) is crossed over its highest point: the tool rises before it
//! moves across, or moves across before it goes down. Where a pass leaves the surface (or the
//! boundary) the crossing is found to within the tolerance by halving too. Sampling cannot see
//! everything between its samples: the tolerance holds for dropped heights that bend no tighter
//! than the samples can follow, and a feature far finer than the step (a staircase of many
//! treads within one stretch whose heights happen to lie on its chord) can still be cut
//! through.
//!
//! The heights are those of every 3-axis operation ([`Heights`]); the path is checked with
//! [`crate::verify`] against them before it is returned. Rapids run at the retract height, so it
//! must be at or above the highest point of the surface within reach of the boundary (plus the
//! stock to leave); an operation refuses heights that are not. A path with nothing to cut has
//! no moves.

use std::cell::Cell;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use solidcore_record::{Region2, Vec2, Vec3};
use solidcraft_geom::Ring;
use solidcraft_geom::prelude::*;

use crate::check::{verify, verify_over};
use crate::cutter::Cutter;
use crate::dropcutter::drop_z;
use crate::ops::{Feeds, Heights};
use crate::path::{MAX_MOVES, Move, Toolpath};
use crate::surface::Surface;
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// Most drop-cutter samples one operation starts from (passes times samples along them, or grid
/// nodes), before refinement. Beyond it the operation is refused rather than left running.
pub const MAX_SAMPLES: usize = 4_000_000;
/// Most nodes of a waterline's grid.
pub const MAX_GRID_NODES: usize = 4_000_000;
/// Most drop-cutter evaluations of one operation, refinement and link checks included.
pub(crate) const MAX_DROPS: usize = 40_000_000;
/// Most levels of one operation.
pub(crate) const MAX_LEVELS: usize = 10_000;
/// Most halvings of one base step.
const MAX_DEPTH: u32 = 24;
/// Where along a stretch, besides half way, the dropped height is tested against the chord:
/// `(3 - √5) / 2` and one less it, far from the simple fractions where a staircase of even
/// treads lies on the chord (quarters would miss any four treads).
const CHORD_TEST: f64 = 0.381_966_011_250_105_1;
/// Most samples along one link checked for clearance.
const MAX_LINK_SAMPLES: usize = 10_000;
/// Largest coordinate or height (mm).
const MAX_COORD: f64 = 1.0e6;
/// Smallest tolerance accepted (mm).
const MIN_TOL: f64 = 1e-6;
/// Most sample visits of one operation's levels: every level runs over the samples of every
/// pass (roughing) or every cell of the grid (waterline) where something can be cut there.
const MAX_WORK: f64 = 1.0e9;

/// The feed of a cutting move `len` long that goes down (into the material) by `drop`: the cut
/// feed, slowed so the tool never goes down faster than the plunge feed. A move straight down
/// is fed at the plunge feed and a level one at the cut feed, with no jump between.
pub(crate) fn descent_feed(feeds: &Feeds, len: f64, drop: f64) -> f64 {
    if !(drop > 1e-9) || !(len > 0.0) {
        return feeds.cut;
    }
    (feeds.plunge * len / drop).clamp(feeds.plunge.min(feeds.cut), feeds.cut)
}

/// The feed a finishing pass goes down the surface at, steeper than its descent limit: `given`,
/// else the cut feed (see [`ParallelParams::descent_feed`]).
pub(crate) fn descent_of(feeds: &Feeds, given: Option<f64>) -> Result<f64> {
    match given {
        None => Ok(feeds.cut),
        Some(f) if f.is_finite() && f > 0.0 && f <= 1.0e6 => Ok(f),
        Some(_) => invalid("the descent feed must be above 0"),
    }
}

/// The steepest a finishing move may go down at the cut feed by default: 70° from horizontal.
pub const DEFAULT_DESCENT_LIMIT: f64 = 70.0 * std::f64::consts::PI / 180.0;

fn default_descent_limit() -> f64 {
    DEFAULT_DESCENT_LIMIT
}

/// How far above the material left the tool stops coming down at rapid before it feeds into it
/// (mm), where that is below the retract height.
pub const ENTRY_GAP: f64 = 1.0;
/// How far above a level a dropped height must be to count as solid there (relative to the
/// size of the level, at least 1): a surface resting exactly at the level is clear.
const LEVEL_EPS: f64 = 1e-9;

/// Where the cutter's centre may go, seen along the tool axis.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Boundary {
    /// Low and high corners of the rectangle the cutter's centre stays in; left out, the
    /// surface's extent seen from above (for a waterline grown by the cutter's radius, stock to
    /// leave included, and one grid step, so the contour round the outer walls is inside it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<[Vec2; 2]>,
    /// A region the cutter's centre must also stay inside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Region2>,
}

/// Raster finishing: passes side by side, each following the surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParallelParams {
    /// `bottom` is the floor: where the surface is lower the tool runs at it. `top` is not used.
    pub heights: Heights,
    pub feeds: Feeds,
    /// Distance between passes (mm), above 0 and at most the tool diameter.
    pub stepover: f64,
    /// Direction of the passes (radians, counter-clockwise from +X about the tool axis).
    pub angle: f64,
    #[serde(default)]
    pub boundary: Boundary,
    /// Material left on the surface (mm).
    pub stock_to_leave: f64,
    /// Distance between samples along a pass before refinement (mm); a step above the tool's
    /// radius is taken as the radius. Features narrower than it less the tool diameter can be
    /// missed where a pass leaves the surface.
    pub step: f64,
    /// Largest distance between the path and the surface's dropped height (mm).
    pub tolerance: f64,
    /// Every other pass runs back; otherwise all run the same way.
    pub zigzag: bool,
    /// Moves that go down steeper than this from horizontal (radians, 0 to a right angle) are
    /// descents: they are slowed so the tool goes down no faster than the descent feed. Moves
    /// along the surface that go down less steeply run at the cut feed. 0 slows every move that
    /// goes down; a right angle none. Left out, [`DEFAULT_DESCENT_LIMIT`].
    #[serde(default = "default_descent_limit")]
    pub descent_limit: f64,
    /// Feed for the moves inside a pass that go down the surface at least as steeply as the
    /// descent limit (mm/min); entries from above always come down at the plunge feed. Left out,
    /// the cut feed: the pass runs over material a roughing left (the stock to leave), so going
    /// down a wall is a side cut, not a plunge. Give the plunge feed when finishing straight
    /// from uncut stock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descent_feed: Option<f64>,
}

/// Roughing a stock box in levels: at each level the tool clears what is left above it, raster
/// fashion, staying `stock_to_leave` clear of the part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoughingParams {
    /// `top` is the top of the stock, `bottom` the lowest level.
    pub heights: Heights,
    pub feeds: Feeds,
    /// Most material taken per level (mm).
    pub stepdown: f64,
    /// Distance between passes (mm), above 0 and at most the tool diameter.
    pub stepover: f64,
    /// Direction of the passes (radians, counter-clockwise from +X about the tool axis).
    pub angle: f64,
    /// The stock's extent seen from above, as the cutter's centre may reach it.
    #[serde(default)]
    pub boundary: Boundary,
    /// Material left on the part (mm).
    pub stock_to_leave: f64,
    /// Distance between samples along a pass before refinement (mm); a step above the tool's
    /// radius is taken as the radius.
    pub step: f64,
    /// Largest distance between the path and the surface's dropped height (mm).
    pub tolerance: f64,
    /// Every other pass runs back; otherwise all run the same way.
    pub zigzag: bool,
}

/// Z-level finishing: at each level the tool runs round the part where the dropped cutter
/// meets that level, which follows steep walls closely.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaterlineParams {
    /// Levels run from `top` down to `bottom`, both included. A level at a floor traces the foot
    /// of the walls standing on it.
    pub heights: Heights,
    pub feeds: Feeds,
    /// Most distance between levels (mm).
    pub stepdown: f64,
    #[serde(default)]
    pub boundary: Boundary,
    /// Material left on the walls (mm).
    pub stock_to_leave: f64,
    /// Spacing of the grid the levels are traced on (mm): the loops' points are about this far
    /// apart where the wall is straight, closer where it bulges between them. A solid strip
    /// narrower than it can be missed between the grid's nodes.
    pub step: f64,
    /// How close to the level each point is found (mm).
    pub tolerance: f64,
    /// Climb milling (the wall on the right of the tool's travel); otherwise conventional.
    pub climb: bool,
}

fn finite_in(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && v >= lo && v <= hi
}

pub(crate) fn check_tolerance(tol: f64) -> Result<()> {
    if finite_in(tol, MIN_TOL, 100.0) { Ok(()) } else { invalid("the tolerance must be from 0.000001 to 100 mm") }
}

pub(crate) fn check_step(step: f64) -> Result<()> {
    if finite_in(step, 1e-4, MAX_COORD) { Ok(()) } else { invalid("the sample step must be from 0.0001 mm to 1000 m") }
}

pub(crate) fn check_leave(leave: f64) -> Result<()> {
    if finite_in(leave, 0.0, 1000.0) { Ok(()) } else { invalid("the stock to leave must be from 0 to 1000 mm") }
}

fn check_stepover(stepover: f64, tool: &Tool) -> Result<()> {
    if stepover > 0.0 && stepover <= tool.diameter { Ok(()) } else { invalid("the stepover must be above 0 and at most the tool diameter") }
}

fn check_descent_limit(angle: f64) -> Result<()> {
    if finite_in(angle, 0.0, std::f64::consts::FRAC_PI_2) { Ok(()) } else { invalid("the descent limit must be from 0 to 90 degrees") }
}

pub(crate) fn check_angle(angle: f64) -> Result<()> {
    if finite_in(angle, -1000.0, 1000.0) { Ok(()) } else { invalid("the pass angle must be a finite number of radians") }
}

pub(crate) fn check_stepdown(stepdown: f64) -> Result<()> {
    if finite_in(stepdown, 1e-4, MAX_COORD) { Ok(()) } else { invalid("the stepdown must be above 0") }
}

/// The tool's cutting end, refused for a tool that cannot mill.
pub(crate) fn tool_cutter(tool: &Tool) -> Result<Cutter> {
    tool.validate()?;
    crate::ops::cuts_sideways(tool)?;
    tool.cutter()
}

/// How many halvings take `step` below `tol`.
pub(crate) fn depth_for(step: f64, tol: f64) -> u32 {
    let d = (step / tol).log2().ceil();
    if d.is_finite() { (d.max(2.0) as u32).min(MAX_DEPTH) } else { MAX_DEPTH }
}

/// Levels from `top` down to `bottom`, evenly spaced at most `stepdown` apart: `top` itself
/// first when `with_top`, `bottom` always last.
pub(crate) fn levels(top: f64, bottom: f64, stepdown: f64, with_top: bool) -> Result<Vec<f64>> {
    let n = ((top - bottom) / stepdown - 1e-9).ceil().max(0.0);
    if !(n <= MAX_LEVELS as f64) {
        return Err(CamError::TooLarge(format!("more than {MAX_LEVELS} levels")));
    }
    let n = n as usize;
    if n == 0 {
        return Ok(vec![bottom]);
    }
    let first = usize::from(!with_top);
    Ok((first..=n).map(|k| if k == n { bottom } else { top - (top - bottom) * k as f64 / n as f64 }).collect())
}

/// Counts drop-cutter evaluations against [`MAX_DROPS`], shared by the threads that plan an
/// operation's levels side by side.
pub(crate) struct Budget(std::sync::atomic::AtomicUsize);

impl Budget {
    pub(crate) fn new() -> Budget {
        Budget(std::sync::atomic::AtomicUsize::new(MAX_DROPS))
    }

    pub(crate) fn spend(&self) -> Result<()> {
        use std::sync::atomic::Ordering::Relaxed;
        let left = self.0.load(Relaxed);
        // A stop asked for from another thread ends the samples (looked at now and then).
        if left.is_multiple_of(256) {
            crate::stop::check()?;
        }
        if self.0.try_update(Relaxed, Relaxed, |l| l.checked_sub(1)).is_err() {
            return Err(CamError::TooLarge(format!("the operation needs more than {MAX_DROPS} drop-cutter samples; use a larger step or tolerance")));
        }
        Ok(())
    }
}

/// What is under the cutter at one sample of a pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Tip {
    /// The cutter's centre is outside the boundary: nothing is cut here.
    Out,
    /// Nothing is under the cutter.
    Clear,
    /// The tip's height along the tool axis where the cutter rests on the surface, the stock
    /// to leave included.
    At(f64),
}

impl Tip {
    fn kind(self) -> u8 {
        match self {
            Tip::Out => 0,
            Tip::Clear => 1,
            Tip::At(_) => 2,
        }
    }
}

/// A sampled pass: the parameter along it and what is under the cutter there, in order.
pub(crate) type Profile = Vec<(f64, Tip)>;

fn push_sample(out: &mut Profile, s: (f64, Tip)) -> Result<()> {
    if out.len() >= MAX_MOVES {
        return Err(CamError::TooLarge(format!("a pass has more than {MAX_MOVES} points")));
    }
    out.push(s);
    Ok(())
}

/// One pass to sample: `tip` says what is under the cutter at a parameter, `point` where the
/// tip is at a parameter and height (to measure the chord error).
pub(crate) struct Pass<'a> {
    pub tip: &'a dyn Fn(f64) -> Result<Tip>,
    pub point: &'a dyn Fn(f64, f64) -> Vec3,
    pub tol: f64,
}

impl Pass<'_> {
    /// Samples `t0..=t1` at `n` even steps, refining each at most `depth` halvings deep, taking
    /// no stretch longer than `cap` (in the parameter) as straight while it can still be halved.
    pub(crate) fn sample_within(&self, t0: f64, t1: f64, n: usize, depth: u32, cap: f64) -> Result<Profile> {
        self.sample_within_from((t0, (self.tip)(t0)?), t1, n, depth, cap)
    }

    /// [`Pass::sample_within`] from the sample `first` already taken at its start (the end of
    /// the stretch before), so that point is not dropped on again.
    pub(crate) fn sample_within_from(&self, first: (f64, Tip), t1: f64, n: usize, depth: u32, cap: f64) -> Result<Profile> {
        let n = n.max(1);
        let mut out = Vec::new();
        let t0 = first.0;
        let mut a = first;
        push_sample(&mut out, a)?;
        for k in 1..=n {
            let t = if k == n { t1 } else { t0 + (t1 - t0) * k as f64 / n as f64 };
            let b = (t, (self.tip)(t)?);
            self.refine(a, b, depth, cap, &mut out)?;
            a = b;
        }
        Ok(out)
    }

    /// Whether the sample `s` is within the tolerance of the straight line from `a` to `b`
    /// (the points of samples on the surface).
    fn on_chord(&self, a: (f64, Vec3), b: (f64, Vec3), s: (f64, Tip)) -> bool {
        let Tip::At(h) = s.1 else { return false };
        let u = if b.0 != a.0 { (s.0 - a.0) / (b.0 - a.0) } else { 0.5 };
        (self.point)(s.0, h).dist(a.1.lerp(b.1, u)) <= self.tol
    }

    /// Adds the points after `a` up to `b`.
    fn refine(&self, a: (f64, Tip), b: (f64, Tip), depth: u32, cap: f64, out: &mut Profile) -> Result<()> {
        let both_at = matches!((a.1, b.1), (Tip::At(_), Tip::At(_)));
        if a.1.kind() == b.1.kind() && !both_at {
            return push_sample(out, b);
        }
        if depth == 0 && !both_at {
            // A change of what is under the cutter, found to within the finest step: the tool
            // crosses it at the surface's height, rising over clear space before the surface
            // or going on over it after the surface.
            match (a.1, b.1) {
                (Tip::Clear, Tip::At(h)) => push_sample(out, (a.0, Tip::At(h)))?,
                (Tip::At(h), Tip::Clear) => push_sample(out, (b.0, Tip::At(h)))?,
                _ => {}
            }
            return push_sample(out, b);
        }
        let tm = 0.5 * (a.0 + b.0);
        let m = (tm, (self.tip)(tm)?);
        if let (Tip::At(ha), Tip::At(hm), Tip::At(hb)) = (a.1, m.1, b.1) {
            // A stretch longer than the cap is halved untested while it can be.
            if (b.0 - a.0).abs() > cap && depth > 0 {
                self.refine(a, m, depth - 1, cap, out)?;
                return self.refine(m, b, depth - 1, cap, out);
            }
            // The dropped height is tested against the chord half way and at two points that
            // are no simple fraction of the stretch, so a ramp or a step symmetric about the
            // middle, or a staircase of even treads, is not taken for a straight stretch.
            let (pa, pb) = ((a.0, (self.point)(a.0, ha)), (b.0, (self.point)(b.0, hb)));
            let mut top = ha.max(hm).max(hb);
            let mut straight = self.on_chord(pa, pb, m);
            for u in [CHORD_TEST, 1.0 - CHORD_TEST] {
                let t = a.0 + (b.0 - a.0) * u;
                let q = (t, (self.tip)(t)?);
                straight &= self.on_chord(pa, pb, q);
                if let Tip::At(h) = q.1 {
                    top = top.max(h);
                }
            }
            if straight {
                return push_sample(out, b);
            }
            if depth == 0 {
                // Still out of tolerance over the finest step: cross over the highest point.
                if top > ha {
                    push_sample(out, (a.0, Tip::At(top)))?;
                }
                if top > hb {
                    push_sample(out, (b.0, Tip::At(top)))?;
                }
                return push_sample(out, b);
            }
        } else if let (0, Tip::At(ha), Tip::At(hb)) = (depth, a.1, b.1) {
            // A gap narrower than the finest step between two points on the surface: bridged.
            let top = ha.max(hb);
            if top > ha {
                push_sample(out, (a.0, Tip::At(top)))?;
            }
            if top > hb {
                push_sample(out, (b.0, Tip::At(top)))?;
            }
            return push_sample(out, b);
        }
        self.refine(a, m, depth - 1, cap, out)?;
        self.refine(m, b, depth - 1, cap, out)
    }
}

/// The stretches of a sampled pass that cut, as `(parameter, height)`. Heights are raised to
/// `level`; a stretch cuts where its height is below `above` (the material already removed
/// down to there); where nothing is under the cutter the tool cuts at `level` if `clear_cuts`
/// (stock with no part under it) and does not otherwise. Where the straight line between two
/// samples crosses `level` or `above` a point is added there, its height from `rest` (where
/// the cutter rests at that parameter), so every point is at or above the surface.
pub(crate) fn spans(
    profile: &[(f64, Tip)],
    level: f64,
    above: f64,
    clear_cuts: bool,
    rest: &dyn Fn(f64) -> Result<Tip>,
) -> Result<Vec<Vec<(f64, f64)>>> {
    let mut out = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    let flush = |cur: &mut Vec<(f64, f64)>, out: &mut Vec<Vec<(f64, f64)>>| {
        if cur.len() >= 2 {
            out.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    let cuts = |z: f64| z < above - 1e-9;
    let mut prev: Option<(f64, Tip)> = None;
    for &(t, tip) in profile {
        if let Some((pt, Tip::At(a))) = prev
            && let Tip::At(b) = tip
        {
            let mut xs: Vec<f64> = [level, above]
                .into_iter()
                .filter(|c| c.is_finite() && (a - c) * (b - c) < 0.0)
                .map(|c| (c - a) / (b - a))
                .filter(|u| u.is_finite() && *u > 0.0 && *u < 1.0)
                .collect();
            xs.sort_by(f64::total_cmp);
            for u in xs {
                // A crossing point is on the edge of what cuts: it belongs to the stretch.
                let tc = pt + (t - pt) * u;
                let h = match rest(tc)? {
                    Tip::At(h) => h.max(a + (b - a) * u),
                    _ => a + (b - a) * u,
                };
                cur.push((tc, h.max(level)));
            }
        }
        let z = match tip {
            Tip::At(h) => Some(h.max(level)),
            Tip::Clear if clear_cuts => Some(level),
            _ => None,
        };
        match z {
            Some(z) if cuts(z) => cur.push((t, z)),
            _ => flush(&mut cur, &mut out),
        }
        prev = Some((t, tip));
    }
    flush(&mut cur, &mut out);
    Ok(out)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Rect {
    pub(crate) min: Vec2,
    pub(crate) max: Vec2,
}

/// The region of a boundary as polylines, for fast containment.
pub(crate) struct Fence {
    outer: Ring<Vec2>,
    holes: Vec<Ring<Vec2>>,
}

impl Fence {
    pub(crate) fn new(region: &Region2, tol: f64) -> Fence {
        Fence { outer: region.outer.polyline(tol), holes: region.holes.iter().map(|h| h.polyline(tol)).collect() }
    }

    pub(crate) fn contains(&self, p: Vec2) -> bool {
        self.outer.contains(p) && !self.holes.iter().any(|h| h.contains(p))
    }
}

impl Boundary {
    pub(crate) fn resolve(&self, surface: &Surface) -> Result<Rect> {
        let (min, max) = match self.rect {
            Some([a, b]) => (a, b),
            None => {
                let b = surface.bounds();
                if b.is_empty() {
                    return invalid("the surface has no triangles: give the boundary's rectangle");
                }
                (Vec2::new(b.min.x, b.min.y), Vec2::new(b.max.x, b.max.y))
            }
        };
        if ![min.x, min.y, max.x, max.y].iter().all(|v| finite_in(*v, -MAX_COORD, MAX_COORD)) {
            return invalid("the boundary's corners must be finite numbers within 1000 m");
        }
        if !(min.x <= max.x) || !(min.y <= max.y) {
            return invalid("the boundary's low corner must not be above or right of its high corner");
        }
        Ok(Rect { min, max })
    }
}

/// The surface seen by a vertical cutter: what is under it at each point of the plane.
pub(crate) struct Field<'a> {
    surface: &'a Surface,
    /// The cutter grown by the stock to leave.
    pub(crate) cutter: Cutter,
    leave: f64,
    fence: Option<Fence>,
    budget: Budget,
    /// The longest stretch of a pass taken as straight ([`stretch_cap`]).
    pub(crate) stretch: f64,
}

impl<'a> Field<'a> {
    pub(crate) fn new(surface: &'a Surface, cutter: &Cutter, leave: f64, region: Option<&Region2>, tol: f64) -> Result<Field<'a>> {
        let grown = cutter.grown(leave)?;
        Ok(Field {
            surface,
            cutter: grown,
            leave,
            fence: region.map(|r| Fence::new(r, tol.min(1e-3))),
            budget: Budget::new(),
            stretch: stretch_cap(&grown, tol),
        })
    }

    pub(crate) fn at(&self, p: Vec2) -> Result<Tip> {
        if self.fence.as_ref().is_some_and(|f| !f.contains(p)) {
            return Ok(Tip::Out);
        }
        self.budget.spend()?;
        Ok(match drop_z(self.surface, &self.cutter, p.x, p.y) {
            Some(z) => Tip::At(z + self.leave),
            None => Tip::Clear,
        })
    }

    /// Where the tip rests on the surface at `p` (the stock to leave included), the boundary
    /// aside; `None` where nothing is under the cutter.
    pub(crate) fn rest(&self, p: Vec2) -> Result<Option<f64>> {
        self.budget.spend()?;
        Ok(drop_z(self.surface, &self.cutter, p.x, p.y).map(|z| z + self.leave))
    }

    /// The highest the tip can rest anywhere within reach of the rectangle; `None` when no
    /// triangle is in reach.
    fn top(&self, r: &Rect) -> Option<f64> {
        let reach = self.cutter.radius;
        let (lo, hi) = (Vec2::new(r.min.x - reach, r.min.y - reach), Vec2::new(r.max.x + reach, r.max.y + reach));
        let top = Cell::new(f64::NEG_INFINITY);
        self.surface.visit(
            |b| b.max.x >= lo.x && b.min.x <= hi.x && b.max.y >= lo.y && b.min.y <= hi.y && b.max.z > top.get(),
            |_, t| {
                if let Some(z) = top_within(t, lo, hi)
                    && z > top.get()
                {
                    top.set(z);
                }
            },
        );
        let z = top.get();
        z.is_finite().then_some(z + self.leave)
    }

    /// Refuses a retract height a sideways rapid at it could hit the part from.
    pub(crate) fn check_retract(&self, r: &Rect, retract: f64) -> Result<()> {
        match self.top(r) {
            Some(top) if retract < top - 1e-9 => {
                invalid(format!("the retract height {retract} is below the top of the surface ({top}, stock to leave included)"))
            }
            _ => Ok(()),
        }
    }
}

/// The highest point of the triangle over the rectangle `lo..hi` (seen from above): the
/// triangle cut to it side by side; `None` when they do not overlap.
fn top_within(t: &[Vec3; 3], lo: Vec2, hi: Vec2) -> Option<f64> {
    let mut poly: Vec<Vec3> = t.to_vec();
    // Each side as (axis, bound, keep the side above the bound).
    for (axis, bound, above) in [(0, lo.x, true), (0, hi.x, false), (1, lo.y, true), (1, hi.y, false)] {
        let coord = |p: &Vec3| if axis == 0 { p.x } else { p.y };
        let keep = |p: &Vec3| if above { coord(p) >= bound } else { coord(p) <= bound };
        let mut next = Vec::with_capacity(poly.len() + 1);
        for (i, a) in poly.iter().enumerate() {
            let Some(b) = poly.get((i + 1) % poly.len()) else { continue };
            if keep(a) {
                next.push(*a);
            }
            if keep(a) != keep(b) {
                let (ca, cb) = (coord(a), coord(b));
                let u = if (cb - ca).abs() > 1e-300 { ((bound - ca) / (cb - ca)).clamp(0.0, 1.0) } else { 0.0 };
                next.push(a.lerp(*b, u));
            }
        }
        poly = next;
        if poly.is_empty() {
            return None;
        }
    }
    poly.iter().map(|p| p.z).reduce(f64::max)
}

/// One raster pass: the points `origin + dir·t` for `t` in `t0..=t1`.
#[derive(Clone, Copy, Debug)]
struct Line {
    origin: Vec2,
    dir: Vec2,
    t0: f64,
    t1: f64,
}

impl Line {
    fn at(&self, t: f64) -> Vec2 {
        self.origin + self.dir * t
    }

    fn sample(&self, field: &Field, step: f64, tol: f64) -> Result<Profile> {
        let tip = |t: f64| field.at(self.at(t));
        let point = |t: f64, h: f64| self.at(t).extend(h);
        let pass = Pass { tip: &tip, point: &point, tol };
        let n = ((self.t1 - self.t0) / step).ceil().max(1.0) as usize;
        pass.sample_within(self.t0, self.t1, n, depth_for(step, tol), field.stretch)
    }
}

/// The longest stretch of a pass taken as straight without halving it (mm). Under a cutter
/// with a corner the dropped height bends no tighter than the corner, so a stretch of
/// `√(8·corner·tol)` departs from its chord by at most the tolerance where the surface is not
/// steep; a flat cutter gets its radius. Never above the cutter's radius, nor below the
/// tolerance.
fn stretch_cap(c: &Cutter, tol: f64) -> f64 {
    let bend = if c.corner > 0.0 { (8.0 * c.corner * tol).sqrt() } else { f64::INFINITY };
    bend.min(c.radius).max(tol)
}

/// Passes at `angle` across the rectangle, at most `stepover` apart, from one side to the other.
fn raster(r: &Rect, angle: f64, stepover: f64, step: f64) -> Result<Vec<Line>> {
    let dir = Vec2::from_angle(angle);
    let nrm = dir.perp();
    let corners = [r.min, Vec2::new(r.max.x, r.min.y), r.max, Vec2::new(r.min.x, r.max.y)];
    let (mut v0, mut v1) = (f64::INFINITY, f64::NEG_INFINITY);
    for c in corners {
        v0 = v0.min(c.dot(nrm));
        v1 = v1.max(c.dot(nrm));
    }
    let gaps = ((v1 - v0) / stepover - 1e-9).ceil().max(0.0);
    if !(gaps < MAX_SAMPLES as f64) {
        return Err(CamError::TooLarge(format!("more than {MAX_SAMPLES} passes; use a larger stepover")));
    }
    let gaps = gaps as usize;
    let mut lines = Vec::new();
    let mut samples = 0.0;
    for k in 0..=gaps {
        let v = if gaps == 0 { 0.5 * (v0 + v1) } else { v0 + (v1 - v0) * k as f64 / gaps as f64 };
        let origin = nrm * v;
        // The stretch of the line inside the rectangle, slab by slab.
        let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
        for (o, d, lo, hi) in [(origin.x, dir.x, r.min.x, r.max.x), (origin.y, dir.y, r.min.y, r.max.y)] {
            if d.abs() <= 1e-12 {
                if o < lo - 1e-9 || o > hi + 1e-9 {
                    t1 = f64::NEG_INFINITY;
                }
                continue;
            }
            let (a, b) = ((lo - o) / d, (hi - o) / d);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        if !(t0 <= t1) || !t0.is_finite() || !t1.is_finite() {
            continue;
        }
        samples += ((t1 - t0) / step).ceil() + 1.0;
        if !(samples <= MAX_SAMPLES as f64) {
            return Err(CamError::TooLarge(format!("more than {MAX_SAMPLES} samples; use a larger step or stepover")));
        }
        lines.push(Line { origin, dir, t0, t1 });
    }
    Ok(lines)
}

/// Collects moves, keeping where the tool is.
pub(crate) struct Out3 {
    moves: Vec<Move>,
    pos: Option<Vec3>,
    /// Whether the tool is down in the material at the end of a cut it may link on from.
    down: bool,
    h: Heights,
    feeds: Feeds,
    /// A move that goes down by at least this share of its length is slowed by
    /// [`descent_feed`]; 0 slows every move that goes down, infinity none.
    plunge_slope: f64,
    /// The feed such a move goes down no faster than: the plunge feed, or a finishing pass's
    /// descent feed ([`Out3::descending`]).
    descent: f64,
}

/// How the tool gets from the end of one cut (the first point) to the start of the next (the
/// second) at feed: the points after where it is, the last the next cut's start; `None` where
/// it must lift and come down again instead.
pub(crate) type Link<'a> = &'a dyn Fn(Vec3, Vec3) -> Result<Option<Vec<Vec3>>>;

impl Out3 {
    /// `descent_limit`: moves that go down at least this steeply (radians from horizontal)
    /// are slowed by [`descent_feed`].
    pub(crate) fn new(h: Heights, feeds: Feeds, descent_limit: f64) -> Out3 {
        // A right angle (or more): no move inside a pass is slowed.
        let plunge_slope = if descent_limit >= std::f64::consts::FRAC_PI_2 - 1e-12 { f64::INFINITY } else { descent_limit.sin().max(0.0) };
        Out3 { moves: Vec::new(), pos: None, down: false, h, feeds, plunge_slope, descent: feeds.plunge }
    }

    /// Moves inside a pass that go down at least as steeply as the descent limit are slowed to
    /// go down no faster than `feed` (entries from above still come down at the plunge feed).
    pub(crate) fn descending(mut self, feed: f64) -> Out3 {
        self.descent = feed;
        self
    }

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

    /// Straight up to the retract height, if below it.
    pub(crate) fn lift(&mut self) -> Result<()> {
        self.down = false;
        match self.pos {
            Some(q) if q.z < self.h.retract => self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.retract) }),
            _ => Ok(()),
        }
    }

    /// Up, across at the retract height (the clearance height for the first move) and down
    /// to `p`, the last of it at the plunge feed: from the retract height, or, where the
    /// material left over `p` is known to reach no higher than `top`, from [`ENTRY_GAP`] above
    /// that, coming down through the air above it at rapid.
    fn enter(&mut self, p: Vec3, top: Option<f64>) -> Result<()> {
        match self.pos {
            None => self.push(Move::Rapid { to: Vec3::new(p.x, p.y, self.h.clearance) })?,
            Some(q) => {
                self.lift()?;
                if (q.x - p.x).hypot(q.y - p.y) > 1e-12 {
                    self.push(Move::Rapid { to: Vec3::new(p.x, p.y, q.z.max(self.h.retract)) })?;
                }
            }
        }
        if self.pos.is_some_and(|q| q.z > self.h.retract) {
            self.push(Move::Rapid { to: Vec3::new(p.x, p.y, self.h.retract) })?;
        }
        if let Some(t) = top {
            let air = (t + ENTRY_GAP).max(p.z);
            if self.pos.is_some_and(|q| q.z > air + 1e-9) {
                self.push(Move::Rapid { to: Vec3::new(p.x, p.y, air) })?;
            }
        }
        if self.pos.is_some_and(|q| q.dist(p) > 1e-12) {
            self.push(Move::Feed { to: p, feed: self.feeds.plunge })?;
        }
        self.down = true;
        Ok(())
    }

    fn feed(&mut self, p: Vec3, feed: f64) -> Result<()> {
        if self.pos.is_some_and(|q| q.dist(p) <= 1e-12) {
            return Ok(());
        }
        self.push(Move::Feed { to: p, feed })
    }

    /// A cut to `p`: at the cut feed, or slowed by [`descent_feed`] where it goes down at
    /// least as steeply as the plunge slope.
    fn cut(&mut self, p: Vec3) -> Result<()> {
        let feed = match self.pos {
            Some(q) => {
                let (len, drop) = (q.dist(p), q.z - p.z);
                if drop > 1e-9 && drop >= self.plunge_slope * len {
                    descent_feed(&Feeds { plunge: self.descent, ..self.feeds }, len, drop)
                } else {
                    self.feeds.cut
                }
            }
            None => self.feeds.cut,
        };
        self.feed(p, feed)
    }

    /// Cuts along `pts`, getting to the first point along the way `link` gives when it gives
    /// one and from above otherwise ([`Out3::enter`], `top` the material left there). A move
    /// that goes down at least as steeply as the plunge slope (into the material, off the
    /// part's reach into the stock below, down a wall) is slowed by [`descent_feed`] so the
    /// tool goes down no faster than the plunge feed.
    pub(crate) fn span(&mut self, pts: &[Vec3], link: Link<'_>, top: Option<f64>) -> Result<()> {
        let Some(&first) = pts.first() else { return Ok(()) };
        let way = match self.pos.filter(|_| self.down) {
            Some(q) => link(q, first)?,
            None => None,
        };
        match way {
            Some(way) => way.into_iter().try_for_each(|p| self.cut(p))?,
            None => self.enter(first, top)?,
        }
        for &p in pts.iter().skip(1) {
            self.cut(p)?;
        }
        self.down = true;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<Toolpath> {
        if let Some(q) = self.pos
            && q.z < self.h.clearance
        {
            self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.clearance) })?;
        }
        let path = Toolpath { moves: self.moves };
        verify(&path, &self.h)?;
        Ok(path)
    }

    /// [`Out3::finish`] for a finishing path, whose `heights.top` may be below the part: its
    /// rapids are checked against where the cutter rests on `field`'s surface instead
    /// ([`verify_over`]).
    pub(crate) fn finish_over(mut self, field: &Field) -> Result<Toolpath> {
        if let Some(q) = self.pos
            && q.z < self.h.clearance
        {
            self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.clearance) })?;
        }
        let path = Toolpath { moves: self.moves };
        verify_over(&path, &self.h, &|p| field.rest(p))?;
        Ok(path)
    }
}

/// The way the tool feeds from `a` to `b` (no longer than `max_len` seen from above): the
/// straight line, raised wherever the cutter resting on the surface (the stock to leave
/// included) is higher. The dropped height under the link is sampled every `spacing` and
/// refined like a pass, to half the tolerance, so a step in it between two samples (the edge
/// of a part beside the passes) is found and followed, and the link is no further into the
/// part than half the tolerance where the dropped height bends no tighter than the samples
/// can follow. The points after `a`, the last `b`; `None` where the link leaves the
/// boundary or needs more than [`MAX_LINK_SAMPLES`] points.
pub(crate) fn link_path(field: &Field, a: Vec3, b: Vec3, max_len: f64, spacing: f64, tol: f64) -> Result<Option<Vec<Vec3>>> {
    let len = (a.x - b.x).hypot(a.y - b.y);
    if !(len <= max_len) {
        return Ok(None);
    }
    if len <= 1e-12 {
        // Straight up or down: both ends are points the tool cuts at, on or above the surface.
        return Ok(match field.at(Vec2::new(a.x, a.y))? {
            Tip::Out => None,
            _ => Some(vec![b]),
        });
    }
    // Two samples with nothing under the cutter are not looked between, so they are taken no
    // farther apart than a stretch is taken as straight, nor than a quarter of the radius: a
    // corner of the part the link passes over between them is met.
    let spacing = spacing.min(field.stretch).min(0.25 * field.cutter.radius);
    let n = (len / spacing).ceil().max(1.0);
    if !(n <= MAX_LINK_SAMPLES as f64) {
        return Ok(None);
    }
    let xy = |u: f64| Vec2::new(a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u);
    let tip = |u: f64| field.at(xy(u));
    let point = |u: f64, h: f64| xy(u).extend(h);
    let half = 0.5 * tol;
    let pass = Pass { tip: &tip, point: &point, tol: half };
    let n = n as usize;
    let profile = pass.sample_within(0.0, 1.0, n, depth_for(len / n as f64, half), field.stretch * std::f64::consts::FRAC_1_SQRT_2 / len)?;
    if profile.len() > MAX_LINK_SAMPLES {
        return Ok(None);
    }
    let line = |u: f64| a.z + (b.z - a.z) * u;
    let on_line = |u: f64| xy(u).extend(line(u));
    let mut out: Vec<Vec3> = Vec::new();
    // The last sample and how far the surface is above the line there (-∞: nothing under it).
    let mut prev: Option<(f64, f64)> = None;
    for &(u, tip) in &profile {
        let above = match tip {
            Tip::Out => return Ok(None),
            Tip::Clear => f64::NEG_INFINITY,
            Tip::At(h) => h - line(u),
        };
        if let Some((pu, pa)) = prev
            && (pa > 1e-9) != (above > 1e-9)
        {
            // The way leaves the line or comes back to it between the samples: where the
            // surface crosses it, or over the edge where nothing is under the cutter.
            if pa.is_finite() && above.is_finite() {
                let uc = pu + (u - pu) * (pa / (pa - above));
                out.push(on_line(uc));
            } else {
                out.push(on_line(if above > 1e-9 { pu } else { u }));
            }
        }
        // Raised samples at either end (the tool rising before it moves across, or moving
        // across before it goes down) are kept too.
        if above > 1e-9 {
            out.push(xy(u).extend(line(u) + above));
        }
        prev = Some((u, above));
    }
    out.push(b);
    Ok(Some(out))
}

/// A pass's stretches in the order the tool runs them, as points.
fn ordered(line: &Line, stretches: Vec<Vec<(f64, f64)>>, backward: bool) -> Vec<Vec<Vec3>> {
    let mut out: Vec<Vec<Vec3>> = stretches.into_iter().map(|s| s.into_iter().map(|(t, z)| line.at(t).extend(z)).collect()).collect();
    if backward {
        out.reverse();
        out.iter_mut().for_each(|s| s.reverse());
    }
    out
}

/// Raster finishing: passes at `angle`, `stepover` apart, each following the surface as the
/// dropped cutter rests on it. Where nothing is under the cutter the tool lifts out and comes
/// back in where the surface starts again; where the surface is below the floor it runs at the
/// floor. Between passes (and across gaps) it feeds across when that link is short (at most
/// twice the stepover or the tool diameter) and stays inside the boundary: straight, raised
/// over the surface wherever the cutter resting on it is higher (sampled and refined like a
/// pass, so it does not cut past an edge between two samples); otherwise it retracts and
/// plunges again. A move that goes down at least as steeply as the descent limit is slowed so
/// the tool goes down no faster than the plunge feed, and so is every entry from above; the
/// rest runs at the cut feed.
pub fn parallel(surface: &Surface, tool: &Tool, p: &ParallelParams) -> Result<Toolpath> {
    let _span = solidcraft_profile::span("cam", "parallel");
    let cutter = tool_cutter(tool)?;
    p.heights.validate()?;
    p.feeds.validate()?;
    check_stepover(p.stepover, tool)?;
    check_angle(p.angle)?;
    check_leave(p.stock_to_leave)?;
    check_step(p.step)?;
    check_tolerance(p.tolerance)?;
    check_descent_limit(p.descent_limit)?;
    let descent = descent_of(&p.feeds, p.descent_feed)?;
    let rect = p.boundary.resolve(surface)?;
    let field = Field::new(surface, &cutter, p.stock_to_leave, p.boundary.region.as_ref(), p.tolerance)?;
    field.check_retract(&rect, p.heights.retract)?;
    let step = p.step.min(cutter.radius);
    let lines = raster(&rect, p.angle, p.stepover, step)?;
    let (max_link, spacing) = ((2.0 * p.stepover).max(tool.diameter), step.min(cutter.radius * 0.5));
    let link = |a: Vec3, b: Vec3| link_path(&field, a, b, max_link, spacing, p.tolerance);
    let mut out = Out3::new(p.heights, p.feeds, p.descent_limit).descending(descent);
    for (k, line) in lines.iter().enumerate() {
        let profile = line.sample(&field, step, p.tolerance)?;
        let stretches = spans(&profile, p.heights.bottom, f64::INFINITY, false, &|t| field.at(line.at(t)))?;
        for s in ordered(line, stretches, p.zigzag && k % 2 == 1) {
            out.span(&s, &link, None)?;
        }
    }
    out.finish_over(&field)
}

/// What is under the cutter over a set of samples (a pass, a grid), to skip the levels that
/// cut nothing there.
#[derive(Clone, Copy, Debug)]
struct TipRange {
    /// The lowest and highest dropped heights.
    min: f64,
    max: f64,
    /// Whether nothing is under the cutter somewhere (stock with no part under it).
    clear: bool,
}

impl TipRange {
    fn of(tips: impl IntoIterator<Item = Tip>) -> TipRange {
        let mut r = TipRange { min: f64::INFINITY, max: f64::NEG_INFINITY, clear: false };
        for tip in tips {
            match tip {
                Tip::At(h) => {
                    r.min = r.min.min(h);
                    r.max = r.max.max(h);
                }
                Tip::Clear => r.clear = true,
                Tip::Out => {}
            }
        }
        r
    }

    /// Whether a level at `level`, the material already cut down to `above`, cuts anything on
    /// the pass: a sample cuts at `max(level, height)` where that is below `above` (as in
    /// [`spans`]), and at the level where nothing is under the cutter.
    fn cuts(&self, level: f64, above: f64) -> bool {
        level < above - 1e-9 && (self.clear || self.min < above - 1e-9)
    }
}

/// Layered roughing of the stock box from `heights.top` down to `heights.bottom`: at each level
/// (at most a stepdown below the last) the tool runs raster passes at
/// `max(level, dropped height + stock to leave)`, only where that is below the level before
/// (what the earlier levels left), so it never cuts into the part and never cuts air it has
/// already cleared. Where no part is under the cutter it cuts at the level. Stretches are
/// linked as in [`parallel`]; each new stretch is entered by coming down at rapid through
/// the air the levels before cleared, to [`ENTRY_GAP`] above the material left there (the
/// level before, or the top of the stock), and plunging straight down from there at the
/// plunge feed: a ramp or helix entry is a later improvement, so use a centre-cutting tool.
/// Every move that goes down (off the part's reach into stock no level has cut yet, down a
/// slope of the part into the material) is slowed so the tool goes down no faster than the
/// plunge feed.
/// A level is skipped for a pass that has nothing to cut at it; an operation whose levels
/// would visit more than a billion samples between them is refused.
pub fn roughing(surface: &Surface, tool: &Tool, p: &RoughingParams) -> Result<Toolpath> {
    let _span = solidcraft_profile::span("cam", "roughing");
    let cutter = tool_cutter(tool)?;
    crate::chamfer::roughs(tool, "roughing")?;
    p.heights.validate()?;
    p.feeds.validate()?;
    check_stepdown(p.stepdown)?;
    check_stepover(p.stepover, tool)?;
    check_angle(p.angle)?;
    check_leave(p.stock_to_leave)?;
    check_step(p.step)?;
    check_tolerance(p.tolerance)?;
    let rect = p.boundary.resolve(surface)?;
    let field = Field::new(surface, &cutter, p.stock_to_leave, p.boundary.region.as_ref(), p.tolerance)?;
    field.check_retract(&rect, p.heights.retract)?;
    let step = p.step.min(cutter.radius);
    let lines = raster(&rect, p.angle, p.stepover, step)?;
    let levels = levels(p.heights.top, p.heights.bottom, p.stepdown, false)?;
    // The surface under each pass, sampled once for every level.
    let profiles = lines.iter().map(|l| l.sample(&field, step, p.tolerance)).collect::<Result<Vec<_>>>()?;
    let ranges: Vec<TipRange> = profiles.iter().map(|pr| TipRange::of(pr.iter().map(|s| s.1))).collect();
    // What was cut down to before each level, and the work of running every level.
    let aboves: Vec<f64> = std::iter::once(p.heights.top).chain(levels.iter().copied()).take(levels.len()).collect();
    let valid = aboves.iter().zip(&levels).filter(|(above, level)| **level < **above - 1e-9).count();
    let mut work = (levels.len() as f64) * (profiles.len() as f64);
    for (profile, range) in profiles.iter().zip(&ranges) {
        // The levels before going below the pass's lowest point (`aboves` falls level by level).
        let cutting = if range.clear { valid } else { aboves.partition_point(|a| *a - 1e-9 > range.min).min(valid) };
        work += profile.len() as f64 * cutting as f64;
    }
    if !(work <= MAX_WORK) {
        return Err(CamError::TooLarge(format!(
            "the roughing visits more than {MAX_WORK} samples over its levels; use fewer levels, a larger step or stepover, or a smaller boundary"
        )));
    }
    let (max_link, spacing) = ((2.0 * p.stepover).max(tool.diameter), step.min(cutter.radius * 0.5));
    let link = |a: Vec3, b: Vec3| link_path(&field, a, b, max_link, spacing, p.tolerance);
    let mut out = Out3::new(p.heights, p.feeds, 0.0);
    let mut above = p.heights.top;
    for level in levels {
        // Each level starts from above: never a link down into the next level.
        out.lift()?;
        for (k, ((line, profile), range)) in lines.iter().zip(&profiles).zip(&ranges).enumerate() {
            // A pass with nothing to cut at this level costs no more than this test.
            if !range.cuts(level, above) {
                continue;
            }
            let stretches = spans(profile, level, above, true, &|t| field.at(line.at(t)))?;
            for s in ordered(line, stretches, p.zigzag && k % 2 == 1) {
                // Over every point of a stretch the levels before cut down to `above`.
                out.span(&s, &link, Some(above))?;
            }
        }
        above = level;
    }
    out.finish()
}

/// Z-level finishing: for each level from `heights.top` down to `heights.bottom` the
/// cutter-location contours where the dropped cutter's tip is exactly at the level, traced
/// over a grid of drop-cutter samples (marching squares) and run at the level. Each crossing of
/// a grid edge is found by halving to within an eighth of the tolerance (so a loop round a
/// round wall lies on a circle closely enough to be written as arcs), on the side where the cutter is
/// clear, so no point is in the surface. A cell whose opposite corners disagree (a saddle) is
/// settled by a sample at its centre. Loops are closed; contours that leave the boundary are
/// open. Where the wall bulges between two points of a contour so that the straight feed
/// between them would cut into it by more than the tolerance, a point on the wall is added
/// between them. A dropped height at the level itself counts as clear, so a level at a floor
/// traces the foot of its walls. Each contour is entered from the retract height at the plunge
/// feed and left by retracting. Undercut walls, which a vertical tool cannot reach, are traced
/// where the tool can stand. Levels with no contour are skipped; an operation whose levels
/// would visit more than a billion grid cells between them is refused.
pub fn waterline(surface: &Surface, tool: &Tool, p: &WaterlineParams) -> Result<Toolpath> {
    let _span = solidcraft_profile::span("cam", "waterline");
    let cutter = tool_cutter(tool)?;
    p.heights.validate()?;
    p.feeds.validate()?;
    check_stepdown(p.stepdown)?;
    check_leave(p.stock_to_leave)?;
    check_step(p.step)?;
    check_tolerance(p.tolerance)?;
    // The grid covers the whole rectangle; the region clips the contours afterwards.
    let field = Field::new(surface, &cutter, p.stock_to_leave, None, p.tolerance)?;
    let mut rect = p.boundary.resolve(surface)?;
    if p.boundary.rect.is_none() {
        // The contour round the part's outer walls is up to the cutter's radius outside it.
        let grow = field.cutter.radius + p.step;
        rect = Rect { min: rect.min - Vec2::new(grow, grow), max: rect.max + Vec2::new(grow, grow) };
    }
    field.check_retract(&rect, p.heights.retract)?;
    let fence = p.boundary.region.as_ref().map(|r| Fence::new(r, p.tolerance.min(1e-3)));
    let (w, hgt) = (rect.max.x - rect.min.x, rect.max.y - rect.min.y);
    if !(w > 0.0) || !(hgt > 0.0) {
        return invalid("a waterline's boundary must have an area");
    }
    let (nx, ny) = ((w / p.step).ceil().max(1.0), (hgt / p.step).ceil().max(1.0));
    if !((nx + 1.0) * (ny + 1.0) <= MAX_GRID_NODES as f64) {
        return Err(CamError::TooLarge(format!("the waterline grid has more than {MAX_GRID_NODES} nodes; use a larger step")));
    }
    let (nx, ny) = (nx as usize, ny as usize);
    let grid = Grid::new(&field, rect, nx, ny)?;
    // Only a level with solid and clear nodes both has contours; each such level visits
    // every cell.
    let levels: Vec<f64> = levels(p.heights.top, p.heights.bottom, p.stepdown, true)?.into_iter().filter(|l| grid.mixed(*l)).collect();
    if !(levels.len() as f64 * (nx as f64 * ny as f64) <= MAX_WORK) {
        return Err(CamError::TooLarge(format!(
            "the waterline visits more than {MAX_WORK} grid cells over its levels; use fewer levels, a larger step or a smaller boundary"
        )));
    }
    let mut out = Out3::new(p.heights, p.feeds, 0.0);
    for level in levels {
        for chain in grid.contours(level, p.tolerance)? {
            for piece in grid.follow(&chain, level, p.tolerance, p.step)? {
                for mut run in clip(piece, fence.as_ref()) {
                    if !p.climb {
                        run.reverse();
                    }
                    let pts: Vec<Vec3> = run.iter().map(|q| q.extend(level)).collect();
                    let Some(&first) = pts.first() else { continue };
                    out.enter(first, None)?;
                    for &q in pts.iter().skip(1) {
                        out.feed(q, p.feeds.cut)?;
                    }
                    out.lift()?;
                }
            }
        }
    }
    out.finish_over(&field)
}

/// Whether the tip resting at `t` is above `level`: the material the tool must stay out of.
/// A tip resting at the level itself (within [`LEVEL_EPS`]) is clear, so a level at a floor
/// traces the foot of the walls on it.
fn solid(t: Tip, level: f64) -> bool {
    matches!(t, Tip::At(h) if h > level + LEVEL_EPS * level.abs().max(1.0))
}

/// The most halvings of one segment of a contour while following the wall.
const MAX_FOLLOW_DEPTH: u32 = 16;

/// The parts of a contour inside the fence (all of it without one), each with two points or
/// more. A closed contour that stays inside stays closed.
fn clip(chain: Vec<Vec2>, fence: Option<&Fence>) -> Vec<Vec<Vec2>> {
    let Some(fence) = fence else { return if chain.len() >= 2 { vec![chain] } else { vec![] } };
    let inside: Vec<bool> = chain.iter().map(|p| fence.contains(*p)).collect();
    if inside.iter().all(|i| *i) {
        return if chain.len() >= 2 { vec![chain] } else { vec![] };
    }
    let closed = chain.len() > 2 && chain.first().zip(chain.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-12);
    // A closed contour is started at a point outside, so no run is cut in two at its seam.
    let start = if closed { inside.iter().position(|i| !*i).unwrap_or(0) } else { 0 };
    let n = if closed { chain.len() - 1 } else { chain.len() };
    let order: Vec<usize> = (0..n).map(|k| (start + k) % n).chain(closed.then_some(start)).collect();
    let mut runs = Vec::new();
    let mut cur = Vec::new();
    for i in order {
        match (chain.get(i), inside.get(i)) {
            (Some(p), Some(true)) => cur.push(*p),
            _ => {
                if cur.len() >= 2 {
                    runs.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
        }
    }
    if cur.len() >= 2 {
        runs.push(cur);
    }
    runs
}

/// Drop-cutter samples over a rectangle, for tracing levels.
struct Grid<'a> {
    field: &'a Field<'a>,
    rect: Rect,
    nx: usize,
    ny: usize,
    tips: Vec<Tip>,
    /// What is under the cutter over all the nodes, to skip the levels without a contour.
    range: TipRange,
}

impl<'a> Grid<'a> {
    fn new(field: &'a Field<'a>, rect: Rect, nx: usize, ny: usize) -> Result<Grid<'a>> {
        let mut tips = Vec::with_capacity((nx + 1) * (ny + 1));
        let mut g = Grid { field, rect, nx, ny, tips: Vec::new(), range: TipRange::of([]) };
        for j in 0..=ny {
            for i in 0..=nx {
                tips.push(field.at(g.node(i, j))?);
            }
        }
        g.range = TipRange::of(tips.iter().copied());
        g.tips = tips;
        Ok(g)
    }

    /// Whether some nodes are solid at `level` and some clear: only then is there a contour.
    fn mixed(&self, level: f64) -> bool {
        let r = &self.range;
        solid(Tip::At(r.max), level) && (r.clear || !solid(Tip::At(r.min), level))
    }

    fn node(&self, i: usize, j: usize) -> Vec2 {
        let (r, nx, ny) = (&self.rect, self.nx as f64, self.ny as f64);
        let x = if i == self.nx { r.max.x } else { r.min.x + (r.max.x - r.min.x) * i as f64 / nx };
        let y = if j == self.ny { r.max.y } else { r.min.y + (r.max.y - r.min.y) * j as f64 / ny };
        Vec2::new(x, y)
    }

    fn tip(&self, i: usize, j: usize) -> Tip {
        self.tips.get(j * (self.nx + 1) + i).copied().unwrap_or(Tip::Clear)
    }

    /// The edge from node `(i, j)` along +X (`vertical` false) or +Y.
    fn edge(&self, i: usize, j: usize, vertical: bool) -> usize {
        2 * (j * (self.nx + 1) + i) + usize::from(vertical)
    }

    /// The contours at `level`, each a run of points with the material on its right: a closed
    /// one ends where it starts.
    fn contours(&self, level: f64, tol: f64) -> Result<Vec<Vec<Vec2>>> {
        let solid = |t: Tip| solid(t, level);
        let mut points: HashMap<usize, Vec2> = HashMap::new();
        // Each segment runs from the crossing on one edge to the crossing on another.
        let mut segs: Vec<(usize, usize)> = Vec::new();
        for j in 0..self.ny {
            for i in 0..self.nx {
                // Corners counter-clockwise, and the edge from each to the next.
                let c = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                let s = c.map(|(a, b)| solid(self.tip(a, b)));
                if s.iter().all(|v| *v == s[0]) {
                    continue;
                }
                let e = [self.edge(i, j, false), self.edge(i + 1, j, true), self.edge(i, j + 1, false), self.edge(i, j, true)];
                // (edge, entering the material going round counter-clockwise)
                let mut xs: Vec<(usize, bool)> = Vec::with_capacity(4);
                for k in 0..4 {
                    let (a, b) = (s[k], s[(k + 1) % 4]);
                    if a != b {
                        let (ia, ib) = (c[k], c[(k + 1) % 4]);
                        let (pin, pout) = if a { (ia, ib) } else { (ib, ia) };
                        if let std::collections::hash_map::Entry::Vacant(v) = points.entry(e[k]) {
                            v.insert(self.crossing(self.node(pin.0, pin.1), self.node(pout.0, pout.1), level, tol)?);
                        }
                        xs.push((e[k], b));
                    }
                }
                let joined_inside = if xs.len() == 4 {
                    let centre = self.node(i, j).lerp(self.node(i + 1, j + 1), 0.5);
                    solid(self.field.at(centre)?)
                } else {
                    false
                };
                let n = xs.len();
                for (k, &(edge, enters)) in xs.iter().enumerate() {
                    if !enters {
                        continue;
                    }
                    // Enter the material, then leave it at the next crossing round the cell;
                    // in a saddle whose middle is solid, at the crossing before.
                    let partner = if joined_inside && n == 4 { (k + n - 1) % n } else { (k + 1) % n };
                    if let Some(&(to, false)) = xs.get(partner) {
                        segs.push((edge, to));
                    }
                }
            }
        }
        Ok(chain(&segs, &points))
    }

    /// Where the level is crossed between a solid node and a clear one: the clear end of the
    /// last halving, within an eighth of `tol` of it.
    fn crossing(&self, solid_at: Vec2, clear_at: Vec2, level: f64, tol: f64) -> Result<Vec2> {
        let (mut a, mut b) = (solid_at, clear_at);
        for _ in 0..64 {
            if a.dist(b) <= 0.125 * tol {
                break;
            }
            let m = a.lerp(b, 0.5);
            if solid(self.field.at(m)?, level) {
                a = m;
            } else {
                b = m;
            }
        }
        Ok(b)
    }

    /// The contour `chain` (the material on its right) as runs the tool can feed along at
    /// `level`. Between two points of the grid's contour a straight feed would cut into a
    /// convex wall by the bulge of the wall between them, so wherever the segment is solid
    /// half way or at a quarter a point is added on the wall there, found by halving outward
    /// to within `tol`, and both halves are followed again; a segment within `tol` of the wall
    /// is kept. Where no clear point is found within two grid steps (`step`) of the segment,
    /// or the halvings run out, the tool does not feed across: the run ends there and the next
    /// one starts on the far side.
    fn follow(&self, chain: &[Vec2], level: f64, tol: f64, step: f64) -> Result<Vec<Vec<Vec2>>> {
        let Some(&start) = chain.first() else { return Ok(Vec::new()) };
        let closed = chain.len() > 2 && chain.last().is_some_and(|l| l.dist(start) <= 1e-12);
        let mut runs: Vec<Vec<Vec2>> = Vec::new();
        let mut cur = vec![start];
        for w in chain.windows(2) {
            let &[p, q] = w else { continue };
            if !self.segment(p, q, level, tol, step, MAX_FOLLOW_DEPTH, &mut cur)? {
                runs.push(std::mem::replace(&mut cur, vec![q]));
            }
        }
        runs.push(cur);
        if closed && runs.len() > 1 {
            // A loop broken somewhere: its first run goes on from where its last one ends.
            let first = runs.remove(0);
            if let Some(last) = runs.last_mut() {
                last.extend(first.into_iter().skip(1));
            }
        }
        runs.retain(|r| r.len() >= 2);
        Ok(runs)
    }

    /// Adds the points after `p` up to `q` to `out`, `false` (with nothing added) when the tool
    /// cannot feed from `p` to `q`.
    #[allow(clippy::too_many_arguments)]
    fn segment(&self, p: Vec2, q: Vec2, level: f64, tol: f64, step: f64, depth: u32, out: &mut Vec<Vec2>) -> Result<bool> {
        let Some(away) = (q - p).normalized().map(Vec2::perp) else {
            out.push(q);
            return Ok(true);
        };
        for u in [0.5, 0.25, 0.75] {
            let s = p.lerp(q, u);
            if !solid(self.field.at(s)?, level) {
                continue;
            }
            let Some(c) = self.clear_beside(s, away, level, tol, 2.0 * step)? else { return Ok(false) };
            if c.dist(s) <= tol {
                // The wall is within the tolerance of the segment here.
                continue;
            }
            let r = &self.rect;
            if depth == 0 || c.x < r.min.x || c.x > r.max.x || c.y < r.min.y || c.y > r.max.y {
                return Ok(false);
            }
            let mut part = Vec::new();
            if !self.segment(p, c, level, tol, step, depth - 1, &mut part)? || !self.segment(c, q, level, tol, step, depth - 1, &mut part)? {
                return Ok(false);
            }
            out.extend(part);
            return Ok(true);
        }
        out.push(q);
        Ok(true)
    }

    /// The first clear point from the solid point `s` along `dir` (a unit vector), no further
    /// than `reach`: found by doubling the distance from `tol`, then halving to within `tol`
    /// and taking the clear end. A point `tol` away when that is clear already.
    fn clear_beside(&self, s: Vec2, dir: Vec2, level: f64, tol: f64, reach: f64) -> Result<Option<Vec2>> {
        let (mut lo, mut hi) = (0.0, tol);
        let mut found = false;
        for _ in 0..64 {
            if !solid(self.field.at(s + dir * hi)?, level) {
                found = true;
                break;
            }
            if hi >= reach {
                break;
            }
            lo = hi;
            hi = (2.0 * hi).min(reach);
        }
        if !found {
            return Ok(None);
        }
        for _ in 0..64 {
            if hi - lo <= tol {
                break;
            }
            let m = 0.5 * (lo + hi);
            if solid(self.field.at(s + dir * m)?, level) {
                lo = m;
            } else {
                hi = m;
            }
        }
        Ok(Some(s + dir * hi))
    }
}

/// Joins segments that share edges into runs of points: open ones first, from their free
/// ends, then closed loops, each in the order its first segment was found.
fn chain(segs: &[(usize, usize)], points: &HashMap<usize, Vec2>) -> Vec<Vec<Vec2>> {
    let by_start: HashMap<usize, usize> = segs.iter().enumerate().map(|(i, s)| (s.0, i)).collect();
    let ends: std::collections::HashSet<usize> = segs.iter().map(|s| s.1).collect();
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    let walk = |first: usize, used: &mut Vec<bool>| {
        let mut pts = Vec::new();
        let Some(&(start, _)) = segs.get(first) else { return pts };
        pts.extend(points.get(&start).copied());
        let mut at = Some(first);
        while let Some(i) = at {
            match used.get_mut(i) {
                Some(u) if !*u => *u = true,
                _ => break,
            }
            let Some(&(_, end)) = segs.get(i) else { break };
            pts.extend(points.get(&end).copied());
            at = by_start.get(&end).copied();
        }
        pts
    };
    for (i, s) in segs.iter().enumerate() {
        if !ends.contains(&s.0) && !used.get(i).copied().unwrap_or(true) {
            out.push(walk(i, &mut used));
        }
    }
    for i in 0..segs.len() {
        if !used.get(i).copied().unwrap_or(true) {
            out.push(walk(i, &mut used));
        }
    }
    out.retain(|c| c.len() >= 2);
    out
}
