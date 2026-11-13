//! Read-only CAD fit and check feedback in the Form editor.
use super::*;
use crate::ui_kit::BackingTooltipExt as _;
use solidcraft_engine::schema::forms_prototype::{CheckResult, CheckStage, CheckStatus, CheckedShape, FitBasis, ValidationReply};

#[derive(Clone, PartialEq)]
struct Key {
    stamp: Stamp,
    context: u64,
    tolerance: u64,
}
#[cfg(any(test, not(target_arch = "wasm32")))]
type Input = solidcraft_engine::forms_prototype::validation::Input;
type Stamp = solidcraft_engine::forms_prototype::validation::Stamp;
type Outcome = Result<ValidationReply, String>;
#[cfg(target_arch = "wasm32")]
pub type BrowserCheckOutcome = Outcome;

/// The shell owns its dedicated worker across Form/editor/document resets.
#[cfg(target_arch = "wasm32")]
pub trait BrowserCheckHost: super::combine_preview::BrowserPreviewHost {
    fn busy(&self) -> bool;
    fn submit(
        &self,
        request: Vec<u8>,
        cancelled: std::rc::Rc<std::cell::Cell<bool>>,
        finished: std::sync::mpsc::Sender<BrowserCheckOutcome>,
        ctx: &egui::Context,
    ) -> Result<(), String>;
}
#[cfg(not(target_arch = "wasm32"))]
struct CheckPermit {
    active: Arc<std::sync::atomic::AtomicBool>,
}
#[cfg(not(target_arch = "wasm32"))]
impl CheckPermit {
    fn acquire(active: Arc<std::sync::atomic::AtomicBool>) -> Option<Self> {
        use std::sync::atomic::Ordering;
        active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| Self { active })
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for CheckPermit {
    fn drop(&mut self) {
        self.active.store(false, std::sync::atomic::Ordering::Release);
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn worker_gate() -> Arc<std::sync::atomic::AtomicBool> {
    static ACTIVE: std::sync::OnceLock<Arc<std::sync::atomic::AtomicBool>> = std::sync::OnceLock::new();
    ACTIVE.get_or_init(|| Arc::new(std::sync::atomic::AtomicBool::new(false))).clone()
}
fn worker_busy(app: &SolveApp) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = app;
        worker_gate().load(std::sync::atomic::Ordering::Acquire)
    }
    #[cfg(target_arch = "wasm32")]
    {
        app.browser_form_check.as_ref().is_some_and(|host| host.busy())
    }
}
struct Pending {
    key: Key,
    done: std::sync::mpsc::Receiver<Outcome>,
    #[cfg(target_arch = "wasm32")]
    _cancel: Cancel,
}
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
struct Cancel(std::rc::Rc<std::cell::Cell<bool>>);

#[cfg(target_arch = "wasm32")]
impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.set(true);
    }
}
#[derive(Default)]
pub(super) struct State {
    #[cfg(any(test, not(target_arch = "wasm32")))]
    source: Option<(Stamp, Arc<Input>)>,
    #[cfg(not(target_arch = "wasm32"))]
    source_pending: Option<SourcePending>,
    #[cfg(not(target_arch = "wasm32"))]
    source_error: Option<(Stamp, String)>,
    pending: Option<Pending>,
    ready: Option<(Key, Outcome)>,
}
#[cfg(not(target_arch = "wasm32"))]
struct SourcePending {
    stamp: Stamp,
    done: std::sync::mpsc::Receiver<Arc<Input>>,
}
#[cfg(any(test, not(target_arch = "wasm32")))]
fn source(app: &SolveApp) -> Option<&Arc<Input>> {
    let current = Stamp::current(&app.session)?;
    app.forms_prototype.diagnostics.source.as_ref().filter(|(given, _)| *given == current).map(|(_, input)| input)
}
/// Current source identity, including when its immutable input is not ready yet.
pub(super) fn prepared_stamp(app: &SolveApp) -> Option<Stamp> {
    Stamp::current(&app.session)
}
/// Share a producer-qualified read query, independently of GUI fit tolerance.
#[cfg(any(test, not(target_arch = "wasm32")))]
pub(super) fn prepared_input(app: &SolveApp) -> Option<Arc<Input>> {
    source(app).cloned()
}
/// One source-stamp computation for a control-queue polling pass.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn prepared(app: &SolveApp) -> (Option<Stamp>, Option<Arc<Input>>) {
    let current = Stamp::current(&app.session);
    let input = app.forms_prototype.diagnostics.source.as_ref().filter(|(given, _)| current.as_ref() == Some(given)).map(|(_, input)| input.clone());
    (current, input)
}
pub(super) fn preparation_error(app: &SolveApp) -> Option<&str> {
    let current = prepared_stamp(app)?;
    preparation_error_for(app, &current)
}
pub(super) fn preparation_error_for<'a>(app: &'a SolveApp, current: &Stamp) -> Option<&'a str> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.forms_prototype.diagnostics.source_error.as_ref().filter(|(given, _)| given == current).map(|(_, error)| error.as_str())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (app, current);
        None
    }
}
#[cfg(test)]
pub(super) fn install_source(app: &mut SolveApp, input: Arc<Input>) {
    if let Some(current) = prepared_stamp(app) {
        app.forms_prototype.diagnostics.source = Some((current, input));
    }
}
/// Prepare on source changes, not when a button/control request asks for a check.
/// A failed or disabled CAD display still has this independent source producer.
pub(super) fn prepare(app: &mut SolveApp, ctx: &egui::Context) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (app, ctx);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let current = prepared_stamp(app);
        if let Some(pending) = &app.forms_prototype.diagnostics.source_pending {
            let result = match pending.done.try_recv() {
                Ok(input) => Some(Ok(input)),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err(app.tr("forms.diagnostics.stopped").to_owned())),
            };
            if let Some(result) = result {
                let given = pending.stamp.clone();
                app.forms_prototype.diagnostics.source_pending = None;
                if current.as_ref() == Some(&given) {
                    match result {
                        Ok(input) => app.forms_prototype.diagnostics.source = Some((given, input)),
                        Err(error) => app.forms_prototype.diagnostics.source_error = Some((given, error)),
                    }
                }
            }
        }
        let Some(current) = current else { return };
        let state = &app.forms_prototype.diagnostics;
        if state.source_pending.is_some()
            || state.source.as_ref().is_some_and(|(given, _)| *given == current)
            || state.source_error.as_ref().is_some_and(|(given, _)| *given == current)
        {
            return;
        }
        // Prepare once at an edit boundary, not once per sculpt/slider frame.
        // Existing immutable inputs remain usable while their stamp is current.
        if app.forms_prototype.drag.is_some()
            || app.forms_prototype.gizmo_drag.is_some()
            || app.forms_prototype.pending.is_some()
            || app.forms_prototype.interchange.is_some()
            || extrusion::active(app)
            || ctx.input(|input| input.pointer.primary_down())
        {
            return;
        }
        let definition = state
            .source
            .as_ref()
            .filter(|(given, _)| given.same_definition(&current))
            .map(|(_, input)| input.definition())
            .or_else(|| live_cad::selection_geometry(app).map(|exact| exact.definition.clone()));
        let Some(form) = &app.session.forms_prototype else { return };
        let definition = definition.unwrap_or_else(|| Arc::new(form.definition()));
        let context = match solidcraft_engine::forms_prototype::validation::Context::capture(&app.session) {
            Ok(context) => Arc::new(context),
            Err(error) => {
                app.forms_prototype.diagnostics.source_error = Some((current, error.to_string()));
                return;
            }
        };
        let (tx, done) = std::sync::mpsc::channel();
        let wake = ctx.clone();
        match std::thread::Builder::new().name("form-check-source".into()).spawn(move || {
            let _ = tx.send(Arc::new(context.with_source(definition)));
            wake.request_repaint();
        }) {
            Ok(_) => {
                app.forms_prototype.diagnostics.source_error = None;
                app.forms_prototype.diagnostics.source_pending = Some(SourcePending { stamp: current, done });
            }
            Err(error) => app.forms_prototype.diagnostics.source_error = Some((current, error.to_string())),
        }
    }
}
fn key(app: &SolveApp) -> Option<Key> {
    Some(Key { stamp: Stamp::current(&app.session)?, context: context::key(app), tolerance: controls::tolerance(app)?.to_bits() })
}
fn ready(app: &SolveApp) -> Option<&Outcome> {
    let current = key(app)?;
    app.forms_prototype.diagnostics.ready.as_ref().filter(|(given, _)| *given == current).map(|(_, result)| result)
}
pub(super) fn tick(app: &mut SolveApp) {
    let current = key(app);
    if app.forms_prototype.diagnostics.ready.as_ref().is_some_and(|(given, _)| Some(given) != current.as_ref()) {
        app.forms_prototype.diagnostics.ready = None;
    }
    #[cfg(target_arch = "wasm32")]
    if app.forms_prototype.diagnostics.pending.as_ref().is_some_and(|pending| Some(&pending.key) != current.as_ref()) {
        app.forms_prototype.diagnostics.pending = None;
    }
    let Some(pending) = &app.forms_prototype.diagnostics.pending else { return };
    let result = match pending.done.try_recv() {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(_) => Err(app.tr("forms.diagnostics.stopped").to_owned()),
    };
    let given = pending.key.clone();
    app.forms_prototype.diagnostics.pending = None;
    if current.as_ref() == Some(&given) {
        app.forms_prototype.diagnostics.ready = Some((given, result));
    }
}
#[cfg(any(not(target_arch = "wasm32"), test))]
fn compute_value(
    input: Arc<Input>,
    params: &serde_json::Value,
    level: solidcraft_engine::schema::ParamCheck,
) -> solidcraft_engine::Result<serde_json::Value> {
    solidcraft_engine::schema::run_read_query_checked(level, params, "form.prototype.validate", || {
        let args: solidcraft_engine::schema::forms_prototype::Validate =
            serde_json::from_value(params.clone()).map_err(|error| solidcraft_engine::EngineError::Other(error.to_string()))?;
        Ok(json!(input.check(&args)?))
    })
}
#[cfg(any(not(target_arch = "wasm32"), test))]
fn compute(input: Arc<Input>, params: &serde_json::Value, level: solidcraft_engine::schema::ParamCheck) -> Outcome {
    let value = compute_value(input, params, level).map_err(|error| error.to_string())?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}
#[cfg(not(target_arch = "wasm32"))]
fn start(app: &mut SolveApp, ctx: &egui::Context) {
    let _ = start_with_gate(app, ctx, worker_gate(), || {});
}
#[cfg(target_arch = "wasm32")]
fn start(app: &mut SolveApp, ctx: &egui::Context) {
    use solidcraft_engine::forms_prototype::validation::BrowserCheck;
    let Some(key) = key(app) else { return };
    let Some(host) = &app.browser_form_check else { return };
    if app.forms_prototype.diagnostics.pending.is_some() || host.busy() {
        return;
    }
    let tolerance = f64::from_bits(key.tolerance);
    let params = json!({"tolerance":tolerance});
    let cancel = Cancel::default();
    let (finished, done) = std::sync::mpsc::channel();
    let result = authorize(app, &params)
        .and_then(|()| BrowserCheck::capture(&app.session, tolerance)?.encode())
        .map_err(|error| error.to_string())
        .and_then(|request| host.submit(request, cancel.0.clone(), finished, ctx));
    match result {
        Ok(()) => {
            app.forms_prototype.diagnostics.ready = None;
            app.forms_prototype.diagnostics.pending = Some(Pending { key, done, _cancel: cancel });
        }
        Err(error) => app.forms_prototype.diagnostics.ready = Some((key, Err(error))),
    }
}

fn authorize(app: &SolveApp, params: &serde_json::Value) -> solidcraft_engine::Result<()> {
    solidcraft_engine::find_command("form.prototype.validate")
        .ok_or_else(|| solidcraft_engine::EngineError::UnknownCommand("form.prototype.validate".into()))
        .and_then(|spec| app.session.check_command(spec, params))
}
#[cfg(not(target_arch = "wasm32"))]
fn start_with_gate(
    app: &mut SolveApp,
    ctx: &egui::Context,
    gate: Arc<std::sync::atomic::AtomicBool>,
    before_compute: impl FnOnce() + Send + 'static,
) -> Option<std::thread::JoinHandle<()>> {
    if app.forms_prototype.diagnostics.pending.is_some() {
        return None;
    }
    let key = key(app)?;
    let input = prepared_input(app)?;
    let permit = CheckPermit::acquire(gate)?;
    let params = json!({"tolerance":f64::from_bits(key.tolerance)});
    let level = app.session.param_check;
    let scope = solidcraft_engine::io::vfs::current_scope();
    let allowed = authorize(app, &params);
    if let Err(error) = allowed {
        app.forms_prototype.diagnostics.ready = Some((key, Err(error.to_string())));
        return None;
    }
    let wake = ctx.clone();
    let (tx, done) = std::sync::mpsc::channel();
    match std::thread::Builder::new().name("form-cad-check".into()).spawn(move || {
        // The worker owns the permit even if closing/replacing the editor drops
        // its receiver. Unwinding or a failed send releases it through Drop.
        let _permit = permit;
        before_compute();
        let outcome = solidcraft_engine::io::vfs::scoped(scope, || compute(input, &params, level));
        let _ = tx.send(outcome);
        wake.request_repaint();
    }) {
        Ok(thread) => {
            app.forms_prototype.diagnostics.ready = None;
            app.forms_prototype.diagnostics.pending = Some(Pending { key, done });
            Some(thread)
        }
        Err(error) => {
            app.forms_prototype.diagnostics.ready = Some((key, Err(error.to_string())));
            None
        }
    }
}
fn unavailable(app: &SolveApp) -> Option<&'static str> {
    #[cfg(target_arch = "wasm32")]
    if app.browser_form_check.is_none() {
        return Some("forms.diagnostics.desktop_required");
    }
    if app.forms_prototype.diagnostics.pending.is_some() {
        return Some("forms.diagnostics.checking");
    }
    if worker_busy(app) {
        return Some("forms.diagnostics.worker_busy");
    }
    if controls::tolerance(app).is_none() {
        return Some("forms.prototype.invalid_tolerance");
    }
    if app.forms_prototype.pending.is_some()
        || app.forms_prototype.interchange.is_some()
        || extrusion::active(app)
        || app.forms_prototype.drag.is_some()
        || app.forms_prototype.gizmo_drag.is_some()
    {
        return Some("forms.diagnostics.wait_for_edit");
    }
    let Some(form) = app.session.forms_prototype.as_ref() else { return Some("forms.prototype.active_form_required") };
    if form.repair.is_some() || form.has_tool_preview() {
        return Some("forms.diagnostics.finish_tool");
    }
    #[cfg(not(target_arch = "wasm32"))]
    if source(app).is_none() {
        return Some("forms.diagnostics.fit_pending");
    }
    #[cfg(target_arch = "wasm32")]
    if !solidcraft_engine::forms_prototype::validation::BrowserCheck::available(&app.session) {
        return Some("forms.diagnostics.browser_source_required");
    }
    None
}
fn number(value: f64) -> String {
    if value != 0.0 && value.abs() < 0.001 { format!("{value:.3e}") } else { format!("{value:.4}") }
}
fn fit_line(app: &SolveApp) -> String {
    if let (Some(report), Some(tolerance)) = (live_cad::report(app), controls::tolerance(app)) {
        let profile = app.session.forms_prototype.as_ref().is_some_and(|form| form.profile.is_some());
        let kind = app.tr(if profile {
            "forms.diagnostics.surface_patch"
        } else if report.solid {
            "forms.diagnostics.solid"
        } else {
            "forms.diagnostics.surface"
        });
        return solidcraft_ui_text::format(
            "forms.diagnostics.fit_line",
            &[("kind", kind), ("deviation", &number(report.sampled_deviation_mm)), ("tolerance", &number(tolerance))],
        );
    }
    app.tr(if live_cad::busy(app) { "forms.diagnostics.fit_pending" } else { "forms.diagnostics.fit_unavailable" }).to_owned()
}
fn errors(app: &SolveApp) -> Vec<(&'static str, String)> {
    let mut errors = Vec::new();
    if let Some(message) = preparation_error(app) {
        errors.push(("forms.diagnostics.action_hint", message.to_owned()));
    }
    if let Some((message, _, true)) = &app.status {
        errors.push(("forms.diagnostics.action_hint", message.clone()));
    }
    if let Some(message) = &app.forms_prototype.live.error {
        errors.push(("forms.diagnostics.fit_hint", message.clone()));
    }
    if let Some(message) = &app.forms_prototype.context.error {
        errors.push(("forms.diagnostics.context_hint", message.clone()));
    }
    if let Some(result) = ready(app) {
        match result {
            Err(message) => errors.push(("forms.diagnostics.action_hint", message.clone())),
            Ok(report) => {
                let hint = if report.stage == CheckStage::Source { "forms.diagnostics.context_hint" } else { "forms.diagnostics.fit_hint" };
                if let Some(message) = &report.conversion.message {
                    errors.push((hint, message.clone()));
                }
                for check in [&report.record, &report.smlib] {
                    if check.status == CheckStatus::Failed
                        && let Some(message) = &check.message
                    {
                        errors.push(("forms.diagnostics.check_hint", message.clone()));
                    }
                }
            }
        }
    }
    let mut unique = Vec::new();
    for error in errors {
        if !unique.iter().any(|(_, message)| message == &error.1) {
            unique.push(error);
        }
    }
    unique
}
pub(super) fn show_status(app: &SolveApp, ui: &mut egui::Ui) {
    let fit = ui.add(egui::Label::new(fit_line(app)).wrap());
    crate::scenario::publish_target("forms_fit_status", fit.rect);
    let notices = errors(app);
    if let Some((hint, error)) = notices.first() {
        ui.add(egui::Label::new(egui::RichText::new(error).color(Tokens::get().error)).truncate()).backing_on_hover_text(error);
        ui.add(egui::Label::new(app.tr(hint)).wrap());
    }
}
fn show_check_button(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let reason = unavailable(app);
        let response = crate::ui_kit::button(ui, app.tr("cmd.form.prototype.validate"), crate::ui_kit::Button::Enabled(reason.is_none()));
        let response = if let Some(reason) = reason { response.backing_on_disabled_hover_text(app.tr(reason)) } else { response };
        crate::scenario::publish_handle("forms_check_cad", response.rect.center());
        if response.clicked() {
            start(app, ui.ctx());
        }
        if app.forms_prototype.diagnostics.pending.is_some() {
            ui.spinner();
            ui.label(app.tr("forms.diagnostics.checking"));
        }
    });
    if app.forms_prototype.diagnostics.pending.is_none() && worker_busy(app) {
        ui.add(egui::Label::new(app.tr("forms.diagnostics.worker_busy")).wrap());
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    if let Some(Ok(report)) = ready(app) {
        let complete = summary(report) == "forms.diagnostics.completed";
        let color = if complete { Tokens::get().accent } else { Tokens::get().warning };
        ui.add(egui::Label::new(egui::RichText::new(app.tr(summary(report))).color(color)).wrap());
    }
}
pub(super) fn show_legacy_details(app: &mut SolveApp, ui: &mut egui::Ui) {
    show_check_button(app, ui);
    let title = app.tr("forms.diagnostics.details").to_owned();
    crate::ui_kit::editor_section(ui, "forms_cad_diagnostics", &title, false, |ui| show_details(app, ui));
}
pub(super) fn show_check(app: &mut SolveApp, ui: &mut egui::Ui) {
    show_check_button(app, ui);
    show_details(app, ui);
}
fn show_details(app: &SolveApp, ui: &mut egui::Ui) {
    ui.add(egui::Label::new(app.tr("forms.diagnostics.sample_limit")).wrap());
    for (_, error) in errors(app) {
        ui.add(egui::Label::new(error).wrap());
    }
    if let Some(Ok(report)) = ready(app) {
        details(app, ui, report);
    }
}
fn summary(report: &ValidationReply) -> &'static str {
    if [&report.conversion, &report.record, &report.smlib].iter().any(|check| check.status == CheckStatus::Failed)
        || report.fit.as_ref().is_some_and(|fit| {
            !fit.sampled_deviation_mm.is_finite() || fit.sampled_deviation_mm < 0.0 || fit.sampled_deviation_mm > report.tolerance_mm
        })
    {
        "forms.diagnostics.attention"
    } else if report.stage != CheckStage::Complete
        || report.shape.is_none()
        || report.fit.is_none()
        || [&report.conversion, &report.record, &report.smlib].iter().any(|check| check.status != CheckStatus::Passed)
    {
        "forms.diagnostics.partial"
    } else {
        "forms.diagnostics.completed"
    }
}
pub(super) fn inspect(app: &SolveApp) -> serde_json::Value {
    let current = ready(app);
    json!({"pending":app.forms_prototype.diagnostics.pending.is_some(),
        "report":current.and_then(|result|result.as_ref().ok()),"error":current.and_then(|result|result.as_ref().err())})
}
fn check_row(app: &SolveApp, ui: &mut egui::Ui, label: &str, check: &CheckResult) {
    let status = app.tr(match check.status {
        CheckStatus::Passed => "forms.diagnostics.passed",
        CheckStatus::Failed => "forms.diagnostics.failed",
        CheckStatus::NotSupported => "forms.diagnostics.not_supported",
        CheckStatus::NotRun => "forms.diagnostics.not_run",
    });
    ui.label(solidcraft_ui_text::format("forms.diagnostics.check_line", &[("check", app.tr(label)), ("status", status)]));
    if let Some(message) = &check.message {
        ui.add(egui::Label::new(message).wrap());
    }
}
fn details(app: &SolveApp, ui: &mut egui::Ui, report: &ValidationReply) {
    ui.add(egui::Label::new(app.tr("forms.diagnostics.scope")).wrap());
    if report.staged_forms_not_checked > 0 {
        ui.label(solidcraft_ui_text::format("forms.diagnostics.staged", &[("count", &report.staged_forms_not_checked.to_string())]));
    }
    if let Some(shape) = report.shape {
        ui.label(solidcraft_ui_text::format(
            "forms.diagnostics.shape_checked",
            &[(
                "kind",
                app.tr(match shape {
                    CheckedShape::Solid => "forms.diagnostics.solid",
                    CheckedShape::Surface => "forms.diagnostics.surface",
                }),
            )],
        ));
    }
    if let Some(fit) = &report.fit {
        let basis = app.tr(match fit.basis {
            FitBasis::SubdivisionSamples => "forms.diagnostics.subdivision_samples",
            FitBasis::BoundaryAndGuideSamples => "forms.diagnostics.guide_samples",
        });
        ui.label(solidcraft_ui_text::format(
            "forms.diagnostics.measured",
            &[("basis", basis), ("deviation", &number(fit.sampled_deviation_mm)), ("tolerance", &number(report.tolerance_mm))],
        ));
        ui.label(solidcraft_ui_text::format("forms.diagnostics.patches", &[("count", &fit.patches.to_string())]));
    }
    check_row(app, ui, "forms.diagnostics.conversion", &report.conversion);
    check_row(app, ui, "forms.diagnostics.record", &report.record);
    check_row(app, ui, "forms.diagnostics.smlib", &report.smlib);
}

#[cfg(test)]
pub(super) fn inject_for_test(app: &mut SolveApp, outcome: Outcome) {
    let key = key(app).unwrap();
    let (tx, done) = std::sync::mpsc::channel();
    tx.send(outcome).unwrap();
    app.forms_prototype.diagnostics.pending = Some(Pending {
        key,
        done,
        #[cfg(target_arch = "wasm32")]
        _cancel: Cancel::default(),
    });
}
#[cfg(test)]
pub(super) fn compute_for_test(app: &SolveApp) -> Outcome {
    let input = Arc::new(Input::capture(&app.session).map_err(|error| error.to_string())?);
    let tolerance = controls::tolerance(app).ok_or("invalid diagnostic test tolerance")?;
    compute(input, &json!({"tolerance":tolerance}), app.session.param_check)
}
#[cfg(test)]
pub(super) fn tick_for_test(app: &mut SolveApp) {
    tick(app);
}
#[cfg(test)]
pub(super) fn current_for_test(app: &SolveApp) -> Option<&Outcome> {
    ready(app)
}
#[cfg(test)]
pub(super) fn summary_for_test(report: &ValidationReply) -> &'static str {
    summary(report)
}
#[cfg(test)]
pub(super) fn source_for_test(app: &SolveApp) -> Option<Arc<Input>> {
    prepared_input(app)
}
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn flush_source_for_test(app: &mut SolveApp, ctx: &egui::Context) -> Result<(), String> {
    let pending = app.forms_prototype.diagnostics.source_pending.take().ok_or("no diagnostic source preparation is pending")?;
    let input = pending.done.recv().map_err(|error| error.to_string())?;
    let (tx, done) = std::sync::mpsc::channel();
    tx.send(input).map_err(|error| error.to_string())?;
    app.forms_prototype.diagnostics.source_pending = Some(SourcePending { stamp: pending.stamp, done });
    prepare(app, ctx);
    Ok(())
}
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn start_for_test(app: &mut SolveApp, ctx: &egui::Context) {
    let _ = start_with_gate(app, ctx, Arc::new(std::sync::atomic::AtomicBool::new(false)), || {});
}
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn start_controlled_for_test(
    app: &mut SolveApp,
    ctx: &egui::Context,
    gate: Arc<std::sync::atomic::AtomicBool>,
    before_compute: impl FnOnce() + Send + 'static,
) -> Option<std::thread::JoinHandle<()>> {
    start_with_gate(app, ctx, gate, before_compute)
}
#[cfg(test)]
pub(super) fn compute_value_for_test(
    input: Arc<Input>,
    params: &serde_json::Value,
    level: solidcraft_engine::schema::ParamCheck,
) -> solidcraft_engine::Result<serde_json::Value> {
    compute_value(input, params, level)
}
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn flush_check_for_test(app: &mut SolveApp) -> Result<(), String> {
    let pending = app.forms_prototype.diagnostics.pending.take().ok_or("no diagnostic check is pending")?;
    let outcome = pending.done.recv().map_err(|error| error.to_string())?;
    let (tx, done) = std::sync::mpsc::channel();
    tx.send(outcome).map_err(|error| error.to_string())?;
    app.forms_prototype.diagnostics.pending = Some(Pending { key: pending.key, done });
    tick(app);
    Ok(())
}
