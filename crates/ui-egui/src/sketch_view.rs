//! Runtime projection of the design's sketch editor onto its owning view.
use crate::SolveApp;
use solidcraft_engine::{ViewContext, ViewDescriptor};
use solidcraft_views::{SketchSession, ViewKind};

pub(crate) fn active_sketch(app: &SolveApp) -> Option<u64> {
    let painted = app.viewport.compass_view.unwrap_or_else(|| app.docs.views.active().id);
    (app.session.sketching_view == Some(painted)).then_some(app.session.active_sketch).flatten()
}

pub(crate) fn active_sheet_sketch(app: &SolveApp) -> Option<u64> {
    let painted = app.viewport.compass_view.unwrap_or_else(|| app.docs.views.active().id);
    app.session.active_sheet_sketch.as_ref().filter(|entry| entry.view == Some(painted)).map(|entry| entry.sketch)
}

pub(crate) fn name(app: &SolveApp, view: u64) -> Option<String> {
    if let Some(entry) = app.session.active_sheet_sketch.as_ref().filter(|entry| entry.view == Some(view)) {
        return app.session.doc.drawing(entry.drawing)?.sheet_sketch(entry.sketch).map(|sketch| sketch.name.clone());
    }
    (app.session.sketching_view == Some(view))
        .then_some(app.session.active_sketch)
        .flatten()
        .and_then(|sketch| app.session.doc.feature(sketch).map(|feature| feature.name.clone()))
}

pub(crate) fn owner_label(app: &SolveApp, view: u64) -> Option<String> {
    use solidcraft_engine::doc::drawing::Owner;
    let entry = app.session.active_sheet_sketch.as_ref().filter(|entry| entry.view == Some(view))?;
    match entry.owner {
        Owner::Sheet(_) => Some(app.tr("sketch_sheet.on_sheet").to_string()),
        Owner::View(id) => {
            let drawing = app.session.doc.drawing(entry.drawing)?;
            let view = drawing.view(id)?;
            let number =
                drawing.views.iter().filter(|candidate| candidate.sheet == view.sheet).position(|candidate| candidate.id == id)?.saturating_add(1);
            let name = crate::drawing_workspace::browser::view_name(app, drawing, view, number);
            Some(solidcraft_ui_text::format("sketch_sheet.on_view", &[("view", &name)]))
        }
    }
}

pub(crate) fn sync(app: &mut SolveApp) -> Result<(), String> {
    let open: Vec<_> = crate::view_windows::layout_views(app)
        .into_iter()
        .map(|(window, view)| ViewDescriptor { id: view.id, kind: view.kind, window: u64::from(window), subject: view.subject.clone() })
        .collect();
    let focused = crate::view_windows::focused_view(app);
    let mut recent = vec![focused];
    recent.extend(app.session.view_context.recent().iter().copied().filter(|id| *id != focused && open.iter().any(|view| view.id == *id)));
    app.session.set_view_context(ViewContext::new(open, focused, recent)?)?;
    project(app);
    Ok(())
}

pub(crate) fn prepare_start(app: &mut SolveApp, explicit: Option<u64>) -> Result<u64, String> {
    sync(app)?;
    if explicit.is_none() && !app.session.view_context.descriptors().iter().any(|view| view.kind == ViewKind::Model) {
        if !app.docs.views.active().kind.can_tile() {
            return Err(app.tr("sketch_mode.model_card_unavailable").into());
        }
        let mut next = app.docs.views.clone();
        let previous = next.active().id;
        let id = next.open(ViewKind::Model, None, false);
        next.activate(previous);
        if !next.get(id).is_some_and(|view| view.kind == ViewKind::Model) {
            return Err(app.tr("sketch_mode.model_card_unavailable").into());
        }
        let placed = next.fill_empty(0, id) || next.show_beside(id, solidcraft_views::Edge::Right);
        let placed = placed || {
            let park = next
                .tiles()
                .filter(|tiles| tiles.len() >= solidcraft_views::MAX_TILES)
                .and_then(|tiles| tiles.views().into_iter().rev().find(|other| *other != previous && *other != solidcraft_views::EMPTY));
            park.is_some_and(|park| next.unshow(park) && next.show_beside(id, solidcraft_views::Edge::Right))
        };
        if !placed {
            return Err(app.tr("sketch_mode.model_card_unavailable").into());
        }
        crate::documents::change_views(app, |views| *views = next);
        sync(app)?;
    }
    app.session.view_context.resolve_sketch_target(explicit)
}

/// Lifecycle transitions do not participate in linked navigation. Once the user moves the
/// owner camera away from this target, normal camera linking resumes.
pub(crate) fn camera_transition(app: &mut SolveApp, source: u64, camera: solidcraft_viewport::Camera) -> bool {
    let painted = app.viewport.compass_view.unwrap_or_else(|| app.docs.views.active().id);
    if painted != source {
        return false;
    }
    let transition = app.sketch_camera_target.is_some_and(|target| target == camera || app.cam_anim.is_some_and(|animation| animation.to == target));
    if !transition {
        app.sketch_camera_target = None;
    }
    transition
}

pub(crate) fn before_close(app: &mut SolveApp, closing: &[u64]) -> bool {
    if !app.session.editing_view().is_some_and(|view| closing.contains(&view)) {
        return true;
    }
    let Some(view) = app.session.editing_view() else { return true };
    app.sketch_notices.dismiss(app.session.uid);
    app.stopped_sketch_refreshes.remove(&app.session.uid);
    let result = app.session.prepare_view_close(view).map_err(|error| error.to_string());
    consume_finishes(app);
    if let Err(error) = result {
        app.set_status(error, true);
        return false;
    }
    true
}

/// Passive painting updates presentation only; it never records focus or changes the MRU.
pub(crate) fn project(app: &mut SolveApp) {
    let sketch = app.session.active_sketch.or_else(|| app.session.active_sheet_sketch.as_ref().map(|entry| entry.sketch));
    let editing = app.session.editing_view().zip(sketch).map(|(view, sketch)| (view, SketchSession { sketch }));
    let local = editing.filter(|(id, _)| app.docs.views.window_of(*id).is_some());
    app.docs.views.set_editing(local);
    crate::view_windows::sync_sketch_editing(app, editing);
}

/// Prepare an independently authoring command before either JSON validation or a UI form.
/// The engine owns the transition; the UI consumes its event even when the new command fails.
pub(crate) fn prepare_edit(
    app: &mut SolveApp,
    command: &str,
    params: &serde_json::Value,
) -> Result<Option<solidcraft_engine::SketchFinishEvent>, String> {
    let starts = solidcraft_engine::find_command(command).is_some_and(|spec| app.session.command_starts_edit(spec, params));
    let applying_same_dialog = app.dialog.as_ref().is_some_and(|dialog| dialog.command() == Some(command));
    if !starts {
        return Ok(None);
    }
    app.stopped_sketch_refreshes.remove(&app.session.uid);
    // A Cancel Sketch still computing stays a cancel when another edit starts: the new edit must
    // not commit the sketch the person walked away from.
    if app.session.active_sketch.is_some() && app.deferred.as_ref().is_some_and(|deferred| deferred.command.0 == "sketch.cancel") {
        app.deferred = None;
        let _ = app.run("sketch.cancel", serde_json::json!({}));
    }
    let already_prepared = app.prepared_edit_starts.get(&app.session.uid).is_some_and(|id| id == command);
    let result = app.session.prepare_edit_start(command, params).map_err(|error| error.to_string());
    if result.is_ok() {
        if !already_prepared && !applying_same_dialog {
            app.sketch_notices.dismiss(app.session.uid);
        }
        app.prepared_edit_starts.remove(&app.session.uid);
        if applying_same_dialog {
            app.sketch_edit_sequences.insert(app.session.uid, app.session.edit_start_sequence());
        }
    }
    consume_finishes(app);
    result
}

/// Events from accepted workers and live commands share one FIFO. A refused scratch transaction
/// never reaches this session's queue, and draining twice can never restore a camera twice.
pub(crate) fn consume_finishes(app: &mut SolveApp) {
    if app.sketch_finish_processing {
        return;
    }
    app.sketch_finish_processing = true;
    let sequence = app.session.edit_start_sequence();
    if app.sketch_edit_sequences.insert(app.session.uid, sequence).is_some_and(|before| before != sequence) {
        app.sketch_notices.dismiss(app.session.uid);
        app.prepared_edit_starts.remove(&app.session.uid);
        app.start_after_roll = None;
        app.stopped_sketch_refreshes.remove(&app.session.uid);
    }
    while let Some(event) = app.session.take_sketch_finish_event() {
        if matches!(event.domain, solidcraft_engine::SketchDomain::Model) {
            app.sketch_closed_in(Some(event.sketch), event.view);
        } else {
            app.tool = None;
            crate::dim_view::cancel();
        }
        let window = event
            .view
            .and_then(|owner| crate::view_windows::layout_views(app).into_iter().find(|(_, view)| view.id == owner).map(|(window, _)| window))
            .unwrap_or_else(|| app.view_windows.current_window());
        let exists = match event.domain {
            solidcraft_engine::SketchDomain::Model => app.session.doc.feature(event.sketch).is_some(),
            solidcraft_engine::SketchDomain::Sheet { drawing, .. } => {
                app.session.doc.drawing(drawing).is_some_and(|drawing| drawing.sheet_sketch(event.sketch).is_some())
            }
        };
        if exists {
            app.sketch_notices.enqueue(app.session.uid, window, event);
        }
    }
    app.sketch_finish_processing = false;
    project(app);
    refresh_model(app);
}

pub(crate) fn show_notice(app: &mut SolveApp, ui: &egui::Ui) {
    let window = app.view_windows.current_window();
    if !app.sketch_notices.has_for(app.session.uid, window) {
        return;
    }
    // This overlay belongs to the window host, not whichever tile painted last.
    let t = solidcraft_theme::Tokens::get();
    let mut rect = crate::shell::bottom_readout_rect(app, ui.max_rect());
    // Reserve the shared bottom navigation row even when the last painted tile hides it.
    if app.preferences.navigation_bar.edge == crate::ui_kit::NavigationEdge::Bottom {
        rect.max.y -=
            crate::ui_kit::NavigationMetrics::for_size(app.preferences.navigation_bar.size).height() + crate::ui_kit::NAV_BOTTOM + t.space_8;
    }
    let free = rect.shrink(t.space_8);
    let obstacles = app.viewport.host_windows.as_deref().unwrap_or_default();
    let Some(event) = app.sketch_notices.show(ui, free, obstacles, app.session.uid, window) else { return };
    reopen_finished(app, event);
}

pub(crate) fn reopen_finished(app: &mut SolveApp, event: solidcraft_engine::SketchFinishEvent) {
    match event.domain {
        solidcraft_engine::SketchDomain::Model => {
            if !app
                .session
                .doc
                .feature(event.sketch)
                .is_some_and(|feature| matches!(feature.kind, solidcraft_engine::doc::FeatureKind::Sketch { .. }))
            {
                return;
            }
            let owner = event.view.filter(|id| crate::view_windows::layout_views(app).into_iter().any(|(_, view)| view.id == *id));
            if let Some(view) = owner
                && let Err(error) = crate::view_windows::focus_view(app, view, None)
            {
                app.set_status(error, true);
                return;
            }
            app.edit_sketch_at_view(event.sketch, None, owner);
        }
        solidcraft_engine::SketchDomain::Sheet { drawing, sheet } => {
            if !app.session.doc.drawing(drawing).is_some_and(|drawing| drawing.sheet_sketch(event.sketch).is_some()) {
                return;
            }
            let owner = event.view.filter(|id| {
                crate::view_windows::layout_views(app)
                    .into_iter()
                    .any(|(_, view)| view.id == *id && crate::sheet_views::resolve(app, view.subject.as_deref()) == Ok((drawing, sheet)))
            });
            if let Some(view) = owner
                && let Err(error) = crate::view_windows::focus_view(app, view, None)
            {
                app.set_status(error, true);
                return;
            }
            let result = if let Some(view) = owner {
                let descriptor = crate::view_windows::layout_views(app)
                    .into_iter()
                    .find(|(_, descriptor)| descriptor.id == view)
                    .map(|(_, descriptor)| descriptor.clone());
                descriptor.ok_or_else(|| app.tr("sketch_sheet.invalid_view").to_string()).and_then(|view| crate::sheet_views::bind(app, &view))
            } else {
                // The notice is already an exact drawing/sheet receiver. A closed host is
                // resolved by prepare_sheet_start; public drawing.open would create an unrelated view.
                app.session.open_drawing = Some(drawing);
                crate::drawing_sheet_page::select_sheet(app, sheet);
                Ok(())
            }
            .and_then(|()| {
                app.run("drawing.sketch.edit", serde_json::json!({"sketch":solidcraft_engine::doc::ids::encode(event.sketch),"view":owner}))
            });

            if let Err(error) = result {
                app.set_status(error, true);
            }
        }
    }
}

/// Cache-only work has no command authorization or editor/history side effects.
pub(crate) struct ModelRefresh {
    pub(crate) started: f64,
    pub(crate) progress: std::sync::Arc<solidcraft_engine::doc::progress::Progress>,
    done: std::sync::mpsc::Receiver<Result<solidcraft_engine::RefreshedSketchModel, String>>,
}

fn begin_refresh(
    session: &solidcraft_engine::Session,
    quality: solidcraft_engine::doc::display_quality::DisplayQuality,
) -> Result<ModelRefresh, String> {
    let ahead = session.ahead();
    let (tx, done) = std::sync::mpsc::channel();
    let progress = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
    let watching = progress.clone();
    let compute = move || {
        let result = solidcraft_engine::doc::progress::watch(watching, || ahead.refresh_sketch_model(quality));
        let _ = tx.send(result.map_err(|error| error.to_string()));
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::Builder::new().name("sketch model".into()).spawn(compute).map_err(|error| error.to_string())?;
    #[cfg(target_arch = "wasm32")]
    compute();
    Ok(ModelRefresh { done, started: crate::now_ms(), progress })
}

/// A small global cap bounds native rebuild memory even with many open designs.
const MAX_REFRESHES: usize = 2;

pub(crate) fn refresh_model(app: &mut SolveApp) {
    // An opening feature editor evaluates the finished sketch itself; if it is cancelled or
    // refused, the design still needs the refresh and the next poll starts it.
    if !app.session.needs_sketch_finish_evaluation()
        || app.pending_feature_edit.is_some()
        || app.sketch_refreshes.contains_key(&app.session.uid)
        || app.stopped_sketch_refreshes.contains(&app.session.uid)
        || app.sketch_refreshes.len() >= MAX_REFRESHES
    {
        return;
    }
    let quality = solidcraft_engine::doc::display_quality::DisplayQuality::parse(&app.preferences.display_quality).unwrap_or_default();
    match begin_refresh(&app.session, quality) {
        Ok(job) => {
            let wait = app.apply_off_thread.unwrap_or(crate::dialogs::APPLY_AT_ONCE);
            if !wait.is_zero() {
                match job.done.recv_timeout(wait) {
                    Ok(Ok(prepared)) => {
                        let _ = app.session.accept_refreshed_sketch_model(prepared);
                        return;
                    }
                    Ok(Err(error)) => {
                        app.set_status(error, true);
                        return;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            app.sketch_refreshes.insert(app.session.uid, job);
        }
        Err(error) => app.set_status(error, true),
    }
}

pub(crate) fn poll_refreshes(app: &mut SolveApp, ctx: &egui::Context) {
    let mut completed = Vec::new();
    for (uid, job) in &app.sketch_refreshes {
        match job.done.try_recv() {
            Ok(result) => completed.push((*uid, Some(result))),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => completed.push((*uid, None)),
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }
    for (uid, result) in completed {
        let cancelled = app.sketch_refreshes.remove(&uid).is_some_and(|job| job.progress.is_cancelled());
        if cancelled {
            continue;
        }
        match result {
            // A feature editor opening meanwhile evaluates the finished sketch itself, against the
            // design as it was when it started: adopting the refresh first would change that design
            // under it, and the editor would refuse to open.
            Some(Ok(_)) if uid == app.session.uid && app.pending_feature_edit.is_some() => {}
            Some(Ok(prepared)) if uid == app.session.uid => {
                let _ = app.session.accept_refreshed_sketch_model(prepared);
            }
            Some(Ok(prepared)) => {
                if let Some(slot) = app.docs.background_slot_mut(uid) {
                    let _ = slot.session.accept_refreshed_sketch_model(prepared);
                }
            }
            Some(Err(error)) if uid == app.session.uid => app.set_status(error, true),
            _ => {}
        }
    }
    refresh_model(app);
    let quality = solidcraft_engine::doc::display_quality::DisplayQuality::parse(&app.preferences.display_quality).unwrap_or_default();
    for session in app.docs.background() {
        if app.sketch_refreshes.len() >= MAX_REFRESHES {
            break;
        }
        if session.needs_sketch_finish_evaluation()
            && !app.sketch_refreshes.contains_key(&session.uid)
            && !app.stopped_sketch_refreshes.contains(&session.uid)
            && let Ok(job) = begin_refresh(session, quality)
        {
            app.sketch_refreshes.insert(session.uid, job);
        }
    }
    if !app.sketch_refreshes.is_empty()
        || (app.session.needs_sketch_finish_evaluation() && !app.stopped_sketch_refreshes.contains(&app.session.uid))
        || app.docs.background().any(|session| session.needs_sketch_finish_evaluation() && !app.stopped_sketch_refreshes.contains(&session.uid))
    {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

#[cfg(test)]
impl ModelRefresh {
    pub(crate) fn held() -> (Self, std::sync::mpsc::Sender<Result<solidcraft_engine::RefreshedSketchModel, String>>) {
        let (tx, done) = std::sync::mpsc::channel();
        (Self { done, started: crate::now_ms(), progress: Default::default() }, tx)
    }

    /// Wait for real worker completion, then leave its result for the normal polling path.
    pub(crate) fn completed(mut self) -> Self {
        let result = self.done.recv().unwrap();
        let (tx, done) = std::sync::mpsc::channel();
        tx.send(result).unwrap();
        self.done = done;
        self
    }
}

impl Drop for ModelRefresh {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

pub(crate) fn cancel_refresh(app: &mut SolveApp) {
    if let Some(job) = app.sketch_refreshes.get(&app.session.uid) {
        job.progress.cancel();
    }
    app.stopped_sketch_refreshes.insert(app.session.uid);
    app.start_after_roll = None;
    app.prepared_edit_starts.remove(&app.session.uid);
    app.session.cancel_prepared_edit_start();
}

pub(crate) fn prepare_sheet_start(app: &mut SolveApp, command: &str, params: &mut serde_json::Value) -> Result<u64, String> {
    let explicit = match params.get("view") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(value.as_u64().ok_or_else(|| app.tr("sketch_sheet.invalid_view").to_string())?),
    };
    if let Some(view) = explicit {
        let descriptor = crate::view_windows::layout_views(app)
            .into_iter()
            .find(|(_, descriptor)| descriptor.id == view && descriptor.kind == ViewKind::Sheet)
            .map(|(_, descriptor)| descriptor.clone())
            .ok_or_else(|| app.tr("sketch_sheet.invalid_view").to_string())?;
        crate::sheet_views::bind(app, &descriptor)?;
    }
    let drawing = app.session.open_drawing.ok_or_else(|| app.tr("sketch_sheet.no_sheet").to_string())?;
    let record = app.session.doc.drawing(drawing).ok_or_else(|| app.tr("sketch_sheet.no_sheet").to_string())?;
    let decode = |key| params.get(key).and_then(serde_json::Value::as_str).and_then(solidcraft_engine::doc::ids::decode);
    let sheet = if command == "drawing.sketch.edit" {
        decode("sketch").and_then(|id| record.sheet_sketch(id)).and_then(|sketch| record.sketch_sheet(sketch))
    } else {
        decode("owner").and_then(|owner| {
            record.sheets.iter().any(|sheet| sheet.id == owner).then_some(owner).or_else(|| record.view(owner).map(|view| view.sheet))
        })
    }
    .ok_or_else(|| app.tr("sketch_sheet.no_sheet").to_string())?;
    let matches = |id| {
        app.session
            .view_context
            .descriptors()
            .iter()
            .find(|view| view.id == id && view.kind == ViewKind::Sheet)
            .is_some_and(|view| crate::sheet_views::resolve(app, view.subject.as_deref()) == Ok((drawing, sheet)))
    };
    let target = if let Some(view) = explicit {
        if !matches(view) {
            return Err(app.tr("sketch_sheet.invalid_view").into());
        }
        Some(view)
    } else {
        std::iter::once(app.session.view_context.focused())
            .chain(app.session.view_context.recent().iter().copied())
            .chain(app.session.view_context.descriptors().iter().map(|view| view.id))
            .find(|view| matches(*view))
    };
    let target = match target {
        Some(view) => view,
        None => crate::sheet_views::open_in(app, drawing, sheet, false)?,
    };
    let descriptor = crate::view_windows::layout_views(app)
        .into_iter()
        .find(|(_, descriptor)| descriptor.id == target && descriptor.kind == ViewKind::Sheet)
        .map(|(_, descriptor)| descriptor.clone())
        .ok_or_else(|| app.tr("sketch_sheet.invalid_view").to_string())?;
    crate::sheet_views::bind(app, &descriptor)?;
    if let Some(params) = params.as_object_mut() {
        params.insert("view".into(), serde_json::json!(target));
    }
    Ok(target)
}

/// Generic entry for the existing Sheet Sketch ribbon slot; pointer sketch authoring stays in its frontend.
pub(crate) fn start_sheet(app: &mut SolveApp) -> Result<(), String> {
    sync(app)?;
    let focused = crate::view_windows::focused_view(app);
    let view = crate::view_windows::layout_views(app)
        .into_iter()
        .find(|(_, view)| view.id == focused)
        .map(|(_, view)| view.clone())
        .filter(|view| view.kind == ViewKind::Sheet)
        .ok_or_else(|| app.tr("sketch_sheet.no_sheet").to_string())?;
    let (drawing, sheet) = crate::sheet_views::resolve(app, view.subject.as_deref())?;
    crate::sheet_views::bind(app, &view)?;
    let record = app.session.doc.drawing(drawing).ok_or_else(|| app.tr("sketch_sheet.no_sheet").to_string())?;
    let selected = crate::drawing_sheet_page::selected_drawing_view(app).filter(|view| record.view(*view).is_some_and(|view| view.sheet == sheet));
    let owner = selected
        .or_else(|| {
            app.session.selection.iter().find_map(|item| match item {
                solidcraft_engine::Sel::Drawing { drawing: selected, object } if *selected == drawing => {
                    record.view(*object).filter(|view| view.sheet == sheet).map(|view| view.id)
                }
                _ => None,
            })
        })
        .unwrap_or(sheet);
    app.run("drawing.sketch.create", serde_json::json!({"owner":solidcraft_engine::doc::ids::encode(owner),"view":focused}))?;
    Ok(())
}
