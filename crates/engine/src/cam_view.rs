//! The CAM simulation view's state ([`Session::cam_view`]): a job's program loaded into a
//! [`Playback`] (`solidcraft_cam::playback`), what the view shows, whether it plays and how fast,
//! and what a picture of it holds. View state only: never saved, never an undo step. The
//! `cam.view.*` commands (`cmd/cam_view.rs`) change it; a window plays it on and draws it, and a
//! headless picture of the model ([`crate::view`]) shows it through [`scene`].
//!
//! A window that cuts the stock off its own thread takes the playback out with
//! [`CamView::take_playback`] and puts it back with [`CamView::put_back`]; meanwhile the
//! commands that need it answer that the view is busy.

use std::sync::Arc;

use solidcore_record::Vec3;
use solidcraft_cam::findings::{FindingEvent, Severity};
use solidcraft_cam::playback::{MoveKind, Playback};
use solidcraft_cam::sim::StockShape;
use solidcraft_cam::stock_mesh::{CLASS_TOOL, display_step, tile_mesh};
use solidcraft_cam::tool_mesh::ToolMeshes;
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb3, Mesh};
use solidcraft_viewport::{Rgb, Scene, SceneLine, SceneMesh};

use crate::schema::cam_view::{
    CamViewStateReply, ColourBy, FindingCount, FindingsListing, FindingsSoFar, ViewFinding, ViewLayers, ViewMachineWords, ViewOpRef, ViewPosition,
    ViewTool,
};
use crate::{EngineError, Result, Session};

/// The colours of the CAM layer in a headless picture (the light theme's look).
pub mod colors {
    use solidcraft_viewport::Rgb;

    pub const STOCK: Rgb = Rgb(196, 170, 120);
    /// The cut stock and the toolpaths, by tool slot (round again past the last).
    pub const TOOLS: [Rgb; 6] = [Rgb(70, 130, 200), Rgb(60, 160, 110), Rgb(200, 120, 50), Rgb(150, 90, 180), Rgb(190, 70, 90), Rgb(90, 150, 160)];
    pub const RAPID: Rgb = Rgb(220, 60, 60);
    pub const REMAINING: Rgb = Rgb(150, 156, 166);
    pub const TOOL: Rgb = Rgb(200, 204, 210);
    pub const HOLDER: Rgb = Rgb(110, 116, 126);
    pub const ERROR: Rgb = Rgb(220, 30, 30);
    pub const WARNING: Rgb = Rgb(230, 150, 20);
}

/// A loaded simulation view.
pub struct CamView {
    playback: Option<Playback>,
    pub layers: ViewLayers,
    pub colour_by: ColourBy,
    pub playing: bool,
    /// Times the estimated real time.
    pub speed: f64,
    /// Moves a second instead, when given.
    pub moves_per_s: Option<f64>,
    pub looping: bool,
    /// Changes with every change of the view.
    pub frame: u64,
    /// The program's text, for a line's G-code.
    text: Arc<str>,
    line_starts: Vec<usize>,
    /// Each tool's display meshes (tip at the origin, axis +Z), by number.
    pub tool_meshes: Vec<(u32, Arc<ToolMeshes>)>,
    pub stock: StockShape,
    /// Work zero in the design (the job's `origin`): the view is drawn there.
    pub origin: Vec3,
}

fn starts(text: &str) -> Vec<usize> {
    let mut v = vec![0];
    v.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    v
}

impl CamView {
    pub fn new(playback: Playback, text: Arc<str>, tool_meshes: Vec<(u32, Arc<ToolMeshes>)>, stock: StockShape, origin: Vec3) -> CamView {
        let line_starts = starts(&text);
        CamView {
            playback: Some(playback),
            layers: ViewLayers::default(),
            colour_by: ColourBy::Tool,
            playing: false,
            speed: 1.0,
            moves_per_s: None,
            looping: false,
            frame: 1,
            text,
            line_starts,
            tool_meshes,
            stock,
            origin,
        }
    }

    pub fn playback(&self) -> Option<&Playback> {
        self.playback.as_ref()
    }

    pub fn playback_mut(&mut self) -> Option<&mut Playback> {
        self.playback.as_mut()
    }

    /// The playback, for work off the window's thread; the view is busy until it is put back.
    pub fn take_playback(&mut self) -> Option<Playback> {
        self.playback.take()
    }

    pub fn put_back(&mut self, p: Playback) {
        self.playback = Some(p);
        self.bump();
    }

    pub fn bump(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// The text of G-code line `line` (from 1).
    pub fn line_text(&self, line: usize) -> &str {
        let Some(&a) = line.checked_sub(1).and_then(|k| self.line_starts.get(k)) else { return "" };
        let b = self.line_starts.get(line).copied().unwrap_or(self.text.len());
        self.text.get(a..b).unwrap_or("").trim_end()
    }

    /// The tool's meshes for tool `number`.
    pub fn tool_mesh(&self, number: u32) -> Option<&ToolMeshes> {
        self.tool_meshes.iter().find(|(n, _)| *n == number).map(|(_, m)| m.as_ref())
    }

    /// What `cam.view.state` answers.
    pub fn state_reply(&self, listing: FindingsListing, limit: usize) -> Result<CamViewStateReply> {
        let Some(p) = self.playback() else { return Err(EngineError::Other("the CAM view is busy cutting the stock: ask again".into())) };
        let (m, f) = p.at();
        let plan = p.plan();
        let tl = p.timeline();
        let mv = tl.moves.get(m);
        let step = plan.step(m);
        let pose = p.pose_now();
        let joints = plan.joints(m, f);
        let tool = step.as_ref().and_then(|s| p.tool(s.slot)).map(|t| ViewTool {
            number: t.number,
            kind: serde_json::to_value(t.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
            diameter: t.diameter,
            stickout: t.stickout,
        });
        let line = mv.map_or(0, |x| x.line);
        let op = mv.and_then(|x| x.op).and_then(|o| tl.ops.get(o).map(|r| ViewOpRef { index: o, name: r.name.clone() }));
        let rotary = !plan.machine().rotaries.is_empty();
        let position = ViewPosition {
            tip: pose.map_or([0.0; 3], |q| [q.tip.x, q.tip.y, q.tip.z]),
            axis: pose.map_or([0.0, 0.0, 1.0], |q| [q.axis.x, q.axis.y, q.axis.z]),
            machine: joints.map_or_else(ViewMachineWords::default, |j| ViewMachineWords {
                x: j.linear.x,
                y: j.linear.y,
                z: j.linear.z,
                a_deg: rotary.then(|| j.rotary.first().copied().unwrap_or(0.0).to_degrees()),
            }),
        };
        let time_s = p.time_now();
        let duration_s = tl.duration;
        let end = if f >= 1.0 { m + 1 } else { m };
        let merged = p.findings().merged_until(end);
        let list = merged.list();
        let by_code = list
            .iter()
            .map(|x| FindingCount {
                code: x.code.clone(),
                severity: x.severity.label().into(),
                count: x.count,
                first_line: x.lines.first().copied().unwrap_or(0),
                lines: x.lines.clone(),
                message: x.message.clone(),
            })
            .collect();
        let findings = (listing == FindingsListing::All).then(|| p.findings().until(end).take(limit).map(finding_view).collect());
        Ok(CamViewStateReply {
            at_move: m,
            moves: plan.moves(),
            fraction: f,
            line,
            gcode: self.line_text(line).to_string(),
            kind: mv.map_or("", |x| kind_label(x.kind)).into(),
            op,
            tool,
            position,
            feed: mv.and_then(|x| x.feed),
            rpm: mv.and_then(|x| x.rpm),
            time_s,
            duration_s,
            percent: if duration_s > 0.0 { (100.0 * time_s / duration_s).clamp(0.0, 100.0) } else { 0.0 },
            removed_volume: p.removed_volume(),
            playing: self.playing,
            speed: self.speed,
            moves_per_s: self.moves_per_s,
            looping: self.looping,
            layers: self.layers,
            findings_so_far: if listing == FindingsListing::None {
                FindingsSoFar::default()
            } else {
                FindingsSoFar {
                    errors: merged.count(Severity::Error),
                    warnings: merged.count(Severity::Warning),
                    infos: merged.count(Severity::Info),
                    by_code,
                }
            },
            findings,
            checked_moves: p.checked(),
            keyframes: p.keyframes(),
            keyframe_bytes: p.keyframe_bytes(),
            frame: self.frame,
        })
    }
}

pub fn kind_label(k: MoveKind) -> &'static str {
    match k {
        MoveKind::Rapid => "rapid",
        MoveKind::Feed => "feed",
        MoveKind::Arc => "arc",
        MoveKind::Plunge => "plunge",
    }
}

pub fn finding_view(e: &FindingEvent) -> ViewFinding {
    ViewFinding {
        code: e.code.clone(),
        severity: e.severity.label().into(),
        at_move: e.at_move,
        line: e.line,
        op: e.op,
        at: e.at.map(|p| [p.x, p.y, p.z]),
        depth: e.depth,
        message: e.message.clone(),
    }
}

/// The CAM layer's bounds: the stock.
pub fn bounds(s: &Session) -> Option<Aabb3> {
    let v = s.cam_view.as_deref()?;
    let mut b = Aabb3::EMPTY;
    match v.stock {
        StockShape::Box { min, max } => {
            b.add(min);
            b.add(max);
        }
        StockShape::Cylinder { x_min, x_max, radius } => {
            b.add(Vec3::new(x_min, -radius, -radius));
            b.add(Vec3::new(x_max, radius, radius));
        }
    }
    Some(Aabb3 { min: b.min + v.origin, max: b.max + v.origin })
}

/// `m` moved so that its origin is at `at` and its X, Y and Z along `frame` (the spindle's own
/// frame against the work).
fn placed(m: &Mesh, at: Vec3, frame: [Vec3; 3]) -> Mesh {
    let [u, v, w] = frame;
    let mut out = m.clone();
    out.normals.clear();
    for p in &mut out.positions {
        *p = at + u * p.x + v * p.y + w * p.z;
    }
    out
}

fn tool_color(slot: u8) -> Rgb {
    colors::TOOLS.get(usize::from(slot) % colors::TOOLS.len()).copied().unwrap_or(colors::TOOL)
}

/// The CAM layer of a headless picture: the stock as cut so far (coloured by the tool that cut
/// it last), the toolpaths (done bright, still to come dim), the tool and its holder at their
/// pose and the findings so far as marks.
pub fn scene(s: &Session, sc: &mut Scene) {
    let Some(v) = s.cam_view.as_deref() else { return };
    let mut local = Scene::default();
    scene_in_work(v, &mut local);
    // Work coordinates are the design's less the job's origin.
    let o = v.origin;
    for mut m in local.meshes {
        if o.len() > 0.0 {
            let mut mesh = (*m.mesh).clone();
            for q in &mut mesh.positions {
                *q += o;
            }
            m.mesh = Arc::new(mesh);
        }
        sc.meshes.push(m);
    }
    for mut l in local.lines {
        for q in &mut l.points {
            *q += o;
        }
        sc.lines.push(l);
    }
}

fn scene_in_work(v: &CamView, sc: &mut Scene) {
    let Some(p) = v.playback() else { return };
    let plan = p.plan();
    let tl = p.timeline();
    let (now, frac) = p.at();
    // Tools coloured in the order the program uses them.
    let mut used: Vec<u8> = Vec::new();
    for c in &tl.tool_changes {
        if let Some(m) = tl.moves.get(c.at_move)
            && !used.contains(&m.slot)
        {
            used.push(m.slot);
        }
    }
    let tool_color = |slot: u8| tool_color(u8::try_from(used.iter().position(|u| *u == slot).unwrap_or(0)).unwrap_or(0));
    // The stock.
    if v.layers.stock {
        let step = display_step(plan, 512);
        let (tx, ty) = plan.grid().tiles();
        let mut all = Mesh::default();
        for tj in 0..ty {
            for ti in 0..tx {
                let t = tile_mesh(plan, p.state(), ti, tj, step);
                let Ok(base) = u32::try_from(all.positions.len()) else { break };
                all.positions.extend(t.positions);
                all.triangles.extend(t.triangles.into_iter().map(|q| q.map(|i| i.saturating_add(base))));
                all.tri_face.extend(t.tri_face);
            }
        }
        let mut faces: Vec<(u32, Rgb)> = Vec::new();
        for slot in 0..=u8::MAX.saturating_sub(1) {
            if plan.tool_number(slot).is_none() {
                break;
            }
            // The stock keeps the tool that cut each cell, so it is coloured by tool either way.
            faces.push((CLASS_TOOL + u32::from(slot), tool_color(slot)));
        }
        sc.meshes.push(SceneMesh { face_colors: faces, ..SceneMesh::solid(Arc::new(all), colors::STOCK) });
    }
    // The toolpaths: runs of moves of one kind, done or to come.
    if v.layers.toolpaths || v.layers.rapids {
        let mut run: Vec<Vec3> = Vec::new();
        let mut key: Option<(bool, bool, u8)> = None;
        let flush = |run: &mut Vec<Vec3>, key: Option<(bool, bool, u8)>, sc: &mut Scene| {
            if let Some((rapid, done, slot)) = key
                && run.len() > 1
            {
                let color = if !done {
                    colors::REMAINING
                } else if rapid {
                    colors::RAPID
                } else {
                    tool_color(slot)
                };
                sc.lines.push(SceneLine {
                    points: std::mem::take(run),
                    color,
                    alpha: if done { 1.0 } else { 0.45 },
                    width: if rapid { 0.8 } else { 1.3 },
                    on_top: false,
                });
            }
            run.clear();
        };
        for (k, m) in tl.moves.iter().enumerate() {
            let rapid = m.kind == MoveKind::Rapid;
            if (rapid && !v.layers.rapids) || (!rapid && !v.layers.toolpaths) {
                flush(&mut run, key, sc);
                key = None;
                continue;
            }
            let done = k < now || (k == now && frac >= 1.0);
            if !done && !v.layers.remaining_path {
                continue;
            }
            let this = Some((rapid, done, m.slot));
            let pts: Vec<Vec3> = plan.samples(k, 0.5, 64).into_iter().map(|q| q.tip).collect();
            if this != key {
                flush(&mut run, key, sc);
                key = this;
                if let Some(first) = pts.first() {
                    run.push(*first);
                }
            }
            run.extend(pts.into_iter().skip(1));
        }
        flush(&mut run, key, sc);
    }
    // The tool and its holder at the pose now.
    let Some(pose) = p.pose_now() else { return };
    let frame = p.head_frame_now();
    let step = plan.step(now);
    let tool = step.as_ref().and_then(|st| p.tool(st.slot));
    if let Some(t) = tool
        && let Some(meshes) = v.tool_mesh(t.number)
    {
        if v.layers.tool {
            for m in [&meshes.cutting, &meshes.shank] {
                if !m.triangles.is_empty() {
                    sc.meshes.push(SceneMesh::solid(Arc::new(placed(m, pose.tip, frame)), colors::TOOL));
                }
            }
        }
        if v.layers.holder && !meshes.holder.triangles.is_empty() {
            sc.meshes.push(SceneMesh::solid(Arc::new(placed(&meshes.holder, pose.tip, frame)), colors::HOLDER));
        }
    }
    // The findings so far, as crosses over everything.
    if v.layers.gouges || v.layers.collisions {
        let end = if frac >= 1.0 { now + 1 } else { now };
        let size = plan.cell().max(0.2) * 6.0;
        for e in p.findings().until(end) {
            let Some(at) = e.at else { continue };
            let gouge = matches!(e.code.as_str(), "gouge" | "rapid-in-stock" | "cut-spindle-off" | "shank-rubs");
            if (gouge && !v.layers.gouges) || (!gouge && !v.layers.collisions) {
                continue;
            }
            let color = if e.severity == Severity::Error { colors::ERROR } else { colors::WARNING };
            for d in [Vec3::new(size, 0.0, 0.0), Vec3::new(0.0, size, 0.0), Vec3::new(0.0, 0.0, size)] {
                sc.lines.push(SceneLine { points: vec![at - d, at + d], color, alpha: 1.0, width: 2.4, on_top: true });
            }
        }
    }
}
