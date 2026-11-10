//! Native Check/Start ownership. Preparation and retirement use one bounded
//! worker; the window accepts only the already-proved real-session commit.
use crate::SolveApp;
use egui::Context;
use serde_json::{Value, json};
use solidcraft_engine::render_preparation::Kind;

pub(crate) fn enabled(app: &SolveApp) -> bool {
    !cfg!(target_arch = "wasm32") && app.apply_off_thread.is_some()
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use crate::control::ControlRequest;
    use solidcraft_engine::render::job::CancelToken;
    use solidcraft_engine::render_preparation::{Prepared, finish_retained, prepare};
    use solidcraft_engine::schema::ParamCheck;
    use std::collections::BTreeMap;
    use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError};
    use std::sync::{Arc, Weak};

    #[derive(Default)]
    pub(crate) struct State {
        pending: Option<Pending>,
        batches: batches::State,
        #[cfg(test)]
        proof_ready: Option<SyncSender<bool>>,
        #[cfg(test)]
        retired_probe: Option<SyncSender<()>>,
        #[cfg(test)]
        disconnect_retirement: bool,
        #[cfg(test)]
        disconnect_input: bool,
    }
    impl Drop for State {
        fn drop(&mut self) {
            if let Some(pending) = &mut self.pending {
                // This app-global owner is not replaced on a tab/document reset.
                // Real app teardown may perform ordinary destructors. During
                // interaction, a failed return stays held in this bounded slot.
                if !pending.committed {
                    pending.cancel.cancel();
                }
                if let Some(worker) = &mut pending.worker {
                    let _ = worker.retire();
                }
            }
        }
    }

    struct Guard {
        uid: u64,
        document: Weak<solidcraft_engine::doc::Document>,
        model: Weak<solidcraft_engine::doc::ModelState>,
        revision: u64,
        camera: (solidcraft_viewport::UpAxis, [u64; 8]),
        moves: BTreeMap<u64, solidcraft_engine::doc::Mat>,
        exploded: BTreeMap<u64, solidcraft_engine::doc::Mat>,
        visibility: Value,
        shot: String,
        visual: Value,
        lights: solidcraft_engine::schema::render_lights::Visibility,
    }
    fn camera(app: &SolveApp) -> (solidcraft_viewport::UpAxis, [u64; 8]) {
        let c = app.cam;
        (c.up, [c.target.x, c.target.y, c.target.z, c.yaw, c.pitch, c.roll, c.distance, c.fov].map(f64::to_bits))
    }
    fn same_moves(a: &BTreeMap<u64, solidcraft_engine::doc::Mat>, b: &BTreeMap<u64, solidcraft_engine::doc::Mat>) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|((ka, a), (kb, b))| ka == kb && a.iter().flatten().zip(b.iter().flatten()).all(|(a, b)| a.to_bits() == b.to_bits()))
    }
    impl Guard {
        fn new(app: &SolveApp) -> Self {
            Self {
                uid: app.session.uid,
                document: Arc::downgrade(&app.session.doc),
                model: Arc::downgrade(&app.session.model.state()),
                revision: app.session.revision,
                camera: camera(app),
                moves: app.session.pending_moves.clone(),
                exploded: app.session.exploded.clone(),
                visibility: json!(app.session.visibility),
                shot: app.render_workspace.view.name.clone(),
                visual: super::super::ribbon::final_visual(app),
                lights: app.render_workspace.view.lights.visibility.clone(),
            }
        }
        fn current(&self, app: &SolveApp) -> bool {
            self.document.ptr_eq(&Arc::downgrade(&app.session.doc))
                && self.model.ptr_eq(&Arc::downgrade(&app.session.model.state()))
                && self.revision == app.session.revision
                && !app.session.model.is_stale()
                && self.camera == camera(app)
                && same_moves(&self.moves, &app.session.pending_moves)
                && same_moves(&self.exploded, &app.session.exploded)
                && self.visibility == json!(app.session.visibility)
                && self.shot == app.render_workspace.view.name
                && self.visual == super::super::ribbon::final_visual(app)
                && self.lights == app.render_workspace.view.lights.visibility
                && !app.file_load.busy()
                && app.file_picker.is_none()
                && app.saving.is_none()
        }
    }

    enum Origin {
        Ui,
        Control { request: ControlRequest, echo: Option<ParamCheck> },
    }
    struct Pending {
        kind: Kind,
        params: Value,
        unfiltered: Value,
        guard: Guard,
        origin: Option<Origin>,
        scope: Option<solidcraft_engine::io::vfs::PathScope>,
        cancel: CancelToken,
        cancelled: Option<String>,
        worker: Option<Worker>,
        completion: Option<Result<Value, String>>,
        committed: bool,
        applied: bool,
        unavailable: Option<String>,
        warnings: Vec<String>,
        level: ParamCheck,
        #[cfg(test)]
        proof_ready: Option<SyncSender<bool>>,
        #[cfg(test)]
        retired_probe: Option<SyncSender<()>>,
        #[cfg(test)]
        disconnect_retirement: bool,
        #[cfg(test)]
        disconnect_input: bool,
    }
    struct PreparationInput {
        snapshot: solidcraft_engine::Session,
        kind: Kind,
        params: Value,
        cancel: CancelToken,
        scope: Option<solidcraft_engine::io::vfs::PathScope>,
    }
    struct Worker {
        ready: Receiver<Result<Prepared, String>>,
        returning: SyncSender<Prepared>,
        acknowledged: Receiver<()>,
        prepared: Option<Prepared>,
        returned: bool,
        // A worker which disconnects before receiving its source cannot cause
        // that source to be destroyed on the window. True app teardown owns it.
        unavailable_input: Option<PreparationInput>,
    }
    impl Worker {
        fn retire(&mut self) -> Result<(), String> {
            if self.returned {
                return Ok(());
            }
            let Some(prepared) = self.prepared.take() else { return Ok(()) };
            match self.returning.try_send(prepared) {
                Ok(()) => {
                    self.returned = true;
                    Ok(())
                }
                Err(TrySendError::Full(prepared) | TrySendError::Disconnected(prepared)) => {
                    // Never free a returned heavy capture on the interactive UI,
                    // report released credit, or start an unbounded fallback.
                    self.prepared = Some(prepared);
                    Err(solidcraft_ui_text::tr("render.preparation_retirement_failed").into())
                }
            }
        }
    }
    fn command(kind: Kind) -> &'static str {
        match kind {
            Kind::Check => "render.preflight",
            Kind::Start => "render.start",
        }
    }
    fn cancel_pending(pending: &mut Pending, reason: String) {
        if pending.committed {
            return;
        }
        if pending.cancelled.is_none() {
            pending.cancelled = Some(reason)
        }
        pending.cancel.cancel();
    }
    fn begin(
        app: &mut SolveApp,
        kind: Kind,
        request: Value,
        origin: Origin,
        scope: Option<solidcraft_engine::io::vfs::PathScope>,
        level: ParamCheck,
    ) -> Result<(), String> {
        if app.render_preparation.pending.is_some() || super::super::live::final_pending(app) || batches::unavailable(&app.render_preparation.batches)
        {
            return Err(app.tr("render.ribbon_job_active").into());
        }
        if kind == Kind::Start && batches::busy(&app.render_preparation.batches, app.session.uid) && request.get("replace") != Some(&json!(true)) {
            return Err(app.tr("render.ribbon_job_active").into());
        }
        if app.file_load.busy() || app.file_picker.is_some() || app.saving.is_some() {
            return Err(app.tr("render.ribbon_finish_dialog").into());
        }
        app.session.set_validation(app.preferences.validation);
        let params = super::super::light::filter_request(app, request.clone())?;
        let issues: Vec<String> = solidcraft_engine::schema::issues(command(kind), &params).into_iter().map(|issue| issue.message).collect();
        if level == ParamCheck::Strict
            && let Some(first) = issues.first()
        {
            return Err(solidcraft_engine::params::bad(command(kind), first).to_string());
        }
        let pending = Pending {
            kind,
            params,
            unfiltered: request,
            guard: Guard::new(app),
            origin: Some(origin),
            scope,
            cancel: CancelToken::default(),
            cancelled: None,
            worker: None,
            completion: None,
            committed: false,
            applied: false,
            unavailable: None,
            warnings: if level == ParamCheck::Warn { issues } else { Vec::new() },
            level,
            #[cfg(test)]
            proof_ready: app.render_preparation.proof_ready.clone(),
            #[cfg(test)]
            retired_probe: app.render_preparation.retired_probe.clone(),
            #[cfg(test)]
            disconnect_retirement: app.render_preparation.disconnect_retirement,
            #[cfg(test)]
            disconnect_input: app.render_preparation.disconnect_input,
        };
        super::super::live::suspend_for_preparation(app);
        app.render_preparation.pending = Some(pending);
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn test_configure(app: &mut SolveApp, ready: SyncSender<bool>, retired: SyncSender<()>, disconnect: bool) {
        app.render_preparation.proof_ready = Some(ready);
        app.render_preparation.retired_probe = Some(retired);
        app.render_preparation.disconnect_retirement = disconnect;
    }
    #[cfg(test)]
    pub(crate) fn test_disconnect_input(app: &mut SolveApp) {
        app.render_preparation.disconnect_input = true;
    }

    pub(crate) fn begin_ui(app: &mut SolveApp, kind: Kind, request: Value) -> Result<(), String> {
        begin(app, kind, request, Origin::Ui, solidcraft_engine::io::vfs::current_scope(), app.session.param_check)
    }
    pub(crate) fn begin_batch_ui(app: &mut SolveApp, request: Value) -> Result<(), String> {
        batches::begin(app, request, Origin::Ui, solidcraft_engine::io::vfs::current_scope(), app.session.param_check)
    }
    pub(crate) fn busy(app: &SolveApp) -> bool {
        app.render_preparation.pending.is_some() || batches::busy(&app.render_preparation.batches, app.session.uid)
    }
    pub(crate) fn cancel(app: &mut SolveApp) -> bool {
        let Some(pending) = &mut app.render_preparation.pending else {
            let mut state = std::mem::take(&mut app.render_preparation);
            let uid = app.session.uid;
            let cancelled = batches::cancel_owner(app, &mut state.batches, uid);
            app.render_preparation = state;
            return cancelled;
        };
        if pending.committed || pending.applied {
            return false;
        }
        cancel_pending(pending, solidcraft_ui_text::tr("render.preparation_cancelled").into());
        true
    }
    pub(crate) fn document_closing(app: &mut SolveApp, uid: u64) {
        let mut state = std::mem::take(&mut app.render_preparation);
        if let Some(pending) = &mut state.pending
            && pending.guard.uid == uid
            && !pending.committed
        {
            cancel_pending(pending, solidcraft_ui_text::tr("render.preparation_cancelled").into());
        }
        batches::cancel_owner(app, &mut state.batches, uid);
        app.render_preparation = state;
    }

    fn spawn(app: &SolveApp, pending: &Pending) -> Result<Worker, String> {
        let (input_tx, input_rx) = std::sync::mpsc::sync_channel::<PreparationInput>(1);
        let (ready_tx, ready) = std::sync::mpsc::sync_channel(1);
        let (returning, retired) = std::sync::mpsc::sync_channel(1);
        let (ack, acknowledged) = std::sync::mpsc::sync_channel(1);
        #[cfg(test)]
        let gate = app.apply_gate.clone();
        #[cfg(test)]
        let proof_ready = pending.proof_ready.clone();
        #[cfg(test)]
        let retired_probe = pending.retired_probe.clone();
        #[cfg(test)]
        let disconnect_retirement = pending.disconnect_retirement;
        #[cfg(test)]
        let disconnect_input = pending.disconnect_input;
        #[cfg(test)]
        let (input_closed_tx, input_closed_rx) = std::sync::mpsc::sync_channel(1);
        solidcraft_engine::kernel::kernel_thread("render-preparation")
            .spawn(move || {
                #[cfg(test)]
                if disconnect_input {
                    drop(input_rx);
                    let _ = input_closed_tx.send(());
                    return;
                }
                let Ok(PreparationInput { snapshot, kind, params, cancel, scope }) = input_rx.recv() else { return };
                solidcraft_engine::io::vfs::scoped(scope, || {
                    let result = prepare(snapshot, kind, params, cancel).map_err(|error| error.to_string());
                    #[cfg(test)]
                    if let Some(ready) = proof_ready {
                        let _ = ready.send(result.is_ok());
                    }
                    #[cfg(test)]
                    if let Some(gate) = gate {
                        // Deterministic test delay after proof owns its actual leases.
                        // A disconnected gate drops the result here on this worker.
                        if gate.lock().map_or(true, |gate| gate.recv().is_err()) {
                            drop(result);
                            let _ = ready_tx.send(Err("render preparation test gate disconnected".into()));
                            let _ = ack.send(());
                            if let Some(retired) = retired_probe {
                                let _ = retired.send(());
                            }
                            return;
                        }
                    }
                    let successful = result.is_ok();
                    #[cfg(test)]
                    if disconnect_retirement && successful {
                        // Inject an actual disconnected return/ack channel after
                        // handing the owned proof to the ordinary UI receiver.
                        if let Err(returned) = ready_tx.send(result) {
                            drop(returned);
                        }
                        return;
                    }
                    match ready_tx.send(result) {
                        Ok(()) if successful => {
                            if let Ok(prepared) = retired.recv() {
                                drop(prepared);
                            }
                        }
                        // A closed window never leaves a heavy result on that window.
                        // SendError retains it until this worker's drop below.
                        Err(returned) => drop(returned),
                        Ok(()) => {}
                    }
                    let _ = ack.send(());
                    #[cfg(test)]
                    if let Some(retired) = retired_probe {
                        let _ = retired.send(());
                    }
                });
            })
            .map_err(|error| error.to_string())?;
        // Only create the heavy source after a worker exists. Failed OS thread
        // creation therefore drops no scratch Session on the window.
        let mut snapshot = app.session.scratch();
        snapshot.param_check = pending.level;
        let input = PreparationInput {
            snapshot,
            kind: pending.kind,
            params: pending.params.clone(),
            cancel: pending.cancel.clone(),
            scope: pending.scope.clone(),
        };
        #[cfg(test)]
        if disconnect_input {
            let _ = input_closed_rx.recv();
        }
        let unavailable_input = input_tx.try_send(input).err().map(|error| match error {
            TrySendError::Full(input) | TrySendError::Disconnected(input) => input,
        });
        Ok(Worker { ready, returning, acknowledged, prepared: None, returned: false, unavailable_input })
    }
    fn answer(pending: &mut Pending) {
        let Some(origin) = pending.origin.take() else { return };
        let result = pending.completion.take().unwrap_or_else(|| Err(solidcraft_ui_text::tr("render.preparation_retirement_failed").into()));
        if let Origin::Control { request, echo } = origin {
            let mut reply = match result {
                Ok(value) => json!({"ok":true,"result":value}),
                Err(error) => json!({"ok":false,"error":error}),
            };
            if let Some(level) = echo {
                reply["param_check"] = json!(level.name())
            }
            let _ = request.reply.send(reply);
        }
    }
    fn apply(app: &mut SolveApp, pending: &mut Pending) {
        let kind = pending.kind;
        let level = pending.level;
        let scope = pending.scope.clone();
        let chat_turn = match &pending.origin {
            Some(Origin::Control { request, .. }) => request.chat_turn.clone(),
            _ => None,
        };
        let Some(prepared) = pending.worker.as_mut().and_then(|worker| worker.prepared.as_mut()) else { return };
        let mut committed = false;
        let mut result = crate::agent_chat::with_tool_request(app, chat_turn.as_ref(), |app| {
            solidcraft_engine::io::vfs::scoped(scope, || {
                app.finish_render_preparation(command(kind), &pending.params, |session| {
                    let own = session.param_check;
                    session.param_check = level;
                    let result = finish_retained(session, prepared).map_err(|error| error.to_string());
                    session.param_check = own;
                    committed = kind == Kind::Start && result.is_ok();
                    result
                })
            })
        });
        pending.committed = committed;
        if let Ok(value) = &mut result {
            if !pending.warnings.is_empty()
                && let Some(object) = value.as_object_mut()
            {
                object.insert("param_warnings".into(), json!(pending.warnings));
            }
            if matches!(pending.origin, Some(Origin::Ui)) && pending.kind == Kind::Start {
                result = super::super::ribbon::follow_started(app, value).map(|()| value.clone());
            }
        }
        if pending.kind == Kind::Check {
            match &result {
                Ok(reply) => {
                    super::super::cpu::record_checked(app, &pending.unfiltered, reply);
                    super::super::inspector::record_check(app, &pending.unfiltered, reply);
                    app.set_status(app.tr("render.image_supported").to_string(), false);
                }
                Err(error) => super::super::inspector::record_check(app, &pending.unfiltered, &json!({"error":error})),
            }
        }
        if let Err(error) = &result {
            app.set_status(error.clone(), true)
        }
        pending.completion = Some(result);
        pending.applied = true;
    }

    fn report_failure(app: &mut SolveApp, pending: &Pending, error: &str) {
        if pending.kind == Kind::Check {
            super::super::inspector::record_check(app, &pending.unfiltered, &json!({"error":error}));
        }
        app.set_status(error.to_string(), pending.cancelled.is_none());
    }

    pub(crate) fn tick(app: &mut SolveApp, ctx: &Context) {
        let mut state = std::mem::take(&mut app.render_preparation);
        // Always poll an existing batch worker, including while an individual
        // request waits. A waiting Single cannot deadlock batch retirement.
        batches::poll(app, ctx, &mut state.batches);
        // A queued same-document replacement cannot overtake the original
        // first batch front. Prove/commit/retire that front first, then leave
        // later fronts paused while replacement proof runs. A failed proof
        // therefore resumes the original queue without cancelling it.
        let prior_first = state
            .pending
            .as_ref()
            .filter(|p| p.kind == Kind::Start && p.unfiltered.get("replace") == Some(&json!(true)))
            .filter(|p| p.worker.is_none() && p.cancelled.is_none() && batches::first_pending(&state.batches, p.guard.uid))
            .map(|p| p.guard.uid);
        if let Some(uid) = prior_first {
            batches::schedule(app, ctx, &mut state.batches, Some(uid));
        }
        let Some(pending) = &mut state.pending else {
            batches::schedule(app, ctx, &mut state.batches, None);
            app.render_preparation = state;
            return;
        };
        if !pending.applied {
            if pending.worker.is_none() && batches::unavailable(&state.batches) {
                cancel_pending(pending, app.tr("render.preparation_unavailable").into());
            }
            if !pending.guard.current(app) {
                cancel_pending(pending, app.tr("render.live.final_changed").into())
            }
            if let Some(Origin::Control { request, .. }) = &pending.origin
                && crate::control::chat_turn_cancelled(request.chat_turn.as_deref())
            {
                cancel_pending(pending, solidcraft_ui_text::tr("render.preparation_cancelled").into());
            }
        }
        if pending.worker.is_none() {
            if let Some(error) = &pending.cancelled {
                report_failure(app, pending, error);
                pending.completion = Some(Err(error.clone()));
                answer(pending);
                state.pending = None;
            } else if prior_first.is_none() && !batches::lane_busy(&state.batches) && super::super::live::preparation_ready(app) {
                match spawn(app, pending) {
                    Ok(worker) => {
                        if worker.unavailable_input.is_some() {
                            pending.unavailable = Some(app.tr("render.preparation_retirement_failed").into());
                        }
                        pending.worker = Some(worker);
                    }
                    Err(error) => {
                        report_failure(app, pending, &error);
                        pending.completion = Some(Err(error));
                        answer(pending);
                        state.pending = None
                    }
                }
            }
            app.render_preparation = state;
            ctx.request_repaint_after(std::time::Duration::from_millis(20));
            return;
        }
        if pending.unavailable.is_none() {
            let ready = pending.worker.as_ref().filter(|_| !pending.applied).map(|worker| worker.ready.try_recv());
            match ready {
                Some(Ok(Ok(prepared))) => {
                    if let Some(worker) = &mut pending.worker {
                        worker.prepared = Some(prepared)
                    }
                    if let Some(error) = &pending.cancelled {
                        report_failure(app, pending, error);
                        pending.completion = Some(Err(error.clone()));
                        pending.applied = true;
                    } else {
                        apply(app, pending);
                    }
                }
                Some(Ok(Err(error))) => {
                    report_failure(app, pending, &error);
                    pending.completion = Some(Err(error));
                    pending.applied = true
                }
                Some(Err(TryRecvError::Disconnected)) => pending.unavailable = Some(app.tr("render.preparation_retirement_failed").into()),
                _ => {}
            }
            if pending.applied {
                if let Some(worker) = &mut pending.worker
                    && let Err(error) = worker.retire()
                {
                    pending.unavailable = Some(error)
                }
                if pending.unavailable.is_none() {
                    match pending.worker.as_ref().map(|worker| worker.acknowledged.try_recv()) {
                        Some(Ok(())) => {
                            answer(pending);
                            state.pending = None
                        }
                        Some(Err(TryRecvError::Disconnected)) => pending.unavailable = Some(app.tr("render.preparation_retirement_failed").into()),
                        _ => {}
                    }
                }
            }
        }
        if let Some(pending) = &mut state.pending
            && let Some(error) = &pending.unavailable
        {
            app.set_status(error.clone(), true);
            // A committed Start has a real identity even if retirement fails.
            // Never hide that owning job or cancel its transferred token.
            if !pending.committed {
                pending.completion = Some(Err(error.clone()))
            }
            answer(pending);
        }
        app.render_preparation = state;
        if app.render_preparation.pending.is_some() || batches::has_work(&app.render_preparation.batches) {
            ctx.request_repaint_after(std::time::Duration::from_millis(20))
        }
    }

    pub(crate) fn progress(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
        let Some(pending) = &app.render_preparation.pending else { return batches::progress(app, ui) };
        let key = if pending.unavailable.is_some() {
            "render.preparation_unavailable"
        } else if pending.cancelled.is_some() {
            "render.image_cancelling"
        } else if pending.applied {
            "render.preparation_retiring"
        } else if pending.worker.is_none() && batches::lane_busy(&app.render_preparation.batches) {
            "render.batch_waiting_worker"
        } else if pending.worker.is_none() {
            "render.live.final_waiting"
        } else if pending.kind == Kind::Check {
            "render.preparation_checking"
        } else {
            "render.image_preparing"
        };
        crate::ui_kit::text(ui, app.tr(key), crate::ui_kit::Text::Inline);
        let cancel = crate::ui_kit::button(
            ui,
            app.tr("cmd.render_cancel"),
            crate::ui_kit::Button::Enabled(pending.unavailable.is_none() && (!pending.applied || pending.committed)),
        );
        crate::scenario::publish_target("render:preparation-cancel", cancel.rect);
        if cancel.clicked() {
            super::super::ribbon::perform_job(app, "render.cancel").unwrap_or_else(|error| app.set_status(error, true))
        }
        true
    }
    pub(crate) fn inspect(app: &SolveApp) -> Value {
        app.render_preparation.pending.as_ref().map_or(Value::Null, |pending| {
            json!({
                "kind":command(pending.kind),"waiting_for_live":pending.worker.is_none()&&!super::super::live::preparation_ready(app),
                "waiting_for_worker":pending.worker.is_none()&&batches::lane_busy(&app.render_preparation.batches),"preparing":pending.worker.is_some()&&!pending.applied,
                "cancelled":pending.cancelled,"retiring":pending.applied,"unavailable":pending.unavailable,"committed":pending.committed
            })
        })
    }
    pub(crate) fn inspect_batches(app: &SolveApp) -> Value {
        batches::inspect(&app.render_preparation.batches)
    }

    // One coordinator shares the native preparation lane with individual Check/Start.
    mod batches {
        use super::*;
        use solidcraft_engine::render_preparation::batch as api;
        use std::sync::mpsc;

        #[derive(Default)]
        pub(super) struct State {
            slots: [Option<Slot>; crate::documents::MAX_OPEN],
            active: Option<Active>,
            cursor: usize,
        }
        struct Source {
            uid: u64,
            document: Weak<solidcraft_engine::doc::Document>,
            model: Weak<solidcraft_engine::doc::ModelState>,
            revision: u64,
            moves: BTreeMap<u64, solidcraft_engine::doc::Mat>,
            exploded: BTreeMap<u64, solidcraft_engine::doc::Mat>,
            context: Value,
        }
        impl Source {
            fn context(s: &solidcraft_engine::Session) -> Value {
                json!({"visibility":s.visibility,"sketch":s.active_sketch,"feature_sketch":s.feature_edit_sketch,
                    "component":s.active_component,"occurrence":s.active_occurrence_path,
                    "camera":s.camera.map(|c|(c.up,[c.target.x,c.target.y,c.target.z,c.yaw,c.pitch,c.roll,c.distance,c.fov].map(f64::to_bits)))})
            }
            fn new(s: &solidcraft_engine::Session) -> Self {
                Self {
                    uid: s.uid,
                    document: Arc::downgrade(&s.doc),
                    model: Arc::downgrade(&s.model.state()),
                    revision: s.revision,
                    moves: s.pending_moves.clone(),
                    exploded: s.exploded.clone(),
                    context: Self::context(s),
                }
            }
            fn current(&self, s: &solidcraft_engine::Session) -> bool {
                self.uid == s.uid
                    && self.document.ptr_eq(&Arc::downgrade(&s.doc))
                    && self.model.ptr_eq(&Arc::downgrade(&s.model.state()))
                    && self.revision == s.revision
                    && !s.model.is_stale()
                    && same_moves(&self.moves, &s.pending_moves)
                    && same_moves(&self.exploded, &s.exploded)
                    && self.context == Self::context(s)
            }
        }
        struct First {
            params: Option<Value>,
            scope: Option<solidcraft_engine::io::vfs::PathScope>,
            source: Source,
            origin: Option<Origin>,
            level: ParamCheck,
            warnings: Vec<String>,
        }
        struct Slot {
            uid: u64,
            first: Option<First>,
            plan: Option<api::Plan>,
            cancelled: Option<String>,
            #[cfg(test)]
            proof_ready: Option<SyncSender<bool>>,
            #[cfg(test)]
            retired_probe: Option<SyncSender<()>>,
            #[cfg(test)]
            disconnect_retirement: bool,
        }
        // One fixed, by-value message in the existing capacity-one channel. Private
        // constructors make these slots exclusive; no new Box/allocation or owner
        // transfer is introduced before the empty worker has started. The extra
        // inline Option slot is bounded UI channel metadata, not renderer credit.
        struct Input {
            claim: Option<api::Claim>,
            retire: Option<api::Plan>,
        }
        impl Input {
            fn claim(claim: api::Claim) -> Self {
                Self { claim: Some(claim), retire: None }
            }
            fn retire(plan: api::Plan) -> Self {
                Self { claim: None, retire: Some(plan) }
            }
            fn is_retirement(&self) -> bool {
                self.retire.is_some()
            }
        }
        struct Worker {
            ready: Receiver<api::Prepared>,
            returning: SyncSender<api::Prepared>,
            acknowledged: Receiver<()>,
            prepared: Option<api::Prepared>,
            _unavailable_input: Option<Input>,
            returned: bool,
        }
        struct Active {
            slot: usize,
            worker: Worker,
            cancel: CancelToken,
            cleanup_only: bool,
            applied: bool,
            committed: bool,
            completion: Option<Result<Value, String>>,
            unavailable: Option<String>,
        }
        impl Drop for State {
            fn drop(&mut self) {
                // Only the real app teardown destroys these global slots. Tab,
                // document and host lifecycles never replace this owner.
                if let Some(active) = &mut self.active {
                    if !active.committed {
                        active.cancel.cancel();
                    }
                    let _ = active.worker.retire();
                }
            }
        }
        impl Worker {
            fn retire(&mut self) -> Result<(), String> {
                if self.returned {
                    return Ok(());
                }
                let Some(prepared) = self.prepared.take() else { return Ok(()) };
                match self.returning.try_send(prepared) {
                    Ok(()) => {
                        self.returned = true;
                        Ok(())
                    }
                    Err(TrySendError::Full(p) | TrySendError::Disconnected(p)) => {
                        self.prepared = Some(p);
                        Err(solidcraft_ui_text::tr("render.preparation_retirement_failed").into())
                    }
                }
            }
            fn send(&mut self, sender: SyncSender<Input>, input: Input) -> Result<(), String> {
                match sender.try_send(input) {
                    Ok(()) => Ok(()),
                    Err(TrySendError::Full(input) | TrySendError::Disconnected(input)) => {
                        self._unavailable_input = Some(input);
                        Err(solidcraft_ui_text::tr("render.preparation_retirement_failed").into())
                    }
                }
            }
        }
        fn empty_worker(app: &SolveApp, slot: &Slot) -> Result<(Worker, SyncSender<Input>), String> {
            let (input_tx, input_rx) = mpsc::sync_channel::<Input>(1);
            let (ready_tx, ready) = mpsc::sync_channel(1);
            let (returning, returned) = mpsc::sync_channel(1);
            let (ack, acknowledged) = mpsc::sync_channel(1);
            #[cfg(test)]
            let gate = app.apply_gate.clone();
            #[cfg(test)]
            let proof = slot.proof_ready.clone();
            #[cfg(test)]
            let retired = slot.retired_probe.clone();
            #[cfg(test)]
            let disconnect = slot.disconnect_retirement;
            #[cfg(not(test))]
            let _ = (app, slot);
            solidcraft_engine::kernel::kernel_thread("render-batch-preparation")
                .spawn(move || {
                    let release = match input_rx.recv() {
                        Ok(Input { claim: Some(claim), retire: None }) => api::with_scope(claim, |claim| {
                            let prepared = api::prepare(claim);
                            #[cfg(test)]
                            if let Some(proof) = proof {
                                let _ = proof.send(true);
                            }
                            #[cfg(test)]
                            if let Some(gate) = gate
                                && gate.lock().map_or(true, |gate| gate.recv().is_err())
                            {
                                drop(prepared);
                                return true;
                            }
                            #[cfg(test)]
                            if disconnect {
                                drop(returned);
                                if let Err(prepared) = ready_tx.send(prepared) {
                                    drop(prepared);
                                }
                                return false;
                            }
                            match ready_tx.send(prepared) {
                                Ok(()) => {
                                    if let Ok(prepared) = returned.recv() {
                                        drop(prepared);
                                    }
                                }
                                Err(prepared) => drop(prepared),
                            }
                            true
                        }),
                        Ok(Input { claim: None, retire: Some(plan) }) => {
                            api::retire_plan(plan);
                            true
                        }
                        // Constructors never produce an invalid choice. Keep even an
                        // internally malformed message's last owners on this worker,
                        // under their own original scopes, before acknowledging it.
                        Ok(input) => {
                            if let Some(claim) = input.claim {
                                api::with_scope(claim, drop);
                            }
                            if let Some(plan) = input.retire {
                                api::retire_plan(plan);
                            }
                            true
                        }
                        Err(_) => true,
                    };
                    // Scope and all source/plan backing have retired before ack.
                    if !release {
                        return;
                    }
                    let _ = ack.send(());
                    #[cfg(test)]
                    if let Some(retired) = retired {
                        let _ = retired.send(());
                    }
                })
                .map_err(|e| e.to_string())?;
            Ok((Worker { ready, returning, acknowledged, prepared: None, _unavailable_input: None, returned: false }, input_tx))
        }
        pub(super) fn lane_busy(state: &State) -> bool {
            state.active.is_some()
        }
        pub(super) fn unavailable(state: &State) -> bool {
            state.active.as_ref().is_some_and(|a| a.unavailable.is_some())
        }
        pub(super) fn busy(state: &State, uid: u64) -> bool {
            state.slots.iter().flatten().any(|s| s.uid == uid)
        }
        pub(super) fn has_work(state: &State) -> bool {
            state.slots.iter().any(Option::is_some)
        }
        pub(super) fn first_pending(state: &State, uid: u64) -> bool {
            state.slots.iter().flatten().any(|s| s.uid == uid && s.first.is_some())
        }
        pub(super) fn begin(
            app: &mut SolveApp,
            params: Value,
            origin: Origin,
            scope: Option<solidcraft_engine::io::vfs::PathScope>,
            level: ParamCheck,
        ) -> Result<(), String> {
            if super::super::super::live::final_pending(app) {
                return Err(app.tr("render.ribbon_job_active").into());
            }
            if unavailable(&app.render_preparation.batches) || app.render_preparation.pending.as_ref().is_some_and(|p| p.unavailable.is_some()) {
                return Err(app.tr("render.preparation_unavailable").into());
            }
            if busy(&app.render_preparation.batches, app.session.uid) {
                return Err(app.tr("render.ribbon_job_active").into());
            }
            if app.file_load.busy() || app.file_picker.is_some() || app.saving.is_some() {
                return Err(app.tr("render.ribbon_finish_dialog").into());
            }
            app.session.set_validation(app.preferences.validation);
            let warnings: Vec<_> = solidcraft_engine::schema::issues("render.batch", &params).into_iter().map(|i| i.message).collect();
            if level == ParamCheck::Strict
                && let Some(first) = warnings.first()
            {
                return Err(solidcraft_engine::params::bad("render.batch", first).to_string());
            }
            let index = app
                .render_preparation
                .batches
                .slots
                .iter()
                .position(Option::is_none)
                .ok_or_else(|| app.tr("render.batch_owner_limit").to_string())?;
            // UI passes its already-filtered request. Native control preserves
            // the exact supplied API intent, including first-saved fallback.
            let first = First {
                params: Some(params),
                scope,
                source: Source::new(&app.session),
                origin: Some(origin),
                level,
                warnings: if level == ParamCheck::Warn { warnings } else { Vec::new() },
            };
            let slot = Slot {
                uid: app.session.uid,
                first: Some(first),
                plan: None,
                cancelled: None,
                #[cfg(test)]
                proof_ready: app.render_preparation.proof_ready.clone(),
                #[cfg(test)]
                retired_probe: app.render_preparation.retired_probe.clone(),
                #[cfg(test)]
                disconnect_retirement: app.render_preparation.disconnect_retirement,
            };
            super::super::super::live::suspend_for_preparation(app);
            app.render_preparation.batches.slots[index] = Some(slot);
            Ok(())
        }
        fn answer(first: &mut First, result: Result<Value, String>) {
            if let Some(Origin::Control { request, echo }) = first.origin.take() {
                let mut reply = match result {
                    Ok(v) => json!({"ok":true,"result":v}),
                    Err(e) => json!({"ok":false,"error":e}),
                };
                if let Some(level) = echo {
                    reply["param_check"] = json!(level.name());
                }
                let _ = request.reply.send(reply);
            }
        }
        pub(super) fn cancel_owner(app: &mut SolveApp, state: &mut State, uid: u64) -> bool {
            let mut found = false;
            for (i, slot) in state.slots.iter_mut().enumerate() {
                if let Some(slot) = slot
                    && slot.uid == uid
                {
                    found = true;
                    slot.cancelled.get_or_insert_with(|| solidcraft_ui_text::tr("render.preparation_cancelled").into());
                    if let Some(active) = &mut state.active
                        && active.slot == i
                        && !active.committed
                    {
                        active.cancel.cancel();
                    }
                }
            }
            if found {
                let _ = crate::documents::with_render_owner(app, uid, |s, _| api::cancel(s));
            }
            found
        }
        fn commit(app: &mut SolveApp, uid: u64, prepared: &mut api::Prepared, first: Option<&First>) -> Result<Value, String> {
            let level = first.map(|f| f.level);
            let chat = first.and_then(|f| f.origin.as_ref()).and_then(|o| match o {
                Origin::Control { request, .. } => request.chat_turn.clone(),
                Origin::Ui => None,
            });
            let mut finish = |s: &mut solidcraft_engine::Session| {
                let own = s.param_check;
                if let Some(level) = level {
                    s.param_check = level;
                }
                let out = api::finish_retained(s, prepared).map_err(|e| e.to_string());
                s.param_check = own;
                out
            };
            if app.session.uid == uid {
                crate::agent_chat::with_tool_request(app, chat.as_ref(), |app| app.finish_render_preparation("render.batch", &Value::Null, finish))
            } else {
                crate::documents::with_render_owner(app, uid, |session, view| {
                    let out = finish(session);
                    if out.is_ok() {
                        super::super::super::follow_batch_view(view, session.render_queue_state());
                    }
                    out
                })
                .unwrap_or_else(|| Err(solidcraft_ui_text::tr("render.live.final_changed").into()))
            }
        }
        pub(super) fn poll(app: &mut SolveApp, ctx: &Context, state: &mut State) {
            let Some(mut active) = state.active.take() else { return };
            let Some(slot) = state.slots[active.slot].as_mut() else {
                active.unavailable = Some(app.tr("render.preparation_retirement_failed").into());
                state.active = Some(active);
                return;
            };
            let mut released = false;
            if !active.applied && !active.cleanup_only {
                let current = crate::documents::render_owner(app, slot.uid);
                if current.is_none() || slot.first.as_ref().is_some_and(|f| current.is_none_or(|s| !f.source.current(s))) {
                    slot.cancelled.get_or_insert_with(|| solidcraft_ui_text::tr("render.live.final_changed").into());
                    active.cancel.cancel();
                }
                if slot.first.as_ref().and_then(|f| f.origin.as_ref()).is_some_and(|o| match o {
                    Origin::Control { request, .. } => crate::control::chat_turn_cancelled(request.chat_turn.as_deref()),
                    Origin::Ui => false,
                }) {
                    slot.cancelled.get_or_insert_with(|| solidcraft_ui_text::tr("render.preparation_cancelled").into());
                    active.cancel.cancel();
                }
            }
            if active.unavailable.is_none() && !active.applied && !active.cleanup_only {
                match active.worker.ready.try_recv() {
                    Ok(mut prepared) => {
                        let mut result = commit(app, slot.uid, &mut prepared, slot.first.as_ref());
                        active.committed = prepared.committed();
                        if let Ok(reply) = &mut result {
                            if let Some(first) = &slot.first
                                && !first.warnings.is_empty()
                                && let Some(obj) = reply.as_object_mut()
                            {
                                obj.insert("param_warnings".into(), json!(first.warnings));
                            }
                            match api::take_plan(&mut prepared) {
                                Ok(plan) => slot.plan = plan,
                                Err(e) => result = Err(e.to_string()),
                            }
                        }
                        active.worker.prepared = Some(prepared);
                        active.applied = true;
                        active.completion = Some(result);
                    }
                    Err(TryRecvError::Disconnected) => active.unavailable = Some(app.tr("render.preparation_retirement_failed").into()),
                    Err(TryRecvError::Empty) => {}
                }
            }
            if active.unavailable.is_none() && (active.applied || active.cleanup_only) {
                if let Err(e) = active.worker.retire() {
                    active.unavailable = Some(e);
                }
                if active.unavailable.is_none() {
                    match active.worker.acknowledged.try_recv() {
                        Ok(()) => {
                            let result = active.completion.take().unwrap_or(Ok(Value::Null));
                            if let Err(error) = &result {
                                app.set_status(error.clone(), slot.cancelled.is_none() || active.cleanup_only);
                            }
                            if let Some(first) = &mut slot.first {
                                answer(first, result);
                            }
                            slot.first = None;
                            let index = active.slot;
                            released = true;
                            // A Plan is never dropped by clearing this slot.
                            // Cancelled committed plans get a separate worker retirement.
                            if state.slots[index].as_ref().is_some_and(|s| s.plan.is_none()) {
                                state.slots[index] = None;
                            }
                        }
                        Err(TryRecvError::Disconnected) => active.unavailable = Some(app.tr("render.preparation_retirement_failed").into()),
                        Err(TryRecvError::Empty) => {}
                    }
                }
            }
            if let Some(error) = &active.unavailable {
                app.set_status(error.clone(), true);
                if let Some(slot) = state.slots[active.slot].as_mut()
                    && let Some(first) = &mut slot.first
                {
                    let reply = if active.committed { active.completion.take().unwrap_or_else(|| Err(error.clone())) } else { Err(error.clone()) };
                    answer(first, reply);
                }
            }
            if !released {
                state.active = Some(active);
                ctx.request_repaint_after(std::time::Duration::from_millis(20));
            }
        }
        pub(super) fn schedule(app: &mut SolveApp, ctx: &Context, state: &mut State, only_first: Option<u64>) {
            if state.active.is_some() {
                return;
            }
            for offset in 0..state.slots.len() {
                let index = (state.cursor + offset) % state.slots.len();
                let Some(slot) = state.slots[index].as_mut() else { continue };
                if only_first.is_some_and(|uid| slot.uid != uid || slot.first.is_none()) {
                    continue;
                }
                let owner = crate::documents::render_owner(app, slot.uid);
                if owner.is_none() || slot.first.as_ref().is_some_and(|f| owner.is_none_or(|s| !f.source.current(s))) {
                    slot.cancelled.get_or_insert_with(|| solidcraft_ui_text::tr("render.live.final_changed").into());
                }
                if slot.first.as_ref().and_then(|f| f.origin.as_ref()).is_some_and(|o| match o {
                    Origin::Control { request, .. } => crate::control::chat_turn_cancelled(request.chat_turn.as_deref()),
                    Origin::Ui => false,
                }) {
                    slot.cancelled.get_or_insert_with(|| solidcraft_ui_text::tr("render.preparation_cancelled").into());
                }
                if slot.cancelled.is_some() && slot.plan.is_none() {
                    if let Some(first) = &mut slot.first {
                        answer(first, Err(slot.cancelled.clone().unwrap_or_default()));
                    }
                    state.slots[index] = None;
                    continue;
                }
                if slot.first.is_some() && !super::super::super::live::preparation_ready(app) {
                    continue;
                }
                if let Some(plan) = &slot.plan
                    && slot.cancelled.is_none()
                    && let Some(session) = owner
                {
                    let q = session.render_queue_state();
                    if q.waiting == 0 {
                        slot.cancelled = Some(solidcraft_ui_text::tr("render.preparation_cancelled").into());
                    } else if q.running != 0 {
                        continue;
                    }
                    if !plan.owner_matches(session) {
                        slot.cancelled = Some(solidcraft_ui_text::tr("render.live.final_changed").into());
                    }
                }
                // Start the empty receiver before taking any heavy claim/plan.
                let (mut worker, sender) = match empty_worker(app, slot) {
                    Ok(w) => w,
                    Err(error) => {
                        // Spawn failed before a scratch owner existed. An idle heavy
                        // plan remains in its bounded slot, unavailable until teardown.
                        let cancel = CancelToken::default();
                        let (tx, rx) = mpsc::sync_channel(1);
                        drop(tx);
                        let (ret, _) = mpsc::sync_channel(1);
                        let (ack, acked) = mpsc::sync_channel(1);
                        drop(ack);
                        let held =
                            Worker { ready: rx, returning: ret, acknowledged: acked, prepared: None, _unavailable_input: None, returned: false };
                        state.active = Some(Active {
                            slot: index,
                            worker: held,
                            cancel,
                            cleanup_only: true,
                            applied: true,
                            committed: false,
                            completion: Some(Err(error.clone())),
                            unavailable: Some(error),
                        });
                        return;
                    }
                };
                let cancel = CancelToken::default();
                let mut completion = None;
                let input = if slot.cancelled.is_some() {
                    slot.plan.take().map(Input::retire)
                } else if let Some(first) = &mut slot.first {
                    let result = crate::documents::with_render_owner(app, slot.uid, |session, _| {
                        let own = session.param_check;
                        session.param_check = first.level;
                        let result = api::claim(session, &mut first.params, &mut first.scope, cancel.clone());
                        session.param_check = own;
                        result
                    });
                    match result {
                        Some(Ok(claim)) => Some(Input::claim(claim)),
                        Some(Err(error)) => {
                            slot.cancelled = Some(error.to_string());
                            completion = Some(Err(error.to_string()));
                            None
                        }
                        None => {
                            slot.cancelled = Some(solidcraft_ui_text::tr("render.live.final_changed").into());
                            None
                        }
                    }
                } else {
                    match crate::documents::with_render_owner(app, slot.uid, |session, _| {
                        let result = api::claim_next(session, &mut slot.plan, cancel.clone());
                        if result.is_err()
                            && let Some(plan) = &slot.plan
                        {
                            // Route equality alone is not cancellation authority.
                            // Abort only this matching epoch before retiring it.
                            let _ = api::cancel_plan(session, plan);
                        }
                        result
                    }) {
                        Some(Ok(Some(claim))) => Some(Input::claim(claim)),
                        Some(Ok(None)) => None,
                        Some(Err(error)) => {
                            slot.cancelled = Some(error.to_string());
                            completion = Some(Err(error.to_string()));
                            slot.plan.take().map(Input::retire)
                        }
                        None => slot.plan.take().map(Input::retire),
                    }
                };
                if let Some(input) = input {
                    let cleanup = input.is_retirement();
                    let failure = worker.send(sender, input).err();
                    state.active = Some(Active {
                        slot: index,
                        worker,
                        cancel,
                        cleanup_only: cleanup,
                        applied: cleanup,
                        committed: false,
                        completion,
                        unavailable: failure,
                    });
                } else {
                    // Metadata-only refusal/None owns no capture. Close the empty
                    // worker input and wait its acknowledgement before reusing lane.
                    drop(sender);
                    state.active = Some(Active {
                        slot: index,
                        worker,
                        cancel,
                        cleanup_only: true,
                        applied: true,
                        committed: false,
                        completion: completion.or_else(|| slot.cancelled.clone().map(Err)),
                        unavailable: None,
                    });
                }
                state.cursor = (index + 1) % state.slots.len();
                ctx.request_repaint_after(std::time::Duration::from_millis(20));
                return;
            }
        }
        pub(super) fn inspect(state: &State) -> Value {
            json!({"owners":state.slots.iter().flatten().map(|s|json!({"uid":s.uid,"first":s.first.is_some(),"retained_plan_bytes":s.plan.as_ref().map(api::Plan::retained_bytes),"cancelled":s.cancelled})).collect::<Vec<_>>(),
                "worker":state.active.as_ref().map(|a|json!({"uid":state.slots[a.slot].as_ref().map(|s|s.uid),"preparing":!a.applied&&!a.cleanup_only,"retiring":a.applied||a.cleanup_only,"unavailable":a.unavailable,"committed":a.committed}))})
        }
        pub(super) fn progress(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
            let state = &app.render_preparation.batches;
            if !busy(state, app.session.uid) {
                return false;
            }
            let active = state.active.as_ref().filter(|a| state.slots[a.slot].as_ref().is_some_and(|s| s.uid == app.session.uid));
            if active.is_none() && app.session.render_queue_state().running != 0 {
                return false;
            }
            let key = match active {
                Some(a) if a.unavailable.is_some() => "render.preparation_unavailable",
                Some(a) if a.applied || a.cleanup_only => "render.preparation_retiring",
                Some(_) => "render.batch_preparing",
                None => "render.batch_waiting_worker",
            };
            crate::ui_kit::text(ui, app.tr(key), crate::ui_kit::Text::Inline);
            let cancel = crate::ui_kit::button(ui, app.tr("cmd.render_cancel"), crate::ui_kit::Button::Enabled(!unavailable(state)));
            crate::scenario::publish_target("render:preparation-cancel", cancel.rect);
            if cancel.clicked() {
                super::super::super::ribbon::perform_job(app, "render.cancel").unwrap_or_else(|e| app.set_status(e, true));
            }
            true
        }
    }

    fn cancel_batch_control(app: &mut SolveApp, req: ControlRequest) -> Option<ControlRequest> {
        let params = req.params.get("params").cloned().filter(|p| !p.is_null()).unwrap_or_else(|| json!({}));
        if params.get("job").is_some_and(|job| !job.is_null()) {
            return Some(req);
        }
        let echo = match crate::control::request_param_check(&req.method, &req.params) {
            Ok(level) => level,
            Err(error) => {
                let _ = req.reply.send(json!({"ok":false,"error":error}));
                return None;
            }
        };
        let result = (|| -> Result<Option<Value>, String> {
            let scope = crate::control::request_scope(app, &req.params)?.or_else(solidcraft_engine::io::vfs::current_scope);
            let level = echo.unwrap_or(app.session.param_check);
            let warnings: Vec<_> = solidcraft_engine::schema::issues("render.cancel", &params).into_iter().map(|i| i.message).collect();
            if level == ParamCheck::Strict
                && let Some(first) = warnings.first()
            {
                return Err(solidcraft_engine::params::bad("render.cancel", first).to_string());
            }
            let own = app.session.param_check;
            app.session.param_check = level;
            let checked =
                solidcraft_engine::io::vfs::scoped(scope, || solidcraft_engine::render_preparation::batch::check_cancel(&app.session, &params));
            app.session.param_check = own;
            if checked.map_err(|e| e.to_string())?.is_some() {
                return Ok(None);
            }
            let mut state = std::mem::take(&mut app.render_preparation);
            let uid = app.session.uid;
            let cancelled = batches::cancel_owner(app, &mut state.batches, uid);
            app.render_preparation = state;
            let mut reply = json!({"preparing":true,"cancel_requested":cancelled,"queue":app.session.render_queue_state()});
            if level == ParamCheck::Warn && !warnings.is_empty() {
                reply["param_warnings"] = json!(warnings);
            }
            Ok(Some(reply))
        })();
        let mut reply = match result {
            Ok(None) => return Some(req),
            Ok(Some(value)) => json!({"ok":true,"result":value}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        if let Some(level) = echo {
            reply["param_check"] = json!(level.name());
        }
        let _ = req.reply.send(reply);
        None
    }

    pub(crate) fn control(app: &mut SolveApp, req: ControlRequest) -> Option<ControlRequest> {
        if !super::enabled(app) {
            return Some(req);
        }
        if matches!(req.method.as_str(), "engine.batch" | "engine.script") {
            let steps = req.params.as_array().or_else(|| req.params.get("commands").and_then(Value::as_array));
            if steps.is_some_and(|steps| {
                steps.iter().any(|step| {
                    step.get("command")
                        .or_else(|| step.get("id"))
                        .and_then(Value::as_str)
                        .and_then(solidcraft_engine::find_command)
                        .is_some_and(|spec| matches!(spec.id, "render.start" | "render.preflight" | "render.batch"))
                })
            }) {
                let echo = match crate::control::request_param_check(&req.method, &req.params) {
                    Ok(level) => level,
                    Err(error) => {
                        let _ = req.reply.send(json!({"ok":false,"error":error}));
                        return None;
                    }
                };
                if let Err(error) = crate::control::request_scope(app, &req.params) {
                    let _ = req.reply.send(json!({"ok":false,"error":error}));
                    return None;
                }
                let mut value = json!({"ok":false,"error":"native render preparation must be sent as an individual engine.execute request"});
                if let Some(level) = echo {
                    value["param_check"] = json!(level.name());
                }
                let _ = req.reply.send(value);
                return None;
            }
            return Some(req);
        }
        if !matches!(req.method.as_str(), "engine.execute" | "command") {
            return Some(req);
        }
        let id = req
            .params
            .get("command")
            .or_else(|| req.params.get("id"))
            .and_then(Value::as_str)
            .and_then(solidcraft_engine::find_command)
            .map(|spec| spec.id);
        let kind = match id {
            Some("render.preflight") => Kind::Check,
            Some("render.start") => Kind::Start,
            Some("render.batch") => Kind::Start,
            Some("render.cancel")
                if app.session.render_queue_state().job.is_none() && batches::busy(&app.render_preparation.batches, app.session.uid) =>
            {
                return cancel_batch_control(app, req);
            }
            _ => return Some(req),
        };
        let echo = match crate::control::request_param_check(&req.method, &req.params) {
            Ok(level) => level,
            Err(error) => {
                let _ = req.reply.send(json!({"ok":false,"error":error}));
                return None;
            }
        };
        let scope = match crate::control::request_scope(app, &req.params) {
            Ok(scope) => scope.or_else(solidcraft_engine::io::vfs::current_scope),
            Err(error) => {
                let _ = req.reply.send(json!({"ok":false,"error":error}));
                return None;
            }
        };
        let params = req.params.get("params").cloned().filter(|value| !value.is_null()).unwrap_or_else(|| json!({}));
        let reply = req.reply.clone();
        let level = echo.unwrap_or(app.session.param_check);
        let chat_turn = req.chat_turn.clone();
        let batch = id == Some("render.batch");
        let result = crate::agent_chat::with_tool_request(app, chat_turn.as_ref(), |app| {
            if batch {
                batches::begin(app, params, Origin::Control { request: req, echo }, scope, level)
            } else {
                begin(app, kind, params, Origin::Control { request: req, echo }, scope, level)
            }
        });
        if let Err(error) = result {
            let mut value = json!({"ok":false,"error":error});
            if let Some(level) = echo {
                value["param_check"] = json!(level.name())
            }
            let _ = reply.send(value);
        }
        None
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{State, begin_batch_ui, begin_ui, busy, cancel, control, document_closing, inspect, inspect_batches, progress, tick};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    #[derive(Default)]
    pub(crate) struct State;
    pub(crate) fn busy(_: &SolveApp) -> bool {
        false
    }
    pub(crate) fn cancel(_: &mut SolveApp) -> bool {
        false
    }
    pub(crate) fn tick(_: &mut SolveApp, _: &Context) {}
    pub(crate) fn inspect(_: &SolveApp) -> Value {
        Value::Null
    }
    pub(crate) fn inspect_batches(_: &SolveApp) -> Value {
        Value::Null
    }
    pub(crate) fn document_closing(_: &mut SolveApp, _: u64) {}
    pub(crate) fn progress(_: &mut SolveApp, _: &mut egui::Ui) -> bool {
        false
    }
    pub(crate) fn begin_ui(_: &mut SolveApp, _: Kind, _: Value) -> Result<(), String> {
        Err("native preparation is unavailable".into())
    }
    pub(crate) fn begin_batch_ui(_: &mut SolveApp, _: Value) -> Result<(), String> {
        Err("native preparation is unavailable".into())
    }
    pub(crate) fn control(_: &mut SolveApp, req: crate::control::ControlRequest) -> Option<crate::control::ControlRequest> {
        Some(req)
    }
}
#[cfg(target_arch = "wasm32")]
pub(crate) use web::{State, begin_batch_ui, begin_ui, busy, cancel, control, document_closing, inspect, inspect_batches, progress, tick};

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use native::{test_configure, test_disconnect_input};
