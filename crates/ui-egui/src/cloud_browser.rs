//! Start-page repository browsing and the initial cloud destination picker. Commands own
//! IO and durability; these caches are presentation only and never decide publication.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use serde_json::json;
use solidcraft_cloud::{
    Connection, Destination,
    jobs::{JobStatus, Phase},
};
use solidcraft_engine::doc::Document;

use crate::{SolveApp, i18n::tr, ui_kit};
pub(crate) mod management;
mod previews;
mod status;
pub(crate) mod tabs;
mod versions;

#[derive(Default)]
pub(crate) struct Browser {
    selected: Option<String>,
    catalogs: BTreeMap<String, Catalog>,
    picker: Option<Picker>,
    error: Option<String>,
    statuses: Vec<SaveStatus>,
    publications: BTreeMap<String, usize>,
    previews: previews::State,
    versions: Option<versions::Choice>,
    local_captures: std::collections::BTreeSet<String>,
    completed_syncs: std::collections::BTreeSet<String>,
    receiving: std::collections::BTreeSet<String>,
    management: management::State,
    pending_since: BTreeMap<tabs::PendingKey, web_time::Instant>,
}

struct SaveStatus {
    destination: Destination,
    operation: u64,
    phase: Phase,
    conflict: bool,
    outdated: bool,
    failed: bool,
    revision: Option<String>,
    local_revisions: Vec<solidcraft_cloud::LocalRevision>,
}

struct Catalog {
    connection: Connection,
    entries: Arc<Vec<Destination>>,
    stored: Vec<SaveStatus>,
    pending: Option<String>,
    local_loaded: bool,
    message: Option<String>,
}

struct Picker {
    owner: Arc<Document>,
    tab: usize,
    connection: String,
    project: String,
    new_project: bool,
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
            app.cloud.browser.error = Some(error);
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
    let (project, new_project) = default_project(&app.cloud.browser, &connection.id);
    app.cloud.browser.picker = Some(Picker {
        owner: app.session.doc.clone(),
        tab: app.docs.active,
        connection: connection.id.clone(),
        project,
        new_project,
        name: app.session.doc.name.clone(),
    });
    app.cloud.browser.selected = Some(connection.id);
    app.cloud.browser.error = None;
}

pub(crate) fn choosing_destination(app: &SolveApp) -> bool {
    app.cloud.browser.picker.is_some()
}

fn project_names(browser: &Browser, connection: &str) -> BTreeSet<String> {
    browser.catalogs.get(connection).into_iter().flat_map(|c| c.entries.iter()).map(|d| d.project.clone()).collect()
}

fn default_project(browser: &Browser, connection: &str) -> (String, bool) {
    let projects = project_names(browser, connection);
    (projects.get("Designs").or_else(|| projects.first()).cloned().unwrap_or_else(|| "Designs".into()), projects.is_empty())
}

pub(crate) fn synchronizing(app: &SolveApp, connection: &str) -> bool {
    app.cloud.browser.catalogs.get(connection).is_some_and(|catalog| catalog.pending.is_some())
        || app.cloud.operations.busy("cloud.sync", connection)
        || app.cloud.operations.busy("cloud.save", connection)
}

fn request(app: &mut SolveApp, connection: &Connection) {
    let result = app.run("cloud.list", json!({"connection":connection.id}));
    match result.and_then(|r| r.get("operation").and_then(|v| v.as_str()).map(str::to_owned).ok_or_else(|| "missing list operation".into())) {
        Ok(operation) => {
            let previous = app.cloud.browser.catalogs.remove(&connection.id);
            if let Some(old) = previous.as_ref().and_then(|c| c.pending.as_ref()) {
                let _ = app.run("cloud.cancel", json!({"operation":old}));
            }
            let (entries, stored, local_loaded) =
                previous.filter(|c| c.connection == *connection).map(|c| (c.entries, c.stored, c.local_loaded)).unwrap_or_default();
            app.cloud.browser.catalogs.insert(
                connection.id.clone(),
                Catalog { connection: connection.clone(), entries, stored, pending: Some(operation), local_loaded, message: None },
            );
        }
        Err(error) => {
            app.cloud.browser.catalogs.entry(connection.id.clone()).or_insert_with(|| Catalog {
                connection: connection.clone(),
                entries: Default::default(),
                stored: Vec::new(),
                pending: None,
                local_loaded: false,
                message: Some(error.clone()),
            });
            app.cloud.browser.error = Some(error);
        }
    }
}

pub(crate) fn select(app: &mut SolveApp, id: &str) {
    if let Some(connection) = app.session.cloud_settings().connection(id).cloned() {
        app.cloud.browser.selected = Some(id.into());
        request(app, &connection);
    }
}

fn entries(status: &JobStatus, connection: &str) -> Result<(Vec<Destination>, Vec<SaveStatus>), String> {
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
    if status.catalog.len() > 10_000 {
        return Err("stored catalog is too large".into());
    }
    let listed: std::collections::BTreeSet<_> = entries.iter().map(|d| (&d.connection, &d.project, &d.file)).collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut stored = Vec::new();
    for entry in &status.catalog {
        entry.validate(connection).map_err(|e| e.to_string())?;
        let d = &entry.destination;
        let key = (&d.connection, &d.project, &d.file);
        if !listed.contains(&key) || !seen.insert(key) {
            return Err("stored status has no listed design".into());
        }
        stored.push(SaveStatus {
            destination: entry.destination.clone(),
            operation: status.operation.strip_prefix("cloud-").and_then(|id| id.parse().ok()).unwrap_or(0),
            phase: entry.phase,
            conflict: entry.conflict,
            outdated: entry.outdated,
            failed: entry.phase == Phase::Failed,
            revision: entry.revision.clone(),
            local_revisions: entry.local_revisions.clone(),
        });
    }
    Ok((entries, stored))
}

/// Poll completion in every tab, including when Start is hidden. Downloaded designs are
/// completed in a fresh editor so changing tabs or editing during IO cannot lose work.
pub(crate) fn tick(app: &mut SolveApp, statuses: &[JobStatus]) {
    poll(app, statuses);
    management::tick(app, statuses);
}

fn poll(app: &mut SolveApp, statuses: &[JobStatus]) {
    app.cloud.browser.publications.retain(|id, _| statuses.iter().any(|s| &s.operation == id));
    let mut refresh = std::collections::BTreeSet::new();
    app.cloud.browser.receiving =
        statuses.iter().filter(|s| s.phase == Phase::Syncing && !s.saved_locally && s.destination.is_none()).map(|s| s.connection.clone()).collect();
    app.cloud.browser.completed_syncs.retain(|id| statuses.iter().any(|s| &s.operation == id));
    for status in statuses.iter().filter(|s| s.phase == Phase::Synced && !s.saved_locally && s.destination.is_none() && !s.connection.is_empty()) {
        if app.cloud.browser.completed_syncs.insert(status.operation.clone()) {
            refresh.insert(status.connection.clone());
        }
    }
    app.cloud.browser.local_captures.retain(|id| statuses.iter().any(|s| &s.operation == id));
    let mut new_captures = std::collections::BTreeSet::new();
    for status in statuses.iter().filter(|s| s.saved_locally) {
        if app.cloud.browser.local_captures.insert(status.operation.clone()) {
            refresh.insert(status.connection.clone());
            if !matches!(status.change, Some(solidcraft_cloud::management::Change::Delete)) {
                new_captures.insert(status.operation.clone());
            }
        }
    }
    for status in statuses.iter().filter(|s| !s.publications.is_empty()) {
        let seen = app.cloud.browser.publications.entry(status.operation.clone()).or_default();
        if *seen < status.publications.len() {
            refresh.insert(status.connection.clone());
            *seen = status.publications.len();
        }
    }
    app.cloud.browser.statuses = statuses
        .iter()
        .filter(|s| s.saved_locally || (s.phase == Phase::Failed && s.destination.is_some()))
        .filter_map(|s| {
            Some(SaveStatus {
                destination: s.destination.clone()?,
                operation: s.operation.strip_prefix("cloud-").and_then(|id| id.parse().ok()).unwrap_or(0),
                phase: s.phase,
                conflict: s.publications.iter().any(|p| p.conflict && Some(&p.destination) == s.destination.as_ref()),
                outdated: false,
                failed: s.phase == Phase::Failed || (s.phase == Phase::SavedLocally && s.message.is_some()),
                revision: s.revision.clone(),
                local_revisions: Vec::new(),
            })
        })
        .collect();
    let attention = app.tr("cloud.needs_attention").to_owned();
    let settings = app.session.cloud_settings();
    let mut refreshed = Vec::new();
    app.cloud.browser.catalogs.retain(|id, c| settings.connection(id) == Some(&c.connection));
    for (id, catalog) in &mut app.cloud.browser.catalogs {
        if let Some(operation) = catalog.pending.as_ref()
            && let Some(status) = statuses.iter().find(|s| s.operation == *operation)
        {
            match status.phase {
                Phase::Ready | Phase::Working if status.phase == Phase::Ready || (!catalog.local_loaded && status.result.is_some()) => {
                    match entries(status, id) {
                        Ok((entries, stored)) => {
                            catalog.entries = Arc::new(entries);
                            catalog.stored = stored;
                            catalog.local_loaded = true;
                            refreshed.push(id.clone());
                            catalog.message = status.message.clone();
                        }
                        Err(error) => catalog.message = Some(error),
                    }
                    if status.phase == Phase::Ready {
                        catalog.pending = None;
                    }
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
        for status in statuses.iter().filter(|s| s.connection == *id && new_captures.contains(&s.operation)) {
            if let Some(destination) = &status.destination
                && !catalog.entries.contains(destination)
            {
                Arc::make_mut(&mut catalog.entries).push(destination.clone());
            }
        }
    }
    for id in refreshed {
        previews::clear(app, &id);
    }
    previews::poll(app, statuses);
    let pending: std::collections::BTreeSet<_> = app
        .cloud
        .browser
        .statuses
        .iter()
        .chain(app.cloud.browser.catalogs.values().flat_map(|c| &c.stored))
        .filter(|s| matches!(s.phase, Phase::SavedLocally | Phase::Syncing))
        .map(|s| tabs::key(&s.destination, s.revision.as_deref()))
        .collect();
    app.cloud.browser.pending_since.retain(|key, _| pending.contains(key));
    for key in pending {
        app.cloud.browser.pending_since.entry(key).or_insert_with(web_time::Instant::now);
    }
    if !app.session.cloud_configuring() {
        for id in refresh {
            if app.cloud.browser.catalogs.contains_key(&id)
                && let Some(connection) = app.session.cloud_settings().connection(&id).cloned()
            {
                request(app, &connection);
            }
        }
    }
}

pub(crate) fn opening_preview(app: &SolveApp, destination: &Destination) -> Option<egui::TextureHandle> {
    previews::texture(app, destination)
}

pub(crate) fn error(app: &mut SolveApp, error: String) {
    app.cloud.browser.error = Some(error);
}

fn open(app: &mut SolveApp, destination: &Destination) {
    let revisions = app
        .cloud
        .browser
        .catalogs
        .get(&destination.connection)
        .into_iter()
        .flat_map(|c| &c.stored)
        .find(|s| &s.destination == destination)
        .map(|s| s.local_revisions.clone())
        .unwrap_or_default();
    if revisions.len() > 1 {
        app.cloud.browser.versions = Some(versions::Choice::new(destination.clone(), revisions));
        return;
    }
    open_revision(app, destination, None);
}

fn open_revision(app: &mut SolveApp, destination: &Destination, revision: Option<&str>) {
    if app.file_load.busy() {
        return;
    }
    if app.docs.count() >= crate::documents::MAX_OPEN && !crate::documents::active_is_blank(app) {
        app.cloud.browser.error = Some(tr("cloud.tabs_full").into());
        return;
    }
    match app.run("cloud.open", json!({"destination":destination, "local_revision":revision})) {
        Ok(result) => {
            app.home.open = false;
            app.cloud.browser.error = None;
            if let Some(operation) = result.get("operation").and_then(|v| v.as_str()) {
                let request = crate::file_load::Request::new(
                    &destination.file,
                    crate::file_load::Kind::CloudOpen { operation: operation.into(), destination: destination.clone() },
                );
                if let Err(error) = crate::file_load::begin_request(app, request) {
                    let _ = app.run("cloud.cancel", json!({"operation":operation}));
                    app.cloud.browser.error = Some(error);
                }
            }
        }
        Err(error) => app.cloud.browser.error = Some(error),
    }
}

#[cfg(test)]
fn phase(status: &SaveStatus) -> &'static str {
    status::presentation(Some(status), false).label
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
    previews::textures(app, ui.ctx());
    let mut seen = Vec::new();
    let mut connection = selected(app);
    if let Some(c) = &connection
        && !app.cloud.browser.catalogs.contains_key(&c.id)
        && !app.session.cloud_configuring()
    {
        request(app, c);
    }
    let notice = status::notice(app, connection.as_ref());
    let indicator = notice.as_deref().map(|label| ui_kit::FileStatus { label, tone: ui_kit::FileStatusTone::Attention, pulse: false });
    ui_kit::FileView::section_with_status(ui, tr("cloud.title"), t.space_10, indicator, |ui| {
        ui_kit::FileView::control_row(ui, |ui| {
            let mut action = None;
            let label = tr("cloud.add_storage");
            let width = ui.painter().layout_no_wrap(label.into(), egui::FontId::proportional(t.font_label), t.form_ink).size().x
                + t.space_24
                + t.space_12 * 2.0;
            let response = ui
                .add_enabled_ui(app.cloud.setup.is_none(), |ui| {
                    ui_kit::settings_control_width(ui, width, |ui| {
                        ui_kit::dropdown_menu(ui, "cloud_add_storage", label, |ui| {
                            action = crate::cloud_setup::repository_choices(ui);
                        })
                    })
                })
                .inner;
            crate::scenario::publish_target("cloud:connect", response.rect);
            crate::scenario::publish_target("cloud:add_storage", response.rect);
            if let Some(command) = action {
                app.start(command);
            }
            let saving = app.session.cloud_save_pending()
                || app
                    .session
                    .cloud_binding()
                    .map(|b| b.destination.connection.as_str())
                    .or_else(|| connection.as_ref().map(|c| c.id.as_str()))
                    .is_some_and(|id| app.cloud.operations.busy("cloud.save", id));
            if connection.is_some() && busy_button(ui, "cloud:save", tr("cmd.cloud.save"), true, saving) {
                app.start("cloud.save");
            }
            if button(ui, "cloud:settings", tr("cmd.cloud.settings"), true) {
                app.start("cloud.settings");
            }
            if connection.is_some() && button(ui, "cloud:manage", tr("cloud.manage"), !app.session.cloud_configuring()) {
                management::start(app, "cloud.history");
            }
            if connection.is_some() && button(ui, "cloud:trash", tr("cmd.cloud.trash"), !app.session.cloud_configuring()) {
                management::start(app, "cloud.trash");
            }
            ui.hyperlink_to(crate::i18n::tr_label("Help"), "https://tangency-labs.github.io/solidcraft/guide/cloud-saves.html");
        });
        ui.add_space(t.space_12);
        if let Some(c) = &mut connection {
            let connections = app.session.cloud_settings().connections.clone();
            ui_kit::FileView::detail(ui, tr("cloud.repository"));
            ui.add_space(t.space_4);
            ui_kit::FileView::control_row(ui, |ui| {
                let response = ui_kit::settings_control_width(ui, t.form_card_width, |ui| {
                    ui_kit::dropdown_menu(ui, "cloud_repo", &c.repository_label(), |ui| {
                        for candidate in &connections {
                            if ui_kit::list_picker(ui, &candidate.id, &candidate.repository_label(), candidate.id == c.id).clicked() {
                                *c = candidate.clone();
                            }
                        }
                    })
                });
                crate::scenario::publish_target("cloud:repositories", response.rect);
                let synchronizing = synchronizing(app, &c.id);
                if busy_button(ui, "cloud:sync", tr("cmd.cloud.sync"), !app.session.cloud_configuring(), synchronizing)
                    && let Err(error) = app.run("cloud.sync", json!({"connection":c.id}))
                {
                    app.cloud.browser.error = Some(error);
                }
                if solidcraft_cloud::setup::github_website(c).is_ok()
                    && button(ui, "cloud:website", tr("cmd.cloud.website"), !app.session.cloud_configuring())
                {
                    match app.run("cloud.website", json!({"connection":c.id})) {
                        Ok(value) => {
                            if let Some(url) = value.get("url").and_then(serde_json::Value::as_str) {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                            }
                        }
                        Err(error) => app.cloud.browser.error = Some(error),
                    }
                }
            });
            app.cloud.browser.selected = Some(c.id.clone());
            if let Some(catalog) = app.cloud.browser.catalogs.get(&c.id) {
                let pending = catalog.pending.is_some();
                let designs = catalog.entries.clone();
                ui.add_space(t.space_8);
                if designs.is_empty() && !pending {
                    ui_kit::FileView::detail(ui, tr("cloud.empty"));
                }
                let statuses = &app.cloud.browser.statuses;
                let mut wanted = None;
                let mut manage = None;
                ui_kit::FileView::recent_list(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt(("cloud_designs", &c.id)).max_height(t.file_recent_height * 6.0).show_rows(
                        ui,
                        t.file_recent_height,
                        designs.len(),
                        |ui, rows| {
                            for index in rows {
                                let Some(destination) = designs.get(index) else { continue };
                                seen.push(destination.clone());
                                let status =
                                    statuses.iter().chain(&catalog.stored).filter(|s| &s.destination == destination).max_by_key(|s| s.operation);
                                let path = format!("{} / {}", destination.project, destination.file);
                                let state = status::presentation(status, app.cloud.browser.receiving.contains(&c.id));
                                let outdated = status.is_some_and(|s| s.outdated);
                                let response = ui
                                    .add_enabled_ui(!app.file_load.busy() && !outdated, |ui| {
                                        ui_kit::FileView::recent_row_with_status(
                                            ui,
                                            &format!("cloud:design:{index}"),
                                            ui_kit::RecentRow {
                                                title: destination.file.rsplit('/').next().unwrap_or(&destination.file),
                                                path: &path,
                                                when: "",
                                                workspace: tr("home.design_workspace"),
                                                thumbnail: previews::image(app, destination),
                                                thumbnail_failed: previews::failed(app, destination),
                                            },
                                            state,
                                        )
                                    })
                                    .inner;
                                let response = if outdated { response.on_disabled_hover_text(tr("cmd.cloud.sync")) } else { response };
                                crate::scenario::publish_target(&format!("cloud:design:{index}"), response.rect);
                                ui_kit::Menu::context(&response, |ui| {
                                    if crate::context_menu::button(ui, tr("cloud.manage"), "settings").clicked() {
                                        manage = Some(destination.clone());
                                        ui.close();
                                    }
                                });
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
                if let Some(destination) = manage {
                    management::open(app, destination, false);
                }
            }
        } else {
            ui_kit::FileView::detail(ui, tr("cloud.setup_hint"));
        }
    });
    ui.add_space(t.file_section_gap);
    for destination in seen {
        previews::touch(app, destination);
    }
}

fn button(ui: &mut egui::Ui, id: &str, label: &str, enabled: bool) -> bool {
    let response = ui_kit::button(ui, label, ui_kit::Button::Enabled(enabled));
    crate::scenario::publish_target(id, response.rect);
    response.clicked()
}

fn busy_button(ui: &mut egui::Ui, id: &str, label: &str, enabled: bool, busy: bool) -> bool {
    let icon = match id {
        "cloud:manage:refresh" => "redo",
        "cloud:sync" => "sync",
        "cloud:manage:rename" => "settings",
        "cloud:manage:delete" => "delete",
        "cloud:manage:discard" => "stop",
        "cloud:manage:restore" => "undo",
        _ => "save",
    };
    let response = ui_kit::busy_button(ui, label, icon, enabled, busy);
    crate::scenario::publish_target(id, response.rect);
    crate::scenario::publish_count(&format!("{id}:enabled"), f64::from(response.enabled()));
    response.clicked()
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    management::show(app, ctx);
    versions::show(app, ctx);
    let Some(mut picker) = app.cloud.browser.picker.take() else { return };
    if picker.tab != app.docs.active || !Arc::ptr_eq(&picker.owner, &app.session.doc) {
        app.set_status(app.tr("cloud.design_changed"), true);
        return;
    }
    let connections = app.session.cloud_settings().connections.clone();
    let previous_connection = picker.connection.clone();
    let (_, exists, valid) = destination(app, &picker);
    let (closed, action) = ui_kit::Window::new(egui::Id::new("cloud_destination"), "save", tr("cmd.cloud.save"))
        .fields()
        .cards(2.0)
        .actions(tr("ui.save"), "Cancel", None)
        .show(ctx, valid, |ui| {
            ui_kit::row(ui, tr("cloud.repository"), |ui| {
                let response = ui_kit::dropdown_menu(
                    ui,
                    "cloud_save_repo",
                    &connections.iter().find(|c| c.id == picker.connection).map(|c| c.repository_label()).unwrap_or_default(),
                    |ui| {
                        for c in &connections {
                            let response = ui_kit::list_picker(ui, &c.id, &c.repository_label(), picker.connection == c.id);
                            crate::scenario::publish_target(&format!("cloud:save-repository:{}", c.id), response.rect);
                            if response.clicked() && picker.connection != c.id {
                                picker.connection = c.id.clone();
                                (picker.project, picker.new_project) = default_project(&app.cloud.browser, &c.id);
                            }
                        }
                    },
                );
                crate::scenario::publish_target("cloud:save-repository", response.rect);
            });
            let mut focus_project = false;
            ui_kit::row(ui, tr("ui.project"), |ui| {
                let current = if picker.new_project { tr("cloud.project_new") } else { &picker.project }.to_owned();
                let response = ui_kit::dropdown_menu(ui, "cloud_save_project", &current, |ui| {
                    for project in project_names(&app.cloud.browser, &picker.connection) {
                        let selected = !picker.new_project && picker.project == project;
                        let response = ui_kit::list_picker(ui, &format!("cloud:project-option:{project}"), &project, selected);
                        crate::scenario::publish_target(&format!("cloud:project-option:{project}"), response.rect);
                        if response.clicked() {
                            picker.project = project;
                            picker.new_project = false;
                        }
                    }
                    let response = ui_kit::list_picker(ui, "cloud:project-new", tr("cloud.project_new"), picker.new_project);
                    crate::scenario::publish_target("cloud:project-new", response.rect);
                    if response.clicked() && !picker.new_project {
                        picker.project.clear();
                        picker.new_project = true;
                        focus_project = true;
                    }
                });
                crate::scenario::publish_target("cloud:project-choice", response.rect);
            });
            if picker.new_project {
                ui_kit::row(ui, "", |ui| {
                    let response =
                        ui_kit::bounded_cell_editor(ui, egui::Id::new("cloud_save_project_name"), &mut picker.project, tr("ui.project"), false, 512);
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), tr("ui.project")));
                    crate::scenario::publish_target("cloud:project", response.rect);
                    if focus_project {
                        response.request_focus();
                    }
                });
            }
            ui_kit::row(ui, tr("cloud.design_name"), |ui| {
                let response = ui_kit::bounded_cell_editor(ui, egui::Id::new("cloud_save_design_name"), &mut picker.name, "", false, 128);
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
    if previous_connection != picker.connection
        && !app.cloud.browser.catalogs.contains_key(&picker.connection)
        && !app.session.cloud_configuring()
        && let Some(connection) = connections.iter().find(|c| c.id == picker.connection)
    {
        request(app, connection);
    }
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
