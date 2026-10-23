//! Manufacture workspace shell: enter/exit the MANUFACTURE tab and host follow-up panels.
//!
//! Setup / WCS editing lives in `manufacture_setup` on this PR. Later stacked PRs add
//! op inspectors. Session-only fields are not companion HTTP or GPL CAM runtimes.

use crate::SolveApp;
use solidcraft_manufacture::ValidationIssue;

/// Sub-panels of the Manufacture workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MfgTab {
    #[default]
    Setup,
}

/// Manufacture process family for a new setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CamMode {
    #[default]
    Milling,
    Turning,
    Additive,
    Inspection,
    Fabrication,
}

#[derive(Default)]
pub struct State {
    pub active: bool,
    /// Name of the currently active setup (key into `ManufactureSettings::setups`).
    pub active_setup: String,
    pub tab: MfgTab,
    pub cam_mode: CamMode,
    pub revision: u64,
    pub last_issues: Vec<ValidationIssue>,
    pub setup_dialog: Option<crate::manufacture_setup::SetupDialogState>,
    pub menu_dialog: Option<crate::manufacture_setup::MenuDialogState>,
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
    close_floating_dialogs(app);
    app.manufacture_workspace.active = false;
    if app.ui.tab == "MANUFACTURE" {
        app.ui.tab = "MODEL".into();
    }
}

pub fn reset_document(app: &mut SolveApp) {
    app.manufacture_workspace = State::default();
}

pub fn close_floating_dialogs(app: &mut SolveApp) {
    app.manufacture_workspace.setup_dialog = None;
    app.manufacture_workspace.menu_dialog = None;
    app.ui.pick_bodies = false;
}

/// Return false to cancel starting `id` after handling it here.
pub fn start_hook(app: &mut SolveApp, id: &str) -> bool {
    match id {
        "manufacture.setup.new" => {
            crate::manufacture_setup::open_new(app);
            false
        }
        "manufacture.setup.edit" => {
            let name = app.manufacture_workspace.active_setup.clone();
            if !name.is_empty() {
                crate::manufacture_setup::open_edit(app, &name);
            }
            false
        }
        _ => true,
    }
}

pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if app.manufacture_workspace.setup_dialog.is_none() {
        return;
    }
    if !ctx.input(|i| i.pointer.primary_clicked()) {
        return;
    }
    let Some(pos) = ctx.pointer_interact_pos() else {
        return;
    };
    let Some(rect) = app.viewport.rect else {
        return;
    };
    if !rect.contains(pos) {
        return;
    }
    let proj = crate::viewport::projection(app, rect);
    let _ = crate::manufacture_setup::try_pick_origin(app, &proj, pos);
}

pub fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::manufacture_setup::show_dialogs(app, ui);
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::viewport::show(app, ui);
}

/// Body visibility filter used by the viewport once Setup selection lands.
pub fn setup_shows_body(app: &SolveApp, name: &str) -> bool {
    let Some(state) = app.manufacture_workspace.setup_dialog.as_ref() else {
        return true;
    };
    if state.draft.model_bodies.is_empty() {
        return true;
    }
    state.draft.model_bodies.iter().any(|body| body == name)
}

pub fn setup_body_key(app: &SolveApp) -> u64 {
    let Some(state) = app.manufacture_workspace.setup_dialog.as_ref() else {
        return 0;
    };
    state.draft.model_bodies.len() as u64
}

/// Forward WCS origin picks to the setup dialog when open.
pub fn try_pick_origin(app: &mut SolveApp, proj: &crate::viewport::Proj, p: egui::Pos2) -> bool {
    crate::manufacture_setup::try_pick_origin(app, proj, p)
}
