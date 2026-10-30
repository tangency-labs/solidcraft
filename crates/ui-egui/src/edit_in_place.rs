//! Edit in place: double-clicking a linked component opens the design it comes from, with the
//! rest of the assembly drawn faded around it where the component sits, so its own timeline,
//! browser and commands are at hand. Finish Edit saves the part and goes back to the assembly,
//! which reads the link again.
//!
//! The part is an ordinary tab: saving, autosave, recovery, closing and quitting treat it as any
//! other design. The assembly around it is a picture of the assembly's tab, taken when the edit
//! starts and again after Update Links.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::{apply_point, apply_vector, mat_inverse, mat_mul};
use solidcraft_engine::geom::Aabb3;

use crate::SolveApp;
use crate::gpu::GpuScene;
use crate::theme::Tokens;

/// The assembly around a part being edited in place.
#[derive(Clone)]
pub struct PartContext {
    /// The assembly's file and name.
    pub assembly: String,
    pub assembly_name: String,
    /// The part's file.
    pub part: String,
    /// The linked component edited, and the placement the assembly is drawn around.
    pub component: u64,
    pub placement: Vec<u64>,
    /// How many of the assembly's bodies are drawn around the part.
    pub bodies: usize,
    /// Their triangles in the part's frame, three points and normals each, worked out once.
    pub triangles: Arc<Vec<([f32; 3], [f32; 3])>>,
    /// Their extent in the part's frame, so fitting the view frames the assembly too.
    pub bounds: Aabb3,
    /// The part's tab was opened for this edit, so Finish Edit closes it again.
    pub opened: bool,
}

/// Update Links run on the assembly while its part is edited in place.
struct Refresh {
    /// The assembly's file, and its document when the run started: a result for an assembly
    /// changed since is dropped (showing it changes nothing).
    assembly: String,
    base: Arc<solidcraft_engine::doc::Document>,
    /// The linked files as they were when the run started reading them.
    stamps: Vec<(u64, crate::linked_source::Stamp)>,
    done: Receiver<Result<Session, String>>,
}

/// The edit in place of the tab shown, and one waiting for its part to open.
#[derive(Default)]
pub struct EditInPlace {
    pub active: Option<PartContext>,
    pending: Option<PartContext>,
    /// Finish Edit is waiting for the save of the part at this file.
    finish_after_save: Option<String>,
    /// The part's own agent asked to finish the edit of the part at this file during its turn.
    finish_after_turn: Option<String>,
    /// The assembly's own agent asked to edit this component of the assembly at this file, at
    /// this placement, during its turn.
    enter_after_turn: Option<(String, u64, Option<Vec<u64>>)>,
    refresh: Option<Refresh>,
    /// `component.edit_in_place` is run by the window ([`open`]), not as a command: its open may ask.
    ask: bool,
}

impl EditInPlace {
    /// Update Links is running on an assembly whose part is edited in place.
    pub fn refreshing(&self) -> bool {
        self.refresh.is_some()
    }
}

/// A linked component the window edits in place when it is double-clicked: in a saved design
/// (its links are found from its file) with `double_click_components` on.
fn edits_on_double_click(app: &SolveApp, component: u64) -> bool {
    app.preferences.double_click_components && app.session.path.is_some() && app.session.doc.links.contains_key(&component)
}

/// A component double-clicked in the browser: a linked one is edited in place (Preferences:
/// `double_click_components`), any other, or any in a design not saved yet, is made the active
/// component (`component.activate` with `params`). Activate itself always does the latter.
pub(crate) fn double_click_component(app: &mut SolveApp, params: Value) {
    let component = params["component"].as_u64().unwrap_or(0);
    if edits_on_double_click(app, component) {
        let path = params["path"].as_array().map(|p| p.iter().filter_map(Value::as_u64).collect::<Vec<_>>());
        open(app, component, path);
    } else {
        let _ = app.run("component.activate", params);
    }
}

/// A face of the body `body` double-clicked in the view: the component the body belongs to, at
/// that body's placement, is edited in place when it is linked and activated otherwise. A body of
/// the design itself or of the active component is left alone, as are features. Whether it acted.
pub(crate) fn double_click_face(app: &mut SolveApp, body: &str) -> bool {
    if !app.preferences.double_click_components {
        return false;
    }
    let Some(component) = app.session.world_state().body(body).map(|b| b.component).filter(|c| *c != 0) else { return false };
    let placement = app
        .session
        .doc
        .placements()
        .into_iter()
        .filter(|(c, _, _)| *c == component)
        .map(|(_, path, _)| path)
        .find(|path| app.session.occurrence_bodies(path, true).is_ok_and(|bodies| bodies.iter().any(|b| b == body)));
    if edits_on_double_click(app, component) {
        open(app, component, placement);
        return true;
    }
    let active = app.session.active_component == component
        && (placement.is_none() || app.session.active_occurrence_path.is_none() || app.session.active_occurrence_path == placement);
    if active {
        return false;
    }
    let occurrence = placement.as_ref().and_then(|p| p.last().copied());
    app.run("component.activate", json!({"component": component, "occurrence": occurrence, "path": placement})).is_ok()
}

/// Something runs that a change of the design shown would spoil: a file being read, written or
/// picked, the assembly's refresh, or an undo, a queued command or a delete check, which would
/// land in the other design.
fn busy(app: &SolveApp) -> bool {
    app.file_picker_busy()
        || app.file_load.busy()
        || app.saving.is_some()
        || app.edit_in_place.refreshing()
        || app.deferred.is_some()
        || !app.deferred_queue.is_empty()
        || app.delete_check.is_some()
}

/// Why [`busy`]: the assembly's refresh, or something else that has to finish first.
fn busy_reason(app: &SolveApp) -> String {
    match &app.edit_in_place.refresh {
        Some(r) => crate::i18n::format("ui.edit_in_place_refreshing", &[("name", &file_name(&r.assembly))]),
        None => crate::i18n::tr("ui.edit_in_place_wait").to_string(),
    }
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_stem().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// A dialog, tool, sketch or feature edit, or the parameters or design properties panel (with
/// text typed and not applied yet) is open in the design shown: leaving its tab would drop it.
fn working(app: &SolveApp) -> bool {
    app.dialog.is_some()
        || app.applying.is_some()
        || app.editing_state.is_some()
        || app.pending_feature_edit.is_some()
        || app.tool.is_some()
        || app.session.active_sketch.is_some()
        || app.params.is_some()
        || app.design_properties.is_some()
}

/// The bodies of `session` around its linked component `component`, in that component's frame
/// at `placement`: how many, their triangles and their extent. `None` when the placement is gone.
fn snapshot(session: &Session, component: u64, placement: &[u64]) -> Option<(usize, Vec<([f32; 3], [f32; 3])>, Aabb3)> {
    let frame = session.occurrence_placement(placement).ok().and_then(|p| mat_inverse(&p.world))?;
    let world = session.world_state();
    let (mut bodies, mut triangles, mut bounds) = (0, Vec::new(), Aabb3::default());
    for b in world.bodies.iter().filter(|b| b.component != component && session.body_visible(&b.name)) {
        let (mesh, placed) = b.mesh_in_place();
        let m = mat_mul(&frame, &placed);
        bodies += 1;
        for t in &mesh.triangles {
            // A triangle with an index out of range is left out whole, so the rest stay in step.
            let corners: Option<Vec<_>> = t.iter().map(|k| Some((*mesh.positions.get(*k as usize)?, *mesh.normals.get(*k as usize)?))).collect();
            for (p, n) in corners.into_iter().flatten() {
                let p = apply_point(&m, p);
                bounds.add(p);
                triangles.push((p.to_f32(), apply_vector(&m, n).to_f32()));
            }
        }
    }
    Some((bodies, triangles, bounds))
}

/// Edit the linked component `component` in place at its placement `path` (default: its first),
/// as the window does it (a double-click, Edit in Place in its menu): `component.edit_in_place`,
/// with the questions an open can ask (a network location, a design open elsewhere) asked.
pub(crate) fn open(app: &mut SolveApp, component: u64, path: Option<Vec<u64>>) {
    let mut params = json!({ "component": component });
    if let Some(path) = path {
        params["path"] = json!(path);
    }
    app.edit_in_place.ask = true;
    let _ = app.run("component.edit_in_place", params);
    app.edit_in_place.ask = false;
}

/// `component.edit_in_place` ran: `result` names the component and its placement, and the window
/// opens the edit. Run as a command, it asks nothing: what would need a question fails instead.
pub(crate) fn command_ran(app: &mut SolveApp, result: &Value) -> Result<bool, String> {
    let component = result["component"].as_u64().unwrap_or(0);
    let path = result["path"].as_array().map(|p| p.iter().filter_map(Value::as_u64).collect::<Vec<_>>());
    let ask = app.edit_in_place.ask;
    // Leaving the tab would stop the design's agent in the middle of its turn. Asked by that agent
    // (a command, which it sends during its turn), the part opens once the turn ends.
    if app.agent_chat.state() == crate::agent_chat::State::Busy {
        if ask {
            return Err(crate::i18n::tr("ui.edit_in_place_agent_busy").to_string());
        }
        if app.edit_in_place.enter_after_turn.is_some() {
            return Err(crate::i18n::tr("ui.edit_in_place_already_queued").to_string());
        }
        // Everything that would make it fail at the turn's end fails it now.
        let (context, _) = prepare(app, component, path.clone(), false)?;
        app.edit_in_place.enter_after_turn = Some((context.assembly, component, path));
        app.set_status(crate::i18n::tr("ui.edit_in_place_opens_after_turn").to_string(), false);
        return Ok(true);
    }
    enter(app, component, path, ask).map(|()| false)
}

/// What holds up entering an edit from the design shown.
fn check_enter(app: &SolveApp) -> Result<(), String> {
    if busy(app) {
        return Err(busy_reason(app));
    }
    if working(app) {
        return Err(crate::i18n::tr("ui.edit_in_place_busy").to_string());
    }
    if app.session.path.is_none() {
        return Err(crate::i18n::tr("ui.edit_in_place_save_first").to_string());
    }
    Ok(())
}

/// Open the design the linked component `component` comes from, with the assembly around it as
/// its placement `path` (default: its first) puts it (`ask`: with the open's questions).
fn enter(app: &mut SolveApp, component: u64, path: Option<Vec<u64>>, ask: bool) -> Result<(), String> {
    let (context, link) = prepare(app, component, path, ask)?;
    app.edit_in_place.pending = Some(context);
    crate::network_prompt::open_linked(app, &link);
    Ok(())
}

/// The edit [`enter`] would start, and the link it opens, or why it can't: everything is checked
/// and nothing changes.
fn prepare(app: &mut SolveApp, component: u64, path: Option<Vec<u64>>, ask: bool) -> Result<(PartContext, Value), String> {
    check_enter(app)?;
    let Some(assembly) = app.session.path.clone() else {
        return Err(crate::i18n::tr("ui.edit_in_place_save_first").to_string());
    };
    let links = app.session.execute("component.links", &json!({})).map_err(|e| e.to_string())?;
    let Some(link) = links["links"].as_array().into_iter().flatten().find(|l| l["component"].as_u64() == Some(component)).cloned() else {
        return Err(app.tr_label("ui.relink_not_linked").to_string());
    };
    let Some(part) = link["file"].as_str().map(str::to_string) else {
        return Err(app.tr_label("ui.relink_not_linked").to_string());
    };
    if !ask {
        // A network location not allowed yet, a missing file or one open in another SolidCraft
        // would ask or stop the open: as a command, that is its answer.
        if link["found"] != true {
            return Err(format!("{part}: {}", link["reason"].as_str().or(link["host"].as_str()).unwrap_or("not found")));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if app.locks.is_some()
            && crate::documents::session_at(app, &part).is_none()
            && solidcraft_engine::io::design_lock::held_elsewhere(std::path::Path::new(&part), solidcraft_engine::io::design_lock::now_secs())
                .is_some()
        {
            return Err(crate::i18n::format("ui.edit_in_place_open_elsewhere", &[("name", &file_name(&part))]));
        }
    }
    let placement = path.unwrap_or_else(|| app.session.doc.first_path(component));
    let Some((bodies, triangles, bounds)) = snapshot(&app.session, component, &placement) else {
        return Err(app.tr_label("ui.relink_not_linked").to_string());
    };
    let opened = crate::documents::session_at(app, &part).is_none();
    let context = PartContext {
        assembly,
        assembly_name: app.session.doc.name.clone(),
        part,
        component,
        placement,
        bodies,
        triangles: Arc::new(triangles),
        bounds,
        opened,
    };
    Ok((context, link))
}

/// The design shown has become the part an edit waits for: the edit starts there. An edit whose
/// part did not open (the open failed or was cancelled) is dropped by [`tick`].
pub(crate) fn attach(app: &mut SolveApp) {
    let shown = app.session.path.as_deref();
    let Some(mut context) = app.edit_in_place.pending.take_if(|c| shown.is_some_and(|p| crate::documents::same_path(p, &c.part))) else {
        return;
    };
    // Entered again in a tab an earlier edit opened: that tab is still this edit's to close.
    if let Some(old) = app.edit_in_place.active.as_ref().filter(|c| crate::documents::same_path(&c.part, &context.part)) {
        context.opened |= old.opened;
    }
    app.edit_in_place.active = Some(context);
    app.fit_view();
}

/// The name of the design shown, while it reads its links for a part edited in place (it is
/// not saved until the read is done).
pub(crate) fn refreshing_assembly(app: &SolveApp) -> Option<String> {
    let r = app.edit_in_place.refresh.as_ref()?;
    app.session.path.as_deref().filter(|p| crate::documents::same_path(p, &r.assembly))?;
    Some(app.session.doc.name.clone())
}

/// Update Links pressed while a part is edited in place: the assembly's tab reads its links again
/// (off the window thread), and the picture around the part follows.
pub(crate) fn refresh(app: &mut SolveApp) {
    // A refresh already running is the one asked for.
    if app.edit_in_place.refreshing() {
        return;
    }
    if busy(app) {
        app.set_status(busy_reason(app), true);
        return;
    }
    let Some(assembly) = app.edit_in_place.active.as_ref().map(|c| c.assembly.clone()) else { return };
    let Some(session) = crate::documents::session_at(app, &assembly) else {
        let name = app.edit_in_place.active.as_ref().map(|c| c.assembly_name.clone()).unwrap_or_default();
        app.set_status(crate::i18n::format("ui.edit_in_place_assembly_closed", &[("name", &name)]), true);
        return;
    };
    let mut scratch = session.scratch();
    // Link paths are kept relative to the design's own file.
    scratch.path.clone_from(&session.path);
    let base = session.doc.clone();
    let stamps = crate::linked_source::stamps_of(&mut scratch, app.preferences.update_changed_links);
    let (tx, done) = std::sync::mpsc::channel();
    let run = move || {
        let r = scratch.execute("component.update_links", &json!({})).map(|_| scratch).map_err(|e| e.to_string());
        let _ = tx.send(r);
    };
    #[cfg(not(target_arch = "wasm32"))]
    if std::thread::Builder::new().name("edit-in-place-links".into()).spawn(run).is_err() {
        app.set_status(crate::i18n::tr("ui.edit_in_place_refresh_stopped").to_string(), true);
        return;
    }
    #[cfg(target_arch = "wasm32")]
    run();
    app.edit_in_place.refresh = Some(Refresh { assembly, base, stamps, done });
    let name = app.edit_in_place.active.as_ref().map(|c| c.assembly_name.clone()).unwrap_or_default();
    app.set_status(crate::i18n::format("ui.edit_in_place_refreshing", &[("name", &name)]), false);
}

/// Take up a finished refresh: the assembly's tab adopts it when it is still the assembly the
/// run started from, and the picture around the part is taken again.
fn take_refresh(app: &mut SolveApp) {
    let Some(r) = &app.edit_in_place.refresh else { return };
    // An undo, a command or a feature still being computed in the assembly goes first: taken up
    // under it, the refresh would be what it undoes or what it is refused for.
    let shown = app.session.path.as_deref().is_some_and(|p| crate::documents::same_path(p, &r.assembly));
    if shown && (app.deferred.is_some() || !app.deferred_queue.is_empty() || app.applying.is_some()) {
        return;
    }
    let result = match r.done.try_recv() {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(crate::i18n::tr("ui.edit_in_place_refresh_stopped").to_string()),
    };
    let Some(r) = app.edit_in_place.refresh.take() else { return };
    let mut computed = match result {
        Ok(computed) => computed,
        Err(e) => {
            app.set_status(e, true);
            return;
        }
    };
    let Some(session) = crate::documents::session_at_mut(app, &r.assembly) else { return };
    if !Arc::ptr_eq(&session.doc, &r.base) {
        app.set_status(crate::i18n::tr("ui.edit_in_place_refresh_dropped").to_string(), true);
        return;
    }
    // What changed in the assembly meanwhile without changing its document (a pick, an
    // activation, a component dragged but not captured) stays.
    computed.selection.clone_from(&session.selection);
    computed.active_sketch = session.active_sketch;
    computed.active_component = session.active_component;
    computed.active_occurrence_path.clone_from(&session.active_occurrence_path);
    let occurrences = &computed.doc.occurrences;
    computed.pending_moves =
        session.pending_moves.iter().filter(|(id, _)| occurrences.iter().any(|o| o.id == **id)).map(|(id, m)| (*id, *m)).collect();
    session.accept_computed(computed);
    // Every link was read as its file was when the run started: one saved since is read again
    // when the assembly's tab comes back.
    crate::documents::record_stamps_at(app, &r.assembly, &r.stamps);
    // Every edit of a part of this assembly, in the tab shown or another, is drawn around anew.
    let of_assembly = |c: &&mut PartContext| crate::documents::same_path(&c.assembly, &r.assembly);
    let placements: Vec<(u64, Vec<u64>)> =
        crate::documents::edits_mut(app).into_iter().filter(of_assembly).map(|c| (c.component, c.placement.clone())).collect();
    let snaps: Vec<_> = placements
        .iter()
        .map(|(component, placement)| crate::documents::session_at(app, &r.assembly).and_then(|s| snapshot(s, *component, placement)))
        .collect();
    for (context, snap) in crate::documents::edits_mut(app).into_iter().filter(of_assembly).zip(snaps) {
        if let Some((bodies, triangles, bounds)) = snap {
            (context.bodies, context.triangles, context.bounds) = (bodies, Arc::new(triangles), bounds);
        }
    }
    let language = crate::i18n::Locale::for_language(&app.preferences.language);
    app.set_status(language.format("ui.command_done", &[("command", language.label(crate::command_label("component.update_links")))]), false);
}

/// Finish Edit from the window (its button, the back button): a part with changes is saved
/// first, with whatever the save asks, and the edit finishes once the save is done.
pub(crate) fn finish(app: &mut SolveApp) {
    if let Err(why) = finish_with(app, true) {
        app.set_status(why, true);
    }
}

/// `component.finish_edit` run as a command: it opens nothing, so a part with changes is
/// refused (save it first), as is anything else that holds Finish Edit up. The refusal is
/// its error, which the caller reports; `true`: it waits for the end of the agent's turn.
pub(crate) fn finish_command(app: &mut SolveApp) -> Result<bool, String> {
    finish_with(app, false)
}

/// Save the part (`save`: and wait for it), go back to the assembly, close the part's tab when
/// the edit opened it and read the assembly's links again. `true`: it waits for the agent.
fn finish_with(app: &mut SolveApp, save: bool) -> Result<bool, String> {
    let Some(part) = app.edit_in_place.active.as_ref().map(|c| c.part.clone()) else {
        return Err(crate::i18n::tr("ui.edit_in_place_none").to_string());
    };
    if busy(app) {
        return Err(busy_reason(app));
    }
    if !can_finish(app) {
        return Err(crate::i18n::tr("ui.edit_in_place_busy").to_string());
    }
    // Saved to another file (Save As, a copy of a read-only design): the assembly still links this one.
    if !app.session.path.as_deref().is_some_and(|p| crate::documents::same_path(p, &part)) {
        return Err(crate::i18n::format("ui.edit_in_place_saved_elsewhere", &[("name", &file_name(&part))]));
    }
    if app.session.is_dirty() {
        if !save {
            return Err(crate::i18n::tr("ui.edit_in_place_not_saved").to_string());
        }
        app.edit_in_place.finish_after_save = Some(part);
        crate::toolbar::save(app);
        return Ok(false);
    }
    // Leaving the tab would stop the part's agent in the middle of its turn. Asked by that agent
    // (a command, which it can only send during its turn), the edit finishes once the turn ends.
    if app.agent_chat.state() == crate::agent_chat::State::Busy {
        if save {
            return Err(crate::i18n::tr("ui.edit_in_place_agent_busy").to_string());
        }
        app.edit_in_place.finish_after_turn = Some(part);
        app.set_status(crate::i18n::tr("ui.edit_in_place_after_turn").to_string(), false);
        return Ok(true);
    }
    let Some(context) = app.edit_in_place.active.take() else { return Ok(false) };
    let part_tab = app.docs.active;
    if !matches!(crate::documents::reuse_path(app, &context.assembly), Ok(true)) {
        let why = crate::i18n::format("ui.edit_in_place_assembly_closed", &[("name", &context.assembly_name)]);
        app.edit_in_place.active = Some(context);
        return Err(why);
    }
    if context.opened {
        crate::documents::close(app, part_tab, false);
    }
    // Coming back to its tab reads what changed, unless it read nothing (`update_changed_links`
    // off, the web build without file times): then the part is read here.
    if (!app.preferences.update_changed_links || cfg!(target_arch = "wasm32")) && app.file_load.idle() {
        let mut request = crate::file_load::Request::new("", crate::file_load::Kind::UpdateLinks);
        request.options.insert("component".into(), json!(context.component));
        request.design = Some(context.assembly);
        let _ = crate::file_load::begin_request(app, request);
    }
    Ok(false)
}

/// Nothing is under way in the part that leaving its tab would cut short or that waits for an
/// answer: a dialog or tool, a feature being computed or edited, a sketch, a prompt or Home.
fn can_finish(app: &SolveApp) -> bool {
    !working(app)
        && app.docs.closing.is_none()
        && app.docs.quitting.is_none()
        && app.docs.batch.is_none()
        && app.network_prompt.is_none()
        && app.lock_prompts.is_empty()
        && !app.disk_prompt
        && app.tree.capture_prompt.is_none()
        && !app.home.open
}

/// Each frame: a refresh that finished is taken up; an edit waiting for a part that nothing will
/// open any more is dropped; and Finish Edit goes on once the part's save is done, in the
/// part's own tab (a save that failed, was cancelled or went elsewhere leaves the edit as it was).
pub(crate) fn tick(app: &mut SolveApp) {
    take_refresh(app);
    if app.edit_in_place.pending.is_some() && !app.file_load.busy() && app.network_prompt.is_none() && app.lock_prompts.is_empty() {
        app.edit_in_place.pending = None;
    }
    let waiting = app.saving.is_some() || app.file_picker.is_some() || app.disk_prompt || app.tree.capture_prompt.is_some();
    if !waiting && let Some(part) = app.edit_in_place.finish_after_save.take() {
        let shown = app.session.path.as_deref().is_some_and(|p| crate::documents::same_path(p, &part));
        if shown && !app.session.is_dirty() {
            finish(app);
        } else if shown {
            app.set_status(crate::i18n::tr("ui.edit_in_place_not_saved").to_string(), true);
        }
    }
    // The agent's turn ended: the finish it asked for goes on, as the command would have, in the
    // part's own tab (shown elsewhere, or edited differently since, it is dropped). What runs in
    // the design meanwhile (an undo, a file being written) is waited for, not a reason to drop it.
    let after_turn = app.agent_chat.state() != crate::agent_chat::State::Busy && !busy(app);
    if after_turn
        && let Some(part) = app.edit_in_place.finish_after_turn.take()
        && app.edit_in_place.active.as_ref().is_some_and(|c| crate::documents::same_path(&c.part, &part))
        && let Err(why) = finish_with(app, false)
    {
        app.set_status(why, true);
    }
    // Likewise the edit the assembly's agent asked for, from the assembly's tab.
    if after_turn
        && !busy(app)
        && let Some((assembly, component, path)) = app.edit_in_place.enter_after_turn.take()
        && app.session.path.as_deref().is_some_and(|p| crate::documents::same_path(p, &assembly))
        && let Err(why) = enter(app, component, path, false)
    {
        app.set_status(why, true);
    }
}

/// The agent's turn was stopped: what it asked to do once the turn ended is not done.
pub(crate) fn forget_after_turn(app: &mut SolveApp) {
    app.edit_in_place.finish_after_turn = None;
    app.edit_in_place.enter_after_turn = None;
}

/// The mouse's back button (or a keyboard's Back key) pressed while a part is edited in place:
/// Finish Edit, which says why when something open in the part holds it up.
pub(crate) fn back(app: &mut SolveApp) {
    if app.edit_in_place.active.is_some() {
        finish(app);
    }
}

/// What the scene key needs of the edit: the assembly picture shown (0: none).
pub(crate) fn scene_key(app: &SolveApp) -> usize {
    app.edit_in_place.active.as_ref().map_or(0, |c| Arc::as_ptr(&c.triangles) as usize)
}

/// The extent of the assembly around the part, when one is shown.
pub(crate) fn bounds(app: &SolveApp) -> Option<Aabb3> {
    app.edit_in_place.active.as_ref().map(|c| c.bounds).filter(|b| !b.is_empty())
}

/// Draw the assembly around the part, faded and out of reach of picks.
pub(crate) fn draw_context(app: &SolveApp, sc: &mut GpuScene) {
    let Some(context) = &app.edit_in_place.active else { return };
    let col = sc.color(crate::viewport::c4(Tokens::get().body.gamma_multiply(0.35)));
    for (p, n) in context.triangles.iter() {
        sc.ghost_tri(*p, *n, col);
    }
}

/// Above the browser's rows while a part is edited in place: which assembly it is in (Finish
/// Editing is on the MODEL tab).
pub(crate) fn browser_strip(app: &SolveApp, ui: &mut egui::Ui) {
    let Some(context) = &app.edit_in_place.active else { return };
    let t = Tokens::get();
    let refreshing = app.edit_in_place.refresh.as_ref().is_some_and(|r| crate::documents::same_path(&r.assembly, &context.assembly));
    let key = if refreshing { "ui.edit_in_place_refreshing" } else { "ui.edit_in_place_banner" };
    let text = crate::i18n::format(key, &[("name", &context.assembly_name)]);
    ui.horizontal(|ui| {
        ui.add_space(t.space_6);
        crate::ui_kit::text(ui, &text, crate::ui_kit::Text::Hint("edit_in_place"));
    });
    ui.add_space(t.space_3);
}

#[cfg(test)]
#[path = "edit_in_place_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
