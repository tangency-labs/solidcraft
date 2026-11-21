//! UI for the named arrangements command family. Only the menu opens this dialog.

use crate::{SolveApp, ui_kit};
use serde_json::json;

#[derive(Clone)]
enum Editor {
    Save,
    Manage,
    Rename(String),
}

#[derive(Clone)]
struct Dialog {
    editor: Editor,
    text: String,
    first: bool,
    error: Option<String>,
}

fn id(ctx: &egui::Context) -> egui::Id {
    egui::Id::unique(("view_setup_dialog", ctx.viewport_id()))
}

pub(crate) fn handle(app: &mut SolveApp, ctx: &egui::Context, row: &str) -> bool {
    let editor = match row {
        "layout.save_view" => Editor::Save,
        "layout.manage" => Editor::Manage,
        _ => {
            let Some(name) = row.strip_prefix("layout.saved.") else { return false };
            if let Err(error) = app.run("tab.layout.apply", json!({"name": name})) {
                app.set_status(message(app, &error), true);
            }
            return true;
        }
    };
    let id = id(ctx);
    ctx.data_mut(|data| data.insert_temp(id, Dialog { editor, text: String::new(), first: true, error: None }));
    true
}

pub(crate) fn message(app: &SolveApp, error: &str) -> String {
    let key = match error {
        "layout_name_exists" => "view_layout.error_exists",
        "too_many_layouts" => "view_layout.error_limit",
        "invalid_layout_setup" => "view_layout.error_setup",
        "no_such_layout" => "view_layout.error_missing",
        "invalid_layout_name" => "view_layout.error_name",
        _ => return error.to_owned(),
    };
    app.tr(key).to_owned()
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let id = id(ctx);
    let Some(mut state) = ctx.data(|data| data.get_temp::<Dialog>(id)) else { return };
    let names = crate::view_setups::names(app);
    let mut command = None;
    let mut dismiss = false;
    let modal = ui_kit::Container::new(ctx, ui_kit::Surface::Window).modal(id).show(ctx, |ui| {
        ui.set_width(340.0);
        match &state.editor {
            Editor::Manage => {
                ui.heading(app.tr("view_layout.manage_title"));
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for name in &names {
                        ui.push_id(name, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::Label::new(name).truncate());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui_kit::button(ui, app.tr("ui.delete"), ui_kit::Button::Small).clicked() {
                                        command = Some(("tab.layout.remove", json!({"name": name})));
                                    }
                                    if ui_kit::button(ui, app.tr("view_layout.rename"), ui_kit::Button::Small).clicked() {
                                        state.editor = Editor::Rename(name.clone());
                                        state.text.clone_from(name);
                                        state.first = true;
                                        state.error = None;
                                    }
                                });
                            });
                        });
                    }
                });
                dismiss = ui_kit::button(ui, app.tr("ui.close"), ui_kit::Button::Primary).clicked();
            }
            Editor::Save | Editor::Rename(_) => {
                let save = matches!(&state.editor, Editor::Save);
                ui.heading(app.tr(if save { "view_layout.save_title" } else { "view_layout.rename_title" }));
                let (entered, field) = ui_kit::text_field_reserving(ui, &mut state.text, app.tr("view_layout.name"), 0.0);
                if let Some((cut, _)) = state.text.char_indices().nth(64) {
                    state.text.truncate(cut);
                }
                field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, app.tr("view_layout.name")));
                if state.first {
                    field.request_focus();
                    state.first = false;
                }
                ui.horizontal(|ui| {
                    dismiss = ui_kit::button(ui, app.tr("ui.cancel"), ui_kit::Button::Small).clicked();
                    let submit = ui.add_enabled_ui(!state.text.trim().is_empty(), |ui| {
                        ui_kit::button(ui, app.tr(if save { "ui.save" } else { "view_layout.rename" }), ui_kit::Button::Primary)
                    });
                    if !state.text.trim().is_empty() && (submit.inner.clicked() || entered) {
                        command = Some(match &state.editor {
                            Editor::Rename(name) => ("tab.layout.rename", json!({"name": name, "new_name": state.text})),
                            _ => ("tab.layout.save", json!({"name": state.text})),
                        });
                    }
                });
            }
        }
        if let Some(error) = &state.error {
            ui.label(error);
        }
    });
    if let Some((method, request)) = command {
        match app.run(method, request) {
            Ok(_) if method == "tab.layout.save" => dismiss = true,
            Ok(_) => {
                state.editor = Editor::Manage;
                state.error = None;
            }
            Err(error) => state.error = Some(message(app, &error)),
        }
    }
    let closed = dismiss || modal.should_close();
    ctx.data_mut(|data| {
        if closed {
            data.remove::<Dialog>(id);
        } else {
            data.insert_temp(id, state);
        }
    });
}
