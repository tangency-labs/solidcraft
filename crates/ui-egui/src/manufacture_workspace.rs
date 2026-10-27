//! Manufacture workspace: process tabs (Milling / Turning / Additive / Inspection / Fabrication)
//! replace the Design ribbon. New Setup opens the SETUP window. Setups live in the left browser.

use crate::SolveApp;

/// Process families shown as the Manufacture workspace's top tabs.
pub const TABS: &[&str] = &["MILLING", "TURNING", "ADDITIVE", "INSPECTION", "FABRICATION"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CamMode {
    #[default]
    Milling,
    Turning,
    Additive,
    Inspection,
    Fabrication,
}

impl CamMode {
    pub fn tab(self) -> &'static str {
        match self {
            Self::Milling => "MILLING",
            Self::Turning => "TURNING",
            Self::Additive => "ADDITIVE",
            Self::Inspection => "INSPECTION",
            Self::Fabrication => "FABRICATION",
        }
    }

    pub fn from_tab(tab: &str) -> Option<Self> {
        Some(match tab {
            "MILLING" => Self::Milling,
            "TURNING" => Self::Turning,
            "ADDITIVE" => Self::Additive,
            "INSPECTION" => Self::Inspection,
            "FABRICATION" => Self::Fabrication,
            _ => return None,
        })
    }
}

#[derive(Default)]
pub struct State {
    pub active: bool,
    pub active_setup: String,
    pub cam_mode: CamMode,
    pub setup_dialog: Option<crate::manufacture_setup::SetupDialogState>,
    pub selected_tool: Option<usize>,
    pub tool_library_dialog: Option<crate::manufacture_tools::ToolLibraryState>,
    pub edit_tool_dialog: Option<crate::manufacture_tools::EditToolState>,
    pub manage_stub_dialog: Option<crate::manufacture_tools::ManageStubState>,
}

pub fn is_tab(tab: &str) -> bool {
    CamMode::from_tab(tab).is_some() || tab == "MANUFACTURE"
}

pub fn active(app: &SolveApp) -> bool {
    app.manufacture_workspace.active
}

pub fn show_tab(app: &mut SolveApp, tab: &str) {
    if let Some(mode) = CamMode::from_tab(tab) {
        app.manufacture_workspace.cam_mode = mode;
        app.ui.tab = tab.into();
    }
}

pub fn sync_tab(app: &mut SolveApp) {
    if let Some(mode) = CamMode::from_tab(&app.ui.tab) {
        app.manufacture_workspace.cam_mode = mode;
        return;
    }
    app.ui.tab = app.manufacture_workspace.cam_mode.tab().into();
}

pub fn enter(app: &mut SolveApp) {
    if crate::render_workspace::active(app) {
        let _ = crate::render_workspace::enter(app, false);
    }
    app.manufacture_workspace.active = true;
    if CamMode::from_tab(&app.ui.tab).is_none() {
        app.ui.tab = app.manufacture_workspace.cam_mode.tab().into();
    }
}

pub fn exit(app: &mut SolveApp) {
    if !active(app) {
        return;
    }
    close_floating_dialogs(app);
    app.manufacture_workspace.active = false;
    if is_tab(&app.ui.tab) {
        app.ui.tab = "MODEL".into();
    }
}

pub fn reset_document(app: &mut SolveApp) {
    app.manufacture_workspace = State::default();
}

pub fn close_floating_dialogs(app: &mut SolveApp) {
    app.manufacture_workspace.setup_dialog = None;
    app.manufacture_workspace.tool_library_dialog = None;
    app.manufacture_workspace.edit_tool_dialog = None;
    app.manufacture_workspace.manage_stub_dialog = None;
    app.ui.pick_bodies = false;
}

fn tab_for_command(id: &str) -> Option<&'static str> {
    Some(match id {
        s if s.starts_with("manufacture.turning.") || s.starts_with("manufacture.part_handling.") => "TURNING",
        s if s.starts_with("manufacture.additive.") => "ADDITIVE",
        s if s.starts_with("manufacture.inspection.") => "INSPECTION",
        s if s.starts_with("manufacture.fabrication.") => "FABRICATION",
        s if s.starts_with("manufacture.2d.")
            || s.starts_with("manufacture.3d.")
            || s.starts_with("manufacture.multiaxis.")
            || s == "manufacture.drill"
            || s == "manufacture.hole_recognition" =>
        {
            "MILLING"
        }
        _ => return None,
    })
}

/// Return false to cancel starting `id` after handling it here.
pub fn start_hook(app: &mut SolveApp, id: &str) -> bool {
    match id {
        "manufacture.setup.new" => {
            crate::manufacture_setup::open_new(app);
            false
        }
        "cam.setup" => {
            crate::manufacture_setup::open_new(app);
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
        "manufacture.manage.machine_library" => {
            crate::manufacture_tools::open_manage_stub(app, crate::manufacture_tools::ManageStubKind::MachineLibrary);
            false
        }
        "manufacture.manage.post_library" => {
            crate::manufacture_tools::open_manage_stub(app, crate::manufacture_tools::ManageStubKind::PostLibrary);
            false
        }
        "manufacture.manage.template_library" => {
            crate::manufacture_tools::open_manage_stub(app, crate::manufacture_tools::ManageStubKind::TemplateLibrary);
            false
        }
        id if id.starts_with("manufacture.") => {
            enter(app);
            if let Some(tab) = tab_for_command(id) {
                show_tab(app, tab);
            }
            true
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
    crate::manufacture_tools::show_dialogs(app, ui);
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::viewport::show(app, ui);
}

/// Hide bodies that are not in the setup being edited, or in the setup selected in the browser.
pub fn setup_shows_body(app: &SolveApp, name: &str) -> bool {
    if let Some(state) = app.manufacture_workspace.setup_dialog.as_ref() {
        if state.draft.model_bodies.is_empty() {
            return true;
        }
        return state.draft.model_bodies.iter().any(|body| body == name);
    }
    if !active(app) {
        return true;
    }
    let Some(setup) = app.session.doc.cam.setup(&app.manufacture_workspace.active_setup) else {
        return true;
    };
    if setup.model_bodies.is_empty() {
        return true;
    }
    setup.model_bodies.iter().any(|body| body == name)
}

pub fn setup_body_key(app: &SolveApp) -> u64 {
    if let Some(state) = app.manufacture_workspace.setup_dialog.as_ref() {
        return state.draft.model_bodies.len() as u64;
    }
    app.session.doc.cam.setup(&app.manufacture_workspace.active_setup).map(|s| s.model_bodies.len() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;
    use serde_json::json;

    #[test]
    fn new_setup_opens_the_setup_window_without_creating() {
        let mut h = Harness::new();
        h.step(&json!({"start": "manufacture.setup.new"})).unwrap();
        h.frames(2);
        assert!(active(&h.app));
        assert!(h.app.manufacture_workspace.setup_dialog.is_some());
        assert!(h.app.session.doc.cam.setups.is_empty());
        assert_eq!(h.app.manufacture_workspace.setup_dialog.as_ref().unwrap().name, "Setup1");
        assert!(h.widget("SETUP").is_some() || h.widget("Setup").is_some());
    }

    #[test]
    fn ok_without_bodies_does_not_save() {
        let mut h = Harness::new();
        h.step(&json!({"start": "manufacture.setup.new"})).unwrap();
        h.frames(2);
        h.step(&json!({"widget": "OK"})).unwrap();
        h.frames(2);
        assert!(h.app.session.doc.cam.setups.is_empty());
        assert!(h.app.manufacture_workspace.setup_dialog.is_some());
    }

    #[test]
    fn mill_panel_is_gone() {
        let mut h = Harness::new();
        h.step(&json!({"start": "manufacture.setup.new"})).unwrap();
        h.frames(2);
        assert!(h.widget("Add Setup").is_none());
        assert!(h.widget("Work offset").is_none());
        assert!(h.widget("G54").is_none());
        assert!(h.widget("CAM Setup").is_none());
        assert!(h.widget("New Setup").is_some());
        assert!(crate::scenario::handle_at("row:Setups").is_some());
        assert!(crate::scenario::handle_at("row:No setups yet").is_some());
    }

    #[test]
    fn manufacture_uses_process_tabs_not_the_design_ribbon() {
        let mut h = Harness::new();
        h.step(&json!({"start": "manufacture.setup.new"})).unwrap();
        h.frames(3);
        assert_eq!(crate::workspace::visible_tabs(&h.app), TABS);
        assert_eq!(h.app.ui.tab, "MILLING");
        for tab in ["MODEL", "SURFACE", "ASSEMBLE", "INSERT", "INSPECT", "MANAGE", "VIEW"] {
            assert!(crate::scenario::handle_at(&format!("tab:{tab}")).is_none(), "design tab {tab} must not paint in Manufacture");
        }
        for tab in TABS {
            assert!(crate::scenario::handle_at(&format!("tab:{tab}")).is_some(), "missing process tab {tab}");
        }
        assert!(crate::scenario::handle_at("button:solid.extrude").is_none());
        assert!(crate::scenario::handle_at("button:inspect.zebra").is_none());
        assert!(crate::scenario::handle_at("button:manufacture.setup.new").is_some());
        assert!(crate::scenario::handle_at("button:manufacture.2d.contour").is_some());
        assert!(crate::scenario::handle_at("button:manufacture.turning.face").is_none());
        h.step(&json!({"click": {"handle": "tab:TURNING"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "TURNING");
        assert_eq!(h.app.manufacture_workspace.cam_mode, CamMode::Turning);
        assert!(crate::scenario::handle_at("button:manufacture.turning.face").is_some());
        assert!(crate::scenario::handle_at("button:manufacture.2d.contour").is_none());
        assert!(crate::scenario::handle_at("button:solid.extrude").is_none());
    }

    #[test]
    fn tool_library_opens_without_creating_setup() {
        let mut h = Harness::new();
        h.step(&json!({"start": "manufacture.tools.library"})).unwrap();
        h.frames(2);
        assert!(active(&h.app));
        assert!(h.app.manufacture_workspace.tool_library_dialog.is_some());
        assert!(h.app.session.doc.cam.setups.is_empty());
        assert!(h.app.session.doc.cam.tools.is_empty());
        assert!(h.widget("Tool Library").is_some());
    }
}
