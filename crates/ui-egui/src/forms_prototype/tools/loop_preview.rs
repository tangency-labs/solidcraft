//! A hovered loop uses the same qualified transaction that a click accepts.
use super::*;

type Curves = Vec<Vec<Vec3>>;
type Command = Result<solidcraft_engine::ComputedCommands, String>;
type Geometry = Result<Curves, String>;

pub(super) fn unavailable(app: &SolveApp) -> Option<&'static str> {
    app.session.forms_prototype.as_ref()?.hierarchy.as_ref()?.has_trim_wall().then_some("forms.insert.whole_loop_trim_wall")
}

#[derive(Clone, PartialEq)]
struct Source {
    document: u64,
    document_id: String,
    authoring: u64,
    form: (u64, u64),
    options: solidcraft_engine::schema::forms_prototype::FinishOptions,
    tool_preview: bool,
    context: u64,
    tolerance: u64,
}
fn source(app: &SolveApp) -> Option<Source> {
    let form = app.session.forms_prototype.as_ref()?;
    if form.repair.is_some() || form.profile.is_some() || unavailable(app).is_some() {
        return None;
    }
    Some(Source {
        document: app.session.uid,
        document_id: app.session.document_id.clone(),
        authoring: app.session.authoring_revision(),
        form: form.source_revision(),
        options: form.finish_options.clone(),
        tool_preview: form.has_tool_preview(),
        context: context::key(app),
        tolerance: controls::tolerance(app)?.to_bits(),
    })
}
#[derive(Clone, PartialEq)]
struct Key {
    source: Source,
    revision: u64,
    face: usize,
    axis: usize,
    edge: Option<[usize; 2]>,
}
impl Key {
    fn parameters(&self) -> serde_json::Value {
        if let Some(edge) = self.edge {
            json!({"edge":edge})
        } else {
            json!({"face":self.face,"axis":self.axis,"tolerance":f64::from_bits(self.source.tolerance)})
        }
    }
}
struct Pending {
    key: Key,
    geometry: std::sync::mpsc::Receiver<Geometry>,
    command: std::sync::mpsc::Receiver<Command>,
    finished: bool,
}
#[derive(Default)]
pub(super) struct State {
    desired: Option<Key>,
    pending: Option<Pending>,
    ready: Option<(Key, Geometry)>,
    waiting: bool,
    #[cfg(all(test, not(target_arch = "wasm32")))]
    worker_gate: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(all(not(test), not(target_arch = "wasm32")))]
fn worker_gate() -> Arc<std::sync::atomic::AtomicBool> {
    static ACTIVE: std::sync::OnceLock<Arc<std::sync::atomic::AtomicBool>> = std::sync::OnceLock::new();
    ACTIVE.get_or_init(Default::default).clone()
}

fn key(app: &SolveApp, face: usize, axis: usize) -> Option<Key> {
    let form = app.session.forms_prototype.as_ref()?;
    let polygon = form.logical.faces.get(face)?;
    if axis > 1 || polygon.is_empty() {
        return None;
    }
    // A refined face's polygon may contain T-controls. Its parameter side is
    // resolved by the command, while a base quad names the seed edge directly.
    let edge = if form.hierarchy.is_none() {
        if polygon.len() != 4 {
            return None;
        }
        Some([*polygon.get(axis)?, *polygon.get(axis + 1)?])
    } else {
        None
    };
    Some(Key { source: source(app)?, revision: form.revision, face, axis, edge })
}
fn current(app: &SolveApp, key: &Key) -> bool {
    source(app).as_ref() == Some(&key.source)
}
pub(super) fn cancel(app: &mut SolveApp) {
    // Keep an unfinished worker until it drains: rapidly toggling tools must
    // not create an unbounded number of detached CAD computations.
    app.forms_prototype.tools.loop_preview.desired = None;
    app.forms_prototype.tools.loop_preview.ready = None;
    app.forms_prototype.tools.loop_preview.waiting = false;
    if app.forms_prototype.tools.loop_preview.pending.as_ref().is_some_and(|pending| pending.finished) {
        app.forms_prototype.tools.loop_preview.pending = None;
    }
    app.forms_prototype.tools.preview = None;
}
pub(super) fn poll(app: &mut SolveApp) {
    let result = {
        let Some(pending) = &app.forms_prototype.tools.loop_preview.pending else { return };
        if pending.finished {
            return;
        }
        match pending.geometry.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(app.tr("forms.prototype.preview_stopped").to_owned())),
        }
    };
    let Some(result) = result else { return };
    let Some(pending) = &mut app.forms_prototype.tools.loop_preview.pending else { return };
    pending.finished = true;
    let key = pending.key.clone();
    if app.forms_prototype.tools.loop_preview.desired.is_none() {
        app.forms_prototype.tools.loop_preview.pending = None;
        app.forms_prototype.tools.loop_preview.ready = None;
        return;
    }
    if current(app, &key) {
        app.forms_prototype.tools.loop_preview.ready = Some((key, result));
    }
}
pub(super) fn show_status(app: &SolveApp, ui: &mut egui::Ui) {
    let state = &app.forms_prototype.tools.loop_preview;
    let Some(desired) = &state.desired else { return };
    if app.forms_prototype.tools.mode != Mode::Edge
        || !app.forms_prototype.tools.whole_loop
        || app.forms_prototype.tools.axis != desired.axis
        || !current(app, desired)
    {
        return;
    }
    if state.waiting || state.pending.as_ref().is_some_and(|pending| pending.key == *desired && !pending.finished) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(app.tr("forms.prototype.previewing"));
        });
    }
    if let Some((given, Err(error))) = &state.ready
        && given == desired
    {
        ui.add(egui::Label::new(egui::RichText::new(error).color(Tokens::get().warning)).wrap());
    }
}
pub(super) fn hover(app: &mut SolveApp, ctx: &egui::Context, face: usize, axis: usize) {
    poll(app);
    let Some(desired) = key(app, face, axis) else {
        cancel(app);
        return;
    };
    let state = &mut app.forms_prototype.tools.loop_preview;
    let changed = state.desired.as_ref() != Some(&desired);
    state.desired = Some(desired.clone());
    if state.pending.as_ref().is_some_and(|pending| pending.finished && pending.key != desired) {
        state.pending = None;
        state.ready = None;
    }
    let display_key = (desired.revision, face, axis, true);
    if changed || app.forms_prototype.tools.preview.as_ref().is_some_and(|p| (p.0, p.1, p.2, p.3) != display_key) {
        app.forms_prototype.tools.preview = None;
    }
    if let Some((given, result)) = &app.forms_prototype.tools.loop_preview.ready
        && given == &desired
    {
        if app.forms_prototype.tools.preview.is_none() {
            let geometry = result.as_ref().cloned().unwrap_or_default();
            app.forms_prototype.tools.preview = Some((desired.revision, face, axis, true, geometry));
        }
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if app.forms_prototype.tools.loop_preview.pending.is_none() {
        start(app, ctx, desired);
    }
    // Until a browser worker host exists, fitting is explicit-click work there.
    let _ = ctx;
    if app.forms_prototype.tools.loop_preview.pending.as_ref().is_some_and(|p| !p.finished) {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

#[cfg(any(not(target_arch = "wasm32"), test))]
fn compute(copy: solidcraft_engine::Session, parameters: serde_json::Value, stopped: &str) -> (Geometry, Command) {
    let prepared = copy.compute_commands(&[("form.prototype.insert_loop".into(), parameters)], None);
    match prepared {
        Ok(prepared) => {
            let geometry = (|| -> Geometry {
                let seams = prepared.form_loop_seams().map_err(|error| error.to_string())?;
                let expected: BTreeSet<_> = seams.iter().copied().collect();
                let form = prepared.form_preview().ok_or_else(|| stopped.to_owned())?;
                let mut curves = std::collections::BTreeMap::new();
                for (edge, path) in &form.surface.boundaries {
                    let [a, b] = *edge;
                    let edge = [a.min(b), a.max(b)];
                    if expected.contains(&edge)
                        && (path.len() < 2 || path.iter().any(|point| !point.is_finite()) || curves.insert(edge, path.clone()).is_some())
                    {
                        return Err(stopped.to_owned());
                    }
                }
                seams.into_iter().map(|edge| curves.remove(&edge).ok_or_else(|| stopped.to_owned())).collect()
            })();
            match geometry {
                Ok(curves) => (Ok(curves), Ok(prepared)),
                Err(error) => (Err(error.clone()), Err(error)),
            }
        }
        Err(error) => {
            let message = error.to_string();
            (Err(message.clone()), Err(message))
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn start(app: &mut SolveApp, ctx: &egui::Context, key: Key) {
    #[cfg(test)]
    let gate = app.forms_prototype.tools.loop_preview.worker_gate.clone();
    #[cfg(not(test))]
    let gate = worker_gate();
    let Some(permit) = finish::WorkerPermit::acquire(gate) else {
        app.forms_prototype.tools.loop_preview.waiting = true;
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        return;
    };
    app.forms_prototype.tools.loop_preview.waiting = false;
    let copy = app.session.scratch();
    let stopped = app.tr("forms.prototype.preview_stopped").to_owned();
    let parameters = key.parameters();
    let (geometry_tx, geometry) = std::sync::mpsc::channel();
    let (command_tx, command) = std::sync::mpsc::channel();
    let wake = ctx.clone();
    match std::thread::Builder::new().name("form-loop-preview".into()).spawn(move || {
        let _permit = permit;
        let (curves, prepared) = compute(copy, parameters, &stopped);
        let _ = geometry_tx.send(curves);
        let _ = command_tx.send(prepared);
        wake.request_repaint();
    }) {
        Ok(_) => app.forms_prototype.tools.loop_preview.pending = Some(Pending { key, geometry, command, finished: false }),
        Err(error) => app.forms_prototype.tools.loop_preview.ready = Some((key, Err(error.to_string()))),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn occupy_worker_for_test(app: &SolveApp) -> Option<finish::WorkerPermit> {
    finish::WorkerPermit::acquire(app.forms_prototype.tools.loop_preview.worker_gate.clone())
}
pub(super) fn commit(app: &mut SolveApp, ctx: &egui::Context, face: usize, axis: usize) -> bool {
    if let Some(reason) = unavailable(app) {
        cancel(app);
        app.set_status(app.tr(reason).to_owned(), true);
        return true;
    }
    let Some(desired) = key(app, face, axis) else { return false };
    if app.forms_prototype.pending.is_some() {
        return true;
    }
    if app.forms_prototype.tools.loop_preview.pending.as_ref().is_some_and(|pending| pending.key == desired) {
        if let Some(pending) = app.forms_prototype.tools.loop_preview.pending.take() {
            finish::await_command(app, "form.prototype.insert_loop", pending.command);
        }
    } else {
        // A cold click on a new seed uses the existing command worker, while
        // at most one obsolete hover computation drains independently.
        finish::command(app, ctx, "form.prototype.insert_loop", desired.parameters());
    }
    cancel(app);
    true
}

#[cfg(test)]
pub(super) fn install_for_test(app: &mut SolveApp, face: usize, axis: usize) -> (Geometry, solidcraft_engine::doc::forms::Definition) {
    let desired = key(app, face, axis).unwrap();
    let result = compute(app.session.scratch(), desired.parameters(), "preview stopped");
    let (geometry_tx, geometry) = std::sync::mpsc::channel();
    let (command_tx, command) = std::sync::mpsc::channel();
    let expected = result.1.as_ref().unwrap().form_preview().unwrap().definition();
    let given_geometry = result.0.clone();
    geometry_tx.send(result.0).unwrap();
    command_tx.send(result.1).unwrap();
    app.forms_prototype.tools.loop_preview.desired = Some(desired.clone());
    app.forms_prototype.tools.loop_preview.pending = Some(Pending { key: desired, geometry, command, finished: false });
    (given_geometry, expected)
}

#[cfg(test)]
pub(super) fn has_ready_for_test(app: &SolveApp) -> bool {
    app.forms_prototype.tools.loop_preview.ready.is_some()
}

#[cfg(test)]
pub(super) fn has_pending_for_test(app: &SolveApp) -> bool {
    app.forms_prototype.tools.loop_preview.pending.is_some()
}
