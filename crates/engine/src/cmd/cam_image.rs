//! Pictures of a CAM job's toolpaths for agents (`cam.job {image, images_dir}` and `cam.view`):
//! an agent can look at a PNG but cannot open the HTML preview. The part is the design's own
//! picture (the scene the MCP `screenshot` tool renders, without the ground grid and the
//! origin's axes), drawn by the renderer the host installed or, headless without one, by the CPU
//! rasterizer, so no GPU is needed. The toolpaths, the tool, work zero and the rotary axes are
//! then drawn over it in software with the same camera's projection, so they show whatever drew
//! the part: cutting moves in one colour per operation, rapids dashed and lighter, plunges darker,
//! entries ringed, and every stretch the part hides from the camera faint (a ray towards the eye
//! meets the job's tessellated surface: what the picture shows, not a model decision). A legend
//! in the corner names the operations in the built-in font.
//!
//! Every `cam.job` reply also carries each toolpath's extents in work coordinates and the range
//! of each rotary axis as the machine turns it ([`extents`]), so an agent can check a toolpath
//! without a picture at all.

use std::path::Path;

use serde_json::{Map, Value, json};
use solidcore_record::{Vec2, Vec3};
use solidcraft_cam::gcode5::{JointWords, MachineOperation, MachineProgram, Post5, emit_program, parse5};
use solidcraft_cam::kinematics::{Carrier, Joints, Machine, inverse};
use solidcraft_cam::{CommentStyle, Move5, Pose, Surface};
use solidcraft_geom::Aabb3;
use solidcraft_geom::prelude::*;
use solidcraft_viewport::{Camera, StandardView};

use super::{Planned, num, parse, run_job};
use crate::cmd::CommandSpec;
use crate::params::bad;
use crate::schema::cam::{CamJobParams, RotaryRange};
use crate::schema::cam_image::{CamViewParams, CamViewReply, ImagesReport, LegendEntry, ToolpathExtents, ViewCamera, ViewOperation};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const VIEW_CONTRACT: TypedContract<CamViewParams> = TypedContract::new();

pub(crate) static COMMANDS: &[CommandSpec] = &[CommandSpec::new("cam.view", "CAM View", crate::checked!("cam.view", view))
    .noundo()
    .icon("play")
    .contract(&VIEW_CONTRACT)
    .output(output_schema_for::<CamViewReply>)
    .params(
        "job: {…} | job_path: .json (as cam.job); path: the PNG to write; op?: an operation's name (default: every operation); \
         view?: iso | top | bottom | front | back | left | right (default iso, or front with a rotary axis: from -Y, the A axis \
         level); width?, height?: 16-4096 px (960 × 600) → path, image: {view, width, height, renderer, camera (as view.apply \
         takes it), legend: [{operation, colour, rgb}], key}, operations: [{name, kind, drawn, extents: {all, cut?, rotary, \
         summary}}], passed, \
         failures, notes. Plans and checks the job as cam.job {dry_run: true} and writes no G-code",
    )];

/// Picture sizes accepted and the default (pixels).
const SIZE_RANGE: (u32, u32) = (16, 4096);
const DEFAULT_SIZE: (u32, u32) = (960, 600);
const VIEW_NAMES: &str = "iso, top, bottom, front, back, left, right";
/// Most ray casts one picture makes to find what the part hides; past it the rest is drawn as
/// seen.
const MAX_RAYS: usize = 400_000;
/// Longest stretch (pixels) drawn as one piece, seen or hidden.
const PIECE_PX: f64 = 6.0;
/// Most pieces one move is cut into.
const MAX_PIECES: usize = 64;
/// Entries marked per operation.
const MAX_ENTRIES: usize = 2_000;
/// Operations the legend names one by one.
const LEGEND_ROWS: usize = 12;
/// Longest name the legend writes (characters).
const LEGEND_CHARS: usize = 48;

/// The operations' colours, in program order (repeating), away from the axes' red, green and blue.
const PALETTE: [(&str, [u8; 3]); 8] = [
    ("orange", [240, 120, 0]),
    ("purple", [140, 60, 200]),
    ("teal", [0, 150, 150]),
    ("magenta", [215, 0, 140]),
    ("ochre", [170, 120, 0]),
    ("brown", [125, 70, 35]),
    ("pink", [245, 95, 130]),
    ("gold", [225, 185, 0]),
];
const X_COLOUR: [u8; 3] = [215, 35, 35];
const Y_COLOUR: [u8; 3] = [30, 150, 50];
const Z_COLOUR: [u8; 3] = [40, 70, 225];
const ROTARY_COLOUR: [u8; 3] = [35, 35, 45];
const TOOL_COLOUR: [u8; 3] = [60, 60, 70];
const TEXT_COLOUR: [u8; 3] = [25, 25, 30];
const KEY_COLOUR: [u8; 3] = [90, 90, 100];

/// An operation's colour (by its index in the job).
pub(super) fn colour_of(index: usize) -> [u8; 3] {
    PALETTE.get(index % PALETTE.len()).map_or([0, 0, 0], |c| c.1)
}

fn colour_name(index: usize) -> &'static str {
    PALETTE.get(index % PALETTE.len()).map_or("black", |c| c.0)
}

/// What a call asks to be drawn, its paths checked.
pub(super) struct Request {
    image: Option<String>,
    dir: Option<String>,
    view: Option<StandardView>,
    size: (u32, u32),
    only: Option<String>,
}

/// At most `n` characters of `s`, for messages.
fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

impl Request {
    /// The pictures `p` asks for (`only`: draw that operation alone), every path checked against
    /// the session's folders before anything is computed.
    pub(super) fn of(cmd: &str, p: &CamJobParams, only: Option<&str>) -> Result<Request> {
        let path = |v: &Option<String>, what: &str| -> Result<Option<String>> {
            match v {
                None => Ok(None),
                Some(x) if x.trim().is_empty() => Err(bad(cmd, format!("`{what}` must not be empty"))),
                Some(x) => {
                    solidcraft_io::vfs::check(x).map_err(|e| EngineError::Other(e.to_string()))?;
                    Ok(Some(x.clone()))
                }
            }
        };
        let image = path(&p.image, "image")?;
        let dir = path(&p.images_dir, "images_dir")?;
        if image.is_some() && (image == p.output || image == p.preview) {
            return Err(bad(cmd, "`image` must be a file of its own, not the `output` or the `preview`"));
        }
        if image.is_none() && dir.is_none() && (p.view.is_some() || p.width.is_some() || p.height.is_some()) {
            return Err(bad(
                cmd,
                "`view`, `width` and `height` shape the pictures: give `image` (one picture of the job) or `images_dir` (one per operation)",
            ));
        }
        let view = match &p.view {
            None => None,
            Some(v) => Some(StandardView::parse(v.trim()).ok_or_else(|| bad(cmd, format!("unknown view `{}`: {VIEW_NAMES}", short(v, 40))))?),
        };
        let size = |v: Option<u32>, d: u32, what: &str| match v {
            None => Ok(d),
            Some(n) if (SIZE_RANGE.0..=SIZE_RANGE.1).contains(&n) => Ok(n),
            Some(_) => Err(bad(cmd, format!("`{what}` must be {} to {} pixels", SIZE_RANGE.0, SIZE_RANGE.1))),
        };
        let size = (size(p.width, DEFAULT_SIZE.0, "width")?, size(p.height, DEFAULT_SIZE.1, "height")?);
        Ok(Request { image, dir, view, size, only: only.map(str::to_string) })
    }

    pub(super) fn wanted(&self) -> bool {
        self.image.is_some() || self.dir.is_some()
    }
}

/// A box as `[[low x, y, z], [high x, y, z]]`.
fn corners(b: &Aabb3) -> [[f64; 3]; 2] {
    [[b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]]
}

/// Marks each operation's first line in a copy of the program, so its blocks can be told apart
/// once read back.
fn marker(k: usize) -> String {
    format!("SCOPMARK{k}")
}

/// Each operation's rotary range as the program writes it, radians: the program written again
/// with a comment naming each operation (comments move nothing), read back, and every block
/// counted to the operation whose comment comes before it. The post unwinds the angles within
/// the machine's limits (rewinds) and writes an indexed operation at its own angle, so this is
/// what the machine turns, not what chaining the poses gives. `None` when it cannot be read.
fn written_rotary(machine: &Machine, post: &Post5, program: &MachineProgram) -> Option<Vec<Vec<(f64, f64)>>> {
    let mut marked = program.clone();
    marked.name = "PROGRAM".into();
    for (k, op) in marked.operations.iter_mut().enumerate() {
        match op {
            MachineOperation::Continuous(o) => o.name = marker(k),
            MachineOperation::Indexed(o) => o.name = marker(k),
        }
    }
    let mut post = post.clone();
    post.base.comments = CommentStyle::Parentheses;
    post.base.operation_name_only = true;
    let text = emit_program(&marked, &post, machine).ok()?;
    let words = parse5(&text, &machine.letters()).ok()?;
    // The line (from 1) each operation starts on.
    let mut starts: Vec<usize> = Vec::with_capacity(marked.operations.len());
    let mut lines = text.lines().enumerate();
    for k in 0..marked.operations.len() {
        let tag = format!("({})", marker(k));
        let (n, _) = lines.by_ref().find(|(_, l)| l.contains(&tag))?;
        starts.push(n + 1);
    }
    let mut ranges = vec![vec![(f64::INFINITY, f64::NEG_INFINITY); machine.rotaries.len()]; starts.len()];
    for w in &words {
        let (line, rotary) = match w {
            JointWords::Rapid { line, rotary, .. }
            | JointWords::Feed { line, rotary, .. }
            | JointWords::Arc { line, rotary, .. }
            | JointWords::PlaneArc { line, rotary, .. } => (*line, rotary),
            JointWords::Dwell { .. } => continue,
        };
        let Some(op) = starts.iter().rposition(|&s| s < line) else { continue };
        if let Some(r) = ranges.get_mut(op) {
            for (span, a) in r.iter_mut().zip(rotary) {
                *span = (span.0.min(*a), span.1.max(*a));
            }
        }
    }
    Some(ranges)
}

/// Sets every operation's toolpath extents: the tip's box over every move and over the cutting
/// moves, in work coordinates, and each rotary's range as the written program turns it (as
/// [`written_rotary`] reads it, else the poses' angles chained from one operation to the next).
pub(super) fn extents(machine: &Machine, post: &Post5, program: &MachineProgram, planned: &mut [Planned]) {
    let letters: Vec<char> = machine.rotaries.iter().map(|r| r.letter.char()).collect();
    let written = if letters.is_empty() { None } else { written_rotary(machine, post, program) };
    let mut joints: Option<Joints> = None;
    for (k, p) in planned.iter_mut().enumerate() {
        let (mut all, mut cut) = (Aabb3::EMPTY, Aabb3::EMPTY);
        let mut ranges = vec![(f64::INFINITY, f64::NEG_INFINITY); letters.len()];
        let mut last: Option<Vec3> = None;
        for m in &p.poses.moves {
            let Some(to) = m.target() else { continue };
            all.add(to.tip);
            if matches!(m, Move5::Feed { .. }) {
                cut.add(to.tip);
                if let Some(from) = last {
                    cut.add(from);
                }
            }
            last = Some(to.tip);
            if written.is_none()
                && !letters.is_empty()
                && let Ok(j) = inverse(machine, &to, joints.as_ref())
            {
                for (r, a) in ranges.iter_mut().zip(&j.rotary) {
                    r.0 = r.0.min(*a);
                    r.1 = r.1.max(*a);
                }
                joints = Some(j);
            }
        }
        if let Some(w) = written.as_ref().and_then(|w| w.get(k)) {
            ranges.clone_from(w);
        }
        let rotary: Vec<RotaryRange> = letters
            .iter()
            .zip(&ranges)
            .filter(|(_, r)| r.0 <= r.1)
            .map(|(l, r)| RotaryRange { letter: l.to_string(), min_deg: r.0.to_degrees(), max_deg: r.1.to_degrees() })
            .collect();
        let span = |lo: f64, hi: f64| format!("{}..{}", num(lo), num(hi));
        let mut summary = if cut.is_empty() {
            "no cutting moves".to_string()
        } else {
            format!("cut X {} Y {} Z {}", span(cut.min.x, cut.max.x), span(cut.min.y, cut.max.y), span(cut.min.z, cut.max.z))
        };
        if !all.is_empty() {
            let _ = std::fmt::Write::write_fmt(
                &mut summary,
                format_args!("; all moves X {} Y {} Z {}", span(all.min.x, all.max.x), span(all.min.y, all.max.y), span(all.min.z, all.max.z)),
            );
        }
        for r in &rotary {
            let _ = std::fmt::Write::write_fmt(&mut summary, format_args!("; {} {}°", r.letter, span(r.min_deg, r.max_deg)));
        }
        p.report.extents = ToolpathExtents {
            all: if all.is_empty() { [[0.0; 3]; 2] } else { corners(&all) },
            cut: (!cut.is_empty()).then(|| corners(&cut)),
            rotary,
            summary,
        };
    }
}

fn view_name(v: StandardView) -> &'static str {
    match v {
        StandardView::Front => "front",
        StandardView::Back => "back",
        StandardView::Top => "top",
        StandardView::Bottom => "bottom",
        StandardView::Left => "left",
        StandardView::Right => "right",
        StandardView::Iso => "iso",
    }
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// How to read the pictures.
fn key(machine: &Machine) -> Vec<String> {
    let mut k = vec![
        "solid lines: cutting moves, one colour per operation (legend)".to_string(),
        "dashed lighter lines: rapids".into(),
        "darker, thicker lines: plunges (feeding down the tool axis)".into(),
        "ringed dots: entries, where a cut starts after a rapid".into(),
        "faint lines: moves the part hides from this view".into(),
        "grey shape: the tool where the first operation drawn starts its first cut".into(),
        "red, green and blue arrows: work zero's X, Y and Z".into(),
    ];
    if machine.rotaries.iter().any(|r| r.carrier == Carrier::Table) {
        k.push("dark dash-dot line with a ring: a rotary table's axis (A); the ring's arrow turns as a positive angle turns the part".into());
    }
    k
}

/// What every picture of a job is drawn from.
pub(super) struct Setting<'a> {
    pub machine: &'a Machine,
    /// Work zero in the design.
    pub origin: Vec3,
    /// The part in work coordinates: what hides a stretch of toolpath from the camera.
    pub surface: &'a Surface,
    pub tol: f64,
    /// The job's name.
    pub name: &'a str,
}

/// Draws the pictures `req` asks for → what was drawn, `None` when it asks for none.
pub(super) fn write(cmd: &str, s: &Session, req: &Request, set: &Setting<'_>, planned: &[Planned]) -> Result<Option<ImagesReport>> {
    if !req.wanted() {
        return Ok(None);
    }
    let only = match &req.only {
        None => None,
        Some(name) => match planned.iter().position(|p| p.report.name == *name) {
            Some(i) => Some(i),
            None => {
                let names: Vec<String> = planned.iter().map(|p| format!("`{}`", p.report.name)).collect();
                return Err(bad(cmd, format!("no operation `{}` in the job (it has {})", short(name, 80), names.join(", "))));
            }
        },
    };
    let view = req.view.unwrap_or(if set.machine.rotaries.is_empty() { StandardView::Iso } else { StandardView::Front });
    let (w, h) = req.size;
    let mut report = ImagesReport {
        view: view_name(view).into(),
        width: w,
        height: h,
        legend: planned
            .iter()
            .enumerate()
            .map(|(i, p)| LegendEntry { operation: p.report.name.clone(), colour: colour_name(i).into(), rgb: hex(colour_of(i)) })
            .collect(),
        key: key(set.machine),
        ..ImagesReport::default()
    };
    let every: Vec<usize> = (0..planned.len()).collect();
    if let Some(path) = &req.image {
        let ops = only.map_or_else(|| every.clone(), |i| vec![i]);
        let cam = camera(view, set, planned, &ops, w, h);
        let (base, renderer) = base_picture(s, &cam, w, h)?;
        let png = picture(base, &cam, set, planned, &ops, view, w, h)?;
        super::write_file(path, &png).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        report.image = Some(path.clone());
        report.renderer = renderer.into();
        report.camera = camera_report(&cam);
    }
    if let Some(dir) = &req.dir {
        solidcraft_io::vfs::create_dir_all(dir).map_err(|e| EngineError::Other(format!("{dir}: {e}")))?;
        let cam = camera(view, set, planned, &every, w, h);
        let (base, renderer) = base_picture(s, &cam, w, h)?;
        for (i, p) in planned.iter().enumerate() {
            let file = Path::new(dir).join(format!("{:02}-{}.png", i + 1, slug(&p.report.name))).to_string_lossy().into_owned();
            let png = picture(base.clone(), &cam, set, planned, &[i], view, w, h)?;
            super::write_file(&file, &png).map_err(|e| EngineError::Other(format!("{file}: {e}")))?;
            report.images.push(file);
        }
        if report.image.is_none() {
            report.renderer = renderer.into();
            report.camera = camera_report(&cam);
        }
    }
    Ok(Some(report))
}

/// An operation's name as a file name: lower-case letters and digits, dashes between.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() { "operation".into() } else { out.into() }
}

fn camera_report(cam: &Camera) -> ViewCamera {
    ViewCamera { target: [cam.target.x, cam.target.y, cam.target.z], yaw: cam.yaw, pitch: cam.pitch, distance: cam.distance, fov: cam.fov }
}

/// The tool's length drawn: its stickout, at least its flutes.
fn tool_length(p: &Planned) -> f64 {
    p.tool.stickout.unwrap_or(p.tool.flute_length).max(p.tool.flute_length)
}

/// Where an operation's tool is drawn: where its first cut starts, else its first pose.
fn tool_pose(p: &Planned) -> Option<Pose> {
    let mut before: Option<Pose> = None;
    for m in &p.poses.moves {
        if let (Move5::Feed { .. }, Some(b)) = (m, before) {
            return Some(b);
        }
        if let Some(t) = m.target() {
            before = Some(t);
        }
    }
    p.poses.moves.iter().find_map(Move5::target)
}

/// An orthographic camera from `view` framing the part, work zero and the drawn toolpaths.
fn camera(view: StandardView, set: &Setting<'_>, planned: &[Planned], ops: &[usize], w: u32, h: u32) -> Camera {
    let mut b = set.surface.bounds();
    b.add(Vec3::ZERO);
    for p in ops.iter().filter_map(|&i| planned.get(i)) {
        for pose in p.poses.poses() {
            b.add(pose.tip);
        }
    }
    if let Some(p) = ops.first().and_then(|&i| planned.get(i))
        && let Some(t) = tool_pose(p)
    {
        b.add(t.up(tool_length(p)));
    }
    // The work box back in the design, as drawn.
    let b = if b.min.is_finite() && b.max.is_finite() { Aabb3 { min: b.min + set.origin, max: b.max + set.origin } } else { b };
    let mut cam = Camera::default();
    cam.set_view(view);
    if b.min.is_finite() && b.max.is_finite() {
        // A little more room than a plain fit, for the legend in the corner.
        let room = b.inflated(b.diagonal() * 0.06);
        cam.fit_aspect(&solidcraft_render::view_bounds(&room), f64::from(w) / f64::from(h.max(1)));
    }
    cam
}

/// The design drawn from `cam`, without the grid and the origin's axes: by the renderer the host
/// installed, or the CPU rasterizer when there is none or it fails.
fn base_picture(s: &Session, cam: &Camera, w: u32, h: u32) -> Result<(Vec<u8>, &'static str)> {
    let scene = crate::view::scene_with_guides(s, cam, None, crate::view::Guides { grid: false, axes: false })?;
    if let Some(r) = crate::capture::installed() {
        match crate::capture::rgba_with(Some(&*r), &scene, cam, w, h) {
            Ok(p) => return Ok((p.rgba, p.renderer)),
            Err(e) => log::warn!("cam picture: the installed renderer failed ({e}); drawing the part on the CPU"),
        }
    }
    let c = solidcraft_viewport::render(&scene, cam, w as usize, h as usize);
    if c.rgba.len() as u64 != u64::from(w) * u64::from(h) * 4 {
        return Err(EngineError::Other(format!("the CPU rasterizer drew {} bytes for {w} × {h} pixels", c.rgba.len())));
    }
    Ok((c.rgba, "cpu"))
}

/// One picture: the toolpaths of `ops` drawn over `base` (the part), as PNG.
#[allow(clippy::too_many_arguments)]
fn picture(
    base: Vec<u8>,
    cam: &Camera,
    set: &Setting<'_>,
    planned: &[Planned],
    ops: &[usize],
    view: StandardView,
    w: u32,
    h: u32,
) -> Result<Vec<u8>> {
    let mut o = Overlay::new(base, cam, set, w, h);
    for &i in ops {
        if let Some(p) = planned.get(i) {
            o.toolpath(p, colour_of(i));
        }
    }
    for &i in ops {
        if let Some(p) = planned.get(i) {
            o.entries(p, colour_of(i));
        }
    }
    if let Some(&i) = ops.first()
        && let Some(p) = planned.get(i)
    {
        o.tool(p, colour_of(i));
    }
    o.rotary_axes(set.machine);
    o.work_zero();
    o.legend(set.name, planned, ops, view);
    solidcraft_viewport::png(w as usize, h as usize, o.ink.rgba).ok_or_else(|| EngineError::Other("PNG encoding failed".into()))
}

/// A point on the picture, x right and y down, in pixels.
type P = (f64, f64);

fn mix(a: [u8; 3], b: [u8; 3], t: f64) -> [f64; 3] {
    let f = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t) / 255.0;
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

fn plain(c: [u8; 3]) -> [f64; 3] {
    mix(c, c, 0.0)
}

/// RGBA pixels drawn on in software: anti-aliased lines, dots and filled shapes.
struct Ink {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
}

impl Ink {
    fn blend(&mut self, x: i64, y: i64, c: [f64; 3], a: f64) {
        if x < 0 || y < 0 || !(a > 0.0) {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.w || y >= self.h {
            return;
        }
        let a = a.min(1.0);
        let i = (y * self.w + x) * 4;
        if let Some(px) = self.rgba.get_mut(i..i + 4) {
            for (v, c) in px.iter_mut().zip(c) {
                let old = f64::from(*v) / 255.0;
                *v = ((old * (1.0 - a) + c * a).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            if let Some(alpha) = px.get_mut(3) {
                *alpha = 255;
            }
        }
    }

    /// The part of `a`–`b` inside the picture grown by `m` (Liang–Barsky), or none.
    fn clip(&self, a: P, b: P, m: f64) -> Option<(P, P)> {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let edges = [(-dx, a.0 + m), (dx, self.w as f64 + m - a.0), (-dy, a.1 + m), (dy, self.h as f64 + m - a.1)];
        for (p, q) in edges {
            if p.abs() < 1e-12 {
                if q < 0.0 {
                    return None;
                }
                continue;
            }
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return None;
            }
        }
        Some(((a.0 + dx * t0, a.1 + dy * t0), (a.0 + dx * t1, a.1 + dy * t1)))
    }

    /// A line `width` pixels wide from `a` to `b`, round-ended.
    fn segment(&mut self, a: P, b: P, c: [f64; 3], width: f64, alpha: f64) {
        if ![a.0, a.1, b.0, b.1, width, alpha].iter().all(|v| v.is_finite()) {
            return;
        }
        let hw = (width * 0.5).clamp(0.25, 50.0);
        let reach = hw + 1.0;
        let Some((a, b)) = self.clip(a, b, reach) else { return };
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        let dist = |x: f64, y: f64| {
            let t = if len2 > 1e-12 { (((x - a.0) * dx + (y - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            (x - a.0 - dx * t).hypot(y - a.1 - dy * t)
        };
        // Along the longer direction one column (or row) at a time, across it the band the line covers.
        let steep = dy.abs() > dx.abs();
        let (ua, ub, va, du, dv) = if steep { (a.1, b.1, a.0, dy, dx) } else { (a.0, b.0, a.1, dx, dy) };
        let ratio = if du.abs() > 1e-9 { (len2.sqrt() / du.abs()).min(1.5) } else { 1.0 };
        let span = reach * ratio + 1.0;
        let (lo, hi) = (ua.min(ub) - reach, ua.max(ub) + reach);
        let mut u = lo.floor() as i64;
        while (u as f64) <= hi.ceil() {
            let pu = u as f64 + 0.5;
            let t = if du.abs() > 1e-9 { ((pu - ua) / du).clamp(0.0, 1.0) } else { 0.0 };
            let vc = va + dv * t;
            let mut v = (vc - span).floor() as i64;
            while (v as f64) <= (vc + span).ceil() {
                let pv = v as f64 + 0.5;
                let (x, y) = if steep { (pv, pu) } else { (pu, pv) };
                let cover = (hw + 0.5 - dist(x, y)).clamp(0.0, 1.0);
                if cover > 0.0 {
                    let (px, py) = if steep { (v, u) } else { (u, v) };
                    self.blend(px, py, c, alpha * cover);
                }
                v += 1;
            }
            u += 1;
        }
    }

    /// A dashed line, `phase` carrying the pattern on from the line before.
    fn dashed(&mut self, a: P, b: P, c: [f64; 3], width: f64, alpha: f64, phase: &mut f64) {
        const ON: f64 = 6.0;
        const PERIOD: f64 = 10.0;
        let len = (b.0 - a.0).hypot(b.1 - a.1);
        if !len.is_finite() || len > 1e5 {
            // Off the picture by far: dashes there would never show.
            if let Some((ca, cb)) = self.clip(a, b, 2.0) {
                self.dashed(ca, cb, c, width, alpha, phase);
            }
            return;
        }
        let at = |d: f64| {
            let t = if len > 0.0 { d / len } else { 0.0 };
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        };
        let mut d = 0.0;
        while d < len {
            let into = phase.rem_euclid(PERIOD);
            let step = if into < ON { ON - into } else { PERIOD - into };
            let end = (d + step).min(len);
            if into < ON {
                self.segment(at(d), at(end), c, width, alpha);
            }
            *phase += end - d;
            d = end;
        }
    }

    fn disc(&mut self, at: P, r: f64, c: [f64; 3], alpha: f64) {
        if !(at.0.is_finite() && at.1.is_finite())
            || at.0 < -r - 2.0
            || at.1 < -r - 2.0
            || at.0 > self.w as f64 + r + 2.0
            || at.1 > self.h as f64 + r + 2.0
        {
            return;
        }
        let (x0, x1) = ((at.0 - r - 1.0).floor() as i64, (at.0 + r + 1.0).ceil() as i64);
        let (y0, y1) = ((at.1 - r - 1.0).floor() as i64, (at.1 + r + 1.0).ceil() as i64);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = (x as f64 + 0.5 - at.0).hypot(y as f64 + 0.5 - at.1);
                self.blend(x, y, c, alpha * (r + 0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    /// The inside of `edges` (even-odd), anti-aliased across and sampled four times down each row.
    fn fill(&mut self, edges: &[(P, P)], c: [f64; 3], alpha: f64) {
        let finite = |p: &P| p.0.is_finite() && p.1.is_finite();
        if edges.is_empty() || !edges.iter().all(|(a, b)| finite(a) && finite(b)) {
            return;
        }
        let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for (a, b) in edges {
            x0 = x0.min(a.0).min(b.0);
            x1 = x1.max(a.0).max(b.0);
            y0 = y0.min(a.1).min(b.1);
            y1 = y1.max(a.1).max(b.1);
        }
        let x0 = x0.floor().max(0.0) as i64;
        let x1 = x1.ceil().min(self.w as f64) as i64;
        let y0 = y0.floor().max(0.0) as i64;
        let y1 = y1.ceil().min(self.h as f64) as i64;
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let cols = (x1 - x0) as usize;
        let mut cover = vec![0.0f64; cols];
        let mut xs: Vec<f64> = Vec::new();
        for y in y0..y1 {
            cover.iter_mut().for_each(|v| *v = 0.0);
            for k in 0..4 {
                let sy = y as f64 + (f64::from(k) + 0.5) / 4.0;
                xs.clear();
                for (a, b) in edges {
                    if (a.1 <= sy) != (b.1 <= sy) {
                        xs.push(a.0 + (sy - a.1) * (b.0 - a.0) / (b.1 - a.1));
                    }
                }
                xs.sort_by(f64::total_cmp);
                for [l, r] in xs.as_chunks::<2>().0 {
                    let (l, r) = (l.max(x0 as f64), r.min(x1 as f64));
                    if r <= l {
                        continue;
                    }
                    let mut col = l.floor() as i64;
                    while (col as f64) < r {
                        let overlap = r.min(col as f64 + 1.0) - l.max(col as f64);
                        if let Some(v) = usize::try_from(col - x0).ok().and_then(|i| cover.get_mut(i)) {
                            *v += overlap.max(0.0) * 0.25;
                        }
                        col += 1;
                    }
                }
            }
            for (i, v) in cover.iter().enumerate() {
                if *v > 0.0 {
                    self.blend(x0 + i as i64, y, c, alpha * v.min(1.0));
                }
            }
        }
    }

    /// `text` in the built-in font, its baseline starting at `at`, capitals `cap` pixels tall →
    /// how far it reaches to the right.
    fn text(&mut self, text: &str, at: P, cap: f64, c: [f64; 3]) -> f64 {
        let Ok(geom) = solidcraft_sketch::text_geometry(text, Vec2::new(0.0, 0.0), cap, 0.0) else { return 0.0 };
        let mut edges: Vec<(P, P)> = Vec::new();
        let put = |q: Vec2| (at.0 + q.x, at.1 - q.y);
        let chain = |pts: &[Vec2], edges: &mut Vec<(P, P)>| {
            for w in pts.windows(2) {
                if let [a, b] = w {
                    edges.push((put(*a), put(*b)));
                }
            }
        };
        for g in &geom {
            match g {
                solidcraft_sketch::LinkGeom::Line(a, b) => chain(&[*a, *b], &mut edges),
                solidcraft_sketch::LinkGeom::Conic { a, apex, b, rho } => {
                    // A rational quadratic: weight rho / (1 - rho) on the apex (0.5: a plain Bézier).
                    let wgt = if *rho > 0.0 && *rho < 1.0 { rho / (1.0 - rho) } else { 1.0 };
                    let pts: Vec<Vec2> = (0..=8)
                        .map(|i| {
                            let t = f64::from(i) / 8.0;
                            let (p, q, r) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * wgt, t * t);
                            let d = p + q + r;
                            Vec2::new((a.x * p + apex.x * q + b.x * r) / d, (a.y * p + apex.y * q + b.y * r) / d)
                        })
                        .collect();
                    chain(&pts, &mut edges);
                }
                solidcraft_sketch::LinkGeom::Spline { pts, periodic, .. } => {
                    let mut pts = pts.clone();
                    if *periodic && let Some(f) = pts.first().copied() {
                        pts.push(f);
                    }
                    chain(&pts, &mut edges);
                }
                _ => {}
            }
        }
        let reach = edges.iter().map(|(a, b)| a.0.max(b.0)).fold(at.0, f64::max) - at.0;
        self.fill(&edges, c, 1.0);
        reach
    }
}

/// The overlay of one picture: the camera's projection onto its pixels, and what hides what.
struct Overlay<'a> {
    ink: Ink,
    cam: &'a Camera,
    set: &'a Setting<'a>,
    radius: f64,
    /// From the part towards the eye (the camera is orthographic).
    back: Vec3,
    /// How far from a point a ray towards the eye starts, so the surface the tip touches does
    /// not hide it.
    lift: f64,
    rays: usize,
    /// The picture's scale: a few percent of the part's size, mm.
    size: f64,
}

impl<'a> Overlay<'a> {
    fn new(rgba: Vec<u8>, cam: &'a Camera, set: &'a Setting<'a>, w: u32, h: u32) -> Overlay<'a> {
        let b = set.surface.bounds();
        let diag = if b.is_empty() { 10.0 } else { b.diagonal().max(1.0) };
        Overlay {
            ink: Ink { w: w as usize, h: h as usize, rgba },
            cam,
            set,
            radius: cam.distance.max(diag) * 4.0,
            back: cam.back(),
            lift: (set.tol * 4.0).max(0.02),
            rays: 0,
            size: diag,
        }
    }

    /// A point in work coordinates on the picture.
    fn screen(&self, p: Vec3) -> Option<P> {
        let (w, h) = (self.ink.w as f64, self.ink.h as f64);
        self.cam.to_screen(p + self.set.origin, w, h, self.radius).map(|(x, y, _)| (x, y)).filter(|(x, y)| x.is_finite() && y.is_finite())
    }

    /// Whether the part hides the work point `p` from the camera.
    fn hidden(&mut self, p: Vec3) -> bool {
        if self.rays >= MAX_RAYS {
            return false;
        }
        self.rays += 1;
        self.set.surface.raycast(p + self.back * self.lift, self.back).is_some()
    }

    /// The move from `a` to `b` (work coordinates), cut into pieces seen or hidden.
    fn pieces(&mut self, a: Vec3, b: Vec3, sa: P, sb: P) -> Vec<(P, P, bool)> {
        let px = (sb.0 - sa.0).hypot(sb.1 - sa.1);
        let n = ((px / PIECE_PX).ceil() as usize).clamp(1, MAX_PIECES);
        let mut out = Vec::with_capacity(n);
        let mut from = sa;
        for k in 0..n {
            let t1 = (k + 1) as f64 / n as f64;
            let tm = (k as f64 + 0.5) / n as f64;
            let to = (sa.0 + (sb.0 - sa.0) * t1, sa.1 + (sb.1 - sa.1) * t1);
            let hidden = self.hidden(a.lerp(b, tm));
            match out.last_mut() {
                Some((_, end, h)) if *h == hidden => *end = to,
                _ => out.push((from, to, hidden)),
            }
            from = to;
        }
        out
    }

    /// An operation's toolpath: cuts solid, plunges darker, rapids dashed and lighter, hidden
    /// stretches faint.
    fn toolpath(&mut self, p: &Planned, colour: [u8; 3]) {
        let cut = plain(colour);
        let plunge = mix(colour, [0, 0, 0], 0.4);
        let rapid = mix(colour, [255, 255, 255], 0.45);
        let mut phase = 0.0;
        let mut prev: Option<(Pose, P)> = None;
        for (pose, cutting) in p.poses.points() {
            let Some(sp) = self.screen(pose.tip) else { continue };
            if let Some((from, sf)) = prev {
                // A move shorter than a pixel waits for the next one, unless it changes from cut to rapid.
                let tiny = (sp.0 - sf.0).hypot(sp.1 - sf.1) < 0.6;
                if tiny && cutting {
                    continue;
                }
                let d = pose.tip - from.tip;
                let len = d.len();
                let down = cutting && len > 1e-9 && d.dot(from.axis) < -0.7 * len;
                for (a, b, hidden) in self.pieces(from.tip, pose.tip, sf, sp) {
                    let alpha = if hidden { 0.4 } else { 1.0 };
                    if !cutting {
                        self.ink.dashed(a, b, rapid, if hidden { 1.0 } else { 1.4 }, alpha * 0.95, &mut phase);
                    } else if down {
                        self.ink.segment(a, b, plunge, if hidden { 1.6 } else { 2.8 }, alpha);
                    } else {
                        self.ink.segment(a, b, cut, if hidden { 1.3 } else { 2.2 }, alpha);
                    }
                }
            }
            prev = Some((pose, sp));
        }
    }

    /// The points where cuts start after a rapid, ringed.
    fn entries(&mut self, p: &Planned, colour: [u8; 3]) {
        let mut marks = 0;
        let mut after_rapid: Option<Vec3> = None;
        for m in &p.poses.moves {
            match *m {
                Move5::Rapid { to } => after_rapid = Some(to.tip),
                Move5::Feed { .. } => {
                    if let Some(at) = after_rapid.take()
                        && let Some(s) = self.screen(at)
                    {
                        self.ink.disc(s, 4.0, mix(colour, [0, 0, 0], 0.55), 1.0);
                        self.ink.disc(s, 2.3, mix(colour, [255, 255, 255], 0.35), 1.0);
                        marks += 1;
                        if marks >= MAX_ENTRIES {
                            return;
                        }
                    }
                }
                Move5::Dwell { .. } => {}
            }
        }
    }

    /// The tool where the operation's first cut starts: its outline seen from the camera.
    fn tool(&mut self, p: &Planned, colour: [u8; 3]) {
        let Some(pose) = tool_pose(p) else { return };
        let Some(axis) = pose.axis.normalized() else { return };
        let r = p.tool.radius().max(0.05);
        let len = tool_length(p);
        let u = axis.any_perp();
        let v = axis.cross(u);
        let mut pts: Vec<P> = Vec::with_capacity(48);
        for k in 0..24 {
            let a = f64::from(k) / 24.0 * std::f64::consts::TAU;
            let ring = (u * a.cos() + v * a.sin()) * r;
            for h in [0.0, len] {
                if let Some(s) = self.screen(pose.tip + axis * h + ring) {
                    pts.push(s);
                }
            }
        }
        let hull = hull(pts);
        if hull.len() < 3 {
            return;
        }
        let edges: Vec<(P, P)> = hull.iter().zip(hull.iter().cycle().skip(1)).map(|(a, b)| (*a, *b)).collect();
        self.ink.fill(&edges, plain(TOOL_COLOUR), 0.3);
        for (a, b) in &edges {
            self.ink.segment(*a, *b, plain(colour), 1.5, 1.0);
        }
        // The tool's axis, up from the tip.
        if let (Some(a), Some(b)) = (self.screen(pose.tip), self.screen(pose.tip + axis * (len * 1.2))) {
            self.ink.dashed(a, b, plain(TOOL_COLOUR), 1.0, 0.9, &mut 0.0);
        }
    }

    /// An arrow from `a` to `b` (pixels) with its letter beyond the head.
    fn arrow(&mut self, a: P, b: P, c: [u8; 3], letter: &str) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        let cap = self.cap();
        if len < 4.0 {
            // Pointing at the camera: a ring round work zero, the letter beside it.
            self.ink.disc(a, 6.0, plain(c), 1.0);
            self.ink.disc(a, 4.0, [1.0, 1.0, 1.0], 1.0);
            self.ink.text(letter, (a.0 + 8.0, a.1 + cap + 6.0), cap, plain(c));
            return;
        }
        self.ink.segment(a, b, plain(c), 2.5, 1.0);
        let (ux, uy) = (dx / len, dy / len);
        let head = 9.0f64.min(len * 0.5);
        for side in [-1.0, 1.0] {
            let q = (b.0 - ux * head - uy * head * 0.45 * side, b.1 - uy * head + ux * head * 0.45 * side);
            self.ink.segment(b, q, plain(c), 2.5, 1.0);
        }
        let at = (b.0 + ux * (cap + 2.0) - cap * 0.35, b.1 + uy * (cap + 2.0) + cap * 0.5);
        self.ink.text(letter, at, cap, plain(c));
    }

    /// The legend's letter height (pixels).
    fn cap(&self) -> f64 {
        (self.ink.h as f64 / 55.0).clamp(7.0, 18.0)
    }

    /// Work zero's X, Y and Z.
    fn work_zero(&mut self) {
        let l = (self.size * 0.15).clamp(1.0, 1.0e4);
        let Some(o) = self.screen(Vec3::ZERO) else { return };
        for (d, c, letter) in [(Vec3::X, X_COLOUR, "X"), (Vec3::Y, Y_COLOUR, "Y"), (Vec3::Z, Z_COLOUR, "Z")] {
            if let Some(t) = self.screen(d * l) {
                self.arrow(o, t, c, letter);
            }
        }
        self.ink.disc(o, 3.5, [0.0, 0.0, 0.0], 1.0);
    }

    /// Every rotary table's axis through the part, with a ring whose arrow turns as a positive
    /// angle turns the part.
    fn rotary_axes(&mut self, machine: &Machine) {
        let b = self.set.surface.bounds();
        if b.is_empty() {
            return;
        }
        let colour = plain(ROTARY_COLOUR);
        for r in machine.rotaries.iter().filter(|r| r.carrier == Carrier::Table) {
            let Some(d) = r.direction.normalized() else { continue };
            let c = r.center - machine.work_origin;
            // The axis's reach over the part's box, and a sixth of its size beyond each end.
            let mut t = (f64::INFINITY, f64::NEG_INFINITY);
            for k in 0..8 {
                let q = Vec3::new(
                    if k & 1 == 0 { b.min.x } else { b.max.x },
                    if k & 2 == 0 { b.min.y } else { b.max.y },
                    if k & 4 == 0 { b.min.z } else { b.max.z },
                );
                let s = (q - c).dot(d);
                t = (t.0.min(s), t.1.max(s));
            }
            let more = self.size / 6.0;
            let (a, e) = (c + d * (t.0 - more), c + d * (t.1 + more));
            let (Some(sa), Some(se)) = (self.screen(a), self.screen(e)) else { continue };
            // Dash and dot.
            let mut phase = 0.0;
            self.ink.dashed(sa, se, colour, 1.6, 1.0, &mut phase);
            // The ring at the far end, a quarter turn of it with an arrow.
            let ring_r = (self.size * 0.08).max(0.5);
            let at = c + d * (t.1 + more * 0.6);
            let u = d.any_perp();
            let v = d.cross(u);
            let sign = if r.reversed { -1.0 } else { 1.0 };
            let pts: Vec<P> = (0..=32)
                .filter_map(|k| {
                    let a = sign * f64::from(k) / 32.0 * std::f64::consts::TAU * 0.8;
                    self.screen(at + (u * a.cos() + v * a.sin()) * ring_r)
                })
                .collect();
            for w in pts.windows(2) {
                if let [p0, p1] = w {
                    self.ink.segment(*p0, *p1, colour, 1.6, 1.0);
                }
            }
            if let [.., p0, p1] = pts.as_slice() {
                let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
                let len = dx.hypot(dy);
                if len > 1e-6 {
                    let (ux, uy) = (dx / len, dy / len);
                    for side in [-1.0, 1.0] {
                        let q = (p1.0 - ux * 8.0 - uy * 3.6 * side, p1.1 - uy * 8.0 + ux * 3.6 * side);
                        self.ink.segment(*p1, q, colour, 1.6, 1.0);
                    }
                }
            }
            let cap = self.cap();
            if let Some(s) = self.screen(e) {
                self.ink.text(&r.letter.char().to_string(), (s.0 + 6.0, s.1 - 6.0), cap, colour);
            }
        }
    }

    /// The job's name and view, and each operation drawn in its colour, top left.
    fn legend(&mut self, name: &str, planned: &[Planned], ops: &[usize], view: StandardView) {
        let mut rows: Vec<(Option<[u8; 3]>, String)> =
            vec![(None, format!("{} - {} view, work coordinates", short(name, LEGEND_CHARS), view_name(view)))];
        for &i in ops.iter().take(LEGEND_ROWS) {
            if let Some(p) = planned.get(i) {
                rows.push((
                    Some(colour_of(i)),
                    format!("{}. {} ({}, T{})", i + 1, short(&p.report.name, LEGEND_CHARS), p.report.kind, p.tool.number),
                ));
            }
        }
        if ops.len() > LEGEND_ROWS {
            rows.push((None, format!("and {} more", ops.len() - LEGEND_ROWS)));
        }
        rows.push((None, "solid: cut, dashed: rapid, dot: entry, faint: hidden".into()));
        self.rows(rows);
    }

    /// A key in the top left corner: one row each, a colour swatch before those that have one;
    /// the last row smaller.
    fn rows(&mut self, rows: Vec<(Option<[u8; 3]>, String)>) {
        let cap = self.cap();
        let line = cap * 1.9;
        let pad = cap * 0.8;
        // The box first, as wide as its longest row is likely to be.
        let longest = rows.iter().map(|(_, t)| t.chars().count()).max().unwrap_or(0) as f64;
        let width = (longest * cap * 0.62 + cap * 3.0 + pad * 2.0).min(self.ink.w as f64 - 4.0);
        let height = rows.len() as f64 * line + pad;
        let (x0, y0) = (4.0, 4.0);
        let corners = [(x0, y0), (x0 + width, y0), (x0 + width, y0 + height), (x0, y0 + height)];
        let edges: Vec<(P, P)> = corners.iter().zip(corners.iter().cycle().skip(1)).map(|(a, b)| (*a, *b)).collect();
        self.ink.fill(&edges, [1.0, 1.0, 1.0], 0.82);
        let rows_len = rows.len();
        for (k, (colour, text)) in rows.into_iter().enumerate() {
            let base = y0 + pad + (k as f64) * line + cap;
            let mut x = x0 + pad;
            if let Some(c) = colour {
                self.ink.segment((x, base - cap * 0.4), (x + cap * 2.0, base - cap * 0.4), plain(c), cap * 0.5, 1.0);
                x += cap * 2.6;
            }
            let ink = if k + 1 == rows_len { KEY_COLOUR } else { TEXT_COLOUR };
            self.ink.text(&text, (x, base), cap * if k + 1 == rows_len { 0.85 } else { 1.0 }, plain(ink));
        }
    }
}

/// The convex hull of `pts`, counter-clockwise (monotone chain).
fn hull(mut pts: Vec<P>) -> Vec<P> {
    pts.retain(|p| p.0.is_finite() && p.1.is_finite());
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: P, a: P, b: P| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut lower: Vec<P> = Vec::new();
    for &p in &pts {
        while let [.., a, b] = lower.as_slice()
            && cross(*a, *b, p) <= 0.0
        {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<P> = Vec::new();
    for &p in pts.iter().rev() {
        while let [.., a, b] = upper.as_slice()
            && cross(*a, *b, p) <= 0.0
        {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// `cam.view`: the job planned as a dry run that draws one picture.
fn view(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.view";
    let params: CamViewParams = parse(cmd, p, "")?;
    let mut q = Map::new();
    for k in ["job", "job_path"] {
        if let Some(v) = p.get(k).filter(|v| !v.is_null()) {
            q.insert(k.into(), v.clone());
        }
    }
    q.insert("dry_run".into(), json!(true));
    q.insert("image".into(), json!(params.path));
    if let Some(v) = &params.view {
        q.insert("view".into(), json!(v));
    }
    if let Some(w) = params.width {
        q.insert("width".into(), json!(w));
    }
    if let Some(h) = params.height {
        q.insert("height".into(), json!(h));
    }
    let raw = Value::Object(q);
    let job: CamJobParams = parse(cmd, &raw, "")?;
    let reply = run_job(s, cmd, &raw, job, params.op.as_deref())?;
    let Some(image) = reply.images else { return Err(EngineError::Other("cam.view: no picture was drawn".into())) };
    let drawn = |name: &str| params.op.as_deref().is_none_or(|o| o == name);
    let out = CamViewReply {
        path: params.path.clone(),
        image,
        operations: reply
            .operations
            .into_iter()
            .map(|o| ViewOperation { drawn: drawn(&o.name), name: o.name, kind: o.kind, extents: o.extents })
            .collect(),
        passed: reply.passed,
        failures: reply.failures,
        notes: reply.notes,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(out).map_err(|e| EngineError::Other(e.to_string()))
}
