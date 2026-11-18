//! Report a problem: the in-app report that files a private ticket on tickets.solidcraft.io.
//! Submit sends the report as a draft, uploads the ticked attachments and opens the draft's
//! page in the browser, where the person signs in if needed and files it. Each attachment is a
//! box with what it holds said in plain words and shown before it goes; the design is never
//! sent unless its box is ticked.

use std::sync::Arc;

use egui::{ColorImage, Id, Ui, vec2};
use serde_json::{Value, json};
use solidcraft_ui_text::{format, tr};

use super::Draft;
use crate::tickets::{self, Outcome, api, payload};
use crate::{SolveApp, ui_kit};

/// How many lines of the log go with a report, and at most how many bytes of them.
pub const LOG_LINES: usize = 500;
pub const LOG_BYTES: u64 = 128 << 10;
/// Commands a report keeps as its recent steps (`repro.steps`).
pub const MAX_STEPS: usize = 50;
/// A recent step's parameters longer than this (as JSON) are left out.
const MAX_STEP_PARAMS: usize = 2048;
/// An unsent design left for recovery is attached in place of a new, empty one only when it was
/// autosaved this recently (seconds).
const RECOVERY_FRESH_SECS: u64 = 24 * 3600;

/// What the report form holds besides the shared draft.
pub struct TicketDraft {
    pub form: payload::Form,
    pub ticks: payload::Ticks,
    pub evidence: payload::Evidence,
    /// The report is about a failed operation (its Report… or Create Issue button).
    pub from_failure: bool,
    /// The agent report this dialog is for (its id), when an agent opened it.
    pub agent: Option<String>,
    /// This report, among the reports sent in this run: what is being sent, and how it went, is
    /// this dialog's only when it is this report's (`tickets::Tickets::owner`).
    pub id: u64,
}

/// Whether this build can file reports on the ticket server (the GitHub draft otherwise).
pub fn enabled(app: &SolveApp) -> bool {
    app.services.tickets_http.is_some() && !app.preferences.report_on_github
}

/// The build and machine: what "System info" sends, as fields and as the exact text shown.
/// Nothing about the design.
pub fn system(app: &SolveApp, ctx: Option<&egui::Context>) -> (serde_json::Map<String, Value>, String) {
    use crate::help::{BUILD_DATE, COMMIT, VERSION};
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let renderer = if app.viewport.gpu.is_some() { "GPU" } else { "CPU" };
    let gpu = app.help.gpu.clone().unwrap_or_default();
    let mut f = serde_json::Map::new();
    f.insert("app_version".into(), json!(VERSION));
    f.insert("commit".into(), json!(COMMIT));
    f.insert("built".into(), json!(BUILD_DATE));
    f.insert("target".into(), json!(format!("{arch}-{os}")));
    f.insert("os".into(), json!(os));
    f.insert("arch".into(), json!(arch));
    f.insert("renderer".into(), json!(renderer));
    if !gpu.is_empty() {
        f.insert("gpu".into(), json!(gpu));
    }
    let mut s = format!("SolidCraft {VERSION} ({COMMIT}, built {BUILD_DATE})\nTarget: {arch}-{os}\nRenderer: {renderer}");
    if !gpu.is_empty() {
        s.push_str(&format!(", {gpu}"));
    }
    s.push('\n');
    if let Some(api) = app.services.graphics_backend {
        f.insert("graphics_api".into(), json!(api));
        s.push_str(&format!("Graphics API: {api}\n"));
    }
    let adapters: Vec<String> = app.services.graphics_adapters.iter().take(4).map(|(n, l)| format!("{n} ({l})")).collect();
    if !adapters.is_empty() {
        s.push_str(&format!("Adapters: {}\n", adapters.join("; ")));
        f.insert("adapters".into(), json!(adapters));
    }
    if let Some(ws) = app.services.window_system {
        f.insert("window_system".into(), json!(ws));
        s.push_str(&format!("Window system: {ws}\n"));
    }
    if let Some(ctx) = ctx {
        let r = ctx.content_rect();
        let w = format!("{:.0}×{:.0} points at {:.2} pixels per point", r.width(), r.height(), ctx.pixels_per_point());
        s.push_str(&format!("Window: {w}\n"));
        f.insert("window".into(), json!(w));
    }
    let locale = solidcraft_ui_text::language();
    f.insert("locale".into(), json!(locale));
    (f, s)
}

/// `s` as a report carries it: at most `limit` bytes, the home folder as `~`.
fn clean(s: &str, home: Option<&str>, limit: usize) -> String {
    let s = super::limited(s, limit);
    match home {
        Some(h) => s.replace(h, "~"),
        None => s,
    }
}

/// The last lines of the app's log on disk (`log_file`), home folder as `~`. Without one (the
/// web build) the design's command log and the Messages.
pub fn log_lines(app: &SolveApp) -> Vec<String> {
    let home = super::home_text();
    let file = crate::log_file::tail(LOG_LINES, LOG_BYTES);
    if !file.is_empty() {
        return file.iter().map(|l| clean(l, home.as_deref(), 2000)).collect();
    }
    let clean = |s: &str| clean(s, home.as_deref(), 2000);
    let mut lines: Vec<String> = app.session.log.iter().flat_map(|l| l.lines().map(clean).collect::<Vec<_>>()).collect();
    for m in app.messages.iter() {
        let level = if m.error { "error" } else { "info" };
        lines.push(format!("[{level}] {}", clean(&m.text)));
    }
    let n = lines.len();
    lines.drain(..n.saturating_sub(LOG_LINES));
    lines
}

/// The design's file name as it goes with a report.
fn design_name(app: &SolveApp) -> String {
    app.session
        .path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()))
        .filter(|n| n.ends_with(".sc"))
        .unwrap_or_else(|| {
            let n: String =
                app.session.doc.name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).take(80).collect();
            format!("{}.sc", if n.trim().is_empty() { "design" } else { n.trim() })
        })
}

/// Commands whose parameters name files or folders: a step keeps only the file's name.
fn names_files(id: &str) -> bool {
    ["file.", "doc.", "plugin.", "cloud.", "library."].iter().any(|p| id.starts_with(p))
}

/// Every string in `v` with the home folder as `~`.
fn scrub(v: &mut Value, home: &str) {
    match v {
        Value::String(s) => *s = s.replace(home, "~"),
        Value::Array(a) => a.iter_mut().for_each(|v| scrub(v, home)),
        Value::Object(o) => o.values_mut().for_each(|v| scrub(v, home)),
        _ => {}
    }
}

/// One recent step as a report carries it: the command, whether it worked and why not, and its
/// parameters, unless they are large or name files (then only the file's name).
pub fn step(id: &str, params: Option<&Value>, error: Option<&str>, home: Option<&str>) -> Value {
    let mut s = json!({ "command": super::limited(id, 200), "ok": error.is_none() });
    if let Some(e) = error {
        s["error"] = json!(clean(e, home, 2000));
    }
    let Some(p) = params.filter(|p| !p.is_null() && p.as_object().is_none_or(|o| !o.is_empty())) else { return s };
    if names_files(id) {
        if let Some(path) = p.get("path").and_then(Value::as_str) {
            let name = path.rsplit(['/', '\\']).next().unwrap_or_default();
            s["file"] = json!(super::limited(name, 200));
        }
        if id == "file.import" {
            // Preserve the importer slot and option names without leaking option values.
            // Plugin options can contain local paths or private configuration (#18760).
            if let Some(importer) = p.get("format").and_then(Value::as_str) {
                s["importer"] = json!(importer.chars().filter(|c| !c.is_control()).take(120).collect::<String>());
            }
            if let Some(options) = p.get("options").and_then(Value::as_object) {
                s["option_keys"] = json!(
                    options.keys().take(32).map(|key| key.chars().filter(|c| !c.is_control()).take(100).collect::<String>()).collect::<Vec<_>>()
                );
            }
        }
        return s;
    }
    if serde_json::to_string(p).map_or(usize::MAX, |t| t.len()) > MAX_STEP_PARAMS {
        s["params_omitted"] = json!(true);
        return s;
    }
    let mut p = p.clone();
    if let Some(h) = home {
        scrub(&mut p, h);
    }
    s["params"] = p;
    s
}

/// A command ran with the result `r`: it is one of the steps a report lists (`repro.steps`),
/// opens, saves and selections included, and a line of the log. A gesture's frames are not.
pub(crate) fn record_step(app: &mut SolveApp, id: &str, r: &Result<Value, String>) {
    if r.as_ref().is_ok_and(|v| v.get("continuous").and_then(Value::as_bool) == Some(true)) {
        return;
    }
    match r {
        Ok(_) => log::info!("command {id}"),
        Err(e) => log::warn!("command {id} failed: {e}"),
    }
    let home = super::home_text();
    let steps = &mut app.issue_report.steps;
    steps.push_back(step(id, None, r.as_ref().err().map(String::as_str), home.as_deref()));
    while steps.len() > MAX_STEPS {
        steps.pop_front();
    }
}

/// The parameters of the step just recorded ([`record_step`]), when it was command `id`.
pub(crate) fn step_params(app: &mut SolveApp, id: &str, params: &Value) {
    let id = solidcraft_engine::find_command(id).map_or(id, |spec| spec.id);
    let Some(last) = app.issue_report.steps.back_mut() else { return };
    if last.get("command").and_then(Value::as_str) != Some(id) || last.get("params").is_some() || last.get("file").is_some() {
        return;
    }
    let error = last.get("error").and_then(Value::as_str).map(str::to_string);
    *last = step(id, Some(params), error.as_deref(), super::home_text().as_deref());
}

/// The steps a crash report recorded before the crash (`recent_commands`), as a report carries them.
fn crash_steps(report: &Value) -> Vec<Value> {
    let home = super::home_text();
    report
        .get("recent_commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_STEPS)
        .filter_map(|c| {
            let id = c.get("command")?.as_str()?;
            let failed = c.get("ok").and_then(Value::as_bool) == Some(false);
            let error = failed.then(|| c.get("error").and_then(Value::as_str).unwrap_or("failed"));
            Some(step(id, c.get("params"), error, home.as_deref()))
        })
        .collect()
}

/// A file of at most `max` bytes, or nothing.
fn read_bounded(path: &std::path::Path, max: usize) -> Option<Vec<u8>> {
    let m = std::fs::metadata(path).ok()?;
    (m.is_file() && m.len() <= max as u64).then(|| std::fs::read(path).ok()).flatten()
}

/// The design a report is about, its name, where it came from (a message id the dialog shows),
/// and how the report says so (`repro.design`).
type DesignChoice = (Option<(String, Vec<u8>)>, Option<&'static str>, Option<&'static str>);

/// The design a report attaches.
/// - About a crash: the crash report's copy of the crashed session's design (its last autosave,
///   or its file), never the one open now; none when the crash left no copy.
/// - Otherwise the open design, unless it is new and empty while a design left by a run that
///   ended (a crash, a lost graphics device) waits for recovery: then that one.
fn design_for(app: &SolveApp, crash: Option<&Value>) -> DesignChoice {
    if let Some(report) = crash {
        let path = report.get("design_path").and_then(Value::as_str).map(std::path::PathBuf::from);
        let Some(bytes) = path.and_then(|p| read_bounded(&p, solidcraft_engine::crash::MAX_DESIGN_BYTES)) else {
            return (None, Some("report.design_none_crash"), Some("none: the crashed session left no saved copy of its design"));
        };
        let base = report.get("base").and_then(|b| b.get("file")).and_then(Value::as_str);
        let name = base
            .and_then(|f| f.rsplit(['/', '\\']).next())
            .filter(|n| n.ends_with(".sc"))
            .map_or_else(|| "crashed-session.sc".to_string(), |n| super::limited(n, 120));
        return (Some((name, bytes)), Some("report.design_from_crash"), Some("the crashed session's design, from the crash report"));
    }
    let blank = app.session.path.is_none() && app.session.doc.features.is_empty();
    if blank && let Some(dir) = app.autosave.as_ref().map(|a| a.dir.as_path()) {
        let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let left = app.recoverable.first().cloned().or_else(|| {
            solidcraft_engine::recovery::orphans(dir).into_iter().next().filter(|e| now.saturating_sub(e.saved_at) <= RECOVERY_FRESH_SECS)
        });
        if let Some(entry) = left
            && let Ok((doc, _)) = solidcraft_engine::recovery::load(dir, &entry.id)
        {
            let name: String = entry.name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).take(80).collect();
            let name = format!("{}.sc", if name.trim().is_empty() { "recovered" } else { name.trim() });
            let bytes = solidcraft_engine::io::write_design(&doc);
            return (
                Some((name, bytes)),
                Some("report.design_from_recovery"),
                Some("the design kept for recovery when the last run ended, not opened"),
            );
        }
    }
    (Some((design_name(app), solidcraft_engine::io::write_design(&app.session.doc))), None, None)
}

/// A failed importer reads a separate file; its error does not require the open design.
fn failed_plugin_import(app: &SolveApp, failing: Option<&payload::Failing>) -> bool {
    match failing {
        Some(f) if f.command.is_some() => f.command.as_deref() == Some("file.import"),
        _ => app.issue_report.steps.back().is_some_and(|step| {
            step.get("command").and_then(Value::as_str) == Some("file.import") && step.get("ok").and_then(Value::as_bool) == Some(false)
        }),
    }
}

/// Everything the boxes can attach, collected when the report opens. `failing`: the operation
/// the report is about, if any.
pub fn evidence(
    app: &SolveApp,
    ctx: Option<&egui::Context>,
    failing: Option<payload::Failing>,
    crash: Option<(String, String)>,
) -> payload::Evidence {
    // A crash report that does not read is still about the crash: it then has no design to give.
    let report: Option<Value> = crash.as_ref().map(|(_, json)| serde_json::from_str(json).unwrap_or(Value::Null));
    let import_failed = report.is_none() && failed_plugin_import(app, failing.as_ref());
    let (design, design_from, design_said) = if import_failed {
        // The failed plugin read a different input file. Offering the open design risks sharing
        // unrelated private work and cannot reproduce the failure (#18760).
        (None, Some("report.design_unrelated_import"), Some("none: the failed plugin import read a separate input file"))
    } else {
        design_for(app, report.as_ref())
    };
    // How to get back to where the failure happened: the design (its history is in the file)
    // plus the one failed operation, on this exact build.
    let mut session = serde_json::Map::new();
    let start = match (&report, design_from) {
        (Some(_), _) => "crashed",
        (None, Some("report.design_unrelated_import")) if app.session.path.is_some() => "opened",
        (None, Some("report.design_unrelated_import")) => "new",
        (None, Some(_)) => "recovered",
        (None, None) if app.session.path.is_some() => "opened",
        (None, None) => "new",
    };
    session.insert("session_start".into(), json!(start));
    if let Some((_, bytes)) = &design {
        session.insert("design_sha256".into(), json!(api::sha256_hex(bytes)));
    }
    if let Some(said) = design_said {
        session.insert("design".into(), json!(said));
    }
    session.insert("format_version".into(), json!(solidcraft_engine::doc::format::FORMAT));
    if report.is_none() {
        session.insert("undo_steps".into(), json!(app.session.undo.len()));
        session.insert("redo_steps".into(), json!(app.session.redo.len()));
    }
    session.insert("commit".into(), json!(crate::help::COMMIT));
    let steps = match &report {
        Some(r) => crash_steps(r),
        None => app.issue_report.steps.iter().cloned().collect(),
    };
    let (system_fields, system_text) = system(app, ctx);
    payload::Evidence {
        system: system_text,
        system_fields,
        app_version: crate::help::VERSION.to_string(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        session,
        failing,
        steps,
        log: log_lines(app),
        screenshot: None,
        design,
        design_from,
        crash,
    }
}

/// The failed operation a draft was made for: its command and parameters as the command API
/// took them, and its error.
fn failing_of(draft: &Draft) -> Option<payload::Failing> {
    if let Some((id, json)) = &draft.crash {
        let v: Value = serde_json::from_str(json).unwrap_or_default();
        let command = v.get("command").and_then(|c| c.get("command")).and_then(Value::as_str).map(str::to_string);
        let params = v.get("command").and_then(|c| c.get("params")).cloned();
        let error = v.get("message").and_then(Value::as_str).unwrap_or("internal error").to_string();
        // Where it broke, from the kind the crash handler recorded (never from its message): a
        // panic the kernel caught is the kernel's, one in a command the app's.
        let kind = v.get("kind").and_then(Value::as_str).unwrap_or_default();
        let origin = match kind {
            "kernel_panic_recovered" => Some("solidcore-kernel".to_string()),
            "command_panic" => Some("app".to_string()),
            _ => None,
        };
        let code = (!kind.is_empty()).then(|| format!("{kind} ({id})"));
        return Some(payload::Failing { command, params, error, origin, code });
    }
    let super::Origin::View { error: Some(error) } = &draft.origin else { return None };
    let (command, params) = draft.failed_command.clone().map_or((None, None), |(c, p)| (Some(c), Some(p)));
    Some(payload::Failing { command, params, error: error.clone(), origin: None, code: None })
}

/// The description a failed operation's report starts with: what failed, how, and a line
/// for the person to say what they were trying to do.
fn template(label: &str, failing: &payload::Failing) -> String {
    let mut s = format!("**Failed:** {label}");
    if let Some(c) = &failing.command {
        s.push_str(&format!(" (`{c}`)"));
    }
    // Cut where a reader can tell it was cut; the whole error is in the report details.
    s.push_str(&format!("\n**Error:** {}\n", readable(&failing.error, 1000)));
    if let Some(Value::Object(p)) = &failing.params {
        // The settings a person reads; the full parameters go in the report details.
        let key: Vec<String> = p.iter().filter(|(_, v)| !v.is_object() && !v.is_array()).take(8).map(|(k, v)| format!("{k} = {v}")).collect();
        if !key.is_empty() {
            s.push_str(&format!("**Settings:** {}\n", key.join(", ")));
        }
    }
    s.push_str(&format!("\n{}\n", tr("report.trying_to_do")));
    s
}

/// The 3D view's part of a window screenshot (`view` in points), or the whole of it.
pub fn crop(image: &Arc<ColorImage>, view: Option<egui::Rect>, pixels_per_point: f32) -> Arc<ColorImage> {
    let Some(view) = view.filter(|r| r.is_positive()) else { return image.clone() };
    let [w, h] = image.size;
    let px = |v: f32, max: usize| ((v * pixels_per_point).round().max(0.0) as usize).min(max);
    let (x0, y0, x1, y1) = (px(view.min.x, w), px(view.min.y, h), px(view.max.x, w), px(view.max.y, h));
    if x1 <= x0 + 8 || y1 <= y0 + 8 {
        return image.clone();
    }
    let mut pixels = Vec::with_capacity((x1 - x0) * (y1 - y0));
    for y in y0..y1 {
        pixels.extend_from_slice(image.pixels.get(y * w + x0..y * w + x1).unwrap_or_default());
    }
    if pixels.len() != (x1 - x0) * (y1 - y0) {
        return image.clone();
    }
    Arc::new(ColorImage::new([x1 - x0, y1 - y0], pixels))
}

pub fn png(image: &ColorImage) -> Option<Vec<u8>> {
    solidcraft_control_egui::screenshot::encode(image, "view.png").ok()
}

/// The report to send: the form and the ticked boxes only.
pub fn outgoing(draft: &Draft, ticket: &TicketDraft) -> payload::Outgoing {
    let mut evidence = ticket.evidence.clone();
    if ticket.ticks.screenshot {
        evidence.screenshot = draft.view.as_ref().or(draft.image.as_ref()).and_then(|i| png(i));
    }
    // An agent's report sent from this dialog: the person pressed Send, with what they ticked.
    let via = if ticket.agent.is_some() { payload::Via::Agent { confirmed_by: Some("user".into()) } } else { payload::Via::Dialog };
    payload::build_via(&ticket.form, ticket.ticks, &evidence, &format!("solidcraft {}", crate::help::VERSION), &via)
}

/// The form for a draft, pre-filled with what the app knows.
pub fn new_ticket(app: &mut SolveApp, draft: &Draft, ctx: Option<&egui::Context>) -> TicketDraft {
    let failing = failing_of(draft);
    let evidence = evidence(app, ctx, failing.clone(), draft.crash.clone());
    let label = draft.title_prefix.trim_end_matches(": ").to_string();
    let (title, description) = match &failing {
        Some(f) if draft.crash.is_none() => {
            let first = f.error.lines().next().unwrap_or_default();
            (readable(&format!("{label}: {first}"), payload::MAX_TITLE), template(&label, f))
        }
        _ => (draft.title.clone(), draft.description.clone()),
    };
    let mut form = payload::Form { app: api::DEFAULT_APP.into(), title, description, kind: payload::Kind::Bug };
    // The crash report, like the design, is the person's to tick: never ticked to start with.
    let mut ticks = payload::Ticks::default();
    if let Some(ask) = &draft.agent {
        // What the agent asked for, but the design and crash report stay the person's to tick.
        ticks = payload::Ticks { design: false, crash: false, ..ask.ticks };
        form.kind = ask.kind;
        form.app.clone_from(&ask.app);
        if !draft.title.is_empty() {
            form.title.clone_from(&draft.title);
        }
        if !draft.description.is_empty() {
            form.description = if failing.is_some() { format!("{}\n\n{}", draft.description, form.description) } else { draft.description.clone() };
        }
    }
    let id = tickets::new_owner(app);
    TicketDraft { form, ticks, evidence, from_failure: failing.is_some(), agent: draft.agent.as_ref().map(|a| a.id.clone()), id }
}

fn heading(ui: &mut Ui, id: &str) {
    ui_kit::text(ui, tr(id), ui_kit::Text::Heading(id));
}

fn note(ui: &mut Ui, id: &str, text: &str) {
    ui_kit::text(ui, text, ui_kit::Text::Note(id));
}

fn check(ui: &mut Ui, id: &str, value: &mut bool) {
    let r = ui_kit::checkbox(ui, tr(id), value);
    crate::scenario::publish_target(&format!("report:{id}"), r.rect);
}

fn shown(ui: &mut Ui, id: &str, title: &str, text: &str) {
    ui_kit::section(ui, id, title, false, |ui| {
        egui::ScrollArea::vertical().id_salt(id).max_height(160.0).show(ui, |ui| {
            ui_kit::text(ui, text, ui_kit::Text::MonoNote(id));
        });
    });
}

/// The model box, right under the description where it is seen: what it holds, who sees it,
/// and what a report about a failure loses without it.
fn model_box(ticket: &mut TicketDraft, ui: &mut Ui) {
    let Some((name, bytes)) = ticket.evidence.design.clone() else {
        // Said rather than attaching the design open now, which is not the one it is about.
        if let Some(id) = ticket.evidence.design_from {
            note(ui, "report.design_from", tr(id));
        }
        return;
    };
    check(ui, "report.design", &mut ticket.ticks.design);
    note(ui, "report.design_note", tr("report.design_note"));
    note(ui, "report.design_file", &format("report.design_file", &[("name", &name), ("size", &payload::size_text(bytes.len()))]));
    if let Some(id) = ticket.evidence.design_from {
        note(ui, "report.design_from", tr(id));
    }
    if !ticket.ticks.design {
        if ticket.from_failure {
            ui_kit::text(ui, tr("report.design_warning"), ui_kit::Text::Error("report.design_warning"));
        } else {
            note(ui, "report.design_hint", tr("report.design_hint"));
        }
    }
}

/// The other attachment boxes, each with what it holds.
fn attachments(app: &SolveApp, draft: &mut Draft, ticket: &mut TicketDraft, ui: &mut Ui) -> bool {
    let mut retake = false;
    heading(ui, "report.attach");
    check(ui, "report.screenshot", &mut ticket.ticks.screenshot);
    let image = draft.view.clone().or_else(|| draft.image.clone());
    if let Some(image) = image {
        let texture = draft.texture.get_or_insert_with(|| ui.ctx().load_texture("ticket-report-view", image, egui::TextureOptions::LINEAR)).clone();
        ui.add(egui::Image::new(&texture).max_size(vec2(220.0, 124.0)).maintain_aspect_ratio(true));
    } else if !draft.note.is_empty() {
        note(ui, "report.screenshot_state", &draft.note);
    }
    let r = ui_kit::button(ui, tr("report.retake"), ui_kit::Button::Enabled(app.tickets.sending.is_none()));
    crate::scenario::publish_target("report:retake", r.rect);
    retake |= r.clicked();

    let ev = &ticket.evidence;
    check(ui, "report.log", &mut ticket.ticks.log);
    if ev.log.is_empty() {
        note(ui, "report.log_note", tr("report.log_empty"));
    } else {
        note(ui, "report.log_note", &format("report.log_note", &[("count", &ev.log.len().to_string())]));
        shown(ui, "report-log", tr("report.log_show"), &ev.log.join("\n"));
    }

    check(ui, "report.system", &mut ticket.ticks.system);
    note(ui, "report.system_note", tr("report.system_note"));
    shown(ui, "report-system", tr("report.system_show"), &ev.system);

    check(ui, "report.repro", &mut ticket.ticks.repro);
    note(ui, "report.repro_note", tr("report.repro_note"));
    shown(ui, "report-repro", tr("report.repro_show"), &payload::repro_text(ev, ticket.ticks.design && ev.design.is_some()));

    if ev.crash.is_some() {
        check(ui, "report.crash", &mut ticket.ticks.crash);
        note(ui, "report.crash_note", tr("report.crash_note"));
    }
    retake
}

/// A title for a report sent without one: the description's first line, else a plain one.
pub fn title_or_default(title: &str, description: &str) -> String {
    let title = title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    let first = description.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("**")).unwrap_or_default();
    let first = first.trim_start_matches('#').trim();
    if first.is_empty() { tr("report.default_title").to_string() } else { readable(first, 120) }
}

/// The form as it goes: the question the description ends with dropped when it was left
/// unanswered, and a title made from the description when there is none.
pub fn ready_to_send(form: &mut payload::Form) {
    form.description = without_unanswered(&form.description);
    form.title = title_or_default(&form.title, &form.description);
}

/// `text` in at most `max` characters, cut at a line break or a space and ended with "…" when
/// it is longer: never in the middle of a word ("…Surface Natur").
pub fn readable(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(1);
    let end = text.char_indices().nth(keep).map_or(text.len(), |(i, _)| i);
    let head = text.get(..end).unwrap_or_default();
    // A boundary in the first half would throw away too much: cut the word then.
    let half = head.len() / 2;
    let cut = head.rfind('\n').filter(|&i| i >= half).or_else(|| head.rfind(char::is_whitespace).filter(|&i| i >= half)).unwrap_or(head.len());
    let head = head.get(..cut).unwrap_or(head).trim_end_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-' | '('));
    format!("{head}…")
}

/// The description without the "What were you trying to do?" line it started with, when nothing
/// was written after it.
pub fn without_unanswered(description: &str) -> String {
    let question = tr("report.trying_to_do").trim();
    let trimmed = description.trim_end();
    match trimmed.rsplit_once('\n') {
        Some((before, last)) if last.trim() == question => format!("{}\n", before.trim_end()),
        None if trimmed.trim() == question => String::new(),
        _ => description.to_string(),
    }
}

/// What the dialog shows instead of the form once Send was pressed.
enum Phase {
    Form,
    Sending { done: usize, total: usize },
    Opened(String),
    Error { why: String, kept: bool },
}

fn phase(app: &SolveApp, id: u64) -> Phase {
    if app.tickets.owner != id {
        return Phase::Form;
    }
    if let Some(s) = &app.tickets.sending {
        return Phase::Sending { done: s.sent, total: s.total.max(1) };
    }
    match &app.tickets.outcome {
        Some(Outcome::Opened(url)) => Phase::Opened(url.clone()),
        Some(Outcome::Queued(why)) => Phase::Error { why: why.clone(), kept: true },
        Some(Outcome::Failed(why)) => Phase::Error { why: why.clone(), kept: false },
        _ => Phase::Form,
    }
}

/// The address, big enough to read and select, and a button that copies it.
fn link_with_copy(ui: &mut Ui, url: &str) {
    // The page's secret (after `#`) is not shown: the link and Copy link carry it.
    let shown = url.split('#').next().unwrap_or(url).trim_start_matches("https://");
    let r = ui.hyperlink_to(shown, url);
    crate::scenario::publish_target("report:link", r.rect);
    let r = ui_kit::button(ui, tr("report.copy_link"), ui_kit::Button::Normal);
    crate::scenario::publish_target("report:copy_link", r.rect);
    if r.clicked() {
        ui.ctx().copy_text(url.to_string());
    }
}

/// Sending: progress and Cancel. Done: Thanks with the link. Failed: the reason, Retry and Copy
/// details. Never nothing.
fn after_send(app: &mut SolveApp, ui: &mut Ui, id: u64, phase: Phase) {
    match phase {
        Phase::Form => {}
        Phase::Sending { done, total } => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui_kit::text(ui, tr("report.sending_title"), ui_kit::Text::Heading("report.sending_title"));
            });
            note(ui, "report.sending", &format("report.sending", &[("done", &payload::size_text(done)), ("total", &payload::size_text(total))]));
            ui_kit::progress(ui, "report-progress", done as f32 / total as f32);
            let r = ui_kit::button(ui, tr("report.cancel_send"), ui_kit::Button::Normal);
            crate::scenario::publish_target("report:cancel_send", r.rect);
            if r.clicked() {
                tickets::cancel(app);
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        Phase::Opened(url) => {
            ui_kit::text(ui, tr("report.thanks"), ui_kit::Text::Heading("report.thanks"));
            note(ui, "report.opened", tr("report.opened"));
            link_with_copy(ui, &url);
            let r = ui_kit::button(ui, tr("report.open_again"), ui_kit::Button::Normal);
            crate::scenario::publish_target("report:open_again", r.rect);
            if r.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
        Phase::Error { why, kept } => {
            ui_kit::text(ui, tr("report.error_title"), ui_kit::Text::Heading("report.error_title"));
            ui_kit::text(ui, &why, ui_kit::Text::Error("report.error"));
            if kept {
                note(ui, "report.kept", tr("report.kept"));
            }
            ui.horizontal_wrapped(|ui| {
                let r = ui_kit::button(ui, tr("report.retry"), ui_kit::Button::Normal);
                crate::scenario::publish_target("report:retry", r.rect);
                if r.clicked() {
                    tickets::retry(app, id);
                }
                let r = ui_kit::button(ui, tr("report.copy_details"), ui_kit::Button::Normal);
                crate::scenario::publish_target("report:copy_details", r.rect);
                if r.clicked() {
                    ui.ctx().copy_text(format!(
                        "SolidCraft {} ({}): could not send a problem report to {}: {why}",
                        crate::help::VERSION,
                        crate::help::COMMIT,
                        api::server()
                    ));
                }
            });
        }
    }
}

/// Returns whether to retake the screenshot and whether the user closed the form.
pub(super) fn show(app: &mut SolveApp, draft: &mut Draft, ctx: &egui::Context) -> (bool, bool) {
    let mut ticket = match draft.ticket.take() {
        Some(t) => t,
        None => Box::new(new_ticket(app, draft, Some(ctx))),
    };
    let mut retake = false;
    let mut github = false;
    let t = solidcraft_theme::Tokens::get();
    let screen = ctx.content_rect();
    let cards = ((screen.width() - 2.0 * t.space_16) / t.form_card_width).clamp(0.5, 2.0);
    let current = phase(app, ticket.id);
    let on_form = matches!(current, Phase::Form);
    // Once sent (or sending), the form makes way for how it goes: Send is gone until it closes.
    let ok = if on_form { tr("report.send") } else { "" };
    let (closed, action) = ui_kit::Window::new(Id::unique("sc_issue_report"), "bug_report", tr("report.title"))
        .cards(cards)
        .placed(egui::Align2::CENTER_TOP, egui::pos2(screen.center().x, screen.top() + t.space_16))
        .fields()
        .keep_open_on_escape()
        .actions(ok, tr("report.close"), None)
        .show(ctx, on_form && app.tickets.sending.is_none(), |ui| {
            if !on_form {
                after_send(app, ui, ticket.id, current);
                return;
            }
            let chrome = ui_kit::header_height() + t.form_footer_height + 6.0 * t.space_8;
            let scroll = (screen.height() - 2.0 * t.space_16 - chrome).clamp(60.0, 640.0);
            ui.allocate_ui(vec2(ui.available_width(), scroll), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("ticket-report")
                    .max_height(scroll)
                    .min_scrolled_height(scroll)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui_kit::row(ui, tr("report.type"), |ui| {
                            let mut kind = payload::Kind::ALL.iter().position(|k| *k == ticket.form.kind).unwrap_or(0);
                            let labels = [tr("report.type_bug"), tr("report.type_feature"), tr("report.type_question")];
                            if ui_kit::segmented(
                                ui,
                                &mut kind,
                                &labels,
                                ui_kit::Segments {
                                    targets: &["report:type:bug", "report:type:feature", "report:type:question"],
                                    ..Default::default()
                                },
                            ) {
                                ticket.form.kind = payload::Kind::ALL.get(kind).copied().unwrap_or_default();
                            }
                        });
                        heading(ui, "report.summary");
                        let r = ui_kit::bounded_cell_editor(
                            ui,
                            Id::unique("ticket_report_title"),
                            &mut ticket.form.title,
                            tr("report.summary_hint"),
                            false,
                            payload::MAX_TITLE,
                        );
                        crate::scenario::publish_target("report:title", r.rect);
                        heading(ui, "report.description");
                        let r = ui_kit::text_area(
                            ui,
                            tr("report.description"),
                            Id::unique("ticket_report_body"),
                            &mut ticket.form.description,
                            5,
                            payload::MAX_DESCRIPTION,
                        );
                        crate::scenario::publish_target("report:description", r.rect);
                        if !draft.focus_requested {
                            r.request_focus();
                            draft.focus_requested = true;
                        }
                        ui_kit::divider(ui, "report-model");
                        model_box(&mut ticket, ui);
                        ui_kit::divider(ui, "report-attach");
                        retake = attachments(app, draft, &mut ticket, ui);
                        ui_kit::divider(ui, "report-other");
                        let r = ui.link(tr("report.github_instead"));
                        crate::scenario::publish_target("report:github", r.rect);
                        github = r.clicked();
                    });
            });
        });
    if action == ui_kit::Action::Ok && on_form && app.tickets.sending.is_none() {
        // Never refused for a missing title: one is made from the description.
        ready_to_send(&mut ticket.form);
        let report = outgoing(draft, &ticket);
        tickets::send_for(app, report, ticket.id);
    }
    if retake {
        // Retake brings the whole report up to date, not only its picture; what was typed and
        // ticked stays.
        ticket.evidence = evidence(app, Some(ctx), failing_of(draft), draft.crash.clone());
    }
    if github {
        // The public GitHub draft, as before tickets: same answers.
        draft.github = true;
        draft.title.clone_from(&ticket.form.title);
        draft.description = without_unanswered(&ticket.form.description);
    }
    draft.ticket = Some(ticket);
    let close = closed || action == ui_kit::Action::Cancel;
    if close {
        // Closing keeps a report that is being sent going; it is the person's to cancel.
        draft.ticket = None;
    }
    (retake, close)
}
