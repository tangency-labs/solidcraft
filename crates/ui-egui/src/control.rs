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
//! - `document.export`, `document.import {text, name?}`, `document.saved {revision}`: the design
//!   for a host that keeps the files itself (#8517)
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
//! - `ui.panel_fixture {enabled?, preset?, translucency_percent?, reduced_motion?}`: opt-in synthetic content in the real panel host;
//!   `{}` observes the current host, probe and Session without ticking or model commands
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
//! - `ui.recover {action?: recover|discard|later, id?}`: the "Recover unsaved design?" prompt
//! - `ui.agent_cursor {show?, speed?: 1–5 (times) | "instant", follow_camera?}`: the agent cursor
//!   (#35); while it is shown, `engine.execute` first glides to the command's button and picks
//!   and replies once the command has run (`"animate": false` skips that one call)
//! - `ui.resize {width, height}`, `app.quit`
//!
//! `control.pending` and `control.cancel` belong to the connection: its host answers them
//! (`solidcraft_control::server`) through each request's [`ControlRequest::ticket`].

use solidcraft_engine::geom::prelude::*;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use solidcraft_engine::io::vfs;
use solidcraft_engine::schema::ParamCheck;
use solidcraft_engine::{batch_params, timing};
use solidcraft_viewport::{Camera, StandardView};

use crate::SolveApp;
use crate::control_params as params;

// The request, its reply and the design watch are the shared control channel's
// (`solidcraft_control`); SolidCraft's handlers below answer them.
pub(crate) use solidcraft_control::{ControlRequest, ControlResponse, DOCUMENT_WATCH, DocumentWatch, cancelled_chat_response, chat_turn_cancelled};

/// Publish the active design for subscribed connections ([`DocumentWatch`]). Called at the end of
/// each frame's logic (which runs while the window is hidden too) and of its drawing, so a
/// change made in either goes out in that frame.
pub(crate) fn publish_design(app: &mut SolveApp, watch: &DocumentWatch) {
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
    let text = e.to_string();
    let mut reply = json!({"ok": false, "error": text});
    if let Some(reason) = solidcraft_engine::panels::commands::structured_error(&text) {
        reply["panel_error"] = reason;
    }
    Outcome::Done(reply)
}
fn wrap(r: Result<Value, String>) -> Outcome {
    match r {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

/// The design's own Sketches folder (`Visibility::hidden_folders`), which `showSketches` sets.
const SKETCHES_FOLDER: &str = "c0/sketches";

/// The open dialog as `ui.inspect`, `ui.start` and `ui.editFeature` report it: its command, its
/// picks and what OK runs, in the commands' own terms rather than the form's internals (#9087).
fn dialog_state(app: &SolveApp) -> Value {
    // No feature dialog: the "Recover unsaved design?" prompt, if it is up (answer it with ui.recover).
    let Some(d) = app.dialog.as_ref() else { return crate::recovery_ui::describe(app).unwrap_or(Value::Null) };
    let runs = crate::dialogs::apply_commands(app, d);
    json!({
        "command": d.command(),
        "title": crate::dialogs::report_title(d),
        "editing": d.editing.map(|(feature, _)| feature),
        "inputs": d.inputs.iter().enumerate().map(|(i, input)| json!({"label": input.label, "items": input.items, "active": i == d.active})).collect::<Vec<_>>(),
        "error": d.error,
        "commands": runs.as_ref().ok().map(|cmds| cmds.iter().map(|(command, params)| json!({"command": command, "params": params})).collect::<Vec<_>>()),
        "waiting": runs.err(),
    })
}

/// The windows open beside the model (#9087), each named by what opens it: the command
/// `ui.start` takes, or the `ui.help` item.
fn open_windows(app: &SolveApp) -> Vec<&'static str> {
    [
        (app.params.is_some(), "parameters.change"),
        (app.design_properties.is_some(), "document.metadata"),
        (app.sketch_report.is_some(), "sketch.inspect"),
        (app.bom.is_some(), "assembly.bom"),
        (app.material_card_review.is_some(), "material.properties.validate"),
        (app.help.about, "about"),
        (app.updater.open, "updates"),
        (app.keymap.open, "shortcuts"),
        (app.crashes.open, "crashes"),
        (app.issue_report.draft.is_some(), "report"),
        (app.messages.open, "messages"),
    ]
    .into_iter()
    .filter_map(|(open, command)| open.then_some(command))
    .collect()
}

/// What a start or an edit opened: the tool, the dialog and the windows. `pending` while it
/// waits for the history to compute first; it opens on a later frame (`ui.inspect` shows it).
fn started(app: &SolveApp) -> Value {
    json!({
        "tool": app.tool.as_ref().map(|t| t.cmd),
        "dialog": dialog_state(app),
        "windows": open_windows(app),
        "pending": app.pending_feature_edit.is_some() || app.start_after_roll.is_some(),
        "status": app.status.as_ref().map(|(text, _, error)| json!({"text": text, "error": error})),
    })
}

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

/// Run one command as `engine.execute` does: through `app.run`, then the first-body fit and the
/// camera the command asked for.
fn execute(app: &mut SolveApp, id: &str, params: Value) -> Result<Value, String> {
    if solidcraft_engine::panels::commands::is_command(id) {
        // No first-body meshing, camera-follow or fit tail for application layout.
        return app.run(id, params);
    }
    // User bindings live in the UI. A bridged agent needs the same key the menus show,
    // including null for a cleared binding; the standalone engine keeps its defaults.
    if id == "engine.commands" {
        let mut commands = app.session.execute(id, &params).map_err(|e| e.to_string())?;
        if let Some(rows) = commands.as_array_mut() {
            for row in rows.iter_mut().filter_map(Value::as_object_mut) {
                let key = row.get("id").and_then(Value::as_str).and_then(|id| crate::keymap::effective(app, id));
                row.insert("shortcut".into(), json!(key));
                if let Some(action) =
                    row.get("id").and_then(Value::as_str).and_then(solidcraft_engine::panels::ContextualPanelAction::from_command_id)
                {
                    let reason = app.panel_contextual_disabled_reason(action);
                    row.insert("enabled".into(), json!(reason.is_none()));
                    row.insert("disabled_reason".into(), json!(reason));
                }
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
    // Eligibility follows the same case-insensitive IDs as the engine.
    let fit_id = solidcraft_engine::find_command(id).map_or(id, |spec| spec.id);
    let fit_command = !matches!(
        fit_id,
        "view.fit"
            | "view.home"
            | "view.tool"
            | "view.tool.zoom"
            | "view.spin"
            | "view.visual_style"
            | "view.grid"
            | "view.ground_shadow"
            | "view.navigation_bar"
            | "view.ribbon_tabs_only"
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
    ) && !fit_id.starts_with("settings.");
    // History changes and session swaps can return to the first-body fitting window.
    let history_may_fit = app.session.undo.len() <= 2
        || SWITCHES_DESIGN.contains(&fit_id)
        || matches!(fit_id, "edit.undo" | "edit.restore_checkpoint" | "doc.open" | "doc.recover" | "sketch.finish");
    // Placement failure does not establish an empty model. Keep its camera and
    // let the existing placement diagnostic explain the unavailable world.
    let was_empty = fit_command && history_may_fit && app.session.world_state().is_ok_and(|world| world.bodies.is_empty());
    let r = app.run(id, params);
    // Update Links in a part edited in place reads the assembly around it too, as its button does.
    if r.is_ok() && is_command(Some(id), "component.update_links") && app.edit_in_place.active.is_some() {
        crate::edit_in_place::refresh(app);
    }
    if r.is_ok()
        && was_empty
        && app.session.active_sketch.is_none()
        // Cheap test first: placing the world (`world_state`) after every step of a batch in a
        // design with components placed every component's bodies again once per step.
        && app.session.undo.len() <= 2
        && crate::open_meshing::session_world(&app.session).bodies.len() == 1
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

/// A batch step's parameters: missing or null is no parameters.
fn step_params(step: &Value) -> Value {
    step.get("params").cloned().filter(|p| !p.is_null()).unwrap_or(json!({}))
}

/// Most steps in one `engine.batch` (as in the MCP `batch` tool).
const MAX_BATCH: usize = 10_000;

/// Commands that load a file: through `engine.execute` they go to the app's file loader
/// (`file_load::control`: off the window thread, with progress and Cancel), which a batch would
/// bypass.
const LOADS_A_FILE: [&str; 5] = ["doc.open", "file.insert_step", "file.insert_mesh", "pointcloud.insert", "pointcloud.reload"];

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
    if steps.iter().any(|step| batch_step_command(step).is_some_and(|id| id.eq_ignore_ascii_case("plugin.run"))) {
        return Some(solidcraft_ui_text::tr("ui.plugin_standalone").into());
    }
    if let Some((i, c)) = steps.iter().enumerate().find_map(|(i, step)| switches_design(step).map(|c| (i, c))) {
        return Some(format!("engine.batch unavailable: step {i} ({c}) switches to another design"));
    }
    let (i, c) = steps.iter().enumerate().find_map(|(i, step)| LOADS_A_FILE.into_iter().find(|id| is_command(command(step), id)).map(|c| (i, c)))?;
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
fn batch(app: &mut SolveApp, q: params::Batch) -> Outcome {
    let steps = &q.commands;
    if steps.len() > MAX_BATCH {
        return err(format!("engine.batch: commands must be an array of at most {MAX_BATCH} steps"));
    }
    let panels = steps.iter().filter(|step| command(step).is_some_and(solidcraft_engine::panels::commands::is_command)).count();
    if panels > 0 {
        if let Some(why) = batch_refusal(steps) {
            return err(why);
        }
        let rollback = q.rollback;
        if let Some(refusal) = solidcraft_engine::batch_preflight::check(steps, app.session.param_check, &["view.pick"]) {
            return ok(refusal.reply(steps.len(), rollback));
        }
        if rollback {
            if panels != steps.len() {
                return err(solidcraft_engine::panels::commands::error_text(solidcraft_engine::panels::error(
                    solidcraft_engine::panels::Rule::Capability,
                    "atomic mixed document/layout batches are unavailable; use rollback:false or separate transactions",
                )));
            }
            return wrap(app.run_panel_batch(steps).map_err(solidcraft_engine::panels::commands::error_text));
        }
        // Explicit sequential mode has per-command atomicity and no document
        // checkpoint; previous application changes are truthfully retained.
        let mut results = Vec::new();
        for (i, step) in steps.iter().enumerate() {
            let result = command(step).ok_or_else(|| "missing command".to_string()).and_then(|id| execute(app, id, step_params(step)));
            match result {
                Ok(result) => results.push(result),
                Err(error) => return ok(solidcraft_engine::panels::commands::sequential_failure(i, command(step), error, results)),
            }
        }
        return ok(json!({"completed":results.len(),"results":results,"undone":0}));
    }
    // (With the agent cursor shown, the pointer has already acted out a short summary of the
    // batch: `agent_cursor::intercept`.)
    if let Some(why) = batch_refusal(steps) {
        return err(why);
    }
    let rollback = q.rollback;
    // A step the batch would refuse at its turn refuses it now, before anything runs (#5853).
    if let Some(refusal) = solidcraft_engine::batch_preflight::check(steps, app.session.param_check, &["view.pick"]) {
        return ok(refusal.reply(steps.len(), rollback));
    }
    let profile = q.profile;
    let timed = profile || q.timings;
    let mut rows = Vec::new();
    let clock = timing::Clock::start();
    let checkpoint = if rollback { app.run("edit.checkpoint", json!({})).ok().and_then(|v| v["checkpoint"].as_u64()) } else { None };
    let mut results = Vec::with_capacity(steps.len());
    // With `coalesce_parameters`, each run of plain parameter edits rebuilds once (#5822); a run
    // whose combined edit is refused goes step by step, so a refused edit is named by its step.
    let coalesce = q.coalesce_parameters;
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
                Some(id) => execute(app, id, step_params(step)),
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

/// `engine.script {commands}` (or the bare array): each step runs as `engine.execute` runs it, so
/// the window follows it as it follows a batch step (#12629). The first failure stops the script,
/// and the steps before it stay. Its words are the engine script's, or the panel error when a step
/// is a panel command.
fn script(app: &mut SolveApp, p: &Value) -> Outcome {
    let steps = p.as_array().or_else(|| p.get("commands").and_then(Value::as_array));
    if steps.is_some_and(|steps| steps.iter().any(|step| batch_step_command(step).is_some_and(|id| id.eq_ignore_ascii_case("plugin.run")))) {
        return err(solidcraft_ui_text::tr("ui.plugin_standalone"));
    }
    let steps = match solidcraft_engine::script_steps(p) {
        Ok(steps) => steps,
        Err(e) => return err(e),
    };
    if let Some(c) = steps.iter().find_map(switches_design) {
        return err(format!("engine.script unavailable: {c} switches to another design (send it on its own)"));
    }
    let panels = steps.iter().any(|step| command(step).is_some_and(solidcraft_engine::panels::commands::is_command));
    let mut results = Vec::with_capacity(steps.len());
    for (i, step) in steps.iter().enumerate() {
        let Some(id) = command(step) else {
            return if panels {
                Outcome::Done(json!({"ok": false, "error": "missing command", "step": i + 1}))
            } else {
                err(format!("script step {}: missing `command`", i + 1))
            };
        };
        match execute(app, id, step_params(step)) {
            Ok(result) => results.push(result),
            Err(error) if panels => {
                let mut reply = json!({"ok": false, "error": error, "step": i + 1});
                if let Some(reason) = solidcraft_engine::panels::commands::structured_error(&error) {
                    reply["panel_error"] = reason;
                }
                return Outcome::Done(reply);
            }
            Err(error) => return err(format!("step {} ({id}): {error}", i + 1)),
        }
    }
    ok(json!(results))
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
pub(crate) fn view_scene(app: &SolveApp, guides: solidcraft_engine::view::Guides) -> Result<solidcraft_viewport::Scene, String> {
    let mut palette = crate::viewport::scene_palette(app.viewport_dark(), app.preferences.color_vision);
    // The view's own background colour (#7688).
    if let Some([r, g, b]) = app.preferences.viewport_background {
        palette.background = (solidcraft_viewport::Rgb(r, g, b), solidcraft_viewport::Rgb(r, g, b));
    }
    let mut scene = solidcraft_engine::view::scene_with_thread_markings(
        &app.session,
        &app.cam,
        crate::viewport::section_plane(app),
        solidcraft_engine::view::world_curve_parts,
        &palette,
        guides,
        app.preferences.cosmetic_threads,
    )
    .map_err(|e| e.to_string())?;
    scene.plain_cap = !crate::viewport::section_hatch(app);
    if app.preferences.section_hatch == crate::prefs::SectionHatch::One {
        scene.part_hatch = None;
    }
    Ok(scene)
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
    solidcraft_engine::capture::rgba(&view_scene(app, window_guides(app))?, &app.cam, w, h).map(|p| p.rgba)
}

/// [`view_rgba`] as PNG.
pub fn view_png(app: &SolveApp, w: u32, h: u32) -> Result<Vec<u8>, String> {
    let rgba = view_rgba(app, w, h)?;
    solidcraft_viewport::png(w as usize, h as usize, rgba).ok_or_else(|| "PNG encoding failed".to_string())
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
    let warnings = match contract_check_before_run(command(p).unwrap_or_default(), p.get("params").unwrap_or(&Value::Null), level) {
        Ok(warnings) => warnings,
        Err(e) => return err(e),
    };
    let mut out = dispatch(app, ctx, method, p);
    if let Outcome::Done(v) = &mut out {
        add_param_warnings(v, warnings);
    }
    out
}

/// A call's parameters against its command's contract, for a command that does not run on a
/// session at `level` (one that switches design, one the file loader runs): `Err` refuses the
/// call, `Ok` holds the warnings for its result.
pub(crate) fn contract_check_before_run(id: &str, params: &Value, level: ParamCheck) -> Result<Vec<String>, String> {
    let issues: Vec<String> = solidcraft_engine::schema::issues(id, params).into_iter().map(|i| i.message).collect();
    match (level, issues.first()) {
        (ParamCheck::Strict, Some(first)) => Err(solidcraft_engine::params::bad(id, first.as_str()).to_string()),
        (ParamCheck::Warn, _) => Ok(issues),
        _ => Ok(Vec::new()),
    }
}

/// Add what `contract_check_before_run` found to a reply's result.
pub(crate) fn add_param_warnings(reply: &mut Value, warnings: Vec<String>) {
    if !warnings.is_empty()
        && let Some(o) = reply.get_mut("result").and_then(Value::as_object_mut)
    {
        o.insert("param_warnings".into(), json!(warnings));
    }
}

/// Run `f` on the params read as `T`, or refuse them.
fn with<T: DeserializeOwned>(p: &Value, f: impl FnOnce(T) -> Outcome) -> Outcome {
    match params::parse(p) {
        Ok(t) => f(t),
        Err(e) => err(e),
    }
}

fn dispatch(app: &mut SolveApp, ctx: &egui::Context, method: &str, p: &Value) -> Outcome {
    crate::render_workspace::sync_backend(app);
    match method {
        "ui.blur" => ui_blur(app, ctx, p),
        "ui.panel_fixture" => {
            let result = crate::panel_runtime::control_specimen(app, p);
            if result.is_ok() && (p.get("enabled").is_some() || p.get("preset").is_some()) {
                ctx.request_repaint();
            }
            wrap(result)
        }
        "ui.status_fixture" => wrap(crate::viewport_status_fixture::control(app, p)),
        "ui.workspace" => with(p, |q| ui_workspace(app, q)),
        "engine.execute" | "command" => with(p, |q: params::Execute| {
            let id = q.command.clone();
            wrap(execute(app, &id, q.params()))
        }),
        "engine.batch" => with(p, |q| batch(app, q)),
        "ui.agent_cursor" => ui_agent_cursor(app, p),
        "engine.script" => script(app, p),
        "engine.commands" => wrap(execute(app, "engine.commands", json!({}))),
        "document.inspect" => wrap(app.session.execute("document.inspect", p).map_err(|e| e.to_string())),
        "document.revision" => wrap(app.session.execute("document.revision", p).map_err(|e| e.to_string())),
        // A host that keeps the files itself (an editing embed, #8517).
        "document.export" => wrap(crate::documents::export(app)),
        "document.import" => wrap(crate::documents::import(app, p)),
        "document.saved" => wrap(crate::documents::saved(app, p)),
        // The connection already counts as subscribed (`DocumentWatch::join`), and its events start
        // from this answer: published first, so no event repeats it and none older follows.
        "control.subscribe" => {
            let revision = app.session.design_revision();
            DOCUMENT_WATCH.publish(&app.session.document_id, revision);
            ok(json!({"document_id": app.session.document_id, "revision": revision}))
        }
        // What the MCP server bridged to this app shapes its output with (Preferences › AI).
        "mcp.options" => ok(json!({"compact": app.preferences.mcp_compact})),
        "ui.messages" => ok(crate::messages::control(app, p)),
        "ui.inspect" => ui_inspect(app),
        "ui.file" => with(p, |q| ui_file(app, q)),
        "ui.documents" => ok(crate::documents::control(app, p)),
        "ui.operations" => wrap(crate::operations::control(app, p)),
        "ui.home" => with(p, |q| ui_home(app, q)),
        "ui.set" => ui_set(app, p),
        "ui.view" => with(p, |q| ui_view(app, q)),
        "ui.camera" => wrap(camera(app, p)),
        "ui.palette" => ui_palette(app, p),
        "ui.start" => with(p, |q| ui_start(app, q)),
        "ui.move" => with(p, |q| ui_move(app, q)),
        "ui.click" => with(p, |q| ui_click(app, q)),
        "ui.drag" => with(p, |q| ui_drag(app, q)),
        "ui.editFeature" => with(p, |q| ui_edit_feature(app, q)),
        "ui.editFolder" => ui_edit_folder(app, p),
        "ui.selection" => ui_selection(app),
        "ui.triad" => ui_triad(app),
        "ui.dialogInput" => with(p, |q| ui_dialog_input(app, q)),
        "ui.handles" => with(p, |q: params::Handles| {
            // The handles drawn last frame (toolbar buttons `toolbar:<id>`, panels, browser rows…).
            let list = crate::scenario::handles(&q.prefix);
            ok(json!(list.into_iter().map(|(n, p)| json!({"name": n, "x": p.x, "y": p.y})).collect::<Vec<_>>()))
        }),
        "ui.pick" => wrap(crate::view_pick::pick_command(app, p)),
        "ui.at" => match params::At::parse(p) {
            Ok(at) => ui_at(app, at),
            Err(e) => err(e),
        },
        "ui.worldToScreen" => with(p, |q| ui_world_to_screen(app, q)),
        "ui.sketchToScreen" => with(p, |q| ui_sketch_to_screen(app, q)),
        "ui.scroll" => with(p, |q| ui_scroll(app, ctx, q)),
        "ui.key" => with(p, |q: params::KeyPress| {
            let (key, modifiers, pressed) = (q.key.0, q.modifiers(), q.pressed);
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
        }),
        "ui.text" => with(p, |q: params::Text| {
            app.synthetic.push(egui::Event::Text(q.text));
            ok(Value::Null)
        }),
        "ui.icon_audit" => ui_icon_audit(app, p),
        "ui.menu" => match params::Menu::parse(p) {
            Ok(menu) => ui_menu(app, menu),
            Err(e) => err(e),
        },
        "ui.confirm" => with(p, |q| ui_confirm(app, q)),
        "ui.shortcut" => with(p, |q| ui_shortcut(app, q)),
        "ui.browser" => ui_browser(app),
        "ui.capture" => with(p, |q: params::Action| match q.action {
            Some(a) => match crate::browser::answer_capture(app, &a) {
                Ok(()) => ok(json!({"pending_moves": app.session.pending_moves.len()})),
                Err(e) => err(e),
            },
            None => ok(json!({"question": app.tree.capture_prompt, "pending_moves": app.session.pending_moves.len()})),
        }),
        "ui.recover" => match p.get("action").and_then(Value::as_str) {
            Some(_) if p.get("id").is_some_and(|v| !v.is_string()) => err("`id` must be a string (a design's `id` from `ui.inspect`'s `dialog`)"),
            Some(a) => match crate::recovery_ui::answer(app, a, p.get("id").and_then(Value::as_str)) {
                Ok(()) => ok(json!({"dialog": crate::recovery_ui::describe(app), "file_busy": app.file_load.busy()})),
                Err(e) => err(e),
            },
            None => ok(json!({"dialog": crate::recovery_ui::describe(app)})),
        },
        "ui.prefs" => wrap(crate::prefs::control(app, p)),
        "ui.agent_chat" => wrap(crate::agent_chat::control(app, ctx, p)),
        "ui.help" => with(p, |q| ui_help(app, ctx, q)),
        "ui.selectOther" => wrap(crate::select_other::control(app, p)),
        "ui.menuPick" => with(p, |q: params::MenuPick| wrap(crate::context_menu::pick(app, &q.item))),
        "ui.rename" => with(p, |q: params::Rename| {
            if crate::context_menu::finish_rename(app, q.text.as_deref(), q.commit) {
                ok(json!({"done": true}))
            } else {
                err("no rename in progress")
            }
        }),
        "ui.window" => with(p, |q: params::Action| match crate::titlebar::window_action(app, ctx, q.action.as_deref().unwrap_or("")) {
            Ok(()) => ok(json!({"custom_titlebar": app.custom_titlebar, "maximized": ctx.input(|i| i.viewport().maximized)})),
            Err(e) => err(e),
        }),
        "ui.resize" => wrap(solidcraft_control_egui::window::resize(ctx, p)),
        "ui.screenshot" => with(p, |q: params::Screenshot| match q.path.as_deref().map(vfs::check) {
            Some(Err(e)) => err(e),
            _ => Outcome::Screenshot { path: q.path },
        }),
        "ui.render" => with(p, |q| ui_render(app, q)),
        "app.quit" => with(p, |q| app_quit(app, ctx, q)),
        other => err(format!("unknown method `{other}`")),
    }
}

/// `ui.blur {radius_px?, surface_alpha?, fixture?}`: the blur harness and what it reports.
fn ui_blur(app: &mut SolveApp, ctx: &egui::Context, p: &Value) -> Outcome {
    // Internal rendering harness; deliberately no saved preference or product UI.
    let radius = if let Some(radius) = p.get("radius_px") {
        let Some(radius) = radius.as_f64().filter(|v| v.is_finite() && (0.0..=60.0).contains(v)) else {
            return err("radius_px must be finite and between 0 and 60");
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
    // Explicit, labelled evidence fixtures only. They can force a failure, never fake
    // real renderer support. All validation precedes mutation, including old fields.
    let fixture = if let Some(fixture) = p.get("fixture") {
        let Some(fixture) =
            fixture.as_str().filter(|s| matches!(*s, "clear" | "gpu-software" | "reduce-transparency" | "web-unsupported" | "guard-60"))
        else {
            return err("unknown blur evidence fixture");
        };
        Some(fixture)
    } else {
        None
    };
    if let Some(radius) = radius {
        if let Err(error) = app.blur.set_radius(radius) {
            return err(error);
        }
        app.blur_session.harness_radius = Some(radius);
        ctx.request_repaint();
    }
    if let Some(alpha) = alpha {
        app.blur.set_surface_alpha(alpha);
        ctx.request_repaint();
    }
    if let Some(fixture) = fixture {
        match fixture {
            "clear" => {
                app.blur_session.harness_unavailable = None;
                app.blur_session.harness_radius = None;
                app.blur_session.evidence_fixture = None;
            }
            "gpu-software" => {
                app.blur_session.harness_unavailable = Some("prefs.blur_software");
                app.blur_session.evidence_fixture = Some("gpu-software");
            }
            "reduce-transparency" => {
                app.blur_session.harness_unavailable = Some("prefs.blur_reduce_transparency");
                app.blur_session.evidence_fixture = Some("reduce-transparency");
            }
            "web-unsupported" => {
                app.blur_session.harness_unavailable = Some("prefs.blur_web");
                app.blur_session.evidence_fixture = Some("web-unsupported");
            }
            "guard-60" => {
                let radius = app.blur.radius();
                for _ in 0..60 {
                    app.blur_session.guard.observe(radius, 23.0, true);
                }
                app.blur_session.evidence_fixture = Some("guard-60");
            }
            _ => {}
        }
        ctx.request_repaint();
    }
    let availability = crate::prefs::blur_availability(app);
    let reason = crate::prefs::theme_option_reason(availability).map(|reason| app.tr(reason));
    ok(
        json!({"radius_px":app.blur.radius(), "max_radius_px":60, "capability":app.blur.capability_for(ctx.viewport_id()), "counters":app.blur.counters(),
            "available": availability == crate::theme_options::Availability::Available, "reason": reason,
            "evidence_fixture": app.blur_session.evidence_fixture,
            "guard": {"tier_cap": app.blur_session.guard.cap, "streak": app.blur_session.guard.streak,
                "notice": app.blur_session.guard.notice, "opted_out": app.blur_session.guard.opted_out}}),
    )
}

/// `ui.editFeature {feature}`: open a feature's edit dialog.
fn ui_edit_feature(app: &mut SolveApp, q: params::EditFeature) -> Outcome {
    let key = q.feature.key();
    let Some(id) = app.session.doc.find_feature(&key).map(|f| f.id) else { return err(format!("no feature `{key}`")) };
    app.edit_feature(id);
    let mut reply = started(app);
    if let Some(o) = reply.as_object_mut() {
        o.insert("marker".into(), json!(app.session.doc.marker));
    }
    ok(reply)
}

/// `ui.editFolder {folder}`: open a timeline folder's edit dialog, by id or name.
fn ui_edit_folder(app: &mut SolveApp, p: &Value) -> Outcome {
    let Some(id) = (match p.get("folder") {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(x)) => app.session.doc.timeline_groups.iter().find(|group| group.name == *x).map(|group| group.id),
        _ => return err("`folder` must be a timeline folder id or name"),
    }) else {
        return err("no such timeline folder");
    };
    if let Err(why) = crate::hole_folder::open(app, id) {
        return err(why);
    }
    ok(started(app))
}

/// `ui.selection`: selection, dialog inputs, hover and drawn sketch dimensions.
fn ui_selection(app: &SolveApp) -> Outcome {
    ok(json!({
        "selection": app.session.selection,
        "dialog": app.dialog.as_ref().map(|d| d.inputs.iter().map(|i| json!({"label": i.label, "items": i.items})).collect::<Vec<_>>()),
        "hover": app.viewport.hover,
        "dimensions": crate::dim_view::drawn().into_iter().map(|(id, p)| json!({"id": id, "x": p.x, "y": p.y})).collect::<Vec<_>>(),
        "shown_dimensions": crate::dim_view::model_drawn()
            .into_iter()
            .map(|(sketch, id, p)| json!({"sketch": sketch, "id": id, "x": p.x, "y": p.y}))
            .collect::<Vec<_>>(),
        "dimension_selected": crate::dim_view::selected(app),
        "dimension_editing": crate::dim_view::editing(),
    }))
}

/// `ui.palette {open?, text?}`: open the command palette (with `text` typed), or close it.
fn ui_palette(app: &mut SolveApp, p: &Value) -> Outcome {
    if p.get("open").and_then(Value::as_bool) == Some(false) {
        app.ui.palette_open = false;
        app.sbox.text.clear();
    } else {
        app.start("ui.palette");
        if let Some(query) = p.get("text").and_then(Value::as_str) {
            app.sbox.text = query.chars().take(crate::shortcut_box::MAX_QUERY).collect();
        }
    }
    ok(json!({"open": app.ui.palette_open, "text": app.sbox.text}))
}

/// `ui.icon_audit {reset?, stop?}`: the icons requested since the audit started.
fn ui_icon_audit(_app: &mut SolveApp, p: &Value) -> Outcome {
    let b = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
    if b("reset") {
        crate::icon_svg::audit::start();
    }
    let requested = crate::icon_svg::audit::take();
    if b("stop") {
        crate::icon_svg::audit::stop();
    }
    ok(json!({"requested": requested}))
}

/// `ui.triad`: screen points of the 3D move triad's handles (x, y, z arrows and the plane square).
fn ui_triad(app: &SolveApp) -> Outcome {
    let Some(rect) = app.viewport.rect else { return err("no viewport") };
    let proj = crate::viewport::projection(app, rect);
    ok(json!(crate::sketch3d::handle_points(app, &proj).into_iter().map(|(n, p)| json!({"handle": n, "x": p.x, "y": p.y})).collect::<Vec<_>>()))
}

/// `ui.confirm {accept?}`: the Delete confirmation.
fn ui_confirm(app: &mut SolveApp, q: params::Confirm) -> Outcome {
    let Some(a) = q.accept else { return ok(json!({"pending": app.menu.confirm.as_ref().map(|c| c.report.clone())})) };
    if crate::delete::confirm(app, a) { ok(json!({"accepted": a})) } else { err("nothing is waiting for a confirmation") }
}

/// `ui.shortcut {command, key?, slot?, replace?}`: a command's keys, or bind one.
fn ui_shortcut(app: &mut SolveApp, q: params::Shortcut) -> Outcome {
    let id = q.command.as_str();
    let slot = q.slot.map(|n| usize::try_from(n).unwrap_or(usize::MAX));
    if let Some(k) = &q.key
        && let Err(e) = crate::keymap::bind_at(app, id, slot, k, q.replace)
    {
        return err(e);
    }
    ok(json!({"command": id, "key": crate::keymap::effective(app, id), "keys": crate::keymap::keys(app, id)}))
}

/// `ui.browser`: the browser rows drawn last frame and what is picked.
fn ui_browser(app: &SolveApp) -> Outcome {
    ok(json!({
        "occurrences": crate::browser::drawn_occurrences().iter().map(|(path, label, r, open)| json!({"path": path, "label": label, "y": r.center().y, "open": open})).collect::<Vec<_>>(),
        "rows": crate::browser::drawn_rows().iter().map(|(label, r, _, open)| json!({"label": label, "y": r.center().y, "open": open})).collect::<Vec<_>>(),
        "selection": app.session.selection,
        "picked_components": app.tree.picked_components,
    }))
}

/// `ui.workspace {workspace?}`: enter the design or render workspace; returns the one shown.
fn ui_workspace(app: &mut SolveApp, q: params::WorkspaceParams) -> Outcome {
    if let Some(w) = q.workspace
        && let Err(e) = crate::render_workspace::enter(app, w == params::Workspace::Render)
    {
        return err(e);
    }
    ok(json!({"workspace":if crate::render_workspace::active(app) {"render"} else {"design"}}))
}

/// `ui.agent_cursor {show?, speed?, follow_camera?}`: the agent cursor's settings.
fn ui_agent_cursor(app: &mut SolveApp, p: &Value) -> Outcome {
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

/// `ui.inspect`: UI state, viewport rect, camera, tool and dialog.
fn ui_inspect(app: &SolveApp) -> Outcome {
    ok(json!({
        "render": crate::render_workspace::inspect_state(app),
        "ui": ui_state(app),
        "visibility": app.session.visibility,
        "viewport": app.viewport.rect.map(|r| json!([r.left(), r.top(), r.width(), r.height()])),
        "camera": app.cam,
        "tool": app.tool.as_ref().map(|t| t.cmd),
        "dialog": dialog_state(app),
        "windows": open_windows(app),
        "status": app.status.as_ref().map(|(text, _, error)| json!({"text": text, "error": error})),
        "hover": app.viewport.hover,
        "snap_points": crate::snap_wake::inspect(app),
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
    }))
}

/// `ui.file {path, insert?, units?}`: open or insert a file through the file loader.
fn ui_file(app: &mut SolveApp, q: params::File) -> Outcome {
    // A control call never meets the unit dialog an STL or OBJ file asks a person: it names
    // `units` itself, or the file is in millimetres.
    let kind = if q.insert { crate::file_load::Kind::Insert } else { crate::file_load::Kind::Open };
    let mut request = crate::file_load::Request::new(&q.path, kind);
    if let Some(units) = q.units {
        request.options.insert("units".into(), units);
    }
    let _ = crate::file_load::begin_request(app, request);
    ok(json!({"busy": app.file_load.busy()}))
}

/// `ui.home {open?, sample?}`: the Home page.
fn ui_home(app: &mut SolveApp, q: params::Home) -> Outcome {
    // The index is checked before anything changes: `open_sample` drops an index that names
    // no sample and says nothing, so the request would look like it had worked (#9087).
    let sample = match q.sample {
        Some(sample) => {
            let count = solidcraft_engine::sample::samples().len();
            let Some(index) = sample.as_i64() else {
                return err(format!("`sample` must be an index from 0 to {}", count - 1));
            };
            if !(0..count as i64).contains(&index) {
                return err(format!("`sample` must be an index from 0 to {}, not {index}", count - 1));
            }
            Some(index as usize)
        }
        None => None,
    };
    match q.open {
        Some(true) => {
            let rev = app.session.revision;
            app.home.show(rev);
            crate::panel_runtime::hide_app_host(app);
        }
        Some(false) => app.home.open = false,
        None => {}
    }
    if let Some(index) = sample {
        crate::home::open_sample(app, index);
    }
    ok(json!({"open": app.home.open, "recent": app.home.recent}))
}

/// `ui.set {...UiState fields}`: UI state and what is hidden.
fn ui_set(app: &mut SolveApp, p: &Value) -> Outcome {
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

/// `ui.view {view, animate?}`: a standard view, snapped or animated.
fn ui_view(app: &mut SolveApp, q: params::View) -> Outcome {
    let v = q.view.as_str();
    // A name it doesn't know is refused before the view changes at all.
    if v != "fit" && v != "home" && StandardView::parse(v).is_none() {
        return err(format!("unknown view `{v}`"));
    }
    app.viewport.orbit_free = None;
    if q.animate {
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
        v => {
            if let Some(sv) = StandardView::parse(v) {
                app.cam.set_view(sv);
            }
        }
    }
    ok(json!(app.cam))
}

/// `ui.start {command, params?}`: like clicking the toolbar button. `params`: what the dialog
/// opens on (joint.drive, motion.study). Refused: nothing starts, or the dialog closes.
fn ui_start(app: &mut SolveApp, q: params::Start) -> Outcome {
    let id = q.command.as_str();
    let preset = match (id, q.params) {
        (_, Value::Null) => None,
        ("joint.drive" | "motion.study", params) => Some(params),
        ("solid.extrude" | "solid.revolve", params) => return start_on_sketch(app, id, &params),
        _ => {
            return err(format!(
                "`{id}` takes no ui.start params yet (joint.drive takes joint, values; motion.study takes study; solid.extrude and solid.revolve take sketch)"
            ));
        }
    };
    app.start(id);
    let Some(params) = preset else { return ok(started(app)) };
    let set = if id == "motion.study" {
        let Some(crate::dialogs::Kind::Motion(k)) = app.dialog.as_mut().map(|d| &mut d.kind) else {
            return err("Motion Study did not open");
        };
        crate::dialogs_motion::preset(&app.session, k, &params)
    } else {
        if app.tree.capture_prompt.is_some() {
            return err("Drive Joints waits for the Capture Position question (ui.capture)");
        }
        let Some(crate::dialogs::Kind::Assembly(k)) = app.dialog.as_mut().map(|d| &mut d.kind) else {
            let why = app.status.as_ref().filter(|s| s.2).map(|s| s.0.clone());
            return err(why.unwrap_or_else(|| "Drive Joints did not open".into()));
        };
        crate::dialogs_assembly::preset(&app.session, k, &params)
    };
    if let Err(e) = set {
        crate::dialogs::cancel(app);
        return err(e);
    }
    ok(started(app))
}

/// `ui.start {command: "solid.extrude" | "solid.revolve", params: {sketch}}`: what Extrude and
/// Revolve on a sketch's menu in the history tree do (#6940). The sketch, by id or name, starts
/// with its profiles picked and stays shown while the dialog is open, even when an earlier
/// feature uses it.
fn start_on_sketch(app: &mut SolveApp, id: &str, params: &Value) -> Outcome {
    let Some(o) = params.as_object() else { return err("params must be an object: {sketch}") };
    if let Some(key) = o.keys().find(|k| k.as_str() != "sketch") {
        return err(format!("unknown param `{key}` ({id} takes sketch)"));
    }
    let key = match o.get("sketch") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(name)) => name.clone(),
        _ => return err("`sketch` must be a sketch id or name"),
    };
    let sketch = app.session.doc.find_feature(&key).filter(|f| matches!(f.kind, solidcraft_engine::doc::FeatureKind::Sketch { .. }));
    let Some(sketch) = sketch.map(|f| f.id) else { return err(format!("no sketch `{key}`")) };
    crate::dialogs::start_from_sketch(app, id, sketch);
    ok(started(app))
}

/// `ui.move {x, y, shift?, ctrl?, alt?, cmd?}`: the pointer moves, with those keys held.
fn ui_move(app: &mut SolveApp, q: params::Move) -> Outcome {
    app.synthetic.push(egui::Event::ModifiersChanged(q.mods.held()));
    app.synthetic.push(egui::Event::PointerMoved(q.at.0));
    ok(Value::Null)
}

/// `ui.click {x, y, button?, shift?, ctrl?, alt?, double?}`: a click there.
fn ui_click(app: &mut SolveApp, q: params::Click) -> Outcome {
    let (pos, button, modifiers) = (q.at.0, q.button.egui(), q.mods.press());
    app.synthetic.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    app.synthetic.push(egui::Event::PointerMoved(pos));
    // `double: true` sends two clicks in one frame, which egui reads as a double-click.
    for _ in 0..if q.double { 2 } else { 1 } {
        app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers });
        app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers });
    }
    ok(Value::Null)
}

/// `ui.drag {x0, y0, x1, y1, button?, steps?, hold?, hold_end?, via?}`: press, move, release.
fn ui_drag(app: &mut SolveApp, q: params::Drag) -> Outcome {
    let (button, modifiers) = (q.button.egui(), q.mods.press());
    let (a, z) = (egui::pos2(q.x0 as f32, q.y0 as f32), egui::pos2(q.x1 as f32, q.y1 as f32));
    // Positioning precedes the press by a frame: it is not a modifier-only gesture.
    app.synthetic.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    app.synthetic.push(egui::Event::PointerMoved(a));
    app.synthetic.push(egui::Event::PointerButton { pos: a, button, pressed: true, modifiers });
    // `hold`: frames to keep the button down before moving (press-and-hold).
    for _ in 0..q.hold.clamp(0.0, 600.0) as usize {
        app.synthetic.push(egui::Event::PointerMoved(a));
    }
    let steps = q.steps.clamp(1.0, 200.0) as usize;
    // `via`: [[x, y], ...] the pointer passes through on its way (a lasso's loop, #4376).
    let via: Vec<egui::Pos2> = q
        .via
        .iter()
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
    for _ in 0..q.hold_end.clamp(0.0, 600.0) as usize {
        app.synthetic.push(egui::Event::PointerMoved(z));
    }
    app.synthetic.push(egui::Event::PointerButton { pos: z, button, pressed: false, modifiers });
    ok(Value::Null)
}

/// `ui.dialogInput {index}`: the selection input picks go to.
fn ui_dialog_input(app: &mut SolveApp, q: params::DialogInput) -> Outcome {
    // What clicking a selection input's chip does: make it the one picks go to.
    let i = q.index;
    match app.dialog.as_mut() {
        Some(d) if i < d.inputs.len() => {
            d.active = i;
            ok(json!({"label": d.inputs.get(i).map(|x| x.label)}))
        }
        _ => err("no dialog input with that index"),
    }
}

/// `ui.at {…}`: the screen point of a row, handle, dimension or model point.
fn ui_at(app: &mut SolveApp, at: params::At) -> Outcome {
    use params::At;
    let shown = |found: Option<egui::Pos2>, what: String| match found {
        Some(q) => ok(json!([q.x, q.y])),
        None => err(what),
    };
    let world = match at {
        At::Occurrence { key, control } => {
            return shown(crate::scenario::handle_at(&format!("{control}:occurrence:{key}")), "occurrence row or control is not shown".into());
        }
        At::Handle(h) => return shown(crate::scenario::handle_at(&h), format!("handle {h} is not shown")),
        At::Browser { label, chevron } => {
            let rows = crate::browser::drawn_rows();
            let row = rows.iter().find(|r| r.0 == label);
            let found = if chevron { row.and_then(|r| r.2.map(|f| f.center())) } else { row.map(|r| r.1.left_center() + egui::vec2(120.0, 0.0)) };
            return shown(found, format!("no browser row `{label}`{} on screen", if chevron { " with a fold arrow" } else { "" }));
        }
        At::Dimension(d) => return shown(crate::dim_view::text_at(&d), format!("dimension {d} is not shown")),
        At::World(w) => Some(w),
        At::Sketch(s) => {
            let st = crate::open_meshing::session_world(&app.session);
            app.session.active_sketch.and_then(|id| st.sketch(id)).map(|ss| ss.plane.to_world(s))
        }
        At::Plane(n) => {
            let Some(rect) = app.viewport.rect else { return err("no viewport") };
            crate::viewport::origin_plane_target(app, &crate::viewport::projection(app, rect), &n)
        }
        At::Axis(n) => {
            let size = crate::selection::origin_size(app.cam.half_height());
            crate::viewport::origin_axes(app).into_iter().find(|(name, _)| *name == n).map(|(_, d)| d * (size * 0.8))
        }
        At::Gizmo { name, by, turn } => crate::gizmo::handle_point(&name, by, turn),
    };
    let Some(rect) = app.viewport.rect else { return err("no viewport") };
    let proj = crate::viewport::projection(app, rect);
    shown(world.and_then(|w| proj.to_screen(w)), "not on screen".into())
}

/// `ui.worldToScreen {points}`: screen points of world points.
fn ui_world_to_screen(app: &mut SolveApp, q: params::Points<[f64; 3]>) -> Outcome {
    let Some(rect) = app.viewport.rect else { return err("no viewport") };
    let proj = crate::viewport::projection(app, rect);
    let pts: Vec<Value> =
        q.points.iter().map(|&[x, y, z]| proj.to_screen(solidcore_record::Vec3::new(x, y, z)).map_or(Value::Null, |q| json!([q.x, q.y]))).collect();
    ok(json!(pts))
}

/// `ui.sketchToScreen {points}`: screen points of active-sketch coordinates.
fn ui_sketch_to_screen(app: &mut SolveApp, q: params::Points<[f64; 2]>) -> Outcome {
    // Screen points of active-sketch coordinates (for driving sketch interaction).
    let st = crate::open_meshing::session_world(&app.session);
    let (Some(rect), Some(ss)) = (app.viewport.rect, app.session.active_sketch.and_then(|s| st.sketch(s))) else {
        return err("no active sketch in a drawn viewport");
    };
    let proj = crate::viewport::projection(app, rect);
    let pts: Vec<Value> = q
        .points
        .iter()
        .map(|&[x, y]| proj.to_screen(ss.plane.to_world(solidcore_record::Vec2::new(x, y))).map_or(Value::Null, |p| json!([p.x, p.y])))
        .collect();
    ok(json!(pts))
}

/// `ui.scroll {x, y, delta?, ctrl?, unit?}`: mouse-wheel input.
fn ui_scroll(app: &mut SolveApp, ctx: &egui::Context, q: params::Scroll) -> Outcome {
    // Mouse-wheel semantics (lines): zooms regardless of the trackpad pan
    // preference, so scripts and agents keep working. Trackpads report points.
    // `ctrl: true` replays a Ctrl+wheel pinch in points (#8480). An explicit `unit`
    // overrides that choice; without ctrl the default remains a line wheel.
    let speed = ctx.options(|o| o.input_options.line_scroll_speed).max(1.0);
    let ctrl = q.ctrl.unwrap_or(false);
    let unit = match q.unit.as_ref() {
        Some(Value::String(unit)) if unit == "point" => egui::MouseWheelUnit::Point,
        Some(Value::String(unit)) if unit == "line" => egui::MouseWheelUnit::Line,
        Some(_) => return err("`unit` must be point or line"),
        None if ctrl => egui::MouseWheelUnit::Point,
        None => egui::MouseWheelUnit::Line,
    };
    let delta = match unit {
        // A point wheel reaches the app as points; a line wheel is scaled up by the
        // line scroll speed, so divide it out and both land on the same points.
        egui::MouseWheelUnit::Line => q.delta / f64::from(speed),
        _ => q.delta,
    };
    let (at, delta) = (q.at.0, delta as f32);
    if !delta.is_finite() {
        return err("x, y and delta must fit finite screen coordinates");
    }
    let modifiers = if ctrl { egui::Modifiers::CTRL } else { Default::default() };
    let explicit = q.ctrl.is_some();
    app.synthetic.push(egui::Event::PointerMoved(at));
    if explicit {
        app.synthetic.push(egui::Event::ModifiersChanged(modifiers));
    }
    app.synthetic.push(egui::Event::MouseWheel {
        source: egui::MouseWheelSource::Unknown,
        unit,
        delta: egui::vec2(0.0, delta),
        modifiers,
        phase: egui::TouchPhase::Move,
    });
    if explicit {
        app.synthetic.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    }
    ok(Value::Null)
}

/// `ui.menu {x?, y?, target?, close?}`: the context menu.
fn ui_menu(app: &mut SolveApp, menu: params::Menu) -> Outcome {
    match menu {
        params::Menu::Close => {
            crate::context_menu::close(app);
            ok(json!({"open": false}))
        }
        params::Menu::Open { at, target } => {
            // A right-click opens a menu only on something that is there.
            if let Err(e) = crate::context_menu::check_target(app, &target) {
                return err(e);
            }
            crate::context_menu::request_open(app, at.0, target);
            ok(json!({"pending": true}))
        }
        params::Menu::Describe => ok(crate::context_menu::describe(app)),
    }
}

/// `ui.help {item?}`: the Help menu; the version and diagnostics.
fn ui_help(app: &mut SolveApp, ctx: &egui::Context, q: params::Help) -> Outcome {
    // The Help menu's items (the macOS menu bar sends the same); none only reports.
    const ITEMS: [&str; 8] = ["about", "updates", "shortcuts", "report", "crashes", "docs", "messages", "install_cli"];
    match q.item.as_deref() {
        Some(i) if !ITEMS.contains(&i) => return err(format!("unknown help item `{i}` ({})", ITEMS.join(", "))),
        Some(i) => crate::help::run(app, ctx, i),
        None => {}
    }
    ok(
        json!({"diagnostics": crate::help::diagnostics(app), "version": crate::help::VERSION, "commit": crate::help::COMMIT, "built": crate::help::BUILD_DATE}),
    )
}

/// `ui.render {path?, width?, height?, grid?, axes?}`: a headless render of the view.
fn ui_render(app: &mut SolveApp, q: params::Render) -> Outcome {
    let w = q.width.clamp(16.0, 8192.0) as u32;
    let h = q.height.clamp(16.0, 8192.0) as u32;
    // As the window shows it (`view_scene`). The grid and the axes as the window shows
    // them, unless the request says: `grid` and `axes` leave them out or put them in.
    let shown = window_guides(app);
    let guides = solidcraft_engine::view::Guides { grid: q.grid.unwrap_or(shown.grid), axes: q.axes.unwrap_or(shown.axes) };
    // A path outside the request's folders is refused before anything is drawn.
    if let Some(Err(e)) = q.path.as_deref().map(vfs::check) {
        return err(e);
    }
    let scene = match view_scene(app, guides) {
        Ok(scene) => scene,
        Err(e) => return err(e),
    };
    let png = match solidcraft_engine::capture::png(&scene, &app.cam, w, h) {
        Ok(png) => png,
        Err(e) => return err(format!("render failed: {e}")),
    };
    // What drew it (gpu, gpu-software or cpu), and the warning a CPU render carries.
    let (renderer, warning) = (png.renderer, solidcraft_engine::capture::warning(png.renderer));
    match q.path {
        Some(path) => match vfs::check(&path).and_then(|()| std::fs::write(&path, &png.bytes)) {
            Ok(()) => ok(json!({"path": path, "bytes": png.bytes.len(), "renderer": renderer, "warning": warning})),
            Err(e) => err(e),
        },
        None => ok(json!({"bytes": png.bytes.len(), "renderer": renderer, "warning": warning})),
    }
}

/// `app.quit {force?}`: quit, asking about unsaved designs unless forced.
fn app_quit(app: &mut SolveApp, ctx: &egui::Context, q: params::Quit) -> Outcome {
    // `force`: quit without asking about unsaved designs.
    if q.force {
        app.quit_requested = true;
    } else {
        crate::documents::request_quit(app);
    }
    if app.quit_requested {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    ok(json!({"quitting": app.quit_requested, "asking": app.docs.closing.is_some()}))
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
    let bytes = solidcraft_control_egui::screenshot::encode(image, path)?;
    vfs::write(path, &bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    fn panel_command_app() -> crate::SolveApp {
        use crate::panel_runtime::*;
        let h = PanelMountHarness::new(false).unwrap();
        let catalogue = solidcraft_engine::panels::fixture_command_catalogue(h.runtime.layout().registry()).unwrap();
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.enable_panel_runtime(h.runtime, Box::<FakeAppPanelServices>::default()).unwrap();
        app.panel_runtime_mut().unwrap().set_command_adapters(Box::new(catalogue), Box::new(FakeNativeAdapter::new(false))).unwrap();
        app.enable_universal_panel_host(HostOptions::default()).unwrap();
        prepare_panel_commands(&mut app);
        app
    }
    fn prepare_panel_commands(app: &mut crate::SolveApp) {
        use solidcraft_engine::panels::{LogicalRect, Point, Size};
        let capacity = LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1400.0, height: 1000.0 } };
        app.panel_runtime.as_mut().unwrap().prepare_host(&app.session, capacity).unwrap();
    }
    fn panel_control(app: &mut crate::SolveApp, method: &str, params: serde_json::Value) -> serde_json::Value {
        let (request, _) = super::ControlRequest::new(method, params);
        match super::handle(app, &egui::Context::default(), &request) {
            super::Outcome::Done(reply) => reply,
            _ => panic!("unexpected screenshot"),
        }
    }
    fn panel_steps() -> Vec<serde_json::Value> {
        vec![
            json!({"command":"panel.open","params":{"kind":"fixture.log","instance":"log","destination":{"type":"new_section","section":"logs","index":0,
            "location":{"type":"dock","zone":"left"},"size":{"width":280,"height":240}}}}),
            json!({"command":"panel.collapse","params":{"target":{"type":"instance","id":"log"},"collapsed":true}}),
        ]
    }
    #[test]
    fn panel_control_single_atomic_batch_and_script_share_live_runtime_without_geometry_or_camera_work() {
        use solidcraft_engine::panels::ActionSink;
        let mut single = panel_command_app();
        let mut batch = panel_command_app();
        let mut script = panel_command_app();
        // A one-body design with a deliberately different camera exercises the
        // old first-body-fit condition, rather than making it vacuously false.
        for app in [&mut single, &mut batch, &mut script] {
            app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            app.cam.distance *= 1.5;
            app.session.mark_saved();
        }
        let doc = single.session.doc.clone();
        let model = single.session.model.state();
        let camera = single.cam;
        let meshes: Vec<_> = model.bodies.iter().map(|body| body.cached_mesh()).collect();
        let invariant = (single.session.document_id.clone(), single.session.revision, single.session.undo.len(), single.session.is_dirty());
        for step in panel_steps() {
            assert_eq!(panel_control(&mut single, "engine.execute", step)["ok"], true);
        }
        assert_eq!(panel_control(&mut batch, "engine.batch", json!({"commands":panel_steps(),"param_check":"strict"}))["ok"], true);
        assert_eq!(panel_control(&mut script, "engine.script", json!({"commands":panel_steps(),"param_check":"strict"}))["ok"], true);
        let live = single.panel_runtime().unwrap().layout().inspect();
        assert_eq!(batch.panel_runtime().unwrap().layout().layout(), &live.layout);
        assert_eq!(script.panel_runtime().unwrap().layout().layout(), &live.layout);
        assert_eq!(panel_control(&mut single, "engine.execute", json!({"command":"layout.get"}))["result"]["state"]["revision"], live.revision);
        assert!(crate::agent_cursor::picks(&single, "panel.open", &panel_steps()[0]["params"]).is_empty());
        assert_eq!(single.cam, camera);
        assert_eq!((single.session.document_id.clone(), single.session.revision, single.session.undo.len(), single.session.is_dirty()), invariant);
        assert!(std::sync::Arc::ptr_eq(&doc, &single.session.doc));
        assert!(std::sync::Arc::ptr_eq(&model, &single.session.model.state()));
        for (old, new) in meshes.iter().zip(single.session.model.state().bodies.iter().map(|body| body.cached_mesh())) {
            match (old, new) {
                (Some(a), Some(b)) => assert!(std::sync::Arc::ptr_eq(a, &b)),
                (None, None) => {}
                _ => panic!("layout changed tessellation cache"),
            }
        }
    }
    #[test]
    fn panel_control_reports_structured_missing_capability_unknown_fields_and_lifecycle_batch_errors() {
        use solidcraft_engine::panels::ActionSink;
        let mut disabled = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let reply = panel_control(&mut disabled, "engine.execute", json!({"command":"layout.get"}));
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["panel_error"]["rule"], "capability");
        let mut app = panel_command_app();
        for check in ["off", "warn", "strict"] {
            let reply = panel_control(&mut app, "engine.execute", json!({"command":"panel.list","params":{"unknown":true},"param_check":check}));
            assert_eq!(reply["ok"], false);
            assert_eq!(reply["panel_error"]["rule"], "schema");
            assert_eq!(reply["param_check"], check);
        }
        assert_eq!(panel_control(&mut app, "engine.execute", panel_steps()[0].clone())["ok"], true);
        let before = app.panel_runtime().unwrap().layout().inspect();
        let reply = panel_control(
            &mut app,
            "engine.batch",
            json!({"commands":[
            {"command":"panel.close","params":{"target":{"type":"instance","id":"log"}}},
            {"command":"panel.open","params":{"kind":"fixture.log","instance":"log"}}]}),
        );
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["panel_error"]["rule"], "lifecycle");
        assert_eq!(app.panel_runtime().unwrap().layout().inspect(), before);
        // Refused before its first step, nothing ran: `rolled_back` as a rollback that went back
        // reports it (#10749).
        let refused = json!([panel_steps()[1].clone(), {"command":"solid.boxx","params":{}}]);
        let reply = panel_control(&mut app, "engine.batch", json!({"commands":refused.clone()}));
        assert_eq!((&reply["result"]["completed"], &reply["result"]["rolled_back"]), (&json!(0), &json!(true)), "{reply}");
        let reply = panel_control(&mut app, "engine.batch", json!({"commands":refused,"rollback":false}));
        assert_eq!(reply["result"]["rolled_back"], false, "{reply}");
        assert_eq!(app.panel_runtime().unwrap().layout().inspect(), before);
    }
    #[test]
    fn panel_control_catalogue_batches_and_scripts_retain_memory_and_refuse_unsupported_atomic_domains() {
        let mut app = panel_command_app();
        assert_eq!(panel_control(&mut app, "engine.execute", panel_steps()[0].clone())["ok"], true);
        let reply = panel_control(
            &mut app,
            "engine.batch",
            json!({"commands":[
            {"command":"layout.save","params":{"name":"Control saved"}},
            {"command":"layout.reset"}, {"command":"layout.apply","params":{"name":"Control saved"}}, {"command":"layout.get"}]}),
        );
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["completed"], 4);
        assert_eq!(reply["result"]["results"][3]["state"]["layout"]["instances"][0]["id"], "log");
        let mixed = json!({"commands":[{"command":"layout.get"},{"command":"file.new"}]});
        let before = app.session.document_id.clone();
        let reply = panel_control(&mut app, "engine.batch", mixed);
        assert_eq!(reply["panel_error"]["rule"], "capability");
        assert_eq!(app.session.document_id, before);
        let reply = panel_control(
            &mut app,
            "engine.script",
            json!({"commands":[
            {"command":"layout.workspace.select","params":{"workspace":"fixture.other"}},
            {"command":"layout.apply","params":{"name":"Control saved"}}, {"command":"layout.get"}]}),
        );
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"][2]["state"]["layout"]["workspace"], "fixture.other");
    }

    #[test]
    fn null_panel_params_match_single_atomic_sequential_and_script_routes() {
        let mut app = panel_command_app();
        let expected = app.run("panel.list", json!({})).unwrap();
        assert_eq!(app.run("panel.list", serde_json::Value::Null).unwrap(), expected);
        let single = panel_control(&mut app, "engine.execute", json!({"command":"panel.list","params":null}));
        assert_eq!(single["result"], expected);
        for (method, rollback) in [("engine.batch", true), ("engine.batch", false), ("engine.script", false)] {
            let reply = panel_control(
                &mut app,
                method,
                json!({"rollback":rollback,"commands":[
                    {"command":"panel.list","params":null},{"command":"layout.get","params":null},
                ]}),
            );
            assert_eq!(reply["ok"], true, "{method}: {reply}");
            let results = if method == "engine.script" { &reply["result"] } else { &reply["result"]["results"] };
            assert_eq!(results[0], expected, "{method}");
            assert_eq!(results[1]["state"], expected["state"]);
        }
    }

    #[test]
    fn null_normal_document_params_remain_accepted_in_panel_containing_sequences() {
        for method in ["engine.batch", "engine.script"] {
            let mut app = panel_command_app();
            let previous = app.session.document_id.clone();
            let reply = panel_control(
                &mut app,
                method,
                json!({"rollback":false,"commands":[
                    {"command":"file.new","params":null},{"command":"panel.list","params":null},{"command":"parameters.list","params":null},
                ]}),
            );
            assert_eq!(reply["ok"], true, "{reply}");
            assert!(reply["result"].get("failed").is_none(), "{reply}");
            assert_ne!(app.session.document_id, previous);
            assert_eq!(app.panel_runtime().unwrap().environment().document.as_ref().unwrap().0, app.session.document_id);
        }
    }
    #[test]
    fn blur_control_validates_before_mutation_and_reports_no_gpu() {
        let mut h = crate::scenario::Harness::new();
        for invalid in [json!(-1), json!(61), json!("18"), json!(null)] {
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

    #[test]
    fn blur_evidence_fixtures_are_atomic_transient_and_never_fake_support() {
        let mut h = crate::scenario::Harness::new();
        let source: Value = serde_json::from_str(include_str!("../tests/fixtures/theme-blur.json")).unwrap();
        assert_eq!(h.call("ui.prefs", json!({"theme_import":source,"theme_options":{"blur-review":{"blur":40}}}))["ok"], true);
        h.frames(2);
        let saved = h.app.preferences.theme_options.clone();
        let revision = h.app.session.revision;
        for invalid in [json!(null), json!([]), json!("available"), json!("unknown")] {
            assert_eq!(h.call("ui.blur", json!({"fixture":invalid,"radius_px":8}))["ok"], false);
            assert_eq!(h.app.blur.radius(), 40.0);
            assert_eq!(h.app.blur_session.harness_radius, None);
        }
        for fixture in ["gpu-software", "reduce-transparency", "web-unsupported"] {
            let reply = h.call("ui.blur", json!({"fixture":fixture}));
            assert_eq!(reply["result"]["evidence_fixture"], fixture);
            assert_eq!(reply["result"]["available"], false);
            h.frames(2);
            assert_eq!(h.app.blur.counters().textures_created, 0);
            assert_eq!(h.app.blur.counters().markers_emitted, 0);
            assert_eq!(h.app.preferences.theme_options, saved);
        }
        h.call("ui.blur", json!({"fixture":"clear"}));
        h.call("ui.blur", json!({"fixture":"guard-60"}));
        assert_eq!((h.app.blur_session.guard.cap, h.app.blur_session.guard.notice), (4, true));
        assert_eq!(h.app.blur_session.evidence_fixture, Some("guard-60"));
        h.app.blur_session.guard.undo();
        assert_eq!((h.app.blur_session.guard.cap, h.app.blur_session.guard.opted_out), (5, true));
        h.call("ui.blur", json!({"fixture":"guard-60"}));
        assert_eq!(h.app.blur_session.guard.cap, 5);
        assert_eq!(h.app.session.revision, revision, "quality actions never enter document undo");
        assert_eq!(h.app.preferences.theme_options, saved);
        h.call("ui.blur", json!({"fixture":"clear"}));
        assert!(h.app.blur_session.evidence_fixture.is_none());
        assert!(matches!(h.app.blur.capability(), crate::blur::Capability::Unavailable(_)), "clearing a fixture cannot manufacture a GPU");
    }

    use serde_json::json;
    use solidcraft_engine::panels::ActionSink;
    use solidcraft_engine::{Sel, Session};

    use super::*;
    use crate::Services;

    #[test]
    fn contextual_command_metadata_matches_live_focus_without_side_effects() {
        fn metadata(app: &mut SolveApp, reason_key: Option<&str>) {
            let enabled = reason_key.is_none();
            let document = app.session.doc.clone();
            let model = app.session.model.state();
            let revision = app.session.revision;
            let evaluations = app.session.evaluation_count();
            let undo = app.session.undo.len();
            let before = crate::panel_runtime::inspect_host_specimen(app).unwrap();
            let command = solidcraft_engine::find_command("panel.focus.next").unwrap();
            let interactive = crate::render_workspace::interactive_command_info(app, command);
            let commands = execute(app, "engine.commands", json!({})).unwrap();
            let row = commands.as_array().unwrap().iter().find(|row| row["id"] == "panel.focus.next").unwrap();
            assert_eq!(interactive.enabled, enabled);
            assert_eq!(row["enabled"], enabled);
            assert_eq!(row["disabled_reason"], json!(interactive.disabled_reason));
            assert_eq!(row["disabled_reason"], json!(reason_key.map(|key| app.tr(key))), "each context reports its exact localized refusal");
            assert_eq!(crate::panel_runtime::inspect_host_specimen(app).unwrap(), before);
            assert!(std::sync::Arc::ptr_eq(&document, &app.session.doc));
            assert!(std::sync::Arc::ptr_eq(&model, &app.session.model.state()));
            assert_eq!(app.session.revision, revision);
            assert_eq!(app.session.evaluation_count(), evaluations);
            assert_eq!(app.session.undo.len(), undo);
        }
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        metadata(&mut h.app, Some("ui.panel_host.rule.availability"));
        crate::panel_runtime::control_specimen(&mut h.app, &json!({"preset":"docks"})).unwrap();
        metadata(&mut h.app, Some("ui.panel_host.rule.availability")); // Host exists, but has no presented frame.
        h.frames(4);
        let header = h
            .app
            .universal_panel_frame()
            .unwrap()
            .controls
            .iter()
            .find(|control| {
                control.enabled
                    && matches!(
                        control.target,
                        crate::panel_runtime::HostControlTarget::Pointer(crate::panel_runtime::HostPointerTarget::Header { .. })
                    )
            })
            .unwrap()
            .clone();
        h.ctx.memory_mut(|memory| memory.request_focus(header.id));
        metadata(&mut h.app, None);
        h.ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique("foreign-contextual-focus")));
        metadata(&mut h.app, Some("ui.panel_host.rule.availability"));
        h.ctx.memory_mut(|memory| memory.request_focus(header.id));
        let old_document_id = h.app.session.document_id.clone();
        h.app.session.document_id = "metadata-incoming-document".into();
        metadata(&mut h.app, Some("ui.panel_host.rule.availability"));
        h.app.session.document_id = old_document_id;
        metadata(&mut h.app, None);
        let section = h.app.universal_panel_frame().unwrap().layout.sections[0].section.clone();
        let stale_revision = h.app.universal_panel_frame().unwrap().layout.revision;
        h.app
            .dispatch_panel_layout(solidcraft_engine::panels::LayoutAction::Apply {
                expected_revision: Some(stale_revision),
                mutations: vec![solidcraft_engine::panels::Mutation::Collapse { section, collapsed: true }],
            })
            .unwrap();
        assert_ne!(h.app.panel_runtime().unwrap().layout().inspect().revision, stale_revision);
        metadata(&mut h.app, Some("ui.panel_host.rule.revision"));
    }

    #[test]
    fn panel_fixture_validates_whole_request_before_activation() {
        let mut app = SolveApp::new(Session::default(), Services::default());
        let doc = app.session.doc.clone();
        let model = app.session.model.state();
        let camera = app.cam;
        for params in [
            Value::Null,
            json!([]),
            json!({"enabled":null}),
            json!({"enabled":"true"}),
            json!({"preset":false}),
            json!({"preset":"unknown"}),
            json!({"preset":"x".repeat(4096)}),
            json!({"enabled":false,"preset":"docks"}),
            json!({"enabled":true,"unknown":0}),
            json!({"preset":"surfaces","translucency_percent":-1}),
            json!({"preset":"surfaces","translucency_percent":101}),
            json!({"preset":"surfaces","translucency_percent":"50"}),
            json!({"preset":"surfaces","translucency_percent":null}),
            json!({"translucency_percent":50}),
            json!({"reduced_motion":true}),
            json!({"preset":"docks","reduced_motion":null}),
            json!({"enabled":false,"reduced_motion":true}),
            json!({"enabled":false,"translucency_percent":50}),
        ] {
            assert!(crate::panel_runtime::control_specimen(&mut app, &params).is_err(), "{params}");
            assert!(app.panel_runtime().is_none());
            assert!(!app.universal_panel_host_enabled());
            assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc));
            assert!(std::sync::Arc::ptr_eq(&model, &app.session.model.state()));
            assert_eq!(app.cam, camera);
        }
        let reply = crate::panel_runtime::control_specimen(&mut app, &json!({})).unwrap();
        assert_eq!(reply["enabled"], false);
        assert!(reply["host"].is_null());
    }

    #[test]
    fn panel_fixture_presets_mount_retained_content_and_inspection_does_not_tick() {
        let mut h = crate::scenario::Harness::new();
        for (name, count) in [
            ("docks", 4),
            ("tabs", 4),
            ("collapsed", 4),
            ("floating", 4),
            ("overflow", 24),
            ("pinned", 4),
            ("columns", 3),
            ("stack_tabs", 4),
            ("content_fill", 2),
            ("inline", 1),
            ("surfaces", 6),
        ] {
            let before = crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap()["session"].clone();
            h.step(&json!({"panel_fixture":{"enabled":true,"preset":name}})).unwrap();
            h.frames(2);
            assert!(h.app.universal_panel_host_enabled());
            assert!(h.app.universal_panel_frame().is_some());
            let first = crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap();
            let second = crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap();
            assert_eq!(first, second, "inspection itself must not tick or dispatch");
            assert_eq!(first["runtime"]["probe"]["preset"], name);
            assert_eq!(first["runtime"]["controller_count"], count);
            assert_eq!(first["session"], before, "host-only work changed Session for {name}");
            assert_eq!(first["runtime"]["probe"]["content_command_calls"], 0);
            let takeover = crate::panel_runtime::control_specimen(&mut h.app, &json!({"enabled":true,"preset":"tabs"}));
            assert!(takeover.is_err());
            assert_eq!(crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap(), second);
            h.step(&json!({"panel_fixture":{"enabled":false}})).unwrap();
            assert!(h.app.panel_runtime().is_some());
            assert!(h.app.universal_panel_host_enabled());
        }
    }

    #[test]
    fn panel_surface_fixture_converts_translucency_without_saving_preferences() {
        for percent in [0, 50, 100] {
            let mut h = crate::scenario::Harness::new();
            let preferences = serde_json::to_value(&h.app.preferences).unwrap();
            crate::panel_runtime::control_specimen(&mut h.app, &json!({"preset":"surfaces","translucency_percent":percent})).unwrap();
            h.frames(4);
            let reply = crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap();
            let sections = reply["host"]["sections"].as_array().unwrap();
            assert!(!sections.is_empty());
            for section in sections {
                let alpha = section["paint_alpha"].as_u64().unwrap();
                assert!(
                    alpha >= ((100 - percent) as f64 * 255.0 / 100.0).ceil() as u64,
                    "readability protection can only increase the requested opacity: {section}"
                );
                assert!(!section["contrast"].is_null(), "every painted surface reports its contrast decision");
            }
            assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), preferences);
        }
    }

    #[test]
    fn panel_fixture_live_opacity_updates_retain_controllers_layout_and_document() {
        let mut h = crate::scenario::Harness::new();
        crate::panel_runtime::control_specimen(&mut h.app, &json!({"preset":"surfaces","translucency_percent":0})).unwrap();
        h.frames(4);
        let document = h.app.session.doc.clone();
        let model = h.app.session.model.state();
        let camera = h.app.cam;
        let preferences = serde_json::to_value(&h.app.preferences).unwrap();
        let before = crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap();
        let sections = h
            .app
            .universal_panel_frame()
            .unwrap()
            .layout
            .sections
            .iter()
            .map(|section| (section.section.clone(), section.outer, section.body, section.surface))
            .collect::<Vec<_>>();
        for percent in [50, 100, 0] {
            let prior = crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap();
            let updated = crate::panel_runtime::control_specimen(&mut h.app, &json!({"translucency_percent":percent})).unwrap();
            assert_eq!(updated["runtime"], prior["runtime"], "updating opacity does not tick, dispatch or replace content");
            h.frames(4);
            let after = crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap();
            assert_eq!(after["runtime"]["layout"], before["runtime"]["layout"]);
            for (old, new) in
                before["runtime"]["probe"]["controllers"].as_array().unwrap().iter().zip(after["runtime"]["probe"]["controllers"].as_array().unwrap())
            {
                assert_eq!(new["controller_identity"], old["controller_identity"]);
                assert_eq!(new["draft"], old["draft"]);
                assert_eq!(new["transaction_live"], old["transaction_live"]);
            }
            assert_eq!(
                h.app
                    .universal_panel_frame()
                    .unwrap()
                    .layout
                    .sections
                    .iter()
                    .map(|section| (section.section.clone(), section.outer, section.body, section.surface))
                    .collect::<Vec<_>>(),
                sections
            );
            assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
            assert!(std::sync::Arc::ptr_eq(&model, &h.app.session.model.state()));
            assert_eq!(h.app.cam, camera);
            assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), preferences);
            if percent == 0 {
                assert!(h.app.universal_panel_frame().unwrap().layout.sections.iter().all(|section| section.paint_alpha == Some(255)));
            }
        }
        h.app.disable_universal_panel_host().unwrap();
        assert!(crate::panel_runtime::control_specimen(&mut h.app, &json!({"translucency_percent":50})).is_err());
    }

    #[test]
    fn panel_fixture_language_and_motion_refresh_preserve_layout_controllers_and_session() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
        crate::panel_runtime::control_specimen(&mut h.app, &json!({"preset":"surfaces"})).unwrap();
        h.frames(4);
        let runtime = h.app.panel_runtime().unwrap();
        let instance = runtime.layout().layout().instances.iter().find(|instance| instance.kind.0 == "fixture.editor").unwrap().id.clone();
        h.app.run("panel.focus", json!({"target":{"type":"instance","id":instance}})).unwrap();
        h.frames(4);
        let owner = h.app.panel_runtime().unwrap().instance_owner(&instance).unwrap();
        let draft_id = egui::Id::unique(("specimen-draft", &instance, &owner));
        h.ctx.memory_mut(|memory| memory.request_focus(draft_id));
        h.frame_with(vec![egui::Event::Text("retained locale selection".into())]);
        let mut state = egui::TextEdit::load_state(&h.ctx, draft_id).unwrap();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(1), egui::text::CCursor::new(4))));
        state.store(&h.ctx, draft_id);
        h.frames(1);
        let selection = egui::TextEdit::load_state(&h.ctx, draft_id).unwrap().cursor.char_range();
        assert!(!selection.unwrap().is_empty(), "exercise an actual nonempty draft selection");
        assert_eq!(h.ctx.memory(|memory| memory.focused()), Some(draft_id));
        let before = crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap();
        let document = h.app.session.doc.clone();
        let model = h.app.session.model.state();
        let camera = h.app.cam;
        for (language, expected) in [("ar", "rtl"), ("he", "rtl"), ("en", "ltr")] {
            h.app.preferences.language = language.into();
            h.frames(4);
            assert_eq!(h.ctx.memory(|memory| memory.focused()), Some(draft_id), "locale changes retain the actual focused draft");
            assert_eq!(egui::TextEdit::load_state(&h.ctx, draft_id).unwrap().cursor.char_range(), selection);
            let after = crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap();
            assert_eq!(after["host"]["direction"], expected);
            assert_eq!(after["runtime"]["layout"], before["runtime"]["layout"]);
            for (old, new) in
                before["runtime"]["probe"]["controllers"].as_array().unwrap().iter().zip(after["runtime"]["probe"]["controllers"].as_array().unwrap())
            {
                assert_eq!(new["controller_identity"], old["controller_identity"]);
                assert_eq!(new["draft"], old["draft"]);
                assert_eq!(new["transaction_live"], old["transaction_live"]);
            }
            assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
            assert!(std::sync::Arc::ptr_eq(&model, &h.app.session.model.state()));
            assert_eq!(h.app.cam, camera);
        }
        for reduced in [true, false] {
            let updated = crate::panel_runtime::control_specimen(&mut h.app, &json!({"reduced_motion":reduced})).unwrap();
            assert_eq!(updated["runtime"]["reduced_motion"], reduced);
            assert_eq!(updated["runtime"]["layout"], before["runtime"]["layout"]);
            h.frames(4);
            assert_eq!(h.ctx.memory(|memory| memory.focused()), Some(draft_id), "motion updates retain the actual focused draft");
            assert_eq!(egui::TextEdit::load_state(&h.ctx, draft_id).unwrap().cursor.char_range(), selection);
        }
    }

    #[test]
    fn panel_fixture_teardown_honors_live_editor_guard_atomically() {
        fn click_visible_guard(h: &mut crate::scenario::Harness, guard: &str) {
            for _ in 0..16 {
                let body =
                    h.app.universal_panel_frame().unwrap().layout.sections.iter().find(|section| section.instance.0 == "specimen-0").unwrap().body;
                let body = crate::panel_runtime::egui_rect(body.expect("expanded specimen body"));
                let checkbox = h
                    .output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .find(|(_, node)| node.role() == egui::accesskit::Role::CheckBox && node.label() == Some(guard))
                    .and_then(|(_, node)| node.bounds())
                    .map(|bounds| {
                        egui::Rect::from_min_max(egui::pos2(bounds.x0 as f32, bounds.y0 as f32), egui::pos2(bounds.x1 as f32, bounds.y1 as f32))
                    });
                if let Some(checkbox) = checkbox.filter(|bounds| body.contains(bounds.center())) {
                    assert!(checkbox.intersect(body).is_positive(), "the real checkbox must be visible inside its actual Body clip");
                    let at = checkbox.center();
                    h.frame_with(vec![egui::Event::PointerMoved(at)]);
                    for pressed in [true, false] {
                        h.frame_with(vec![
                            egui::Event::PointerMoved(at),
                            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                        ]);
                    }
                    return;
                }
                // The specimen owns this ScrollArea. Accessibility also lists
                // clipped content, so reveal the real checkbox before clicking.
                let at = egui::pos2(body.center().x, body.top() + 4.0);
                h.frame_with(vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::MouseWheel {
                        source: egui::MouseWheelSource::Unknown,
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -120.0),
                        modifiers: egui::Modifiers::NONE,
                        phase: egui::TouchPhase::Move,
                    },
                ]);
                for _ in 0..120 {
                    if !h.ctx.input(|input| input.is_scrolling()) {
                        break;
                    }
                    h.frame();
                }
                assert!(!h.ctx.input(|input| input.is_scrolling()), "the checkbox click requires a settled Body scroll");
            }
            panic!("the actual specimen guard checkbox was never revealed in its Body");
        }
        let mut h = crate::scenario::Harness::new();
        h.step(&json!({"panel_fixture":{"preset":"docks"}})).unwrap();
        let guard = h.app.tr("ui.panel_fixture.guard").to_string();
        click_visible_guard(&mut h, &guard);
        let before = crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap();
        let editor = &before["runtime"]["probe"]["controllers"][0];
        assert_eq!(editor["reject_cancel"], true);
        assert_eq!(editor["transaction_live"], true);
        assert!(crate::panel_runtime::control_specimen(&mut h.app, &json!({"enabled":false})).is_err());
        assert_eq!(crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap(), before);
        click_visible_guard(&mut h, &guard);
        h.step(&json!({"panel_fixture":{"enabled":false}})).unwrap();
        let layout = h.app.panel_runtime().unwrap().layout().layout();
        let browser = solidcraft_engine::panels::InstanceId::new("browser").unwrap();
        assert!(layout.instance(&browser).is_ok(), "the workspace restores production panels after teardown");
    }

    #[test]
    fn panel_fixture_rejects_takeover_and_teardown_of_unrelated_runtime() {
        let harness = crate::panel_runtime::PanelMountHarness::new(false).unwrap();
        #[derive(Default)]
        struct Provider {
            models: crate::panel_runtime::FakeModels,
            commands: crate::panel_runtime::FakeContentCommands,
            local: crate::panel_runtime::FakeLocalServices,
        }
        impl crate::panel_runtime::AppPanelServices for Provider {
            fn refresh_read_only(&mut self, _: &Session) -> solidcraft_engine::panels::PanelResult<()> {
                Ok(())
            }
            fn services(&mut self) -> crate::panel_runtime::ContentServices<'_> {
                crate::panel_runtime::ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
            }
        }
        let mut app = SolveApp::new(Session::default(), Services::default());
        app.panel_runtime = Some(crate::panel_runtime::AppPanelRuntime::new(harness.runtime, Box::<Provider>::default()).unwrap());
        let before = app.panel_runtime().unwrap().layout().inspect();
        for params in [json!({"enabled":true}), json!({"enabled":false})] {
            assert!(crate::panel_runtime::control_specimen(&mut app, &params).is_err());
            assert_eq!(app.panel_runtime().unwrap().layout().inspect(), before);
        }
    }

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
        use std::sync::atomic::AtomicBool;
        use std::time::Duration;
        static WATCH: DocumentWatch = DocumentWatch::new();
        let now = || WATCH.current();
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

    /// #3750: the "Recover unsaved design?" prompt is `ui.inspect`'s `dialog`, and `ui.recover`
    /// answers it without clicking: Discard deletes the autosave and closes the prompt.
    #[test]
    fn inspect_reports_the_recovery_prompt_and_ui_recover_discards() {
        use solidcraft_engine::recovery::{Autosaver, orphans};
        let dir = std::env::temp_dir().join(format!("solidcraft-recover-prompt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // An app that crashed with an unsaved box: its autosave is left unlocked.
        let mut crashed = Session::default();
        crashed.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let mut a = Autosaver::new(&dir, std::time::Duration::from_secs(3600)).unwrap();
        assert!(a.save(&crashed).unwrap());
        drop(a);
        let mut h = crate::scenario::Harness::new();
        crate::recovery_ui::start(&mut h.app, &dir, std::time::Duration::from_secs(3600));
        h.frames(2);
        let d = &h.call("ui.inspect", json!({}))["result"]["dialog"];
        assert_eq!(d["kind"], "recover_unsaved", "{d}");
        assert_eq!(d["actions"], json!(["recover", "discard", "later"]));
        assert_eq!(d["designs"][0]["features"], 1, "{d}");
        let id = d["designs"][0]["id"].as_str().unwrap().to_string();
        let r = h.call("ui.recover", json!({"action": "discard", "id": id}));
        assert_eq!(r["ok"], true, "{r}");
        assert!(h.call("ui.inspect", json!({}))["result"]["dialog"].is_null());
        assert!(orphans(&dir).iter().all(|e| e.id != id), "the autosave is deleted");
        drop(h);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A non-string `id` is refused (it is not "the first one"), and a recover the open-design
    /// limit refuses keeps its entry offered.
    #[test]
    fn ui_recover_refuses_a_non_string_id_and_keeps_an_entry_the_limit_refuses() {
        use solidcraft_engine::recovery::{Autosaver, orphans};
        let dir = std::env::temp_dir().join(format!("solidcraft-recover-refuse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut crashed = Session::default();
        crashed.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let mut a = Autosaver::new(&dir, std::time::Duration::from_secs(3600)).unwrap();
        assert!(a.save(&crashed).unwrap());
        drop(a);
        let mut h = crate::scenario::Harness::new();
        crate::recovery_ui::start(&mut h.app, &dir, std::time::Duration::from_secs(3600));
        h.frames(2);
        let id = h.app.recoverable[0].id.clone();
        let r = h.call("ui.recover", json!({"action": "discard", "id": 0}));
        assert_eq!(r["ok"], false, "{r}");
        assert!(r["error"].as_str().is_some_and(|e| e.contains("string")), "{r}");
        assert_eq!(h.app.recoverable.len(), 1, "nothing was discarded");
        assert!(orphans(&dir).iter().any(|e| e.id == id));
        // Full: 32 designs open and the active one is not blank.
        for _ in 1..crate::documents::MAX_OPEN {
            crate::documents::new_tab(&mut h.app);
        }
        h.app.session.path = Some("occupied.sc".into());
        let r = h.call("ui.recover", json!({"action": "recover", "id": id}));
        assert_eq!(r["ok"], false, "{r}");
        assert!(r["error"].as_str().is_some_and(|e| e.contains("too many open designs")), "{r}");
        assert_eq!(h.app.recoverable.len(), 1, "the entry is still offered");
        assert!(h.call("ui.inspect", json!({}))["result"]["dialog"]["designs"][0]["id"] == id.as_str());
        drop(h);
        let _ = std::fs::remove_dir_all(&dir);
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
            let want = if dark { solidcraft_theme::Tokens::dark() } else { solidcraft_theme::Tokens::light() }.viewport_top;
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
                assert_eq!(view_scene(&app, Default::default()).unwrap().plain_cap, !hatch);
                app.session.selection = vec![Sel::Plane { name: "XZ".into() }];
                app.start("inspect.section");
                let Some(crate::dialogs::Kind::Section { hatch: preview_hatch, .. }) = app.dialog.as_mut().map(|d| &mut d.kind) else {
                    panic!("no section dialog")
                };
                *preview_hatch = !hatch;
                assert_eq!(
                    view_scene(&app, Default::default()).unwrap().plain_cap,
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
        app.cam.set_view(solidcraft_viewport::StandardView::Front);
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

    /// A step of a long design leaves its components unplaced: the first-body fit's check
    /// (`execute`) looks at the undo history before placing the world, which used to place
    /// every component's bodies after every step of a batch.
    #[test]
    fn a_step_does_not_place_the_world_for_the_first_body_fit() {
        let mut h = crate::scenario::Harness::new();
        // The harness call settles frames, which legitimately place the world for drawing.
        let steps = super::params::parse(&json!({"commands": [
            {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10, "body_name": "Cube"}},
            {"command": "component.from_bodies", "params": {"bodies": ["Cube"]}},
            {"command": "occurrence.copy", "params": {"component": "Cube", "translate": [0, 40, 0]}},
        ]}))
        .expect("batch params");
        let Outcome::Done(v) = super::batch(&mut h.app, steps) else {
            panic!("a command batch cannot be a screenshot");
        };
        assert!(v["result"].get("failed").is_none(), "{v}");
        assert!(h.app.session.undo.len() > 2, "past the first-body fit");
        // Straight to `execute`, with no frame: drawing places the world too.
        assert!(h.app.session.cached_world_state().is_none());
        let camera = h.app.cam;
        let document = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len());
        let r = super::execute(&mut h.app, "select.clear", json!({}));
        assert!(r.is_ok(), "{r:?}");
        assert!(h.app.session.cached_world_state().is_none(), "clearing selection placed the components before the command");
        assert_eq!(h.app.cam, camera);
        assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
        let r = super::execute(&mut h.app, "solid.sphere", json!({"radius": 3, "center": [80, 0, 0], "operation": "new"}));
        assert!(r.is_ok(), "{r:?}");
        assert!(h.app.session.cached_world_state().is_none(), "the step placed the components again");
        assert_eq!(crate::open_meshing::session_world(&h.app.session).bodies.len(), 3);
    }

    #[test]
    fn excluded_control_view_commands_leave_a_cold_component_world_unplaced() {
        for id in ["view.grid", "VIEW.GRID", "view.tool.zoom", "VIEW.TOOL.ZOOM"] {
            let mut saved = Session::default();
            saved.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Cube"})).unwrap();
            saved.execute("component.from_bodies", &json!({"bodies":["Cube"]})).unwrap();
            let mut h = crate::scenario::Harness::new();
            h.app.session = Session::opened((*saved.doc).clone(), None);
            assert_eq!(h.app.session.undo.len(), 0, "history permits fitting, so only the command exclusion can prevent placement");
            assert!(!h.app.session.doc.occurrences.is_empty());
            assert!(h.app.session.cached_world_state().is_none());
            let camera = h.app.cam;
            let document = h.app.session.doc.clone();
            let r = super::execute(&mut h.app, id, json!({"on":false}));
            assert!(r.is_ok(), "{r:?}");
            assert_eq!(r.unwrap()["on"], false, "the engine accepted {id}");
            if id == "view.grid" {
                assert!(!h.app.ui.show_grid);
            }
            assert!(h.app.session.cached_world_state().is_none(), "{id} placed a world it cannot fit");
            assert_eq!(h.app.cam, camera);
            assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
            assert!(h.app.session.undo.is_empty() && h.app.session.redo.is_empty());
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

    /// #12629: each step of `engine.script` gets what the window does after the command, as the
    /// same step of `engine.batch` gets it: the grid, the tool, the spin, a preference, the echo.
    #[test]
    fn a_script_step_gets_the_windows_follow_up_like_a_batch_step() {
        let steps = json!([
            {"command": "view.grid", "params": {"on": false}},
            {"command": "view.tool", "params": {"tool": "pan"}},
            {"command": "view.spin", "params": {"on": true}},
            {"command": "view.angle_readout", "params": {"enabled": true}},
            {"command": "sketch.create", "params": {"plane": "XY"}},
            {"command": "sketch.finish", "params": {}},
        ]);
        let window = |method: &str| {
            let mut h = crate::scenario::Harness::new();
            // No checkpoint, so the batch runs exactly the script's steps.
            let v = h.call(method, json!({"commands": steps, "rollback": false}));
            assert_eq!(v["ok"], true, "{method}: {v}");
            let app = &h.app;
            (app.ui.show_grid, app.viewport.nav, app.viewport.spin, app.preferences.show_view_angles, app.session.log.clone())
        };
        let batch = window("engine.batch");
        assert_eq!((batch.0, batch.1, batch.2, batch.3), (false, Some(crate::viewport::NavMode::Pan), true, true));
        assert_eq!(window("engine.script"), batch);
    }

    /// The first failing step stops a script, and the steps before it stay with what the window
    /// did after them: a preference set by step 1 is kept when step 3 fails.
    #[test]
    fn a_script_stops_at_its_failing_step_and_keeps_the_ones_before() {
        let mut h = crate::scenario::Harness::new();
        assert!(!h.app.preferences.show_view_angles && h.app.ui.show_grid);
        let v = h.call(
            "engine.script",
            json!({"commands": [
                {"command": "view.angle_readout", "params": {"enabled": true}},
                {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}},
                {"command": "no.such_command"},
                {"command": "view.grid", "params": {"on": false}},
            ]}),
        );
        assert_eq!(v["ok"], false, "{v}");
        assert!(v["error"].as_str().is_some_and(|e| e.starts_with("step 3 (no.such_command): ")), "{v}");
        assert!(h.app.preferences.show_view_angles, "step 1's preference stays");
        assert_eq!(inspect(&mut h)["bodies"].as_array().map(Vec::len), Some(1), "step 2's box stays");
        assert!(h.app.ui.show_grid, "nothing after the failing step ran");
        let v = h.call("engine.script", json!([{"params": {}}]));
        assert_eq!(v["error"], "script step 1: missing `command`", "{v}");
    }

    /// A panel command among a script's steps no longer changes how the other steps run.
    #[test]
    fn a_script_with_and_without_a_panel_step_leaves_the_window_alike() {
        let window = |panel: bool| {
            let mut app = panel_command_app();
            let mut steps = vec![
                json!({"command": "view.grid", "params": {"on": false}}),
                json!({"command": "view.tool", "params": {"tool": "pan"}}),
                json!({"command": "view.angle_readout", "params": {"enabled": true}}),
            ];
            if panel {
                steps.push(json!({"command": "layout.get"}));
            }
            let v = panel_control(&mut app, "engine.script", json!({"commands": steps}));
            assert_eq!(v["ok"], true, "{v}");
            (app.ui.show_grid, app.viewport.nav, app.preferences.show_view_angles)
        };
        assert_eq!(window(false), window(true));
        assert_eq!(window(false), (false, Some(crate::viewport::NavMode::Pan), true));
        // A failing step of such a script is named by its number, with the panel's own reason.
        let mut app = panel_command_app();
        let steps = json!([{"command": "view.grid", "params": {"on": false}}, {"command": "panel.list", "params": {"unknown": true}}]);
        let v = panel_control(&mut app, "engine.script", json!({"commands": steps}));
        assert_eq!((&v["ok"], &v["step"], &v["panel_error"]["rule"]), (&json!(false), &json!(2), &json!("schema")), "{v}");
        assert!(!app.ui.show_grid, "the step before it stays");
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
        let v = batch(&mut h, json!({"commands": [{"command": "pointcloud.reload", "params": {"cloud": 1}}]}));
        assert_eq!(v["error"], "engine.batch unavailable: step 0 (pointcloud.reload) loads a file", "{v}");
        // In any spelling the engine takes.
        let v = batch(&mut h, json!({"commands": [{"command": "POINTCLOUD.RELOAD", "params": {"cloud": 1}}]}));
        assert_eq!(v["error"], "engine.batch unavailable: step 0 (pointcloud.reload) loads a file", "{v}");
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
        // Refused before its first step (#10749): nothing ran, so the design is where the batch
        // started, which `rolled_back` says as it does after a rollback that went back.
        let refused = json!([{"command": "solid.box", "params": {"length": 5, "width": 5, "height": 5}}, {"command": "solid.boxx", "params": {}}]);
        let v = batch(&mut h, json!({"commands": refused.clone()}));
        assert_eq!((&v["result"]["completed"], &v["result"]["not_run"]), (&json!(0), &json!([0, 1])), "{v}");
        assert_eq!((&v["result"]["rolled_back"], &v["result"]["undone"]), (&json!(true), &json!(0)), "{v}");
        let v = batch(&mut h, json!({"commands": refused, "rollback": false}));
        assert_eq!(v["result"]["rolled_back"], json!(false), "{v}");
        assert_eq!(inspect(&mut h)["bodies"].as_array().map(Vec::len), Some(1), "nothing ran");
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

    #[test]
    fn existing_single_body_control_queries_keep_the_current_camera_and_history() {
        let mut saved = Session::default();
        saved.execute("solid.box", &json!({"length":80,"width":50,"height":12,"body_name":"Subject"})).unwrap();
        let mut h = crate::scenario::Harness::new();
        crate::documents::open_tab(&mut h.app, Session::opened((*saved.doc).clone(), None)).unwrap();
        h.call("ui.resize", json!({"width":1440,"height":1000}));
        h.frames(4);
        crate::render_workspace::enter(&mut h.app, true).unwrap();
        h.frames(4);
        h.app.cancel_view_animation();
        h.app.cam.distance = 321.0;
        h.app.cam.yaw = 0.3;
        let camera = h.app.cam;
        let document = h.app.session.doc.clone();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        assert_eq!(h.app.session.world_state().unwrap().bodies.len(), 1);
        assert_eq!(h.app.session.undo.len(), 0, "a freshly opened one-body design exercises the old refit heuristic");
        for command in ["appearance.library", "render.settings.get", "render.scene"] {
            let reply = h.call("engine.execute", json!({"command":command,"params":{}}));
            assert_eq!(reply["ok"], true, "{command}: {reply}");
            h.frames(4);
            assert_eq!(h.app.cam, camera, "read-only {command} must keep the product framing");
            assert!(h.app.pending_fit.is_none());
            assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
            assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        }
        let reply = h.call(
            "engine.batch",
            json!({"commands":[
                {"command":"appearance.library","params":{}}, {"command":"render.scene","params":{}}
            ]}),
        );
        assert_eq!(reply["ok"], true, "{reply}");
        h.frames(4);
        assert_eq!(h.app.cam, camera);
        assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
    }

    #[test]
    fn control_undo_from_three_steps_to_two_still_fits_the_first_body() {
        for id in ["edit.undo", "EDIT.UNDO"] {
            let mut h = crate::scenario::Harness::new();
            h.frames(4);
            h.app.run("solid.box", json!({"length":80,"width":50,"height":12,"body_name":"Subject"})).unwrap();
            h.app.run("appearance.assign", json!({"bodies":["Subject"],"color":"#204080"})).unwrap();
            let feature = h.app.session.doc.features[0].id;
            h.app.run("timeline.delete", json!({"feature":feature})).unwrap();
            assert_eq!(h.app.session.undo.len(), 3);
            assert!(h.app.session.world_state().unwrap().bodies.is_empty());
            h.app.cancel_view_animation();
            h.app.cam.distance = 10_000.0;
            assert!(h.app.pending_fit.is_none());
            let r = super::execute(&mut h.app, id, json!({}));
            assert!(r.is_ok(), "{r:?}");
            assert_eq!(h.app.session.undo.len(), 2);
            assert_eq!(h.app.session.world_state().unwrap().bodies.len(), 1);
            assert!(h.app.pending_fit.is_some(), "Undo crossing the early-history boundary must retain the measured first-body fit");
            h.frames(4);
            assert!(h.app.pending_fit.is_none());
            assert!(h.app.cam.distance < 1000.0);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn control_finish_edit_from_a_long_empty_part_still_fits_the_returned_assembly() {
        for id in ["component.finish_edit", "COMPONENT.FINISH_EDIT"] {
            let mut h = crate::scenario::Harness::new();
            h.frames(4);
            let part_path = h.dir.join("part.sc");
            let assembly_path = h.dir.join("assembly.sc");
            let mut part = Session::default();
            part.execute("solid.box", &json!({"length":80,"width":50,"height":12,"body_name":"Subject"})).unwrap();
            part.execute("file.save", &json!({"path":part_path})).unwrap();
            let component = h.app.run("component.insert_design", json!({"path":part_path})).unwrap()["component"].as_u64().unwrap();
            h.app.run("file.save", json!({"path":assembly_path})).unwrap();
            h.settle();
            h.app.cancel_view_animation();
            h.app.cam.distance = 10_000.0;
            let assembly_camera = h.app.cam;
            let assembly = h.app.session.doc.clone();
            let assembly_history = (h.app.session.undo.len(), h.app.session.redo.len());
            assert_eq!(assembly_history, (1, 0));
            assert_eq!(h.app.session.world_state().unwrap().bodies.len(), 1);

            h.app.run("component.edit_in_place", json!({"component":component})).unwrap();
            h.settle();
            assert!(h.app.edit_in_place.active.is_some());
            assert_eq!(h.app.session.path.as_deref(), part_path.to_str());
            assert!(h.app.session.undo.is_empty());
            h.app.run("appearance.assign", json!({"bodies":["Subject"],"color":"#204080"})).unwrap();
            h.app.run("parameters.add", json!({"name":"clearance","expression":"1 mm"})).unwrap();
            let feature = h.app.session.doc.features[0].id;
            h.app.run("timeline.delete", json!({"feature":feature})).unwrap();
            h.app.run("file.save", json!({})).unwrap();
            assert_eq!(h.app.session.undo.len(), 3);
            assert!(!h.app.session.is_dirty());
            assert!(h.app.session.world_state().unwrap().bodies.is_empty());
            assert!(h.app.pending_fit.is_none());

            // Finish switches synchronously to the assembly's own saved history and
            // last loaded one-body model, before any background link refresh lands.
            let r = super::execute(&mut h.app, id, json!({}));
            assert!(r.is_ok(), "{id}: {r:?}");
            assert!(h.app.edit_in_place.active.is_none());
            assert_eq!(h.app.session.path.as_deref(), assembly_path.to_str());
            assert!(std::sync::Arc::ptr_eq(&assembly, &h.app.session.doc));
            assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), assembly_history);
            assert_eq!(h.app.session.world_state().unwrap().bodies.len(), 1);
            assert_eq!(
                (h.app.cam.yaw, h.app.cam.pitch, h.app.cam.roll, h.app.cam.fov, h.app.cam.up),
                (assembly_camera.yaw, assembly_camera.pitch, assembly_camera.roll, assembly_camera.fov, assembly_camera.up),
                "returning to the tab restores its orientation and projection while fitting its body"
            );
            assert!(h.app.pending_fit.is_some(), "{id}: a session swap must retain the measured first-body fit");
            h.settle();
            assert!(h.app.pending_fit.is_none());
            assert!(h.app.cam.distance < 1000.0);
            assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), assembly_history);
        }
    }

    #[test]
    fn control_first_body_creation_still_fits_but_later_appearance_edits_do_not() {
        let mut h = crate::scenario::Harness::new();
        h.frames(4);
        h.app.cancel_view_animation();
        h.app.cam.distance = 10_000.0;
        let (request, _receiver) =
            ControlRequest::new("engine.execute", json!({"command":"solid.box","params":{"length":80,"width":50,"height":12,"body_name":"Subject"}}));
        let Outcome::Done(reply) = handle(&mut h.app, &h.ctx, &request) else { panic!("a command reply cannot be a screenshot") };
        assert_eq!(reply["ok"], true, "{reply}");
        assert!(h.app.pending_fit.is_some(), "the first actual body must request the measured fit");
        h.frames(4);
        assert!(h.app.pending_fit.is_none());
        assert!(h.app.cam.distance < 1000.0);
        h.app.cam.yaw = 0.7;
        h.app.cam.distance = 321.0;
        let camera = h.app.cam;
        let undo = h.app.session.undo.len();
        let reply = h.call("engine.execute", json!({"command":"appearance.assign","params":{"bodies":["Subject"],"color":"#204080"}}));
        assert_eq!(reply["ok"], true, "{reply}");
        h.frames(4);
        assert_eq!(h.app.cam, camera);
        assert!(h.app.pending_fit.is_none());
        assert_eq!(h.app.session.undo.len(), undo + 1);
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
        // Settings profiles are read and written by the app, not the engine, and stay inside too (#11362).
        let profile = |id: &str, params: Value| json!({"command": id, "params": params});
        let poc = json!({"format": "solidcraft-profile/1", "name": "scope-poc"});
        assert_eq!(call("engine.execute", profile("settings.profile.import", json!({"profile": poc})), true)["ok"], true);
        let inside = json!({"name": "scope-poc", "path": at(&root, "p.json")});
        assert_eq!(call("engine.execute", profile("settings.profile.export", inside), true)["ok"], true);
        assert_eq!(call("engine.execute", profile("settings.profile.import", json!({"path": at(&root, "p.json")})), true)["ok"], true);
        std::fs::write(at(&base, "p.json"), poc.to_string()).unwrap();
        for (method, params) in [
            ("engine.execute", export(at(&out, "a.step"))),
            ("engine.execute", export(at(&root, "../out/a.step"))),
            ("engine.script", json!({"commands": [{"command": "file.export", "params": {"path": at(&out, "a.step")}}]})),
            ("ui.render", json!({"path": at(&out, "a.png"), "width": 32, "height": 32})),
            ("ui.screenshot", json!({"path": at(&out, "b.png")})),
            ("engine.execute", profile("settings.profile.export", json!({"name": "scope-poc", "path": at(&out, "p.json")}))),
            ("engine.execute", profile("settings.profile.import", json!({"path": at(&base, "p.json")}))),
            ("engine.execute", profile("settings.profile.import", json!({"path": at(&out, "missing.json")}))),
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
        for command in ["solid.fillet", "sketch.create", "inspect.zebra"] {
            let r = h.call("ui.start", json!({"command": command, "params": {"distance": 5}}));
            assert!(r["ok"] == false && r["error"].as_str().unwrap().contains("takes no ui.start params"), "{r}");
            assert!(h.app.dialog.is_none() && h.app.tool.is_none() && h.app.session.analysis.is_none(), "{command} started nothing");
        }
        // Extrude takes a sketch (#6940) and nothing else: an unknown param is refused by name.
        let r = h.call("ui.start", json!({"command": "solid.extrude", "params": {"distance": 5}}));
        assert!(r["ok"] == false && r["error"].as_str().unwrap().contains("unknown param `distance`"), "{r}");
        assert!(h.app.dialog.is_none() && h.app.tool.is_none(), "Extrude started nothing");
    }

    /// #9087: the dialog is reported as data (its command, inputs and what OK runs), not as the
    /// form's Debug text; `ui.start` and `ui.editFeature` reply with what they opened, and the
    /// tool windows (Parameters, Bill of Materials) are reported by the command that opens them.
    #[test]
    fn dialogs_and_tool_windows_are_reported_as_data() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let mut h = crate::scenario::Harness::new();
        h.app = SolveApp::new(s, Services::default());
        let r = h.call("ui.start", json!({"command": "solid.fillet"}));
        let d = &r["result"]["dialog"];
        assert_eq!((&d["command"], &d["title"], &d["editing"]), (&json!("solid.fillet"), &json!("FILLET"), &Value::Null), "{r}");
        assert_eq!((&d["inputs"][0]["active"], &d["inputs"][0]["items"]), (&json!(true), &json!([])), "{r}");
        assert!(d["commands"].is_null() && d["waiting"].is_string(), "no edge picked yet: {r}");
        assert_eq!((&r["result"]["pending"], &r["result"]["windows"]), (&json!(false), &json!([])), "{r}");
        assert_eq!(h.call("ui.inspect", json!({}))["result"]["dialog"], *d, "ui.inspect reports the same");
        crate::dialogs::cancel(&mut h.app);

        let feature = h.app.session.doc.features[0].id;
        let r = h.call("ui.editFeature", json!({"feature": feature}));
        let d = &r["result"]["dialog"];
        assert_eq!((&d["command"], &d["editing"]), (&json!("solid.box"), &json!(feature)), "{r}");
        // OK rebuilds the feature in place, from what the form holds.
        let runs = &d["commands"][0];
        assert_eq!((&runs["command"], &runs["params"]["command"]), (&json!("timeline.redefine"), &json!("solid.box")), "{r}");
        assert_eq!((&runs["params"]["feature"], &runs["params"]["params"]["length"]), (&json!(feature), &json!("40")), "{r}");
        assert!(r["result"].get("marker").is_some(), "{r}");
        crate::dialogs::cancel(&mut h.app);

        for (command, windows) in
            [("parameters.change", json!(["parameters.change"])), ("assembly.bom", json!(["parameters.change", "assembly.bom"]))]
        {
            let r = h.call("ui.start", json!({"command": command}));
            assert_eq!((&r["result"]["dialog"], &r["result"]["tool"], &r["result"]["windows"]), (&Value::Null, &Value::Null, &windows), "{r}");
            assert_eq!(h.call("ui.inspect", json!({}))["result"]["windows"], windows, "{command}");
        }
        h.call("ui.help", json!({"item": "about"}));
        assert_eq!(
            h.call("ui.inspect", json!({}))["result"]["windows"],
            json!(["parameters.change", "assembly.bom", "about"]),
            "the Help menu's windows too"
        );

        // What is under the pointer is a selection-like item, not the viewport's Debug text.
        h.app.viewport.hover = Some(crate::viewport::Hit::Face { body: "Body1".into(), index: 2, point: solidcore_record::Vec3::new(1.0, 2.0, 3.0) });
        let hover = json!({"type": "face", "body": "Body1", "index": 2, "point": [1.0, 2.0, 3.0]});
        assert_eq!(h.call("ui.selection", json!({}))["result"]["hover"], hover);
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
        // A refused sample leaves Home as it was, even beside `open: false`.
        h.call("ui.home", json!({"open": true}));
        assert_eq!(h.call("ui.home", json!({"open": false, "sample": 99}))["ok"], false);
        assert_eq!(h.call("ui.home", json!({}))["result"]["open"], true, "Home is still open");
        // A sample that exists still opens.
        let r = h.call("ui.home", json!({"sample": 0}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["result"]["open"], true, "the sample was asked for: {r}");
    }
}
