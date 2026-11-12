//! `cam.simulate`, and `cam.job`'s `simulate` option: a written program run through a model of
//! the stock (`solidcraft_cam::sim`) as an independent check of the G-code, and what it leaves
//! held against the part. The program is parsed as the controller would read it; the job gives
//! the machine, the tools, the part's bodies and the tolerance.
//!
//! The simulation starts from stock taken from the part: its box on a 3-axis machine, the
//! cylinder about work X through its farthest point from the axis on a 4th axis.

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_cam::kinematics::Machine;
use solidcraft_cam::sim::{RESOLUTION_HINT, SimParams, SimReport, StockShape, simulate as sweep, stock_around};
use solidcraft_cam::{Surface, Tool};

use super::{MAX_GCODE_BYTES, TOLERANCE_RANGE, cam_err, finite_point, load_job, machine_of, num, parse, tools_of, v3};
use crate::params::bad;
use crate::schema::TypedContract;
use crate::schema::cam::CamJob;
use crate::schema::cam_sim::{CamSimulateParams, CamSimulateReply, LeftoverReport, PartLeftReport, RapidInMaterial, SimGougeReport, SimStockReport};
use crate::{EngineError, Result, Session};

pub(super) const CONTRACT: TypedContract<CamSimulateParams> = TypedContract::new();

/// Most points along a side of the stock's preview mesh.
const STL_SIDE: usize = 1024;
/// Rapids in material listed in a reply.
const LISTED: usize = 100;
/// The share of the job's tolerance the part's bodies are tessellated within for the simulation:
/// the facets of a convex surface are inside it and those of a hollow one outside, by up to that.
const CHORD_SHARE: f64 = 0.25;

/// The simulation's verdict when nothing fails but material is left on the part.
pub(super) const PART_NOT_FINISHED: &str = "part not finished";

/// The part's bodies (the job's `bodies`, else every visible body) tessellated from their exact
/// surfaces within a quarter of `tol` as the toolpaths' part is, in work coordinates (`place`): what the
/// simulated stock is held against for gouges and material left on the part. The note says how
/// it was made.
pub(super) fn part_surface(cmd: &str, s: &Session, job: &CamJob, place: &super::Placement, tol: f64, notes: &mut Vec<String>) -> Result<Surface> {
    let (names, _) = super::job_bodies(cmd, s, job)?;
    if names.is_empty() {
        return Err(bad(cmd, "the job has no bodies to hold the program against: the design has no visible body, or give `bodies`"));
    }
    let st = s.world_state()?;
    let chord = tol * CHORD_SHARE;
    let out = super::bodies_mesh(cmd, &st, &names, place, chord, notes)?;
    let surface = Surface::from_mesh(&out).map_err(|e| cam_err(cmd, "the part", e))?;
    if surface.is_empty() {
        return Err(bad(cmd, "the part's bodies have no surface to hold the program against"));
    }
    notes.push(format!(
        "the simulation holds the stock against the bodies tessellated from their exact surfaces at a chord tolerance of {} mm (a quarter of \
         the job's tolerance), in work coordinates (the design less origin [{}, {}, {}])",
        num(chord),
        num(place.origin.x),
        num(place.origin.y),
        num(place.origin.z)
    ));
    Ok(surface)
}

pub(super) fn stock_report(stock: &StockShape) -> SimStockReport {
    match *stock {
        StockShape::Box { min, max } => {
            SimStockReport { kind: "box".into(), min: Some([min.x, min.y, min.z]), max: Some([max.x, max.y, max.z]), ..SimStockReport::default() }
        }
        StockShape::Cylinder { x_min, x_max, radius } => {
            SimStockReport { kind: "cylinder".into(), x_range: Some([x_min, x_max]), radius: Some(radius), ..SimStockReport::default() }
        }
    }
}

/// Simulates `text` for the job's `machine` and `tools` from stock taken from `part`, held
/// against `part` at `tol`: cuts into the part and rapids through material fail, material left
/// on the part beyond the tolerance, a cell and `leave` (the job's most stock to leave) is the
/// verdict [`PART_NOT_FINISHED`]. The reply and the simulation's own report.
#[allow(clippy::too_many_arguments)]
pub(super) fn check_program(
    cmd: &str,
    text: &str,
    machine: &Machine,
    tools: &[Tool],
    part: &Surface,
    tol: f64,
    resolution: Option<f64>,
    leave: f64,
) -> Result<(CamSimulateReply, SimReport)> {
    // `cam.simulate` takes the cell as `resolution`; a job (`cam.job`) as `simulate_resolution`.
    let key = if cmd == "cam.simulate" { "resolution" } else { "simulate_resolution" };
    if resolution.is_some_and(|r| !(1e-4..=1e4).contains(&r)) {
        return Err(bad(cmd, format!("`{key}` must be from 0.0001 to 10000 mm")));
    }
    let stock = stock_around(part, machine).map_err(|e| cam_err(cmd, "the stock", e))?;
    let report = sweep(text, machine, tools, &stock, part, &SimParams { tolerance: tol, resolution }).map_err(|e| cam_err(cmd, "simulation", e))?;
    let mut failures = Vec::new();
    let mut warnings: Vec<String> = report.warnings.iter().map(|w| w.replace(RESOLUTION_HINT, &format!("give `{key}`"))).collect();
    if report.moves == 0 {
        // Nothing checked is not a pass.
        failures.push(
            "simulation: no move of the program could be followed (none comes after every axis of the machine, its rotary included, is \
             written)"
                .to_string(),
        );
    }
    if report.gouge_cells > 0 {
        failures.push(format!(
            "simulation: the program cuts up to {} mm into the part over {} cells of the simulated stock",
            num(report.gouge_worst),
            report.gouge_cells
        ));
    }
    if let Some(first) = report.rapids_in_material.first() {
        failures.push(format!(
            "simulation: {} rapids cut material (a crash on the machine), the first at line {}, {} mm deep",
            report.rapids_in_material.len(),
            first.line,
            num(first.depth)
        ));
    }
    let mut notes = vec![match stock {
        StockShape::Box { .. } => "the simulation starts from the part's box".to_string(),
        StockShape::Cylinder { radius, .. } => {
            format!("the simulation starts from a cylinder of radius {} mm about work X over the part's length", num(radius))
        }
    }];
    notes.push(format!("simulated on {} cells of {} mm", report.cells, num(report.cell)));
    if report.rapids_in_material.len() > LISTED {
        notes.push(format!("the first {LISTED} of {} rapids in material are listed", report.rapids_in_material.len()));
    }
    let (part_left, left_words) = part_left(&report, tol, leave);
    if let Some(w) = &left_words {
        warnings.push(w.clone());
    }
    let unseen = report.part_left.hidden_area;
    if unseen > 0.0 {
        warnings.push(format!(
            "the part has faces under its own material along {} (an undercut, a hole across it) over about {} mm²: the simulation's \
             height map cannot see them, so this setup cannot cut them and what stands there is not checked as cut",
            if matches!(report.stock, solidcraft_cam::sim::StockGrid::Radii { .. }) { "the rays from the A axis" } else { "Z" },
            num(unseen.round())
        ));
    }
    let verdict = if !failures.is_empty() {
        "failed"
    } else if left_words.is_some() {
        PART_NOT_FINISHED
    } else {
        "passed"
    };
    let reply = CamSimulateReply {
        part_left,
        verdict: verdict.to_string(),
        path: String::new(),
        stock: stock_report(&stock),
        resolution: report.cell,
        cells: report.cells,
        gouge: SimGougeReport { worst: report.gouge_worst, cells: report.gouge_cells, at: report.gouge_at.map(|p| [p.x, p.y, p.z]) },
        rapids_in_material: report
            .rapids_in_material
            .iter()
            .take(LISTED)
            .map(|r| RapidInMaterial { line: r.line, volume: r.volume, depth: r.depth })
            .collect(),
        leftover: LeftoverReport {
            max: report.leftover_max,
            mean: report.leftover_mean,
            cells: report.machined_cells,
            at: report.leftover_at.map(|p| [p.x, p.y, p.z]),
        },
        removed_volume: report.removed_volume,
        minutes: report.minutes,
        slot_moves: report.removals.iter().filter(|r| r.slot).count(),
        air_moves: report.air_moves,
        air_length: report.air_length,
        passed: failures.is_empty(),
        failures,
        warnings,
        notes,
        stock_stl: None,
        param_warnings: Vec::new(),
    };
    Ok((reply, report))
}

/// What `report` left on the part over the whole part: material under the part's own material
/// along Z on a height map (a setup from the other side cuts it; not when part material stands
/// both over and under it) and the rest. When any of the rest is thicker than the tolerance, a
/// cell and `leave` (the job's most stock to leave), the finding that says so.
fn part_left(report: &SimReport, tol: f64, leave: f64) -> (PartLeftReport, Option<String>) {
    let l = &report.part_left;
    let heights = matches!(report.stock, solidcraft_cam::sim::StockGrid::Heights { .. });
    let allowed = tol + report.cell + leave.max(0.0);
    let (mut later, mut open) = (0.0, 0.0);
    let mut spots: Vec<(f64, Vec3)> = Vec::new();
    for p in &l.pieces {
        if heights && p.hidden && !p.enclosed {
            later += p.volume;
        } else {
            open += p.volume;
            spots.push((p.thick, p.middle()));
        }
    }
    spots.sort_by(|a, b| b.0.total_cmp(&a.0));
    let over: Vec<(f64, Vec3)> = spots.iter().copied().filter(|s| s.0 > allowed).take(solidcraft_cam::sim::SPOTS).collect();
    let words = over.first().map(|(t, at)| {
        let more: Vec<String> =
            over.iter().skip(1).map(|(t, at)| format!("{} mm at ({}, {}, {})", num(*t), num(at.x), num(at.y), num(at.z))).collect();
        format!(
            "{PART_NOT_FINISHED}: about {} cm³ is left on the part, up to {} mm at ({}, {}, {}){} (held against the whole part, beyond the \
             tolerance, a cell and the stock to leave, {} mm): cut it, or leave it to another setup on purpose",
            num(open / 1000.0),
            num(*t),
            num(at.x),
            num(at.y),
            num(at.z),
            if more.is_empty() { String::new() } else { format!("; also {}", more.join(", ")) },
            num(allowed)
        )
    });
    let reply = PartLeftReport {
        volume: l.volume,
        hidden_volume: l.hidden_volume,
        max: l.max(),
        spots: l.spots.iter().map(|(t, at)| [at.x, at.y, at.z, *t]).collect(),
        hidden_area: l.hidden_area,
        allowed,
        later_volume: later,
        open_volume: open,
        open_max: spots.first().map_or(0.0, |s| s.0),
    };
    (reply, words)
}

/// The most `stock_to_leave` of the job's operations that are turned on (an indexed one's inner
/// operation's), 0 for none.
pub(super) fn most_left(job: &CamJob) -> f64 {
    use crate::schema::cam::OperationSpec as O;
    job.operations
        .iter()
        .filter(|o| super::op_enabled(o))
        .filter_map(|o| match o {
            O::Contour(c) => c.stock_to_leave,
            O::Pocket(c) => c.stock_to_leave,
            O::Parallel(c) => c.stock_to_leave,
            O::Roughing(c) => c.stock_to_leave,
            O::Waterline(c) => c.stock_to_leave,
            O::Scallop(c) => c.stock_to_leave,
            O::SteepShallow(c) => c.stock_to_leave,
            O::Pencil(c) => c.stock_to_leave,
            O::RestFinish(c) => c.stock_to_leave,
            O::RotaryFinish(c) => c.stock_to_leave,
            O::RotaryRoughing(c) => c.stock_to_leave,
            O::WrapPocket(c) => c.stock_to_leave,
            O::WrapContour(c) => c.stock_to_leave,
            O::Bore(c) => c.stock_to_leave,
            O::Adaptive(c) => c.stock_to_leave,
            O::AdaptiveRoughing(c) => c.stock_to_leave,
            O::Indexed(c) => op3_left(&c.operation),
            O::IndexedPattern(c) => op3_left(&c.operation),
            O::Wrap(_) | O::Drill(_) | O::Chamfer(_) | O::ThreadMill(_) | O::Face(_) => None,
        })
        .filter(|l| l.is_finite())
        .fold(0.0, f64::max)
}

/// The `stock_to_leave` of an indexed operation's inner operation.
fn op3_left(op: &crate::schema::cam::Op3Spec) -> Option<f64> {
    use crate::schema::cam::Op3Spec as O;
    match op {
        O::Adaptive(c) => c.stock_to_leave,
        O::AdaptiveRoughing(c) => c.stock_to_leave,
        O::Contour(c) => c.stock_to_leave,
        O::Pocket(c) => c.stock_to_leave,
        O::Parallel(c) => c.stock_to_leave,
        O::Roughing(c) => c.stock_to_leave,
        O::Waterline(c) => c.stock_to_leave,
        O::Scallop(c) => c.stock_to_leave,
        O::SteepShallow(c) => c.stock_to_leave,
        O::Pencil(c) => c.stock_to_leave,
        O::RestFinish(c) => c.stock_to_leave,
        O::Drill(_) | O::Chamfer(_) => None,
    }
}

/// The most programs `cam.simulate` runs after its first (`then`).
const MAX_PROGRAMS_THEN: usize = 200;

/// `cam.simulate`.
pub(super) fn simulate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.simulate";
    let params: CamSimulateParams = parse(cmd, p, "")?;
    let then = params.then.clone().unwrap_or_default();
    if then.len() > MAX_PROGRAMS_THEN {
        return Err(bad(cmd, format!("`then` names at most {MAX_PROGRAMS_THEN} programs")));
    }
    let programs: Vec<&String> = std::iter::once(&params.path).chain(&then).collect();
    if let Some(out) = &params.stock_stl {
        if out.trim().is_empty() {
            return Err(bad(cmd, "a file path must not be empty"));
        }
        if programs.contains(&out) {
            return Err(bad(cmd, "`stock_stl` must not be a G-code file"));
        }
        solidcraft_io::vfs::check(out).map_err(|e| EngineError::Other(e.to_string()))?;
    }
    // The job the program was written for, read as `cam.job` reads it.
    let (job, base) = load_job(cmd, p, params.job.is_some(), params.job_path.as_ref())?;
    let tol = job.tolerance.unwrap_or(0.01);
    if !(tol.is_finite() && (TOLERANCE_RANGE.0..=TOLERANCE_RANGE.1).contains(&tol)) {
        return Err(bad(cmd, "the job's `tolerance` must be from 0.0001 to 1 mm"));
    }
    let origin = job.origin.unwrap_or([0.0; 3]);
    finite_point(&origin, "`origin`").map_err(|e| bad(cmd, e))?;
    let machine = machine_of(cmd, &job, base.as_deref())?;
    let tools = tools_of(cmd, &job, base.as_deref())?;
    // The programs one after another, as the machine runs them on the same stock.
    let mut text = String::new();
    for path in &programs {
        let left = MAX_GCODE_BYTES.saturating_sub(text.len());
        let bytes = solidcraft_io::vfs::read(path, left).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&bytes));
    }
    let mut part_notes = Vec::new();
    let part = part_surface(cmd, s, &job, &super::Placement { origin: v3(origin), axes: None }, tol, &mut part_notes)?;
    let (mut reply, report) = check_program(cmd, &text, &machine, &tools, &part, tol, params.resolution, most_left(&job))?;
    reply.path = params.path.clone();
    let spindle = super::rates::rates_of(cmd, &job, base.as_deref())?.and_then(|r| r.spindle_rpm);
    program_asks(cmd, &text, spindle, &mut reply.warnings, &mut reply.notes)?;
    if !then.is_empty() {
        reply.notes.push(format!(
            "{} programs simulated in order on one stock; line numbers count on through them from {}",
            programs.len(),
            params.path
        ));
    }
    reply.notes.extend(part_notes);
    if let Some(out) = &params.stock_stl {
        export_stock(out, &report)?;
        reply.stock_stl = Some(out.clone());
    }
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// What program `text` asks of the machine beyond its moves, as `cam.job` reports it: a warning
/// for spindle speeds outside the machine's `spindle` range (lowest and highest rpm, when known),
/// which the controller runs at the range's nearest end, and a note of the stops it makes for a
/// tool change (an `M6`, or a programmed stop `M0` on a controller that ignores it), each one the
/// operator's to answer.
fn program_asks(cmd: &str, text: &str, spindle: Option<[f64; 2]>, warnings: &mut Vec<String>, notes: &mut Vec<String>) -> Result<()> {
    if let Some(range) = spindle
        && let Some(w) = solidcraft_cam::machine_time::spindle_range_warning(text, range).map_err(|e| cam_err(cmd, "reading the program", e))?
    {
        warnings.push(w);
    }
    let stops = solidcraft_cam::machine_time::tool_change_lines(text).map_err(|e| cam_err(cmd, "reading the program", e))?;
    if !stops.is_empty() {
        let lines: Vec<String> = stops.iter().take(MAX_STOPS_LISTED).map(usize::to_string).collect();
        let more = if stops.len() > MAX_STOPS_LISTED { ", ..." } else { "" };
        notes.push(format!(
            "the program stops for {} tool changes (an `M6`, or a programmed stop `M0`), at lines {}{more}: the operator puts in the next tool and sets its length at each",
            stops.len(),
            lines.join(", ")
        ));
    }
    Ok(())
}

/// Most tool-change stops listed by line in [`program_asks`]'s note.
const MAX_STOPS_LISTED: usize = 20;

/// The stock the simulation left, written to `out` as a binary STL file (export: the stock's
/// display mesh, never a decision).
fn export_stock(out: &str, report: &SimReport) -> Result<()> {
    let mesh = report.stock.mesh(STL_SIDE);
    super::write_file(out, &solidcraft_io::stl_binary(&[mesh], "stock")).map_err(|e| EngineError::Other(format!("{out}: {e}")))
}
