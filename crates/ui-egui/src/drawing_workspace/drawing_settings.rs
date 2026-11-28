//! The Drawing Settings window (#19048): the open drawing's projection, line group, font, language
//! and updates. Each row runs `drawing.set` with its one key as soon as it is picked, so each change
//! is one drawing undo step. The window reads the drawing every frame, so an undo or redo shows at
//! once. File › Drawing Settings…, the Browser's drawing row and `ui.start` of `drawing.set` open it;
//! `engine.execute` of `drawing.set` never does.

use serde_json::{Value, json};
use solidcraft_engine::doc::drawing::{Drawing, Updates};
use solidcraft_engine::geom::{cell::Projection, paper::line::LineGroup};

use crate::SolveApp;
use crate::ui_kit;

/// The command every row runs, and the id the menus and `ui.start` open the window by.
pub(crate) const COMMAND: &str = "drawing.set";

/// The built-in font's entry in the font list, for automation.
const FONT_BUILT_IN: &str = "drawing:drawing_settings:font:built_in";

/// The window while it is open; the settings themselves live in the drawing.
pub(crate) struct Settings {
    /// The design tab and the drawing it was opened on: it closes when either is no longer open.
    pub(super) owner: (u64, u64),
    /// What is typed in the font list's search field.
    font_filter: String,
    /// The engine's note on why the drawing's text is set in the built-in font, with the font it
    /// was made for: made again only when the drawing's font changes (a pick, an undo, a redo).
    font_note: Option<(String, Note)>,
}

/// A font's note, or the lookup still making it. The lookup reads the font folders: cold, a few
/// hundred milliseconds for a font that isn't installed, so it runs on a worker, not in a frame.
enum Note {
    Ready(Option<String>),
    #[cfg(not(target_arch = "wasm32"))]
    Looking(crate::starting::Worker<Option<String>>),
}

impl Note {
    /// Start the lookup for `font`; made at once where no thread can run it (and on the web, which
    /// has no threads or font folders).
    fn look_up(ctx: &egui::Context, font: &str) -> Self {
        let make = |font: &str| solidcraft_engine::sketch::paper_font_note(&solidcraft_engine::sketch::font_dirs(), font);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let owned = font.to_owned();
            if let Ok(worker) = crate::starting::Worker::spawn("font note", ctx.clone(), move || make(&owned)) {
                return Note::Looking(worker);
            }
        }
        let _ = ctx;
        Note::Ready(make(font))
    }

    /// The note once it is made; none while the lookup runs. Never waits.
    fn poll(&mut self) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Note::Looking(worker) = self {
            *self = match worker.poll() {
                crate::starting::Poll::Running => return None,
                crate::starting::Poll::Done(note) => Note::Ready(note),
                crate::starting::Poll::Failed => Note::Ready(None),
            };
        }
        match self {
            Note::Ready(note) => note.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            Note::Looking(_) => None,
        }
    }

    fn looking(&self) -> bool {
        match self {
            Note::Ready(_) => false,
            #[cfg(not(target_arch = "wasm32"))]
            Note::Looking(_) => true,
        }
    }
}

impl Settings {
    /// The note on `font`, looked up once per font.
    fn note(&mut self, ctx: &egui::Context, font: &str) -> Option<String> {
        if self.font_note.as_ref().is_none_or(|(made_for, _)| made_for != font) {
            self.font_note = Some((font.to_string(), Note::look_up(ctx, font)));
        }
        self.font_note.as_mut().and_then(|(_, note)| note.poll())
    }
}

fn tr(id: &str) -> &str {
    solidcraft_ui_text::tr(id)
}

/// Whether the open window is still looking up its font's note: background work that
/// `Harness::settle` waits for, as it waits for a file loading.
pub(crate) fn looking(app: &SolveApp) -> bool {
    app.drawing_workspace.drawing_settings.as_ref().and_then(|settings| settings.font_note.as_ref()).is_some_and(|(_, note)| note.looking())
}

/// Whether the window can open: in Drawing, with a drawing open.
pub(crate) fn available(app: &SolveApp) -> bool {
    super::active(app) && app.session.drawing(COMMAND).is_ok()
}

/// The window for the drawing open now; none without one.
pub(super) fn open(app: &SolveApp) -> Option<Settings> {
    let drawing = app.session.drawing(COMMAND).ok().filter(|_| super::active(app))?;
    Some(Settings { owner: (app.session.uid, drawing.id), font_filter: String::new(), font_note: None })
}

/// Draw the window when it is open, and run the change a row asked for. Called once per frame.
pub(super) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut settings) = app.drawing_workspace.drawing_settings.take() else { return };
    let Ok(drawing) = app.session.drawing(COMMAND).cloned() else { return };
    if !super::active(app) || settings.owner != (app.session.uid, drawing.id) {
        return;
    }
    let note = settings.note(ctx, &drawing.font);
    let mut change = None;
    let closed = ui_kit::Window::new(egui::Id::unique("sc_drawing_settings"), "settings", tr("cmd.drawing_set")).show_body(ctx, |ui| {
        change = rows(ui, &drawing, &mut settings.font_filter, note.as_deref());
    });
    if let Some((key, value)) = change {
        apply(app, &mut settings, key, value);
    }
    if !closed {
        app.drawing_workspace.drawing_settings = Some(settings);
    }
}

/// Run `drawing.set` with one key: one undo step. Its reply carries the font's note, so a font pick
/// needs no second look at the font folders. A refusal goes to the status line, as a sheet's does.
fn apply(app: &mut SolveApp, settings: &mut Settings, key: &str, value: Value) {
    let mut params = json!({});
    params[key] = value;
    match app.run(COMMAND, params) {
        Ok(reply) => {
            if let Some(font) = reply["font"].as_str() {
                settings.font_note = Some((font.to_string(), Note::Ready(reply["font_note"].as_str().map(str::to_string))));
            }
        }
        Err(error) => app.set_status(error, true),
    }
}

/// The window's rows, and the change one of them asks for: the key and value `drawing.set` takes.
fn rows(ui: &mut egui::Ui, drawing: &Drawing, font_filter: &mut String, note: Option<&str>) -> Option<(&'static str, Value)> {
    let mut change = None;
    let projections = [(Projection::First, "ui.first_angle", "first"), (Projection::Third, "ui.third_angle", "third")];
    if let Some(projection) = segments(ui, tr("ui.drawing_settings.projection"), "projection", drawing.projection, &projections) {
        change = Some(("projection", json!(projection)));
    }

    let label = tr("ui.drawing_settings.line_group");
    let saved = serde_json::to_value(drawing.line_group).ok();
    let selected = LineGroup::NAMES.iter().position(|name| saved.as_ref().and_then(Value::as_str) == Some(*name));
    let current = selected.and_then(|index| LineGroup::NAMES.get(index)).map(|name| format!("{name} mm")).unwrap_or_default();
    let options = || LineGroup::NAMES.iter().map(|name| (name.to_string(), format!("{name} mm"))).collect::<Vec<_>>();
    if let Some(index) = pick(ui, label, "drawing_settings_line_group", &current, options, selected)
        && let Some(name) = LineGroup::NAMES.get(index)
    {
        change = Some(("line_group", json!(name)));
    }
    help(ui, label, "ui.drawing_settings.line_group_help");

    if let Some(font) = font_row(ui, drawing, font_filter, note) {
        change = Some(("font", json!(font)));
    }

    let label = tr("ui.drawing_settings.language");
    let tag = drawing.language.as_str();
    let locales = crate::i18n::LOCALES;
    let selected = locales.iter().position(|locale| locale.id == tag);
    // A language outside the catalogs shows its own tag: `Locale::for_language` would say English.
    let current = selected.and_then(|index| locales.get(index)).map_or_else(|| tag.to_string(), |locale| locale.display_name().into_owned());
    let options = || locales.iter().map(|locale| (locale.id.to_string(), locale.display_name().into_owned())).collect::<Vec<_>>();
    if let Some(index) = pick(ui, label, "drawing_settings_language", &current, options, selected)
        && let Some(locale) = locales.get(index)
    {
        change = Some(("language", json!(locale.id)));
    }
    help(ui, label, "ui.drawing_settings.language_help");

    let label = tr("ui.drawing_settings.updates");
    let updates = [(Updates::Automatic, "ui.drawing_panel.auto", "automatic"), (Updates::Deferred, "ui.drawing_settings.deferred", "deferred")];
    if let Some(updates) = segments(ui, label, "updates", drawing.updates, &updates) {
        change = Some(("updates", json!(updates)));
    }
    if drawing.updates == Updates::Deferred {
        help(ui, label, "ui.drawing_settings.deferred_help");
    }

    ui.separator();
    ui_kit::text(ui, tr("ui.drawing_settings.drawing_immediate"), ui_kit::Text::Note("drawing_settings_immediate"));
    change
}

/// A row of segments over `choices` (value, message id, handle suffix): the value picked, when it
/// isn't the drawing's already. Each segment's handle is `drawing:drawing_settings:{key}:{suffix}`.
fn segments<T: Copy + PartialEq>(ui: &mut egui::Ui, label: &str, key: &str, current: T, choices: &[(T, &str, &str)]) -> Option<T> {
    let mut selected = choices.iter().position(|(value, _, _)| *value == current).unwrap_or(choices.len());
    let labels: Vec<&str> = choices.iter().map(|(_, id, _)| tr(id)).collect();
    let targets: Vec<String> = choices.iter().map(|(_, _, suffix)| format!("drawing:drawing_settings:{key}:{suffix}")).collect();
    let targets: Vec<&str> = targets.iter().map(String::as_str).collect();
    let clicked = ui_kit::row(ui, label, |ui| {
        ui_kit::segmented(ui, &mut selected, &labels, ui_kit::Segments { workspace_accent: true, targets: &targets, ..Default::default() })
    });
    choices.get(selected).map(|(value, _, _)| *value).filter(|value| clicked && *value != current)
}

/// A dropdown row showing `current`; its list, `(key, text)` per entry, is built only while it is
/// open. The index picked, when it isn't `selected` already. Each entry's handle is
/// `dialog:list:{id}:{key}`.
fn pick(
    ui: &mut egui::Ui,
    label: &str,
    id: &str,
    current: &str,
    options: impl FnOnce() -> Vec<(String, String)>,
    selected: Option<usize>,
) -> Option<usize> {
    let mut picked = None;
    ui_kit::row(ui, label, |ui| {
        ui_kit::dropdown_menu(ui, id, current, |ui| {
            for (index, (key, text)) in options().iter().enumerate() {
                if ui_kit::list_picker(ui, &format!("{id}:{key}"), text, selected == Some(index)).clicked() {
                    picked = Some(index);
                    ui.close();
                }
            }
        });
    });
    picked.filter(|index| Some(*index) != selected)
}

/// The font row: the built-in font first, then the installed families (listed once in the
/// background, none on the web), never a font file, since drawings name fonts by family. Under it,
/// why the drawing's text is set in the built-in font, when it is. The family picked, when it
/// isn't the drawing's already.
fn font_row(ui: &mut egui::Ui, drawing: &Drawing, filter: &mut String, note: Option<&str>) -> Option<String> {
    let label = tr("ui.drawing_settings.font");
    let builtin = solidcraft_ui_text::format("sketch_text.builtin_font", &[("family", solidcraft_engine::sketch::DEFAULT_FONT_FAMILY)]);
    let current = if drawing.font.trim().is_empty() { builtin.clone() } else { drawing.font.clone() };
    let families = crate::sketch_text::families(ui.ctx());
    let mut options = vec![ui_kit::SearchOption { value: "", label: &builtin, handle: Some(FONT_BUILT_IN) }];
    if let Some(list) = &families {
        options.extend(list.iter().map(|family| ui_kit::SearchOption { value: family, label: family, handle: None }));
    }
    let loading = families.is_none().then(|| tr("sketch_text.loading_fonts"));
    let chosen = ui_kit::row(ui, label, |ui| ui_kit::searchable_dropdown(ui, "drawing_settings_font", &current, filter, &options, None, loading));
    if let Some(note) = note {
        let note = crate::i18n::tr_diagnostic(note);
        ui_kit::below_row_control(ui, label, ui.available_width(), |ui| {
            ui_kit::text(ui, &note, ui_kit::Text::Note("drawing_settings_font_note"));
        });
    }
    let chosen = chosen.filter(|font| *font != drawing.font)?;
    filter.clear();
    Some(chosen)
}

/// A dim help line under the row labelled `label`.
fn help(ui: &mut egui::Ui, label: &str, id: &'static str) {
    ui_kit::below_row_control(ui, label, ui.available_width(), |ui| {
        ui_kit::text(ui, tr(id), ui_kit::Text::Note(id));
    });
}
