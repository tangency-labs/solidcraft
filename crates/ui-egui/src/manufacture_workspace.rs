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
    Tools,
    TwoD,
    Contour,
    Drill,
    HoleRecognition,
    ThreeD,
    MultiAxis,
    Turning,
    PartHandling,
    Inspection,
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
    pub selected_tool: Option<usize>,
    pub tool_library_dialog: Option<crate::manufacture_tools::ToolLibraryState>,
    pub edit_tool_dialog: Option<crate::manufacture_tools::EditToolState>,
    pub manage_stub_dialog: Option<crate::manufacture_tools::ManageStubState>,
    pub selected_op: Option<usize>,
    pub twod_dialog: Option<crate::manufacture_2d::TwodDialogState>,
    pub contour_dialog: Option<crate::manufacture_contour::ContourDialogState>,
    pub drill_dialog: Option<crate::manufacture_drill::DrillDialogState>,
    pub hole_rec_dialog: Option<crate::manufacture_hole_recognition::HoleRecDialogState>,
    pub threed_dialog: Option<crate::manufacture_3d::ThreedDialogState>,
    pub multiaxis_dialog: Option<crate::manufacture_multiaxis::MultiAxisDialogState>,
    pub turning_dialog: Option<crate::manufacture_turning::TurningDialogState>,
    pub part_handling_dialog: Option<crate::manufacture_part_handling::PartHandlingDialogState>,
    pub inspection_dialog: Option<crate::manufacture_inspection::InspectionDialogState>,
    pub manual_inspection_dialog: Option<crate::manufacture_inspection::ManualInspectionDialogState>,
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
    app.manufacture_workspace.tool_library_dialog = None;
    app.manufacture_workspace.edit_tool_dialog = None;
    app.manufacture_workspace.manage_stub_dialog = None;
    app.manufacture_workspace.twod_dialog = None;
    app.manufacture_workspace.contour_dialog = None;
    app.manufacture_workspace.drill_dialog = None;
    app.manufacture_workspace.hole_rec_dialog = None;
    app.manufacture_workspace.threed_dialog = None;
    app.manufacture_workspace.multiaxis_dialog = None;
    app.manufacture_workspace.turning_dialog = None;
    app.manufacture_workspace.part_handling_dialog = None;
    app.manufacture_workspace.inspection_dialog = None;
    app.manufacture_workspace.manual_inspection_dialog = None;
    app.ui.pick_bodies = false;
}

/// Pick the first setup, or create Setup1, so tool dialogs have a document target.
pub fn ensure_setup(app: &mut SolveApp) {
    if !app.manufacture_workspace.active_setup.is_empty() && app.session.doc.manufacture.setups.contains_key(&app.manufacture_workspace.active_setup)
    {
        return;
    }
    if let Some(name) = app.session.doc.manufacture.setups.keys().next().cloned() {
        app.manufacture_workspace.active_setup = name;
        return;
    }
    match app.run("manufacture.settings.set", serde_json::json!({"name": "Setup1", "set": {}})) {
        Ok(_) => app.manufacture_workspace.active_setup = "Setup1".into(),
        Err(e) => app.set_status(e, true),
    }
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
        "manufacture.tools.library" => {
            crate::manufacture_tools::open_tool_library(app);
            false
        }
        "manufacture.tools.new" => {
            crate::manufacture_tools::open_edit_tool(app, None);
            false
        }
        "manufacture.2d.contour" => {
            crate::manufacture_contour::open_new(app);
            false
        }
        "manufacture.drill" => {
            crate::manufacture_drill::open_new(app);
            false
        }
        "manufacture.hole_recognition" => {
            crate::manufacture_hole_recognition::open_new(app);
            false
        }
        id if id.starts_with("manufacture.inspection.") => crate::manufacture_inspection::open_from_command(app, id),
        id if id.starts_with("manufacture.part_handling.") => crate::manufacture_part_handling::open_from_command(app, id),
        id if id.starts_with("manufacture.turning.") => crate::manufacture_turning::open_from_command(app, id),
        id if id.starts_with("manufacture.multiaxis.") => crate::manufacture_multiaxis::open_from_command(app, id),
        id if id.starts_with("manufacture.3d.") => crate::manufacture_3d::open_from_command(app, id),
        id if id.starts_with("manufacture.2d.") => crate::manufacture_2d::open_from_command(app, id),
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
    crate::manufacture_tools::show_dialogs(app, ui);
    crate::manufacture_2d::show_dialogs(app, ui);
    crate::manufacture_contour::show_dialogs(app, ui);
    crate::manufacture_drill::show_dialogs(app, ui);
    crate::manufacture_hole_recognition::show_dialogs(app, ui);
    crate::manufacture_3d::show_dialogs(app, ui);
    crate::manufacture_multiaxis::show_dialogs(app, ui);
    crate::manufacture_turning::show_dialogs(app, ui);
    crate::manufacture_part_handling::show_dialogs(app, ui);
    crate::manufacture_inspection::show_dialogs(app, ui);
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
