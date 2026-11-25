//! Insert Design's options: once a design is picked (from the file picker or the designs open in
//! other tabs), the panel on the right asks how a linked design that has components of its own
//! comes in, and OK inserts it beside the window as before (#5526, #11593). Both options start
//! off: off, the design comes in as one linked component, which is what `component.insert_design`
//! does when it is not told. A script, the control channel or MCP calls the command with its own
//! `structure` and `flexible` and never meets this panel.

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, form_row};
use crate::ui_kit::BackingTooltipExt as _;

/// The design being inserted and how it comes in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InsertDesign {
    pub path: String,
    /// The status line once it is in (a design open here with unsaved changes says which file).
    pub note: Option<String>,
    /// Keep the linked design's own components as components inside the linked one.
    pub keep: bool,
    /// The new occurrence is flexible: the design's joints come too. Needs `keep`.
    pub flexible: bool,
}

/// Open the panel for the design at `path`, both options off.
pub fn open(app: &mut SolveApp, path: &str, note: Option<String>) {
    app.dialog = Some(Dialog::new(Kind::InsertDesign(InsertDesign { path: path.into(), note, keep: false, flexible: false }), Vec::new()));
}

pub fn title() -> &'static str {
    "INSERT DESIGN"
}

pub fn rows(ui: &mut egui::Ui, f: &mut InsertDesign) {
    let name = std::path::Path::new(&f.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| f.path.clone());
    form_row(ui, solidcraft_ui_text::tr("ui.file"), |ui| {
        ui.label(name).backing_on_hover_text(&f.path);
    });
    let r = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("insert_design.keep_components"), &mut f.keep);
    crate::scenario::publish_handle("dialog:insert_design:keep", r.rect.center());
    // One component brings no joints, so Flexible waits for the components, greyed and saying why.
    if !f.keep {
        f.flexible = false;
    }
    ui.add_enabled_ui(f.keep, |ui| {
        let help = solidcraft_ui_text::tr("insert_design.flexible_help");
        let r = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("insert_design.flexible"), &mut f.flexible)
            .backing_on_hover_text(help)
            .backing_on_disabled_hover_text(help);
        crate::scenario::publish_handle("dialog:insert_design:flexible", r.rect.center());
    });
}

/// OK: insert the design, read beside the window, as these options ask
/// ([`crate::linked_source::insert_design_options`]). The choice is remembered for a paste of a
/// copied occurrence, which has no panel of its own.
pub fn run(app: &mut SolveApp, f: &InsertDesign) -> Result<(), String> {
    app.insert_design_keeps_components = f.keep;
    app.insert_design_async(&f.path, f.note.clone(), &crate::linked_source::insert_design_options(f.keep, f.flexible));
    Ok(())
}
