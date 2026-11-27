//! A file that belongs to another design, opened by itself (#6892): an internal component's
//! file, or a drawing's, says in its header which design it belongs to
//! (`solidcraft_io::InternalOf`). Away from that design it is most likely a recovery file a crash
//! left, or a copy someone took out of the design's work folder. Opening one through its design
//! (Edit in place) shows nothing. Opened while its design is not open here, one window says what
//! the file is and offers the two things that make sense: open the design it belongs to, where
//! Recovery components brings its work in, or open it as a new design to save as a file of its
//! own. The engine only reports (`doc.open`'s `internal_of`) and opens (`as_copy`); this window
//! is the app's layer.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::ui_kit;

/// The window's state: the file opened and the design it belongs to.
#[derive(Clone, Debug, PartialEq)]
pub struct InternalFile {
    /// The file that was opened.
    pub path: String,
    /// The design's name, as the file's tag has it.
    pub design: String,
    /// The design's file, when it was found holding this file.
    pub owner: Option<String>,
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

/// After an open: the window, when the reply says the file belongs to another design and that
/// design is not open in this app. An open that asked for a copy has its answer already.
pub(crate) fn offer(app: &mut SolveApp, reply: &Value) {
    app.internal_file = None;
    let (Some(design), Some(path)) = (reply["internal_of"]["design"].as_str(), reply["path"].as_str()) else { return };
    if reply["as_copy"] == true {
        return;
    }
    let owner = solidcraft_engine::internal_owner(path);
    // Its design is open in another tab: this is Edit in place, or the same as it.
    let open_here = |file: &str| app.docs.background().filter_map(|s| s.path.as_deref()).any(|open| same_file(open, file));
    if owner.as_deref().is_some_and(open_here) {
        return;
    }
    app.internal_file = Some(InternalFile { path: path.to_string(), design: design.to_string(), owner });
}

fn same_file(a: &str, b: &str) -> bool {
    let full = |p: &str| std::path::absolute(p).unwrap_or_else(|_| p.into());
    full(a) == full(b)
}

/// Open the design the file belongs to, in place of the file.
pub(crate) fn open_owner(app: &mut SolveApp) {
    let Some(owner) = app.internal_file.take().and_then(|file| file.owner) else { return };
    // The file is not left open by itself in a tab beside its design: its tab becomes a blank
    // design, which the open then uses.
    let _ = app.run("file.new", json!({}));
    // A design can be large: the app opens it with progress and Cancel, a test here and now.
    #[cfg(not(test))]
    app.open_path_async(&owner);
    #[cfg(test)]
    app.open_path(&owner);
}

/// Open the file as a new, unsaved design of its own, in place of the file as it is.
pub(crate) fn open_copy(app: &mut SolveApp) {
    let Some(file) = app.internal_file.take() else { return };
    let mut options = serde_json::Map::new();
    options.insert("as_copy".into(), json!(true));
    // The file is the design in this tab, so opening its path again would only come back to
    // this tab: the open runs here, in its place.
    let params = app.load_params(&file.path, &options);
    match app.run("doc.open", params) {
        Ok(reply) => {
            app.fit_view();
            app.say_kind_note(&reply);
        }
        Err(e) => app.set_status(e, true),
    }
}

/// Neither: the file is not left open by itself.
pub(crate) fn close(app: &mut SolveApp) {
    if app.internal_file.take().is_some() {
        let _ = app.run("file.new", json!({}));
    }
}

/// The window, while a file that belongs to another design is open by itself.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(file) = app.internal_file.clone() else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let name = file_name(&file.path);
    let detail = solidcraft_ui_text::format("internal_file.detail", &[("name", &name), ("design", &file.design)]);
    let open_owner_label = solidcraft_ui_text::format("internal_file.open_owner", &[("design", &file.design)]);
    let open_copy_label = solidcraft_ui_text::tr("internal_file.open_copy").to_string();
    let close_label = solidcraft_ui_text::tr("internal_file.close").to_string();
    // With the design found, opening it is the answer offered first; without it, the copy is.
    let found = file.owner.is_some();
    let (ok, left) = if found { (open_owner_label.as_str(), Some(open_copy_label.as_str())) } else { (open_copy_label.as_str(), None) };
    let (closed, action) =
        ui_kit::Confirm::new(egui::Id::unique("sc_internal_file"), "status_warning", solidcraft_ui_text::tr("internal_file.title"))
            .tone(ui_kit::Tone::Warning)
            .actions(ok, &close_label, left)
            .secondary_left()
            .show(ctx, true, |ui| {
                ui_kit::text(ui, &detail, ui_kit::Text::Report("internal_file", false));
                if !found {
                    let missing = solidcraft_ui_text::format("internal_file.owner_missing", &[("design", &file.design)]);
                    ui_kit::text(ui, &missing, ui_kit::Text::Note("internal_file_owner_missing"));
                }
            });
    match action {
        ui_kit::Action::Ok if found => open_owner(app),
        ui_kit::Action::Ok | ui_kit::Action::Apply => open_copy(app),
        ui_kit::Action::Cancel => close(app),
        _ if closed => close(app),
        _ => {}
    }
    if app.internal_file.is_some() && ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        close(app);
        app.esc_handled = true;
    }
}
