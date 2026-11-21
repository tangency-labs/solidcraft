//! A file's import options, asked before it is opened or inserted from the window.
//!
//! The format table (`solidcraft_engine::io::formats`) lists each format's import options, every
//! one a parameter of `doc.open` and of the matching `file.insert_*` command. When a person opens,
//! inserts or drops a file whose format has options shown here, a small dialog lists them, one row
//! each, at the values the import would use anyway (the table's default, or what Preferences
//! chose, such as a FreeCAD file's history). Open (or Insert) imports with the values changed in
//! the dialog; Cancel, Esc or the close button imports nothing. A file whose format has none goes
//! straight in, as before. Preferences > General > Dialogs "Ask for a file's import options"
//! turns the dialog off, which keeps the import as it was before the dialog existed.
//!
//! The up axis (`up`) is asked here only for a format with no stand-up offer after the import
//! (glTF, which is stood up by default). After a STEP, IGES, FreeCAD or Rhino file is in, the
//! stand-up offer ([`crate::stand_up::offered_for`]) sees how its bodies lie, gives the exporter's
//! reason, remembers an answer per file and turns the whole component in one undoable step, while
//! an `up` sent with the import would skip that offer: asking it before the file is read, without
//! seeing it, would ask the same question twice in two ways.
//!
//! A KiCad board is asked here too: the components, the copper, the silkscreen, the board's colour
//! and thickness (starting at the board's own, read from its file) and whether the board is a solid
//! or a mesh. A board always asks, whatever the preference says: it had its own window before the
//! preference existed, and turning the preference off must not take its choices away. A board
//! chosen to insert opens as a design of its own, as it always has (`doc.open` reads boards).
//!
//! Only interactive entry points ask ([`crate::file_load::begin`]): a script, the control channel
//! or MCP calls the commands with their own options and never meets this dialog.

use serde_json::{Map, Value, json};
use solidcraft_engine::io::formats::{self, ImportTarget, OptionKind, OptionSpec};

use crate::{SolveApp, file_load, ui_kit};

/// Whether the dialog leaves `param` out for `path` because something else asks it: the up axis
/// of a file the stand-up offer turns after the import (see the module docs).
fn asked_elsewhere(path: &str, param: &str) -> bool {
    param == "up" && crate::stand_up::offered_for(path)
}

/// One option's value, as its row edits it.
#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    Bool(bool),
    /// An index into the option's choices.
    Choice(usize),
    Text(String),
    Number(f64),
    /// sRGB.
    Color([u8; 3]),
}

/// One option of the format, its value now and the value the import would use without the dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub param: &'static str,
    pub value: Choice,
    pub initial: Choice,
}

/// The values of a format's options shown in a window.
#[derive(Clone, Debug, PartialEq)]
pub struct Values {
    /// The format table's row id.
    pub format: &'static str,
    pub fields: Vec<Field>,
}

impl Values {
    /// The options of the format of `path` shown here, leaving out `skip` and the options asked
    /// elsewhere, each at the value the import would use: what `preferred` names for it, else the
    /// file's own value (a KiCad board's thickness), else the table's default. `None` when the
    /// format has no import row.
    pub fn for_path(path: &str, skip: &[&str], preferred: &Map<String, Value>) -> Option<Values> {
        let spec = formats::import_for_path(path)?;
        let import = spec.import.as_ref()?;
        let fields = import
            .options
            .iter()
            .filter(|o| !asked_elsewhere(path, o.param) && !skip.contains(&o.param))
            .map(|o| {
                let initial = start(o, path, preferred);
                Field { param: o.param, value: initial.clone(), initial }
            })
            .collect();
        Some(Values { format: spec.id, fields })
    }

    /// The command parameters of the options changed from the value the import would use anyway:
    /// an untouched dialog imports exactly as the file did before it existed.
    pub fn params(&self) -> Map<String, Value> {
        let mut out = Map::new();
        for f in &self.fields {
            if f.value != f.initial
                && let Some(spec) = self.spec(f.param)
                && let Some(v) = to_json(spec, &f.value)
            {
                out.insert(f.param.into(), v);
            }
        }
        out
    }

    /// [`Self::params`], plus each shown value the import of `path` would no longer use by itself
    /// now that `preferred` (the preferences when the answer comes) names another: a preference
    /// changed while the question waited (a FreeCAD file's history) does not override what was
    /// shown. A value read from the file (a board's thickness) is the import's own, so an
    /// untouched board still sends nothing.
    pub fn params_now(&self, path: &str, preferred: &Map<String, Value>) -> Map<String, Value> {
        let mut out = self.params();
        for f in &self.fields {
            if let Some(spec) = self.spec(f.param)
                && start(spec, path, preferred) != f.value
                && let Some(v) = to_json(spec, &f.value)
            {
                out.insert(f.param.into(), v);
            }
        }
        out
    }

    fn spec(&self, param: &str) -> Option<&'static OptionSpec> {
        formats::find(self.format)?.import.as_ref()?.options.iter().find(|o| o.param == param)
    }
}

/// The value the import of `path` uses for an option by itself: what `preferred` names, else the
/// file's own value (a KiCad board's thickness), else the table's default.
fn start(o: &OptionSpec, path: &str, preferred: &Map<String, Value>) -> Choice {
    preferred.get(o.param).and_then(|v| parse(o, v)).or_else(|| from_file(o, path)).unwrap_or_else(|| default(o))
}

/// The table's default for an option.
fn default(o: &OptionSpec) -> Choice {
    match &o.kind {
        OptionKind::Bool { default } => Choice::Bool(*default),
        OptionKind::Choice { choices, default } => Choice::Choice((*default).min(choices.len().saturating_sub(1))),
        OptionKind::Text { default, .. } => Choice::Text((*default).to_string()),
        OptionKind::Number { default, .. } => Choice::Number(*default),
        OptionKind::Color { default } => Choice::Color(*default),
    }
}

/// The option's value in the file at `path`, for an option the command reads from the file when
/// it is not given one; `None` when the file names none (or cannot be read).
fn from_file(o: &OptionSpec, path: &str) -> Option<Choice> {
    match &o.kind {
        OptionKind::Number { min, max, from_file: Some(read), .. } => {
            read(path).filter(|n| n.is_finite() && (*min..=*max).contains(n)).map(Choice::Number)
        }
        _ => None,
    }
}

/// `#rrggbb` (or `rrggbb`) as sRGB bytes.
fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// A command parameter's value as the option's row holds it; `None` when it is not one of its values.
fn parse(o: &OptionSpec, v: &Value) -> Option<Choice> {
    match &o.kind {
        OptionKind::Bool { .. } => v.as_bool().map(Choice::Bool),
        OptionKind::Choice { choices, .. } => {
            let s = v.as_str()?;
            choices.iter().position(|(value, _)| *value == s).map(Choice::Choice)
        }
        OptionKind::Text { .. } => v.as_str().map(|s| Choice::Text(s.to_string())),
        OptionKind::Number { min, max, .. } => v.as_f64().filter(|n| n.is_finite() && (*min..=*max).contains(n)).map(Choice::Number),
        OptionKind::Color { .. } => v.as_str().and_then(parse_hex).map(Choice::Color),
    }
}

/// The command parameter for a row's value.
fn to_json(o: &OptionSpec, v: &Choice) -> Option<Value> {
    match (&o.kind, v) {
        (OptionKind::Bool { .. }, Choice::Bool(b)) => Some(json!(b)),
        (OptionKind::Choice { choices, .. }, Choice::Choice(i)) => choices.get(*i).map(|(value, _)| json!(value)),
        (OptionKind::Text { .. }, Choice::Text(s)) => Some(json!(s)),
        (OptionKind::Number { min, max, .. }, Choice::Number(n)) if n.is_finite() => Some(json!(n.clamp(*min, *max))),
        (OptionKind::Color { .. }, Choice::Color([r, g, b])) => Some(json!(format!("#{r:02x}{g:02x}{b:02x}"))),
        _ => None,
    }
}

/// The rows of `values`, one per option, the control by the option's kind. Labels and choices are
/// the table's English, translated here. Each row publishes `import_options:<param>`; a choice's
/// items are the dropdown's `dialog:<label>:<choice>`.
pub fn option_rows(ui: &mut egui::Ui, values: &mut Values) {
    let format = values.format;
    for field in &mut values.fields {
        let Some(spec) = formats::find(format).and_then(|f| f.import.as_ref()).and_then(|i| i.options.iter().find(|o| o.param == field.param)) else {
            continue;
        };
        let handle = format!("import_options:{}", field.param);
        let response = match (&spec.kind, &mut field.value) {
            (OptionKind::Bool { .. }, Choice::Bool(on)) => ui_kit::checkbox(ui, solidcraft_ui_text::tr_label(spec.label), on),
            (OptionKind::Choice { choices, .. }, Choice::Choice(index)) => {
                let labels: Vec<&str> = choices.iter().map(|(_, label)| *label).collect();
                ui_kit::choice_row(ui, spec.label, &handle, index, &labels)
            }
            (OptionKind::Text { hint, .. }, Choice::Text(text)) => {
                ui_kit::row(ui, spec.label, |ui| ui_kit::text_field_reserving(ui, text, hint, 0.0).1)
            }
            (OptionKind::Number { min, max, unit, .. }, Choice::Number(n)) => {
                let suffix = if unit.is_empty() { String::new() } else { format!(" {unit}") };
                ui_kit::row(ui, spec.label, |ui| ui_kit::number_field(ui, n, *min..=*max, 0.1, &suffix))
            }
            // The kit's colour button and its picker, as the Appearance dialog's.
            (OptionKind::Color { .. }, Choice::Color(rgb)) => ui_kit::row(ui, spec.label, |ui| ui_kit::color_field(ui, rgb)),
            _ => continue,
        };
        crate::scenario::publish_handle(&handle, response.rect.center());
    }
}

/// A file waiting for its import options.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub path: String,
    /// Insert it into the design (else open it as a new design).
    pub insert: bool,
    pub values: Values,
    /// The tab shown when the question was asked: an insert lands in that design, not in one
    /// shown by the time the answer comes. `None` for an open.
    pub tab: Option<u64>,
}

/// Ask for the import options of `path` before it is opened or inserted. Returns whether the file
/// was held for the answer (the caller then stops: the dialog starts the import itself). A file
/// whose format shows no options here goes on at once, and so does every file but a KiCad board
/// while the preference is off (see the module docs). A file asked while another waits (several
/// dropped at once) waits its turn, and is asked once that one is answered.
pub fn ask(app: &mut SolveApp, path: &str, insert: bool) -> bool {
    let Some(target) = formats::import_for_path(path).and_then(|f| f.import.as_ref()).map(|i| i.target) else { return false };
    let board = target == ImportTarget::Kicad;
    if !app.preferences.ask_import_options && !board {
        return false;
    }
    let preferred = file_load::preferred_options(app, path);
    let Some(values) = Values::for_path(path, &[], &preferred) else { return false };
    if values.fields.is_empty() {
        return false;
    }
    // A board chosen to insert opens as a design of its own (see the module docs).
    let insert = insert && !board;
    let tab = insert.then(|| app.docs.active_tab());
    let ask = Ask { path: path.into(), insert, values, tab };
    if app.import_options.is_some() {
        app.import_options_waiting.push_back(ask);
    } else {
        app.import_options = Some(ask);
    }
    true
}

/// The import the dialog's answer starts: the file with the options changed in it, and each shown
/// value the preferences would now change ([`Values::params_now`]); an insert into the design that
/// asked.
fn request(app: &SolveApp, ask: &Ask) -> file_load::Request {
    let kind = if ask.insert { file_load::Kind::Insert } else { file_load::Kind::Open };
    let mut request = file_load::Request::new(&ask.path, kind);
    request.options = ask.values.params_now(&ask.path, &file_load::preferred_options(app, &ask.path));
    request.tab = ask.tab;
    request
}

/// The dialog, while a file waits for its options. Enter imports, Esc cancels.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut ask) = app.import_options.take() else { return };
    let file = std::path::Path::new(&ask.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| ask.path.clone());
    let format = formats::find(ask.values.format).map_or("", |f| solidcraft_ui_text::tr_label(f.label));
    let (title_id, ok) = if ask.insert { ("import_options.title_insert", "ui.insert") } else { ("import_options.title_open", "ui.open") };
    let title = solidcraft_ui_text::format(title_id, &[("format", format)]);
    let ok = solidcraft_ui_text::tr(ok);
    let cancel = solidcraft_ui_text::tr("ui.cancel");
    // The window widens so its longest label (a KiCad board's copper) is not cut short: the rows'
    // form publishes the width its label column needs beyond the design's (#10620), within the
    // screen.
    let t = solidcraft_theme::Tokens::get();
    let grow = egui::Id::unique("sc_import_options_label_grow");
    let extra = ctx.data(|d| d.get_temp::<f32>(grow)).unwrap_or(0.0);
    let width = (t.form_card_width * 1.5 + extra).min((ctx.content_rect().width() - t.space_24 * 2.0).max(t.form_card_width));
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_import_options"), "import", &title)
        .actions(ok, cancel, None)
        .cards(width / t.form_card_width)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, true, |ui| {
            ui_kit::scope_grow(ui, grow, |ui| {
                ui_kit::text(ui, &file, ui_kit::Text::Report("file", false));
                option_rows(ui, &mut ask.values);
            });
        });
    for (handle, key) in [("import_options:ok", "dialog:ok"), ("import_options:cancel", "dialog:cancel")] {
        if let Some(at) = crate::scenario::handle_at(key) {
            crate::scenario::publish_handle(handle, at);
        }
    }
    match action {
        ui_kit::Action::Ok => {
            let r = request(app, &ask);
            let _ = file_load::begin_request(app, r);
        }
        ui_kit::Action::None if !closed => {
            app.import_options = Some(ask);
            return;
        }
        // Esc gives this question up only: Home, drawn after this, must not close on it too.
        _ => {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        }
    }
    // The next file of a drop of several is asked now.
    app.import_options = app.import_options_waiting.pop_front();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_start_at_the_defaults_or_the_preferred_value_and_send_only_changes() {
        let none = Map::new();
        let step = Values::for_path("part.STEP", &[], &none).expect("STEP imports");
        let params: Vec<&str> = step.fields.iter().map(|f| f.param).collect();
        assert_eq!(params, ["names"], "the up axis stays with the stand-up offer");
        assert!(step.params().is_empty(), "untouched, the import is as before");

        let mut solids = Map::new();
        solids.insert("history".into(), json!("solids"));
        let mut fc = Values::for_path("model.FCStd", &[], &solids).expect("FreeCAD imports");
        let history = fc.fields.iter_mut().find(|f| f.param == "history").expect("history is asked");
        assert_eq!(history.initial, Choice::Choice(1), "the preference's solids-only import is the starting value");
        history.value = Choice::Choice(0);
        assert_eq!(Value::Object(fc.params()), json!({"history": "features"}));

        for path in ["mesh.stl", "design.solidcraft", "cloud.ply", "notes.txt"] {
            assert!(Values::for_path(path, &[], &none).is_none_or(|v| v.fields.is_empty()), "{path} has nothing to ask");
        }
        // No stand-up offer follows a glTF import: its up axis is asked here, Y up first.
        for path in ["scene.glb", "scene.GLTF"] {
            let gltf = Values::for_path(path, &[], &none).expect("glTF imports");
            assert_eq!(gltf.fields, [Field { param: "up", value: Choice::Choice(0), initial: Choice::Choice(0) }], "{path}");
        }
        for path in ["part.igs", "model.FCStd", "shape.3dm"] {
            let v = Values::for_path(path, &[], &none).expect("imports");
            assert!(v.fields.iter().all(|f| f.param != "up"), "{path}: its up axis is the stand-up offer's");
        }
        let board = Values::for_path("b.kicad_pcb", &["components", "copper", "silkscreen", "color"], &none).unwrap();
        assert_eq!(board.fields.iter().map(|f| f.param).collect::<Vec<_>>(), ["thickness", "representation"], "`skip` leaves options out");
    }

    /// Every label and choice the dialog shows is in the catalogs, so it is translated.
    #[test]
    fn every_shown_label_is_in_the_catalogs() {
        crate::i18n::install();
        let mut english = std::collections::HashSet::new();
        solidcraft_ui_text::each_english_message(|_, text| {
            english.insert(text.to_string());
        });
        for f in formats::importable(true) {
            let Some(values) = Values::for_path(&format!("x.{}", f.extensions[0]), &[], &Map::new()) else { continue };
            for field in &values.fields {
                let spec = values.spec(field.param).unwrap();
                let mut texts = vec![spec.label];
                if let OptionKind::Choice { choices, .. } = &spec.kind {
                    texts.extend(choices.iter().map(|(_, label)| *label));
                }
                for text in texts {
                    assert!(english.contains(text), "`{text}` ({} import) is not in the catalogs", f.id);
                }
            }
        }
    }

    #[test]
    fn a_step_file_waits_for_its_options_and_the_preference_skips_them() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert!(app.preferences.ask_import_options, "asked by default");
        file_load::begin(&mut app, "part.step", false);
        let held = app.import_options.clone().expect("asked");
        assert_eq!((held.path.as_str(), held.insert), ("part.step", false));
        assert!(app.file_load.idle(), "nothing is imported until Open");

        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.preferences.ask_import_options = false;
        file_load::begin(&mut app, "part.step", true);
        assert!(app.import_options.is_none(), "the preference turns the dialog off");
    }

    /// The FreeCAD history shown is the one imported, even when the preference changes while the
    /// question waits: the preference no longer adds its own value over the shown one.
    #[test]
    fn a_preference_changed_while_the_question_waits_does_not_change_the_import() {
        for (before, shown, sent) in [(false, "solids", "solids"), (true, "features", "features")] {
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
            app.preferences.freecad_history = before;
            assert!(ask(&mut app, "model.FCStd", false), "a FreeCAD file asks");
            let held = app.import_options.clone().expect("asked");
            let history = held.values.fields.iter().find(|f| f.param == "history").expect("history is asked");
            let spec = held.values.spec("history").unwrap();
            assert_eq!(to_json(spec, &history.value), Some(json!(shown)), "shown at the preference's value");
            assert!(request(&app, &held).options.is_empty(), "unchanged, the preference itself still applies");
            app.preferences.freecad_history = !before;
            assert_eq!(
                Value::Object(request(&app, &held).options),
                json!({"history": sent}),
                "after the preference changed, the shown history is sent"
            );
        }
    }

    /// An insert lands in the design whose tab asked, not in one shown by the time it is answered;
    /// an open has no tab to land in.
    #[test]
    fn an_insert_lands_in_the_tab_that_asked() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert!(ask(&mut app, "part.step", true));
        let asked = app.docs.current_tab();
        let held = app.import_options.take().expect("asked");
        assert_eq!(held.tab, Some(asked));
        crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
        assert_ne!(app.docs.current_tab(), asked, "another tab is shown");
        assert_eq!(request(&app, &held).tab, Some(asked), "the insert is for the tab that asked");

        assert!(ask(&mut app, "part.step", false));
        let held = app.import_options.take().expect("asked");
        assert_eq!((held.tab, request(&app, &held).tab), (None, None), "an open makes its own tab");
    }

    /// Several files dropped at once are each asked in turn: a later one does not replace the
    /// first, and Cancel gives up only the file it is asked for.
    #[test]
    fn each_file_of_a_drop_is_asked_in_turn() {
        let (a, b) = (step_file("drop-a"), step_file("drop-b"));
        let (pa, pb) = (a.to_string_lossy().to_string(), b.to_string_lossy().to_string());
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.drop_path(&pa);
        h.app.drop_path(&pb);
        h.frames(3);
        assert_eq!(h.app.import_options.as_ref().map(|a| a.path.as_str()), Some(pa.as_str()), "the first file is asked first");
        assert_eq!(h.app.import_options_waiting.len(), 1, "the second waits its turn");
        h.step(&json!({"click": {"handle": "import_options:cancel"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.import_options.as_ref().map(|a| a.path.as_str()), Some(pb.as_str()), "Cancel gave up only the first");
        assert!(h.app.file_load.idle(), "the cancelled file is not read");
        h.step(&json!({"click": {"handle": "import_options:ok"}})).unwrap();
        h.settle();
        assert!(h.app.import_options.is_none() && h.app.import_options_waiting.is_empty());
        let names: Vec<String> = h.app.session.world_state().unwrap().bodies.iter().map(|b| b.name.clone()).collect();
        assert_eq!(names.len(), 1, "the second file was imported: {names:?}");
        for p in [a, b] {
            let _ = std::fs::remove_dir_all(p.parent().unwrap());
        }
    }

    /// On Home, Esc gives the question up and leaves Home open, as the unit question does (#15598).
    #[test]
    fn escape_gives_the_question_up_and_keeps_home_open() {
        let path = step_file("home-escape");
        let mut h = crate::scenario::Harness::new();
        h.app.home.show(0);
        h.frames(3);
        h.app.open_chosen_file(&path.to_string_lossy());
        h.frames(3);
        assert!(h.app.home.open && h.app.import_options.is_some(), "asked on Home");
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(3);
        assert!(h.app.import_options.is_none(), "Esc gave the question up");
        assert!(h.app.home.open, "Esc did not close Home behind the question");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A STEP file in a temporary folder, from another program (SolidCraft's own exports are
    /// never offered to be stood up), with two boxes in one product.
    fn step_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-import-options-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{name}.step"));
        let mut src = solidcraft_engine::Session::default();
        src.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        src.execute("file.export", &json!({"path": path.to_string_lossy()})).unwrap();
        path
    }

    /// Opened from the window, a STEP file shows its options at their defaults; Cancel reads
    /// nothing; a changed choice reaches the command; a control-channel open never asks.
    #[test]
    fn a_step_file_opened_from_the_window_asks_and_passes_the_choice_on() {
        let path = step_file("open");
        let p = path.to_string_lossy().to_string();
        let mut h = crate::scenario::Harness::new();
        h.app.open_chosen_file(&p);
        h.frames(3);
        let ask = h.app.import_options.clone().expect("the options are asked");
        assert_eq!(ask.values.fields[0].value, Choice::Choice(0), "the table's default");
        assert!(h.widget("Body names").is_some(), "the option's row is drawn");
        assert!(h.widget("The solid's own name").is_some(), "the default choice is shown");
        assert!(crate::scenario::handle_at("import_options:ok").is_some());

        // Cancel imports nothing.
        h.step(&json!({"click": {"handle": "import_options:cancel"}})).unwrap();
        h.frames(2);
        assert!(h.app.import_options.is_none() && h.app.file_load.idle());
        assert!(h.app.session.doc.features.is_empty(), "Cancel read the file");

        // Esc cancels too.
        h.app.open_chosen_file(&p);
        h.frames(2);
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(2);
        assert!(h.app.import_options.is_none() && h.app.session.doc.features.is_empty(), "Esc read the file");

        // A changed choice is the command's parameter.
        h.app.open_chosen_file(&p);
        h.frames(2);
        h.step(&json!({"click": {"handle": "import_options:names"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "dialog:Body names:The part that holds it"}})).unwrap();
        h.frames(2);
        let held = h.app.import_options.clone().expect("still open");
        assert_eq!(Value::Object(request(&h.app, &held).options), json!({"names": "product"}));
        h.step(&json!({"click": {"handle": "import_options:ok"}})).unwrap();
        h.settle();
        assert!(h.app.import_options.is_none());
        // The file's solid is `Body1` in a product named after the file: named after the part.
        let names: Vec<String> = h.app.session.world_state().unwrap().bodies.iter().map(|b| b.name.clone()).collect();
        assert_eq!(names, ["open"], "the chosen names did not reach the import");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Insert asks too, and Enter imports with the defaults: the same import as without the dialog.
    #[test]
    fn an_inserted_step_file_asks_and_enter_imports() {
        let path = step_file("insert");
        let mut h = crate::scenario::Harness::new();
        h.app.insert_path_async(&path.to_string_lossy());
        h.frames(3);
        let ask = h.app.import_options.clone().expect("the options are asked");
        assert!(ask.insert && h.widget("Insert").is_some(), "the primary action names the insert");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.import_options.is_none(), "Enter closed the dialog");
        assert_eq!(h.app.session.doc.features.len(), 1, "Enter imported the file");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A file whose format has no import options (STL asks only its unit) and a design go on
    /// without this dialog.
    #[test]
    fn a_file_without_options_is_not_asked() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        file_load::begin(&mut app, "part.stl", true);
        assert!(app.import_options.is_none() && app.mesh_units.is_some(), "an STL asks its unit only");
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        file_load::begin(&mut app, "design.solidcraft", false);
        assert!(app.import_options.is_none(), "a design has nothing to ask");
    }

    /// The size of the design's bounding box along X, Y and Z.
    fn size(session: &solidcraft_engine::Session) -> [f64; 3] {
        let s = solidcraft_engine::view::bounds(session).expect("bodies").size();
        [s.x, s.y, s.z]
    }

    /// A glTF file opened from the window asks its up axis, at "Y up" (no stand-up offer follows
    /// its import). Enter keeps it: nothing is sent and the file is stood up as before; Z up chosen
    /// sends `up: "z"` and the file keeps its axes.
    #[test]
    fn a_gltf_file_asks_its_up_axis_and_sends_it_only_when_changed() {
        let dir = std::env::temp_dir().join(format!("solidcraft-import-options-{}-gltf", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("slab.glb").to_string_lossy().to_string();
        let mut src = solidcraft_engine::Session::default();
        src.execute("solid.box", &json!({"length": 10, "width": 40, "height": 5})).unwrap();
        src.execute("file.export", &json!({"path": path})).unwrap();
        // What the command makes of the file by itself, and with its axes kept.
        let mut by_default = solidcraft_engine::Session::default();
        by_default.execute("doc.open", &json!({"path": path})).unwrap();
        let mut kept = solidcraft_engine::Session::default();
        kept.execute("doc.open", &json!({"path": path, "up": "z"})).unwrap();
        let (stood, axes) = (size(&by_default), size(&kept));
        assert!((stood[1] - axes[1]).abs() > 1.0, "the two imports differ: {stood:?} {axes:?}");

        let open = |change: bool| {
            let mut h = crate::scenario::Harness::new();
            h.app.open_chosen_file(&path);
            h.frames(3);
            let ask = h.app.import_options.clone().expect("a glTF file asks its up axis");
            assert_eq!(ask.values.fields[0].param, "up");
            assert!(h.widget("Up axis").is_some() && h.widget("The file has Y up").is_some(), "Up axis at Y up");
            if change {
                h.step(&json!({"click": {"handle": "import_options:up"}})).unwrap();
                h.frames(2);
                h.step(&json!({"click": {"handle": "dialog:Up axis:Keep the file's axes (Z up)"}})).unwrap();
                h.frames(2);
                let held = h.app.import_options.clone().expect("still open");
                assert_eq!(Value::Object(request(&h.app, &held).options), json!({"up": "z"}));
                h.step(&json!({"click": {"handle": "import_options:ok"}})).unwrap();
            } else {
                assert!(request(&h.app, &ask).options.is_empty(), "the default sends nothing");
                h.step(&json!({"key": "Enter"})).unwrap();
            }
            h.settle();
            assert!(h.app.import_options.is_none(), "the file was imported");
            size(&h.app.session)
        };
        let s = open(false);
        assert!((s[1] - stood[1]).abs() < 1e-6 && (s[2] - stood[2]).abs() < 1e-6, "Enter: stood up as before: {s:?} {stood:?}");
        let s = open(true);
        assert!((s[1] - axes[1]).abs() < 1e-6 && (s[2] - axes[2]).abs() < 1e-6, "Z up: not turned: {s:?} {axes:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A control-channel (MCP) open or insert runs the command with its own options: never the dialog.
    #[test]
    fn a_control_open_never_asks() {
        let path = step_file("control");
        let board = board_file("control");
        let mut h = crate::scenario::Harness::new();
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.control_rx = Some(rx);
        for (command, params) in [
            ("doc.open", json!({"path": path.to_string_lossy()})),
            ("file.insert_step", json!({"path": path.to_string_lossy()})),
            // A board, which always asks from the window, never asks a caller either.
            ("doc.open", json!({"path": board.to_string_lossy(), "copper": "mesh"})),
        ] {
            let (req, reply) = crate::control::ControlRequest::new("engine.execute", json!({"command": command, "params": params}));
            tx.send(req).unwrap();
            let mut answer = None;
            for _ in 0..2000 {
                h.frames(1);
                assert!(h.app.import_options.is_none(), "{command} from the control channel opened the dialog");
                answer = reply.try_recv().ok();
                if answer.is_some() {
                    break;
                }
            }
            let answer = answer.expect("answered");
            assert_eq!(answer["ok"], true, "{command}: {answer}");
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        let _ = std::fs::remove_dir_all(board.parent().unwrap());
    }

    /// A KiCad board 2.2 mm thick, its own thickness, with a trace on the front copper.
    fn board_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-import-options-{}-board-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{name}.kicad_pcb"));
        std::fs::write(
            &path,
            r#"(kicad_pcb (version 20240108) (generator pcbnew)
  (general (thickness 2.2))
  (gr_rect (start -20 -20) (end 20 20) (stroke (width 0.1) (type default)) (fill none) (layer "Edge.Cuts"))
  (segment (start -10 0) (end 10 0) (width 2) (layer "F.Cu") (net 0)))"#,
        )
        .unwrap();
        path
    }

    /// A board's options are the table's KiCad row, each starting where `doc.open` does without
    /// it (the thickness at the board's own), and each change is sent as that parameter.
    #[test]
    fn a_boards_options_start_at_the_import_defaults_and_send_each_change() {
        let board = board_file("values");
        let p = board.to_string_lossy();
        let mut v = Values::for_path(&p, &[], &Map::new()).expect("a board imports");
        let shown: Vec<(&str, Choice)> = v.fields.iter().map(|f| (f.param, f.initial.clone())).collect();
        assert_eq!(
            shown,
            [
                ("components", Choice::Bool(true)),
                ("copper", Choice::Choice(1)),
                ("silkscreen", Choice::Bool(true)),
                ("color", Choice::Color([13, 84, 46])),
                ("thickness", Choice::Number(2.2)),
                ("representation", Choice::Choice(0)),
            ],
            "every option the old window had, and the board as a solid or a mesh"
        );
        assert!(v.params().is_empty(), "untouched, the board opens as `doc.open` opens it");
        // At the answer, the thickness read from the board and the table's colour are still the
        // import's own: an untouched board sends nothing.
        assert!(v.params_now(&p, &Map::new()).is_empty(), "untouched, nothing is sent at the answer either");
        // A project file finds its board; a board that cannot be read starts at the table's 1.6 mm.
        let project = Values::for_path(&board.with_extension("kicad_pro").to_string_lossy(), &[], &Map::new()).unwrap();
        assert_eq!(project.fields[4].initial, Choice::Number(2.2));
        let missing = Values::for_path("missing.kicad_pcb", &[], &Map::new()).unwrap();
        assert_eq!(missing.fields[4].initial, Choice::Number(1.6));

        let changes = [Choice::Bool(false), Choice::Choice(2), Choice::Bool(false), Choice::Color([1, 2, 3]), Choice::Number(2.4), Choice::Choice(1)];
        for (field, value) in v.fields.iter_mut().zip(changes) {
            field.value = value;
        }
        assert_eq!(
            Value::Object(v.params()),
            json!({"components": false, "copper": "mesh", "silkscreen": false, "color": "#010203", "thickness": 2.4, "representation": "mesh"})
        );
        // The copper back at the overlay it started at is not sent again.
        v.fields[1].value = Choice::Choice(1);
        assert!(!v.params().contains_key("copper"));
        let _ = std::fs::remove_dir_all(board.parent().unwrap());
    }

    /// Every way of opening a board from the window asks its options before anything is read, and
    /// a board chosen to insert opens as a design of its own, as before. The preference that turns
    /// the other formats' questions off leaves a board's alone.
    #[test]
    fn every_way_of_opening_a_board_asks_its_options() {
        let check = |h: &mut crate::scenario::Harness, how: &str| {
            h.frames(2);
            let ask = h.app.import_options.clone().unwrap_or_else(|| panic!("{how} skipped the board's options"));
            assert_eq!((ask.values.format, ask.insert, ask.tab), ("kicad", false, None), "{how}: a board opens as a design of its own");
            assert!(!h.app.file_load.busy(), "{how} read the board before its options were chosen");
        };
        let mut h = crate::scenario::Harness::new();
        h.app.open_chosen_file("board.kicad_pcb");
        check(&mut h, "the Open picker");
        let mut h = crate::scenario::Harness::new();
        h.app.drop_path("board.kicad_pcb");
        check(&mut h, "a drop");
        // The Open command itself (the palette, a pinned button, `ui.start`).
        let mut h = crate::scenario::Harness::new();
        h.step(&json!({ "choose_file": "board.kicad_pcb" })).unwrap();
        h.app.start("doc.open");
        check(&mut h, "the Open command");
        // The app bar's File > Open and Ctrl+O.
        for by_menu in [true, false] {
            let mut h = crate::scenario::Harness::new();
            h.frames(2);
            h.step(&json!({ "choose_file": "board.kicad_pcb" })).unwrap();
            if by_menu {
                h.step(&json!({ "click": { "handle": "appbar:file" } })).unwrap();
                h.step(&json!({ "widget": "Open…" })).unwrap();
            } else {
                h.step(&json!({ "key": "O", "cmd": true })).unwrap();
            }
            check(&mut h, if by_menu { "the File menu" } else { "Ctrl+O" });
        }
        // Insert Design's picker, and an insert while the preference is off.
        let mut h = crate::scenario::Harness::new();
        h.step(&json!({ "choose_file": "board.kicad_pcb" })).unwrap();
        h.app.pick_design_to_insert();
        check(&mut h, "Insert Design");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.ask_import_options = false;
        h.app.insert_path_async("board.kicad_pcb");
        check(&mut h, "an insert with the preference off");
    }

    /// The window shows every option at the board's values; Cancel reads nothing; the options
    /// changed in it are `doc.open`'s, and the board comes in as `doc.open` with them makes it.
    #[test]
    fn a_board_opened_from_the_window_imports_as_doc_open_with_its_choices() {
        let board = board_file("window");
        let p = board.to_string_lossy().to_string();
        let mut h = crate::scenario::Harness::new();
        h.app.open_chosen_file(&p);
        h.frames(4);
        for label in [
            "Include components",
            "Copper (traces, vias, zones, pads)",
            "Overlay on the board",
            "Include silkscreen",
            "Board color",
            "Board thickness",
            "Board",
            "Solid",
        ] {
            assert!(h.widget(label).is_some(), "`{label}` is missing");
        }
        // The window widens for its longest label: the copper's is drawn whole, not cut short.
        let drawn = h
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text.starts_with("Copper (") => Some(text.galley.job.text.to_string()),
                _ => None,
            })
            .expect("the copper row drew its label");
        assert!(drawn.contains("pads)") && !drawn.contains('…'), "the copper label is cut short: {drawn:?}");
        for handle in ["components", "copper", "silkscreen", "color", "thickness", "representation"] {
            assert!(crate::scenario::handle_at(&format!("import_options:{handle}")).is_some(), "no `{handle}` row");
        }
        assert_eq!(h.app.import_options.as_ref().unwrap().values.fields[4].value, Choice::Number(2.2), "the board's thickness");
        h.step(&json!({"click": {"handle": "import_options:cancel"}})).unwrap();
        h.frames(2);
        assert!(h.app.import_options.is_none() && h.app.file_load.idle() && h.app.session.doc.features.is_empty(), "Cancel read the board");

        h.app.open_chosen_file(&p);
        h.frames(3);
        h.step(&json!({"click": {"handle": "import_options:copper"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "dialog:Copper (traces, vias, zones, pads):Modeled"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "import_options:silkscreen"}})).unwrap();
        h.frames(2);
        // The colour picker and the number field are the kit's, tested there: set their values.
        if let Some(ask) = h.app.import_options.as_mut() {
            ask.values.fields[3].value = Choice::Color([200, 30, 30]);
            ask.values.fields[4].value = Choice::Number(1.2);
        }
        h.frames(1);
        let held = h.app.import_options.clone().expect("still open");
        let params = request(&h.app, &held).options;
        assert_eq!(Value::Object(params.clone()), json!({"copper": "mesh", "silkscreen": false, "color": "#c81e1e", "thickness": 1.2}));
        h.step(&json!({"click": {"handle": "import_options:ok"}})).unwrap();
        h.settle();
        assert!(h.app.import_options.is_none() && !h.app.file_load.busy());

        let mut direct = solidcraft_engine::Session::default();
        let mut with_path = params.clone();
        with_path.insert("path".into(), json!(p));
        direct.execute("doc.open", &Value::Object(with_path)).unwrap();
        let names = |s: &solidcraft_engine::Session| {
            let mut n: Vec<String> = s.world_state().unwrap().bodies.iter().map(|b| b.name.clone()).collect();
            n.sort();
            n
        };
        assert_eq!(names(&h.app.session), names(&direct), "the window's import is `doc.open`'s");
        assert!(names(&direct).iter().any(|n| n == "Copper F.Cu"), "{:?}", names(&direct));
        assert_eq!(size(&h.app.session), size(&direct), "the board's thickness reached the import");
        let _ = std::fs::remove_dir_all(board.parent().unwrap());
    }
}
