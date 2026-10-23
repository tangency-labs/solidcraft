//! Multi-Axis milling dialogs (Tool / Geometry / Heights / Passes / Multi-Axis / Linking).
//! Document writes go through manufacture.settings.set; not companion HTTP or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{
    AdvSwarfGeometryMode, ContourHeights, ContourLinking, Coolant, HeightFrom, HeightPlane, HighFeedMode, MultiAxisFinishPattern,
    MultiAxisMachiningType, MultiAxisOperation, MultiAxisStrategy, Operation, RampType, RotaryAxisOrientation, RotaryPassStyle, SidewaysCompensation,
    SwarfDriveMode, Tool,
};

use crate::{SolveApp, viewport};

const HEIGHT_FROM: &[HeightFrom] = &[
    HeightFrom::ClearanceHeight,
    HeightFrom::RetractHeight,
    HeightFrom::FeedHeight,
    HeightFrom::StockTop,
    HeightFrom::TopHeight,
    HeightFrom::SelectedContours,
    HeightFrom::ModelTop,
    HeightFrom::ModelBottom,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MultiAxisTab {
    #[default]
    Tool,
    Geometry,
    Heights,
    Passes,
    MultiAxis,
    Linking,
}

#[derive(Clone, Debug)]
pub struct MultiAxisDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: MultiAxisOperation,
    pub tab: MultiAxisTab,
}

pub fn open(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::MultiAxis(op)) = setup.operations.get(op_index).cloned() else {
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
    app.manufacture_workspace.turning_dialog = None;
    app.manufacture_workspace.part_handling_dialog = None;
    app.manufacture_workspace.inspection_dialog = None;
    app.manufacture_workspace.manual_inspection_dialog = None;
    app.manufacture_workspace.fabrication_dialog = None;
    app.manufacture_workspace.nest_dialog = None;
    app.manufacture_workspace.multiaxis_dialog =
        Some(MultiAxisDialogState { setup_name: setup_name.to_string(), op_index, draft: op, tab: MultiAxisTab::Tool });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::MultiAxis;
}

/// Create a multi-axis op with `strategy` on the active setup and open its inspector.
pub fn open_new(app: &mut SolveApp, strategy: MultiAxisStrategy) {
    crate::manufacture_workspace::ensure_setup(app);
    let setup_name = app.manufacture_workspace.active_setup.clone();
    if setup_name.is_empty() {
        return;
    }
    let Some(mut setup) = app.session.doc.manufacture.setups.get(&setup_name).cloned() else {
        return;
    };
    setup.operations.push(Operation::MultiAxis(MultiAxisOperation::with_strategy(strategy)));
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

/// Return false when `id` is a multi-axis command this panel handles.
pub fn open_from_command(app: &mut SolveApp, id: &str) -> bool {
    let strategy = match id {
        "manufacture.multiaxis.swarf" => MultiAxisStrategy::Swarf,
        "manufacture.multiaxis.rough" => MultiAxisStrategy::MultiAxisRoughing,
        "manufacture.multiaxis.rotary" => MultiAxisStrategy::RotaryParallel,
        "manufacture.multiaxis.finish" => MultiAxisStrategy::MultiAxisFinishing,
        "manufacture.multiaxis.contour" => MultiAxisStrategy::MultiAxisContour,
        "manufacture.multiaxis.adv_swarf" => MultiAxisStrategy::AdvancedSwarf,
        _ => return true,
    };
    open_new(app, strategy);
    false
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.multiaxis_dialog.clone() else {
        return;
    };
    let tools: Vec<Tool> = app.session.doc.manufacture.setups.get(&state.setup_name).map(|s| s.tools.clone()).unwrap_or_default();

    let mut draft = state.draft.clone();
    let mut tab = state.tab;
    let title = dialog_title(&draft);

    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_multiaxis_dialog"), "settings", &title).fields().cards(1.5).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                for (v, l) in [
                    (MultiAxisTab::Tool, "Tool"),
                    (MultiAxisTab::Geometry, "Geometry"),
                    (MultiAxisTab::Heights, "Heights"),
                    (MultiAxisTab::Passes, "Passes"),
                    (MultiAxisTab::MultiAxis, "Multi-Axis"),
                    (MultiAxisTab::Linking, "Linking"),
                ] {
                    let sel = tab == v;
                    if ui.selectable_label(sel, if sel { RichText::new(l).strong() } else { RichText::new(l) }).clicked() {
                        tab = v;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| match tab {
                MultiAxisTab::Tool => tool_tab(ui, &mut draft, &tools),
                MultiAxisTab::Geometry => geometry_tab(ui, &mut draft),
                MultiAxisTab::Heights => heights_tab(ui, &mut draft.heights, &mut draft.depth),
                MultiAxisTab::Passes => passes_tab(ui, &mut draft),
                MultiAxisTab::MultiAxis => multiaxis_tab(ui, &mut draft),
                MultiAxisTab::Linking => linking_tab(ui, &mut draft.linking),
            });
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;

    if cancel {
        app.manufacture_workspace.multiaxis_dialog = None;
        return;
    }
    if commit && save(app, &state.setup_name, state.op_index, draft.clone()) {
        app.manufacture_workspace.multiaxis_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.multiaxis_dialog {
        d.draft = draft;
        d.tab = tab;
    }
}

fn dialog_title(draft: &MultiAxisOperation) -> String {
    let kind = draft.strategy.label().to_uppercase();
    if draft.name.is_empty() || draft.name == draft.strategy.label() { kind } else { format!("{kind} : {}", draft.name.to_uppercase()) }
}

fn tool_tab(ui: &mut egui::Ui, draft: &mut MultiAxisOperation, tools: &[Tool]) {
    ui.heading("Tool");
    let tnum = draft.tool_number;
    let label = tools.iter().find(|t| t.number == tnum).map(|t| format!("#{} ø{:.2}", t.number, t.diameter)).unwrap_or_else(|| format!("#{tnum}"));
    egui::ComboBox::from_id_salt("ma_tool").selected_text(label).show_ui(ui, |ui| {
        for t in tools {
            if ui.selectable_label(tnum == t.number, format!("#{} ø{:.2}", t.number, t.diameter)).clicked() {
                draft.tool_number = t.number;
            }
        }
    });
    if draft.strategy.is_swarf() {
        ui.label(RichText::new("Prefer flat / bull / ball / tapered mills with flute length covering the face.").weak());
    }
    if matches!(draft.strategy, MultiAxisStrategy::RotaryParallel | MultiAxisStrategy::RotaryContour | MultiAxisStrategy::MultiAxisFinishing) {
        ui.label(RichText::new("Ball or circle-segment tools are typical for rotary / finishing.").weak());
    }
    ui.separator();
    ui.heading("Feed & Speed");
    ui.horizontal(|ui| {
        ui.label("Spindle");
        let mut rpm = draft.feeds.spindle_rpm;
        if ui.add(egui::DragValue::new(&mut rpm).speed(100).range(1..=60_000).suffix(" rpm")).changed() {
            draft.feeds.spindle_rpm = rpm;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Cutting Feedrate");
        ui.add(egui::DragValue::new(&mut draft.feeds.feedrate).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
    });
    ui.horizontal(|ui| {
        ui.label("Plunge Feedrate");
        ui.add(egui::DragValue::new(&mut draft.feeds.plunge).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
    });
    let coolant = draft.coolant;
    ui.horizontal(|ui| {
        ui.label("Coolant");
        egui::ComboBox::from_id_salt("ma_coolant").selected_text(coolant.label()).show_ui(ui, |ui| {
            for c in [Coolant::None, Coolant::Flood, Coolant::Mist, Coolant::Air, Coolant::ThroughSpindle] {
                if ui.selectable_label(coolant == c, c.label()).clicked() {
                    draft.coolant = c;
                }
            }
        });
    });
}

const MULTIAXIS_STRATEGIES: &[MultiAxisStrategy] = &[
    MultiAxisStrategy::RotaryOffsetRoughing,
    MultiAxisStrategy::MultiAxisRoughing,
    MultiAxisStrategy::Swarf,
    MultiAxisStrategy::AdvancedSwarf,
    MultiAxisStrategy::MultiAxisContour,
    MultiAxisStrategy::RotaryParallel,
    MultiAxisStrategy::RotaryContour,
    MultiAxisStrategy::MultiAxisFinishing,
];

fn geometry_tab(ui: &mut egui::Ui, draft: &mut MultiAxisOperation) {
    ui.heading("Geometry");
    ui.horizontal(|ui| {
        ui.label("Strategy");
        egui::ComboBox::from_id_salt("ma_strategy").selected_text(draft.strategy.label()).show_ui(ui, |ui| {
            for s in MULTIAXIS_STRATEGIES {
                if ui.selectable_label(draft.strategy == *s, s.label()).clicked() {
                    draft.strategy = *s;
                }
            }
        });
    });

    if draft.strategy.is_rotary() {
        ui.horizontal(|ui| {
            ui.label("Rotary Orientation");
            egui::ComboBox::from_id_salt("ma_orient").selected_text(draft.rotary_orientation.label()).show_ui(ui, |ui| {
                for o in
                    [RotaryAxisOrientation::SetupX, RotaryAxisOrientation::SetupY, RotaryAxisOrientation::SetupZ, RotaryAxisOrientation::Selection]
                {
                    if ui.selectable_label(draft.rotary_orientation == o, o.label()).clicked() {
                        draft.rotary_orientation = o;
                    }
                }
            });
        });
        ui.checkbox(&mut draft.set_angular_limits, "Set Angular Limits");
        if draft.set_angular_limits {
            ui.horizontal(|ui| {
                ui.label("Start");
                ui.add(egui::DragValue::new(&mut draft.start_angle_deg).speed(1.0).suffix(" deg"));
            });
            ui.horizontal(|ui| {
                ui.label("End");
                ui.add(egui::DragValue::new(&mut draft.end_angle_deg).speed(1.0).suffix(" deg"));
            });
        }
    }

    match draft.strategy {
        MultiAxisStrategy::Swarf => {
            ui.horizontal(|ui| {
                ui.label("Drive Mode");
                egui::ComboBox::from_id_salt("ma_swarf_drive").selected_text(draft.swarf_drive.label()).show_ui(ui, |ui| {
                    for m in [SwarfDriveMode::Contours, SwarfDriveMode::Surfaces] {
                        if ui.selectable_label(draft.swarf_drive == m, m.label()).clicked() {
                            draft.swarf_drive = m;
                        }
                    }
                });
            });
            ui.label(RichText::new("First contour selection sets tip direction.").weak());
        }
        MultiAxisStrategy::AdvancedSwarf => {
            ui.horizontal(|ui| {
                ui.label("Select geometry using");
                egui::ComboBox::from_id_salt("ma_adv_geom").selected_text(draft.adv_geometry.label()).show_ui(ui, |ui| {
                    for m in [AdvSwarfGeometryMode::Surfaces, AdvSwarfGeometryMode::GuideCurves] {
                        if ui.selectable_label(draft.adv_geometry == m, m.label()).clicked() {
                            draft.adv_geometry = m;
                        }
                    }
                });
            });
            if draft.adv_geometry == AdvSwarfGeometryMode::GuideCurves {
                ui.label("Select Upper and Lower Guide Curves.");
            } else {
                ui.label("Select surfaces to machine.");
            }
        }
        MultiAxisStrategy::MultiAxisFinishing => {
            ui.label("Select floor and wall faces for circle-segment finishing.");
            ui.horizontal(|ui| {
                ui.label("Pattern");
                egui::ComboBox::from_id_salt("ma_finish_pat").selected_text(draft.finish_pattern.label()).show_ui(ui, |ui| {
                    for p in [MultiAxisFinishPattern::Scallop, MultiAxisFinishPattern::Blend] {
                        if ui.selectable_label(draft.finish_pattern == p, p.label()).clicked() {
                            draft.finish_pattern = p;
                        }
                    }
                });
            });
        }
        MultiAxisStrategy::MultiAxisContour | MultiAxisStrategy::MultiAxisRoughing => {
            ui.label("Select drive surfaces / containment boundaries.");
        }
        MultiAxisStrategy::RotaryOffsetRoughing => {
            ui.label("Select pocket / rotary stock regions to clear.");
        }
        _ => {}
    }

    ui.horizontal(|ui| {
        ui.label("Radial Stock to Leave");
        ui.add(egui::DragValue::new(&mut draft.radial_stock_mm).speed(0.01).range(0.0..=10.0).suffix(" mm"));
    });
    ui.horizontal(|ui| {
        ui.label("Axial Stock to Leave");
        ui.add(egui::DragValue::new(&mut draft.axial_stock_mm).speed(0.01).range(0.0..=10.0).suffix(" mm"));
    });

    ui.separator();
    let n = if draft.targets.face_sample_points.is_empty() { draft.targets.drill_points.len() } else { draft.targets.face_sample_points.len() };
    ui.label(format!("{n} selection(s)"));
    ui.horizontal(|ui| {
        if ui.button("Add sample point").clicked() && draft.targets.face_sample_points.len() < solidcraft_manufacture::MAX_SAMPLE_POINTS_PER_OP {
            draft.targets.face_sample_points.push([0.0, 0.0, 0.0]);
        }
        if ui.button("Clear").clicked() {
            draft.targets.face_sample_points.clear();
            draft.targets.drill_points.clear();
        }
    });
    let mut remove = None;
    for (i, pt) in draft.targets.face_sample_points.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("#{i}"));
            ui.add(egui::DragValue::new(&mut pt[0]).speed(0.1).prefix("X "));
            ui.add(egui::DragValue::new(&mut pt[1]).speed(0.1).prefix("Y "));
            ui.add(egui::DragValue::new(&mut pt[2]).speed(0.1).prefix("Z "));
            if ui.small_button("×").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        draft.targets.face_sample_points.remove(i);
    }
}

fn heights_tab(ui: &mut egui::Ui, heights: &mut ContourHeights, depth: &mut f64) {
    ui.heading("Heights");
    height_row(ui, "Clearance", &mut heights.clearance);
    height_row(ui, "Retract", &mut heights.retract);
    height_row(ui, "Feed", &mut heights.feed);
    height_row(ui, "Top", &mut heights.top);
    height_row(ui, "Bottom", &mut heights.bottom);
    ui.horizontal(|ui| {
        ui.label("Cut depth");
        ui.add(egui::DragValue::new(depth).speed(0.1).range(0.01..=500.0).suffix(" mm"));
    });
}

fn height_row(ui: &mut egui::Ui, label: &str, plane: &mut HeightPlane) {
    ui.group(|ui| {
        ui.label(RichText::new(label).strong());
        ui.horizontal(|ui| {
            ui.label("From");
            egui::ComboBox::from_id_salt(format!("mah_{label}")).selected_text(plane.from.label()).show_ui(ui, |ui| {
                for f in HEIGHT_FROM {
                    if ui.selectable_label(plane.from == *f, f.label()).clicked() {
                        plane.from = *f;
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Offset");
            ui.add(egui::DragValue::new(&mut plane.offset_mm).speed(0.1).suffix(" mm"));
        });
    });
}

fn passes_tab(ui: &mut egui::Ui, draft: &mut MultiAxisOperation) {
    ui.heading("Passes");
    ui.horizontal(|ui| {
        ui.label("Tolerance");
        ui.add(egui::DragValue::new(&mut draft.pass_opts.tolerance_mm).speed(0.001).range(0.001..=1.0).suffix(" mm"));
    });
    ui.horizontal(|ui| {
        ui.label("Sideways Compensation");
        egui::ComboBox::from_id_salt("ma_sideways").selected_text(draft.pass_opts.sideways.label()).show_ui(ui, |ui| {
            for s in [SidewaysCompensation::Left, SidewaysCompensation::Right, SidewaysCompensation::Center] {
                if ui.selectable_label(draft.pass_opts.sideways == s, s.label()).clicked() {
                    draft.pass_opts.sideways = s;
                }
            }
        });
    });

    match draft.strategy {
        MultiAxisStrategy::RotaryParallel | MultiAxisStrategy::RotaryContour => {
            ui.separator();
            ui.heading("Rotary Passes");
            ui.horizontal(|ui| {
                ui.label("Style");
                egui::ComboBox::from_id_salt("ma_rotary_style").selected_text(draft.rotary_style.label()).show_ui(ui, |ui| {
                    for s in [RotaryPassStyle::Spiral, RotaryPassStyle::Line, RotaryPassStyle::Circular] {
                        if ui.selectable_label(draft.rotary_style == s, s.label()).clicked() {
                            draft.rotary_style = s;
                        }
                    }
                });
            });
            match draft.rotary_style {
                RotaryPassStyle::Line => {
                    ui.horizontal(|ui| {
                        ui.label("Stepover Angle");
                        ui.add(egui::DragValue::new(&mut draft.stepover_angle_deg).speed(0.1).range(0.1..=90.0).suffix(" deg"));
                    });
                }
                RotaryPassStyle::Spiral | RotaryPassStyle::Circular => {
                    ui.horizontal(|ui| {
                        ui.label("Stepover");
                        ui.add(egui::DragValue::new(&mut draft.stepover).speed(0.01).range(0.05..=1.0));
                    });
                }
            }
            ui.horizontal(|ui| {
                ui.label("Tool Offset");
                ui.add(egui::DragValue::new(&mut draft.tool_offset_mm).speed(0.05).suffix(" mm"));
            });
        }
        MultiAxisStrategy::RotaryOffsetRoughing | MultiAxisStrategy::MultiAxisRoughing => {
            ui.horizontal(|ui| {
                ui.label("Stepover");
                ui.add(egui::DragValue::new(&mut draft.stepover).speed(0.01).range(0.05..=1.0));
            });
            ui.checkbox(&mut draft.multiple_depths, "Multiple Depths");
            if draft.multiple_depths {
                ui.horizontal(|ui| {
                    ui.label("Max Stepdown");
                    ui.add(egui::DragValue::new(&mut draft.max_stepdown_mm).speed(0.05).range(0.05..=50.0).suffix(" mm"));
                });
            }
        }
        MultiAxisStrategy::Swarf | MultiAxisStrategy::AdvancedSwarf => {
            ui.horizontal(|ui| {
                ui.label("Tool Offset");
                ui.add(egui::DragValue::new(&mut draft.tool_offset_mm).speed(0.05).suffix(" mm"));
            });
            ui.checkbox(&mut draft.multiple_depths, "Multiple Depths");
            if draft.multiple_depths {
                ui.horizontal(|ui| {
                    ui.label("Max Stepdown");
                    ui.add(egui::DragValue::new(&mut draft.max_stepdown_mm).speed(0.05).range(0.05..=50.0).suffix(" mm"));
                });
            }
            if draft.strategy == MultiAxisStrategy::AdvancedSwarf && draft.adv_geometry == AdvSwarfGeometryMode::Surfaces {
                ui.checkbox(&mut draft.check_for_gouges, "Check for Gouges");
            }
        }
        MultiAxisStrategy::MultiAxisContour => {
            ui.horizontal(|ui| {
                ui.label("Stepover");
                ui.add(egui::DragValue::new(&mut draft.stepover).speed(0.01).range(0.05..=1.0));
            });
            ui.checkbox(&mut draft.multiple_depths, "Multiple Depths");
            if draft.multiple_depths {
                ui.horizontal(|ui| {
                    ui.label("Max Stepdown");
                    ui.add(egui::DragValue::new(&mut draft.max_stepdown_mm).speed(0.05).range(0.05..=50.0).suffix(" mm"));
                });
            }
        }
        MultiAxisStrategy::MultiAxisFinishing => {
            ui.horizontal(|ui| {
                ui.label("Cusp Height");
                ui.add(egui::DragValue::new(&mut draft.cusp_height_mm).speed(0.01).range(0.01..=5.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Stepover");
                ui.add(egui::DragValue::new(&mut draft.stepover).speed(0.01).range(0.05..=1.0));
            });
            ui.horizontal(|ui| {
                ui.label("Pattern");
                egui::ComboBox::from_id_salt("ma_pass_finish").selected_text(draft.finish_pattern.label()).show_ui(ui, |ui| {
                    for p in [MultiAxisFinishPattern::Scallop, MultiAxisFinishPattern::Blend] {
                        if ui.selectable_label(draft.finish_pattern == p, p.label()).clicked() {
                            draft.finish_pattern = p;
                        }
                    }
                });
            });
        }
    }
}

fn multiaxis_tab(ui: &mut egui::Ui, draft: &mut MultiAxisOperation) {
    ui.heading("Multi-Axis");
    ui.horizontal(|ui| {
        ui.label("Machining Type");
        egui::ComboBox::from_id_salt("ma_mach_type").selected_text(draft.machining_type.label()).show_ui(ui, |ui| {
            for t in [MultiAxisMachiningType::ThreeAxis, MultiAxisMachiningType::FourAxis, MultiAxisMachiningType::FiveAxis] {
                if ui.selectable_label(draft.machining_type == t, t.label()).clicked() {
                    draft.machining_type = t;
                }
            }
        });
    });
    ui.label(RichText::new("Match machine capability (3 / 4 / simultaneous 5-axis).").weak());

    if matches!(
        draft.strategy,
        MultiAxisStrategy::MultiAxisFinishing
            | MultiAxisStrategy::AdvancedSwarf
            | MultiAxisStrategy::MultiAxisContour
            | MultiAxisStrategy::MultiAxisRoughing
    ) {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Lead Angle");
            ui.add(egui::DragValue::new(&mut draft.lead_angle_deg).speed(0.5).suffix(" deg"));
        });
        ui.horizontal(|ui| {
            ui.label("Tilt Angle");
            ui.add(egui::DragValue::new(&mut draft.tilt_angle_deg).speed(0.5).suffix(" deg"));
        });
    }

    if draft.strategy.is_rotary() {
        ui.separator();
        ui.label(format!("Rotary orientation: {}", draft.rotary_orientation.label()));
        ui.checkbox(&mut draft.set_angular_limits, "Limit angular sector");
    }
}

fn linking_tab(ui: &mut egui::Ui, linking: &mut ContourLinking) {
    ui.heading("Linking");
    egui::ComboBox::from_id_salt("ma_high_feed").selected_text(linking.high_feed_mode.label()).show_ui(ui, |ui| {
        for m in [HighFeedMode::PreserveRapid, HighFeedMode::AlwaysUseHighFeed, HighFeedMode::NeverUseHighFeed] {
            if ui.selectable_label(linking.high_feed_mode == m, m.label()).clicked() {
                linking.high_feed_mode = m;
            }
        }
    });
    ui.checkbox(&mut linking.keep_tool_down, "Keep Tool Down");
    ui.horizontal(|ui| {
        ui.label("Safe Distance");
        ui.add(egui::DragValue::new(&mut linking.safe_distance_mm).speed(0.01).range(0.0..=100.0).suffix(" mm"));
    });
    ui.checkbox(&mut linking.lead_in, "Lead-In");
    if linking.lead_in {
        ui.horizontal(|ui| {
            ui.label("Lead-In Radius");
            ui.add(egui::DragValue::new(&mut linking.lead_in_radius_mm).speed(0.05).range(0.0..=50.0).suffix(" mm"));
        });
    }
    ui.horizontal(|ui| {
        ui.label("Ramp Type");
        egui::ComboBox::from_id_salt("ma_ramp").selected_text(linking.ramp_type.label()).show_ui(ui, |ui| {
            for r in [RampType::Plunge, RampType::ZigZag, RampType::Helix] {
                if ui.selectable_label(linking.ramp_type == r, r.label()).clicked() {
                    linking.ramp_type = r;
                }
            }
        });
    });
}

fn save(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: MultiAxisOperation) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        return false;
    };
    if op_index >= setup.operations.len() {
        return false;
    }
    setup.operations[op_index] = Operation::MultiAxis(draft);
    match serde_json::to_value(&setup.operations) {
        Ok(ops) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops}})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.multiaxis.saved").to_string(), false);
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

pub fn paint_overlays(app: &SolveApp, painter: &egui::Painter, proj: &viewport::Proj) {
    let Some(state) = &app.manufacture_workspace.multiaxis_dialog else {
        return;
    };
    let pts = if state.draft.targets.face_sample_points.is_empty() {
        state.draft.targets.drill_points.as_slice()
    } else {
        state.draft.targets.face_sample_points.as_slice()
    };
    if pts.is_empty() {
        return;
    }
    let zs = state.draft.height_zs(0.0);
    let color = egui::Color32::from_rgb(200, 120, 255);
    let mut screen = Vec::new();
    for pt in pts {
        if let Some(s) = proj.to_screen(solidcraft_engine::geom::Vec3::new(pt[0], pt[1], zs.bottom)) {
            screen.push(s);
            painter.circle_filled(s, 3.0, color);
        }
    }
    for w in screen.windows(2) {
        painter.line_segment([w[0], w[1]], egui::Stroke::new(1.5, color));
    }
    if screen.len() >= 3 {
        painter.line_segment([screen[screen.len() - 1], screen[0]], egui::Stroke::new(1.5, color));
    }
}
