//! Native checkout setup. Tests resolve destinations; Preferences owns durable application.
use super::*;
use solidcraft_cloud::{Authentication, SshIdentity};
use std::sync::Arc;

mod key;

pub(crate) struct Wizard {
    connection: Connection,
    executable: String,
    existing: bool,
    pending: Option<String>,
    verified: Option<(Connection, String)>,
    error: Option<String>,
    owner: Arc<()>,
    key: Option<key::Editor>,
}

pub(crate) fn open(app: &mut SolveApp) {
    crate::prefs::open_at(app, crate::prefs::PAGE_CLOUD);
    begin(app);
    let Some(editor) = app.cloud.editor.as_mut() else { return };
    if editor.pending.is_some() || editor.git.is_some() || app.cloud.setup.is_some() {
        return;
    }
    editor.git = Some(Wizard {
        connection: Connection {
            id: "git".into(),
            name: String::new(),
            provider: String::new(),
            credential: String::new(),
            authentication: Authentication::System,
            ..Default::default()
        },
        executable: editor.draft.git_executable.clone(),
        existing: false,
        pending: None,
        verified: None,
        error: None,
        owner: Arc::new(()),
        key: None,
    });
}

pub(crate) fn prefill(app: &mut SolveApp, connection: Connection) {
    if let Some(wizard) = app.cloud.editor.as_mut().and_then(|e| e.git.as_mut()) {
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
            && Settings { git_executable: self.executable.clone(), ..Default::default() }.validate().is_ok()
    }

    fn verified(&self) -> bool {
        self.verified.as_ref().is_some_and(|(connection, executable)| connection == &self.connection && executable == &self.executable)
    }

    fn poll(&mut self, all: &[JobStatus]) -> Result<(), String> {
        let Some(id) = &self.pending else { return Ok(()) };
        let status = all.iter().find(|s| s.operation == *id).ok_or_else(|| tr("cloud.setup_failed").to_owned())?;
        match status.phase {
            Phase::Ready => {
                let mut connection: Connection =
                    serde_json::from_value(status.result.clone().ok_or_else(|| tr("cloud.setup_failed").to_owned())?).map_err(|e| e.to_string())?;
                connection.validate().map_err(|e| e.to_string())?;
                connection.name = connection.repository_label();
                self.connection = connection.clone();
                self.verified = Some((connection, self.executable.clone()));
                self.pending = None;
                self.error = None;
            }
            Phase::Failed | Phase::Cancelled => {
                self.pending = None;
                return Err(status.message.clone().unwrap_or_else(|| tr("cloud.setup_failed").into()));
            }
            _ => {}
        }
        Ok(())
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
            && let Some(wizard) = app.cloud.editor.as_mut().and_then(|e| e.git.as_mut())
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
                ui_kit::bounded_cell_editor(ui, egui::Id::new(("cloud_git_field", key)), value, "", false, 4096)
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
    let Some(mut wizard) = app.cloud.editor.as_mut().and_then(|e| e.git.take()) else { return };
    if let Err(error) = wizard.poll(&statuses(app)) {
        wizard.pending = None;
        wizard.error = Some(error);
    }
    key::poll(app, &mut wizard);
    wizard.connection.name = wizard.connection.repository_label();
    let busy = wizard.pending.is_some() || wizard.key.as_ref().is_some_and(key::Editor::busy);
    let enabled = !busy && !app.file_picker_busy() && !app.session.cloud_configuring();
    let mut browse = None;
    let primary = tr(if wizard.verified() { "cloud.git_use" } else { "cmd.cloud.test" });
    let (closed, action) = ui_kit::Window::new(egui::Id::new("cloud_git_setup"), "settings", tr("cloud.git_edit"))
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
                            if wizard.connection.authentication != Authentication::Ssh {
                                wizard.connection.ssh_identity = SshIdentity::System;
                            }
                        }
                    }
                });
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
            let editor = app.cloud.editor.as_mut();
            editor.ok_or_else(|| tr("cloud.settings_changed").to_owned()).and_then(|editor| {
                editor.stage(wizard.connection.clone())?;
                editor.draft.git_executable = wizard.executable.clone();
                Ok(())
            })
        } else {
            let mut candidate = wizard.connection.clone();
            candidate.git_remote = None;
            if !wizard.existing {
                candidate.id = "git".into();
            }
            app.run("cloud.test", json!({"proposed":candidate,"git_executable":wizard.executable})).and_then(|reply| {
                wizard.pending = Some(reply.get("operation").and_then(|v| v.as_str()).ok_or_else(|| tr("cloud.setup_failed").to_owned())?.into());
                Ok(())
            })
        };
        if wizard.verified() && result.is_ok() {
            app.set_status(tr("cloud.setup_ready"), false);
            return;
        }
        wizard.error = result.err();
    }
    let owner = wizard.owner.clone();
    if let Some(editor) = app.cloud.editor.as_mut() {
        editor.git = Some(wizard);
    }
    if let Some(field) = browse {
        pick(app, field, owner);
    }
}

#[cfg(test)]
mod tests;
