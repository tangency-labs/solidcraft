//! Pending preference edits, persisted through the shared cloud command interface.
use crate::{SolveApp, i18n::tr, ui_kit};
use serde_json::json;
use solidcraft_cloud::{
    Connection, Settings,
    jobs::{JobStatus, Phase},
};

mod repositories;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod git;

pub(crate) const HELP: &str = "https://tangency-labs.github.io/solidcraft/guide/cloud-saves.html";

pub(crate) fn connections(app: &SolveApp) -> &[Connection] {
    app.cloud.editor.as_ref().map(|e| e.draft.connections.as_slice()).unwrap_or(&app.session.cloud_settings().connections)
}

pub(crate) struct Editor {
    baseline: Settings,
    draft: Settings,
    selected: Option<String>,
    pending: Option<(String, bool)>,
    error: Option<String>,
    test: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    git: Option<git::Wizard>,
}

pub(crate) fn begin(app: &mut SolveApp) {
    if app.cloud.editor.is_none() {
        let settings = app.session.cloud_settings().clone();
        app.cloud.editor = Some(Editor {
            selected: settings.default_connection.clone().or_else(|| settings.connections.first().map(|c| c.id.clone())),
            baseline: settings.clone(),
            draft: settings,
            pending: None,
            error: None,
            test: None,
            #[cfg(not(target_arch = "wasm32"))]
            git: None,
        });
    }
}

pub(crate) fn busy(app: &SolveApp) -> bool {
    app.cloud.editor.as_ref().is_some_and(|e| e.pending.is_some())
}

pub(crate) fn changed(app: &SolveApp) -> bool {
    app.cloud.editor.as_ref().is_some_and(|e| e.draft != e.baseline)
}

pub(crate) fn cancel(app: &mut SolveApp) {
    if busy(app) {
        return;
    }
    crate::cloud_setup::cancel_preferences(app);
    #[cfg(not(target_arch = "wasm32"))]
    git::cancel(app);
    app.cloud.editor = None;
}

pub(crate) fn defaults(app: &mut SolveApp) {
    begin(app);
    if let Some(editor) = app.cloud.editor.as_mut().filter(|e| e.pending.is_none()) {
        editor.draft.restore_behavior_defaults();
    }
}

pub(crate) fn stage(app: &mut SolveApp, connection: Connection) -> Result<(), String> {
    let editor = app.cloud.editor.as_mut().ok_or_else(|| tr("cloud.settings_changed").to_owned())?;
    editor.stage(connection)
}

impl Editor {
    fn dialog_open(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.git.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    fn stage(&mut self, mut connection: Connection) -> Result<(), String> {
        connection.name = connection.repository_label();
        let editor = self;
        if editor.pending.is_some() {
            return Err(tr("cloud.apply_pending").into());
        }
        let mut proposed = editor.draft.clone();
        let added = proposed.connection(&connection.id).is_none();
        if let Some(existing) = proposed.connections.iter_mut().find(|c| c.id == connection.id) {
            *existing = connection.clone();
        } else {
            proposed.connections.push(connection.clone());
        }
        if added && proposed.default_connection.is_none() {
            proposed.default_connection = Some(connection.id.clone());
        }
        proposed.validate().map_err(|e| e.to_string())?;
        editor.draft = proposed;
        editor.selected = Some(connection.id);
        Ok(())
    }
}

/// True means the ordinary Preferences action can finish immediately.
pub(crate) fn apply(app: &mut SolveApp, close: bool) -> bool {
    let Some(mut editor) = app.cloud.editor.take() else { return true };
    let result = if editor.pending.is_some() || editor.dialog_open() || app.cloud.setup.is_some() || app.session.cloud_configuring() {
        Err(tr("cloud.apply_pending").to_owned())
    } else if editor.draft == editor.baseline {
        app.cloud.editor = Some(editor);
        return true;
    } else if app.session.cloud_settings() != &editor.baseline {
        Err(tr("cloud.settings_changed").to_owned())
    } else {
        editor.draft.validate().map_err(|e| e.to_string()).and_then(|()| {
            let reply = app.run("cloud.configure", json!({"settings":editor.draft}))?;
            if let Some(id) = reply.get("operation").and_then(|v| v.as_str()) {
                editor.pending = Some((id.into(), close));
            } else {
                editor.baseline = editor.draft.clone();
            }
            Ok(())
        })
    };
    let immediate = result.is_ok() && editor.pending.is_none();
    editor.error = result.err();
    app.cloud.editor = Some(editor);
    immediate
}

fn statuses(app: &mut SolveApp) -> Vec<JobStatus> {
    let mut all = app.session.cloud_operations().unwrap_or_default();
    for tab in app.docs.background_mut() {
        all.extend(tab.session.cloud_operations().unwrap_or_default());
    }
    all
}

/// Apply completes only after the worker confirms durable connection storage.
pub(crate) fn advance(app: &mut SolveApp) -> Option<bool> {
    let all = statuses(app);
    let live = app.session.cloud_settings().clone();
    let editor = app.cloud.editor.as_mut()?;
    if let Some((id, close)) = &editor.pending {
        let Some(status) = all.iter().find(|s| s.operation == *id) else {
            editor.pending = None;
            editor.error = Some(tr("cloud.settings_changed").into());
            return None;
        };
        match status.phase {
            Phase::Ready | Phase::Synced => {
                let close = *close;
                editor.pending = None;
                editor.baseline = live.clone();
                editor.draft = live;
                editor.error = None;
                return Some(close);
            }
            Phase::Failed | Phase::Cancelled => {
                editor.pending = None;
                editor.error = Some(status.message.clone().unwrap_or_else(|| tr("cloud.setup_failed").into()));
            }
            _ => {}
        }
    } else if !editor.dialog_open() && editor.draft == editor.baseline && editor.baseline != live {
        editor.baseline = live.clone();
        editor.draft = live;
    }
    None
}

fn button(ui: &mut egui::Ui, id: &str, label: &str, enabled: bool) -> bool {
    let r = ui.add_enabled_ui(enabled, |ui| ui_kit::button(ui, label, ui_kit::Button::Normal)).inner;
    crate::scenario::publish_target(id, r.rect);
    r.clicked()
}

pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    begin(app);
    let Some(mut editor) = app.cloud.editor.take() else { return };
    let all = statuses(app);
    let enabled = editor.pending.is_none() && !editor.dialog_open() && !app.session.cloud_configuring() && app.cloud.setup.is_none();
    ui_kit::settings_heading(ui, tr("cloud.title"), tr("cloud.settings_hint"));
    let mut reconnect = None;
    let mut connect = None;
    #[cfg(not(target_arch = "wasm32"))]
    let mut edit = None;
    ui.add_enabled_ui(enabled && app.services.cloud_host.is_some(), |ui| {
        ui_kit::settings_section(ui, "cloud_repositories", tr("cloud.connections"), |ui| {
            if editor.draft.connection(editor.selected.as_deref().unwrap_or("")).is_none() {
                editor.selected = editor.draft.connections.first().map(|c| c.id.clone());
            }
            connect = repositories::table(ui, &mut editor, &all);
            if let Some(c) = editor.selected.as_deref().and_then(|id| editor.draft.connections.iter_mut().find(|c| c.id == id)) {
                let applied = app.session.cloud_settings().connection(&c.id) == Some(c);
                let active = all.iter().any(|s| s.connection == c.id && matches!(s.phase, Phase::Working | Phase::Syncing));
                ui.horizontal_wrapped(|ui| {
                    if busy_button(
                        ui,
                        "cloud_settings:test",
                        tr("cmd.cloud.test"),
                        applied && !active,
                        app.cloud.operations.busy("cloud.test", &c.id),
                    ) {
                        match app.run("cloud.test", json!({"connection":c.id})) {
                            Ok(reply) => {
                                editor.test = reply.get("operation").and_then(|v| v.as_str()).map(str::to_owned);
                                editor.error = None;
                            }
                            Err(error) => editor.error = Some(error),
                        }
                    }
                    if busy_button(
                        ui,
                        "cloud_settings:sync",
                        tr("cmd.cloud.sync"),
                        applied && !active,
                        crate::cloud_browser::synchronizing(app, &c.id),
                    ) && let Err(error) = app.run("cloud.sync", json!({"connection":c.id}))
                    {
                        editor.error = Some(error);
                    }
                    if c.provider == "github" && button(ui, "cloud_settings:reconnect", tr("cloud.reconnect"), !active) {
                        reconnect = Some(c.clone());
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if c.provider != "github" && button(ui, "cloud_settings:advanced", tr("cloud.git_edit"), !active) {
                        edit = Some(c.clone());
                    }
                });
                if !applied {
                    ui_kit::text(ui, tr("cloud.apply_first"), ui_kit::Text::Hint("cloud_apply_first"));
                }
                repositories::status(app, ui, c, &all, editor.test.as_deref(), &mut editor.error);
                if all.iter().any(|s| s.connection == c.id && s.storage_persistent == Some(false)) {
                    ui_kit::text(ui, tr("cloud.storage_temporary"), ui_kit::Text::Report("cloud_storage_persistence", true));
                }
            }
        });
        ui_kit::settings_section(ui, "cloud_sync", tr("cmd.cloud.sync"), |ui| {
            let r = ui_kit::checkbox(ui, tr("cloud.sync_after_save"), &mut editor.draft.sync_after_save);
            crate::scenario::publish_target("cloud_settings:sync_after_save", r.rect);
            let r = ui_kit::checkbox(ui, tr("cloud.retry_pending"), &mut editor.draft.retry_pending);
            crate::scenario::publish_target("cloud_settings:retry_pending", r.rect);
        });
    });
    if editor.pending.is_some() {
        ui.spinner();
        ui.label(tr("cloud.apply_pending"));
    }
    if let Some(error) = &editor.error {
        ui_kit::text(ui, error, ui_kit::Text::Report("cloud_settings_error", true));
        if !busy(app) && button(ui, "cloud_settings:reload", tr("ui.agent_chat_refresh"), enabled) {
            editor.draft = app.session.cloud_settings().clone();
            editor.baseline = editor.draft.clone();
            editor.error = None;
        }
    }
    ui.hyperlink_to(crate::i18n::tr_label("Help"), HELP);
    app.cloud.editor = Some(editor);
    if let Some(command) = connect {
        app.start(command);
    }
    if let Some(connection) = reconnect {
        app.start("cloud.connect");
        crate::cloud_setup::prefill(app, connection);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if edit.is_some() {
        app.start("cloud.configure");
        if let Some(connection) = edit {
            git::prefill(app, connection);
        }
    }
}

#[cfg(test)]
mod tests;

fn busy_button(ui: &mut egui::Ui, id: &str, label: &str, enabled: bool, busy: bool) -> bool {
    let icon = if id.starts_with("cloud_settings:cancel") {
        "stop"
    } else if id == "cloud_settings:sync" {
        "sync"
    } else {
        "inspect"
    };
    let response = ui_kit::busy_button(ui, label, icon, enabled, busy);
    crate::scenario::publish_target(id, response.rect);
    response.clicked()
}
