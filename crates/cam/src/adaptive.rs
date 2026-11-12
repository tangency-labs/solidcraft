//! Adaptive clearing: an engagement-limited way of clearing a region at a level, for machines
//! that cannot take a full-width cut (a small, not very rigid mill above all).
//!
//! The material still to cut at a level is kept as a raster of cells over the region: a cell is
//! material while no tool position so far has come within the tool's radius of its centre. The
//! cutter's centre is confined to where it may stand (inside a pocket's walls by the radius and
//! the stock to leave, or wherever the drop-cutter says it clears the part at the level), and
//! only material within reach of that is kept, so corners a round tool cannot reach are not
//! chased. The path is walked in short steps: at each step a fan of headings either side of the
//! current one (turning no more than a set angle per step, so the path stays smooth) is tried,
//! the engagement each would give (the share of the tool's circumference in material there) is
//! estimated from the raster, and the heading whose engagement is nearest the optimal load, with
//! the material on the side climb (or conventional) milling wants, is taken. A heading that
//! would engage the tool more than the limit is never taken. When no heading cuts any more, the
//! walk ends; the next starts where cleared space meets material again, reached by a feed through
//! cleared material when one leads there with the tool down, or by lifting to the retract height
//! otherwise. Material no walk can reach from cleared space is entered with a helix where it has
//! room (or from open air outside the stock), and refused otherwise.
//!
//! The planner's raster is its own; [`crate::engagement`] measures a finished path against the region
//! itself (exact containment, and exact distances to the moves already made), so the report does
//! not depend on the model the path was planned with.

use std::collections::VecDeque;
use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};
use solidcore_record::{Region2, Vec2, Vec3};
use solidcraft_geom::Ring;
use solidcraft_geom::prelude::*;

use crate::check::verify;
use crate::cutter::Cutter;
use crate::dropcutter::{drop_z, drop_z_blocks};
use crate::engagement::SKIN;
use crate::offset::Shape;
use crate::ops::{Cut, Feeds, Heights};
use crate::path::{MAX_MOVES, Move, Toolpath, arc_sweep};
use crate::piece::Piece;
use crate::surface::Surface;
use crate::surface_ops::{Boundary, Budget, check_leave, check_stepdown, check_tolerance, descent_feed, levels, tool_cutter};
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// Most cells of one level's material raster.
pub const MAX_CELLS: usize = 6_000_000;
/// Most steps of the walks of one operation, every level together.
const MAX_STEPS: usize = 3_000_000;
/// Most walks of one level.
const MAX_PASSES: usize = 50_000;
/// Most cell visits of one 2D adaptive operation (rasters, distance transforms, searches,
/// clearing): it plans one level and repeats it.
const MAX_WORK: f64 = 6.0e9;
/// Most cell visits of one level of an adaptive roughing, per cell of its raster: some 500
/// walks' searches and refreshes. A level's work does not depend on how many levels there
/// are, so a larger stepdown (fewer levels) is never refused where a smaller one is planned.
const LEVEL_WORK_PER_CELL: f64 = 8000.0;
/// Cell visits a step of a walk takes, about, per cell of the raster its sweep looks at (the
/// sweep itself, the engagement round the fan of headings, the exact distances near the cut).
const STEP_WORK_PER_SWEEP_CELL: f64 = 3.0;
/// How many times the steps a level is expected to take its budget allows.
const STEP_SLACK: f64 = 3.0;
/// Fewest cell visits a level of an adaptive roughing is allowed (a small raster).
const MIN_LEVEL_WORK: f64 = 2.0e8;
/// Fewest cell visits the level of a 2D adaptive clearing is allowed (it plans one level and
/// repeats it): a pocket not much wider than the tool, at a light load, takes many short walks
/// whose steps cost several times what a step in the open does. A 12 mm tool at 0.2 mm in a
/// 36 x 24 mm pocket needs some 9e8 (a few seconds).
const MIN_LEVEL_WORK_2D: f64 = 1.5e9;
/// The share of a level's material within reach the walks may leave, as crumbs for the finish
/// or a rest pass, when they keep missing it: a clearing stopped by its work limit with no more
/// left is planned (with a warning naming what is left) rather than refused.
const CRUMB_SHARE: f64 = 0.005;
/// The area (mm²) of a level's material within reach the walks may leave as crumbs, whatever
/// its share.
const CRUMB_AREA: f64 = 1.0;
/// Once only crumbs are left, the work the walks may go on spending on them, as a share of what
/// the level took to get there: past it the walks are only missing the crumbs and stop.
const CRUMB_WORK: f64 = 0.25;
/// The fewest cell visits each level of a 2D clearing is allowed when `planned` levels are
/// planned: the floor is the clearing's, shared among its levels when each is planned on its own
/// (rest machining), so a refusal does not take the floor's time over at every level.
pub(crate) fn level_floor_2d(planned: usize) -> f64 {
    MIN_LEVEL_WORK_2D / planned.max(1) as f64
}
/// The work refusal's text, so the clearing can tell it from the others and look for a load
/// that plans.
const WALKS_MISS: &str =
    "the adaptive clearing needs far more work at one level than its area and load call for (the walks keep missing the material)";
/// About how many cell visits a level of an adaptive roughing takes per cell of its raster (a
/// plate round a boss: some 3000).
pub(crate) const TYPICAL_LEVEL_WORK_PER_CELL: f64 = 3000.0;
/// Material at most this thick (mm) left above a level by earlier cuts is not chased by rest
/// machining (the operation's tolerance when that is larger).
pub const REST_TOLERANCE: f64 = 0.02;
/// Most cell visits an adaptive roughing is expected to take, all its levels together (a few
/// minutes of planning); a job expected to take more is refused before any is done.
pub(crate) const MAX_ROUGHING_WORK: f64 = 3.0e10;
/// Points round the tool the planner's engagement estimate looks at.
const RING: usize = 120;
/// Headings tried either side of the current one.
const FAN: i32 = 6;
/// Largest turn of the heading in one step (radians).
const MAX_TURN: f64 = 25.0 * PI / 180.0;
/// How far below the limit the planner keeps its own estimate (radians), for the raster's error.
const PLAN_MARGIN: f64 = 4.0 * PI / 180.0;
/// Most half turns of one entry helix.
const MAX_HALF_TURNS: f64 = 20_000.0;
/// Largest coordinate or height (mm).
const MAX_COORD: f64 = 1.0e6;
/// Engagement at or above this counts as a full slot (radians).
pub const FULL_SLOT: f64 = 170.0 * PI / 180.0;
/// The smallest entry helix, as a share of the tool's diameter, that a helix with no room is
/// shrunk to before the clearing is refused.
pub const MIN_HELIX_SHARE: f64 = 0.25;

/// What a move of an adaptive path is for, so a report can tell the entries from the clearing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveRole {
    /// Rapids and feeds through air: positioning, lifting, going down outside the material.
    Position,
    /// A helix into the material (and the circle at its foot).
    Entry,
    /// Clearing at the level.
    Cut,
    /// A feed at the level through material already cleared.
    Link,
}

/// How an adaptive clearing cuts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveParams {
    /// The radial width of cut the path aims at (mm), above 0 and at most the tool's diameter:
    /// on a straight stretch along a straight edge of material the tool takes this much.
    pub optimal_load: f64,
    /// Engagement below which a stretch counts as too light (radians): the walk steers towards
    /// the material harder there.
    pub min_engagement: f64,
    /// Engagement the path never exceeds outside an entry helix (radians), 5° to 180°.
    pub max_engagement: f64,
    /// Material left on the walls (mm); in 3D roughing, on the part all round.
    pub stock_to_leave: f64,
    /// Chord error allowed on curved boundaries, and how far the path may be simplified (mm).
    pub tolerance: f64,
    /// Diameter of the circle the tool's centre follows down an entry helix (mm), above 0 and at
    /// most the tool's diameter (so no core is left standing in the middle).
    pub helix_diameter: f64,
    /// Descent angle of the entry helix (radians), 0.5° to 45°.
    pub ramp_angle: f64,
    /// Climb milling (the material on the right of the tool's travel); otherwise conventional.
    pub climb: bool,
}

/// 3D adaptive roughing of a stock box in levels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveRoughingParams {
    /// `top` is the top of the stock, `bottom` the lowest level.
    pub heights: Heights,
    pub feeds: Feeds,
    /// Most material per level (mm).
    pub stepdown: f64,
    /// The stock's extent seen from above (left out: the surface's extent). The tool may go
    /// outside it (into open air) to enter and to turn.
    #[serde(default)]
    pub boundary: Boundary,
    /// Add a level at every flat area of the part (raised by the stock to leave) between the
    /// regular levels, so a flat is roughed to an even skin.
    #[serde(default)]
    pub flats: bool,
    pub adaptive: AdaptiveParams,
    /// What earlier cuts left of the stock (rest machining): the levels clear only what is
    /// still above them there. Left out, the stock box is whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest: Option<HeightMap>,
    /// Step-up levels at most this far apart (mm), cut after each regular level on the way back
    /// up toward the level above, and only where the regular level could not reach (shallow
    /// ground the part kept it off): the terraces left there are at most this high. Left out,
    /// none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_up: Option<f64>,
}

/// An adaptive toolpath and what each of its moves is for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdaptivePath {
    pub path: Toolpath,
    /// One per move of `path`.
    pub roles: Vec<MoveRole>,
    /// Levels the path cuts at.
    pub levels: usize,
    /// Entry helices.
    pub helices: usize,
    /// Times the tool lifts to the retract height between cuts at a level.
    pub lifts: usize,
    /// Area of material within the tool's reach the walks could not clear, summed over the
    /// levels (mm², to the raster's resolution).
    pub left_area: f64,
    /// Area of material seen from above that the tool cannot reach at all (mm², to the raster's
    /// resolution): corners tighter than its radius and places narrower than it, left for a
    /// smaller tool. 2D clearing only; the stock to leave on the walls is not counted.
    #[serde(default)]
    pub corner_area: f64,
    pub warnings: Vec<String>,
}

impl AdaptiveParams {
    fn validate(&self, tool: &Tool) -> Result<()> {
        if !(self.optimal_load > 0.0) || !(self.optimal_load <= tool.diameter) {
            return invalid("the optimal load must be above 0 and at most the tool diameter");
        }
        let deg = PI / 180.0;
        if !(self.max_engagement >= 5.0 * deg) || !(self.max_engagement <= PI) {
            return invalid("the largest engagement must be from 5 to 180 degrees");
        }
        if !(self.min_engagement >= 0.0) || !(self.min_engagement < self.max_engagement) {
            return invalid("the smallest engagement must be 0 or more and below the largest");
        }
        check_leave(self.stock_to_leave)?;
        check_tolerance(self.tolerance)?;
        if !(self.helix_diameter > 0.0) || !(self.helix_diameter <= tool.diameter) {
            return invalid("the helix diameter must be above 0 and at most the tool diameter");
        }
        if !(self.ramp_angle >= 0.5 * deg) || !(self.ramp_angle <= 45.0 * deg) {
            return invalid("the helix ramp angle must be from 0.5 to 45 degrees");
        }
        Ok(())
    }
}

/// Adaptive clearing mills with the tool's side and enters along a helix of `ramp_angle`.
fn mills(tool: &Tool, ramp_angle: f64) -> Result<()> {
    tool.validate()?;
    crate::ops::mills(tool, ramp_angle)
}

/// Refuses levels deeper apart than the tool's flutes reach: below them the shank would rub on
/// the material the flutes leave standing (the deep, light cut adaptive clearing is for makes
/// that easy to ask for).
fn check_flutes(tool: &Tool, top: f64, zs: &[f64]) -> Result<()> {
    let mut prev = top;
    for &z in zs {
        if prev - z > tool.flute_length + 1e-9 {
            return invalid(format!(
                "a level {:.3} mm deep is more than the tool's flute length {:.3} mm: use a smaller stepdown",
                prev - z,
                tool.flute_length
            ));
        }
        prev = z;
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// Cell visits this thread's levels have counted (the tests' measure of a clearing's work).
    static VISITS: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}

/// The cell visits [`Work`] counted on this thread while `f` ran, with what `f` gave.
#[cfg(test)]
pub(crate) fn visits_in<R>(f: impl FnOnce() -> R) -> (R, f64) {
    let before = VISITS.with(std::cell::Cell::get);
    let r = f();
    (r, VISITS.with(std::cell::Cell::get) - before)
}

/// Counts cell visits: those of the level being planned against its own budget, and all of
/// them against the sum of the budgets so far.
struct Work {
    total: std::cell::Cell<f64>,
    level: std::cell::Cell<f64>,
    limit: f64,
    level_limit: f64,
    /// The fewest visits a level is allowed.
    floor: f64,
}

impl Work {
    /// Before any level: `setup` visits for rasterising the region; each level is allowed at
    /// least `floor`.
    fn new(setup: f64, floor: f64) -> Work {
        Work { total: std::cell::Cell::new(0.0), level: std::cell::Cell::new(0.0), limit: setup, level_limit: setup, floor }
    }

    /// A new level starts, allowed `budget` visits of its own (at least the floor).
    fn start_level(&mut self, budget: f64) {
        let budget = budget.max(self.floor);
        self.level.set(0.0);
        self.level_limit = budget;
        self.limit += budget;
    }

    fn add(&mut self, n: f64) -> Result<()> {
        self.add_shared(n)
    }

    /// Whether `n` more visits would pass the level's or the whole budget.
    fn would_pass(&self, n: f64) -> bool {
        self.total.get() + n > self.limit || self.level.get() + n > self.level_limit
    }

    fn add_shared(&self, n: f64) -> Result<()> {
        crate::stop::check()?;
        #[cfg(test)]
        VISITS.with(|v| v.set(v.get() + n));
        self.total.set(self.total.get() + n);
        self.level.set(self.level.get() + n);
        if self.level.get() > self.level_limit || self.total.get() > self.limit {
            return Err(CamError::TooLarge(format!("{WALKS_MISS}; use a larger optimal load or engagement limit, or split the region")));
        }
        Ok(())
    }
}

/// The most cell visits one level over a raster of `cells` may take when its walks are expected
/// to take `steps` steps of `step_work` visits each: the searches between the walks (scaled to
/// the raster) and the walks themselves (scaled to the area and the load), each with room to
/// spare.
pub(crate) fn level_work_limit(cells: usize, steps: f64, step_work: f64) -> f64 {
    (cells as f64 * LEVEL_WORK_PER_CELL).max(MIN_LEVEL_WORK) + STEP_SLACK * steps * step_work
}

/// What an adaptive clearing at `knobs` is expected to take: steps per level over `area` mm².
fn expected_steps(knobs: &Knobs, area: f64) -> f64 {
    area / (knobs.w * knobs.step)
}

/// The smallest optimal load (mm) at which `fits` holds for the knobs it gives, the rest of
/// `params` kept but for the largest engagement, raised with the load (to one and a half times
/// the load's, and at least 15° more): `None` when not even the tool's diameter does.
fn smallest_load(tool: &Tool, params: &AdaptiveParams, fits: &dyn Fn(&Knobs) -> bool) -> Option<f64> {
    let r = tool.radius();
    let knobs = |w: f64| {
        let opt = (1.0 - w / r).clamp(-1.0, 1.0).acos();
        let most = params.max_engagement.max((opt * 1.5).max(opt + 15.0 * PI / 180.0).min(PI));
        Knobs::new(tool, &AdaptiveParams { optimal_load: w, max_engagement: most, ..*params })
    };
    let (mut lo, mut hi) = (params.optimal_load, tool.diameter);
    if !fits(&knobs(hi)) {
        return None;
    }
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if fits(&knobs(mid)) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(hi)
}

/// What to say when a clearing at `params` would take too many steps: the smallest load that
/// would plan, or that none does.
fn load_hint(tool: &Tool, params: &AdaptiveParams, fits: &dyn Fn(&Knobs) -> bool) -> String {
    match smallest_load(tool, params, fits) {
        // Rounded up to the next 0.01 mm, so the load named does plan.
        Some(w) => {
            format!("use an optimal load of at least {:.2} mm ({:.1}% of the tool's diameter)", (w * 100.0).ceil() / 100.0, w / tool.diameter * 100.0)
        }
        None => "no optimal load up to the tool's diameter plans it: use a larger tool, or split the region".into(),
    }
}

/// Refuses, before any is done, an adaptive roughing of `levels` levels over a raster of
/// `cells` whose walks are expected to take `steps` steps of `step_work` visits a level, when
/// that would take minutes of planning ([`MAX_ROUGHING_WORK`] as it typically goes), more steps
/// a level than a walk may take, or more drop-cutter samples than an operation may take: with
/// the numbers and `hint`, what to change.
pub(crate) fn check_roughing_work(levels: usize, cells: usize, steps: f64, step_work: f64, hint: &dyn Fn() -> String) -> Result<()> {
    let expected = levels as f64 * (cells as f64 * TYPICAL_LEVEL_WORK_PER_CELL + steps * step_work);
    if !(expected <= MAX_ROUGHING_WORK) {
        return Err(CamError::TooLarge(format!(
            "the adaptive roughing has {levels} levels over a raster of {cells} cells, some {steps:.2e} steps a level: some {expected:.1e} cell visits, minutes of planning (at most {MAX_ROUGHING_WORK:.0e}); use a larger stepdown (fewer levels) or a smaller boundary, or {}",
            hint()
        )));
    }
    if !(steps <= MAX_STEPS as f64) {
        return Err(CamError::TooLarge(format!(
            "the adaptive roughing needs some {steps:.2e} steps a level, more than {MAX_STEPS}; {}, or use a smaller boundary",
            hint()
        )));
    }
    if !(levels as f64 * cells as f64 <= crate::surface_ops::MAX_DROPS as f64) {
        return Err(CamError::TooLarge(format!(
            "the adaptive roughing has {levels} levels over a raster of {cells} cells, more drop-cutter samples than {}; use a larger stepdown (fewer levels), a larger tool (a coarser raster), or a smaller boundary",
            crate::surface_ops::MAX_DROPS
        )));
    }
    Ok(())
}

/// A raster of square cells.
#[derive(Clone, Copy, Debug)]
struct Grid {
    origin: Vec2,
    h: f64,
    nx: usize,
    ny: usize,
}

impl Grid {
    fn new(lo: Vec2, hi: Vec2, h: f64) -> Result<Grid> {
        if ![lo.x, lo.y, hi.x, hi.y].iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) || !(h > 0.0) {
            return invalid("the region must lie within 1000 m");
        }
        let fx = ((hi.x - lo.x) / h).ceil() + 1.0;
        let fy = ((hi.y - lo.y) / h).ceil() + 1.0;
        if !(fx >= 1.0 && fy >= 1.0) || !(fx * fy <= MAX_CELLS as f64) {
            return Err(CamError::TooLarge(format!(
                "the region needs more than {MAX_CELLS} cells for this tool, load and engagement limit; use a larger tool, optimal load or engagement limit, or split the region"
            )));
        }
        Ok(Grid { origin: lo, h, nx: fx as usize, ny: fy as usize })
    }

    fn len(&self) -> usize {
        self.nx * self.ny
    }

    fn center(&self, i: usize, j: usize) -> Vec2 {
        Vec2::new(self.origin.x + i as f64 * self.h, self.origin.y + j as f64 * self.h)
    }

    fn center_of(&self, k: usize) -> Vec2 {
        self.center(k % self.nx.max(1), k / self.nx.max(1))
    }

    /// The cell whose centre is nearest `p`.
    fn cell(&self, p: Vec2) -> Option<(usize, usize)> {
        let fi = ((p.x - self.origin.x) / self.h).round();
        let fj = ((p.y - self.origin.y) / self.h).round();
        if fi >= 0.0 && fj >= 0.0 && fi < self.nx as f64 && fj < self.ny as f64 { Some((fi as usize, fj as usize)) } else { None }
    }

    fn index(&self, p: Vec2) -> Option<usize> {
        self.cell(p).map(|(i, j)| j * self.nx + i)
    }

    /// The cells whose centres may be within `r` of the box `lo..hi`, as index ranges.
    fn span(&self, lo: Vec2, hi: Vec2, r: f64) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        let clamp = |v: f64, n: usize| -> usize {
            if !(v > 0.0) {
                0
            } else if v >= n as f64 {
                n
            } else {
                v as usize
            }
        };
        let i0 = clamp(((lo.x - r - self.origin.x) / self.h).floor(), self.nx);
        let i1 = clamp(((hi.x + r - self.origin.x) / self.h).ceil() + 1.0, self.nx);
        let j0 = clamp(((lo.y - r - self.origin.y) / self.h).floor(), self.ny);
        let j1 = clamp(((hi.y + r - self.origin.y) / self.h).ceil() + 1.0, self.ny);
        (i0..i1, j0..j1)
    }
}

const FAR: f64 = f64::INFINITY;

#[cfg(test)]
thread_local! {
    /// Distances [`envelope`] worked out on this thread, and the cells of the windows
    /// [`distance_window`] looked at.
    static DISTANCES_KEPT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static WINDOW_CELLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// What `f` costs the distance fields on this thread: the distances worked out along rows and
/// columns, and the cells of the windows they were worked out over.
#[cfg(test)]
pub(crate) fn distances_in<R>(f: impl FnOnce() -> R) -> (R, usize, usize) {
    let (k0, w0) = (DISTANCES_KEPT.with(std::cell::Cell::get), WINDOW_CELLS.with(std::cell::Cell::get));
    let r = f();
    (r, DISTANCES_KEPT.with(std::cell::Cell::get) - k0, WINDOW_CELLS.with(std::cell::Cell::get) - w0)
}

/// [`distance_window`] over `win` for the cells of `inner`, against the whole of `win` worked out
/// and cut down to `inner`: whether they are the same, on a raster of `nx` × `ny` cells with
/// sites where `site` says.
#[cfg(test)]
pub(crate) fn window_matches_whole(
    nx: usize,
    ny: usize,
    site: &dyn Fn(usize) -> bool,
    win: (usize, usize, usize, usize),
    inner: (usize, usize, usize, usize),
) -> Result<bool> {
    let g = Grid::new(Vec2::ZERO, Vec2::new(nx as f64 * 0.5, ny as f64 * 0.5), 0.5)?;
    let mut work = Work::new(1e12, 0.0);
    let win = Window { i0: win.0, i1: win.1, j0: win.2, j1: win.3 };
    let inner = Window { i0: inner.0, i1: inner.1, j0: inner.2, j1: inner.3 };
    let part = distance_window(&g, site, win, inner, &mut work)?;
    let whole = distance_window(&g, site, win, win, &mut work)?;
    let wnx = win.i1 - win.i0;
    let inx = inner.i1 - inner.i0;
    let mut same = part.len() == inx * (inner.j1 - inner.j0);
    for j in inner.j0..inner.j1 {
        for i in inner.i0..inner.i1 {
            let a = part.get((j - inner.j0) * inx + (i - inner.i0));
            let b = whole.get((j - win.j0) * wnx + (i - win.i0));
            same &= a.is_some() && a == b;
        }
    }
    Ok(same)
}

/// Squared distances along one line to the nearest site (where `f` is 0) by the lower envelope
/// of the parabolas rooted at the sites, for the cells from `from` on (as many as `d` holds).
fn envelope(f: &[f64], d: &mut [f64], from: usize, v: &mut Vec<usize>, z: &mut Vec<f64>) {
    v.clear();
    z.clear();
    for (q, &fq) in f.iter().enumerate() {
        if !fq.is_finite() {
            continue;
        }
        let qf = q as f64;
        loop {
            match (v.last().copied(), z.last().copied()) {
                (Some(p), Some(zp)) => {
                    let pf = p as f64;
                    let fp = f.get(p).copied().unwrap_or(FAR);
                    let s = ((fq + qf * qf) - (fp + pf * pf)) / (2.0 * (qf - pf));
                    if s <= zp {
                        v.pop();
                        z.pop();
                        continue;
                    }
                    v.push(q);
                    z.push(s);
                }
                _ => {
                    v.push(q);
                    z.push(f64::NEG_INFINITY);
                }
            }
            break;
        }
    }
    let mut k = 0usize;
    for (q, out) in d.iter_mut().enumerate() {
        let q = q + from;
        if v.is_empty() {
            *out = FAR;
            continue;
        }
        while k + 1 < v.len() && z.get(k + 1).is_some_and(|zk| *zk < q as f64) {
            k += 1;
        }
        let p = v.get(k).copied().unwrap_or(0);
        let dq = q as f64 - p as f64;
        *out = dq * dq + f.get(p).copied().unwrap_or(FAR);
    }
}

/// Distance (mm) from each cell's centre to the nearest cell's centre where `site` holds;
/// infinite where there is none.
fn distance_field(g: &Grid, site: &dyn Fn(usize) -> bool, work: &mut Work) -> Result<Vec<f64>> {
    let full = Window { i0: 0, i1: g.nx, j0: 0, j1: g.ny };
    distance_window(g, site, full, full, work)
}

/// A rectangle of a raster's cells: columns `i0..i1`, rows `j0..j1`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Window {
    i0: usize,
    i1: usize,
    j0: usize,
    j1: usize,
}

impl Window {
    /// Grown by `m` cells on every side, within the raster.
    fn grown(&self, m: usize, g: &Grid) -> Window {
        Window { i0: self.i0.saturating_sub(m), i1: (self.i1 + m).min(g.nx), j0: self.j0.saturating_sub(m), j1: (self.j1 + m).min(g.ny) }
    }

    /// Also covering cell `(i, j)`.
    fn with(w: Option<Window>, i: usize, j: usize) -> Window {
        match w {
            None => Window { i0: i, i1: i + 1, j0: j, j1: j + 1 },
            Some(w) => Window { i0: w.i0.min(i), i1: w.i1.max(i + 1), j0: w.j0.min(j), j1: w.j1.max(j + 1) },
        }
    }
}

/// [`distance_field`] from the sites inside `win`, for the cells of `inner` (within `win`) only,
/// indexed from its corner row by row. Counted as the work of the whole of `win`.
fn distance_window(g: &Grid, site: &dyn Fn(usize) -> bool, win: Window, inner: Window, work: &mut Work) -> Result<Vec<f64>> {
    let (nx, ny) = (win.i1.saturating_sub(win.i0), win.j1.saturating_sub(win.j0));
    work.add(3.0 * (nx * ny) as f64)?;
    #[cfg(test)]
    WINDOW_CELLS.with(|n| n.set(n.get() + nx * ny));
    let (i0, i1) = (inner.i0.clamp(win.i0, win.i1) - win.i0, inner.i1.clamp(win.i0, win.i1) - win.i0);
    let (j0, j1) = (inner.j0.clamp(win.j0, win.j1) - win.j0, inner.j1.clamp(win.j0, win.j1) - win.j0);
    let (inx, iny) = (i1.saturating_sub(i0), j1.saturating_sub(j0));
    #[cfg(test)]
    DISTANCES_KEPT.with(|n| n.set(n.get() + inx * ny + inx * iny));
    // Along the rows of `win`, kept for the columns of `inner`; then down those columns, kept
    // for the rows of `inner`: the columns outside it are never needed.
    let mut rows = vec![FAR; inx * ny];
    let n = nx.max(ny);
    let (mut f, mut d, mut v, mut z) = (vec![FAR; n], vec![FAR; n], Vec::with_capacity(n), Vec::with_capacity(n + 1));
    for j in 0..ny {
        for (i, fi) in f.iter_mut().enumerate().take(nx) {
            *fi = if site((win.j0 + j) * g.nx + win.i0 + i) { 0.0 } else { FAR };
        }
        if let (Some(fr), Some(dr)) = (f.get(..nx), d.get_mut(..inx)) {
            envelope(fr, dr, i0, &mut v, &mut z);
        }
        if let (Some(row), Some(dr)) = (rows.get_mut(j * inx..(j + 1) * inx), d.get(..inx)) {
            row.copy_from_slice(dr);
        }
    }
    let mut out = vec![FAR; inx * iny];
    for i in 0..inx {
        for (j, fj) in f.iter_mut().enumerate().take(ny) {
            *fj = rows.get(j * inx + i).copied().unwrap_or(FAR);
        }
        if let (Some(fc), Some(dc)) = (f.get(..ny), d.get_mut(..iny)) {
            envelope(fc, dc, j0, &mut v, &mut z);
        }
        for (j, dj) in d.iter().enumerate().take(iny) {
            if let Some(o) = out.get_mut(j * inx + i) {
                *o = if dj.is_finite() { dj.sqrt() * g.h } else { FAR };
            }
        }
    }
    Ok(out)
}

/// [`distance_window`] for each of `sites` (whether each cell of `win` is a site, row by row),
/// the rows and then the columns shared out over `crew`: the same distances, and the same work
/// counted, as one after the other.
fn distance_windows_shared(g: &Grid, sites: Vec<Vec<bool>>, win: Window, inner: Window, work: &mut Work, crew: &Crew<'_>) -> Result<Vec<Vec<f64>>> {
    let (nx, ny) = (win.i1.saturating_sub(win.i0), win.j1.saturating_sub(win.j0));
    let (i0, i1) = (inner.i0.clamp(win.i0, win.i1) - win.i0, inner.i1.clamp(win.i0, win.i1) - win.i0);
    let (j0, j1) = (inner.j0.clamp(win.j0, win.j1) - win.j0, inner.j1.clamp(win.j0, win.j1) - win.j0);
    let (inx, iny) = (i1.saturating_sub(i0), j1.saturating_sub(j0));
    let fields = sites.len();
    for _ in 0..fields {
        work.add(3.0 * (nx * ny) as f64)?;
        #[cfg(test)]
        WINDOW_CELLS.with(|n| n.set(n.get() + nx * ny));
        #[cfg(test)]
        DISTANCES_KEPT.with(|n| n.set(n.get() + inx * ny + inx * iny));
    }
    let lost = || CamError::Unsupported("a worker thread lost its work".into());
    let parts = (2 * crew.threads()).max(1);
    // Along the rows of `win`, kept for the columns of `inner`: every field's rows in turn.
    let sites: std::sync::Arc<[Vec<bool>]> = sites.into();
    let rows_of = move |t: usize| -> std::ops::Range<usize> { (t * fields * ny / parts)..((t + 1) * fields * ny / parts) };
    let found = crew.run(
        parts,
        std::sync::Arc::new(move |t: usize| {
            let n = nx.max(ny);
            let (mut f, mut d, mut v, mut z) = (vec![FAR; n], vec![FAR; n], Vec::with_capacity(n), Vec::with_capacity(n + 1));
            let mut out = Vec::with_capacity(rows_of(t).len() * inx);
            for row in rows_of(t) {
                let (field, j) = (row / ny.max(1), row % ny.max(1));
                let site = sites.get(field).map_or(&[][..], |s| &s[..]);
                for (i, fi) in f.iter_mut().enumerate().take(nx) {
                    *fi = if site.get(j * nx + i).copied().unwrap_or(false) { 0.0 } else { FAR };
                }
                if let (Some(fr), Some(dr)) = (f.get(..nx), d.get_mut(..inx)) {
                    envelope(fr, dr, i0, &mut v, &mut z);
                }
                out.extend_from_slice(d.get(..inx).unwrap_or(&[]));
            }
            Worked::Line(out)
        }),
    );
    let mut rows = Vec::with_capacity(fields * ny * inx);
    for w in found {
        let Worked::Line(part) = w else { return Err(lost()) };
        rows.extend(part);
    }
    if rows.len() != fields * ny * inx {
        return Err(lost());
    }
    // Then down those columns, kept for the rows of `inner`: every field's columns in turn.
    let rows: std::sync::Arc<[f64]> = rows.into();
    let h = g.h;
    let cols_of = move |t: usize| -> std::ops::Range<usize> { (t * fields * inx / parts)..((t + 1) * fields * inx / parts) };
    let found = crew.run(
        parts,
        std::sync::Arc::new(move |t: usize| {
            let n = nx.max(ny);
            let (mut f, mut d, mut v, mut z) = (vec![FAR; n], vec![FAR; n], Vec::with_capacity(n), Vec::with_capacity(n + 1));
            let mut out = Vec::with_capacity(cols_of(t).len() * iny);
            for col in cols_of(t) {
                let (field, i) = (col / inx.max(1), col % inx.max(1));
                for (j, fj) in f.iter_mut().enumerate().take(ny) {
                    *fj = rows.get((field * ny + j) * inx + i).copied().unwrap_or(FAR);
                }
                if let (Some(fc), Some(dc)) = (f.get(..ny), d.get_mut(..iny)) {
                    envelope(fc, dc, j0, &mut v, &mut z);
                }
                out.extend(d.iter().take(iny).map(|dj| if dj.is_finite() { dj.sqrt() * h } else { FAR }));
            }
            Worked::Line(out)
        }),
    );
    let mut cols = Vec::with_capacity(fields * inx * iny);
    for w in found {
        let Worked::Line(part) = w else { return Err(lost()) };
        cols.extend(part);
    }
    if cols.len() != fields * inx * iny {
        return Err(lost());
    }
    // Column by column into each field's window, row by row.
    let mut outs = vec![vec![FAR; inx * iny]; fields];
    for (col, line) in cols.chunks(iny.max(1)).enumerate() {
        let (field, i) = (col / inx.max(1), col % inx.max(1));
        if let Some(o) = outs.get_mut(field) {
            for (j, v) in line.iter().enumerate() {
                if let Some(x) = o.get_mut(j * inx + i) {
                    *x = *v;
                }
            }
        }
    }
    Ok(outs)
}

/// Where the cutter's centre may stand at a level, exactly (with the planner's margin).
trait Domain {
    fn allowed(&self, p: Vec2) -> Result<bool>;
}

/// Inside a pocket's walls.
struct Walls {
    shape: Shape,
    /// The tool radius, the stock to leave and the margin.
    wall: f64,
}

impl Domain for Walls {
    fn allowed(&self, p: Vec2) -> Result<bool> {
        Ok(p.is_finite() && self.shape.clear(p, self.wall))
    }
}

/// Clear of a surface at a level, inside a rectangle.
struct Clear<'a> {
    surface: &'a Surface,
    /// The cutter grown by the stock to leave and the margin.
    cutter: Cutter,
    /// How far the grown cutter's tip is below the real one.
    grown_by: f64,
    z: f64,
    lo: Vec2,
    hi: Vec2,
    budget: &'a Budget,
}

impl Domain for Clear<'_> {
    fn allowed(&self, p: Vec2) -> Result<bool> {
        if !(p.x >= self.lo.x && p.x <= self.hi.x && p.y >= self.lo.y && p.y <= self.hi.y) {
            return Ok(false);
        }
        self.budget.spend()?;
        // Clear unless the grown cutter rests higher than the level: tips a micrometre and more
        // below it never block, so the surface under them is not opened.
        let (z, by) = (self.z, self.grown_by);
        Ok(!drop_z_blocks(self.surface, &self.cutter, p.x, p.y, z - by - 1e-6, |zg| zg + by > z + 1e-9))
    }
}

/// A heightfield of what earlier cuts left: cell `(i, j)` is the column over the square of
/// side `dx` by `dy` about `(x0 + (i + ½)·dx, y0 + (j + ½)·dy)`, its top at `heights[j·nx + i]`
/// (the simulation's [`crate::sim::StockGrid::Heights`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeightMap {
    pub x0: f64,
    pub y0: f64,
    pub dx: f64,
    pub dy: f64,
    pub nx: usize,
    pub ny: usize,
    pub heights: Vec<f64>,
}

/// Most cells of a height map.
pub const MAX_MAP_CELLS: usize = 64_000_000;

impl HeightMap {
    pub fn validate(&self) -> Result<()> {
        if ![self.x0, self.y0, self.dx, self.dy].iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) || !(self.dx > 0.0) || !(self.dy > 0.0) {
            return invalid("the stock's height map needs finite corners and cells above 0 mm");
        }
        if self.nx == 0 || self.ny == 0 || self.nx.checked_mul(self.ny).is_none_or(|n| n > MAX_MAP_CELLS || n != self.heights.len()) {
            return invalid(format!("the stock's height map needs nx × ny heights, at most {MAX_MAP_CELLS}"));
        }
        if !self.heights.iter().all(|z| z.is_finite() && z.abs() <= MAX_COORD) {
            return invalid("the stock's heights must be finite numbers within 1000 m");
        }
        Ok(())
    }

    /// The top of the cell `q` is in, as the simulation that made the map has it (a cell is cut
    /// where the tool covered its centre); `None` off the map.
    pub fn top_at(&self, q: Vec2) -> Option<f64> {
        let fi = (q.x - self.x0) / self.dx;
        let fj = (q.y - self.y0) / self.dy;
        if !(fi >= 0.0 && fj >= 0.0 && fi < self.nx as f64 && fj < self.ny as f64) {
            return None;
        }
        self.heights.get(fj as usize * self.nx + fi as usize).copied()
    }

    /// The highest top over the disc of radius `rad` about `c` (and a cell round it); `None`
    /// where the disc is off the map.
    fn top_over(&self, c: Vec2, rad: f64) -> Option<f64> {
        let i0 = ((c.x - rad - self.x0) / self.dx - 1.0).floor().max(0.0);
        let i1 = ((c.x + rad - self.x0) / self.dx + 1.0).ceil().min(self.nx as f64 - 1.0);
        let j0 = ((c.y - rad - self.y0) / self.dy - 1.0).floor().max(0.0);
        let j1 = ((c.y + rad - self.y0) / self.dy + 1.0).ceil().min(self.ny as f64 - 1.0);
        if !(i0 <= i1 && j0 <= j1) {
            return None;
        }
        let mut most = f64::NEG_INFINITY;
        let reach = rad + self.dx.hypot(self.dy);
        for j in j0 as usize..=j1 as usize {
            for i in i0 as usize..=i1 as usize {
                let p = Vec2::new(self.x0 + (i as f64 + 0.5) * self.dx, self.y0 + (j as f64 + 0.5) * self.dy);
                if p.dist(c) <= reach
                    && let Some(z) = self.heights.get(j * self.nx + i)
                {
                    most = most.max(*z);
                }
            }
        }
        most.is_finite().then_some(most)
    }
}

/// What an adaptive clearing knows of the stock beyond its own region (2D) or boundary (3D):
/// where the stock is seen from above, how high it reaches, and what earlier cuts left of it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StockModel {
    /// The stock's low and high corners seen from above; `None`: it reaches past the region
    /// everywhere (a 2D clearing then enters with a helix).
    #[serde(default)]
    pub rect: Option<[Vec2; 2]>,
    /// The stock's top; `None`: the operation's top.
    #[serde(default)]
    pub top: Option<f64>,
    /// What earlier operations (or setups) left: where the map has a cell, the material ends at
    /// its top. Rest machining plans on it.
    #[serde(default)]
    pub heights: Option<HeightMap>,
    /// Material left above a level no thicker than this is taken as cut (mm): the cusps and
    /// the cells of the map are not chased.
    #[serde(default)]
    pub rest_tolerance: f64,
}

impl StockModel {
    fn validate(&self) -> Result<()> {
        if let Some([lo, hi]) = self.rect
            && (![lo.x, lo.y, hi.x, hi.y].iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) || !(lo.x < hi.x) || !(lo.y < hi.y))
        {
            return invalid("the stock's corners must be finite, the low one below and left of the high one");
        }
        if self.top.is_some_and(|t| !(t.is_finite() && t.abs() <= MAX_COORD)) {
            return invalid("the stock's top must be a finite height");
        }
        if !(self.rest_tolerance >= 0.0 && self.rest_tolerance <= 100.0) {
            return invalid("the rest tolerance must be from 0 to 100 mm");
        }
        if let Some(m) = &self.heights {
            m.validate()?;
        }
        Ok(())
    }

    fn in_rect(&self, q: Vec2) -> bool {
        self.rect.is_none_or(|[lo, hi]| q.x >= lo.x && q.x <= hi.x && q.y >= lo.y && q.y <= hi.y)
    }

    /// Whether there is stock at `q` at the level `z`.
    fn material_at(&self, q: Vec2, z: f64) -> bool {
        self.in_rect(q)
            && self.top.is_none_or(|t| t > z + self.rest_tolerance)
            && self.heights.as_ref().and_then(|m| m.top_at(q)).is_none_or(|t| t > z + self.rest_tolerance)
    }

    /// The highest the stock reaches over the disc of radius `rad` about `c`, at most `cap`.
    fn top_over(&self, c: Vec2, rad: f64, cap: f64) -> f64 {
        let t = self.top.map_or(cap, |t| t.min(cap));
        match self.heights.as_ref().and_then(|m| m.top_over(c, rad)) {
            Some(h) => h.min(t),
            None => t,
        }
    }
}

/// Where the cutter's centre may stand in a 2D clearing when the stock's extent is known: inside
/// the regions clear of their walls as [`Walls`] has it, and also past a region's boundary where
/// the tool meets no stock outside the regions (open air beyond the stock's edge, or a region
/// drawn round the stock).
struct Fence {
    shape: Shape,
    lo: Vec2,
    hi: Vec2,
    /// The boundary of the stock outside the regions: the regions' boundary inside the stock and
    /// the stock's edges outside the regions.
    walls: Vec<Piece>,
    wall: f64,
}

impl Fence {
    fn new(shape: Shape, lo: Vec2, hi: Vec2, wall: f64) -> Fence {
        let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
        let edges: Vec<Piece> = (0..4).filter_map(|i| Some(Piece::Line { a: *corners.get(i)?, b: *corners.get((i + 1) % 4)? })).collect();
        let size = (hi - lo).len().max(1.0);
        let eps = 1e-9 * size;
        let nudge = 1e-6 * size;
        let strictly_in = |q: Vec2| q.x > lo.x + eps && q.x < hi.x - eps && q.y > lo.y + eps && q.y < hi.y - eps;
        // The stretches of `piece` between where it crosses `others`.
        let split = |piece: &Piece, others: &mut dyn Iterator<Item = &Piece>| -> Vec<(f64, f64)> {
            let mut ts = vec![0.0, 1.0];
            let mut hits = Vec::new();
            for o in others {
                hits.clear();
                piece.intersections(o, &mut hits);
                ts.extend(hits.iter().map(|h| h.0));
            }
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
            ts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).filter(|(a, b)| b - a > 1e-12).collect()
        };
        let mut walls = Vec::new();
        for piece in shape.loops().iter().flatten() {
            for (t0, t1) in split(piece, &mut edges.iter()) {
                if strictly_in(piece.point_at(0.5 * (t0 + t1))) {
                    walls.push(piece.sub(t0, t1));
                }
            }
        }
        for e in &edges {
            let inward = e.tangent_at(0.5).perp();
            for (t0, t1) in split(e, &mut shape.loops().iter().flatten()) {
                let m = e.point_at(0.5 * (t0 + t1)) + inward * nudge;
                if !shape.inside(m, nudge) {
                    walls.push(e.sub(t0, t1));
                }
            }
        }
        Fence { shape, lo, hi, walls, wall }
    }

    /// Whether a tool of radius `wall` centred on `p` stays off the stock outside the regions.
    fn clear(&self, p: Vec2, wall: f64) -> bool {
        if !p.is_finite() {
            return false;
        }
        if self.shape.clear(p, wall) {
            return true;
        }
        let in_stock = p.x >= self.lo.x && p.x <= self.hi.x && p.y >= self.lo.y && p.y <= self.hi.y;
        if in_stock && !self.shape.inside(p, wall.max(1e-6)) {
            return false;
        }
        self.walls.iter().all(|w| w.dist(p) >= wall - 1e-7)
    }
}

impl Domain for Fence {
    fn allowed(&self, p: Vec2) -> Result<bool> {
        Ok(self.clear(p, self.wall))
    }
}

#[cfg(test)]
thread_local! {
    /// Centres [`shape_raster`] measured on this thread.
    static RASTER_MEASURED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// [`shape_raster`] of `regions` on a raster of cell `h` over their extent and `wall` round it:
/// the cells, the centres it measured, and the centres where it differs from the exact tests.
#[cfg(test)]
pub(crate) fn shape_raster_against_exact(regions: &[Region2], h: f64, wall: f64) -> Result<(usize, usize, usize)> {
    let shape = Shape::from_regions(regions, 1e-3)?;
    let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for piece in shape.loops().iter().flatten() {
        let (a, b) = piece.bounds();
        lo = Vec2::new(lo.x.min(a.x), lo.y.min(a.y));
        hi = Vec2::new(hi.x.max(b.x), hi.y.max(b.y));
    }
    let g = Grid::new(lo - Vec2::new(wall, wall), hi + Vec2::new(wall, wall), h)?;
    let before = RASTER_MEASURED.with(std::cell::Cell::get);
    let (clear, inside) = shape_raster(&g, &shape, wall, h);
    let measured = RASTER_MEASURED.with(std::cell::Cell::get) - before;
    let wrong = (0..g.len())
        .filter(|&c| {
            let p = g.center_of(c);
            clear.get(c) != Some(&shape.clear(p, wall)) || inside.get(c) != Some(&shape.inside(p, h))
        })
        .count();
    Ok((g.len(), measured, wrong))
}

/// Over the raster's cell centres, row by row, [`Shape::clear`]`(p, wall)` and
/// [`Shape::inside`]`(p, sag)` as those exact tests give them. A centre's exact distance to the
/// boundary bounds the next ones along its row (one `s` further on is at least that less `s`
/// from it): where the bound keeps a centre past `wall` from the boundary and past the chords
/// both tests walk its arcs in, nothing between the two crosses the boundary, so it has the
/// answers of the centre measured; elsewhere it is measured itself.
fn shape_raster(g: &Grid, shape: &Shape, wall: f64, sag: f64) -> (Vec<bool>, Vec<bool>) {
    // Slack for the rounding of the centres and the distances.
    const SLACK: f64 = 1e-6;
    let chords = shape.chord_sag(wall.max(1e-6)).max(shape.chord_sag(sag));
    let far = (wall - 1e-7).max(chords) + SLACK;
    let mut clear = Vec::with_capacity(g.len());
    let mut inside = Vec::with_capacity(g.len());
    for j in 0..g.ny {
        // The last centre measured on this row: its column, distance and answers.
        let mut seen: Option<(usize, f64, bool, bool)> = None;
        for i in 0..g.nx {
            let known = seen.and_then(|(i0, d, c, n)| (d - (i - i0) as f64 * g.h >= far).then_some((c, n)));
            let (c, n) = match known {
                Some(cn) => cn,
                None => {
                    #[cfg(test)]
                    RASTER_MEASURED.with(|m| m.set(m.get() + 1));
                    let p = g.center(i, j);
                    let d = shape.dist(p);
                    let (c, n) = (d >= wall - 1e-7 && shape.inside(p, wall.max(1e-6)), shape.inside(p, sag));
                    seen = Some((i, d, c, n));
                    (c, n)
                }
            };
            clear.push(c);
            inside.push(n);
        }
    }
    (clear, inside)
}

/// A hash of a cell's index for the maps the walks look up for every point they test: a
/// multiply (the map only needs its cells spread), not the default's keyed hash.
#[derive(Default)]
struct CellHasher(u64);

impl std::hash::Hasher for CellHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(*b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }

    fn write_usize(&mut self, n: usize) {
        self.0 = (self.0.rotate_left(5) ^ n as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

/// The planner's knobs, derived from the tool and the parameters.
#[derive(Clone, Copy, Debug)]
struct Knobs {
    r: f64,
    /// Cell size.
    h: f64,
    /// Step length of a walk.
    step: f64,
    /// The radial width of cut aimed at.
    w: f64,
    /// Margin kept from where the centre may not go.
    margin: f64,
    /// Engagement the walk aims at, the most it takes, and the least it wants.
    target: f64,
    limit: f64,
    least: f64,
    climb: bool,
    helix_radius: f64,
    ramp: f64,
    /// How far the path may be simplified.
    simplify: f64,
}

impl Knobs {
    fn new(tool: &Tool, p: &AdaptiveParams) -> Knobs {
        let r = tool.radius();
        let limit = (p.max_engagement - PLAN_MARGIN).max(p.max_engagement * 0.5);
        let load = p.optimal_load.min(2.0 * r);
        let target = (1.0 - load / r).clamp(-1.0, 1.0).acos().min(limit * 0.92);
        // The width actually aimed at: the load's, or less where the limit holds the engagement
        // below the load's. The raster and the steps follow it, so a step into material (a
        // concave wall above all) stays within the limit.
        let w = (r * (1.0 - target.cos())).min(load);
        // The raster follows the tool, not the load: where the path is near the edge of what
        // it has cut, the engagement is settled by exact distances to the moves, so a light
        // load needs no finer cells (only more steps).
        let h = r / 16.0;
        let step = (w * 0.5).clamp(h, r * 0.25);
        // Only points in line with their neighbours are left out: the walk's engagement holds for
        // the points it took, and a corner cut short near the edge of a cut can bite much deeper.
        let simplify = (p.tolerance * 0.01).min(1e-4);
        Knobs {
            r,
            h,
            step,
            w,
            margin: step * step / (8.0 * r) + simplify + 1e-4 * r.max(1.0),
            target,
            limit,
            least: p.min_engagement.min(target),
            climb: p.climb,
            helix_radius: p.helix_diameter * 0.5,
            ramp: p.ramp_angle,
            simplify,
        }
    }
}

impl Knobs {
    /// How far a walk may go without cutting before it ends: a half turn at its tightest turn
    /// and a quarter more, enough to round a corner of the material and come back to it, too
    /// little to wander off through what is already cleared.
    fn max_air(&self) -> f64 {
        1.25 * PI * self.step / MAX_TURN
    }

    /// How far from material the distances to it are kept exact: past where a walk starts
    /// and where it steers through air.
    fn near(&self) -> f64 {
        2.0 * self.r + 2.0 * self.step + 4.0 * self.h + self.max_air()
    }

    /// About how many cell visits a step of a walk takes: the cells its sweep looks at.
    fn step_work(&self) -> f64 {
        let span = 2.0 * (self.r + self.h) / self.h + 2.0 + self.step / self.h;
        span * span * STEP_WORK_PER_SWEEP_CELL
    }
}

/// How a level's path gets from one cut to the next.
#[derive(Clone, Debug, PartialEq)]
enum Step {
    /// From the retract height straight down to the level at the point (cleared or open air).
    Drop(Vec2),
    /// A helix of `radius` about `center` from where the material starts down to the level,
    /// then a circle.
    Helix { center: Vec2, from: Option<f64>, radius: f64 },
    /// A feed at the level through cleared material.
    Link(Vec<Vec2>),
    /// Clearing at the level along the points (from where the tool is).
    Cut(Vec<Vec2>),
}

fn rotate(v: Vec2, a: f64) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let l2 = d.len2();
    let t = if l2 > 0.0 { ((p - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + d * t)
}

/// The path a level's walks have cut so far, as [`Level::engage`] needs it.
struct Cuts {
    /// Distance from each cell's centre to the path so far (where it is within reach of it).
    dpath: Vec<f64>,
    /// The path so far, for exact distances.
    segs: Vec<(Vec2, Vec2)>,
    /// For each cell not yet settled by the raster (its centre neither well inside nor well
    /// outside the radius of the path), the moves that come within the radius and the slack of
    /// its centre: all the moves that may cut a point the cell settles.
    band: std::collections::HashMap<usize, Vec<u32>, std::hash::BuildHasherDefault<CellHasher>>,
}

impl Cuts {
    /// Whether the point is still material: stock (`stock_at`) not within `r` of the path. The
    /// raster settles it where the path is well inside or well outside the radius, exact
    /// distances to the path do in between; with how many moves those took (none: the raster
    /// settled it).
    fn material_at(&self, g: &Grid, r: f64, stock_at: &dyn Fn(Vec2) -> bool, q: Vec2) -> (bool, Option<usize>) {
        let Some(c) = g.index(q) else { return (false, None) };
        let d = self.dpath.get(c).copied().unwrap_or(FAR);
        let slack = g.h * std::f64::consts::FRAC_1_SQRT_2 + 1e-9;
        if d <= r - slack {
            return (false, None);
        }
        if !stock_at(q) {
            return (false, None);
        }
        if d > r + slack {
            return (true, None);
        }
        let Some(near) = self.band.get(&c) else { return (true, None) };
        (!near.iter().any(|id| self.segs.get(*id as usize).is_some_and(|(a, b)| seg_dist(q, *a, *b) < r)), Some(near.len()))
    }

    /// The points of the `ring` about `q` in material: how many, the sum of their directions,
    /// and the moves the exact tests took (none: the raster settled every point).
    fn engage(&self, g: &Grid, r: f64, stock_at: &dyn Fn(Vec2) -> bool, ring: &[Vec2], q: Vec2) -> (usize, Vec2, Option<usize>) {
        let (mut n, mut dir, mut visits) = (0usize, Vec2::ZERO, None);
        for u in ring {
            let (m, v) = self.material_at(g, r, stock_at, q + *u);
            if let Some(v) = v {
                visits = Some(visits.unwrap_or(0) + v);
            }
            if m {
                n += 1;
                dir += *u;
            }
        }
        (n, dir, visits)
    }
}

/// What a level's crew of threads works out for it.
enum Worked {
    /// [`Cuts::engage`] at one point of a walk's fan (`None`: the path could not be read).
    Engaged(Option<(usize, Vec2, Option<usize>)>),
    /// Distances along some lines of a raster's window.
    Line(Vec<f64>),
}

type Crew<'a> = crate::par::Crew<'a, Worked>;

/// The path a level's walks cut could not be read: a thread stopped while it wrote it.
const LOST_PATH: &str = "a worker thread lost the clearing's path";

/// Engagements the crews' threads worked out (the tests' measure of the fan's sharing).
#[cfg(test)]
static CREW_ENGAGED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many engagements the crews' threads have worked out so far, in any test.
#[cfg(test)]
pub(crate) fn crew_engaged() -> usize {
    CREW_ENGAGED.load(std::sync::atomic::Ordering::Relaxed)
}

/// One level's material raster and what the walks need.
struct Level<'a> {
    g: Grid,
    k: Knobs,
    domain: &'a dyn Domain,
    allow: Vec<bool>,
    /// Whether a point is stock, exactly.
    stock_at: &'a (dyn Fn(Vec2) -> bool + Sync),
    /// The path so far, shared with the threads that score a walk's fan.
    cuts: std::sync::Arc<std::sync::RwLock<Cuts>>,
    /// Material still there: stock farther than the radius from the path (all of it, so the
    /// engagement sees what the tool cannot reach too).
    mat: Vec<bool>,
    /// Cells within reach of where the centre may stand: the material the walks clear.
    reach: Vec<bool>,
    /// Distance from each cell to the nearest material within reach (refreshed between walks).
    dmat: Vec<f64>,
    /// Distance from each cell to the nearest material of any kind (refreshed with `dmat`).
    /// Both are exact up to [`Knobs::near`]; a larger distance only says the material is
    /// farther than that.
    dall: Vec<f64>,
    /// Whether `dmat` and `dall` have been worked out once.
    fresh: bool,
    /// The cells whose material the walks cleared since the distances were last refreshed.
    dirty: Option<Window>,
    /// Cells of material within reach still there, and at the start.
    left: usize,
    start_left: usize,
    /// The links' search: the cell each cell was reached from, and the search that reached it.
    parent: Vec<u32>,
    seen: Vec<u32>,
    search: u32,
    /// Distance from each cell to the nearest cell the centre may not stand at.
    room: Vec<f64>,
    tried: Vec<bool>,
    ring: std::sync::Arc<[Vec2]>,
    /// The smallest entry helix radius used where the one asked for had no room.
    shrunk: Option<f64>,
    /// Whether material out of reach of the cleared area may be entered with a helix (false: it
    /// is left, and `unentered` says so).
    helix: bool,
    /// Material was left that only a helix could have entered.
    unentered: bool,
    /// Whether the walks may stop with crumbs left ([`CRUMB_SHARE`], [`CRUMB_AREA`]) when they
    /// keep missing them, rather than be refused (a 2D clearing, whose finish or rest pass takes
    /// them).
    crumbs: bool,
    /// The level's work when only crumbs were first left.
    crumbs_from: Option<f64>,
    /// The walks stopped with crumbs left: the material within reach then (cells).
    stopped: Option<usize>,
    steps: &'a mut usize,
    work: &'a mut Work,
}

impl<'a> Level<'a> {
    /// The level's raster: where the centre may stand (`allow`, exact at each cell's centre) and
    /// the stock (`stock_cell` at each cell's centre, `stock_at` for any point the tool's edge
    /// reaches).
    fn new(
        g: Grid,
        k: Knobs,
        domain: &'a dyn Domain,
        allow: Vec<bool>,
        stock_cell: &dyn Fn(usize, Vec2) -> bool,
        stock_at: &'a (dyn Fn(Vec2) -> bool + Sync),
        steps: &'a mut usize,
        work: &'a mut Work,
        known: (Option<Vec<bool>>, Option<Vec<f64>>),
    ) -> Result<Level<'a>> {
        let mat: Vec<bool> = (0..g.len()).map(|c| stock_cell(c, g.center_of(c))).collect();
        // The level's budget: its raster and the walks its material is expected to take.
        let area = mat.iter().filter(|m| **m).count() as f64 * g.h * g.h;
        work.start_level(level_work_limit(g.len(), expected_steps(&k, area), k.step_work()));
        // The reach and the room `allow` gives, worked out here unless `known` (from the same
        // `allow`): counted as worked out either way.
        let (known_reach, known_room) = known;
        let reach = match known_reach.filter(|r| r.len() == g.len()) {
            Some(r) => {
                work.add(3.0 * g.len() as f64)?;
                r
            }
            None => Level::reach_of(&distance_field(&g, &|c| allow.get(c).copied().unwrap_or(false), work)?, &k),
        };
        let room = match known_room.filter(|r| r.len() == g.len()) {
            Some(r) => {
                work.add(3.0 * g.len() as f64)?;
                r
            }
            None => distance_field(&g, &|c| !allow.get(c).copied().unwrap_or(false), work)?,
        };
        let left = mat.iter().zip(&reach).filter(|(m, r)| **m && **r).count();
        let rho = k.r * (1.0 - SKIN);
        let ring: std::sync::Arc<[Vec2]> = (0..RING).map(|i| Vec2::from_angle(TAU * i as f64 / RING as f64) * rho).collect();
        let n = g.len();
        Ok(Level {
            g,
            k,
            domain,
            allow,
            stock_at,
            cuts: std::sync::Arc::new(std::sync::RwLock::new(Cuts {
                dpath: vec![FAR; n],
                segs: Vec::new(),
                band: std::collections::HashMap::default(),
            })),
            mat,
            reach,
            dmat: vec![FAR; n],
            dall: vec![FAR; n],
            fresh: false,
            dirty: None,
            left,
            start_left: left,
            parent: vec![0; n],
            seen: vec![0; n],
            search: 0,
            room,
            tried: vec![false; n],
            ring,
            shrunk: None,
            helix: true,
            unentered: false,
            crumbs: false,
            crumbs_from: None,
            stopped: None,
            steps,
            work,
        })
    }

    /// The cells within reach of where the centre may stand, from the distance to it.
    fn reach_of(dist: &[f64], k: &Knobs) -> Vec<bool> {
        dist.iter().map(|d| *d <= k.r - 0.5 * k.h).collect()
    }

    fn grid_allows(&self, p: Vec2) -> bool {
        self.g.index(p).and_then(|c| self.allow.get(c).copied()).unwrap_or(false)
    }

    fn dmat_at(&self, p: Vec2) -> f64 {
        self.g.index(p).and_then(|c| self.dmat.get(c).copied()).unwrap_or(FAR)
    }

    /// Material within reach at cell `c`.
    fn target(&self, c: usize) -> bool {
        self.mat.get(c).copied().unwrap_or(false) && self.reach.get(c).copied().unwrap_or(false)
    }

    fn material_cells(&self) -> usize {
        self.left
    }

    /// The path so far, to read.
    fn cuts(&self) -> Result<std::sync::RwLockReadGuard<'_, Cuts>> {
        self.cuts.read().map_err(|_| CamError::Unsupported(LOST_PATH.into()))
    }

    /// The engagement at `q` (radians of the circumference in material, at most a half turn)
    /// and where the material lies (the sum of the directions to it).
    fn engage(&self, q: Vec2) -> Result<(f64, Vec2)> {
        let cuts = self.cuts()?;
        let mut n = 0usize;
        let mut dir = Vec2::ZERO;
        for u in self.ring.iter() {
            let (m, visits) = cuts.material_at(&self.g, self.k.r, self.stock_at, q + *u);
            if let Some(v) = visits {
                self.work.add_shared(v as f64)?;
            }
            if m {
                n += 1;
                dir += *u;
            }
        }
        Ok(((TAU * n as f64 / RING as f64).min(PI), dir))
    }

    /// [`Level::engage`] at every point of `qs`, in order, the crew's threads sharing them: the
    /// same engagements, and the same work counted in the same order, as one after the other.
    fn engage_all(&self, qs: &[Vec2], crew: &Crew<'a>) -> Result<Vec<(f64, Vec2)>> {
        if !crew.shared() || qs.len() <= 1 {
            return qs.iter().map(|q| self.engage(*q)).collect();
        }
        let (cuts, g, r, stock_at, ring) = (std::sync::Arc::clone(&self.cuts), self.g, self.k.r, self.stock_at, std::sync::Arc::clone(&self.ring));
        let at: std::sync::Arc<[Vec2]> = qs.into();
        let found = crew.run(
            qs.len(),
            std::sync::Arc::new(move |i: usize| {
                Worked::Engaged(match (cuts.read(), at.get(i)) {
                    (Ok(c), Some(q)) => Some(c.engage(&g, r, stock_at, &ring, *q)),
                    _ => None,
                })
            }),
        );
        #[cfg(test)]
        CREW_ENGAGED.fetch_add(qs.len(), std::sync::atomic::Ordering::Relaxed);
        let mut out = Vec::with_capacity(qs.len());
        for (q, w) in qs.iter().zip(found) {
            let Worked::Engaged(Some((n, dir, visits))) = w else { return Err(CamError::Unsupported(LOST_PATH.into())) };
            if let Some(v) = visits {
                if self.work.would_pass(v as f64) {
                    // The work runs out part way round this point's ring: where, as one after
                    // the other.
                    out.push(self.engage(*q)?);
                    continue;
                }
                self.work.add_shared(v as f64)?;
            }
            out.push(((TAU * n as f64 / RING as f64).min(PI), dir));
        }
        Ok(out)
    }

    /// Records the tool's centre moving from `a` to `b`; how many cells of material it cleared.
    fn sweep(&mut self, a: Vec2, b: Vec2) -> Result<usize> {
        let mut guard = self.cuts.write().map_err(|_| CamError::Unsupported(LOST_PATH.into()))?;
        let cuts = &mut *guard;
        let id = cuts.segs.len() as u32;
        cuts.segs.push((a, b));
        let slack = self.g.h * std::f64::consts::FRAC_1_SQRT_2 + 1e-9;
        let (settled, unsettled) = (self.k.r - slack, self.k.r + slack);
        let lo = Vec2::new(a.x.min(b.x), a.y.min(b.y));
        let hi = Vec2::new(a.x.max(b.x), a.y.max(b.y));
        let (is, js) = self.g.span(lo, hi, self.k.r + self.g.h);
        self.work.add((is.len() * js.len()) as f64)?;
        let mut n = 0;
        for j in js {
            for i in is.clone() {
                let c = j * self.g.nx + i;
                let d = seg_dist(self.g.center(i, j), a, b);
                let Some(dp) = cuts.dpath.get_mut(c) else { continue };
                if *dp <= settled {
                    continue;
                }
                if d < *dp {
                    *dp = d;
                    if d <= self.k.r
                        && let Some(m) = self.mat.get_mut(c)
                        && *m
                    {
                        *m = false;
                        n += 1;
                        if self.reach.get(c).copied().unwrap_or(false) {
                            self.left = self.left.saturating_sub(1);
                        }
                        self.dirty = Some(Window::with(self.dirty, i, j));
                    }
                }
                if *dp <= settled {
                    cuts.band.remove(&c);
                } else if d < unsettled {
                    cuts.band.entry(c).or_default().push(id);
                }
            }
        }
        Ok(n)
    }

    /// Records the tool's centre going round the circle of radius `rad` about `c`.
    fn bore(&mut self, c: Vec2, rad: f64) -> Result<()> {
        let n = ((TAU * rad / self.k.step).ceil() as usize).clamp(16, 4096);
        let mut prev = c + Vec2::new(rad, 0.0);
        for i in 1..=n {
            let next = c + Vec2::from_angle(TAU * i as f64 / n as f64) * rad;
            self.sweep(prev, next)?;
            prev = next;
        }
        Ok(())
    }

    /// Brings the distances to the material up to date with what the walks cleared: all of
    /// them the first time, then only round the cells cleared since. Material is only ever
    /// cleared, so a distance grows only where the material nearest it went: within
    /// [`Knobs::near`] of a cell cleared. Those cells are worked out again from the material
    /// within that much more round them; the rest keep theirs.
    fn refresh(&mut self, crew: &Crew<'a>) -> Result<()> {
        let full = Window { i0: 0, i1: self.g.nx, j0: 0, j1: self.g.ny };
        let (outer, inner) = if self.fresh {
            let Some(d) = self.dirty.take() else { return Ok(()) };
            let m = (self.k.near() / self.g.h).ceil() as usize + 1;
            (d.grown(2 * m, &self.g), d.grown(m, &self.g))
        } else {
            self.fresh = true;
            self.dirty = None;
            (full, full)
        };
        let (mat, reach) = (&self.mat, &self.reach);
        let (dmat, dall) = if crew.shared() {
            let (wnx, wny) = (outer.i1.saturating_sub(outer.i0), outer.j1.saturating_sub(outer.j0));
            let (mut near, mut all) = (Vec::with_capacity(wnx * wny), Vec::with_capacity(wnx * wny));
            for j in outer.j0..outer.j1 {
                for i in outer.i0..outer.i1 {
                    let c = j * self.g.nx + i;
                    let m = mat.get(c).copied().unwrap_or(false);
                    near.push(m && reach.get(c).copied().unwrap_or(false));
                    all.push(m);
                }
            }
            let mut both = distance_windows_shared(&self.g, vec![near, all], outer, inner, self.work, crew)?.into_iter();
            let lost = || CamError::Unsupported("a worker thread lost its work".into());
            (both.next().ok_or_else(lost)?, both.next().ok_or_else(lost)?)
        } else {
            let dmat = distance_window(
                &self.g,
                &|c| mat.get(c).copied().unwrap_or(false) && reach.get(c).copied().unwrap_or(false),
                outer,
                inner,
                self.work,
            )?;
            (dmat, distance_window(&self.g, &|c| mat.get(c).copied().unwrap_or(false), outer, inner, self.work)?)
        };
        let wnx = inner.i1 - inner.i0;
        for j in inner.j0..inner.j1 {
            for i in inner.i0..inner.i1 {
                let (c, w) = (j * self.g.nx + i, (j - inner.j0) * wnx + (i - inner.i0));
                if let (Some(d), Some(v)) = (self.dmat.get_mut(c), dmat.get(w)) {
                    *d = *v;
                }
                if let (Some(d), Some(v)) = (self.dall.get_mut(c), dall.get(w)) {
                    *d = *v;
                }
            }
        }
        Ok(())
    }

    /// A cell the tool can stand in with (next to) no material: links run through these.
    fn free(&self, c: usize) -> bool {
        self.allow.get(c).copied().unwrap_or(false) && self.dall.get(c).is_some_and(|d| *d >= self.k.r + self.k.h)
    }

    /// A free cell next to material, not started from before.
    fn frontier(&self, c: usize) -> bool {
        self.free(c)
            && !self.tried.get(c).copied().unwrap_or(true)
            && self.dmat.get(c).is_some_and(|d| *d <= self.k.r + 2.0 * self.k.step + 2.0 * self.k.h)
    }

    /// The direction from cell `c` towards the nearest material.
    fn towards_material(&self, c: usize) -> Vec2 {
        let (nx, i, j) = (self.g.nx, c % self.g.nx.max(1), c / self.g.nx.max(1));
        let at = |i: usize, j: usize| self.dmat.get(j * nx + i).copied().filter(|d| d.is_finite());
        let here = at(i, j).unwrap_or(0.0);
        let dx = at(i + 1, j).unwrap_or(here) - if i > 0 { at(i - 1, j).unwrap_or(here) } else { here };
        let dy = at(i, j + 1).unwrap_or(here) - if j > 0 { at(i, j - 1).unwrap_or(here) } else { here };
        (-Vec2::new(dx, dy)).normalized().unwrap_or(Vec2::new(1.0, 0.0))
    }

    /// The heading that puts the material on the side the milling wants.
    fn heading_for(&self, towards: Vec2) -> Vec2 {
        if self.k.climb { towards.perp() } else { -towards.perp() }
    }

    /// The nearest frontier cell reachable from `from` through free cells, and the link there.
    fn linked_frontier(&mut self, from: Vec2) -> Result<Option<(Vec<Vec2>, usize)>> {
        let Some(start) = self.g.index(from) else { return Ok(None) };
        let n = self.g.len();
        self.search = self.search.wrapping_add(1);
        if self.search == 0 {
            self.seen.fill(0);
            self.search = 1;
        }
        let search = self.search;
        let mut queue = VecDeque::new();
        if let (Some(p), Some(s)) = (self.parent.get_mut(start), self.seen.get_mut(start)) {
            *p = start as u32;
            *s = search;
        }
        queue.push_back(start);
        let (nx, ny) = (self.g.nx as isize, self.g.ny as isize);
        let mut visited = 0usize;
        let mut found = None;
        while let Some(c) = queue.pop_front() {
            visited += 1;
            if self.frontier(c) {
                found = Some(c);
                break;
            }
            let (ci, cj) = ((c % self.g.nx) as isize, (c / self.g.nx) as isize);
            for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let (i, j) = (ci + di, cj + dj);
                if i < 0 || j < 0 || i >= nx || j >= ny {
                    continue;
                }
                let d = (j * nx + i) as usize;
                if self.seen.get(d).is_some_and(|s| *s != search) && self.free(d) {
                    if let (Some(p), Some(s)) = (self.parent.get_mut(d), self.seen.get_mut(d)) {
                        *p = c as u32;
                        *s = search;
                    }
                    queue.push_back(d);
                }
            }
        }
        self.work.add(visited as f64 * 8.0)?;
        let Some(end) = found else { return Ok(None) };
        let mut cells = vec![end];
        let mut c = end;
        while c != start {
            let Some(p) = self.parent.get(c).map(|p| *p as usize) else { break };
            if p == c || cells.len() > n {
                break;
            }
            cells.push(p);
            c = p;
        }
        cells.reverse();
        let mut pts: Vec<Vec2> = vec![from];
        pts.extend(cells.iter().skip(1).map(|c| self.g.center_of(*c)));
        Ok(Some((self.shortcut(pts)?, end)))
    }

    /// Whether the straight feed from `a` to `b` runs through free cells only.
    fn free_line(&self, a: Vec2, b: Vec2) -> bool {
        let n = (a.dist(b) / (0.5 * self.k.h)).ceil().clamp(1.0, 1.0e6) as usize;
        (1..=n).all(|i| self.g.index(a.lerp(b, i as f64 / n as f64)).is_some_and(|c| self.free(c)))
    }

    /// Whether the straight feed from `a` to `b` stays exactly where the centre may go.
    fn exact_line(&self, a: Vec2, b: Vec2) -> Result<bool> {
        let m = (a.dist(b) / self.k.step).ceil().clamp(1.0, 1.0e6) as usize;
        for i in 1..=m {
            if !self.domain.allowed(a.lerp(b, i as f64 / m as f64))? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The link's corners, with the corners a straight feed can skip left out. Consecutive
    /// corners are cell centres (where the centre may stand) a cell apart, so the link through
    /// all of them is always safe; a shortcut is taken only when it is too.
    fn shortcut(&self, pts: Vec<Vec2>) -> Result<Vec<Vec2>> {
        let Some(&first) = pts.first() else { return Ok(pts) };
        let mut out = vec![first];
        let mut i = 0usize;
        while i + 1 < pts.len() {
            let a = pts.get(i).copied().unwrap_or(first);
            let mut j = i + 1;
            while j + 1 < pts.len() && j < i + 2000 && pts.get(j + 1).is_some_and(|b| self.free_line(a, *b)) {
                j += 1;
            }
            if j > i + 1 && !pts.get(j).map_or(Ok(false), |b| self.exact_line(a, *b))? {
                j = i + 1;
            }
            if let Some(b) = pts.get(j) {
                out.push(*b);
            }
            i = j;
        }
        Ok(out)
    }

    /// The frontier cell nearest `near` anywhere (reached by lifting).
    fn any_frontier(&self, near: Option<Vec2>) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for c in 0..self.g.len() {
            if !self.frontier(c) {
                continue;
            }
            let d = near.map_or(0.0, |p| self.g.center_of(c).dist(p));
            if best.is_none_or(|(_, b)| d < b) {
                best = Some((c, d));
                if near.is_none() {
                    break;
                }
            }
        }
        best.map(|(c, _)| c)
    }

    /// Whether some material is out of reach of every free cell (it needs an entry of its own).
    fn untouched(&mut self) -> Result<Option<Vec<f64>>> {
        let free: Vec<bool> = (0..self.g.len()).map(|c| self.free(c)).collect();
        let dfree = distance_field(&self.g, &|c| free.get(c).copied().unwrap_or(false), self.work)?;
        let reach = self.k.r + 3.0 * self.k.step + 2.0 * self.k.h;
        let any = dfree.iter().enumerate().any(|(c, d)| self.target(c) && *d > reach);
        Ok(any.then_some(dfree))
    }

    /// Where an entry helix goes into untouched material: the cell with the most room round it.
    fn helix_site(&self, dfree: &[f64], near: Option<Vec2>, rad: f64) -> Result<std::result::Result<Vec2, f64>> {
        let need = rad + self.k.h;
        let reach = self.k.r + 3.0 * self.k.step + 2.0 * self.k.h;
        let mut best: Option<(usize, f64, f64)> = None;
        let mut most = 0.0f64;
        for c in 0..self.g.len() {
            let untouched = self.target(c) && dfree.get(c).is_some_and(|d| *d > reach);
            if !untouched || !self.allow.get(c).copied().unwrap_or(false) {
                continue;
            }
            let room = self.room.get(c).copied().unwrap_or(0.0);
            most = most.max(room);
            if room < need {
                continue;
            }
            // Of the cells with room, the one nearest the tool (the most room when it is up).
            let d = near.map_or(-room, |p| self.g.center_of(c).dist(p) - room * 1e-3);
            if best.is_none_or(|(_, _, b)| d < b) {
                best = Some((c, room, d));
            }
        }
        let Some((c, _, _)) = best else { return Ok(Err(most)) };
        let center = self.g.center_of(c);
        let n = ((TAU * rad / self.k.step).ceil() as usize).clamp(16, 4096);
        for i in 0..n {
            if !self.domain.allowed(center + Vec2::from_angle(TAU * i as f64 / n as f64) * rad)? {
                return Ok(Err(most.min(rad)));
            }
        }
        Ok(Ok(center))
    }

    /// Walks from `start` heading `heading`, clearing as it goes; the points from `start` on,
    /// ending at the last one that cut.
    fn walk(&mut self, start: Vec2, heading: Vec2, crew: &Crew<'a>) -> Result<Vec<Vec2>> {
        let k = self.k;
        let mut pts = vec![start];
        let (mut pos, mut dir) = (start, heading.normalized().unwrap_or(Vec2::new(1.0, 0.0)));
        let mut air = 0.0;
        let mut last_cut = 0usize;
        let max_air = k.max_air();
        let side = if k.climb { 1.0 } else { -1.0 };
        // Steps through air not yet recorded: they are recorded when the walk cuts again, and
        // dropped with the end of the walk otherwise, so the model holds only what is cut.
        let mut pending: Vec<(Vec2, Vec2)> = Vec::new();
        let mut fan: Vec<(f64, Vec2, Vec2, f64)> = Vec::with_capacity(4 * FAN as usize + 2);
        let mut tries: Vec<(f64, Vec2, Vec2)> = Vec::with_capacity(2 * FAN as usize + 1);
        let mut at: Vec<Vec2> = Vec::with_capacity(2 * FAN as usize + 1);
        loop {
            if *self.steps >= MAX_STEPS {
                return Err(CamError::TooLarge(format!(
                    "the adaptive clearing needs more than {MAX_STEPS} steps; use a larger optimal load or engagement limit, or a smaller region"
                )));
            }
            if pts.len() >= MAX_MOVES / 4 {
                break;
            }
            let mut next = None;
            // The usual fan first; when no heading in it keeps within the limit, sharper turns
            // with shorter steps (turning away from the material lightens the cut).
            for (spread, length) in [(MAX_TURN, k.step), (4.0 * MAX_TURN, 0.5 * k.step)] {
                fan.clear();
                tries.clear();
                at.clear();
                for i in -FAN..=FAN {
                    let turn = spread * i as f64 / FAN as f64;
                    let d = rotate(dir, turn);
                    let q = pos + d * length;
                    if self.grid_allows(q) {
                        tries.push((turn, d, q));
                        at.push(q);
                    }
                }
                let engaged = self.engage_all(&at, crew)?;
                for (&(turn, d, q), &(phi, mdir)) in tries.iter().zip(&engaged) {
                    if phi > k.limit {
                        continue;
                    }
                    let score = if phi > 0.0 {
                        // Material on the wrong side of the travel costs as much as missing the load.
                        let wrong = (d.cross(mdir) * side).max(0.0) / mdir.len().max(1e-12);
                        let light = (k.least - phi).max(0.0);
                        (phi - k.target).abs() + light + wrong * k.target + 0.05 * turn.abs()
                    } else {
                        // Cutting air: towards the nearest material.
                        10.0 + self.dmat_at(q).min(1.0e6) / k.r + 0.05 * turn.abs()
                    };
                    fan.push((score, q, d, phi));
                }
                fan.sort_by(|a, b| a.0.total_cmp(&b.0));
                for &(_, q, d, phi) in &fan {
                    if self.domain.allowed(q)? {
                        next = Some((q, d, phi));
                        break;
                    }
                }
                if next.is_some() {
                    break;
                }
            }
            let Some((q, d, phi)) = next else { break };
            *self.steps += 1;
            let cleared = if phi > 0.0 {
                for (a, b) in std::mem::take(&mut pending) {
                    self.sweep(a, b)?;
                }
                self.sweep(pos, q)?
            } else {
                pending.push((pos, q));
                0
            };
            pts.push(q);
            pos = q;
            dir = d;
            if phi > 0.0 {
                // Recorded in the model: the path must keep it.
                last_cut = pts.len() - 1;
            }
            // Material in the tool's circle at the new point is cut by the move there: the
            // walk is cutting, whether or not a cell's centre was cleared (a load lighter than
            // the raster's cell clears few of them).
            if cleared > 0 || phi > 0.0 {
                air = 0.0;
            } else {
                air += pos.dist(pts.get(pts.len().saturating_sub(2)).copied().unwrap_or(pos)).max(1e-9);
                if air > max_air {
                    break;
                }
            }
        }
        pts.truncate(last_cut + 1);
        Ok(pts)
    }

    /// Whether `cells` of material within reach are only crumbs ([`CRUMB_SHARE`],
    /// [`CRUMB_AREA`]) of the level's.
    fn only_crumbs(&self, cells: usize) -> bool {
        cells as f64 <= CRUMB_SHARE * self.start_left as f64 || cells as f64 * self.g.h * self.g.h <= CRUMB_AREA
    }

    /// Plans the level: the steps from wherever the tool is (`None`: up) to every cut. Where the
    /// level may stop with crumbs left (`crumbs`), walks refused for their work, or spending
    /// more than [`CRUMB_WORK`] of the level's on crumbs, stop it there: the steps so far are its
    /// plan, and `stopped` says what is left.
    fn plan(&mut self, pos: Option<Vec2>, floor_at: &dyn Fn(Vec2, f64) -> Option<f64>) -> Result<(Vec<Step>, usize)> {
        crate::par::with_crew(crate::par::crew_threads(), |crew| self.plan_with(pos, floor_at, crew))
    }

    /// [`Level::plan`], `crew` sharing the work of the walks.
    fn plan_with(&mut self, mut pos: Option<Vec2>, floor_at: &dyn Fn(Vec2, f64) -> Option<f64>, crew: &Crew<'a>) -> Result<(Vec<Step>, usize)> {
        let mut steps = Vec::new();
        let mut helices = 0usize;
        let mut retry = true;
        for _ in 0..MAX_PASSES {
            let before = self.left;
            if self.crumbs && before > 0 && self.only_crumbs(before) {
                let spent = self.work.level.get();
                let from = *self.crumbs_from.get_or_insert(spent);
                if spent - from > CRUMB_WORK * from {
                    self.stopped = Some(before);
                    break;
                }
            }
            match self.pass(&mut steps, &mut helices, &mut pos, &mut retry, floor_at, crew) {
                Ok(true) => {}
                Ok(false) => break,
                // Walks refused for their work (a walk's moves dropped with the material it was
                // clearing, so what was left before it is what is left).
                Err(CamError::TooLarge(m)) if self.crumbs && m.starts_with(WALKS_MISS) && self.only_crumbs(before) => {
                    self.left = before;
                    self.stopped = Some(before);
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok((steps, helices))
    }

    /// One walk of [`Level::plan`] (or a helix and the walk round it), its steps added to
    /// `steps`: false when the level is done.
    fn pass(
        &mut self,
        steps: &mut Vec<Step>,
        helices: &mut usize,
        pos: &mut Option<Vec2>,
        retry: &mut bool,
        floor_at: &dyn Fn(Vec2, f64) -> Option<f64>,
        crew: &Crew<'a>,
    ) -> Result<bool> {
        solidcraft_profile::scope("cam", "adaptive.refresh", || self.refresh(crew))?;
        if self.material_cells() == 0 {
            return Ok(false);
        }
        let linked = match *pos {
            Some(p) => solidcraft_profile::scope("cam", "adaptive.linked_frontier", || self.linked_frontier(p))?,
            None => None,
        };
        let (lead, start, heading) = if let Some((link, c)) = linked {
            if let Some(t) = self.tried.get_mut(c) {
                *t = true;
            }
            let at = self.g.center_of(c);
            (Some(Step::Link(link)), at, self.heading_for(self.towards_material(c)))
        } else if let Some(c) = solidcraft_profile::scope("cam", "adaptive.any_frontier", || self.any_frontier(*pos)) {
            if let Some(t) = self.tried.get_mut(c) {
                *t = true;
            }
            let at = self.g.center_of(c);
            (Some(Step::Drop(at)), at, self.heading_for(self.towards_material(c)))
        } else if let Some(dfree) = solidcraft_profile::scope("cam", "adaptive.untouched", || self.untouched())? {
            if !self.helix {
                // No entry of its own: what the walks from the cleared area did not reach
                // is left (after one more try from every start).
                if *retry {
                    *retry = false;
                    self.tried.fill(false);
                    return Ok(true);
                }
                self.unentered = true;
                return Ok(false);
            }
            let asked = self.k.helix_radius;
            let (center, rad) = match self.helix_site(&dfree, *pos, asked)? {
                Ok(c) => (c, asked),
                Err(most) => {
                    // The largest helix the room takes (a cell's margin inside it), down to
                    // the smallest worth a helix: used, and said, where the one asked for
                    // has no room.
                    let fit = ((most - self.k.h) * (1.0 - 1e-6) * 100.0).floor() / 100.0;
                    let least = self.k.r * MIN_HELIX_SHARE;
                    let site = if fit >= least && fit < asked { self.helix_site(&dfree, *pos, fit)?.ok() } else { None };
                    match site {
                        Some(c) => {
                            self.shrunk = Some(self.shrunk.map_or(fit, |s| s.min(fit)));
                            (c, fit)
                        }
                        // Only crumbs left where no helix fits (material along a wall the walks
                        // missed, held off the cleared area by what the tool cannot reach in the
                        // corner beside it): left for the finish or a rest pass, as when the
                        // walks keep missing them, rather than refusing a level cleared but for them.
                        None if self.crumbs && self.only_crumbs(self.left) => {
                            self.stopped = Some(self.left);
                            return Ok(false);
                        }
                        None => {
                            return invalid(format!(
                                "material the tool cannot reach from the cleared area within the engagement limit has no room for the entry helix of diameter {:.3} mm, nor for one of {:.3} mm, the smallest this tool takes ({}% of its diameter): use a smaller tool or a larger engagement limit",
                                2.0 * asked,
                                2.0 * least,
                                MIN_HELIX_SHARE * 100.0
                            ));
                        }
                    }
                }
            };
            let from = floor_at(center, rad + self.k.r);
            steps.push(Step::Helix { center, from, radius: rad });
            *helices += 1;
            self.bore(center, rad)?;
            let at = center + Vec2::new(rad, 0.0);
            *pos = Some(at);
            let tangent = if self.k.climb { Vec2::new(0.0, 1.0) } else { Vec2::new(0.0, -1.0) };
            // The bore's outside is material: walk round it from where the helix ends.
            solidcraft_profile::scope("cam", "adaptive.refresh", || self.refresh(crew))?;
            let pts = solidcraft_profile::scope("cam", "adaptive.walk", || self.walk(at, tangent, crew))?;
            if pts.len() >= 2 {
                *pos = pts.last().copied();
                steps.push(Step::Cut(pts));
            }
            return Ok(true);
        } else if *retry && self.material_cells() > 0 {
            // Every start was tried while the material round it was another shape: try them
            // all once more on what is left (slivers along a wall the walks turned off).
            *retry = false;
            self.tried.fill(false);
            return Ok(true);
        } else {
            return Ok(false);
        };
        let pts = solidcraft_profile::scope("cam", "adaptive.walk", || self.walk(start, heading, crew))?;
        if pts.len() >= 2 {
            if let Some(l) = lead {
                steps.push(l);
            }
            *pos = pts.last().copied();
            steps.push(Step::Cut(pts));
        }
        Ok(true)
    }
}

/// Points of a polyline within `tol` of it, its ends kept (Douglas-Peucker, without recursion).
fn simplify(pts: &[Vec2], tol: f64) -> Vec<Vec2> {
    let n = pts.len();
    if n <= 2 || !(tol > 0.0) {
        return pts.to_vec();
    }
    let mut keep = vec![false; n];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    let mut stack = vec![(0usize, n - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (Some(pa), Some(pb)) = (pts.get(a).copied(), pts.get(b).copied()) else { continue };
        let mut worst = (a, 0.0f64);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = seg_dist(*p, pa, pb);
            if d > worst.1 {
                worst = (i, d);
            }
        }
        if worst.1 > tol {
            if let Some(k) = keep.get_mut(worst.0) {
                *k = true;
            }
            stack.push((a, worst.0));
            stack.push((worst.0, b));
        }
    }
    pts.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect()
}

/// Collects moves with their roles, keeping the tool's position.
struct Emit {
    moves: Vec<Move>,
    roles: Vec<MoveRole>,
    pos: Option<Vec3>,
    h: Heights,
    feeds: Feeds,
    lifts: usize,
}

impl Emit {
    fn new(h: Heights, feeds: Feeds) -> Emit {
        Emit { moves: Vec::new(), roles: Vec::new(), pos: None, h, feeds, lifts: 0 }
    }

    fn push(&mut self, m: Move, role: MoveRole) -> Result<()> {
        if self.moves.len() >= MAX_MOVES {
            return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
        }
        if let Some(to) = m.target() {
            self.pos = Some(to);
        }
        self.moves.push(m);
        self.roles.push(role);
        Ok(())
    }

    /// Up to the retract height if below it.
    fn up(&mut self) -> Result<()> {
        match self.pos {
            Some(q) if q.z < self.h.retract => self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.retract) }, MoveRole::Position),
            _ => Ok(()),
        }
    }

    /// Over `p` at the retract height (from the clearance height for the first move).
    fn over(&mut self, p: Vec2) -> Result<()> {
        match self.pos {
            None => {
                self.push(Move::Rapid { to: p.extend(self.h.clearance) }, MoveRole::Position)?;
                self.push(Move::Rapid { to: p.extend(self.h.retract) }, MoveRole::Position)
            }
            Some(q) => {
                if q.z < self.h.retract {
                    self.lifts += 1;
                }
                self.up()?;
                if Vec2::new(q.x, q.y).dist(p) > 1e-9 { self.push(Move::Rapid { to: p.extend(self.h.retract) }, MoveRole::Position) } else { Ok(()) }
            }
        }
    }

    /// Straight down to `z` at the plunge feed (through air).
    fn down(&mut self, z: f64) -> Result<()> {
        match self.pos {
            Some(q) if q.z > z + 1e-9 => self.push(Move::Feed { to: Vec3::new(q.x, q.y, z), feed: self.feeds.plunge }, MoveRole::Position),
            _ => Ok(()),
        }
    }

    fn feed_to(&mut self, p: Vec2, z: f64, role: MoveRole) -> Result<()> {
        if self.pos.is_some_and(|q| Vec2::new(q.x, q.y).dist(p) <= 1e-9 && (q.z - z).abs() <= 1e-9) {
            return Ok(());
        }
        self.push(Move::Feed { to: p.extend(z), feed: self.feeds.cut }, role)
    }

    /// Down a helix of radius `rad` about `c` from `from` to `z`, starting and ending at
    /// `c + (rad, 0)` after a last circle at `z`; counter-clockwise when `ccw`.
    fn helix(&mut self, c: Vec2, rad: f64, from: f64, z: f64, ramp: f64, ccw: bool) -> Result<()> {
        let start = c + Vec2::new(rad, 0.0);
        self.over(start)?;
        self.down(from.max(z))?;
        let drop = from - z;
        let per_half = PI * rad * ramp.tan();
        let halves = if drop > 1e-9 { (drop / per_half - 1e-9).ceil().max(1.0) } else { 0.0 };
        if !(halves <= MAX_HALF_TURNS) {
            return invalid("the entry helix needs too many turns; use a steeper ramp angle or a larger helix diameter");
        }
        // An even number of half turns, so the helix ends where it started.
        let halves = (halves / 2.0).ceil() as usize * 2;
        let sign = if ccw { 1.0 } else { -1.0 };
        let total = halves + 2;
        for k in 1..=total {
            let p = c + Vec2::from_angle(sign * PI * k as f64) * rad;
            let zk = if k >= halves { z } else { from - drop * k as f64 / halves.max(1) as f64 };
            let dz = self.pos.map_or(0.0, |q| q.z - zk);
            let feed = descent_feed(&self.feeds, (PI * rad).hypot(dz), dz);
            self.push(Move::Arc { to: p.extend(zk), center: c, ccw, feed }, MoveRole::Entry)?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(Toolpath, Vec<MoveRole>, usize)> {
        if let Some(q) = self.pos
            && q.z < self.h.clearance
        {
            self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.clearance) }, MoveRole::Position)?;
        }
        let path = Toolpath { moves: self.moves };
        verify(&path, &self.h)?;
        Ok((path, self.roles, self.lifts))
    }
}

/// Writes one level's steps at `z`; `slab_top`: where the material starts above the level.
fn emit_level(e: &mut Emit, steps: &[Step], z: f64, slab_top: f64, k: &Knobs) -> Result<()> {
    for step in steps {
        match step {
            Step::Drop(p) => {
                e.over(*p)?;
                e.down(z)?;
            }
            Step::Helix { center, from, radius } => {
                let top = from.unwrap_or(slab_top).max(z).min(e.h.retract);
                e.helix(*center, *radius, top, z, k.ramp, k.climb)?;
            }
            Step::Link(pts) => {
                for p in pts.iter().skip(1) {
                    e.feed_to(*p, z, MoveRole::Link)?;
                }
            }
            Step::Cut(pts) => {
                for p in simplify(pts, k.simplify).iter().skip(1) {
                    e.feed_to(*p, z, MoveRole::Cut)?;
                }
            }
        }
    }
    e.up()
}

/// Drop-cutter samples one thread takes at least.
const DROPS_PER_THREAD: usize = 512;

/// [`drop_z`] of `cutter` at each of `at`, in order, over the cores.
fn drops(surface: &Surface, cutter: &Cutter, at: &[Vec2]) -> Result<Vec<Option<f64>>> {
    let ranges = crate::par::split(&vec![1; at.len()], crate::par::threads_for(at.len(), DROPS_PER_THREAD));
    let parts = crate::par::map(ranges, |r| {
        crate::stop::check()?;
        Ok(at.get(r).unwrap_or(&[]).iter().map(|q| drop_z(surface, cutter, q.x, q.y)).collect::<Vec<_>>())
    })?;
    Ok(parts.into_iter().flatten().collect())
}

/// Points along a path's cutting moves, at most `spacing` apart (arcs followed).
fn cutting_samples(path: &Toolpath, spacing: f64) -> Vec<Vec3> {
    let mut out = Vec::new();
    let mut pos: Option<Vec3> = None;
    for m in &path.moves {
        match *m {
            Move::Feed { to, .. } => {
                if let Some(p) = pos {
                    let n = (p.dist(to) / spacing).ceil().clamp(1.0, 1.0e5) as usize;
                    out.extend((1..=n).map(|i| p.lerp(to, i as f64 / n as f64)));
                }
            }
            Move::Arc { to, center, ccw, .. } => {
                if let Some(p) = pos {
                    let start = Vec2::new(p.x, p.y) - center;
                    let (r, sweep) = (start.len(), arc_sweep(p, to, center, ccw));
                    let n = ((r * sweep.abs()).hypot(to.z - p.z) / spacing).ceil().clamp(1.0, 1.0e5) as usize;
                    for i in 1..=n {
                        let t = i as f64 / n as f64;
                        out.push((center + Vec2::from_angle(start.angle() + sweep * t) * r).extend(p.z + (to.z - p.z) * t));
                    }
                }
            }
            Move::PlaneArc { to, center, plane, ccw, .. } => {
                if let Some(from) = pos {
                    let arc = crate::path::PlaneArc { from, to, center, plane, ccw };
                    let n = (arc.length() / spacing).ceil().clamp(1.0, 1.0e5) as usize;
                    out.extend((1..=n).map(|i| arc.at(i as f64 / n as f64)));
                }
            }
            _ => {}
        }
        if let Some(to) = m.target() {
            pos = Some(to);
        }
    }
    out
}

/// Adaptive clearing of `regions` (holes are islands) in depth passes, with nothing known of
/// the stock but that it fills the regions: [`adaptive_in`] with the default [`StockModel`].
pub fn adaptive(regions: &[Region2], tool: &Tool, params: &AdaptiveParams, cut: &Cut) -> Result<AdaptivePath> {
    adaptive_in(regions, tool, params, cut, &StockModel::default())
}

/// Adaptive clearing of `regions` (holes are islands) in depth passes, cleared out to the walls
/// (less the stock to leave), of the material `stock` says is there.
///
/// With the stock's extent, the material is the regions' part of it, and the tool may also
/// stand past a region's boundary where it meets no stock outside the regions: it goes down in
/// the open air beside the stock and cuts in from there, and never cuts air where a region
/// reaches past the stock. Material no walk can reach from open or cleared space is entered with
/// a helix, refused when the helix has no room. Levels at or above the stock's top are left
/// out. Without a map of earlier cuts every level is the same and is planned once; with one,
/// each level is planned on what the map leaves at it (rest machining).
pub fn adaptive_in(regions: &[Region2], tool: &Tool, params: &AdaptiveParams, cut: &Cut, stock: &StockModel) -> Result<AdaptivePath> {
    adaptive_budgeted(regions, tool, params, cut, stock, 1.0)
}

#[cfg(test)]
thread_local! {
    /// Clearings this thread planned once (each load tried counts).
    static PLANNED_ONCE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Levels whose walks this thread stopped with only crumbs left.
    static CRUMB_STOPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many levels' walks this thread stopped with only crumbs left ([`CRUMB_SHARE`]).
#[cfg(test)]
pub(crate) fn crumb_stops() -> usize {
    CRUMB_STOPS.with(std::cell::Cell::get)
}

/// How many clearings this thread planned once, each load tried counted.
#[cfg(test)]
pub(crate) fn clearings_planned() -> usize {
    PLANNED_ONCE.with(std::cell::Cell::get)
}

/// [`adaptive_in`] with its work allowance scaled by `budget` (1 as planned for real; the tests
/// take less to make a refusal quick). A clearing refused for the work its walks take names the
/// smallest optimal load that plans, found by planning at larger loads.
pub(crate) fn adaptive_budgeted(
    regions: &[Region2],
    tool: &Tool,
    params: &AdaptiveParams,
    cut: &Cut,
    stock: &StockModel,
    budget: f64,
) -> Result<AdaptivePath> {
    let progress = std::cell::Cell::new(1.0);
    match adaptive_once(regions, tool, params, cut, stock, budget, &progress) {
        Err(CamError::TooLarge(msg)) if msg.starts_with(WALKS_MISS) => {
            let hint = match planning_load(regions, tool, params, cut, stock, budget, progress.get())? {
                Some(w) => format!(
                    "use an optimal load of at least {w:.2} mm ({:.1}% of the tool's diameter), the smallest that plans here (its engagement limit raised with it as by default), or split the region",
                    w / tool.diameter * 100.0
                ),
                None => "no optimal load up to the tool's diameter plans it: use a larger tool, or split the region".into(),
            };
            Err(CamError::TooLarge(format!("{WALKS_MISS}; {hint}")))
        }
        r => r,
    }
}

/// `params` at optimal load `w`, its largest engagement raised with the load (to one and a half
/// times the load's, and at least 15° more).
fn raised(tool: &Tool, params: &AdaptiveParams, w: f64) -> AdaptiveParams {
    let r = tool.radius();
    let w = w.min(tool.diameter);
    let opt = (1.0 - w / r).clamp(-1.0, 1.0).acos();
    let most = params.max_engagement.max((opt * 1.5).max(opt + 15.0 * PI / 180.0).min(PI));
    AdaptiveParams { optimal_load: w, max_engagement: most, ..*params }
}

/// The smallest optimal load above `params`' that plans, by planning: `left` is the share of the
/// material a refused attempt left, so the next tries a load about as many times larger as that
/// attempt fell short (the work a clearing takes goes as the area over the load), then halves the
/// gap to the last refused load twice. Rounded up to 0.01 mm, at a load that was planned.
fn planning_load(
    regions: &[Region2],
    tool: &Tool,
    params: &AdaptiveParams,
    cut: &Cut,
    stock: &StockModel,
    budget: f64,
    left: f64,
) -> Result<Option<f64>> {
    let d = tool.diameter;
    let plans = |w: f64| -> Result<(bool, f64)> {
        let progress = std::cell::Cell::new(1.0);
        match adaptive_once(regions, tool, &raised(tool, params, w), cut, stock, budget, &progress) {
            Ok(_) => Ok((true, 0.0)),
            Err(CamError::TooLarge(_)) => Ok((false, progress.get())),
            Err(e) => Err(e),
        }
    };
    let ceil = |w: f64| ((w * 100.0).ceil() / 100.0).min(d);
    let (mut lo, mut left) = (params.optimal_load, left);
    let mut hi = None;
    for _ in 0..8 {
        if lo >= d {
            break;
        }
        // A load as many times larger as the share of the material cleared fell short of all of
        // it, and a fifth more.
        let grow = (1.2 / (1.0 - left.clamp(0.0, 0.9))).clamp(1.25, 4.0);
        let w = ceil(lo * grow);
        let (ok, l) = plans(w)?;
        if ok {
            hi = Some(w);
            break;
        }
        (lo, left) = (w, l);
    }
    let Some(mut hi) = hi else { return Ok(None) };
    for _ in 0..2 {
        let mid = ceil(0.5 * (lo + hi));
        if mid >= hi || mid <= lo {
            break;
        }
        if plans(mid)?.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Ok(Some(hi))
}

/// [`adaptive_in`] once, its work scaled by `budget`; when refused for the work its walks take,
/// `progress` is the share of the material within reach they left.
fn adaptive_once(
    regions: &[Region2],
    tool: &Tool,
    params: &AdaptiveParams,
    cut: &Cut,
    stock: &StockModel,
    budget: f64,
    progress: &std::cell::Cell<f64>,
) -> Result<AdaptivePath> {
    #[cfg(test)]
    PLANNED_ONCE.with(|n| n.set(n.get() + 1));
    mills(tool, params.ramp_angle)?;
    params.validate(tool)?;
    cut.heights.validate()?;
    cut.feeds.validate()?;
    check_stepdown(cut.stepdown)?;
    stock.validate()?;
    let all = levels(cut.heights.top, cut.heights.bottom, cut.stepdown, false)?;
    check_flutes(tool, cut.heights.top, &all)?;
    let k = Knobs::new(tool, params);
    let shape = Shape::from_regions(regions, params.tolerance)?;
    let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for piece in shape.loops().iter().flatten() {
        let (a, b) = piece.bounds();
        lo = Vec2::new(lo.x.min(a.x), lo.y.min(a.y));
        hi = Vec2::new(hi.x.max(b.x), hi.y.max(b.y));
    }
    let wall = k.r + params.stock_to_leave + k.margin;
    // With the stock's extent the tool may go past the regions' boundary into open air: the
    // raster reaches as far round them as an adaptive roughing's round its stock.
    let grow = if stock.rect.is_some() { k.r + 2.0 * k.step + 4.0 * k.h } else { 2.0 * k.h };
    let g = Grid::new(lo - Vec2::new(grow, grow), hi + Vec2::new(grow, grow), k.h)?;
    let pieces = shape.loops().iter().map(Vec::len).sum::<usize>().max(1);
    let fence = stock.rect.map(|[slo, shi]| Fence::new(shape.clone(), slo, shi, wall));
    let walls = Walls { shape, wall };
    let domain: &dyn Domain = match &fence {
        Some(f) => f,
        None => &walls,
    };
    let fence_pieces = fence.as_ref().map_or(0, |f| f.walls.len());
    let top = stock.top.map_or(cut.heights.top, |t| t.min(cut.heights.top));
    let zs: Vec<f64> = all.iter().copied().filter(|z| *z < top - 1e-9).collect();
    let per_level = stock.heights.is_some();
    let planned_levels = if per_level { zs.len() } else { zs.len().min(1) };
    let mut work = Work::new(MAX_WORK * budget, level_floor_2d(planned_levels) * budget);
    work.add(g.len() as f64 * (pieces + fence_pieces) as f64 * 0.05)?;
    // Where the centre may stand, and the cells inside the regions (the material's raster), from
    // one pass over the walls; a fence tests the centres off the regions' walls itself.
    let (clear, inside) = solidcraft_profile::scope("cam", "adaptive.raster", || shape_raster(&g, &walls.shape, wall, k.h));
    let mut allow = Vec::with_capacity(g.len());
    for (c, ok) in clear.iter().enumerate() {
        allow.push(match &fence {
            Some(f) if !*ok => f.allowed(g.center_of(c))?,
            _ => *ok,
        });
    }
    drop(clear);
    let inside_at = |c: usize| inside.get(c).copied().unwrap_or(false);
    let standing = allow.iter().filter(|a| **a).count();
    if standing == 0 {
        return invalid("the tool does not fit the region");
    }
    // Material the tool cannot reach from anywhere it may stand, past the stock to leave on the
    // walls (and a cell and a half for the raster): corners tighter than its radius and places
    // narrower than it, left for a smaller tool. Seen from above, at the first level.
    let corner_span = solidcraft_profile::span("cam", "adaptive.corners");
    // The distance to where the centre may stand gives the levels' reach (the same at each).
    let mut reach: Option<Vec<bool>> = None;
    let corner_area = match zs.first() {
        Some(&z0) => {
            let dist = distance_field(&g, &|c| allow.get(c).copied().unwrap_or(false), &mut work)?;
            reach = Some(Level::reach_of(&dist, &k));
            let past = k.r + params.stock_to_leave + k.margin + 1.5 * k.h;
            let n = (0..g.len())
                .filter(|&c| {
                    let p = g.center_of(c);
                    dist.get(c).is_some_and(|d| *d > past) && inside_at(c) && stock.material_at(p, z0)
                })
                .count();
            n as f64 * k.h * k.h
        }
        None => 0.0,
    };
    drop(corner_span);
    // Refused at once where the walks would take too many steps: the area the centre may
    // cover and the tool's reach round it, at every level planned.
    let area = standing as f64 * k.h * k.h + (hi.x - lo.x + hi.y - lo.y) * 2.0 * k.r;
    let fits = |kn: &Knobs| expected_steps(kn, area) * planned_levels as f64 <= MAX_STEPS as f64;
    if !fits(&k) {
        return Err(CamError::TooLarge(format!(
            "the adaptive clearing at an optimal load of {} mm needs some {:.2e} steps over {area:.0} mm² and {planned_levels} planned levels, more than {MAX_STEPS}; {}",
            params.optimal_load,
            expected_steps(&k, area) * planned_levels as f64,
            load_hint(tool, params, &fits)
        )));
    }
    let mut e = Emit::new(cut.heights, cut.feeds);
    let mut slab_top = top;
    let (mut helices, mut left, mut cut_levels) = (0usize, 0usize, 0usize);
    let mut once: Option<(Vec<Step>, usize, usize)> = None;
    let mut room: Option<Vec<f64>> = None;
    let mut shrunk: Option<f64> = None;
    // Some level's walks stopped with only crumbs left.
    let mut crumbs = false;
    for &z in &zs {
        let (plan, n_helix, n_left) = match &once {
            Some(p) if !per_level => p.clone(),
            _ => {
                let mut steps_taken = 0usize;
                let cell_stock = |c: usize, p: Vec2| inside_at(c) && stock.material_at(p, z);
                let at = |p: Vec2| stock.material_at(p, z);
                let cap = slab_top;
                // A plan made once is written at every level: its helices start from the level
                // above where they are written (`None`), cleared by then. A level planned on its
                // own starts them where the map leaves material over them.
                let floor_at = |c: Vec2, rad: f64| per_level.then(|| stock.top_over(c, rad, cap).max(z));
                let _span = solidcraft_profile::span("cam", "adaptive.level");
                let mut lv = solidcraft_profile::scope("cam", "adaptive.level_new", || {
                    Level::new(g, k, domain, allow.clone(), &cell_stock, &at, &mut steps_taken, &mut work, (reach.clone(), room.clone()))
                })?;
                // Where the centre may stand is the same at every level: so is its room.
                room.get_or_insert_with(|| lv.room.clone());
                lv.crumbs = true;
                let (plan, n_helix) = match lv.plan(None, &floor_at) {
                    Ok(p) => p,
                    Err(e) => {
                        progress.set(lv.left as f64 / lv.start_left.max(1) as f64);
                        return Err(e);
                    }
                };
                shrunk = smaller(shrunk, lv.shrunk);
                if lv.stopped.is_some() {
                    crumbs = true;
                    #[cfg(test)]
                    CRUMB_STOPS.with(|n| n.set(n.get() + 1));
                }
                let n_left = lv.material_cells();
                (plan, n_helix, n_left)
            }
        };
        if !per_level && once.is_none() {
            once = Some((plan.clone(), n_helix, n_left));
        }
        if !plan.is_empty() {
            cut_levels += 1;
        }
        helices += n_helix;
        left += n_left;
        emit_level(&mut e, &plan, z, slab_top, &k)?;
        slab_top = z;
    }
    let (path, roles, lifts) = e.finish()?;
    // The path's own check: every cutting point stays inside the walls (off the stock outside
    // the regions).
    let wall = k.r + params.stock_to_leave;
    let _check_span = solidcraft_profile::span("cam", "adaptive.walls_check");
    for p in cutting_samples(&path, k.r * 0.25) {
        let q = Vec2::new(p.x, p.y);
        let ok = match &fence {
            Some(f) => f.clear(q, wall - 1e-6),
            None => walls.shape.clear(q, wall - 1e-6),
        };
        if !ok {
            return Err(CamError::Unsafe(format!("the adaptive path comes closer than the tool radius to a wall at ({:.4}, {:.4})", p.x, p.y)));
        }
    }
    let left_area = left as f64 * k.h * k.h;
    let mut warnings = Vec::new();
    if cut_levels == 0 {
        warnings.push("there is no stock to clear in the regions between the operation's top and bottom".to_string());
    }
    if crumbs {
        warnings.push(format!(
            "the walks kept missing the last of the material and stopped: about {left_area:.2} mm² within reach was left (crumbs along the walls and in corners) for the finishing pass or a rest pass to take"
        ));
    } else if left > 0 {
        warnings.push(format!("about {left_area:.2} mm² of material within reach was left (narrow corners the walk did not get into)"));
    }
    if corner_area > 0.0 {
        warnings.push(format!(
            "about {corner_area:.2} mm² (seen from above) is left in corners tighter than the tool's radius {:.3} mm or places narrower than the tool: clear it with a smaller tool after this one",
            k.r
        ));
    }
    warnings.extend(shrunk_warning(&k, shrunk));
    Ok(AdaptivePath { path, roles, levels: cut_levels, helices, lifts, left_area, corner_area, warnings })
}

/// One level of an adaptive roughing planned: its steps (helices starting at the level until
/// set from the levels above), and the cells whose material it cleared.
struct LevelPlan {
    plan: Vec<Step>,
    n_helix: usize,
    n_left: usize,
    shrunk: Option<f64>,
    unentered: bool,
    cleared: Vec<usize>,
}

/// Memory the levels of an adaptive roughing planned at once may take, and about what a level
/// takes per cell of its raster (its distance fields, maps and searches).
const LEVELS_BYTES: usize = 1 << 30;
const LEVEL_BYTES_PER_CELL: usize = 96;

/// The smaller of two helix radii, either of which may be missing.
fn smaller(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, y) => x.or(y),
    }
}

/// What to say of entry helices shrunk to the room they had (`shrunk`: the smallest radius used).
fn shrunk_warning(k: &Knobs, shrunk: Option<f64>) -> Option<String> {
    shrunk.map(|r| {
        format!(
            "the entry helix of diameter {:.3} mm had no room where the material is entered: helices down to {:.3} mm across, the largest that fit, were used",
            2.0 * k.helix_radius,
            2.0 * r
        )
    })
}

/// The stock rectangle of an adaptive roughing.
fn stock_rect(b: &Boundary, surface: &Surface) -> Result<(Vec2, Vec2)> {
    let (lo, hi) = match b.rect {
        Some([a, c]) => (a, c),
        None => {
            let bb = surface.bounds();
            if bb.is_empty() {
                return invalid("the surface has no triangles: give the boundary's rectangle");
            }
            (Vec2::new(bb.min.x, bb.min.y), Vec2::new(bb.max.x, bb.max.y))
        }
    };
    if ![lo.x, lo.y, hi.x, hi.y].iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) {
        return invalid("the boundary's corners must be finite numbers within 1000 m");
    }
    if !(lo.x < hi.x) || !(lo.y < hi.y) {
        return invalid("the boundary's low corner must be below and left of its high corner");
    }
    Ok((lo, hi))
}

/// The heights of the flat, upward-facing areas of the surface over the rectangle.
fn flat_heights(surface: &Surface, lo: Vec2, hi: Vec2) -> Vec<f64> {
    let mut zs: Vec<f64> = Vec::new();
    for (i, t) in surface.triangles().iter().enumerate() {
        let Some(n) = surface.facet_normal(i) else { continue };
        if n.z < 0.9999 {
            continue;
        }
        let (minx, maxx) = (t[0].x.min(t[1].x).min(t[2].x), t[0].x.max(t[1].x).max(t[2].x));
        let (miny, maxy) = (t[0].y.min(t[1].y).min(t[2].y), t[0].y.max(t[1].y).max(t[2].y));
        if maxx < lo.x || minx > hi.x || maxy < lo.y || miny > hi.y {
            continue;
        }
        let z = t[0].z.max(t[1].z).max(t[2].z);
        if !zs.iter().any(|q| (q - z).abs() < 1e-6) {
            zs.push(z);
            if zs.len() > 10_000 {
                break;
            }
        }
    }
    zs
}

/// 3D adaptive roughing: the stock box from `heights.top` down to `heights.bottom` in levels at
/// most a stepdown apart (and at the flats, with `flats`), each cleared with the engagement-limited
/// walk where the dropped cutter (grown by the stock to leave) is at or below the level. The
/// tool enters each level from open air outside the stock where it can, and with a helix where
/// the material is enclosed. Every cutting point is checked with the drop-cutter before the path
/// is returned.
pub fn adaptive_roughing(surface: &Surface, tool: &Tool, p: &AdaptiveRoughingParams) -> Result<AdaptivePath> {
    adaptive_roughing_in(surface, tool, p, None)
}

/// Height apart of the points a column of [`adaptive_roughing_in`]'s `solid` stock is looked at
/// between two levels (mm), and the most points looked at in one column.
const SOLID_STEP: f64 = 0.25;
const MAX_SOLID_SAMPLES: usize = 400;

/// [`adaptive_roughing`] of a stock whose material is known point by point (`solid`, in the
/// surface's coordinates): what an earlier setup left, with what it cut under other material
/// open. Each level clears only where there is material between it and the level above (for a
/// step-up level, between it and how far down the levels so far cleared), not the air that
/// setup already cut. Material thinner than a quarter of a millimetre between two levels may be
/// left.
pub fn adaptive_roughing_in(
    surface: &Surface,
    tool: &Tool,
    p: &AdaptiveRoughingParams,
    solid: Option<&(dyn Fn(Vec3) -> bool + Sync)>,
) -> Result<AdaptivePath> {
    let _span = solidcraft_profile::span("cam", "adaptive_roughing");
    mills(tool, p.adaptive.ramp_angle)?;
    let cutter = tool_cutter(tool)?;
    p.adaptive.validate(tool)?;
    p.heights.validate()?;
    p.feeds.validate()?;
    check_stepdown(p.stepdown)?;
    let k = Knobs::new(tool, &p.adaptive);
    let leave = p.adaptive.stock_to_leave;
    let (slo, shi) = stock_rect(&p.boundary, surface)?;
    let fence: Option<(Ring<Vec2>, Vec<Ring<Vec2>>)> =
        p.boundary.region.as_ref().map(|r| {
            (r.outer.polyline(p.adaptive.tolerance.min(1e-3)), r.holes.iter().map(|h| h.polyline(p.adaptive.tolerance.min(1e-3))).collect())
        });
    let in_stock = |q: Vec2| {
        q.x >= slo.x
            && q.x <= shi.x
            && q.y >= slo.y
            && q.y <= shi.y
            && fence.as_ref().is_none_or(|(o, hs)| o.contains(q) && !hs.iter().any(|h| h.contains(q)))
    };
    let grow = k.r + 2.0 * k.step + 4.0 * k.h;
    let (rlo, rhi) = (slo - Vec2::new(grow, grow), shi + Vec2::new(grow, grow));
    let g = Grid::new(rlo - Vec2::new(k.h, k.h), rhi + Vec2::new(k.h, k.h), k.h)?;
    // Rapids run at the retract height: it must clear the part over the stock and round it.
    let reach = k.r + leave;
    let mut top = f64::NEG_INFINITY;
    for t in surface.triangles() {
        let (minx, maxx) = (t[0].x.min(t[1].x).min(t[2].x), t[0].x.max(t[1].x).max(t[2].x));
        let (miny, maxy) = (t[0].y.min(t[1].y).min(t[2].y), t[0].y.max(t[1].y).max(t[2].y));
        if maxx >= rlo.x - reach && minx <= rhi.x + reach && maxy >= rlo.y - reach && miny <= rhi.y + reach {
            top = top.max(t[0].z.max(t[1].z).max(t[2].z));
        }
    }
    if top.is_finite() && p.heights.retract < top + leave - 1e-9 {
        return invalid(format!(
            "the retract height {} is below the top of the surface ({}, stock to leave included)",
            p.heights.retract,
            top + leave
        ));
    }
    let mut zs = levels(p.heights.top, p.heights.bottom, p.stepdown, false)?;
    if p.flats {
        for f in flat_heights(surface, rlo, rhi) {
            let z = f + leave;
            if z > p.heights.bottom + 1e-6 && z < p.heights.top - 1e-6 && !zs.iter().any(|q| (q - z).abs() < p.adaptive.tolerance.max(1e-3)) {
                zs.push(z);
            }
        }
        zs.sort_by(|a, b| b.total_cmp(a));
    }
    check_flutes(tool, p.heights.top, &zs)?;
    // Each regular level, then its step-up levels from just above it toward the level above
    // (the stock's top for the first), lowest first; `true` marks a step-up level.
    let mut order: Vec<(f64, bool)> = Vec::with_capacity(zs.len());
    for (i, &z) in zs.iter().enumerate() {
        order.push((z, false));
        if let Some(s) = p.step_up {
            if !(s.is_finite() && (1e-2..=MAX_COORD).contains(&s)) {
                return invalid("the step-up must be at least 0.01 mm");
            }
            let above = if i == 0 { p.heights.top } else { zs.get(i - 1).copied().unwrap_or(p.heights.top) };
            let ups = levels(above, z, s, false)?;
            // Up from just above z, without z itself and without the level above.
            order.extend(ups.iter().rev().skip(1).filter(|u| **u < above - 1e-6).map(|u| (*u, true)));
        }
    }
    if order.len() > crate::surface_ops::MAX_LEVELS {
        return Err(CamError::TooLarge(format!("more than {} levels with the step-up levels", crate::surface_ops::MAX_LEVELS)));
    }
    // Grown by the stock to leave all round and by the margin sideways only, so a floor at the
    // stock to leave is a level the centre may stand at.
    let grown = {
        let g = cutter.grown(leave)?;
        let c = Cutter { radius: g.radius + k.margin, ..g };
        c.validate()?;
        c
    };
    if let Some(m) = &p.rest {
        m.validate()?;
    }
    // What earlier cuts left (rest machining): material thinner than this above a level is
    // not chased.
    let left_by = StockModel { rect: None, top: None, heights: p.rest.clone(), rest_tolerance: p.adaptive.tolerance.max(REST_TOLERANCE) };
    // Refused at once where the levels over this raster would take minutes or their walks too
    // many steps, with the smallest load that would plan; each level is then held to a budget
    // of its own.
    let area = (shi.x - slo.x) * (shi.y - slo.y);
    let fits = |kn: &Knobs| {
        let steps = expected_steps(kn, area);
        order.len() as f64 * (g.len() as f64 * TYPICAL_LEVEL_WORK_PER_CELL + steps * kn.step_work()) <= MAX_ROUGHING_WORK && steps <= MAX_STEPS as f64
    };
    check_roughing_work(order.len(), g.len(), expected_steps(&k, area), k.step_work(), &|| load_hint(tool, &p.adaptive, &fits))?;
    let budget = Budget::new();
    // How far down each cell has been cleared by the levels so far (or by earlier cuts).
    let mut floor: Vec<f64> = (0..g.len()).map(|c| left_by.top_over(g.center_of(c), 0.0, p.heights.top)).collect();
    let mut e = Emit::new(p.heights, p.feeds);
    let (mut helices, mut cut_levels, mut left) = (0usize, 0usize, 0usize);
    let mut shrunk: Option<f64> = None;
    // Step-up levels that left material they could not enter from the area already cleared.
    let mut unentered: Vec<f64> = Vec::new();
    // Where the grown cutter rests on the part at each cell's centre inside the rectangle (None
    // outside it), found once for all levels.
    let inside = |q: Vec2| q.x >= rlo.x && q.x <= rhi.x && q.y >= rlo.y && q.y <= rhi.y;
    let at: Vec<Vec2> = (0..g.len()).map(|c| g.center_of(c)).filter(|q| inside(*q)).collect();
    for _ in &at {
        budget.spend()?;
    }
    let mut dropped = solidcraft_profile::scope("cam", "adaptive_roughing.drops", || drops(surface, &grown, &at))?.into_iter();
    let mut rests: Vec<Option<Option<f64>>> = Vec::with_capacity(g.len());
    for c in 0..g.len() {
        rests.push(if inside(g.center_of(c)) { Some(dropped.next().flatten()) } else { None });
    }
    // One level's walks, `floor_now` the floor the levels before it left (a step-up level clears
    // only what they left above it); `None` for a step-up level with nothing to clear. Its
    // helices start where they are written ([`Step::Helix`]'s `from`, set from the floor once
    // the levels above are planned): a regular level's walks do not depend on the levels above,
    // so those are planned side by side, and the step-up levels in order.
    let plan_level = |z: f64, up: bool, floor_now: &[f64]| -> Result<Option<LevelPlan>> {
        let _span = solidcraft_profile::span("cam", if up { "adaptive_roughing.step_up_level" } else { "adaptive_roughing.level" });
        let uncut = |q: Vec2| !up || g.index(q).and_then(|c| floor_now.get(c)).is_some_and(|f| *f > z + 1e-6);
        // Each level's work and walk steps are its own.
        let mut steps_taken = 0usize;
        let mut work = Work::new(0.0, MIN_LEVEL_WORK);
        let domain = Clear { surface, cutter: grown, grown_by: leave, z, lo: rlo, hi: rhi, budget: &budget };
        // As `domain.allowed` at each cell's centre.
        let allow: Vec<bool> = rests.iter().map(|r| r.is_some_and(|zg| zg.is_none_or(|zg| zg + leave <= z + 1e-9))).collect();
        if up && !(0..g.len()).any(|c| allow.get(c).copied().unwrap_or(false) && in_stock(g.center_of(c)) && uncut(g.center_of(c))) {
            return Ok(None);
        }
        // With the stock known point by point: whether there is material between the level and
        // the one above (a regular level: the next regular level up, else the top; a step-up
        // level: how far down the levels so far cleared), at each cell.
        let above = zs.iter().copied().filter(|q| *q > z + 1e-9).fold(p.heights.top, f64::min);
        let filled: Option<Vec<bool>> = match solid {
            None => None,
            Some(holds) => {
                let lo = z + left_by.rest_tolerance;
                let mut v = vec![false; g.len()];
                for (c, f) in v.iter_mut().enumerate() {
                    let q = g.center_of(c);
                    // Every cell of the raster's rectangle: `here` is asked at points off the cells'
                    // centres too, and takes the cell a point lies in.
                    if rests.get(c).is_none_or(|r| r.is_none()) {
                        continue;
                    }
                    let hi = if up { floor_now.get(c).copied().unwrap_or(above).min(above) } else { above };
                    if hi <= lo {
                        continue;
                    }
                    let n = ((hi - lo) / SOLID_STEP).ceil().clamp(1.0, MAX_SOLID_SAMPLES as f64) as usize;
                    *f = (0..=n).any(|i| holds(Vec3::new(q.x, q.y, lo + (hi - lo) * i as f64 / n as f64)));
                }
                Some(v)
            }
        };
        let filled_at = |q: Vec2| filled.as_ref().is_none_or(|v| g.index(q).and_then(|c| v.get(c)).copied().unwrap_or(false));
        let here = |q: Vec2| in_stock(q) && uncut(q) && left_by.material_at(q, z) && filled_at(q);
        let mut lv = Level::new(g, k, &domain, allow, &|_, q| here(q), &here, &mut steps_taken, &mut work, (None, None))?;
        // A step-up level enters only from the area the levels so far cleared: material
        // enclosed above it (a recess whose floor lies between two levels, too narrow for a
        // helix) is left to the finish rather than failing the roughing.
        lv.helix = !up;
        let before = lv.mat.clone();
        let (plan, n_helix) = lv.plan(None, &|_, _| None)?;
        let cleared = before.iter().zip(&lv.mat).enumerate().filter(|(_, (was, now))| **was && !**now).map(|(c, _)| c).collect();
        Ok(Some(LevelPlan { plan, n_helix, n_left: lv.material_cells(), shrunk: lv.shrunk, unentered: lv.unentered, cleared }))
    };
    // The regular levels over the cores, as many at once as their rasters' memory allows. Once
    // one is refused the ones after it are not planned: the refusal is the result.
    let regular: Vec<f64> = order.iter().filter(|(_, up)| !up).map(|(z, _)| *z).collect();
    let refused = std::sync::atomic::AtomicUsize::new(usize::MAX);
    let at_once = crate::par::threads_for(regular.len(), 1).min((LEVELS_BYTES / g.len().saturating_mul(LEVEL_BYTES_PER_CELL).max(1)).max(1));
    let mut planned = crate::par::map_pulled(regular.len(), at_once, |n| {
        use std::sync::atomic::Ordering::Relaxed;
        if refused.load(Relaxed) < n {
            return None;
        }
        let r = plan_level(regular.get(n).copied().unwrap_or(p.heights.bottom), false, &[]);
        if r.is_err() {
            refused.fetch_min(n, Relaxed);
        }
        Some(r)
    })?
    .into_iter();
    for &(z, up) in &order {
        let done = if up {
            plan_level(z, true, &floor)?
        } else {
            match planned.next().flatten() {
                Some(r) => r?,
                None => return Err(CamError::Unsupported("a level of the adaptive roughing was not planned".into())),
            }
        };
        let Some(mut lp) = done else { continue };
        // Each helix starts where the levels above left material over it (or the level).
        for s in &mut lp.plan {
            if let Step::Helix { center, from, radius } = s {
                let (c, rad) = (*center, *radius + k.r);
                let (is, js) = g.span(c, c, rad);
                let mut most = z;
                for j in js {
                    for i in is.clone() {
                        if g.center(i, j).dist(c) <= rad {
                            most = most.max(floor.get(j * g.nx + i).copied().unwrap_or(p.heights.top));
                        }
                    }
                }
                *from = Some(most);
            }
        }
        shrunk = smaller(shrunk, lp.shrunk);
        if lp.unentered {
            unentered.push(z);
        }
        for c in lp.cleared {
            if let Some(f) = floor.get_mut(c) {
                *f = f.min(z);
            }
        }
        if lp.plan.is_empty() {
            continue;
        }
        cut_levels += 1;
        helices += lp.n_helix;
        left += lp.n_left;
        emit_level(&mut e, &lp.plan, z, p.heights.top, &k)?;
    }
    let (path, roles, lifts) = e.finish()?;
    // The path's own check: no cutting point is below where the cutter rests on the surface.
    let rest = cutter.grown(leave)?;
    let samples = cutting_samples(&path, k.r * 0.25);
    // The samples' drop-cutter work is counted first, then the samples are looked at over the
    // cores; the first (in path order) that cuts into the part is the one said.
    for _ in &samples {
        budget.spend()?;
    }
    let ranges = crate::par::split(&vec![1; samples.len()], crate::par::threads_for(samples.len(), DROPS_PER_THREAD));
    let gouges: Vec<Option<Vec3>> = crate::par::map(ranges, |r| {
        crate::stop::check()?;
        let below = |q: &Vec3| drop_z_blocks(surface, &rest, q.x, q.y, q.z - leave, |zg| zg + leave > q.z + 1e-6);
        Ok(samples.get(r).unwrap_or(&[]).iter().find(|q| below(q)).copied())
    })?;
    if let Some(q) = gouges.into_iter().flatten().next() {
        return Err(CamError::Unsafe(format!("the adaptive path cuts into the part at ({:.4}, {:.4}, {:.4})", q.x, q.y, q.z)));
    }
    let left_area = left as f64 * k.h * k.h;
    let mut warnings = Vec::new();
    if p.boundary.rect.is_none() {
        // The tool goes down outside the stock as open air: a blank larger than the part would be
        // cut into from the side at the level's full depth.
        warnings.push(
            "the stock was taken as the part's extent seen from above, and the tool goes down to each level just outside it: if the blank is larger, give the boundary's rectangle as the blank's extent"
                .into(),
        );
    }
    if left > 0 {
        warnings
            .push(format!("about {left_area:.2} mm² of material within reach was left over the levels (narrow corners the walk did not get into)"));
    }
    if !unentered.is_empty() {
        let at: Vec<String> = unentered.iter().map(|z| format!("{z:.3}")).collect();
        warnings.push(format!(
            "step-up levels at Z {} left material they could not enter from the area already cleared (a step-up level bores no helix of its own): the finish takes it",
            at.join(", ")
        ));
    }
    warnings.extend(shrunk_warning(&k, shrunk));
    Ok(AdaptivePath { path, roles, levels: cut_levels, helices, lifts, left_area, corner_area: 0.0, warnings })
}
