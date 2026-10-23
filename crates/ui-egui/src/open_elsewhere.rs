//! Two writers on one design (#5467): the desktop app keeps a lock file next to every design it
//! has open (`solidcraft_engine::io::design_lock`), says so when a design it opens is open in
//! another SolidCraft (on another computer, or in another window here), and asks before a Save
//! replaces a file that another program or computer changed since it was opened.
//!
//! Only interactive actions ask. A control-channel or script `doc.open` opens and reports
//! `open_elsewhere` in its result; a programmatic `file.save` over a changed file is refused with
//! an error until it passes `overwrite: true`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;
use solidcraft_engine::io::design_lock::{self, Lock};

use crate::SolveApp;
use crate::file_load::Request;
use crate::ui_kit;

/// The lock files this app holds, one per open design file (the desktop app turns them on).
#[derive(Default)]
pub struct Locks {
    /// Each locked design's file, and when it was opened (Unix seconds).
    held: BTreeMap<String, u64>,
    /// When the locks were last written again.
    refreshed: u64,
}

/// An Open waiting for the answer to "open elsewhere".
pub struct LockPrompt {
    request: Request,
    lock: Lock,
}

/// Keep lock files for the designs this app opens (the desktop app; never the browser or tests).
pub fn start(app: &mut SolveApp) {
    if !cfg!(target_arch = "wasm32") {
        app.locks = Some(Locks::default());
    }
}

/// The design files open in this app that it should hold a lock on: every tab's file, except
/// a design opened read-only (whoever has it open is the one writing it).
fn wanted(app: &SolveApp) -> BTreeSet<String> {
    std::iter::once(&app.session)
        .chain(app.docs.background())
        .filter(|s| !s.read_only)
        .filter_map(|s| s.path.clone())
        .filter(|p| solidcraft_engine::io::is_design_path(p))
        .collect()
}

/// Take a lock for each design just opened or saved under a new name, drop the ones no longer
/// open, and write them all again every [`design_lock::REFRESH_SECS`]. A lock that can't be
/// written (a read-only folder) is no reason to stop anything: the design stays open without one.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if app.locks.is_none() {
        return;
    }
    let wanted = wanted(app);
    let Some(locks) = app.locks.as_mut() else { return };
    let now = design_lock::now_secs();
    let refresh = now.saturating_sub(locks.refreshed) >= design_lock::REFRESH_SECS;
    if refresh {
        locks.refreshed = now;
    }
    locks.held.retain(|path, _| {
        let keep = wanted.contains(path);
        if !keep {
            design_lock::remove(std::path::Path::new(path));
        }
        keep
    });
    for path in wanted {
        let new = !locks.held.contains_key(&path);
        let opened = *locks.held.entry(path.clone()).or_insert(now);
        if new || refresh {
            let file = std::path::Path::new(&path);
            // Opened anyway while another app holds it: its lock stays until it goes stale.
            if design_lock::held_elsewhere(file, now).is_none() {
                let _ = design_lock::write(file, &Lock { refreshed: now, ..Lock::mine(opened) });
            }
        }
    }
    if !locks.held.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_secs(design_lock::REFRESH_SECS));
    }
}

/// The app is closing: its locks go.
pub fn close(app: &mut SolveApp) {
    if let Some(locks) = app.locks.take() {
        for path in locks.held.keys() {
            design_lock::remove(std::path::Path::new(path));
        }
    }
}

/// Before an interactive Open of a design: when another SolidCraft has it open, hold the request
/// and ask (`None`); otherwise hand it back to be opened.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn check_open(app: &mut SolveApp, request: Request) -> Option<Request> {
    if app.locks.is_none() || request.reply.is_some() || !solidcraft_engine::io::is_design_path(&request.path) {
        return Some(request);
    }
    match design_lock::held_elsewhere(std::path::Path::new(&request.path), design_lock::now_secs()) {
        Some(lock) => {
            app.lock_prompts.push(LockPrompt { request, lock });
            None
        }
        None => Some(request),
    }
}

/// Save to the design's own file, unless it was opened read-only (Save As instead) or something
/// else changed the file since it was opened or saved (ask first). `false`: Save As or the
/// question is under way instead.
pub(crate) fn check_save(app: &mut SolveApp) -> bool {
    if app.session.read_only {
        app.set_status(crate::i18n::tr("document.read_only_save"), false);
        crate::toolbar::save_copy(app);
        return false;
    }
    if app.session.disk_changed() {
        app.disk_prompt = true;
        return false;
    }
    true
}

/// The file name a prompt names.
fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// The "open elsewhere" and "changed on disk" questions, when one is waiting.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let _language = crate::i18n::scope(&app.preferences.language);
    show_lock_prompt(app, ctx);
    show_disk_prompt(app, ctx);
}

fn show_lock_prompt(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(prompt) = app.lock_prompts.first() else { return };
    let name = file_name(&prompt.request.path);
    let lock = &prompt.lock;
    let who = if lock.user.is_empty() { lock.host.clone() } else { format!("{} ({})", lock.host, lock.user) };
    let title = crate::i18n::format("document.open_elsewhere", &[("name", &name)]);
    let detail = crate::i18n::format(
        "document.open_elsewhere_detail",
        &[("name", &name), ("who", &who), ("when", &crate::recovery_ui::ago(lock.opened)), ("app", &lock.app)],
    );
    let (closed, action) = ui_kit::Confirm::new(egui::Id::new("sc_open_elsewhere"), "status_warning", &title)
        .tone(ui_kit::Tone::Warning)
        .actions(crate::i18n::tr("document.open_read_only"), crate::i18n::tr("ui.cancel"), Some(crate::i18n::tr("document.open_anyway")))
        .selected(ui_kit::Action::Ok)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &detail, ui_kit::Text::Report("open_elsewhere", false));
        });
    let mut answer = match action {
        ui_kit::Action::Ok => Some(LockAnswer::ReadOnly),
        ui_kit::Action::Apply => Some(LockAnswer::Anyway),
        ui_kit::Action::Cancel => Some(LockAnswer::Cancel),
        ui_kit::Action::None if closed => Some(LockAnswer::Cancel),
        ui_kit::Action::None => None,
    };
    if ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        answer = Some(LockAnswer::Cancel);
        app.esc_handled = true;
    }
    if let Some(a) = answer {
        answer_lock(app, a);
    }
}

/// What "open elsewhere" was answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockAnswer {
    ReadOnly,
    Anyway,
    Cancel,
}

/// Act on the "open elsewhere" answer.
pub fn answer_lock(app: &mut SolveApp, answer: LockAnswer) {
    if app.lock_prompts.is_empty() {
        return;
    }
    let LockPrompt { mut request, .. } = app.lock_prompts.remove(0);
    match answer {
        LockAnswer::ReadOnly => {
            request.options.insert("read_only".into(), json!(true));
            reopen(app, request);
        }
        LockAnswer::Anyway => reopen(app, request),
        LockAnswer::Cancel => {}
    }
}

/// Open the held request now that it is answered (without asking again).
fn reopen(app: &mut SolveApp, request: Request) {
    #[cfg(not(target_arch = "wasm32"))]
    let _ = crate::file_load::queue(app, request);
    #[cfg(target_arch = "wasm32")]
    let _ = crate::file_load::begin_request(app, request);
}

/// What "changed on disk" was answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskAnswer {
    SaveCopy,
    Overwrite,
    Cancel,
}

fn show_disk_prompt(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.disk_prompt {
        return;
    }
    let name = app.session.path.as_deref().map(file_name).unwrap_or_default();
    let title = crate::i18n::format("document.changed_on_disk", &[("name", &name)]);
    let detail = crate::i18n::format("document.changed_on_disk_detail", &[("name", &name)]);
    let (closed, action) = ui_kit::Confirm::new(egui::Id::new("sc_changed_on_disk"), "status_warning", &title)
        .tone(ui_kit::Tone::Warning)
        .actions(crate::i18n::tr("document.save_copy"), crate::i18n::tr("ui.cancel"), Some(crate::i18n::tr("document.overwrite")))
        .selected(ui_kit::Action::Ok)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &detail, ui_kit::Text::Report("changed_on_disk", false));
        });
    let mut answer = match action {
        ui_kit::Action::Ok => Some(DiskAnswer::SaveCopy),
        ui_kit::Action::Apply => Some(DiskAnswer::Overwrite),
        ui_kit::Action::Cancel => Some(DiskAnswer::Cancel),
        ui_kit::Action::None if closed => Some(DiskAnswer::Cancel),
        ui_kit::Action::None => None,
    };
    if ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        answer = Some(DiskAnswer::Cancel);
        app.esc_handled = true;
    }
    if let Some(a) = answer {
        answer_disk(app, a);
    }
}

/// Act on the "changed on disk" answer. A Save answering "save changes?" on close or quit goes on
/// from here as it would have from the Save.
pub fn answer_disk(app: &mut SolveApp, answer: DiskAnswer) {
    if !std::mem::take(&mut app.disk_prompt) {
        return;
    }
    match answer {
        DiskAnswer::SaveCopy => crate::toolbar::save_copy(app),
        DiskAnswer::Overwrite => {
            if !crate::save_job::start(app, "file.save", json!({"overwrite": true})) {
                crate::documents::saved_for_close(app);
            }
        }
        DiskAnswer::Cancel => crate::documents::saved_for_close(app),
    }
}
