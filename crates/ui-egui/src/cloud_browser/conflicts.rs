//! User-initiated conflict choices. The save queue owns durability and both-head checks.
use super::*;
use solidcraft_cloud::{conflicts::Keep, management::Change};

#[derive(Default)]
pub(super) struct State {
    panel: Option<Panel>,
}

struct Panel {
    copy: Destination,
    original: Destination,
    local_revision: Option<String>,
    remote_revision: Option<String>,
    remote_available: bool,
    name: String,
    pending: Option<String>,
    queued: bool,
    checking: Option<String>,
    editor: Option<Arc<Document>>,
    error: Option<String>,
}

fn observed(app: &SolveApp, destination: &Destination) -> Option<(Option<String>, bool)> {
    app.cloud
        .browser
        .statuses
        .iter()
        .chain(app.cloud.browser.catalogs.get(&destination.connection).into_iter().flat_map(|c| &c.stored))
        .filter(|s| &s.destination == destination)
        .max_by_key(|s| s.operation)
        .map(|s| (s.revision.clone(), !s.outdated && s.local_revisions.len() <= 1 && matches!(s.phase, Phase::Ready | Phase::Synced)))
}

pub(super) fn open(app: &mut SolveApp, copy: Destination) {
    if app.cloud.browser.conflicts.panel.is_some() {
        return;
    }
    let Some(original) = solidcraft_cloud::conflicts::original(&copy).ok().flatten() else { return };
    // Reuse the already loaded catalog; only a missing original needs a lookup. The
    // publication worker compares both observed revisions before changing either file.
    let local = observed(app, &copy).and_then(|(revision, _)| revision);
    let remote = observed(app, &original);
    let (remote_revision, remote_available) = remote.clone().unwrap_or_default();
    let name = format!("{}-copy", original.file);
    let editor = (!app.home.open && app.session.cloud_binding().is_some_and(|b| b.destination == copy)).then(|| app.session.doc.clone());
    let mut panel = Panel {
        copy,
        original,
        local_revision: local,
        remote_revision,
        remote_available,
        name,
        pending: None,
        queued: false,
        checking: None,
        editor,
        error: None,
    };
    if remote.is_none() {
        check_original(app, &mut panel);
    }
    app.cloud.browser.conflicts.panel = Some(panel);
}

// A removed original is absent from the live catalog. Inspect its tombstone only
// when needed; this lets Keep local recover a remotely deleted design as well.
fn check_original(app: &mut SolveApp, panel: &mut Panel) {
    match app.run("cloud.history", json!({"destination":panel.original})) {
        Ok(result) => panel.checking = result.get("operation").and_then(|v| v.as_str()).map(str::to_owned),
        Err(error) => panel.error = Some(error),
    }
}

pub(super) fn tick(app: &mut SolveApp, statuses: &[JobStatus]) {
    let Some(mut panel) = app.cloud.browser.conflicts.panel.take() else { return };
    if let Some(id) = &panel.checking
        && let Some(status) = statuses.iter().find(|s| &s.operation == id)
    {
        match status.phase {
            Phase::Ready => {
                let history = status.result.clone().and_then(|value| serde_json::from_value::<solidcraft_cloud::management::History>(value).ok());
                if let Some(history) = history.filter(|h| {
                    h.revisions.len() <= 50
                        && h.revision.as_ref().is_some_and(|r| !r.is_empty() && r.len() <= 128 && !r.chars().any(char::is_control))
                }) {
                    panel.remote_revision = history.revision;
                    panel.remote_available = !history.queued;
                } else {
                    panel.error = Some(tr("cloud.conflict_sync_first").into());
                }
                panel.checking = None;
            }
            Phase::Failed | Phase::Cancelled => {
                panel.error = status.message.clone().or_else(|| Some(tr("cloud.conflict_sync_first").into()));
                panel.checking = None;
            }
            _ => {}
        }
    }
    if let Some(managed) = statuses.iter().flat_map(|s| &s.managed).find(|m| m.source == panel.copy && m.previous == panel.local_revision) {
        if let Some(connection) = app.session.cloud_settings().connection(&panel.copy.connection).cloned() {
            super::request(app, &connection);
        }
        if matches!(managed.change, Change::Resolve { keep: Keep::Remote, .. })
            && !app.home.open
            && panel.editor.as_ref().is_some_and(|doc| Arc::ptr_eq(doc, &app.session.doc))
        {
            // The detached local tab keeps any edits; open the chosen repository version
            // through the normal loader in its own tab.
            super::open(app, &panel.original);
        }
        return;
    }
    if let Some(id) = &panel.pending
        && let Some(status) = statuses.iter().find(|s| &s.operation == id)
    {
        match status.phase {
            Phase::Synced => {
                if let Some(connection) = app.session.cloud_settings().connection(&panel.copy.connection).cloned() {
                    super::request(app, &connection);
                }
                return;
            }
            Phase::Failed | Phase::Cancelled => {
                panel.pending = None;
                panel.error = status.message.clone().or_else(|| Some(tr("cloud.needs_attention").into()));
            }
            Phase::SavedLocally => {
                panel.pending = None;
                panel.queued = true;
                panel.error = Some(tr("cloud.conflict_queued").into());
            }
            _ => {}
        }
    } else {
        if !panel.queued {
            panel.local_revision = observed(app, &panel.copy).and_then(|(revision, _)| revision);
        }
        if !panel.queued
            && let Some((revision, available)) = observed(app, &panel.original)
        {
            panel.remote_revision = revision;
            panel.remote_available = available;
        }
    }
    app.cloud.browser.conflicts.panel = Some(panel);
}

pub(super) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut panel) = app.cloud.browser.conflicts.panel.take() else { return };
    if app.session.cloud_settings().connection(&panel.copy.connection).is_none() {
        return;
    }
    let busy = panel.pending.is_some() || panel.checking.is_some();
    let enabled = !busy && !panel.queued && !app.session.cloud_configuring() && !app.file_load.busy() && panel.local_revision.is_some();
    let mut choice = None;
    let mut manage = false;
    let mut synchronize = false;
    let (closed, action) = ui_kit::Window::new(egui::Id::new("cloud_conflict"), "settings", tr("cmd.cloud.resolve"))
        .fields()
        .cards(2.5)
        .actions("", tr("ui.close"), None)
        .show(ctx, true, |ui| {
            ui.label(format!("{} / {}", panel.original.project, panel.original.file));
            ui_kit::text(ui, tr("cloud.conflict_hint"), ui_kit::Text::Hint("cloud_conflict_hint"));
            ui_kit::FileView::control_row(ui, |ui| {
                if button(ui, "cloud:conflict:local", tr("cloud.conflict_keep_local"), enabled && panel.remote_available) {
                    choice = Some(Change::Resolve { file: panel.original.file.clone(), revision: panel.remote_revision.clone(), keep: Keep::Local });
                }
                if button(ui, "cloud:conflict:remote", tr("cloud.conflict_keep_remote"), enabled && panel.remote_available) {
                    choice = Some(Change::Resolve { file: panel.original.file.clone(), revision: panel.remote_revision.clone(), keep: Keep::Remote });
                }
            });
            ui.add_space(crate::theme::Tokens::get().space_12);
            ui.label(tr("cloud.conflict_rename"));
            let response = ui
                .add_enabled_ui(enabled, |ui| ui_kit::bounded_cell_editor(ui, egui::Id::new("cloud_conflict_name"), &mut panel.name, "", false, 256))
                .inner;
            crate::scenario::publish_target("cloud:conflict:name", response.rect);
            let rename = Change::Rename { file: panel.name.trim().into() };
            if button(
                ui,
                "cloud:conflict:both",
                tr("cloud.conflict_keep_both"),
                enabled && rename.validate(&panel.copy).is_ok() && !panel.name.trim().starts_with("conflicts/"),
            ) {
                choice = Some(rename);
            }
            ui.add_space(crate::theme::Tokens::get().space_12);
            ui_kit::FileView::control_row(ui, |ui| {
                synchronize = busy_button(ui, "cloud:sync", tr("cmd.cloud.sync"), !busy, synchronizing(app, &panel.copy.connection));
                manage = button(ui, "cloud:conflict:manage", tr("cloud.manage"), !busy);
            });
            if busy {
                ui_kit::text(ui, tr("cloud.status_pushing"), ui_kit::Text::Hint("cloud_conflict_busy"));
            }
            if let Some(error) = &panel.error {
                ui_kit::text(ui, error, ui_kit::Text::Error("cloud_conflict_error"));
            }
        });
    if closed || action == ui_kit::Action::Cancel {
        return;
    }
    if manage {
        super::management::open(app, panel.copy, false);
        return;
    }
    if synchronize {
        match app.run("cloud.sync", json!({"connection":panel.copy.connection})) {
            Ok(_) => {
                panel.error = None;
                if !panel.remote_available {
                    check_original(app, &mut panel);
                }
            }
            Err(error) => panel.error = Some(error),
        }
    }
    if let Some(change) = choice {
        let (command, mut params) = match &change {
            Change::Rename { file } => ("cloud.rename", json!({"file":file})),
            Change::Resolve { revision, keep, .. } => ("cloud.resolve", json!({"remote_revision":revision,"keep":keep})),
            _ => {
                app.cloud.browser.conflicts.panel = Some(panel);
                return;
            }
        };
        params["destination"] = json!(panel.copy);
        params["revision"] = json!(panel.local_revision);
        match app.run(command, params) {
            Ok(result) => {
                panel.pending = result.get("operation").and_then(|v| v.as_str()).map(str::to_owned);
                panel.error = None;
            }
            Err(error) => panel.error = Some(error),
        }
    }
    app.cloud.browser.conflicts.panel = Some(panel);
}
