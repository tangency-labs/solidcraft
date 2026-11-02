//! Send to Slicer's network folders (#2304): the window that adds or edits one (its type, a name,
//! the folder, on this computer or on an SFTP server, and a Test button), the copy a send to one
//! goes on with in the background, and their rows in Preferences › General. The commands do the
//! work (`file.slicers.add_remote` and `file.slicers.remove_remote`, kept in the preferences by
//! `SolveApp::run`). The Test button and the copy run off the window thread: a share whose server
//! has gone, or an SFTP server that does not answer, can keep a call waiting.
//!
//! An SFTP server's key is never trusted silently: Test shows the fingerprint of a server
//! `~/.ssh/known_hosts` doesn't list, and only the Trust button that follows trusts it.
//!
//! The browser has no folder dialog, and offers no network folder.

use serde_json::{Value, json};
use solidcraft_engine::remote_slicers::{self, Dest, RemoteTarget, SftpAddress, Tested};

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::ui_kit;

/// Edge of a network folder's icon in a list.
pub(crate) const ICON: f32 = 16.0;

/// The window, and a copy going on.
#[derive(Default)]
pub struct State {
    form: Option<Form>,
    /// A folder chosen in the folder dialog, waiting for the window to take it.
    picked: Option<String>,
    /// A send whose copy goes on in the background: its ticket, the network folder's name and the
    /// file that is being copied.
    sending: Option<(u64, String, String)>,
}

/// The Type row: a folder on this computer, or one on an SFTP server.
const FOLDER: usize = 0;
const SFTP: usize = 1;

/// The values of the window.
struct Form {
    /// [`FOLDER`] or [`SFTP`].
    kind: usize,
    name: String,
    folder: String,
    /// The SFTP server, `user@host[:port]`.
    server: String,
    /// The folder on the SFTP server.
    remote_folder: String,
    /// The private key file; empty for the keys of ssh-agent.
    key: String,
    /// The fingerprint of the SFTP server's key the user trusted (Test, then Trust).
    host_key: Option<String>,
    /// The name of the network folder being edited; none for a new one.
    editing: Option<String>,
    /// Opened from Send to Slicer: the network folder it adds becomes the dialog's choice.
    from_dialog: bool,
    test: Test,
    /// Why adding it was refused.
    error: Option<String>,
}

#[derive(Default)]
enum Test {
    #[default]
    NotRun,
    Running(std::sync::mpsc::Receiver<Tested>),
    Done(Tested),
}

impl Form {
    fn new(from_dialog: bool) -> Form {
        Form {
            kind: FOLDER,
            name: String::new(),
            folder: String::new(),
            server: String::new(),
            remote_folder: String::new(),
            key: String::new(),
            host_key: None,
            editing: None,
            from_dialog,
            test: Test::NotRun,
            error: None,
        }
    }

    /// The key file as given, when one is.
    fn key_file(&self) -> Option<&str> {
        Some(self.key.trim()).filter(|k| !k.is_empty())
    }

    /// The network folder the window describes, named `name`, or why it describes none yet.
    fn target(&self, name: &str) -> Result<RemoteTarget, String> {
        if self.kind == FOLDER {
            return remote_slicers::check_folder(&self.folder).map(|folder| RemoteTarget::folder_target(name, folder));
        }
        let address = SftpAddress::parse(&self.server)?;
        let folder = remote_slicers::check_remote_folder(&self.remote_folder)?;
        let key = self.key_file().map(remote_slicers::check_key_file).transpose()?;
        Ok(RemoteTarget::sftp_target(name, &address, folder, key, self.host_key.as_deref()))
    }

    /// What is wrong with an SFTP field filled in so far.
    fn field_problem(&self) -> Option<String> {
        if self.kind != SFTP {
            return None;
        }
        let server = Some(self.server.trim()).filter(|s| !s.is_empty()).and_then(|s| SftpAddress::parse(s).err());
        server.or_else(|| self.key_file().and_then(|k| remote_slicers::check_key_file(k).err()))
    }

    /// The fields that, changed, make a test answer stale.
    fn fields(&self) -> (usize, String, String, String, String) {
        (self.kind, self.folder.clone(), self.server.clone(), self.remote_folder.clone(), self.key.clone())
    }
}

/// Network folders can be offered: the host can ask for a folder (not the browser).
pub fn available(app: &SolveApp) -> bool {
    app.services.pick_folder.is_some()
}

/// The window for a new network folder.
pub fn open_new(app: &mut SolveApp, from_dialog: bool) {
    app.network_folder.picked = None;
    app.network_folder.form = Some(Form::new(from_dialog));
}

/// The window for the network folder named `name`.
pub fn open_edit(app: &mut SolveApp, name: &str) {
    let Some(t) = app.preferences.network_folders.iter().find(|t| t.name == name) else { return };
    let mut form = Form::new(false);
    form.name = t.name.clone();
    form.editing = Some(t.name.clone());
    match &t.dest {
        Dest::Folder { folder } => form.folder = folder.clone(),
        Dest::Sftp { folder, key, host_key, .. } => {
            form.kind = SFTP;
            form.server = t.address().map(|a| a.to_string()).unwrap_or_default();
            form.remote_folder = folder.clone();
            form.key = key.clone().unwrap_or_default();
            form.host_key = host_key.clone();
        }
    }
    app.network_folder.picked = None;
    app.network_folder.form = Some(form);
}

/// Take a network folder out of the list; the files sent there stay.
pub fn remove(app: &mut SolveApp, name: &str) {
    if let Err(e) = app.run("file.slicers.remove_remote", json!({ "name": name })) {
        app.set_status(e, true);
    }
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// The window, when it is open.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if let Some(path) = app.network_folder.picked.take()
        && let Some(form) = app.network_folder.form.as_mut()
    {
        form.folder = path;
        form.test = Test::NotRun;
        form.error = None;
    }
    let Some(mut form) = app.network_folder.form.take() else { return };
    let _language = crate::i18n::scope(&app.preferences.language);
    if let Test::Running(rx) = &form.test {
        use std::sync::mpsc::TryRecvError;
        match rx.try_recv() {
            Ok(answer) => form.test = Test::Done(answer),
            Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
            Err(TryRecvError::Disconnected) => {
                let stopped = crate::i18n::tr("ui.network_folder_test_stopped").to_string();
                form.test = Test::Done(Tested { ok: false, error: Some(stopped), host_key: None });
            }
        }
    }
    let taken = app
        .preferences
        .network_folders
        .iter()
        .any(|t| same_name(&t.name, &form.name) && form.editing.as_deref().is_none_or(|old| !same_name(old, &t.name)));
    let problem = remote_slicers::check_name(&form.name).err().filter(|_| !form.name.trim().is_empty());
    let problem = problem.or_else(|| taken.then(|| crate::i18n::tr("ui.network_folder_name_taken").to_string()));
    let problem = problem.or_else(|| form.field_problem());
    let valid = problem.is_none()
        && remote_slicers::check_name(&form.name).is_ok()
        && form.target(&form.name).is_ok()
        && !matches!(form.test, Test::Running(_));
    let tr = |id: &str| crate::i18n::tr(id).to_string();
    let title = tr(if form.editing.is_some() { "ui.network_folder_edit_title" } else { "ui.network_folder_title" });
    let ok = tr(if form.editing.is_some() { "ui.network_folder_save" } else { "ui.network_folder_add" });
    let (test, note) = (tr("ui.network_folder_test"), tr("ui.network_folder_note"));
    let (name_label, name_hint) = (tr("ui.network_folder_name"), tr("ui.network_folder_name_hint"));
    let (folder_label, folder_hint, browse_label) =
        (tr("ui.network_folder_folder"), tr("ui.network_folder_folder_hint"), tr("ui.network_folder_browse"));
    let (type_label, folder_kind, sftp_kind, sftp_note) =
        (tr("ui.network_folder_type"), tr("ui.network_folder_type_folder"), tr("ui.network_folder_type_sftp"), tr("ui.network_folder_sftp_note"));
    let (server_label, server_hint, remote_hint) =
        (tr("ui.network_folder_server"), tr("ui.network_folder_server_hint"), tr("ui.network_folder_remote_hint"));
    let (key_label, key_hint, trust_label) = (tr("ui.network_folder_key"), tr("ui.network_folder_key_hint"), tr("ui.network_folder_trust"));
    let (before_name, before) = (form.name.clone(), form.fields());
    let mut browse = false;
    let mut trust: Option<String> = None;
    let picking = app.file_picker_busy();
    let (closed, action) = ui_kit::Confirm::new(egui::Id::new("sc_network_folder"), "network_folder", &title)
        .actions(&ok, "Cancel", Some(&test))
        .secondary_left()
        .show(ctx, valid, |ui| {
            ui_kit::text(ui, if form.kind == SFTP { &sftp_note } else { &note }, ui_kit::Text::Note("network_folder_note"));
            crate::dialogs::form_row(ui, &type_label, |ui| {
                let targets = ["dialog:NetworkFolder:Folder", "dialog:NetworkFolder:SFTP"];
                let segments = ui_kit::Segments { targets: &targets, intrinsic: true, ..Default::default() };
                ui_kit::segmented(ui, &mut form.kind, &[folder_kind.as_str(), sftp_kind.as_str()], segments);
            });
            ui_kit::text_row(ui, &name_label, &mut form.name, &name_hint);
            if form.kind == SFTP {
                ui_kit::text_row(ui, &server_label, &mut form.server, &server_hint);
                ui_kit::text_row(ui, &folder_label, &mut form.remote_folder, &remote_hint);
                ui_kit::text_row(ui, &key_label, &mut form.key, &key_hint);
            } else {
                ui_kit::text_row(ui, &folder_label, &mut form.folder, &folder_hint);
                crate::dialogs::form_row(ui, "", |ui| {
                    ui.add_enabled_ui(!picking, |ui| {
                        let r = ui_kit::button(ui, &browse_label, ui_kit::Button::Small);
                        crate::scenario::publish_handle("dialog:NetworkFolder:Browse", r.rect.center());
                        browse = r.clicked();
                    });
                });
            }
            match &form.test {
                Test::NotRun => {}
                Test::Running(_) => {
                    ui_kit::text(ui, crate::i18n::tr("ui.network_folder_testing"), ui_kit::Text::Hint("network_folder_test"));
                }
                Test::Done(tested) => trust = show_tested(ui, tested, &trust_label),
            }
            if let Some(e) = problem.as_ref().or(form.error.as_ref()) {
                ui_kit::text(ui, e, ui_kit::Text::Error("network_folder_error"));
            }
        });
    let now = form.fields();
    if form.name != before_name || now != before {
        form.error = None;
    }
    if now != before {
        // Another server: the key trusted was that one's.
        if form.server.trim() != before.2.trim() {
            form.host_key = None;
        }
        form.test = Test::NotRun;
    }
    if browse {
        app.pick_folder(|app, path| app.network_folder.picked = path);
    }
    if let Some(fingerprint) = trust {
        form.host_key = Some(fingerprint);
        start_test(&mut form);
    }
    match action {
        ui_kit::Action::Apply => start_test(&mut form),
        ui_kit::Action::Ok if valid => {
            if !save(app, &mut form) {
                app.network_folder.form = Some(form);
            }
            return;
        }
        ui_kit::Action::Cancel => return,
        _ if closed => return,
        _ => {}
    }
    app.network_folder.form = Some(form);
}

/// What a test found; the fingerprint the user chose to trust, when they did.
fn show_tested(ui: &mut egui::Ui, tested: &Tested, trust_label: &str) -> Option<String> {
    if tested.ok {
        ui_kit::text(ui, crate::i18n::tr("ui.network_folder_test_ok"), ui_kit::Text::Report("network_folder_test", false));
    }
    let Some(key) = &tested.host_key else {
        if let Some(e) = &tested.error {
            ui_kit::text(ui, e, ui_kit::Text::Error("network_folder_test"));
        }
        return None;
    };
    let fingerprint = [("fingerprint", key.fingerprint.as_str()), ("algorithm", key.algorithm.as_str())];
    // A server known_hosts lists with another key can't be trusted here: the error says what to do.
    if key.trusted || key.known_hosts {
        if tested.ok {
            ui_kit::text(ui, &crate::i18n::format("ui.network_folder_host_trusted", &fingerprint), ui_kit::Text::Hint("network_folder_host"));
        } else if let Some(e) = &tested.error {
            ui_kit::text(ui, e, ui_kit::Text::Error("network_folder_test"));
        }
        return None;
    }
    let line = crate::i18n::format(if key.changed { "ui.network_folder_host_changed" } else { "ui.network_folder_host_unknown" }, &fingerprint);
    ui_kit::text(ui, &line, if key.changed { ui_kit::Text::Error("network_folder_host") } else { ui_kit::Text::Note("network_folder_host") });
    let mut trusted = None;
    crate::dialogs::form_row(ui, "", |ui| {
        let r = ui_kit::button(ui, trust_label, ui_kit::Button::Small);
        crate::scenario::publish_handle("dialog:NetworkFolder:Trust", r.rect.center());
        if r.clicked() {
            trusted = Some(key.fingerprint.clone());
        }
    });
    trusted
}

/// Write and delete a small file in the folder, off the window thread.
fn start_test(form: &mut Form) {
    if matches!(form.test, Test::Running(_)) {
        return;
    }
    let target = match form.target("Test") {
        Ok(target) => target,
        Err(e) => {
            form.test = Test::Done(Tested { ok: false, error: Some(e), host_key: None });
            return;
        }
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let started = std::thread::Builder::new().name("network-folder-test".into()).spawn(move || {
        let _ = tx.send(remote_slicers::test_target(&target, None));
    });
    form.test = match started {
        Ok(_) => Test::Running(rx),
        Err(e) => Test::Done(Tested { ok: false, error: Some(e.to_string()), host_key: None }),
    };
}

/// Add (or change) the network folder; true when it was.
fn save(app: &mut SolveApp, form: &mut Form) -> bool {
    let name = form.name.trim().to_string();
    let mut params = json!({ "name": name, "folder": form.folder.trim() });
    if form.kind == SFTP {
        params = json!({ "name": name, "folder": form.remote_folder.trim(), "sftp": form.server.trim() });
        if let Some(key) = form.key_file() {
            params["key"] = json!(key);
        }
        if let Some(host_key) = &form.host_key {
            params["host_key"] = json!(host_key);
        }
    }
    let folder = form.target(&name).map(|t| t.shown()).unwrap_or_default();
    let added = match app.run("file.slicers.add_remote", params) {
        Ok(v) => v,
        Err(e) => {
            form.error = Some(e);
            return false;
        }
    };
    let id = added.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    // Renamed: the old entry goes, and what chose it now chooses the new one.
    if let Some(old) = form.editing.as_deref().filter(|old| !same_name(old, &name)) {
        let old_id = format!("{}{old}", remote_slicers::ID_PREFIX);
        let _ = app.run("file.slicers.remove_remote", json!({ "name": old }));
        if app.preferences.send_slicer == old_id {
            app.preferences.send_slicer = id.clone();
        }
        choose_in_dialog(app, &id, Some(&old_id));
    }
    if form.from_dialog {
        choose_in_dialog(app, &id, None);
    }
    app.set_status(crate::i18n::format("ui.network_folder_added", &[("name", &name), ("folder", &folder)]), false);
    true
}

/// Make `id` the choice of the open Send to Slicer dialog (only in place of `instead`, when given).
fn choose_in_dialog(app: &mut SolveApp, id: &str, instead: Option<&str>) {
    if let Some(Kind::SaveMesh(k)) = app.dialog.as_mut().map(|d| &mut d.kind) {
        crate::dialogs_save_mesh::choose(k, id, instead);
    }
}

/// After `file.send_to_slicer` answered: a copy to a network folder that goes on in the background
/// is followed until it ends. True when one was.
pub fn follow(app: &mut SolveApp, result: &Value) -> bool {
    let Some(ticket) = result.get("sending").and_then(Value::as_u64) else { return false };
    let name = result.get("target").and_then(Value::as_str).unwrap_or_default().to_string();
    let path = result.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
    app.set_status(crate::i18n::format("ui.network_folder_sending", &[("target", &name)]), false);
    app.network_folder.sending = Some((ticket, name, path));
    true
}

/// A copy to a network folder is going on (a second send waits for it).
pub fn sending(app: &SolveApp) -> Option<&str> {
    app.network_folder.sending.as_ref().map(|(_, name, _)| name.as_str())
}

/// Per frame: say where the copy went, or why it didn't, once it has ended.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(ticket) = app.network_folder.sending.as_ref().map(|s| s.0) else { return };
    let Some(answer) = remote_slicers::poll(ticket) else {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        return;
    };
    let Some((_, name, path)) = app.network_folder.sending.take() else { return };
    let _language = crate::i18n::scope(&app.preferences.language);
    match answer {
        Ok(sent) => {
            let to = sent.path.to_string_lossy();
            app.set_status(crate::i18n::format("ui.sent_to_network_folder", &[("target", &name), ("path", &to)]), false);
        }
        Err(e) => {
            let line = crate::i18n::format("ui.network_folder_send_failed", &[("target", &name), ("error", &e), ("path", &path)]);
            app.set_status(line, true);
        }
    }
}

/// Preferences › General: the network folders, to change or remove, and a button to add one.
pub fn prefs_rows(app: &mut SolveApp, ui: &mut egui::Ui) {
    let mut edit = None;
    let mut gone = None;
    if app.preferences.network_folders.is_empty() {
        ui_kit::text(ui, crate::i18n::tr("ui.network_folders_none"), ui_kit::Text::Hint("network_folders_none"));
    }
    for t in &app.preferences.network_folders {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ICON, ICON), egui::Sense::hover());
            crate::icons::paint_icon(ui.painter(), rect, "network_folder", 1.0);
            // The actions sit at the right edge; the name and folder take what is left and truncate.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui_kit::button(ui, crate::i18n::tr("ui.network_folder_remove"), ui_kit::Button::Small).clicked() {
                    gone = Some(t.name.clone());
                }
                if ui_kit::button(ui, crate::i18n::tr("ui.network_folder_edit"), ui_kit::Button::Small).clicked() {
                    edit = Some(t.name.clone());
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui_kit::text(ui, &format!("{} — {}", t.name, t.shown()), ui_kit::Text::Inline);
                });
            });
        });
    }
    let add = ui_kit::button(ui, crate::i18n::tr("ui.network_folder_add_button"), ui_kit::Button::Small);
    crate::scenario::publish_handle("prefs:network_folder_add", add.rect.center());
    if add.clicked() {
        open_new(app, false);
    }
    if let Some(name) = edit {
        open_edit(app, &name);
    }
    if let Some(name) = gone {
        remove(app, &name);
    }
}
