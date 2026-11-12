//! A job's program as the machine takes it (a child of `cmd/cam.rs`): what the machine does at a
//! tool change (`tool_change`: `auto`, `stop` or `ignored`), the job written one program per
//! tool (`split_by_tool`) for a controller that ignores tool changes, each program complete on
//! its own.

use std::ops::Range;
use std::path::Path;

use solidcraft_cam::gcode5::{MachineProgram, Post5, emit_program};
use solidcraft_cam::kinematics::Machine;
use solidcraft_cam::readback5::readback_program;

use super::{Planned, cam_err, num};
use crate::Result;
use crate::schema::cam::{ProgramPart, ToolChangeMode};

/// One program of a job written one program per tool: what the reply says of it, and its text.
pub(super) struct Written {
    pub part: ProgramPart,
    pub text: String,
}

/// The runs of `planned` (in order) that use one tool each.
pub(super) fn tool_runs(planned: &[Planned]) -> Vec<Range<usize>> {
    let mut runs: Vec<Range<usize>> = Vec::new();
    for (k, p) in planned.iter().enumerate() {
        match runs.last_mut() {
            Some(r) if planned.get(r.start).is_some_and(|q| q.tool.number == p.tool.number) => r.end = k + 1,
            _ => runs.push(k..k + 1),
        }
    }
    runs
}

/// The tools of `planned`, each once, in the order they are first used.
fn tools_used(planned: &[Planned]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for p in planned {
        if !out.contains(&p.tool.number) {
            out.push(p.tool.number);
        }
    }
    out
}

fn tool_list(tools: &[u32]) -> String {
    tools.iter().map(|t| format!("T{t}")).collect::<Vec<_>>().join(", ")
}

/// The warning for a program of several tools on a machine that ignores tool changes, when it is
/// written as one program.
pub(super) fn warning(mode: Option<ToolChangeMode>, planned: &[Planned], split: bool) -> Option<String> {
    let tools = tools_used(planned);
    (mode == Some(ToolChangeMode::Ignored) && tools.len() > 1 && !split).then(|| {
        format!(
            "tool changes: this machine ignores tool changes (`tool_change: ignored`), and the program uses {} tools ({}): it stops \
             (`M0`) at each change after the first for the operator to put in the next tool and set its length. Or write it one program \
             per tool (`split_by_tool: true`) and run the programs in order",
            tools.len(),
            tool_list(&tools)
        )
    })
}

/// What the setup sheet says about tool changes; `None` when the job does not say what the
/// machine does.
pub(super) fn sheet_note(mode: Option<ToolChangeMode>, planned: &[Planned], split: bool) -> Option<String> {
    let tools = tools_used(planned);
    let list = tool_list(&tools);
    Some(match mode? {
        ToolChangeMode::Ignored if split => "This machine ignores tool changes (M6). Run the programs below in order: before each one, put in \
             its tool and set the tool's length offset, then start it."
            .to_string(),
        ToolChangeMode::Ignored if tools.len() > 1 => format!(
            "This machine ignores tool changes (M6), and this program uses {} tools ({list}): it stops (M0) at each change after \
             the first, its message naming the tool. Put in that tool, set its length offset, and press Cycle Start.",
            tools.len()
        ),
        ToolChangeMode::Ignored => format!("This machine ignores tool changes (M6): put in {list} and set its length before you start."),
        ToolChangeMode::Stop => format!(
            "The machine stops at each tool change (M6): put in the tool the program names ({list}, in that order), set its length \
             offset, and press Cycle Start."
        ),
        ToolChangeMode::Auto => format!("The tool changer loads each tool ({list}): check that each is in the pocket its number names."),
    })
}

/// `output` with the program's place and tool before its extension: `part.ngc` → `part-2-T6.ngc`.
fn part_path(output: &str, order: usize, tool: u32, ext: &str) -> String {
    let p = Path::new(output);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "program".into());
    let ext = p.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| ext.to_string());
    p.with_file_name(format!("{stem}-{order}-T{tool}.{ext}")).to_string_lossy().to_string()
}

/// `text` (a program `post` wrote) with `lines` of comment after its first line (the program's
/// name), or first when the program does not start with a comment.
fn with_header(text: &str, post: &Post5, lines: &[String]) -> String {
    let comments: Vec<String> = lines.iter().filter_map(|l| post.base.comment_line(l)).collect();
    if comments.is_empty() {
        return text.to_string();
    }
    let mut it = text.split_inclusive('\n');
    let first = it.next().unwrap_or_default();
    let starts_with_comment = post.base.comment_line("x").and_then(|c| c.chars().next()).is_some_and(|ch| first.starts_with(ch));
    let mut out = String::with_capacity(text.len() + 200);
    if starts_with_comment {
        out.push_str(first);
        for c in &comments {
            out.push_str(c);
            out.push('\n');
        }
    } else {
        for c in &comments {
            out.push_str(c);
            out.push('\n');
        }
        out.push_str(first);
    }
    for rest in it {
        out.push_str(rest);
    }
    out
}

/// The job's program (`program`, its operations planned as `planned`) written one program per
/// run of operations with one tool, each with the safe start and work offset the post writes,
/// the tool change and the length offset on the first Z, and a header naming its tool, the
/// tool's stickout and its place in the order; each read back against its toolpaths. The paths
/// are made from `output` (none in a dry run).
pub(super) fn split(
    cmd: &str,
    program: &MachineProgram,
    planned: &[Planned],
    post: &Post5,
    machine: &Machine,
    output: Option<&str>,
) -> Result<Vec<Written>> {
    let runs = tool_runs(planned);
    let n = runs.len();
    let mut out = Vec::with_capacity(n);
    for (k, run) in runs.iter().enumerate() {
        let order = k + 1;
        let Some(first) = planned.get(run.start) else { continue };
        let tool = &first.tool;
        let ops: Vec<_> = program.operations.get(run.clone()).map(<[_]>::to_vec).unwrap_or_default();
        let names: Vec<String> = planned.get(run.clone()).unwrap_or_default().iter().map(|p| p.report.name.clone()).collect();
        let part = MachineProgram { name: format!("{} {order} of {n}", program.name), operations: ops, work_offset: program.work_offset };
        let text = emit_program(&part, post, machine).map_err(|e| cam_err(cmd, &format!("writing program {order} of {n}"), e))?;
        let rb = readback_program(&text, &part, post, machine).map_err(|e| cam_err(cmd, &format!("reading program {order} of {n} back"), e))?;
        if !rb.within() {
            return Err(crate::EngineError::Other(format!(
                "program {order} of {n} (tool {}) reads back {} mm from its toolpaths: it was not written",
                tool.number,
                num(rb.max_tip_deviation)
            )));
        }
        let stickout = tool.stickout.map_or_else(|| "not given".to_string(), |s| format!("{} mm", num(s)));
        let mut header = vec![
            if order == 1 {
                format!("program 1 of {n} - run it first")
            } else {
                format!("program {order} of {n} - run it after program {}", order - 1)
            },
            format!("this program uses T{} - {} D{} - stickout {stickout}", tool.number, tool.kind.label(), num(tool.diameter)),
            format!("put in T{} and set its length offset H{} before you start", tool.number, tool.number),
        ];
        if !tool.description.trim().is_empty() {
            header.insert(2, tool.description.trim().to_string());
        }
        let text = with_header(&text, post, &header);
        let path = output.map(|o| part_path(o, order, tool.number, &post.base.extension));
        let minutes = planned.get(run.clone()).unwrap_or_default().iter().map(|p| p.report.cut_minutes).sum();
        out.push(Written {
            part: ProgramPart {
                order,
                path,
                tool: tool.number,
                stickout: tool.stickout,
                operations: names,
                lines: text.lines().count(),
                cut_minutes: minutes,
            },
            text,
        });
    }
    Ok(out)
}

/// A program part in words, for the setup sheet.
pub(super) fn part_words(p: &ProgramPart) -> String {
    let file = p.path.as_deref().and_then(|f| Path::new(f).file_name()).map(|f| f.to_string_lossy().to_string());
    format!(
        "{}T{}{}: {} ({} min at feed)",
        file.map_or(String::new(), |f| format!("{f}: ")),
        p.tool,
        p.stickout.map_or(String::new(), |s| format!(", stickout {} mm", num(s))),
        p.operations.join(", "),
        num((p.cut_minutes * 10.0).round() / 10.0)
    )
}
