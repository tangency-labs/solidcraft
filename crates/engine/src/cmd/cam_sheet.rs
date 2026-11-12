//! The setup sheet of `cam.job` (docs/cam-jobs.md, "Setup sheet"): what the machine operator
//! needs to set the job up, built from the planned operations, the written program and the
//! checks, and the reply's `summary` with the same tables in compact form. The sheet itself is
//! the CAM crate's [`SetupSheet`]; this fills it in. Nothing on it depends on the date or on
//! where it is made, so the same job gives the same bytes.

use std::path::Path;

use solidcore_record::Vec3;
use solidcraft_cam::gcode5::{JointWords, MachineOperation, Post5, parse5};
use solidcraft_cam::kinematics::{Carrier, Joints, Machine};
use solidcraft_cam::setup_sheet::{
    ProgramExtents, SetupSheet, SheetCheck, SheetFormat, SheetOperation, SheetTool, Totals, fmt_num, indexed_reach, program_reach, reach,
    rotary_note, rotary_words, work_zero,
};
use solidcraft_cam::work_offset_code;
use solidcraft_geom::prelude::*;

use super::{Planned, num, same_file};
use crate::params::bad;
use crate::schema::cam::ReadbackReport;
use crate::schema::cam_sheet::{SetupSummary, SummaryOperation, SummaryTool, SummaryTotals};
use crate::schema::cam_sim::CamSimulateReply;
use crate::{Result, Session};

/// Largest angle a rotary turns to before the sheet warns (degrees): a full turn either way.
const FULL_TURN_DEG: f64 = 360.0;

/// The format of the sheet at `path`, refused before anything is computed when it is neither
/// HTML nor Markdown.
pub(super) fn format_of(cmd: &str, path: Option<&str>) -> Result<Option<SheetFormat>> {
    match path {
        None => Ok(None),
        Some(p) => SheetFormat::from_path(p.trim()).map(Some).map_err(|_| bad(cmd, format!("`setup_sheet` {p}: give a .html or .md file"))),
    }
}

/// Refuses a setup sheet written over the G-code or the preview.
pub(super) fn distinct(cmd: &str, output: Option<&str>, preview: Option<&str>, sheet: Option<&str>) -> Result<()> {
    let Some(sheet) = sheet else { return Ok(()) };
    let same = |other: Option<&str>| other.is_some_and(|o| same_file(o, sheet));
    if same(output) || same(preview) {
        return Err(bad(cmd, "`setup_sheet` must be a file of its own, not the `output` or the `preview`"));
    }
    Ok(())
}

/// What the sheet is made from.
pub(super) struct Inputs<'a> {
    pub name: &'a str,
    pub machine: &'a Machine,
    pub post_name: &'a str,
    pub post: &'a Post5,
    pub work_offset: Option<u8>,
    /// The design point that is work zero.
    pub origin: Vec3,
    /// The part's lowest and highest corner in work coordinates, from its exact bounds.
    pub part: (Vec3, Vec3),
    pub planned: &'a [Planned],
    /// The program as written.
    pub text: &'a str,
    pub readback: &'a ReadbackReport,
    pub failures: &'a [String],
    pub notes: &'a [String],
    /// The G-code file, when one is written.
    pub program_file: Option<&'a str>,
    /// The machine's spindle-speed and feed limits the job runs into.
    pub attention: Vec<String>,
    /// The stock-removal simulation, when it ran.
    pub simulation: Option<&'a CamSimulateReply>,
    /// What the machine does at a tool change and what that asks of the operator.
    pub tool_change: Option<String>,
    /// With one program per tool: each, in words, in the order to run them.
    pub programs: Vec<String>,
    /// With one program per tool: each one's file name, in the order to run them.
    pub program_files: Vec<String>,
    /// What to look at that does not fail the job.
    pub warnings: &'a [String],
}

/// The part's lowest and highest corner in work coordinates: the union of its bodies' (`names`)
/// exact bounds, each box's corners taken into work coordinates (round them when the part is
/// turned).
pub(super) fn part_extent(s: &Session, names: &[String], place: &super::Placement) -> Result<(Vec3, Vec3)> {
    let st = s.world_state()?;
    let (mut lo, mut hi) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    for b in st.bodies.iter().filter(|b| names.contains(&b.name)) {
        let bounds = b.body.exact_bounds()?;
        for k in 0..8 {
            let c = Vec3::new(
                if k & 1 == 0 { bounds.min.x } else { bounds.max.x },
                if k & 2 == 0 { bounds.min.y } else { bounds.max.y },
                if k & 4 == 0 { bounds.min.z } else { bounds.max.z },
            );
            let w = place.in_work(c);
            lo = lo.min(w);
            hi = hi.max(w);
        }
    }
    if !(lo.is_finite() && hi.is_finite()) {
        return Ok((Vec3::ZERO, Vec3::ZERO));
    }
    Ok((lo, hi))
}

/// The letter of the machine's one rotary when it is a table about work X through work zero
/// (a 4th axis set up the usual way), whose centreline Y0 Z0 is then on.
fn rotary_about_x(machine: &Machine) -> Option<char> {
    let [axis] = machine.rotaries.as_slice() else { return None };
    let d = axis.direction.normalized()?;
    let on_x = (d.x.abs() - 1.0).abs() <= 1e-6;
    let through = axis.center.y.abs() <= 1e-6 && axis.center.z.abs() <= 1e-6 && machine.work_origin.len() <= 1e-6;
    (axis.carrier == Carrier::Table && on_x && through).then(|| axis.letter.char())
}

/// The work offset the program runs in, in words.
fn work_offset_words(post: &Post5, work_offset: Option<u8>) -> String {
    if !post.base.writes_work_offset() {
        return "not written by the post: the program runs in whichever work offset the controller has active".into();
    }
    work_offset_code(work_offset.unwrap_or(1)).unwrap_or_else(|_| "G54".into())
}

/// The lowest and highest position of every axis the written program reaches, as the reader
/// reads it; `None` when it cannot read the program or it has no motion.
fn extents(text: &str, machine: &Machine) -> Option<ProgramExtents> {
    let letters = machine.letters();
    let blocks = parse5(text, &letters).ok()?;
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut ranges: Vec<(f64, f64)> = vec![(f64::INFINITY, f64::NEG_INFINITY); letters.len()];
    for b in &blocks {
        let (linear, rotary) = match b {
            JointWords::Rapid { linear, rotary, .. }
            | JointWords::Feed { linear, rotary, .. }
            | JointWords::Arc { linear, rotary, .. }
            | JointWords::PlaneArc { linear, rotary, .. } => (*linear, rotary),
            JointWords::Dwell { .. } => continue,
        };
        lo = lo.min(linear);
        hi = hi.max(linear);
        for (range, a) in ranges.iter_mut().zip(rotary) {
            range.0 = range.0.min(*a);
            range.1 = range.1.max(*a);
        }
    }
    if !(lo.is_finite() && hi.is_finite()) {
        return None;
    }
    let rotary =
        letters.iter().zip(&ranges).filter(|(_, r)| r.0 <= r.1).map(|(l, r)| (l.char().to_string(), r.0.to_degrees(), r.1.to_degrees())).collect();
    Some(ProgramExtents { min: [lo.x, lo.y, lo.z], max: [hi.x, hi.y, hi.z], rotary })
}

/// The setup sheet of a planned and checked job.
pub(super) fn build(i: &Inputs<'_>) -> SetupSheet {
    let machine = i.machine;
    let letters: Vec<String> = machine.letters().iter().map(|l| l.char().to_string()).collect();
    let mut warnings: Vec<String> = i.failures.iter().map(|f| format!("Failed check: {f}")).collect();
    warnings.extend(i.warnings.iter().cloned());

    // The operations in order, each with how low its tip goes and where the rotaries are: as
    // the written program has them, or where its operations cannot be told apart in it, as
    // the toolpaths give them.
    let named: Vec<(&str, &solidcraft_cam::Tool)> = i
        .planned
        .iter()
        .map(|p| match &p.op {
            MachineOperation::Indexed(o) => (o.name.as_str(), &o.tool),
            MachineOperation::Continuous(o) => (o.name.as_str(), &o.tool),
        })
        .collect();
    let written = program_reach(i.text, i.post, machine, &named).unwrap_or_else(|e| {
        warnings.push(format!("the written program could not be read for how low each operation reaches ({e})"));
        None
    });
    let mut operations: Vec<SheetOperation> = Vec::with_capacity(i.planned.len());
    let mut previous: Option<Joints> = None;
    for (k, p) in i.planned.iter().enumerate() {
        let (rpm, coolant) = match &p.op {
            MachineOperation::Indexed(o) => (o.spindle_rpm, o.coolant),
            MachineOperation::Continuous(o) => (o.spindle_rpm, o.coolant),
        };
        let reached = match &written {
            Some(w) => Ok(w.get(k).cloned().flatten()),
            None => match &p.op {
                MachineOperation::Indexed(o) => indexed_reach(machine, o),
                MachineOperation::Continuous(_) => reach(machine, &p.poses, previous.as_ref()),
            },
        };
        let (z_min, rotary) = match reached {
            Ok(Some(r)) => {
                let words = rotary_words(&letters, &r.rotary);
                if r.last.is_some() {
                    previous = r.last;
                }
                (Some(r.z_min), words)
            }
            Ok(None) => (None, String::new()),
            Err(e) => {
                warnings.push(format!("operation `{}`: how low it reaches could not be worked out ({e})", p.report.name));
                (None, String::new())
            }
        };
        for w in &p.report.warnings {
            warnings.push(format!("operation `{}`: {w}", p.report.name));
        }
        operations.push(SheetOperation {
            name: p.report.name.clone(),
            kind: p.report.kind.clone(),
            tool: p.tool.number,
            spindle_rpm: rpm,
            feed: p.feeds.cut,
            plunge_feed: p.feeds.plunge,
            coolant,
            rotary,
            cut_length: p.report.cut_length,
            cut_minutes: p.report.cut_minutes,
            z_min,
            gouge_checked: false,
        });
    }

    // The tools the operations use, by number.
    let mut tools: Vec<SheetTool> = Vec::new();
    for (p, o) in i.planned.iter().zip(&operations) {
        let t = &p.tool;
        let row = match tools.iter_mut().position(|r| r.number == t.number) {
            Some(k) => tools.get_mut(k),
            None => {
                tools.push(SheetTool {
                    number: t.number,
                    kind: t.kind.label().into(),
                    description: t.description.clone(),
                    diameter: t.diameter,
                    corner_radius: t
                        .corner_radius
                        .filter(|_| matches!(t.kind, solidcraft_cam::ToolKind::BullNose | solidcraft_cam::ToolKind::FaceMill)),
                    flutes: t.flutes,
                    flute_length: t.flute_length,
                    shank_diameter: t.shank_diameter,
                    stickout: t.stickout,
                    holder: t.holder.iter().map(|h| [h.diameter, h.length]).collect(),
                    z_min: None,
                    operations: Vec::new(),
                });
                tools.last_mut()
            }
        };
        if let Some(row) = row {
            row.operations.push(o.name.clone());
            row.z_min = match (row.z_min, o.z_min) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
    }
    tools.sort_by_key(|t| t.number);
    let tool_changes = i.planned.iter().map(|p| p.tool.number).fold((0usize, None), |(n, last), t| (n + usize::from(last != Some(t)), Some(t))).0;

    // The program as written.
    let extents = extents(i.text, machine);
    match &extents {
        None => warnings.push("the written program could not be read for its extents".into()),
        Some(x) => {
            for (l, lo, hi) in &x.rotary {
                if lo.abs() > FULL_TURN_DEG + 1e-6 || hi.abs() > FULL_TURN_DEG + 1e-6 {
                    warnings.push(format!(
                        "the program turns {l} past a full turn (to {}°): check that the controller's {l} axis allows it",
                        fmt_num(if hi.abs() > lo.abs() { *hi } else { *lo }, 3)
                    ));
                }
            }
        }
    }

    // Work zero on the part.
    let (lo, hi) = i.part;
    let zero = work_zero(i.origin, lo, hi, rotary_about_x(machine), None);

    let rb = i.readback;
    let mut checks = vec![SheetCheck {
        name: "Read-back".into(),
        passed: rb.pass,
        detail: format!(
            "the written program, read back through the machine's kinematics, is within {} mm and {}° of its toolpaths (the post's precision allows {} mm and {}°)",
            num(rb.max_tip_deviation),
            num(rb.max_axis_deviation_deg),
            num(rb.allowed_tip),
            num(rb.allowed_axis_deg)
        ),
    }];

    if let Some(sim) = i.simulation {
        let unfinished = sim.verdict == super::sim::PART_NOT_FINISHED;
        checks.push(SheetCheck {
            name: "Simulation".into(),
            passed: sim.passed && !unfinished,
            detail: if sim.passed && unfinished {
                sim.warnings
                    .iter()
                    .find(|w| w.starts_with(super::sim::PART_NOT_FINISHED))
                    .cloned()
                    .unwrap_or_else(|| super::sim::PART_NOT_FINISHED.into())
            } else if sim.passed {
                format!(
                    "the program cut from the stock ({} mm cells): no gouge and no rapid in material; {}",
                    num(sim.resolution),
                    leftover_words(sim.leftover.max, sim.resolution)
                )
            } else {
                sim.failures.join("; ")
            },
        });
    }

    let totals = Totals {
        operations: operations.len(),
        tools: tools.len(),
        tool_changes,
        cut_length: operations.iter().map(|o| o.cut_length).sum(),
        cut_minutes: operations.iter().map(|o| o.cut_minutes).sum(),
        program_minutes: Some(rb.cut_minutes).filter(|m| m.is_finite()),
        program_lines: i.text.lines().count(),
    };
    SetupSheet {
        job: i.name.to_string(),
        machine: machine.name.clone(),
        post: i.post_name.to_string(),
        // Written one program per tool: the header names those, not the file of every tool.
        program_file: if i.program_files.is_empty() {
            i.program_file.and_then(|f| Path::new(f).file_name()).map(|f| f.to_string_lossy().into_owned())
        } else {
            None
        },
        program_files: i.program_files.clone(),
        work_offset: work_offset_words(i.post, i.work_offset),
        work_zero: zero,
        rotaries: machine.rotaries.iter().map(rotary_note).collect(),
        stock: None,
        fixtures: Vec::new(),
        attention: i.attention.clone(),
        tools,
        operations,
        totals,
        extents,
        checks,
        warnings,
        notes: i.notes.to_vec(),
        workholding: None,
        tool_change: i.tool_change.clone(),
        programs: i.programs.clone(),
        picture: None,
        picture_file: None,
        revision: None,
        probing: Vec::new(),
        stops: Vec::new(),
    }
}

/// What the simulation left on the part, in words: within a cell of the simulation is as close
/// as it can tell (a sliver at a wall, not material to worry about).
fn leftover_words(max: f64, cell: f64) -> String {
    if max <= cell {
        format!("at most {} mm left on the part, within one {} mm cell of the simulation", num(max), num(cell))
    } else {
        format!("up to {} mm left on the part", num(max))
    }
}

fn round(v: f64, decimals: i32) -> f64 {
    let k = 10f64.powi(decimals);
    let r = (v * k).round() / k;
    if r == 0.0 { 0.0 } else { r }
}

fn holder_text(t: &SheetTool) -> Option<String> {
    (!t.holder.is_empty()).then(|| t.holder.iter().map(|[d, l]| format!("Ø{} x {}", fmt_num(*d, 3), fmt_num(*l, 3))).collect::<Vec<_>>().join(" + "))
}

/// The reply's compact form of the sheet.
pub(super) fn summary(s: &SetupSheet) -> SetupSummary {
    let z = &s.work_zero;
    SetupSummary {
        work_zero: z.words.clone(),
        part_extent: [z.part_min.map(|v| round(v, 3)), z.part_max.map(|v| round(v, 3))],
        work_offset: s.work_offset.clone(),
        rotary: s.rotaries.iter().flat_map(|r| r.words.iter().cloned()).collect(),
        tools: s
            .tools
            .iter()
            .map(|t| SummaryTool {
                number: t.number,
                kind: t.kind.clone(),
                diameter: t.diameter,
                flute_length: t.flute_length,
                stickout: t.stickout,
                holder: holder_text(t),
                z_min: t.z_min.map(|v| round(v, 3)),
                operations: t.operations.clone(),
            })
            .collect(),
        operations: s
            .operations
            .iter()
            .map(|o| SummaryOperation {
                name: o.name.clone(),
                kind: o.kind.clone(),
                tool: o.tool,
                spindle_rpm: o.spindle_rpm,
                feed: o.feed,
                plunge_feed: o.plunge_feed,
                rotary: (!o.rotary.is_empty()).then(|| o.rotary.clone()),
                cut_length: round(o.cut_length, 1),
                cut_minutes: round(o.cut_minutes, 2),
            })
            .collect(),
        totals: SummaryTotals {
            operations: s.totals.operations,
            tools: s.totals.tools,
            tool_changes: s.totals.tool_changes,
            cut_length: round(s.totals.cut_length, 1),
            cut_minutes: round(s.totals.cut_minutes, 2),
            program_minutes: s.totals.program_minutes.map(|m| round(m, 2)),
        },
        checks: s
            .checks
            .iter()
            .map(|c| match c.detail.strip_prefix(super::sim::PART_NOT_FINISHED).and_then(|d| d.strip_prefix(": ")) {
                // Not a pass and not a failure: a finding.
                Some(d) if !c.passed => format!("{}: {} ({d})", c.name, super::sim::PART_NOT_FINISHED),
                _ => format!("{}: {} ({})", c.name, if c.passed { "passed" } else { "FAILED" }, c.detail),
            })
            .collect(),
        warnings: s.warnings.clone(),
    }
}
