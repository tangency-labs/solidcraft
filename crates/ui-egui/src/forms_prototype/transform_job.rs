//! Coalesced native Move, Rotate and Scale for hierarchical Forms.
use super::*;
#[cfg(not(target_arch = "wasm32"))]
use solidcraft_engine::forms_prototype::FormTransformJob;
use solidcraft_engine::forms_prototype::PreparedFormTransform;
use std::sync::mpsc::{Receiver, TryRecvError};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Owner {
    Pointer,
    Gizmo,
}
impl Owner {
    fn drag(self, forms: &super::State) -> Option<&Drag> {
        match self {
            Self::Pointer => forms.drag.as_ref(),
            Self::Gizmo => forms.gizmo_drag.as_ref(),
        }
    }

    fn drag_mut(self, forms: &mut super::State) -> Option<&mut Drag> {
        match self {
            Self::Pointer => forms.drag.as_mut(),
            Self::Gizmo => forms.gizmo_drag.as_mut(),
        }
    }
}

type Outcome = Result<PreparedFormTransform, String>;
struct Pending {
    sequence: u64,
    done: Receiver<Outcome>,
}
struct Gesture {
    owner: Owner,
    source: FormTransformStamp,
    space: FormTargetSpace,
    request: transform_feedback::Value,
    parameters: serde_json::Value,
    sequence: u64,
    settled: Option<u64>,
    authored: bool,
    failed: bool,
    released: bool,
    pending: Option<Pending>,
}
#[derive(Default)]
pub(super) struct State {
    gesture: Option<Gesture>,
    reset_gizmo: bool,
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(super) gate: Arc<std::sync::atomic::AtomicBool>,
}

/// Browser gestures and ordinary base cages keep the existing synchronous path.
pub(super) fn source(app: &SolveApp) -> Option<FormTransformStamp> {
    if cfg!(target_arch = "wasm32") || app.session.forms_prototype.as_ref().is_none_or(|f| f.hierarchy.is_none()) {
        return None;
    }
    FormTransformStamp::capture(&app.session)
}
pub(super) fn active(app: &SolveApp) -> bool {
    app.forms_prototype.transform_job.gesture.is_some()
}
pub(super) fn waiting(app: &SolveApp) -> bool {
    app.forms_prototype.transform_job.gesture.as_ref().is_some_and(|gesture| gesture.settled != Some(gesture.sequence))
}
pub(super) fn released(app: &SolveApp) -> bool {
    app.forms_prototype.transform_job.gesture.as_ref().is_some_and(|gesture| gesture.released)
}
pub(super) fn readout_phase(app: &SolveApp) -> Option<transform_feedback::Phase> {
    let gesture = app.forms_prototype.transform_job.gesture.as_ref()?;
    if gesture.failed || !gesture.source.is_current(&app.session) {
        return None;
    }
    Some(if gesture.settled != Some(gesture.sequence) {
        transform_feedback::Phase::Requested
    } else if live_cad::mesh(app).is_some() {
        transform_feedback::Phase::Accepted
    } else {
        transform_feedback::Phase::Fitting
    })
}
pub(super) fn inspect(app: &SolveApp) -> serde_json::Value {
    app.forms_prototype.transform_job.gesture.as_ref().map_or_else(
        || json!({"active":false}),
        |gesture| {
            json!({
                "active":true,"released":gesture.released,"pending":gesture.pending.is_some(),
                "requested":gesture.sequence,"settled":gesture.settled,"authored":gesture.authored,"failed":gesture.failed,
            })
        },
    )
}

/// The frozen gesture packet makes a repeated scalar/vector request an
/// identical absolute target. Avoid rebuilding a dense positions array merely
/// because a worker repaint arrived while the pointer stayed still.
pub(super) fn repeats(app: &SolveApp, owner: Owner, request: transform_feedback::Value) -> bool {
    app.forms_prototype
        .transform_job
        .gesture
        .as_ref()
        .is_some_and(|gesture| gesture.owner == owner && !gesture.released && gesture.request == request)
}

pub(super) fn submit(
    app: &mut SolveApp,
    owner: Owner,
    space: FormTargetSpace,
    request: transform_feedback::Value,
    mut parameters: serde_json::Value,
) -> bool {
    // The authoring checkpoint is chosen when the job starts, not when the
    // pointer sample arrives. Discarded results never consume the first one.
    parameters["checkpoint"] = json!(false);
    let slot = &mut app.forms_prototype.transform_job.gesture;
    if let Some(gesture) = slot {
        if gesture.owner != owner || gesture.space != space || gesture.released || !gesture.source.is_current(&app.session) {
            stale(app);
            return false;
        }
        gesture.request = request;
        if gesture.parameters == parameters {
            return true;
        }
        gesture.parameters = parameters;
        gesture.sequence = gesture.sequence.wrapping_add(1);
        gesture.failed = false;
    } else {
        let Some(source) = owner.drag(&app.forms_prototype).and_then(|drag| drag.job_source.as_ref()) else { return false };
        if !source.is_current(&app.session) {
            stale(app);
            return false;
        }
        app.forms_prototype.transform_job.gesture = Some(Gesture {
            owner,
            source: source.clone(),
            space,
            request,
            parameters,
            sequence: 0,
            settled: None,
            authored: false,
            failed: false,
            released: false,
            pending: None,
        });
    }
    app.status = None;
    true
}

pub(super) fn release(app: &mut SolveApp) {
    let Some(gesture) = &mut app.forms_prototype.transform_job.gesture else { return };
    gesture.released = true;
    if gesture.pending.is_none() && gesture.settled == Some(gesture.sequence) {
        app.forms_prototype.transform_job.gesture = None;
        transform_feedback::clear(app);
    }
}
pub(super) fn cancel(app: &mut SolveApp) -> bool {
    let Some(gesture) = app.forms_prototype.transform_job.gesture.take() else { return false };
    transform_feedback::clear(app);
    if gesture.authored && gesture.source.is_current(&app.session) {
        run(app, "form.prototype.undo", json!({}));
    }
    true
}

/// The last submitted sample stays with its own document. No pointer from a
/// different tab can continue the gesture while its result is waiting.
pub(super) fn suspend(app: &mut SolveApp) {
    if !active(app) {
        return;
    }
    release(app);
    app.forms_prototype.drag = None;
    app.forms_prototype.gizmo_drag = None;
    app.forms_prototype.transform_job.reset_gizmo = true;
    selection::cancel_press(app);
}
pub(super) fn take_gizmo_reset(app: &mut SolveApp) -> bool {
    std::mem::take(&mut app.forms_prototype.transform_job.reset_gizmo)
}
fn stale(app: &mut SolveApp) {
    app.forms_prototype.transform_job.gesture = None;
    app.forms_prototype.drag = None;
    app.forms_prototype.gizmo_drag = None;
    app.forms_prototype.transform_job.reset_gizmo = true;
    selection::cancel_press(app);
    transform_feedback::clear(app);
    app.set_status(app.tr("forms.prototype.changed_during_fit").to_owned(), true);
}
pub(super) fn discard_stale(app: &mut SolveApp) {
    if app.forms_prototype.transform_job.gesture.as_ref().is_some_and(|gesture| !gesture.source.is_current(&app.session)) {
        stale(app);
    }
}

pub(super) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    discard_stale(app);
    poll(app);
    #[cfg(not(target_arch = "wasm32"))]
    dispatch(app, ctx);
    #[cfg(target_arch = "wasm32")]
    let _ = ctx;
}
fn poll(app: &mut SolveApp) {
    let Some(mut gesture) = app.forms_prototype.transform_job.gesture.take() else { return };
    let result = gesture.pending.as_ref().and_then(|pending| match pending.done.try_recv() {
        Ok(result) => Some(result),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err("Form transform computation stopped".into())),
    });
    if let Some(result) = result
        && let Some(pending) = gesture.pending.take()
        && pending.sequence == gesture.sequence
    {
        gesture.settled = Some(pending.sequence);
        match result.and_then(|ready| app.session.accept_form_transform(ready).map_err(|error| error.to_string())) {
            Ok((changed, _)) => {
                command_completed(app, gesture.space.command());
                gesture.authored |= changed;
                let Some(source) = FormTransformStamp::capture(&app.session) else {
                    stale(app);
                    return;
                };
                gesture.source = source;
                let current = TransformSource::current(app);
                if let Some(drag) = gesture.owner.drag_mut(&mut app.forms_prototype) {
                    drag.record_move(current);
                }
            }
            Err(error) => {
                gesture.failed = true;
                transform_feedback::clear(app);
                app.set_status(error, true);
            }
        }
    }
    if gesture.released && gesture.pending.is_none() && gesture.settled == Some(gesture.sequence) {
        transform_feedback::clear(app);
    } else {
        app.forms_prototype.transform_job.gesture = Some(gesture);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn prepare(app: &SolveApp, gesture: &Gesture) -> Result<FormTransformJob, String> {
    let mut params = gesture.parameters.clone();
    params["checkpoint"] = json!(!gesture.authored);
    app.session.prepare_form_transform(gesture.space, params).map_err(|error| error.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn dispatch(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(gesture) = app.forms_prototype.transform_job.gesture.as_ref() else { return };
    if gesture.pending.is_some() || gesture.settled == Some(gesture.sequence) {
        return;
    }
    #[cfg(test)]
    let gate = app.forms_prototype.transform_job.gate.clone();
    #[cfg(not(test))]
    let gate = extrusion::worker_gate();
    let Some(permit) = extrusion::WorkerPermit::acquire(gate) else {
        // An abandoned job may belong to another editor or window.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        return;
    };
    let sequence = gesture.sequence;
    let job = prepare(app, gesture);
    let (tx, done) = std::sync::mpsc::channel();
    let wake = ctx.clone();
    let launched = job.and_then(|job| {
        std::thread::Builder::new()
            .name("form-transform".into())
            .spawn(move || {
                let result = job.compute().map_err(|error| error.to_string());
                drop(permit);
                let _ = tx.send(result);
                wake.request_repaint();
            })
            .map_err(|error| error.to_string())
    });
    if let Some(gesture) = &mut app.forms_prototype.transform_job.gesture {
        match launched {
            Ok(_) => gesture.pending = Some(Pending { sequence, done }),
            Err(error) => {
                gesture.settled = Some(sequence);
                gesture.failed = true;
                transform_feedback::clear(app);
                app.set_status(error, true);
                if released(app) {
                    app.forms_prototype.transform_job.gesture = None;
                }
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn flush_for_test(app: &mut SolveApp, ctx: &egui::Context) -> Result<(), String> {
    for _ in 0..4 {
        tick(app, ctx);
        let Some(pending) = app.forms_prototype.transform_job.gesture.as_mut().and_then(|gesture| gesture.pending.take()) else {
            return Ok(());
        };
        let result = pending.done.recv().map_err(|_| "test transform worker disconnected".to_owned())?;
        let (tx, done) = std::sync::mpsc::channel();
        tx.send(result).map_err(|_| "test transform receiver disconnected".to_owned())?;
        if let Some(gesture) = &mut app.forms_prototype.transform_job.gesture {
            gesture.pending = Some(Pending { done, ..pending });
        }
        poll(app);
        if !waiting(app) {
            return Ok(());
        }
    }
    Err("test transform worker did not settle the newest sample".into())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn hold_for_test(app: &mut SolveApp) -> Result<(FormTransformJob, std::sync::mpsc::Sender<Outcome>), String> {
    let gesture = app.forms_prototype.transform_job.gesture.as_ref().ok_or("missing test transform gesture")?;
    let job = prepare(app, gesture)?;
    let (tx, done) = std::sync::mpsc::channel();
    let sequence = gesture.sequence;
    if let Some(gesture) = &mut app.forms_prototype.transform_job.gesture {
        gesture.pending = Some(Pending { sequence, done });
    }
    Ok((job, tx))
}
