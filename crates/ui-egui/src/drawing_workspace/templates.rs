//! Apply Drawing Template and Save as Drawing Template (#19035), on the Browser's drawing row.
//! Apply opens a window that lists this design's kind of built-ins and a template file, and shows
//! what a dry run of the one picked says before anything changes; its Apply is one undo step. Save
//! asks for a `.scdwg` file with the save picker, as the engine always writes one. The menu items and `ui.start` of either command
//! open them; `engine.execute` never does.

use serde_json::{Value, json};
use solidcraft_engine::doc::drawing::{Drawing, Sheet, template};
use solidcraft_engine::doc::ids;
use solidcraft_engine::io::DRAWING_EXT;
use solidcraft_theme::Tokens;

use crate::SolveApp;
use crate::ui_kit::{self, BackingTooltipExt as _};

/// The command Apply runs, and the id the menu and `ui.start` open its window by.
pub(super) const APPLY: &str = "drawing.template.apply";
/// The command Save runs, and the id the menu and `ui.start` start it by.
pub(super) const SAVE: &str = "drawing.template.save";

/// The Apply window while it is open.
pub(super) struct ApplyTemplate {
    /// The design tab and the drawing it was opened on: it closes when either is no longer open.
    owner: (u64, u64),
    /// This design's kind of built-ins, as Preferences lists them: (name, title).
    built_ins: Vec<(&'static str, String)>,
    /// The template picked: a built-in's name or a template file's path.
    chosen: Option<String>,
    /// The dry run of a template: its name or path, the revision it was asked at, and the reply or
    /// the engine's refusal. Asked again only when the choice or the revision changes.
    answer: Option<(String, u64, Result<Value, String>)>,
}

fn tr(id: &str) -> &str {
    solidcraft_ui_text::tr(id)
}

impl ApplyTemplate {
    /// The window for the drawing open now; none without one.
    pub(super) fn open(app: &SolveApp) -> Option<Self> {
        let drawing = app.session.drawing(APPLY).ok().filter(|_| super::active(app))?;
        let built_ins = crate::prefs::templates_for(template::is_assembly(&app.session.doc))
            .into_iter()
            .filter_map(|name| Some((name, template::title(name)?)))
            .collect();
        Some(Self { owner: (app.session.uid, drawing.id), built_ins, chosen: None, answer: None })
    }

    /// Runs the dry run of the choice when it has no answer at this revision. Never while a picker
    /// is open, since the engine refuses commands then.
    fn ask(&mut self, app: &mut SolveApp) {
        let Some(template) = self.chosen.clone() else { return };
        let revision = app.session.revision;
        if self.answer(revision).is_some() || app.file_picker_busy() {
            return;
        }
        let reply = app.run(APPLY, json!({"template": template, "dry_run": true}));
        self.answer = Some((template, revision, reply));
    }

    /// The dry run's answer for the choice at `revision`, if it was asked.
    fn answer(&self, revision: u64) -> Option<&Result<Value, String>> {
        let (template, at, reply) = self.answer.as_ref()?;
        (self.chosen.as_ref() == Some(template) && *at == revision).then_some(reply)
    }

    /// The template Apply runs: the choice, once its dry run says something changes.
    fn ready(&self, revision: u64) -> Option<String> {
        let Some(Ok(reply)) = self.answer(revision) else { return None };
        let changes = reply["changed"].as_array().is_some_and(|changed| !changed.is_empty());
        self.chosen.clone().filter(|_| changes)
    }

    /// The template file picked, if the choice is one.
    fn file(&self) -> Option<&str> {
        self.chosen.as_deref().filter(|chosen| self.built_ins.iter().all(|(name, _)| name != chosen))
    }
}

/// Draw the Apply window when it is open, and run what it asks for. Called once per frame.
pub(super) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut form) = app.drawing_workspace.apply_template.take() else { return };
    let Ok(drawing) = app.session.drawing(APPLY).cloned() else { return };
    if !super::active(app) || form.owner != (app.session.uid, drawing.id) {
        return;
    }
    form.ask(app);
    let revision = app.session.revision;
    let ready = form.ready(revision);
    let picker = app.services.pick_open.is_some() && !app.file_picker_busy();
    let mut asked = (None, false);
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_drawing_apply_template"), "import", tr("cmd.drawing_template_apply"))
        .actions(tr("ui.apply"), tr("ui.cancel"), None)
        .show(ctx, ready.is_some(), |ui| asked = body(ui, &form, &drawing, revision, picker));
    if closed || action == ui_kit::Action::Cancel {
        return;
    }
    if action == ui_kit::Action::Ok
        && let Some(chosen) = ready
    {
        match app.run(APPLY, json!({"template": chosen})) {
            Ok(reply) => {
                // Sheet Settings then warns about the views left outside, as after a paper change.
                if let Some(sheet) = crate::drawing_sheet_page::active_sheet(app) {
                    super::panels::note_outside(app, sheet, &reply);
                }
                return;
            }
            Err(error) => app.set_status(error, true),
        }
    }
    let (picked, browse) = asked;
    if let Some(name) = picked {
        form.chosen = Some(name.to_string());
    }
    if browse {
        let owner = form.owner;
        pick_template_file(app, move |app, path| {
            if let Some(form) = app.drawing_workspace.apply_template.as_mut().filter(|form| form.owner == owner) {
                form.chosen = Some(path);
            }
        });
    }
    app.drawing_workspace.apply_template = Some(form);
}

/// The window's body: the built-ins, the file button and the dry run's answer. The built-in
/// clicked, and whether the file button was. Each built-in's handle is `drawing:template:<name>`,
/// the file button's `drawing:template:file`.
fn body(ui: &mut egui::Ui, form: &ApplyTemplate, drawing: &Drawing, revision: u64, picker: bool) -> (Option<&'static str>, bool) {
    ui_kit::section_heading(ui, tr("ui.drawing_template.built_in"));
    let mut picked = None;
    for (name, title) in &form.built_ins {
        let row = ui_kit::list_picker(ui, name, title, form.chosen.as_deref() == Some(*name));
        crate::scenario::publish_target(&format!("drawing:template:{name}"), row.rect);
        if row.clicked() {
            picked = Some(*name);
        }
    }
    ui.add_space(Tokens::get().space_8);
    let file = ui_kit::button(ui, tr("ui.drawing_template.file"), ui_kit::Button::Enabled(picker))
        .backing_on_disabled_hover_text(tr("ui.drawing_first.unavailable"));
    crate::scenario::publish_target("drawing:template:file", file.rect);
    if let Some(path) = form.file() {
        let name = std::path::Path::new(path).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(path);
        ui_kit::text(ui, name, ui_kit::Text::Note("drawing_template_file"));
    }
    if let Some(reply) = form.answer(revision) {
        ui.separator();
        answer(ui, drawing, reply);
    }
    (picked, file.clicked())
}

/// What the dry run says: the engine's refusal, or each sheet that changes with the typed text it
/// drops, and the views the new sheets leave outside their drawing area.
fn answer(ui: &mut egui::Ui, drawing: &Drawing, reply: &Result<Value, String>) {
    let reply = match reply {
        Ok(reply) => reply,
        Err(error) => {
            ui_kit::text(ui, &crate::i18n::tr_diagnostic(error), ui_kit::Text::Error("drawing_template_error"));
            return;
        }
    };
    let changed = reply["changed"].as_array().map(Vec::as_slice).unwrap_or_default();
    if changed.is_empty() {
        ui_kit::text(ui, tr("ui.drawing_template.unchanged"), ui_kit::Text::Note("drawing_template_unchanged"));
    }
    for change in changed {
        let id = change["sheet"].as_str().and_then(ids::decode);
        let Some(sheet) = drawing.sheets.iter().find(|sheet| Some(sheet.id) == id) else { continue };
        let line = solidcraft_ui_text::format("ui.drawing_template.changes", &[("sheet", &sheet.name)]);
        ui_kit::text(ui, &line, ui_kit::Text::Report("drawing_template_changes", false));
        let lost: Vec<&str> = change["lost"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|cell| cell_label(sheet, cell)).collect();
        if !lost.is_empty() {
            let line = solidcraft_ui_text::format("ui.drawing_template.lost", &[("cells", &lost.join(", "))]);
            ui_kit::text(ui, &line, ui_kit::Text::Report("drawing_template_lost", false));
        }
    }
    let outside = reply["outside"].as_array().map_or(0, Vec::len);
    if outside > 0 {
        super::panels::outside_box(ui, outside, |ui| {
            ui.label(tr("ui.drawing_template.stay"));
        });
    }
}

/// A title block cell by its label on `sheet`, else by its name.
fn cell_label<'a>(sheet: &'a Sheet, name: &'a str) -> &'a str {
    let cell = sheet.title_block.as_ref().and_then(|block| block.cells.iter().find(|cell| cell.name == name));
    cell.and_then(|cell| cell.label.as_ref()).map_or(name, |label| label.text.as_str())
}

/// Asks for a template file, then runs `then` with its path: only when one was picked and the same
/// design is still open in Drawing, so a cancel or a tab switch while the picker was open does
/// nothing. The first-sheet card's template button and the Apply window's file button use it.
/// Templates are `.scdwg` files; older `.json` ones still open.
pub(super) fn pick_template_file(app: &mut SolveApp, then: impl FnOnce(&mut SolveApp, String) + 'static) {
    let uid = app.session.uid;
    app.pick_open_for(&[DRAWING_EXT, "json"], move |app, path| {
        if super::active(app)
            && app.session.uid == uid
            && let Some(path) = path
        {
            then(app, path);
        }
    });
}

/// Save as Drawing Template: the save picker, named after the drawing, then `drawing.template.save`
/// to the file picked while the same drawing is still open.
pub(super) fn save(app: &mut SolveApp) {
    let Ok(drawing) = app.session.drawing(SAVE) else { return };
    let owner = (app.session.uid, drawing.id);
    let name = format!("{} template.{DRAWING_EXT}", drawing.name);
    app.pick_save(&name, &[DRAWING_EXT], move |app, path| {
        let Some(path) = path else { return };
        if !super::active(app) || app.session.open_drawing.map(|id| (app.session.uid, id)) != Some(owner) {
            return;
        }
        match app.run(SAVE, json!({"path": path})) {
            // A `.json` name went beside it as `.scdwg`, and `run` already said so.
            Ok(reply) if reply.get("renamed_from").is_some_and(|from| !from.is_null()) => {}
            Ok(reply) => {
                let written = reply["path"].as_str().unwrap_or(&path);
                app.set_status(solidcraft_ui_text::format("ui.drawing_template.saved", &[("path", written)]), false);
            }
            Err(error) => app.set_status(error, true),
        }
    });
}
