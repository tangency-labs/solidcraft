//! Setup sheets: what the machine operator needs to set a job up, written beside its program.
//! The job and its program, where work zero is on the part, the rotary axes' conventions, the
//! tools to load with how far each reaches down, the operations in order with their cutting
//! data and times, the checks the program passed and anything to look out for.
//!
//! A [`SetupSheet`] holds the data; [`SetupSheet::html`] writes it as a self-contained page
//! (no scripts, no network, print styles) and [`SetupSheet::markdown`] as Markdown. Every text
//! taken from the job (names, descriptions, notes) is escaped for the format it is written in,
//! and nothing on the sheet depends on the date or the machine it is made on: the same job
//! gives the same bytes.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::frame::rotate;
use crate::gcode::operation_comment;
use crate::gcode5::{JointWords, Post5, parse5};
use crate::indexed::IndexedOperation;
use crate::kinematics::{Carrier, Joints, Machine, RotaryAxis, inverse};
use crate::multiaxis::Toolpath5;
use crate::{CommentStyle, Move, Post, Result, Tool, invalid};

/// The format a sheet is written in, from its file's extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SheetFormat {
    Html,
    Markdown,
}

impl SheetFormat {
    /// The format of a sheet written to `path`: `.html` or `.htm` for HTML, `.md` or
    /// `.markdown` for Markdown, in any case. Anything else is refused.
    pub fn from_path(path: &str) -> Result<SheetFormat> {
        let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if ext.contains('/') || ext.contains('\\') {
            return invalid("a setup sheet is a .html or .md file");
        }
        match ext.as_str() {
            "html" | "htm" => Ok(SheetFormat::Html),
            "md" | "markdown" => Ok(SheetFormat::Markdown),
            _ => invalid("a setup sheet is a .html or .md file"),
        }
    }
}

/// Where work zero is on the part.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkZero {
    /// The design point that is work zero (design coordinates, mm).
    pub design_point: [f64; 3],
    /// The part's lowest and highest X, Y and Z from work zero (mm).
    pub part_min: [f64; 3],
    pub part_max: [f64; 3],
    /// Farthest the part reaches from a rotary axis along work X (mm), on a 4th axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// The same in words, a sentence each.
    pub words: Vec<String>,
}

/// A rotary axis's convention, in words.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RotaryNote {
    pub letter: String,
    /// A sentence each: what it turns and about which line, which way positive turns, its travel.
    pub words: Vec<String>,
    /// Its lowest and highest angle in the machine settings (degrees); `None`: no limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits_deg: Option<[f64; 2]>,
}

/// A tool to load.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SheetTool {
    pub number: u32,
    /// Its kind in words (`flat end mill`, ...).
    pub kind: String,
    pub description: String,
    pub diameter: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radius: Option<f64>,
    pub flutes: u32,
    pub flute_length: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shank_diameter: Option<f64>,
    /// Tip to holder face (mm), when the tool says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stickout: Option<f64>,
    /// The holder from its face up, as `[diameter, length]` sections (mm); empty: not given.
    pub holder: Vec<[f64; 2]>,
    /// The lowest Z the program sends the tool tip to, in program coordinates (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_min: Option<f64>,
    /// The operations that use it, in order.
    pub operations: Vec<String>,
}

/// One operation, as the program runs it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SheetOperation {
    pub name: String,
    pub kind: String,
    pub tool: u32,
    pub spindle_rpm: f64,
    /// The cutting and plunge feeds (mm/min).
    pub feed: f64,
    pub plunge_feed: f64,
    pub coolant: bool,
    /// The rotary angles it runs at, in words (`A90°`, `A -180° to 180°`); empty on a 3-axis machine.
    pub rotary: String,
    /// Distance and time at feed (mm, minutes).
    pub cut_length: f64,
    pub cut_minutes: f64,
    /// The lowest Z of the tool tip in program coordinates (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_min: Option<f64>,
    /// Whether it was checked for gouges.
    pub gouge_checked: bool,
}

/// The job's totals.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Totals {
    pub operations: usize,
    pub tools: usize,
    /// Tool changes the program makes, the first load included.
    pub tool_changes: usize,
    pub cut_length: f64,
    /// Time at feed as planned (minutes).
    pub cut_minutes: f64,
    /// Time at feed as the controller times the written program (minutes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_minutes: Option<f64>,
    /// Lines of G-code.
    pub program_lines: usize,
}

/// A check the program was put through.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SheetCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// The lowest and highest position each axis of the written program reaches.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProgramExtents {
    /// X, Y, Z lowest and highest (mm, program coordinates).
    pub min: [f64; 3],
    pub max: [f64; 3],
    /// Each rotary letter with its lowest and highest angle (degrees).
    pub rotary: Vec<(String, f64, f64)>,
}

/// Everything on a setup sheet.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SetupSheet {
    /// The job's (and program's) name.
    pub job: String,
    pub machine: String,
    pub post: String,
    /// The G-code file's name, when one is written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_file: Option<String>,
    /// The programs' file names, in run order, when the job is written one program per tool
    /// (then named in the header in place of `program_file`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub program_files: Vec<String>,
    /// The work offset the program runs in (`G54`), or what the post does instead.
    pub work_offset: String,
    pub work_zero: WorkZero,
    pub rotaries: Vec<RotaryNote>,
    /// The stock, in words, when the job says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock: Option<String>,
    /// What holds the stock (the job's fixtures), each in words with where it is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixtures: Vec<String>,
    /// The machine's spindle-speed and feed limits the job runs into (speeds and feeds held
    /// to them, blocks the controller slows), stated first so the operator reads them before
    /// the program runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attention: Vec<String>,
    pub tools: Vec<SheetTool>,
    pub operations: Vec<SheetOperation>,
    pub totals: Totals,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extents: Option<ProgramExtents>,
    pub checks: Vec<SheetCheck>,
    pub warnings: Vec<String>,
    /// Notes on how the job was planned (tessellation and the like).
    pub notes: Vec<String>,
    /// What holds the stock on the machine (the job's workholding), in words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workholding: Option<String>,
    /// What the machine does at a tool change and what that asks of the operator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_change: Option<String>,
    /// The programs to run, in order, each in words (a job written one program per tool).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<String>,
    /// A picture of the stock, the part and work zero (PNG), put into the HTML page.
    #[serde(skip)]
    pub picture: Option<Vec<u8>>,
    /// The picture's file name beside a Markdown sheet, when it is written there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture_file: Option<String>,
    /// The plan's revision (`stops.revision`), on every program's header too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// How to probe the stock and set work zero ([`probing_steps`]), first on the sheet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub probing: Vec<ProbeStep>,
    /// The programs' stops for the operator (probing, the zero check, tool checks), in words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stops: Vec<String>,
}

/// One step of probing the stock ([`probing_steps`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProbeStep {
    pub step: usize,
    /// Where the probe touches, with the tip's height.
    pub touch: String,
    /// The direction it comes from.
    pub approach: String,
    /// What to set on the controller.
    pub set: String,
    /// What to enter in SolidCraft.
    pub enter: String,
}

/// The six steps of probing a box of stock (`lo`, `hi` in work coordinates; work zero at its
/// top's middle) held `above_jaws` mm above the vise's jaws, with a probe whose tip is `tip` mm
/// across: the top, the fixed jaw's top beside it, the X faces, the Y faces, the readings
/// entered, and the zero check.
pub fn probing_steps(lo: Vec3, hi: Vec3, above_jaws: f64, tip: f64) -> Vec<ProbeStep> {
    let (mx, my, top) = ((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z);
    let grip = (hi.z - lo.z) - above_jaws;
    let d = (above_jaws / 2.0).clamp(0.0, 3.0);
    let at = |x: f64, y: f64, z: f64| format!("X{} Y{} Z{}", mm(x), mm(y), mm(z));
    let step = |n: usize, touch: String, approach: &str, set: &str, enter: String| ProbeStep {
        step: n,
        touch,
        approach: approach.to_string(),
        set: set.to_string(),
        enter,
    };
    vec![
        step(
            1,
            format!("the stock's top in the middle, at {}", at(mx, my, top)),
            "from above (-Z)",
            "Z0 here",
            "top: the machine Z the DRO shows (to measure the height)".into(),
        ),
        step(
            2,
            format!("the fixed jaw's top just left of the stock, at {}", at(lo.x - 5.0, my, top - above_jaws)),
            "from above (-Z)",
            "nothing",
            format!("jaw_top: the machine Z the DRO shows; the stock's height is top - jaw_top + {}", mm(grip)),
        ),
        step(
            3,
            format!("the -X face, then the +X face, at Y{} with the tip {} mm below the top (Z{})", mm(my), mm(d), mm(top - d)),
            "from -X toward +X, then from +X toward -X",
            "X0 at the middle: (x_minus + x_plus) / 2",
            "x_minus and x_plus".into(),
        ),
        step(
            4,
            format!("the -Y face, then the +Y face, at X{} with the tip {} mm below the top (Z{})", mm(mx), mm(d), mm(top - d)),
            "from -Y toward +Y, then from +Y toward -Y",
            "Y0 at the middle: (y_minus + y_plus) / 2",
            "y_minus and y_plus".into(),
        ),
        step(
            5,
            "nothing: enter the readings in SolidCraft".into(),
            "-",
            "nothing",
            format!(
                "the readings in cam.make {{measured: {{setup, readings}}}}: the size is x_plus - x_minus - {} (the tip), and the reply says unchanged (run program 1) or re-planned (load the new revision)",
                mm(tip)
            ),
        ),
        step(
            6,
            format!("nothing: Cycle Start takes the tip to 5 mm above X0 Y0 ({})", at(0.0, 0.0, 5.0)),
            "from above (-Z)",
            "nothing; if it is not there, Reset and zero again",
            "nothing".into(),
        ),
    ]
}

/// A PNG's width and height, from its header; `None` for anything else.
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.get(..8)? != b"\x89PNG\r\n\x1a\n" || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let word = |at: usize| png.get(at..at + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_be_bytes);
    Some((word(16)?, word(20)?)).filter(|(w, h)| *w > 0 && *h > 0)
}

/// `bytes` in base64 (RFC 4648, with padding), for a picture put into a page.
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(char::from(ABC[((n >> (18 - 6 * k)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// How far a toolpath reaches as the program writes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Reach {
    /// The lowest Z of the tip in program coordinates (mm).
    pub z_min: f64,
    /// Each rotary's lowest and highest angle as written (radians, joint order).
    pub rotary: Vec<(f64, f64)>,
    /// Where the axes are at the end, to start the next operation from.
    pub last: Option<Joints>,
}

/// How low the tip of `path` goes and how far the rotaries turn, as a program for `machine`
/// writes it: in machine axis positions. `previous` is where the axes are before the path (the last operation's
/// end), so continuous rotaries take the same turns the program does. `None` for a path
/// without a pose.
pub fn reach(machine: &Machine, path: &Toolpath5, previous: Option<&Joints>) -> Result<Option<Reach>> {
    let mut last: Option<Joints> = previous.cloned();
    let mut z_min = f64::INFINITY;
    let mut rotary: Vec<(f64, f64)> = vec![(f64::INFINITY, f64::NEG_INFINITY); machine.rotaries.len()];
    let mut any = false;
    for pose in path.poses() {
        let joints = inverse(machine, &pose, last.as_ref())?;
        let z = joints.linear.z;
        if !z.is_finite() {
            return invalid("a tool position is not finite");
        }
        z_min = z_min.min(z);
        for (range, a) in rotary.iter_mut().zip(&joints.rotary) {
            range.0 = range.0.min(*a);
            range.1 = range.1.max(*a);
        }
        any = true;
        last = Some(joints);
    }
    if !any {
        return Ok(None);
    }
    Ok(Some(Reach { z_min, rotary, last }))
}

/// [`reach`] for an indexed operation: its rotaries at the angles it asks for, which the
/// program writes as they are (not as an equal turn nearer the previous operation's), and the
/// tip as the program writes it: the moves of its frame. `None` for an operation without a
/// move.
pub fn indexed_reach(machine: &Machine, op: &IndexedOperation) -> Result<Option<Reach>> {
    if op.rotary.len() != machine.rotaries.len() {
        return invalid(format!("the operation has {} rotary angles; the machine has {}", op.rotary.len(), machine.rotaries.len()));
    }
    if op.rotary.iter().any(|a| !a.is_finite()) {
        return invalid("the operation's rotary angles must be finite");
    }
    let tips: Vec<Vec3> = op.path.moves.iter().filter_map(Move::target).collect();
    let Some(last) = tips.last().copied() else { return Ok(None) };
    if tips.iter().any(|t| !t.is_finite()) {
        return invalid("a tool position is not finite");
    }
    let z_min = tips.iter().map(|t| t.z).fold(f64::INFINITY, f64::min);
    let rotary = op.rotary.iter().map(|&a| (a, a)).collect();
    Ok(Some(Reach { z_min, rotary, last: Some(Joints { linear: last, rotary: op.rotary.clone() }) }))
}

/// The comment line a post writes at the start of the operation `name` with `tool`, or `None`
/// for a post that writes no comments.
fn operation_line(post: &Post, name: &str, tool: &Tool) -> Option<String> {
    let text = post.comment_text(&operation_comment(post, name, tool));
    match post.comments {
        CommentStyle::Parentheses => Some(format!("({text})")),
        CommentStyle::Semicolon => Some(format!("; {text}")),
        CommentStyle::None => None,
    }
}

/// How low each operation of a written program reaches and how far it turns the rotaries, read
/// from the program itself (`text`, as [`crate::gcode5::emit_program`] wrote it for `post` and
/// `machine` from operations named `ops` with their tools, in order): each operation's blocks
/// are those after the comment the post writes at its start. The lowest program Z of any
/// block, and each rotary's range over its cuts (over every block when it has no cut, since
/// the first blocks of an operation may still be at the last one's angles). `None` when the
/// operations cannot be told apart in the text (a post that writes no comments); an operation
/// without a move is `None`. Reading the program means rewinds, unwound turns and indexed
/// angles are on the sheet as the controller will see them.
pub fn program_reach(text: &str, post: &Post5, machine: &Machine, ops: &[(&str, &Tool)]) -> Result<Option<Vec<Option<Reach>>>> {
    // The line (1-based, as the reader counts) of each operation's comment, found from the end
    // so a header comment of the same text is never taken for one.
    let lines: Vec<&str> = text.lines().collect();
    let mut starts: Vec<usize> = Vec::with_capacity(ops.len());
    let mut end = lines.len();
    for (name, tool) in ops.iter().rev() {
        let Some(want) = operation_line(&post.base, name, tool) else { return Ok(None) };
        let Some(at) = lines.get(..end).and_then(|before| before.iter().rposition(|l| l.trim_end() == want)) else { return Ok(None) };
        starts.push(at + 1);
        end = at;
    }
    starts.reverse();

    struct Acc {
        z_min: f64,
        cut: Vec<(f64, f64)>,
        all: Vec<(f64, f64)>,
        any: bool,
    }
    let n = machine.rotaries.len();
    let empty = || vec![(f64::INFINITY, f64::NEG_INFINITY); n];
    let mut acc: Vec<Acc> = ops.iter().map(|_| Acc { z_min: f64::INFINITY, cut: empty(), all: empty(), any: false }).collect();
    let widen = |ranges: &mut [(f64, f64)], angles: &[f64]| {
        for (r, a) in ranges.iter_mut().zip(angles) {
            r.0 = r.0.min(*a);
            r.1 = r.1.max(*a);
        }
    };
    let mut k: Option<usize> = None;
    for b in parse5(text, &machine.letters())? {
        let (line, linear, rotary, cut) = match &b {
            JointWords::Rapid { line, linear, rotary, .. } => (*line, *linear, rotary, false),
            JointWords::Feed { line, linear, rotary, .. }
            | JointWords::Arc { line, linear, rotary, .. }
            | JointWords::PlaneArc { line, linear, rotary, .. } => (*line, *linear, rotary, true),
            JointWords::Dwell { .. } => continue,
        };
        // The blocks come in line order: the operation is the last one started before the line.
        while starts.get(k.map_or(0, |k| k + 1)).is_some_and(|&s| s < line) {
            k = Some(k.map_or(0, |k| k + 1));
        }
        let Some(a) = k.and_then(|k| acc.get_mut(k)) else { continue };
        if !linear.is_finite() || rotary.iter().any(|v| !v.is_finite()) {
            return invalid(format!("line {line}: a position is not finite"));
        }
        a.z_min = a.z_min.min(linear.z);
        widen(&mut a.all, rotary);
        if cut {
            widen(&mut a.cut, rotary);
        }
        a.any = true;
    }
    let reaches = acc
        .into_iter()
        .map(|a| {
            let has_cut = a.cut.iter().all(|r| r.0 <= r.1);
            a.any.then_some(Reach { z_min: a.z_min, rotary: if has_cut { a.cut } else { a.all }, last: None })
        })
        .collect();
    Ok(Some(reaches))
}

/// A number as the sheet shows it: at most `decimals` places, trailing zeros dropped, `-` for
/// a value that is not finite.
pub fn fmt_num(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return "-".into();
    }
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s.as_str() };
    if s == "-0" { "0".into() } else { s.to_string() }
}

fn mm(v: f64) -> String {
    fmt_num(v, 3)
}

/// The name of the coordinate direction `d` is along (`+X`, `-Z`), or its components.
fn direction_name(d: Vec3) -> String {
    let names = [(Vec3::X, "X"), (Vec3::Y, "Y"), (Vec3::Z, "Z")];
    for (axis, name) in names {
        let c = d.dot(axis);
        if (c.abs() - 1.0).abs() <= 1e-6 {
            return format!("{}{name}", if c > 0.0 { "+" } else { "-" });
        }
    }
    format!("[{}, {}, {}]", fmt_num(d.x, 4), fmt_num(d.y, 4), fmt_num(d.z, 4))
}

/// Words for a direction on a mill seen from its front: `-Y` is towards the operator.
fn direction_hint(name: &str) -> &'static str {
    match name {
        "-Y" => " (towards the front of the machine)",
        "+Y" => " (towards the back of the machine)",
        "+X" => " (to the right)",
        "-X" => " (to the left)",
        "+Z" => " (up)",
        "-Z" => " (down)",
        _ => "",
    }
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// Where `0` lies on one axis of the part's extent `lo..hi`, in a sentence.
fn axis_words(letter: char, lo: f64, hi: f64, sides: [&str; 2], beyond: [&str; 2]) -> String {
    let span = (hi - lo).abs();
    let eps = (span * 1e-4).max(1e-3);
    let mid = 0.5 * (lo + hi);
    if (lo - 0.0).abs() <= eps {
        format!("{letter}0 is on the part's {} ({letter} {} to {}).", sides[0], mm(lo), mm(hi))
    } else if (hi - 0.0).abs() <= eps {
        format!("{letter}0 is on the part's {} ({letter} {} to {}).", sides[1], mm(lo), mm(hi))
    } else if mid.abs() <= eps {
        format!("{letter}0 is in the middle of the part in {letter} ({letter} {} to {}).", mm(lo), mm(hi))
    } else if lo < 0.0 && hi > 0.0 {
        format!("{letter}0 is inside the part's extent, {} mm from its {} and {} mm from its {}.", mm(-lo), sides[0], mm(hi), sides[1])
    } else if lo > 0.0 {
        format!("{letter}0 is {} mm {} the part's {} (the part runs {letter} {} to {}).", mm(lo), beyond[0], sides[0], mm(lo), mm(hi))
    } else {
        format!("{letter}0 is {} mm {} the part's {} (the part runs {letter} {} to {}).", mm(-hi), beyond[1], sides[1], mm(lo), mm(hi))
    }
}

/// The job's stock in words, for the operator who saws and clamps it: a box by its corners and
/// size, a bar by its diameter and its ends along X, in work coordinates.
pub fn stock_words(stock: &crate::stock::Stock) -> String {
    match *stock {
        crate::stock::Stock::Box { min, max } => {
            let size = max - min;
            format!(
                "A block {} x {} x {} mm (X x Y x Z), from X{} Y{} Z{} to X{} Y{} Z{}; its top is at Z{}.",
                mm(size.x),
                mm(size.y),
                mm(size.z),
                mm(min.x),
                mm(min.y),
                mm(min.z),
                mm(max.x),
                mm(max.y),
                mm(max.z),
                mm(max.z)
            )
        }
        crate::stock::Stock::Cylinder { radius, x_min, x_max } => format!(
            "A bar {} mm in diameter about the X axis (Y0 Z0), from X{} to X{} ({} mm long).",
            mm(2.0 * radius),
            mm(x_min),
            mm(x_max),
            mm(x_max - x_min)
        ),
    }
}

/// Where work zero is on the part in words: the design point, the part's extent from it, and
/// each axis. `lo`/`hi` are the part's extent in work coordinates; `rotary_x` is the letter of a
/// rotary table about work X through work zero (a 4th axis), whose centreline Y0 Z0 is on.
pub fn work_zero(design_point: Vec3, lo: Vec3, hi: Vec3, rotary_x: Option<char>, radius: Option<f64>) -> WorkZero {
    let mut words = vec![
        format!("Work zero (X0 Y0 Z0) is the design point ({}, {}, {}).", mm(design_point.x), mm(design_point.y), mm(design_point.z)),
        format!(
            "The part runs X {} to {}, Y {} to {}, Z {} to {} from work zero ({} x {} x {} mm).",
            mm(lo.x),
            mm(hi.x),
            mm(lo.y),
            mm(hi.y),
            mm(lo.z),
            mm(hi.z),
            mm(hi.x - lo.x),
            mm(hi.y - lo.y),
            mm(hi.z - lo.z)
        ),
        axis_words('X', lo.x, hi.x, ["left side (lowest X)", "right side (highest X)"], ["left of", "right of"]),
    ];
    match rotary_x {
        Some(letter) => {
            words.push(format!(
                "Y0 Z0 is on the {letter} axis centreline: set the work offset's Y and Z on the rotary axis, not on the part's surface."
            ));
            if let Some(r) = radius {
                words.push(format!("The part reaches {} mm from the {letter} axis (stock at least {} mm across).", mm(r), mm(2.0 * r)));
            }
            words.push(format!("With {letter}0 the part sits as modelled: its +Z side up under the tool."));
        }
        None => {
            words.push(axis_words('Y', lo.y, hi.y, ["front side (lowest Y)", "back side (highest Y)"], ["in front of", "behind"]));
            words.push(axis_words('Z', lo.z, hi.z, ["bottom", "top"], ["below", "above"]));
        }
    }
    WorkZero { design_point: arr(design_point), part_min: arr(lo), part_max: arr(hi), radius, words }
}

/// A rotary axis's convention in words: what it turns, about which line, which way a positive
/// angle turns it and what comes up under the tool, and its travel.
pub fn rotary_note(axis: &RotaryAxis) -> RotaryNote {
    let l = axis.letter.char();
    let d = axis.direction.normalized().unwrap_or(Vec3::Z);
    let dname = direction_name(d);
    let sign = if axis.reversed { -1.0 } else { 1.0 };
    let through = if axis.center.len() <= 1e-9 {
        "through work zero".to_string()
    } else {
        format!("through ({}, {}, {})", mm(axis.center.x), mm(axis.center.y), mm(axis.center.z))
    };
    let mut words = Vec::new();
    let seen = format!("seen from the {dname} end looking back along the axis");
    let turn = if axis.reversed { "clockwise" } else { "counter-clockwise" };
    match axis.carrier {
        Carrier::Table => {
            words.push(format!("{l} turns the table (and the part) about {dname} {through}."));
            // The work direction a turn of +90° brings up to +Z, and which way the top moves.
            let up = rotate(Vec3::Z, d, -sign * std::f64::consts::FRAC_PI_2);
            let top = d.cross(Vec3::Z) * sign;
            let up_name = direction_name(up);
            words.push(format!(
                "Positive {l} turns the table {turn} {seen}{}.",
                if axis.reversed { " (the controller's positive turns it left-handed: the table is set up reversed)" } else { "" }
            ));
            match top.normalized() {
                Some(t) => {
                    let t = direction_name(t);
                    words.push(format!(
                        "{l}+90° brings the part's {up_name} side up under the tool; as {l} rises the top of the part moves towards {t}{}.",
                        direction_hint(&t)
                    ));
                }
                None => words.push(format!("{l}+90° brings the part's {up_name} side up under the tool.")),
            }
        }
        Carrier::Head => {
            words.push(format!("{l} tilts the spindle head about {dname} {through} (from the head's control point)."));
            words.push(format!("Positive {l} turns the head {turn} {seen}."));
        }
    }
    let limits_deg = axis.limits.map(|(lo, hi)| [lo.to_degrees(), hi.to_degrees()]);
    match limits_deg {
        Some([lo, hi]) => words.push(format!("{l} travels from {}° to {}° in the machine settings.", fmt_num(lo, 3), fmt_num(hi, 3))),
        None => words.push(format!("{l} has no travel limits in the job's machine: check the controller's own limits against the range below.")),
    }
    RotaryNote { letter: l.to_string(), words, limits_deg }
}

/// The rotary angles an operation runs at, in words: one angle when every rotary stays put
/// (`A90°`), else each rotary's range.
pub fn rotary_words(letters: &[String], ranges: &[(f64, f64)]) -> String {
    letters
        .iter()
        .zip(ranges)
        .filter(|(_, r)| r.0 <= r.1)
        .map(|(l, r)| {
            let (lo, hi) = (r.0.to_degrees(), r.1.to_degrees());
            if (hi - lo).abs() < 5e-4 { format!("{l}{}°", fmt_num(lo, 3)) } else { format!("{l} {}° to {}°", fmt_num(lo, 3), fmt_num(hi, 3)) }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A character that is not a control character, else a space.
fn printable(c: char) -> char {
    if c.is_control() { ' ' } else { c }
}

/// `text` safe inside HTML text and attribute values.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(printable(c)),
        }
    }
    out
}

/// `text` as plain text in Markdown, a table cell included: on one line, the characters that
/// mark emphasis, code, links, headings and table cells backslash-escaped, and HTML's `<`, `>`
/// and `&` as entities so a viewer that passes HTML through shows them rather than running
/// them. The sheet never starts a line with escaped text, so list and heading markers that only
/// count at the start of a line (`-`, `+`, `1.`) are left as they are.
pub fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\\' | '|' | '*' | '_' | '`' | '[' | ']' | '#' | '~' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(printable(c)),
        }
    }
    out
}

/// The holder sections in words.
fn holder_words(t: &SheetTool) -> String {
    if t.holder.is_empty() {
        return "not given".into();
    }
    t.holder.iter().map(|[d, l]| format!("Ø{} x {}", mm(*d), mm(*l))).collect::<Vec<_>>().join(" + ")
}

fn opt_mm(v: Option<f64>) -> String {
    v.map_or_else(|| "-".to_string(), mm)
}

/// The tool's size in words: diameter, corner radius.
fn tool_size(t: &SheetTool) -> String {
    match t.corner_radius {
        Some(r) => format!("Ø{} R{}", mm(t.diameter), mm(r)),
        None => format!("Ø{}", mm(t.diameter)),
    }
}

const STYLE: &str = "body{font:14px/1.4 system-ui,sans-serif;color:#111;background:#fff;margin:24px;max-width:1100px}\
h1{font-size:22px;margin:0 0 4px}h2{font-size:16px;margin:22px 0 6px;border-bottom:1px solid #999;padding-bottom:2px}\
table{border-collapse:collapse;width:100%;margin:4px 0}th,td{border:1px solid #bbb;padding:3px 6px;text-align:left;vertical-align:top}\
th{background:#eee}td.n{text-align:right;font-variant-numeric:tabular-nums}.ok{color:#060}.bad{color:#a00;font-weight:bold}\
.meta td:first-child{width:160px;font-weight:bold}ul{margin:4px 0;padding-left:22px}.tick{display:inline-block;width:14px;height:14px;\
border:1px solid #333;margin-right:6px;vertical-align:middle}\
@media print{body{margin:10mm;font-size:11px;max-width:none}h2{break-after:avoid}tr{break-inside:avoid}th{background:#ddd !important}}";

impl SetupSheet {
    /// The sheet as a self-contained HTML page: no scripts, nothing loaded, print styles.
    pub fn html(&self) -> String {
        let e = escape_html;
        let mut h = String::with_capacity(8192);
        h.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
        h.push_str(&format!("<title>Setup sheet: {}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n", e(&self.job)));
        h.push_str(&format!("<h1>Setup sheet: {}</h1>\n", e(&self.job)));
        h.push_str("<table class=\"meta\">\n");
        let mut meta = vec![("Machine", self.machine.clone()), ("Post", self.post.clone())];
        if !self.program_files.is_empty() {
            meta.push(("Programs", self.program_files.join(", ")));
        } else if let Some(f) = &self.program_file {
            meta.push(("Program", f.clone()));
        }
        if let Some(r) = &self.revision {
            meta.push(("Revision", r.clone()));
        }
        meta.push(("Work offset", self.work_offset.clone()));
        meta.push(("Units", "mm, mm/min".into()));
        for (k, v) in meta {
            h.push_str(&format!("<tr><td>{k}</td><td>{}</td></tr>\n", e(&v)));
        }
        h.push_str("</table>\n");
        if !self.probing.is_empty() {
            h.push_str("<h2>Probing</h2>\n<ol>\n");
            for p in &self.probing {
                h.push_str(&format!("<li>Touch {}; approach {}; set {}; enter {}.</li>\n", e(&p.touch), e(&p.approach), e(&p.set), e(&p.enter)));
            }
            h.push_str("</ol>\n");
        }
        if !self.stops.is_empty() {
            h.push_str("<h2>Stops</h2>\n<ul>\n");
            for st in &self.stops {
                h.push_str(&format!("<li>{}</li>\n", e(st)));
            }
            h.push_str("</ul>\n");
        }
        if !self.attention.is_empty() {
            h.push_str("<h2>Before you start: the machine's limits</h2>\n<ul>\n");
            for a in &self.attention {
                h.push_str(&format!("<li class=\"bad\">{}</li>\n", e(a)));
            }
            h.push_str("</ul>\n");
        }
        if let Some(t) = &self.tool_change {
            h.push_str(&format!("<h2>Tool changes</h2>\n<p class=\"bad\">{}</p>\n", e(t)));
        }
        if !self.programs.is_empty() {
            h.push_str("<h2>Programs, in the order to run them</h2>\n<ol>\n");
            for p in &self.programs {
                h.push_str(&format!("<li><span class=\"tick\"></span>{}</li>\n", e(p)));
            }
            h.push_str("</ol>\n");
        }
        if let Some(png) = &self.picture {
            // An SVG holding the PNG: drawn inside the page, nothing fetched.
            let (w, hh) = png_size(png).unwrap_or((800, 520));
            h.push_str(&format!(
                "<h2>Setup</h2>\n<svg role=\"img\" aria-label=\"The stock, the part and work zero\" viewBox=\"0 0 {w} {hh}\" \
style=\"max-width:100%;width:{w}px\"><image width=\"{w}\" height=\"{hh}\" href=\"data:image/png;base64,{}\"/></svg>\n",
                base64(png)
            ));
        }

        h.push_str("<h2>Work zero</h2>\n<ul>\n");
        for w in &self.work_zero.words {
            h.push_str(&format!("<li>{}</li>\n", e(w)));
        }
        h.push_str("</ul>\n");
        for r in &self.rotaries {
            h.push_str(&format!("<h2>{} axis</h2>\n<ul>\n", e(&r.letter)));
            for w in &r.words {
                h.push_str(&format!("<li>{}</li>\n", e(w)));
            }
            h.push_str("</ul>\n");
        }
        if let Some(stock) = &self.stock {
            h.push_str(&format!("<h2>Stock</h2>\n<p>{}</p>\n", e(stock)));
        }
        if let Some(w) = &self.workholding {
            h.push_str(&format!("<h2>Workholding</h2>\n<p>{}</p>\n", e(w)));
        }
        if !self.fixtures.is_empty() {
            h.push_str("<h2>Fixtures</h2>\n<ul>\n");
            for f in &self.fixtures {
                h.push_str(&format!("<li>{}</li>\n", e(f)));
            }
            h.push_str("</ul>\n");
        }

        h.push_str(
            "<h2>Tools</h2>\n<table>\n<tr><th>Loaded</th><th>T</th><th>Kind</th><th>Size</th><th>Flutes</th><th>Flute length</th>\
<th>Shank</th><th>Stickout</th><th>Holder</th><th>Z min</th><th>Operations</th><th>Description</th></tr>\n",
        );
        for t in &self.tools {
            h.push_str(&format!(
                "<tr><td><span class=\"tick\"></span></td><td class=\"n\">T{}</td><td>{}</td><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td>\
<td class=\"n\">{}</td><td class=\"n\">{}</td><td>{}</td><td class=\"n\">{}</td><td>{}</td><td>{}</td></tr>\n",
                t.number,
                e(&t.kind),
                e(&tool_size(t)),
                t.flutes,
                mm(t.flute_length),
                opt_mm(t.shank_diameter),
                opt_mm(t.stickout),
                e(&holder_words(t)),
                opt_mm(t.z_min),
                e(&t.operations.join(", ")),
                e(&t.description)
            ));
        }
        h.push_str("</table>\n");

        h.push_str(
            "<h2>Operations</h2>\n<table>\n<tr><th>#</th><th>Operation</th><th>Kind</th><th>T</th><th>Spindle</th><th>Feed</th>\
<th>Plunge</th><th>Coolant</th><th>Rotary</th><th>Z min</th><th>Cut length</th><th>Minutes</th></tr>\n",
        );
        for (i, o) in self.operations.iter().enumerate() {
            h.push_str(&format!(
                "<tr><td class=\"n\">{}</td><td>{}</td><td>{}</td><td class=\"n\">T{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td>\
<td class=\"n\">{}</td><td>{}</td><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>\n",
                i + 1,
                e(&o.name),
                e(&o.kind),
                o.tool,
                fmt_num(o.spindle_rpm, 0),
                fmt_num(o.feed, 0),
                fmt_num(o.plunge_feed, 0),
                if o.coolant { "on" } else { "off" },
                e(if o.rotary.is_empty() { "-" } else { &o.rotary }),
                opt_mm(o.z_min),
                fmt_num(o.cut_length, 0),
                fmt_num(o.cut_minutes, 1)
            ));
        }
        let t = &self.totals;
        h.push_str(&format!(
            "<tr><th colspan=\"10\">Total ({} operations, {} tools, {} tool changes)</th><th class=\"n\">{}</th><th class=\"n\">{}</th></tr>\n</table>\n",
            t.operations,
            t.tools,
            t.tool_changes,
            fmt_num(t.cut_length, 0),
            fmt_num(t.cut_minutes, 1)
        ));
        h.push_str("<ul>\n");
        for line in self.total_lines() {
            h.push_str(&format!("<li>{}</li>\n", e(&line)));
        }
        h.push_str("</ul>\n");

        h.push_str("<h2>Checks</h2>\n<table>\n<tr><th>Check</th><th>Result</th><th>Detail</th></tr>\n");
        for c in &self.checks {
            let (class, word) = if c.passed { ("ok", "passed") } else { ("bad", "FAILED") };
            h.push_str(&format!("<tr><td>{}</td><td class=\"{class}\">{word}</td><td>{}</td></tr>\n", e(&c.name), e(&c.detail)));
        }
        h.push_str("</table>\n");

        h.push_str("<h2>Warnings</h2>\n");
        if self.warnings.is_empty() {
            h.push_str("<p>None.</p>\n");
        } else {
            h.push_str("<ul>\n");
            for w in &self.warnings {
                h.push_str(&format!("<li class=\"bad\">{}</li>\n", e(w)));
            }
            h.push_str("</ul>\n");
        }
        if !self.notes.is_empty() {
            h.push_str("<h2>Notes</h2>\n<ul>\n");
            for n in &self.notes {
                h.push_str(&format!("<li>{}</li>\n", e(n)));
            }
            h.push_str("</ul>\n");
        }
        h.push_str("</body>\n</html>\n");
        h
    }

    /// The totals and the program's extents, a sentence each.
    fn total_lines(&self) -> Vec<String> {
        let t = &self.totals;
        let mut lines = vec![format!(
            "{} mm at feed in {} min as planned; rapids and tool changes are not included.",
            fmt_num(t.cut_length, 0),
            fmt_num(t.cut_minutes, 1)
        )];
        if let Some(m) = t.program_minutes {
            lines.push(format!("{} min at feed as the controller times the written program ({} lines).", fmt_num(m, 1), t.program_lines));
        }
        if let Some(x) = &self.extents {
            lines.push(format!(
                "The program moves X {} to {}, Y {} to {}, Z {} to {}.",
                mm(x.min[0]),
                mm(x.max[0]),
                mm(x.min[1]),
                mm(x.max[1]),
                mm(x.min[2]),
                mm(x.max[2])
            ));
            for (l, lo, hi) in &x.rotary {
                lines.push(format!("It turns {l} from {}° to {}°.", fmt_num(*lo, 3), fmt_num(*hi, 3)));
            }
        }
        lines
    }

    /// The sheet as Markdown.
    pub fn markdown(&self) -> String {
        let e = escape_markdown;
        let mut m = String::with_capacity(8192);
        m.push_str(&format!("# Setup sheet: {}\n\n", e(&self.job)));
        m.push_str(&format!("- **Machine:** {}\n- **Post:** {}\n", e(&self.machine), e(&self.post)));
        if !self.program_files.is_empty() {
            m.push_str(&format!("- **Programs:** {}\n", e(&self.program_files.join(", "))));
        } else if let Some(f) = &self.program_file {
            m.push_str(&format!("- **Program:** {}\n", e(f)));
        }
        if let Some(r) = &self.revision {
            m.push_str(&format!("- **Revision:** {}\n", e(r)));
        }
        m.push_str(&format!("- **Work offset:** {}\n- **Units:** mm, mm/min\n\n", e(&self.work_offset)));
        if !self.probing.is_empty() {
            m.push_str("## Probing\n\n");
            for p in &self.probing {
                m.push_str(&format!("{}. Touch {}; approach {}; set {}; enter {}.\n", p.step, e(&p.touch), e(&p.approach), e(&p.set), e(&p.enter)));
            }
            m.push('\n');
        }
        if !self.stops.is_empty() {
            m.push_str("## Stops\n\n");
            for st in &self.stops {
                m.push_str(&format!("- {}\n", e(st)));
            }
            m.push('\n');
        }
        if !self.attention.is_empty() {
            m.push_str("## Before you start: the machine's limits\n\n");
            for a in &self.attention {
                m.push_str(&format!("- **{}**\n", e(a)));
            }
            m.push('\n');
        }
        if let Some(t) = &self.tool_change {
            m.push_str(&format!("## Tool changes\n\n**{}**\n\n", e(t)));
        }
        if !self.programs.is_empty() {
            m.push_str("## Programs, in the order to run them\n\n");
            for (k, p) in self.programs.iter().enumerate() {
                m.push_str(&format!("{}. [ ] {}\n", k + 1, e(p)));
            }
            m.push('\n');
        }
        if let Some(f) = &self.picture_file {
            m.push_str(&format!("## Setup\n\n![The stock, the part and work zero]({})\n\n", f.replace(' ', "%20").replace(')', "%29")));
        }
        m.push_str("## Work zero\n\n");
        for w in &self.work_zero.words {
            m.push_str(&format!("- {}\n", e(w)));
        }
        m.push('\n');
        for r in &self.rotaries {
            m.push_str(&format!("## {} axis\n\n", e(&r.letter)));
            for w in &r.words {
                m.push_str(&format!("- {}\n", e(w)));
            }
            m.push('\n');
        }
        if let Some(stock) = &self.stock {
            m.push_str(&format!("## Stock\n\n- {}\n\n", e(stock)));
        }
        if let Some(w) = &self.workholding {
            m.push_str(&format!("## Workholding\n\n- {}\n\n", e(w)));
        }
        if !self.fixtures.is_empty() {
            m.push_str("## Fixtures\n\n");
            for f in &self.fixtures {
                m.push_str(&format!("- {}\n", e(f)));
            }
            m.push('\n');
        }
        m.push_str("## Tools\n\n| T | Kind | Size | Flutes | Flute length | Shank | Stickout | Holder | Z min | Operations | Description |\n");
        m.push_str("|---:|---|---|---:|---:|---:|---:|---|---:|---|---|\n");
        for t in &self.tools {
            m.push_str(&format!(
                "| T{} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                t.number,
                e(&t.kind),
                e(&tool_size(t)),
                t.flutes,
                e(&mm(t.flute_length)),
                e(&opt_mm(t.shank_diameter)),
                e(&opt_mm(t.stickout)),
                e(&holder_words(t)),
                e(&opt_mm(t.z_min)),
                e(&t.operations.join(", ")),
                e(&t.description)
            ));
        }
        m.push_str("\n## Operations\n\n| # | Operation | Kind | T | Spindle | Feed | Plunge | Coolant | Rotary | Z min | Cut length | Minutes |\n");
        m.push_str("|---:|---|---|---:|---:|---:|---:|---|---|---:|---:|---:|\n");
        for (i, o) in self.operations.iter().enumerate() {
            m.push_str(&format!(
                "| {} | {} | {} | T{} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                i + 1,
                e(&o.name),
                e(&o.kind),
                o.tool,
                fmt_num(o.spindle_rpm, 0),
                fmt_num(o.feed, 0),
                fmt_num(o.plunge_feed, 0),
                if o.coolant { "on" } else { "off" },
                e(if o.rotary.is_empty() { "-" } else { &o.rotary }),
                e(&opt_mm(o.z_min)),
                fmt_num(o.cut_length, 0),
                e(&fmt_num(o.cut_minutes, 1))
            ));
        }
        let t = &self.totals;
        m.push_str(&format!("\n**Total:** {} operations, {} tools, {} tool changes.\n\n", t.operations, t.tools, t.tool_changes));
        for line in self.total_lines() {
            m.push_str(&format!("- {}\n", e(&line)));
        }
        m.push_str("\n## Checks\n\n| Check | Result | Detail |\n|---|---|---|\n");
        for c in &self.checks {
            m.push_str(&format!("| {} | {} | {} |\n", e(&c.name), if c.passed { "passed" } else { "**FAILED**" }, e(&c.detail)));
        }
        m.push_str("\n## Warnings\n\n");
        if self.warnings.is_empty() {
            m.push_str("None.\n");
        } else {
            for w in &self.warnings {
                m.push_str(&format!("- {}\n", e(w)));
            }
        }
        if !self.notes.is_empty() {
            m.push_str("\n## Notes\n\n");
            for n in &self.notes {
                m.push_str(&format!("- {}\n", e(n)));
            }
        }
        m
    }

    /// The sheet in `format`.
    pub fn render(&self, format: SheetFormat) -> String {
        match format {
            SheetFormat::Html => self.html(),
            SheetFormat::Markdown => self.markdown(),
        }
    }
}
