//! Programmatic control of the running app (agents, tests).
//!
//! Methods (JSON lines over the host's transport, `{"id", "method", "params"}`):
//! - `engine.execute {command, params}`: run any command with JSON parameters (no dialogs)
//! - `engine.script {commands: [...]}`: run a command script
//! - `engine.batch {commands: [...], rollback?}`: the steps in one request, each as
//!   `engine.execute` runs it; stops at the first failure and goes back to the checkpoint
//!   (per-step results, `failed`, `undone`, `rolled_back`). Refused (`engine.batch unavailable: …`)
//!   when a step loads a file; with the agent cursor shown, the pointer acts out a short summary
//!   of it first
//! - `engine.commands`: every command with tab, panel, params and enablement
//! - `document.inspect {measure?}`: the design (parameters, timeline, bodies, sketches)
//! - `ui.inspect`: UI state, viewport rect, camera, tool and dialog
//! - `ui.drag {x0, y0, x1, y1, button?, shift?, ctrl?, steps?, hold?}` (box selection, navigation;
//!   `hold` keeps the button down that many frames before moving);
//!   `ui.selection` (selection, dialog inputs, hover, drawn sketch dimensions);
//!   `ui.sketchToScreen {points: [[x,y]…]}` (screen points of active-sketch coordinates); `ui.editFeature {feature}` (edit dialog)
//! - `ui.set {...UiState fields}`; `ui.view {view: front|back|top|bottom|left|right|iso|home|fit, animate?: bool}` (snaps unless animate)
//! - `ui.start {command}`: like clicking the toolbar button (starts tools/dialogs)
//! - `ui.click {x, y, button?, shift?}`, `ui.move {x, y}`: real
//!   pointer input in screen points; `ui.scroll {x, y, delta}`: mouse-wheel input in
//!   points (zooms); `ui.key {key, cmd?, shift?}`, `ui.text {text}`
//! - `ui.screenshot {path?}`: PNG of the window; `ui.render {path, width?, height?, grid?, axes?}`:
//!   headless CPU render of the model with the current camera, in the window's theme, with the
//!   section the viewport shows (no window needed); the ground grid and the origin's axes as the
//!   window shows them, unless `grid` or `axes` says
//! - `ui.icon_audit {reset?, stop?}`: collect/drain requested icon names, resolved masters and placeholder flags;
//! - `ui.menu {x?, y?, target?, close?}`: open a context menu at a point (the viewport's marking
//!   menu, or a browser menu with `target: {type: body|sketch|component, …}`); without a point,
//!   the open menu and its items. `ui.menuPick {item}` runs an item by id or label;
//!   `ui.rename {text?, commit?}` finishes the rename box
//! - `ui.window {action: minimize|maximize|restore|toggle|close}`: what the title bar's buttons do
//! - `ui.confirm {accept?}`: the Delete confirmation (without `accept`: what it lists)
//! - `ui.shortcut {command, key?, slot?, replace?}`: a command's keys, or bind one ("" clears)
//! - `ui.help {item?: about|updates|shortcuts|report}`: the Help menu; returns the version and diagnostics
//! - `ui.prefs {...}`: the preferences (default_units, nav, zoom_reverse, orbit_around, msaa,
//!   length_decimals, angle_decimals); returns them all
//! - `ui.capture {action?: capture|revert|cancel}`: the Capture Position question
//! - `ui.agent_cursor {show?, speed?: 1–5 (times) | "instant", follow_camera?}`: the agent cursor
//!   (#35); while it is shown, `engine.execute` first glides to the command's button and picks
//!   and replies once the command has run (`"animate": false` skips that one call)
//! - `ui.resize {width, height}`, `app.quit`

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};
use solidcraft_engine::io::vfs;
use solidcraft_engine::render::{Camera, StandardView};
use solidcraft_engine::schema::ParamCheck;
use solidcraft_engine::{batch_params, timing};

use crate::SolveApp;

pub type ControlResponse = Value;

pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<ControlResponse>,
    /// Set once the window starts on it; until then it waits its turn (`control.running`, #3654).
    pub started: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Chat requests already admitted by the gate may outlive a tab switch. This turn's
    /// validity follows them through queues, computation and cursor animation.
    pub(crate) chat_turn: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (Self, std::sync::mpsc::Receiver<ControlResponse>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Self { method: method.into(), params, reply: tx, started: Default::default(), chat_turn: None }, rx)
    }

    pub(crate) fn refuse_cancelled_chat(&self) -> bool {
        if chat_turn_cancelled(self.chat_turn.as_deref()) {
            let _ = self.reply.send(cancelled_chat_response());
            true
        } else {
            false
        }
    }
}

pub(crate) fn chat_turn_cancelled(turn: Option<&std::sync::atomic::AtomicBool>) -> bool {
    turn.is_some_and(|active| !active.load(std::sync::atomic::Ordering::Relaxed))
}

pub(crate) fn cancelled_chat_response() -> Value {
    json!({"ok": false, "error": "not run: the agent chat turn ended or its document tab was left"})
}

/// The active design's id and revision for control connections that sent `control.subscribe`
/// (#6985): the window publishes them each frame while one is subscribed, and each such
/// connection waits for them to move, then sends a `design.changed` line.
#[derive(Default)]
pub struct DesignWatch {
    subscribers: AtomicUsize,
    now: Mutex<Option<(String, u64)>>,
    moved: Condvar,
}

/// The app's [`DesignWatch`].
pub static DESIGN_WATCH: DesignWatch = DesignWatch::new();

impl DesignWatch {
    pub const fn new() -> Self {
        DesignWatch { subscribers: AtomicUsize::new(0), now: Mutex::new(None), moved: Condvar::new() }
    }

    fn now(&self) -> MutexGuard<'_, Option<(String, u64)>> {
        self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The design as it is now; waiters wake only when it moved. Only the latest is kept, so
    /// changes made faster than a connection sends them go out as one.
    pub fn publish(&self, document_id: &str, revision: u64) {
        let mut now = self.now();
        if now.as_ref().is_some_and(|(id, r)| id == document_id && *r == revision) {
            return;
        }
        *now = Some((document_id.to_string(), revision));
        self.moved.notify_all();
    }

    /// Whether a connection is subscribed: the window publishes only then.
    pub fn subscribed(&self) -> bool {
        self.subscribers.load(Ordering::Relaxed) > 0
    }

    /// Count a subscribed connection until the returned guard is dropped.
    pub fn join(&'static self) -> Subscriber {
        self.subscribers.fetch_add(1, Ordering::Relaxed);
        Subscriber(self)
    }

    /// Wait up to `wait` for the design to differ from `seen`, or for `stop`; the design then
    /// (`None` before anything was published).
    pub fn wait(&self, seen: &(String, u64), stop: &AtomicBool, wait: Duration) -> Option<(String, u64)> {
        let unmoved = |now: &mut Option<(String, u64)>| now.as_ref() == Some(seen) && !stop.load(Ordering::Relaxed);
        let (now, _) = self.moved.wait_timeout_while(self.now(), wait, unmoved).unwrap_or_else(PoisonError::into_inner);
        now.clone()
    }
}

/// A subscribed connection, counted by its [`DesignWatch`] while this lives.
pub struct Subscriber(&'static DesignWatch);

impl Drop for Subscriber {
    fn drop(&mut self) {
        self.0.subscribers.fetch_sub(1, Ordering::Relaxed);
        // Under the lock, so a connection about to wait sees its `stop` set before it does.
        let _now = self.0.now();
        self.0.moved.notify_all();
    }
}

/// Publish the active design for subscribed connections ([`DesignWatch`]). Called at the end of
/// each frame's logic (which runs while the window is hidden too) and of its drawing, so a
/// change made in either goes out in that frame.
pub(crate) fn publish_design(app: &mut SolveApp, watch: &DesignWatch) {
    if watch.subscribed() {
        let revision = app.session.design_revision();
        watch.publish(&app.session.document_id, revision);
    }
}

pub enum Outcome {
    Done(Value),
    Screenshot { path: Option<String> },
}

fn ok(v: Value) -> Outcome {
    Outcome::Done(json!({"ok": true, "result": v}))
}
fn err(e: impl std::fmt::Display) -> Outcome {
    Outcome::Done(json!({"ok": false, "error": e.to_string()}))
}
fn wrap(r: Result<Value, String>) -> Outcome {
    match r {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

/// The design's own Sketches folder (`Visibility::hidden_folders`), which `showSketches` sets.
const SKETCHES_FOLDER: &str = "c0/sketches";

/// `UiState` as `ui.inspect` and `ui.set` report it, with the browser's view of the design
/// (kept in the session) under the names the UI state used to give it.
fn ui_state(app: &SolveApp) -> Value {
    let mut u = serde_json::to_value(&app.ui).unwrap_or_default();
    let v = &app.session.visibility;
    if let Some(o) = u.as_object_mut() {
        o.insert("showOrigin".into(), json!(app.origin_shown()));
        o.insert("showSketches".into(), json!(!app.session.folder_hidden(SKETCHES_FOLDER)));
        o.insert("hiddenProfiles".into(), json!(v.hidden_profiles));
        o.insert("shownDims".into(), json!(v.shown_dimensions));
        o.insert("lockedBodies".into(), json!(v.locked_bodies));
    }
    u
}

fn key_from(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).or(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(egui::Key::Enter),
        "esc" | "escape" => Some(egui::Key::Escape),
        "delete" => Some(egui::Key::Delete),
        "space" => Some(egui::Key::Space),
        "tab" => Some(egui::Key::Tab),
        _ => None,
    })
}

/// Run one command as `engine.execute` does: through `app.run`, then the first-body fit and the
/// camera the command asked for.
fn execute(app: &mut SolveApp, id: &str, params: Value) -> Result<Value, String> {
    // User bindings live in the UI. A bridged agent needs the same key the menus show,
    // including null for a cleared binding; the standalone engine keeps its defaults.
    if id == "engine.commands" {
        let mut commands = app.session.execute(id, &params).map_err(|e| e.to_string())?;
        if let Some(rows) = commands.as_array_mut() {
            for row in rows.iter_mut().filter_map(Value::as_object_mut) {
                let key = row.get("id").and_then(Value::as_str).and_then(|id| crate::keymap::effective(app, id));
                row.insert("shortcut".into(), json!(key));
            }
        }
        return Ok(commands);
    }
    // `view.pick` is answered by the viewport, not the engine: pixels and rays need the
    // window's camera and pick index, and the command must not touch the document, the
    // selection or the undo stack. Intercepted here so `engine.execute` and `engine.batch`
    // (and through them MCP `execute` and `batch`) reach it like any other command.
    if id == "view.pick" {
        return crate::view_pick::pick_command(app, &params);
    }
    let r = app.run(id, params);
    // Update Links in a part edited in place reads the assembly around it too, as its button does.
    if r.is_ok() && is_command(Some(id), "component.update_links") && app.edit_in_place.active.is_some() {
        crate::edit_in_place::refresh(app);
    }
    if r.is_ok()
        && !matches!(
            id,
            "view.fit"
                | "view.home"
                | "view.tool"
                | "view.spin"
                | "view.visual_style"
                | "view.grid"
                | "view.ground_shadow"
                | "view.navigation_bar"
                | "view.next_tab"
                | "view.previous_tab"
                | "view.layout"
                | "view.navigate"
                | "view.ambient_occlusion"
                | "view.perspective"
                | "view.angle_readout"
                | "view.graphics_preset"
                | "view.history_layout"
                | "timeline.play"
                | "browser.expand_all"
                | "browser.collapse_all"
        )
        && !id.starts_with("settings.")
        && app.session.active_sketch.is_none()
        && app.session.world_state().bodies.len() == 1
        && app.session.undo.len() <= 2
    {
        app.fit_view();
    }
    // After the first-body fit, so a camera the command asked for (`view.apply`) wins.
    app.follow_view_request(false);
    r
}

/// A batch step's command id.
fn command(step: &Value) -> Option<&str> {
    step.get("command").or_else(|| step.get("id")).and_then(Value::as_str)
}

/// Most steps in one `engine.batch` (as in the MCP `batch` tool).
const MAX_BATCH: usize = 10_000;

/// Commands that load a file: through `engine.execute` they go to the app's file loader
/// (`file_load::control`: off the window thread, with progress and Cancel), which a batch would
/// bypass.
const LOADS_A_FILE: [&str; 4] = ["doc.open", "file.insert_step", "file.insert_mesh", "pointcloud.insert"];

/// Edit in Place, Finish Edit and the next or previous tab go to another design's tab: a batch's
/// rollback, or a script's later steps, would land in the wrong design.
const SWITCHES_DESIGN: [&str; 4] = ["component.edit_in_place", "component.finish_edit", "view.next_tab", "view.previous_tab"];

/// `command` names the command `id`, in any of the spellings the engine takes.
fn is_command(command: Option<&str>, id: &str) -> bool {
    command.and_then(solidcraft_engine::find_command).is_some_and(|spec| spec.id == id)
}

/// The command of `step` when it goes to another design's tab.
fn switches_design(step: &Value) -> Option<&'static str> {
    SWITCHES_DESIGN.into_iter().find(|id| is_command(command(step), id))
}

/// Why `engine.batch` refuses these steps before running any: a step loads a file or switches
/// to another design (send such steps one by one).
pub(crate) fn batch_refusal(steps: &[Value]) -> Option<String> {
    if let Some((i, c)) = steps.iter().enumerate().find_map(|(i, step)| switches_design(step).map(|c| (i, c))) {
        return Some(format!("engine.batch unavailable: step {i} ({c}) switches to another design"));
    }
    let (i, c) = steps.iter().enumerate().find_map(|(i, step)| command(step).filter(|c| LOADS_A_FILE.contains(c)).map(|c| (i, c)))?;
    Some(format!("engine.batch unavailable: step {i} ({c}) loads a file"))
}

/// A batch step's command id (the agent cursor points at its toolbar button).
pub(crate) fn batch_step_command(step: &Value) -> Option<&str> {
    command(step)
}

/// `engine.batch {commands, rollback?, timings?, profile?}`: the steps in one request, so in one
/// frame (#743). With `timings` the reply adds each step's time and `total_ms`, with `profile`
/// each step's kernel share too (`solidcraft_engine::timing`, #1833). Each
/// step runs as `engine.execute` runs it; the first failure stops the batch and, with
/// `rollback` (the default), the design goes back to its checkpoint. Step failures are part of
/// the reply; refusals (`engine.batch unavailable: …`) come before anything runs, so the client
/// can send the steps one by one instead.
fn batch(app: &mut SolveApp, p: &Value) -> Outcome {
    let Some(steps) = p.get("commands").and_then(Value::as_array).filter(|s| s.len() <= MAX_BATCH) else {
        return err(format!("engine.batch: commands must be an array of at most {MAX_BATCH} steps"));
    };
    // (With the agent cursor shown, the pointer has already acted out a short summary of the
    // batch: `agent_cursor::intercept`.)
    if let Some(why) = batch_refusal(steps) {
        return err(why);
    }
    // A step the batch would refuse at its turn refuses it now, before anything runs (#5853).
    if let Some(refusal) = solidcraft_engine::batch_preflight::check(steps, app.session.param_check, &["view.pick"]) {
        return ok(refusal.reply(steps.len()));
    }
    let rollback = p.get("rollback").and_then(Value::as_bool).unwrap_or(true);
    let profile = p.get("profile").and_then(Value::as_bool).unwrap_or(false);
    let timed = profile || p.get("timings").and_then(Value::as_bool).unwrap_or(false);
    let mut rows = Vec::new();
    let clock = timing::Clock::start();
    let checkpoint = if rollback { app.run("edit.checkpoint", json!({})).ok().and_then(|v| v["checkpoint"].as_u64()) } else { None };
    let mut results = Vec::with_capacity(steps.len());
    // With `coalesce_parameters`, each run of plain parameter edits rebuilds once (#5822); a run
    // whose combined edit is refused goes step by step, so a refused edit is named by its step.
    let coalesce = p.get("coalesce_parameters").and_then(Value::as_bool).unwrap_or(false);
    for run in batch_params::plan(steps, coalesce) {
        let singles = match run {
            batch_params::BatchRun::Step(i) => vec![i],
            batch_params::BatchRun::Params { steps: indices, changes } => {
                let (r, took) = timing::time(profile, || execute(app, "parameters.change", batch_params::combined_params(&changes)));
                match r {
                    Ok(v) => {
                        if timed {
                            rows.push(took.to_json(indices.first().copied().unwrap_or(0), "parameters.change"));
                        }
                        results.extend(batch_params::step_results(steps, &indices, &v));
                        continue;
                    }
                    Err(_) => indices,
                }
            }
        };
        for i in singles {
            let Some(step) = steps.get(i) else { continue };
            let (r, took) = timing::time(profile, || match command(step) {
                Some(id) => execute(app, id, step.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}))),
                None => Err("missing `command`".to_string()),
            });
            if timed {
                rows.push(took.to_json(i, command(step).unwrap_or_default()));
            }
            match r {
                Ok(v) => results.push(v),
                Err(e) => {
                    let restored = checkpoint.is_some_and(|id| app.run("edit.restore_checkpoint", json!({"checkpoint": id})).is_ok());
                    let failed = json!({"index": i, "command": command(step), "error": e});
                    // `rolled_back`: the design really is back at its checkpoint.
                    let reply = json!({"completed": i, "results": results, "failed": failed, "undone": if restored { i } else { 0 }, "rolled_back": restored});
                    return ok(with_timings(reply, timed, rows, clock.ms()));
                }
            }
        }
    }
    if let Some(id) = checkpoint {
        let _ = app.run("edit.restore_checkpoint", json!({"checkpoint": id, "forget": true}));
    }
    ok(with_timings(json!({"completed": results.len(), "results": results, "undone": 0}), timed, rows, clock.ms()))
}

/// A batch reply with `timings` and `total_ms` when they were asked for.
fn with_timings(mut reply: Value, timed: bool, rows: Vec<Value>, total_ms: f64) -> Value {
    if timed && let Some(o) = reply.as_object_mut() {
        o.insert("timings".into(), Value::Array(rows));
        o.insert("total_ms".into(), json!(timing::round_ms(total_ms)));
    }
    reply
}

/// A request from an MCP server carries the folders its agent may use (`allow_paths`); every
/// file the request reaches must lie inside them (docs/mcp.md). `None`: it names none.
pub fn request_scope(app: &SolveApp, params: &Value) -> Result<Option<vfs::PathScope>, String> {
    let Some(allowed) = params.get("allow_paths") else { return Ok(None) };
    let folders = |v: &Value| v.as_array().and_then(|a| a.iter().map(|r| r.as_str().map(std::path::PathBuf::from)).collect::<Option<Vec<_>>>());
    let Some(roots) = folders(allowed) else {
        return Err("`allow_paths` must be a list of folders".into());
    };
    // The folders the agent opened or saved a design in (#7181), judged as the open design's is.
    let designs = match params.get("design_folders") {
        None => Vec::new(),
        Some(v) => folders(v)
            .filter(|d| d.len() <= vfs::MAX_DESIGN_FOLDERS)
            .ok_or_else(|| format!("`design_folders` must be a list of at most {} folders", vfs::MAX_DESIGN_FOLDERS))?,
    };
    // Plus the app's own recovery folder and, when it is a safe one, the open design's folder.
    let roots = roots.into_iter().chain(solidcraft_engine::recovery::default_dir());
    let scope = designs.iter().fold(vfs::PathScope::new(roots), |s, d| s.with_design_folder(d));
    Ok(Some(scope.with_design(app.session.path.as_deref())))
}

/// What a picture of the view shows: the model as the window shows it (theme and colour vision
/// mode, the section the viewport shows: an open Section Analysis dialog's preview, a sketch's
/// Slice), with the grid and the origin's axes as `guides` says.
pub(crate) fn view_scene(app: &SolveApp, guides: solidcraft_engine::view::Guides) -> solidcraft_engine::render::Scene {
    let mut palette = crate::viewport::scene_palette(app.viewport_dark(), app.preferences.color_vision);
    // The view's own background colour (#7688).
    if let Some([r, g, b]) = app.preferences.viewport_background {
        palette.background = (solidcraft_engine::render::Rgb(r, g, b), solidcraft_engine::render::Rgb(r, g, b));
    }
    let mut scene = solidcraft_engine::view::scene_with_thread_markings(
        &app.session,
        &app.cam,
        crate::viewport::section_plane(app),
        solidcraft_engine::view::world_curve_parts,
        &palette,
        guides,
        app.preferences.cosmetic_threads,
    );
    scene.plain_cap = !crate::viewport::section_hatch(app);
    scene
}

/// The grid and the origin's axes as the window shows them (the Grid and Origin switches).
pub(crate) fn window_guides(app: &SolveApp) -> solidcraft_engine::view::Guides {
    solidcraft_engine::view::Guides {
        grid: (app.session.active_sketch.is_some() || app.ui.show_grid) && !crate::viewport::active_view(app).hide_grid,
        axes: app.origin_visible(),
    }
}

/// A picture of the view as the window shows it, RGBA, drawn on the GPU with the viewport's
/// pipelines (the renderer the app installs).
pub(crate) fn view_rgba(app: &SolveApp, w: u32, h: u32) -> Result<Vec<u8>, String> {
    solidcraft_engine::capture::rgba(&view_scene(app, window_guides(app)), &app.cam, w, h).map(|p| p.rgba)
}

/// [`view_rgba`] as PNG.
pub fn view_png(app: &SolveApp, w: u32, h: u32) -> Result<Vec<u8>, String> {
    let rgba = view_rgba(app, w, h)?;
    solidcraft_engine::render::png(w as usize, h as usize, rgba).ok_or_else(|| "PNG encoding failed".to_string())
}

pub fn handle(app: &mut SolveApp, ctx: &egui::Context, req: &ControlRequest) -> Outcome {
    crate::agent_chat::with_tool_request(app, req.chat_turn.as_ref(), |app| handle_request(app, ctx, req))
}

fn handle_request(app: &mut SolveApp, ctx: &egui::Context, req: &ControlRequest) -> Outcome {
    let level = match request_param_check(&req.method, &req.params) {
        Ok(level) => level,
        Err(e) => return err(e),
    };
    let scope = match request_scope(app, &req.params) {
        Ok(scope) => scope,
        Err(e) => return err(e),
    };
    if scope.is_none() && level.is_none() {
        return dispatch(app, ctx, &req.method, &req.params);
    }
    let mut p = req.params.clone();
    if let Some(o) = p.as_object_mut() {
        o.remove("allow_paths");
        o.remove("design_folders");
        o.remove("param_check");
    }
    let run = |app: &mut SolveApp| match level {
        Some(level) => dispatch_checked(app, ctx, &req.method, &p, level),
        None => dispatch(app, ctx, &req.method, &p),
    };
    match scope {
        Some(scope) => vfs::scoped(Some(scope), || run(app)),
        None => run(app),
    }
}

/// The request's `param_check` (#9184): how the commands of an `engine.execute`, `engine.batch`
/// or `engine.script` are checked against their contracts, whatever the app's own session level;
/// `None` when it names none (other methods ignore the field).
pub(crate) fn request_param_check(method: &str, params: &Value) -> Result<Option<ParamCheck>, String> {
    if !matches!(method, "engine.execute" | "command" | "engine.batch" | "engine.script") {
        return Ok(None);
    }
    match params.get("param_check") {
        None => Ok(None),
        Some(v) => v.as_str().and_then(ParamCheck::parse).map(Some).ok_or_else(|| "`param_check` must be off, warn or strict".to_string()),
    }
}

/// `dispatch` of an `engine.execute`, `engine.batch` or `engine.script` with its commands checked
/// at `level`, echoed in the reply as `param_check`, so a client can tell an app that checked
/// from an older one that ignored the field. The session goes back to its own level afterwards.
fn dispatch_checked(app: &mut SolveApp, ctx: &egui::Context, method: &str, p: &Value, level: ParamCheck) -> Outcome {
    let mut out = if switches_design(p).is_some() {
        switch_checked(app, ctx, method, p, level)
    } else {
        let own = app.session.param_check;
        app.session.param_check = level;
        let out = dispatch(app, ctx, method, p);
        app.session.param_check = own;
        out
    };
    if let Outcome::Done(Value::Object(o)) = &mut out {
        o.insert("param_check".into(), json!(level.name()));
    }
    out
}

/// A command that switches to another design, checked at `level` before it runs: it runs at the
/// app's own level, since the session it leaves and the one it brings in each keep theirs.
fn switch_checked(app: &mut SolveApp, ctx: &egui::Context, method: &str, p: &Value, level: ParamCheck) -> Outcome {
    let id = command(p).unwrap_or_default();
    let issues: Vec<String> = solidcraft_engine::schema::issues(id, p.get("params").unwrap_or(&Value::Null)).into_iter().map(|i| i.message).collect();
    if level == ParamCheck::Strict
        && let Some(first) = issues.first()
    {
        return err(solidcraft_engine::params::bad(id, first.as_str()));
    }
    let mut out = dispatch(app, ctx, method, p);
    if level == ParamCheck::Warn
        && !issues.is_empty()
        && let Outcome::Done(v) = &mut out
        && let Some(o) = v.get_mut("result").and_then(Value::as_object_mut)
    {
        o.insert("param_warnings".into(), json!(issues));
    }
    out
}

fn dispatch(app: &mut SolveApp, ctx: &egui::Context, method: &str, p: &Value) -> Outcome {
    crate::render_workspace::sync_backend(app);
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    let f = |k: &str| p.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
    let b = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
    match method {
        "ui.blur" => {
            // Internal rendering harness; deliberately no saved preference or product UI.
            let radius = if let Some(radius) = p.get("radius_px") {
                let Some(radius) = radius.as_f64().filter(|v| v.is_finite() && (0.0..=40.0).contains(v)) else {
                    return err("radius_px must be finite and between 0 and 40");
                };
                Some(radius as f32)
            } else {
                None
            };
            let alpha = if let Some(alpha) = p.get("surface_alpha") {
                let alpha = if alpha.is_null() {
                    None
                } else {
                    let Some(alpha) = alpha.as_f64().filter(|v| v.is_finite() && (0.0..=1.0).contains(v)) else {
                        return err("surface_alpha must be null or between 0 and 1");
                    };
                    Some((alpha * 255.0).round() as u8)
                };
                Some(alpha)
            } else {
                None
            };
            if let Some(radius) = radius {
                if let Err(error) = app.blur.set_radius(radius) {
                    return err(error);
                }
                ctx.request_repaint();
            }
            if let Some(alpha) = alpha {
                app.blur.set_surface_alpha(alpha);
                ctx.request_repaint();
            }
            ok(
                json!({"radius_px":app.blur.radius(), "max_radius_px":40, "capability":app.blur.capability_for(ctx.viewport_id()), "counters":app.blur.counters()}),
            )
        }
        "ui.status_fixture" => wrap(crate::viewport_status_fixture::control(app, p)),
        "ui.workspace" => {
            if let Some(workspace) = s("workspace") {
                if !matches!(workspace, "design" | "render") {
                    return err("workspace must be design or render");
                }
                if let Err(e) = crate::render_workspace::enter(app, workspace == "render") {
                    return err(e);
                }
            }
            ok(json!({"workspace":if crate::render_workspace::active(app) {"render"} else {"design"}}))
        }
        "engine.execute" | "command" => {
            let Some(id) = s("command").or(s("id")) else { return err("missing `command`") };
            wrap(execute(app, id, p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}))))
        }
        "engine.batch" => batch(app, p),
        "ui.agent_cursor" => {
            let s = &mut app.ui.agent_cursor;
            if let Some(v) = p.get("show").and_then(Value::as_bool) {
                s.show = v;
            }
            if let Some(v) = p.get("follow_camera").and_then(Value::as_bool) {
                s.follow_camera = v;
            }
            if let Some(v) = p.get("speed") {
                match crate::agent_cursor::Speed::from_value(v) {
                    Some(sp) => s.speed = sp,
                    None => return err("speed must be a number from 1 to 5, or \"instant\""),
                }
            }
            ok(json!(app.ui.agent_cursor))
        }
        "engine.script" => {
            let steps = p.as_array().or_else(|| p.get("commands").and_then(Value::as_array));
            if let Some(c) = steps.and_then(|steps| steps.iter().find_map(switches_design)) {
                return err(format!("engine.script unavailable: {c} switches to another design (send it on its own)"));
            }
            let mut replaced = false;
            let r = app
                .session
                .run_script_each(p, &mut |i, _| {
                    let id = steps.and_then(|steps| steps.get(i)).and_then(command).and_then(solidcraft_engine::find_command).map(|spec| spec.id);
                    replaced |= matches!(id, Some("file.new" | "doc.open" | "doc.recover"));
                })
                .map(|v| json!(v))
                .map_err(|e| e.to_string());
            // A successful replacement still happened if a later script step failed.
            if replaced {
                crate::documents::reset_chat(app);
                app.edit_in_place.active = None;
            }
            app.follow_view_request(false);
            wrap(r)
        }
        "engine.commands" => wrap(execute(app, "engine.commands", json!({}))),
        "document.inspect" => wrap(app.session.execute("document.inspect", p).map_err(|e| e.to_string())),
        "document.revision" => wrap(app.session.execute("document.revision", p).map_err(|e| e.to_string())),
        // The connection already counts as subscribed (`DesignWatch::join`), and its events start
        // from this answer: published first, so no event repeats it and none older follows.
        "control.subscribe" => {
            let revision = app.session.design_revision();
            DESIGN_WATCH.publish(&app.session.document_id, revision);
            ok(json!({"document_id": app.session.document_id, "revision": revision}))
        }
        "ui.messages" => ok(crate::messages::control(app, p)),
        "ui.inspect" => ok(json!({
            "render": crate::render_workspace::inspect_state(app),
            "ui": ui_state(app),
            "visibility": app.session.visibility,
            "viewport": app.viewport.rect.map(|r| json!([r.left(), r.top(), r.width(), r.height()])),
            "camera": app.cam,
            "tool": app.tool.as_ref().map(|t| t.cmd),
            "dialog": app.dialog.as_ref().map(|d| format!("{d:?}")),
            "status": app.status.as_ref().map(|(text, _, error)| json!({"text": text, "error": error})),
            "hover": app.viewport.hover.as_ref().map(|h| format!("{h:?}")),
            "renderer": if app.viewport.gpu.is_some() { "gpu" } else { "cpu" },
            "frame_ms": app.frame_ms,
            "draw": app.viewport.draw_stats.as_ref().and_then(|s| s.lock().ok().map(|d| json!({
                "draw_calls": d.draw_calls, "triangles": d.triangles, "line_segments": d.line_segments, "supersampled": d.supersampled,
            }))),
            "adapter": app.viewport.adapter,
            "file_busy": app.file_load.busy(),
            "file_picker_busy": app.file_picker_busy(),
            "file_dialog_open": app.file_picker_busy(),
            "build_ms": app.viewport.build_ms,
            "display_lod": app.viewport.lod.lock().map(|l| serde_json::to_value(&l.stats).unwrap_or_default()).unwrap_or_default(),
            "preview": {"active": app.preview.active, "busy": app.preview.busy, "error": app.preview.error, "ms": app.preview.ms, "replaced": app.preview.replaced},
            "home": app.home.open,
            "recent_thumbnails_ready": app.home.ready_recents(),
            "sample_thumbnails_ready": app.home.ready_samples(),
            "documents": app.docs.count(),
            "quit_requested": app.quit_requested,
            "quitting": app.docs.quitting.is_some(),
        })),
        "ui.file" => {
            let Some(path) = s("path") else { return err("path is required") };
            if p.get("insert").and_then(Value::as_bool).unwrap_or(false) {
                app.insert_path_async(path);
            } else {
                app.open_path_async(path);
            }
            ok(json!({"busy": app.file_load.busy()}))
        }
        "ui.documents" => ok(crate::documents::control(app, p)),
        "ui.operations" => wrap(crate::operations::control(app, p)),
        "ui.home" => {
            match p.get("open").and_then(Value::as_bool) {
                Some(true) => {
                    let rev = app.session.revision;
                    app.home.show(rev);
                }
                Some(false) => app.home.open = false,
                None => {}
            }
            if let Some(sample) = p.get("sample") {
                // The index is checked here: `open_sample` drops an index that names no sample
                // and says nothing, so the request would look like it had worked (#9087).
                let count = solidcraft_engine::sample::samples().len();
                let Some(index) = sample.as_i64() else {
                    return err(format!("`sample` must be an index from 0 to {}", count - 1));
                };
                if !(0..count as i64).contains(&index) {
                    return err(format!("`sample` must be an index from 0 to {}, not {index}", count - 1));
                }
                crate::home::open_sample(app, index as usize);
            }
            ok(json!({"open": app.home.open, "recent": app.home.recent}))
        }
        "ui.set" => {
            // What is hidden lives in the session (`browser.visibility`); `ui.set` still sets it.
            const VIS: [(&str, &str); 9] = [
                ("hiddenBodies", "hidden_bodies"),
                ("hiddenOrigin", "hidden_origin"),
                ("hiddenSketches", "hidden_sketches"),
                ("shownSketches", "shown_sketches"),
                ("hiddenJoints", "hidden_joints"),
                ("hiddenProfiles", "hidden_profiles"),
                ("shownDims", "shown_dimensions"),
                ("lockedBodies", "locked_bodies"),
                ("hiddenFolders", "hidden_folders"),
            ];
            let mut cur = serde_json::to_value(&app.ui).unwrap_or(json!({}));
            let mut vis = serde_json::to_value(&app.session.visibility).unwrap_or(json!({}));
            let mut sketches = None;
            if let (Some(o), Some(vo), Some(src)) = (cur.as_object_mut(), vis.as_object_mut(), p.as_object()) {
                for (k, v) in src {
                    let shown = |v: &Value| v.as_bool().map(|b| json!(!b)).unwrap_or_else(|| v.clone());
                    match (VIS.iter().find(|(camel, _)| camel == k), k.as_str()) {
                        (Some((_, snake)), _) => vo.insert(snake.to_string(), v.clone()),
                        (None, "showOrigin") => vo.insert("origin_hidden".into(), shown(v)),
                        (None, "showSketches") => {
                            sketches = Some(v.as_bool().ok_or_else(|| "`showSketches` must be true or false".to_string()));
                            None
                        }
                        _ => o.insert(k.clone(), v.clone()),
                    };
                }
            }
            let sketches = match sketches.transpose() {
                Ok(s) => s,
                Err(e) => return err(e),
            };
            match (serde_json::from_value::<crate::UiState>(cur), serde_json::from_value::<solidcraft_engine::Visibility>(vis)) {
                (Ok(mut u), Ok(mut v)) => {
                    if let Some(shown) = sketches {
                        v.hidden_folders.retain(|k| k != SKETCHES_FOLDER);
                        if !shown {
                            v.hidden_folders.push(SKETCHES_FOLDER.to_string());
                        }
                    }
                    if crate::workspace::render_tab(&u.tab) != crate::render_workspace::active(app)
                        && let Err(e) = crate::render_workspace::enter(app, crate::workspace::render_tab(&u.tab))
                    {
                        return err(e);
                    }
                    u.ribbon_context = app.ui.ribbon_context.clone();
                    app.ui = u;
                    app.session.visibility = v;
                    ok(ui_state(app))
                }
                (Err(e), _) | (_, Err(e)) => err(e),
            }
        }
        "ui.view" => {
            let v = s("view").unwrap_or("home");
            app.viewport.orbit_free = None;
            if p.get("animate").and_then(Value::as_bool).unwrap_or(false) {
                if v != "fit" && v != "home" && StandardView::parse(v).is_none() {
                    return err(format!("unknown view `{v}`"));
                }
                app.animate_view(v);
                return ok(json!({ "animating": true, "to": app.cam_anim.map(|a| a.to) }));
            }
            app.cancel_view_animation();
            match v {
                "fit" => app.fit_view(),
                "home" => {
                    app.cam.set_view(StandardView::Iso);
                    app.fit_view();
                }
                v => match StandardView::parse(v) {
                    Some(sv) => app.cam.set_view(sv),
                    None => return err(format!("unknown view `{v}`")),
                },
            }
            ok(json!(app.cam))
        }
        "ui.camera" => wrap(camera(app, p)),
        "ui.palette" => {
            if p.get("open").and_then(Value::as_bool) == Some(false) {
                app.ui.palette_open = false;
                app.sbox.text.clear();
            } else {
                app.start("ui.palette");
                if let Some(query) = s("text") {
                    app.sbox.text = query.chars().take(crate::shortcut_box::MAX_QUERY).collect();
                }
            }
            ok(json!({"open": app.ui.palette_open, "text": app.sbox.text}))
        }
        "ui.start" => {
            let Some(id) = s("command") else { return err("missing `command`") };
            // `params`: what the dialog opens on. Refused: nothing starts, or the dialog closes.
            let params = p.get("params").filter(|v| !v.is_null());
            if params.is_some() && id != "joint.drive" && id != "motion.study" {
                return err(format!("`{id}` takes no ui.start params yet (joint.drive takes joint, values; motion.study takes study)"));
            }
            app.start(id);
            if let Some(params) = params.filter(|_| id == "motion.study") {
                let Some(crate::dialogs::Kind::Motion(k)) = app.dialog.as_mut().map(|d| &mut d.kind) else {
                    return err("Motion Study did not open");
                };
                if let Err(e) = crate::dialogs_motion::preset(&app.session, k, params) {
                    crate::dialogs::cancel(app);
                    return err(e);
                }
            } else if let Some(params) = params {
                if app.tree.capture_prompt.is_some() {
                    return err("Drive Joints waits for the Capture Position question (ui.capture)");
                }
                let Some(crate::dialogs::Kind::Assembly(k)) = app.dialog.as_mut().map(|d| &mut d.kind) else {
                    let why = app.status.as_ref().filter(|s| s.2).map(|s| s.0.clone());
                    return err(why.unwrap_or_else(|| "Drive Joints did not open".into()));
                };
                if let Err(e) = crate::dialogs_assembly::preset(&app.session, k, params) {
                    crate::dialogs::cancel(app);
                    return err(e);
                }
            }
            ok(json!({"tool": app.tool.as_ref().map(|t| t.cmd), "dialog": app.dialog.as_ref().map(|d| format!("{d:?}"))}))
        }
        "ui.move" | "ui.click" => {
            let (Some(x), Some(y)) = (f("x"), f("y")) else { return err("missing x/y") };
            if x.abs() > 1e6 || y.abs() > 1e6 {
                return err("x/y must be within ±1000000 screen points");
            }
            let pos = egui::pos2(x as f32, y as f32);
            let modifiers = if method == "ui.move" {
                egui::Modifiers { shift: b("shift"), alt: b("alt"), ctrl: b("ctrl"), command: b("ctrl") || b("cmd"), mac_cmd: b("cmd") }
            } else {
                egui::Modifiers::NONE
            };
            app.synthetic.push(egui::Event::ModifiersChanged(modifiers));
            app.synthetic.push(egui::Event::PointerMoved(pos));
            if method == "ui.click" {
                let button = match s("button") {
                    Some("right") => egui::PointerButton::Secondary,
                    Some("middle") => egui::PointerButton::Middle,
                    _ => egui::PointerButton::Primary,
                };
                let modifiers = egui::Modifiers { shift: b("shift"), ctrl: b("ctrl"), command: b("ctrl"), alt: b("alt"), ..Default::default() };
                // `double: true` sends two clicks in one frame, which egui reads as a double-click.
                for _ in 0..if b("double") { 2 } else { 1 } {
                    app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers });
                    app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers });
                }
            }
            ok(Value::Null)
        }
        "ui.drag" => {
            let (Some(x0), Some(y0), Some(x1), Some(y1)) = (f("x0"), f("y0"), f("x1"), f("y1")) else { return err("missing x0/y0/x1/y1") };
            let button = match s("button") {
                Some("right") => egui::PointerButton::Secondary,
                Some("middle") => egui::PointerButton::Middle,
                _ => egui::PointerButton::Primary,
            };
            let modifiers = egui::Modifiers { shift: b("shift"), ctrl: b("ctrl"), command: b("ctrl"), alt: b("alt"), ..Default::default() };
            let (a, z) = (egui::pos2(x0 as f32, y0 as f32), egui::pos2(x1 as f32, y1 as f32));
            // Positioning precedes the press by a frame: it is not a modifier-only gesture.
            app.synthetic.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
            app.synthetic.push(egui::Event::PointerMoved(a));
            app.synthetic.push(egui::Event::PointerButton { pos: a, button, pressed: true, modifiers });
            // `hold`: frames to keep the button down before moving (press-and-hold).
            for _ in 0..f("hold").unwrap_or(0.0).clamp(0.0, 600.0) as usize {
                app.synthetic.push(egui::Event::PointerMoved(a));
            }
            let steps = f("steps").unwrap_or(10.0).clamp(1.0, 200.0) as usize;
            // `via`: [[x, y], ...] the pointer passes through on its way (a lasso's loop, #4376).
            let via: Vec<egui::Pos2> = p
                .get("via")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(500)
                .filter_map(|q| Some(egui::pos2(q.get(0)?.as_f64()? as f32, q.get(1)?.as_f64()? as f32)))
                .filter(|q| q.is_finite())
                .collect();
            let mut from = a;
            for to in via.into_iter().chain([z]) {
                for k in 1..=steps {
                    app.synthetic.push(egui::Event::PointerMoved(from + (to - from) * (k as f32 / steps as f32)));
                }
                from = to;
            }
            // `hold_end`: frames to keep the button down at the end (to look at a drag's snap).
            for _ in 0..f("hold_end").unwrap_or(0.0).clamp(0.0, 600.0) as usize {
                app.synthetic.push(egui::Event::PointerMoved(z));
            }
            app.synthetic.push(egui::Event::PointerButton { pos: z, button, pressed: false, modifiers });
            ok(Value::Null)
        }
        "ui.editFeature" => {
            let key = match p.get("feature") {
                Some(Value::Number(n)) => n.to_string(),
                Some(Value::String(x)) => x.clone(),
                _ => return err("`feature` must be an id or name"),
            };
            let Some(id) = app.session.doc.find_feature(&key).map(|f| f.id) else { return err(format!("no feature `{key}`")) };
            app.edit_feature(id);
            ok(json!({"dialog": app.dialog.as_ref().map(|d| format!("{d:?}")), "marker": app.session.doc.marker}))
        }
        "ui.selection" => ok(json!({
            "selection": app.session.selection,
            "dialog": app.dialog.as_ref().map(|d| d.inputs.iter().map(|i| json!({"label": i.label, "items": i.items})).collect::<Vec<_>>()),
            "hover": app.viewport.hover.as_ref().map(|h| format!("{h:?}")),
            "dimensions": crate::dim_view::drawn().into_iter().map(|(id, p)| json!({"id": id, "x": p.x, "y": p.y})).collect::<Vec<_>>(),
            "shown_dimensions": crate::dim_view::model_drawn()
                .into_iter()
                .map(|(sketch, id, p)| json!({"sketch": sketch, "id": id, "x": p.x, "y": p.y}))
                .collect::<Vec<_>>(),
            "dimension_selected": crate::dim_view::selected(app),
            "dimension_editing": crate::dim_view::editing(),
        })),
        "ui.triad" => {
            // Screen points of the 3D move triad's handles (x, y, z arrows and the plane square).
            let Some(rect) = app.viewport.rect else { return err("no viewport") };
            let proj = crate::viewport::projection(app, rect);
            ok(json!(
                crate::sketch3d::handle_points(app, &proj).into_iter().map(|(n, p)| json!({"handle": n, "x": p.x, "y": p.y})).collect::<Vec<_>>()
            ))
        }
        "ui.dialogInput" => {
            // What clicking a selection input's chip does: make it the one picks go to.
            let i = p.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            match app.dialog.as_mut() {
                Some(d) if i < d.inputs.len() => {
                    d.active = i;
                    ok(json!({"label": d.inputs.get(i).map(|x| x.label)}))
                }
                _ => err("no dialog input with that index"),
            }
        }
        "ui.handles" => {
            // The handles drawn last frame (toolbar buttons `toolbar:<id>`, panels, browser rows…).
            let list = crate::scenario::handles(s("prefix").unwrap_or(""));
            ok(json!(list.into_iter().map(|(n, p)| json!({"name": n, "x": p.x, "y": p.y})).collect::<Vec<_>>()))
        }
        "ui.pick" => wrap(crate::view_pick::pick_command(app, p)),
        "ui.at" => {
            if let Some(path) = p.get("occurrence").and_then(Value::as_array) {
                let ids: Option<Vec<u64>> = path.iter().map(Value::as_u64).collect();
                let Some(ids) = ids else { return err("occurrence path must contain integer ids") };
                let key = ids.iter().map(u64::to_string).collect::<Vec<_>>().join("/");
                let control = s("control").unwrap_or("row");
                return match crate::scenario::handle_at(&format!("{control}:occurrence:{key}")) {
                    Some(q) => ok(json!([q.x, q.y])),
                    None => err("occurrence row or control is not shown"),
                };
            }
            // A scenario point: {world: [x,y,z]} | {sketch: [x,y]} | {plane: "XY"} (the middle of an
            // origin plane's square) → screen point.
            if let Some(h) = p.get("handle").and_then(Value::as_str) {
                return match crate::scenario::handle_at(h) {
                    Some(q) => ok(json!([q.x, q.y])),
                    None => err(format!("handle {h} is not shown")),
                };
            }
            // A browser row by its label: the row itself, or its fold arrow.
            for (k, chevron) in [("browser", false), ("chevron", true)] {
                if let Some(label) = p.get(k).and_then(Value::as_str) {
                    let rows = crate::browser::drawn_rows();
                    let found = rows
                        .iter()
                        .find(|r| r.0 == label)
                        .and_then(|r| if chevron { r.2.map(|f| f.center()) } else { Some(r.1.left_center() + egui::vec2(120.0, 0.0)) });
                    return match found {
                        Some(q) => ok(json!([q.x, q.y])),
                        None => err(format!("no browser row `{label}`{} on screen", if chevron { " with a fold arrow" } else { "" })),
                    };
                }
            }
            if let Some(d) = p.get("dimension").and_then(Value::as_str) {
                return match crate::dim_view::text_at(d) {
                    Some(q) => ok(json!([q.x, q.y])),
                    None => err(format!("dimension {d} is not shown")),
                };
            }
            let Some(rect) = app.viewport.rect else { return err("no viewport") };
            let proj = crate::viewport::projection(app, rect);
            let num = |v: &Value, i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(0.0);
            let world = if let Some(w) = p.get("world") {
                Some(solidcraft_engine::geom::Vec3::new(num(w, 0), num(w, 1), num(w, 2)))
            } else if let Some(s) = p.get("sketch") {
                let st = app.session.world_state();
                app.session
                    .active_sketch
                    .and_then(|id| st.sketch(id))
                    .map(|ss| ss.plane.to_world(solidcraft_engine::geom::Vec2::new(num(s, 0), num(s, 1))))
            } else if let Some(n) = p.get("plane").and_then(Value::as_str) {
                crate::viewport::origin_plane_target(app, &proj, n)
            } else if let Some(n) = p.get("axis").and_then(Value::as_str) {
                // Along an origin axis, on its solid part.
                let size = crate::selection::origin_size(app.cam.half_height());
                crate::viewport::origin_axes(app).into_iter().find(|(name, _)| *name == n).map(|(_, d)| d * (size * 0.8))
            } else if let Some(n) = p.get("gizmo").and_then(Value::as_str) {
                // A handle of the move triad ("x", "xy", "rz", "ball"), moved `by` [x,y,z] mm or
                // turned `turn` degrees (rings).
                let by = p.get("by").map(|b| solidcraft_engine::geom::Vec3::new(num(b, 0), num(b, 1), num(b, 2))).unwrap_or_default();
                crate::gizmo::handle_point(n, by, p.get("turn").and_then(Value::as_f64).unwrap_or(0.0))
            } else {
                None
            };
            match world.and_then(|w| proj.to_screen(w)) {
                Some(q) => ok(json!([q.x, q.y])),
                None => err(format!("not on screen: {}", p)),
            }
        }
        "ui.worldToScreen" => {
            let Some(rect) = app.viewport.rect else { return err("no viewport") };
            let proj = crate::viewport::projection(app, rect);
            let pts: Vec<Value> = p
                .get("points")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(1000)
                .map(|v| {
                    let c = |i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(0.0);
                    proj.to_screen(solidcraft_engine::geom::Vec3::new(c(0), c(1), c(2))).map_or(Value::Null, |q| json!([q.x, q.y]))
                })
                .collect();
            ok(json!(pts))
        }
        "ui.sketchToScreen" => {
            // Screen points of active-sketch coordinates (for driving sketch interaction).
            let st = app.session.world_state();
            let (Some(rect), Some(ss)) = (app.viewport.rect, app.session.active_sketch.and_then(|s| st.sketch(s))) else {
                return err("no active sketch in a drawn viewport");
            };
            let proj = crate::viewport::projection(app, rect);
            let pts: Vec<Value> = p
                .get("points")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(1000)
                .map(|v| {
                    let c = |i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(0.0);
                    let q = solidcraft_engine::geom::Vec2::new(c(0), c(1));
                    proj.to_screen(ss.plane.to_world(q)).map_or(Value::Null, |p| json!([p.x, p.y]))
                })
                .collect();
            ok(json!(pts))
        }
        "ui.scroll" => {
            let (Some(x), Some(y)) = (f("x"), f("y")) else { return err("missing x/y") };
            // Mouse-wheel semantics (lines): zooms regardless of the trackpad pan
            // preference, so scripts and agents keep working. Trackpads report points.
            // `ctrl: true` replays a Ctrl+wheel pinch in points (#8480). An explicit `unit`
            // overrides that choice; without ctrl the default remains a line wheel.
            let speed = ctx.options(|o| o.input_options.line_scroll_speed).max(1.0);
            let points = f("delta").unwrap_or(-100.0);
            let ctrl = b("ctrl");
            let unit = match p.get("unit") {
                Some(Value::String(unit)) if unit == "point" => egui::MouseWheelUnit::Point,
                Some(Value::String(unit)) if unit == "line" => egui::MouseWheelUnit::Line,
                Some(_) => return err("`unit` must be point or line"),
                None if ctrl => egui::MouseWheelUnit::Point,
                None => egui::MouseWheelUnit::Line,
            };
            let delta = match unit {
                // A point wheel reaches the app as points; a line wheel is scaled up by the
                // line scroll speed, so divide it out and both land on the same points.
                egui::MouseWheelUnit::Line => points / f64::from(speed),
                _ => points,
            };
            let (x, y, delta) = (x as f32, y as f32, delta as f32);
            if !x.is_finite() || !y.is_finite() || !delta.is_finite() {
                return err("x, y and delta must fit finite screen coordinates");
            }
            let modifiers = if ctrl { egui::Modifiers::CTRL } else { Default::default() };
            let explicit = p.get("ctrl").is_some();
            app.synthetic.push(egui::Event::PointerMoved(egui::pos2(x, y)));
            if explicit {
                app.synthetic.push(egui::Event::ModifiersChanged(modifiers));
            }
            app.synthetic.push(egui::Event::MouseWheel { unit, delta: egui::vec2(0.0, delta), modifiers, phase: egui::TouchPhase::Move });
            if explicit {
                app.synthetic.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
            }
            ok(Value::Null)
        }
        "ui.key" => {
            let Some(key) = s("key").and_then(key_from) else { return err("unknown key") };
            // Preserve the control channel's legacy primary-modifier alias. An explicit
            // ctrl flag opts into physical Control/Command state (ctrl:false for Cmd alone).
            let explicit_ctrl = p.get("ctrl").is_some();
            let ctrl = if explicit_ctrl { b("ctrl") } else { b("cmd") };
            let modifiers = egui::Modifiers { shift: b("shift"), alt: b("alt"), ctrl, command: b("cmd") || ctrl, mac_cmd: b("cmd") && explicit_ctrl };
            let pressed = match p.get("pressed") {
                None => None,
                Some(Value::Bool(pressed)) => Some(*pressed),
                Some(_) => return err("pressed must be a boolean"),
            };
            app.synthetic.push(egui::Event::ModifiersChanged(modifiers));
            if pressed != Some(false) {
                app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
            }
            if pressed != Some(true) {
                app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
            }
            if pressed.is_none() {
                app.synthetic.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
            }
            ok(Value::Null)
        }
        "ui.text" => {
            app.synthetic.push(egui::Event::Text(s("text").unwrap_or("").to_string()));
            ok(Value::Null)
        }
        "ui.icon_audit" => {
            if b("reset") {
                crate::icon_svg::audit::start();
            }
            let requested = crate::icon_svg::audit::take();
            if b("stop") {
                crate::icon_svg::audit::stop();
            }
            ok(json!({"requested": requested}))
        }
        "ui.menu" => {
            if b("close") {
                crate::context_menu::close(app);
                return ok(json!({"open": false}));
            }
            if let (Some(x), Some(y)) = (f("x"), f("y")) {
                let target = match p.get("target") {
                    Some(v) => match serde_json::from_value::<crate::context_menu::Target>(v.clone()) {
                        Ok(t) => t,
                        Err(e) => return err(e),
                    },
                    None => crate::context_menu::Target::Viewport,
                };
                crate::context_menu::request_open(app, egui::pos2(x as f32, y as f32), target);
                return ok(json!({"pending": true}));
            }
            ok(crate::context_menu::describe(app))
        }
        "ui.confirm" => match p.get("accept").and_then(Value::as_bool) {
            Some(a) => {
                if crate::delete::confirm(app, a) {
                    ok(json!({"accepted": a}))
                } else {
                    err("nothing is waiting for a confirmation")
                }
            }
            None => ok(json!({"pending": app.menu.confirm.as_ref().map(|c| c.report.clone())})),
        },
        "ui.shortcut" => {
            let Some(id) = s("command") else { return err("missing `command`") };
            let slot = p.get("slot").and_then(Value::as_u64).map(|n| usize::try_from(n).unwrap_or(usize::MAX));
            let report = |app: &SolveApp| ok(json!({"command": id, "key": crate::keymap::effective(app, id), "keys": crate::keymap::keys(app, id)}));
            match s("key") {
                Some(k) => match crate::keymap::bind_at(app, id, slot, k, b("replace")) {
                    Ok(()) => report(app),
                    Err(e) => err(e),
                },
                None => report(app),
            }
        }
        "ui.browser" => ok(json!({
            "occurrences": crate::browser::drawn_occurrences().iter().map(|(path, label, r, open)| json!({"path": path, "label": label, "y": r.center().y, "open": open})).collect::<Vec<_>>(),
            "rows": crate::browser::drawn_rows().iter().map(|(label, r, _, open)| json!({"label": label, "y": r.center().y, "open": open})).collect::<Vec<_>>(),
            "selection": app.session.selection,
            "picked_components": app.tree.picked_components,
        })),
        "ui.capture" => match s("action") {
            Some(a) => match crate::browser::answer_capture(app, a) {
                Ok(()) => ok(json!({"pending_moves": app.session.pending_moves.len()})),
                Err(e) => err(e),
            },
            None => ok(json!({"question": app.tree.capture_prompt, "pending_moves": app.session.pending_moves.len()})),
        },
        "ui.prefs" => wrap(crate::prefs::control(app, p)),
        "ui.agent_chat" => wrap(crate::agent_chat::control(app, ctx, p)),
        "ui.help" => match s("item") {
            Some(i @ ("about" | "updates" | "shortcuts" | "report" | "crashes")) => {
                crate::help::run(app, ctx, i);
                ok(
                    json!({"diagnostics": crate::help::diagnostics(app), "version": crate::help::VERSION, "commit": crate::help::COMMIT, "built": crate::help::BUILD_DATE}),
                )
            }
            _ => ok(
                json!({"diagnostics": crate::help::diagnostics(app), "version": crate::help::VERSION, "commit": crate::help::COMMIT, "built": crate::help::BUILD_DATE}),
            ),
        },
        "ui.selectOther" => wrap(crate::select_other::control(app, p)),
        "ui.menuPick" => {
            let Some(key) = s("item") else { return err("missing `item` (id or label)") };
            wrap(crate::context_menu::pick(app, key))
        }
        "ui.rename" => {
            if crate::context_menu::finish_rename(app, s("text"), p.get("commit").and_then(Value::as_bool).unwrap_or(true)) {
                ok(json!({"done": true}))
            } else {
                err("no rename in progress")
            }
        }
        "ui.window" => match crate::titlebar::window_action(app, ctx, s("action").unwrap_or("")) {
            Ok(()) => ok(json!({"custom_titlebar": app.custom_titlebar, "maximized": ctx.input(|i| i.viewport().maximized)})),
            Err(e) => err(e),
        },
        "ui.resize" => {
            let (Some(w), Some(h)) = (f("width"), f("height")) else { return err("missing width/height") };
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w.clamp(320.0, 8192.0) as f32, h.clamp(240.0, 8192.0) as f32)));
            ok(Value::Null)
        }
        "ui.screenshot" => match s("path").map(vfs::check) {
            Some(Err(e)) => err(e),
            _ => Outcome::Screenshot { path: s("path").map(str::to_string) },
        },
        "ui.render" => {
            let w = f("width").unwrap_or(1280.0).clamp(16.0, 8192.0) as u32;
            let h = f("height").unwrap_or(800.0).clamp(16.0, 8192.0) as u32;
            // As the window shows it (`view_scene`). The grid and the axes as the window shows
            // them, unless the request says: `grid` and `axes` leave them out or put them in.
            let shown = window_guides(app);
            let guides = solidcraft_engine::view::Guides {
                grid: p.get("grid").and_then(Value::as_bool).unwrap_or(shown.grid),
                axes: p.get("axes").and_then(Value::as_bool).unwrap_or(shown.axes),
            };
            // A path outside the request's folders is refused before anything is drawn.
            if let Some(Err(e)) = s("path").map(vfs::check) {
                return err(e);
            }
            let scene = view_scene(app, guides);
            let png = match solidcraft_engine::capture::png(&scene, &app.cam, w, h) {
                Ok(png) => png,
                Err(e) => return err(format!("render failed: {e}")),
            };
            // What drew it (gpu, gpu-software or cpu), and the warning a CPU render carries.
            let (renderer, warning) = (png.renderer, solidcraft_engine::capture::warning(png.renderer));
            match s("path") {
                Some(path) => match vfs::check(path).and_then(|()| std::fs::write(path, &png.bytes)) {
                    Ok(()) => ok(json!({"path": path, "bytes": png.bytes.len(), "renderer": renderer, "warning": warning})),
                    Err(e) => err(e),
                },
                None => ok(json!({"bytes": png.bytes.len(), "renderer": renderer, "warning": warning})),
            }
        }
        "app.quit" => {
            // `force`: quit without asking about unsaved designs.
            if p.get("force").and_then(Value::as_bool).unwrap_or(false) {
                app.quit_requested = true;
            } else {
                crate::documents::request_quit(app);
            }
            if app.quit_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ok(json!({"quitting": app.quit_requested, "asking": app.docs.closing.is_some()}))
        }
        other => err(format!("unknown method `{other}`")),
    }
}

/// `ui.camera`: the viewport's orbit camera (`target`, `yaw`, `pitch`, `roll` in radians,
/// `distance` in mm, `fov`, `up`), or a new one. A field left out keeps its value, so a reply
/// passed back sets the same view again. `fov` is the angle the window draws with (see [`drawn`]). `fit`: frame the listed occurrences (ids or names;
/// `root`: the bodies outside every component), or everything (`true`), keeping the direction;
/// `false` frames nothing.
/// Nothing changes when a field is refused.
fn camera(app: &mut SolveApp, p: &Value) -> Result<Value, String> {
    const FIELDS: [&str; 8] = ["target", "yaw", "pitch", "roll", "distance", "fov", "up", "fit"];
    let o = match p {
        Value::Null => serde_json::Map::new(),
        Value::Object(o) => o.clone(),
        _ => return Err("params must be an object".into()),
    };
    if let Some(k) = o.keys().find(|k| !FIELDS.contains(&k.as_str())) {
        return Err(format!("unknown camera field `{k}` (it takes {})", FIELDS.join(", ")));
    }
    let mut cam = drawn(app);
    let sets = o.keys().any(|k| k != "fit");
    if sets {
        let mut v = serde_json::to_value(cam).map_err(|e| e.to_string())?;
        for (k, x) in o.iter().filter(|(k, _)| *k != "fit") {
            v[k] = x.clone();
        }
        cam = serde_json::from_value(v).map_err(|e| format!("bad camera: {e}"))?;
        // The distance cap is far past any Fit (so every reply replays) and still finite in f32.
        if !cam.is_valid() || cam.distance > 1e15 || cam.pitch.abs() > std::f64::consts::FRAC_PI_2 {
            return Err("the camera must be finite, with a distance up to 1e15 mm and a pitch within ±π/2".into());
        }
        // Straight up or down keeps a heading, as the standard views do.
        let lim = std::f64::consts::FRAC_PI_2 - 1e-6;
        cam.pitch = cam.pitch.clamp(-lim, lim);
        // Perspective, its angle and the up axis are settings the camera follows, not its own.
        if cam.fov != drawn(app).fov {
            return Err("`fov` follows Perspective view (ui.set {perspective}) and its angle (ui.prefs {perspective_fov})".into());
        }
        cam.fov = app.cam.fov;
        if cam.up != app.cam.up {
            return Err("`up` follows the up axis preference (ui.prefs {up_axis})".into());
        }
    }
    let fit = match o.get("fit") {
        None | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) if !crate::render_workspace::active(app) && crate::viewport::view_bounds(app).is_empty() => {
            return Err("nothing is shown to fit".into());
        }
        Some(Value::Bool(true)) => Some(None),
        Some(Value::Array(list)) if !list.is_empty() && list.len() <= 10_000 => {
            let occs = list.iter().map(|v| occurrence(&app.session, v)).collect::<Result<Vec<u64>, String>>()?;
            let b = crate::dialogs_assembly::occurrence_bounds(&app.session, &occs);
            if b.is_empty() {
                return Err("those occurrences show nothing to fit".into());
            }
            Some(Some(b))
        }
        Some(_) => return Err("`fit` must be true, false or a list of occurrences".into()),
    };
    // A read leaves a view change that is animating alone.
    if !sets && fit.is_none() {
        return serde_json::to_value(drawn(app)).map_err(|e| e.to_string());
    }
    app.cancel_view_animation();
    app.viewport.orbit_free = None;
    app.cam = cam;
    match fit {
        Some(None) => app.fit_view(),
        Some(Some(b)) => app.fit_bounds(b),
        None => {}
    }
    serde_json::to_value(drawn(app)).map_err(|e| e.to_string())
}

/// The camera as the window draws it: with the view's angle ([`SolveApp::view_fov`]), or in Render
/// with the Render camera's own.
fn drawn(app: &SolveApp) -> Camera {
    let mut cam = app.cam;
    if !crate::render_workspace::active(app) {
        cam.fov = app.view_fov();
    }
    cam
}

/// An occurrence by id or name (or a component's name: its first occurrence); 0 is the root.
fn occurrence(s: &solidcraft_engine::Session, v: &Value) -> Result<u64, String> {
    let key = match v {
        Value::Number(n) => n.to_string(),
        Value::String(x) => x.clone(),
        _ => return Err("an occurrence must be an id or a name".into()),
    };
    if key == "0" || key.eq_ignore_ascii_case("root") {
        return Ok(0);
    }
    s.doc
        .occurrences
        .iter()
        .find(|o| o.id.to_string() == key || o.name == key)
        .map(|o| o.id)
        .or_else(|| s.doc.find_component(&key).and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id))
        .ok_or_else(|| format!("no occurrence `{key}`"))
}

/// Save a screenshot PNG; without a `path`, in the temporary folder ([`vfs::temp_dir`], which in
/// the browser keeps it in memory: std's would panic there).
pub fn save_screenshot(image: &egui::ColorImage, path: Option<&str>) -> Value {
    let [w, h] = image.size;
    let path = path.map(str::to_string).unwrap_or_else(|| vfs::temp_dir().join("solidcraft-shot.png").to_string_lossy().to_string());
    match write_screenshot(image, &path) {
        Ok(()) => json!({"ok": true, "result": {"path": path, "width": w, "height": h}}),
        Err(e) => json!({"ok": false, "error": e}),
    }
}

/// Write `image` to `path` in the format its extension names (a PNG for `.png`), through
/// [`vfs`] like every other save: the disk natively, a download or browser storage in the web
/// app (#7192).
pub(crate) fn write_screenshot(image: &egui::ColorImage, path: &str) -> Result<(), String> {
    let [w, h] = image.size;
    let mut buf = Vec::with_capacity(w * h * 4);
    for c in &image.pixels {
        buf.extend_from_slice(&c.to_array());
    }
    let img = image::RgbaImage::from_raw(w as u32, h as u32, buf).ok_or("bad image")?;
    let format = image::ImageFormat::from_path(path).map_err(|e| e.to_string())?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    img.write_to(&mut bytes, format).map_err(|e| e.to_string())?;
    vfs::write(path, bytes.get_ref()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn blur_control_validates_before_mutation_and_reports_no_gpu() {
        let mut h = crate::scenario::Harness::new();
        for invalid in [json!(-1), json!(41), json!("18"), json!(null)] {
            let reply = h.call("ui.blur", json!({"radius_px":invalid}));
            assert_eq!(reply["ok"], false, "{reply}");
            assert_eq!(h.app.blur.radius(), 0.0);
        }
        let reply = h.call("ui.blur", json!({"radius_px":18,"surface_alpha":2}));
        assert_eq!(reply["ok"], false);
        assert_eq!(h.app.blur.radius(), 0.0);
        let reply = h.call("ui.blur", json!({"radius_px":18,"surface_alpha":0.7}));
        assert_eq!(reply["ok"], true);
        assert!(matches!(h.app.blur.capability(), crate::blur::Capability::Unavailable(_)));
        h.frames(2);
        assert_eq!(h.app.blur.counters().textures_created, 0);
        assert_eq!(h.app.blur.counters().markers_emitted, 0);
        h.call("ui.blur", json!({"radius_px":0,"surface_alpha":null}));
        assert_eq!(h.app.blur.radius(), 0.0);
    }

    use serde_json::json;
    use solidcraft_engine::{Sel, Session};

    use super::*;
    use crate::Services;

    #[test]
    fn viewport_status_fixture_rejects_hostile_presets_without_mutating_the_session() {
        let mut h = crate::scenario::Harness::new();
        let revision = h.app.session.revision;
        for params in [Value::Null, json!([]), json!({}), json!({"preset":false}), json!({"preset":[]}), json!({"preset":"unknown"})] {
            let reply = h.call("ui.status_fixture", params);
            assert_eq!(reply["ok"], false, "{reply}");
            assert_eq!(h.app.session.revision, revision);
        }
        let reply = h.call("ui.status_fixture", json!({"preset":"mode"}));
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(h.app.session.revision, revision);
        assert!(h.app.session.analysis.is_none());
        for preset in ["off", "mode"] {
            h.call("ui.status_fixture", json!({"preset":preset}));
            h.call("ui.resize", json!({"width":200,"height":600}));
            h.frames(4);
            assert_eq!(h.app.session.revision, revision);
            h.call("ui.resize", json!({"width":1400,"height":900}));
            h.frames(4);
        }
    }

    #[test]
    fn touchpad_automation_moves_with_modifiers_and_releases_without_buttons() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.nav = "touchpad".into();
        h.call("ui.move", json!({"x":800,"y":500}));
        let before = h.app.cam;
        h.call("ui.move", json!({"x":800,"y":500,"shift":true}));
        h.step(&json!({"move":[830,520],"shift":true})).unwrap();
        assert_ne!(h.app.cam.target, before.target);
        assert_eq!(h.app.cam.yaw, before.yaw);
        assert!(!h.ctx.input(|i| i.pointer.any_down()));
        h.call("ui.move", json!({"x":830,"y":520,"alt":true}));
        h.step(&json!({"move":[850,530],"alt":true})).unwrap();
        assert!((h.app.cam.yaw - before.yaw - 0.16).abs() < 1e-6);
        let after = h.app.cam;
        h.call("ui.move", json!({"x":800,"y":500}));
        h.frames(3);
        assert_eq!(h.app.cam, after);
        assert!(h.ctx.input(|i| i.modifiers.is_none()));
        assert!(h.app.session.selection.is_empty());
        assert!(h.app.viewport.boxsel.is_none());
    }

    /// #8480: `ui.scroll` with `ctrl: true` replays a Ctrl+wheel pinch in points. Without
    /// `ctrl` the wheel stays a line wheel on the mouse path.
    #[test]
    fn scroll_sends_a_native_pinch_with_ctrl() {
        let mut app = SolveApp::new(Session::default(), Services::default());
        let ctx = crate::fonts::test_context();
        let (req, _) = ControlRequest::new("ui.scroll", json!({"x":200,"y":300,"delta":40,"ctrl":true}));
        assert!(matches!(handle(&mut app, &ctx, &req), Outcome::Done(_)));
        // The pointer event is delivered a frame before the wheel (one pointer event per frame).
        let mut events = Vec::new();
        for _ in 0..3 {
            let mut raw = egui::RawInput::default();
            app.raw_input_hook(&mut raw);
            events.extend(raw.events);
        }
        let wheels: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                egui::Event::MouseWheel { unit, delta, modifiers, .. } => Some((*unit, *delta, *modifiers)),
                _ => None,
            })
            .collect();
        assert_eq!(wheels.len(), 1, "one wheel event: {wheels:?}");
        let (unit, delta, modifiers) = wheels[0];
        assert_eq!(unit, egui::MouseWheelUnit::Point, "the pinch replay uses points");
        assert!((delta.y - 40.0).abs() < 1e-6, "points pass through: {delta:?}");
        assert!(modifiers.matches_any(egui::Modifiers::COMMAND), "Ctrl is held: {modifiers:?}");
        let changes: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                egui::Event::ModifiersChanged(m) => Some(*m),
                _ => None,
            })
            .collect();
        assert!(changes.iter().any(|m| m.matches_any(egui::Modifiers::COMMAND)), "Ctrl is set for the wheel: {changes:?}");
        assert_eq!(changes.last().copied(), Some(egui::Modifiers::NONE), "Ctrl is released after it: {changes:?}");
        app.synthetic.clear();

        let (req, _) = ControlRequest::new("ui.scroll", json!({"x":200,"y":300,"delta":40}));
        assert!(matches!(handle(&mut app, &ctx, &req), Outcome::Done(_)));
        let mut events = Vec::new();
        for _ in 0..2 {
            let mut raw = egui::RawInput::default();
            app.raw_input_hook(&mut raw);
            events.extend(raw.events);
        }
        let wheel = events.iter().find_map(|e| match e {
            egui::Event::MouseWheel { unit, delta, modifiers, .. } => Some((*unit, *delta, *modifiers)),
            _ => None,
        });
        let (unit, delta, modifiers) = wheel.expect("a wheel event");
        assert_eq!(unit, egui::MouseWheelUnit::Line, "without ctrl the wheel stays a mouse wheel");
        assert!((delta.y * ctx.options(|o| o.input_options.line_scroll_speed) - 40.0).abs() < 1e-3, "lines land on the same points: {delta:?}");
        assert!(!modifiers.matches_any(egui::Modifiers::COMMAND), "no modifier without ctrl");
        assert!(app.synthetic.is_empty());
    }

    #[test]
    fn scroll_rejects_invalid_input_without_queuing_events() {
        let mut app = SolveApp::new(Session::default(), Services::default());
        let ctx = crate::fonts::test_context();
        for invalid in
            [json!({"unit":"invalid"}), json!({"unit":42}), json!({"unit":null}), json!({"x":1e300}), json!({"y":-1e300}), json!({"delta":1e300})]
        {
            let mut params = json!({"x":200,"y":300,"delta":40,"ctrl":true});
            params.as_object_mut().unwrap().extend(invalid.as_object().unwrap().clone());
            let (req, _) = ControlRequest::new("ui.scroll", params.clone());
            let Outcome::Done(reply) = handle(&mut app, &ctx, &req) else { panic!("not a scroll reply") };
            assert_eq!(reply["ok"], false, "{params}: {reply}");
            assert!(app.synthetic.is_empty(), "a rejected scroll must not queue input: {params}");
        }
    }

    #[test]
    fn touchpad_drag_positioning_releases_previous_motion_modifiers() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.nav = "touchpad".into();
        h.call("ui.move", json!({"x":800,"y":500}));
        for modifier in ["shift", "alt"] {
            let mut params = json!({"x":800,"y":500});
            params[modifier] = json!(true);
            h.call("ui.move", params);
            let before = h.app.cam;
            h.call("ui.drag", json!({"x0":900,"y0":500,"x1":930,"y1":520,"shift":true,"steps":2}));
            assert_eq!(h.app.cam, before, "positioning a selection drag must not inherit {modifier} motion");
            assert!(!h.ctx.input(|i| i.pointer.any_down()));
        }
    }

    #[test]
    fn touchpad_text_prefix_does_not_collapse_queued_pointer_frames() {
        let mut app = SolveApp::new(Session::default(), Services::default());
        let ctx = crate::fonts::test_context();
        let (req, _) = ControlRequest::new("ui.text", json!({"text":"prefix"}));
        assert!(matches!(handle(&mut app, &ctx, &req), Outcome::Done(_)));
        for params in [json!({"x":10,"y":10,"shift":true}), json!({"x":20,"y":20,"alt":true}), json!({"x":30,"y":30})] {
            let (req, _) = ControlRequest::new("ui.move", params);
            assert!(matches!(handle(&mut app, &ctx, &req), Outcome::Done(_)));
        }
        for (p, shift, alt) in [(10.0, true, false), (20.0, false, true), (30.0, false, false)] {
            let mut raw = egui::RawInput::default();
            app.raw_input_hook(&mut raw);
            let mut out = ctx.run_ui(raw, |ui| {
                ui.input(|i| {
                    assert_eq!(i.pointer.hover_pos(), Some(egui::pos2(p, p)));
                    assert_eq!((i.modifiers.shift, i.modifiers.alt), (shift, alt));
                });
            });
            out.textures_delta.clear();
        }
        assert!(app.synthetic.is_empty());
    }

    #[test]
    fn touchpad_automation_preserves_each_queued_moves_modifiers() {
        let mut app = SolveApp::new(Session::default(), Services::default());
        let ctx = crate::fonts::test_context();
        for params in [json!({"x":10,"y":10,"shift":true}), json!({"x":20,"y":20,"alt":true}), json!({"x":30,"y":30})] {
            let (req, _) = ControlRequest::new("ui.move", params);
            assert!(matches!(handle(&mut app, &ctx, &req), Outcome::Done(_)));
        }
        for (p, shift, alt) in [(10.0, true, false), (20.0, false, true), (30.0, false, false)] {
            let mut raw = egui::RawInput::default();
            app.raw_input_hook(&mut raw);
            let mut out = ctx.run_ui(raw, |ui| {
                ui.input(|i| {
                    assert_eq!(i.pointer.hover_pos(), Some(egui::pos2(p, p)));
                    assert_eq!((i.modifiers.shift, i.modifiers.alt), (shift, alt));
                    assert!(!i.pointer.any_down());
                });
            });
            out.textures_delta.clear();
        }
        assert!(app.synthetic.is_empty());
    }

    /// #6646: `document.revision` names the active design and what it holds. The revision moves on
    /// an edit, a parameter, a hide, an undo and a redo, never on a read or a selection, and never
    /// comes back; the id stays through edits and is another for another tab of the same name.
    #[test]
    fn document_revision_follows_the_design_and_tells_tabs_apart() {
        fn now(h: &mut crate::scenario::Harness) -> (String, u64) {
            let r = h.call("document.revision", json!({}));
            assert_eq!(r["ok"], true, "{r}");
            (r["result"]["document_id"].as_str().unwrap_or_default().to_string(), r["result"]["revision"].as_u64().unwrap_or_default())
        }
        fn run(h: &mut crate::scenario::Harness, command: &str, params: Value) {
            let r = h.call("engine.execute", json!({"command": command, "params": params}));
            assert_eq!(r["ok"], true, "{command}: {r}");
        }
        let mut h = crate::scenario::Harness::new();
        let (id, mut last) = now(&mut h);
        assert!(!id.is_empty() && last > 0, "{id} {last}");
        // Reads and a selection leave it as it is (the view's own revision moves on a selection).
        let view = h.app.session.revision;
        h.call("document.inspect", json!({}));
        h.call("ui.inspect", json!({}));
        run(&mut h, "select.clear", json!({}));
        assert!(h.app.session.revision > view);
        assert_eq!(now(&mut h), (id.clone(), last));
        for (command, params) in [
            ("solid.box", json!({"length": 10, "width": 10, "height": 10})),
            ("parameters.add", json!({"name": "w", "expression": "12 mm"})),
            ("browser.visibility", json!({"folder": "bodies", "visible": false})),
            ("edit.undo", json!({})),
            ("edit.redo", json!({})),
        ] {
            run(&mut h, command, params);
            let (same, revision) = now(&mut h);
            assert_eq!(same, id, "{command} keeps the id");
            assert!(revision > last, "{command}: {revision} after {last}");
            assert_eq!(now(&mut h).1, revision, "asking again after {command}");
            last = revision;
        }
        // Another tab with the same name is another design; back on the first, its revision is
        // the one it had.
        h.call("ui.documents", json!({"action": "new"}));
        let tabs = h.call("ui.documents", json!({}))["result"]["documents"].clone();
        assert_eq!(tabs[0]["name"], tabs[1]["name"], "{tabs}");
        let (other, other_revision) = now(&mut h);
        assert_ne!(other, id);
        assert!(other_revision > last, "{other_revision} after {last}");
        h.call("ui.documents", json!({"action": "switch", "index": 0}));
        assert_eq!(now(&mut h), (id.clone(), last));
        // A design started in its place is another one too, and its revision is new.
        run(&mut h, "file.new", json!({}));
        let (fresh, revision) = now(&mut h);
        assert!(fresh != id && fresh != other, "{fresh}");
        assert!(revision > other_revision, "{revision} after {other_revision}");
    }

    /// #6985: while a connection is subscribed, a frame publishes the active design as
    /// `document.revision` names it: the same after a selection, a new revision after an edit,
    /// another id on another tab; nothing while none is. `control.subscribe` answers what
    /// `document.revision` does, and a connection that waited through several changes is told
    /// only of the latest.
    #[test]
    fn subscribed_frames_publish_the_design() {
        static WATCH: DesignWatch = DesignWatch::new();
        let now = || WATCH.now().clone();
        let mut h = crate::scenario::Harness::new();
        publish_design(&mut h.app, &WATCH);
        assert_eq!(now(), None, "nobody subscribed");
        let counted = WATCH.join();
        let subscribed = h.call("control.subscribe", json!({}));
        assert_eq!(subscribed["result"], h.call("document.revision", json!({}))["result"], "{subscribed}");
        publish_design(&mut h.app, &WATCH);
        let (id, first) = now().unwrap();
        assert_eq!(json!({"document_id": id, "revision": first}), subscribed["result"]);
        h.call("engine.execute", json!({"command": "select.clear", "params": {}}));
        publish_design(&mut h.app, &WATCH);
        assert_eq!(now(), Some((id.clone(), first)), "a selection is no change");
        h.call("engine.execute", json!({"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}));
        publish_design(&mut h.app, &WATCH);
        let (same, second) = now().unwrap();
        assert!(same == id && second > first, "{same} {second} after {first}");
        h.call("engine.execute", json!({"command": "edit.undo", "params": {}}));
        publish_design(&mut h.app, &WATCH);
        let third = now().unwrap().1;
        assert!(third > second, "{third} after {second}");
        let stop = AtomicBool::new(false);
        assert_eq!(WATCH.wait(&(id.clone(), first), &stop, Duration::ZERO), Some((id.clone(), third)));
        // New on a blank design reuses its tab: the box comes back first.
        h.call("engine.execute", json!({"command": "edit.redo", "params": {}}));
        h.call("ui.documents", json!({"action": "new"}));
        publish_design(&mut h.app, &WATCH);
        assert!(now().is_some_and(|(other, _)| other != id), "{:?}", now());
        drop(counted);
        assert!(!WATCH.subscribed());
    }

    fn render(app: &mut SolveApp, path: &std::path::Path) -> Vec<u8> {
        let (req, _rx) = ControlRequest::new("ui.render", json!({"path": path.to_string_lossy(), "width": 200, "height": 140}));
        match handle(app, &crate::fonts::test_context(), &req) {
            Outcome::Done(v) => assert_eq!(v["ok"], true, "{v}"),
            Outcome::Screenshot { .. } => panic!("ui.render took a screenshot"),
        }
        std::fs::read(path).unwrap()
    }

    /// `ui.render` draws in the window's theme (#974): its background is the viewport's, light
    /// or dark, and with the 3D view in the other theme's colours, the view's (#4146).
    #[test]
    fn model_render_is_in_the_windows_theme() {
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let mut app = SolveApp::new(s, Services::default());
        let dir = std::env::temp_dir().join(format!("solidcraft-render-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        use crate::theme::ViewportTheme;
        for (window, view) in
            [(false, ViewportTheme::Theme), (true, ViewportTheme::Theme), (true, ViewportTheme::Light), (false, ViewportTheme::Dark)]
        {
            app.ui.dark = window;
            app.preferences.viewport_theme = view;
            let dark = app.viewport_dark();
            let theme = format!("{window}-{}", view.id());
            let png = render(&mut app, &dir.join(format!("{theme}.png")));
            let img = image::load_from_memory(&png).unwrap().to_rgba8();
            // The top row is the background's top: its most common colour.
            let mut counts = std::collections::HashMap::new();
            for x in 0..img.width() {
                *counts.entry(img.get_pixel(x, 0).0).or_insert(0) += 1;
            }
            let top = counts.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c).unwrap();
            let want = if dark { crate::theme::Tokens::dark() } else { crate::theme::Tokens::light() }.viewport_top;
            let near = top.iter().zip([want.r(), want.g(), want.b()]).all(|(a, b)| a.abs_diff(b) <= 2);
            assert!(near, "{theme}: the render's top row is {top:?}, the view's background {want:?}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A model render shows the section the window shows: the open Section Analysis dialog's
    /// preview too, not only a committed `inspect.section`.
    #[test]
    fn model_render_shows_the_section_dialog_preview() {
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let mut app = SolveApp::new(s, Services::default());
        let dir = std::env::temp_dir().join(format!("solidcraft-render-section-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let whole = render(&mut app, &dir.join("whole.png"));
        app.session.selection = vec![Sel::Plane { name: "XZ".into() }];
        app.start("inspect.section");
        let Some(crate::dialogs::Kind::Section { offset, .. }) = app.dialog.as_mut().map(|d| &mut d.kind) else { panic!("no section dialog") };
        *offset = "15 mm".into();
        assert!(app.session.section.is_none() && crate::viewport::section_plane(&app).is_some());
        let preview = render(&mut app, &dir.join("preview.png"));
        assert!(whole != preview, "the model render ignores the section the window shows");
        // Committed, it renders the same.
        app.run("inspect.section", json!({"plane": "XZ", "offset": 15})).unwrap();
        app.dialog = None;
        assert!(render(&mut app, &dir.join("committed.png")) == preview, "the committed section renders differently");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn model_render_cosmetic_threads_preserve_section_hatch_preview() {
        let mut app = SolveApp::new(Session::default(), Services::default());
        for cosmetic_threads in [false, true] {
            app.preferences.cosmetic_threads = cosmetic_threads;
            for hatch in [false, true] {
                app.session.section_hatch = hatch;
                assert_eq!(view_scene(&app, Default::default()).plain_cap, !hatch);
                app.session.selection = vec![Sel::Plane { name: "XZ".into() }];
                app.start("inspect.section");
                let Some(crate::dialogs::Kind::Section { hatch: preview_hatch, .. }) = app.dialog.as_mut().map(|d| &mut d.kind) else {
                    panic!("no section dialog")
                };
                *preview_hatch = !hatch;
                assert_eq!(
                    view_scene(&app, Default::default()).plain_cap,
                    hatch,
                    "the preview's hatch overrides the session with cosmetic_threads={cosmetic_threads}"
                );
                app.dialog = None;
            }
        }
    }

    /// `ui.render` with extra request parameters.
    /// A `ui.render` of `app` to `path` with `extra` parameters: the PNG, and what drew it.
    fn render_with(app: &mut SolveApp, path: &std::path::Path, extra: serde_json::Value) -> (Vec<u8>, String) {
        let mut params = json!({"path": path.to_string_lossy(), "width": 200, "height": 140});
        for (k, v) in extra.as_object().into_iter().flatten() {
            params[k.as_str()] = v.clone();
        }
        let (req, _rx) = ControlRequest::new("ui.render", params);
        let renderer = match handle(app, &crate::fonts::test_context(), &req) {
            Outcome::Done(v) => {
                assert_eq!(v["ok"], true, "{v}");
                v["result"]["renderer"].as_str().or(v["renderer"].as_str()).unwrap_or("unknown").to_string()
            }
            Outcome::Screenshot { .. } => panic!("ui.render took a screenshot"),
        };
        (std::fs::read(path).unwrap(), renderer)
    }

    /// A model render draws the ground grid (it cuts through a part that straddles Z 0) and the
    /// origin's axes as the window does, by the Grid and Origin switches, and `grid` and `axes`
    /// in the request say otherwise for that render.
    #[test]
    fn model_render_follows_the_grid_and_origin_switches_unless_asked() {
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20, "corner": [10, -15, -10]})).unwrap();
        let mut app = SolveApp::new(s, Services::default());
        app.cam.set_view(solidcraft_engine::render::StandardView::Front);
        let dir = std::env::temp_dir().join(format!("solidcraft-render-guides-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shot.png");
        // Every picture is compared byte for byte, so each must come from the same renderer.
        let drawn_by = std::cell::RefCell::new(Vec::new());
        let shot = |app: &mut SolveApp, extra: serde_json::Value| {
            let (png, renderer) = render_with(app, &path, extra);
            drawn_by.borrow_mut().push(renderer);
            png
        };
        let all = shot(&mut app, json!({}));
        let no_grid = shot(&mut app, json!({"grid": false}));
        let no_axes = shot(&mut app, json!({"axes": false}));
        let bare = shot(&mut app, json!({"grid": false, "axes": false}));
        let pictures = [&all, &no_grid, &no_axes, &bare];
        for (i, a) in pictures.iter().enumerate() {
            for b in &pictures[i + 1..] {
                assert!(a != b, "two of the pictures are the same");
            }
        }
        // A picture that is not the one expected: both are kept, with how much they differ, so a
        // failure that does not come back on a rerun still leaves its evidence.
        let same = |step: &str, got: Vec<u8>, want: &Vec<u8>| {
            if got == *want {
                return;
            }
            let kept = std::env::temp_dir().join(format!("solidcraft-render-guides-failed-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&kept);
            let _ = std::fs::write(kept.join(format!("{step}-got.png")), &got);
            let _ = std::fs::write(kept.join(format!("{step}-want.png")), want);
            let differ = match (image::load_from_memory(&got), image::load_from_memory(want)) {
                (Ok(a), Ok(b)) if a.width() == b.width() && a.height() == b.height() => {
                    let (a, b) = (a.to_rgba8(), b.to_rgba8());
                    format!("{} of {} pixels differ", a.pixels().zip(b.pixels()).filter(|(x, y)| x != y).count(), a.width() * a.height())
                }
                (Ok(a), Ok(b)) => format!("{}×{} against {}×{}", a.width(), a.height(), b.width(), b.height()),
                _ => "a picture does not decode".to_string(),
            };
            panic!("{step}: not the picture expected ({differ}; drawn by {:?}); both kept in {}", drawn_by.borrow(), kept.display());
        };
        // With the Grid switch off the render has no grid, and `grid: true` brings it back.
        app.ui.show_grid = false;
        same("grid-off", shot(&mut app, json!({})), &no_grid);
        same("grid-off-asked-on", shot(&mut app, json!({"grid": true})), &all);
        // With the Origin off too it has neither; `axes: true` puts the axes back.
        app.set_origin_shown(false);
        same("both-off", shot(&mut app, json!({})), &bare);
        same("both-off-axes-asked-on", shot(&mut app, json!({"axes": true})), &no_grid);
        // The Origin and the Grid back on: the first picture again.
        app.ui.show_grid = true;
        app.set_origin_shown(true);
        same("both-on-again", shot(&mut app, json!({})), &all);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn batch(h: &mut crate::scenario::Harness, params: serde_json::Value) -> serde_json::Value {
        h.call("engine.batch", params)
    }

    fn inspect(h: &mut crate::scenario::Harness) -> serde_json::Value {
        h.call("document.inspect", json!({}))["result"].clone()
    }

    /// Timing fields (`ms`) differ from run to run.
    fn without_timings(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(o) => o.iter().filter(|(k, _)| *k != "ms").map(|(k, v)| (k.clone(), without_timings(v))).collect(),
            serde_json::Value::Array(a) => a.iter().map(without_timings).collect(),
            v => v.clone(),
        }
    }

    /// #743: a batch is one request through the app's queue, answered within a frame, however
    /// many steps it has.
    #[test]
    fn engine_batch_runs_in_one_frame() {
        let mut h = crate::scenario::Harness::new();
        let steps: Vec<_> = (0..40)
            .flat_map(|_| [json!({"command": "sketch.create", "params": {"plane": "XY"}}), json!({"command": "sketch.finish", "params": {}})])
            .collect();
        let (v, frames) = h.call_queued("engine.batch", json!({"commands": steps}));
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["result"]["completed"], 80, "{v}");
        assert_eq!(v["result"]["results"].as_array().map(Vec::len), Some(80));
        assert!(v["result"].get("failed").is_none(), "{v}");
        assert!(frames <= 2, "a batch of 80 steps took {frames} frames");
        assert_eq!(inspect(&mut h)["sketches"].as_array().map(Vec::len), Some(40));
    }

    /// #5822: `engine.batch {coalesce_parameters}` sets a run of parameter edits together, one
    /// rebuild, and answers each step; a refused edit is named by its own step and the batch goes
    /// back. Unasked, each edit runs on its own as before.
    #[test]
    fn engine_batch_coalesces_parameter_edits_when_asked() {
        let mut h = crate::scenario::Harness::new();
        let build = json!([
            {"command": "parameters.add", "params": {"name": "w", "expression": "10 mm"}},
            {"command": "parameters.add", "params": {"name": "d", "expression": "10 mm"}},
            {"command": "solid.box", "params": {"length": "w", "width": "d", "height": 10}},
        ]);
        assert_eq!(batch(&mut h, json!({"commands": build}))["ok"], true);
        let edits = json!([
            {"command": "parameters.change", "params": {"name": "w", "expression": "20 mm"}},
            {"command": "parameters.change", "params": {"name": "d", "expression": "30 mm"}},
        ]);
        let v = batch(&mut h, json!({"commands": edits, "coalesce_parameters": true}));
        let results = v["result"]["results"].as_array().unwrap();
        assert_eq!(results.len(), 2, "{v}");
        assert!(results.iter().all(|r| r["coalesced"] == 2), "{v}");
        assert_eq!(results[1]["value"], 30.0, "{v}");
        let volume = |h: &mut crate::scenario::Harness| {
            h.call("engine.execute", json!({"command": "inspect.measure", "params": {}}))["result"]["total"]["volume_mm3"].as_f64()
        };
        assert!(volume(&mut h).is_some_and(|v| (v - 6000.0).abs() < 1e-6));
        let bad = json!([
            {"command": "parameters.change", "params": {"name": "w", "expression": "5 mm"}},
            {"command": "parameters.change", "params": {"name": "d", "expression": "1 +"}},
        ]);
        let v = batch(&mut h, json!({"commands": bad, "coalesce_parameters": true}));
        assert_eq!(v["result"]["failed"]["index"], 1, "{v}");
        assert_eq!(v["result"]["rolled_back"], true, "{v}");
        assert!(volume(&mut h).is_some_and(|v| (v - 6000.0).abs() < 1e-6), "the batch went back");
        // Unasked: one result per edit, none coalesced.
        let v = batch(&mut h, json!({"commands": edits}));
        assert!(v["result"]["results"].as_array().unwrap().iter().all(|r| r.get("coalesced").is_none()), "{v}");
    }

    /// #1833: `engine.batch` times its steps where they run, the failing one included, with the
    /// kernel's share when `profile` asks; unasked, the reply is as before.
    #[test]
    fn engine_batch_times_its_steps() {
        let mut h = crate::scenario::Harness::new();
        let steps = json!([
            {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}},
            {"command": "solid.box", "params": {"length": 4, "width": 4, "height": 30, "operation": "join"}},
        ]);
        let v = batch(&mut h, json!({"commands": steps, "profile": true}));
        let rows = v["result"]["timings"].as_array().cloned().unwrap_or_default();
        assert_eq!(rows.len(), 2, "{v}");
        assert_eq!(rows[1]["index"], 1);
        assert!(rows[1]["profile"]["boolean_ms"].as_f64().is_some_and(|ms| ms > 0.0), "{v}");
        assert!(v["result"]["total_ms"].as_f64().is_some(), "{v}");
        // A step that fails at its turn is timed too.
        let v = batch(&mut h, json!({"timings": true, "commands": [{"command": "solid.extrude", "params": {"distance": "bogus +"}}]}));
        assert_eq!(v["result"]["timings"].as_array().map(Vec::len), Some(1), "{v}");
        assert!(v["result"]["timings"][0].get("profile").is_none(), "{v}");
        let v = batch(&mut h, json!({"commands": [{"command": "solid.box", "params": {"length": 1, "width": 1, "height": 1}}]}));
        assert!(v["result"].get("timings").is_none() && v["result"].get("total_ms").is_none(), "{v}");
    }

    #[test]
    fn engine_batch_rolls_back_at_the_first_failure() {
        let mut h = crate::scenario::Harness::new();
        assert_eq!(h.call("engine.execute", json!({"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}))["ok"], true);
        let before = inspect(&mut h);
        let v = batch(
            &mut h,
            json!({"commands": [
                {"command": "solid.sphere", "params": {"radius": 3, "center": [50, 0, 0]}},
                {"command": "solid.extrude", "params": {"distance": "bogus +"}},
                {"command": "solid.sphere", "params": {"radius": 3, "center": [80, 0, 0]}},
            ]}),
        );
        assert_eq!(v["ok"], true, "{v}");
        let r = &v["result"];
        assert_eq!((r["completed"].clone(), r["failed"]["index"].clone(), r["undone"].clone()), (json!(1), json!(1), json!(1)), "{v}");
        assert_eq!(r["failed"]["command"], "solid.extrude");
        let after = inspect(&mut h);
        assert_eq!(after["undo"], before["undo"], "{after}");
        assert_eq!(after["bodies"], before["bodies"]);
        assert_eq!(without_timings(&after["timeline"]), without_timings(&before["timeline"]));
    }

    #[test]
    fn engine_batch_without_rollback_keeps_completed_steps() {
        let mut h = crate::scenario::Harness::new();
        let v = batch(
            &mut h,
            json!({"rollback": false, "commands": [
                {"command": "solid.sphere", "params": {"radius": 3}},
                // Fails at its turn (an unknown command would be refused before the first step, #5853).
                {"command": "solid.extrude", "params": {"distance": "bogus +"}},
            ]}),
        );
        assert_eq!(v["result"]["failed"]["index"], 1, "{v}");
        assert_eq!(v["result"]["undone"], 0, "{v}");
        assert_eq!(inspect(&mut h)["bodies"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn engine_batch_step_without_a_command_fails_that_step() {
        let mut h = crate::scenario::Harness::new();
        let v = batch(&mut h, json!({"commands": [{"params": {}}]}));
        assert_eq!(v["result"]["failed"]["index"], 0, "{v}");
        assert!(v["result"]["failed"]["error"].as_str().is_some_and(|e| e.contains("missing `command`")), "{v}");
    }

    /// #5853: a step the batch would refuse at its turn (an unknown command; with strict
    /// parameter checking, a call that breaks its command's contract) refuses the batch before
    /// its first step runs, however late the step is.
    #[test]
    fn engine_batch_refuses_a_bad_step_before_the_first_runs() {
        let mut h = crate::scenario::Harness::new();
        let before = inspect(&mut h);
        let v = batch(
            &mut h,
            json!({"commands": [
                {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}},
                {"command": "solid.sphere", "params": {"radius": 3, "center": [50, 0, 0]}},
                {"command": "solid.extrud", "params": {"distance": 5}},
            ]}),
        );
        assert_eq!(v["ok"], true, "{v}");
        let r = &v["result"];
        assert_eq!((r["completed"].clone(), r["failed"]["index"].clone(), r["undone"].clone()), (json!(0), json!(2), json!(0)), "{v}");
        assert!(r["failed"]["error"].as_str().is_some_and(|e| e.starts_with("unknown command `solid.extrud`") && e.contains("solid.extrude")), "{v}");
        assert_eq!(r["not_run"], json!([0, 1, 2]), "{v}");
        let after = inspect(&mut h);
        assert_eq!((after["bodies"].clone(), after["undo"].clone()), (before["bodies"].clone(), before["undo"].clone()), "nothing ran: {after}");
        // Strict checking refuses a broken contract up front too; off, the step runs (and the
        // box command reads what it knows).
        h.app.session.param_check = solidcraft_engine::schema::ParamCheck::Strict;
        let steps = json!([
            {"command": "solid.sphere", "params": {"radius": 3}},
            {"command": "solid.box", "params": {"lenght": 10, "width": 10, "height": 10}},
        ]);
        let v = batch(&mut h, json!({"commands": steps}));
        assert_eq!((v["result"]["completed"].clone(), v["result"]["failed"]["index"].clone()), (json!(0), json!(1)), "{v}");
        assert!(v["result"]["failed"]["error"].as_str().is_some_and(|e| e.contains("lenght")), "{v}");
        assert_eq!(inspect(&mut h)["bodies"], before["bodies"], "nothing ran");
        // `view.pick` is answered by the app, outside the command registry: not refused.
        h.app.session.param_check = solidcraft_engine::schema::ParamCheck::Off;
        let v = batch(&mut h, json!({"commands": [{"command": "view.pick", "params": {"x": 1, "y": 1}}]}));
        assert!(!v["result"]["failed"]["error"].as_str().is_some_and(|e| e.starts_with("unknown command")), "{v}");
    }

    /// A request's own `param_check` (#9184): `engine.execute`, `engine.batch` and `engine.script`
    /// run at it and say so in the reply, whatever the app's session level, which is its own again
    /// afterwards. A command that switches designs keeps the app's level.
    #[test]
    fn a_request_carries_its_own_param_check_level() {
        use solidcraft_engine::schema::ParamCheck;
        let mut h = crate::scenario::Harness::new();
        let before = inspect(&mut h);
        let warning = "unknown key `operaton`; did you mean `operation`?";
        let misspelt = |level: &str| json!({"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10, "operaton": "cut"}, "param_check": level});

        let v = h.call("engine.execute", misspelt("strict"));
        assert_eq!((&v["ok"], &v["param_check"]), (&json!(false), &json!("strict")), "{v}");
        assert!(v["error"].as_str().is_some_and(|e| e.contains(warning)), "{v}");
        assert_eq!(inspect(&mut h)["bodies"], before["bodies"], "refused unrun");
        assert_eq!(h.app.session.param_check, ParamCheck::Off, "the app's own level again");

        let v = h.call("engine.execute", misspelt("warn"));
        assert_eq!((&v["ok"], &v["param_check"], &v["result"]["param_warnings"]), (&json!(true), &json!("warn"), &json!([warning])), "{v}");
        assert_eq!(h.app.session.param_check, ParamCheck::Off);

        let steps = json!([
            {"command": "solid.sphere", "params": {"radius": 3, "center": [50, 0, 0]}},
            {"command": "solid.box", "params": {"lenght": 10, "width": 10, "height": 10}},
        ]);
        let shown = inspect(&mut h)["bodies"].clone();
        let v = h.call("engine.batch", json!({"commands": steps, "param_check": "strict"}));
        assert_eq!((&v["result"]["completed"], &v["result"]["failed"]["index"], &v["param_check"]), (&json!(0), &json!(1), &json!("strict")), "{v}");
        assert_eq!(inspect(&mut h)["bodies"], shown, "nothing ran");
        let step = json!({"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10, "operaton": "cut"}});
        let v = h.call("engine.script", json!({"commands": [step], "param_check": "warn"}));
        assert_eq!((&v["param_check"], &v["result"][0]["param_warnings"]), (&json!("warn"), &json!([warning])), "{v}");

        // Asked nothing, told nothing; a word that is no level is refused.
        let v = h.call("engine.execute", json!({"command": "inspect.measure", "params": {}}));
        assert_eq!((&v["ok"], v.get("param_check")), (&json!(true), None), "{v}");
        let v = h.call("engine.execute", json!({"command": "inspect.measure", "params": {}, "param_check": "loud"}));
        assert!(v["ok"] == false && v["error"].as_str().is_some_and(|e| e.contains("`param_check` must be off, warn or strict")), "{v}");
        // The level is this request's, not another design's.
        let v = h.call("engine.execute", json!({"command": "view.next_tab", "params": {}, "param_check": "strict"}));
        assert_eq!((&v["ok"], &v["param_check"]), (&json!(true), &json!("strict")), "{v}");
        assert_eq!(h.app.session.param_check, ParamCheck::Off);
        // Its own parameters are still checked at the request's level, before it runs.
        let v = h.call("engine.execute", json!({"command": "view.next_tab", "params": {"nonsense": 1}, "param_check": "strict"}));
        assert!(v["ok"] == false && v["error"].as_str().is_some_and(|e| e.contains("nonsense")), "{v}");
        assert_eq!(v["param_check"], "strict", "{v}");
        let v = h.call("engine.execute", json!({"command": "view.next_tab", "params": {"nonsense": 1}, "param_check": "warn"}));
        assert!(v["result"]["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{v}");
        assert_eq!(h.app.session.param_check, ParamCheck::Off);
        // Other methods ignore the field, whatever it holds.
        let v = h.call("ui.inspect", json!({"param_check": "loud"}));
        assert_eq!((&v["ok"], v.get("param_check")), (&json!(true), None), "{v}");
    }

    #[test]
    fn engine_batch_is_refused_when_a_step_loads_a_file() {
        let mut h = crate::scenario::Harness::new();
        let v = batch(
            &mut h,
            json!({"commands": [
                {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}},
                {"command": "doc.open", "params": {"path": "elsewhere.solidcraft"}},
            ]}),
        );
        assert_eq!(v["ok"], false, "{v}");
        assert_eq!(v["error"], "engine.batch unavailable: step 1 (doc.open) loads a file", "{v}");
        assert_eq!(inspect(&mut h)["bodies"].as_array().map(Vec::len), Some(0), "nothing may run before the refusal");
    }

    #[test]
    fn engine_batch_runs_while_the_agent_cursor_is_shown() {
        let mut h = crate::scenario::Harness::new();
        assert_eq!(h.call("ui.agent_cursor", json!({"show": true}))["ok"], true);
        let v = batch(&mut h, json!({"commands": [{"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}]}));
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["result"]["completed"], 1, "{v}");
        assert_eq!(inspect(&mut h)["bodies"].as_array().map(Vec::len), Some(1));
    }

    /// A failed batch says whether the design really went back (`rolled_back`), not just whether
    /// it was asked to.
    #[test]
    fn engine_batch_reports_whether_it_rolled_back() {
        let mut h = crate::scenario::Harness::new();
        let steps = json!([{"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}, {"command": "solid.extrude", "params": {"distance": "bogus +"}}]);
        let v = batch(&mut h, json!({"commands": steps.clone()}));
        assert_eq!((&v["result"]["rolled_back"], &v["result"]["undone"]), (&json!(true), &json!(1)), "{v}");
        let v = batch(&mut h, json!({"commands": steps, "rollback": false}));
        assert_eq!((&v["result"]["rolled_back"], &v["result"]["undone"]), (&json!(false), &json!(0)), "{v}");
        assert_eq!(inspect(&mut h)["bodies"].as_array().map(Vec::len), Some(1));
    }

    /// Every step of a batch from an MCP server stays inside the agent's folders (#205), as an
    /// `engine.execute` would.
    #[test]
    fn engine_batch_stays_inside_allow_paths() {
        let mut h = crate::scenario::Harness::new();
        let base = vfs::temp_dir().join(format!("solidcraft-batch-scope-{}", std::process::id()));
        let (root, out) = (base.join("root"), base.join("out"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let export = |d: &std::path::Path| json!({"command": "file.export", "params": {"path": d.join("a.step").to_string_lossy()}});
        let v = batch(
            &mut h,
            json!({"allow_paths": [root.to_string_lossy()], "commands": [
                {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}},
                export(&root),
                export(&out),
            ]}),
        );
        assert_eq!(v["result"]["failed"]["index"], 2, "{v}");
        assert!(v["result"]["failed"]["error"].as_str().is_some_and(|e| e.contains("outside the folders this agent may use")), "{v}");
        assert!(root.join("a.step").exists());
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0, "nothing written outside");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// An MCP server's `doc.open` and imports load off the window thread (`file_load`), where
    /// the agent's folders hold as they do for every other request.
    #[test]
    fn file_loads_stay_inside_allow_paths() {
        let mut h = crate::scenario::Harness::new();
        let base = vfs::temp_dir().join(format!("solidcraft-load-scope-{}", std::process::id()));
        let (root, out) = (base.join("root"), base.join("out"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        for d in [&root, &out] {
            std::fs::write(d.join("part.solidcraft"), solidcraft_engine::io::write_design(&s.doc)).unwrap();
        }
        s.execute("file.export", &json!({"path": out.join("part.step").to_string_lossy()})).unwrap();
        let load = |command: &str, d: &std::path::Path, file: &str| json!({"command": command, "params": {"path": d.join(file).to_string_lossy()}, "allow_paths": [root.to_string_lossy()]});
        for (command, file) in [("doc.open", "part.solidcraft"), ("file.insert_step", "part.step")] {
            let (v, _) = h.call_queued("engine.execute", load(command, &out, file));
            assert!(v["ok"] == false && v.to_string().contains("outside the folders this agent may use"), "{command}: {v}");
        }
        let (v, _) = h.call_queued("engine.execute", load("doc.open", &root, "part.solidcraft"));
        assert_eq!(v["ok"], true, "{v}");
        let (v, _) = h.call_queued("engine.execute", json!({"command": "doc.open", "params": {"path": "x"}, "allow_paths": "/"}));
        assert!(v["error"].as_str().is_some_and(|e| e.contains("allow_paths")), "{v}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Each step runs as `engine.execute` runs it: same results, same design.
    #[test]
    fn engine_batch_matches_engine_execute() {
        let steps = [
            json!({"command": "sketch.create", "params": {"plane": "XY"}}),
            json!({"command": "sketch.rectangle.two_point", "params": {"p0": [0, 0], "p1": [40, 30]}}),
            json!({"command": "sketch.finish", "params": {}}),
            json!({"command": "solid.extrude", "params": {"distance": 20}}),
        ];
        let mut a = crate::scenario::Harness::new();
        let batched = batch(&mut a, json!({"commands": steps}));
        let mut b = crate::scenario::Harness::new();
        let one_by_one: Vec<_> = steps.iter().map(|s| b.call("engine.execute", s.clone())["result"].clone()).collect();
        assert_eq!(without_timings(&batched["result"]["results"]), without_timings(&json!(one_by_one)), "{batched}");
        assert_eq!(without_timings(&inspect(&mut a)["timeline"]), without_timings(&inspect(&mut b)["timeline"]));
        assert_eq!(a.app.cam, b.app.cam, "the first-body fit must happen as for engine.execute");
    }

    /// A request carrying `allow_paths` (from an MCP server, #205) reaches files only inside
    /// those folders; the same request without it, like the user's own dialogs, is unconfined.
    #[test]
    fn requests_with_allow_paths_stay_inside_them() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let mut app = SolveApp::new(s, Services::default());
        let base = vfs::temp_dir().join(format!("solidcraft-control-scope-{}", std::process::id()));
        let (root, out) = (base.join("root"), base.join("out"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let at = |d: &std::path::Path, f: &str| d.join(f).to_string_lossy().to_string();
        let mut call = |method: &str, mut params: Value, scoped: bool| {
            if scoped {
                params["allow_paths"] = json!([root.to_string_lossy()]);
            }
            let (req, _rx) = ControlRequest::new(method, params);
            match handle(&mut app, &crate::fonts::test_context(), &req) {
                Outcome::Done(v) => v,
                Outcome::Screenshot { path } => json!({"ok": true, "screenshot": path}),
            }
        };
        let export = |p: String| json!({"command": "file.export", "params": {"path": p}});
        assert_eq!(call("engine.execute", export(at(&root, "a.step")), true)["ok"], true);
        if crate::gpu::test_picture_renderer() {
            assert_eq!(call("ui.render", json!({"path": at(&root, "a.png"), "width": 32, "height": 32}), true)["ok"], true);
        }
        assert_eq!(call("ui.screenshot", json!({"path": at(&root, "b.png")}), true)["ok"], true);
        for (method, params) in [
            ("engine.execute", export(at(&out, "a.step"))),
            ("engine.execute", export(at(&root, "../out/a.step"))),
            ("engine.script", json!({"commands": [{"command": "file.export", "params": {"path": at(&out, "a.step")}}]})),
            ("ui.render", json!({"path": at(&out, "a.png"), "width": 32, "height": 32})),
            ("ui.screenshot", json!({"path": at(&out, "b.png")})),
        ] {
            let v = call(method, params, true);
            assert!(v["ok"] == false && v.to_string().contains("outside the folders this agent may use"), "{method}: {v}");
        }
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0, "nothing written outside");
        assert_eq!(call("engine.execute", export(at(&out, "a.step")), false)["ok"], true, "without allow_paths: unconfined");
        let bad = call("engine.execute", json!({"command": "file.new", "allow_paths": "/"}), false);
        assert!(bad["error"].as_str().is_some_and(|e| e.contains("allow_paths")), "{bad}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A screenshot is written through `vfs`, as other saves are, so the web app can hand it out
    /// as a download (#7192). On the disk it is the same PNG with the same reply. A folder scope
    /// active while it is written refuses a path outside it, which shows the write goes through
    /// `vfs`. (`ui.screenshot` checks an agent's folders when the request arrives; the write comes
    /// in a later frame, outside the request's scope, as before.)
    #[test]
    fn screenshots_are_written_through_vfs_with_the_same_reply() {
        let base = vfs::temp_dir().join(format!("solidcraft-shot-scope-{}", std::process::id()));
        let (root, out) = (base.join("root"), base.join("out"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let image = egui::ColorImage::new([4, 3], vec![egui::Color32::GREEN; 12]);
        let path = root.join("shot.png").to_string_lossy().to_string();
        assert_eq!(save_screenshot(&image, Some(&path)), json!({"ok": true, "result": {"path": path, "width": 4, "height": 3}}));
        let png = image::load_from_memory_with_format(&std::fs::read(&path).unwrap(), image::ImageFormat::Png).unwrap().into_rgba8();
        assert_eq!((png.dimensions(), png.get_pixel(0, 0).0), ((4, 3), [0, 255, 0, 255]));
        let outside = out.join("shot.png").to_string_lossy().to_string();
        let v = vfs::scoped(Some(vfs::PathScope::new([root.clone()])), || save_screenshot(&image, Some(&outside)));
        assert!(v["ok"] == false && v["error"].as_str().is_some_and(|e| e.contains("outside the folders this agent may use")), "{v}");
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0, "nothing written outside");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Without a `path`, a screenshot goes to `vfs::temp_dir`: in the web app a folder kept in
    /// memory, where std's temporary folder panics (#7192).
    #[test]
    fn a_screenshot_without_a_path_goes_to_the_vfs_temporary_folder() {
        let image = egui::ColorImage::new([4, 3], vec![egui::Color32::GREEN; 12]);
        let path = vfs::temp_dir().join("solidcraft-shot.png");
        let reply = save_screenshot(&image, None);
        assert_eq!(reply, json!({"ok": true, "result": {"path": path.to_string_lossy(), "width": 4, "height": 3}}));
        assert!(path.is_file());
    }

    /// A request's `design_folders` (where its MCP agent opened or saved a design, #7181) count
    /// as the open design's folder does: their files are reachable, their hidden files and the
    /// folders beside them are not, and a list that is not one of at most
    /// `vfs::MAX_DESIGN_FOLDERS` folders is refused.
    #[test]
    fn a_requests_design_folders_count_as_the_open_designs() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let mut app = SolveApp::new(s, Services::default());
        let base = vfs::temp_dir().join(format!("solidcraft-control-design-folders-{}", std::process::id()));
        let (root, proj, out) = (base.join("root"), base.join("proj"), base.join("out"));
        for d in [&root, &proj.join(".git"), &out] {
            std::fs::create_dir_all(d).unwrap();
        }
        let at = |d: &std::path::Path, f: &str| d.join(f).to_string_lossy().to_string();
        let mut call = |path: String, designs: Value| {
            let params =
                json!({"command": "file.export", "params": {"path": path}, "allow_paths": [root.to_string_lossy()], "design_folders": designs});
            let (req, _rx) = ControlRequest::new("engine.execute", params);
            match handle(&mut app, &crate::fonts::test_context(), &req) {
                Outcome::Done(v) => v,
                Outcome::Screenshot { path } => json!({"ok": true, "screenshot": path}),
            }
        };
        let designs = json!([proj.to_string_lossy()]);
        assert_eq!(call(at(&proj, "a.step"), designs.clone())["ok"], true);
        for p in [at(&proj, ".secret.step"), at(&proj, ".git/a.step"), at(&out, "a.step")] {
            let v = call(p.clone(), designs.clone());
            assert!(v["ok"] == false && v.to_string().contains("outside the folders this agent may use"), "{p}: {v}");
        }
        assert!(!proj.join(".secret.step").exists() && !proj.join(".git/a.step").exists());
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0, "nothing written beside it");
        let too_many = json!(vec![proj.to_string_lossy(); vfs::MAX_DESIGN_FOLDERS + 1]);
        for bad in [json!(proj.to_string_lossy()), json!([1]), too_many] {
            let v = call(at(&proj, "b.step"), bad);
            assert!(v["error"].as_str().is_some_and(|e| e.contains("design_folders")), "{v}");
        }
        assert!(!proj.join("b.step").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// `ui.camera` reads the camera and sets it; its reply, passed back, sets the same view
    /// again. Fields left out keep their values, `fit` frames occurrences, and a refused field
    /// changes nothing.
    #[test]
    fn ui_camera_reads_sets_and_replays_the_view() {
        let mut s = Session::default();
        for (name, x) in [("A", 0), ("B", 100)] {
            s.execute("component.activate", &json!({"component": "root"})).unwrap();
            s.execute("component.create", &json!({ "name": name })).unwrap();
            s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]})).unwrap();
        }
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let mut h = crate::scenario::Harness::new();
        h.app = SolveApp::new(s, Services::default());
        h.frames(2);
        let before = h.app.cam;
        // Reading during an animated view change leaves it running (no frames run between).
        fn raw(h: &mut crate::scenario::Harness, method: &str, params: Value) -> Value {
            let (req, _rx) = ControlRequest::new(method, params);
            match handle(&mut h.app, &h.ctx, &req) {
                Outcome::Done(v) => v,
                Outcome::Screenshot { .. } => Value::Null,
            }
        }
        raw(&mut h, "ui.view", json!({"view": "top", "animate": true}));
        let r = raw(&mut h, "ui.camera", Value::Null);
        assert!(r["ok"] == true && h.app.cam_anim.is_some(), "{r}");
        // Setting it, even to where it is now, ends the animation there.
        let yaw = h.app.cam.yaw;
        let r = raw(&mut h, "ui.camera", json!({ "yaw": yaw }));
        assert!(r["ok"] == true && h.app.cam_anim.is_none(), "{r}");
        h.app.cam = before;
        let mut cam = |p: Value| {
            let r = h.call("ui.camera", p);
            assert_eq!(r["ok"], true, "{r}");
            (r["result"].clone(), h.app.cam)
        };
        let (start, now) = cam(json!({}));
        assert_eq!(start, json!(now));
        let (set, _) = cam(json!({"target": [5, 6, 7], "yaw": 0.5, "pitch": -0.25, "distance": 300}));
        assert_eq!(
            (set["target"].clone(), set["yaw"].clone(), set["pitch"].clone(), set["distance"].clone()),
            (json!([5.0, 6.0, 7.0]), json!(0.5), json!(-0.25), json!(300.0))
        );
        assert_eq!(set["roll"], start["roll"], "a field left out keeps its value");
        let (top, _) = cam(json!({"pitch": std::f64::consts::FRAC_PI_2}));
        assert!(top["pitch"].as_f64().unwrap() < std::f64::consts::FRAC_PI_2, "straight down keeps a heading: {top}");
        let (again, now) = cam(set.clone());
        assert_eq!((again, json!(now)), (set.clone(), set.clone()), "the reply sets the same view again");
        let (fit, _) = cam(json!({"fit": ["B"]}));
        let t: Vec<f64> = fit["target"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        // Framed as Fit frames (the viewport's panels shift the target a little): on B, not A.
        assert!((100.0..120.0).contains(&t[0]) && (0.0..20.0).contains(&t[1]) && (0.0..10.0).contains(&t[2]), "{fit}");
        assert_eq!(fit["yaw"], set["yaw"], "fit keeps the direction");
        let fov = h.app.cam.fov + 0.25;
        for bad in [
            json!({"zoom": 2}),
            json!({"distance": -1}),
            json!({"distance": 1e300}),
            json!({"pitch": 2}),
            json!({"yaw": "x"}),
            json!({"fov": fov}),
            json!({"up": "y"}),
            json!({"yaw": 1, "fit": ["Nope"]}),
            json!({"fit": []}),
            json!({"fit": 1}),
            json!([1, 2]),
        ] {
            let before = h.app.cam;
            let r = h.call("ui.camera", bad.clone());
            assert_eq!(r["ok"], false, "{bad}: {r}");
            assert_eq!(h.app.cam, before, "{bad}");
        }
        // Nothing shown: nothing to fit.
        let mut empty = crate::scenario::Harness::new();
        let before = empty.app.cam;
        let r = empty.call("ui.camera", json!({"yaw": 1, "fit": true}));
        assert!(r["ok"] == false && empty.app.cam == before, "{r}");
    }

    /// With Perspective view on, `ui.camera` reports the angle the window draws with (#5364) and
    /// its reply replays; another angle is refused, naming the preference. A sketch draws parallel.
    #[test]
    fn ui_camera_reports_the_drawn_angle_and_its_reply_replays() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.set", json!({"perspective": true}));
        h.call("ui.prefs", json!({"perspective_fov": 1.0}));
        let read = h.call("ui.camera", Value::Null);
        assert_eq!(read["result"]["fov"], json!(1.0), "{read}");
        let stored = h.app.cam.fov;
        let again = h.call("ui.camera", read["result"].clone());
        assert_eq!(again["result"], read["result"], "the reply replays");
        assert_eq!(h.app.cam.fov, stored, "the camera's own angle is left as it was");
        let refused = h.call("ui.camera", json!({"fov": 0.6}));
        assert!(refused["ok"] == false && refused["error"].as_str().unwrap().contains("ui.prefs {perspective_fov}"), "{refused}");
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        assert_eq!(h.call("ui.camera", Value::Null)["result"]["fov"], json!(0.0), "a sketch draws parallel");
    }

    /// `ui.start` params set what a dialog opens on: Drive Joints on a joint (by name or id) at
    /// the given values, driving nothing yet. A param it doesn't take is refused and closes it.
    #[test]
    fn ui_start_params_open_drive_joints_where_asked() {
        let mut s = Session::default();
        for (name, x) in [("A", 0), ("B", 30), ("C", 60)] {
            s.execute("component.activate", &json!({"component": "root"})).unwrap();
            s.execute("component.create", &json!({ "name": name })).unwrap();
            s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]})).unwrap();
        }
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        s.execute("occurrence.ground", &json!({"occurrence": "A:1", "grounded": true})).unwrap();
        let at = |o: &str, x: i32| json!({"occurrence": o, "point": [x, 10, 5], "z": [1, 0, 0]});
        s.execute("joint.create", &json!({"type": "revolute", "a": at("A", 25), "b": at("B", 25), "flip": true, "name": "Hinge"})).unwrap();
        s.execute("joint.create", &json!({"type": "slider", "a": at("B", 55), "b": at("C", 55), "flip": true, "name": "Rail"})).unwrap();
        let mut h = crate::scenario::Harness::new();
        h.app = SolveApp::new(s, Services::default());
        let before = h.app.session.doc.clone();
        let drive = |h: &crate::scenario::Harness| match h.app.dialog.as_ref().map(|d| &d.kind) {
            Some(crate::dialogs::Kind::Assembly(crate::dialogs_assembly::Asm::Drive { joint, values, .. })) => {
                (h.app.session.doc.assembly.joints[*joint].name.clone(), values.clone())
            }
            other => panic!("{other:?}"),
        };
        let r = h.call("ui.start", json!({"command": "joint.drive", "params": {"joint": "Rail", "values": [12.5]}}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(drive(&h), ("Rail".to_string(), vec!["12.5 mm".to_string()]));
        let hinge = h.app.session.doc.assembly.joints[0].id;
        h.call("ui.start", json!({"command": "joint.drive", "params": {"joint": hinge, "values": ["45 deg"]}}));
        assert_eq!(drive(&h), ("Hinge".to_string(), vec!["45 deg".to_string()]));
        assert_eq!(h.app.session.doc, before, "nothing is driven until OK");
        for bad in [json!({"joint": "Nope"}), json!({"joint": "Rail", "value": 3}), json!({"values": [1, 2]}), json!({"values": [f64::MAX]})] {
            let r = h.call("ui.start", json!({"command": "joint.drive", "params": bad}));
            assert_eq!(r["ok"], false, "{bad}: {r}");
            assert!(h.app.dialog.is_none(), "{bad}");
        }
        for command in ["solid.extrude", "sketch.create", "inspect.zebra"] {
            let r = h.call("ui.start", json!({"command": command, "params": {"distance": 5}}));
            assert!(r["ok"] == false && r["error"].as_str().unwrap().contains("takes no ui.start params"), "{r}");
            assert!(h.app.dialog.is_none() && h.app.tool.is_none() && h.app.session.analysis.is_none(), "{command} started nothing");
        }
    }

    /// #9087: a sample index that names no sample is refused, so an agent can tell a typo from
    /// a sample that opened. The index is checked before it reaches `open_sample`, which would
    /// otherwise drop it and reply `ok`.
    #[test]
    fn ui_home_refuses_a_sample_index_that_names_no_sample() {
        let mut h = crate::scenario::Harness::new();
        let count = solidcraft_engine::sample::samples().len();
        for bad in [json!({"sample": count}), json!({"sample": 99}), json!({"sample": -1})] {
            let r = h.call("ui.home", bad.clone());
            assert_eq!(r["ok"], false, "{bad}: {r}");
            let error = r["error"].as_str().unwrap_or_default();
            assert!(error.contains(&format!("0 to {}", count - 1)), "the error names the valid range: {bad}: {r}");
        }
        // A sample that exists still opens.
        let r = h.call("ui.home", json!({"sample": 0}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["result"]["open"], true, "the sample was asked for: {r}");
    }
}
