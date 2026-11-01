//! Live operation previews. While a feature dialog is open, its commands are evaluated on a
//! scratch copy of the session ([`solidcraft_engine::Session::preview`]), off the UI thread
//! where threads exist, and the result is drawn in place of the bodies it changes: new surface
//! tinted, removed material as a translucent ghost. The last good preview stays up while the
//! next one computes; once the current inputs make the feature fail, nothing is previewed and
//! the error shows in the dialog (#3254).
//! Nothing here touches the document or the undo history.
//!
//! A feature that adds or removes a tool body (join, cut, intersect) shows only its tool. That
//! is all its preview builds: its boolean, which can take seconds (a text joined onto a part),
//! runs once, on OK, and an error it finds shows then, the dialog staying open.

use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use serde_json::Value;
use solidcraft_engine::Session;
use solidcraft_engine::doc::progress::Progress;
use solidcraft_engine::doc::{ModelBody, ModelState};
use solidcraft_engine::geom::{Aabb3, Mesh, Ring, Vec3};

use crate::SolveApp;
use crate::gpu::{GpuScene, SceneSlot};
use crate::theme::Tokens;

/// Colours a preview is drawn with (straight sRGBA).
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub body: [u8; 4],
    pub added: [u8; 4],
    /// A body the preview creates (New Body), so it doesn't look like a join.
    pub new_body: [u8; 4],
    pub removed: [u8; 4],
    pub edge: [u8; 4],
    /// Whether a moved component's old place is drawn as a ghost (Drive Joints' Show ghost).
    pub ghost: bool,
}

impl Colors {
    pub fn from_tokens(t: &Tokens) -> Colors {
        let c = |c: egui::Color32| c.to_srgba_unmultiplied();
        Colors { body: c(t.body), added: c(t.preview_add), new_body: c(t.preview_new), removed: c(t.preview_cut), edge: c(t.body_edge), ghost: true }
    }

    fn for_tool(t: &Tokens, tab: &str, active: bool) -> Colors {
        let mut colors = Self::from_tokens(t);
        let accent = crate::theme::TabAccent::for_tab(t, tab);
        if active && accent.color != t.accent {
            colors.added = accent.color.to_srgba_unmultiplied();
            colors.new_body = t.body.lerp_to_gamma(accent.color, 0.45).to_srgba_unmultiplied();
        }
        // Removed material retains its warning colour and opacity.
        colors
    }
}

pub struct SketchPreview {
    pub sketch: std::sync::Arc<solidcraft_engine::doc::SolvedSketch>,
    pub curves: Vec<usize>,
    pub points: Vec<usize>,
}

/// A computed preview: the scene to draw and the bodies it stands in for.
pub struct Built {
    pub scene: GpuScene,
    pub sketches: Vec<SketchPreview>,
    pub replaced: Vec<String>,
    /// Why the feature failed, when the scene only shows its tool body.
    pub error: Option<String>,
    /// Extent of what the preview shows (the view's depth range must cover it).
    pub bounds: Aabb3,
    /// The smallest and largest radius a fillet used when it reduced one too large for its edges
    /// (`fit: reduce`).
    pub applied: Option<(f64, f64)>,
    /// The radius spline of each edge a fillet tapered (`fit: taper`), with a point on the edge.
    pub tapered: Vec<(Vec3, solidcraft_engine::kernel::Taper)>,
    /// The reply of a previewed drive that stopped short of the asked values (the dialog says
    /// where and why, in the UI's language).
    pub stopped: Option<serde_json::Value>,
}

type Job = Result<Built, String>;

/// How many obsolete preview jobs may still run, left behind for newer inputs (#7167). Beyond
/// them, a new job waits for the running one as before, so a quick series of picks keeps at
/// most this many kernel computations going beside the current one.
const DETACHED: usize = 2;

#[derive(Default)]
pub struct PreviewState {
    /// Key of the commands whose result (or error) is shown.
    shown_key: Option<u64>,
    running: Option<(u64, Receiver<Job>)>,
    /// Asks the running job to stop between features once it is obsolete.
    running_progress: Option<Arc<Progress>>,
    /// Jobs left behind for inputs that changed to another setup (#7167): asked to stop, their
    /// results are dropped when they come. With [`DETACHED`] of them running, the next obsolete
    /// job is waited for.
    detached: Vec<Receiver<Job>>,
    /// The running job builds a tool alone (for a join, cut or intersect).
    running_tool: bool,
    /// Inputs of a joint or distance preview, excluding only its driven values.
    running_stream_key: Option<u64>,
    /// Bodies the preview stands in for (hidden in the model scene).
    pub replaced: Vec<String>,
    /// The preview scene for the GPU and its key (changes whenever the scene does).
    pub slot: SceneSlot,
    pub key: u64,
    /// Is a preview scene up?
    pub active: bool,
    pub sketches: Vec<SketchPreview>,
    /// Why the current inputs don't make a feature (nothing is previewed then).
    pub error: Option<String>,
    /// A preview is being computed.
    pub busy: bool,
    /// Extent of the preview scene.
    pub bounds: Aabb3,
    /// Time the last preview took (ms), for `ui.inspect`.
    pub ms: f64,
    started: f64,
    /// The smallest and largest radius the previewed fillet used when it reduced the one asked for.
    pub applied: Option<(f64, f64)>,
    /// The reply of a previewed drive that stopped short (see [`Built::stopped`]).
    pub stopped: Option<serde_json::Value>,
    /// The radius splines the previewed fillet tapered its edges with.
    pub tapered: Vec<(Vec3, solidcraft_engine::kernel::Taper)>,
    straight: crate::extrude_preview::StraightPreview,
    moving: crate::move_preview::MovePreview,
    /// PreviewReselect: Primary held - hide the preview and restore replaced bodies for picks.
    pub rolled_back: bool,
}

impl PreviewState {
    /// Bodies the model scene should hide for the preview (empty while rolled back).
    pub fn effective_replaced(&self) -> &[String] {
        if self.rolled_back { &[] } else { &self.replaced }
    }

    /// Stop drawing the preview scene and give the bodies it stood in for back to the model.
    fn hide_scene(&mut self) {
        if self.active || !self.replaced.is_empty() {
            self.active = false;
            self.bounds = Aabb3::EMPTY;
            self.replaced.clear();
            self.set_scene(GpuScene::default());
        }
    }

    fn clear(&mut self) {
        self.straight.clear();
        self.moving.clear();
        self.detach();
        self.running_tool = false;
        self.running_stream_key = None;
        self.shown_key = None;
        self.error = None;
        self.applied = None;
        self.stopped = None;
        self.tapered.clear();
        self.sketches.clear();
        self.busy = false;
        self.rolled_back = false;
        self.hide_scene();
    }

    /// Drop the preview of a design that leaves the window. Its scene goes too: the viewport
    /// keeps drawing the last scene it took until the slot it reads hands it another, so a fresh
    /// state (another slot, empty) would leave the old preview drawn over the next design.
    pub fn reset(&mut self) {
        self.detach();
        let (slot, key, detached) = (std::mem::take(&mut self.slot), self.key, std::mem::take(&mut self.detached));
        *self = PreviewState { slot, key, detached, ..PreviewState::default() };
        self.set_scene(GpuScene::default());
    }

    fn set_scene(&mut self, sc: GpuScene) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(sc);
        }
        self.key = self.key.wrapping_add(1);
    }

    fn apply(&mut self, key: u64, job: Job) {
        self.shown_key = Some(key);
        self.ms = crate::now_ms() - self.started;
        match job {
            Ok(b) => {
                self.sketches = b.sketches;
                self.replaced = b.replaced;
                self.bounds = b.bounds;
                self.active = true;
                self.error = b.error;
                self.applied = b.applied;
                self.stopped = b.stopped;
                self.tapered = b.tapered;
                self.set_scene(b.scene);
            }
            // The current inputs make no feature: the last good geometry would contradict
            // them (a 1 mm extrusion still drawn at 0 mm), so nothing is drawn.
            Err(e) => {
                self.hide_scene();
                self.sketches.clear();
                self.error = Some(e);
                self.applied = None;
                self.stopped = None;
                self.tapered.clear();
            }
        }
    }

    /// Errors validate only the current inputs. Keep the last good geometry while
    /// computing new inputs, but never let an obsolete result block their Apply.
    fn poll(&mut self, key: u64, stream_key: Option<u64>) {
        if self.shown_key != Some(key) {
            self.sketches.clear();
            self.error = None;
        }
        if let Some((k, rx)) = &self.running {
            let k = *k;
            let job = match rx.try_recv() {
                Ok(job) => Some(job),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err("the preview failed".into())),
            };
            if let Some(job) = job {
                self.running = None;
                let good = matches!(&job, Ok(b) if b.error.is_none());
                let intermediate = good && (self.running_tool || stream_key.is_some() && self.running_stream_key == stream_key);
                self.running_stream_key = None;
                self.running_tool = false;
                // A held slider, a dragged arrow or an animation may advance faster than the
                // worker. Show completed joint/distance results from the same setup, and tools built for
                // inputs since changed, then compute the latest. Errors still require an exact
                // key.
                if k == key || intermediate {
                    self.apply(k, job);
                }
            }
        }
        // A detached job's result is never shown: drop it once it is done.
        self.detached.retain(|rx| matches!(rx.try_recv(), Err(TryRecvError::Empty)));
        // The inputs moved to another setup (another selection, for the features whose distance,
        // radius or pose streams) while a job runs: its result could only be dropped, so stop
        // waiting for it and let the current inputs start now (#7167). A tool, or a value
        // streaming within one setup, still shows each finished result; inputs that don't stream
        // wait as before. At most `DETACHED` jobs are left behind; while they run, the next
        // obsolete one is waited for as before.
        let other_setup = !self.running_tool && stream_key.is_some() && self.running_stream_key.is_some() && self.running_stream_key != stream_key;
        if self.running.as_ref().is_some_and(|(k, _)| *k != key) && other_setup && self.detached.len() < DETACHED {
            self.detach();
        }
    }

    /// Stop waiting for the running job: ask it to stop between features, and count it among the
    /// detached jobs until its thread ends (also when a closed dialog or another design leaves it
    /// behind, so `DETACHED` bounds what runs).
    fn detach(&mut self) {
        if let Some(progress) = self.running_progress.take() {
            progress.cancel();
        }
        if let Some((_, rx)) = self.running.take() {
            self.detached.push(rx);
        }
        self.running_tool = false;
        self.running_stream_key = None;
    }
}

/// Joint motion can stream successful intermediate poses while values change. Include
/// every other input, the document revision and colours so a different joint, changed
/// limits, feature dialog or document cannot reuse an obsolete pose.
fn motion_key(cmds: &[(String, Value)], revision: u64, colors: Colors) -> Option<u64> {
    if !cmds.iter().any(|(c, p)| c == "joint.drive" || c == "joint.create" && p.get("values").is_some())
        || !cmds.iter().all(|(c, _)| {
            matches!(
                c.as_str(),
                "joint.drive"
                    | "joint.create"
                    | "joint.as_built"
                    | "joint.edit"
                    | "joint.limits"
                    | "joint.gear"
                    | "joint.rack_pinion"
                    | "joint.screw"
                    | "joint.cam"
            )
        })
    {
        return None;
    }
    let mut setup = cmds.to_vec();
    for (command, params) in &mut setup {
        if matches!(command.as_str(), "joint.drive" | "joint.create")
            && let Some(p) = params.as_object_mut()
        {
            p.remove("values");
            if command == "joint.drive" {
                p.remove("value");
            }
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&setup).ok()?.hash(&mut h);
    revision.hash(&mut h);
    format!("{colors:?}").hash(&mut h);
    Some(h.finish())
}

/// Tool previews already stream in `poll`. New-body extrusions, face offsets, fillets and
/// chamfers instead build their full result; allow successful intermediate distances (a
/// fillet's radii, a chamfer's distances and angle) only within the same setup. Preserve selections, feature identity, extent mode, revision and colours. The
/// exact job key still ensures the latest distance is computed and errors never go stale.
fn stream_key(cmds: &[(String, Value)], revision: u64, colors: Colors) -> Option<u64> {
    if let Some(key) = motion_key(cmds, revision, colors) {
        return Some(key);
    }
    if cmds.is_empty() || as_new_body(cmds).is_some() {
        return None;
    }
    let mut setup = cmds.to_vec();
    for (command, params) in &mut setup {
        let (command, params) = if command == "timeline.redefine" {
            let command = params.get("command")?.as_str()?.to_string();
            (command, params.get_mut("params")?)
        } else {
            (command.clone(), params)
        };
        let fields: &[&str] = match command.as_str() {
            "solid.extrude" => &["distance", "distance2", "to_offset"],
            "solid.offset_face" => &["distance"],
            // A radius dragged or typed: each result shows while the next computes, rather than
            // the preview holding still until the input stops changing.
            "solid.fillet" => &["radius", "radius2", "mid"],
            "solid.chamfer" => &["distance", "distance2", "angle"],
            _ => return None,
        };
        let params = params.as_object_mut()?;
        for field in fields {
            // Keep presence: changing extent mode must not reuse the old result.
            if let Some(value) = params.get_mut(*field) {
                *value = Value::Null;
            }
        }
    }
    Some(job_key(&setup, revision, colors))
}

/// Keep the preview in step with the open dialog: start a computation when its inputs changed,
/// take finished results. Called once per frame.
pub fn update(app: &mut SolveApp, ctx: &egui::Context) {
    let cmds = match app.dialog.as_ref().filter(|d| d.previews() && d.preview_enabled).map(|d| crate::dialogs::preview_commands(app, d)) {
        Some(Ok(c)) if !c.is_empty() => c,
        _ if crate::sketch_attachment_ui::commands(app).is_some() => crate::sketch_attachment_ui::commands(app).unwrap_or_default(),
        // A parameter being edited in Change Parameters.
        _ if app.params.as_ref().and_then(|p| p.preview_commands()).is_some() => {
            app.params.as_ref().and_then(|p| p.preview_commands()).unwrap_or_default()
        }
        // No dialog, or inputs still missing: nothing to preview.
        _ => {
            app.preview.clear();
            return;
        }
    };
    let mut colors = Colors::for_tool(&Tokens::get(), &app.ui.tab, app.tool.is_some() || app.dialog.is_some());
    if app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::Assembly(crate::dialogs_assembly::Asm::Drive { .. }))) {
        colors.ghost = app.preferences.drive_joints_ghost;
    }
    let key = job_key(&cmds, app.session.revision, colors);
    let stream_key = stream_key(&cmds, app.session.revision, colors);
    let tool = as_new_body(&cmds).is_some();
    let pv = &mut app.preview;
    pv.poll(key, stream_key);
    if pv.running.is_none() && pv.shown_key != Some(key) {
        pv.started = crate::now_ms();
        pv.running_stream_key = stream_key;
        let mut straight = std::mem::take(&mut app.preview.straight);
        let mut job = straight.compute(app, &cmds, colors);
        app.preview.straight = straight;
        if job.is_none() {
            let mut moving = std::mem::take(&mut app.preview.moving);
            job = moving.compute(app, &cmds, colors);
            app.preview.moving = moving;
        }
        if let Some(job) = job {
            app.preview.apply(key, job);
            app.preview.busy = false;
            return;
        }
        let scratch = app.session.scratch();
        spawn(&mut app.preview, key, scratch, cmds, colors, tool, Arc::new(Progress::default()));
    }
    app.preview.busy = app.preview.running.is_some();
    if app.preview.busy {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// What tells one preview computation from another: its commands, the document revision they
/// run on and the colours.
fn job_key(cmds: &[(String, Value)], revision: u64, colors: Colors) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(cmds).unwrap_or_default().hash(&mut h);
    revision.hash(&mut h);
    format!("{colors:?}").hash(&mut h);
    h.finish()
}

/// Start computing the preview of `cmds`: for a tool feature (`tool`), only its tool.
#[cfg(not(target_arch = "wasm32"))]
fn spawn(pv: &mut PreviewState, key: u64, scratch: Session, cmds: Vec<(String, Value)>, colors: Colors, tool: bool, progress: Arc<Progress>) {
    let (tx, rx) = std::sync::mpsc::channel();
    // Watched by `progress`, so a job left behind for other inputs skips its remaining features.
    let watched = progress.clone();
    let spawned = std::thread::Builder::new().name("preview".into()).spawn(move || {
        let job = solidcraft_engine::doc::progress::watch(watched, || {
            if tool { compute_tool(&scratch, &cmds, colors) } else { compute(&scratch, &cmds, colors) }
        });
        let _ = tx.send(job);
    });
    match spawned {
        Ok(_) => {
            pv.running = Some((key, rx));
            pv.running_progress = Some(progress);
            pv.running_tool = tool;
        }
        Err(e) => pv.apply(key, Err(e.to_string())),
    }
}

/// No threads on the web: compute right away.
#[cfg(target_arch = "wasm32")]
fn spawn(pv: &mut PreviewState, key: u64, scratch: Session, cmds: Vec<(String, Value)>, colors: Colors, tool: bool, _progress: Arc<Progress>) {
    pv.apply(key, if tool { compute_tool(&scratch, &cmds, colors) } else { compute(&scratch, &cmds, colors) });
}

/// Evaluate the commands on the scratch session and build the preview scene.
/// Features that add or remove a tool body (extrude, revolve, primitives with join, cut or
/// intersect) show the tool itself: tinted for join, see-through red for cut. Other features
/// show their result in place of the bodies they change. When the feature fails (a boolean the
/// kernel can't do yet), the tool body is still shown, with the error.
pub fn compute(s: &Session, cmds: &[(String, Value)], colors: Colors) -> Job {
    if cmds.iter().any(|(command, params)| {
        matches!(
            command.as_str(),
            "sketch.redefine"
                | "sketch.mirror"
                | "sketch.pattern.rectangular"
                | "sketch.pattern.circular"
                | "sketch.offset"
                | "sketch.fillet"
                | "sketch.chamfer.equal_distance"
                | "sketch.chamfer.two_distance"
                | "sketch.chamfer.distance_angle"
                | "sketch.insert_svg"
                | "sketch.insert_dxf"
        ) || command == "construct.plane.along_path"
            || params.get("command").and_then(Value::as_str) == Some("construct.plane.along_path")
    }) {
        let preview = s.preview_visible_world(cmds).map_err(|error| error.to_string())?;
        return std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut built = build(&preview.before, &preview.after, colors);
            built.bounds = state_bounds(&preview.after);
            built
        }))
        .map_err(|_| "the preview failed".to_string());
    }
    // Joints and drives move occurrences: shown on the placed model.
    if let Some(job) = crate::dialogs_assembly::world_preview(s, cmds, colors) {
        return job;
    }
    match as_new_body(cmds) {
        Some(_) => {
            let mut b = compute_tool(s, cmds, colors)?;
            b.error = s.preview(cmds).err().map(|e| e.to_string());
            Ok(b)
        }
        None => {
            let p = s.preview(cmds).map_err(|e| e.to_string())?;
            let guard =
                |f: &dyn Fn() -> Built| std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|_| "the preview failed".to_string());
            let mut b = guard(&|| build(&p.before, &p.after, colors))?;
            b.bounds = state_bounds(&p.after);
            // The radius a previewed fillet used when it reduced the one asked for (a new
            // fillet or an edited one: the feature the replies name).
            let features = || p.replies.iter().filter_map(|r| r.get("feature").and_then(Value::as_u64));
            b.applied = features().find_map(|id| p.after.fitted_range(id));
            b.tapered = features().find_map(|id| p.after.tapered(id)).map(<[_]>::to_vec).unwrap_or_default();
            Ok(b)
        }
    }
}

/// The tool body of a feature that adds or removes one, built on its own (as a new body): what
/// its live preview shows, without the boolean. When even the tool fails, the feature's own
/// error. Other features: their whole preview ([`compute`]).
fn compute_tool(s: &Session, cmds: &[(String, Value)], colors: Colors) -> Job {
    let Some((alt, op)) = as_new_body(cmds) else { return compute(s, cmds, colors) };
    // Where the view shows it: in a component placed away from its own frame (an imported
    // part stood upright) the model's frame would draw the tool at right angles to the face.
    let tool = match s.preview_world(&alt) {
        Ok(t) => t,
        Err(e) => return Err(s.preview(cmds).err().map_or_else(|| e.to_string(), |r| r.to_string())),
    };
    let mut b = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build_tool(&tool.before, &tool.after, colors, &op)))
        .map_err(|_| "the preview failed".to_string())?;
    b.bounds = state_bounds(&tool.after);
    Ok(b)
}

fn extrusion_params<'a>(id: &str, params: &'a Value) -> Option<&'a Value> {
    match id {
        "solid.extrude" => Some(params),
        "timeline.redefine" if params.get("command").and_then(Value::as_str) == Some("solid.extrude") => params.get("params"),
        _ => None,
    }
}

/// Check references without intersecting the tool with the target's geometry.
pub(crate) fn extrusion_target_error(s: &Session, cmds: &[(String, Value)], before: &ModelState) -> Option<String> {
    for (id, params) in cmds {
        let params = extrusion_params(id, params)?;
        let op = params.get("operation").and_then(Value::as_str).unwrap_or("new");
        if op == "new" {
            continue;
        }
        let targets = params
            .get("targets")
            .and_then(Value::as_array)
            .filter(|targets| !targets.is_empty())
            .or_else(|| params.get("participants").and_then(Value::as_array));
        let mut found = false;
        if let Some(targets) = targets.filter(|targets| !targets.is_empty()) {
            for target in targets.iter().filter_map(Value::as_str) {
                if target == "*" {
                    found |= !before.bodies.is_empty();
                } else if before.body(target).is_some() {
                    found = true;
                } else {
                    return Some(format!("unknown body `{target}`"));
                }
            }
        } else {
            // A cut or intersect from a component with no body of its own takes part in every
            // body: the command decides that (#1234), and the preview asks it the same (#3085).
            found = params.get("faces").and_then(|_| params.get("face_body")).and_then(Value::as_str).map_or_else(
                || {
                    before.bodies.iter().any(|body| s.doc.model_body_component(body) == s.active_component)
                        || (matches!(op, "cut" | "intersect") && s.cut_takes_every_body(before))
                },
                |name| s.world_state().body(name).is_some(),
            );
        }
        if !found && matches!(op, "cut" | "intersect") {
            return Some("there is no body to cut or intersect".into());
        }
    }
    None
}

fn state_bounds(st: &ModelState) -> Aabb3 {
    st.bodies.iter().fold(Aabb3::EMPTY, |b, x| b.union(&x.mesh().bounds()))
}

/// The tool bodies a join/cut/intersect adds: opaque and tinted for join, see-through for cut
/// (red) and intersect.
fn build_tool(before: &ModelState, after: &ModelState, c: Colors, op: &str) -> Built {
    let mut sc = GpuScene::default();
    let removed = sc.color(c.removed);
    let intersect = sc.color([c.added[0], c.added[1], c.added[2], c.removed[3]]);
    let added = sc.color(c.added);
    let edge = if op == "cut" { sc.color([c.removed[0], c.removed[1], c.removed[2], 255]) } else { sc.color(c.edge) };
    for a in after.bodies.iter().filter(|a| before.body(&a.name).is_none()) {
        let m = a.mesh();
        for t in &m.triangles {
            for k in t {
                let i = *k as usize;
                if let (Some(p), Some(n)) = (m.positions.get(i), m.normals.get(i)) {
                    match op {
                        "cut" => sc.xray_tri(p.to_f32(), n.to_f32(), removed),
                        "intersect" => sc.xray_tri(p.to_f32(), n.to_f32(), intersect),
                        _ => sc.tri(p.to_f32(), n.to_f32(), added),
                    }
                }
            }
        }
        for (ei, e) in m.edges.iter().enumerate() {
            if m.shaded_edge_visible(ei) {
                for w in e.windows(2) {
                    sc.line(w[0].to_f32(), w[1].to_f32(), edge, 1.3, op != "join");
                }
            }
        }
    }
    Built {
        scene: sc,
        sketches: Vec::new(),
        replaced: Vec::new(),
        error: None,
        bounds: Aabb3::EMPTY,
        applied: None,
        tapered: Vec::new(),
        stopped: None,
    }
}

/// The same commands making a new body instead of joining, cutting or intersecting, and the
/// operation they had.
fn as_new_body(cmds: &[(String, Value)]) -> Option<(Vec<(String, Value)>, String)> {
    let mut out = cmds.to_vec();
    let mut changed: Option<String> = None;
    for (command, p) in &mut out {
        let inner = p.get("command").and_then(Value::as_str).map(str::to_string);
        // A combine's operation is between bodies that already exist: it has no new-body form
        // (and no tool body of its own to show).
        if inner.as_deref().unwrap_or(command.as_str()) == "solid.combine" {
            continue;
        }
        let p = if p.get("command").is_some() { p.get_mut("params")? } else { p };
        if let Some(op) = p.get_mut("operation")
            && let Some(o) = op.as_str().filter(|o| *o != "new").map(str::to_string)
        {
            *op = Value::from("new");
            changed = Some(o);
        }
    }
    changed.map(|op| (out, op))
}

fn same(a: &Mesh, b: &Mesh) -> bool {
    a.triangles.len() == b.triangles.len() && a.positions == b.positions
}

/// The preview scene: changed and new bodies (new surface tinted) and, as a ghost, the old
/// version of changed or removed bodies.
pub fn build(before: &ModelState, after: &ModelState, c: Colors) -> Built {
    let mut sc = GpuScene::default();
    let added_color = sc.color(c.added);
    let added_plane = sc.color([c.added[0], c.added[1], c.added[2], 64]);
    let body_color = sc.color(c.body);
    let new_body = sc.color(c.new_body);
    let removed = sc.color(c.removed);
    let edge = sc.color(c.edge);
    let mut replaced = Vec::new();
    for construct in &after.construct {
        if before.construct.iter().any(|old| old == construct) {
            continue;
        }
        if let solidcraft_engine::doc::construct::ConstructGeom::Plane(plane) = construct.geom {
            let (o, x, y) = (plane.origin, plane.x * 10.0, plane.y * 10.0);
            let quad = [o - x - y, o + x - y, o + x + y, o - x + y];
            for triangle in [[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]] {
                for p in triangle {
                    sc.xray_tri(p.to_f32(), plane.normal().to_f32(), added_plane);
                }
            }
            for (a, b) in Ring::from(quad).edges() {
                sc.line(a.to_f32(), b.to_f32(), added_color, 1.3, true);
            }
        }
    }
    for a in &after.bodies {
        let am = a.mesh();
        let old_body = before.body(&a.name);
        let old = old_body.map(|b| b.mesh());
        if old.as_ref().is_some_and(|bm| same(bm, &am)) {
            continue;
        }
        let nf = am.tri_face.iter().max().map_or(0, |m| *m as usize + 1);
        let added: Vec<bool> = match (&old, old_body) {
            (Some(bm), Some(ob)) => either(new_faces(&am, nf, bm), &named_new(a, ob)),
            _ => vec![true; nf],
        };
        // Material removed from a body (a hole, a pocket): its new faces also show through the
        // body in the cut colour, so the cut is seen from any side.
        let cut = old.as_ref().is_some_and(|bm| am.measure().volume < bm.measure().volume - 1e-6);
        for (t, f) in am.triangles.iter().zip(&am.tri_face) {
            let new = added.get(*f as usize).copied().unwrap_or(true);
            let col = match (new, old.is_some()) {
                (false, _) => body_color,
                (true, true) => added_color,
                (true, false) => new_body,
            };
            for k in t {
                let i = *k as usize;
                if let (Some(p), Some(n)) = (am.positions.get(i), am.normals.get(i)) {
                    sc.tri(p.to_f32(), n.to_f32(), col);
                    if new && cut {
                        sc.xray_tri(p.to_f32(), n.to_f32(), removed);
                    }
                }
            }
        }
        for (ei, e) in am.edges.iter().enumerate() {
            if !am.shaded_edge_visible(ei) {
                continue;
            }
            for w in e.windows(2) {
                sc.line(w[0].to_f32(), w[1].to_f32(), edge, 1.3, false);
            }
        }
        if old.is_some() {
            replaced.push(a.name.clone());
        }
    }
    for b in &before.bodies {
        let gone = after.body(&b.name).is_none();
        if !gone && !replaced.contains(&b.name) {
            continue;
        }
        let (shared, tol) = sharing(b, after);
        let shared: Vec<(&Mesh, &[bool])> = shared.iter().map(|(m, named)| (m.as_ref(), named.as_slice())).collect();
        // Where the change adds material (a round in a concave corner, a join), the old surface
        // lies under the new and is pushed in, to stay hidden; where it removes, out (see `ghost`).
        let grew = after.body(&b.name).is_some_and(|a| a.mesh().measure().volume > b.mesh().measure().volume + 1e-6);
        for t in ghost(&b.mesh(), &shared, if grew { -tol } else { tol }) {
            for (p, n) in t {
                sc.ghost_tri(p.to_f32(), n.normalized().unwrap_or(n).to_f32(), removed);
            }
        }
        if gone {
            replaced.push(b.name.clone());
        }
    }
    let sketches = after
        .sketches
        .iter()
        .filter_map(|sketch| {
            let old = before.sketch(sketch.feature);
            let old_curves =
                old.into_iter().flat_map(|s| s.sketch.curves.iter().map(|c| (c.id.as_str(), c))).collect::<std::collections::HashMap<_, _>>();
            let old_points =
                old.into_iter().flat_map(|s| s.sketch.points.iter().map(|p| (p.id.as_str(), p.pos))).collect::<std::collections::HashMap<_, _>>();
            let indices = sketch
                .sketch
                .curves
                .iter()
                .enumerate()
                .filter_map(|(index, curve)| {
                    let unchanged = old_curves.get(curve.id.as_str()).is_some_and(|previous| {
                        previous.kind == curve.kind
                            && curve.kind.point_ids().iter().all(|i| old.and_then(|s| s.sketch.point(*i)) == sketch.sketch.point(*i))
                    });
                    if unchanged { None } else { Some(index) }
                })
                .collect::<Vec<_>>();
            let points = sketch
                .sketch
                .points
                .iter()
                .enumerate()
                .filter_map(|(i, p)| if old_points.get(p.id.as_str()) == Some(&p.pos) { None } else { Some(i) })
                .collect::<Vec<_>>();
            if indices.is_empty() && points.is_empty() { None } else { Some(SketchPreview { sketch: sketch.clone(), curves: indices, points }) }
        })
        .collect();
    Built { scene: sc, sketches, replaced, error: None, bounds: Aabb3::EMPTY, applied: None, tapered: Vec::new(), stopped: None }
}

/// The meshes of the bodies after the change that may share an old body's surface (its new
/// version, a body a join made of it and its tool, the pieces a split made), each with the
/// faces its names call new (see [`named_new`]), and the largest display tolerance among them
/// and the old body: how far apart two meshes of one surface may lie.
type Shared = (std::sync::Arc<Mesh>, Vec<bool>);
fn sharing(old: &ModelBody, after: &ModelState) -> (Vec<Shared>, f64) {
    let bounds = old.mesh().bounds();
    let shared: Vec<&ModelBody> = after.bodies.iter().filter(|b| b.name == old.name || b.mesh().bounds().meets(&bounds)).collect();
    let tol = shared.iter().map(|b| b.display_tol()).fold(old.display_tol(), f64::max);
    (shared.iter().map(|b| (b.mesh(), named_new(b, old))).collect(), tol)
}

/// Which faces of `new` are named after none of `old`'s (pieces of one face aside): faces the
/// change made, told by the history the face names follow (or by naming's own reading of the
/// geometry). A small round lies so close to the corner it replaces, within the tolerance two
/// meshes of one surface differ by, that the meshes alone can't tell it is new: a 1 mm round on
/// the sample plate was taken for the old surface, untinted, and the ghost was measured from it
/// in a sawtooth again. Empty when the faces are only numbered (a body no feature made), which
/// says nothing about where they came from.
fn named_new(new: &ModelBody, old: &ModelBody) -> Vec<bool> {
    use solidcraft_engine::doc::naming::{face_names, strip_piece};
    let numbered = |names: &[String], body: &str| names.iter().any(|n| n.starts_with(&format!("{body}:face:")));
    let (old_names, new_names) = (face_names(old), face_names(new));
    if numbered(&old_names, &old.name) || numbered(&new_names, &new.name) {
        return Vec::new();
    }
    let known: std::collections::HashSet<&str> = old_names.iter().map(|n| strip_piece(n)).collect();
    new_names.iter().map(|n| !known.contains(strip_piece(n))).collect()
}

/// Faces new by either reading: `off` the old surface, or `named` new.
fn either(mut off: Vec<bool>, named: &[bool]) -> Vec<bool> {
    for (o, n) in off.iter_mut().zip(named) {
        *o |= *n;
    }
    off
}

/// How many times a ghost triangle is halved to find where it leaves the new surface.
const GHOST_SPLITS: u32 = 12;

/// A ghost triangle on the new surface is split down to this share of the old body's size
/// before it is left out, so that a hole the change made inside it is found.
const GHOST_PIECE: f64 = 1.0 / 16.0;

/// A ghost vertex: its position and normal.
type GhostVertex = (Vec3, Vec3);

/// A ghost vertex while the ghost is cut: its position on the old mesh, its normal, and how far
/// it is drawn pushed off the old mesh (see [`pushes`]).
type ClipVertex = (Vec3, Vec3, Vec3);

/// Past the edge of the kept surface where new surface takes over (where a round leaves the
/// wall), the distance to the kept surface is taken this many times steeper, so the ghost
/// starts right at that edge and not a tolerance and a half from it.
const GHOST_BEYOND: f64 = 8.0;

/// The ghost: the part of the old mesh that is not on the surface the bodies after the change
/// (`new`) keep of it, i.e. the material the change removed, as triangles. Two meshes of one
/// surface are different chord approximations up to `tol` apart, so the old surface a change
/// keeps can't be told from the new by depth: drawn whole, the old body showed through the
/// new one as a red sawtooth on curved faces (under a fillet on a cylinder, on a split or
/// joined wall). So the old surface is left out only where it lies within `tol` and a half of
/// the kept surface, and triangles that cross that line are split and cut along it. The kept
/// surface is the new faces' complement (see [`KeptSurface`]): measured from the round as
/// well, the removed corner thins out to nothing beside it and the ghost stopped short of it
/// facet by facet, a sawtooth again; measured from the wall the round leaves, the distance
/// grows straight away from the round's edge and the ghost reaches it in a straight line.
/// What is left lies in front of the new surface, and is drawn so, pushed out by `tol` as well:
/// the new mesh may stand that far proud of the old one where they meet (the round's facets
/// over the wall's), and won the depth test there in pale spikes through the ghost. A negative
/// `tol` pushes it in instead, for old surface the change buries under new material.
fn ghost(old: &Mesh, new: &[(&Mesh, &[bool])], tol: f64) -> Vec<[GhostVertex; 3]> {
    // Each face is pushed along its own normals, so nothing on it slides along it (the line where
    // the ghost ends stays where it was cut); where that opens a sharp edge, a seam closes it.
    let vertex = |(p, n): (Vec3, Vec3)| -> ClipVertex { (p, n, n.normalized().unwrap_or_default() * tol) };
    let seams: Vec<[ClipVertex; 3]> = seams(old, tol > 0.0).into_iter().map(|t| t.map(vertex)).collect();
    let tol = tol.abs();
    let tris = old.triangles.iter().filter_map(|t| {
        let [a, b, c] = old.tri(t)?;
        let n = |k: u32| old.normals.get(k as usize).copied();
        let [na, nb, nc] = [n(t[0])?, n(t[1])?, n(t[2])?];
        Some(([(a, na), (b, nb), (c, nc)].map(vertex), *t))
    });
    let drawn = |t: [ClipVertex; 3]| t.map(|(p, n, push)| (p + push, n));
    let clear = 1.5 * tol;
    let index = if tol > 0.0 { KeptSurface::of(old, new, 2.0 * clear) } else { None };
    let Some(index) = index else { return tris.map(|(t, _)| t).chain(seams).map(drawn).collect() };
    // A face the change doesn't touch is triangulated as before: its triangles are kept surface,
    // found without measuring (a modeled thread beside a fillet has a hundred thousand).
    let key = |t: [Vec3; 3]| {
        let mut k = t.map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]);
        k.sort_unstable();
        k
    };
    let unchanged: std::collections::HashSet<_> = new.iter().flat_map(|(m, _)| m.triangles.iter().filter_map(|t| m.tri(t))).map(key).collect();
    let mut near: Vec<Option<f64>> = vec![None; old.positions.len()];
    let piece = old.bounds().diagonal() * GHOST_PIECE;
    let mut clip = GhostClip { index: &index, clear, piece, fine: 4.0 * clear, out: Vec::new() };
    for (t, k) in tris {
        if unchanged.contains(&key(t.map(|(p, _, _)| p))) {
            continue;
        }
        let d = k.map(|k| {
            let at = old.positions.get(k as usize).copied().unwrap_or_default();
            near.get_mut(k as usize).map_or(0.0, |d| *d.get_or_insert_with(|| index.distance(at)))
        });
        clip.split(t, d, GHOST_SPLITS);
    }
    for t in seams {
        let d = t.map(|(p, _, _)| index.distance(p));
        clip.split(t, d, GHOST_SPLITS);
    }
    clip.out.into_iter().map(drawn).collect()
}

/// The seams that close an old mesh pushed off itself along its normals (out when `out`, else
/// in): along each sharp edge its two faces' copies of the edge part, and a seam joins them,
/// shaded from the one face's normal to the other's like a small round; where three or more
/// faces meet, a fan joins their copies of the corner. Only edges the push opens are seamed
/// (convex ones pushing out, concave ones pushing in). The faces' copies of a vertex differ in
/// their last bits, so positions are matched to within a hair. As (position, normal) triangles.
fn seams(old: &Mesh, out: bool) -> Vec<[(Vec3, Vec3); 3]> {
    let hair = (old.bounds().diagonal() * 1e-7).max(1e-12);
    let cell = |p: Vec3| [p.x, p.y, p.z].map(|v| (v / hair).round() as i64);
    let at = |k: u32| Some((old.positions.get(k as usize).copied()?, old.normals.get(k as usize).copied()?.normalized()?));
    let same = |n: Vec3, m: Vec3| n.dot(m) > 0.9999;
    // Each side of each triangle, by its ends' cells: (its ends, the corner across from it).
    let mut sides: std::collections::HashMap<[[i64; 3]; 2], Vec<[u32; 3]>> = std::collections::HashMap::new();
    for t in &old.triangles {
        for (a, b, c) in [(t[0], t[1], t[2]), (t[1], t[2], t[0]), (t[2], t[0], t[1])] {
            let (Some((pa, _)), Some((pb, _))) = (at(a), at(b)) else { continue };
            let (ka, kb) = (cell(pa), cell(pb));
            let (key, ends) = if ka <= kb { ([ka, kb], [a, b, c]) } else { ([kb, ka], [b, a, c]) };
            sides.entry(key).or_default().push(ends);
        }
    }
    let mut seams = Vec::new();
    for pair in sides.values() {
        let [[a0, a1, ac], [b0, b1, _]] = pair.as_slice() else { continue };
        let (Some(a0), Some(a1), Some(b0), Some(b1), Some((pc, _))) = (at(*a0), at(*a1), at(*b0), at(*b1), at(*ac)) else { continue };
        if same(a0.1, b0.1) && same(a1.1, b1.1) {
            continue;
        }
        // Convex: the one face lies behind the other's plane.
        let convex = (pc - a0.0).dot(b0.1) < 0.0;
        if convex == out {
            seams.push([a0, a1, b1]);
            seams.push([a0, b1, b0]);
        }
    }
    // Corners: the distinct normals at each position, fanned round their mean.
    let mut corners: std::collections::HashMap<[i64; 3], Vec<(Vec3, Vec3)>> = std::collections::HashMap::new();
    for k in 0..old.positions.len() as u32 {
        if let Some((p, n)) = at(k) {
            let here = corners.entry(cell(p)).or_default();
            if !here.iter().any(|(_, m)| same(n, *m)) {
                here.push((p, n));
            }
        }
    }
    for mut here in corners.into_values().filter(|h| h.len() >= 3) {
        let Some(mean) = here.iter().fold(Vec3::ZERO, |a, (_, n)| a + *n).normalized() else { continue };
        let x = (here[0].1 - mean * here[0].1.dot(mean)).normalized().unwrap_or_default();
        let y = mean.cross(x);
        here.sort_by(|(_, n), (_, m)| n.dot(y).atan2(n.dot(x)).total_cmp(&m.dot(y).atan2(m.dot(x))));
        for k in 2..here.len() {
            if let (Some(a), Some(b), Some(c)) = (here.first(), here.get(k - 1), here.get(k)) {
                seams.push([*a, *b, *c]);
            }
        }
    }
    seams
}

/// The surface the bodies after a change keep of an old body's, to measure the old surface
/// from: their faces that are not new (off the old surface, see [`new_faces`], or named new),
/// and the new faces only where a point lies exactly on them (a face a join made of an old one
/// and a flush new one).
struct KeptSurface {
    kept: Option<SurfaceIndex>,
    fresh: Option<SurfaceIndex>,
    exact: f64,
    far: f64,
}

impl KeptSurface {
    /// None when the bodies after the change have no surface.
    fn of(old: &Mesh, new: &[(&Mesh, &[bool])], far: f64) -> Option<KeptSurface> {
        let index = SurfaceIndex::new(old);
        let (mut kept, mut fresh) = (Vec::new(), Vec::new());
        for (m, named) in new {
            let nf = m.tri_face.iter().max().map_or(0, |f| *f as usize + 1);
            let off = either(faces_off(m, nf, index.as_ref()), named);
            for (t, f) in m.triangles.iter().zip(&m.tri_face) {
                if let Some(t) = m.tri(t) {
                    if off.get(*f as usize).copied().unwrap_or(true) { fresh.push(t) } else { kept.push(t) }
                }
            }
        }
        let exact = (old.bounds().diagonal() * 1e-9).max(1e-12);
        let seam = exact * 1e3;
        let fresh = SurfaceIndex::of_tris(fresh, 2.0 * seam);
        let mut kept = SurfaceIndex::of_tris(kept, far);
        if let (Some(kept), Some(fresh)) = (&mut kept, &fresh) {
            // The sides and corners of kept triangles that new surface continues: a fan of
            // triangles meets the edge of a face at single corners too.
            let on = |p: Vec3| fresh.distance(p) <= seam;
            let open = |u: Vec3, v: Vec3| on(u) && on(v) && on((u + v) / 2.0);
            kept.open = kept
                .tris
                .iter()
                .map(|[a, b, c]| {
                    let sides = u8::from(open(*a, *b)) | u8::from(open(*b, *c)) << 1 | u8::from(open(*c, *a)) << 2;
                    sides | u8::from(on(*a)) << 3 | u8::from(on(*b)) << 4 | u8::from(on(*c)) << 5
                })
                .collect();
        }
        (kept.is_some() || fresh.is_some()).then_some(KeptSurface { kept, fresh, exact, far })
    }

    /// How far `p` is from the kept surface, or `far` when it is at least that far.
    fn distance(&self, p: Vec3) -> f64 {
        if self.fresh.as_ref().is_some_and(|f| f.distance(p) <= self.exact) {
            return 0.0;
        }
        self.kept.as_ref().map_or(self.far, |k| k.reach(p, GHOST_BEYOND))
    }
}

/// Halvings of a side to find where the ghost ends on it.
const GHOST_CROSSING: u32 = 12;

/// The vertex `s` of the way from `u` to `v`.
fn lerp(u: ClipVertex, v: ClipVertex, s: f64) -> ClipVertex {
    let at = |a: Vec3, b: Vec3| a + (b - a) * s;
    (at(u.0, v.0), at(u.1, v.1), at(u.2, v.2))
}

struct GhostClip<'a> {
    /// The surface the change keeps.
    index: &'a KeptSurface,
    /// At least this far from the new surface is removed material.
    clear: f64,
    /// See [`GHOST_PIECE`].
    piece: f64,
    /// A triangle that crosses where the ghost ends is halved until no side is longer, then cut.
    fine: f64,
    out: Vec<[ClipVertex; 3]>,
}

impl GhostClip<'_> {
    /// Keeps triangle `t`, whose corners lie `d` from the new mesh, or the part of it that is
    /// removed material. A triangle that can't be decided is halved across its longest side, so
    /// a long thin one (a strip of a cylinder's wall) is cut along its length only.
    fn split(&mut self, t: [ClipVertex; 3], d: [f64; 3], splits: u32) {
        let [a, b, c] = t;
        let mid = |u: ClipVertex, v: ClipVertex| lerp(u, v, 0.5);
        let m = [mid(a, b), mid(b, c), mid(c, a)];
        let dm = m.map(|(p, _, _)| self.index.distance(p));
        let centre = self.index.distance((a.0 + b.0 + c.0) / 3.0);
        let samples = || d.iter().chain(&dm).chain([&centre]);
        let sides = [a.0.dist(b.0), b.0.dist(c.0), c.0.dist(a.0)];
        let longest = sides.iter().copied().fold(0.0, f64::max);
        if samples().all(|d| *d >= self.clear) {
            self.out.push(t);
        } else if samples().all(|d| *d < self.clear) && longest <= self.piece {
            // On the new surface: nothing removed here.
        } else if splits > 0 && longest > self.fine {
            // Rotate so the longest side is the first (a to b), then halve it.
            let (t, d, dm) = match sides.iter().position(|s| *s >= longest).unwrap_or(0) {
                1 => ([b, c, a], [d[1], d[2], d[0]], dm[1]),
                2 => ([c, a, b], [d[2], d[0], d[1]], dm[2]),
                _ => (t, d, dm[0]),
            };
            let [a, b, c] = t;
            let ab = mid(a, b);
            self.split([a, ab, c], [d[0], dm, d[2]], splits - 1);
            self.split([ab, b, c], [dm, d[1], d[2]], splits - 1);
        } else {
            self.cut(t, d);
        }
    }

    /// How far along from `p` to `q` the ghost ends, `kept` saying whether it holds at `p`.
    fn crossing(&self, p: Vec3, q: Vec3, kept: bool) -> f64 {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..GHOST_CROSSING {
            let s: f64 = (lo + hi) / 2.0;
            if (self.index.distance(p + (q - p) * s) >= self.clear) == kept {
                lo = s;
            } else {
                hi = s;
            }
        }
        (lo + hi) / 2.0
    }

    /// Keeps the part of `t` at least `clear` from the new mesh, cut where its sides cross that
    /// line (found by halving: past the kept surface's edge the distance is far from linear).
    fn cut(&mut self, t: [ClipVertex; 3], d: [f64; 3]) {
        let mut kept: Vec<ClipVertex> = Vec::with_capacity(4);
        let next = ([t[1], t[2], t[0]], [d[1], d[2], d[0]]);
        for ((p, dp), (q, dq)) in t.into_iter().zip(d).zip(next.0.into_iter().zip(next.1)) {
            if dp >= self.clear {
                kept.push(p);
            }
            if (dp >= self.clear) != (dq >= self.clear) {
                kept.push(lerp(p, q, self.crossing(p.0, q.0, dp >= self.clear)));
            }
        }
        for k in 2..kept.len() {
            if let (Some(a), Some(b), Some(c)) = (kept.first(), kept.get(k - 1), kept.get(k)) {
                self.out.push([*a, *b, *c]);
            }
        }
    }
}

/// Which of the `nf` faces of `m` are new surface, i.e. not on the old mesh? Each is decided by
/// a vote of points on it (the centroids of up to [`FACE_SAMPLES`] of its triangles, spread over
/// the face), weighted by their triangles' areas: a round's triangles crowd where it blends
/// into the faces beside it, close to the old surface, and an even vote of nine of them called
/// a 2 mm round on a plate old surface, drawn untinted. One pass over `m`'s triangles gathers
/// the centroids, and the old mesh is indexed once, so the cost stays near-linear however many
/// faces there are (a modeled thread has thousands).
fn new_faces(m: &Mesh, nf: usize, old: &Mesh) -> Vec<bool> {
    faces_off(m, nf, SurfaceIndex::new(old).as_ref())
}

/// [`new_faces`] against an index of the old mesh (None: it has no surface).
fn faces_off(m: &Mesh, nf: usize, index: Option<&SurfaceIndex>) -> Vec<bool> {
    let mut cents: Vec<Vec<(Vec3, f64)>> = vec![Vec::new(); nf];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if let (Some(face), Some([a, b, c])) = (cents.get_mut(*f as usize), m.tri(t)) {
            face.push(((a + b + c) / 3.0, (b - a).cross(c - a).len() / 2.0));
        }
    }
    cents
        .iter()
        .map(|cents| {
            let Some(index) = index else { return true };
            if cents.is_empty() {
                return true;
            }
            let step = cents.len().div_ceil(FACE_SAMPLES);
            let (mut off, mut all) = (0.0, 0.0);
            for (p, area) in cents.iter().step_by(step.max(1)) {
                all += area;
                if !index.contains(*p) {
                    off += area;
                }
            }
            if all > 0.0 { off * 3.0 > all } else { true }
        })
        .collect()
}

/// Most points a face's vote in [`new_faces`] takes.
const FACE_SAMPLES: usize = 64;

/// The triangles of a mesh in a uniform grid, to ask whether a point lies on the surface (within
/// its tessellation tolerance) by looking only at the triangles near it.
struct SurfaceIndex {
    tris: Vec<[Vec3; 3]>,
    /// Per triangle, its sides (bit 0: a to b, 1: b to c, 2: c to a) and corners (bits 3 to 5:
    /// a, b, c) at an edge of the surface past which [`SurfaceIndex::reach`] grows steeper; none
    /// when empty.
    open: Vec<u8>,
    tol: f64,
    lo: [f64; 3],
    cell: [f64; 3],
    res: usize,
    cells: Vec<Vec<u32>>,
}

impl SurfaceIndex {
    /// Most triangle-in-cell entries the grid may hold; a coarser grid is used beyond it.
    const BUDGET: usize = 4_000_000;

    /// None for a mesh with no surface.
    fn new(m: &Mesh) -> Option<SurfaceIndex> {
        Self::of(&[m], (m.bounds().diagonal() * 2e-3).max(1e-4))
    }

    /// An index of the surface of `meshes` together that finds it up to `tol` from a point.
    /// None when they have no surface.
    fn of(meshes: &[&Mesh], tol: f64) -> Option<SurfaceIndex> {
        Self::of_tris(meshes.iter().flat_map(|m| m.triangles.iter().filter_map(|t| m.tri(t))).collect(), tol)
    }

    /// An index of the surface of triangles `tris`.
    fn of_tris(tris: Vec<[Vec3; 3]>, tol: f64) -> Option<SurfaceIndex> {
        let mut bb = Aabb3::EMPTY;
        tris.iter().flatten().for_each(|p| bb.add(*p));
        if bb.is_empty() || !tol.is_finite() || tol <= 0.0 {
            return None;
        }
        let lo = [bb.min.x - tol, bb.min.y - tol, bb.min.z - tol];
        let ext = [bb.max.x - bb.min.x + 2.0 * tol, bb.max.y - bb.min.y + 2.0 * tol, bb.max.z - bb.min.z + 2.0 * tol];
        // Four cells per cube root of the triangle count along each axis: few triangles to test
        // per point, as the ghost asks for many points.
        let mut res = ((tris.len() as f64).cbrt() * 4.0).round().clamp(1.0, 128.0) as usize;
        loop {
            let cell = [ext[0] / res as f64, ext[1] / res as f64, ext[2] / res as f64];
            let mut index = SurfaceIndex { tris: Vec::new(), open: Vec::new(), tol, lo, cell, res, cells: vec![Vec::new(); res * res * res] };
            let mut entries = 0usize;
            'fill: for (i, t) in tris.iter().enumerate() {
                let (a, b) = index.range(t);
                for x in a[0]..=b[0] {
                    for y in a[1]..=b[1] {
                        for z in a[2]..=b[2] {
                            entries += 1;
                            if entries > Self::BUDGET && res > 1 {
                                break 'fill;
                            }
                            if let Some(c) = index.cells.get_mut((x * res + y) * res + z) {
                                c.push(i as u32);
                            }
                        }
                    }
                }
            }
            if entries <= Self::BUDGET || res == 1 {
                index.tris = tris;
                return Some(index);
            }
            res /= 2;
        }
    }

    fn cell_of(&self, v: f64, axis: usize) -> usize {
        let (lo, cell) = (self.lo.get(axis).copied().unwrap_or(0.0), self.cell.get(axis).copied().unwrap_or(1.0));
        let k = ((v - lo) / cell).floor();
        if k.is_finite() { (k.max(0.0) as usize).min(self.res - 1) } else { 0 }
    }

    /// The cells (inclusive corners) a triangle, grown by the tolerance, touches.
    fn range(&self, t: &[Vec3; 3]) -> ([usize; 3], [usize; 3]) {
        let (mut a, mut b) = ([0; 3], [0; 3]);
        for axis in 0..3 {
            let c = t.map(|p| [p.x, p.y, p.z][axis]);
            let (lo, hi) = (c[0].min(c[1]).min(c[2]) - self.tol, c[0].max(c[1]).max(c[2]) + self.tol);
            a[axis] = self.cell_of(lo, axis);
            b[axis] = self.cell_of(hi, axis);
        }
        (a, b)
    }

    /// Does `p` lie on the surface?
    fn contains(&self, p: Vec3) -> bool {
        let (x, y, z) = (self.cell_of(p.x, 0), self.cell_of(p.y, 1), self.cell_of(p.z, 2));
        let Some(cell) = self.cells.get((x * self.res + y) * self.res + z) else { return false };
        cell.iter().filter_map(|i| self.tris.get(*i as usize)).any(|[a, b, c]| point_triangle_dist(p, *a, *b, *c) < self.tol)
    }

    /// How far `p` is from the surface, or the tolerance when it is at least that far.
    fn distance(&self, p: Vec3) -> f64 {
        self.reach(p, 1.0)
    }

    /// [`SurfaceIndex::distance`], but past an open side or corner of a triangle (see `open`) its part
    /// along the triangle's plane counts `steep` times.
    fn reach(&self, p: Vec3, steep: f64) -> f64 {
        let (x, y, z) = (self.cell_of(p.x, 0), self.cell_of(p.y, 1), self.cell_of(p.z, 2));
        let Some(cell) = self.cells.get((x * self.res + y) * self.res + z) else { return self.tol };
        let open = |i: u32| self.open.get(i as usize).copied().unwrap_or(0);
        cell.iter()
            .filter_map(|i| Some((self.tris.get(*i as usize)?, open(*i))))
            .map(|([a, b, c], open)| if open == 0 { point_triangle_dist(p, *a, *b, *c) } else { point_triangle_reach(p, [*a, *b, *c], open, steep) })
            .fold(self.tol, f64::min)
    }
}

/// [`point_triangle_dist`], with the part along the triangle's plane counting `steep` times
/// past the sides and corners marked in `open` (bits as in [`SurfaceIndex::open`]).
fn point_triangle_reach(p: Vec3, [a, b, c]: [Vec3; 3], open: u8, steep: f64) -> f64 {
    let n = (b - a).cross(c - a);
    let Some(nn) = n.normalized() else { return f64::INFINITY };
    let h = (p - a).dot(nn);
    let q = p - nn * h;
    let sides = [(a, b), (b, c), (c, a)];
    if sides.iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= -1e-12) {
        return h.abs();
    }
    let corner = |k: usize| open & (1 << (3 + k % 3)) != 0;
    sides
        .iter()
        .enumerate()
        .map(|(k, (u, v))| {
            let d = p.dist_to_segment(*u, *v);
            // Nearest an open side, or an open corner at its end.
            let s = (p - *u).dot(*v - *u) / (*v - *u).dot(*v - *u).max(f64::MIN_POSITIVE);
            let open = open & (1 << k) != 0 || (s <= 0.0 && corner(k)) || (s >= 1.0 && corner(k + 1));
            if open { h.abs() + steep * (d * d - h * h).max(0.0).sqrt() } else { d }
        })
        .fold(f64::INFINITY, f64::min)
}

pub fn point_triangle_dist(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let n = (b - a).cross(c - a);
    let Some(nn) = n.normalized() else { return f64::INFINITY };
    let h = (p - a).dot(nn);
    let q = p - nn * h;
    let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= -1e-12);
    if inside { h.abs() } else { [(a, b), (b, c), (c, a)].iter().map(|(u, v)| p.dist_to_segment(*u, *v)).fold(f64::INFINITY, f64::min) }
}

/// The same computed sketch preview is drawn over both native GPU and CPU viewports.
pub fn paint_sketches(app: &SolveApp, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    if app.preview.rolled_back {
        return;
    }
    if !app
        .dialog
        .as_ref()
        .is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::SketchModify(_) | crate::dialogs::Kind::InsertDrawing(_)) && d.previews())
    {
        return;
    }
    let stroke = egui::Stroke::new(2.5, Tokens::get().preview_sketch);
    let mut budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
    let mut curves = 0;
    for preview in &app.preview.sketches {
        let sketch = &preview.sketch;
        for index in &preview.curves {
            crate::tools::paint_curve(&sketch.sketch, *index, &sketch.plane, painter, proj, stroke, &mut budget);
            curves += 1;
        }
        for index in &preview.points {
            if let Some(point) = sketch.sketch.point(*index).and_then(|p| proj.to_screen(sketch.plane.to_world(p))) {
                painter.circle_filled(point, 3.0, stroke.color);
            }
        }
    }
    crate::scenario::publish_count("sketch_preview_curves", curves as f64);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_preview_follows_tab_without_recolouring_removal_or_idle_previews() {
        for dark in [false, true] {
            for vision in crate::theme::ColorVision::ALL {
                let t = Tokens::themed(dark, vision);
                let base = Colors::from_tokens(&t);
                for tab in ["MODEL", "SKETCH", "SURFACE", "ASSEMBLE", "INSERT", "INSPECT", "MANAGE", "VIEW", "SHEET METAL", "RENDER"] {
                    for active in [false, true] {
                        let colors = Colors::for_tool(&t, tab, active);
                        assert_eq!(colors.removed, base.removed);
                        assert_eq!(colors.body, base.body);
                        assert_eq!(colors.edge, base.edge);
                        if !active || crate::theme::TabAccent::for_tab(&t, tab).color == t.accent {
                            assert_eq!(colors.added, base.added);
                            assert_eq!(colors.new_body, base.new_body);
                        } else {
                            let accent = crate::theme::TabAccent::for_tab(&t, tab);
                            assert_eq!(colors.added, accent.color.to_srgba_unmultiplied());
                            assert_ne!(colors.new_body, colors.added);
                        }
                    }
                }
            }
        }
    }

    fn colors() -> Colors {
        Colors { body: [1, 1, 1, 255], added: [2, 2, 2, 255], new_body: [4, 4, 4, 255], removed: [3, 3, 3, 100], edge: [0, 0, 0, 255], ghost: true }
    }

    #[test]
    fn correcting_inputs_clears_old_errors_and_discards_late_old_jobs() {
        let mut pv = PreviewState {
            shown_key: Some(1),
            error: Some("unknown_angle".into()),
            active: true,
            replaced: vec!["Turn".into()],
            key: 9,
            ..PreviewState::default()
        };
        let (tx, rx) = std::sync::mpsc::channel();
        pv.running = Some((1, rx));
        pv.poll(2, None);
        assert!(pv.error.is_none(), "the corrected inputs can Apply while their preview computes");
        assert!(pv.running.is_some());
        assert!(pv.active);
        assert_eq!(pv.replaced, ["Turn"]);
        assert_eq!(pv.key, 9, "the last good geometry stays visible");
        tx.send(Err("unknown_angle".into())).unwrap();
        pv.poll(2, None);
        assert!(pv.error.is_none(), "a late obsolete result cannot disable Apply again");
        assert!(pv.running.is_none());
        assert_eq!(pv.shown_key, Some(1));
        assert_eq!(pv.key, 9);

        let (tx, rx) = std::sync::mpsc::channel();
        pv.running = Some((2, rx));
        tx.send(Err("current invalid expression".into())).unwrap();
        pv.poll(2, None);
        assert_eq!(pv.error.as_deref(), Some("current invalid expression"));
        assert_eq!(pv.shown_key, Some(2));
        assert!(!pv.active && pv.replaced.is_empty(), "the current inputs failed: nothing is previewed (#3254)");
        assert_ne!(pv.key, 9, "the viewport takes the empty scene");
        pv.poll(2, None);
        assert_eq!(pv.error.as_deref(), Some("current invalid expression"), "current errors still prevent invalid Apply");
    }

    fn pose() -> Built {
        Built {
            scene: GpuScene::default(),
            sketches: Vec::new(),
            replaced: vec!["Lid".into()],
            error: None,
            bounds: Aabb3::EMPTY,
            applied: None,
            tapered: Vec::new(),
            stopped: None,
        }
    }

    /// The worker completes while the slider is still held or Play keeps advancing.
    /// Intermediate successful poses must reach the viewport, and the final value
    /// must still be computed rather than treating that intermediate pose as current.
    #[test]
    fn joint_motion_shows_completed_poses_while_inputs_keep_advancing() {
        let mut pv = PreviewState::default();
        for (running, wanted) in [(1, 2), (2, 3), (3, 4)] {
            let (tx, rx) = std::sync::mpsc::channel();
            pv.running = Some((running, rx));
            pv.running_stream_key = Some(42);
            pv.poll(wanted, Some(42));
            assert!(pv.running.is_some());
            tx.send(Ok(pose())).unwrap();
            pv.poll(wanted, Some(42));
            assert!(pv.active, "motion should be visible before input stops");
            assert_eq!(pv.replaced, ["Lid"]);
            assert_eq!(pv.key, running, "every completed pose reaches the scene");
            assert_eq!(pv.shown_key, Some(running));
            assert_ne!(pv.shown_key, Some(wanted), "the latest pose still needs a job");
        }
        let (tx, rx) = std::sync::mpsc::channel();
        pv.running = Some((4, rx));
        pv.running_stream_key = Some(42);
        tx.send(Ok(pose())).unwrap();
        pv.poll(4, Some(42));
        assert_eq!(pv.shown_key, Some(4), "release catches up to the exact final pose");
        assert!(pv.running.is_none());
    }

    #[test]
    fn intermediate_motion_rejects_other_setups_and_obsolete_errors() {
        for (context, job) in [
            (None, Ok(pose())),
            (Some(43), Ok(pose())),
            (Some(42), Err("old invalid value".into())),
            (Some(42), Ok(Built { error: Some("old tool preview error".into()), ..pose() })),
        ] {
            let mut pv = PreviewState { running_stream_key: Some(42), ..PreviewState::default() };
            let (tx, rx) = std::sync::mpsc::channel();
            pv.running = Some((1, rx));
            tx.send(job).unwrap();
            pv.poll(2, context);
            assert!(!pv.active);
            assert!(pv.error.is_none(), "obsolete errors must not block Apply");
            assert!(pv.running.is_none());
        }
    }

    #[test]
    fn motion_context_changes_only_when_the_joint_setup_changes() {
        let commands = |angle| {
            vec![
                ("joint.edit".into(), json!({"joint": 1, "flip": true})),
                ("joint.limits".into(), json!({"joint": 1, "min": "-135 deg", "max": "0 deg"})),
                ("joint.drive".into(), json!({"joint": 1, "values": [angle]})),
            ]
        };
        let key = motion_key(&commands("-30 deg"), 7, colors());
        assert!(key.is_some());
        assert_eq!(key, motion_key(&commands("-60 deg"), 7, colors()));
        assert_ne!(key, motion_key(&commands("-60 deg"), 8, colors()));
        let mut different = commands("-60 deg");
        different[2].1["joint"] = json!(2);
        assert_ne!(key, motion_key(&different, 7, colors()));
        different = commands("-60 deg");
        different[1].1["min"] = json!("-90 deg");
        assert_ne!(key, motion_key(&different, 7, colors()));
        different = commands("-60 deg");
        different[0].1["flip"] = json!(false);
        assert_ne!(key, motion_key(&different, 7, colors()));
        let mut other_colors = colors();
        other_colors.body = [9, 9, 9, 255];
        assert_ne!(key, motion_key(&commands("-60 deg"), 7, other_colors));
        assert!(motion_key(&[("solid.extrude".into(), json!({"distance": 10}))], 7, colors()).is_none());
        for command in ["joint.create", "joint.drive"] {
            assert_eq!(
                motion_key(&[(command.into(), json!({"joint": 1, "values": [0]}))], 7, colors()),
                motion_key(&[(command.into(), json!({"joint": 1, "values": [90]}))], 7, colors()),
            );
        }
        // A gear dialog playing its driving joint streams its poses too.
        let gear = |angle: &str| {
            vec![
                ("joint.gear".to_string(), json!({"a": 1, "b": 2, "ra": 10, "rb": 20})),
                ("joint.drive".to_string(), json!({"joint": 1, "values": [angle]})),
            ]
        };
        assert!(motion_key(&gear("0 deg"), 7, colors()).is_some());
        assert_eq!(motion_key(&gear("0 deg"), 7, colors()), motion_key(&gear("90 deg"), 7, colors()));
        // So does a cam dialog turning its cam.
        let cam = |angle: &str| {
            vec![
                ("joint.cam".to_string(), json!({"a": 1, "b": 2, "profile": {"sketch": 3}})),
                ("joint.drive".to_string(), json!({"joint": 1, "values": [angle]})),
            ]
        };
        assert!(motion_key(&cam("0 deg"), 7, colors()).is_some());
        assert_eq!(motion_key(&cam("0 deg"), 7, colors()), motion_key(&cam("90 deg"), 7, colors()));
    }

    #[test]
    fn edited_pattern_preview_includes_moved_instances_only() -> Result<(), Box<dyn std::error::Error>> {
        for (command, initial, edits) in [
            (
                "sketch.pattern.rectangular",
                json!({"entities":["seed"],"count":3,"spacing":10,"count2":2,"spacing2":15}),
                vec![(json!({"spacing":14}), 4), (json!({"spacing2":19}), 3)],
            ),
            (
                "sketch.pattern.circular",
                json!({"entities":["seed"],"center":"origin","count":5}),
                vec![(json!({"count":20}), 19), (json!({"count":2}), 1)],
            ),
        ] {
            let mut s = Session::default();
            s.execute("sketch.create", &json!({"plane":"XY"}))?;
            s.execute("sketch.circle.center", &json!({"center":[-50,0],"radius":2,"id":"seed"}))?;
            let pattern = s.execute(command, &initial)?;
            // An identical result must not draw the seed or any existing copies.
            let unchanged = compute(&s, &[(command.into(), json!({"pattern":pattern["pattern"]}))], colors())?;
            assert!(unchanged.sketches.is_empty());
            for (mut edit, expected) in edits {
                edit["pattern"] = pattern["pattern"].clone();
                let built = compute(&s, &[(command.into(), edit)], colors())?;
                assert_eq!(built.sketches.len(), 1, "{command}");
                for preview in &built.sketches {
                    assert_eq!(preview.curves.len(), expected, "{command}");
                    assert_eq!(preview.points.len(), expected, "{command}");
                    assert!(preview.curves.iter().all(|i| preview.sketch.sketch.curves.get(*i).is_some_and(|c| c.id != "seed")));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn sketch_preview_includes_changed_curve_shapes() -> Result<(), Box<dyn std::error::Error>> {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane":"XY"}))?;
        s.execute("sketch.circle.center", &json!({"center":[0,0],"radius":2,"id":"circle"}))?;
        s.execute("sketch.line", &json!({"points":[[10,0],[20,0]],"ids":["line"]}))?;
        let before = s.model.state();
        let mut after = before.as_ref().clone();
        for sketch in &mut after.sketches {
            let sketch = &mut std::sync::Arc::make_mut(sketch).sketch;
            for curve in &mut sketch.curves {
                match &mut curve.kind {
                    solidcraft_engine::sketch::CurveKind::Circle { r, .. } => *r = 3.0,
                    solidcraft_engine::sketch::CurveKind::Line { a, b } => std::mem::swap(a, b),
                    _ => {}
                }
            }
        }
        let built = build(&before, &after, colors());
        assert_eq!(built.sketches.len(), 1);
        for preview in &built.sketches {
            assert_eq!(preview.curves, vec![0, 1]);
            assert!(preview.points.is_empty());
        }
        Ok(())
    }

    /// Combine previews its result (cut, join or intersect between existing bodies) without an
    /// error, so its dialog's OK is enabled. Before, the preview also tried the command as a new
    /// body, which a combine has no form of, and always reported "operation must be join, cut or
    /// intersect".
    #[test]
    fn combine_previews_its_result_without_an_error() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20, "body_name": "Cube"})).unwrap();
        s.execute("solid.cylinder", &json!({"base": [10, 10, -5], "diameter": 6, "height": 30, "body_name": "Pin"})).unwrap();
        for op in ["cut", "join", "intersect"] {
            let cmd = ("solid.combine".to_string(), json!({"target": "Cube", "tools": ["Pin"], "operation": op, "keep_tools": false}));
            let built = compute(&s, &[cmd], colors()).unwrap_or_else(|e| panic!("{op}: {e}"));
            assert!(built.error.is_none(), "{op}: {:?}", built.error);
        }
    }

    /// A plate in its own component, stood up by its placement (an imported part set upright)
    /// and edited with that component active: the extrusion's tool is drawn where the plate is
    /// shown, on its top face, not at right angles to it in the component's own frame.
    #[test]
    fn a_tool_on_a_turned_component_previews_where_it_is_shown() {
        let mut s = Session::default();
        let made = s.execute("component.create", &json!({"name": "Plate", "grounded": false})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 8, "height": 30, "corner": [0, 0, -30], "body_name": "Plate"})).unwrap();
        s.execute("occurrence.move", &json!({"occurrence": made["occurrence"], "axis": [1, 0, 0], "angle": "90 deg"})).unwrap();
        s.execute("component.activate", &json!({"component": made["component"], "occurrence": made["occurrence"], "path": [made["occurrence"]]}))
            .unwrap();
        // Shown lying flat: 20 × 30, 8 high. Its top face pulled up 10.
        let cmd = ("solid.extrude".to_string(), json!({"face": [10, 15, 8], "face_body": "Plate", "distance": "10 mm", "operation": "join"}));
        let b = compute_tool(&s, &[cmd], colors()).unwrap().bounds;
        assert!((b.max.z - 18.0).abs() < 1e-6 && (b.max.y - 30.0).abs() < 1e-6, "on top, not at right angles: {b:?}");
    }

    fn count(scene: &GpuScene, bytes: &[u8], stride: usize, col: [u8; 4]) -> usize {
        bytes
            .chunks(stride)
            .filter(|v| {
                let Some(i) = v.get(24..28).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes) else { return false };
                scene.palette.get(crate::gpu::PaletteIndex(i)) == Some(col)
            })
            .count()
    }

    #[test]
    fn a_hole_shows_its_cut_through_the_body() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let p = s.preview(&[("solid.hole".to_string(), json!({"position": [20, 15, 20], "diameter": 5, "depth": 10}))]).unwrap();
        let b = build(&p.before, &p.after, colors());
        // The hole's walls are drawn see-through in the cut colour (seen from any side).
        assert!(count(&b.scene, &b.scene.xray, 28, colors().removed) > 0);
        // A feature that only adds material shows no cut.
        let p = s.preview(&[("solid.box".to_string(), json!({"length": 10, "width": 10, "height": 30, "operation": "join"}))]).unwrap();
        let b = build(&p.before, &p.after, colors());
        assert_eq!(count(&b.scene, &b.scene.xray, 28, colors().removed), 0);
    }

    fn plate() -> Session {
        let mut s = Session::default();
        for (id, p) in [
            ("sketch.create", json!({"plane": "XY"})),
            ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})),
            ("sketch.finish", json!({})),
            ("solid.extrude", json!({"distance": 20})),
        ] {
            s.execute(id, &p).unwrap();
        }
        s
    }

    /// The surface test the preview used to run per face against every triangle.
    fn brute_force_on_surface(m: &Mesh, p: Vec3) -> bool {
        let bb = m.bounds();
        let tol = (bb.diagonal() * 2e-3).max(1e-4);
        m.triangles.iter().filter_map(|t| m.tri(t)).any(|[a, b, c]| point_triangle_dist(p, a, b, c) < tol)
    }

    /// A flat sheet of `n × n` squares, two triangles and one face each, lifted by `z`.
    fn sheet(n: usize, z: f64) -> Mesh {
        let mut m = Mesh::default();
        for i in 0..=n {
            for j in 0..=n {
                m.positions.push(Vec3::new(i as f64, j as f64, z));
                m.normals.push(Vec3::Z);
            }
        }
        for i in 0..n {
            for j in 0..n {
                let a = (i * (n + 1) + j) as u32;
                let (b, c, d) = (a + 1, a + n as u32 + 1, a + n as u32 + 2);
                m.triangles.extend([[a, c, d], [a, d, b]]);
                m.tri_face.extend([(i * n + j) as u32; 2]);
            }
        }
        m
    }

    /// The grid answers as a scan of every triangle does, inside, beside and far from the mesh.
    #[test]
    fn surface_index_matches_a_scan() {
        let m = sheet(12, 0.0);
        let index = SurfaceIndex::new(&m).unwrap();
        for k in 0..2000 {
            let f = k as f64;
            let p = Vec3::new((f * 0.37).rem_euclid(15.0) - 1.5, (f * 0.61).rem_euclid(15.0) - 1.5, ((f * 0.13).rem_euclid(1.0) - 0.5) * 0.05);
            assert_eq!(index.contains(p), brute_force_on_surface(&m, p), "{p:?}");
        }
        assert!(SurfaceIndex::new(&Mesh::default()).is_none());
    }

    /// How long the faces of a sheet of `n × n` squares take to classify against an old sheet of
    /// the same size. The meshes are built before the clock starts and the faces are collected
    /// before it stops: only the classification, the work that used to freeze the app, is measured.
    fn classify_sheet(n: usize) -> std::time::Duration {
        let (old, same, lifted) = (sheet(n, 0.0), sheet(n, 0.0), sheet(n, 5.0));
        let nf = n * n;
        let started = std::time::Instant::now();
        let on = new_faces(&same, nf, &old);
        let off = new_faces(&lifted, nf, &old);
        let took = started.elapsed();
        assert!(on.iter().all(|new| !new), "faces on the old surface are not new");
        assert!(off.iter().all(|new| *new), "faces off the old surface are new");
        took
    }

    /// Whether a job four times the size took at most about twice linear: `pair` times the small job,
    /// then the big one. Linear is 4x, the quadratic that the bug cost 16x or more, so eight times the
    /// small time is the limit. The load of the other test binaries can rise between the two runs, so a
    /// pair that misses is measured again, up to three times. Load only ever slows a run, so each size
    /// keeps its fastest time and the ratio is taken of those: a retry cannot pass a regression on a
    /// small run that a spike happened to slow, once an earlier small run was fast.
    ///
    /// `slack` is the room the noise of a busy machine gets, on top of the ratio.
    fn scales_linearly(mut pair: impl FnMut() -> (std::time::Duration, std::time::Duration), slack: std::time::Duration) -> Result<(), String> {
        let limit = |small: f64| 8.0 * small + slack.as_secs_f64();
        let (mut small, mut big) = (f64::INFINITY, f64::INFINITY);
        let mut runs = Vec::new();
        for _ in 0..3 {
            let (s, b) = pair();
            small = small.min(s.as_secs_f64());
            big = big.min(b.as_secs_f64());
            runs.push(format!("{s:?} then {b:?}"));
            if big < limit(small) {
                return Ok(());
            }
        }
        Err(format!("{}; fastest {small:.3} s and {big:.3} s, limit {:.3} s", runs.join(", "), limit(small)))
    }

    #[test]
    fn scales_linearly_keeps_each_sizes_fastest_run() {
        let pairs = |p: Vec<(u64, u64)>| {
            let mut p = p.into_iter().map(|(s, b)| (std::time::Duration::from_millis(s), std::time::Duration::from_millis(b)));
            move || p.next().unwrap_or((std::time::Duration::from_secs(1), std::time::Duration::from_secs(1)))
        };
        let slack = std::time::Duration::from_millis(0);
        // Linear: four times the work, four times the time.
        assert!(scales_linearly(pairs(vec![(5, 20)]), slack).is_ok());
        // A big run slowed by load, then a quiet pair.
        assert!(scales_linearly(pairs(vec![(5, 80), (5, 20)]), slack).is_ok());
        // A quadratic cost (16x) whose later small runs a spike slowed: the fast first one still counts.
        let r = scales_linearly(pairs(vec![(5, 80), (15, 80), (15, 80)]), slack);
        assert!(r.as_ref().is_err_and(|e| e.contains("fastest 0.005 s and 0.080 s, limit 0.040 s")), "{r:?}");
    }

    /// Thousands of faces (a modeled thread's) are classified at once, not face by face against
    /// every triangle: filleting beside a thread froze the app for minutes.
    ///
    /// A ratio and not a limit: a test must not depend on wall-clock timing (CONTRIBUTING.md), the
    /// machine is busy. 900 faces are classified, then 3 600, four times as many: linear is 4x, the
    /// face-against-triangle cost 16x or more.
    #[test]
    fn many_faces_are_classified_quickly() {
        /// The room the noise of a run on a busy machine gets, on top of the ratio: the 900 faces
        /// take a fraction of a second, so this is the noise of one run and not a change of scale.
        const SLACK: std::time::Duration = std::time::Duration::from_millis(20);
        if let Err(missed) = scales_linearly(|| (classify_sheet(30), classify_sheet(60)), SLACK) {
            panic!("the faces do not scale linearly: {missed}");
        }
    }

    /// A fillet preview stands in for the body: the round face is new, the rest is kept, and the
    /// old body is the ghost. The session is unchanged.
    #[test]
    fn fillet_preview_scene() {
        let s = plate();
        let (rev, undo) = (s.revision, s.undo.len());
        let b = compute(&s, &[("solid.fillet".into(), json!({"edges": [[0, 0, 10]], "radius": 3}))], colors()).unwrap();
        assert_eq!(b.replaced, vec!["Body1".to_string()]);
        let added = count(&b.scene, &b.scene.tris, crate::gpu::TRI_SIZE, colors().added);
        let kept = count(&b.scene, &b.scene.tris, crate::gpu::TRI_SIZE, colors().body);
        assert!(added > 0 && kept > 0, "added {added} kept {kept}");
        assert!(!b.scene.ghost.is_empty());
        assert_eq!(s.revision, rev);
        assert_eq!(s.undo.len(), undo);
    }

    /// Where the preview draws the ghost's vertices.
    fn ghost_vertices(scene: &GpuScene) -> Vec<Vec3> {
        scene
            .ghost
            .chunks(crate::gpu::TRI_SIZE)
            .filter_map(|v| {
                let f = |k: usize| v.get(k..k + 4).and_then(|b| b.try_into().ok()).map(f32::from_le_bytes).map(f64::from);
                Some(Vec3::new(f(0)?, f(4)?, f(8)?))
            })
            .collect()
    }

    /// A tool a join takes in leaves no ghost where the joined body keeps its wall: drawn whole,
    /// that wall fought the joined one in pink stripes. Inside the other body, where its surface
    /// is gone, it shows.
    #[test]
    fn a_joined_tool_shows_no_ghost_on_the_wall_the_join_keeps() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20, "body_name": "Cube"})).unwrap();
        s.execute("solid.cylinder", &json!({"base": [10, 10, -5], "diameter": 6, "height": 30, "body_name": "Pin"})).unwrap();
        let join = json!({"target": "Cube", "tools": ["Pin"], "operation": "join", "keep_tools": false});
        let p = s.preview(&[("solid.combine".into(), join)]).unwrap();
        assert!(p.after.body("Pin").is_none());
        let radial = |q: &Vec3| (q.x - 10.0).hypot(q.y - 10.0);
        let ghost = ghost_vertices(&build(&p.before, &p.after, colors()).scene);
        let pin_wall = |q: &&Vec3| radial(q) > 2.5 && radial(q) < 3.5;
        let outside = ghost.iter().filter(pin_wall).filter(|q| q.z < -0.5 || q.z > 20.5).count();
        assert_eq!(outside, 0, "ghost on the pin's wall the join keeps");
        assert!(ghost.iter().filter(pin_wall).any(|q| q.z > 1.0 && q.z < 19.0), "the pin's wall inside the cube is gone");
    }

    /// A body joined flush to another extends its faces: the old part of a face the join made
    /// bigger is kept surface, not the ghost, though the face as a whole is new.
    #[test]
    fn a_flush_join_shows_no_ghost_on_the_faces_it_extends() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20, "body_name": "Cube"})).unwrap();
        s.execute("solid.box", &json!({"corner": [20, 0, 0], "length": 40, "width": 20, "height": 20, "body_name": "Long"})).unwrap();
        let join = json!({"target": "Cube", "tools": ["Long"], "operation": "join", "keep_tools": false});
        let p = s.preview(&[("solid.combine".into(), join)]).unwrap();
        let ghost = ghost_vertices(&build(&p.before, &p.after, colors()).scene);
        // The ghost is drawn pushed out by the tolerance (under 0.1 here).
        let outer =
            |q: &&Vec3| [q.y, q.z].iter().any(|v| v.abs() < 0.2 || (v - 20.0).abs() < 0.2) && q.x > 0.5 && q.x < 59.5 && (q.x - 20.0).abs() > 0.2;
        assert_eq!(ghost.iter().filter(outer).count(), 0, "ghost on the joined bodies' outer faces");
        assert!(ghost.iter().any(|q| (q.x - 20.0).abs() < 0.2 && q.y > 1.0 && q.y < 19.0), "the wall between them is gone");
    }

    /// Points over a triangle, corners included.
    fn spread([a, b, c]: [Vec3; 3]) -> impl Iterator<Item = Vec3> {
        (0..=4).flat_map(move |i| (0..=4 - i).map(move |j| a + (b - a) * (i as f64 / 4.0) + (c - a) * (j as f64 / 4.0)))
    }

    /// The ghost of a fillet is the corner it removes, filled right to where the round blends
    /// into the wall and the top, and nothing of the wall and top the fillet keeps. The old and
    /// new meshes facet the wall differently: the whole old body, drawn behind the new one,
    /// showed through it as a red sawtooth under the round.
    #[test]
    fn a_fillet_ghost_is_the_removed_corner() {
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"base": [0, 0, 0], "radius": 12, "height": 14})).unwrap();
        let p = s.preview(&[("solid.fillet".into(), json!({"edges": [[12, 0, 14]], "radius": 2}))]).unwrap();
        let radial = |q: &Vec3| q.x.hypot(q.y);
        let kept = |q: &&Vec3| q.z < 11.9 || radial(q) < 9.9;
        // The old surface in the ghost: the corner, from the round's start on the wall (z 12)
        // and on the top (radius 10) to the edge, and none of the wall and top the fillet keeps.
        // (The ghost is drawn pushed out by the tolerance, under 0.03 here.)
        let old = p.before.bodies.first().unwrap();
        let (shared, tol) = sharing(old, &p.after);
        let new: Vec<(&Mesh, &[bool])> = shared.iter().map(|(m, named)| (m.as_ref(), named.as_slice())).collect();
        let tris: Vec<[Vec3; 3]> = ghost(&old.mesh(), &new, tol).iter().map(|t| t.map(|(p, _)| p)).collect();
        let points: Vec<Vec3> = tris.iter().copied().flat_map(spread).collect();
        assert_eq!(points.iter().filter(kept).count(), 0, "ghost on the surface the fillet keeps");
        let wall = points.iter().filter(|q| radial(q) > 11.9).map(|q| q.z).fold(f64::INFINITY, f64::min);
        let top = points.iter().filter(|q| q.z > 13.9).map(radial).fold(f64::INFINITY, f64::min);
        assert!(wall < 12.01, "the ghost on the wall stops at z {wall}");
        assert!(top < 10.01, "the ghost on the top stops at radius {top}");
        // Nowhere inside the filleted body: the round's facets, inside it by up to the
        // tolerance, stood in front of the old wall's where they meet and showed through the
        // ghost in pale spikes.
        let inside = |q: &&Vec3| {
            let (r, z) = (radial(q), q.z);
            let (r, z) = (r + 1e-6, z + 1e-6);
            r < 12.0 && z < 14.0 && (z <= 12.0 || r <= 10.0 || (r - 10.0).hypot(z - 12.0) < 2.0)
        };
        assert_eq!(points.iter().filter(inside).count(), 0, "ghost inside the new body");
        // Where the ghost ends on the wall, all the way round: one height, not a tooth per facet.
        let mut lowest = [f64::INFINITY; 72];
        for q in points.iter().filter(|q| radial(q) > 11.9) {
            let k = ((q.y.atan2(q.x) + std::f64::consts::PI) / std::f64::consts::TAU * 72.0) as usize;
            if let Some(z) = lowest.get_mut(k.min(71)) {
                *z = z.min(q.z);
            }
        }
        let (lo, hi) = lowest.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), z| (l.min(*z), h.max(*z)));
        assert!(hi - lo < 0.01, "the ghost ends between z {lo} and {hi} on the wall");
        // The wall's ghost and the top's, pushed apart along their own normals, are joined over
        // the old edge all the way round: the copies of an edge vertex in the two faces differ
        // in their last bits, and a crack opened between them.
        let mut joined = [false; 72];
        let fine = |[a, b, c]: [Vec3; 3]| {
            (0..=24).flat_map(move |i| (0..=24 - i).map(move |j| a + (b - a) * (i as f64 / 24.0) + (c - a) * (j as f64 / 24.0)))
        };
        let over = |q: &Vec3| radial(q) > 12.0 + tol / 4.0 && q.z > 14.0 + tol / 4.0;
        for q in tris.iter().filter(|t| t.iter().all(|q| radial(q) > 11.9 && q.z > 13.9)).flat_map(|t| fine(*t)).filter(over) {
            let k = ((q.y.atan2(q.x) + std::f64::consts::PI) / std::f64::consts::TAU * 72.0) as usize;
            if let Some(j) = joined.get_mut(k.min(71)) {
                *j = true;
            }
        }
        assert!(joined.iter().all(|j| *j), "the ghost has a crack along the old edge: {joined:?}");
        // The round itself is new surface, drawn in the cut colour right to its edges on the
        // wall and the top: with the ghost it fills the removed corner.
        let b = build(&p.before, &p.after, colors());
        let cut = vertices(&b.scene, &b.scene.xray, 28, colors().removed);
        let wall = cut.iter().filter(|q| radial(q) > 11.9).map(|q| q.z).fold(f64::INFINITY, f64::min);
        let top = cut.iter().filter(|q| q.z > 13.9).map(radial).fold(f64::INFINITY, f64::min);
        assert!(wall < 12.05 && top < 10.05, "the round's cut colour stops at z {wall} on the wall, radius {top} on the top");
        assert_eq!(vertices(&b.scene, &b.scene.xray, 28, colors().removed).iter().filter(kept).count(), 0);
    }

    /// The vertices of a scene buffer drawn in `col`.
    fn vertices(scene: &GpuScene, bytes: &[u8], stride: usize, col: [u8; 4]) -> Vec<Vec3> {
        bytes
            .chunks(stride)
            .filter(|v| {
                let Some(i) = v.get(24..28).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes) else { return false };
                scene.palette.get(crate::gpu::PaletteIndex(i)) == Some(col)
            })
            .filter_map(|v| {
                let f = |k: usize| v.get(k..k + 4).and_then(|b| b.try_into().ok()).map(f32::from_le_bytes).map(f64::from);
                Some(Vec3::new(f(0)?, f(4)?, f(8)?))
            })
            .collect()
    }

    /// A 2 mm round on the boss of the sample plate is new surface, tinted and in the cut
    /// colour. Its triangles crowd where it blends into the wall and the top, close to the old
    /// surface, and an even vote of nine of them called it old: drawn untinted, it showed as a
    /// pale strip under the removed corner.
    #[test]
    fn a_small_round_on_a_large_body_is_new_surface() {
        let mut s = Session::default();
        for c in solidcraft_engine::sample::script()["commands"].as_array().unwrap() {
            s.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
        }
        let p = s.preview(&[("solid.fillet".into(), json!({"edges": [[40, 13, 22]], "radius": 2}))]).unwrap();
        let b = build(&p.before, &p.after, colors());
        let r = |q: &Vec3| (q.x - 40.0).hypot(q.y - 25.0);
        let round = |q: &Vec3| q.z > 20.05 && q.z < 21.95 && r(q) > 10.05 && r(q) < 11.95;
        let tinted = vertices(&b.scene, &b.scene.tris, crate::gpu::TRI_SIZE, colors().added);
        assert!(tinted.iter().filter(|q| round(q)).count() > 100, "the round is not tinted as new surface");
        assert!(vertices(&b.scene, &b.scene.tris, crate::gpu::TRI_SIZE, colors().body).iter().all(|q| !round(q)));
        assert!(vertices(&b.scene, &b.scene.xray, 28, colors().removed).iter().any(round), "the round is not in the cut colour");
    }

    /// Rounds and chamfers of every common size on the sample plate's boss (radius 12, top at
    /// z 22): the new face is tinted, and the ghost is the removed corner, ending where the new
    /// face starts in one line all the way round, nowhere inside the new body. A 1 mm round was
    /// taken for the old surface, untinted, with the ghost's edge a sawtooth.
    #[test]
    fn rounds_and_chamfers_of_any_size_show_the_removed_corner_cleanly() {
        let mut s = Session::default();
        for c in solidcraft_engine::sample::script()["commands"].as_array().unwrap() {
            s.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
        }
        let r = |q: &Vec3| (q.x - 40.0).hypot(q.y - 25.0);
        for size in [0.25, 0.5, 1.0, 2.0, 3.0] {
            for round in [true, false] {
                let (cmd, params) = if round {
                    ("solid.fillet", json!({"edges": [[40, 13, 22]], "radius": size}))
                } else {
                    ("solid.chamfer", json!({"edges": [[40, 13, 22]], "distance": size}))
                };
                let case = format!("{cmd} {size}");
                let p = s.preview(&[(cmd.into(), params)]).unwrap();
                let b = build(&p.before, &p.after, colors());
                let face = |q: &Vec3| q.z > 22.0 - size + 0.01 && q.z < 21.99 && r(q) > 12.0 - size + 0.01 && r(q) < 11.99;
                let centres = |col| {
                    vertices(&b.scene, &b.scene.tris, crate::gpu::TRI_SIZE, col)
                        .chunks(3)
                        .map(|t| t.iter().fold(Vec3::ZERO, |a, p| a + *p) / 3.0)
                        .collect::<Vec<_>>()
                };
                assert!(centres(colors().added).iter().any(face), "{case}: the new face is not tinted");
                assert!(!centres(colors().body).iter().any(face), "{case}: the new face is drawn as old");
                let old = p.before.bodies.first().unwrap();
                let (shared, tol) = sharing(old, &p.after);
                let new: Vec<(&Mesh, &[bool])> = shared.iter().map(|(m, named)| (m.as_ref(), named.as_slice())).collect();
                let points: Vec<Vec3> = ghost(&old.mesh(), &new, tol).into_iter().map(|t| t.map(|(p, _)| p)).flat_map(spread).collect();
                // On the wall, the lowest point of the ghost all the way round.
                let mut lowest = [f64::INFINITY; 72];
                for q in points.iter().filter(|q| r(q) > 11.99 && q.z > 21.0 - size) {
                    let k = ((q.y - 25.0).atan2(q.x - 40.0) + std::f64::consts::PI) / std::f64::consts::TAU * 72.0;
                    if let Some(z) = lowest.get_mut((k as usize).min(71)) {
                        *z = z.min(q.z);
                    }
                }
                let (lo, hi) = lowest.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), z| (l.min(*z), h.max(*z)));
                assert!(
                    lo > 22.0 - size - 0.01 && hi < 22.0 - size + 0.02,
                    "{case}: the ghost ends between z {lo} and {hi} on the wall (tolerance {tol})"
                );
                // On the top, the ghost reaches the new face's edge (a polygon at radius
                // 12 - size) at every angle, its corners too: a round of kept top was left
                // round each, a pale notch in the band.
                let mut inner = [f64::INFINITY; 360];
                for q in points.iter().filter(|q| q.z > 22.0 && r(q) < 12.0) {
                    let k = ((q.y - 25.0).atan2(q.x - 40.0) + std::f64::consts::PI) / std::f64::consts::TAU * 360.0;
                    if let Some(m) = inner.get_mut((k as usize).min(359)) {
                        *m = m.min(r(q));
                    }
                }
                let widest = inner.iter().copied().filter(|m| m.is_finite()).fold(0.0, f64::max);
                assert!(widest < 12.0 - size + 0.03, "{case}: the ghost on the top stops at radius {widest} in places");
                let inside = |q: &&Vec3| {
                    let (out, down) = (12.0 - r(q) - 1e-6, 22.0 - q.z - 1e-6);
                    let corner = if round { (size - out).max(0.0).hypot((size - down).max(0.0)) < size } else { out + down > size };
                    out > 0.0 && down > 0.0 && (out >= size || down >= size || corner)
                };
                assert_eq!(points.iter().filter(inside).count(), 0, "{case}: ghost inside the new body");
            }
        }
    }

    /// A hole the change made inside one large triangle of the old mesh is found, and its old
    /// surface is the ghost.
    #[test]
    fn a_hole_inside_one_old_triangle_is_in_the_ghost() {
        let mut old = sheet(1, 0.0);
        for p in &mut old.positions {
            *p = Vec3::new(p.x * 12.0, p.y * 12.0, 0.0);
        }
        let mut new = sheet(12, 0.0);
        let hole = |f: u32| (5..7).contains(&(f / 12)) && (5..7).contains(&(f % 12));
        let (tris, faces): (Vec<[u32; 3]>, Vec<u32>) =
            new.triangles.iter().zip(&new.tri_face).filter(|(_, f)| !hole(**f)).map(|(t, f)| (*t, *f)).unzip();
        (new.triangles, new.tri_face) = (tris, faces);
        let g = ghost(&old, &[(&new, &[])], 0.01);
        let area: f64 = g.iter().map(|[a, b, c]| (b.0 - a.0).cross(c.0 - a.0).len() / 2.0).sum();
        assert!(area > 3.0 && area < 4.5, "ghost area {area}, the hole is 4");
        assert!(g.iter().flatten().all(|(p, _)| (4.4..7.6).contains(&p.x) && (4.4..7.6).contains(&p.y)));
    }

    /// Removed material thinner than two meshes of one surface may differ (a face moved by a
    /// hair) shows no ghost, and the kept surface none either; all of a moved face shows. Only
    /// the triangles that cross the line where the ghost ends are split, so it is quick.
    #[test]
    fn a_hair_thin_change_leaves_no_ghost() {
        let started = std::time::Instant::now();
        let (old, near, far) = (sheet(60, 0.0), sheet(60, 0.02), sheet(60, 1.0));
        assert!(ghost(&old, &[(&near, &[])], 0.02).is_empty());
        assert!(ghost(&old, &[(&old, &[])], 0.02).is_empty());
        assert_eq!(ghost(&old, &[(&far, &[])], 0.02).len(), old.triangles.len());
        assert!(started.elapsed().as_secs() < 5, "took {:?}", started.elapsed());
    }

    /// A cut shows its tool see-through and replaces nothing.
    #[test]
    fn cut_preview_scene() {
        let mut s = plate();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.circle.center", &json!({"center": [20, 15], "radius": 5})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        let b = compute(&s, &[("solid.extrude".into(), json!({"distance": 20, "operation": "cut"}))], colors()).unwrap();
        assert!(b.replaced.is_empty());
        assert!(b.scene.tris.is_empty() && !b.scene.xray.is_empty());
        assert!(b.error.is_none());
    }

    /// A new body is all new surface and replaces nothing; a bad value is an error.
    #[test]
    fn extrude_preview_scene() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.circle.center", &json!({"center": [0, 0], "radius": 5})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        let b = compute(&s, &[("solid.extrude".into(), json!({"distance": "15 mm"}))], colors()).unwrap();
        assert!(b.replaced.is_empty());
        assert_eq!(count(&b.scene, &b.scene.tris, crate::gpu::TRI_SIZE, colors().body), 0);
        // #1805: a new body has its own colour, not a join's.
        assert!(count(&b.scene, &b.scene.tris, crate::gpu::TRI_SIZE, colors().new_body) > 0);
        assert_eq!(count(&b.scene, &b.scene.tris, crate::gpu::TRI_SIZE, colors().added), 0);
        assert!(b.scene.ghost.is_empty());
        assert!(compute(&s, &[("solid.extrude".into(), json!({"distance": "1 +"}))], colors()).is_err());
        assert!(s.model.state().bodies.is_empty());
    }

    fn session_of(cmds: &[(&str, Value)]) -> Session {
        let mut s = Session::default();
        for (id, p) in cmds {
            s.execute(id, p).unwrap();
        }
        s
    }

    /// A block and a two-letter text to cut into it.
    fn text_on_block() -> (Session, Vec<(String, Value)>) {
        let s = session_of(&[
            ("sketch.create", json!({"plane": "XY"})),
            ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [60, 20]})),
            ("sketch.finish", json!({})),
            ("solid.extrude", json!({"distance": 10})),
            ("sketch.create", json!({"plane": "XY"})),
            ("sketch.text", json!({"text": "AB", "at": [5, 5], "height": 10})),
            ("sketch.finish", json!({})),
        ]);
        (s, vec![("solid.extrude".to_string(), json!({"distance": 2, "operation": "cut"}))])
    }

    /// A tool feature's live preview builds its tool without the boolean: a cut into nothing
    /// shows its tool with no error (OK will report it), while the whole preview has the error.
    #[test]
    fn a_tool_preview_builds_the_tool_without_the_boolean() {
        let s = session_of(&[
            ("sketch.create", json!({"plane": "XY"})),
            ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})),
            ("sketch.finish", json!({})),
        ]);
        let cmds = vec![("solid.extrude".to_string(), json!({"distance": 2, "operation": "cut"}))];
        let tool = compute_tool(&s, &cmds, colors()).unwrap();
        assert!(tool.error.is_none() && !tool.scene.xray.is_empty(), "{:?}", tool.error);
        let whole = compute(&s, &cmds, colors()).unwrap();
        assert!(whole.error.as_deref().is_some_and(|e| e.contains("no body to cut")), "{:?}", whole.error);
        assert_eq!(whole.scene.xray.len(), tool.scene.xray.len());
    }

    /// #3085: a cut sketched in the root, where every body lives in a component, cuts every body
    /// it meets (#1234). The preview said "there is no body to cut or intersect" all the same,
    /// while the cut itself went through.
    #[test]
    fn a_cut_from_an_empty_root_is_not_refused_by_its_preview() {
        let mut s = session_of(&[
            ("component.create", json!({"name": "Part"})),
            ("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"})),
            ("component.activate", json!({"component": "root"})),
            ("sketch.create", json!({"plane": "XY"})),
            ("sketch.rectangle.two_point", json!({"p0": [3, 3], "p1": [7, 7]})),
            ("sketch.finish", json!({})),
        ]);
        let params = json!({"distance": 20, "operation": "cut"});
        let cmds = vec![("solid.extrude".to_string(), params.clone())];
        assert_eq!(extrusion_target_error(&s, &cmds, &s.model.state()), None);
        s.execute("solid.extrude", &params).unwrap();
        let volume = s.model.state().body("Block").map(|b| solidcraft_engine::kernel::measure(&b.body).unwrap().volume).unwrap_or(0.0);
        assert!((volume - (1000.0 - 160.0)).abs() < 1e-6, "{volume}");
    }

    /// While an arrow is dragged the inputs change faster than a tool is built: a tool built
    /// for inputs since changed is shown anyway (then the latest computed), an error only for
    /// the inputs it belongs to.
    #[test]
    fn a_tool_for_inputs_since_changed_still_shows() {
        let (s, cmds) = text_on_block();
        let tool = compute_tool(&s, &cmds, colors()).unwrap();
        let mut pv = PreviewState::default();
        let (tx, rx) = std::sync::mpsc::channel();
        pv.running = Some((1, rx));
        pv.running_tool = true;
        tx.send(Ok(tool)).unwrap();
        pv.poll(2, None);
        assert!(pv.running.is_none() && pv.active && pv.shown_key == Some(1), "shown, and key 2 still to compute");
        let (tx, rx) = std::sync::mpsc::channel();
        pv.running = Some((3, rx));
        pv.running_tool = true;
        tx.send(Err("too far".into())).unwrap();
        pv.poll(4, None);
        assert!(pv.error.is_none() && pv.shown_key == Some(1), "an old error is dropped");
    }

    /// A design leaving the window (another tab) takes its preview off the viewport: an empty
    /// scene in the slot the viewport reads, under a new key. A fresh slot left the fillet
    /// preview of the old design drawn over the new tab's empty design.
    #[test]
    fn reset_hands_the_viewport_an_empty_scene() {
        let mut pv = PreviewState::default();
        let mut sc = GpuScene::default();
        let white = sc.color([255; 4]);
        sc.xray_tri([0.0; 3], [0.0, 0.0, 1.0], white);
        pv.set_scene(sc);
        pv.active = true;
        pv.replaced = vec!["Body1".into()];
        let slot = pv.slot.clone();
        let key = pv.key;
        // The viewport took the scene.
        assert!(slot.lock().unwrap().take().is_some_and(|s| !s.xray.is_empty()));
        pv.reset();
        assert!(std::sync::Arc::ptr_eq(&slot, &pv.slot), "the slot the viewport reads");
        assert_ne!(pv.key, key, "a new key: the viewport takes the slot again");
        assert!(slot.lock().unwrap().as_ref().is_some_and(|s| s.xray.is_empty() && s.blocks.is_empty()));
        assert!(!pv.active && pv.replaced.is_empty());
    }
}

#[cfg(test)]
#[path = "preview_drag_tests.rs"]
mod drag_tests;
