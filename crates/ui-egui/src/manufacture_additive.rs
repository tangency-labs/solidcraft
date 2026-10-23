//! Additive → FFF toolpath and From G-code dialogs.
//! Document writes go through manufacture.settings.set; not companion HTTP or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{AdditiveKind, AdditiveOperation, Operation};

use crate::SolveApp;

#[derive(Clone, Debug)]
pub struct AdditiveDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: AdditiveOperation,
}

#[derive(Clone, Debug, Default)]
pub struct FromGcodeState {
    pub source_path: String,
    pub gcode: String,
    pub setup_name: String,
}

pub fn open(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::Additive(op)) = setup.operations.get(op_index).cloned() else {
        return;
    };
    clear_other(app);
    app.manufacture_workspace.selected_op = Some(op_index);
    app.manufacture_workspace.additive_dialog = Some(AdditiveDialogState { setup_name: setup_name.to_string(), op_index, draft: op });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Additive;
    app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Additive;
}

pub fn open_from_gcode(app: &mut SolveApp) {
    clear_other(app);
    app.manufacture_workspace.from_gcode_dialog = Some(FromGcodeState::default());
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Additive;
    app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Additive;
}

/// Create an additive op with `kind` on the active setup and open its inspector.
pub fn open_new(app: &mut SolveApp, kind: AdditiveKind) {
    push_op(app, Operation::Additive(AdditiveOperation::with_kind(kind)), |app, name, idx| open(app, name, idx));
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
    setup.operation_type = solidcraft_manufacture::SetupOperationType::Additive;
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

/// Return false when `id` is an additive command this panel handles.
pub fn open_from_command(app: &mut SolveApp, id: &str) -> bool {
    match id {
        "manufacture.additive.toolpath" => open_new(app, AdditiveKind::GeneratedToolpath),
        "manufacture.additive.from_gcode" => open_from_gcode(app),
        "manufacture.additive.simulate" => {
            let setup = app.manufacture_workspace.active_setup.clone();
            run_simulate(app, &setup);
        }
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
    show_op_dialog(app, ui);
    show_from_gcode(app, ui);
}

fn show_from_gcode(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.from_gcode_dialog.clone() else {
        return;
    };
    let mut draft = state.clone();
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("mfg_additive_from_gcode"), "settings", "From G-code").fields().cards(1.5).show(
        ui.ctx(),
        true,
        |ui| {
            ui.label("Import an FFF print file to create an Additive setup.");
            ui.horizontal(|ui| {
                ui.label("Setup name");
                ui.text_edit_singleline(&mut draft.setup_name);
            });
            ui.horizontal(|ui| {
                ui.label("Source path");
                ui.text_edit_singleline(&mut draft.source_path);
            });
            ui.label(RichText::new("G-code").strong());
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut draft.gcode).desired_width(f32::INFINITY).desired_rows(12).code_editor());
            });
        },
    );
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;
    if cancel {
        app.manufacture_workspace.from_gcode_dialog = None;
        return;
    }
    if commit {
        import_gcode(app, &draft);
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.from_gcode_dialog {
        *d = draft;
    }
}

fn import_gcode(app: &mut SolveApp, st: &FromGcodeState) {
    crate::manufacture_workspace::ensure_setup(app);
    let setup_name = if st.setup_name.trim().is_empty() { app.manufacture_workspace.active_setup.clone() } else { st.setup_name.trim().to_string() };
    if setup_name.is_empty() {
        return;
    }
    let op = AdditiveOperation::from_gcode(&st.source_path, &st.gcode);
    let mut operations = app.session.doc.manufacture.setups.get(&setup_name).map(|s| s.operations.clone()).unwrap_or_default();
    operations.push(Operation::Additive(op));
    let op_index = operations.len() - 1;
    let ot = solidcraft_manufacture::SetupOperationType::Additive;
    match (serde_json::to_value(&operations), serde_json::to_value(ot)) {
        (Ok(ops), Ok(ot)) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops, "operation_type": ot}}))
        {
            Ok(_) => {
                app.manufacture_workspace.active_setup = setup_name.clone();
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.manufacture_workspace.from_gcode_dialog = None;
                app.set_status(app.tr("manufacture.additive.imported").to_string(), false);
                open(app, &setup_name, op_index);
            }
            Err(e) => app.set_status(e, true),
        },
        (Err(e), _) | (_, Err(e)) => app.set_status(e.to_string(), true),
    }
}

fn show_op_dialog(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.additive_dialog.clone() else {
        return;
    };
    let mut draft = state.draft.clone();
    let title = draft.kind.label();
    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_additive_dialog"), "settings", title).fields().cards(1.5).show(ui.ctx(), true, |ui| {
            ui.label(RichText::new(draft.kind.label()).strong());
            match draft.kind {
                AdditiveKind::ImportedGcode => {
                    if !draft.source_path.is_empty() {
                        ui.label(format!("Source: {}", draft.source_path));
                    }
                    ui.label(format!("G-code bytes: {}", draft.gcode.len()));
                }
                AdditiveKind::GeneratedToolpath => {
                    ui.label("Generated layered FFF toolpath for simulation / post.");
                }
            }
            ui.separator();
            drag(ui, "Layer height", &mut draft.layer_height_mm, 0.01, 0.05..=2.0, " mm");
            ui.horizontal(|ui| {
                ui.label("Layers");
                ui.add(egui::DragValue::new(&mut draft.layer_count).speed(1).range(1..=512));
            });
            drag(ui, "Nozzle diameter", &mut draft.nozzle_diameter_mm, 0.01, 0.1..=2.0, " mm");
            drag(ui, "Build size XY", &mut draft.size_xy_mm, 0.5, 1.0..=500.0, " mm");
            ui.horizontal(|ui| {
                ui.label("Print temp");
                ui.add(egui::DragValue::new(&mut draft.print_temp_c).suffix(" C").range(0..=400));
            });
            ui.horizontal(|ui| {
                ui.label("Bed temp");
                ui.add(egui::DragValue::new(&mut draft.bed_temp_c).suffix(" C").range(0..=200));
            });
            drag(ui, "Print feedrate", &mut draft.feedrate_mmpm, 1.0, 1.0..=30_000.0, " mm/min");
            drag(ui, "Travel feedrate", &mut draft.travel_mmpm, 1.0, 1.0..=50_000.0, " mm/min");
            ui.checkbox(&mut draft.relative_extrusion, "Relative extrusion mode");
            ui.separator();
            if ui.button("Simulate Additive Toolpath").clicked() {
                run_simulate(app, &state.setup_name);
            }
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;
    if cancel {
        app.manufacture_workspace.additive_dialog = None;
        return;
    }
    if commit && save(app, &state.setup_name, state.op_index, draft.clone()) {
        app.manufacture_workspace.additive_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.additive_dialog {
        d.draft = draft;
    }
}

pub fn run_simulate(app: &mut SolveApp, setup_name: &str) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        app.set_status(app.tr("manufacture.additive.simulate_empty").to_string(), true);
        return;
    };
    let Some(op) = setup.operations.iter().find_map(|op| match op {
        Operation::Additive(o) => Some(o),
        _ => None,
    }) else {
        app.set_status(app.tr("manufacture.additive.simulate_empty").to_string(), true);
        return;
    };
    let segments = op.simulation_segments();
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Additive;
    app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Additive;
    app.set_status(format!("{} ({} layers, {} segments)", app.tr("manufacture.additive.simulated"), op.layer_count, segments.len()), false);
}

fn drag(ui: &mut egui::Ui, label: &str, v: &mut f64, speed: f64, range: std::ops::RangeInclusive<f64>, suffix: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(v).speed(speed).range(range).suffix(suffix));
    });
}

fn save(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: AdditiveOperation) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        return false;
    };
    if op_index >= setup.operations.len() {
        return false;
    }
    setup.operations[op_index] = Operation::Additive(draft);
    setup.operation_type = solidcraft_manufacture::SetupOperationType::Additive;
    match (serde_json::to_value(&setup.operations), serde_json::to_value(setup.operation_type)) {
        (Ok(ops), Ok(ot)) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops, "operation_type": ot}}))
        {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.additive.saved").to_string(), false);
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
