//! Stock-removal simulation: an independent check of a written program. The G-code is read
//! with [`parse5`] as the controller would run it, the tool is swept through a model of the
//! stock along every block, and what is left is held against the part.
//!
//! Two stock models, both dexels (one column of material per cell of a grid):
//!
//! - **3-axis** ([`StockShape::Box`]): a heightfield over the stock box, one top height per
//!   cell, sampled at the cell's centre. Each block lowers every cell to the tool's lower
//!   envelope along it, computed exactly per cell rather than at points along the move: along
//!   a straight move the height of the tool above a cell is a convex function of the position
//!   along the move, so its least value is found in closed form (flat, ball, drill point) or by
//!   bisection on its slope to rounding (bull nose, chamfer mill). A level arc is exact too (the
//!   nearest point of the arc to the cell). An arc that also moves in Z (a helix), and an arc in
//!   the XZ or YZ plane, are followed as chords at most a quarter of the tolerance from them.
//! - **4th axis** ([`StockShape::Cylinder`], or a [`StockShape::Box`] bar the axis runs
//!   through, on a rotary about work X through work zero): a grid over (x, θ) holding the radius
//!   the material reaches along the ray from the axis at that x and angle (for a box bar, from
//!   where the ray leaves its rectangle). A block that turns the rotary while X moves is a helix
//!   in the work frame, so the tool is placed at points along every block (every axis moving
//!   evenly from the block's start to its end, as written), each close enough to the last that
//!   the cusp the sampling leaves is at most half the tolerance: for a tool with a round corner
//!   of radius r the points are at most √(4·r·tol) apart (the tip's travel, and the travel of
//!   the cutting end as the axis turns), for a flat end mill √(4·R·tol) (the scallop its rim
//!   leaves sideways), for a drill point tol / (its slope). At each point every ray near the
//!   tool is cut over the stretch of it inside the tool (the cutting end and the endless
//!   cylinder above it are convex, so that is one stretch; closed form for flat and ball,
//!   bisection for the others). A cut that reaches out of the material along the ray ends the
//!   ray where the tool enters it; one that crosses the ray inside the material without
//!   reaching out of it (beside the ray, as an indexed flat, a pocket or a hole in the part's
//!   side does to the rays at a slant through it) leaves a stretch of air inside the ray's
//!   material. So a ray is its material from the axis out to its end less the air in it, and
//!   indexed work (3+1: the A axis held while X, Y and Z move) is modelled in the part's own
//!   frame as exactly as turning is.
//!
//! The tool is its cutting end with an endless cylinder of its diameter above (a cut deeper
//! than the flutes still removes material). A block before which the position is not known
//! (the program's first, or the first after a return home, `G28`/`G30`/`G53`) is taken as
//! coming straight down the tool axis from above the stock, which is where those moves leave
//! the tool. Rapids sweep a straight line like cuts.
//!
//! The grid is split into bands of rows (of angles on a 4th axis), one per core up to sixteen,
//! and every band runs the whole program in order on a thread of its own (one thread on the
//! web): a cell is only ever changed by its own band, so the result is the same as on one
//! thread, and what each block removed is the sum over the bands.
//!
//! What is left is compared with the part (a [`Surface`]): each cell's part height (radius) is
//! where a ray from above (from outside) first meets the part. A cell cut below the part by
//! more than the tolerance is measured again in 3D over the material the cut column runs
//! through, so a cut beside a steep wall that stays within the tolerance of the wall is not a
//! gouge, and air down a hole or a pocket is no gouge however far it is from its walls.
//! Leftover is the height (radius) of the stock above the part, along Z (along the ray), over
//! the cells that were cut, measured in 3D from the part's surface where it could be the most:
//! material left beside a steep wall is as thick as it is there (a sliver a cell's centre falls
//! in is not the wall's height), material on a floor its height above it. Stock left in a hole
//! through the part is leftover from the part's bottom up. A ray with air inside
//! its material is held against every stretch of the part along it (found from where it
//! crosses the part's surface): part material in its air is a gouge (measured in 3D down that
//! stretch of air), and each piece of its material outside the part is leftover, measured in
//! 3D from the part's surface (a sliver along a wall the ray runs beside is as thin as it is,
//! however long along the ray).
//!
//! A height map cannot tell air under material from material. For a part with faces under its
//! own material along Z (an undercut, a hole across it), the 3-axis stock is also kept as lines
//! along X and along Y (a tri-dexel stock, [`SimState::track_sides`]), cut by every block as the
//! height map is: what they leave with part material both over and under it along Z (where no
//! setup along Z, from above or turned over, reaches) is left on the part and accounted for by no
//! later setup along Z, and what they cut into the part there is a gouge. The report hands them
//! on ([`SimReport::sides`]) to a next setup turned onto its side. Other parts keep none.
//!
//! What the program left over the whole part, not only where it cut ([`PartLeft`]), is found by
//! holding columns of the stock (every few cells) against all of the part's stretches along
//! them: stock never cut over a pocket, a bore's core on a 4th axis, and what stands under the
//! part's own material are found too.
//!
//! [`simulate`] runs in three parts that can also be driven one at a time, for a view that
//! plays the program move by move: [`plan_sim`] reads the program into steps (one per motion
//! block) and the grid; [`SimState`] is the stock part way through, and [`SimState::advance`]
//! cuts it on to a later step, or part of the way through one (lowering a cell is a minimum
//! over the tool's positions, so cutting the first part of a move and then the rest leaves what
//! cutting all of it does, to rounding); [`SimState::finish`] holds what is left against the
//! part. Advancing whole steps in any number of calls leaves the stock bit for bit as one call
//! does. The stock keeps, per tile of [`TILE`] × [`TILE`] cells, an upper bound of its heights
//! (radii) for quick rejects and a version that changes whenever a cell of the tile is cut, and
//! per cell the tool that cut it last, for a view. [`simulate_from`] starts from the stock an
//! earlier setup left.

use std::cell::Cell;
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb3, Mesh};

use crate::gcode::words;
use crate::gcode5::{JointWords, parse5};
use crate::kinematics::{Joints, Machine, forward};
use crate::multiaxis::Pose;
use crate::path::{ArcPlane, arc_sweep};
use crate::surface::{Surface, nearest_on_triangle};
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

mod compare;
mod dexels;
mod heights;
mod left;
mod params;
mod profile;
mod program;
mod rays;
mod report;
mod state;

use self::compare::*;
use self::dexels::*;
use self::heights::*;
use self::left::*;
use self::params::*;
use self::profile::*;
use self::program::*;
use self::rays::*;
use self::state::*;

pub(crate) use self::compare::column_depth;
#[cfg(test)]
pub(crate) use self::compare::distance_within;
pub use self::dexels::SideLines;
pub use self::heights::Removed;
pub use self::left::{LeftPiece, PartLeft, SPOT_APART, SPOTS};
pub use self::params::{DEFAULT_CELL_TOLERANCES, DEFAULT_CELLS, Footprint, Grid, MAX_CELLS, SimParams, StockShape, stock_around};
pub(crate) use self::profile::Profile;
pub use self::program::{PartFloor, SimPlan, StepView, plan_sim};
pub use self::rays::Gaps;
#[cfg(test)]
pub(crate) use self::rays::column_reach;
pub use self::report::{BlockRemoval, SimReport, StockGrid};
pub use self::state::SimState;

/// Most cell updates one simulation makes: beyond it the simulation is refused.
const MAX_VISITS: u64 = 4_000_000_000;
/// The share of [`MAX_VISITS`] a simulation without a resolution is fitted within.
const FIT_SHARE: f64 = 0.5;
/// Cells along a side of a tile of the stock (its quick-reject maxima and versions).
pub const TILE: usize = 16;
/// The tool slot of a cell no tool has cut.
pub const UNCUT: u8 = 255;
/// About how many columns (lines) a check over the whole part looks at ([`PartLeft`]).
pub const WHOLE_SAMPLES: usize = 60_000;

/// `√(a² + b²)`, without the care (and the cost) of `f64::hypot`.
fn hyp(a: f64, b: f64) -> f64 {
    (a * a + b * b).sqrt()
}

/// `v` as an index from 0 to `n - 1` (rounding down; out of range values clamped).
fn index(v: f64, n: usize) -> usize {
    let top = n.saturating_sub(1) as f64;
    if v.is_nan() { 0 } else { v.floor().clamp(0.0, top) as usize }
}

/// Counts cell updates against [`MAX_VISITS`], over every band.
fn count(visits: &AtomicU64, n: u64) -> Result<()> {
    // Nothing visited: the count every band shares is not touched (the bands a tool position
    // misses would otherwise all write it, each holding up the others).
    if n == 0 {
        return Ok(());
    }
    if visits.fetch_add(n, Ordering::Relaxed).saturating_add(n) > MAX_VISITS {
        return Err(CamError::TooLarge("the program sweeps too much stock to simulate at this resolution: give a coarser resolution".into()));
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// The cell updates this thread's last simulation counted (the tests' measure of its work).
    pub(crate) static SIM_VISITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Simulates `text` (G-code for `machine`, in absolute coordinates) cutting `stock` with
/// `tools` (by number), and compares what is left with `part`. See the module documentation.
///
/// Without a resolution in `params`, a program whose cuts would visit more cells at the default
/// cell than the simulation allows (a long 4th-axis program) is simulated at a cell coarse
/// enough to fit, with a warning saying so; the cell used is in the report.
pub fn simulate(text: &str, machine: &Machine, tools: &[Tool], stock: &StockShape, part: &Surface, params: &SimParams) -> Result<SimReport> {
    simulate_with(text, machine, tools, stock, Start::default(), part, params, false).map(|(r, _)| r)
}

/// [`simulate`] (or [`simulate_watched`] when `watch`) from the stock an earlier setup left:
/// `stock`'s box lowered to the height map `start` (work coordinates) before the program cuts,
/// its material ending below at `under` where that setup stood upside down from this one
/// ([`SimState::raise_under`]), and, with `carried` (the material that setup left at a work
/// point), kept along X and Y too with what that setup cut under other material open
/// ([`SimState::carve_sides`]). Without `carried`, `hidden` says whether the part has faces
/// under its own material along Z, as the caller read them on its exact faces: the stock is
/// then kept along X and Y too ([`SimState::track_sides`]) or not; `None` (no exact faces to
/// read) and the simulation looks down rays through the part's surface for such faces.
pub fn simulate_from(
    text: &str,
    machine: &Machine,
    tools: &[Tool],
    stock: &StockShape,
    start: Option<&crate::adaptive::HeightMap>,
    under: Option<&crate::adaptive::HeightMap>,
    carried: Option<&Carried>,
    hidden: Option<bool>,
    part: &Surface,
    params: &SimParams,
    watch: bool,
) -> Result<(SimReport, Vec<(usize, f64)>)> {
    simulate_with(text, machine, tools, stock, Start { map: start, under, carried, hidden }, part, params, watch)
}

/// Whether an earlier setup left material at a point (in this setup's work coordinates).
pub type Carried = dyn Fn(Vec3) -> bool + Send + Sync;

/// What a simulation starts from besides the stock's box ([`simulate_from`]).
#[derive(Clone, Copy, Default)]
struct Start<'a> {
    map: Option<&'a crate::adaptive::HeightMap>,
    under: Option<&'a crate::adaptive::HeightMap>,
    carried: Option<&'a Carried>,
    /// Whether the part has faces under its own material along Z, read on its exact faces
    /// (`None`: unknown, found from the part's surface instead).
    hidden: Option<bool>,
}

/// [`simulate`], watching every move as it cuts for a cut into `part`: also the program line
/// and the depth (measured in 3D, as the report measures gouges) of every move that cuts into
/// the part deeper than the tolerance, in program order, so that the caller can tell whose cut
/// each is (an operation told to cut what the model does not show, or another).
pub fn simulate_watched(
    text: &str,
    machine: &Machine,
    tools: &[Tool],
    stock: &StockShape,
    part: &Surface,
    params: &SimParams,
) -> Result<(SimReport, Vec<(usize, f64)>)> {
    simulate_with(text, machine, tools, stock, Start::default(), part, params, true)
}

fn simulate_with(
    text: &str,
    machine: &Machine,
    tools: &[Tool],
    stock: &StockShape,
    start: Start,
    part: &Surface,
    params: &SimParams,
    watch: bool,
) -> Result<(SimReport, Vec<(usize, f64)>)> {
    let mut plan = plan_sim(text, machine, tools, stock, params)?;
    let mut coarser = None;
    if params.resolution.is_none() {
        let visits: f64 = (0..plan.moves()).map(|k| plan.step_cost(k)).sum();
        let budget = FIT_SHARE * MAX_VISITS as f64;
        if visits > budget {
            // The cells a tool position visits go as the square of the cell's size.
            let cell = plan.cell() * (visits / budget).sqrt();
            plan = plan_sim(text, machine, tools, stock, &SimParams { resolution: Some(cell), ..*params })?;
            coarser = Some(visits);
        }
    }
    let mut state = SimState::new(&plan);
    let mut floor = None;
    if let Some(m) = start.map {
        state.lower_to(m);
        if let Some(u) = start.under {
            state.raise_under(u);
        }
        if !part.is_empty() {
            let f = Arc::new(plan.part_floor(part));
            state.hold_part(&f);
            floor = Some(f);
        }
    }
    if let Some(carried) = start.carried {
        state.carve_sides(&plan, carried, part);
    } else if start.hidden.unwrap_or_else(|| hides_faces(part, &plan.grid)) {
        state.track_sides(&plan);
    }
    // A stock an earlier setup left is held up to the part along its side lines too.
    if floor.is_some() || start.carried.is_some() {
        state.hold_sides(part);
    }
    let watch = watch && !part.is_empty();
    if watch {
        state.set_floor(&floor.unwrap_or_else(|| Arc::new(plan.part_floor(part))));
    }
    if state.tracks_sides() {
        // In runs of steps, so the side lines are cut no higher than the stock stands round
        // each step a run before (whole steps: the stock is as one advance leaves it).
        let mut k = 0;
        while k < plan.moves() {
            k = (k + SIDE_RUN).min(plan.moves());
            state.advance(&plan, k, 0.0)?;
        }
    } else {
        state.advance(&plan, plan.moves(), 0.0)?;
    }
    #[cfg(test)]
    SIM_VISITS.with(|v| v.set(state.visits.load(Ordering::Relaxed)));
    let mut cuts = Vec::new();
    if watch {
        for (r, step) in state.removed().iter().zip(&plan.steps) {
            if r.gouge > plan.tol {
                let d = column_depth(part, r.gouge_from, r.gouge_at, r.gouge);
                if d > plan.tol {
                    cuts.push((step.line, d));
                }
            }
        }
    }
    let mut report = state.finish(&plan, part);
    if let Some(visits) = coarser {
        report.warnings.push(format!(
            "the program would visit about {visits:.3e} cells at the default resolution, more than the simulation allows: it was simulated \
             at a cell of {:.3} mm instead ({RESOLUTION_HINT} to choose one)",
            plan.cell()
        ));
    }
    Ok((report, cuts))
}

/// How the warning of a simulation run coarser than the default names the key that sets its
/// cell; a command whose key is another says that one in its place.
pub const RESOLUTION_HINT: &str = "give `resolution`";
