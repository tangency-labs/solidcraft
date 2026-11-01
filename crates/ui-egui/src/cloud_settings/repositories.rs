//! Repository rows and a fixed operation footer for the Cloud Saves preferences page.
use super::*;

pub(super) fn table(ui: &mut egui::Ui, editor: &mut Editor, statuses: &[JobStatus]) -> Option<&'static str> {
    let mut remove = None;
    let mut add = None;
    ui_kit::table(
        ui,
        "cloud_repositories",
        &[
            ui_kit::Column::new(tr("cloud.repository"), 1.0).flex().min_content_width(180.0),
            ui_kit::Column::new(crate::i18n::tr_label("Default"), 0.5),
            ui_kit::Column::new(crate::i18n::tr_label("Remove"), 0.0).icon_only(),
        ],
        tr("cloud.connections"),
        |ui, table| {
            for connection in &editor.draft.connections {
                let selected = editor.selected.as_deref() == Some(&connection.id);
                let active = statuses.iter().any(|s| s.connection == connection.id && matches!(s.phase, Phase::Working | Phase::Syncing));
                let mut picked = false;
                let response = table.row(ui, &connection.id, selected, |row| {
                    row.cell(|ui| {
                        picked = ui_kit::text(ui, &connection.repository_label(), ui_kit::Text::Cell).clicked();
                    });
                    row.cell(|ui| {
                        let mut default = editor.draft.default_connection.as_deref() == Some(&connection.id);
                        let response = ui_kit::checkbox(ui, "", &mut default).on_hover_text(tr("cloud.default_repository"));
                        response
                            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Checkbox, ui.is_enabled(), tr("cloud.default_repository")));
                        crate::scenario::publish_target(&format!("cloud_settings:default:{}", connection.id), response.rect);
                        if selected {
                            crate::scenario::publish_target("cloud_settings:default", response.rect);
                        }
                        if response.changed() {
                            editor.draft.default_connection = default.then(|| connection.id.clone());
                        }
                    });
                    row.cell(|ui| {
                        let response = ui.add_enabled_ui(!active, |ui| ui_kit::bin_button(ui, crate::i18n::tr_label("Remove"))).inner;
                        crate::scenario::publish_target(&format!("cloud_settings:remove:{}", connection.id), response.rect);
                        if selected {
                            crate::scenario::publish_target("cloud_settings:disconnect", response.rect);
                        }
                        if response.clicked() {
                            remove = Some(connection.id.clone());
                        }
                    });
                });
                crate::scenario::publish_target(&format!("cloud_settings:row:{}", connection.id), response.rect);
                if response.clicked() || picked {
                    editor.selected = Some(connection.id.clone());
                }
            }
            ui.add_enabled_ui(editor.draft.connections.len() < 128, |ui| {
                let response = table.add_prompt(ui, tr("cloud.add_storage"), tr("cloud.add_storage"));
                crate::scenario::publish_target("cloud_settings:connect", response.rect);
                if let Some(choice) =
                    ui_kit::Container::new(ui.ctx(), ui_kit::Surface::Popup).menu(&response).show(crate::cloud_setup::repository_choices)
                {
                    add = choice.inner;
                }
            });
        },
    );
    if let Some(id) = remove {
        editor.draft.connections.retain(|c| c.id != id);
        if editor.draft.default_connection.as_ref() == Some(&id) {
            editor.draft.default_connection = None;
        }
        if editor.selected.as_ref() == Some(&id) {
            editor.selected = editor.draft.connections.first().map(|c| c.id.clone());
        }
    }
    ui_kit::text(ui, tr("cloud.disconnect_hint"), ui_kit::Text::Hint("cloud_disconnect_hint"));
    add
}

pub(super) fn status(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    connection: &Connection,
    statuses: &[JobStatus],
    test: Option<&str>,
    error: &mut Option<String>,
) {
    let active = statuses.iter().find(|s| s.connection == connection.id && matches!(s.phase, Phase::Working | Phase::Syncing));
    let tested = statuses.iter().find(|s| s.connection == connection.id && Some(s.operation.as_str()) == test);
    let message = if active.is_some() {
        if active.is_some_and(|s| Some(s.operation.as_str()) == test) { tr("cloud.testing") } else { tr("cloud.syncing") }
    } else if let Some(status) = tested {
        status.message.as_deref().unwrap_or_else(|| if matches!(status.phase, Phase::Ready | Phase::Synced) { tr("cloud.test_ok") } else { "" })
    } else {
        ""
    };
    let response =
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), ui_kit::row_height()), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let id = active.map(|s| format!("cloud_settings:cancel:{}", s.operation)).unwrap_or_else(|| "cloud_settings:cancel".into());
            if busy_button(ui, &id, tr("cmd.cloud.cancel"), active.is_some(), active.is_some_and(|s| app.cloud.operations.cancelling(&s.operation)))
                && let Some(status) = active
                && let Err(message) = app.run("cloud.cancel", json!({"operation":status.operation}))
            {
                *error = Some(message);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(message).truncate()).on_hover_text(message);
            });
        });
    crate::scenario::publish_rect("cloud_settings:status", response.response.rect);
}
