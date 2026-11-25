//! Reviewed issue drafts with local evidence; never publishes or uploads a report.

#[cfg(test)]
use crate::ui_kit::{Container, Surface};
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::{ColorImage, TextureHandle};

use crate::{SolveApp, dialogs, help};

mod form;
pub mod ticket;

/// The most an answer keeps, in characters (as `TextEdit::char_limit` counts them, not bytes).
const MAX_ANSWER: usize = 2000;
/// An answer with fewer characters left than this says how many.
const ANSWER_ROOM: usize = 200;
const MAX_BODY: usize = 24_000;
// Room for all three answers (up to four bytes a character) and the original bounded diagnostic
// context.
const MAX_REPORT: usize = 3 * 4 * MAX_ANSWER + MAX_BODY + 1000;
const MAX_IMAGE_PIXELS: usize = 16_000_000;
const ISSUE_URL: &str = "https://github.com/tangency-labs/solidcraft/issues/new";
/// GitHub turns away a request whose link and headers together pass about 7,000 bytes: someone
/// who isn't signed in gets "Whoops, something went wrong!" instead of the form (#20510).
const GITHUB_REQUEST: usize = 7000;
/// What a browser sends besides the link: its own headers and GitHub's cookies. An estimate, with
/// room to spare: a signed-out visit sends about 1,000 bytes, and signed-in cookies are larger.
const HEADER_ROOM: usize = 3000;
const SIGN_IN_URL: &str = "https://github.com/login?return_to=";
const MAX_URL: usize = GITHUB_REQUEST - HEADER_ROOM;
/// Someone who isn't signed in is sent to sign in with the link to return to, encoded once more.
const MAX_SIGN_IN_URL: usize = GITHUB_REQUEST - HEADER_ROOM - SIGN_IN_URL.len();

/// Draw CAD behind a report or native picker without delivering input or focus to editors.
/// Fresh focus history also prevents inline editors from committing on report focus loss.
pub(crate) struct CadInput {
    ctx: egui::Context,
    input: Option<egui::InputState>,
    memory: Option<egui::Memory>,
    previous_blocked: bool,
}

impl CadInput {
    pub(crate) fn block(app: &SolveApp, ui: &mut egui::Ui, guard: &mut Option<Self>) {
        if guard.is_none() {
            *guard = Self::begin(app, ui.ctx());
        }
        if guard.is_some() {
            ui.visuals_mut().disabled_alpha = 1.0;
            ui.disable();
        }
    }

    pub(crate) fn begin(app: &SolveApp, ctx: &egui::Context) -> Option<Self> {
        if app.issue_report.draft.is_none() && app.file_picker.is_none() {
            return None;
        }
        let mut raw = ctx.input(|i| i.raw.clone());
        raw.events.clear();
        raw.dropped_files.clear();
        raw.hovered_files.clear();
        let neutral = egui::InputState::default().begin_pass(raw.clone(), true, ctx.pixels_per_point(), egui::InputOptions::default());
        let input = ctx.input_mut(|i| std::mem::replace(i, neutral));
        let memory = ctx.memory_mut(|m| {
            let mut saved = std::mem::take(m);
            let mut neutral = egui::Memory::default();
            neutral.options = saved.options.clone();
            Arc::make_mut(&mut neutral.options.dark_style).visuals.disabled_alpha = 1.0;
            Arc::make_mut(&mut neutral.options.light_style).visuals.disabled_alpha = 1.0;
            neutral.data = saved.data.clone();
            neutral.to_global = saved.to_global.clone();
            *neutral.areas_mut() = saved.areas().clone();
            neutral.caches = std::mem::take(&mut saved.caches);
            *m = neutral;
            saved
        });
        let previous_blocked = crate::ui_kit::input_blocked();
        crate::ui_kit::set_input_blocked(true);
        Some(Self { ctx: ctx.clone(), input: Some(input), memory: Some(memory), previous_blocked })
    }
}

impl Drop for CadInput {
    fn drop(&mut self) {
        crate::ui_kit::set_input_blocked(self.previous_blocked);
        if let Some(input) = self.input.take() {
            self.ctx.input_mut(|i| *i = input);
        }
        if let Some(mut memory) = self.memory.take() {
            self.ctx.memory_mut(|m| {
                // Keep current layout/widget state and caches; isolate only input/focus history.
                memory.data = std::mem::take(&mut m.data);
                memory.caches = std::mem::take(&mut m.caches);
                memory.to_global = std::mem::take(&mut m.to_global);
                *memory.areas_mut() = m.areas().clone();
                *m = memory;
            });
        }
    }
}

#[derive(Default)]
pub struct ReportState {
    pub draft: Option<Draft>,
    /// Only typed answers, kept in memory until restored, discarded or the app exits.
    earlier: Option<Answers>,
    pending: Option<Pending>,
    /// A screenshot waiting for the preview being computed (#7171).
    waiting: Option<Waiting>,
    /// What was typed while a capture hid the report, for the form once it is back.
    held: Vec<egui::Event>,
    /// The report's field that had focus when a capture hid it.
    refocus: Option<egui::Id>,
    cad_focus: Option<egui::Id>,
    /// The commands run this session, for a report's recent steps (`ticket::record_step`).
    pub(crate) steps: std::collections::VecDeque<serde_json::Value>,
}

/// How long a screenshot waits for a preview being computed before it shows it unfinished.
const PREVIEW_WAIT_MS: f64 = 10_000.0;
/// Keystrokes kept while a capture hides the report.
const MAX_HELD: usize = 4096;

struct Waiting {
    deadline: f64,
    /// [`context_signature`] when it began: the preview it waits for belongs to the report
    /// unless the design, the dialog or the selection changed meanwhile.
    context: u64,
}

pub struct Draft {
    pub title: String,
    title_prefix: String,
    /// Collected technical context, separately reviewable and editable.
    pub body: String,
    pub(crate) description: String,
    steps: String,
    expected: String,
    opened_with: Answers,
    restore_allowed: bool,
    image: Option<Arc<ColorImage>>,
    texture: Option<TextureHandle>,
    /// What the technical details are built from, to rebuild them on Retake (#7350).
    origin: Origin,
    /// [`Draft::body`] as built: when it is unchanged, the user didn't edit it.
    built_body: String,
    /// Said with the next screenshot: the technical details were rebuilt for it.
    rebuilt: Option<&'static str>,
    pub note: String,
    source_signature: u64,
    /// The description has taken the focus it gets on opening; Continue finding it empty clears
    /// this, so it takes the focus once more (#7190).
    focus_requested: bool,
    missing_description: bool,
    opened_github: bool,
    /// The in-app report's own state (`ticket`), made when it first shows.
    ticket: Option<Box<ticket::TicketDraft>>,
    /// The screenshot's 3D view part, for the in-app report.
    view: Option<Arc<ColorImage>>,
    /// The crash report a crash draft was made from: its id and JSON.
    crash: Option<(String, String)>,
    /// The command that failed, as the command API takes it, for a report made from a failed
    /// operation (`draft_for` with an error and its dialog).
    failed_command: Option<(String, serde_json::Value)>,
    /// The person chose the public GitHub draft over the ticket server.
    github: bool,
    /// An agent's `diagnostics.report_issue` opened this report for the person to agree to
    /// attachments only they can agree to.
    pub(crate) agent: Option<crate::tickets::agent::Ask>,
}

impl Draft {
    fn new(title: String, body: String, source_signature: u64) -> Self {
        Self {
            opened_with: Answers { title: title.clone(), ..Default::default() },
            restore_allowed: false,
            title,
            title_prefix: String::new(),
            body,
            description: String::new(),
            steps: String::new(),
            expected: String::new(),
            image: None,
            texture: None,
            origin: Origin::View { error: None },
            built_body: String::new(),
            rebuilt: None,
            note: String::new(),
            source_signature,
            focus_requested: false,
            missing_description: false,
            opened_github: false,
            ticket: None,
            view: None,
            crash: None,
            failed_command: None,
            github: false,
            agent: None,
        }
    }

    fn can_restore_answers(&self) -> bool {
        self.restore_allowed && [&self.title, &self.description, &self.steps, &self.expected].iter().all(|s| s.trim().is_empty())
    }

    fn report_title(&self) -> String {
        if self.title.trim().is_empty() {
            let summary = limited(self.description.trim().lines().next().unwrap_or_default(), 160);
            limited(&format!("{}{summary}", self.title_prefix), 160)
        } else {
            limited(&self.title, 160)
        }
    }

    fn report_body(&self) -> String {
        let answer = |text: &str| if text.trim().is_empty() { "Not provided".to_string() } else { limited_chars(text, MAX_ANSWER) };
        let mut report = format!(
            "## Actual behavior\n{}\n\n## Reproduction\n{}\n\n## Expected behavior\n{}\n\n",
            answer(&self.description),
            answer(&self.steps),
            answer(&self.expected)
        );
        let room = MAX_REPORT.saturating_sub(report.len());
        if self.body.len() <= room {
            report.push_str(&self.body);
        } else {
            let marker = "\n[technical details truncated]\n";
            report.push_str(&limited(&self.body, room.saturating_sub(marker.len())));
            report.push_str(marker);
        }
        report
    }
}

#[derive(Default)]
struct Answers {
    title: String,
    description: String,
    steps: String,
    expected: String,
}

impl Answers {
    fn edited(draft: &Draft) -> Option<Self> {
        let keep = |text: &str, initial: &str, max| {
            if text.trim().is_empty() || text == initial { String::new() } else { limited_chars(text, max) }
        };
        let initial = &draft.opened_with;
        let answers = Self {
            title: keep(&draft.title, &initial.title, 160),
            description: keep(&draft.description, &initial.description, MAX_ANSWER),
            steps: keep(&draft.steps, &initial.steps, MAX_ANSWER),
            expected: keep(&draft.expected, &initial.expected, MAX_ANSWER),
        };
        [&answers.title, &answers.description, &answers.steps, &answers.expected].iter().any(|s| !s.is_empty()).then_some(answers)
    }

    fn restore(self, draft: &mut Draft) {
        draft.title = self.title;
        draft.description = self.description;
        draft.steps = self.steps;
        draft.expected = self.expected;
        draft.focus_requested = false;
        draft.missing_description = false;
        draft.opened_github = false;
    }
}

enum Origin {
    /// [`draft_for`] with the dialog open and the error the report was made with.
    View { error: Option<String> },
    /// [`crash_draft`]: the crash report, not the view.
    Crash,
}

/// What a draft's note says after a Retake rebuilt its technical details for the current view.
const NOTE_DETAILS_UPDATED: &str =
    "Technical details updated to match the current view. Screenshot captured locally. Review it before attaching it on GitHub.";
const NOTE_DETAILS_KEPT: &str = "Screenshot captured locally. Your edited technical details were kept: check that they still match the current view.";

struct Pending {
    token: u64,
    signature: u64,
    deadline: f64,
    window: egui::Rect,
    pixels_per_point: f32,
    /// The longest side a texture can have here: the screenshot's preview is one.
    max_side: usize,
}

/// A bounded formatting sink also avoids allocating a full hostile Debug/JSON value first.
struct Text {
    value: String,
    limit: usize,
}

impl std::fmt::Write for Text {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let room = self.limit.saturating_sub(self.value.len());
        if s.len() <= room {
            self.value.push_str(s);
            return Ok(());
        }
        let mut end = room.min(s.len());
        while !s.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        self.value.push_str(s.get(..end).unwrap_or_default());
        Err(std::fmt::Error)
    }
}

fn debug(value: &impl std::fmt::Debug, limit: usize) -> String {
    let mut text = Text { value: String::new(), limit };
    if write!(&mut text, "{value:#?}").is_err() {
        text.value.push_str("\n[truncated]");
    }
    text.value
}

struct JsonBytes {
    value: Vec<u8>,
    limit: usize,
}
impl std::io::Write for JsonBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let n = bytes.len().min(self.limit.saturating_sub(self.value.len()));
        self.value.extend_from_slice(bytes.get(..n).unwrap_or_default());
        if n != bytes.len() {
            return Err(std::io::Error::other("report context limit"));
        }
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn json_context(value: &impl serde::Serialize, limit: usize) -> String {
    let mut bytes = JsonBytes { value: Vec::new(), limit };
    let truncated = serde_json::to_writer_pretty(&mut bytes, value).is_err();
    let mut value = String::from_utf8_lossy(&bytes.value).into_owned();
    if truncated {
        value.push_str("\n[truncated]");
    }
    value
}

fn truncate(value: &mut String, limit: usize) {
    if value.len() > limit {
        let mut end = limit;
        while !value.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        value.truncate(end);
    }
}

fn limited(value: &str, limit: usize) -> String {
    let mut end = limit.min(value.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value.get(..end).unwrap_or_default().to_string()
}

/// [`truncate`] by characters: an answer's limit counts them, not bytes (#7191).
fn truncate_chars(value: &mut String, limit: usize) {
    if let Some((end, _)) = value.char_indices().nth(limit) {
        value.truncate(end);
    }
}

/// [`limited`] by characters.
fn limited_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn block(value: &str) -> String {
    value.lines().map(|line| format!("    {line}\n")).collect()
}

/// Hash directly through formatting, without constructing an unbounded Debug string.
struct Fingerprint(std::collections::hash_map::DefaultHasher);
impl std::fmt::Write for Fingerprint {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write(s.as_bytes());
        Ok(())
    }
}

/// What a report's context is drawn from: the design, the dialog and the selection.
fn context_signature(app: &SolveApp, dialog: Option<&dialogs::Dialog>) -> u64 {
    let mut h = Fingerprint(std::collections::hash_map::DefaultHasher::new());
    app.session.revision.hash(&mut h.0);
    app.docs.active.hash(&mut h.0);
    app.docs.count().hash(&mut h.0);
    app.session.doc.name.hash(&mut h.0);
    // What the dialog makes, not its state: a preview error it shows (a sketch fillet's) is often
    // what is being reported.
    if let Some(d) = dialog {
        let _ = write!(&mut h, "{:?}{:?}{:?}", d.kind, d.inputs, d.extra);
    }
    let _ = write!(&mut h, "{:?}{}", app.session.selection, app.session.all_bodies_selected());
    h.0.finish()
}

/// [`context_signature`] and the preview shown with it.
fn source_signature(app: &SolveApp, dialog: Option<&dialogs::Dialog>) -> u64 {
    let mut h = Fingerprint(std::collections::hash_map::DefaultHasher::new());
    context_signature(app, dialog).hash(&mut h.0);
    app.preview.key.hash(&mut h.0);
    app.preview.error.hash(&mut h.0);
    app.preview.busy.hash(&mut h.0);
    h.0.finish()
}

fn signature(app: &SolveApp, dialog: Option<&dialogs::Dialog>) -> u64 {
    let mut h = Fingerprint(std::collections::hash_map::DefaultHasher::new());
    source_signature(app, dialog).hash(&mut h.0);
    let _ = write!(&mut h, "{:?}{:?}{:?}", app.cam, app.viewport.rect, app.ui.perspective);
    h.0.finish()
}

fn reportable(feature: &solidcraft_engine::doc::Feature) -> bool {
    !matches!(
        feature.kind,
        solidcraft_engine::doc::FeatureKind::Sketch { .. }
            | solidcraft_engine::doc::FeatureKind::Import { .. }
            | solidcraft_engine::doc::FeatureKind::MeshImport { .. }
    )
}

fn feature_context(feature: &solidcraft_engine::doc::Feature) -> String {
    format!("Feature {}: {}\n{}", feature.id, debug(&feature.name, 160), block(&json_context(&feature.kind, 2000)))
}

/// Messages and feature errors listed in a report (#7348), each entry cut to `MAX_ENTRY_CHARS`.
const REPORT_MESSAGES: usize = 8;
const REPORT_FAILURES: usize = 10;
const MAX_ENTRY_CHARS: usize = 300;

/// The user's home folder as text, for [`report_text`]; a bare root is never replaced.
fn home_text() -> Option<String> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    let home = home.to_string_lossy().trim_end_matches(['/', '\\']).to_string();
    (home.chars().count() > 1).then_some(home)
}

/// Untrusted text (messages, feature errors) as one quoted line: the home folder shown as `~`,
/// cut to `MAX_ENTRY_CHARS` characters.
fn report_text(text: &str, home: Option<&str>) -> String {
    let mut text = limited(text, MAX_ENTRY_CHARS * 8);
    if let Some(home) = home.filter(|h| h.chars().count() > 1) {
        text = text.replace(home, "~");
    }
    if text.chars().count() > MAX_ENTRY_CHARS {
        text = format!("{}…", text.chars().take(MAX_ENTRY_CHARS).collect::<String>());
    }
    format!("{text:?}")
}

/// How long ago `at` was, at `now` (both ms), in English like the rest of the report.
fn report_ago(now: f64, at: f64) -> String {
    let s = ((now - at) / 1000.0).max(0.0);
    let s = if s.is_finite() { s.floor() } else { 0.0 };
    if s < 60.0 {
        format!("{s:.0} s ago")
    } else if s < 3600.0 {
        format!("{:.0} min ago", (s / 60.0).floor())
    } else {
        format!("{:.0} h ago", (s / 3600.0).floor())
    }
}

/// The last messages of the Messages log, oldest first.
fn messages_context(messages: &crate::messages::Messages, now: f64, home: Option<&str>) -> String {
    let all = messages.iter();
    let total = all.len();
    if total == 0 {
        return "\nMessages: none\n".into();
    }
    let shown = total.min(REPORT_MESSAGES);
    let mut out = format!("\nMessages (last {shown} of {total}):\n");
    for m in all.skip(total.saturating_sub(shown)) {
        let level = if m.error { "error" } else { "info" };
        let repeats = if m.count > 1 { format!(" (×{})", m.count) } else { String::new() };
        out.push_str(&format!("    {} {level}: {}{repeats}\n", report_ago(now, m.at), report_text(&m.text, home)));
    }
    out
}

/// Every feature whose result has an error, in timeline order, not only the selected or latest one.
fn failures_context(app: &SolveApp, home: Option<&str>) -> String {
    let mut failing = app.session.doc.features.iter().filter_map(|f| app.session.model.result(f.id)?.error.as_deref().map(|e| (f, e)));
    let mut out = String::new();
    let mut count = 0usize;
    for (feature, error) in failing.by_ref().take(REPORT_FAILURES) {
        count += 1;
        out.push_str(&format!(
            "    Feature {} {} ({}): {}\n",
            feature.id,
            report_text(&feature.name, home),
            feature.kind.type_name(),
            report_text(error, home)
        ));
    }
    let more = failing.count();
    match (count, more) {
        (0, _) => "\nFailing features: none\n".into(),
        (n, 0) => format!("\nFailing features ({n}):\n{out}"),
        (n, more) => format!("\nFailing features (first {n} of {}):\n{out}", n + more),
    }
}

/// Read-only context excludes whole documents, sketch geometry and imported file contents.
pub fn draft_for(app: &SolveApp, dialog: Option<&dialogs::Dialog>, error: Option<&str>) -> Draft {
    let mut context = String::new();
    let label = dialog.map(dialogs::report_title).unwrap_or_else(|| crate::mouse_gesture::Studio::of(app).label().into());
    if let Some(d) = dialog {
        match dialogs::apply_commands(app, d) {
            Ok(commands) => context.push_str(&block(&json_context(&commands, 10_000))),
            Err(e) => {
                context.push_str(&format!("Could not build command parameters: {}\n", debug(&e, 1000)));
                context.push_str(&block(&debug(&d.kind, 6000)));
                context.push_str(&block(&debug(&d.inputs, 3000)));
                context.push_str(&block(&debug(&d.extra, 1000)));
            }
        }
    }
    if app.session.all_bodies_selected() {
        let count = app.session.selected_count().unwrap_or(0);
        let _ = writeln!(&mut context, "\nSelect All: every shown body ({count}) is selected.");
    }
    context.push_str("\nSelected items (including occurrence paths):\n");
    context.push_str(&block(&debug(&app.session.selection.iter().take(32).collect::<Vec<_>>(), 3000)));
    for item in app.session.selection.iter().take(8) {
        if let solidcraft_engine::Sel::Feature { id } = item
            && let Some(feature) = app.session.doc.feature(*id)
            && reportable(feature)
        {
            context.push_str(&feature_context(feature));
            if let Some(e) = app.session.model.result(*id).and_then(|r| r.error.as_deref()) {
                context.push_str(&format!("Feature error: {}\n", debug(&e, 1000)));
            }
        }
    }
    if dialog.is_none()
        && let Some(feature) = app.session.doc.features.iter().rev().find(|f| reportable(f))
    {
        context.push_str("\nLatest modeling operation:\n");
        context.push_str(&feature_context(feature));
        if let Some(e) = app.session.model.result(feature.id).and_then(|r| r.error.as_deref()) {
            context.push_str(&format!("Feature error: {}\n", debug(&e, 1000)));
        }
    }
    let home = home_text();
    context.push_str(&failures_context(app, home.as_deref()));
    context.push_str(&messages_context(&app.messages, crate::now_ms(), home.as_deref()));
    let actual = error.map(|e| debug(&e, 4000)).unwrap_or_else(|| "No command error was recorded.".into());
    let image_state = if error.is_some() {
        "A failed preview may display the last valid geometry; that geometry is not evidence that the failed operation succeeded."
    } else {
        "The screenshot shows the current view."
    };
    let mut body = format!(
        "## Operation context\n{actual}\n\n{image_state}\n\n## Settings and selection\n{context}\n## Environment\n{}\nRevision: {}\nCamera: {}\n\n## Visual context\nAttach the reviewed screenshot or a short example showing the problem.\n",
        block(&debug(&help::diagnostics(app), 2000)),
        app.session.revision,
        block(&debug(&app.cam, 1500))
    );
    if body.len() > MAX_BODY {
        truncate(&mut body, MAX_BODY - 32);
        body.push_str("\n[report context truncated]\n");
    }
    let mut title = if error.is_some() { format!("{label}: operation failed") } else { String::new() };
    truncate(&mut title, 160);
    let mut draft = Draft::new(title, body, source_signature(app, dialog));
    draft.restore_allowed = dialog.is_none() && error.is_none();
    draft.title_prefix = format!("{label}: ");
    // More than the details show of it, so a rebuild shows the same.
    draft.origin = Origin::View { error: error.map(|e| limited(e, 8000)) };
    // The failed operation exactly as the command API takes it: the attached design plus this
    // command replays the failure (`solidcraft-cli run replay.json --in design.sc`).
    if error.is_some()
        && let Some(d) = dialog
    {
        draft.failed_command = dialogs::apply_commands(app, d).ok().and_then(|c| c.last().cloned());
    }
    draft.built_body.clone_from(&draft.body);
    draft
}

/// A draft from a crash report (`solidcraft_engine::crash`): what happened, where, the command
/// and how to replay it. The design stays on this computer; the draft says how to share it.
pub fn crash_draft(report: &serde_json::Value) -> Draft {
    let s = |k: &str| report.get(k).and_then(serde_json::Value::as_str).unwrap_or("?").to_string();
    let kind = report.get("kind").and_then(serde_json::Value::as_str);
    let what = match kind {
        Some("command_panic") => "An internal error stopped a command (the design was left unchanged)",
        Some("kernel_panic_recovered") => "The geometry kernel hit an internal error (caught; the operation failed)",
        Some("unclean_exit") => "An earlier SolidCraft session ended without recording a clean exit",
        _ => "SolidCraft hit an internal error",
    };
    let detection = if kind == Some("unclean_exit") {
        "\n\nDetected on a later launch; the exact exit time was not recorded. A system shutdown or forced stop can also cause an unclean exit."
    } else {
        ""
    };
    let command = report.get("command").and_then(|c| c.get("command")).and_then(serde_json::Value::as_str);
    let frames: Vec<&str> =
        report.get("backtrace").and_then(serde_json::Value::as_array).into_iter().flatten().filter_map(serde_json::Value::as_str).take(25).collect();
    let recent: Vec<String> = report
        .get("recent_commands")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = c.get("command")?.as_str()?;
            Some(if c.get("ok").and_then(serde_json::Value::as_bool) == Some(false) { format!("{id} (failed)") } else { id.to_string() })
        })
        .collect();
    let app = report.get("app");
    let field = |o: Option<&serde_json::Value>, k: &str| o.and_then(|o| o.get(k)).and_then(serde_json::Value::as_str).unwrap_or("?").to_string();
    let system = report.get("system");
    let mut body = format!(
        "## What happened\n{what}{}.{detection}\n\nMessage: {}\nLocation: {}\nThread: {}\nRecorded {} time(s), first recorded {}\n\n## Backtrace\n{}\n## Command\n{}\n## Recent commands\n{}\n\n## Reproduction\nCrash report `{}` on the reporter's computer holds {}. Replay it with `solidcraft-cli crashes replay {}`.\nOnly attach the design if it can be shared publicly.\n\n## Environment\n{}\n",
        command.map(|c| format!(" while running `{c}`")).unwrap_or_default(),
        limited(&s("message"), 2000),
        s("location"),
        s("thread"),
        report.get("count").and_then(serde_json::Value::as_u64).unwrap_or(1),
        s("first_seen"),
        block(&frames.join("\n")),
        block(&json_context(&report.get("command"), 4000)),
        if recent.is_empty() { "(none recorded)".to_string() } else { recent.join(", ") },
        s("id"),
        match (report.get("design_path").is_some(), report.get("replay_path").is_some()) {
            (true, true) => "the design as it was before the crash (design.sc) and the commands that crash it (replay.json)",
            (false, true) => "the commands that crash it on a new design (replay.json)",
            _ => "no replay (see its notes)",
        },
        s("id"),
        block(&format!(
            "SolidCraft {} ({}, built {})\nOS: {} {}\n{}",
            field(app, "version"),
            field(app, "commit"),
            field(app, "built"),
            field(system, "os"),
            field(system, "arch"),
            field(system, "environment")
        )),
    );
    if body.len() > MAX_BODY {
        truncate(&mut body, MAX_BODY - 32);
        body.push_str("\n[report context truncated]\n");
    }
    let title = ticket::readable(&format!("Crash: {}", s("message").lines().next().unwrap_or_default()), 160);
    let mut draft = Draft::new(title, body, 0);
    draft.origin = Origin::Crash;
    draft.description =
        limited(&format!("{what}{}.", command.map(|c| format!(" while running `{}`", limited(c, 160))).unwrap_or_default()), MAX_ANSWER);
    draft.opened_with.description.clone_from(&draft.description);
    draft
}

/// Open a reviewed draft for a crash report (no screenshot: the view shows nothing of it).
pub fn begin_crash(app: &mut SolveApp, ctx: &egui::Context, report: &serde_json::Value) {
    if keep_open_draft(app, true) {
        return;
    }
    if app.issue_report.draft.is_none() {
        app.issue_report.cad_focus = ctx.memory(|m| m.focused());
    }
    cancel_capture(app);
    let mut draft = crash_draft(report);
    let id = report.get("id").and_then(serde_json::Value::as_str).unwrap_or("report");
    draft.crash = serde_json::to_string_pretty(report).ok().map(|json| (limited(id, 80), limited(&json, 1 << 20)));
    // A screenshot can still be added of the view as it is now.
    draft.source_signature = source_signature(app, None);
    app.issue_report.draft = Some(draft);
}

/// A query value as GitHub reads it: a space is `+`, so a `+` is encoded, as are `&`, `#`, `%`,
/// `=`, quotes, line breaks and every byte outside ASCII. The rest of what RFC 3986 allows in a
/// query stays as it is: a report's link is about a fifth longer than its text, not two-thirds.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b == b' ' {
            out.push('+');
        } else if b.is_ascii_alphanumeric() || b"-_.~!$()*,;:@/?".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(&mut out, "%{b:02X}");
        }
    }
    out
}

/// The length of `url` as GitHub's sign-in page carries it back: encoded once more.
fn sign_in_length(url: &str) -> usize {
    url.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.*~".contains(&b) { 1 } else { 3 }).sum()
}

/// The link to a new issue with `title` and `body` filled in, and whether `body` was too long
/// for it (the link then carries the title alone): a report went in it whole only up to 2,000
/// characters, with every space and line break taking three, so most went through the
/// clipboard and a quarter of them arrived empty (#7103). Long drafts stay local and can be
/// copied into the browser editor; never discard them.
pub fn draft_url(title: &str, body: &str) -> (String, bool) {
    let title = limited(title, 160);
    let base = format!("{ISSUE_URL}?title={}", encode(&title));
    if body.len() <= MAX_URL {
        let url = format!("{base}&body={}", encode(body));
        if url.len() <= MAX_URL && sign_in_length(&url) <= MAX_SIGN_IN_URL {
            return (url, false);
        }
    }
    (base, true)
}

pub fn begin(app: &mut SolveApp, ctx: &egui::Context, dialog: Option<&dialogs::Dialog>, error: Option<&str>) {
    if keep_open_draft(app, dialog.is_some() || error.is_some()) {
        return;
    }
    if app.issue_report.draft.is_none() {
        app.issue_report.cad_focus = ctx.memory(|m| m.focused());
    }
    cancel_capture(app);
    app.issue_report.draft = Some(window_draft(app, ctx, dialog, error));
    capture(app, ctx, dialog);
}

/// Reopening must not replace the reviewed context or the typing waiting for its screenshot.
fn keep_open_draft(app: &mut SolveApp, new_context: bool) -> bool {
    let state = &mut app.issue_report;
    let Some(draft) = state.draft.as_mut() else { return false };
    if new_context {
        draft.note = solidcraft_ui_text::tr("ui.report_finish_current").to_string();
    }
    true
}

/// [`draft_for`] and the window it is shown in.
fn window_draft(app: &SolveApp, ctx: &egui::Context, dialog: Option<&dialogs::Dialog>, error: Option<&str>) -> Draft {
    let mut draft = draft_for(app, dialog, error);
    let rect = ctx.content_rect();
    let _ = write!(
        &mut draft.body,
        "\nWindow: {:.0}×{:.0} logical points; {:.2} physical pixels/point\n",
        rect.width(),
        rect.height(),
        ctx.pixels_per_point()
    );
    truncate(&mut draft.body, MAX_BODY);
    draft.built_body.clone_from(&draft.body);
    draft
}

/// Retake: a draft whose design, dialog, selection or preview changed since it was made gets its
/// technical details rebuilt for the current view first, and keeps what was typed (#7350). Edited
/// details stay as edited; a crash draft's come from its crash report and stay too.
fn retake(app: &mut SolveApp, ctx: &egui::Context, dialog: Option<&dialogs::Dialog>) {
    let current = source_signature(app, dialog);
    let Some(draft) = app.issue_report.draft.as_ref() else { return };
    let changed = draft.source_signature != current;
    let fresh = match &draft.origin {
        Origin::View { error } if changed => Some(window_draft(app, ctx, dialog, error.as_deref())),
        _ => None,
    };
    let Some(draft) = app.issue_report.draft.as_mut() else { return };
    draft.rebuilt = None;
    if changed {
        draft.source_signature = current;
    }
    if let Some(fresh) = fresh {
        if draft.body == draft.built_body {
            draft.body.clone_from(&fresh.body);
            draft.rebuilt = Some(NOTE_DETAILS_UPDATED);
        } else {
            draft.rebuilt = Some(NOTE_DETAILS_KEPT);
        }
        draft.built_body = fresh.body;
        draft.title_prefix = fresh.title_prefix;
    }
    capture(app, ctx, dialog);
}

fn cancel_capture(app: &mut SolveApp) {
    app.issue_report.waiting = None;
    if let Some(pending) = app.issue_report.pending.take() {
        app.shots.queued.retain(|(token, ..)| *token != pending.token);
    }
}

fn capture(app: &mut SolveApp, ctx: &egui::Context, dialog: Option<&dialogs::Dialog>) {
    capture_unless_busy(app, ctx, dialog, true);
}

/// Take a screenshot for the report; `wait`: while the preview is being computed, wait for it
/// with the report open ([`tick`] takes it once it is done, #7171).
fn capture_unless_busy(app: &mut SolveApp, ctx: &egui::Context, dialog: Option<&dialogs::Dialog>, wait: bool) {
    cancel_capture(app);
    let current_source = source_signature(app, dialog);
    let context = context_signature(app, dialog);
    let full = app.shots.queued.len() >= 16;
    let busy = app.preview.busy;
    // The screenshot the draft has stays until a new one is taken (#7350).
    let Some(draft) = app.issue_report.draft.as_mut() else { return };
    if draft.source_signature != current_source {
        draft.note =
            "Settings or model changed while waiting for the preview. Retake the screenshot to update the technical details for the current view."
                .into();
        return;
    }
    if wait && (busy || full) {
        draft.note = "Waiting for the preview to finish to take the screenshot…".into();
        app.issue_report.waiting = Some(Waiting { deadline: crate::now_ms() + PREVIEW_WAIT_MS, context });
        ctx.request_repaint();
        return;
    }
    if full {
        draft.note = "Screenshot unavailable while the preview is updating. Reopen the report after it settles, or continue with text.".into();
        return;
    }
    if busy {
        draft.note = "The preview is still being computed: the screenshot shows it unfinished.".into();
        let line = format!(
            "\nThe preview was still being computed {:.0} s after the report opened; the screenshot shows it unfinished.\n",
            PREVIEW_WAIT_MS / 1000.0
        );
        if !draft.body.contains(line.trim()) {
            let built = draft.body == draft.built_body;
            draft.body.push_str(&line);
            truncate(&mut draft.body, MAX_BODY);
            if built {
                draft.built_body.clone_from(&draft.body);
            }
        }
    }
    // The form hides for the capture: give its focused field back after it.
    if draft.focus_requested {
        app.issue_report.refocus = ctx.memory(|m| m.focused());
    }
    let Some(token) = app.shots.token.checked_add(1) else {
        draft.note = "Screenshot request limit reached; restart the app to capture another frame.".into();
        return;
    };
    app.shots.token = token;
    if !busy {
        draft.note = "Capturing the current app view locally…".into();
    }
    let now = crate::now_ms();
    app.issue_report.pending = Some(Pending {
        token,
        signature: signature(app, dialog),
        deadline: now + 8000.0,
        window: ctx.content_rect(),
        pixels_per_point: ctx.pixels_per_point(),
        max_side: ctx.input(|i| i.max_texture_side),
    });
    app.shots.queued.push((token, now + 150.0, 0));
    ctx.request_repaint();
}

pub(crate) fn pending(app: &SolveApp) -> bool {
    app.issue_report.pending.is_some()
}

/// While a capture hides the report, what is typed waits for the form to come back, which gets
/// its focused field back: a screenshot taken once the preview is done can hide the form while
/// its user types (#7171).
pub(crate) fn hold_input(app: &mut SolveApp, raw: &mut egui::RawInput) {
    let state = &mut app.issue_report;
    if state.draft.is_none() {
        state.held.clear();
    } else if state.pending.is_some() {
        let mut kept = Vec::with_capacity(raw.events.len());
        for event in raw.events.drain(..) {
            if typing(&event) {
                if state.held.len() < MAX_HELD {
                    state.held.push(event);
                }
            } else {
                kept.push(event);
            }
        }
        raw.events = kept;
    } else if !state.held.is_empty() {
        let mut events = std::mem::take(&mut state.held);
        events.append(&mut raw.events);
        raw.events = events;
    }
}

/// What [`hold_input`] keeps for the form.
pub(crate) fn typing(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Text(_) | egui::Event::Key { .. } | egui::Event::Paste(_) | egui::Event::Copy | egui::Event::Cut | egui::Event::Ime(_)
    )
}

pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if let Some(waiting) = &app.issue_report.waiting {
        let settled = !app.preview.busy && app.shots.queued.len() < 16;
        let (late, context) = (crate::now_ms() >= waiting.deadline, waiting.context);
        if settled || late {
            app.issue_report.waiting = None;
            let dialog = app.dialog.clone();
            // The preview the report waited for is what it shows now.
            let source = source_signature(app, dialog.as_ref());
            if context_signature(app, dialog.as_ref()) == context
                && let Some(draft) = app.issue_report.draft.as_mut()
            {
                draft.source_signature = source;
            }
            capture_unless_busy(app, ctx, dialog.as_ref(), false);
            // The capture hides the form before it is drawn this frame: what was typed in it
            // waits with what comes while it is hidden.
            if pending(app) {
                let state = &mut app.issue_report;
                ctx.input(|i| {
                    for event in i.events.iter().filter(|e| typing(e)) {
                        if state.held.len() < MAX_HELD {
                            state.held.push(event.clone());
                        }
                    }
                });
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
    let problem = app.issue_report.pending.as_ref().and_then(|p| {
        if signature(app, app.dialog.as_ref()) != p.signature || p.window != ctx.content_rect() || p.pixels_per_point != ctx.pixels_per_point() {
            Some("The model, inputs or view changed during capture. Retake the current view.")
        } else if crate::now_ms() >= p.deadline {
            Some("No app frame was presented. Restore the window and retake, or continue with text.")
        } else {
            None
        }
    });
    if let Some(problem) = problem {
        cancel_capture(app);
        if let Some(draft) = app.issue_report.draft.as_mut() {
            draft.note = problem.into();
        }
    }
    if pending(app) {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

pub(crate) fn receive(app: &mut SolveApp, token: u64, image: Arc<ColorImage>) {
    let Some(max_side) = app.issue_report.pending.as_ref().filter(|p| p.token == token).map(|p| p.max_side) else {
        return;
    };
    let stale = app.issue_report.pending.as_ref().is_some_and(|p| p.signature != signature(app, app.dialog.as_ref()));
    let in_app = ticket::enabled(app);
    let ctx_width = app.issue_report.pending.as_ref().map_or(1.0, |p| p.window.width());
    let view_rect = app.viewport.rect.map(|r| r.translate(-app.issue_report.pending.as_ref().map_or(egui::Vec2::ZERO, |p| p.window.min.to_vec2())));
    app.issue_report.pending = None;
    if let Some(draft) = app.issue_report.draft.as_mut() {
        if stale {
            draft.note = "The model, inputs or view changed during capture. Retake the current view.".into();
            return;
        }
        match fitted(image, MAX_IMAGE_PIXELS, max_side) {
            Some(image) => {
                draft.view = in_app.then(|| {
                    let scale = image.size[0] as f32 / ctx_width.max(1.0);
                    ticket::crop(&image, view_rect, scale)
                });
                draft.image = Some(image);
                draft.texture = None;
                draft.note = draft.rebuilt.take().unwrap_or("Screenshot captured locally. Review it before attaching it on GitHub.").into();
            }
            None => draft.note = "Screenshot dimensions are unsupported. The text draft is still available.".into(),
        }
    }
}

/// `image` with at most `max` pixels and no side longer than `max_side`: a larger frame (a 6K
/// display, a window across several screens, #7091) is averaged down by the smallest whole
/// factor that fits. A preview texture over the GPU's limit fails its upload, and the window's
/// device panics on that. `None` only for a frame whose size doesn't match its pixels.
fn fitted(image: Arc<ColorImage>, max: usize, max_side: usize) -> Option<Arc<ColorImage>> {
    let [w, h] = image.size;
    let n = w.checked_mul(h)?;
    if n == 0 || n != image.pixels.len() || max == 0 || max_side == 0 {
        return None;
    }
    let fits = |k: usize| w.div_ceil(k) <= max_side && h.div_ceil(k) <= max_side && w.div_ceil(k).saturating_mul(h.div_ceil(k)) <= max;
    if fits(1) {
        return Some(image);
    }
    // Ends: at a factor of the longer side the frame is one pixel.
    let mut k = 2;
    while !fits(k) {
        k += 1;
    }
    let [ow, oh] = [w.div_ceil(k), h.div_ceil(k)];
    let mut pixels = Vec::with_capacity(ow * oh);
    for oy in 0..oh {
        let rows = oy * k..((oy + 1) * k).min(h);
        for ox in 0..ow {
            let cols = ox * k..((ox + 1) * k).min(w);
            let (mut sum, mut count) = ([0_u64; 4], 0_u64);
            for y in rows.clone() {
                for p in image.pixels.get(y * w + cols.start..y * w + cols.end).unwrap_or_default() {
                    for (s, c) in sum.iter_mut().zip(p.to_array()) {
                        *s += u64::from(c);
                    }
                    count += 1;
                }
            }
            let [r, g, b, a] = sum.map(|s| u8::try_from(s / count.max(1)).unwrap_or(u8::MAX));
            pixels.push(egui::Color32::from_rgba_premultiplied(r, g, b, a));
        }
    }
    Some(Arc::new(ColorImage::new([ow, oh], pixels)))
}

/// The report card is hidden only while a frame is captured, so it cannot cover the evidence.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if pending(app) {
        return;
    }
    if let Some(id) = app.issue_report.refocus.take() {
        ctx.memory_mut(|m| m.request_focus(id));
    }
    let Some(mut draft) = app.issue_report.draft.take() else { return };
    let (retake, close) = if ticket::enabled(app) && !draft.github { ticket::show(app, &mut draft, ctx) } else { form::show(app, &mut draft, ctx) };
    if !close {
        app.issue_report.draft = Some(draft);
        if retake {
            let dialog = app.dialog.clone();
            self::retake(app, ctx, dialog.as_ref());
        }
    } else {
        if let Some(answers) = Answers::edited(&draft) {
            app.issue_report.earlier = Some(answers);
        }
        cancel_capture(app);
        // Pointer release may have happened during review; never finalize it against
        // the report's last cursor position when CAD interaction resumes.
        if app.viewport.text_drag.is_some() {
            let _ = app.session.execute("sketch.drag_end", &serde_json::json!({}));
            crate::sketch_text_move::end_drag(app);
        }
        app.viewport.point_drag = None;
        app.viewport.boxsel = None;
        app.viewport.axis_drag = None;
        app.viewport.orbit_free = None;
        crate::dim_view::drag_end();
        crate::sketch3d::drag_end();
        crate::drag_snap::clear();
        crate::gizmo::cancel(ctx, egui::Id::unique("sc_move_triad"));
        crate::gizmo::cancel(ctx, egui::Id::unique("sc_occ_triad"));
        if let Some(focus) = app.issue_report.cad_focus.take() {
            ctx.memory_mut(|m| m.request_focus(focus));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use serde_json::json;
    use solidcraft_engine::{Sel, Session};

    fn app() -> SolveApp {
        SolveApp::new(Session::default(), Default::default())
    }

    fn combine(app: &mut SolveApp) {
        app.start("solid.combine");
        let dialog = app.dialog.as_mut().unwrap();
        dialog.kind = dialogs::Kind::Combine {
            operation: 2,
            keep_tools: true,
            offset: "0 mm".into(),
            round_edges: false,
            picked_faces: false,
            radius: String::new(),
        };
        dialog.inputs.get_mut(0).unwrap().items = vec![Sel::Body { name: "Stock".into() }];
        dialog.inputs.get_mut(1).unwrap().items = vec![Sel::Body { name: "Cutter".into() }];
    }

    fn click_commands(h: &mut crate::scenario::Harness, label: &str) -> Vec<egui::OutputCommand> {
        let pos = h.widget(label).unwrap();
        click_commands_at(h, pos)
    }

    fn click_commands_at(h: &mut crate::scenario::Harness, [x, y]: [f64; 2]) -> Vec<egui::OutputCommand> {
        assert_eq!(h.call("ui.move", json!({"x":x,"y":y}))["ok"], true);
        let (request, _reply) = crate::control::ControlRequest::new("ui.click", json!({"x":x,"y":y}));
        assert!(matches!(crate::control::handle(&mut h.app, &h.ctx, &request), crate::control::Outcome::Done(value) if value["ok"] == true));
        let mut commands = Vec::new();
        for _ in 0..12 {
            h.frame();
            commands.extend(h.output.commands.clone());
        }
        commands
    }

    fn open_commands(h: &mut crate::scenario::Harness) -> Vec<egui::OutputCommand> {
        let pos = crate::scenario::handle_at("dialog:ok");
        assert!(pos.is_some(), "the report primary action is drawn");
        pos.map_or_else(Vec::new, |pos| click_commands_at(h, [f64::from(pos.x), f64::from(pos.y)]))
    }

    #[test]
    fn issue_report_reopening_keeps_unfinished_answers_and_reviewed_context() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let mut draft = draft_for(&h.app, None, None);
        draft.title = "Edited summary".into();
        draft.description = "The report loses my notes".into();
        draft.steps = "Open the report twice".into();
        draft.expected = "Keep the notes".into();
        draft.body = "Reviewed and redacted diagnostics".into();
        let image = Arc::new(ColorImage::filled([8, 8], egui::Color32::GREEN));
        draft.image = Some(image.clone());
        let before = (draft.report_title(), draft.report_body());
        h.app.issue_report.draft = Some(draft);
        h.frames(4);
        let ctx = h.ctx.clone();
        begin(&mut h.app, &ctx, None, None);
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        assert_eq!((draft.report_title(), draft.report_body()), before);
        assert!(Arc::ptr_eq(draft.image.as_ref().unwrap(), &image));
        assert!(!pending(&h.app), "reopening must not replace the reviewed screenshot");
    }

    #[test]
    fn issue_report_both_close_controls_offer_to_restore_answers() {
        for (close, [width, height]) in [("dialog:cancel", [1280, 720]), ("dialog:close", [640, 480])] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":width,"height":height}));
            h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
            let draft = h.app.issue_report.draft.as_mut().unwrap();
            draft.title = "Saved summary".into();
            draft.steps = "Open, close and reopen the report".into();
            draft.expected = "Offer my answers again".into();
            draft.body = "Old diagnostics".into();
            draft.image = Some(Arc::new(ColorImage::filled([8, 8], egui::Color32::GREEN)));
            h.frames(8);
            h.step(&json!({"text":"Keep this description after closing"})).unwrap();
            h.step(&json!({"click":{"handle":close}})).unwrap();
            assert!(h.app.issue_report.draft.is_none(), "closing releases CAD input");
            let ctx = h.ctx.clone();
            begin(&mut h.app, &ctx, None, None);
            cancel_capture(&mut h.app);
            let fresh_body = h.app.issue_report.draft.as_ref().unwrap().body.clone();
            h.frames(8);
            assert!(h.widget("Restore answers").is_some(), "{close}: closed answers must remain available");
            let [x, y] = h.widget("Restore answers").unwrap();
            assert!((0.0..f64::from(width)).contains(&x) && (0.0..f64::from(height)).contains(&y), "the recovery offer is visible");
            h.step(&json!({"widget":"Restore answers"})).unwrap();
            let draft = h.app.issue_report.draft.as_ref().unwrap();
            assert_eq!(draft.description, "Keep this description after closing");
            assert_eq!(draft.title, "Saved summary");
            assert_eq!(draft.steps, "Open, close and reopen the report");
            assert_eq!(draft.expected, "Offer my answers again");
            assert_eq!(draft.body, fresh_body, "restoring answers keeps the new diagnostics");
            assert!(draft.image.is_none(), "the old screenshot does not belong to the new report");
            assert!(h.app.issue_report.earlier.is_none(), "restoring consumes the saved answers");
        }
    }

    #[test]
    fn issue_report_reopening_preserves_capture_and_held_typing() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
        h.frames(8);
        let ctx = h.ctx.clone();
        retake(&mut h.app, &ctx, None);
        let token = h.app.issue_report.pending.as_ref().unwrap().token;
        let body = h.app.issue_report.draft.as_ref().unwrap().body.clone();
        let mut input = egui::RawInput { events: vec![egui::Event::Text("Typed during capture".into())], ..Default::default() };
        hold_input(&mut h.app, &mut input);
        begin(&mut h.app, &ctx, None, None);
        begin(&mut h.app, &ctx, None, Some("A different error"));
        begin_crash(&mut h.app, &ctx, &json!({"message":"A different crash"}));
        assert_eq!(h.app.issue_report.pending.as_ref().unwrap().token, token);
        assert_eq!(h.app.issue_report.held.len(), 1);
        assert_eq!(h.app.issue_report.draft.as_ref().unwrap().body, body);
        cancel_capture(&mut h.app);
        h.frames(8);
        assert_eq!(h.app.issue_report.draft.as_ref().unwrap().description, "Typed during capture");
    }

    #[test]
    fn issue_report_recovery_never_overwrites_new_or_contextual_answers_and_can_be_discarded() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        combine(&mut h.app);
        let mut started = draft_for(&h.app, None, None);
        started.description = "A different report".into();
        for draft in [
            started,
            draft_for(&h.app, None, Some("New error")),
            draft_for(&h.app, h.app.dialog.as_ref(), None),
            crash_draft(&json!({"message":"New crash"})),
        ] {
            h.app.issue_report.earlier = Some(Answers { description: "Saved answer".into(), ..Default::default() });
            let before = draft.report_body();
            h.app.issue_report.draft = Some(draft);
            h.frames(8);
            assert!(h.widget("Restore answers").is_none());
            assert_eq!(h.app.issue_report.draft.as_ref().unwrap().report_body(), before);
            assert_eq!(h.app.issue_report.earlier.as_ref().unwrap().description, "Saved answer");
        }
        h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
        h.frames(8);
        h.step(&json!({"widget":"Discard answers"})).unwrap();
        assert!(h.app.issue_report.earlier.is_none());
        assert!(h.widget("Restore answers").is_none());
        assert!(h.app.issue_report.draft.as_ref().unwrap().description.is_empty());
    }

    #[test]
    fn issue_report_closing_empty_or_prefilled_drafts_keeps_earlier_answers() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.issue_report.earlier = Some(Answers { description: "Saved answer".into(), ..Default::default() });
        for draft in [
            draft_for(&h.app, None, None),
            draft_for(&h.app, None, Some("Generated error summary")),
            crash_draft(&json!({"message":"Generated crash summary"})),
        ] {
            h.app.issue_report.draft = Some(draft);
            h.frames(8);
            h.step(&json!({"click":{"handle":"dialog:close"}})).unwrap();
            assert!(h.app.issue_report.draft.is_none());
            assert_eq!(h.app.issue_report.earlier.as_ref().unwrap().description, "Saved answer");
        }
        let mut crash = crash_draft(&json!({"message":"Generated crash summary"}));
        crash.steps = "My typed reproduction".into();
        let answers = Answers::edited(&crash).unwrap();
        assert_eq!(answers.steps, "My typed reproduction");
        assert!(answers.title.is_empty() && answers.description.is_empty());
    }

    #[test]
    fn issue_report_browser_handoff_does_not_prove_submission_or_discard_answers() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
        h.frames(8);
        h.step(&json!({"text":"First report"})).unwrap();
        open_commands(&mut h);
        let ctx = h.ctx.clone();
        begin(&mut h.app, &ctx, None, None);
        assert_eq!(h.app.issue_report.draft.as_ref().unwrap().description, "First report");
        assert!(!pending(&h.app));
        h.step(&json!({"click":{"handle":"dialog:close"}})).unwrap();
        assert_eq!(h.app.issue_report.earlier.as_ref().unwrap().description, "First report");
        begin(&mut h.app, &ctx, None, None);
        cancel_capture(&mut h.app);
        h.frames(8);
        h.step(&json!({"widget":"Restore answers"})).unwrap();
        assert_eq!(h.app.issue_report.draft.as_ref().unwrap().description, "First report");
        assert!(!h.app.issue_report.draft.as_ref().unwrap().opened_github);
    }

    #[test]
    fn issue_report_explains_browser_submission_and_can_copy_an_incomplete_report() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let draft = draft_for(&h.app, None, None);
        assert!(draft.description.is_empty());
        let reviewed = draft.report_body();
        h.app.issue_report.draft = Some(draft);
        h.frames(16);
        for label in [
            "Open GitHub issue form",
            "Copy report",
            "SolidCraft cannot submit reports directly. Submit on GitHub; no token is needed in the app.",
            "If you are offline or GitHub does not open, use Copy report and keep the text to submit later.",
        ] {
            assert!(h.widget(label).is_some(), "missing submission action or explanation: {label}");
        }
        let before = (h.app.session.doc.to_json(), h.app.session.revision, h.app.session.undo.len());
        let commands = click_commands(&mut h, "Copy report");
        assert!(commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == &reviewed)));
        assert!(!commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(_))));
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        assert!(!draft.missing_description && !draft.opened_github);
        assert_eq!(draft.note, "Report copied. Keep this text to submit on GitHub when you are online.");
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.revision, h.app.session.undo.len()));
    }

    #[test]
    fn issue_report_short_handoff_keeps_a_copy_if_the_browser_cannot_open() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let mut draft = Draft::new("Preview missing".into(), "Synthetic diagnostics".into(), 0);
        draft.description = "The preview disappeared.".into();
        let reviewed = draft.report_body();
        let (url, copy_needed) = draft_url(&draft.report_title(), &reviewed);
        assert!(!copy_needed, "exercise a report small enough to prefill through the URL");
        h.app.issue_report.draft = Some(draft);
        h.frames(16);
        // The harness consumes no browser or clipboard outputs; a host failure must leave
        // both a complete copy request and the editable draft available for another attempt.
        let commands = open_commands(&mut h);
        assert!(commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == &reviewed)));
        assert!(commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(open) if open.url == url)));
        assert_eq!(h.app.issue_report.draft.as_ref().unwrap().report_body(), reviewed);
        let commands = click_commands(&mut h, "Copy report");
        assert!(commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == &reviewed)));
    }

    #[test]
    fn issue_report_compact_footer_keeps_both_export_actions_visible_and_clickable() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.ui_scale = 2.0;
        h.call("ui.resize", json!({"width":640,"height":480}));
        let mut draft = Draft::new("Preview missing".into(), "Synthetic diagnostics".into(), 0);
        draft.description = "The preview disappeared.".into();
        let reviewed = draft.report_body();
        h.app.issue_report.draft = Some(draft);
        h.frames(20);
        let footer = crate::scenario::rect_of("dialog:footer").unwrap();
        assert!(h.ctx.content_rect().contains_rect(footer), "the footer must fit the viewport: {footer:?}");
        for target in ["dialog:ok", "dialog:apply"] {
            let rect = crate::scenario::rect_of(target).unwrap();
            assert!(footer.contains_rect(rect), "{target} is clipped: {rect:?} outside {footer:?}");
        }
        let commands = click_commands(&mut h, "Copy report");
        assert!(commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == &reviewed)));
        let commands = click_commands(&mut h, "Open GitHub");
        assert!(commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(url) if url.url.starts_with(ISSUE_URL))));
        click_commands(&mut h, "Close");
        assert!(h.app.issue_report.draft.is_none(), "the header close remains available");
    }

    #[test]
    fn issue_report_title_uses_sketch_workspace_and_description() {
        let mut app = app();
        app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let mut draft = draft_for(&app, None, None);
        assert!(draft.title.is_empty());
        draft.description = "  Line disappears after drawing\nMore details".into();
        assert_eq!(draft.report_title(), "Sketch: Line disappears after drawing");
        assert!(!draft.report_title().contains("unexpected result"));
        draft.title = "Custom summary".into();
        assert_eq!(draft.report_title(), "Custom summary");
    }

    #[test]
    fn issue_report_title_preserves_recorded_failure() {
        let mut app = app();
        app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let mut draft = draft_for(&app, None, Some("Synthetic command error"));
        draft.description = "Line could not be drawn".into();
        assert_eq!(draft.report_title(), "Sketch: operation failed");
    }

    #[test]
    fn issue_report_form_separates_user_answers_from_technical_context() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
        h.frames(4);
        for label in ["What happened?", "Steps to reproduce (optional)", "Expected result (optional)", "Technical details", "Open GitHub issue form"]
        {
            assert!(h.widget(label).is_some(), "missing report form control: {label}");
        }
        assert!(h.widget("Describe the intended result.").is_none(), "Markdown instructions must not be editable answers");
    }

    #[test]
    fn issue_report_answers_preserve_the_reviewed_context_within_the_export_budget() {
        let mut draft = draft_for(&app(), None, None);
        draft.description = "a".repeat(MAX_ANSWER);
        draft.steps = "b".repeat(MAX_ANSWER);
        draft.expected = "c".repeat(MAX_ANSWER);
        draft.body = format!("{}END_CONTEXT", "d".repeat(MAX_BODY - 11));
        let report = draft.report_body();
        assert!(report.len() <= MAX_REPORT && report.ends_with(&draft.body));
        for answer in [&draft.description, &draft.steps, &draft.expected] {
            assert!(report.contains(answer));
        }
        draft.description = "現象🚀".repeat(MAX_REPORT);
        draft.body = "調査🚀".repeat(MAX_REPORT);
        let bounded = draft.report_body();
        assert!(bounded.len() <= MAX_REPORT && bounded.ends_with("[technical details truncated]\n"));
    }

    /// An answer keeps 2,000 characters in any script (#7191): the limit counted bytes, so one in
    /// Chinese kept 666. Wherever it is cut, no character is split.
    #[test]
    fn issue_report_answers_keep_2000_characters_in_any_script() {
        let mut draft = draft_for(&app(), None, None);
        draft.body = format!("{}END_CONTEXT", "d".repeat(MAX_BODY - 11));
        let mixed = |n: usize| "a現🚀é".chars().cycle().take(n).collect::<String>();
        for n in [MAX_ANSWER - 1, MAX_ANSWER, MAX_ANSWER + 1, MAX_ANSWER + 2, MAX_ANSWER + 3, 3000] {
            (draft.description, draft.steps, draft.expected) = (mixed(n), "現".repeat(n), "🚀".repeat(n));
            let kept = n.min(MAX_ANSWER);
            let answers = format!(
                "## Actual behavior\n{}\n\n## Reproduction\n{}\n\n## Expected behavior\n{}\n\n",
                mixed(kept),
                "現".repeat(kept),
                "🚀".repeat(kept)
            );
            let report = draft.report_body();
            assert!(report.starts_with(&answers), "{n} characters");
            assert!(report.len() <= MAX_REPORT && report.ends_with(&draft.body), "{n} characters: the technical details still fit");
        }
    }

    /// Near the limit, an answer says how many characters are left: a paste past it is visibly
    /// cut (#7191).
    #[test]
    fn issue_report_answer_near_its_limit_says_how_many_characters_are_left() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
        h.frames(16);
        h.ctx.memory_mut(|m| m.request_focus(egui::Id::unique("issue_report_body")));
        h.frames(1);
        h.frame_with(vec![egui::Event::Paste("故障".repeat(1500))]);
        h.frames(4);
        let draft = h.app.issue_report.draft.as_mut().unwrap();
        assert_eq!(draft.description, "故障".repeat(MAX_ANSWER / 2), "3,000 Chinese characters pasted, 2,000 kept");
        // An answer that gets past the limit some other way is cut there too, between characters.
        let mixed = |n: usize| "a現🚀é".chars().cycle().take(n).collect::<String>();
        (draft.steps, draft.expected) = ("步".repeat(MAX_ANSWER - 150), mixed(MAX_ANSWER + 3));
        h.frames(4);
        assert_eq!(h.app.issue_report.draft.as_ref().unwrap().expected, mixed(MAX_ANSWER));
        for note in ["0 characters left", "150 characters left"] {
            assert!(h.widget(note).is_some(), "{note}");
        }
        h.app.issue_report.draft.as_mut().unwrap().steps = "步".repeat(MAX_ANSWER - 1);
        h.frames(4);
        assert!(h.widget("1 character left").is_some());
        h.app.issue_report.draft.as_mut().unwrap().steps = "步".repeat(MAX_ANSWER - 200);
        h.frames(4);
        assert!(h.widget("1 character left").is_none() && h.widget("200 characters left").is_none(), "no note far from the limit");
    }

    #[test]
    fn issue_report_requires_an_answer_before_browser_handoff_without_changing_the_design() {
        for (size, scale) in [([1600, 1000], 1.0), ([640, 480], 2.0)] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.ui_scale = scale;
            h.call("ui.resize", json!({"width":size[0],"height":size[1]}));
            h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
            h.frames(16);
            let before = (h.app.session.doc.to_json(), h.app.session.revision, h.app.session.undo.len());
            let commands = open_commands(&mut h);
            assert!(!commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(_))));
            assert!(h.app.issue_report.draft.as_ref().unwrap().missing_description);
            assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::unique("issue_report_body")));
            h.step(&json!({"text":"The preview disappeared"})).unwrap();
            let commands = open_commands(&mut h);
            assert!(commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(url) if url.url.starts_with(ISSUE_URL))));
            assert!(h.app.issue_report.draft.as_ref().unwrap().description.contains("The preview disappeared"));
            assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.revision, h.app.session.undo.len()));
        }
    }

    /// Opening GitHub with an empty description gives it the focus once (#7190). It used to take the
    /// focus back every frame, so what was typed in Summary went into the description.
    #[test]
    fn issue_report_empty_description_takes_the_focus_once() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
        h.frames(16);
        let description = Some(egui::Id::unique("issue_report_body"));
        open_commands(&mut h);
        assert_eq!(h.ctx.memory(|m| m.focused()), description);
        // Click the Summary field (found by its id: it has no label of its own) and type.
        let title = egui::Id::unique("issue_report_title").accesskit_id();
        let tree = h.output.accesskit_update.as_ref().unwrap();
        let b = tree.nodes.iter().find(|(id, _)| *id == title).and_then(|(_, n)| n.bounds()).unwrap();
        h.step(&json!({"click":[(b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0]})).unwrap();
        h.step(&json!({"text":"My title"})).unwrap();
        h.frames(4);
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        assert_eq!((draft.title.as_str(), draft.description.as_str()), ("My title", ""));
        assert!(draft.missing_description && h.widget("Describe what happened before continuing.").is_some());
        // Still empty: opening GitHub again gives it the focus again.
        open_commands(&mut h);
        assert_eq!(h.ctx.memory(|m| m.focused()), description);
    }

    #[test]
    fn issue_report_handoff_includes_edits_from_the_click_frame() {
        for field in ["issue_report_body", "issue_report_title", "issue_report_steps", "issue_report_expected", "issue_report_technical"] {
            for clipboard in [false, true] {
                let mut h = crate::scenario::Harness::new();
                h.app.home.open = false;
                let mut draft = Draft::new("Previous summary".into(), "Private diagnostic details".into(), 0);
                if field != "issue_report_body" {
                    draft.description = "Previously reviewed problem".into();
                }
                if clipboard {
                    draft.steps = "Reproduction context. ".repeat(100);
                }
                h.app.issue_report.draft = Some(draft);
                h.frames(16);
                if field == "issue_report_technical" {
                    h.call("ui.scroll", json!({"x":800,"y":600,"delta":-1000}));
                    h.step(&json!({"widget":"Technical details"})).unwrap();
                    h.call("ui.scroll", json!({"x":800,"y":600,"delta":-1000}));
                }
                h.ctx.memory_mut(|m| m.request_focus(egui::Id::unique(field)));
                h.frames(1);
                let [x, y] = h.widget("Open GitHub issue form").unwrap();
                let pos = egui::pos2(x as f32, y as f32);
                let modifiers = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
                h.frame_with(vec![
                    egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers },
                    egui::Event::Paste("Latest reviewed content".into()),
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
                ]);
                // The click waits one frame for the typing before it.
                h.frame();
                let draft = h.app.issue_report.draft.as_ref().unwrap();
                let edited = match field {
                    "issue_report_body" => &draft.description,
                    "issue_report_title" => &draft.title,
                    "issue_report_steps" => &draft.steps,
                    "issue_report_expected" => &draft.expected,
                    _ => &draft.body,
                };
                assert_eq!(edited, "Latest reviewed content", "{field}, clipboard={clipboard}");
                let body = draft.report_body();
                let (url, copy_needed) = draft_url(&draft.report_title(), &body);
                assert!(
                    h.output.commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(u) if u.url == url)),
                    "handoff must use the click frame's edits: {field}, clipboard={clipboard}"
                );
                if copy_needed {
                    assert!(h.output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == &body)));
                }
                assert!(draft.opened_github && !draft.missing_description);
            }
        }
    }

    /// Ctrl+Enter (⌘Enter on a Mac) continues on GitHub from a field, without a line break, and
    /// asks for a description first like the button does (#7203). Holding it opens GitHub once.
    #[test]
    fn issue_report_ctrl_enter_continues_on_github() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
        h.frames(8);
        assert!(h.widget(form::shortcut_hint(h.ctx.os())).is_some(), "the dialog says so");
        // As the platforms send it: ⌘ sets mac_cmd and command, Ctrl elsewhere ctrl and command.
        // egui marks a press of a key it already holds down as a repeat.
        let key = |pressed: bool| {
            let modifiers = if h.ctx.os() == egui::os::OperatingSystem::Mac {
                egui::Modifiers { mac_cmd: true, command: true, ..Default::default() }
            } else {
                egui::Modifiers { ctrl: true, command: true, ..Default::default() }
            };
            egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers }
        };
        let (down, up) = (key(true), key(false));
        // Held for `repeats` frames after the press, then released.
        let press = |h: &mut crate::scenario::Harness, repeats: usize| {
            let mut commands = Vec::new();
            for _ in 0..=repeats {
                h.frame_with(vec![down.clone()]);
                commands.extend(h.output.commands.clone());
            }
            h.frame_with(vec![up.clone()]);
            commands.extend(h.output.commands.clone());
            h.frames(2);
            commands.extend(h.output.commands.clone());
            commands
        };
        let commands = press(&mut h, 0);
        assert!(!commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(_))));
        assert!(h.app.issue_report.draft.as_ref().unwrap().missing_description, "an empty description is asked for");
        h.frame_with(vec![egui::Event::Text("The preview disappeared".into())]);
        // Held down, it continues on GitHub once: its repeats open nothing more.
        let commands = press(&mut h, 3);
        let opened = commands.iter().filter(|c| matches!(c, egui::OutputCommand::OpenUrl(url) if url.url.starts_with(ISSUE_URL))).count();
        assert_eq!(opened, 1, "{commands:?}");
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        assert_eq!(draft.description, "The preview disappeared", "no line break");
        assert!(draft.opened_github);
    }

    #[test]
    fn crash_draft_marks_later_detection_without_losing_native_evidence() {
        let message =
            "SolidCraft 0.3.2 (started 2026-10-01T10:00:00Z) crashed with ACCESS_VIOLATION (0xc0000005) in driver.dll+0x1234 between commands";
        let frame = "0xc0000005 ACCESS_VIOLATION at driver.dll+0x1234";
        let report = json!({
            "kind": "unclean_exit", "count": 2, "first_seen": "2026-10-12T16:23:41Z",
            "message": message, "location": "native exception in driver.dll+0x1234",
            "backtrace": [frame], "native": {"code": "0xc0000005", "module": "driver.dll", "offset": "0x1234"},
        });
        let draft = crash_draft(&report);
        for expected in [
            "Recorded 2 time(s), first recorded 2026-10-12T16:23:41Z",
            "Detected on a later launch; the exact exit time was not recorded.",
            "A system shutdown or forced stop can also cause an unclean exit.",
            message,
            frame,
        ] {
            assert!(draft.body.contains(expected), "missing {expected}: {}", draft.body);
        }
        assert!(!draft.body.contains("Happened 2 time(s)"));
    }

    #[test]
    fn crash_draft_only_explains_later_detection_for_unclean_exits() {
        for kind in ["command_panic", "panic", "kernel_panic_recovered"] {
            let report = json!({"kind": kind, "count": 1, "first_seen": "2026-10-12T16:23:41Z"});
            let draft = crash_draft(&report);
            assert!(draft.body.contains("Recorded 1 time(s), first recorded 2026-10-12T16:23:41Z"), "{kind}: {}", draft.body);
            assert!(!draft.body.contains("Detected on a later launch"), "{kind}: {}", draft.body);
            assert!(!draft.body.contains("exact exit time was not recorded"), "{kind}: {}", draft.body);
        }
    }

    #[test]
    fn issue_report_actual_pending_boolean_settings_and_failure_are_read_only() {
        let mut app = app();
        combine(&mut app);
        let before = (app.session.doc.to_json(), app.session.revision, app.session.undo.len(), app.session.redo.len(), app.cam);
        let draft = draft_for(&app, app.dialog.as_ref(), Some("Boolean tool is outside target"));
        for expected in
            ["solid.combine", "Stock", "Cutter", "cut", "keep_tools", "true", "Boolean tool is outside target", help::VERSION, "last valid geometry"]
        {
            assert!(draft.body.contains(expected), "missing {expected}: {}", draft.body);
        }
        assert_eq!(before, (app.session.doc.to_json(), app.session.revision, app.session.undo.len(), app.session.redo.len(), app.cam));
    }

    /// The sample with Fillet1 failing (the plate made narrower than its fillets).
    fn failing_sample() -> SolveApp {
        let mut s = Session::default();
        s.run_script(&solidcraft_engine::sample::script()).unwrap();
        s.execute("parameters.change", &json!({"name": "width", "expression": "5 mm"})).unwrap();
        SolveApp::new(s, Default::default())
    }

    #[test]
    fn issue_report_lists_the_last_messages_and_a_failing_feature_that_is_not_the_latest() {
        let mut app = failing_sample();
        let fillet = app.session.doc.features.iter().find(|f| f.name == "Fillet1").unwrap();
        let error = app.session.model.result(fillet.id).and_then(|r| r.error.clone()).unwrap();
        let latest = app.session.doc.features.iter().rev().find(|f| reportable(f)).unwrap();
        assert_ne!(latest.id, fillet.id, "Fillet1 must not be the latest operation");
        assert!(!app.session.has_selection());
        let at = crate::now_ms() - 125_000.0;
        for i in 0..12 {
            app.messages.push(&format!("message number {i:02}"), i % 2 == 0, at);
        }
        app.messages.push("message number 11", false, at);
        let draft = draft_for(&app, None, None);
        let line = format!("Feature {} \"Fillet1\" (FilletFeature): {:?}", fillet.id, error);
        assert!(draft.body.contains("Failing features (1):") && draft.body.contains(&line), "missing {line}: {}", draft.body);
        assert!(draft.body.contains("Messages (last 8 of 12):"), "{}", draft.body);
        for i in 0..4 {
            assert!(!draft.body.contains(&format!("message number {i:02}")), "only the last 8 messages: {}", draft.body);
        }
        assert!(draft.body.contains("2 min ago error: \"message number 10\"\n"), "{}", draft.body);
        assert!(draft.body.contains("2 min ago info: \"message number 11\" (×2)\n"), "{}", draft.body);
        let latest = draft.body.find("Latest modeling operation").unwrap();
        assert!(latest < draft.body.find("Failing features").unwrap() && latest < draft.body.find("Messages (").unwrap());
        assert!(draft.body.find("Messages (").unwrap() < draft.body.find("## Environment").unwrap());
    }

    #[test]
    fn issue_report_messages_and_feature_errors_hide_the_home_folder_and_stay_bounded() {
        assert_eq!(report_text("Could not write /home/sample/Designs/part.step", Some("/home/sample")), "\"Could not write ~/Designs/part.step\"");
        assert_eq!(report_text("/home/sample", Some("/")), "\"/home/sample\"", "a bare root is never replaced");
        let mut app = app();
        if let Some(home) = home_text() {
            app.messages.push(&format!("Export failed: {home}/Designs/part.step is read-only"), true, crate::now_ms());
            let draft = draft_for(&app, None, None);
            assert!(draft.body.contains("error: \"Export failed: ~/Designs/part.step is read-only\""), "{}", draft.body);
            assert!(!draft.body.contains(&home), "the home folder leaks: {}", draft.body);
        }
        for i in 0..12 {
            app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":format!("Box{i}")})).unwrap();
        }
        let hostile = "錯誤 ```\n## fake section 🚀".repeat(50_000);
        for result in &mut app.session.model.results {
            result.error = Some(hostile.clone());
        }
        app.messages.push(&hostile, true, f64::NAN);
        let draft = draft_for(&app, None, None);
        assert!(draft.body.len() <= MAX_BODY);
        assert!(draft.body.contains("Failing features (first 10 of 12):"), "{}", draft.body);
        assert!(!draft.body.contains("\n## fake section"), "untrusted text stays on one quoted line");
        let section = draft.body.split("Failing features").nth(1).unwrap();
        let section = section.split("\n## Environment").next().unwrap();
        assert_eq!(section.lines().filter(|l| l.contains("錯誤")).count(), 11, "ten features and one message");
        for line in section.lines() {
            assert!(line.chars().count() < 2 * MAX_ENTRY_CHARS, "an entry is bounded: {} characters", line.chars().count());
        }
        assert!(section.contains("0 s ago error: \"錯誤"), "a broken time still reads: {section}");
    }

    /// The query of `url` as GitHub reads it: names and values, `+` a space.
    fn query(url: &str) -> Vec<(String, String)> {
        let decode = |v: &str| {
            let (mut bytes, mut it) = (Vec::new(), v.bytes());
            while let Some(b) = it.next() {
                bytes.push(match b {
                    b'+' => b' ',
                    b'%' => u8::from_str_radix(&String::from_utf8(vec![it.next().unwrap(), it.next().unwrap()]).unwrap(), 16).unwrap(),
                    b => b,
                });
            }
            String::from_utf8(bytes).unwrap()
        };
        let (_, query) = url.split_once('?').unwrap();
        query.split('&').map(|pair| pair.split_once('=').map(|(k, v)| (decode(k), decode(v))).unwrap()).collect()
    }

    /// A typical report goes in the link whole (#7103, #10556). The link took at most 2,000
    /// characters, with every space and line break taking three: most reports didn't fit, and
    /// GitHub opened with the title alone.
    #[test]
    fn issue_report_link_carries_a_typical_report_whole_and_exactly() {
        let mut app = app();
        app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        let mut draft = draft_for(&app, None, None);
        draft.description = "The fillet fails: \"radius 6 > 5\" & the preview shows #3 + 50% of the edge 🚀\n".repeat(8);
        draft.steps = "1. Sketch a circle on XY\n2. Extrude it 10 mm\n3. Fillet the top edge with 6 mm".into();
        let body = draft.report_body();
        assert!(body.len() >= 1600, "as long as a typical report: {}", body.len());
        let (url, too_long) = draft_url(&draft.report_title(), &body);
        assert!(!too_long, "the report fits the link: {}", url.len());
        assert_eq!(query(&url), [("title".to_string(), draft.report_title()), ("body".to_string(), body)]);
    }

    /// GitHub refuses a request whose link and headers pass about 7,000 bytes, and someone who
    /// isn't signed in first goes to sign in with the link encoded once more (#20510). A report too
    /// long for that goes through the clipboard, never cut.
    #[test]
    fn issue_report_link_stays_within_what_github_keeps() {
        for chunk in ["word ", "x", "ünïcødé ", "a&b=c#d+e%f\"g'\n"] {
            let (mut fitted, mut refused) = (false, false);
            for n in (0..8000).step_by(97) {
                let body = chunk.repeat(n / chunk.len() + 1);
                let (url, too_long) = draft_url("Fillet fails", &body);
                if too_long {
                    assert!(!url.contains("&body="), "{chunk:?}");
                    refused = true;
                } else {
                    assert!(url.len() <= MAX_URL && sign_in_length(&url) <= MAX_SIGN_IN_URL, "{chunk:?} × {n}");
                    // A sign-in link of 6,940 characters got "Whoops" with a browser's headers.
                    assert!(SIGN_IN_URL.len() + sign_in_length(&url) <= 4000, "{chunk:?} × {n}");
                    assert_eq!(query(&url), [("title".to_string(), "Fillet fails".to_string()), ("body".to_string(), body)]);
                    fitted = true;
                }
            }
            assert!(fitted && refused, "{chunk:?}");
        }
    }

    #[test]
    fn issue_report_hostile_context_and_url_are_bounded_without_new_query_parameters() {
        let mut app = app();
        combine(&mut app);
        let hostile = "錯誤 ```\n## fake &labels=secret%20tag 🚀".repeat(40_000);
        let draft = draft_for(&app, app.dialog.as_ref(), Some(&hostile));
        assert!(draft.body.len() <= MAX_BODY);
        assert!(draft.body.contains("[truncated]"));
        let (url, copy) = draft_url("Cut &labels=secret #? 🚀", "actual\n```\n&assignees=someone");
        assert!(!copy);
        assert!(url.starts_with(ISSUE_URL));
        assert_eq!(url.matches('&').count(), 1);
        assert!(url.contains("%26labels%3Dsecret") && url.contains("%F0%9F%9A%80"));
        let (short, copy) = draft_url(&hostile, &draft.body);
        assert!(copy && short.len() <= MAX_URL && !short.contains("&body="));
        assert!(draft.body.contains("Stock"), "fallback must retain the editable full report");
    }

    #[test]
    fn issue_report_help_supports_successful_geometry_without_mutating_it() {
        let mut app = app();
        app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Stock"})).unwrap();
        app.run("solid.box", json!({"length":5,"width":5,"height":5,"body_name":"Cutter"})).unwrap();
        app.run("solid.combine", json!({"target":"Stock","tools":["Cutter"],"operation":"cut","keep_tools":true})).unwrap();
        assert!(app.session.model.results.iter().all(|r| r.error.is_none()));
        let before = (app.session.doc.to_json(), app.session.undo.len(), app.session.redo.len(), app.cam);
        let ctx = crate::fonts::test_context();
        help::run(&mut app, &ctx, "report");
        let draft = app.issue_report.draft.as_ref().unwrap();
        assert!(draft.description.is_empty(), "user answers start empty, without template instructions");
        assert!(draft.body.contains(help::COMMIT));
        assert!(draft.body.contains("combine") && draft.body.contains("keep_tools") && draft.body.contains("Cutter"));
        assert!(pending(&app), "Help must request real evidence, not just copy generic text");
        assert_eq!(before, (app.session.doc.to_json(), app.session.undo.len(), app.session.redo.len(), app.cam));
    }

    #[test]
    fn issue_report_selected_and_recent_sketch_geometry_is_not_exported() {
        let mut app = app();
        app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"PublicExample"})).unwrap();
        let created = app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let sketch = created["sketch"].as_u64().unwrap();
        app.run("sketch.point", json!({"point":[123456.789123,987654.321987]})).unwrap();
        app.run("sketch.finish", json!({})).unwrap();
        app.session.selection = vec![Sel::Feature { id: sketch }];
        let stored = app.session.doc.to_json();
        assert!(stored.contains("123456.789123") && stored.contains("987654.321987"));
        let draft = draft_for(&app, None, None);
        assert!(
            draft.body.contains("\"type\": \"box\"") && draft.body.contains("\"length\""),
            "keep the relevant last modeling settings: {}",
            draft.body
        );
        assert!(!draft.body.contains("123456.789123") && !draft.body.contains("987654.321987"));
        assert!(!draft.body.contains("\"points\""), "never serialize the selected or recent sketch point list");
        assert_eq!(stored, app.session.doc.to_json());
    }

    #[test]
    fn issue_report_error_row_click_opens_the_actual_boolean_draft() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Stock"})).unwrap();
        h.app.home.open = false;
        combine(&mut h.app);
        let error = h.app.run("solid.combine", json!({"target":"Stock","tools":["Cutter"],"operation":"cut","keep_tools":true})).unwrap_err();
        h.app.dialog.as_mut().unwrap().error = Some(error.clone());
        h.frames(3);
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len());
        h.step(&json!({"widget":"Create Issue…"})).unwrap();
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        assert!(draft.body.contains("solid.combine") && draft.body.contains("Cutter") && draft.body.contains(&format!("{error:?}")));
        assert!(h.app.dialog.is_some(), "reporting must not cancel the live feature dialog");
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len()));
    }

    #[test]
    fn issue_report_long_draft_open_click_copies_the_full_reviewed_text() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let mut draft = draft_for(&h.app, None, None);
        draft.body = "Reviewed context 🚀 &labels=private\n".repeat(120);
        draft.description = "The preview disappeared after clicking Extrude.".into();
        draft.steps = "1. Select a face\n2. Start Extrude".into();
        draft.expected = "A preview of the extrusion".into();
        draft.image = Some(Arc::new(ColorImage::new([2, 2], vec![egui::Color32::GREEN; 4])));
        let reviewed = draft.report_body();
        h.app.issue_report.draft = Some(draft);
        h.frames(16);
        assert!(h.widget("Opens your browser and copies the report text for you to paste into GitHub.").is_some());
        assert!(h.widget("Reports are public. A GitHub account is required.").is_some());
        let commands = open_commands(&mut h);
        assert!(commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text.starts_with(&reviewed) && text.contains("drag the PNG file") && text.contains(".png"))), "commands: {commands:?}");
        assert!(
            commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(url) if url.url.starts_with(ISSUE_URL) && !url.url.contains("&body=")))
        );
        assert_eq!(h.app.issue_report.draft.as_ref().unwrap().report_body(), reviewed);
        assert!(h.app.issue_report.draft.as_ref().unwrap().opened_github);
        click_commands(&mut h, "Copy screenshot");
        assert!(!h.app.issue_report.draft.as_ref().unwrap().opened_github, "copying an image replaces the report text on the clipboard");
        assert!(h.widget("Opens your browser and copies the report text for you to paste into GitHub.").is_some());
        open_commands(&mut h);
        click_commands(&mut h, "Remove screenshot");
        assert!(h.app.issue_report.draft.as_ref().unwrap().image.is_none());
        assert!(h.widget("Screenshot removed from this draft.").is_some());
        assert!(h.widget("Report text copied. Paste it into GitHub and submit your report there.").is_some());
    }

    #[test]
    fn issue_report_blank_summary_uses_the_export_title_for_clipboard_guidance() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let mut draft = draft_for(&h.app, None, None);
        draft.title.clear();
        draft.description = "d".repeat(160);
        // Just long enough that the title drawn from the description takes the link over.
        for n in (0..MAX_URL).step_by(16) {
            draft.body = "x".repeat(n);
            if draft_url(&draft.report_title(), &draft.report_body()).1 {
                break;
            }
        }
        let reviewed = draft.report_body();
        assert!(!draft_url("", &reviewed).1, "the fallback title must be what crosses the URL limit");
        assert!(draft_url(&draft.report_title(), &reviewed).1);
        h.app.issue_report.draft = Some(draft);
        h.frames(16);
        assert!(h.widget("Opens your browser and copies the report text for you to paste into GitHub.").is_some());
        let commands = open_commands(&mut h);
        assert!(commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == &reviewed)));
        assert!(h.widget("Report text copied. Paste it into GitHub and submit your report there.").is_some());
    }

    #[test]
    fn issue_report_idle_window_does_not_grow_with_a_screenshot() {
        for size in [[1600, 1000], [900, 700], [640, 480]] {
            for scale in [1.0, 1.25, 1.4, 2.0] {
                let mut h = crate::scenario::Harness::new();
                h.app.preferences.ui_scale = scale;
                h.app.home.open = false;
                h.call("ui.resize", json!({"width":size[0],"height":size[1]}));
                let mut draft = draft_for(&h.app, None, None);
                draft.note = "Screenshot captured locally. Review it before attaching it on GitHub.".into();
                draft.image = Some(Arc::new(ColorImage::new([1600, 1000], vec![egui::Color32::GREEN; 1_600_000])));
                h.app.issue_report.draft = Some(draft);
                h.frames(20);
                let rect =
                    |h: &crate::scenario::Harness| egui::containers::AreaState::load(&h.ctx, egui::Id::unique("sc_issue_report")).unwrap().rect();
                let settled = rect(&h);
                h.frames(120);
                let idle = rect(&h);
                assert!((idle.width() - settled.width()).abs() < 1.0, "size={size:?}: {settled:?} grew to {idle:?}");
                assert!(idle.width() <= 790.0, "report must retain its width limit: {idle:?}");
                assert!(h.ctx.content_rect().shrink(8.0).contains_rect(idle), "size={size:?} scale={scale}: retain an outer margin: {idle:?}");
                let open_label = if h.widget("Open GitHub issue form").is_some() { "Open GitHub issue form" } else { "Open GitHub" };
                for label in [open_label, "Copy report", "Close"] {
                    let [x, y] = h.widget(label).unwrap();
                    assert!(idle.contains(egui::pos2(x as f32, y as f32)), "{label} must stay visible: {idle:?}, {x}, {y}");
                }
            }
        }
    }

    /// #9916: the window opens at the top middle, and dragging its title band moves it off the
    /// model, in a sketch too.
    #[test]
    fn issue_report_window_moves_by_its_title() {
        for sketch in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1400,"height":800}));
            if sketch {
                h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
            }
            h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
            h.frames(10);
            let rect = |h: &crate::scenario::Harness| egui::containers::AreaState::load(&h.ctx, egui::Id::unique("sc_issue_report")).unwrap().rect();
            let at = rect(&h);
            assert!((at.center().x - h.ctx.content_rect().center().x).abs() < 1.0, "sketch={sketch}: it opens at the top middle: {at:?}");
            // On the title, clear of the close button.
            let grip = at.left_top() + egui::vec2(140.0, 18.0);
            h.call("ui.drag", json!({"x0": grip.x, "y0": grip.y, "x1": grip.x - 250.0, "y1": grip.y, "steps": 20}));
            h.frames(4);
            let moved = rect(&h);
            assert!((moved.min.x - (at.min.x - 250.0)).abs() < 2.0 && (moved.min.y - at.min.y).abs() < 2.0, "sketch={sketch}: {at:?} -> {moved:?}");
            assert!(h.app.issue_report.draft.is_some(), "still open");
        }
    }

    /// Save screenshot… writes the PNG where the picker says, the way the web app's other saves
    /// write (#7192), and a failure reads as plain words, not a Rust value.
    #[test]
    fn issue_report_save_screenshot_writes_the_png_and_explains_a_failure() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let mut draft = draft_for(&h.app, None, None);
        draft.image = Some(Arc::new(ColorImage::new([4, 3], vec![egui::Color32::GREEN; 12])));
        h.app.issue_report.draft = Some(draft);
        h.frames(4);
        // The screenshot's buttons sit low in the review's scroll area: scrolled into view first.
        h.call("ui.scroll", json!({"x":800,"y":600,"delta":-1000}));
        h.frames(2);
        for (path, saved) in [(h.dir.join("report.png"), true), (h.dir.join("missing").join("report.png"), false)] {
            let answer = path.to_string_lossy().to_string();
            h.app.services.pick_save = Some(Box::new(move |_, _| Box::pin(std::future::ready(Some(answer.clone())))));
            click_commands(&mut h, "Save screenshot…");
            let note = h.app.issue_report.draft.as_ref().unwrap().note.clone();
            if saved {
                let png = image::load_from_memory_with_format(&std::fs::read(&path).unwrap(), image::ImageFormat::Png).unwrap();
                assert_eq!(png.into_rgba8().dimensions(), (4, 3));
                assert_eq!(note, "Screenshot saved. Attach the PNG in the GitHub draft.");
            } else {
                let why = std::fs::write(&path, b"").unwrap_err();
                assert_eq!(note, format!("Could not save screenshot: {why}"));
            }
        }
    }

    #[test]
    fn issue_report_short_screenshot_handoff_copies_the_image_and_explains_pasting() {
        for (os, shortcut) in [(egui::os::OperatingSystem::Nix, "Ctrl+V"), (egui::os::OperatingSystem::Mac, "Cmd+V")] {
            let mut h = crate::scenario::Harness::new();
            h.ctx.set_os(os);
            h.app.home.open = false;
            let mut draft = draft_for(&h.app, None, None);
            draft.description = "The preview disappeared.".into();
            let image = Arc::new(ColorImage::new([4, 3], vec![egui::Color32::GREEN; 12]));
            draft.image = Some(image.clone());
            let reviewed = draft.report_body();
            h.app.issue_report.draft = Some(draft);
            h.frames(16);
            let commands = open_commands(&mut h);
            assert!(commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyImage(copied) if copied == image.as_ref())));
            assert!(!commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(_))));
            let url = commands
                .iter()
                .find_map(|c| match c {
                    egui::OutputCommand::OpenUrl(url) => Some(&url.url),
                    _ => None,
                })
                .unwrap();
            let body = query(url).into_iter().find(|(key, _)| key == "body").unwrap().1;
            assert!(body.contains("## Visual context\nThe screenshot is NOT attached yet. Click in this box and press"));
            assert!(body.contains(shortcut));
            assert!(!body.contains("Attach the reviewed screenshot"));
            let draft = h.app.issue_report.draft.as_ref().unwrap();
            assert_eq!(draft.report_body(), reviewed, "reviewed diagnostics remain editable and unchanged");
            assert!(draft.note.contains("NOT attached yet") && draft.note.contains(shortcut));
            assert!(draft.opened_github);
        }
    }

    #[test]
    fn issue_report_long_screenshot_handoff_saves_png_and_preserves_text_on_save_failure() {
        let h = crate::scenario::Harness::new();
        for (path, saved) in [(h.dir.join("report.png"), true), (h.dir.join("missing/report.png"), false)] {
            let mut draft = draft_for(&h.app, None, None);
            draft.description = "The preview disappeared.".into();
            draft.steps = "Select a face and start Extrude.".into();
            draft.body.push_str(&"Reviewed diagnostic context. ".repeat(300));
            draft.image = Some(Arc::new(ColorImage::new([4, 3], vec![egui::Color32::GREEN; 12])));
            let reviewed = draft.report_body();
            assert!(draft_url(&draft.report_title(), &reviewed).1);
            let path = path.to_str().unwrap();
            let output = h.ctx.run_ui(Default::default(), |_| form::open_github(&mut draft, &h.ctx, path));
            assert!(!output.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyImage(_))));
            let text = output
                .platform_output
                .commands
                .iter()
                .find_map(|c| match c {
                    egui::OutputCommand::CopyText(text) => Some(text),
                    _ => None,
                })
                .unwrap();
            assert!(output.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(url) if !url.url.contains("&body="))));
            if saved {
                let png = image::load_from_memory_with_format(&std::fs::read(path).unwrap(), image::ImageFormat::Png).unwrap();
                assert_eq!(png.into_rgba8().dimensions(), (4, 3));
                assert!(text.contains("report.png") && text.contains("drag the PNG file"));
                assert!(!text.contains(h.dir.to_str().unwrap()), "the public issue text never names the folder");
                assert!(text.contains("NOT attached yet"));
                assert!(draft.note.contains(path) && draft.note.contains("drag"));
            } else {
                assert_eq!(text, &reviewed);
                assert!(text.contains("Attach the reviewed screenshot or a short example showing the problem."));
                assert!(draft.note.starts_with("Could not save screenshot:"));
            }
            assert_eq!(draft.report_body(), reviewed);
            assert!(draft.opened_github);
        }
    }

    #[test]
    fn issue_report_editing_enter_and_cad_shortcuts_leave_the_live_boolean_unchanged() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Stock"})).unwrap();
        h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"body_name":"Cutter"})).unwrap();
        combine(&mut h.app);
        h.app.issue_report.draft = Some(draft_for(&h.app, h.app.dialog.as_ref(), None));
        h.frames(4);
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        for field in ["issue_report_title", "issue_report_body"] {
            let id = egui::Id::unique(field);
            h.ctx.memory_mut(|m| m.request_focus(id));
            h.frames(1);
            h.step(&json!({"text":"edited-report-marker"})).unwrap();
            h.step(&json!({"key":"Enter"})).unwrap();
            assert!(h.app.dialog.is_some(), "Enter in {field} must not apply the valid Boolean");
            assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
            h.ctx.memory_mut(|m| m.surrender_focus(id));
        }
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        assert!(draft.title.contains("edited-report-marker") && draft.description.contains("edited-report-marker"));
        for key in [json!({"key":"Enter"}), json!({"key":"Escape"}), json!({"key":"Z","cmd":true})] {
            h.step(&key).unwrap();
            assert!(h.app.dialog.is_some() && h.app.issue_report.draft.is_some());
            assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
        }
    }

    #[test]
    fn issue_report_placed_sketch_text_cannot_steal_focus_apply_or_cancel_behind_draft() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let start = h.app.session.undo.len();
        h.app.start("sketch.text");
        h.frames(3);
        h.step(&json!({"click":{"sketch":[0,0]}})).unwrap();
        h.step(&json!({"text":"Pending CAD text"})).unwrap();
        assert!(h.widget("Pending CAD text").is_some(), "the real placed Text editor must be open");
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        h.app.help.menu = true;
        h.frames(2);
        h.step(&json!({"click":{"handle":"menu:report"}})).unwrap();
        h.frames(3);
        assert!(h.widget("Pending CAD text").is_some(), "capture must keep the underlying Text editor drawn");
        let token = h.app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut h.app, token, Arc::new(ColorImage::new([2, 2], vec![egui::Color32::GREEN; 4])));
        h.frames(3);
        for field in ["issue_report_title", "issue_report_body"] {
            let id = egui::Id::unique(field);
            h.ctx.memory_mut(|m| m.request_focus(id));
            h.frames(2);
            assert_eq!(h.ctx.memory(|m| m.focused()), Some(id), "placed Text must not steal report focus");
            h.step(&json!({"text":"report-only-marker"})).unwrap();
            h.step(&json!({"key":"Enter"})).unwrap();
            h.step(&json!({"key":"Escape"})).unwrap();
            assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
            assert!(h.widget("Pending CAD text").is_some(), "report keys must not apply, edit or cancel the Text editor");
        }
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        assert!(draft.title.contains("report-only-marker") && draft.description.contains("report-only-marker"));
        h.step(&json!({"widget":"Close draft"})).unwrap();
        assert!(h.app.issue_report.draft.is_none());
        h.frames(2);
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
        assert!(h.widget("Pending CAD text").is_some());
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!crate::sketch_text::editing(&h.app), "closing the report must return control to the preserved CAD text");
        assert_eq!(h.app.session.undo.len(), start + 1, "placing and typing the text is one undo step");
        let sid = h.app.session.active_sketch.unwrap();
        let sk = h.app.session.doc.sketch(sid).unwrap();
        assert!(sk.links.iter().any(|l| matches!(&l.source, solidcraft_engine::sketch::LinkSource::Text { text, .. } if text == "Pending CAD text")));
    }

    #[test]
    fn issue_report_boundary_preserves_opacity_widget_state_and_real_focus() {
        let mut app = app();
        let ctx = crate::fonts::test_context();
        let focus = egui::Id::unique("report-review-focus");
        let state = egui::Id::unique("cad-layout-state");
        begin(&mut app, &ctx, None, None);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            focused: true,
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            ctx.memory_mut(|m| m.request_focus(focus));
            let barrier = CadInput::begin(&app, &ctx).unwrap();
            ui.visuals_mut().disabled_alpha = 1.0;
            ui.disable();
            assert_eq!(ui.opacity(), 1.0, "capture must retain normal drawing opacity");
            assert!(!ui.is_enabled());
            Container::new(&ctx, Surface::Popup).area(egui::Id::unique("cad-boundary-proof")).fade_in(false).show(&ctx, |ui| {
                assert!(!ui.is_enabled());
                assert_eq!(ui.opacity(), 1.0);
                ui.ctx().data_mut(|d| d.insert_temp(state, 42_u32));
                ui.ctx().memory_mut(|m| m.request_focus(egui::Id::unique("underlying-editor")));
            });
            drop(barrier);
            assert_eq!(ctx.memory(|m| m.focused()), Some(focus));
            assert_eq!(ctx.data(|d| d.get_temp::<u32>(state)), Some(42));
            assert!(Container::new(&ctx, Surface::Popup).area(egui::Id::unique("report-boundary-proof")).is_enabled());
        });
        output.textures_delta.clear();
        app.issue_report.draft = None;
        assert!(!crate::ui_kit::input_blocked());
    }

    #[test]
    fn issue_report_cached_point_drag_release_does_not_finish_cad_drag() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[0,0],[20,0]],"id":"line"})).unwrap();
        h.frames(16);
        let at = h.call("ui.at", json!({"sketch":[0,0]}));
        let p = egui::pos2(at["result"][0].as_f64().unwrap() as f32, at["result"][1].as_f64().unwrap() as f32);
        h.app.synthetic.push(egui::Event::PointerMoved(p));
        h.frame();
        h.app.synthetic.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.frame();
        let q = p + egui::vec2(40.0, -40.0);
        h.app.synthetic.push(egui::Event::PointerMoved(q));
        h.frames(2);
        let drag = h.app.viewport.point_drag.clone();
        assert!(drag.is_some(), "real viewport point drag must have started");
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        help::run(&mut h.app, &h.ctx, "report");
        // egui calculates this release before the CAD input barrier begins.
        h.app.synthetic.push(egui::Event::PointerButton {
            pos: q,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.frame();
        assert_eq!(h.app.viewport.point_drag, drag, "cached release must not clear or constrain the CAD drag");
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
        let token = h.app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut h.app, token, Arc::new(ColorImage::new([2, 2], vec![egui::Color32::GREEN; 4])));
        h.frames(16);
        h.step(&json!({"widget":"Close draft"})).unwrap();
        assert!(h.app.viewport.point_drag.is_none());
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
    }

    #[test]
    fn issue_report_new_dimension_draft_resumes_and_commits_once_after_close() {
        crate::dim_view::cancel();
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[0,0],[20,0]]})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 80.0;
        h.app.cam.target = solidcore_record::Vec3::new(10.0, 0.0, 0.0);
        h.settle();
        h.step(&json!({"start":"sketch.dimension"})).unwrap();
        for point in [[0, 0, 0], [20, 0, 0], [10, 5, 0]] {
            h.step(&json!({"click":{"world":point}})).unwrap();
        }
        h.step(&json!({"text":"30 mm"})).unwrap();
        assert!(crate::dim_view::editing());
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        help::run(&mut h.app, &h.ctx, "report");
        h.frames(3);
        let token = h.app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut h.app, token, Arc::new(ColorImage::new([2, 2], vec![egui::Color32::GREEN; 4])));
        h.frames(3);
        h.ctx.memory_mut(|m| m.request_focus(egui::Id::unique("issue_report_body")));
        h.step(&json!({"text":"Typing here describes the bug, not a CAD dimension."})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
        assert!(crate::dim_view::editing() && h.widget("30 mm").is_some());
        assert!(h.app.issue_report.draft.as_ref().unwrap().description.contains("Typing here describes"));
        h.step(&json!({"widget":"Close draft"})).unwrap();
        h.frames(2);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::unique("sc_dim_edit")));
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!crate::dim_view::editing());
        assert_eq!(h.app.session.undo.len(), before.1 + 1);
        assert_eq!(h.app.session.doc.param("d1").unwrap().expr, "30 mm");
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before.0);
    }

    #[test]
    fn issue_report_inline_dimension_retains_expression_and_resumes_after_close() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":10,"id":"circle"})).unwrap();
        let out = h.app.run("sketch.dimension", json!({"entities":["circle"],"type":"radius","value":10})).unwrap();
        let param = out["param"].as_str().unwrap().to_owned();
        h.frames(3);
        crate::dim_view::edit_param(&h.app, &param);
        h.frames(2);
        h.step(&json!({"text":"12"})).unwrap();
        assert!(crate::dim_view::editing() && h.widget("12").is_some());
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        help::run(&mut h.app, &h.ctx, "report");
        h.frames(3);
        let token = h.app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut h.app, token, Arc::new(ColorImage::new([2, 2], vec![egui::Color32::GREEN; 4])));
        h.frames(3);
        h.ctx.memory_mut(|m| m.request_focus(egui::Id::unique("issue_report_body")));
        h.step(&json!({"text":"report only"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
        assert!(crate::dim_view::editing() && h.widget("12").is_some());
        h.step(&json!({"widget":"Close draft"})).unwrap();
        h.frames(2);
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
        assert!(crate::dim_view::editing() && h.widget("12").is_some());
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!crate::dim_view::editing());
        assert_eq!(h.app.session.doc.param(&param).unwrap().expr, "12");
        assert_eq!(h.app.session.undo.len(), before.1 + 1);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn issue_report_native_file_drop_cannot_import_behind_review() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.frames(3);
        let tabs_before = h.app.docs.count();
        assert!(!h.app.file_load.busy());
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        help::run(&mut h.app, &h.ctx, "report");
        let path = h.dir.join("private-design.json");
        std::fs::write(&path, h.app.session.doc.to_json()).unwrap();
        #[derive(Debug)]
        struct File(std::path::PathBuf);
        impl egui::DroppedFile for File {
            fn path(&self) -> &std::path::Path {
                &self.0
            }
            fn bytes(&self) -> Result<Vec<u8>, String> {
                std::fs::read(&self.0).map_err(|e| e.to_string())
            }
        }
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
            focused: true,
            dropped_files: vec![Arc::new(File(path))],
            ..Default::default()
        };
        let app = &mut h.app;
        let mut output = h.ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
        assert_eq!(before, (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
        assert!(h.app.issue_report.draft.is_some());
        assert!(!h.app.file_load.busy(), "native drop must not queue an asynchronous open behind the draft");
        assert_eq!(h.app.docs.count(), tabs_before);
    }

    /// A report opened while the preview is being computed waits for it, open, and takes the
    /// screenshot once it is done (#7171). It used to refuse and ask to reopen the report.
    #[test]
    fn issue_report_screenshot_waits_for_the_preview_then_is_taken() {
        let mut app = app();
        let ctx = crate::fonts::test_context();
        app.preview.busy = true;
        begin(&mut app, &ctx, None, None);
        assert!(!pending(&app) && app.issue_report.waiting.is_some());
        assert!(app.issue_report.draft.as_ref().unwrap().note.contains("Waiting for the preview"));
        tick(&mut app, &ctx);
        assert!(!pending(&app), "the preview is still being computed");
        app.preview.busy = false;
        app.preview.key += 1;
        tick(&mut app, &ctx);
        assert!(pending(&app), "taken once the preview is done");
        let token = app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut app, token, Arc::new(ColorImage::new([4, 3], vec![egui::Color32::GREEN; 12])));
        let draft = app.issue_report.draft.as_ref().unwrap();
        assert!(draft.image.is_some(), "{}", draft.note);
        assert!(!draft.body.contains("still being computed"));
        // A selection changed while it waits: the report's context no longer matches the view.
        app.issue_report = ReportState::default();
        app.preview.busy = true;
        begin(&mut app, &ctx, None, None);
        app.session.selection = vec![Sel::Feature { id: 1 }];
        app.preview.busy = false;
        tick(&mut app, &ctx);
        let draft = app.issue_report.draft.as_ref().unwrap();
        assert!(!pending(&app) && draft.image.is_none() && draft.note.contains("Settings or model changed"), "{}", draft.note);
    }

    /// A preview still being computed after a while is what the screenshot shows, and the report
    /// says so: a hanging preview is often what is being reported.
    #[test]
    fn issue_report_screenshot_of_a_preview_that_takes_too_long_shows_it_unfinished() {
        let mut app = app();
        let ctx = crate::fonts::test_context();
        app.preview.busy = true;
        begin(&mut app, &ctx, None, None);
        app.issue_report.waiting.as_mut().unwrap().deadline = -1.0;
        tick(&mut app, &ctx);
        assert!(pending(&app) && app.issue_report.waiting.is_none());
        let draft = app.issue_report.draft.as_ref().unwrap();
        assert!(draft.note.contains("unfinished") && draft.body.contains("still being computed 10 s after"), "{}", draft.note);
        let token = app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut app, token, Arc::new(ColorImage::new([4, 3], vec![egui::Color32::GREEN; 12])));
        assert!(app.issue_report.draft.as_ref().unwrap().image.is_some());
    }

    /// What is typed in the frame the preview finishes, as the waiting screenshot is taken and
    /// hides the form, is kept too (#7171). A preview error the dialog shows meanwhile is not a
    /// change: it is often what is being reported.
    #[test]
    fn issue_report_typing_as_a_waiting_screenshot_is_taken_is_kept() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
        h.frames(8);
        h.frame_with(vec![egui::Event::Text("The preview ".into())]);
        // The wait ends in the next frame (here: its time is up), before the form is drawn.
        let context = context_signature(&h.app, None);
        h.app.issue_report.waiting = Some(Waiting { deadline: -1.0, context });
        h.frame_with(vec![egui::Event::Text("vanished".into())]);
        assert!(pending(&h.app) && h.app.issue_report.waiting.is_none());
        h.frame_with(vec![egui::Event::Text(".".into())]);
        let token = h.app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut h.app, token, Arc::new(ColorImage::new([4, 3], vec![egui::Color32::GREEN; 12])));
        h.frames(4);
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        assert_eq!(draft.description, "The preview vanished.");
        assert!(draft.image.is_some(), "{}", draft.note);
        // The dialog's preview error is not part of the context; its inputs are.
        let mut app = app();
        app.start("solid.combine");
        let before = context_signature(&app, app.dialog.as_ref());
        if let Some(d) = app.dialog.as_mut() {
            d.error = Some("the operation failed".into());
        }
        assert_eq!(context_signature(&app, app.dialog.as_ref()), before);
        if let Some(d) = app.dialog.as_mut() {
            d.extra.insert("probe".into(), json!(1));
        }
        assert_ne!(context_signature(&app, app.dialog.as_ref()), before);
    }

    /// What is typed while a capture hides the report goes into the field it was typed in.
    #[test]
    fn issue_report_typing_while_a_capture_hides_the_report_is_kept() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.issue_report.draft = Some(draft_for(&h.app, None, None));
        h.frames(8);
        h.frame_with(vec![egui::Event::Text("The preview ".into())]);
        let (ctx, dialog) = (h.ctx.clone(), h.app.dialog.clone());
        capture(&mut h.app, &ctx, dialog.as_ref());
        assert!(pending(&h.app));
        h.frame_with(vec![egui::Event::Text("vanished".into())]);
        h.frame_with(vec![egui::Event::Text(".".into())]);
        let token = h.app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut h.app, token, Arc::new(ColorImage::new([4, 3], vec![egui::Color32::GREEN; 12])));
        h.frames(4);
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        assert_eq!(draft.description, "The preview vanished.");
        assert!(draft.image.is_some());
    }

    fn green(w: usize, h: usize) -> Arc<ColorImage> {
        Arc::new(ColorImage::new([w, h], vec![egui::Color32::GREEN; w * h]))
    }

    /// Retake after the design changed rebuilds the technical details for the current view and
    /// keeps what was typed; it used to refuse for the rest of the draft (#7350).
    #[test]
    fn issue_report_retake_after_a_command_rebuilds_the_details_and_keeps_the_answers() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"name":"FirstBox"})).unwrap();
        help::run(&mut h.app, &h.ctx, "report");
        let token = h.app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut h.app, token, green(4, 3));
        h.frames(16);
        let draft = h.app.issue_report.draft.as_mut().unwrap();
        (draft.title, draft.description, draft.steps, draft.expected) = ("Title".into(), "Happened".into(), "Steps".into(), "Expected".into());
        assert!(draft.body.contains("FirstBox") && !draft.body.contains("SecondBox"));
        // An agent runs a command while the report is open.
        h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"name":"SecondBox"})).unwrap();
        h.frames(16);
        h.step(&json!({"widget":"Retake screenshot"})).unwrap();
        assert!(pending(&h.app), "{}", h.app.issue_report.draft.as_ref().unwrap().note);
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        assert!(draft.body.contains("SecondBox") && draft.body.contains(&format!("Revision: {}", h.app.session.revision)), "{}", draft.body);
        assert_eq!((&*draft.title, &*draft.description, &*draft.steps, &*draft.expected), ("Title", "Happened", "Steps", "Expected"));
        assert_eq!(draft.image.as_ref().unwrap().size, [4, 3], "the old screenshot stays until the new one is taken");
        let token = h.app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut h.app, token, green(6, 5));
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        assert_eq!(draft.image.as_ref().unwrap().size, [6, 5]);
        assert_eq!(draft.note, NOTE_DETAILS_UPDATED);
        // And again: the rebuilt draft is the new baseline.
        h.app.run("solid.box", json!({"length":3,"width":3,"height":3,"name":"ThirdBox"})).unwrap();
        let (ctx, dialog) = (h.ctx.clone(), h.app.dialog.clone());
        retake(&mut h.app, &ctx, dialog.as_ref());
        assert!(pending(&h.app) && h.app.issue_report.draft.as_ref().unwrap().body.contains("ThirdBox"));
    }

    /// Technical details edited by hand are the user's: Retake keeps them, and says so.
    #[test]
    fn issue_report_retake_after_a_change_keeps_edited_details() {
        let mut app = app();
        let ctx = crate::fonts::test_context();
        begin(&mut app, &ctx, None, None);
        let draft = app.issue_report.draft.as_mut().unwrap();
        draft.body = "Details with the private parts removed".into();
        draft.description = "Happened".into();
        app.run("solid.box", json!({"length":5,"width":5,"height":5,"body_name":"SecondBox"})).unwrap();
        retake(&mut app, &ctx, None);
        assert!(pending(&app));
        let token = app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut app, token, green(4, 3));
        let draft = app.issue_report.draft.as_ref().unwrap();
        assert_eq!((&*draft.body, &*draft.description), ("Details with the private parts removed", "Happened"));
        assert!(draft.image.is_some());
        assert_eq!(draft.note, NOTE_DETAILS_KEPT);
    }

    /// A crash draft's details come from its crash report: Retake keeps them and takes the
    /// screenshot, also with a feature dialog open.
    #[test]
    fn issue_report_retake_of_a_crash_draft_keeps_its_body() {
        let mut app = app();
        combine(&mut app);
        let ctx = crate::fonts::test_context();
        begin_crash(&mut app, &ctx, &json!({"kind":"command_panic","message":"synthetic panic","id":"crash-1"}));
        let body = app.issue_report.draft.as_ref().unwrap().body.clone();
        assert!(body.contains("synthetic panic"));
        app.session.selection = vec![Sel::Feature { id: 1 }];
        let dialog = app.dialog.clone();
        retake(&mut app, &ctx, dialog.as_ref());
        assert!(pending(&app), "{}", app.issue_report.draft.as_ref().unwrap().note);
        let token = app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut app, token, green(4, 3));
        let draft = app.issue_report.draft.as_ref().unwrap();
        assert_eq!(draft.body, body);
        assert!(draft.image.is_some() && draft.note.starts_with("Screenshot captured locally"), "{}", draft.note);
    }

    /// A capture that is refused, waits, times out, comes back stale or malformed keeps the
    /// screenshot the draft has; only a new frame replaces it (#7350).
    #[test]
    fn issue_report_failed_capture_keeps_the_screenshot() {
        let mut app = app();
        let ctx = crate::fonts::test_context();
        begin(&mut app, &ctx, None, None);
        let token = app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut app, token, green(4, 3));
        let kept = |app: &SolveApp| app.issue_report.draft.as_ref().unwrap().image.as_ref().map(|i| i.size);
        // Waits for the preview, then is refused: the selection changed meanwhile.
        app.preview.busy = true;
        retake(&mut app, &ctx, None);
        assert!(app.issue_report.waiting.is_some());
        assert_eq!(kept(&app), Some([4, 3]));
        app.session.selection = vec![Sel::Feature { id: 1 }];
        app.preview.busy = false;
        tick(&mut app, &ctx);
        assert!(!pending(&app) && app.issue_report.draft.as_ref().unwrap().note.contains("Settings or model changed"));
        assert_eq!(kept(&app), Some([4, 3]), "refused");
        retake(&mut app, &ctx, None);
        app.issue_report.pending.as_mut().unwrap().deadline = -1.0;
        tick(&mut app, &ctx);
        assert!(app.issue_report.draft.as_ref().unwrap().note.contains("No app frame"));
        assert_eq!(kept(&app), Some([4, 3]), "timed out");
        retake(&mut app, &ctx, None);
        let token = app.issue_report.pending.as_ref().unwrap().token;
        app.cam.distance *= 2.0;
        receive(&mut app, token, green(6, 5));
        assert_eq!(kept(&app), Some([4, 3]), "stale");
        retake(&mut app, &ctx, None);
        let token = app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut app, token, Arc::new(ColorImage::new([0, 0], vec![])));
        assert_eq!(kept(&app), Some([4, 3]), "malformed");
        retake(&mut app, &ctx, None);
        let token = app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut app, token, green(6, 5));
        assert_eq!(kept(&app), Some([6, 5]));
    }

    #[test]
    fn issue_report_capture_accepts_only_its_matching_unchanged_frame() {
        let mut app = app();
        let ctx = crate::fonts::test_context();
        begin(&mut app, &ctx, None, None);
        let token = app.issue_report.pending.as_ref().unwrap().token;
        let (reply, _receiver) = std::sync::mpsc::channel();
        app.shots.pending.push((token + 1, None, reply, crate::now_ms() + 8000.0));
        app.shots.token = token + 1;
        let image = Arc::new(ColorImage::new([4, 3], vec![egui::Color32::GREEN; 12]));
        receive(&mut app, token + 1, image.clone());
        assert!(pending(&app) && app.issue_report.draft.as_ref().unwrap().image.is_none());
        receive(&mut app, token, image.clone());
        assert!(!pending(&app));
        assert_eq!(app.issue_report.draft.as_ref().unwrap().image.as_ref().unwrap().size, [4, 3]);
        assert_eq!(app.shots.pending.len(), 1, "report cannot consume the control request");
        app.issue_report = ReportState::default();
        begin(&mut app, &ctx, None, None);
        let token = app.issue_report.pending.as_ref().unwrap().token;
        app.cam.distance *= 2.0;
        receive(&mut app, token, image);
        let draft = app.issue_report.draft.as_ref().unwrap();
        assert!(draft.image.is_none() && draft.note.contains("changed"));
    }

    /// A 6K display's frame, 20.4 million pixels, is over the limit: the report keeps it at half
    /// size instead of refusing it (#7091).
    #[test]
    fn issue_report_capture_keeps_a_frame_over_the_limit_scaled_down() {
        let mut app = app();
        let ctx = crate::fonts::test_context();
        begin(&mut app, &ctx, None, None);
        // As the app's window device has it (the vendored egui-wgpu asks for at least 8192).
        app.issue_report.pending.as_mut().unwrap().max_side = 8192;
        let token = app.issue_report.pending.as_ref().unwrap().token;
        let [w, h] = [6016, 3384];
        let pixels = (0..w * h).map(|i| if i % w < w / 2 { egui::Color32::RED } else { egui::Color32::BLUE }).collect();
        receive(&mut app, token, Arc::new(ColorImage::new([w, h], pixels)));
        let draft = app.issue_report.draft.as_ref().unwrap();
        assert!(draft.note.contains("captured"), "{}", draft.note);
        let image = draft.image.as_ref().expect("the report keeps a screenshot");
        assert_eq!(image.size, [3008, 1692]);
        // The halves still meet in the middle.
        assert_eq!(
            [image.pixels[0], image.pixels[1503], image.pixels[1504], image.pixels[3007]],
            [egui::Color32::RED, egui::Color32::RED, egui::Color32::BLUE, egui::Color32::BLUE]
        );
    }

    /// Three ultrawide screens side by side, 10320 × 1440: under the pixel limit, but wider than
    /// the GPU can make a texture. The preview's upload would fail and the window's device panic
    /// on it; here, where the limit is egui's default 2048, egui's own check panicked.
    #[test]
    fn issue_report_preview_of_a_frame_wider_than_a_texture_fits_the_gpu() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let ctx = h.ctx.clone();
        begin(&mut h.app, &ctx, None, None);
        let token = h.app.shots.token;
        receive(&mut h.app, token, Arc::new(ColorImage::new([10_320, 1440], vec![egui::Color32::GREEN; 10_320 * 1440])));
        h.frames(4);
        let max_side = h.ctx.input(|i| i.max_texture_side);
        let draft = h.app.issue_report.draft.as_ref().unwrap();
        let size = draft.image.as_ref().expect("the report keeps a screenshot").size;
        assert!(size[0] <= max_side && size[1] <= max_side, "{size:?} over {max_side}");
        assert!(draft.texture.is_some(), "the preview is drawn");
    }

    #[test]
    fn a_frame_over_the_limit_is_averaged_down_by_a_whole_factor() {
        let frame = |[w, h]: [usize; 2], colour: &dyn Fn(usize, usize) -> egui::Color32| {
            Arc::new(ColorImage::new([w, h], (0..w * h).map(|i| colour(i % w, i / w)).collect()))
        };
        let small = frame([4, 3], &|_, _| egui::Color32::GREEN);
        assert!(Arc::ptr_eq(&fitted(small.clone(), 12, 4).unwrap(), &small), "a frame within the limits is kept as it is");
        // 15 pixels over a limit of 6: by 2, into 3 × 2; the last column and row average the
        // pixels they cover.
        let grey = egui::Color32::from_gray;
        let checks = frame([5, 3], &|x, y| grey(if (x + y) % 2 == 0 { 0 } else { 200 }));
        let image = fitted(checks, 6, 16).unwrap();
        assert_eq!(image.size, [3, 2]);
        assert_eq!(image.pixels, [grey(100), grey(100), grey(100), grey(100), grey(100), grey(0)]);
        // A long strip goes down by the factor its area or its length needs: 1000 × 2 into 4
        // pixels is by 250, and into a side of 300 by 4.
        let strip = frame([1000, 2], &|_, _| egui::Color32::GREEN);
        assert_eq!(fitted(strip.clone(), 4, 16).unwrap().size, [4, 1]);
        assert_eq!(fitted(strip, 10_000, 300).unwrap().size, [250, 1]);
        let mut short = ColorImage::new([2, 2], vec![egui::Color32::GREEN; 4]);
        short.pixels.pop();
        assert!(fitted(Arc::new(short), 16, 16).is_none(), "a frame with fewer pixels than its size is refused");
        assert!(fitted(Arc::new(ColorImage::new([0, 0], vec![])), 16, 16).is_none());
    }

    #[test]
    fn issue_report_capture_rejects_changed_inputs_timeout_and_malformed_images() {
        let mut app = app();
        combine(&mut app);
        let ctx = crate::fonts::test_context();
        let dialog = app.dialog.clone();
        begin(&mut app, &ctx, dialog.as_ref(), Some("bad Boolean"));
        app.dialog.as_mut().unwrap().extra.insert("direction".into(), json!("negative"));
        tick(&mut app, &ctx);
        assert!(!pending(&app));
        assert!(app.issue_report.draft.as_ref().unwrap().note.contains("changed"));
        app.issue_report = ReportState::default();
        let dialog = app.dialog.clone();
        begin(&mut app, &ctx, dialog.as_ref(), None);
        app.issue_report.pending.as_mut().unwrap().deadline = -1.0;
        tick(&mut app, &ctx);
        assert!(app.issue_report.draft.as_ref().unwrap().note.contains("No app frame"));
        app.issue_report = ReportState::default();
        let dialog = app.dialog.clone();
        begin(&mut app, &ctx, dialog.as_ref(), None);
        let token = app.issue_report.pending.as_ref().unwrap().token;
        receive(&mut app, token, Arc::new(ColorImage::new([0, 0], vec![])));
        assert!(app.issue_report.draft.as_ref().unwrap().image.is_none());
        assert!(app.issue_report.draft.as_ref().unwrap().note.contains("dimensions"));
    }

    // ---- the in-app report (tickets.solidcraft.io) ---------------------------------------------

    use crate::tickets::{self, payload};
    use std::sync::Mutex;

    /// A keyring in memory that remembers every secret it was given.
    #[derive(Default)]
    struct FakeKeyring(Mutex<std::collections::HashMap<String, String>>);

    impl crate::agent_chat::backend::Secrets for FakeKeyring {
        fn get(&self, id: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(id).cloned())
        }
        fn set(&self, id: &str, secret: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(id.into(), secret.into());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }

    type Requests = Arc<Mutex<Vec<crate::agent_chat::backend::HttpRequest>>>;

    /// A ticket server that answers at once: `online` false makes every request fail as if
    /// offline. Records the requests it gets.
    fn fake_server(online: Arc<std::sync::atomic::AtomicBool>, requests: Requests) -> tickets::Http {
        Arc::new(move |r: crate::agent_chat::backend::HttpRequest, done: std::sync::mpsc::Sender<tickets::Reply>| {
            requests.lock().unwrap().push(r.clone());
            if !online.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = done.send(Err("network unreachable".into()));
                return;
            }
            let path = r.url.trim_start_matches(&tickets::api::server()).to_string();
            let reply = match (r.method, path.as_str()) {
                ("POST", "/api/device/code") => {
                    json!({"device_code":"dev-1","user_code":"WDJB-MJHT","verification_uri":format!("{}/device", tickets::api::server()),"interval":1,"expires_in":600})
                }
                ("POST", "/api/device/token") => json!({"access_token":"tkt_secret_token_123","token_type":"Bearer"}),
                ("GET", "/api/apps") => json!({"apps":[{"slug":"solidcraft","name":"SolidCraft"},{"slug":"pcbcraft","name":"PCBCraft"}]}),
                ("POST", "/api/tickets") => json!({"id": 42}),
                ("POST", "/api/drafts") => {
                    json!({"draft_id":"d1","draft_secret":"sec","upload_token":"tdu_draft_token","url":format!("{}/draft/d1#sec", tickets::api::server()),"expires_at":"x","max_bytes":524288000,"max_files":20})
                }
                ("GET", "/api/drafts/d1/status") => {
                    json!({"status":"claimed","ticket_id":77,"ticket_url":"x","access_token":"tkt_remembered_456","token_type":"Bearer","scope":"tickets:report"})
                }
                ("POST", "/api/uploads") => {
                    // Small parts (8 bytes for small files), so a file goes in several.
                    let b: serde_json::Value = serde_json::from_slice(r.body.as_deref().unwrap()).unwrap();
                    let size = b["size"].as_u64().unwrap();
                    assert_eq!(b["sha256"].as_str().map(str::len), Some(64), "the server checks the hash");
                    let part = if size < 1024 { 8 } else { 4096 };
                    json!({"id": format!("up{}", requests.lock().unwrap().len()), "part_size": part, "part_count": size.div_ceil(part), "received": []})
                }
                ("PUT", p) if p.contains("/parts/") => json!({"part": p.rsplit('/').next(), "size": r.body.as_ref().map_or(0, Vec::len)}),
                ("POST", p) if p.ends_with("/complete") => json!({"attachment":{"id":"a"}}),
                _ => json!({}),
            };
            let refuse = r.body.as_ref().is_some_and(|b| String::from_utf8_lossy(b).contains("REFUSE-ME"));
            let status = if refuse {
                400
            } else if path == "/api/tickets" {
                201
            } else {
                200
            };
            let _ = done.send(Ok((status, reply.to_string().into_bytes())));
        })
    }

    fn ticket_app(online: bool, token: Option<&str>) -> (SolveApp, Arc<std::sync::atomic::AtomicBool>, Requests, Arc<FakeKeyring>) {
        let online = Arc::new(std::sync::atomic::AtomicBool::new(online));
        let requests: Requests = Arc::default();
        let keyring = Arc::new(FakeKeyring::default());
        if let Some(token) = token {
            crate::agent_chat::backend::Secrets::set(&*keyring, tickets::SECRET_ID, token).unwrap();
        }
        let services = crate::Services {
            tickets_http: Some(fake_server(online.clone(), requests.clone())),
            ticket_secrets: Some(keyring.clone()),
            ..Default::default()
        };
        (SolveApp::new(Session::default(), services), online, requests, keyring)
    }

    fn evidence_with_design() -> payload::Evidence {
        let mut session = serde_json::Map::new();
        session.insert("session_start".into(), json!("opened"));
        payload::Evidence {
            system: "SolidCraft 1.0\nTarget: x86_64-linux\n".into(),
            system_fields: serde_json::Map::new(),
            app_version: "1.0".into(),
            os: "linux x86_64".into(),
            session,
            failing: Some(payload::Failing {
                command: Some("solid.fillet".into()),
                params: Some(json!({"radius": 2.0})),
                error: "radius too large".into(),
                origin: None,
                code: None,
                failure_code: None,
            }),
            log: vec!["[error] radius too large".into()],
            screenshot: Some(vec![1, 2, 3]),
            design: Some(("SecretBracket.sc".into(), b"SECRET-DESIGN-BYTES".to_vec())),
            crash: None,
            ..Default::default()
        }
    }

    fn form() -> payload::Form {
        payload::Form { app: "solidcraft".into(), title: "Fillet fails".into(), description: "It refused".into(), ..Default::default() }
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("sc-report-{tag}-{}-{}", std::process::id(), crate::now_ms() as u64))
    }

    #[test]
    fn ticket_report_payload_carries_only_the_ticked_boxes() {
        let evidence = evidence_with_design();
        let ticks = payload::Ticks { system: false, repro: false, log: true, screenshot: false, design: false, crash: false };
        let out = payload::build(&form(), ticks, &evidence, "solidcraft test");
        let t = &out.ticket;
        assert!(t.get("system").is_none() && t.get("appVersion").is_none() && t.get("os").is_none(), "system info unticked: {t}");
        assert!(t.get("repro").is_none(), "the failed operation unticked: {t}");
        assert_eq!(out.files.iter().map(|f| f.kind.as_str()).collect::<Vec<_>>(), ["log"]);
        assert_eq!(t["consent"]["kinds"], json!(["log"]));
        assert_eq!((t["consent"]["via"].as_str(), t["consent"]["client"].as_str()), (Some("dialog"), Some("solidcraft test")));
        assert_eq!((t["app"].as_str(), t["type"].as_str(), t["title"].as_str()), (Some("solidcraft"), Some("bug"), Some("Fillet fails")));

        let all = payload::Ticks { system: true, repro: true, log: true, screenshot: true, design: true, crash: true };
        let out = payload::build(&form(), all, &evidence, "solidcraft test");
        assert_eq!(out.ticket["system"]["text"], json!(evidence.system), "the exact text the dialog shows");
        let failing = &out.ticket["repro"]["failing"];
        assert_eq!(
            (failing["command"].as_str(), failing["params"]["radius"].as_f64()),
            (Some("solid.fillet"), Some(2.0)),
            "replayable as the command API takes it"
        );
        assert_eq!(out.ticket["repro"]["session_start"], "opened");
        assert_eq!(out.files.iter().map(|f| f.kind.as_str()).collect::<Vec<_>>(), ["log", "screenshot", "design"]);
        assert!(!out.ticket["consent"]["kinds"].as_array().unwrap().contains(&json!("crash")), "a box with nothing behind it is not consented to");
    }

    #[test]
    fn ticket_report_never_includes_the_design_unless_its_box_is_ticked() {
        assert!(!payload::Ticks::default().design, "the design box is off by default");
        let evidence = evidence_with_design();
        let out = payload::build(&form(), payload::Ticks::default(), &evidence, "solidcraft test");
        let ticket = out.ticket.to_string();
        assert!(!ticket.contains("SecretBracket") && !out.ticket["consent"]["kinds"].as_array().unwrap().contains(&json!("design")));
        for f in &out.files {
            assert!(f.kind != "design" && !f.name.contains("SecretBracket"));
            assert!(!f.bytes.windows(6).any(|w| w == b"SECRET"), "{} carries the design", f.name);
        }
        let ticked = payload::Ticks { design: true, ..Default::default() };
        let out = payload::build(&form(), ticked, &evidence, "solidcraft test");
        let design = out.files.iter().find(|f| f.kind == "design").unwrap();
        assert_eq!((design.name.as_str(), design.bytes.as_slice()), ("SecretBracket.sc", &b"SECRET-DESIGN-BYTES"[..]));

        // The dialog's own evidence: System info and the failed operation's details say nothing
        // of the design but its hash.
        let mut app = app();
        app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"PrivateWidget"})).unwrap();
        Arc::make_mut(&mut app.session.doc).name = "PrivateWidget".into();
        let evidence = ticket::evidence(&app, None, None, None);
        assert!(!evidence.system.contains("PrivateWidget"), "{}", evidence.system);
        assert!(evidence.design.as_ref().is_some_and(|(name, bytes)| name == "PrivateWidget.sc" && !bytes.is_empty()));
        assert_eq!(evidence.session["session_start"], "new");
        assert_eq!(evidence.session["design_sha256"].as_str().map(str::len), Some(64));
        let out = payload::build(&form(), payload::Ticks::default(), &evidence, "t");
        assert!(!out.ticket.to_string().contains("PrivateWidget"));
        assert!(out.files.iter().all(|f| f.kind != "design"));
    }

    /// A failed importer reads a separate file; the report must never suggest the unrelated
    /// design that happens to be open. Keep the filename and explicit importer, but never
    /// expose option values or full local paths in recent steps (#18760).
    #[test]
    fn failed_plugin_import_report_omits_unrelated_design_and_keeps_safe_context() {
        let mut app = app();
        app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"PrivateWidget"})).unwrap();
        let params = json!({
            "path":"/Users/testuser/private/part.xyz",
            "format":"example.import",
            "options":{"scale":10,"api_key":"private-secret-123"}
        });
        ticket::record_step(&mut app, "file.import", &Err("importer refused the file".into()));
        ticket::step_params(&mut app, "file.import", &params);
        let ev = ticket::evidence(&app, None, None, None);
        assert!(ev.design.is_none(), "an unrelated design cannot replay a failed importer");
        assert_eq!(ev.design_from, Some("report.design_unrelated_import"));
        assert_eq!(ev.session["session_start"], "new", "an unrelated design did not come from recovery");
        assert!(ev.session.get("design_sha256").is_none(), "do not hash an unrelated design");
        let step = ev.steps.last().unwrap();
        assert_eq!(step["command"], "file.import");
        assert_eq!(step["file"], "part.xyz");
        assert_eq!(step["importer"], "example.import");
        assert_eq!(step["option_keys"], json!(["api_key", "scale"]));
        let text = step.to_string();
        assert!(!text.contains("private-secret-123") && !text.contains("/Users/testuser/") && !text.contains("PrivateWidget"), "{text}");
        let out = payload::build(&form(), payload::Ticks { design: true, repro: true, ..Default::default() }, &ev, "test");
        assert!(out.files.iter().all(|file| file.kind != "design"), "even ticking Design cannot send an unrelated file");

        // A failed modeling command still offers its design. A successful import step
        // also does not suppress design attachments to a later report.
        let failed = payload::Failing {
            command: Some("solid.fillet".into()),
            params: None,
            error: "too large".into(),
            origin: None,
            code: None,
            failure_code: None,
        };
        assert!(ticket::evidence(&app, None, Some(failed), None).design.is_some());
        app.issue_report.steps.back_mut().unwrap()["ok"] = json!(true);
        assert!(ticket::evidence(&app, None, None, None).design.is_some());
    }

    /// Frames until `done` says stop, collecting the addresses opened in the browser.
    fn frames_until(app: &mut SolveApp, ctx: &egui::Context, mut done: impl FnMut(&SolveApp) -> bool) -> Vec<String> {
        let mut opened = Vec::new();
        for _ in 0..400 {
            let mut out = ctx.run_ui(Default::default(), |ui| tickets::tick(app, ui.ctx()));
            // No painter takes the font atlas: drop it on purpose (egui asserts otherwise).
            out.textures_delta.clear();
            opened.extend(out.platform_output.commands.into_iter().filter_map(|c| match c {
                egui::OutputCommand::OpenUrl(u) => Some(u.url),
                _ => None,
            }));
            if done(app) {
                break;
            }
        }
        opened
    }

    fn agent_status(app: &mut SolveApp, id: &str) -> serde_json::Value {
        app.run("diagnostics.report_issue_status", json!({"report": id})).unwrap()
    }

    fn draft_page() -> String {
        format!("{}/draft/d1#sec", tickets::api::server())
    }

    fn no_token_anywhere(requests: &Requests) {
        for r in requests.lock().unwrap().iter() {
            assert!(!r.url.contains("/api/device/") && !r.url.ends_with("/api/tickets"), "no sign-in, no direct filing: {}", r.url);
            assert!(
                r.headers.iter().filter(|(k, _)| k == "Authorization").all(|(_, v)| v == "Bearer tdu_draft_token"),
                "only the draft's upload token"
            );
        }
    }

    #[test]
    fn ticket_report_send_makes_a_draft_uploads_its_files_and_opens_it_in_the_browser() {
        // A keyring entry from a test build is removed, and none is ever written.
        let (mut app, _, requests, keyring) = ticket_app(true, Some("tkt_from_a_test_build"));
        let ctx = crate::fonts::test_context();
        let report = payload::build(&form(), payload::Ticks { design: true, ..Default::default() }, &evidence_with_design(), "t");
        tickets::send(&mut app, report);
        let opened = frames_until(&mut app, &ctx, |a| a.tickets.sending.is_none());
        assert!(keyring.0.lock().unwrap().is_empty(), "the app keeps no account details");
        assert_eq!(app.tickets.outcome, Some(tickets::Outcome::Opened(draft_page())));
        assert_eq!(opened, [draft_page()], "the draft's page opens in the browser");
        no_token_anywhere(&requests);
        let requests = requests.lock().unwrap().clone();
        let draft = requests.iter().find(|r| r.url.ends_with("/api/drafts")).unwrap();
        assert!(!draft.headers.iter().any(|(k, _)| k == "Authorization"), "no account needed");
        let opens = requests.iter().filter(|r| r.method == "POST" && r.url.ends_with("/api/uploads"));
        assert!(opens.clone().count() >= 2 && opens.clone().all(|r| !String::from_utf8_lossy(r.body.as_deref().unwrap()).contains("ticket_id")));
        let bytes: Vec<u8> = requests.iter().filter(|r| r.method == "PUT").flat_map(|r| r.body.clone().unwrap_or_default()).collect();
        assert!(bytes.windows(19).any(|w| w == b"SECRET-DESIGN-BYTES"), "the ticked design goes with the draft");
        assert!(app.session.log.iter().any(|l| l.contains("Problem report: opened in the browser")), "each step is in the log");
    }

    #[test]
    fn ticket_report_made_offline_is_kept_and_sent_on_retry() {
        let dir = temp_dir("queue");
        let (mut app, online, requests, _) = ticket_app(false, None);
        app.tickets.queue_dir = Some(dir.clone());
        let ctx = crate::fonts::test_context();
        let report = payload::build(&form(), payload::Ticks { design: true, ..Default::default() }, &evidence_with_design(), "t");
        tickets::send(&mut app, report);
        frames_until(&mut app, &ctx, |a| a.tickets.sending.is_none());
        // The step that went unanswered is named (#17932), after one more try.
        assert!(
            matches!(&app.tickets.outcome, Some(tickets::Outcome::Queued(why)) if why.contains("network unreachable") && why.contains("at create report draft (POST /api/drafts)")),
            "{:?}",
            app.tickets.outcome
        );
        assert_eq!(requests.lock().unwrap().len(), 2, "the draft is asked for twice, then the report waits");
        assert_eq!(tickets::queue::list(&dir).len(), 1, "one report kept");
        let sent = requests.lock().unwrap().len();
        frames_until(&mut app, &ctx, |_| false);
        assert_eq!(requests.lock().unwrap().len(), sent, "nothing goes by itself");

        online.store(true, std::sync::atomic::Ordering::SeqCst);
        tickets::retry(&mut app, 0);
        frames_until(&mut app, &ctx, |a| a.tickets.sending.is_none());
        assert_eq!(app.tickets.outcome, Some(tickets::Outcome::Opened(draft_page())));
        assert!(tickets::queue::list(&dir).is_empty(), "sent and removed from the queue");
        let uploaded: Vec<u8> =
            requests.lock().unwrap().iter().filter(|r| r.method == "PUT").flat_map(|r| r.body.clone().unwrap_or_default()).collect();
        assert!(uploaded.windows(19).any(|w| w == b"SECRET-DESIGN-BYTES"));
        no_token_anywhere(&requests);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ticket_report_sends_a_step_again_once_when_it_went_unanswered() {
        // The first part of an upload gets no answer (a connection that hung, #17932): it goes
        // again and the report opens, instead of waiting in the queue.
        let dir = temp_dir("queue-again");
        let (mut app, _, requests, _) = ticket_app(true, None);
        app.tickets.queue_dir = Some(dir.clone());
        let inner = app.services.tickets_http.take().unwrap();
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let drop_once = dropped.clone();
        let seen = requests.clone();
        app.services.tickets_http =
            Some(Arc::new(move |r: crate::agent_chat::backend::HttpRequest, done: std::sync::mpsc::Sender<tickets::Reply>| {
                if r.method == "PUT" && !drop_once.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    seen.lock().unwrap().push(r);
                    let _ = done.send(Err("timeout: global".into()));
                    return;
                }
                inner(r, done);
            }));
        let ctx = crate::fonts::test_context();
        let report = payload::build(&form(), payload::Ticks::default(), &evidence_with_design(), "t");
        tickets::send(&mut app, report);
        frames_until(&mut app, &ctx, |a| a.tickets.sending.is_none());
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(app.tickets.outcome, Some(tickets::Outcome::Opened(draft_page())));
        assert!(tickets::queue::list(&dir).is_empty(), "nothing kept");
        assert!(
            app.session.log.iter().any(|l| l.contains("no answer to upload part 1 of log.txt (PUT /api/uploads/") && l.contains("timeout: global")),
            "{:?}",
            app.session.log
        );
        let puts: Vec<String> = requests.lock().unwrap().iter().filter(|r| r.method == "PUT").map(|r| r.url.clone()).collect();
        assert_eq!(puts.first(), puts.get(1), "the same part again, not the next one: {puts:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn report_harness() -> (crate::scenario::Harness, Requests) {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let (mut server, _, requests, _) = ticket_app(true, None);
        h.app.services.tickets_http = server.services.tickets_http.take();
        h.app.services.ticket_secrets = server.services.ticket_secrets.take();
        help::run(&mut h.app, &h.ctx, "report");
        cancel_capture(&mut h.app);
        h.frames(4);
        (h, requests)
    }

    #[test]
    fn ticket_report_send_without_a_title_still_sends_and_ends_on_thanks() {
        // The bug button opens the report with no title. Send must not refuse it out of sight
        // (the reason Send seemed to do nothing): the title comes from the description.
        let (mut h, requests) = report_harness();
        {
            let t = h.app.issue_report.draft.as_mut().unwrap().ticket.as_mut().unwrap();
            assert!(t.form.title.is_empty());
            t.form.description = "The sketch dimension box covers the value\nmore".into();
        }
        h.frames(2);
        let ok = crate::scenario::handle_at("dialog:ok").expect("Send is drawn");
        let at = [f64::from(ok.x), f64::from(ok.y)];
        click_commands_at(&mut h, at);
        click_commands_at(&mut h, at);
        for _ in 0..200 {
            if h.app.tickets.sending.is_none() {
                break;
            }
            h.frame();
        }
        h.frames(2);
        let requests = requests.lock().unwrap().clone();
        let drafts: Vec<_> = requests.iter().filter(|r| r.url.ends_with("/api/drafts")).collect();
        assert_eq!(drafts.len(), 1, "one draft for a double click");
        let body: serde_json::Value = serde_json::from_slice(drafts[0].body.as_deref().unwrap()).unwrap();
        assert_eq!(body["title"], "The sketch dimension box covers the value");
        // The dialog is now the Thanks box, with the link.
        assert!(h.widget(solidcraft_ui_text::tr("report.thanks")).is_some(), "Thanks is shown");
        assert!(h.widget(solidcraft_ui_text::tr("report.copy_link")).is_some() && h.widget(solidcraft_ui_text::tr("report.open_again")).is_some());
        assert!(crate::scenario::handle_at("dialog:ok").is_none(), "no Send any more");
        assert_eq!(ticket::title_or_default("", ""), solidcraft_ui_text::tr("report.default_title"));
    }

    #[test]
    fn ticket_report_failure_shows_the_reason_with_retry() {
        let (mut h, requests) = report_harness();
        h.app.issue_report.draft.as_mut().unwrap().ticket.as_mut().unwrap().form.title = "REFUSE-ME".into();
        h.frames(2);
        let ok = crate::scenario::handle_at("dialog:ok").expect("Send is drawn");
        click_commands_at(&mut h, [f64::from(ok.x), f64::from(ok.y)]);
        h.frames(10);
        assert!(matches!(h.app.tickets.outcome, Some(tickets::Outcome::Failed(_))));
        assert!(h.widget(solidcraft_ui_text::tr("report.error_title")).is_some(), "the error is shown");
        assert!(h.widget(solidcraft_ui_text::tr("report.retry")).is_some() && h.widget(solidcraft_ui_text::tr("report.copy_details")).is_some());
        assert_eq!(requests.lock().unwrap().iter().filter(|r| r.url.ends_with("/api/drafts")).count(), 1);
    }

    #[test]
    fn ticket_report_opens_from_the_bug_button_and_a_failure_prefills_it() {
        // Without a way to reach the server, the bug button opens the GitHub draft: visible.
        let mut app = app();
        app.home.open = false;
        assert!(!ticket::enabled(&app));
        let ctx = crate::fonts::test_context();
        help::run(&mut app, &ctx, "report");
        cancel_capture(&mut app);
        let _ = ctx.run_ui(Default::default(), |ui| show(&mut app, ui.ctx()));
        assert!(app.issue_report.draft.as_ref().unwrap().ticket.is_none(), "the GitHub draft, shown");

        let (mut app, _, _, _) = ticket_app(true, None);
        app.home.open = false;
        assert!(ticket::enabled(&app));
        help::run(&mut app, &ctx, "report");
        cancel_capture(&mut app);
        let _ = ctx.run_ui(Default::default(), |ui| show(&mut app, ui.ctx()));
        let t = app.issue_report.draft.as_ref().unwrap().ticket.as_ref().unwrap();
        assert!(t.ticks == payload::Ticks::default() && t.form.app == "solidcraft" && !t.from_failure && t.form.title.is_empty());
        app.issue_report.draft = None;

        // From a failed operation: the title, the description template and the failed command.
        let mut app = failing_sample();
        app.services.tickets_http = ticket_app(true, None).0.services.tickets_http.take();
        let dialog = app.dialog.clone();
        let error = dialog.as_ref().and_then(|d| d.error.clone()).or_else(|| app.preview.error.clone()).unwrap_or_else(|| "operation failed".into());
        let draft = draft_for(&app, dialog.as_ref(), Some(&error));
        let t = ticket::new_ticket(&mut app, &draft, None);
        assert!(t.from_failure && t.form.kind == payload::Kind::Bug);
        assert!(t.form.title.contains(error.lines().next().unwrap()), "{}", t.form.title);
        assert!(t.form.description.contains("**Error:**") && t.form.description.contains(solidcraft_ui_text::tr("report.trying_to_do")));

        // The preference brings back the GitHub draft for every report; the link for one.
        app.preferences.report_on_github = true;
        assert!(!ticket::enabled(&app));
        app.preferences.report_on_github = false;
        let mut draft = draft;
        draft.github = true;
        app.issue_report.draft = Some(draft);
        cancel_capture(&mut app);
        let _ = ctx.run_ui(Default::default(), |ui| show(&mut app, ui.ctx()));
        assert!(app.issue_report.draft.as_ref().unwrap().ticket.is_none(), "the GitHub draft, as before");
    }

    /// #17926: a report of a fillet the kernel refuses carries, as `repro.failing.code`, the
    /// stable code of the kernel call the failure came from: the same code the failed command's
    /// error has, and the same for a fillet of the timeline that fails so. A failure made from no
    /// kernel call sends none.
    #[test]
    fn a_failed_fillets_report_carries_its_kernel_code() {
        let wall = || {
            let mut s = Session::default();
            for (c, p) in [
                ("parameters.change", json!({"name":"top_radius", "expression":"0.5 mm"})),
                ("sketch.create", json!({"plane":"XY", "name":"Wall"})),
                ("sketch.rectangle.two_point", json!({"p0":[0,0], "p1":[30,2]})),
                ("sketch.finish", json!({})),
                ("solid.extrude", json!({"distance":"10 mm", "body_name":"Wall"})),
            ] {
                s.execute(c, &p).unwrap();
            }
            s
        };
        let edges = json!([[15, 0, 10], [15, 2, 10]]);
        let sent = |app: &mut SolveApp, error: &str| {
            let draft = draft_for(app, None, Some(error));
            let t = ticket::new_ticket(app, &draft, None);
            let all = payload::Ticks { system: true, repro: true, log: true, screenshot: true, design: true, crash: true };
            payload::build(&t.form, all, &t.evidence, "solidcraft test").ticket["repro"]["failing"].clone()
        };
        let as_json = |c: &solidcraft_engine::kernel::ErrorCode| json!({"call": c.call, "kind": c.kind, "operation": c.operation});

        // a new fillet the kernel refuses: its command's error
        let mut s = wall();
        let e = s.execute("solid.fillet", &json!({"edges": edges, "radius": "1.5 mm"})).unwrap_err();
        let code = e.error_code().cloned().expect("a kernel code");
        let mut app = SolveApp::new(s, Default::default());
        let failing = sent(&mut app, &e.to_string());
        assert_eq!(failing["code"], as_json(&code), "{failing}");

        // a fillet of the timeline that fails so after a parameter change: its result's
        let mut s = wall();
        s.execute("solid.fillet", &json!({"edges": edges, "radius": "top_radius"})).unwrap();
        let _ = s.execute("parameters.change", &json!({"name":"top_radius", "expression":"1.8 mm"}));
        let r = s.model.results.iter().find(|r| r.error.is_some()).cloned().unwrap();
        assert_eq!(r.error_code.as_ref(), Some(&code), "one failure, one code");
        let mut app = SolveApp::new(s, Default::default());
        let failing = sent(&mut app, r.error.as_deref().unwrap());
        assert_eq!(failing["code"], as_json(&code), "{failing}");

        // a failure no kernel call made
        let failing = sent(&mut app, "Synthetic command error");
        assert!(failing.get("code").is_none(), "{failing}");
    }

    #[test]
    fn report_issue_from_an_agent_opens_a_draft_and_never_sends_the_design_on_its_word() {
        let ctx = crate::fonts::test_context();
        let (mut app, _, requests, _) = ticket_app(true, None);
        app.home.open = false;
        frames_until(&mut app, &ctx, |_| true);
        let reply =
            app.run("diagnostics.report_issue", json!({"title": "Fillet refuses 2 mm", "description": "Seen while modelling a bracket"})).unwrap();
        assert_eq!(reply["status"], "pending");
        let id = reply["report"].as_str().unwrap().to_string();
        let opened = frames_until(&mut app, &ctx, |a| a.tickets.sending.is_none() && a.tickets.outcome.is_some());
        frames_until(&mut app, &ctx, |_| true);
        let status = agent_status(&mut app, &id);
        assert_eq!(status["status"], "opened", "{status}");
        assert!(status["url"].as_str().is_some_and(|u| opened.contains(&u.to_string())), "{status} {opened:?}");
        {
            let requests = requests.lock().unwrap();
            let draft = requests.iter().find(|r| r.url.ends_with("/api/drafts")).unwrap();
            let body: serde_json::Value = serde_json::from_slice(draft.body.as_deref().unwrap()).unwrap();
            assert_eq!((body["title"].as_str(), body["consent"]["via"].as_str()), (Some("Fillet refuses 2 mm"), Some("agent")));
            assert!(!body["consent"]["kinds"].as_array().unwrap().contains(&json!("design")));
            assert!(!requests.iter().any(|r| r.body.as_ref().is_some_and(|b| String::from_utf8_lossy(b).contains("\"kind\":\"design\""))));
        }
        no_token_anywhere(&requests);

        // The design asked for: nothing is sent; the dialog opens for the person, the box unticked.
        let (mut app, _, requests, _) = ticket_app(true, None);
        app.home.open = false;
        frames_until(&mut app, &ctx, |_| true);
        let reply = app.run("diagnostics.report_issue", json!({"title": "Needs the model", "design": true})).unwrap();
        let id = reply["report"].as_str().unwrap().to_string();
        frames_until(&mut app, &ctx, |a| a.issue_report.draft.is_some());
        cancel_capture(&mut app);
        let _ = ctx.run_ui(Default::default(), |ui| show(&mut app, ui.ctx()));
        assert_eq!(agent_status(&mut app, &id)["status"], "awaiting_user");
        assert!(requests.lock().unwrap().iter().all(|r| !r.url.contains("/api/drafts") && !r.url.contains("/api/uploads")), "nothing sent");
        let ticket = app.issue_report.draft.as_ref().unwrap().ticket.as_ref().unwrap();
        assert!(!ticket.ticks.design && ticket.form.title == "Needs the model");

        // The person ticks the model and sends: it goes, recorded as confirmed by them.
        let draft = app.issue_report.draft.as_mut().unwrap();
        let mut ticket = draft.ticket.take().unwrap();
        ticket.ticks.design = true;
        let report = ticket::outgoing(draft, &ticket);
        assert_eq!(report.ticket["consent"]["confirmed_by"], "user");
        app.issue_report.draft = None;
        tickets::send(&mut app, report);
        frames_until(&mut app, &ctx, |a| a.tickets.sending.is_none());
        frames_until(&mut app, &ctx, |_| true);
        assert_eq!(agent_status(&mut app, &id)["status"], "opened");
        assert!(requests.lock().unwrap().iter().any(|r| r.body.as_ref().is_some_and(|b| String::from_utf8_lossy(b).contains("\"kind\":\"design\""))));

        // Offline: the agent hears so; the report is kept for the person to send.
        let (mut app, _, _, _) = ticket_app(false, None);
        let dir = temp_dir("agent");
        app.tickets.queue_dir = Some(dir.clone());
        frames_until(&mut app, &ctx, |_| true);
        let id = app.run("diagnostics.report_issue", json!({"title": "Offline"})).unwrap()["report"].as_str().unwrap().to_string();
        frames_until(&mut app, &ctx, |a| a.tickets.outcome.is_some());
        frames_until(&mut app, &ctx, |_| true);
        assert_eq!(agent_status(&mut app, &id)["status"], "queued");
        assert_eq!(tickets::queue::list(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ticket_report_review_fixes_hold() {
        use tickets::api::{allowed_url, answer, on_server};
        // A crash report is offered, never ticked to start with.
        let (mut app, _, _, _) = ticket_app(true, None);
        let ctx = crate::fonts::test_context();
        begin_crash(
            &mut app,
            &ctx,
            &json!({"id": "c1", "kind": "command_panic", "message": "boom", "command": {"command": "solid.fillet", "params": {"radius": 2}}}),
        );
        let draft = app.issue_report.draft.take().unwrap();
        let t = ticket::new_ticket(&mut app, &draft, None);
        assert!(t.evidence.crash.is_some() && !t.ticks.crash && !t.ticks.design);
        assert_eq!(t.evidence.failing.as_ref().and_then(|f| f.origin.as_deref()), Some("app"), "from the crash's kind");

        // Only a refusal of the report itself is final; a busy or broken server is tried again.
        for status in [408, 429, 500, 502, 503] {
            assert!(matches!(answer(Ok((status, b"{}".to_vec()))), Err(tickets::api::Failure::Offline(_))), "{status}");
        }
        assert!(matches!(answer(Ok((400, b"{}".to_vec()))), Err(tickets::api::Failure::Refused(_))));

        // Hosts are compared exactly, not by prefix.
        let server = tickets::api::server();
        assert!(on_server(&format!("{server}/draft/abc#s"), "/draft/"));
        // The server's own page, whatever its path (it moved to /d/).
        let moved = json!({"draft_id": "d2", "upload_token": "tdu_x", "url": format!("{server}/d/d2#s")});
        assert_eq!(tickets::api::parse_draft(&moved).map(|d| d.url), Ok(format!("{server}/d/d2#s")));
        let elsewhere = json!({"draft_id": "d2", "upload_token": "tdu_x", "url": "https://tickets.solidcraft.io.evil.example/d/d2#s"});
        assert!(tickets::api::parse_draft(&elsewhere).is_err());
        for fake in [
            "https://tickets.solidcraft.io.evil.example/draft/x",
            "https://tickets.solidcraft.io@evil.example/draft/x",
            "https://evil.example/https://tickets.solidcraft.io/draft/x",
            "http://tickets.solidcraft.io/draft/x",
        ] {
            assert!(!on_server(fake, "/draft/"), "{fake}");
        }
        assert!(allowed_url("http://127.0.0.1:8787/api/x") && allowed_url("http://localhost/api") && allowed_url("https://x.example/"));
        for fake in ["http://127.0.0.1.evil.example/", "http://localhost.evil.example/", "http://localhost@evil.example/", "ftp://localhost/"] {
            assert!(!allowed_url(fake), "{fake}");
        }

        // How a report closed while it was sent went is not the next report's.
        let (mut app, _, _, _) = ticket_app(true, None);
        let first = tickets::new_owner(&mut app);
        tickets::send_for(&mut app, payload::build(&form(), payload::Ticks::default(), &evidence_with_design(), "t"), first);
        frames_until(&mut app, &ctx, |a| a.tickets.sending.is_none());
        assert_eq!((app.tickets.owner, app.tickets.outcome.clone()), (first, Some(tickets::Outcome::Opened(draft_page()))));
        let draft = draft_for(&app, None, None);
        let next = ticket::new_ticket(&mut app, &draft, None);
        assert_ne!(next.id, first, "a new report starts with nothing sent");

        // The server refusing a kept report removes it and its files.
        let dir = temp_dir("refused");
        let (mut app, online, _, _) = ticket_app(false, None);
        app.tickets.queue_dir = Some(dir.clone());
        let refused = payload::Form { title: "REFUSE-ME".into(), ..form() };
        tickets::send(&mut app, payload::build(&refused, payload::Ticks::default(), &evidence_with_design(), "t"));
        frames_until(&mut app, &ctx, |a| a.tickets.sending.is_none());
        assert_eq!(tickets::queue::list(&dir).len(), 1);
        online.store(true, std::sync::atomic::Ordering::SeqCst);
        tickets::retry(&mut app, 0);
        frames_until(&mut app, &ctx, |a| a.tickets.sending.is_none());
        assert!(matches!(app.tickets.outcome, Some(tickets::Outcome::Failed(_))));
        assert!(std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) == 0, "no report or file left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #17911: a report made from a crash report attaches the crashed session's design (the
    /// crash report's copy), not the empty design open now; with no copy, none, and it says so.
    #[test]
    fn ticket_report_from_a_crash_attaches_the_crashed_sessions_design_or_says_there_is_none() {
        let ctx = crate::fonts::test_context();
        let (mut app, _, _, _) = ticket_app(true, None);
        let dir = temp_dir("crash-design");
        std::fs::create_dir_all(&dir).unwrap();
        let mut crashed = Session::default();
        crashed.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let bytes = solidcraft_engine::io::write_design(&crashed.doc).unwrap();
        let design = dir.join("design.sc");
        std::fs::write(&design, &bytes).unwrap();
        let report = json!({
            "id": "c1", "kind": "unclean_exit", "message": "ended without a clean exit",
            "command": {"command": "solid.pattern.rectangular", "params": {"count1": 5}},
            "base": {"file": "/somewhere/Vent Panel.sc"},
            "design_path": design.to_string_lossy(),
            "recent_commands": [
                {"command": "doc.open", "params": {"path": "/somewhere/Vent Panel.sc"}, "ok": true},
                {"command": "select.set", "params": {"items": [{"type": "body", "name": "Body1"}]}, "ok": true},
                {"command": "solid.fillet", "params": {"radius": 9}, "ok": false, "error": "radius too large"},
            ],
        });
        begin_crash(&mut app, &ctx, &report);
        let draft = app.issue_report.draft.take().unwrap();
        let t = ticket::new_ticket(&mut app, &draft, None);
        let ev = &t.evidence;
        assert_eq!(ev.design.as_ref().map(|(n, b)| (n.as_str(), b.as_slice())), Some(("Vent Panel.sc", bytes.as_slice())));
        assert_ne!(ev.design.as_ref().unwrap().1, solidcraft_engine::io::write_design(&app.session.doc).unwrap(), "not the design open now");
        assert_eq!(ev.design_from, Some("report.design_from_crash"));
        assert_eq!(ev.session["session_start"], "crashed");
        assert_eq!(ev.session["design_sha256"], json!(tickets::api::sha256_hex(&bytes)));
        // The crashed session's steps, opens included; a file's path is reduced to its name.
        let steps: Vec<&str> = ev.steps.iter().filter_map(|s| s["command"].as_str()).collect();
        assert_eq!(steps, ["doc.open", "select.set", "solid.fillet"]);
        assert_eq!((ev.steps[0]["file"].as_str(), ev.steps[0].get("params")), (Some("Vent Panel.sc"), None));
        assert_eq!(ev.steps[2]["error"], "radius too large");
        let sent = payload::build(&form(), payload::Ticks { design: true, ..Default::default() }, ev, "t");
        assert_eq!(sent.ticket["repro"]["steps"].as_array().map(Vec::len), Some(3));
        assert!(sent.files.iter().any(|f| f.kind == "design" && f.name == "Vent Panel.sc" && f.bytes == bytes));

        // No copy of the design: nothing attached, and the report and the dialog say why.
        let mut report = report;
        report.as_object_mut().unwrap().remove("design_path");
        begin_crash(&mut app, &ctx, &report);
        let draft = app.issue_report.draft.take().unwrap();
        let t = ticket::new_ticket(&mut app, &draft, None);
        assert!(t.evidence.design.is_none(), "never the empty design open now");
        assert_eq!(t.evidence.design_from, Some("report.design_none_crash"));
        assert!(t.evidence.session["design"].as_str().is_some_and(|d| d.starts_with("none")));
        assert!(t.evidence.session.get("design_sha256").is_none());
        let sent = payload::build(&form(), payload::Ticks { design: true, ..Default::default() }, &t.evidence, "t");
        assert!(!sent.files.iter().any(|f| f.kind == "design"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A report made in a new, empty design while a design from a run that ended (a lost
    /// graphics device) waits for recovery attaches that design (#17911).
    #[test]
    fn ticket_report_in_an_empty_design_attaches_the_design_left_for_recovery() {
        let (mut app, _, _, _) = ticket_app(true, None);
        let dir = temp_dir("recovery");
        let mut left = Session::default();
        left.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        std::sync::Arc::make_mut(&mut left.doc).name = "Light Switch".into();
        let mut saver = solidcraft_engine::recovery::Autosaver::new(&dir, std::time::Duration::from_secs(1)).unwrap();
        assert!(saver.save(&left).unwrap());
        drop(saver);
        crate::recovery_ui::start(&mut app, &dir, std::time::Duration::from_secs(600));
        assert_eq!(app.recoverable.len(), 1);
        let draft = draft_for(&app, None, None);
        let t = ticket::new_ticket(&mut app, &draft, None);
        let (name, bytes) = t.evidence.design.clone().unwrap();
        assert_eq!(name, "Light Switch.sc");
        assert_eq!(solidcraft_engine::io::read_design(&bytes).unwrap().features.len(), left.doc.features.len());
        assert_eq!((t.evidence.design_from, &t.evidence.session["session_start"]), (Some("report.design_from_recovery"), &json!("recovered")));

        // A design with something in it is the one the report is about.
        app.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).unwrap();
        let draft = draft_for(&app, None, None);
        let t = ticket::new_ticket(&mut app, &draft, None);
        assert_eq!(t.evidence.design_from, None);
        let attached = solidcraft_engine::io::read_design(&t.evidence.design.unwrap().1).unwrap();
        assert_eq!(attached.features, app.session.doc.features);
        if let Some(a) = app.autosave.take() {
            drop(a);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// "Recent steps" were empty: the report sent no steps at all. Every command the session
    /// ran goes, with its parameters, opens and selections included; a gesture's frames do not.
    #[test]
    fn ticket_report_recent_steps_are_the_commands_the_session_ran() {
        let (mut app, _, _, _) = ticket_app(true, None);
        app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        app.run("select.set", json!({"items": [{"type": "body", "name": "Body1"}]})).unwrap();
        assert!(app.run("solid.fillet", json!({"edges": [], "radius": -1})).is_err());
        // An open finishes on a worker: it is recorded without its folders.
        app.finish_command("doc.open", &Ok(json!({"path": "/home/someone/clients/Bracket.sc"})));
        ticket::step_params(&mut app, "doc.open", &json!({"path": "/home/someone/clients/Bracket.sc"}));
        app.finish_command("view.navigate", &Ok(json!({"continuous": true})));
        let draft = draft_for(&app, None, None);
        let t = ticket::new_ticket(&mut app, &draft, None);
        let steps = &t.evidence.steps;
        let ids: Vec<&str> = steps.iter().filter_map(|s| s["command"].as_str()).collect();
        assert_eq!(ids, ["solid.box", "select.set", "solid.fillet", "doc.open"]);
        assert_eq!(steps[0]["params"]["length"], 10);
        assert_eq!(steps[1]["params"]["items"][0]["name"], "Body1");
        assert!(steps[2]["ok"] == false && steps[2]["error"].as_str().is_some_and(|e| !e.is_empty()));
        assert!(steps[3].get("params").is_none() && steps[3]["file"] == "Bracket.sc");
        // The parameters (names in the design) go only with the design, which holds them anyway.
        let with_design = payload::build(&form(), payload::Ticks { design: true, ..Default::default() }, &t.evidence, "t");
        assert_eq!(with_design.ticket["repro"]["steps"], json!(steps));
        let without = payload::build(&form(), payload::Ticks::default(), &t.evidence, "t");
        let bare: Vec<serde_json::Value> = steps
            .iter()
            .map(|s| {
                let mut s = s.clone();
                let o = s.as_object_mut().unwrap();
                o.remove("params");
                o.remove("file");
                s
            })
            .collect();
        assert_eq!(without.ticket["repro"]["steps"], json!(bare));
        assert!(!without.ticket.to_string().contains("Body1") && !without.ticket.to_string().contains("Bracket"));
        assert!(payload::repro_text(&t.evidence, false).contains("\"steps\""), "the dialog shows what goes");
        assert!(!payload::repro_text(&t.evidence, false).contains("Body1") && payload::repro_text(&t.evidence, true).contains("Body1"));
        // Bounded: the last MAX_STEPS.
        for _ in 0..ticket::MAX_STEPS + 5 {
            let _ = app.run("select.clear", json!({}));
        }
        assert_eq!(app.issue_report.steps.len(), ticket::MAX_STEPS);
    }

    /// The log a report attaches is the end of the log file on disk, the run before's end first
    /// when this run has little, bounded in lines and bytes; not two lines of UI notices.
    #[test]
    fn ticket_report_log_is_the_end_of_the_log_file() {
        let dir = temp_dir("log");
        std::fs::create_dir_all(&dir).unwrap();
        let previous: String = (0..1000).map(|i| format!("before {i}\n")).collect();
        std::fs::write(dir.join(crate::log_file::PREVIOUS), previous).unwrap();
        std::fs::write(dir.join(crate::log_file::FILE), "now 1\nnow 2\n").unwrap();
        let lines = crate::log_file::tail_in(&dir, ticket::LOG_LINES, ticket::LOG_BYTES);
        assert_eq!(lines.len(), ticket::LOG_LINES);
        assert_eq!(lines[lines.len() - 3..], ["--- the run before ---", "now 1", "now 2"]);
        assert_eq!(lines[lines.len() - 4], "before 999");
        // Bytes bound it too, and a line cut by the bound is left out whole.
        let long: String = (0..400).map(|i| format!("{i:04} {}\n", "x".repeat(1000))).collect();
        std::fs::write(dir.join(crate::log_file::FILE), long).unwrap();
        let lines = crate::log_file::tail_in(&dir, ticket::LOG_LINES, ticket::LOG_BYTES);
        let bytes: usize = lines.iter().map(|l| l.len() + 1).sum();
        assert!(bytes as u64 <= ticket::LOG_BYTES && lines.len() > 100, "{} lines, {bytes} bytes", lines.len());
        let now = lines.iter().position(|l| l == crate::log_file::RUN_MARK).map_or(0, |i| i + 1);
        assert!(lines[now..].iter().all(|l| l.len() == 1005) && lines.last().unwrap().starts_with("0399"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The error in a report's title and description is cut at a word, with "…", never mid-word
    /// ("…Surface Natur"); the whole error stays in the failed operation's details.
    #[test]
    fn ticket_report_error_is_cut_at_a_word_and_kept_whole_in_the_details() {
        let error = format!("SMLib boolean: SMLib's check refuses the result: {}Surface Natural Boundary", "Face has Coincident Edges; ".repeat(60));
        let failing = payload::Failing { command: Some("solid.extrude".into()), error: error.clone(), ..Default::default() };
        let mut app = failing_sample();
        let mut draft = draft_for(&app, None, Some(&error));
        draft.title_prefix = "EXTRUDE: ".into();
        let t = ticket::new_ticket(&mut app, &draft, None);
        for text in [t.form.title.as_str(), t.form.description.lines().find(|l| l.starts_with("**Error:**")).unwrap()] {
            assert!(text.ends_with('…'), "{text}");
            let kept = text.trim_end_matches('…').trim_start_matches("**Error:** ");
            assert!(error.starts_with(kept.trim_start_matches("EXTRUDE: ")) || t.form.title == text, "{text}");
            let next = error.get(kept.trim_start_matches("EXTRUDE: ").len()..).unwrap_or_default();
            assert!(next.is_empty() || next.starts_with(char::is_whitespace) || next.starts_with(';'), "cut mid-word: {text}");
        }
        assert!(t.form.title.chars().count() <= payload::MAX_TITLE);
        assert_eq!(t.evidence.failing.as_ref().map(|f| f.error.as_str()), Some(error.as_str()));
        assert_eq!(payload::failing_json(&failing)["error"], json!(error));
        assert_eq!(ticket::readable("short", 10), "short");
        assert_eq!(ticket::readable("one two three four", 12), "one two…");
        assert_eq!(ticket::readable("first line\nsecond line goes on", 20), "first line…");
        assert_eq!(ticket::readable("Überlänge", 5), "Über…", "one long word is cut at a character");
    }

    /// A description left as the template, with "What were you trying to do?" unanswered, goes
    /// without the bare question; an answer keeps it.
    #[test]
    fn ticket_report_drops_the_unanswered_question() {
        crate::i18n::install();
        let q = solidcraft_ui_text::tr("report.trying_to_do");
        let template = format!("**Failed:** Fillet (`solid.fillet`)\n**Error:** radius too large\n\n{q}\n");
        assert_eq!(ticket::without_unanswered(&template), "**Failed:** Fillet (`solid.fillet`)\n**Error:** radius too large\n");
        assert_eq!(ticket::without_unanswered(&format!("{template}  \n\n")), "**Failed:** Fillet (`solid.fillet`)\n**Error:** radius too large\n");
        let answered = format!("{template}Rounding the top edge\n");
        assert_eq!(ticket::without_unanswered(&answered), answered);
        let same_line = template.replace(q, &format!("{q} Rounding the top edge"));
        assert_eq!(ticket::without_unanswered(&same_line), same_line);
        assert_eq!(ticket::without_unanswered(q), "");

        // Sent: the ticket's body has no bare question, and the title is not made from it.
        let (mut app, _, _, _) = ticket_app(true, None);
        // Read again: making the app loads the message catalogs, which a test run alone in its
        // own process has not loaded before.
        let q = solidcraft_ui_text::tr("report.trying_to_do");
        let mut draft = draft_for(&app, None, Some("radius too large"));
        draft.title_prefix = "Fillet: ".into();
        let mut t = ticket::new_ticket(&mut app, &draft, None);
        assert!(t.form.description.contains(q), "{}", t.form.description);
        t.form.title.clear();
        ticket::ready_to_send(&mut t.form);
        let report = ticket::outgoing(&draft, &t);
        assert!(!report.ticket["body"].as_str().unwrap().contains(q), "{}", report.ticket["body"]);
        assert_ne!(report.ticket["title"].as_str(), Some(q));
    }
}
