//! The saved configurations offered by interactive Insert Design. Reading runs on the file
//! worker; this form only chooses parameters for the existing insertion command.

use serde_json::json;
use solidcraft_engine::doc::config::ConfigTable;

use crate::ui_kit::BackingTooltipExt as _;
use crate::{SolveApp, file_load, ui_kit};

pub(crate) struct Ask {
    request: file_load::Request,
    pub(crate) rows: ConfigTable,
    /// Zero follows the saved state; the other choices use the source row's stable id.
    pub(crate) selected: usize,
}

/// Read metadata without evaluating the source or changing the destination design. The caller
/// supplies the file worker's path scope and one-request network permission.
pub(crate) fn read(path: &str, design: Option<&str>) -> Result<ConfigTable, String> {
    let result = (|| {
        let bytes = solidcraft_engine::io::vfs::read_design_path(path, design, solidcraft_engine::io::limits::max_design_bytes())
            .map_err(|error| solidcraft_engine::io::limits::explain(solidcraft_engine::io::limits::Kind::Design, error).to_string())?;
        solidcraft_engine::io::read_design(&bytes).map(|doc| doc.configs).map_err(|error| error.to_string())
    })();
    result.map_err(|reason| solidcraft_ui_text::format("insert_design.configuration_read_failed", &[("reason", &reason)]))
}

pub(crate) fn offer(app: &mut SolveApp, request: file_load::Request, rows: ConfigTable) {
    app.insert_design_options = Some(Ask { request, rows, selected: 0 });
}

impl Ask {
    fn request(mut self) -> Option<file_load::Request> {
        if self.selected > 0 {
            let row = self.rows.rows.get(self.selected - 1)?;
            self.request.options.insert("configuration".into(), json!(row.id));
            self.request.options.insert("configuration_identity".into(), json!(self.rows.row_identity(row.id)));
        }
        // Reading the list and inserting the chosen design are separate operations.
        self.request.serial = crate::operations::serial();
        self.request.tries = 0;
        self.request.choose_configuration = false;
        Some(self.request)
    }
}

/// Cancel, Escape and the window's close button leave both designs and undo history untouched.
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut ask) = app.insert_design_options.take() else { return };
    if ask.request.tab.is_some_and(|tab| !app.docs.tab_open(tab)) {
        return;
    }
    if ask.request.tab.is_some_and(|tab| tab != app.docs.current_tab()) {
        app.insert_design_options = Some(ask);
        return;
    }
    let file = std::path::Path::new(&ask.request.path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| ask.request.path.clone());
    let title = app.tr("cmd.component_insert_design");
    let insert = app.tr("ui.insert");
    let cancel = app.tr("ui.cancel");
    let saved = app.tr("insert_design.saved_state");
    let labels: Vec<&str> = std::iter::once(saved).chain(ask.rows.rows.iter().map(|row| row.name.as_str())).collect();
    let valid = ask.selected <= ask.rows.rows.len();
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_insert_design_configuration"), "import", title)
        .actions(insert, cancel, None)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, valid, |ui| {
            ui_kit::text(ui, &file, ui_kit::Text::Report("file", false));
            // Row names are user data, so do not translate names such as "Open" or "Default"
            // into command labels. The shared dropdown still supplies the themed container.
            let mut restore_focus = false;
            let current = labels.get(ask.selected).copied().unwrap_or_default();
            let response = ui_kit::row(ui, app.tr("insert_design.configuration"), |ui| {
                let width = ui.available_width();
                ui_kit::bounded_dropdown_menu(ui, "insert_design_configuration", current, width, |ui| {
                    for (index, name) in labels.iter().enumerate() {
                        let response = ui.selectable_value(&mut ask.selected, index, *name).backing_on_hover_text(*name);
                        if response.clicked() {
                            ui.close();
                            restore_focus = !response.clicked_by(egui::PointerButton::Primary);
                        }
                    }
                })
            });
            if restore_focus {
                response.request_focus();
                ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            }
            crate::scenario::publish_handle("insert_design:configuration", response.rect.center());
            ui_kit::text(ui, app.tr("insert_design.configuration_help"), ui_kit::Text::Report("help", false));
        });
    for (handle, key) in [("insert_design:ok", "dialog:ok"), ("insert_design:cancel", "dialog:cancel")] {
        if let Some(at) = crate::scenario::handle_at(key) {
            crate::scenario::publish_handle(handle, at);
        }
    }
    match action {
        ui_kit::Action::Ok => {
            if let Some(request) = ask.request() {
                let _ = file_load::begin_request(app, request);
            }
        }
        ui_kit::Action::None if !closed => app.insert_design_options = Some(ask),
        _ => {
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        }
    }
}
