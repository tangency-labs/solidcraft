//! A pending joint drive on one desktop control connection. Only Finish edits the design.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Weak};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_engine::doc::Document;
use solidcraft_engine::io::vfs;
use solidcraft_engine::{ComputedCommands, Session};

use crate::control::ControlRequest;
use crate::preview::{Built, Colors};
use crate::{SolveApp, control_job};

pub(crate) const METHOD: &str = "control.joint_drive";

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Begin {
        document_id: String,
        joint: u64,
        #[serde(default)]
        index: usize,
    },
    Update {
        movement: u64,
        value: f64,
    },
    Finish {
        movement: u64,
    },
    Cancel {
        movement: u64,
    },
}

#[derive(Default)]
pub(crate) struct State {
    movement: Option<Movement>,
    update: Option<Update>,
}

impl State {
    pub(crate) fn active(&self) -> bool {
        self.movement.is_some()
    }
}

struct Movement {
    id: u64,
    owner: Weak<()>,
    design: Arc<Document>,
    session: u64,
    revision: u64,
    joint: u64,
    index: usize,
    prepared: Option<ComputedCommands>,
}

struct Update {
    /// Dropped when the request is answered early. Keep its receiver until that worker ends
    /// so repeated cancellations cannot accumulate preview workers.
    request: Option<ControlRequest>,
    start: f64,
    result: Receiver<Result<Prepared, String>>,
}

struct Prepared {
    command: ComputedCommands,
    scene: Built,
    reply: Value,
}

fn local_preview(app: &SolveApp) -> bool {
    app.dialog.is_some()
        || app.tool.is_some()
        || app.params.is_some()
        || app.applying.is_some()
        || app.tree.redefine.is_some()
        || crate::plugin_menu::editing_dialog_open(app)
        || app.session.active_sketch.is_some()
        || app.viewport.joint_handles.is_some()
        || crate::joint_drag::active(app)
        || crate::occurrence_drag::active(app)
}

fn owner(req: &ControlRequest) -> Result<&Weak<()>, String> {
    req.connection
        .as_ref()
        .filter(|connection| connection.strong_count() != 0)
        .ok_or_else(|| "joint drive requires a live desktop control connection".into())
}

fn matching<'a>(app: &'a mut SolveApp, req: &ControlRequest, id: u64) -> Result<&'a mut Movement, String> {
    let owner = owner(req)?;
    app.control_joint_drive
        .movement
        .as_mut()
        .filter(|movement| movement.id == id && movement.owner.ptr_eq(owner))
        .ok_or_else(|| "no matching joint drive movement on this connection".into())
}

fn begin(app: &mut SolveApp, req: &ControlRequest, document_id: &str, joint: u64, index: usize) -> Result<Value, String> {
    let owner = owner(req)?.clone();
    if app.session.document_id != document_id {
        return Err("the active design has changed".into());
    }
    if app.control_joint_drive.active() || local_preview(app) {
        return Err("another edit is in progress".into());
    }
    let joint = app.session.doc.assembly.joints.iter().find(|j| j.id == joint).ok_or("the joint does not exist in the active design")?;
    if joint.kind.dofs().get(index).is_none() {
        return Err("index is outside this joint's coordinates".into());
    }
    app.preview.clear();
    app.control_joint_drive.movement = Some(Movement {
        id: req.serial,
        owner,
        design: app.session.doc.clone(),
        session: app.session.uid,
        revision: app.session.revision,
        joint: joint.id,
        index,
        prepared: None,
    });
    Ok(json!({"movement": req.serial}))
}

fn prepare(copy: Session, joint: u64, index: usize, value: f64, colors: Colors) -> Result<Prepared, String> {
    let command = copy
        .compute_commands(&[("joint.drive".into(), json!({"joint": joint, "index": index, "value": value, "quiet": true}))], None)
        .map_err(|e| e.to_string())?;
    let mut preview = command.preview_visible_world().map_err(|e| e.to_string())?;
    let reply = preview.replies.pop().ok_or("joint drive returned no result")?;
    let colors = Colors { added: colors.body, new_body: colors.body, ..colors };
    let scene = crate::dialogs_assembly::build_world_preview(preview, colors)?;
    Ok(Prepared { command, scene, reply })
}

/// Start one update. Every target is evaluated from the committed source pose.
fn update(app: &mut SolveApp, ctx: &egui::Context, req: ControlRequest, scope: Option<vfs::PathScope>, id: u64, value: f64) {
    let target = matching(app, &req, id).map(|movement| (movement.joint, movement.index));
    let (joint, index) = match target {
        Ok(target) => target,
        Err(error) => {
            reply(&req, Err(error));
            return;
        }
    };
    if app.control_joint_drive.update.is_some() {
        reply(&req, Err("a joint drive update is still running".into()));
        return;
    }
    let mut copy = app.session.scratch();
    copy.param_check = solidcraft_engine::schema::ParamCheck::Strict;
    let mut colors = Colors::from_tokens(&solidcraft_theme::Tokens::get());
    colors.ghost = app.preferences.drive_joints_ghost;
    colors.tint_new = app.preferences.preview_tint_new_faces;
    colors.cut_tint = app.preferences.cut_preview_tint;
    let start = crate::now_ms();
    let work = move || prepare(copy, joint, index, value, colors);
    if app.apply_off_thread.is_none() || cfg!(target_arch = "wasm32") {
        let result = vfs::scoped(scope, work).and_then(|prepared| show(app, &req, start, prepared));
        reply(&req, result);
        return;
    }
    let progress = control_job::progress_of(&req.ticket);
    let wake = ctx.clone();
    let Some(result) = control_job::spawn(scope, progress, move || {
        let result = work();
        wake.request_repaint();
        result
    }) else {
        reply(&req, Err("could not start joint drive update".into()));
        return;
    };
    req.ticket.computing(false);
    app.control_joint_drive.update = Some(Update { request: Some(req), start, result });
    app.preview.busy = true;
    // The worker may wake just before its result is sent.
    ctx.request_repaint_after(Duration::from_millis(16));
}

fn show(app: &mut SolveApp, req: &ControlRequest, start: f64, prepared: Prepared) -> Result<Value, String> {
    expire(app);
    owner(req)?;
    if !prepared.command.is_current(&app.session) {
        discard(app);
        return Err("the design changed while joint drive computed".into());
    }
    let movement = app.control_joint_drive.movement.as_mut().ok_or("the joint drive movement ended")?;
    movement.prepared = Some(prepared.command);
    let revision = app.session.design_revision();
    app.preview.show_joint_drive(req.serial, start, prepared.scene, METHOD, revision);
    Ok(prepared.reply)
}

fn finish(app: &mut SolveApp, req: &ControlRequest, id: u64) -> Result<Value, String> {
    matching(app, req, id)?;
    if app.control_joint_drive.update.as_ref().is_some_and(|update| update.request.is_some()) {
        return Err("a joint drive update is still computing".into());
    }
    let prepared = app.control_joint_drive.movement.take().and_then(|movement| movement.prepared);
    app.preview.clear();
    let Some(prepared) = prepared.filter(ComputedCommands::changes_document) else {
        return Ok(json!({"changed": false}));
    };
    let revision = app.session.revision;
    let selected = app.session.selection.clone();
    let replies = app.session.accept_commands(prepared).map_err(|error| error.to_string())?;
    app.big_operation = true;
    if let Some((id, reply)) = replies.into_iter().next() {
        app.after_command(&id, &Value::Null, revision, selected, Ok(reply))?;
    }
    Ok(json!({"changed": true}))
}

fn cancel(app: &mut SolveApp, req: &ControlRequest, id: u64) -> Result<Value, String> {
    matching(app, req, id)?;
    discard(app);
    Ok(json!({"cancelled": true}))
}

fn reply(req: &ControlRequest, result: Result<Value, String>) {
    let _ = req.reply.send(match result {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => json!({"ok": false, "error": error}),
    });
}

pub(crate) fn run(app: &mut SolveApp, ctx: &egui::Context, req: ControlRequest) {
    expire(app);
    ctx.request_repaint();
    let scope = match crate::control::request_scope(app, &req.params) {
        Ok(scope) => scope,
        Err(error) => {
            reply(&req, Err(error));
            return;
        }
    };
    let mut params = req.params.clone();
    if let Some(params) = params.as_object_mut() {
        for key in ["allow_paths", "design_folders", "param_check"] {
            params.remove(key);
        }
    }
    let result = match crate::control_params::parse(&params) {
        Ok(Action::Begin { document_id, joint, index }) => begin(app, &req, &document_id, joint, index),
        Ok(Action::Update { movement, value }) => return update(app, ctx, req, scope, movement, value),
        Ok(Action::Finish { movement }) => vfs::scoped(scope, || finish(app, &req, movement)),
        Ok(Action::Cancel { movement }) => cancel(app, &req, movement),
        Err(error) => Err(error),
    };
    reply(&req, result);
}

/// Expire the preview even if its client has gone and the control queue is empty.
pub(crate) fn expire(app: &mut SolveApp) {
    if app.control_joint_drive.movement.as_ref().is_some_and(|movement| {
        movement.owner.strong_count() == 0
            || movement.session != app.session.uid
            || movement.revision != app.session.revision
            || !Arc::ptr_eq(&movement.design, &app.session.doc)
            || local_preview(app)
    }) {
        discard(app);
    }
}

/// A movement cannot leave and return with its document tab.
pub(crate) fn discard(app: &mut SolveApp) {
    if app.control_joint_drive.movement.take().is_some() {
        if let Some(request) = app.control_joint_drive.update.as_mut().and_then(|update| update.request.take()) {
            request.ticket.stop();
            reply(&request, Err("the joint drive movement ended before the update finished".into()));
        }
        app.preview.clear();
    }
}

pub(crate) fn poll(app: &mut SolveApp, ctx: &egui::Context) {
    expire(app);
    let Some(pending) = app.control_joint_drive.update.as_mut() else { return };
    if pending.request.as_ref().is_some_and(|request| request.ticket.is_cancelled())
        && let Some(request) = pending.request.take()
    {
        reply(&request, Err("joint drive update was cancelled".into()));
        app.preview.busy = false;
    }
    let result = match pending.result.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Disconnected) => Err("joint drive update ended without a result".into()),
        Err(TryRecvError::Empty) => {
            ctx.request_repaint_after(Duration::from_millis(16));
            return;
        }
    };
    if let Some(Update { request: Some(request), start, .. }) = app.control_joint_drive.update.take() {
        app.preview.busy = false;
        let result = if request.ticket.back() {
            result.and_then(|prepared| show(app, &request, start, prepared))
        } else {
            Err("joint drive update was cancelled".into())
        };
        reply(&request, result);
        ctx.request_repaint();
    }
}
