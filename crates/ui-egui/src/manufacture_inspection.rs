//! Inspection → Probing + Manual dialogs.
//! Document writes go through manufacture.settings.set; not companion HTTP or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{
    InspectionKind, InspectionOperation, ManualInspectionItem, ManualInspectionKind, ManualInspectionOperation, ManualItemType, Operation, Tool,
    ToolKind,
};

use crate::SolveApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InspectionTab {
    #[default]
    Tool,
    Geometry,
    Passes,
}

#[derive(Clone, Debug)]
pub struct InspectionDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: InspectionOperation,
    pub tab: InspectionTab,
}

#[derive(Clone, Debug)]
pub struct ManualInspectionDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: ManualInspectionOperation,
}

pub fn open_inspection(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::Inspection(op)) = setup.operations.get(op_index).cloned() else {
        return;
    };
    clear_other(app);
    app.manufacture_workspace.selected_op = Some(op_index);
    app.manufacture_workspace.inspection_dialog =
        Some(InspectionDialogState { setup_name: setup_name.to_string(), op_index, draft: op, tab: InspectionTab::Tool });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Inspection;
    app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Inspection;
}

pub fn open_manual(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::ManualInspection(op)) = setup.operations.get(op_index).cloned() else {
        return;
    };
    clear_other(app);
    app.manufacture_workspace.selected_op = Some(op_index);
    app.manufacture_workspace.manual_inspection_dialog =
        Some(ManualInspectionDialogState { setup_name: setup_name.to_string(), op_index, draft: op });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Inspection;
    app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Inspection;
}

/// Create a probe / inspection op with `kind` on the active setup and open its inspector.
pub fn open_new_inspection(app: &mut SolveApp, kind: InspectionKind) {
    push_op(app, Operation::Inspection(InspectionOperation::with_kind(kind)), |app, name, idx| open_inspection(app, name, idx));
}

/// Create a manual inspection op with `kind` on the active setup and open its inspector.
pub fn open_new_manual(app: &mut SolveApp, kind: ManualInspectionKind) {
    push_op(app, Operation::ManualInspection(ManualInspectionOperation::with_kind(kind)), |app, name, idx| open_manual(app, name, idx));
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
    let op_index = setup.operations.len() - 1;
    match serde_json::to_value(&setup.operations) {
        Ok(ops) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops}})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                open(app, &setup_name, op_index);
            }
            Err(e) => app.set_status(e, true),
        },
        Err(e) => app.set_status(e.to_string(), true),
    }
}

/// Return false when `id` is an inspection command this panel handles.
pub fn open_from_command(app: &mut SolveApp, id: &str) -> bool {
    match id {
        "manufacture.inspection.wcs" => open_new_inspection(app, InspectionKind::ProbeWcs),
        "manufacture.inspection.geometry" => open_new_inspection(app, InspectionKind::ProbeGeometry),
        "manufacture.inspection.surface" => open_new_inspection(app, InspectionKind::InspectSurface),
        "manufacture.inspection.align" => open_new_inspection(app, InspectionKind::PartAlignment),
        "manufacture.inspection.manual" => open_new_manual(app, ManualInspectionKind::Create),
        "manufacture.inspection.record" => open_new_manual(app, ManualInspectionKind::Record),
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
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    show_inspection(app, ui);
    show_manual(app, ui);
}

fn show_inspection(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.inspection_dialog.clone() else {
        return;
    };
    let tools: Vec<Tool> = app.session.doc.manufacture.setups.get(&state.setup_name).map(|s| s.tools.clone()).unwrap_or_default();
    let mut draft = state.draft.clone();
    let mut tab = state.tab;
    let title = draft.kind.label().to_string();
    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_inspection_dialog"), "settings", &title).fields().cards(1.5).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                for (v, l) in [(InspectionTab::Tool, "Tool"), (InspectionTab::Geometry, "Geometry"), (InspectionTab::Passes, "Passes")] {
                    if ui.selectable_label(tab == v, l).clicked() {
                        tab = v;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| match tab {
                InspectionTab::Tool => inspection_tool_tab(ui, &mut draft, &tools),
                InspectionTab::Geometry => inspection_geometry_tab(ui, &mut draft),
                InspectionTab::Passes => inspection_passes_tab(ui, &mut draft),
            });
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;
    if cancel {
        app.manufacture_workspace.inspection_dialog = None;
        return;
    }
    if commit && save_inspection(app, &state.setup_name, state.op_index, draft.clone()) {
        app.manufacture_workspace.inspection_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.inspection_dialog {
        d.draft = draft;
        d.tab = tab;
    }
}

fn inspection_tool_tab(ui: &mut egui::Ui, draft: &mut InspectionOperation, tools: &[Tool]) {
    ui.label(RichText::new(draft.kind.label()).strong());
    ui.horizontal(|ui| {
        ui.label("Probe #");
        let label =
            tools.iter().find(|t| t.number == draft.tool_number).map(|t| t.display_name()).unwrap_or_else(|| format!("T{}", draft.tool_number));
        egui::ComboBox::from_id_salt("insp_probe_tool").selected_text(label).show_ui(ui, |ui| {
            let probes: Vec<_> = tools.iter().filter(|t| t.kind == ToolKind::Probe).collect();
            let list: Vec<&Tool> = if probes.is_empty() { tools.iter().collect() } else { probes };
            for t in list {
                if ui.selectable_label(draft.tool_number == t.number, t.display_name()).clicked() {
                    draft.tool_number = t.number;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Probe cycle");
        ui.text_edit_singleline(&mut draft.probe_cycle);
    });
    ui.checkbox(&mut draft.print_results, "Print results");
}

fn inspection_geometry_tab(ui: &mut egui::Ui, draft: &mut InspectionOperation) {
    ui.horizontal(|ui| {
        ui.label("Kind");
        egui::ComboBox::from_id_salt("insp_kind").selected_text(draft.kind.label()).show_ui(ui, |ui| {
            for k in InspectionKind::ALL {
                if ui.selectable_label(draft.kind == *k, k.label()).clicked() {
                    draft.kind = *k;
                }
            }
        });
    });
    match draft.kind {
        InspectionKind::ProbeWcs => {
            ui.label("Locate stock or machined features and update the setup WCS offset.");
            ui.horizontal(|ui| {
                ui.label("Work offset S");
                ui.add(egui::DragValue::new(&mut draft.work_offset_s).range(0..=6));
            });
            ui.label(RichText::new("1 = G54 … 6 = G59; 0 omits S.").small().weak());
        }
        InspectionKind::ProbeGeometry => {
            ui.horizontal(|ui| {
                ui.label("Feature");
                ui.text_edit_singleline(&mut draft.feature_label);
            });
            drag(ui, "Nominal", &mut draft.nominal_mm, 0.01, 0.0..=10_000.0, " mm");
            drag(ui, "Upper tol", &mut draft.upper_tol_mm, 0.001, 0.0..=10.0, " mm");
            drag(ui, "Lower tol", &mut draft.lower_tol_mm, 0.001, 0.0..=10.0, " mm");
        }
        InspectionKind::InspectSurface => {
            ui.label("Probe path over free-form surfaces; import results after the cycle.");
            drag(ui, "Point spacing", &mut draft.point_spacing_mm, 0.1, 0.1..=100.0, " mm");
            drag(ui, "Upper tol", &mut draft.upper_tol_mm, 0.001, 0.0..=10.0, " mm");
            drag(ui, "Lower tol", &mut draft.lower_tol_mm, 0.001, 0.0..=10.0, " mm");
        }
        InspectionKind::PartAlignment => {
            ui.label("Best-fit alignment from surface inspection to correct fixture misalignment.");
            ui.checkbox(&mut draft.use_best_fit, "Use best-fit alignment");
            drag(ui, "Point spacing", &mut draft.point_spacing_mm, 0.1, 0.1..=100.0, " mm");
        }
    }
}

fn inspection_passes_tab(ui: &mut egui::Ui, draft: &mut InspectionOperation) {
    drag(ui, "Clearance Z", &mut draft.clearance_z_mm, 0.1, 0.0..=500.0, " mm");
    drag(ui, "Overtravel", &mut draft.overtravel_mm, 0.01, 0.0..=50.0, " mm");
    drag(ui, "Protected feed", &mut draft.protected_feed_mmpm, 1.0, 1.0..=5_000.0, " mm/min");
}

fn show_manual(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.manual_inspection_dialog.clone() else {
        return;
    };
    let mut draft = state.draft.clone();
    let title = draft.kind.label().to_string();
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("mfg_manual_inspection_dialog"), "settings", &title).fields().cards(1.5).show(
        ui.ctx(),
        true,
        |ui| {
            ui.label(RichText::new(draft.kind.label()).strong());
            match draft.kind {
                ManualInspectionKind::Create => {
                    ui.label("Plan caliper / micrometer / height-gauge checks.");
                }
                ManualInspectionKind::Record => {
                    ui.label("Record measured values against the inspection plan.");
                }
            }
            ui.separator();
            let mut remove: Option<usize> = None;
            for (i, item) in draft.items.iter_mut().enumerate() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("#{}", i + 1));
                        ui.text_edit_singleline(&mut item.label);
                        if ui.small_button("×").clicked() {
                            remove = Some(i);
                        }
                    });
                    egui::ComboBox::from_id_salt(("manual_type", i)).selected_text(item.item_type.label()).show_ui(ui, |ui| {
                        for t in ManualItemType::ALL {
                            if ui.selectable_label(item.item_type == *t, t.label()).clicked() {
                                item.item_type = *t;
                            }
                        }
                    });
                    if item.item_type == ManualItemType::Distance {
                        drag(ui, "Nominal", &mut item.nominal_mm, 0.01, 0.0..=10_000.0, " mm");
                        drag(ui, "Upper tol", &mut item.upper_tol_mm, 0.001, 0.0..=10.0, " mm");
                        drag(ui, "Lower tol", &mut item.lower_tol_mm, 0.001, 0.0..=10.0, " mm");
                    }
                    if draft.kind == ManualInspectionKind::Record {
                        match item.item_type {
                            ManualItemType::Distance => {
                                let mut m = item.measured_mm.unwrap_or(item.nominal_mm);
                                drag(ui, "Measured", &mut m, 0.01, -10_000.0..=10_000.0, " mm");
                                item.measured_mm = Some(m);
                                let lo = item.nominal_mm - item.lower_tol_mm;
                                let hi = item.nominal_mm + item.upper_tol_mm;
                                item.pass = Some(m >= lo && m <= hi);
                            }
                            ManualItemType::PassFail => {
                                let mut pass = item.pass.unwrap_or(true);
                                ui.checkbox(&mut pass, "Pass");
                                item.pass = Some(pass);
                            }
                            ManualItemType::Text => {}
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Notes");
                        ui.text_edit_singleline(&mut item.notes);
                    });
                });
            }
            if let Some(i) = remove {
                if draft.items.len() > 1 {
                    draft.items.remove(i);
                }
            }
            if ui.button("Add item").clicked() {
                draft.items.push(ManualInspectionItem { label: format!("Item {}", draft.items.len() + 1), ..Default::default() });
            }
        },
    );
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;
    if cancel {
        app.manufacture_workspace.manual_inspection_dialog = None;
        return;
    }
    if commit && save_manual(app, &state.setup_name, state.op_index, draft.clone()) {
        app.manufacture_workspace.manual_inspection_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.manual_inspection_dialog {
        d.draft = draft;
    }
}

fn drag(ui: &mut egui::Ui, label: &str, v: &mut f64, speed: f64, range: std::ops::RangeInclusive<f64>, suffix: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(v).speed(speed).range(range).suffix(suffix));
    });
}

fn save_inspection(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: InspectionOperation) -> bool {
    save_op(app, setup_name, op_index, Operation::Inspection(draft))
}

fn save_manual(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: ManualInspectionOperation) -> bool {
    save_op(app, setup_name, op_index, Operation::ManualInspection(draft))
}

fn save_op(app: &mut SolveApp, setup_name: &str, op_index: usize, op: Operation) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        return false;
    };
    if op_index >= setup.operations.len() {
        return false;
    }
    setup.operations[op_index] = op;
    match serde_json::to_value(&setup.operations) {
        Ok(ops) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops}})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.inspection.saved").to_string(), false);
                true
            }
            Err(e) => {
                app.set_status(e, true);
                false
            }
        },
        Err(e) => {
            app.set_status(e.to_string(), true);
            false
        }
    }
}
