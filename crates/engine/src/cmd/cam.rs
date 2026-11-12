//! CAM (`cam.job`, `cam.post.list`, `cam.post.get`, `cam.gcode.check`, `cam.simulate`): toolpaths
//! for a design's profiles and surfaces and G-code for a machine, through the `solidcraft-cam`
//! crate. The job (machine, post, tools, operations) is a JSON file next to the design, never part
//! of it, so nothing new is saved in a design (docs/cam-jobs.md). The simulation view's commands
//! (`cam.view.*`) are in `cam_view.rs`, and the pictures of the toolpaths (`cam.view`, and
//! `cam.job`'s `image` and `images_dir`) in `cam_image.rs`. `cam.features`, the holes found on
//! the part's exact faces, is in `cam_features.rs`.
//!
//! `cam.job` plans every operation in work coordinates (the design less the job's origin): the
//! 2D ones on profiles, the 3D ones on the part's bodies tessellated from their exact surfaces
//! within a quarter of the job's tolerance. On a 4th axis a 3-axis operation may be indexed (A
//! held at an angle), and the rotary and wrapped operations turn A as they cut
//! (`cam_xrotary.rs`, with the check that A never turns the part into the tool). It writes the program through the job's post for
//! its machine, reads that back and compares it with the toolpaths, and checks every cutting
//! move for gouges against the part and the job's fixtures and, for tools with holder data, for
//! shank and holder collisions. A program that fails a check is not written unless the call
//! allows it. Every path is checked against the session's
//! folders (`vfs`, which MCP's `--allow-path` confines) before anything is computed or written.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_cam::collision::{Hit, holder_collisions_within};
use solidcraft_cam::drill::tool_gouges;
use solidcraft_cam::gcode5::{JointWords, MachineOperation, MachineProgram, Post5, Rewind, emit_program_report, parse5};
use solidcraft_cam::indexed::{IndexedOperation, frame_for_index};
use solidcraft_cam::kinematics::{AxisLetter, Carrier, Machine, RotaryAxis};
use solidcraft_cam::readback5::readback_program;
use solidcraft_cam::sim::StockShape;
use solidcraft_cam::stock::Stock;
use solidcraft_cam::surface_ops::{Boundary, ParallelParams, RoughingParams, WaterlineParams, parallel, roughing, waterline};
use solidcraft_cam::{
    CamError, Compaction, ContourParams, Cut, DEFAULT_DESCENT_LIMIT, Feeds, Frame, Heights, HolderSection, LineNumbers, PocketParams, SafeRetract,
    Side, Surface, Tool, ToolKind, Toolpath, Toolpath5, band_gouges, compact, contour_with, lift_entries, limit_descents, pocket_with,
};
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::cam::{
    BoundarySpec, CamGcodeCheckParams, CamGcodeCheckReply, CamJob, CamJobParams, CamJobReply, CamPostGetParams, CamPostGetReply, CamPostListParams,
    CamPostListReply, Common, GougeAt, GougeReport, HeightsSpec, MachineSpec, Op3Spec, OperationReport, OperationSpec, Outputs, ParseErrorReport,
    PostEntry, ReadbackReport, RotaryRange, SideSpec, ToolChangeMode,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

#[path = "cam_adaptive.rs"]
mod adaptive;
#[path = "cam_stock.rs"]
mod cam_stock;
#[path = "cam_chamfer.rs"]
mod chamfer;
#[path = "cam_drill.rs"]
mod drill;
#[path = "cam_finish3d.rs"]
mod finish3d;
#[path = "cam_helical.rs"]
mod helical;
#[path = "cam_hold.rs"]
mod hold;
#[path = "cam_leads.rs"]
mod leads;
#[path = "cam_xrotary.rs"]
mod xrotary;

const JOB_CONTRACT: TypedContract<CamJobParams> = TypedContract::new();
const POST_LIST_CONTRACT: TypedContract<CamPostListParams> = TypedContract::new();
const POST_GET_CONTRACT: TypedContract<CamPostGetParams> = TypedContract::new();
const GCODE_CHECK_CONTRACT: TypedContract<CamGcodeCheckParams> = TypedContract::new();

pub(crate) use features::COMMANDS as FEATURE_COMMANDS;
pub(crate) use pictures::COMMANDS as PICTURE_COMMANDS;
pub(crate) use view::COMMANDS as VIEW_COMMANDS;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.job", "CAM Job", crate::checked!("cam.job", job))
        .icon("cam")
        .noundo()
        .contract(&JOB_CONTRACT)
        .output(output_schema_for::<CamJobReply>)
        .params(
            "job: {…} | job_path: .json (the CAM job, docs/cam-jobs.md: name?, machine: \"3axis\" | \"4axis_a\" | Machine JSON \
             | machine_path: Machine JSON file, a_reversed?, a_limits_deg?: [min, max], post: built-in name (cam.post.list) | post_path: Post5 JSON file, work_offset?: 1-6, \
             origin?: [x,y,z] (the design point that is work zero; on a 4th axis the A axis is work X through it), orientation?: \
             \"flip_x\" | \"flip_y\" | {axis, angle_deg} | {sketch} (the part turned for this setup, 3-axis), stock?: {box: {min, \
             max}} | {cylinder: {radius, x_min, x_max}} | {around_part: {margin: [x,y,z], top?, bottom?}} (work coordinates; no rapid \
             may pass through it), breakthrough?: mm a through cut may go below the stock's bottom (0-25), \
             machine_model_path? + workholding_path? | workholding? (every move is then checked against the vise, clamps, parallels, \
             table or rotary unit, the spindle head against the part, and the spindle against its Z travel), bodies?: [names] (the part: what 3D operations follow and every \
             cut is checked for gouges against, drawn in the preview; default every visible body), fixtures?: [names] (checked \
             against, never cut), tolerance?: mm (default 0.01; 3D operations plan on the bodies tessellated within a \
             quarter of it), tools: [Tool JSON] | tool_library_path, machine_rates?: {rapid_xyz_mm_min, rapid_z_mm_min?, \
             rapid_a_deg_min?, max_feed_mm_min?, max_feed_z_mm_min?, max_a_deg_min?, spindle_rpm?: [min, max], spindle_start_seconds?, \
             travel_mm?: [x, y, z], home_z_mm?, …} | machine_rates_path (times the program as the machine runs it and warns about \
             blocks the controller slows), tool_change?: auto | stop | ignored (ignored: the post stops at each change, and a program \
             of several tools is warned about unless split_by_tool), operations: [{name, type: contour | pocket (sketch + profiles? | \
             polygon: [[x,y],…], stepdown?, side? (contour: outside | inside | on), stepover? (pocket), ramp_angle_deg?, entry?, \
             linking?, contour: lead?, vertical_lead?, tabs?, finish_passes?, finish_stepover?, spring_pass?) | drill {sketch + \
             circles?, diameter_range?, sketch_points? | points, cycle?: simple | dwell | peck | chip_break, peck?, …} | chamfer \
             {sketch | polygon | holes, heights: {clearance, retract, top}, width, tip_offset?, side?} | bore {sketch + diameter_range? \
             | holes, pitch? | ramp_angle_deg?, stepover?, finish_passes?} | thread_mill {sketch | centers, pitch, thread_diameter, \
             internal?, right_hand?, thread_depth?, radial_passes?, spring_pass?} | adaptive {sketch + profiles? | polygon, \
             optimal_load?, min_engagement_deg?, max_engagement_deg?, helix_diameter?, compare?} | adaptive_roughing {boundary?, \
             optimal_load?, flats?, step_up?, …} | face {bottom, top?, retract?, clearance?, stepdown?, stepover?, angle_deg?, \
             zigzag?, overhang?, rect?, lead_radius?} | parallel | \
             roughing | waterline | scallop | steep_shallow | pencil | rest_finish (boundary?: {rect?, polygon?, holes?}, stepover?, \
             step?, …) | indexed {a_deg, operation, name?} | indexed_pattern {a_deg: [..] | count, start_deg?, operation, name?} | \
             rotary_finish | rotary_roughing (pattern: lines | helix | rings, clearance, retract, stock_radius?) | wrap | wrap_pocket | \
             wrap_contour (radius, …), tool: number, heights: {clearance, retract, top, bottom}, climb?, stock_to_leave?, tolerance?, spindle_rpm?, \
             feeds?: {cut, plunge, descent?}, coolant?, gouge_check? (false: not against the part, still against fixtures), enabled?}]; unknown keys are refused); output: G-code path (required unless dry_run); preview?: \
             HTML path, another file than output (the part and toolpaths, self-contained); setup_sheet?: .html | .md path (the \
             operator's sheet: work zero, A convention, tools with Z min, operations, checks, warnings); dry_run?: plan and check, write \
             nothing; allow_unsafe?: write even when a check fails (its first line then says so); split_by_tool?: one program \
             per tool, named after output (part-2-T6.ngc), each complete on its own; simulate?: also run the written program through \
             cam.simulate's stock model (a gouge or a rapid in material fails); simulate_resolution?: its cell mm; image?: PNG path (the \
             part with every toolpath drawn over it, written in a dry run too); images_dir?: one PNG per operation; view?: iso | top | \
             bottom | front | back | left | right; width?, height?: 16-4096 px → name, machine, post, operations: [{name, \
             kind, tool, moves, cut_length, cut_minutes, rewinds, max_tilt_deg, a_deg?, gouge, gouge_at?, holder_collisions, adaptive? (the engagement measured \
             along its path: max_engagement_deg, full_slot_share, levels, helices, …, pocket?: the plain pocket compared), warnings, extents: {all, cut?, rotary, summary}}], program_lines, program_head, readback: {max_tip_deviation, \
             max_axis_deviation_deg, allowed_tip, allowed_axis_deg, cut_minutes, pass}, gouge: {worst, tolerance, tessellation, \
             allowed, hits, at?: {operation, move_index, line?, xyz}, skipped, pass}, holder_collisions, rapids: {checked, hits, \
             worst, pass, from_unknown, first: [{line, operation?, kind, depth, at}]}, travel: {checked, violation?, pass} | null, \
             stock, workholding?: {name, holding, machine_model?, checked_moves, hits: [{code, error, tool, operation?, line, moves, \
             at?, message, fix}], stock_above_jaws?, bar_grip?, pass, setup?, setup_warnings?}, cut_minutes, rapid_minutes?, machine_time?: \
             {cut_minutes, written_cut_minutes, rapid_minutes, home_returns, spindle_minutes, total_minutes, slowed_blocks, rotary_over, \
             feed_over, peak_rotary_deg_min?, peak_feed_mm_min?, spindle_out_of_range, span_mm, warnings}, passed, failures, dry_run, \
             outputs: {gcode?, preview?, setup_sheet?, programs?}, notes, simulation? (as cam.simulate), summary: {work_zero, part_extent, work_offset, rotary, tools: \
             [{number, kind, diameter, flute_length, stickout?, holder?, z_min?, operations}], operations: [{name, kind, tool, \
             spindle_rpm, feed, plunge_feed, rotary?, cut_length, cut_minutes}], totals, checks, warnings}, warnings?, programs?: \
             [{order, path?, tool, stickout?, operations, lines, cut_minutes}]. Without dry_run or allow_unsafe a failed check refuses \
             and nothing is written",
        ),
    CommandSpec::new("cam.post.list", "List CAM Posts", crate::checked!("cam.post.list", post_list))
        .icon("list")
        .noundo()
        .doc_only()
        .contract(&POST_LIST_CONTRACT)
        .output(output_schema_for::<CamPostListReply>)
        .params("→ posts: [{name, description, aliases}], the built-in posts a job's `post` names"),
    CommandSpec::new("cam.post.get", "Get CAM Post", crate::checked!("cam.post.get", post_get))
        .icon("inspect")
        .noundo()
        .doc_only()
        .contract(&POST_GET_CONTRACT)
        .output(output_schema_for::<CamPostGetReply>)
        .params("name: a built-in post or one of its other names → {name, post: its Post5 JSON, to copy into a post file for a job's post_path}"),
    CommandSpec::new("cam.gcode.check", "Check G-code", crate::checked!("cam.gcode.check", gcode_check))
        .icon("inspect")
        .noundo()
        .doc_only()
        .contract(&GCODE_CHECK_CONTRACT)
        .output(output_schema_for::<CamGcodeCheckReply>)
        .params(
            "path: G-code file; rapid_rate?: mm/min (to time the rapids) → lines, motion_blocks, rapid_length, feed_length, cut_minutes, \
             rapid_minutes?, arcs, g93_blocks, rotary_ranges: [{letter, min_deg, max_deg}], bounds?: [[x,y,z] low, [x,y,z] high], \
             errors: [{line, message}] (the first line the reader cannot read; it reads the G-code SolidCraft's posts write, others on a \
             best-effort basis)",
        ),
    CommandSpec::new("cam.simulate", "Simulate G-code", crate::checked!("cam.simulate", sim::simulate))
        .noundo()
        .icon("play")
        .contract(&sim::CONTRACT)
        .output(output_schema_for::<crate::schema::cam_sim::CamSimulateReply>)
        .params(
            "path: G-code file; then?: [G-code files] run after it, in order, on the stock it leaves (a job written one program per \
             tool); job: {…} | job_path: .json (the job it was written for: machine, tools, bodies, origin, tolerance); resolution?: \
             cell mm (default 5 × tolerance, coarser to fit a large stock or a long program); stock_stl?: STL path for the stock left \
             → stock: {kind: box | cylinder, min?, max?, x_range?, radius?}, resolution, cells, gouge: {worst, cells, at?}, \
             rapids_in_material: [{line, volume, depth}], leftover: {max, mean, cells, at?}, part_left, verdict, removed_volume, \
             minutes, slot_moves, air_moves, air_length, passed, failures, warnings, notes, stock_stl?. The stock is taken from the \
             part and swept by every block (3-axis: a heightfield; 4th axis: radii over x and A) and compared with the part; a \
             failure is reported, not refused",
        ),
];

/// Largest job, post or tool library file read (bytes).
const MAX_JSON_BYTES: usize = 16 << 20;
/// Largest G-code file `cam.gcode.check` reads (bytes).
const MAX_GCODE_BYTES: usize = 256 << 20;
/// Most operations in a job.
const MAX_OPERATIONS: usize = 200;
/// Most tools in a job.
const MAX_TOOLS: usize = 1000;
/// Most bodies named in a job.
const MAX_BODY_NAMES: usize = 1000;
/// Most points of a polygon.
const MAX_POINTS: usize = 100_000;
/// Largest coordinate accepted (mm).
const MAX_COORD: f64 = 1.0e6;
/// Largest angle accepted (degrees).
const MAX_DEGREES: f64 = 1.0e5;
/// The job's tolerance range (mm).
const TOLERANCE_RANGE: (f64, f64) = (1e-4, 1.0);
/// The share of the job's tolerance the bodies are tessellated within for the 3D operations. The
/// facets of a convex surface are inside it and those of a hollow one outside, by up to that.
const CHORD_SHARE: f64 = 0.25;
/// The share of a 3D operation's part of the tolerance its moves may move by when they are
/// merged into fewer (lines and arcs); the rest is the operation's own.
const COMPACT_SHARE: f64 = 0.2;
/// The share of the gouge check's allowance arcs are followed within for the checks.
const CHECK_ARC_SHARE: f64 = 0.05;
/// Most program lines looked through for the line one move ends on.
const LINE_WINDOW: usize = 20_000;
/// The coarsest the preview draws the part within, when the job's tolerance is finer (mm): the
/// picture needs no more.
const PREVIEW_CHORD: f64 = 0.05;
/// Most triangles and poses each scene of the preview carries.
const PREVIEW_TRIANGLES: usize = 60_000;
const PREVIEW_POSES: usize = 40_000;
/// Lines of the program the reply and the preview show.
const HEAD_LINES: usize = 60;
const PREVIEW_GCODE_LINES: usize = 200;

/// The viewer page; `/*DATA*/null` is replaced with the scenes.
const PREVIEW_TEMPLATE: &str = include_str!("cam_preview.html");

/// The built-in posts: the name a job gives, a description, other names, and the post.
const POSTS: &[(&str, &str, &[&str], fn() -> Post5)] = &[
    (
        "linuxcnc_4axis",
        "LinuxCNC, 3-axis or with a 4th axis: RS-274/NGC with incremental arc centres, the work offset in the header, inverse time for \
         cuts that turn A; .ngc files",
        &["linuxcnc", "linuxcnc-4axis"],
        Post5::linuxcnc_4axis,
    ),
    (
        "grblhal_4axis",
        "grblHAL, 3-axis or with a 4th axis: ; comments, no tool length offsets, a manual tool change (M0 pause), inverse time for cuts \
         that turn A; .nc files",
        &["grblhal", "grblhal-4axis"],
        Post5::grblhal_4axis,
    ),
];

/// The built-in post called `name` (a job's name or an alias, in any case) and its job name.
fn builtin_post(name: &str) -> Option<(&'static str, Post5)> {
    let n = name.trim().to_ascii_lowercase();
    POSTS.iter().find(|(id, _, aliases, _)| *id == n || aliases.contains(&n.as_str())).map(|(id, _, _, make)| (*id, make()))
}

fn post_names() -> String {
    POSTS.iter().map(|(id, ..)| *id).collect::<Vec<_>>().join(", ")
}

/// `p` read into `T`, a failure naming the key path where it went wrong.
fn parse<T: DeserializeOwned>(cmd: &str, p: &Value, what: &str) -> Result<T> {
    let empty = json!({});
    let v = if p.is_null() { &empty } else { p };
    serde_path_to_error::deserialize(v).map_err(|e| {
        let path = e.path().to_string();
        let at = if path == "." {
            what.to_string()
        } else if what.is_empty() {
            format!("`{path}`")
        } else {
            format!("{what} `{path}`")
        };
        if at.is_empty() { bad(cmd, e.into_inner().to_string()) } else { bad(cmd, format!("{at}: {}", e.into_inner())) }
    })
}

fn cam_err(cmd: &str, what: &str, e: CamError) -> EngineError {
    if what.is_empty() { bad(cmd, e.to_string()) } else { bad(cmd, format!("{what}: {e}")) }
}

/// Refuses keys of `given` that `sample` (a value with every field set) does not have, at any
/// depth: the CAM crate's types read unknown keys as nothing, so a misspelt key would otherwise
/// be a setting silently left at its default. An array's items are held against the sample's
/// first item.
fn known_keys(given: &Value, sample: &Value, at: &str) -> std::result::Result<(), String> {
    match (given, sample) {
        (Value::Object(g), Value::Object(s)) => {
            for (k, v) in g {
                let Some(sv) = s.get(k) else {
                    let known: Vec<&str> = s.keys().map(String::as_str).collect();
                    return Err(format!("{at}: unknown key `{k}` (known: {})", known.join(", ")));
                };
                known_keys(v, sv, &format!("{at}.{k}"))?;
            }
            Ok(())
        }
        (Value::Array(g), Value::Array(s)) => match s.first() {
            Some(first) => g.iter().enumerate().try_for_each(|(i, v)| known_keys(v, first, &format!("{at}[{i}]"))),
            None => Ok(()),
        },
        _ => Ok(()),
    }
}

/// A tool with every field set, for [`known_keys`].
fn tool_sample() -> Value {
    let tool = Tool {
        description: "sample".into(),
        spindle_rpm: Some(1.0),
        feed: Some(1.0),
        plunge_feed: Some(1.0),
        stepdown: Some(1.0),
        stepover: Some(1.0),
        corner_radius: Some(1.0),
        tip_diameter: Some(1.0),
        angle_deg: Some(1.0),
        shank_diameter: Some(1.0),
        stickout: Some(1.0),
        holder: vec![HolderSection { diameter: 1.0, length: 1.0 }],
        point_angle_deg: Some(118.0),
        thread_pitch: Some(1.0),
        tooth_height: Some(1.0),
        ..Tool::new(1, ToolKind::BullNose, 1.0, 1.0)
    };
    serde_json::to_value(tool).unwrap_or(Value::Null)
}

/// A machine with every field set, for [`known_keys`].
fn machine_sample() -> Value {
    let axis = RotaryAxis::new(AxisLetter::A, Vec3::X, Vec3::ZERO, Carrier::Table, Some((-1.0, 1.0))).reversed(true);
    let machine = Machine { name: "sample".into(), rotaries: vec![axis], linear_limits: Some((Vec3::ZERO, Vec3::ZERO)), work_origin: Vec3::ZERO };
    serde_json::to_value(machine).unwrap_or(Value::Null)
}

/// A post with every field set, for [`known_keys`].
fn post_sample() -> Value {
    let mut post = Post5::linuxcnc_4axis();
    post.clearance = Some(1.0);
    post.safe_rotate = vec![String::new()];
    post.base.line_numbers = Some(LineNumbers { start: 1, step: 1 });
    post.base.upper_case_comments = true;
    post.base.comment_chars = Some(String::new());
    post.base.tool_list = true;
    post.base.operation_name_only = true;
    post.base.blank_lines = true;
    post.base.operation_start = vec![String::new()];
    post.base.operation_end = vec![String::new()];
    post.base.safe_retract = Some(SafeRetract { lines: vec![String::new()], at_start: true, at_tool_change: true, at_end: true });
    post.base.tool_length_on_first_z = true;
    post.base.modal_motion = true;
    post.safe_rotate_first = true;
    post.coolant_after_turn = true;
    post.inverse_time_decimals = Some(4);
    post.rewind = Some(Rewind { distance: 1.0, retract_feed: 1.0, plunge_feed: 1.0 });
    serde_json::to_value(post).unwrap_or(Value::Null)
}

/// `value` as a `T`, its keys held against `sample` first.
fn crate_value<T: DeserializeOwned>(cmd: &str, value: Value, sample: &Value, what: &str) -> Result<T> {
    known_keys(&value, sample, what).map_err(|e| bad(cmd, e))?;
    serde_path_to_error::deserialize(&value).map_err(|e| {
        let path = e.path().to_string();
        if path == "." { bad(cmd, format!("{what}: {}", e.into_inner())) } else { bad(cmd, format!("{what}.{path}: {}", e.into_inner())) }
    })
}

fn read_json(cmd: &str, path: &str, what: &str) -> Result<Value> {
    let bytes = solidcraft_io::vfs::read(path, MAX_JSON_BYTES).map_err(|e| bad(cmd, format!("{what} {path}: {e}")))?;
    serde_json::from_slice(&bytes).map_err(|e| bad(cmd, format!("{what} {path}: not valid JSON: {e}")))
}

/// `path` read from `base` (the job file's folder) when it is relative and the job came from a file.
fn beside(base: Option<&Path>, path: &str) -> String {
    match base {
        Some(b) if Path::new(path).is_relative() => b.join(path).to_string_lossy().to_string(),
        _ => path.to_string(),
    }
}

fn finite_point(v: &[f64], what: &str) -> std::result::Result<(), String> {
    if v.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD) { Ok(()) } else { Err(format!("{what} must be finite numbers within 1000 m")) }
}

fn radians(deg: f64, what: &str) -> std::result::Result<f64, String> {
    if deg.is_finite() && deg.abs() <= MAX_DEGREES { Ok(deg.to_radians()) } else { Err(format!("{what} must be a finite angle in degrees")) }
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// How a job places the design: turned about `origin` by its `orientation`, then moved so
/// `origin` is work zero. The turned design is still in design units and place, so everything
/// that takes `origin` off a design point works on [`Placement::turn`] of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Placement {
    pub origin: Vec3,
    /// The work axes' directions in the design, when the part is turned.
    pub axes: Option<[Vec3; 3]>,
}

impl Placement {
    /// The design point `p` turned about `origin`.
    pub fn turn(&self, p: Vec3) -> Vec3 {
        self.origin + self.turn_dir(p - self.origin)
    }

    /// The design direction `d` turned.
    pub fn turn_dir(&self, d: Vec3) -> Vec3 {
        match self.axes {
            None => d,
            Some([x, y, z]) => Vec3::new(d.dot(x), d.dot(y), d.dot(z)),
        }
    }

    /// The design point `p` in work coordinates.
    pub fn in_work(&self, p: Vec3) -> Vec3 {
        self.turn(p) - self.origin
    }
}

/// The placement of work zero `origin` (design coordinates) and the turn `orientation`, checked.
fn placement(cmd: &str, s: &Session, origin: [f64; 3], orientation: Option<&crate::schema::cam::OrientationSpec>) -> Result<Placement> {
    use crate::schema::cam::OrientationSpec;
    finite_point(&origin, "`origin`").map_err(|e| bad(cmd, e))?;
    let origin = v3(origin);
    let unit = |v: Vec3, what: &str| -> Result<Vec3> {
        let len = v.dot(v).sqrt();
        if v.x.is_finite() && v.y.is_finite() && v.z.is_finite() && len > 1e-12 {
            Ok(Vec3::new(v.x / len, v.y / len, v.z / len))
        } else {
            Err(bad(cmd, format!("{what} must be a finite, non-zero direction")))
        }
    };
    let axes = match orientation {
        None => None,
        Some(OrientationSpec::Named(n)) => match n.as_str() {
            "flip_x" => Some([Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 0.0, -1.0)]),
            "flip_y" => Some([Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, -1.0)]),
            "none" => None,
            other => {
                return Err(bad(
                    cmd,
                    format!("`orientation` is \"flip_x\", \"flip_y\", {{\"axis\", \"angle_deg\"}} or {{\"sketch\"}}, not \"{other}\""),
                ));
            }
        },
        Some(OrientationSpec::Turn { axis, angle_deg }) => {
            finite_point(axis, "`orientation.axis`").map_err(|e| bad(cmd, e))?;
            let a = unit(v3(*axis), "`orientation.axis`")?;
            if !(angle_deg.is_finite() && angle_deg.abs() <= 3600.0) {
                return Err(bad(cmd, "`orientation.angle_deg` must be a finite angle"));
            }
            let t = -angle_deg.to_radians();
            // The work axes in the design: the design's own axes turned back.
            Some([Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)].map(|e| solidcraft_cam::rotate(e, a, t)))
        }
        Some(OrientationSpec::Sketch { sketch }) => {
            let id = match s.doc.find_feature(sketch) {
                Some(f) if matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. }) => f.id,
                _ => return Err(bad(cmd, format!("`orientation`: no sketch `{sketch}`"))),
            };
            let st = s.model.state();
            let Some(ss) = st.sketch(id) else { return Err(bad(cmd, format!("`orientation`: sketch `{sketch}` has not been solved"))) };
            let (x, y) = (ss.plane.x, ss.plane.y);
            let z = unit(x.cross(y), "the sketch's normal")?;
            let x = unit(x, "the sketch's X")?;
            Some([x, z.cross(x), z])
        }
    };
    Ok(Placement { origin, axes })
}

/// The job's machine: `machine`, or the file `machine_path` (read from `base`, the job file's
/// folder, when relative).
fn machine_of(cmd: &str, job: &CamJob, base: Option<&Path>) -> Result<Machine> {
    let a_options = job.a_reversed.is_some() || job.a_limits_deg.is_some();
    let file;
    let spec = match (&job.machine, &job.machine_path) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give the job `machine` or `machine_path`, not both")),
        (None, None) => return Err(bad(cmd, "the job needs a `machine` (`3axis`, `4axis_a` or Machine JSON) or a `machine_path`")),
        (Some(m), None) => m,
        (None, Some(path)) => {
            let path = beside(base, path);
            match read_json(cmd, &path, "machine_path")? {
                Value::Object(m) => {
                    file = MachineSpec::Custom(m);
                    &file
                }
                _ => return Err(bad(cmd, format!("machine_path {path}: a machine file holds the CAM crate's Machine JSON object"))),
            }
        }
    };
    let machine = match spec {
        MachineSpec::Preset(name) => match name.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "3axis" | "3_axis" | "three_axis" => {
                if a_options {
                    return Err(bad(cmd, "`a_reversed` and `a_limits_deg` are for the `4axis_a` machine"));
                }
                Machine::three_axis()
            }
            "4axis_a" | "4axis" | "4_axis_a" | "four_axis_a" => {
                let limits = match job.a_limits_deg {
                    None => None,
                    Some([lo, hi]) => {
                        let lo = radians(lo, "`a_limits_deg`").map_err(|e| bad(cmd, e))?;
                        let hi = radians(hi, "`a_limits_deg`").map_err(|e| bad(cmd, e))?;
                        if !(lo < hi) {
                            return Err(bad(cmd, "`a_limits_deg` must be [lowest, highest], the lowest first"));
                        }
                        Some((lo, hi))
                    }
                };
                Machine::four_axis_a_with(Vec3::ZERO, job.a_reversed.unwrap_or(false), limits)
            }
            other => return Err(bad(cmd, format!("unknown machine `{other}`: `3axis`, `4axis_a` or a Machine JSON object"))),
        },
        MachineSpec::Custom(m) => {
            if a_options {
                return Err(bad(cmd, "`a_reversed` and `a_limits_deg` are for the `4axis_a` machine; set them in the machine's JSON"));
            }
            crate_value(cmd, Value::Object(m.clone()), &machine_sample(), "`machine`")?
        }
    };
    machine.validate().map_err(|e| cam_err(cmd, "`machine`", e))?;
    Ok(machine)
}

/// The job's post and its name.
fn post_of(cmd: &str, job: &CamJob, base: Option<&Path>) -> Result<(String, Post5)> {
    let post = match (&job.post, &job.post_path) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give the job `post` or `post_path`, not both")),
        (None, None) => return Err(bad(cmd, format!("the job needs a `post` ({}) or a `post_path`", post_names()))),
        (Some(name), None) => match builtin_post(name) {
            Some((id, post)) => (id.to_string(), post),
            None => return Err(bad(cmd, format!("unknown post `{name}` (built in: {})", post_names()))),
        },
        (None, Some(path)) => {
            let path = beside(base, path);
            let post: Post5 = crate_value(cmd, read_json(cmd, &path, "post_path")?, &post_sample(), "the post")?;
            (post.base.name.clone(), post)
        }
    };
    post.1.validate().map_err(|e| cam_err(cmd, "the post", e))?;
    Ok(post)
}

/// The job's tools, each checked.
fn tools_of(cmd: &str, job: &CamJob, base: Option<&Path>) -> Result<Vec<Tool>> {
    let raw: Vec<Value> = match (&job.tools, &job.tool_library_path) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give the job `tools` or `tool_library_path`, not both")),
        (None, None) => return Err(bad(cmd, "the job needs `tools` or a `tool_library_path`")),
        (Some(t), None) => t.iter().cloned().map(Value::Object).collect(),
        (None, Some(path)) => match read_json(cmd, &beside(base, path), "tool_library_path")? {
            Value::Array(a) => a,
            _ => return Err(bad(cmd, "a tool library file holds a JSON array of tools")),
        },
    };
    if raw.is_empty() || raw.len() > MAX_TOOLS {
        return Err(bad(cmd, format!("a job has 1 to {MAX_TOOLS} tools")));
    }
    let sample = tool_sample();
    let mut tools: Vec<Tool> = Vec::with_capacity(raw.len());
    for (i, v) in raw.into_iter().enumerate() {
        let what = format!("`tools[{i}]`");
        let tool: Tool = crate_value(cmd, v, &sample, &what)?;
        tool.validate().map_err(|e| cam_err(cmd, &what, e))?;
        if tools.iter().any(|t| t.number == tool.number) {
            return Err(bad(cmd, format!("two tools are number {}", tool.number)));
        }
        tools.push(tool);
    }
    Ok(tools)
}

/// The part's bodies (the job's `bodies`, else every visible body not in `fixtures`) and the
/// fixtures'. Each must be in the design, and none in both.
fn job_bodies(cmd: &str, s: &Session, job: &CamJob) -> Result<(Vec<String>, Vec<String>)> {
    let st = s.world_state()?;
    let fixtures: Vec<String> = job.fixtures.clone().unwrap_or_default();
    let part: Vec<String> = match &job.bodies {
        Some(b) => b.clone(),
        None => st.bodies.iter().map(|b| b.name.clone()).filter(|n| !fixtures.contains(n) && s.body_visible(n)).collect(),
    };
    if part.len() > MAX_BODY_NAMES || fixtures.len() > MAX_BODY_NAMES {
        return Err(bad(cmd, format!("a job names at most {MAX_BODY_NAMES} bodies")));
    }
    if let Some(missing) = part.iter().chain(&fixtures).find(|n| !st.bodies.iter().any(|b| &b.name == *n)) {
        return Err(bad(cmd, format!("no body `{missing}`")));
    }
    if let Some(both) = part.iter().find(|n| fixtures.contains(n)) {
        return Err(bad(cmd, format!("body `{both}` is in both `bodies` and `fixtures`")));
    }
    if job.bodies.is_some() && part.is_empty() {
        return Err(bad(cmd, "`bodies` names no body: leave it out to take every visible body"));
    }
    Ok((part, fixtures))
}

/// The bodies `names` tessellated from their exact surfaces within `chord` into one mesh, in
/// work coordinates (the design less `origin`): what the 3D operations follow and every
/// operation is checked for gouges against. The toolpaths are output, planned within the job's
/// tolerance as an export is; neither they nor the gouge check are yet on the exact surfaces.
fn bodies_mesh(cmd: &str, st: &solidcraft_doc::ModelState, names: &[String], place: &Placement, chord: f64, notes: &mut Vec<String>) -> Result<Mesh> {
    let mut out = Mesh::default();
    for name in names {
        let Some(b) = st.bodies.iter().find(|b| &b.name == name) else { return Err(bad(cmd, format!("no body `{name}`"))) };
        if b.body.is_mesh() {
            notes.push(format!("body `{name}` is a mesh: its own triangles are used, which may be coarser than the job's tolerance"));
        }
        let mesh = b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(chord))?;
        let Ok(start) = u32::try_from(out.positions.len()) else { return Err(bad(cmd, "the part has too many triangles")) };
        let smooth = mesh.normals.len() == mesh.positions.len();
        out.positions.extend(mesh.positions.iter().map(|p| place.in_work(*p)));
        if smooth {
            out.normals.extend(mesh.normals.iter().map(|n| place.turn_dir(*n)));
        } else {
            // Not directions: the surface takes each triangle's own normal instead.
            out.normals.extend(std::iter::repeat_n(Vec3::ZERO, mesh.positions.len()));
        }
        for t in &mesh.triangles {
            let [Some(a), Some(b), Some(c)] = t.map(|i| i.checked_add(start)) else { return Err(bad(cmd, "the part has too many triangles")) };
            out.triangles.push([a, b, c]);
        }
        if out.triangles.len() > solidcraft_cam::surface_ops::MAX_SAMPLES {
            return Err(bad(cmd, "the part has too many triangles at this tolerance: raise the job's `tolerance`"));
        }
    }
    Ok(out)
}

/// The part, the fixtures and the two together, in work coordinates.
struct Surfaces {
    part: Surface,
    /// The fixtures alone (`None` without any), which an operation that skips the gouge check
    /// against the part is still checked against.
    fixtures: Option<Surface>,
    all: Surface,
}

/// The [`Surfaces`] of the bodies `part` and `fixtures`, tessellated within `chord`.
fn surfaces_of(
    cmd: &str,
    s: &Session,
    part: &[String],
    fixtures: &[String],
    place: &Placement,
    chord: f64,
    notes: &mut Vec<String>,
) -> Result<Surfaces> {
    if part.is_empty() {
        return Err(bad(
            cmd,
            "the job's operations need a part to plan on and to check for gouges: the design has no visible body, or give `bodies`",
        ));
    }
    let st = s.world_state()?;
    let mut both = bodies_mesh(cmd, &st, part, place, chord, notes)?;
    let part = Surface::from_mesh(&both).map_err(|e| cam_err(cmd, "the part", e))?;
    if part.is_empty() {
        return Err(bad(cmd, "the part's bodies have no surface to machine"));
    }
    let (all, fixtures) = if fixtures.is_empty() {
        (part.clone(), None)
    } else {
        let fix = bodies_mesh(cmd, &st, fixtures, place, chord, notes)?;
        let fixture_surface = Surface::from_mesh(&fix).map_err(|e| cam_err(cmd, "the fixtures", e))?;
        let Ok(start) = u32::try_from(both.positions.len()) else { return Err(bad(cmd, "the part has too many triangles")) };
        both.positions.extend(fix.positions);
        both.normals.extend(fix.normals);
        for t in fix.triangles {
            let [Some(a), Some(b), Some(c)] = t.map(|i| i.checked_add(start)) else { return Err(bad(cmd, "the part has too many triangles")) };
            both.triangles.push([a, b, c]);
        }
        (Surface::from_mesh(&both).map_err(|e| cam_err(cmd, "the fixtures", e))?, (!fixture_surface.is_empty()).then_some(fixture_surface))
    };
    notes.push(format!(
        "bodies tessellated from their exact surfaces within {} mm (a quarter of the job's tolerance), in work coordinates (the design \
         less origin [{}, {}, {}])",
        num(chord),
        num(place.origin.x),
        num(place.origin.y),
        num(place.origin.z)
    ));
    if place.axes.is_some() {
        notes.push("the design is turned by the job's `orientation` about its origin before it is machined".into());
    }
    Ok(Surfaces { part, fixtures, all })
}

/// The part's triangles for the preview page only, in work coordinates (the design less
/// `origin`), each as nine numbers: the bodies tessellated within the job's tolerance or
/// [`PREVIEW_CHORD`], whichever is coarser. Nothing is decided with them.
fn preview_part(cmd: &str, s: &Session, names: &[String], place: &Placement, tol: f64) -> Result<Vec<f64>> {
    let st = s.world_state()?;
    let tolerance = solidcraft_kernel::MeshTolerance::chord(tol.max(PREVIEW_CHORD));
    let mut out: Vec<[Vec3; 3]> = Vec::new();
    for name in names {
        let Some(b) = st.bodies.iter().find(|b| &b.name == name) else { return Err(bad(cmd, format!("no body `{name}`"))) };
        let mesh = b.body.mesh(&tolerance)?;
        for t in &mesh.triangles {
            let corner = |i: u32| mesh.positions.get(i as usize).map(|p| place.in_work(*p));
            if let (Some(a), Some(b), Some(c)) = (corner(t[0]), corner(t[1]), corner(t[2])) {
                out.push([a, b, c]);
            }
        }
    }
    Ok(thin(&out, PREVIEW_TRIANGLES).iter().flat_map(|t| t.iter().flat_map(|p| [round(p.x, 100.0), round(p.y, 100.0), round(p.z, 100.0)])).collect())
}

/// A number as the notes and messages show it.
fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// The closed polygon `pts` as a region, counter-clockwise.
fn polygon_region(cmd: &str, what: &str, pts: &[[f64; 2]]) -> Result<Region2> {
    if pts.len() > MAX_POINTS {
        return Err(bad(cmd, format!("{what} has more than {MAX_POINTS} points")));
    }
    let mut v: Vec<Vec2> = Vec::with_capacity(pts.len());
    for p in pts {
        finite_point(p, what).map_err(|e| bad(cmd, e))?;
        let q = Vec2::new(p[0], p[1]);
        if v.last().is_none_or(|l| l.dist(q) > 1e-9) {
            v.push(q);
        }
    }
    if v.len() > 1 && v.first().zip(v.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-9) {
        v.pop();
    }
    if v.len() < 3 {
        return Err(bad(cmd, format!("{what} needs at least 3 distinct points")));
    }
    let area: f64 = v.iter().zip(v.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum::<f64>() * 0.5;
    if !(area.abs() > 1e-9) {
        return Err(bad(cmd, format!("{what} encloses no area")));
    }
    if area < 0.0 {
        v.reverse();
    }
    Ok(Region2 { outer: Loop2::polygon(&v), holes: Vec::new() })
}

/// The closed profiles of sketch `key` in the coordinates of `frame` (inside work coordinates,
/// the design less `origin`): the sketch's plane must be square to the frame's Z, facing it or
/// facing away.
fn sketch_regions(cmd: &str, s: &Session, key: &str, profiles: Option<&[usize]>, place: &Placement, frame: &Frame) -> Result<Vec<Region2>> {
    let id = match s.doc.find_feature(key) {
        Some(f) if matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. }) => f.id,
        _ => return Err(bad(cmd, format!("no sketch `{key}`"))),
    };
    let st = s.model.state();
    let Some(ss) = st.sketch(id) else { return Err(bad(cmd, format!("sketch `{key}` has not been solved"))) };
    let sel = match profiles {
        None => solidcraft_doc::ProfileSel::All,
        Some(ix) => solidcraft_doc::ProfileSel::Indices { indices: ix.to_vec() },
    };
    let regions = solidcraft_doc::select_profiles(ss, &sel)?;
    let plane = &ss.plane;
    let normal = frame.dir_to_local(place.turn_dir(plane.x.cross(plane.y)));
    // A sketch facing away from the tool (drawn on a plane whose normal points down, or op1's
    // outline on the top of a part op2 has turned over) is read as it is seen from the tool:
    // its profiles mirrored.
    let mirrored = normal.z < -(1.0 - 1e-6);
    if !(normal.z > 1.0 - 1e-6 || mirrored) {
        return Err(bad(
            cmd,
            format!("sketch `{key}` is not on a plane square to the operation's tool axis (draw it on a plane facing +Z or -Z of the operation)"),
        ));
    }
    let lx = frame.dir_to_local(place.turn_dir(plane.x));
    let turn = lx.y.atan2(lx.x);
    let map = |q: Vec2| {
        let l = frame.to_local(place.in_work(plane.origin + plane.x * q.x + plane.y * q.y));
        Vec2::new(l.x, l.y)
    };
    // The sketch's X goes to angle `turn`; mirrored, an angle `a` from its X goes to `turn - a`
    // and an arc runs the other way.
    let arc = |start: f64, sweep: f64| if mirrored { (turn - start, -sweep) } else { (start + turn, sweep) };
    let seg = |s: &Seg2| match *s {
        Seg2::Line { a, b } => Seg2::Line { a: map(a), b: map(b) },
        Seg2::Arc { center, radius, start, sweep } => {
            let (start, sweep) = arc(start, sweep);
            Seg2::Arc { center: map(center), radius, start, sweep }
        }
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: map(p0), p1: map(p1), p2: map(p2), p3: map(p3) },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: map(a), apex: map(apex), b: map(b), w },
        Seg2::Bezier { n, p } => Seg2::Bezier { n, p: p.map(map) },
    };
    // Mirroring turns a loop's direction round: turned back, an outline stays counter-clockwise
    // and its holes clockwise, as the sketch has them.
    let lp = |l: &Loop2| {
        let out = Loop2 { segs: l.segs.iter().map(seg).collect() };
        if mirrored { out.reversed() } else { out }
    };
    Ok(regions.iter().map(|r| Region2 { outer: lp(&r.outer), holes: r.holes.iter().map(lp).collect() }).collect())
}

/// What every operation needs, resolved.
struct Resolved {
    tool: Tool,
    rpm: f64,
    feeds: Feeds,
    /// `feeds.descent`, for the 3D finishes.
    descent: Option<f64>,
}

/// Everything an operation is planned with.
struct Ctx<'a> {
    cmd: &'a str,
    s: &'a Session,
    machine: &'a Machine,
    tools: &'a [Tool],
    /// The design's placement: work zero and the job's `orientation`.
    place: Placement,
    /// The job's stock, in work coordinates.
    stock: Option<&'a Stock>,
    tol: f64,
    post: &'a Post5,
    /// The part as the 3D operations follow it, in work coordinates (when one of them is on).
    part: Option<&'a Surface>,
    /// The chord tolerance the part was tessellated within (mm).
    chord: f64,
    /// How closely arcs are followed into the poses the checks run along (mm).
    pose_tol: f64,
    /// The part and the fixtures, for what the tool must clear coming down (a drill's R height).
    all: Option<&'a Surface>,
    /// The `type` of the operation being planned (an indexed one's inner operation's): which
    /// tools it cuts with.
    op_type: std::cell::Cell<&'static str>,
    /// The band a chamfer mill is told to cut into what the model shows (a chamfer, a V-trace,
    /// a spotted hole), handed from its planning to [`Planned::band`].
    band: std::cell::Cell<Option<f64>>,
    /// What an adaptive operation adds to its report, handed from its planning to the report.
    adaptive: std::cell::RefCell<Option<(crate::schema::cam_adaptive::AdaptiveReport, Vec<String>)>>,
    /// The part's machinable features, found when an operation names one (`cam_features.rs`).
    features: &'a features::Lazy,
}

impl<'a> Ctx<'a> {
    fn bad(&self, op: &str, why: impl std::fmt::Display) -> EngineError {
        bad(self.cmd, format!("operation `{op}`: {why}"))
    }

    /// The operation's tool, spindle speed and feeds: its own, else the tool's.
    fn resolve(&self, c: &Common<'_>) -> Result<Resolved> {
        let op = c.name;
        let Some(tool) = self.tools.iter().find(|t| t.number == c.tool) else {
            let numbers: Vec<String> = self.tools.iter().map(|t| t.number.to_string()).collect();
            return Err(self.bad(op, format!("no tool number {} (the job's tools: {})", c.tool, numbers.join(", "))));
        };
        let ty = self.op_type.get();
        if let Some(takes) = takes_kind(ty, tool.kind) {
            return Err(self.bad(op, format!("tool {} is a {}: a {} cuts with {takes}", tool.number, tool.kind.label(), ty.replace('_', " "))));
        }
        let rpm = match c.spindle_rpm.or(tool.spindle_rpm) {
            Some(r) if r > 0.0 && r <= 1.0e6 => r,
            Some(_) => return Err(self.bad(op, "the spindle speed must be above 0 and at most 1000000 rev/min")),
            None => return Err(self.bad(op, format!("give `spindle_rpm`, or tool {} a `spindle_rpm`", tool.number))),
        };
        let feeds = match c.feeds {
            Some(f) => Feeds { cut: f.cut, plunge: f.plunge },
            None => match (tool.feed, tool.plunge_feed) {
                (Some(cut), Some(plunge)) => Feeds { cut, plunge },
                _ => return Err(self.bad(op, format!("give `feeds: {{cut, plunge}}`, or tool {} a `feed` and a `plunge_feed`", tool.number))),
            },
        };
        feeds.validate().map_err(|e| self.bad(op, e))?;
        let descent = match c.feeds.and_then(|f| f.descent) {
            Some(d) if !(d > 0.0 && d <= 1.0e6) => return Err(self.bad(op, "the descent feed must be above 0 and at most 1000000 mm/min")),
            d => d,
        };
        Ok(Resolved { tool: tool.clone(), rpm, feeds, descent })
    }

    fn tolerance(&self, op: &str, t: Option<f64>) -> Result<f64> {
        match t {
            None => Ok(self.tol),
            Some(t) if t.is_finite() && (TOLERANCE_RANGE.0..=TOLERANCE_RANGE.1).contains(&t) => Ok(t),
            Some(_) => Err(self.bad(op, "the tolerance must be from 0.0001 to 1 mm")),
        }
    }

    /// What a 3D operation planned on the tessellated part may take of its tolerance `t` (the
    /// job's by default): the tolerance less the tessellation's chord error, at least half of it.
    fn surface_budget(&self, op: &str, t: Option<f64>) -> Result<f64> {
        let t = self.tolerance(op, t)?;
        Ok((t - self.chord).max(0.5 * t))
    }

    fn degrees(&self, op: &str, deg: f64, what: &str) -> Result<f64> {
        radians(deg, what).map_err(|e| self.bad(op, e))
    }

    /// The part in `frame`'s coordinates, for a 3D operation.
    fn surface_in(&self, op: &str, frame: &Frame) -> Result<std::borrow::Cow<'a, Surface>> {
        let Some(part) = self.part else { return Err(self.bad(op, "a 3D operation needs the part's bodies")) };
        if *frame == Frame::IDENTITY {
            return Ok(std::borrow::Cow::Borrowed(part));
        }
        part.in_frame(frame).map(std::borrow::Cow::Owned).map_err(|e| self.bad(op, e))
    }

    /// The part and the fixtures in `frame`'s coordinates.
    fn all_in(&self, op: &str, frame: &Frame) -> Result<std::borrow::Cow<'a, Surface>> {
        let Some(all) = self.all else { return Err(self.bad(op, "the operation needs the part's bodies")) };
        if *frame == Frame::IDENTITY {
            return Ok(std::borrow::Cow::Borrowed(all));
        }
        all.in_frame(frame).map(std::borrow::Cow::Owned).map_err(|e| self.bad(op, e))
    }

    /// Where a 3D operation's cutter centre may go.
    fn boundary(&self, op: &str, b: Option<&BoundarySpec>) -> Result<Boundary> {
        let Some(b) = b else { return Ok(Boundary::default()) };
        let rect = match b.rect {
            None => None,
            Some([lo, hi]) => {
                finite_point(&[lo[0], lo[1], hi[0], hi[1]], "`boundary.rect`").map_err(|e| self.bad(op, e))?;
                if !(lo[0] < hi[0] && lo[1] < hi[1]) {
                    return Err(self.bad(op, "`boundary.rect` must be [[low x, low y], [high x, high y]]"));
                }
                Some([Vec2::new(lo[0], lo[1]), Vec2::new(hi[0], hi[1])])
            }
        };
        let mut region = match &b.polygon {
            None => None,
            Some(p) => Some(polygon_region(self.cmd, &format!("operation `{op}`: `boundary.polygon`"), p)?),
        };
        if let Some(holes) = &b.holes {
            let Some(r) = region.as_mut() else { return Err(self.bad(op, "`boundary.holes` goes with `boundary.polygon`")) };
            if holes.len() > MAX_POINTS {
                return Err(self.bad(op, format!("`boundary.holes` has more than {MAX_POINTS} polygons")));
            }
            for (i, h) in holes.iter().enumerate() {
                let hole = polygon_region(self.cmd, &format!("operation `{op}`: `boundary.holes[{i}]`"), h)?;
                r.holes.push(hole.outer);
            }
        }
        Ok(Boundary { rect, region })
    }

    /// The regions of a 2D operation in `frame`'s coordinates.
    fn regions(
        &self,
        op: &str,
        sketch: Option<&crate::schema::SketchRef>,
        profiles: Option<&[usize]>,
        polygon: Option<&[[f64; 2]]>,
        frame: &Frame,
    ) -> Result<Vec<Region2>> {
        match (sketch, polygon) {
            (Some(_), Some(_)) => Err(self.bad(op, "give `sketch` or `polygon`, not both")),
            (None, None) => Err(self.bad(op, "give a `sketch` or a `polygon` to cut")),
            (None, Some(_)) if profiles.is_some() => Err(self.bad(op, "`profiles` goes with `sketch`")),
            (Some(k), None) => sketch_regions(self.cmd, self.s, &k.0, profiles, &self.place, frame).map_err(|e| match e {
                EngineError::BadParams { msg, .. } => self.bad(op, msg),
                other => self.bad(op, other),
            }),
            (None, Some(p)) => polygon_region(self.cmd, &format!("operation `{op}`: `polygon`"), p).map(|r| vec![r]),
        }
    }
}

/// Which tools an operation of type `ty` cuts with, said, when `kind` is not one of them.
fn takes_kind(ty: &str, kind: ToolKind) -> Option<&'static str> {
    use ToolKind::*;
    let (ok, takes) = match ty {
        // A chamfer mill spots holes (to the depth of its cone).
        "drill" => (matches!(kind, Drill | FlatEndmill | Chamfer), "a drill, a flat end mill to plunge with, or a chamfer mill to spot"),
        "chamfer" => (matches!(kind, Chamfer), "a chamfer mill"),
        "thread_mill" => (matches!(kind, ThreadMill), "a thread mill"),
        // A chamfer mill traces a line to a depth (`side: on`).
        "contour" => (matches!(kind, FlatEndmill | BullNose | BallEndmill | FaceMill | Chamfer), "an end mill, a face mill or a chamfer mill"),
        "face" => (matches!(kind, FaceMill | FlatEndmill | BullNose), "a face mill, a flat end mill or a bull nose"),
        "roughing" | "rotary_roughing" | "adaptive_roughing" => {
            (matches!(kind, FlatEndmill | BullNose | BallEndmill), "a flat end mill, a ball end mill or a bull nose")
        }
        _ => (matches!(kind, FlatEndmill | BullNose | BallEndmill | FaceMill), "a flat end mill, a ball end mill, a bull nose or a face mill"),
    };
    (!ok).then_some(takes)
}

/// The `type` of `spec` (an indexed one's inner operation's).
fn type_of(spec: &OperationSpec) -> &'static str {
    match spec {
        OperationSpec::Contour(_) => "contour",
        OperationSpec::Pocket(_) => "pocket",
        OperationSpec::Parallel(_) => "parallel",
        OperationSpec::Roughing(_) => "roughing",
        OperationSpec::Waterline(_) => "waterline",
        OperationSpec::Scallop(_) => "scallop",
        OperationSpec::SteepShallow(_) => "steep_shallow",
        OperationSpec::Pencil(_) => "pencil",
        OperationSpec::RestFinish(_) => "rest_finish",
        OperationSpec::Drill(_) => "drill",
        OperationSpec::Chamfer(_) => "chamfer",
        OperationSpec::Bore(_) => "bore",
        OperationSpec::ThreadMill(_) => "thread_mill",
        OperationSpec::Adaptive(_) => "adaptive",
        OperationSpec::AdaptiveRoughing(_) => "adaptive_roughing",
        OperationSpec::Face(_) => "face",
        OperationSpec::RotaryFinish(_) => "rotary_finish",
        OperationSpec::RotaryRoughing(_) => "rotary_roughing",
        OperationSpec::Wrap(_) => "wrap",
        OperationSpec::WrapPocket(_) => "wrap_pocket",
        OperationSpec::WrapContour(_) => "wrap_contour",
        OperationSpec::Indexed(o) => type_of3(&o.operation),
        OperationSpec::IndexedPattern(o) => type_of3(&o.operation),
    }
}

/// The `type` of the 3-axis operation `op`.
fn type_of3(op: &Op3Spec) -> &'static str {
    match op {
        Op3Spec::Contour(_) => "contour",
        Op3Spec::Pocket(_) => "pocket",
        Op3Spec::Parallel(_) => "parallel",
        Op3Spec::Roughing(_) => "roughing",
        Op3Spec::Waterline(_) => "waterline",
        Op3Spec::Scallop(_) => "scallop",
        Op3Spec::SteepShallow(_) => "steep_shallow",
        Op3Spec::Pencil(_) => "pencil",
        Op3Spec::RestFinish(_) => "rest_finish",
        Op3Spec::Drill(_) => "drill",
        Op3Spec::Chamfer(_) => "chamfer",
        Op3Spec::Adaptive(_) => "adaptive",
        Op3Spec::AdaptiveRoughing(_) => "adaptive_roughing",
    }
}

fn heights(h: &HeightsSpec) -> Heights {
    Heights { clearance: h.clearance, retract: h.retract, top: h.top, bottom: h.bottom }
}

/// The operation's stepdown, stepover or step, or its default from the tool.
fn or_tool(v: Option<f64>, tool: Option<f64>, default: f64) -> f64 {
    v.or(tool).unwrap_or(default)
}

/// The share of a 3D operation's budget it plans within, and the share its moves may then move
/// by when merged (with the share it plans within again).
fn split_budget(budget: f64) -> (f64, Option<(f64, f64)>) {
    let planned = budget * (1.0 - COMPACT_SHARE);
    (planned, Some((budget * COMPACT_SHARE, planned)))
}

/// An operation as planned, its tool, its kind, and for a 3D one the tolerance its moves may be
/// merged within and the one it was planned within.
type Planned3 = (Toolpath, Resolved, &'static str, Option<(f64, f64)>);

/// A 3-axis operation planned in `frame`: a 3D one's runs of moves merged where its tolerance leaves
/// room, and its slowed descents worked out again from the coordinates as the post writes them.
fn plan_path(ctx: &Ctx<'_>, spec: &Op3Spec, frame: &Frame) -> Result<(Toolpath, Resolved, &'static str)> {
    let name = spec.common().name;
    let (path, r, kind, merge) = plan_raw(ctx, spec, frame)?;
    let b = &ctx.post.base;
    let mut path = match merge {
        Some((tolerance, planned)) => {
            // An arc may bow out from the moves it replaces as far as they were allowed to cut
            // across a curve when they were planned.
            let how = Compaction { tolerance: tolerance.min(b.arc_tolerance.max(1e-6)), arcs: b.arcs, helical: b.helical_arcs, bow: planned };
            compact(&path, &how).map_err(|e| ctx.bad(name, e))?
        }
        None => path,
    };
    let feed_step = 10f64.powi(-i32::from(b.feed_decimals)) * b.units.mm();
    // The ways down at rapid stop above where the material may reach, not only above the
    // operation's own top.
    if let Some(top) = material_top(ctx, frame) {
        lift_entries(&mut path, top);
    }
    // A 3D finish's moves down a wall go down at its descent feed, its entries (straight down)
    // at the plunge feed: the larger of the two bounds both.
    let takes_descent =
        matches!(spec, Op3Spec::Parallel(_) | Op3Spec::Scallop(_) | Op3Spec::SteepShallow(_) | Op3Spec::Pencil(_) | Op3Spec::RestFinish(_));
    let down = match r.descent {
        Some(d) if takes_descent => d.max(r.feeds.plunge),
        _ => r.feeds.plunge,
    };
    limit_descents(&mut path, down, r.feeds.cut, b.decimals, b.units.mm(), feed_step).map_err(|e| ctx.bad(name, e))?;
    Ok((path, r, kind))
}

/// Where the material may reach in `frame` (the highest Z of the operation's coordinates): the
/// stock's top seen from there; on a machine with rotaries without a bar stock, the round bar
/// through the part's farthest point from the A axis too; otherwise not known (the operations'
/// heights say).
fn material_top(ctx: &Ctx<'_>, frame: &Frame) -> Option<f64> {
    let highest = |lo: Vec3, hi: Vec3| -> f64 {
        let mut top = f64::NEG_INFINITY;
        for i in 0..8 {
            let c = Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
            top = top.max(frame.to_local(c).z);
        }
        top
    };
    // A round bar about work X: its radius where the frame looks square to X.
    let bar = |radius: f64, x0: f64, x1: f64| -> f64 {
        if frame.z.x.abs() <= 1e-9 {
            radius - frame.origin.dot(frame.z)
        } else {
            highest(Vec3::new(x0, -radius, -radius), Vec3::new(x1, radius, radius))
        }
    };
    // The round bar through the part's farthest point from work X.
    let part_bar = || {
        let (mut reach, mut x0, mut x1) = (0.0f64, f64::INFINITY, f64::NEG_INFINITY);
        for t in ctx.part?.triangles() {
            for p in t {
                reach = reach.max(p.y.hypot(p.z));
                x0 = x0.min(p.x);
                x1 = x1.max(p.x);
            }
        }
        (x0 <= x1).then(|| bar(reach, x0, x1))
    };
    let rotary = !ctx.machine.rotaries.is_empty();
    let top = match ctx.stock {
        Some(Stock::Box { min, max }) if !rotary => highest(*min, *max),
        // On a 4th axis the round bar through the part's farthest point is above a box too.
        Some(Stock::Box { min, max }) => part_bar().map_or(highest(*min, *max), |b| b.max(highest(*min, *max))),
        Some(Stock::Cylinder { radius, x_min, x_max }) => bar(*radius, *x_min, *x_max),
        None if rotary => part_bar()?,
        None => return None,
    };
    top.is_finite().then_some(top)
}

/// A 3-axis operation as planned (see [`Planned3`]).
fn plan_raw(ctx: &Ctx<'_>, spec: &Op3Spec, frame: &Frame) -> Result<Planned3> {
    match spec {
        Op3Spec::Contour(o) => {
            let c = o.common();
            let r = ctx.resolve(&c)?;
            let (leave, tol) = (o.stock_to_leave.unwrap_or(0.0), ctx.tolerance(c.name, o.tolerance)?);
            // On a feature: its outline and heights, and the side its kind takes.
            let two = features::op_cut(
                ctx,
                c.name,
                o.sketch.as_ref(),
                o.profiles.as_deref(),
                o.polygon.as_deref(),
                o.feature.as_deref(),
                o.heights.as_ref(),
                frame,
                None,
            )?;
            let side = match o.side.or(two.side).unwrap_or(SideSpec::Outside) {
                SideSpec::Outside => Side::Outside,
                SideSpec::Inside => Side::Inside,
                SideSpec::On => Side::On,
            };
            let regions = two.regions;
            let cut = Cut { heights: two.heights, feeds: r.feeds, stepdown: or_tool(o.stepdown, r.tool.stepdown, r.tool.diameter * 0.5) };
            let params = ContourParams {
                side,
                climb: o.climb.unwrap_or(true),
                stock_to_leave: leave,
                tolerance: tol,
                ramp_angle: ctx.degrees(c.name, o.ramp_angle_deg.unwrap_or(0.0), "`ramp_angle_deg`")?,
            };
            if r.tool.kind == ToolKind::Chamfer {
                if side != Side::On {
                    return Err(ctx.bad(
                        c.name,
                        "a chamfer mill is tapered: it traces a line to a depth (`side: on`) or breaks an edge (`type: chamfer`), it does                          not cut a wall beside a profile",
                    ));
                }
                // A V-trace: the groove down to its depth is its own.
                ctx.band.set(Some(cut.heights.top - cut.heights.bottom));
            }
            let opts = leads::contour_options(ctx, c.name, o, &r.tool)?;
            let surface = ctx.surface_in(c.name, frame)?;
            let above = leads::clear_above(&surface, &r.tool);
            let path = contour_with(&regions, &r.tool, &params, &opts, &cut, &above).map_err(|e| ctx.bad(c.name, e))?;
            Ok((path, r, "contour", None))
        }
        Op3Spec::Pocket(o) => {
            let c = o.common();
            let r = ctx.resolve(&c)?;
            let (leave, tol) = (o.stock_to_leave.unwrap_or(0.0), ctx.tolerance(c.name, o.tolerance)?);
            // On a face at the part's top, the pocket reaches past the face's edges by the tool.
            let two = features::op_cut(
                ctx,
                c.name,
                o.sketch.as_ref(),
                o.profiles.as_deref(),
                o.polygon.as_deref(),
                o.feature.as_deref(),
                o.heights.as_ref(),
                frame,
                Some(r.tool.diameter * 0.5),
            )?;
            let regions = two.regions;
            let cut = Cut { heights: two.heights, feeds: r.feeds, stepdown: or_tool(o.stepdown, r.tool.stepdown, r.tool.diameter * 0.5) };
            let params = PocketParams {
                stepover: or_tool(o.stepover, r.tool.stepover, r.tool.diameter * 0.4),
                climb: o.climb.unwrap_or(true),
                stock_to_leave: leave,
                tolerance: tol,
                ramp_angle: ctx.degrees(c.name, o.ramp_angle_deg.unwrap_or(0.0), "`ramp_angle_deg`")?,
            };
            let opts = leads::pocket_options(ctx, c.name, o, &r.tool)?;
            let surface = ctx.surface_in(c.name, frame)?;
            let above = leads::clear_above(&surface, &r.tool);
            let path = pocket_with(&regions, &r.tool, &params, &opts, &cut, &above).map_err(|e| ctx.bad(c.name, e))?;
            Ok((path, r, "pocket", None))
        }
        Op3Spec::Drill(o) => {
            let (path, r) = drill::plan(ctx, o, frame)?;
            Ok((path, r, "drill", None))
        }
        Op3Spec::Chamfer(o) => {
            let (path, r) = chamfer::plan(ctx, o, frame)?;
            Ok((path, r, "chamfer", None))
        }
        Op3Spec::Adaptive(o) => adaptive::plan(ctx, o, frame).map(|(path, r, kind)| (path, r, kind, None)),
        Op3Spec::AdaptiveRoughing(o) => adaptive::plan_roughing(ctx, o, frame).map(|(path, r, kind)| (path, r, kind, None)),
        Op3Spec::Parallel(o) => {
            let c = o.common();
            let r = ctx.resolve(&c)?;
            let surface = ctx.surface_in(c.name, frame)?;
            let (tolerance, merge) = split_budget(ctx.surface_budget(c.name, o.tolerance)?);
            let descent_limit = match o.descent_limit_deg {
                None => DEFAULT_DESCENT_LIMIT,
                Some(d) if d.is_finite() && (0.0..=90.0).contains(&d) => d.to_radians(),
                Some(_) => return Err(ctx.bad(c.name, "`descent_limit_deg` must be from 0 to 90 degrees")),
            };
            let params = ParallelParams {
                heights: heights(&o.heights),
                feeds: r.feeds,
                descent_feed: r.descent,
                stepover: or_tool(o.stepover, r.tool.stepover, r.tool.diameter * 0.1),
                angle: ctx.degrees(c.name, o.angle_deg.unwrap_or(0.0), "`angle_deg`")?,
                boundary: ctx.boundary(c.name, o.boundary.as_ref())?,
                stock_to_leave: o.stock_to_leave.unwrap_or(0.0),
                step: o.step.unwrap_or(r.tool.diameter * 0.25),
                tolerance,
                zigzag: o.zigzag.unwrap_or(true),
                descent_limit,
            };
            let path = parallel(&surface, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
            Ok((path, r, "parallel", merge))
        }
        Op3Spec::Roughing(o) => {
            let c = o.common();
            let r = ctx.resolve(&c)?;
            let surface = ctx.surface_in(c.name, frame)?;
            let (tolerance, merge) = split_budget(ctx.surface_budget(c.name, o.tolerance)?);
            let params = RoughingParams {
                heights: heights(&o.heights),
                feeds: r.feeds,
                stepdown: or_tool(o.stepdown, r.tool.stepdown, r.tool.diameter * 0.5),
                stepover: or_tool(o.stepover, r.tool.stepover, r.tool.diameter * 0.4),
                angle: ctx.degrees(c.name, o.angle_deg.unwrap_or(0.0), "`angle_deg`")?,
                boundary: ctx.boundary(c.name, o.boundary.as_ref())?,
                stock_to_leave: o.stock_to_leave.unwrap_or(0.25),
                step: o.step.unwrap_or(r.tool.diameter * 0.25),
                tolerance,
                zigzag: o.zigzag.unwrap_or(true),
            };
            let path = roughing(&surface, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
            Ok((path, r, "roughing", merge))
        }
        Op3Spec::Waterline(o) => {
            let c = o.common();
            let r = ctx.resolve(&c)?;
            let surface = ctx.surface_in(c.name, frame)?;
            let (tolerance, merge) = split_budget(ctx.surface_budget(c.name, o.tolerance)?);
            let params = WaterlineParams {
                heights: heights(&o.heights),
                feeds: r.feeds,
                stepdown: or_tool(o.stepdown, r.tool.stepdown, r.tool.diameter * 0.25),
                boundary: ctx.boundary(c.name, o.boundary.as_ref())?,
                stock_to_leave: o.stock_to_leave.unwrap_or(0.0),
                step: o.step.unwrap_or(r.tool.diameter * 0.25),
                tolerance,
                climb: o.climb.unwrap_or(true),
            };
            let path = waterline(&surface, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
            Ok((path, r, "waterline", merge))
        }
        Op3Spec::Scallop(o) => finish3d::plan_scallop(ctx, o, &*ctx.surface_in(&o.name, frame)?),
        Op3Spec::SteepShallow(o) => finish3d::plan_steep_shallow(ctx, o, &*ctx.surface_in(&o.name, frame)?),
        Op3Spec::Pencil(o) => finish3d::plan_pencil(ctx, o, &*ctx.surface_in(&o.name, frame)?),
        Op3Spec::RestFinish(o) => finish3d::plan_rest_finish(ctx, o, &*ctx.surface_in(&o.name, frame)?),
    }
}

/// The `name` of `spec` (an indexed one's inner operation's, else its own).
fn spec_name(spec: &OperationSpec) -> &str {
    spec.common().name
}

/// Whether `spec` is planned and written (`enabled`, true by default).
fn op_enabled(spec: &OperationSpec) -> bool {
    spec.common().enabled
}

/// An operation as written (its machine operation), its moves with the tool axis (for the
/// preview and the checks), its report, and the axes it moves (for the preview).
struct Planned {
    op: MachineOperation,
    poses: Toolpath5,
    /// For each move of `poses`, the move of the operation's own toolpath it comes from.
    sources: Vec<usize>,
    /// Whether it is checked for gouges into the part (always against the fixtures).
    gouge_check: bool,
    /// How far up its axis a chamfer mill's cut is its own (mm): the chamfer, the V-trace or the
    /// spotted hole it is told to cut, which the model need not show. Its gouge check holds the
    /// cone against the part less that band ([`band_gouges`]).
    band: Option<f64>,
    tool: Tool,
    /// The cut and plunge feeds it runs at.
    feeds: Feeds,
    report: OperationReport,
    axes: String,
}

/// The 3-axis operation `op` planned with the rotaries at `rotary`: with every one at zero in
/// work coordinates, indexed (`a_deg`, A held at that angle) in the frame the angle brings
/// under the tool.
fn plan_indexed(ctx: &Ctx<'_>, op: &Op3Spec, rotary: Vec<f64>, a_deg: Option<f64>) -> Result<Planned> {
    let c = op.common();
    let frame = frame_for_index(ctx.machine, &rotary).map_err(|e| ctx.bad(c.name, e))?;
    ctx.op_type.set(type_of3(op));
    ctx.band.set(None);
    ctx.adaptive.borrow_mut().take();
    let (path, r, kind) = plan_path(ctx, op, &frame)?;
    let band = ctx.band.take();
    let stats = path.stats();
    let io = IndexedOperation { name: c.name.into(), tool: r.tool.clone(), spindle_rpm: r.rpm, coolant: c.coolant, rotary, path };
    let (poses, sources) = io.poses_mapped(ctx.machine, ctx.pose_tol).map_err(|e| ctx.bad(c.name, e))?;
    let report = OperationReport {
        name: c.name.into(),
        kind: if a_deg.is_some() { format!("indexed {kind}") } else { kind.into() },
        tool: r.tool.number,
        moves: io.path.moves.len(),
        cut_length: stats.cut_length,
        cut_minutes: stats.cut_minutes,
        max_tilt_deg: poses.stats().max_tilt.to_degrees(),
        a_deg,
        ..OperationReport::default()
    };
    let mut report = report;
    if let Some((extra, warnings)) = ctx.adaptive.borrow_mut().take() {
        report.adaptive = Some(extra);
        report.warnings.extend(warnings);
    }
    let axes = match a_deg {
        Some(a) => format!("3+1 A{}°", num(a)),
        None => "3-axis".into(),
    };
    Ok(Planned { op: MachineOperation::Indexed(io), poses, sources, gouge_check: c.gouge_check, band, tool: r.tool, feeds: r.feeds, report, axes })
}

/// One operation planned: a 3-axis one with every rotary at zero (on a 3-axis machine and a 4th
/// axis alike, in work coordinates), an indexed one at its angle, a rotary or wrapped one turning
/// A as it cuts.
fn plan(ctx: &Ctx<'_>, spec: &OperationSpec) -> Result<Planned> {
    if spec_name(spec).trim().is_empty() {
        return Err(bad(ctx.cmd, "every operation needs a `name` (an `indexed` one may carry it on itself or on its inner `operation`)"));
    }
    let zero = || vec![0.0; ctx.machine.rotaries.len()];
    ctx.op_type.set(type_of(spec));
    match spec {
        OperationSpec::Contour(o) => plan_indexed(ctx, &Op3Spec::Contour(o.clone()), zero(), None),
        OperationSpec::Pocket(o) => plan_indexed(ctx, &Op3Spec::Pocket(o.clone()), zero(), None),
        OperationSpec::Parallel(o) => plan_indexed(ctx, &Op3Spec::Parallel(o.clone()), zero(), None),
        OperationSpec::Roughing(o) => plan_indexed(ctx, &Op3Spec::Roughing(o.clone()), zero(), None),
        OperationSpec::Waterline(o) => plan_indexed(ctx, &Op3Spec::Waterline(o.clone()), zero(), None),
        OperationSpec::Scallop(o) => plan_indexed(ctx, &Op3Spec::Scallop(o.clone()), zero(), None),
        OperationSpec::SteepShallow(o) => plan_indexed(ctx, &Op3Spec::SteepShallow(o.clone()), zero(), None),
        OperationSpec::Pencil(o) => plan_indexed(ctx, &Op3Spec::Pencil(o.clone()), zero(), None),
        OperationSpec::RestFinish(o) => plan_indexed(ctx, &Op3Spec::RestFinish(o.clone()), zero(), None),
        OperationSpec::Drill(o) => plan_indexed(ctx, &Op3Spec::Drill(o.clone()), zero(), None),
        OperationSpec::Chamfer(o) => plan_indexed(ctx, &Op3Spec::Chamfer(o.clone()), zero(), None),
        OperationSpec::Bore(o) => helical::bore_op(ctx, o),
        OperationSpec::ThreadMill(o) => helical::thread_mill_op(ctx, o),
        OperationSpec::Adaptive(o) => plan_indexed(ctx, &Op3Spec::Adaptive(o.clone()), zero(), None),
        OperationSpec::AdaptiveRoughing(o) => plan_indexed(ctx, &Op3Spec::AdaptiveRoughing(o.clone()), zero(), None),
        OperationSpec::Face(o) => cam_stock::plan_face(ctx, o),
        OperationSpec::Indexed(o) => {
            // The inner operation's name, else the wrapper's.
            let name = spec_name(spec).to_string();
            let mut inner = (*o.operation).clone();
            name.clone_into(xrotary::name3_mut(&mut inner));
            if ctx.machine.rotaries.len() != 1 {
                return Err(ctx.bad(&name, "an indexed operation's `a_deg` needs a machine with one rotary axis (`machine: \"4axis_a\"`)"));
            }
            let a = ctx.degrees(&name, o.a_deg, "`a_deg`")?;
            plan_indexed(ctx, &inner, vec![a], Some(o.a_deg))
        }
        OperationSpec::RotaryFinish(o) => xrotary::plan_rotary_finish(ctx, o),
        OperationSpec::RotaryRoughing(o) => xrotary::plan_rotary_roughing(ctx, o),
        OperationSpec::Wrap(o) => xrotary::plan_wrap(ctx, o),
        OperationSpec::WrapPocket(o) => xrotary::plan_wrap_pocket(ctx, o),
        OperationSpec::WrapContour(o) => xrotary::plan_wrap_contour(ctx, o),
        // `xrotary::plan_op` plans a pattern as its indexed operations.
        OperationSpec::IndexedPattern(_) => Err(ctx.bad(spec_name(spec), "an indexed pattern is planned as its indexed operations")),
    }
}

/// The program's name: the job's, else the design's.
fn job_name(cmd: &str, s: &Session, job: &CamJob) -> Result<String> {
    match &job.name {
        Some(n) if n.chars().count() > 200 || n.chars().any(char::is_control) => {
            Err(bad(cmd, "the job's `name` must be at most 200 characters, without control characters"))
        }
        Some(n) => Ok(n.clone()),
        None => Ok(s.doc.name.clone()),
    }
}

/// The job of a call (`inline` under `job`, or the file `path`) and the folder its relative
/// paths are read from (the job file's).
fn load_job(cmd: &str, p: &Value, inline: bool, path: Option<&String>) -> Result<(CamJob, Option<PathBuf>)> {
    match (inline, path) {
        (true, Some(_)) => Err(bad(cmd, "give `job` or `job_path`, not both")),
        (false, None) => Err(bad(cmd, "give the `job` (an object) or a `job_path` (a .json file)")),
        (true, None) => Ok((parse(cmd, p.get("job").unwrap_or(&Value::Null), "`job`")?, None)),
        (false, Some(path)) => {
            let raw = read_json(cmd, path, "job_path")?;
            if !raw.is_object() {
                return Err(bad(cmd, format!("job_path {path}: the job must be a JSON object")));
            }
            Ok((parse(cmd, &raw, &format!("job_path {path}"))?, Path::new(path).parent().map(Path::to_path_buf)))
        }
    }
}

/// A job planned and its program written in memory: what `cam.job` checks and writes, and what
/// the simulation view plays.
struct PlannedJob {
    name: String,
    machine: Machine,
    post_name: String,
    post: Post5,
    tools: Vec<Tool>,
    machine_rates: Option<solidcraft_cam::machine_time::MachineRates>,
    /// The part's bodies.
    bodies: Vec<String>,
    /// The bodies that hold the part.
    fixtures: Vec<String>,
    /// Where the part is in work coordinates.
    place: Placement,
    tol: f64,
    /// The chord the part and fixtures are tessellated within.
    chord: f64,
    /// How deep a cut into the tessellated part is allowed.
    allowed: f64,
    /// The job's `breakthrough`, mm.
    breakthrough: f64,
    surfaces: Surfaces,
    stock: Option<solidcraft_cam::stock::Stock>,
    notes: Vec<String>,
    planned: Vec<Planned>,
    program: MachineProgram,
    /// The program as the post writes it.
    text: String,
}

/// `job` (its relative paths read from `base`) read and checked, every operation planned in
/// order, and the program written in memory.
fn plan_job(cmd: &str, s: &Session, job: &CamJob, base: Option<&Path>) -> Result<PlannedJob> {
    // The job, read and checked.
    if job.operations.is_empty() || job.operations.len() > MAX_OPERATIONS {
        return Err(bad(cmd, format!("a job has 1 to {MAX_OPERATIONS} operations")));
    }
    if !job.operations.iter().any(op_enabled) {
        return Err(bad(cmd, "every operation of the job is turned off (`enabled: false`): turn one on to plan the job"));
    }
    let tol = job.tolerance.unwrap_or(0.01);
    if !(tol.is_finite() && (TOLERANCE_RANGE.0..=TOLERANCE_RANGE.1).contains(&tol)) {
        return Err(bad(cmd, "the job's `tolerance` must be from 0.0001 to 1 mm"));
    }
    let place = placement(cmd, s, job.origin.unwrap_or([0.0; 3]), job.orientation.as_ref())?;
    if job.work_offset.is_some_and(|w| !(1..=6).contains(&w)) {
        return Err(bad(cmd, "`work_offset` must be 1 to 6 (G54 to G59)"));
    }
    let name = job_name(cmd, s, job)?;
    let machine = machine_of(cmd, job, base)?;
    if place.axes.is_some() && !machine.rotaries.is_empty() {
        return Err(bad(cmd, "`orientation` turns the part on a 3-axis machine; a 4th axis turns it with its own angles"));
    }
    let (post_name, mut post) = post_of(cmd, job, base)?;
    // A controller that ignores the tool change stops (`M0`) for the operator at each one.
    post.base.stop_at_tool_change |= job.tool_change == Some(ToolChangeMode::Ignored);
    let tools = tools_of(cmd, job, base)?;
    let machine_rates = rates::rates_of(cmd, job, base)?;
    let (bodies, fixtures) = job_bodies(cmd, s, job)?;
    let mut notes = Vec::new();
    // The part and the fixtures as surfaces: what the 3D and rotary operations follow, what A
    // must never turn into the tool, and what every cut is checked for gouges against.
    let chord = tol * CHORD_SHARE;
    let surfaces = surfaces_of(cmd, s, &bodies, &fixtures, &place, chord, &mut notes)?;
    let stock = cam_stock::stock_of(cmd, s, job, &bodies, &place)?;
    let breakthrough = match job.breakthrough {
        None => 0.0,
        Some(b) if b.is_finite() && (0.0..=25.0).contains(&b) => b,
        Some(_) => return Err(bad(cmd, "the job's `breakthrough` must be from 0 to 25 mm")),
    };
    // How deep a cut into the tessellated part is allowed: the tolerance less what the
    // tessellation itself may be off by.
    let allowed = tol - chord;
    // Arcs are followed far closer than the post's tolerance for the checks, so their chords
    // are not taken for cuts into the part.
    let pose_tol = post.base.arc_tolerance.min(allowed * CHECK_ARC_SHARE).max(1e-6);
    if job.work_offset.is_some() && !post.base.writes_work_offset() {
        notes.push(format!("post `{post_name}` writes no work offset: the program runs in whichever the controller has active"));
    }

    // Every operation planned, in order, then the program written in memory.
    let span = solidcraft_profile::span("cam", "job.plan");
    let features = features::Lazy::new(job.bodies.clone(), place.origin, tol);
    let ctx = Ctx {
        cmd,
        s,
        machine: &machine,
        tools: &tools,
        place,
        stock: stock.as_ref(),
        tol,
        post: &post,
        part: Some(&surfaces.part),
        chord,
        pose_tol,
        all: Some(&surfaces.all),
        op_type: std::cell::Cell::new(""),
        band: std::cell::Cell::new(None),
        adaptive: Default::default(),
        features: &features,
    };
    let mut planned: Vec<Planned> = Vec::with_capacity(job.operations.len());
    for spec in job.operations.iter().filter(|o| op_enabled(o)) {
        let ops = xrotary::plan_op(&ctx, spec, planned.len())?;
        xrotary::add_checked(&ctx, spec, &mut planned, ops)?;
    }
    let program = MachineProgram { name: name.clone(), operations: planned.iter().map(|p| p.op.clone()).collect(), work_offset: job.work_offset };
    let (text, written) = emit_program_report(&program, &post, &machine).map_err(|e| cam_err(cmd, "writing the program", e))?;
    // The rewinds' feeds are time at feed of their operations too, so the operations' minutes add
    // up to the program's as read back.
    for (p, w) in planned.iter_mut().zip(&written) {
        p.report.cut_minutes += w.rewind_minutes;
        p.report.rewinds = w.rewinds;
    }
    drop(span);
    Ok(PlannedJob {
        name,
        machine,
        post_name,
        post,
        tools,
        machine_rates,
        bodies,
        fixtures,
        place,
        tol,
        chord,
        allowed,
        breakthrough,
        surfaces,
        stock,
        notes,
        planned,
        program,
        text,
    })
}

/// `cam.job`.
fn job(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.job";
    let params: CamJobParams = parse(cmd, p, "")?;
    let reply = run_job(s, cmd, p, params, None)?;
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// `cam.job`, or a dry run that draws only operation `only` (`cam.view`).
fn run_job(s: &mut Session, cmd: &str, raw: &Value, params: CamJobParams, only: Option<&str>) -> Result<CamJobReply> {
    let dry_run = params.dry_run.unwrap_or(false);
    let allow_unsafe = params.allow_unsafe.unwrap_or(false);

    // Every path is checked before anything is computed or written.
    let output = match (&params.output, dry_run) {
        (Some(o), _) => Some(o.clone()),
        (None, true) => None,
        (None, false) => return Err(bad(cmd, "give `output` (the G-code file to write) or `dry_run: true`")),
    };
    let sheet_format = sheet::format_of(cmd, params.setup_sheet.as_deref())?;
    let wanted = pictures::Request::of(cmd, &params, only)?;
    if !dry_run {
        for path in output.iter().chain(&params.preview).chain(&params.setup_sheet) {
            if path.trim().is_empty() {
                return Err(bad(cmd, "a file path must not be empty"));
            }
            solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
        }
        if let (Some(o), Some(p)) = (&output, &params.preview)
            && same_file(o, p)
        {
            return Err(bad(cmd, format!("`output` and `preview` must be different files (`{o}` and `{p}` are the same file)")));
        }
        sheet::distinct(cmd, output.as_deref(), params.preview.as_deref(), params.setup_sheet.as_deref())?;
    }
    let (job, base) = load_job(cmd, raw, params.job.is_some(), params.job_path.as_ref())?;
    let base = base.as_deref();

    let PlannedJob {
        name,
        machine,
        post_name,
        post,
        tools,
        machine_rates,
        bodies,
        fixtures,
        place,
        tol,
        chord,
        allowed,
        breakthrough,
        surfaces,
        stock,
        mut notes,
        mut planned,
        program,
        text,
    } = plan_job(cmd, s, &job, base)?;

    // The program as written, read back through the machine and held against the toolpaths.
    let span = solidcraft_profile::span("cam", "job.readback");
    let rb = readback_program(&text, &program, &post, &machine).map_err(|e| cam_err(cmd, "reading the program back", e))?;
    let machine_time = match &machine_rates {
        Some(r) => Some(rates::time_report(cmd, &text, &machine, &post, r)?),
        None => None,
    };
    if let Some(t) = &machine_time {
        notes.extend(t.warnings.iter().cloned());
    }
    drop(span);
    let readback = ReadbackReport {
        max_tip_deviation: rb.max_tip_deviation,
        max_axis_deviation_deg: rb.max_axis_deviation.to_degrees(),
        allowed_tip: rb.allowed_tip,
        allowed_axis_deg: rb.allowed_axis.to_degrees(),
        cut_minutes: rb.cut_minutes,
        pass: rb.within(),
    };
    let mut failures: Vec<String> = Vec::new();
    // Gouges and holder collisions, along every cutting move.
    let span = solidcraft_profile::span("cam", "job.gouge_holder");
    let mut gouge = GougeReport { tolerance: tol, tessellation: chord, allowed, ..GougeReport::default() };
    let mut holder_total: Option<usize> = None;
    // The operations' own toolpaths, as written (indexed ones), to find the line of a move.
    let own_paths: Vec<Option<&Toolpath>> = planned
        .iter()
        .map(|p| match &p.op {
            MachineOperation::Indexed(io) => Some(&io.path),
            MachineOperation::Continuous(_) => None,
        })
        .collect();
    let b = &post.base;
    let lines = move_lines(&text, &own_paths, b.units.mm(), 10f64.powi(-i32::from(b.decimals)) * b.units.mm());
    for (k, p) in planned.iter_mut().enumerate() {
        let step = (p.tool.radius() * 0.5).clamp(0.1, 2.0);
        let op = p.report.name.clone();
        // The moves of the operation's own toolpath hits are along.
        let moves_of = |hits: &[Hit]| hits.iter().filter_map(|h| p.sources.get(h.index)).collect::<BTreeSet<_>>().len();
        let (against, what) = if p.gouge_check {
            (Some(&surfaces.all), "the part or a fixture as tessellated")
        } else {
            // Not against the part, but a fixture is never cut.
            gouge.skipped.push(op.clone());
            p.report.warnings.push("not checked for gouges into the part (`gouge_check: false`); checked against the fixtures".into());
            (surfaces.fixtures.as_ref(), "a fixture")
        };
        if let Some(against) = against {
            let hits = match p.band {
                // A chamfer mill: the band it is told to cut is its own, in the part only; a
                // fixture is held against the whole tool.
                Some(band) if p.gouge_check => op_band_gouges(p, &surfaces, band, allowed, step),
                _ => tool_gouges(against, &p.tool, &p.poses, allowed, step),
            }
            .map_err(|e| cam_err(cmd, &format!("operation `{op}`: gouge check"), e))?;
            let moves = moves_of(&hits);
            if let Some(worst) = hits.iter().max_by(|a, b| a.distance.total_cmp(&b.distance)) {
                let move_index = p.sources.get(worst.index).copied().unwrap_or(worst.index);
                let line = lines.get(k).and_then(|l| l.get(move_index)).copied().flatten();
                let tip = worst.pose.tip;
                let at = GougeAt { operation: op.clone(), move_index, line, xyz: [tip.x, tip.y, tip.z] };
                let exact = if p.gouge_check {
                    format!(
                        " (the check allows {} mm: the {} mm tolerance less the tessellation's {} mm), up to {} mm into the exact part,",
                        num(allowed),
                        num(tol),
                        num(chord),
                        num(worst.distance + chord)
                    )
                } else {
                    String::new()
                };
                failures.push(format!(
                    "gouge: operation `{op}` cuts {} mm into {what}{exact} along {moves} moves; deepest at move {move_index}{} at X{} Y{} Z{} \
                     (work coordinates)",
                    num(worst.distance),
                    line.map_or(String::new(), |l| format!(" (G-code line {l})")),
                    num(tip.x),
                    num(tip.y),
                    num(tip.z)
                ));
                p.report.gouge = worst.distance;
                if worst.distance > gouge.worst {
                    gouge.worst = worst.distance;
                    gouge.at = Some(at.clone());
                }
                p.report.gouge_at = Some(at);
            }
            gouge.hits += moves;
        }
        if !p.tool.envelopes().is_empty() {
            let hits = holder_collisions_within(&surfaces.all, &p.tool, &p.poses, 0.0, allowed, step)
                .map_err(|e| cam_err(cmd, &format!("operation `{op}`: holder check"), e))?;
            let moves = moves_of(&hits);
            p.report.holder_collisions = moves;
            *holder_total.get_or_insert(0) += moves;
            if moves > 0 {
                failures.push(format!("holder: operation `{op}`: the tool's shank or holder touches the part or a fixture along {moves} moves"));
            }
        }
    }
    gouge.pass = gouge.hits == 0;
    drop(span);
    // Every operation's lowest cut against the stock's bottom.
    let span = solidcraft_profile::span("cam", "job.motion");
    if let Some(st) = stock.as_ref() {
        let fixture_low = cam_stock::work_bounds(s, &fixtures, &place)?.map(|b| b.min.z);
        for (op, depth) in cam_stock::check_below_stock(&planned, st, fixture_low, breakthrough, &machine, tol, &mut failures) {
            notes.push(format!(
                "operation `{op}` cuts {} mm below the stock's bottom, as the job's `breakthrough` allows: nothing modelled is under the \
                 stock there",
                num(depth)
            ));
        }
    }
    // Every rapid of the program as written, and every position against the machine's travel.
    let unchecked: Vec<bool> = planned.iter().map(|p| !p.gouge_check || p.band.is_some()).collect();
    let written = cam_stock::Written { text: &text, program: &program, post: &post, machine: &machine };
    let (rapids, travel) = cam_stock::check_motion(cmd, &job, &written, &surfaces, stock.as_ref(), &unchecked, tol, &mut failures)?;
    drop(span);
    // Every move of the program against the job's workholding and machine model.
    let span = solidcraft_profile::span("cam", "job.holding");
    let workholding = if job.workholding.is_some() || job.workholding_path.is_some() || job.machine_model_path.is_some() {
        let shape = stock_shape(cmd, s, stock.as_ref(), &bodies, &place, &machine)?;
        let mut warnings = Vec::new();
        let holding = hold::holding_of(cmd, &job, base, &shape, &machine, &mut notes, &mut warnings)?;
        notes.extend(warnings);
        let blocks = parse5(&text, &machine.letters()).map_err(|e| cam_err(cmd, "reading the program back", e))?;
        let lines: Vec<(usize, usize)> = solidcraft_cam::rapids::owners(&program, &post, &machine, &blocks)
            .map(|o| blocks.iter().zip(o).map(|(b, k)| (b.line(), k)).collect())
            .unwrap_or_default();
        let op_at_line = |l: usize| -> Option<(usize, String)> {
            let at = lines.partition_point(|(bl, _)| *bl <= l).checked_sub(1)?;
            let k = lines.get(at)?.1;
            Some((k, planned.get(k)?.report.name.clone()))
        };
        let mut report = hold::check_holding(
            cmd,
            &text,
            &machine,
            &tools,
            &shape,
            &holding,
            tol,
            &op_at_line,
            &rises_of(&planned, &post),
            &head_bodies(&surfaces),
            &mut failures,
        )?;
        // The vise's grip weighed against the job's deepest side cut (a warning: a rule of thumb).
        if let (Some(r), Some(sc)) = (report.as_mut(), holding.scene.as_deref())
            && let (Some(grip), Some(above), Some(cut)) = (sc.stock_grip, sc.stock_above_jaws, deepest_side_cut(&planned, &shape))
            && let Some(w) = solidcraft_cam::workholding::grip_for_cut(grip, above, &cut, false)
        {
            r.setup_warnings.push(w.message.clone());
            notes.push(format!("workholding: {}", w.message));
        }
        // Every move against the machine model's soft limits, in machine coordinates, from where
        // the workholding puts work zero on the machine (or, without it, with the travel centred
        // on the table).
        if let (Some(m), Some(sc)) = (&holding.model, &holding.scene)
            && let Some(l) = &m.soft_limits
        {
            soft_limits(cmd, &text, &machine, m, sc, l, &op_at_line, &mut failures)?;
        }
        report
    } else {
        None
    };
    drop(span);
    if !readback.pass {
        failures.push(format!(
            "read-back: the written program is up to {} mm and {}° from its toolpaths (the post's precision allows {} mm and {}°)",
            num(readback.max_tip_deviation),
            num(readback.max_axis_deviation_deg),
            num(readback.allowed_tip),
            num(readback.allowed_axis_deg)
        ));
    }
    // The written program swept through stock taken from the part and held against the part.
    let simulation = if params.simulate.unwrap_or(false) {
        let span = solidcraft_profile::span("cam", "job.simulate");
        let mut part_notes = Vec::new();
        let part = sim::part_surface(cmd, s, &job, &place, tol, &mut part_notes)?;
        let (mut reply, _) = sim::check_program(cmd, &text, &machine, &tools, &part, tol, params.simulate_resolution, sim::most_left(&job))?;
        reply.notes.extend(part_notes);
        failures.extend(reply.failures.iter().cloned());
        drop(span);
        Some(reply)
    } else {
        if params.simulate_resolution.is_some() {
            return Err(bad(cmd, "`simulate_resolution` goes with `simulate: true`"));
        }
        None
    };
    let passed = failures.is_empty();
    if !passed && !dry_run && !allow_unsafe {
        return Err(bad(
            cmd,
            format!(
                "nothing was written: checks failed: {}. Fix the job, run it with `dry_run: true` to see the report, or pass `allow_unsafe: true`",
                failures.join("; ")
            ),
        ));
    }
    if let Some(path) = &output
        && let Some(ext) = Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase())
        && ext != post.base.extension.to_ascii_lowercase()
    {
        notes.push(format!("post `{post_name}` writes .{} files; this one is .{ext}", post.base.extension));
    }
    pictures::extents(&machine, &post, &program, &mut planned);
    // One program per tool, and what the machine does at a tool change.
    let split_by_tool = params.split_by_tool.unwrap_or(false);
    let parts = if split_by_tool { programs::split(cmd, &program, &planned, &post, &machine, output.as_deref())? } else { Vec::new() };
    let mut warnings: Vec<String> = programs::warning(job.tool_change, &planned, split_by_tool).into_iter().collect();
    // Material the simulation found left on the part: a finding, not a failed check.
    warnings.extend(simulation.iter().flat_map(|r| r.warnings.iter().filter(|w| w.starts_with(sim::PART_NOT_FINISHED)).cloned()));
    // An older program of every tool under the same name would cut on with the first tool where
    // the machine ignores the tool change.
    if let Some(old) = output.as_deref().filter(|o| split_by_tool && !dry_run && Path::new(o).is_file()) {
        warnings.push(format!(
            "old program: {old} (one program of every tool, written before) is still beside the programs written one per tool: delete it so it is not run by mistake"
        ));
    }
    let setup = sheet::build(&sheet::Inputs {
        name: &name,
        machine: &machine,
        post_name: &post_name,
        post: &post,
        work_offset: job.work_offset,
        origin: place.origin,
        part: sheet::part_extent(s, &bodies, &place)?,
        planned: &planned,
        text: &text,
        readback: &readback,
        failures: &failures,
        notes: &notes,
        program_file: if dry_run { None } else { output.as_deref() },
        attention: machine_time.iter().flat_map(|t| t.limits.iter().cloned()).collect(),
        tool_change: programs::sheet_note(job.tool_change, &planned, split_by_tool),
        programs: parts.iter().map(|w| programs::part_words(&w.part)).collect(),
        simulation: simulation.as_ref(),
        program_files: parts
            .iter()
            .filter_map(|w| w.part.path.as_deref().and_then(|f| Path::new(f).file_name()).map(|f| f.to_string_lossy().to_string()))
            .collect(),
        warnings: &warnings,
    });
    let summary = sheet::summary(&setup);

    let mut outputs = Outputs::default();
    if !dry_run {
        // A program written with failed checks says so in its first line.
        let marked = |t: &str| if passed { t.to_string() } else { mark_unsafe(t, &post) };
        if split_by_tool {
            for programs::Written { part, text: t } in &parts {
                if let Some(path) = &part.path {
                    write_file(path, marked(t).as_bytes()).map_err(|e| {
                        let done = if outputs.programs.is_empty() { String::new() } else { format!(" ({} written)", outputs.programs.join(", ")) };
                        EngineError::Other(format!("{path}: {e}{done}"))
                    })?;
                    outputs.programs.push(path.clone());
                }
            }
        } else if let Some(path) = &output {
            write_file(path, marked(&text).as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
            outputs.gcode = Some(path.clone());
        }
        if let Some(path) = &params.preview {
            let tris = preview_part(cmd, s, &bodies, &place, tol)?;
            let page = preview_page(&name, &tris, &planned, &text);
            write_file(path, page.as_bytes()).map_err(|e| {
                let written = outputs.gcode.as_deref().map_or(String::new(), |g| format!(" (the G-code was written to {g})"));
                EngineError::Other(format!("{path}: {e}{written}"))
            })?;
            outputs.preview = Some(path.clone());
        }
        if let (Some(path), Some(format)) = (&params.setup_sheet, sheet_format) {
            write_file(path, setup.render(format).as_bytes()).map_err(|e| {
                let written: Vec<&str> = outputs.gcode.iter().chain(&outputs.preview).map(String::as_str).collect();
                let written = if written.is_empty() { String::new() } else { format!(" ({} written)", written.join(" and ")) };
                EngineError::Other(format!("{path}: {e}{written}"))
            })?;
            outputs.setup_sheet = Some(path.clone());
        }
    }
    // The pictures an agent asked for, in a dry run too; the part hides what is behind it.
    let images = if wanted.wanted() {
        let setting = pictures::Setting { machine: &machine, origin: place.origin, surface: &surfaces.part, tol, name: &name };
        pictures::write(cmd, s, &wanted, &setting, &planned).map_err(|e| match (e, &outputs.gcode) {
            (EngineError::Other(m), Some(g)) => EngineError::Other(format!("{m} (the G-code was written to {g})")),
            (e, _) => e,
        })?
    } else {
        None
    };
    let cut_minutes = machine_time.as_ref().map_or(readback.cut_minutes, |t| t.cut_minutes);
    let rapid_minutes = machine_time.as_ref().map(|t| t.rapid_minutes);
    Ok(CamJobReply {
        name,
        machine: machine.name.clone(),
        post: post_name,
        operations: planned.into_iter().map(|p| p.report).collect(),
        program_lines: text.lines().count(),
        program_head: text.lines().take(HEAD_LINES).map(str::to_string).collect(),
        cut_minutes,
        rapid_minutes,
        machine_time,
        readback,
        gouge,
        holder_collisions: holder_total,
        rapids,
        travel,
        stock: stock.as_ref().map(cam_stock::stock_report),
        workholding,
        passed,
        failures,
        dry_run,
        outputs,
        notes,
        simulation,
        summary,
        images,
        warnings,
        programs: parts.into_iter().map(|w| w.part).collect(),
        param_warnings: Vec::new(),
    })
}

/// `text` marked as written with failed checks: its first line, when it is the post's comment
/// (the program's name), says so; the line count is unchanged, so every line a failure names
/// stays where it is.
fn mark_unsafe(text: &str, post: &Post5) -> String {
    let Some((first, rest)) = text.split_once('\n') else { return text.to_string() };
    let is_comment = first.trim_start().starts_with('(') || first.trim_start().starts_with(';');
    if !is_comment {
        return text.to_string();
    }
    let inner = first.trim().trim_start_matches(['(', ';']).trim_end_matches(')').trim();
    match post.base.comment_line(&format!("{inner} - WRITTEN WITH FAILED CHECKS")) {
        Some(line) => format!("{line}\n{rest}"),
        None => text.to_string(),
    }
}

/// Writes `bytes` to `path` whole: to a temporary file beside it, then renamed over it.
fn write_file(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    solidcraft_io::vfs::check(path)?;
    solidcraft_io::write_atomic(Path::new(path), bytes, false)
}

/// `path` as one file is named whatever its spelling: absolute, its folder resolved (links,
/// `.` and `..`) or the whole path if the file is there, `/` between parts, and on Windows
/// without the dots and spaces it drops from the end of a name and in lower case, as its file
/// names are not case-sensitive.
fn file_key(path: &str) -> String {
    let p = Path::new(path.trim());
    let abs = if p.is_relative() { std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.to_path_buf()) } else { p.to_path_buf() };
    let lexical = |q: &Path| {
        let mut out = PathBuf::new();
        for c in q.components() {
            match c {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other),
            }
        }
        out
    };
    let abs = lexical(&abs);
    let key = match (abs.parent(), abs.file_name()) {
        (Some(dir), Some(name)) => {
            // Windows drops the dots and spaces at the end of a name (`a.tap.` opens `a.tap`).
            let name = name.to_string_lossy();
            let name = if cfg!(windows) { name.trim_end_matches(['.', ' ']) } else { &name };
            // A file that is there already is named by itself (a link to it, a short name).
            let joined = dir.join(name);
            std::fs::canonicalize(&joined).unwrap_or_else(|_| std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()).join(name))
        }
        _ => abs,
    };
    let text = key.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").map(str::to_string).unwrap_or(text);
    if cfg!(windows) { text.to_lowercase() } else { text }
}

/// Whether two paths name the same file ([`file_key`]).
fn same_file(a: &str, b: &str) -> bool {
    file_key(a) == file_key(b)
}

/// The job's stock as the workholding is placed with it: its own, else the box round the part's
/// exact bodies (on a rotary machine, the round bar round them about work X).
fn stock_shape(cmd: &str, s: &Session, stock: Option<&Stock>, part: &[String], place: &Placement, machine: &Machine) -> Result<StockShape> {
    Ok(match stock {
        Some(&Stock::Box { min, max }) => StockShape::Box { min, max },
        Some(&Stock::Cylinder { radius, x_min, x_max }) => StockShape::Cylinder { x_min, x_max, radius },
        None => {
            let Some(b) = cam_stock::work_bounds(s, part, place)? else {
                return Err(bad(cmd, "the workholding is placed with the stock: give the job a `stock` (the part has no extent to take it from)"));
            };
            if machine.rotaries.is_empty() {
                StockShape::Box { min: b.min, max: b.max }
            } else {
                let radius = b.min.y.abs().max(b.max.y.abs()).hypot(b.min.z.abs().max(b.max.z.abs()));
                StockShape::Cylinder { x_min: b.min.x, x_max: b.max.x, radius }
            }
        }
    })
}

/// What sets how high the tool goes in a program of `planned` for `post`: each operation's
/// highest rapid (its clearance, in program Z: along the tool from the A axis on a 4th axis)
/// and the post's `clearance`, for the fix of a move too high for the spindle.
fn rises_of(planned: &[Planned], post: &Post5) -> hold::Rises {
    let ops = planned
        .iter()
        .filter_map(|p| {
            let top = p
                .poses
                .moves
                .iter()
                .filter_map(|m| match m {
                    solidcraft_cam::Move5::Rapid { to } => Some(to.tip.y * to.axis.y + to.tip.z * to.axis.z),
                    _ => None,
                })
                .reduce(f64::max)?;
            Some((p.report.name.clone(), top))
        })
        .collect();
    hold::Rises { ops, post: post.clearance.filter(|_| post.clearance_retract) }
}

/// The bodies the spindle head is checked against: the part, and the design's fixtures.
fn head_bodies(s: &Surfaces) -> Vec<solidcraft_cam::playback::HeadBody<'_>> {
    let mut out = vec![solidcraft_cam::playback::HeadBody { surface: &s.part, code: "part", noun: "the part" }];
    if let Some(f) = &s.fixtures {
        out.push(solidcraft_cam::playback::HeadBody { surface: f, code: "fixture-body", noun: "a fixture of the design" });
    }
    out
}

/// The job's deepest side cut ([`solidcraft_cam::workholding::grip_for_cut`]): the operation
/// whose cutting moves, square to work Z, go furthest below the stock's top, and how far. A
/// drill, bore, thread mill or chamfer pushes along the tool or cuts lightly near the top, and is
/// left out; so is a move tilted off work Z (a rotary or indexed cut, not across the jaws).
fn deepest_side_cut(planned: &[Planned], stock: &StockShape) -> Option<solidcraft_cam::workholding::SideCut> {
    let StockShape::Box { max, .. } = *stock else { return None };
    let mut deepest: Option<solidcraft_cam::workholding::SideCut> = None;
    for p in planned {
        if matches!(p.report.kind.trim_start_matches("indexed "), "drill" | "bore" | "thread_mill" | "chamfer") {
            continue;
        }
        let low = p
            .poses
            .moves
            .iter()
            .filter_map(|m| match m {
                solidcraft_cam::Move5::Feed { to, .. } if to.axis.z > 1.0 - 1e-6 => Some(to.tip.z),
                _ => None,
            })
            .filter(|z| z.is_finite())
            .fold(f64::INFINITY, f64::min);
        let depth = max.z - low;
        if depth.is_finite() && depth > deepest.as_ref().map_or(0.0, |d| d.depth) {
            deepest = Some(solidcraft_cam::workholding::SideCut { operation: p.report.name.clone(), depth });
        }
    }
    deepest
}

/// Every move of the program `text` against the machine model `m`'s soft limits `l`, in machine
/// coordinates, from where the workholding scene `sc` puts work zero on the machine; without its
/// `work_zero_machine`, X and Y with work zero where the workholding stands on the table and the
/// travel centred on the table (Z is the spindle's own travel, checked per tool with the
/// workholding). The first move beyond them is a failure, with its operation and a fix.
#[allow(clippy::too_many_arguments)]
fn soft_limits(
    cmd: &str,
    text: &str,
    machine: &Machine,
    m: &solidcraft_cam::machine_model::MachineModel,
    sc: &solidcraft_cam::workholding::HoldScene,
    l: &solidcraft_cam::machine_model::SoftLimits,
    op_at_line: &dyn Fn(usize) -> Option<(usize, String)>,
    failures: &mut Vec<String>,
) -> Result<()> {
    let given = sc.work_zero_machine.map(|z| (z, v3(l.min), v3(l.max)));
    let derived = || {
        solidcraft_cam::machine_model::work_zero_machine_xy(m, &sc.table)
            .map(|[x, y]| (Vec3::new(x, y, 0.0), Vec3::new(l.min[0], l.min[1], f64::MIN), Vec3::new(l.max[0], l.max[1], f64::MAX)))
    };
    let Some((zero, lo, hi)) = given.or_else(derived) else { return Ok(()) };
    let t = solidcraft_cam::rapids::check_travel_within(text, machine, lo, hi, zero)
        .map_err(|e| cam_err(cmd, "checking the machine's soft limits", e))?;
    let Some(v) = t.violation else { return Ok(()) };
    let at = if sc.work_zero_machine.is_some() {
        format!("work zero at machine {}, {}, {}", num(zero.x), num(zero.y), num(zero.z))
    } else {
        format!(
            "work zero at machine X{} Y{}, from where the workholding puts it on the table with the travel centred on the table: give the \
             workholding `work_zero_machine` if it is elsewhere",
            num(zero.x),
            num(zero.y)
        )
    };
    // The operation the move is in, and how far the part must move for it to fit: towards the
    // travel's other end by what the move is beyond it.
    let op = op_at_line(v.line).map_or(String::new(), |(_, n)| format!("operation `{n}`: "));
    let (over, towards) = if v.value > v.high { (v.value - v.high, '-') } else { (v.low - v.value, '+') };
    let fix = if v.axis == 'Z' {
        "move work zero, or lower the clearance heights".to_string()
    } else {
        let other = if v.axis == 'X' { "Y (`angle_deg` 90)" } else { "X (`angle_deg` 0)" };
        format!(
            "move the part {} mm or more towards {towards}{} on the table (the workholding's vise `center`, the stock's `offset` along \
             the jaws or `stock_at`), or keep the operation's moves beyond the stock shorter (a face's passes along {other}, a smaller \
             `overhang`)",
            num((over - 1e-6).ceil().max(1.0)),
            v.axis
        )
    };
    failures.push(format!(
        "travel: {op}line {}: {} would be at {} in machine coordinates, beyond the machine's soft limits {} to {} ({at}): {fix}",
        v.line,
        v.axis,
        num(v.value),
        num(v.low),
        num(v.high),
    ));
    Ok(())
}

/// The gouges along the moves of `p`, a chamfer mill told to cut `band` up its axis: into the part
/// less that band (its tip beside the wall, as it runs for a chamfer, is its own too), and into
/// the fixtures with the whole tool; the deeper of the two on each move.
fn op_band_gouges(p: &Planned, surfaces: &Surfaces, band: f64, allowed: f64, step: f64) -> std::result::Result<Vec<Hit>, CamError> {
    let tip_clear = p.report.kind.ends_with("chamfer");
    let mut hits = band_gouges(&surfaces.part, &p.tool, &p.poses, band, tip_clear, allowed, step)?;
    if let Some(fixtures) = &surfaces.fixtures {
        for h in tool_gouges(fixtures, &p.tool, &p.poses, allowed, step)? {
            match hits.iter_mut().find(|k| k.index == h.index) {
                Some(k) if h.distance > k.distance => *k = h,
                Some(_) => {}
                None => hits.push(h),
            }
        }
        hits.sort_by_key(|h| h.index);
    }
    Ok(hits)
}

/// For each operation whose toolpath `paths` gives in the program's own coordinates, the line
/// (counted from 1) of `text` each of its moves ends on, where it is found: the next line that
/// leaves the axes where the move ends (to within `digit` mm, the last written digit), looking
/// on from the move before. A move written as no line (too short to change a word) ends on the
/// line before. Words are in the post's units, `unit` mm each; lines in incremental
/// coordinates or that home the axes leave them unknown.
fn move_lines(text: &str, paths: &[Option<&Toolpath>], unit: f64, digit: f64) -> Vec<Vec<Option<usize>>> {
    // Every line that leaves all three axes known, and where it leaves them.
    let mut ends: Vec<(usize, Vec3)> = Vec::new();
    let mut at: [Option<f64>; 3] = [None; 3];
    let mut incremental = false;
    for (no, raw) in text.lines().enumerate() {
        let mut clean = String::with_capacity(raw.len());
        let mut depth = 0usize;
        for c in raw.chars() {
            match c {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ';' if depth == 0 => break,
                c if depth == 0 => clean.push(c.to_ascii_uppercase()),
                _ => {}
            }
        }
        let mut words: Vec<(char, f64)> = Vec::new();
        let mut chars = clean.chars().peekable();
        while let Some(c) = chars.next() {
            if !c.is_ascii_alphabetic() {
                continue;
            }
            let mut v = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() || d == '.' || d == '-' || d == '+' || d == ' ' {
                    if d != ' ' {
                        v.push(d);
                    }
                    chars.next();
                } else {
                    break;
                }
            }
            if let Ok(x) = v.parse::<f64>() {
                words.push((c, x));
            }
        }
        let g = |code: f64| words.iter().any(|&(c, v)| c == 'G' && (v - code).abs() < 1e-6);
        if g(91.0) {
            incremental = true;
        }
        if g(90.0) {
            incremental = false;
        }
        let homing = g(28.0) || g(53.0);
        let mut moved = false;
        for (k, letter) in ['X', 'Y', 'Z'].into_iter().enumerate() {
            if let Some(&(_, v)) = words.iter().find(|w| w.0 == letter)
                && let Some(slot) = at.get_mut(k)
            {
                *slot = if homing || incremental { None } else { Some(v * unit) };
                moved = true;
            }
        }
        if moved && let [Some(x), Some(y), Some(z)] = at {
            ends.push((no + 1, Vec3::new(x, y, z)));
        }
    }
    let near = 0.5 * digit + 1e-9;
    let mut cursor = 0usize;
    paths
        .iter()
        .map(|path| {
            let Some(path) = path else { return Vec::new() };
            path.moves
                .iter()
                .map(|m| {
                    let to = m.target()?;
                    let from = cursor.saturating_sub(1);
                    let found = ends
                        .get(from..ends.len().min(cursor.saturating_add(LINE_WINDOW)))?
                        .iter()
                        .position(|(_, p)| (p.x - to.x).abs() <= near && (p.y - to.y).abs() <= near && (p.z - to.z).abs() <= near)?;
                    let k = from + found;
                    cursor = k + 1;
                    ends.get(k).map(|(line, _)| *line)
                })
                .collect()
        })
        .collect()
}

/// `text` safe to put inside HTML.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn round(v: f64, k: f64) -> f64 {
    (v * k).round() / k
}

fn pose_rows(path: &Toolpath5) -> Vec<[f64; 7]> {
    path.points()
        .into_iter()
        .map(|(p, cut)| {
            [
                round(p.tip.x, 1000.0),
                round(p.tip.y, 1000.0),
                round(p.tip.z, 1000.0),
                round(p.axis.x, 1000.0),
                round(p.axis.y, 1000.0),
                round(p.axis.z, 1000.0),
                if cut { 1.0 } else { 0.0 },
            ]
        })
        .collect()
}

/// At most `max` of `rows`, evenly picked.
fn thin<T: Copy>(rows: &[T], max: usize) -> Vec<T> {
    let every = rows.len().div_ceil(max.max(1)).max(1);
    rows.iter().step_by(every).copied().collect()
}

/// The preview page: one scene per operation and one for the whole job, over the part's
/// triangles `tris` (nine numbers each).
fn preview_page(name: &str, tris: &[f64], planned: &[Planned], text: &str) -> String {
    let gcode: String = text.lines().take(PREVIEW_GCODE_LINES).fold(String::new(), |mut acc, l| {
        let _ = writeln!(acc, "{l}");
        acc
    });
    let stats = |r: &OperationReport| format!("{} moves · cut {} mm · {} min", r.moves, num(r.cut_length.round()), num(r.cut_minutes));
    let scene = |title: &str, axes: &str, short: &str, desc: &str, poses: Vec<[f64; 7]>, stats: String, tool: Option<&Tool>| {
        json!({
            "name": escape_html(title),
            "axes": escape_html(axes),
            "short": escape_html(short),
            "desc": escape_html(desc),
            "tris": tris,
            "poses": thin(&poses, PREVIEW_POSES),
            "gcode": gcode,
            "stats": stats,
            "tool_radius": tool.map_or(2.0, Tool::radius),
            "tool_length": tool.map_or(25.0, |t| t.stickout.unwrap_or(t.flute_length).max(t.flute_length)),
        })
    };
    let mut scenes = Vec::with_capacity(planned.len() + 1);
    let all: Vec<[f64; 7]> = planned.iter().flat_map(|p| pose_rows(&p.poses)).collect();
    let (moves, length, minutes) =
        planned.iter().fold((0, 0.0, 0.0), |(m, l, t), p| (m + p.report.moves, l + p.report.cut_length, t + p.report.cut_minutes));
    let job_axes: std::collections::BTreeSet<&str> = planned.iter().map(|p| p.axes.as_str()).collect();
    scenes.push(scene(
        name,
        &job_axes.into_iter().collect::<Vec<_>>().join(", "),
        &format!("the whole job, {} operations", planned.len()),
        &format!("Every operation of the job, in program order: {}.", planned.iter().map(|p| p.report.name.as_str()).collect::<Vec<_>>().join(", ")),
        all,
        format!("{moves} moves · cut {} mm · {} min", num(length.round()), num(minutes)),
        planned.first().map(|p| &p.tool),
    ));
    for p in planned {
        let t = &p.tool;
        let desc = format!(
            "{} with tool {} ({} mm {}{}).",
            p.report.kind,
            t.number,
            num(t.diameter),
            t.kind.label(),
            if t.description.is_empty() { String::new() } else { format!(", {}", t.description) }
        );
        scenes.push(scene(&p.report.name, &p.axes, &p.report.kind, &desc, pose_rows(&p.poses), stats(&p.report), Some(t)));
    }
    // `</` would end the script early; `<\/` is the same string in JSON and JavaScript.
    let data = json!({ "scenes": scenes }).to_string().replace("</", "<\\/");
    PREVIEW_TEMPLATE.replacen("/*DATA*/null", &data, 1)
}

/// `cam.post.list`.
fn post_list(_: &mut Session, p: &Value) -> Result<Value> {
    let _: CamPostListParams = parse("cam.post.list", p, "")?;
    let reply = CamPostListReply {
        posts: POSTS
            .iter()
            .map(|(id, description, aliases, _)| PostEntry {
                name: (*id).into(),
                description: (*description).into(),
                aliases: aliases.iter().map(|a| (*a).to_string()).collect(),
            })
            .collect(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// `cam.post.get`.
fn post_get(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.post.get";
    let params: CamPostGetParams = parse(cmd, p, "")?;
    let Some((id, post)) = builtin_post(&params.name) else {
        return Err(bad(cmd, format!("unknown post `{}` (built in: {})", params.name, post_names())));
    };
    let reply = CamPostGetReply {
        name: id.into(),
        post: serde_json::to_value(post).map_err(|e| EngineError::Other(e.to_string()))?,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// The motion of `text`, read with every rotary letter it uses: [`parse5`] refuses a rotary
/// word the machine lacks, so each refusal adds that letter and reads again.
fn parse_any(text: &str) -> (Vec<AxisLetter>, std::result::Result<Vec<JointWords>, CamError>) {
    let mut letters: Vec<AxisLetter> = Vec::new();
    loop {
        match parse5(text, &letters) {
            Err(CamError::Parse(line, msg)) if msg.starts_with("the machine has no ") => {
                let letter = match msg.trim_start_matches("the machine has no ").chars().next() {
                    Some('A') => AxisLetter::A,
                    Some('B') => AxisLetter::B,
                    Some('C') => AxisLetter::C,
                    _ => return (letters, Err(CamError::Parse(line, msg))),
                };
                if letters.contains(&letter) {
                    return (letters, Err(CamError::Parse(line, msg)));
                }
                letters.push(letter);
                letters.sort();
            }
            other => return (letters, other),
        }
    }
}

/// `cam.gcode.check`.
fn gcode_check(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.gcode.check";
    let params: CamGcodeCheckParams = parse(cmd, p, "")?;
    if params.rapid_rate.is_some_and(|r| !(r > 0.0 && r <= 1.0e6)) {
        return Err(bad(cmd, "`rapid_rate` must be above 0 and at most 1000000 mm/min"));
    }
    let bytes = solidcraft_io::vfs::read(&params.path, MAX_GCODE_BYTES).map_err(|e| EngineError::Other(format!("{}: {e}", params.path)))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut reply = CamGcodeCheckReply { path: params.path.clone(), lines: text.lines().count(), ..CamGcodeCheckReply::default() };
    let (letters, parsed) = parse_any(&text);
    // What the reader takes up to its first error, when there is one.
    let blocks = match parsed {
        Ok(b) => b,
        Err(e) => {
            let (line, message) = match e {
                CamError::Parse(line, msg) => (line, msg),
                other => (0, other.to_string()),
            };
            reply.errors.push(ParseErrorReport { line, message });
            let upto: String = text.lines().take(line.saturating_sub(1)).fold(String::new(), |mut acc, l| {
                acc.push_str(l);
                acc.push('\n');
                acc
            });
            parse5(&upto, &letters).unwrap_or_default()
        }
    };
    let mut ranges: Vec<(f64, f64)> = vec![(f64::INFINITY, f64::NEG_INFINITY); letters.len()];
    let (mut lo, mut hi) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    let mut last: Option<(Vec3, Vec<f64>)> = None;
    for b in &blocks {
        reply.motion_blocks += 1;
        let (linear, rotary) = match b {
            JointWords::Rapid { linear, rotary, .. } => {
                if let Some((from, _)) = &last {
                    reply.rapid_length += from.dist(*linear);
                }
                (*linear, rotary.clone())
            }
            JointWords::Feed { linear, rotary, f, inverse_time, mm_per_unit, .. } => {
                let (len, turn) = last
                    .as_ref()
                    .map_or((0.0, 0.0), |(from, r)| (from.dist(*linear), r.iter().zip(rotary).map(|(a, b)| (b - a).abs()).fold(0.0, f64::max)));
                reply.feed_length += len;
                if *inverse_time {
                    reply.g93_blocks += 1;
                    reply.cut_minutes += 1.0 / f;
                } else if len > 1e-9 {
                    reply.cut_minutes += len / (f * mm_per_unit);
                } else {
                    reply.cut_minutes += turn.to_degrees() / f;
                }
                (*linear, rotary.clone())
            }
            JointWords::Arc { linear, rotary, f, mm_per_unit, .. } | JointWords::PlaneArc { linear, rotary, f, mm_per_unit, .. } => {
                reply.arcs += 1;
                // Measured round the arc in its own plane (G17, G18 or G19).
                if let Some((from, _)) = &last
                    && let Some(arc) = b.arc_from(*from)
                {
                    let len = arc.length();
                    reply.feed_length += len;
                    reply.cut_minutes += len / (f * mm_per_unit);
                    // A vertical arc can reach past its ends along Z.
                    if matches!(b, JointWords::PlaneArc { .. }) {
                        for q in arc.points(1e-3) {
                            lo = lo.min(q);
                            hi = hi.max(q);
                        }
                    }
                }
                (*linear, rotary.clone())
            }
            JointWords::Dwell { seconds, .. } => {
                reply.cut_minutes += seconds / 60.0;
                continue;
            }
        };
        lo = lo.min(linear);
        hi = hi.max(linear);
        for (range, a) in ranges.iter_mut().zip(&rotary) {
            range.0 = range.0.min(*a);
            range.1 = range.1.max(*a);
        }
        last = Some((linear, rotary));
    }
    reply.rotary_ranges = letters
        .iter()
        .zip(&ranges)
        .filter(|(_, r)| r.0 <= r.1)
        .map(|(l, r)| RotaryRange { letter: l.char().to_string(), min_deg: r.0.to_degrees(), max_deg: r.1.to_degrees() })
        .collect();
    if lo.is_finite() && hi.is_finite() {
        reply.bounds = Some([[lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]]);
    }
    reply.rapid_minutes = params.rapid_rate.map(|r| reply.rapid_length / r);
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

#[path = "cam_features.rs"]
mod features;
#[path = "cam_image.rs"]
mod pictures;
#[path = "cam_programs.rs"]
mod programs;
#[path = "cam_rates.rs"]
mod rates;
#[path = "cam_sheet.rs"]
mod sheet;
#[path = "cam_sim.rs"]
mod sim;
#[path = "cam_view.rs"]
mod view;

#[cfg(test)]
#[path = "cam_tests.rs"]
mod tests;
