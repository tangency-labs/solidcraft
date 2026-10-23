//! Turning → Part Handling dialogs (Grab / Bar Pull / Return / Tool Call).
//! Document writes go through manufacture.settings.set; not companion HTTP or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{BarPullMode, ChuckOpenMode, Operation, PartHandlingKind, PartHandlingOperation, Tool};

use crate::SolveApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PartHandlingTab {
    #[default]
    Tool,
    Geometry,
    Passes,
}

#[derive(Clone, Debug)]
pub struct PartHandlingDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: PartHandlingOperation,
    pub tab: PartHandlingTab,
}

pub fn open(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::PartHandling(op)) = setup.operations.get(op_index).cloned() else {
        return;
    };
    app.manufacture_workspace.selected_op = Some(op_index);
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
    app.manufacture_workspace.inspection_dialog = None;
    app.manufacture_workspace.manual_inspection_dialog = None;
    app.manufacture_workspace.fabrication_dialog = None;
    app.manufacture_workspace.nest_dialog = None;
    app.manufacture_workspace.part_handling_dialog =
        Some(PartHandlingDialogState { setup_name: setup_name.to_string(), op_index, draft: op, tab: PartHandlingTab::Tool });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::PartHandling;
    app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Turning;
}

/// Create a part-handling op with `kind` on the active setup and open its inspector.
pub fn open_new(app: &mut SolveApp, kind: PartHandlingKind) {
    crate::manufacture_workspace::ensure_setup(app);
    let setup_name = app.manufacture_workspace.active_setup.clone();
    if setup_name.is_empty() {
        return;
    }
    let Some(mut setup) = app.session.doc.manufacture.setups.get(&setup_name).cloned() else {
        return;
    };
    setup.operations.push(Operation::PartHandling(PartHandlingOperation::with_kind(kind)));
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

/// Return false when `id` is a part-handling command this panel handles.
pub fn open_from_command(app: &mut SolveApp, id: &str) -> bool {
    let kind = match id {
        "manufacture.part_handling.grab" => PartHandlingKind::SubspindleGrab,
        "manufacture.part_handling.bar_pull" => PartHandlingKind::BarPull,
        "manufacture.part_handling.return" => PartHandlingKind::SubspindleReturn,
        "manufacture.part_handling.tool_call" => PartHandlingKind::ToolCall,
        _ => return true,
    };
    open_new(app, kind);
    false
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.part_handling_dialog.clone() else {
        return;
    };
    let tools: Vec<Tool> = app.session.doc.manufacture.setups.get(&state.setup_name).map(|s| s.tools.clone()).unwrap_or_default();

    let mut draft = state.draft.clone();
    let mut tab = state.tab;
    let title = draft.kind.label().to_string();

    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_part_handling_dialog"), "settings", &title).fields().cards(1.5).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                for (v, l) in [(PartHandlingTab::Tool, "Tool"), (PartHandlingTab::Geometry, "Geometry"), (PartHandlingTab::Passes, "Passes")] {
                    if ui.selectable_label(tab == v, l).clicked() {
                        tab = v;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| match tab {
                PartHandlingTab::Tool => tool_tab(ui, &mut draft, &tools),
                PartHandlingTab::Geometry => geometry_tab(ui, &mut draft),
                PartHandlingTab::Passes => passes_tab(ui, &mut draft),
            });
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;

    if cancel {
        app.manufacture_workspace.part_handling_dialog = None;
        return;
    }
    if commit && save(app, &state.setup_name, state.op_index, draft.clone()) {
        app.manufacture_workspace.part_handling_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.part_handling_dialog {
        d.draft = draft;
        d.tab = tab;
    }
}

fn tool_tab(ui: &mut egui::Ui, draft: &mut PartHandlingOperation, tools: &[Tool]) {
    ui.label(RichText::new(draft.kind.label()).strong());
    ui.label("Part handling emits machine motion via the post — no cutting toolpath.");
    ui.separator();
    match draft.kind {
        PartHandlingKind::ToolCall => {
            ui.horizontal(|ui| {
                ui.label("Tool #");
                let label = tools
                    .iter()
                    .find(|t| t.number == draft.tool_number)
                    .map(|t| t.display_name())
                    .unwrap_or_else(|| format!("T{}", draft.tool_number));
                egui::ComboBox::from_id_salt("ph_tool").selected_text(label).show_ui(ui, |ui| {
                    for t in tools {
                        if ui.selectable_label(draft.tool_number == t.number, t.display_name()).clicked() {
                            draft.tool_number = t.number;
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Comment");
                ui.text_edit_singleline(&mut draft.comment);
            });
        }
        PartHandlingKind::SubspindleGrab | PartHandlingKind::SubspindleReturn => {
            ui.checkbox(&mut draft.stop_spindle_during_chuck, "Stop spindle during chuck");
            if draft.kind == PartHandlingKind::SubspindleReturn {
                ui.checkbox(&mut draft.stop_spindle_during_return, "Stop spindle during return");
            }
            ui.horizontal(|ui| {
                ui.label("Spindle speed");
                ui.add(egui::DragValue::new(&mut draft.spindle_rpm).speed(10).suffix(" rpm").range(0..=120_000));
            });
            ui.label(RichText::new("0 rpm = leave spindle unchanged").small().weak());
            drag(ui, "Spindle orientation", &mut draft.spindle_orientation_deg, 0.1, -360.0..=360.0, " °");
            ui.checkbox(&mut draft.use_part_catcher, "Use part catcher");
        }
        PartHandlingKind::BarPull => {
            ui.label("Bar pull uses the subspindle to advance stock.");
            ui.checkbox(&mut draft.use_part_catcher, "Use part catcher");
        }
    }
}

fn geometry_tab(ui: &mut egui::Ui, draft: &mut PartHandlingOperation) {
    ui.horizontal(|ui| {
        ui.label("Kind");
        egui::ComboBox::from_id_salt("ph_kind").selected_text(draft.kind.label()).show_ui(ui, |ui| {
            for k in PartHandlingKind::ALL {
                if ui.selectable_label(draft.kind == *k, k.label()).clicked() {
                    draft.kind = *k;
                }
            }
        });
    });
    match draft.kind {
        PartHandlingKind::SubspindleGrab => {
            ui.label(RichText::new("Subspindle planes").strong());
            drag(ui, "Feed Plane", &mut draft.feed_plane_z_mm, 0.1, -10_000.0..=10_000.0, " mm");
            drag(ui, "Chuck Plane", &mut draft.chuck_plane_z_mm, 0.1, -10_000.0..=10_000.0, " mm");
            ui.label(RichText::new("Rapid to Feed Plane, then feed to Chuck Plane before clamp.").small().weak());
        }
        PartHandlingKind::SubspindleReturn => {
            ui.label(RichText::new("Return position").strong());
            drag(ui, "Feed Plane", &mut draft.feed_plane_z_mm, 0.1, -10_000.0..=10_000.0, " mm");
            ui.label(RichText::new("0 mm typically returns the subspindle home (post-dependent).").small().weak());
            ui.separator();
            ui.label(RichText::new("Open Spindle Chucks").strong());
            egui::ComboBox::from_id_salt("ph_chuck_open").selected_text(draft.chuck_open.label()).show_ui(ui, |ui| {
                for m in ChuckOpenMode::ALL {
                    if ui.selectable_label(draft.chuck_open == *m, m.label()).clicked() {
                        draft.chuck_open = *m;
                    }
                }
            });
        }
        PartHandlingKind::BarPull => {
            ui.label(RichText::new("Pull").strong());
            drag(ui, "Pull Distance", &mut draft.pull_distance_mm, 0.1, 0.001..=10_000.0, " mm");
            ui.separator();
            ui.label(RichText::new("Pull Mode").strong());
            egui::ComboBox::from_id_salt("ph_pull_mode").selected_text(draft.pull_mode.label()).show_ui(ui, |ui| {
                for m in [BarPullMode::CompensateToolpaths, BarPullMode::NewWcsOffset] {
                    if ui.selectable_label(draft.pull_mode == m, m.label()).clicked() {
                        draft.pull_mode = m;
                    }
                }
            });
            ui.label(RichText::new("Compensate adds pull distance to later Z; New WCS keeps Z relative to a shifted offset.").small().weak());
        }
        PartHandlingKind::ToolCall => {
            ui.label("Tool Call has no geometry — select the tool on the Tool tab.");
        }
    }
}

fn passes_tab(ui: &mut egui::Ui, draft: &mut PartHandlingOperation) {
    match draft.kind {
        PartHandlingKind::ToolCall => {
            drag(ui, "Dwell", &mut draft.dwell_s, 0.05, 0.0..=60.0, " s");
            ui.horizontal(|ui| {
                ui.label("Comment");
                ui.text_edit_singleline(&mut draft.comment);
            });
        }
        _ => {
            if draft.kind != PartHandlingKind::ToolCall {
                drag(ui, "Feedrate", &mut draft.feedrate_mmpm, 1.0, 0.1..=50_000.0, " mm/min");
            }
            drag(ui, "Dwell", &mut draft.dwell_s, 0.05, 0.0..=60.0, " s");
            if draft.kind == PartHandlingKind::SubspindleGrab {
                ui.label(RichText::new("Feedrate applies from Feed Plane to Chuck Plane.").small().weak());
            }
        }
    }
}

fn drag(ui: &mut egui::Ui, label: &str, v: &mut f64, speed: f64, range: std::ops::RangeInclusive<f64>, suffix: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(v).speed(speed).range(range).suffix(suffix));
    });
}

fn save(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: PartHandlingOperation) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        return false;
    };
    if op_index >= setup.operations.len() {
        return false;
    }
    setup.operations[op_index] = Operation::PartHandling(draft);
    match serde_json::to_value(&setup.operations) {
        Ok(ops) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops}})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.part_handling.saved").to_string(), false);
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
