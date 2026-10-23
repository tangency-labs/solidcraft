//! Turning dialogs (Tool / Geometry / Radii / Passes / Linking).
//! Document writes go through manufacture.settings.set; not companion HTTP or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{Coolant, HighFeedMode, Operation, Tool, TurningMode, TurningOperation, TurningSpindleRotation, TurningStrategy};

use crate::SolveApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TurningTab {
    #[default]
    Tool,
    Geometry,
    Radii,
    Passes,
    Linking,
}

#[derive(Clone, Debug)]
pub struct TurningDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: TurningOperation,
    pub tab: TurningTab,
}

pub fn open(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::Turning(op)) = setup.operations.get(op_index).cloned() else {
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
    app.manufacture_workspace.part_handling_dialog = None;
    app.manufacture_workspace.inspection_dialog = None;
    app.manufacture_workspace.manual_inspection_dialog = None;
    app.manufacture_workspace.fabrication_dialog = None;
    app.manufacture_workspace.nest_dialog = None;
    app.manufacture_workspace.additive_dialog = None;
    app.manufacture_workspace.from_gcode_dialog = None;
    app.manufacture_workspace.turning_dialog =
        Some(TurningDialogState { setup_name: setup_name.to_string(), op_index, draft: op, tab: TurningTab::Tool });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Turning;
    app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Turning;
}

/// Create a turning op with `strategy` on the active setup and open its inspector.
pub fn open_new(app: &mut SolveApp, strategy: TurningStrategy) {
    crate::manufacture_workspace::ensure_setup(app);
    let setup_name = app.manufacture_workspace.active_setup.clone();
    if setup_name.is_empty() {
        return;
    }
    let Some(mut setup) = app.session.doc.manufacture.setups.get(&setup_name).cloned() else {
        return;
    };
    setup.operations.push(Operation::Turning(TurningOperation::with_strategy(strategy)));
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

/// Return false when `id` is a turning command this panel handles.
pub fn open_from_command(app: &mut SolveApp, id: &str) -> bool {
    let strategy = match id {
        "manufacture.turning.face" => TurningStrategy::Face,
        "manufacture.turning.profile" => TurningStrategy::ProfileRoughing,
        "manufacture.turning.groove" => TurningStrategy::Groove,
        "manufacture.turning.thread" => TurningStrategy::Thread,
        "manufacture.turning.chamfer" => TurningStrategy::Chamfer,
        "manufacture.turning.part" => TurningStrategy::Part,
        _ => return true,
    };
    open_new(app, strategy);
    false
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.turning_dialog.clone() else {
        return;
    };
    let tools: Vec<Tool> = app.session.doc.manufacture.setups.get(&state.setup_name).map(|s| s.tools.clone()).unwrap_or_default();

    let mut draft = state.draft.clone();
    let mut tab = state.tab;
    let title = draft.strategy.label().to_string();

    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_turning_dialog"), "settings", &title).fields().cards(1.5).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                for (v, l) in [
                    (TurningTab::Tool, "Tool"),
                    (TurningTab::Geometry, "Geometry"),
                    (TurningTab::Radii, "Radii"),
                    (TurningTab::Passes, "Passes"),
                    (TurningTab::Linking, "Linking"),
                ] {
                    if ui.selectable_label(tab == v, l).clicked() {
                        tab = v;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| match tab {
                TurningTab::Tool => tool_tab(ui, &mut draft, &tools),
                TurningTab::Geometry => geometry_tab(ui, &mut draft),
                TurningTab::Radii => radii_tab(ui, &mut draft),
                TurningTab::Passes => passes_tab(ui, &mut draft),
                TurningTab::Linking => linking_tab(ui, &mut draft),
            });
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;

    if cancel {
        app.manufacture_workspace.turning_dialog = None;
        return;
    }
    if commit && save(app, &state.setup_name, state.op_index, draft.clone()) {
        app.manufacture_workspace.turning_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.turning_dialog {
        d.draft = draft;
        d.tab = tab;
    }
}

fn tool_tab(ui: &mut egui::Ui, draft: &mut TurningOperation, tools: &[Tool]) {
    ui.label(RichText::new(draft.strategy.label()).strong());
    ui.horizontal(|ui| {
        ui.label("Tool #");
        let label =
            tools.iter().find(|t| t.number == draft.tool_number).map(|t| t.display_name()).unwrap_or_else(|| format!("T{}", draft.tool_number));
        egui::ComboBox::from_id_salt("turn_tool").selected_text(label).show_ui(ui, |ui| {
            for t in tools {
                if ui.selectable_label(draft.tool_number == t.number, t.display_name()).clicked() {
                    draft.tool_number = t.number;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Spindle");
        egui::ComboBox::from_id_salt("turn_spindle").selected_text(draft.spindle.label()).show_ui(ui, |ui| {
            for s in [TurningSpindleRotation::Forward, TurningSpindleRotation::Reverse] {
                if ui.selectable_label(draft.spindle == s, s.label()).clicked() {
                    draft.spindle = s;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Coolant");
        egui::ComboBox::from_id_salt("turn_coolant").selected_text(draft.coolant.label()).show_ui(ui, |ui| {
            for c in Coolant::ALL {
                if ui.selectable_label(draft.coolant == *c, c.label()).clicked() {
                    draft.coolant = *c;
                }
            }
        });
    });
    ui.separator();
    ui.label(RichText::new("Feeds").strong());
    drag(ui, "Cutting feedrate", &mut draft.feeds.feedrate, 1.0, 0.1..=50_000.0, " mm/min");
    drag(ui, "Plunge feedrate", &mut draft.feeds.plunge, 1.0, 0.1..=50_000.0, " mm/min");
    ui.horizontal(|ui| {
        ui.label("Spindle speed");
        ui.add(egui::DragValue::new(&mut draft.feeds.spindle_rpm).speed(10).suffix(" rpm").range(1..=120_000));
    });
}

fn geometry_tab(ui: &mut egui::Ui, draft: &mut TurningOperation) {
    ui.horizontal(|ui| {
        ui.label("Strategy");
        egui::ComboBox::from_id_salt("turn_strategy").selected_text(draft.strategy.label()).show_ui(ui, |ui| {
            for s in TurningStrategy::ALL {
                if ui.selectable_label(draft.strategy == *s, s.label()).clicked() {
                    draft.strategy = *s;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Mode");
        egui::ComboBox::from_id_salt("turn_mode").selected_text(draft.mode.label()).show_ui(ui, |ui| {
            for m in [TurningMode::Outside, TurningMode::Inside] {
                if ui.selectable_label(draft.mode == m, m.label()).clicked() {
                    draft.mode = m;
                }
            }
        });
    });
    ui.separator();
    ui.label(RichText::new("Z containment").strong());
    drag(ui, "Front", &mut draft.bounds.front_z_mm, 0.1, -10_000.0..=10_000.0, " mm");
    drag(ui, "Front offset", &mut draft.bounds.front_offset_mm, 0.1, -1_000.0..=1_000.0, " mm");
    drag(ui, "Back", &mut draft.bounds.back_z_mm, 0.1, -10_000.0..=10_000.0, " mm");
    drag(ui, "Back offset", &mut draft.bounds.back_offset_mm, 0.1, -1_000.0..=1_000.0, " mm");
    ui.separator();
    ui.label("Name");
    ui.text_edit_singleline(&mut draft.name);
    if draft.strategy.is_groove() || matches!(draft.strategy, TurningStrategy::ProfileFinishing) {
        ui.checkbox(&mut draft.machine_undercuts, "Machine undercuts");
    }
    ui.checkbox(&mut draft.make_sharp_corners, "Make sharp corners");
}

fn radii_tab(ui: &mut egui::Ui, draft: &mut TurningOperation) {
    ui.label(RichText::new("X radii (diameter / 2)").strong());
    drag(ui, "Outer radius", &mut draft.radii.outer_radius_mm, 0.1, 0.0..=10_000.0, " mm");
    drag(ui, "Inner radius", &mut draft.radii.inner_radius_mm, 0.1, 0.0..=10_000.0, " mm");
    drag(ui, "Clearance", &mut draft.radii.clearance_radius_mm, 0.1, 0.0..=10_000.0, " mm");
    drag(ui, "Retract", &mut draft.radii.retract_radius_mm, 0.1, 0.0..=10_000.0, " mm");
}

fn passes_tab(ui: &mut egui::Ui, draft: &mut TurningOperation) {
    drag(ui, "Depth of cut", &mut draft.depth_mm, 0.01, 0.01..=1_000.0, " mm");
    if draft.strategy.is_roughing() || matches!(draft.strategy, TurningStrategy::Face | TurningStrategy::Groove | TurningStrategy::SingleGroove) {
        drag(ui, "Max stepdown", &mut draft.max_stepdown_mm, 0.01, 0.01..=100.0, " mm");
    }
    drag(ui, "X stock to leave", &mut draft.x_stock_mm, 0.01, 0.0..=50.0, " mm");
    drag(ui, "Z stock to leave", &mut draft.z_stock_mm, 0.01, 0.0..=50.0, " mm");
    if draft.strategy == TurningStrategy::Thread {
        drag(ui, "Thread pitch", &mut draft.thread_pitch_mm, 0.01, 0.1..=20.0, " mm");
    }
    if draft.strategy == TurningStrategy::Chamfer {
        drag(ui, "Chamfer width", &mut draft.chamfer_width_mm, 0.01, 0.01..=50.0, " mm");
    }
    if draft.strategy == TurningStrategy::Part {
        drag(ui, "Peck depth", &mut draft.peck_depth_mm, 0.01, 0.01..=50.0, " mm");
        drag(ui, "Past centerline", &mut draft.past_center_mm, 0.01, 0.0..=20.0, " mm");
        ui.checkbox(&mut draft.use_parts_catcher, "Enable parts catcher");
    }
}

fn linking_tab(ui: &mut egui::Ui, draft: &mut TurningOperation) {
    drag(ui, "Safe distance", &mut draft.linking.safe_distance_mm, 0.1, 0.0..=500.0, " mm");
    drag(ui, "Max stay-down", &mut draft.linking.max_stay_down_mm, 0.1, 0.0..=500.0, " mm");
    ui.checkbox(&mut draft.linking.keep_tool_down, "Keep tool down");
    ui.checkbox(&mut draft.linking.allow_rapid_retract, "Allow rapid retract");
    ui.horizontal(|ui| {
        ui.label("High feed");
        egui::ComboBox::from_id_salt("turn_hfeed").selected_text(draft.linking.high_feed_mode.label()).show_ui(ui, |ui| {
            for m in [HighFeedMode::PreserveRapid, HighFeedMode::AlwaysUseHighFeed, HighFeedMode::NeverUseHighFeed] {
                if ui.selectable_label(draft.linking.high_feed_mode == m, m.label()).clicked() {
                    draft.linking.high_feed_mode = m;
                }
            }
        });
    });
}

fn drag(ui: &mut egui::Ui, label: &str, v: &mut f64, speed: f64, range: std::ops::RangeInclusive<f64>, suffix: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(v).speed(speed).range(range).suffix(suffix));
    });
}

fn save(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: TurningOperation) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        return false;
    };
    if op_index >= setup.operations.len() {
        return false;
    }
    setup.operations[op_index] = Operation::Turning(draft);
    match serde_json::to_value(&setup.operations) {
        Ok(ops) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops}})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.turning.saved").to_string(), false);
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
