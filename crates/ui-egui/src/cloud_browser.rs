//! Start-page repository browsing and the initial cloud destination picker. Commands own
//! IO and durability; these caches are presentation only and never decide publication.
use std::{collections::BTreeMap, sync::Arc};

use serde_json::json;
use solidcraft_cloud::{
    Connection, Destination,
    jobs::{JobStatus, Phase},
};
use solidcraft_engine::{Session, doc::Document};

use crate::{SolveApp, i18n::tr, ui_kit};

#[derive(Default)]
pub(crate) struct Browser {
    selected: Option<String>,
    catalogs: BTreeMap<String, Catalog>,
    picker: Option<Picker>,
    opening: Option<String>,
    error: Option<String>,
    statuses: Vec<SaveStatus>,
}

struct SaveStatus {
    destination: Destination,
    operation: u64,
    phase: Phase,
    conflict: bool,
}

struct Catalog {
    connection: Connection,
    entries: Arc<Vec<Destination>>,
    pending: Option<String>,
    message: Option<String>,
}

struct Picker {
    owner: Arc<Document>,
    tab: usize,
    connection: String,
    project: String,
    name: String,
}

pub(crate) fn start(app: &mut SolveApp, command: &str) {
    if app.services.cloud_host.is_none() {
        return;
    }
    if command == "cloud.list" {
        app.home.show(0);
        return;
    }
    if app.session.cloud_binding().is_some() {
        if let Err(error) = app.run("cloud.save", json!({})) {
            app.set_status(error, true);
        }
        return;
    }
    let Some(connection) = selected(app) else {
        app.start("cloud.connect");
        return;
    };
    if !app.cloud.browser.catalogs.contains_key(&connection.id) && !app.session.cloud_configuring() {
        request(app, &connection);
    }
    app.cloud.browser.picker = Some(Picker {
        owner: app.session.doc.clone(),
        tab: app.docs.active,
        connection: connection.id.clone(),
        project: "Designs".into(),
        name: app.session.doc.name.clone(),
    });
    app.cloud.browser.selected = Some(connection.id);
    app.cloud.browser.error = None;
}

pub(crate) fn choosing_destination(app: &SolveApp) -> bool {
    app.cloud.browser.picker.is_some()
}

fn request(app: &mut SolveApp, connection: &Connection) {
    let result = app.run("cloud.list", json!({"connection":connection.id}));
    match result.and_then(|r| r.get("operation").and_then(|v| v.as_str()).map(str::to_owned).ok_or_else(|| "missing list operation".into())) {
        Ok(operation) => {
            let previous = app.cloud.browser.catalogs.remove(&connection.id);
            if let Some(old) = previous.as_ref().and_then(|c| c.pending.as_ref()) {
                let _ = app.run("cloud.cancel", json!({"operation":old}));
            }
            let entries = previous.filter(|c| c.connection == *connection).map(|c| c.entries).unwrap_or_default();
            app.cloud
                .browser
                .catalogs
                .insert(connection.id.clone(), Catalog { connection: connection.clone(), entries, pending: Some(operation), message: None });
        }
        Err(error) => {
            app.cloud.browser.catalogs.entry(connection.id.clone()).or_insert_with(|| Catalog {
                connection: connection.clone(),
                entries: Default::default(),
                pending: None,
                message: Some(error.clone()),
            });
            app.cloud.browser.error = Some(error);
        }
    }
}

fn entries(status: &JobStatus, connection: &str) -> Result<Vec<Destination>, String> {
    let value = status.result.clone().ok_or("missing repository listing")?;
    let values = value.as_array().ok_or("invalid repository listing")?;
    if values.len() > 10_000 {
        return Err("repository listing is too large".into());
    }
    let mut entries: Vec<Destination> = serde_json::from_value(value).map_err(|e| e.to_string())?;
    for destination in &entries {
        destination.validate().map_err(|e| e.to_string())?;
        if destination.connection != connection {
            return Err("repository listing belongs to another connection".into());
        }
    }
    entries.sort_by(|a, b| (&a.project, &a.file).cmp(&(&b.project, &b.file)));
    entries.dedup();
    Ok(entries)
}

/// Poll completion in every tab, including when Start is hidden. Downloaded designs are
/// completed in a fresh editor so changing tabs or editing during IO cannot lose work.
pub(crate) fn tick(app: &mut SolveApp, statuses: &[JobStatus]) {
    app.cloud.browser.statuses = statuses
        .iter()
        .filter_map(|s| {
            Some(SaveStatus {
                destination: s.destination.clone()?,
                operation: s.operation.strip_prefix("cloud-").and_then(|id| id.parse().ok()).unwrap_or(0),
                phase: s.phase,
                conflict: s.publications.iter().any(|p| p.conflict && Some(&p.destination) == s.destination.as_ref()),
            })
        })
        .collect();
    let attention = app.tr("cloud.needs_attention").to_owned();
    let settings = app.session.cloud_settings();
    app.cloud.browser.catalogs.retain(|id, c| settings.connection(id) == Some(&c.connection));
    for (id, catalog) in &mut app.cloud.browser.catalogs {
        if let Some(operation) = catalog.pending.as_ref()
            && let Some(status) = statuses.iter().find(|s| s.operation == *operation)
        {
            match status.phase {
                Phase::Ready => {
                    match entries(status, id) {
                        Ok(entries) => {
                            catalog.entries = Arc::new(entries);
                            catalog.message = status.message.clone();
                        }
                        Err(error) => catalog.message = Some(error),
                    }
                    catalog.pending = None;
                }
                Phase::Failed | Phase::Cancelled => {
                    catalog.message = status.message.clone().or_else(|| Some(attention.clone()));
                    catalog.pending = None;
                }
                _ => {}
            }
        }
        if catalog.pending.as_ref().is_some_and(|operation| !statuses.iter().any(|s| s.operation == *operation)) {
            catalog.pending = None;
            catalog.message = Some(attention.clone());
        }
        // New local captures appear immediately, even before the next repository refresh.
        for status in statuses.iter().filter(|s| s.connection == *id && s.saved_locally) {
            if let Some(destination) = &status.destination
                && !catalog.entries.contains(destination)
            {
                Arc::make_mut(&mut catalog.entries).push(destination.clone());
            }
        }
    }
    let Some(operation) = app.cloud.browser.opening.clone() else { return };
    let Some(status) = statuses.iter().find(|s| s.operation == operation) else {
        app.cloud.browser.opening = None;
        app.cloud.browser.error = Some(app.tr("cloud.needs_attention").into());
        return;
    };
    match status.phase {
        Phase::Ready => {
            // Do not consume the downloaded snapshot if there is no room to adopt it.
            if app.docs.count() >= crate::documents::MAX_OPEN && !crate::documents::active_is_blank(app) {
                app.cloud.browser.error = Some(tr("cloud.tabs_full").into());
                return;
            }
            if app.file_picker_busy() || app.saving.is_some() {
                return;
            }
            let result = opened_session(app, &operation);
            app.cloud.browser.opening = None;
            match result {
                Ok(session) => {
                    crate::documents::adopt(app, session);
                    app.home.open = false;
                    app.cloud.browser.error = None;
                }
                Err(error) => app.cloud.browser.error = Some(error),
            }
        }
        Phase::Failed | Phase::Cancelled => {
            app.cloud.browser.opening = None;
            app.cloud.browser.error = status.message.clone().or_else(|| Some(tr("cloud.needs_attention").into()));
        }
        _ => {}
    }
}

fn opened_session(app: &mut SolveApp, operation: &str) -> Result<Session, String> {
    if app.session.cloud_operations().map_err(|e| e.to_string())?.iter().any(|s| s.operation == operation) {
        return app.session.cloud_open_session(operation).map_err(|e| e.to_string());
    }
    for slot in app.docs.background_mut() {
        if slot.session.cloud_operations().map_err(|e| e.to_string())?.iter().any(|s| s.operation == operation) {
            return slot.session.cloud_open_session(operation).map_err(|e| e.to_string());
        }
    }
    Err("opening operation no longer available".into())
}

fn open(app: &mut SolveApp, destination: &Destination) {
    if app.cloud.browser.opening.is_some() {
        return;
    }
    match app.run("cloud.open", json!({"destination":destination})) {
        Ok(result) => {
            app.cloud.browser.opening = result.get("operation").and_then(|v| v.as_str()).map(str::to_owned);
            app.cloud.browser.error = None;
        }
        Err(error) => app.cloud.browser.error = Some(error),
    }
}

fn phase(status: &SaveStatus) -> &'static str {
    if status.conflict {
        return tr("cloud.conflict_copy");
    }
    match status.phase {
        Phase::Synced => tr("cloud.synced"),
        Phase::Syncing => tr("cloud.syncing"),
        Phase::SavedLocally => tr("cloud.saved_locally"),
        Phase::Failed | Phase::Cancelled => tr("cloud.needs_attention"),
        Phase::Working => tr("cloud.saving"),
        Phase::Ready => "",
    }
}

fn selected(app: &SolveApp) -> Option<Connection> {
    let settings = app.session.cloud_settings();
    app.cloud
        .browser
        .selected
        .as_deref()
        .and_then(|id| settings.connection(id))
        .or_else(|| settings.default_connection.as_deref().and_then(|id| settings.connection(id)))
        .or_else(|| settings.connections.first())
        .cloned()
}

pub(crate) fn home(app: &mut SolveApp, ui: &mut egui::Ui) {
    if app.services.cloud_host.is_none() {
        return;
    }
    let t = crate::theme::Tokens::get();
    let mut connection = selected(app);
    if let Some(c) = &connection
        && !app.cloud.browser.catalogs.contains_key(&c.id)
        && !app.session.cloud_configuring()
    {
        request(app, c);
    }
    ui_kit::FileView::section(ui, tr("cloud.title"), t.space_10, |ui| {
        ui.horizontal_wrapped(|ui| {
            if button(ui, "cloud:connect", tr("cmd.cloud.connect"), true) {
                app.start("cloud.connect");
            }
            if connection.is_some() && button(ui, "cloud:save", tr("cmd.cloud.save"), !app.session.cloud_save_pending()) {
                app.start("cloud.save");
            }
            if button(ui, "cloud:settings", tr("cmd.cloud.settings"), true) {
                app.start("cloud.settings");
            }
            ui.hyperlink_to(crate::i18n::tr_label("Help"), "https://tangency-labs.github.io/solidcraft/guide/cloud-saves.html");
        });
        ui.add_space(t.space_8);
        if let Some(c) = &mut connection {
            let connections = app.session.cloud_settings().connections.clone();
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("cloud.repository"));
                let response = egui::ComboBox::from_id_salt("cloud_repo").selected_text(&c.name).show_ui(ui, |ui| {
                    for candidate in &connections {
                        if ui.selectable_label(candidate.id == c.id, &candidate.name).clicked() {
                            *c = candidate.clone();
                        }
                    }
                });
                crate::scenario::publish_target("cloud:repositories", response.response.rect);
                if button(ui, "cloud:refresh", tr("ui.agent_chat_refresh"), !app.session.cloud_configuring()) {
                    request(app, c);
                }
                if button(ui, "cloud:sync", tr("cmd.cloud.sync"), !app.session.cloud_configuring())
                    && let Err(error) = app.run("cloud.sync", json!({"connection":c.id}))
                {
                    app.cloud.browser.error = Some(error);
                }
            });
            app.cloud.browser.selected = Some(c.id.clone());
            if let Some(catalog) = app.cloud.browser.catalogs.get(&c.id) {
                let pending = catalog.pending.is_some();
                let designs = catalog.entries.clone();
                if pending {
                    ui.spinner();
                }
                if let Some(message) = &catalog.message {
                    ui_kit::text(ui, message, ui_kit::Text::Report("cloud_list_message", true));
                }
                ui.add_space(t.space_8);
                if designs.is_empty() && !pending {
                    ui_kit::FileView::detail(ui, tr("cloud.empty"));
                }
                let statuses = &app.cloud.browser.statuses;
                let mut wanted = None;
                ui_kit::FileView::recent_list(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt(("cloud_designs", &c.id)).max_height(t.file_recent_height * 6.0).show_rows(
                        ui,
                        t.file_recent_height,
                        designs.len(),
                        |ui, rows| {
                            for index in rows {
                                let Some(destination) = designs.get(index) else { continue };
                                let status = statuses.iter().filter(|s| &s.destination == destination).max_by_key(|s| s.operation);
                                let path = format!("{} / {}", destination.project, destination.file);
                                let response = ui
                                    .add_enabled_ui(app.cloud.browser.opening.is_none(), |ui| {
                                        ui_kit::FileView::recent_row(
                                            ui,
                                            &format!("cloud:design:{index}"),
                                            ui_kit::RecentRow {
                                                title: destination.file.rsplit('/').next().unwrap_or(&destination.file),
                                                path: &path,
                                                when: status.map(phase).unwrap_or(""),
                                                workspace: tr("home.design_workspace"),
                                                thumbnail: None,
                                            },
                                        )
                                    })
                                    .inner;
                                crate::scenario::publish_target(&format!("cloud:design:{index}"), response.rect);
                                if response.clicked() {
                                    wanted = Some(destination.clone());
                                }
                            }
                        },
                    );
                });
                if let Some(destination) = wanted {
                    open(app, &destination);
                }
            }
        } else {
            ui_kit::FileView::detail(ui, tr("cloud.setup_hint"));
        }
        if let Some(error) = &app.cloud.browser.error {
            ui_kit::text(ui, error, ui_kit::Text::Report("cloud_browser_error", true));
        }
    });
    ui.add_space(t.file_section_gap);
}

fn button(ui: &mut egui::Ui, id: &str, label: &str, enabled: bool) -> bool {
    let response = ui.add_enabled_ui(enabled, |ui| ui_kit::button(ui, label, ui_kit::Button::Normal)).inner;
    crate::scenario::publish_target(id, response.rect);
    response.clicked()
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if let Some(operation) = app.cloud.browser.opening.clone() {
        let (closed, action) = ui_kit::Window::new(egui::Id::new("cloud_opening"), "open", tr("cmd.cloud.open"))
            .fields()
            .cards(1.0)
            .actions(tr("ui.open"), "Cancel", None)
            .show(ctx, false, |ui| {
                ui.spinner();
                ui.label(tr("home.opening"));
                if let Some(error) = &app.cloud.browser.error {
                    ui_kit::text(ui, error, ui_kit::Text::Report("cloud_open_error", true));
                }
            });
        if closed || action == ui_kit::Action::Cancel {
            let _ = app.run("cloud.cancel", json!({"operation":operation}));
            app.cloud.browser.opening = None;
        }
    }
    let Some(mut picker) = app.cloud.browser.picker.take() else { return };
    if picker.tab != app.docs.active || !Arc::ptr_eq(&picker.owner, &app.session.doc) {
        app.set_status(app.tr("cloud.design_changed"), true);
        return;
    }
    let connections = app.session.cloud_settings().connections.clone();
    let (_, exists, valid) = destination(app, &picker);
    let (closed, action) = ui_kit::Window::new(egui::Id::new("cloud_destination"), "save", tr("cmd.cloud.save"))
        .fields()
        .cards(2.0)
        .actions(tr("ui.save"), "Cancel", None)
        .show(ctx, valid, |ui| {
            ui_kit::row(ui, tr("cloud.repository"), |ui| {
                egui::ComboBox::from_id_salt("cloud_save_repo")
                    .selected_text(connections.iter().find(|c| c.id == picker.connection).map(|c| c.name.as_str()).unwrap_or(""))
                    .show_ui(ui, |ui| {
                        for c in &connections {
                            ui.selectable_value(&mut picker.connection, c.id.clone(), &c.name);
                        }
                    });
            });
            ui_kit::row(ui, tr("ui.project"), |ui| {
                let response = ui.add(egui::TextEdit::singleline(&mut picker.project).char_limit(512));
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), tr("ui.project")));
                crate::scenario::publish_target("cloud:project", response.rect);
            });
            let projects: std::collections::BTreeSet<_> =
                app.cloud.browser.catalogs.get(&picker.connection).into_iter().flat_map(|c| c.entries.iter()).map(|d| &d.project).collect();
            if !projects.is_empty() {
                egui::ComboBox::from_id_salt("cloud_save_project").selected_text(tr("cloud.existing_project")).show_ui(ui, |ui| {
                    for project in projects {
                        ui.selectable_value(&mut picker.project, project.clone(), project);
                    }
                });
            }
            ui_kit::row(ui, tr("cloud.design_name"), |ui| {
                let response = ui.add(egui::TextEdit::singleline(&mut picker.name).char_limit(128));
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), tr("cloud.design_name")));
                crate::scenario::publish_target("cloud:name", response.rect);
            });
            ui_kit::text(ui, tr("cloud.destination_hint"), ui_kit::Text::Hint("cloud_destination_hint"));
            if exists {
                ui_kit::text(ui, tr("cloud.name_used"), ui_kit::Text::Report("cloud_name_used", true));
            }
            if let Some(error) = &app.cloud.browser.error {
                ui_kit::text(ui, error, ui_kit::Text::Report("cloud_save_error", true));
            }
            ui.hyperlink_to(crate::i18n::tr_label("Help"), "https://tangency-labs.github.io/solidcraft/guide/cloud-saves.html");
        });
    if closed || action == ui_kit::Action::Cancel {
        return;
    }
    // Fields can change in the same frame as Enter; never save the previous frame's path.
    let (destination, _, valid) = destination(app, &picker);
    if action == ui_kit::Action::Ok && valid {
        if let Err(error) = app.run("cloud.save", json!({"destination":destination})) {
            app.cloud.browser.error = Some(error);
            app.cloud.browser.picker = Some(picker);
        } else {
            app.home.open = false;
            app.cloud.browser.error = None;
        }
    } else {
        app.cloud.browser.picker = Some(picker);
    }
}

fn destination(app: &SolveApp, picker: &Picker) -> (Destination, bool, bool) {
    let destination =
        Destination { connection: picker.connection.clone(), project: picker.project.clone(), file: format!("parts/{}", picker.name.trim()) };
    let exists = app.cloud.browser.catalogs.get(&picker.connection).is_some_and(|c| c.entries.contains(&destination))
        && app.session.cloud_binding().is_none_or(|b| b.destination != destination);
    let valid = destination.validate().is_ok()
        && !picker.name.trim().is_empty()
        && !exists
        && !app.session.cloud_configuring()
        && !app.session.cloud_save_pending()
        && app.session.cloud_settings().connection(&picker.connection).is_some();
    (destination, exists, valid)
}

#[cfg(test)]
mod tests;
