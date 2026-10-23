//! Fabrication → Nest + 2D Profile (waterjet / laser / plasma).
//! Document writes go through manufacture.settings.set; not companion HTTP or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{
    FabricationCompensationType, FabricationCuttingMode, FabricationProfileOperation, FabricationQualityControl, FabricationSidewaysCompensation,
    NestSettings, Operation, Tool,
};

use crate::SolveApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FabTab {
    #[default]
    Tool,
    Geometry,
    Passes,
    Linking,
}

#[derive(Clone, Debug)]
pub struct FabricationDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: FabricationProfileOperation,
    pub tab: FabTab,
}

#[derive(Clone, Debug)]
pub struct NestDialogState {
    pub draft: NestSettings,
}

pub fn open(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::FabricationProfile(op)) = setup.operations.get(op_index).cloned() else {
        return;
    };
    clear_other(app);
    app.manufacture_workspace.selected_op = Some(op_index);
    app.manufacture_workspace.fabrication_dialog =
        Some(FabricationDialogState { setup_name: setup_name.to_string(), op_index, draft: op, tab: FabTab::Tool });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Fabrication;
    app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Fabrication;
}

pub fn open_nest(app: &mut SolveApp) {
    clear_other(app);
    app.manufacture_workspace.nest_dialog = Some(NestDialogState { draft: NestSettings::default() });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Fabrication;
    app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Fabrication;
}

/// Create a 2D Profile fabrication op on the active setup and open its inspector.
pub fn open_new(app: &mut SolveApp) {
    push_op(app, Operation::FabricationProfile(FabricationProfileOperation::default()), |app, name, idx| open(app, name, idx));
}

fn push_op(app: &mut SolveApp, op: Operation, open: impl FnOnce(&mut SolveApp, &str, usize)) {
    crate::manufacture_workspace::ensure_setup(app);
    let setup_name = app.manufacture_workspace.active_setup.clone();
    if setup_name.is_empty() {
        return;
    }
    let Some(mut setup) = app.session.doc.manufacture.setups.get(&setup_name).cloned() else {
        return;
    };
    setup.operations.push(op);
    setup.operation_type = solidcraft_manufacture::SetupOperationType::Cutting;
    let op_index = setup.operations.len() - 1;
    match (serde_json::to_value(&setup.operations), serde_json::to_value(setup.operation_type)) {
        (Ok(ops), Ok(ot)) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops, "operation_type": ot}}))
        {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                open(app, &setup_name, op_index);
            }
            Err(e) => app.set_status(e, true),
        },
        (Err(e), _) | (_, Err(e)) => app.set_status(e.to_string(), true),
    }
}

/// Return false when `id` is a fabrication command this panel handles.
pub fn open_from_command(app: &mut SolveApp, id: &str) -> bool {
    match id {
        "manufacture.fabrication.profile" => open_new(app),
        "manufacture.fabrication.nest" => open_nest(app),
        _ => return true,
    }
    false
}

fn clear_other(app: &mut SolveApp) {
    app.manufacture_workspace.tool_library_dialog = None;
    app.manufacture_workspace.edit_tool_dialog = None;
    app.manufacture_workspace.manage_stub_dialog = None;
    app.manufacture_workspace.setup_dialog = None;
    app.manufacture_workspace.drill_dialog = None;
    app.manufacture_workspace.contour_dialog = None;
    app.manufacture_workspace.twod_dialog = None;
    app.manufacture_workspace.threed_dialog = None;
    app.manufacture_workspace.hole_rec_dialog = None;
    app.manufacture_workspace.multiaxis_dialog = None;
    app.manufacture_workspace.turning_dialog = None;
    app.manufacture_workspace.part_handling_dialog = None;
    app.manufacture_workspace.inspection_dialog = None;
    app.manufacture_workspace.manual_inspection_dialog = None;
    app.manufacture_workspace.fabrication_dialog = None;
    app.manufacture_workspace.nest_dialog = None;
    app.manufacture_workspace.additive_dialog = None;
    app.manufacture_workspace.from_gcode_dialog = None;
    app.manufacture_workspace.modify_dialog = None;
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    show_profile(app, ui);
    show_nest(app, ui);
}

fn show_nest(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.nest_dialog.clone() else {
        return;
    };
    let mut draft = state.draft.clone();
    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_nest_dialog"), "settings", "Nest").fields().cards(1.5).show(ui.ctx(), true, |ui| {
            ui.label("Arrange parts on a sheet for waterjet / laser / plasma cutting.");
            ui.separator();
            drag(ui, "Sheet width", &mut draft.sheet_width_mm, 1.0, 10.0..=10_000.0, " mm");
            drag(ui, "Sheet height", &mut draft.sheet_height_mm, 1.0, 10.0..=10_000.0, " mm");
            drag(ui, "Part spacing", &mut draft.part_spacing_mm, 0.1, 0.0..=100.0, " mm");
            drag(ui, "Sheet margin", &mut draft.sheet_margin_mm, 0.1, 0.0..=100.0, " mm");
            ui.checkbox(&mut draft.rotate_parts, "Allow part rotation");
            ui.separator();
            ui.label(RichText::new("Nest packing is a planning step. Apply settings, then generate 2D Profile cuts.").small().weak());
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;
    if cancel {
        app.manufacture_workspace.nest_dialog = None;
        return;
    }
    if commit {
        app.set_status(app.tr("manufacture.fabrication.nest_saved").to_string(), false);
        app.manufacture_workspace.nest_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.nest_dialog {
        d.draft = draft;
    }
}

fn show_profile(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.fabrication_dialog.clone() else {
        return;
    };
    let tools: Vec<Tool> = app.session.doc.manufacture.setups.get(&state.setup_name).map(|s| s.tools.clone()).unwrap_or_default();
    let mut draft = state.draft.clone();
    let mut tab = state.tab;
    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_fab_2d_profile"), "settings", "2D Profile").fields().cards(1.5).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                for (v, l) in [(FabTab::Tool, "Tool"), (FabTab::Geometry, "Geometry"), (FabTab::Passes, "Passes"), (FabTab::Linking, "Linking")] {
                    if ui.selectable_label(tab == v, l).clicked() {
                        tab = v;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| match tab {
                FabTab::Tool => tool_tab(ui, &mut draft, &tools),
                FabTab::Geometry => geometry_tab(ui, &mut draft),
                FabTab::Passes => passes_tab(ui, &mut draft),
                FabTab::Linking => linking_tab(ui, &mut draft),
            });
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;
    if cancel {
        app.manufacture_workspace.fabrication_dialog = None;
        return;
    }
    if commit && save(app, &state.setup_name, state.op_index, draft.clone()) {
        app.manufacture_workspace.fabrication_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.fabrication_dialog {
        d.draft = draft;
        d.tab = tab;
    }
}

fn tool_tab(ui: &mut egui::Ui, draft: &mut FabricationProfileOperation, tools: &[Tool]) {
    ui.label(RichText::new("2D Profile — waterjet / laser / plasma").strong());
    ui.horizontal(|ui| {
        ui.label("Tool #");
        let label =
            tools.iter().find(|t| t.number == draft.tool_number).map(|t| t.display_name()).unwrap_or_else(|| format!("T{}", draft.tool_number));
        egui::ComboBox::from_id_salt("fab_tool").selected_text(label).show_ui(ui, |ui| {
            let nozzles: Vec<_> = tools.iter().filter(|t| t.kind.is_cutting_nozzle()).collect();
            let list: Vec<&Tool> = if nozzles.is_empty() { tools.iter().collect() } else { nozzles };
            for t in list {
                if ui.selectable_label(draft.tool_number == t.number, t.display_name()).clicked() {
                    draft.tool_number = t.number;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Cutting mode");
        egui::ComboBox::from_id_salt("fab_cut_mode").selected_text(draft.cutting_mode.label()).show_ui(ui, |ui| {
            for m in FabricationCuttingMode::ALL {
                if ui.selectable_label(draft.cutting_mode == *m, m.label()).clicked() {
                    draft.cutting_mode = *m;
                    if *m == FabricationCuttingMode::Etch {
                        draft.sideways = FabricationSidewaysCompensation::Center;
                    }
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Quality control");
        egui::ComboBox::from_id_salt("fab_quality").selected_text(draft.quality_control.label()).show_ui(ui, |ui| {
            for q in [FabricationQualityControl::MachineUsesFeeds, FabricationQualityControl::MachineUsesQuality] {
                if ui.selectable_label(draft.quality_control == q, q.label()).clicked() {
                    draft.quality_control = q;
                }
            }
        });
    });
    ui.separator();
    drag(ui, "Cutting feedrate", &mut draft.feeds.feedrate, 1.0, 0.1..=50_000.0, " mm/min");
    drag(ui, "Lead-in feedrate", &mut draft.lead_in_feedrate, 1.0, 0.1..=50_000.0, " mm/min");
    drag(ui, "Lead-out feedrate", &mut draft.lead_out_feedrate, 1.0, 0.1..=50_000.0, " mm/min");
}

fn geometry_tab(ui: &mut egui::Ui, draft: &mut FabricationProfileOperation) {
    ui.label("Select contours / faces / sketches for the cut path (Contour Selection).");
    drag(ui, "Thickness", &mut draft.thickness_mm, 0.1, 0.01..=200.0, " mm");
    ui.checkbox(&mut draft.use_tabs, "Tabs (hold part in sheet)");
    if draft.use_tabs {
        drag(ui, "Tab width", &mut draft.tab_width_mm, 0.1, 0.1..=50.0, " mm");
        drag(ui, "Tab distance", &mut draft.tab_distance_mm, 0.5, 1.0..=1_000.0, " mm");
    }
    ui.label(
        RichText::new(format!("{} contour sample point(s)", draft.targets.face_sample_points.len() + draft.targets.drill_points.len()))
            .small()
            .weak(),
    );
}

fn passes_tab(ui: &mut egui::Ui, draft: &mut FabricationProfileOperation) {
    drag(ui, "Tolerance", &mut draft.tolerance_mm, 0.001, 0.001..=10.0, " mm");
    drag(ui, "Finishing overlap", &mut draft.finishing_overlap_mm, 0.1, 0.0..=100.0, " mm");
    ui.horizontal(|ui| {
        ui.label("Sideways compensation");
        egui::ComboBox::from_id_salt("fab_sideways").selected_text(draft.sideways.label()).show_ui(ui, |ui| {
            for s in [FabricationSidewaysCompensation::Left, FabricationSidewaysCompensation::Right, FabricationSidewaysCompensation::Center] {
                if ui.selectable_label(draft.sideways == s, s.label()).clicked() {
                    draft.sideways = s;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Compensation type");
        egui::ComboBox::from_id_salt("fab_comp").selected_text(draft.compensation.label()).show_ui(ui, |ui| {
            for c in [FabricationCompensationType::InComputer, FabricationCompensationType::InControl] {
                if ui.selectable_label(draft.compensation == c, c.label()).clicked() {
                    draft.compensation = c;
                }
            }
        });
    });
}

fn linking_tab(ui: &mut egui::Ui, draft: &mut FabricationProfileOperation) {
    ui.checkbox(&mut draft.keep_nozzle_down, "Keep nozzle down");
    if draft.keep_nozzle_down {
        drag(ui, "Max stay-down distance", &mut draft.max_stay_down_mm, 1.0, 0.0..=10_000.0, " mm");
    }
    ui.separator();
    ui.label(RichText::new("Leads / pierce").strong());
    drag(ui, "Pierce clearance", &mut draft.pierce_clearance_mm, 0.1, 0.0..=100.0, " mm");
    drag(ui, "Lead-in radius", &mut draft.lead_in_radius_mm, 0.1, 0.0..=100.0, " mm");
    drag(ui, "Lead-in sweep", &mut draft.lead_in_sweep_deg, 1.0, 0.0..=360.0, " °");
    drag(ui, "Linear lead-in", &mut draft.linear_lead_in_mm, 0.1, 0.0..=100.0, " mm");
    drag(ui, "Safe distance", &mut draft.linking.safe_distance_mm, 0.1, 0.0..=500.0, " mm");
}

fn drag(ui: &mut egui::Ui, label: &str, v: &mut f64, speed: f64, range: std::ops::RangeInclusive<f64>, suffix: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(v).speed(speed).range(range).suffix(suffix));
    });
}

fn save(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: FabricationProfileOperation) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        return false;
    };
    if op_index >= setup.operations.len() {
        return false;
    }
    setup.operations[op_index] = Operation::FabricationProfile(draft);
    setup.operation_type = solidcraft_manufacture::SetupOperationType::Cutting;
    match (serde_json::to_value(&setup.operations), serde_json::to_value(setup.operation_type)) {
        (Ok(ops), Ok(ot)) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops, "operation_type": ot}}))
        {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.fabrication.saved").to_string(), false);
                true
            }
            Err(e) => {
                app.set_status(e, true);
                false
            }
        },
        (Err(e), _) | (_, Err(e)) => {
            app.set_status(e.to_string(), true);
            false
        }
    }
}
