//! Which values a link passes to its design: the Parameters section of Insert Design, and the
//! Link Parameters… dialog of a linked component (#18905).
//!
//! Both list the user parameters of the linked design (`component.design_parameters`): each
//! with its value in that design, the value this design would give it by name, an expression of
//! the link's own, and Keep its own. A switch takes every parameter that shares a name with one
//! of this design's. Insert Design asks only when the picked design has user parameters a link
//! can set, and its OK inserts with them (`parameters`, `by_name` and `keep` of
//! `component.insert_design`); Cancel inserts nothing. The Link Parameters… dialog's OK runs
//! `component.link_parameters`; a refusal keeps the dialog open with the reason.
//!
//! Only the interactive entry points ask ([`ask_insert`] from Insert Design's file picker and its
//! list of designs open in other tabs): a
//! script, the control channel or MCP give the commands their own values and never meet a dialog.
//!
//! The linked design is read beside the window ([`Reading`]): a large or slow file never holds
//! the window up, and Cancel drops the read. Link Parameters…'s OK runs as a history command
//! does ([`SolveApp::run_heavy`]): a long rebuild computes beside the window, Stop drops it. Its
//! draft waits with it (`SolveApp::link_parameters_waiting`): a refusal that comes once the
//! rebuild is in opens the dialog again with the reason, and Stop opens it as it was. Until then
//! the design opens no other of these dialogs, and no read ever takes the place of an open one,
//! whichever finishes first (the status line says why). Both the
//! read and the open dialog belong to the design they were opened in: they go with its tab and
//! come back when it is shown again, whatever the "Keep dialogs and tools open when switching
//! designs" preference (`documents::take_work`), since the design they link is often open in
//! another tab; they act on no other design. The waiting draft goes with the design's computing
//! commands, and each design's inserting draft waits for its own insert.

use serde_json::{Value, json};
use solidcraft_engine::doc::expr::Kind;

use crate::ui_kit::BackingTooltipExt as _;
use crate::{SolveApp, file_load, ui_kit};

/// What the dialog's OK sets.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// The design at `path`, inserted on OK; `hosts`: the network servers its reading may use;
    /// `note`: what the status line adds once it is in (an open design's unsaved changes).
    Insert { path: String, hosts: Vec<String>, note: Option<String> },
    /// The link of this component.
    Link { component: u64 },
}

/// One user parameter of the linked design.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub unit: String,
    /// Its value in the linked design, in its own unit, for showing.
    pub design: String,
    /// The value this design would give it by name, for showing; `None` when it has no user
    /// parameter of that name and kind.
    pub here: Option<String>,
    /// The link's own expression for it; empty for none.
    pub expression: String,
    /// Keep its own value even when taken by name would set it.
    pub keep: bool,
}

/// The dialog while it is open.
#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub target: Target,
    /// The design tab it was opened in (`documents::DocSlot::tab`).
    pub tab: u64,
    /// The linked design's file, for the line under the title.
    pub file: String,
    pub rows: Vec<Row>,
    pub by_name: bool,
    /// The link's file changed since this design read it (Link Parameters…): the rows are the
    /// file's as it is now, and OK sets them only with [`Self::update`].
    pub changed: bool,
    /// Take in the changed file with these values, as Update Links does.
    pub update: bool,
    /// Parameters the link has an expression for that the design no longer has (renamed or
    /// deleted there): OK gives them up, and the dialog says so.
    pub gone: Vec<String>,
    /// Why the last OK was refused.
    pub error: Option<String>,
    /// Link Parameters…: what the link read its design from when these rows were read
    /// ([`link_source`]). A link that reads another design since is read again ([`follow_source`]).
    pub source: Option<String>,
}

impl Form {
    /// The form for a `component.design_parameters` reply, in tab `tab`.
    fn from_reply(target: Target, tab: u64, reply: &Value) -> Form {
        let set = reply["set"].as_object();
        let kept: Vec<&str> = reply["keep"].as_array().map(|k| k.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let rows: Vec<Row> = reply["parameters"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|p| {
                        let name = p["name"].as_str()?.to_string();
                        let unit = p["unit"].as_str().unwrap_or_default().to_string();
                        let design = match p["display_value"].as_f64() {
                            Some(v) => crate::dialogs_assembly::num(v, &unit).trim_end().to_string(),
                            None => p["expression"].as_str().unwrap_or_default().to_string(),
                        };
                        let here = p["here"].as_f64().map(|v| crate::dialogs_assembly::num(v, reported_unit(&unit)).trim_end().to_string());
                        let expression = set.and_then(|s| s.get(&name)).and_then(Value::as_str).unwrap_or_default().to_string();
                        let keep = kept.contains(&name.as_str());
                        Some(Row { name, unit, design, here, expression, keep })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let gone = set.map(|s| s.keys().filter(|name| !rows.iter().any(|r| r.name == **name)).cloned().collect()).unwrap_or_default();
        Form {
            target,
            tab,
            file: reply["file"].as_str().unwrap_or_default().to_string(),
            rows,
            by_name: reply["by_name"].as_bool().unwrap_or(false),
            changed: reply["changed"].as_bool().unwrap_or(false),
            update: false,
            gone,
            error: None,
            source: None,
        }
    }

    /// The link's own expressions: each row's, null for a row without one (so a link that had
    /// one gives it back to the design) and for a parameter the design no longer has.
    fn set(&self) -> serde_json::Map<String, Value> {
        self.rows
            .iter()
            .map(|r| {
                let e = r.expression.trim();
                (r.name.clone(), if e.is_empty() || r.keep { Value::Null } else { json!(e) })
            })
            .chain(self.gone.iter().map(|name| (name.clone(), Value::Null)))
            .collect()
    }

    /// This draft over `fresh`, a new read of the design: each row of the design as it is now
    /// keeps what was typed in it here; a parameter the design no longer has that the link set,
    /// or that was given an expression here, is to go.
    fn merged(self, fresh: Form) -> Form {
        let rows: Vec<Row> = fresh
            .rows
            .into_iter()
            .map(|mut r| {
                if let Some(typed) = self.rows.iter().find(|o| o.name == r.name) {
                    r.expression.clone_from(&typed.expression);
                    r.keep = typed.keep;
                }
                r
            })
            .collect();
        let mut gone = fresh.gone;
        for old in &self.rows {
            if !old.expression.trim().is_empty() && !rows.iter().any(|r| r.name == old.name) && !gone.contains(&old.name) {
                gone.push(old.name.clone());
            }
        }
        Form { rows, gone, changed: fresh.changed, file: fresh.file, source: fresh.source, ..self }
    }

    fn keep(&self) -> Vec<&str> {
        self.rows.iter().filter(|r| r.keep).map(|r| r.name.as_str()).collect()
    }
}

/// The unit `here` (and every reply's `value`) is in for a parameter of `unit`: mm for a length,
/// degrees for an angle.
fn reported_unit(unit: &str) -> &'static str {
    match Kind::from_unit(unit) {
        Kind::Length => "mm",
        Kind::Angle => "deg",
        Kind::Unitless => "",
    }
}

/// The linked design's parameters, being read beside the window for the dialog.
pub struct Reading {
    done: std::sync::mpsc::Receiver<Result<Value, String>>,
    target: Target,
    /// The design tab it was asked in.
    tab: u64,
    /// When it started (ms): the window shows it only once it takes a while.
    started: f64,
    /// Read again for the dialog that is open, after its OK was refused ([`refused`]): it
    /// refreshes that dialog's rows, keeping what was typed, instead of opening one.
    refresh: bool,
    /// What the link read its design from when the read started ([`link_source`]).
    source: Option<String>,
}

/// What the link of `target` reads its design from, as a dialog and its read remember it: the
/// file's name, its bytes as this design last read them, and the configuration row. `None` for
/// Insert Design and for a component that is no longer linked. The folder is left out: saving
/// this design rewrites it relative to the design, which changes no source.
fn link_source(app: &SolveApp, target: &Target) -> Option<String> {
    let Target::Link { component } = target else { return None };
    let link = app.session.doc.links.get(component)?;
    let name = link.path.rsplit('/').next().unwrap_or_default();
    Some(format!("{name}\n{}\n{:?}", link.file_sha256.as_deref().unwrap_or_default(), link.configuration))
}

/// A Link Parameters dialog, or the read that opens one, whose link reads another design since
/// (Relink Source, Update Links, an undo of either) or is gone: its rows and what was typed in
/// them are the old design's, so they are dropped and the link is read again as it is now. They
/// are never set on another design than the one they were read from.
fn follow_source(app: &mut SolveApp) {
    let tab = app.docs.active_tab();
    let mut again = None;
    if let Some(form) = app.link_parameters.take_if(|f| f.tab == tab && matches!(f.target, Target::Link { .. })) {
        if link_source(app, &form.target) == form.source {
            app.link_parameters = Some(form);
        } else {
            again = Some(form.target);
        }
    }
    if let Some(reading) = app.link_parameters_reading.take_if(|r| r.tab == tab && matches!(r.target, Target::Link { .. })) {
        if link_source(app, &reading.target) == reading.source {
            app.link_parameters_reading = Some(reading);
        } else if !reading.refresh {
            again = Some(reading.target);
        }
    }
    if let Some(Target::Link { component }) = again
        && app.session.doc.links.contains_key(&component)
    {
        open_link(app, component);
    }
}

/// How long a read goes unseen before the window says it is reading (ms).
const SHOW_READING_AFTER: f64 = 200.0;

/// Read `component.design_parameters {params}` beside the window, on a copy of the design (its
/// parameter values and where it is saved, which a link's path is relative to).
/// Whether this design's last Link Parameters OK still computes: another of its dialogs waits
/// until it is done, so the draft that comes back when it is refused never meets another one.
/// Says so in the status line.
fn busy(app: &mut SolveApp) -> bool {
    let key = if app.link_parameters_waiting.is_some() {
        "ui.link_parameters.busy"
    } else if app.link_parameters_inserting.iter().any(|(_, f)| f.tab == app.docs.current_tab()) {
        "ui.link_parameters.inserting"
    } else {
        return false;
    };
    app.set_status(solidcraft_ui_text::tr(key), true);
    true
}

/// The file operation `serial` ended: an Insert Design draft waiting for it is done with when it
/// inserted, and otherwise comes back, with the reason when it failed ([`bring_back`]).
pub(crate) fn insert_ended(app: &mut SolveApp, serial: u64, outcome: &crate::operations::Outcome) {
    let Some(at) = app.link_parameters_inserting.iter().position(|(waiting, _)| *waiting == serial) else { return };
    let (_, mut form) = app.link_parameters_inserting.remove(at);
    match outcome {
        crate::operations::Outcome::Done => {}
        crate::operations::Outcome::Failed(e) => {
            form.error = Some(e.clone());
            app.link_parameters_back.push(form);
        }
        crate::operations::Outcome::Cancelled => app.link_parameters_back.push(form),
    }
}

/// The Insert Design drafts whose insert is no longer under way and never said how it ended
/// come back as they were; one that came back opens once its design is shown and no other of
/// these dialogs is open or being read. A draft whose design's tab was closed goes with it.
fn bring_back(app: &mut SolveApp) {
    if app.file_load.idle() && !app.link_parameters_inserting.is_empty() {
        let ended = std::mem::take(&mut app.link_parameters_inserting);
        app.link_parameters_back.extend(ended.into_iter().map(|(_, form)| form));
    }
    let docs = &app.docs;
    app.link_parameters_back.retain(|f| docs.tab_open(f.tab));
    if app.link_parameters.is_some() || app.link_parameters_reading.is_some() {
        return;
    }
    let tab = app.docs.active_tab();
    if let Some(at) = app.link_parameters_back.iter().position(|f| f.tab == tab) {
        app.link_parameters = Some(app.link_parameters_back.remove(at));
    }
}

/// Whether one of these dialogs is open (one brought back with a refusal too): another read
/// never takes its place, so what was typed in it is never lost. Says so in the status line.
fn open_already(app: &mut SolveApp) -> bool {
    if app.link_parameters.is_none() {
        return false;
    }
    app.set_status(solidcraft_ui_text::tr("ui.link_parameters.open_already"), true);
    true
}

fn start_reading(app: &mut SolveApp, target: Target, params: Value, hosts: Vec<String>) {
    if busy(app) || open_already(app) {
        return;
    }
    read_beside(app, target, params, hosts, false);
}

fn read_beside(app: &mut SolveApp, target: Target, params: Value, hosts: Vec<String>, refresh: bool) {
    let mut copy = app.session.scratch();
    copy.path.clone_from(&app.session.path);
    let (tx, done) = std::sync::mpsc::channel();
    let read = move || {
        let r = solidcraft_engine::io::vfs::allowing_hosts(&hosts, || copy.execute("component.design_parameters", &params));
        let _ = tx.send(r.map_err(|e| e.to_string()));
    };
    // A thread that can't start leaves the read with no sender: `poll` takes that as a read that
    // failed. The browser build has no threads and reads here, as it reads every file.
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::thread::Builder::new().name("link-parameters".into()).spawn(read);
    #[cfg(target_arch = "wasm32")]
    {
        let mut read = read;
        read();
    }
    let tab = app.docs.active_tab();
    let source = link_source(app, &target);
    app.link_parameters_reading = Some(Reading { done, target, tab, started: crate::now_ms(), refresh, source });
}

/// Insert Design picked the design at `path`: its parameters are read beside the window. When
/// it has user parameters a link can set, this dialog opens and its OK inserts it; otherwise,
/// or when it can't be read here, the insert goes on as before (it says what is wrong itself).
/// `note`: what the status line adds once it is in (a design open in another tab with unsaved
/// changes, whose saved file is the one linked).
pub fn ask_insert(app: &mut SolveApp, path: &str, note: Option<String>) {
    let hosts: Vec<String> = solidcraft_engine::io::vfs::network_host(path).into_iter().collect();
    start_reading(app, Target::Insert { path: path.to_string(), hosts: hosts.clone(), note }, json!({"path": path}), hosts);
}

/// Link Parameters… for the linked `component`.
pub fn open_link(app: &mut SolveApp, component: u64) {
    start_reading(app, Target::Link { component }, json!({"component": component}), Vec::new());
}

/// Take in a finished read: the dialog opens, or the insert goes on without it.
pub(crate) fn poll(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(reading) = &app.link_parameters_reading else { return };
    let outcome = match reading.done.try_recv() {
        Ok(r) => r,
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return;
        }
        // Its thread is gone without a word: the insert goes on without the dialog.
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(String::new()),
    };
    let Some(reading) = app.link_parameters_reading.take() else { return };
    // A read asked in another design's tab is that design's (it travels with its tab).
    if reading.tab != app.docs.active_tab() {
        return;
    }
    if reading.refresh {
        refreshed(app, &reading.target, reading.tab, reading.source, outcome);
        return;
    }
    finish(app, reading.target, reading.tab, reading.source, outcome);
}

/// OK was refused: the dialog opens again with what was typed and why. A Link Parameters dialog
/// also reads the design again beside the window ([`refreshed`]): when its file changed since
/// the dialog read it (a parameter renamed or removed there), the dialog then lists the file as
/// it is now and offers to take it in, so the draft is never lost to a cancel.
pub(crate) fn refused(app: &mut SolveApp, mut form: Form, error: String) {
    form.error = Some(error);
    let target = form.target.clone();
    app.link_parameters = Some(form);
    if let Target::Link { component } = target {
        read_beside(app, target, json!({"component": component}), Vec::new(), true);
    }
}

/// A read again for the open dialog finished: its rows become the design's as it is now, each
/// keeping what was typed in it, and what the link set that the design no longer has is said
/// to go. Nothing when the dialog was closed or is another one by then, or the read failed (the
/// dialog keeps the reason it was refused).
fn refreshed(app: &mut SolveApp, target: &Target, tab: u64, source: Option<String>, outcome: Result<Value, String>) {
    let Ok(reply) = outcome else { return };
    let Some(form) = app.link_parameters.take_if(|f| f.target == *target && f.tab == tab) else { return };
    app.link_parameters = Some(form.merged(Form { source, ..Form::from_reply(target.clone(), tab, &reply) }));
}

fn finish(app: &mut SolveApp, target: Target, tab: u64, source: Option<String>, outcome: Result<Value, String>) {
    // A read that started before the last OK, finished while that computes, or once a dialog is
    // open (a refused draft brought back): nothing opens in its place.
    if busy(app) || open_already(app) {
        return;
    }
    match (target, outcome) {
        (Target::Insert { path, hosts, note }, Ok(reply)) => {
            let mut form = Form::from_reply(Target::Insert { path: path.clone(), hosts, note: note.clone() }, tab, &reply);
            if form.rows.is_empty() {
                app.insert_design_async(&path, note);
                return;
            }
            // Unticked, even when names match: Insert Design keeps the design's saved sizes until
            // the user asks for this design's.
            form.by_name = false;
            app.link_parameters = Some(form);
        }
        (Target::Insert { path, note, .. }, Err(_)) => app.insert_design_async(&path, note),
        (target @ Target::Link { .. }, Ok(reply)) => app.link_parameters = Some(Form { source, ..Form::from_reply(target, tab, &reply) }),
        (Target::Link { .. }, Err(e)) if !e.is_empty() => app.set_status(e, true),
        (Target::Link { .. }, Err(_)) => {}
    }
}

/// While a read takes a while: a window saying so, whose Cancel drops it.
fn show_reading(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(reading) = &app.link_parameters_reading else { return };
    if crate::now_ms() - reading.started < SHOW_READING_AFTER {
        return;
    }
    let title = match reading.target {
        Target::Insert { .. } => solidcraft_ui_text::tr("cmd.component_insert_design"),
        Target::Link { .. } => solidcraft_ui_text::tr("cmd.component_link_parameters"),
    };
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_link_parameters_reading"), "chain", title).show(ctx, false, |ui| {
        ui_kit::text(ui, solidcraft_ui_text::tr("file_load.reading"), ui_kit::Text::Report("link_parameters_reading", false));
    });
    if closed || matches!(action, ui_kit::Action::Cancel) {
        app.link_parameters_reading = None;
    }
}

/// The history command `serial` ended, with `error` when it was refused: a dialog waiting for it
/// opens again with the reason, unless another one is open by then (the status line says why
/// too). Done, it is closed for good.
pub(crate) fn heavy_ended(app: &mut SolveApp, serial: u64, error: Option<&String>) {
    if app.link_parameters_waiting.as_ref().is_none_or(|(waiting, _)| *waiting != serial) {
        return;
    }
    let Some((_, mut form)) = app.link_parameters_waiting.take() else { return };
    if let Some(error) = error
        && app.link_parameters.is_none()
    {
        form.error = None;
        refused(app, form, error.clone());
    }
}

/// The design in the shown tab was replaced (New, Open): the dialogs, reads and waiting drafts of
/// the design it replaced go with it, so none of them is set on, or inserts into, the new one.
pub(crate) fn design_replaced(app: &mut SolveApp) {
    let tab = app.docs.active_tab();
    app.link_parameters.take_if(|f| f.tab == tab);
    app.link_parameters_reading.take_if(|r| r.tab == tab);
    app.link_parameters_waiting.take_if(|(_, f)| f.tab == tab);
    app.link_parameters_inserting.retain(|(_, f)| f.tab != tab);
    app.link_parameters_back.retain(|f| f.tab != tab);
}

/// A dialog waiting for its OK whose command is no longer under way and never ended (Stop
/// dropped it): it opens again as it was.
fn stopped(app: &mut SolveApp) {
    let Some((serial, _)) = &app.link_parameters_waiting else { return };
    if app.heavy_pending(*serial) {
        return;
    }
    let Some((_, form)) = app.link_parameters_waiting.take() else { return };
    if app.link_parameters.is_none() && form.tab == app.docs.active_tab() {
        app.link_parameters = Some(form);
    }
}

/// The dialog, while it is open.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    follow_source(app);
    poll(app, ctx);
    stopped(app);
    bring_back(app);
    show_reading(app, ctx);
    let Some(mut form) = app.link_parameters.clone() else { return };
    let title = match form.target {
        Target::Insert { .. } => solidcraft_ui_text::tr("cmd.component_insert_design"),
        Target::Link { .. } => solidcraft_ui_text::tr("cmd.component_link_parameters"),
    };
    let file = std::path::Path::new(&form.file).file_name().map_or_else(|| form.file.clone(), |n| n.to_string_lossy().to_string());
    let valid = !form.rows.iter().any(|r| !r.keep && !r.expression.trim().is_empty() && check(app, &r.expression, &r.unit).is_err());
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_link_parameters"), "chain", title).cards(2.6).show(ctx, valid, |ui| {
        ui_kit::text(ui, &file, ui_kit::Text::Report("link_parameters_file", false));
        let by_name = ui_kit::checkbox(ui, solidcraft_ui_text::tr("ui.link_parameters.by_name"), &mut form.by_name)
            .backing_on_hover_text(solidcraft_ui_text::tr("ui.link_parameters.by_name_tip"));
        crate::scenario::publish_handle("link_parameters:by_name", by_name.rect.center());
        // A file that changed since this design read it: its parameters are listed as they are
        // now, and OK takes the file in only when asked (else it is refused, saying why).
        if form.changed {
            let update = ui_kit::checkbox(ui, solidcraft_ui_text::tr("ui.link_parameters.update"), &mut form.update)
                .backing_on_hover_text(solidcraft_ui_text::tr("ui.link_parameters.update_tip"));
            crate::scenario::publish_handle("link_parameters:update", update.rect.center());
        }
        table(app, ui, &mut form);
        if !form.gone.is_empty() {
            let gone = solidcraft_ui_text::format("ui.link_parameters.gone", &[("names", &form.gone.join(", "))]);
            ui_kit::text(ui, &gone, ui_kit::Text::Report("link_parameters_gone", false));
        }
        if let Some(error) = &form.error {
            ui_kit::text(ui, error, ui_kit::Text::Error("link_parameters_error"));
        }
    });
    for (handle, key) in [("link_parameters:ok", "dialog:ok"), ("link_parameters:cancel", "dialog:cancel")] {
        if let Some(at) = crate::scenario::handle_at(key) {
            crate::scenario::publish_handle(handle, at);
        }
    }
    match action {
        ui_kit::Action::Ok => {
            app.link_parameters = None;
            if let Err(e) = apply(app, &form) {
                refused(app, form, e);
            }
        }
        ui_kit::Action::Cancel => app.link_parameters = None,
        ui_kit::Action::None if closed => app.link_parameters = None,
        _ => app.link_parameters = Some(form),
    }
}

/// The table of the linked design's parameters.
fn table(app: &mut SolveApp, ui: &mut egui::Ui, form: &mut Form) {
    let t = solidcraft_theme::Tokens::get();
    let (parameter, in_design, from_here, keep) = (
        solidcraft_ui_text::tr("ui.link_parameters.parameter"),
        solidcraft_ui_text::tr("ui.link_parameters.in_design"),
        solidcraft_ui_text::tr("ui.link_parameters.from_here"),
        solidcraft_ui_text::tr("ui.link_parameters.keep"),
    );
    let names: Vec<&str> = form.rows.iter().map(|r| r.name.as_str()).collect();
    let name_width = names.iter().map(|n| ui_kit::ContentWidth::name(ui, n)).fold(t.form_label_width * 0.6, f32::max).min(t.form_card_width * 0.6);
    let columns = [
        ui_kit::Column::new(parameter, 1.0).min_content_width(name_width),
        ui_kit::Column::new(in_design, 0.8),
        ui_kit::Column::new(from_here, 0.8),
        ui_kit::Column::new("Expression", 2.4).flex(),
        ui_kit::Column::new(keep, 0.7),
    ];
    let right = ui.max_rect().right().min(ui.clip_rect().right());
    let by_name = form.by_name;
    ui_kit::table_custom(ui, "link_parameters", &columns, solidcraft_ui_text::tr("ui.link_parameters.none"), |ui, table| {
        table.headers(ui, |_, _| false);
        for r in &mut form.rows {
            table.row(ui, &r.name, false, |row| {
                row.cell(|ui| {
                    ui_kit::text(ui, &r.name, ui_kit::Text::NameCell(false));
                });
                row.cell(|ui| {
                    ui_kit::text(ui, &r.design, ui_kit::Text::MonoCell);
                });
                row.cell(|ui| {
                    ui_kit::text(ui, r.here.as_deref().unwrap_or("—"), ui_kit::Text::MonoCell);
                });
                row.cell(|ui| {
                    // Kept, it takes nothing: no expression either.
                    ui.add_enabled_ui(!r.keep, |ui| {
                        let hint =
                            if by_name && r.here.is_some() && !r.keep { "ui.link_parameters.by_name_hint" } else { "ui.link_parameters.own_hint" };
                        let live = (!r.expression.trim().is_empty()).then(|| check(app, &r.expression, &r.unit));
                        let (shown, error) = match &live {
                            Some(Ok(value)) => (value.clone(), false),
                            Some(Err(message)) => (message.clone(), true),
                            None => (String::new(), false),
                        };
                        let id = ui.make_persistent_id(("link_parameter", &r.name));
                        let response =
                            ui_kit::wide_cell_editor(ui, id, &mut r.expression, solidcraft_ui_text::tr(hint), error, right, (&shown, error));
                        crate::scenario::publish_target_as(
                            &format!("link_parameter:{}", r.name),
                            &format!("link_parameter:{}", r.name),
                            response.rect,
                        );
                        crate::params_dialog::complete(ui, &response, &mut r.expression);
                    });
                });
                row.cell(|ui| {
                    let response = ui_kit::checkbox(ui, "", &mut r.keep).backing_on_hover_text(solidcraft_ui_text::tr("ui.link_parameters.keep_tip"));
                    crate::scenario::publish_handle(&format!("link_parameter:keep:{}", r.name), response.rect.center());
                    if response.changed() && r.keep {
                        r.expression.clear();
                    }
                });
            });
        }
    });
}

/// The value `e` gives a parameter of `unit`, worked out in this design, or why it doesn't.
fn check(app: &mut SolveApp, e: &str, unit: &str) -> Result<String, String> {
    let r = app.session.execute("expr.evaluate", &json!({"expression": e, "unit": unit})).map_err(|e| e.to_string())?;
    if r["ok"].as_bool().unwrap_or(false) {
        let v = r["display_value"].as_f64().unwrap_or(0.0);
        Ok(crate::dialogs_assembly::num(v, unit).trim_end().to_string())
    } else {
        Err(r["error"].as_str().unwrap_or("invalid").to_string())
    }
}

/// OK: insert the design with these values, or set them on the link. Never in another design
/// than the one the dialog was opened in.
fn apply(app: &mut SolveApp, form: &Form) -> Result<(), String> {
    if form.tab != app.docs.active_tab() {
        return Ok(());
    }
    match &form.target {
        Target::Insert { path, hosts, note } => {
            let mut request = file_load::Request::new(path, file_load::Kind::InsertDesign);
            request.allow_hosts = hosts.clone();
            request.note = note.clone();
            let set: serde_json::Map<String, Value> = form.set().into_iter().filter(|(_, e)| !e.is_null()).collect();
            if !set.is_empty() {
                request.options.insert("parameters".into(), Value::Object(set));
            }
            if form.by_name {
                request.options.insert("by_name".into(), json!(true));
            }
            let keep = form.keep();
            if !keep.is_empty() {
                request.options.insert("keep".into(), json!(keep));
            }
            // The draft waits for the insert ([`insert_ended`]): a failure or Stop brings it back.
            // A request that can't be queued is refused here, the dialog staying open.
            let serial = request.serial;
            app.link_parameters_inserting.push((serial, Form { error: None, ..form.clone() }));
            if let Err(e) = file_load::begin_request(app, request) {
                app.link_parameters_inserting.retain(|(waiting, _)| *waiting != serial);
                return Err(e);
            }
            Ok(())
        }
        // A long rebuild computes beside the window; its outcome then shows as any history
        // command's does, and the draft waits for it ([`heavy_ended`]).
        Target::Link { component } => {
            // Rows read from another design than the link reads now are never set on it: the
            // dialog reads the link again ([`follow_source`]).
            if link_source(app, &form.target) != form.source {
                return Ok(());
            }
            let mut params = json!({"component": component, "set": form.set(), "by_name": form.by_name, "keep": form.keep()});
            if form.changed && form.update {
                params["update"] = json!(true);
            }
            let (serial, r) = app.run_heavy_serial("component.link_parameters", params);
            r?;
            if app.heavy_pending(serial) {
                app.link_parameters_waiting = Some((serial, Form { error: None, ..form.clone() }));
            }
            Ok(())
        }
    }
}

/// What the links of the open design pass, for the Parameters window: one line per value, in the
/// links' order, then what the last value edit could not pass and why.
pub fn passed(app: &SolveApp) -> Vec<String> {
    let doc = &app.session.doc;
    let mut lines = Vec::new();
    for (component, link) in &doc.links {
        if !link.parameters_by_name && link.parameters.is_empty() {
            continue;
        }
        let name = doc.components.iter().find(|c| c.id == *component).map_or_else(|| component.to_string(), |c| c.name.clone());
        for (parameter, value) in &link.parameter_values {
            let expression = link.parameters.get(parameter).map_or(parameter.as_str(), String::as_str);
            lines.push(solidcraft_ui_text::format(
                "ui.link_parameters.passed_row",
                &[("expression", expression), ("component", &name), ("parameter", parameter), ("value", &shown(value))],
            ));
        }
        for parameter in &link.parameters_kept {
            lines.push(solidcraft_ui_text::format("ui.link_parameters.kept_row", &[("component", &name), ("parameter", parameter)]));
        }
    }
    lines
}

/// A value a link gave, as it keeps it (`43.857142857142854 mm`), rounded for showing.
fn shown(value: &str) -> String {
    match value.split_once(' ') {
        Some((number, unit)) => match number.parse::<f64>() {
            Ok(v) => crate::dialogs_assembly::num(v, unit),
            Err(_) => value.to_string(),
        },
        None => value.parse::<f64>().map_or_else(|_| value.to_string(), crate::dialogs_coupling::trim),
    }
}

/// The most values a placement's tooltip lists; the Parameters window lists them all.
const TOOLTIP_VALUES: usize = 8;

/// The Browser's tooltip lines for a placement of `component`, when its link passes values: the
/// link's own expressions first, then the values it takes by name, then what it keeps.
pub fn tooltip(doc: &solidcraft_engine::doc::Document, component: u64) -> Option<String> {
    let link = doc.links.get(&component).filter(|l| l.parameters_by_name || !l.parameters.is_empty())?;
    // A link by name that matches no name passes nothing: no badge and no empty list.
    if link.parameter_values.is_empty() && link.parameters_kept.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    let (own, named): (Vec<_>, Vec<_>) = link.parameter_values.iter().partition(|(name, _)| link.parameters.contains_key(*name));
    for (parameter, value) in own.into_iter().chain(named) {
        lines.push(format!("{parameter} = {}", shown(value)));
    }
    for parameter in &link.parameters_kept {
        lines.push(solidcraft_ui_text::format("ui.link_parameters.tooltip_kept", &[("parameter", parameter)]));
    }
    let more = lines.len().saturating_sub(TOOLTIP_VALUES);
    lines.truncate(TOOLTIP_VALUES);
    let mut text = solidcraft_ui_text::tr("ui.link_parameters.tooltip").to_string();
    for line in lines {
        text.push('\n');
        text.push_str(&line);
    }
    if more > 0 {
        text.push('\n');
        text.push_str(&solidcraft_ui_text::format("ui.link_parameters.tooltip_more", &[("count", &more.to_string())]));
    }
    Some(text)
}

#[cfg(test)]
#[path = "link_parameters_dialog_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
