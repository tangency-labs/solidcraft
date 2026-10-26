//! Thin management windows. Repository workers own history, durable changes and conflicts.
use super::*;
use solidcraft_cloud::management::{Change, History, Revision};

#[derive(Default)]
pub(super) struct State {
    panel: Option<Panel>,
    trash: Option<Trash>,
}
struct Panel {
    destination: Destination,
    name: String,
    revision: Option<String>,
    revisions: Vec<Revision>,
    selected: Option<usize>,
    loading: Option<String>,
    loaded: bool,
    pending: Option<(String, Change)>,
    queued: bool,
    queued_revision: Option<String>,
    discarding: Option<String>,
    deleting: bool,
    deleted: bool,
    error: Option<String>,
}
struct Trash {
    connection: String,
    entries: Vec<Destination>,
    loading: Option<String>,
    error: Option<String>,
}

fn operation(result: serde_json::Value) -> Result<String, String> {
    result.get("operation").and_then(serde_json::Value::as_str).map(str::to_owned).ok_or_else(|| "missing cloud operation".into())
}

pub(crate) fn open(app: &mut SolveApp, destination: Destination, deleted: bool) {
    if app.session.cloud_configuring() {
        return;
    }
    let mut panel = Panel {
        name: destination.file.rsplit('/').next().unwrap_or(&destination.file).into(),
        destination,
        revision: None,
        revisions: Vec::new(),
        selected: None,
        loading: None,
        loaded: false,
        pending: None,
        queued: false,
        queued_revision: None,
        discarding: None,
        deleting: false,
        deleted,
        error: None,
    };
    load(app, &mut panel);
    app.cloud.browser.management.trash = None;
    app.cloud.browser.management.panel = Some(panel);
}

pub(crate) fn start(app: &mut SolveApp, command: &str) {
    if command == "cloud.trash" {
        if let Some(c) = super::selected(app) {
            trash(app, &c.id);
        }
        return;
    }
    let selected = || {
        let connection = super::selected(app)?;
        app.cloud.browser.catalogs.get(&connection.id)?.entries.first().cloned()
    };
    let destination = if app.home.open { selected() } else { app.session.cloud_binding().map(|b| b.destination.clone()).or_else(selected) };
    if let Some(destination) = destination {
        open(app, destination, false);
    }
}

fn load(app: &mut SolveApp, panel: &mut Panel) {
    panel.loaded = false;
    panel.queued = false;
    panel.error = None;
    match app.run("cloud.history", json!({"destination":panel.destination})).and_then(operation) {
        Ok(id) => panel.loading = Some(id),
        Err(error) => panel.error = Some(error),
    }
}

pub(super) fn trash(app: &mut SolveApp, connection: &str) {
    let mut view = Trash { connection: connection.into(), entries: Vec::new(), loading: None, error: None };
    match app.run("cloud.trash", json!({"connection":connection})).and_then(operation) {
        Ok(id) => view.loading = Some(id),
        Err(error) => view.error = Some(error),
    }
    app.cloud.browser.management.trash = Some(view);
}

pub(super) fn tick(app: &mut SolveApp, statuses: &[JobStatus]) {
    if let Some(mut panel) = app.cloud.browser.management.panel.take() {
        if let Some(id) = &panel.loading
            && let Some(status) = statuses.iter().find(|s| &s.operation == id)
        {
            match status.phase {
                Phase::Ready => {
                    let parsed = status
                        .result
                        .clone()
                        .ok_or_else(|| "missing history".to_owned())
                        .and_then(|value| serde_json::from_value::<History>(value).map_err(|e| e.to_string()));
                    match parsed.and_then(|h| {
                        if h.revisions.len() > 50 || h.revision.as_ref().is_some_and(|r| r.len() > 128 || r.chars().any(char::is_control)) {
                            return Err("invalid cloud history".into());
                        }
                        for revision in &h.revisions {
                            revision.validate(&panel.destination).map_err(|e| e.to_string())?;
                        }
                        Ok(h)
                    }) {
                        Ok(history) => {
                            panel.queued = history.queued;
                            panel.queued_revision = history.queued.then(|| history.revision.clone()).flatten();
                            panel.deleted = history.deleted;
                            panel.revision = history.revision;
                            panel.revisions = history.revisions;
                            panel.loaded = true;
                            panel.error = status.message.clone();
                        }
                        Err(error) => panel.error = Some(error),
                    }
                    panel.loading = None;
                }
                Phase::Failed | Phase::Cancelled => {
                    panel.loading = None;
                    panel.error = status.message.clone().or_else(|| Some(tr("cloud.needs_attention").into()));
                }
                _ => {}
            }
        }
        let mut keep = true;
        if let Some((id, change)) = &panel.pending
            && let Some(status) = statuses.iter().find(|s| &s.operation == id)
        {
            match status.phase {
                Phase::Synced => {
                    let destination = change.destination(&panel.destination);
                    if let Some(connection) = app.session.cloud_settings().connection(&destination.connection).cloned() {
                        super::request(app, &connection);
                    }
                    let change = change.clone();
                    panel.pending = None;
                    match change {
                        Change::Rename { .. } => {
                            panel.destination = destination;
                            load(app, &mut panel);
                        }
                        Change::Delete => {
                            keep = false;
                            trash(app, &destination.connection);
                        }
                        Change::Resolve { .. } => {
                            keep = false;
                        }
                        Change::Restore { .. } => {
                            keep = false;
                            super::open(app, &destination);
                        }
                    }
                }
                Phase::SavedLocally => {
                    panel.queued_revision = status.revision.clone();
                    panel.pending = None;
                    panel.queued = true;
                    panel.error = status.message.clone();
                }
                Phase::Failed | Phase::Cancelled => {
                    panel.pending = None;
                    panel.error = status.message.clone().or_else(|| Some(tr("cloud.needs_attention").into()));
                }
                _ => {}
            }
        }
        if let Some(id) = &panel.discarding
            && let Some(status) = statuses.iter().find(|s| &s.operation == id)
        {
            match status.phase {
                Phase::Synced => {
                    panel.discarding = None;
                    panel.queued_revision = None;
                    load(app, &mut panel);
                }
                Phase::Failed | Phase::Cancelled => {
                    panel.discarding = None;
                    panel.error = status.message.clone();
                }
                _ => {}
            }
        }
        if keep {
            app.cloud.browser.management.panel = Some(panel);
        }
    }
    if let Some(view) = app.cloud.browser.management.trash.as_mut()
        && let Some(id) = &view.loading
        && let Some(status) = statuses.iter().find(|s| &s.operation == id)
    {
        match status.phase {
            Phase::Ready => {
                match super::entries(status, &view.connection) {
                    Ok((entries, _)) => view.entries = entries,
                    Err(e) => view.error = Some(e),
                };
                view.loading = None;
            }
            Phase::Failed | Phase::Cancelled => {
                view.loading = None;
                view.error = status.message.clone();
            }
            _ => {}
        }
    }
}

pub(super) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    show_trash(app, ctx);
    let Some(mut panel) = app.cloud.browser.management.panel.take() else { return };
    if app.session.cloud_settings().connection(&panel.destination.connection).is_none() {
        return;
    }
    let busy = panel.loading.is_some() || panel.pending.is_some() || panel.discarding.is_some();
    let enabled = panel.loaded && !busy && !panel.queued && !app.session.cloud_configuring();
    let mut change = None;
    let mut refresh = false;
    let mut cancel = false;
    let mut choose = None;
    let mut discard = false;
    let (closed, action) = ui_kit::Window::new(egui::Id::new("cloud_manage"), "settings", tr("cloud.manage"))
        .fields()
        .cards(2.5)
        .actions("", tr("ui.close"), None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &format!("{} / {}", panel.destination.project, panel.destination.file), ui_kit::Text::Hint("cloud_manage_destination"));
            if !panel.deleted
                && let Some(catalog) = app.cloud.browser.catalogs.get(&panel.destination.connection)
            {
                ui.add_enabled_ui(!busy, |ui| {
                    ui_kit::dropdown_menu(ui, "cloud_manage_design", &panel.destination.file, |ui| {
                        for destination in catalog.entries.iter() {
                            if ui_kit::list_picker(
                                ui,
                                &destination.design_path().unwrap_or_default(),
                                &format!("{} / {}", destination.project, destination.file),
                                destination == &panel.destination,
                            )
                            .clicked()
                            {
                                choose = Some(destination.clone());
                            }
                        }
                    })
                });
            }
            if !panel.deleted {
                ui.label(tr("cloud.design_name"));
                let response = ui
                    .add_enabled_ui(enabled, |ui| {
                        ui_kit::bounded_cell_editor(ui, egui::Id::new("cloud_rename_name"), &mut panel.name, "", false, 128)
                    })
                    .inner;
                crate::scenario::publish_target("cloud:manage:name", response.rect);
                ui_kit::FileView::control_row(ui, |ui| {
                    let file = panel
                        .destination
                        .file
                        .rsplit_once('/')
                        .map(|(folder, _)| format!("{folder}/{}", panel.name.trim()))
                        .unwrap_or_else(|| panel.name.trim().to_owned());
                    let rename = Change::Rename { file };
                    if super::busy_button(
                        ui,
                        "cloud:manage:rename",
                        tr("cmd.cloud.rename"),
                        enabled && rename.validate(&panel.destination).is_ok(),
                        panel.pending.as_ref().is_some_and(|(_, c)| matches!(c, Change::Rename { .. })),
                    ) {
                        change = Some(rename);
                    }
                    if super::busy_button(
                        ui,
                        "cloud:manage:delete",
                        tr("cmd.cloud.delete"),
                        enabled,
                        panel.pending.as_ref().is_some_and(|(_, c)| matches!(c, Change::Delete)),
                    ) {
                        panel.deleting = true;
                    }
                });
                if panel.deleting {
                    ui_kit::text(ui, tr("cloud.delete_confirm"), ui_kit::Text::Hint("cloud_delete_confirm"));
                    ui_kit::FileView::control_row(ui, |ui| {
                        if super::button(ui, "cloud:manage:delete:confirm", tr("cmd.cloud.delete"), enabled) {
                            change = Some(Change::Delete);
                            panel.deleting = false;
                        }
                        if super::button(ui, "cloud:manage:delete:cancel", crate::i18n::tr_label("Cancel"), !busy) {
                            panel.deleting = false;
                        }
                    });
                }
            }
            ui.add_space(crate::theme::Tokens::get().space_12);
            ui_kit::text(ui, tr("cmd.cloud.history"), ui_kit::Text::Heading("cloud_history_heading"));
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                for (index, revision) in panel.revisions.iter().enumerate() {
                    let label = format!("{} · {} · {}", revision.date, revision.commit.get(..8).unwrap_or(&revision.commit), revision.message);
                    let response =
                        ui.add_enabled_ui(enabled, |ui| ui_kit::list_picker(ui, &revision.commit, &label, panel.selected == Some(index))).inner;
                    crate::scenario::publish_target(&format!("cloud:manage:revision:{index}"), response.rect);
                    if response.clicked() {
                        panel.selected = Some(index);
                    }
                }
            });
            ui_kit::FileView::control_row(ui, |ui| {
                let selected = panel.selected.and_then(|i| panel.revisions.get(i));
                if super::busy_button(
                    ui,
                    "cloud:manage:restore",
                    tr("cmd.cloud.restore"),
                    enabled && selected.is_some(),
                    panel.pending.as_ref().is_some_and(|(_, c)| matches!(c, Change::Restore { .. })),
                ) && let Some(revision) = selected
                {
                    change = Some(Change::Restore { commit: revision.commit.clone(), file: revision.file.clone() });
                }
                if super::busy_button(ui, "cloud:manage:refresh", tr("ui.agent_chat_refresh"), !busy, panel.loading.is_some()) {
                    refresh = true;
                }
                if panel.pending.is_some() && super::button(ui, "cloud:manage:cancel", crate::i18n::tr_label("Cancel"), true) {
                    cancel = true;
                }
            });
            if panel.queued {
                ui.label(tr("cloud.status_pushing"));
                if super::busy_button(
                    ui,
                    "cloud:manage:discard",
                    tr("cmd.cloud.discard"),
                    panel.queued_revision.is_some() && !busy,
                    panel.discarding.is_some(),
                ) {
                    discard = true;
                }
            }
            if let Some(error) = &panel.error {
                ui_kit::text(ui, error, ui_kit::Text::Report("cloud_manage_error", true));
            }
            ui.hyperlink_to(crate::i18n::tr_label("Help"), "https://tangency-labs.github.io/solidcraft/guide/cloud-saves.html");
        });
    if closed || action == ui_kit::Action::Cancel {
        if let Some(id) = panel.loading {
            let _ = app.run("cloud.cancel", json!({"operation":id}));
        }
        return;
    }
    if let Some(destination) = choose {
        open(app, destination, false);
        return;
    }
    if refresh {
        load(app, &mut panel);
    }
    if discard && let Some(revision) = &panel.queued_revision {
        match app.run("cloud.discard", json!({"destination":panel.destination,"revision":revision})).and_then(operation) {
            Ok(id) => panel.discarding = Some(id),
            Err(error) => panel.error = Some(error),
        }
    }
    if cancel && let Some((id, _)) = &panel.pending {
        let _ = app.run("cloud.cancel", json!({"operation":id}));
    }
    if let Some(change) = change {
        let (command, mut params) = match &change {
            Change::Rename { file } => ("cloud.rename", json!({"file":file})),
            Change::Delete => ("cloud.delete", json!({})),
            Change::Restore { commit, file } => ("cloud.restore", json!({"commit":commit,"file":file})),
            Change::Resolve { revision, keep, .. } => ("cloud.resolve", json!({"remote_revision":revision,"keep":keep})),
        };
        params["destination"] = json!(panel.destination);
        params["revision"] = json!(panel.revision);
        match app.run(command, params).and_then(operation) {
            Ok(id) => panel.pending = Some((id, change)),
            Err(error) => panel.error = Some(error),
        }
    }
    app.cloud.browser.management.panel = Some(panel);
}

fn show_trash(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(view) = app.cloud.browser.management.trash.take() else { return };
    let mut wanted = None;
    let (closed, action) = ui_kit::Window::new(egui::Id::new("cloud_trash"), "delete", tr("cmd.cloud.trash"))
        .fields()
        .cards(2.0)
        .actions("", tr("ui.close"), None)
        .show(ctx, true, |ui| {
            if view.loading.is_some() {
                ui.spinner();
            }
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for (index, destination) in view.entries.iter().enumerate() {
                    if super::button(
                        ui,
                        &format!("cloud:trash:{index}"),
                        &format!("{} / {}", destination.project, destination.file),
                        view.loading.is_none(),
                    ) {
                        wanted = Some(destination.clone());
                    }
                }
            });
            if let Some(error) = &view.error {
                ui_kit::text(ui, error, ui_kit::Text::Report("cloud_trash_error", true));
            }
        });
    if let Some(destination) = wanted {
        open(app, destination, true);
    } else if !closed && action != ui_kit::Action::Cancel {
        app.cloud.browser.management.trash = Some(view);
    }
}
