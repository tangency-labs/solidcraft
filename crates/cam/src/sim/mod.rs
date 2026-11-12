//! Stock-removal simulation: an independent check of a written program. The G-code is read
//! with [`parse5`] as the controller would run it, the tool is swept through a model of the
//! stock along every block, and what is left is held against the part.
//!
//! The stock is a heightfield over the stock box ([`StockShape::Box`]): one top height per
//! cell of a grid, sampled at the cell's centre (a dexel: one column of material per cell).
//! Each block lowers every cell to the tool's lower envelope along it, computed exactly per
//! cell rather than at points along the move: along a straight move the height of the tool
//! above a cell is a convex function of the position along the move, so its least value is
//! found in closed form (flat, ball, drill point) or by bisection on its slope to rounding
//! (bull nose, chamfer mill). A level arc is exact too (the nearest point of the arc to the
//! cell). An arc that also moves in Z (a helix), and an arc in the XZ or YZ plane, are followed
//! as chords at most a quarter of the tolerance from them.
//!
//! The tool is its cutting end with an endless cylinder of its diameter above (a cut deeper
//! than the flutes still removes material). A block before which the position is not known
//! (the program's first, or the first after a return home, `G28`/`G30`/`G53`) is taken as
//! coming straight down the tool axis from above the stock, which is where those moves leave
//! the tool. Rapids sweep a straight line like cuts.
//!
//! The grid is split into bands of rows, one per core up to sixteen, and every band runs the
//! whole program in order on a thread of its own (one thread on the web): a cell is only ever
//! changed by its own band, so the result is the same as on one thread, and what each block
//! removed is the sum over the bands.
//!
//! What is left is compared with the part (a [`Surface`]): each cell's part height is where a
//! ray from above first meets the part. A cell cut below the part by more than the tolerance is
//! measured again in 3D over the material the cut column runs through, so a
//! cut beside a steep wall that stays within the tolerance of the wall is not a gouge, and air
//! down a hole or a pocket is no gouge however far it is from its walls. Leftover is the height
//! of the stock above the part over the cells that were cut, measured in 3D from the part's
//! surface where it could be the most: material left beside a steep wall is as thick as it is
//! there (a sliver a cell's centre falls in is not the wall's height), material on a floor its
//! height above it. Stock left in a hole through the part is leftover from the part's bottom up.
//!
//! [`simulate`] runs in three parts that can also be driven one at a time, for a view that
//! plays the program move by move: [`plan_sim`] reads the program into steps (one per motion
//! block) and the grid; [`SimState`] is the stock part way through, and [`SimState::advance`]
//! cuts it on to a later step, or part of the way through one (lowering a cell is a minimum
//! over the tool's positions, so cutting the first part of a move and then the rest leaves what
//! cutting all of it does, to rounding); [`SimState::finish`] holds what is left against the
//! part. Advancing whole steps in any number of calls leaves the stock bit for bit as one call
//! does. The stock keeps, per tile of [`TILE`] × [`TILE`] cells, an upper bound of its heights
//! for quick rejects and a version that changes whenever a cell of the tile is cut, and per
//! cell the tool that cut it last, for a view. A simulation can start from the stock an earlier
//! setup left ([`SimState::lower_to`], [`SimState::raise_under`], [`SimState::hold_part`]).

use std::cell::Cell;
use std::f64::consts::{PI, TAU};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::Aabb3;
use solidcraft_geom::prelude::*;

use crate::gcode::words;
use crate::gcode5::{JointWords, parse5};
use crate::kinematics::{Joints, Machine, forward};
use crate::multiaxis::Pose;
use crate::path::{ArcPlane, arc_sweep};
use crate::surface::{Surface, nearest_on_triangle};
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

mod compare;
mod heights;
mod params;
mod profile;
mod program;
mod report;
mod state;

use self::compare::*;
use self::heights::*;
use self::params::*;
use self::program::*;
use self::state::*;

pub(crate) use self::compare::column_depth;
#[cfg(test)]
pub(crate) use self::compare::distance_within;
pub use self::heights::Removed;
pub use self::params::{DEFAULT_CELL_TOLERANCES, DEFAULT_CELLS, Grid, MAX_CELLS, SimParams, StockShape, stock_around};
pub(crate) use self::profile::Profile;
pub use self::program::{PartFloor, SimPlan, StepView, plan_sim};
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
/// cell than the simulation allows is simulated at a cell coarse enough to fit, with a warning
/// saying so; the cell used is in the report.
pub fn simulate(text: &str, machine: &Machine, tools: &[Tool], stock: &StockShape, part: &Surface, params: &SimParams) -> Result<SimReport> {
    simulate_with(text, machine, tools, stock, part, params, false).map(|(r, _)| r)
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
    simulate_with(text, machine, tools, stock, part, params, true)
}

fn simulate_with(
    text: &str,
    machine: &Machine,
    tools: &[Tool],
    stock: &StockShape,
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
    let watch = watch && !part.is_empty();
    if watch {
        state.set_floor(&Arc::new(plan.part_floor(part)));
    }
    state.advance(&plan, plan.moves(), 0.0)?;
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
