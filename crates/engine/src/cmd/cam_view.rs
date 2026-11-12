//! `cam.view.*`: the CAM simulation view (state in `crate::cam_view`). `cam.view.load` plans
//! the job in memory (as `cam.job` with `dry_run`) or reads a G-code file of it, and loads the
//! program into a playback at its start; `seek` moves the tool (cutting the stock to there, or putting it back from a keyframe),
//! `play` and `pause` say whether a window plays it on, `show` picks what is drawn, `state`
//! reports where it is and what was found so far, `close` drops it. Every command is view state
//! only: no undo step, nothing saved in the design, no dialog.

use std::sync::Arc;

use serde_json::Value;
use solidcraft_cam::gcode5::parse5;
use solidcraft_cam::machine_time::{DwellUnit, MachineRates};
use solidcraft_cam::playback::{Budget, OpsGiven, Playback, PlaybackSetup, Timeline};
use solidcraft_cam::rapids::owners;
use solidcraft_cam::sim::stock_around;
use solidcraft_cam::sim::{SimParams, plan_sim};
use solidcraft_cam::tool_mesh::tool_meshes;

use super::{
    CommandSpec, MAX_GCODE_BYTES, TOLERANCE_RANGE, cam_err, load_job, machine_of, parse, placement, plan_job, post_of, rates, sim, tools_of, v3,
};
use crate::cam_view::{CamView, finding_view};
use crate::params::bad;
use crate::schema::cam_view::{
    CamViewCloseReply, CamViewEmptyParams, CamViewExportStockParams, CamViewExportStockReply, CamViewLoadParams, CamViewLoadReply, CamViewPlayParams,
    CamViewSeekParams, CamViewShowParams, CamViewShowReply, CamViewStateParams, CamViewStateReply, FindingsListing, OpEnd, OpRef, SeekUntil,
    ViewOperation, ViewToolChange,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const LOAD: TypedContract<CamViewLoadParams> = TypedContract::new();
const SEEK: TypedContract<CamViewSeekParams> = TypedContract::new();
const PLAY: TypedContract<CamViewPlayParams> = TypedContract::new();
const EMPTY: TypedContract<CamViewEmptyParams> = TypedContract::new();
const SHOW: TypedContract<CamViewShowParams> = TypedContract::new();
const STATE: TypedContract<CamViewStateParams> = TypedContract::new();
const EXPORT: TypedContract<CamViewExportStockParams> = TypedContract::new();

/// Findings listed in a load reply.
const LISTED: usize = 1000;
/// Cells cut per piece while looking ahead for the next finding.
const LOOK_AHEAD: f64 = 50.0e6;
/// The rapid rate a timeline uses when the job gives no machine rates (mm/min).
const DEFAULT_RAPID: f64 = 5000.0;

pub(crate) static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.view.load", "Simulation View", crate::checked!("cam.view.load", load))
        .noundo()
        .icon("play")
        .contract(&LOAD)
        .output(output_schema_for::<CamViewLoadReply>)
        .params(
            "job: {…} | job_path: .json (as cam.job); program?: G-code file (default: the job planned and written in memory); \
             resolution?: cell mm; run_to_end?: play it all once at load (every finding known), then back to the start; colour_by?: tool \
             | operation → moves, lines, duration_s, accelerated, operations: [{index, name, tool, first_move, end_move, start_s, \
             end_s}], tool_changes: [{move, line, tool}], stock, resolution, cells, findings, notes, warnings. View state only: the view \
             starts at the program's start",
        ),
    CommandSpec::new("cam.view.seek", "Go To in Simulation", crate::checked!("cam.view.seek", seek))
        .noundo()
        .icon("play")
        .doc_only()
        .contract(&SEEK)
        .output(output_schema_for::<CamViewStateReply>)
        .params(
            "exactly one of move: n (to its end) | line: n | op: name | index (+ at?: start | end) | time_s | percent: 0-100 | \
             tool_change: k | finding: k | by: ±n (+ until?: tool_change | op | finding) → the state (as cam.view.state); the stock is \
             cut to there (or put back from a keyframe)",
        ),
    CommandSpec::new("cam.view.play", "Play Simulation", crate::checked!("cam.view.play", play))
        .noundo()
        .icon("play")
        .doc_only()
        .contract(&PLAY)
        .output(output_schema_for::<CamViewStateReply>)
        .params(
            "speed?: × real time (above 0 to 1000) | moves_per_s?; loop? → the state. A window plays it on; headless it is a flag (seek to move)",
        ),
    CommandSpec::new("cam.view.pause", "Pause Simulation", crate::checked!("cam.view.pause", pause))
        .noundo()
        .icon("play")
        .doc_only()
        .contract(&EMPTY)
        .output(output_schema_for::<CamViewStateReply>)
        .params("→ the state"),
    CommandSpec::new("cam.view.show", "Simulation Layers", crate::checked!("cam.view.show", show))
        .noundo()
        .icon("play")
        .doc_only()
        .contract(&SHOW)
        .output(output_schema_for::<CamViewShowReply>)
        .params(
            "any of toolpaths, rapids, remaining_path, stock, part, tool, holder, gouges, collisions, deviation: bool; colour_by?: tool \
             | operation → layers, colour_by",
        ),
    CommandSpec::new("cam.view.state", "Simulation State", crate::checked!("cam.view.state", state))
        .noundo()
        .icon("play")
        .doc_only()
        .contract(&STATE)
        .output(output_schema_for::<CamViewStateReply>)
        .params(
            "findings?: none | summary (default) | all; limit?: 1-10000 (default 100) → move, moves, fraction, line, gcode, kind, op?, \
             tool?, position: {tip, axis, machine: {x, y, z, a_deg?}}, feed?, rpm?, time_s, duration_s, percent, removed_volume, playing, \
             speed, loop, layers, findings_so_far: {errors, warnings, infos, by_code}, findings?, checked_moves, keyframes, frame",
        ),
    CommandSpec::new("cam.view.close", "Close Simulation", crate::checked!("cam.view.close", close))
        .noundo()
        .icon("play")
        .doc_only()
        .contract(&EMPTY)
        .output(output_schema_for::<CamViewCloseReply>)
        .params("→ closed: true"),
    CommandSpec::new("cam.view.export_stock", "Export Simulated Stock", crate::checked!("cam.view.export_stock", export_stock))
        .noundo()
        .icon("play")
        .doc_only()
        .contract(&EXPORT)
        .output(output_schema_for::<CamViewExportStockReply>)
        .params("path: .stl → path, triangles (the stock as cut so far)"),
];

fn to_value<T: serde::Serialize>(v: T) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| EngineError::Other(e.to_string()))
}

fn loaded<'a>(cmd: &str, s: &'a mut Session) -> Result<&'a mut CamView> {
    s.cam_view.as_deref_mut().ok_or_else(|| bad(cmd, "no CAM view is loaded: run cam.view.load"))
}

fn busy() -> EngineError {
    EngineError::Other("the CAM view is busy cutting the stock: ask again".into())
}

/// A whole number from 0 below `n`.
fn index(cmd: &str, v: f64, n: usize, what: &str) -> Result<usize> {
    if !(v.is_finite() && v >= 0.0 && v.fract() == 0.0 && v < n as f64) {
        return Err(bad(cmd, format!("`{what}` must be a whole number from 0 to {}", n.saturating_sub(1))));
    }
    Ok(v as usize)
}

fn read_text(cmd: &str, path: &str, what: &str, max: usize) -> Result<String> {
    let bytes = solidcraft_io::vfs::read(path, max).map_err(|e| bad(cmd, format!("{what} {path}: {e}")))?;
    String::from_utf8(bytes).map_err(|_| bad(cmd, format!("{what} {path}: not UTF-8 text")))
}

/// `cam.view.load`.
fn load(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.view.load";
    let params: CamViewLoadParams = parse(cmd, p, "")?;
    if params.resolution.is_some_and(|r| !(r.is_finite() && (1e-4..=1e4).contains(&r))) {
        return Err(bad(cmd, "`resolution` must be from 0.0001 to 10000 mm"));
    }
    let (job, base) = load_job(cmd, p, params.job.is_some(), params.job_path.as_ref())?;
    let base = base.as_deref();
    let mut notes: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let (_, post) = post_of(cmd, &job, base)?;
    let dwell = DwellUnit::of(&post.base).map_err(|e| cam_err(cmd, "the post", e))?;
    // The program: the job's own, written in memory, or a file of it.
    let (text, machine, tools, part, tol, ops) = match &params.program {
        None => {
            let pj = plan_job(cmd, s, &job, base)?;
            notes.extend(pj.notes.iter().cloned());
            let blocks = parse5(&pj.text, &pj.machine.letters()).map_err(|e| cam_err(cmd, "reading the program back", e))?;
            let names: Vec<String> = pj.planned.iter().map(|q| q.report.name.clone()).collect();
            let ops = owners(&pj.program, &pj.post, &pj.machine, &blocks).map(|o| (names, o));
            if ops.is_none() {
                notes.push("the program's blocks could not be put under their operations: one operation per tool change is shown".into());
            }
            let part = sim::part_surface(cmd, s, &job, &pj.place, pj.tol, &mut notes)?;
            (pj.text, pj.machine, pj.tools, part, pj.tol, ops)
        }
        Some(path) => {
            if path.trim().is_empty() {
                return Err(bad(cmd, "`program` must not be empty"));
            }
            let text = read_text(cmd, path, "program", MAX_GCODE_BYTES)?;
            let tol = job.tolerance.unwrap_or(0.01);
            if !(tol.is_finite() && (TOLERANCE_RANGE.0..=TOLERANCE_RANGE.1).contains(&tol)) {
                return Err(bad(cmd, "the job's `tolerance` must be from 0.0001 to 1 mm"));
            }
            let place = placement(cmd, s, job.origin.unwrap_or([0.0; 3]), job.orientation.as_ref())?;
            let machine = machine_of(cmd, &job, base)?;
            let tools = tools_of(cmd, &job, base)?;
            let part = sim::part_surface(cmd, s, &job, &place, tol, &mut notes)?;
            notes.push("a program file is shown: one operation per tool change".into());
            (text, machine, tools, part, tol, None)
        }
    };
    // Stock taken from the part, as `cam.simulate` takes it.
    let stock = stock_around(&part, &machine).map_err(|e| cam_err(cmd, "the stock", e))?;
    let rates = match rates::rates_of(cmd, &job, base)? {
        Some(r) => r,
        None => {
            notes.push(format!("no machine rates are given: rapids are timed at {DEFAULT_RAPID} mm/min, cuts at their feed"));
            MachineRates { rapid_xyz_mm_min: DEFAULT_RAPID, ..MachineRates::default() }
        }
    };
    let plan = plan_sim(&text, &machine, &tools, &stock, &SimParams { tolerance: tol, resolution: params.resolution })
        .map_err(|e| cam_err(cmd, "simulation", e))?;
    let given = ops.as_ref().map(|(names, owners)| OpsGiven { names, owners });
    let timeline = Timeline::build(&plan, &text, &rates, dwell, given).map_err(|e| cam_err(cmd, "timing the program", e))?;
    sim::program_asks(cmd, &text, rates.spindle_rpm, &mut warnings, &mut notes)?;
    if plan.moves() == 0 {
        warnings.push("no move of the program could be followed (none comes after every axis of the machine is written)".into());
    }
    let tool_meshes = tools.iter().filter_map(|t| tool_meshes(t, 32).ok().map(|m| (t.number, Arc::new(m)))).collect();
    // No cut into the part is meant: every gouge is an error.
    let setup = PlaybackSetup { tools: tools.clone(), model: None, scene: None, deliberate: vec![false; timeline.ops.len().max(1)], hidden: None };
    let mut playback = Playback::new(Arc::new(plan), Arc::new(timeline), Arc::new(part), setup, Budget::default(), true);
    if params.run_to_end.unwrap_or(false) {
        playback.run_to_end().map_err(|e| cam_err(cmd, "simulation", e))?;
    }
    let tl = playback.timeline();
    let plan = playback.plan();
    let reply = CamViewLoadReply {
        moves: plan.moves(),
        lines: text.lines().count(),
        duration_s: tl.duration,
        accelerated: tl.accelerated,
        operations: tl
            .ops
            .iter()
            .enumerate()
            .map(|(k, o)| ViewOperation {
                index: k,
                name: o.name.clone(),
                tool: o.tool,
                first_move: o.first,
                end_move: o.end,
                start_s: o.t0,
                end_s: o.t1,
            })
            .collect(),
        tool_changes: tl.tool_changes.iter().map(|c| ViewToolChange { at_move: c.at_move, line: c.line, tool: c.tool }).collect(),
        stock: sim::stock_report(&stock),
        resolution: plan.cell(),
        cells: plan.cells(),
        findings: playback.findings().events().iter().take(LISTED).map(finding_view).collect(),
        notes,
        warnings,
        param_warnings: Vec::new(),
    };
    let mut view = CamView::new(playback, Arc::from(text.as_str()), tool_meshes, stock, v3(job.origin.unwrap_or([0.0; 3])));
    view.colour_by = params.colour_by.unwrap_or_default();
    s.cam_view = Some(Box::new(view));
    to_value(reply)
}

/// The place a seek asks for: a move and the fraction of it.
fn target(cmd: &str, p: &mut Playback, q: &CamViewSeekParams) -> Result<(usize, f64)> {
    let given = [
        q.at_move.is_some(),
        q.line.is_some(),
        q.op.is_some(),
        q.time_s.is_some(),
        q.percent.is_some(),
        q.tool_change.is_some(),
        q.finding.is_some(),
        q.by.is_some(),
    ];
    if given.iter().filter(|g| **g).count() != 1 {
        return Err(bad(cmd, "give exactly one of `move`, `line`, `op`, `time_s`, `percent`, `tool_change`, `finding` and `by`"));
    }
    if q.at.is_some() && q.op.is_none() {
        return Err(bad(cmd, "`at` goes with `op`"));
    }
    if q.until.is_some() && q.by.is_none() {
        return Err(bad(cmd, "`until` goes with `by`"));
    }
    let n = p.plan().moves();
    let end = if n == 0 { (0, 0.0) } else { (n - 1, 1.0) };
    let tl = p.timeline();
    if let Some(m) = q.at_move {
        return Ok((index(cmd, m, n, "move")?, 1.0));
    }
    if let Some(l) = q.line {
        if !(l.is_finite() && l >= 1.0 && l.fract() == 0.0 && l <= 1.0e9) {
            return Err(bad(cmd, "`line` must be a whole number from 1"));
        }
        return Ok(tl.at_line(l as usize).map_or((0, 0.0), |m| (m, 1.0)));
    }
    if let Some(op) = &q.op {
        let k = match op {
            OpRef::Index(i) => index(cmd, *i, tl.ops.len(), "op")?,
            OpRef::Name(name) => tl.ops.iter().position(|o| &o.name == name).ok_or_else(|| {
                let names: Vec<&str> = tl.ops.iter().map(|o| o.name.as_str()).take(20).collect();
                bad(cmd, format!("no operation `{name}` (there are: {})", names.join(", ")))
            })?,
        };
        let Some(o) = tl.ops.get(k) else { return Err(bad(cmd, "no such operation")) };
        return Ok(if q.at == Some(OpEnd::End) { (o.end.saturating_sub(1), 1.0) } else { (o.first, 0.0) });
    }
    if let Some(t) = q.time_s {
        if !(t.is_finite() && t >= 0.0) {
            return Err(bad(cmd, "`time_s` must be 0 or more seconds"));
        }
        return Ok(tl.at_time(t.min(tl.duration)));
    }
    if let Some(pc) = q.percent {
        if !(pc.is_finite() && (0.0..=100.0).contains(&pc)) {
            return Err(bad(cmd, "`percent` must be from 0 to 100"));
        }
        return Ok(if pc >= 100.0 { end } else { tl.at_time(tl.duration * pc / 100.0) });
    }
    if let Some(k) = q.tool_change {
        let k = index(cmd, k, tl.tool_changes.len(), "tool_change")?;
        return Ok(tl.tool_changes.get(k).map_or((0, 0.0), |c| (c.at_move, 0.0)));
    }
    if let Some(k) = q.finding {
        let known = p.findings().len();
        if known == 0 {
            return Err(bad(cmd, "no findings are known yet: seek further, `by` with `until: finding`, or load with `run_to_end`"));
        }
        let k = index(cmd, k, known, "finding")?;
        return Ok(p.findings().events().get(k).and_then(|e| e.at_move).map_or((0, 0.0), |m| (m, 1.0)));
    }
    let Some(by) = q.by else { return Ok(p.at()) };
    if !(by.is_finite() && by.fract() == 0.0 && by.abs() <= n as f64) {
        return Err(bad(cmd, format!("`by` must be a whole number of moves, at most {n} either way")));
    }
    let (cur, f) = p.at();
    let forward = by >= 0.0;
    Ok(match q.until {
        None => {
            let m = (cur as f64 + by).clamp(0.0, n.saturating_sub(1) as f64) as usize;
            (m, 1.0)
        }
        Some(SeekUntil::ToolChange) | Some(SeekUntil::Op) => {
            let starts: Vec<usize> = if q.until == Some(SeekUntil::Op) {
                tl.ops.iter().map(|o| o.first).collect()
            } else {
                tl.tool_changes.iter().map(|c| c.at_move).collect()
            };
            if forward {
                starts.iter().copied().find(|&m| m > cur).map_or(end, |m| (m, 0.0))
            } else {
                starts.iter().copied().rev().find(|&m| m < cur || (m == cur && f > 0.0)).map_or((0, 0.0), |m| (m, 0.0))
            }
        }
        Some(SeekUntil::Finding) => {
            if forward {
                // Cut on until a finding after this move turns up, or the end.
                loop {
                    if let Some(m) = p.next_finding_after(cur).and_then(|e| e.at_move) {
                        break (m, 1.0);
                    }
                    if p.checked() >= n {
                        break end;
                    }
                    let r = p.seek(usize::MAX, 1.0, Some(LOOK_AHEAD)).map_err(|e| cam_err(cmd, "seeking", e))?;
                    if r.done && p.next_finding_after(cur).is_none() {
                        break end;
                    }
                }
            } else {
                p.findings().events().iter().rev().filter_map(|e| e.at_move).find(|&m| m < cur).map_or((0, 0.0), |m| (m, 1.0))
            }
        }
    })
}

/// `cam.view.seek`.
fn seek(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.view.seek";
    let q: CamViewSeekParams = parse(cmd, p, "")?;
    let v = loaded(cmd, s)?;
    let pb = v.playback_mut().ok_or_else(busy)?;
    let (m, f) = target(cmd, pb, &q)?;
    pb.seek(m, f, None).map_err(|e| cam_err(cmd, "seeking", e))?;
    v.bump();
    to_value(v.state_reply(FindingsListing::Summary, 100)?)
}

/// `cam.view.play`.
fn play(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.view.play";
    let q: CamViewPlayParams = parse(cmd, p, "")?;
    if q.speed.is_some_and(|x| !(x.is_finite() && x > 0.0 && x <= 1000.0)) {
        return Err(bad(cmd, "`speed` must be above 0 and at most 1000 (times real time)"));
    }
    if q.moves_per_s.is_some_and(|x| !(x.is_finite() && x > 0.0 && x <= 1.0e6)) {
        return Err(bad(cmd, "`moves_per_s` must be above 0 and at most 1000000"));
    }
    if q.speed.is_some() && q.moves_per_s.is_some() {
        return Err(bad(cmd, "give `speed` or `moves_per_s`, not both"));
    }
    let v = loaded(cmd, s)?;
    if let Some(x) = q.speed {
        v.speed = x;
        v.moves_per_s = None;
    }
    if q.moves_per_s.is_some() {
        v.moves_per_s = q.moves_per_s;
    }
    if let Some(l) = q.looping {
        v.looping = l;
    }
    v.playing = true;
    v.bump();
    to_value(v.state_reply(FindingsListing::Summary, 100)?)
}

/// `cam.view.pause`.
fn pause(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.view.pause";
    let _: CamViewEmptyParams = parse(cmd, p, "")?;
    let v = loaded(cmd, s)?;
    v.playing = false;
    v.bump();
    to_value(v.state_reply(FindingsListing::Summary, 100)?)
}

/// `cam.view.show`.
fn show(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.view.show";
    let q: CamViewShowParams = parse(cmd, p, "")?;
    let v = loaded(cmd, s)?;
    let l = &mut v.layers;
    for (slot, value) in [
        (&mut l.toolpaths, q.toolpaths),
        (&mut l.rapids, q.rapids),
        (&mut l.remaining_path, q.remaining_path),
        (&mut l.stock, q.stock),
        (&mut l.part, q.part),
        (&mut l.tool, q.tool),
        (&mut l.holder, q.holder),
        (&mut l.gouges, q.gouges),
        (&mut l.collisions, q.collisions),
        (&mut l.deviation, q.deviation),
    ] {
        if let Some(x) = value {
            *slot = x;
        }
    }
    if let Some(c) = q.colour_by {
        v.colour_by = c;
    }
    v.bump();
    to_value(CamViewShowReply { layers: v.layers, colour_by: v.colour_by })
}

/// `cam.view.state`.
fn state(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.view.state";
    let q: CamViewStateParams = parse(cmd, p, "")?;
    let limit = match q.limit {
        None => 100,
        Some(l) if l.is_finite() && l.fract() == 0.0 && (1.0..=10_000.0).contains(&l) => l as usize,
        Some(_) => return Err(bad(cmd, "`limit` must be a whole number from 1 to 10000")),
    };
    let v = loaded(cmd, s)?;
    to_value(v.state_reply(q.findings.unwrap_or_default(), limit)?)
}

/// `cam.view.close`.
fn close(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.view.close";
    let _: CamViewEmptyParams = parse(cmd, p, "")?;
    let closed = s.cam_view.take().is_some();
    to_value(CamViewCloseReply { closed })
}

/// `cam.view.export_stock`.
fn export_stock(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.view.export_stock";
    let q: CamViewExportStockParams = parse(cmd, p, "")?;
    if q.path.trim().is_empty() {
        return Err(bad(cmd, "`path` must not be empty"));
    }
    solidcraft_io::vfs::check(&q.path).map_err(|e| EngineError::Other(e.to_string()))?;
    let v = loaded(cmd, s)?;
    let pb = v.playback().ok_or_else(busy)?;
    let mesh = pb.state().grid(pb.plan()).mesh(1024);
    let triangles = mesh.triangles.len();
    super::write_file(&q.path, &solidcraft_io::stl_binary(&[mesh], "stock")).map_err(|e| EngineError::Other(format!("{}: {e}", q.path)))?;
    to_value(CamViewExportStockReply { path: q.path, triangles })
}
