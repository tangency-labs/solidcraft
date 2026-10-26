//! Native checkout setup. Home connects directly; Preferences stages edits for Apply.
use super::*;
use solidcraft_cloud::{Authentication, SshIdentity};
use std::sync::Arc;

mod https;
mod key;

pub(crate) struct Wizard {
    connection: Connection,
    executable: String,
    existing: bool,
    staged: bool,
    applying: bool,
    pending: Option<String>,
    verified: Option<(Connection, String)>,
    error: Option<String>,
    owner: Arc<()>,
    key: Option<key::Editor>,
    https: https::Editor,
}

pub(crate) fn open(app: &mut SolveApp) {
    if app.cloud.git_setup.is_some() || app.cloud.editor.as_ref().is_some_and(|e| e.pending.is_some() || e.git.is_some()) || app.cloud.setup.is_some()
    {
        return;
    }
    let staged = app.prefs_window.open && app.prefs_window.section == crate::prefs::PAGE_CLOUD;
    let executable = if staged {
        begin(app);
        app.cloud.editor.as_ref().map(|e| e.draft.git_executable.clone()).unwrap_or_default()
    } else {
        app.session.cloud_settings().git_executable.clone()
    };
    let wizard = Wizard {
        connection: Connection {
            id: "git".into(),
            name: String::new(),
            provider: String::new(),
            credential: String::new(),
            authentication: Authentication::System,
            ..Default::default()
        },
        executable,
        existing: false,
        staged,
        applying: false,
        pending: None,
        verified: None,
        error: None,
        owner: Arc::new(()),
        key: None,
        https: https::Editor::default(),
    };
    put(app, wizard);
}

fn wizard_mut(app: &mut SolveApp) -> Option<&mut Wizard> {
    if app.cloud.git_setup.is_some() { app.cloud.git_setup.as_mut() } else { app.cloud.editor.as_mut().and_then(|e| e.git.as_mut()) }
}

fn put(app: &mut SolveApp, wizard: Wizard) {
    if wizard.staged {
        if let Some(editor) = app.cloud.editor.as_mut() {
            editor.git = Some(wizard);
        }
    } else {
        app.cloud.git_setup = Some(wizard);
    }
}

pub(crate) fn prefill(app: &mut SolveApp, connection: Connection) {
    if let Some(wizard) = wizard_mut(app) {
        wizard.connection = connection;
        wizard.existing = true;
        wizard.verified = None;
    }
}

pub(crate) fn cancel(app: &mut SolveApp) {
    if let Some(wizard) = app.cloud.editor.as_mut().and_then(|e| e.git.take()) {
        cancel_test(app, &wizard);
    }
}

fn cancel_test(app: &mut SolveApp, wizard: &Wizard) {
    if wizard.applying {
        return;
    }
    wizard.https.cancel();
    if let Some(key) = &wizard.key {
        key.cancel(app);
    }
    if let Some(operation) = &wizard.pending {
        let _ = app.run("cloud.cancel", json!({"operation":operation}));
    }
}

impl Wizard {
    fn valid(&self) -> bool {
        !self.connection.checkout.trim().is_empty()
            && !self.connection.branch.is_empty()
            && self.connection.validate().is_ok()
            && self.https.valid(&self.connection)
            && Settings { git_executable: self.executable.clone(), ..Default::default() }.validate().is_ok()
    }

    fn verified(&self) -> bool {
        self.https.unchanged()
            && self.verified.as_ref().is_some_and(|(connection, executable)| connection == &self.connection && executable == &self.executable)
    }

    fn poll(&mut self, all: &[JobStatus]) -> Result<bool, String> {
        let Some(id) = &self.pending else { return Ok(false) };
        let status = all.iter().find(|s| s.operation == *id).ok_or_else(|| tr("cloud.setup_failed").to_owned())?;
        match status.phase {
            Phase::Ready => {
                if self.applying {
                    return Ok(true);
                }
                let mut connection: Connection =
                    serde_json::from_value(status.result.clone().ok_or_else(|| tr("cloud.setup_failed").to_owned())?).map_err(|e| e.to_string())?;
                connection.validate().map_err(|e| e.to_string())?;
                connection.name = connection.repository_label();
                self.connection = connection.clone();
                self.verified = Some((connection, self.executable.clone()));
                self.pending = None;
                self.error = None;
            }
            Phase::Synced if self.applying => return Ok(true),
            Phase::Failed | Phase::Cancelled => {
                self.pending = None;
                self.applying = false;
                return Err(status.message.clone().unwrap_or_else(|| tr("cloud.setup_failed").into()));
            }
            _ => {}
        }
        Ok(false)
    }
}

#[derive(Clone, Copy)]
enum Pick {
    Checkout,
    Key,
    Executable,
}

fn pick(app: &mut SolveApp, field: Pick, owner: Arc<()>) {
    let complete = move |app: &mut SolveApp, path: Option<String>| {
        if let Some(path) = path
            && let Some(wizard) = wizard_mut(app)
            && Arc::ptr_eq(&owner, &wizard.owner)
            && wizard.pending.is_none()
        {
            match field {
                Pick::Checkout => wizard.connection.checkout = path,
                Pick::Key => wizard.connection.ssh_identity = SshIdentity::File(path),
                Pick::Executable => wizard.executable = path,
            }
        }
    };
    match field {
        Pick::Checkout => app.pick_folder(complete),
        Pick::Key => app.pick_file(tr("cloud.ssh_key"), &[], complete),
        Pick::Executable => app.pick_file(tr("cloud.git_executable"), &[], complete),
    }
}

fn field(ui: &mut egui::Ui, key: &str, value: &mut String, browse: bool) -> bool {
    let mut picked = false;
    ui_kit::row(ui, tr(key), |ui| {
        ui.horizontal(|ui| {
            let t = crate::theme::Tokens::get();
            let browse_width = ui.painter().layout_no_wrap(tr("cloud.browse").into(), egui::FontId::proportional(t.font_label), t.form_ink).size().x
                + t.space_16 * 2.0
                + ui.spacing().item_spacing.x;
            let width = if browse { (ui.available_width() - browse_width).max(t.space_24) } else { ui.available_width() };
            let r = ui_kit::settings_control_width(ui, width, |ui| {
                let limit = if key == "cloud.git_username" { 256 } else { 4096 };
                ui_kit::bounded_cell_editor(ui, egui::Id::new(("cloud_git_field", key)), value, "", false, limit)
            });
            crate::scenario::publish_target(&format!("cloud_git:{key}"), r.rect);
            if browse {
                picked = button(ui, &format!("cloud_git:browse:{key}"), tr("cloud.browse"), true);
            }
        });
    });
    picked
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut wizard) = app.cloud.git_setup.take().or_else(|| app.cloud.editor.as_mut().and_then(|e| e.git.take())) else { return };
    if let Err(error) = https::poll(app, &mut wizard) {
        wizard.error = Some(error);
    }
    match wizard.poll(&statuses(app)) {
        Ok(true) => {
            crate::cloud_browser::select(app, &wizard.connection.id);
            app.set_status(tr("cloud.connected"), false);
            return;
        }
        Err(error) => {
            wizard.pending = None;
            wizard.applying = false;
            wizard.error = Some(error);
        }
        Ok(false) => {}
    }
    key::poll(app, &mut wizard);
    wizard.connection.name = wizard.connection.repository_label();
    let busy = wizard.pending.is_some() || wizard.https.busy() || wizard.key.as_ref().is_some_and(key::Editor::busy);
    let enabled = !busy && !app.file_picker_busy() && !app.session.cloud_configuring();
    let mut browse = None;
    let primary = tr(if wizard.verified() { "cloud.git_use" } else { "cmd.cloud.test" });
    let title = tr(if wizard.existing { "cloud.git_edit" } else { "cloud.git_new" });
    let (closed, action) = ui_kit::Window::new(egui::Id::new("cloud_git_setup"), "settings", title)
        .fields()
        .cards(2.2)
        .actions(primary, "Cancel", None)
        .show(ctx, enabled && wizard.valid() && !wizard.key.as_ref().is_some_and(key::Editor::editing), |ui| {
            ui_kit::text(ui, tr("cloud.git_hint"), ui_kit::Text::Inline);
            let choice_label_width = ui_kit::ContentWidth::label(ui, tr("cloud.git_auth")).max(crate::theme::Tokens::get().form_label_width)
                + crate::theme::Tokens::get().space_8;
            ui.add_enabled_ui(enabled, |ui| {
                if field(ui, "cloud.git_checkout", &mut wizard.connection.checkout, true) {
                    browse = Some(Pick::Checkout);
                }
                field(ui, "cloud.git_remote", &mut wizard.connection.remote, false);
                ui_kit::text(ui, tr("cloud.git_remote_hint"), ui_kit::Text::Hint("cloud_git_remote_hint"));
                field(ui, "cloud.git_branch", &mut wizard.connection.branch, false);
                ui_kit::settings_block(ui, tr("cloud.git_auth"), choice_label_width, |ui| {
                    for (kind, key) in [
                        (Authentication::System, "cloud.git_system"),
                        (Authentication::Https, "cloud.git_https"),
                        (Authentication::Ssh, "cloud.git_ssh"),
                    ] {
                        let r = ui_kit::list_picker(ui, key, tr(key), wizard.connection.authentication == kind);
                        crate::scenario::publish_target(&format!("cloud_git:{key}"), r.rect);
                        if r.clicked() {
                            wizard.connection.authentication = kind;
                            if wizard.connection.authentication != Authentication::Https {
                                wizard.https.clear();
                            }
                            if wizard.connection.authentication != Authentication::Ssh {
                                wizard.connection.ssh_identity = SshIdentity::System;
                                wizard.key = None;
                            }
                        }
                    }
                });
                https::show(ui, &mut wizard);
                if wizard.connection.authentication == Authentication::Ssh {
                    ui_kit::settings_block(ui, tr("cloud.git_ssh"), choice_label_width, |ui| {
                        let mut custom = matches!(wizard.connection.ssh_identity, SshIdentity::File(_));
                        let system = ui_kit::list_picker(ui, "cloud_ssh_system", tr("cloud.ssh_system"), !custom);
                        crate::scenario::publish_target("cloud_git:ssh_system", system.rect);
                        if system.clicked() {
                            custom = false;
                        }
                        let key = ui_kit::list_picker(ui, "cloud_ssh_key", tr("cloud.ssh_key"), custom);
                        crate::scenario::publish_target("cloud_git:ssh_key", key.rect);
                        if key.clicked() {
                            custom = true;
                        }
                        match (&wizard.connection.ssh_identity, custom) {
                            (SshIdentity::System, true) => wizard.connection.ssh_identity = SshIdentity::File(String::new()),
                            (SshIdentity::File(_), false) => wizard.connection.ssh_identity = SshIdentity::System,
                            _ => {}
                        }
                    });
                    if let SshIdentity::File(path) = &mut wizard.connection.ssh_identity
                        && field(ui, "cloud.ssh_key", path, true)
                    {
                        browse = Some(Pick::Key);
                    }
                }
                if field(ui, "cloud.git_executable", &mut wizard.executable, true) {
                    browse = Some(Pick::Executable);
                }
                ui_kit::text(ui, tr("cloud.git_executable_hint"), ui_kit::Text::Hint("cloud_git_executable_hint"));
            });
            if wizard.connection.authentication == Authentication::Ssh {
                key::show(app, ui, &mut wizard, enabled);
            }
            if busy {
                ui.spinner();
                ui.label(tr("cloud.connecting"));
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            if wizard.verified() {
                ui.label(tr("cloud.test_ok"));
                if let Some(remote) = &wizard.connection.git_remote {
                    ui_kit::row(ui, tr("cloud.git_fetch"), |ui| {
                        ui.label(&remote.fetch);
                    });
                    for url in &remote.push {
                        ui_kit::row(ui, tr("cloud.git_push"), |ui| {
                            ui.label(url);
                        });
                    }
                }
            }
            if let Some(error) = &wizard.error {
                ui_kit::text(ui, error, ui_kit::Text::Report("cloud_git_error", true));
            }
            ui.hyperlink_to(crate::i18n::tr_label("Help"), HELP);
        });
    if closed || action == ui_kit::Action::Cancel {
        cancel_test(app, &wizard);
        return;
    }
    // Revalidate after this frame's edits: a formerly verified destination cannot slip through.
    if action == ui_kit::Action::Ok && enabled && wizard.valid() && !wizard.key.as_ref().is_some_and(key::Editor::editing) {
        let result = if wizard.verified() {
            if wizard.staged {
                app.cloud.editor.as_mut().ok_or_else(|| tr("cloud.settings_changed").to_owned()).and_then(|editor| {
                    editor.stage(wizard.connection.clone())?;
                    editor.draft.git_executable = wizard.executable.clone();
                    Ok(())
                })
            } else {
                let mut settings = app.session.cloud_settings().clone();
                if let Some(connection) = settings.connections.iter_mut().find(|c| c.id == wizard.connection.id) {
                    *connection = wizard.connection.clone();
                } else {
                    settings.connections.push(wizard.connection.clone());
                }
                if settings.default_connection.is_none() {
                    settings.default_connection = Some(wizard.connection.id.clone());
                }
                settings.git_executable = wizard.executable.clone();
                app.run("cloud.configure", json!({"settings":settings})).and_then(|reply| {
                    wizard.pending = Some(reply.get("operation").and_then(|v| v.as_str()).ok_or_else(|| tr("cloud.setup_failed").to_owned())?.into());
                    wizard.applying = true;
                    Ok(())
                })
            }
        } else {
            https::test(app, &mut wizard)
        };
        if wizard.staged && wizard.verified() && result.is_ok() {
            app.set_status(tr("cloud.setup_ready"), false);
            return;
        }
        wizard.error = result.err();
    }
    let owner = wizard.owner.clone();
    put(app, wizard);
    if let Some(field) = browse {
        pick(app, field, owner);
    }
}

fn test(app: &mut SolveApp, wizard: &mut Wizard) -> Result<(), String> {
    let mut candidate = wizard.connection.clone();
    candidate.git_remote = None;
    if !wizard.existing {
        candidate.id = "git".into();
    }
    app.run("cloud.test", json!({"proposed":candidate,"git_executable":wizard.executable})).and_then(|reply| {
        wizard.pending = Some(reply.get("operation").and_then(|v| v.as_str()).ok_or_else(|| tr("cloud.setup_failed").to_owned())?.into());
        Ok(())
    })
}

#[cfg(test)]
mod tests;
