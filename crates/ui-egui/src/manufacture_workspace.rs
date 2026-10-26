//! Manufacture workspace: process tabs replace the Design ribbon.

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

pub fn close_floating_dialogs(_app: &mut SolveApp) {}

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
        "manufacture.setup.new" | "cam.setup" => {
            enter(app);
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

pub fn tick(_app: &mut SolveApp, _ctx: &egui::Context) {}

pub fn panel(_app: &mut SolveApp, _ui: &mut egui::Ui) {}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::viewport::show(app, ui);
}

pub fn setup_shows_body(_app: &SolveApp, _name: &str) -> bool {
    true
}

pub fn setup_body_key(_app: &SolveApp) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;
    use serde_json::json;

    #[test]
    fn manufacture_uses_process_tabs_not_the_design_ribbon() {
        let mut h = Harness::new();
        enter(&mut h.app);
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
}
