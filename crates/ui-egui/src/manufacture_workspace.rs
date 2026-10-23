//! Manufacture workspace shell: enter/exit the MANUFACTURE tab and host follow-up panels.
//!
//! Setup / op dialogs land in stacked PRs on top of this shell.

use crate::SolveApp;

#[derive(Default)]
pub struct State {
    pub active: bool,
}

pub fn active(app: &SolveApp) -> bool {
    app.manufacture_workspace.active
}

pub fn enter(app: &mut SolveApp) {
    if active(app) {
        return;
    }
    if crate::render_workspace::active(app) {
        let _ = crate::render_workspace::enter(app, false);
    }
    app.manufacture_workspace.active = true;
    if app.ui.tab != "MANUFACTURE" {
        app.ui.tab = "MANUFACTURE".into();
    }
}

pub fn exit(app: &mut SolveApp) {
    if !active(app) {
        return;
    }
    app.manufacture_workspace.active = false;
    if app.ui.tab == "MANUFACTURE" {
        app.ui.tab = "MODEL".into();
    }
}

pub fn reset_document(app: &mut SolveApp) {
    app.manufacture_workspace = State::default();
}

pub fn close_floating_dialogs(_app: &mut SolveApp) {}

/// Return false to cancel starting `id` (reserved for Manufacture commands).
pub fn start_hook(_app: &mut SolveApp, _id: &str) -> bool {
    true
}

pub fn tick(_app: &mut SolveApp, _ctx: &egui::Context) {}

pub fn panel(_app: &mut SolveApp, _ui: &mut egui::Ui) {}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    // Same viewport as Design until overlays land in the Setup PR.
    crate::viewport::show(app, ui);
}

/// Body visibility filter used by the viewport once Setup selection lands.
pub fn setup_shows_body(_app: &SolveApp, _name: &str) -> bool {
    true
}

pub fn setup_body_key(_app: &SolveApp) -> u64 {
    0
}
