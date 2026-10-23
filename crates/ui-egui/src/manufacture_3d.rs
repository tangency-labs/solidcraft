//! 3D milling dialogs (Tool / Geometry / Heights / Passes / Linking).
//! Document writes go through manufacture.settings.set; not companion HTTP or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{
    ContourHeights, ContourLinking, ContourPasses, Coolant, HeightFrom, HeightPlane, HighFeedMode, Operation, RampType, ShallowStyle,
    SidewaysCompensation, ThreedOperation, ThreedStrategy, Tool,
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
pub enum ThreedTab {
    #[default]
    Tool,
    Geometry,
    Heights,
    Passes,
    Linking,
}

#[derive(Clone, Debug)]
pub struct ThreedDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: ThreedOperation,
    pub tab: ThreedTab,
}

pub fn open(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::Threed(op)) = setup.operations.get(op_index).cloned() else {
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
    app.manufacture_workspace.hole_rec_dialog = None;
    app.manufacture_workspace.multiaxis_dialog = None;
    app.manufacture_workspace.turning_dialog = None;
    app.manufacture_workspace.part_handling_dialog = None;
    app.manufacture_workspace.inspection_dialog = None;
    app.manufacture_workspace.manual_inspection_dialog = None;
    app.manufacture_workspace.threed_dialog =
        Some(ThreedDialogState { setup_name: setup_name.to_string(), op_index, draft: op, tab: ThreedTab::Tool });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::ThreeD;
}

/// Create a 3D op with `strategy` on the active setup and open its inspector.
pub fn open_new(app: &mut SolveApp, strategy: ThreedStrategy) {
    crate::manufacture_workspace::ensure_setup(app);
    let setup_name = app.manufacture_workspace.active_setup.clone();
    if setup_name.is_empty() {
        return;
    }
    let Some(mut setup) = app.session.doc.manufacture.setups.get(&setup_name).cloned() else {
        return;
    };
    setup.operations.push(Operation::Threed(ThreedOperation::with_strategy(strategy)));
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

/// Return false when `id` is a 3D milling command this panel handles.
pub fn open_from_command(app: &mut SolveApp, id: &str) -> bool {
    let strategy = match id {
        "manufacture.3d.adaptive" => ThreedStrategy::AdaptiveRoughing,
        "manufacture.3d.parallel" => ThreedStrategy::Parallel,
        "manufacture.3d.scallop" => ThreedStrategy::Scallop,
        "manufacture.3d.steep" => ThreedStrategy::SteepAndShallow,
        "manufacture.3d.contour" => ThreedStrategy::Contour,
        "manufacture.3d.flat" => ThreedStrategy::Flat,
        _ => return true,
    };
    open_new(app, strategy);
    false
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.threed_dialog.clone() else {
        return;
    };
    let tools: Vec<Tool> = app.session.doc.manufacture.setups.get(&state.setup_name).map(|s| s.tools.clone()).unwrap_or_default();

    let mut draft = state.draft.clone();
    let mut tab = state.tab;
    let title = dialog_title(&draft);

    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_3d_dialog"), "settings", &title).fields().cards(1.5).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                for (v, l) in [
                    (ThreedTab::Tool, "Tool"),
                    (ThreedTab::Geometry, "Geometry"),
                    (ThreedTab::Heights, "Heights"),
                    (ThreedTab::Passes, "Passes"),
                    (ThreedTab::Linking, "Linking"),
                ] {
                    let sel = tab == v;
                    if ui.selectable_label(sel, if sel { RichText::new(l).strong() } else { RichText::new(l) }).clicked() {
                        tab = v;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| match tab {
                ThreedTab::Tool => tool_tab(ui, &mut draft, &tools),
                ThreedTab::Geometry => geometry_tab(ui, &mut draft),
                ThreedTab::Heights => heights_tab(ui, &mut draft.heights, &mut draft.depth),
                ThreedTab::Passes => passes_tab(ui, &mut draft),
                ThreedTab::Linking => linking_tab(ui, &mut draft.linking),
            });
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;

    if cancel {
        app.manufacture_workspace.threed_dialog = None;
        return;
    }
    if commit && save(app, &state.setup_name, state.op_index, draft.clone()) {
        app.manufacture_workspace.threed_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.threed_dialog {
        d.draft = draft;
        d.tab = tab;
    }
}

fn dialog_title(draft: &ThreedOperation) -> String {
    let kind = draft.strategy.label().to_uppercase();
    if draft.name.is_empty() || draft.name == draft.strategy.label() { kind } else { format!("{kind} : {}", draft.name.to_uppercase()) }
}

fn tool_tab(ui: &mut egui::Ui, draft: &mut ThreedOperation, tools: &[Tool]) {
    ui.heading("Tool");
    let tnum = draft.tool_number;
    let label = tools.iter().find(|t| t.number == tnum).map(|t| format!("#{} ø{:.2}", t.number, t.diameter)).unwrap_or_else(|| format!("#{tnum}"));
    egui::ComboBox::from_id_salt("3d_tool").selected_text(label).show_ui(ui, |ui| {
        for t in tools {
            if ui.selectable_label(tnum == t.number, format!("#{} ø{:.2}", t.number, t.diameter)).clicked() {
                draft.tool_number = t.number;
            }
        }
    });
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
        egui::ComboBox::from_id_salt("3d_coolant").selected_text(coolant.label()).show_ui(ui, |ui| {
            for c in [Coolant::None, Coolant::Flood, Coolant::Mist, Coolant::Air, Coolant::ThroughSpindle] {
                if ui.selectable_label(coolant == c, c.label()).clicked() {
                    draft.coolant = c;
                }
            }
        });
    });
}

const THREED_STRATEGIES: &[ThreedStrategy] = &[
    ThreedStrategy::AdaptiveRoughing,
    ThreedStrategy::OffsetRoughing,
    ThreedStrategy::ThreePlusTwoRoughing,
    ThreedStrategy::SteepAndShallow,
    ThreedStrategy::Flat,
    ThreedStrategy::Parallel,
    ThreedStrategy::Scallop,
    ThreedStrategy::Contour,
    ThreedStrategy::Ramp,
    ThreedStrategy::Pencil,
    ThreedStrategy::Horizontal,
    ThreedStrategy::Spiral,
    ThreedStrategy::Radial,
    ThreedStrategy::MorphedSpiral,
    ThreedStrategy::Project,
    ThreedStrategy::Blend,
    ThreedStrategy::Morph,
    ThreedStrategy::Corner,
    ThreedStrategy::Flow,
    ThreedStrategy::Deburr,
    ThreedStrategy::Geodesic,
];

fn geometry_tab(ui: &mut egui::Ui, draft: &mut ThreedOperation) {
    ui.heading("Geometry");
    ui.horizontal(|ui| {
        ui.label("Strategy");
        egui::ComboBox::from_id_salt("3d_strategy").selected_text(draft.strategy.label()).show_ui(ui, |ui| {
            for s in THREED_STRATEGIES {
                if ui.selectable_label(draft.strategy == *s, s.label()).clicked() {
                    draft.strategy = *s;
                }
            }
        });
    });
    let n = if draft.targets.face_sample_points.is_empty() { draft.targets.drill_points.len() } else { draft.targets.face_sample_points.len() };
    ui.label(format!("{n} selection(s)"));

    ui.horizontal(|ui| {
        ui.label("Radial Stock to Leave");
        ui.add(egui::DragValue::new(&mut draft.radial_stock_mm).speed(0.01).range(0.0..=10.0).suffix(" mm"));
    });
    ui.horizontal(|ui| {
        ui.label("Axial Stock to Leave");
        ui.add(egui::DragValue::new(&mut draft.axial_stock_mm).speed(0.01).range(0.0..=10.0).suffix(" mm"));
    });

    match draft.strategy {
        ThreedStrategy::AdaptiveRoughing | ThreedStrategy::ThreePlusTwoRoughing => {
            ui.checkbox(&mut draft.use_slot_clearing, "Use Slot Clearing");
            ui.checkbox(&mut draft.machine_shallow_areas, "Machine Shallow Areas");
        }
        ThreedStrategy::OffsetRoughing => {
            ui.checkbox(&mut draft.use_morphed_spiral, "Use Morphed Spiral");
            ui.checkbox(&mut draft.flat_area_detection, "Flat Area Detection");
        }
        ThreedStrategy::SteepAndShallow => {
            ui.horizontal(|ui| {
                ui.label("Threshold Angle");
                ui.add(egui::DragValue::new(&mut draft.threshold_angle_deg).speed(0.5).range(1.0..=89.0).suffix(" deg"));
            });
            ui.horizontal(|ui| {
                ui.label("Shallow Style");
                egui::ComboBox::from_id_salt("3d_shallow").selected_text(draft.shallow_style.label()).show_ui(ui, |ui| {
                    for s in [ShallowStyle::Parallel, ShallowStyle::Scallop] {
                        if ui.selectable_label(draft.shallow_style == s, s.label()).clicked() {
                            draft.shallow_style = s;
                        }
                    }
                });
            });
            ui.checkbox(&mut draft.machine_shallow_areas, "Machine Shallow Areas");
        }
        ThreedStrategy::Flat => {
            ui.checkbox(&mut draft.flat_area_detection, "Flat Area Detection");
        }
        ThreedStrategy::Deburr => {
            ui.label("Select edges / faces to deburr.");
        }
        ThreedStrategy::Project | ThreedStrategy::Blend | ThreedStrategy::Morph | ThreedStrategy::Flow => {
            ui.label("Select drive curves / surfaces for this strategy.");
        }
        _ => {}
    }

    ui.separator();
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
            egui::ComboBox::from_id_salt(format!("3dh_{label}")).selected_text(plane.from.label()).show_ui(ui, |ui| {
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

fn passes_tab(ui: &mut egui::Ui, draft: &mut ThreedOperation) {
    ui.heading("Passes");
    let p: &mut ContourPasses = &mut draft.pass_opts;
    ui.horizontal(|ui| {
        ui.label("Tolerance");
        ui.add(egui::DragValue::new(&mut p.tolerance_mm).speed(0.001).range(0.001..=1.0).suffix(" mm"));
    });
    ui.horizontal(|ui| {
        ui.label("Sideways Compensation");
        egui::ComboBox::from_id_salt("3d_sideways").selected_text(p.sideways.label()).show_ui(ui, |ui| {
            for s in [SidewaysCompensation::Left, SidewaysCompensation::Right, SidewaysCompensation::Center] {
                if ui.selectable_label(p.sideways == s, s.label()).clicked() {
                    p.sideways = s;
                }
            }
        });
    });

    match draft.strategy {
        ThreedStrategy::AdaptiveRoughing | ThreedStrategy::ThreePlusTwoRoughing => {
            ui.horizontal(|ui| {
                ui.label("Optimal Load");
                ui.add(egui::DragValue::new(&mut draft.optimal_load).speed(0.01).range(0.05..=1.0));
            });
            ui.checkbox(&mut draft.pass_opts.multiple_depths, "Multiple Depths");
            if draft.pass_opts.multiple_depths {
                ui.horizontal(|ui| {
                    ui.label("Maximum Roughing Stepdown");
                    ui.add(egui::DragValue::new(&mut draft.max_stepdown_mm).speed(0.05).range(0.05..=50.0).suffix(" mm"));
                });
                ui.horizontal(|ui| {
                    ui.label("Fine Stepdown");
                    ui.add(egui::DragValue::new(&mut draft.fine_stepdown_mm).speed(0.05).range(0.05..=50.0).suffix(" mm"));
                });
            }
        }
        ThreedStrategy::OffsetRoughing => {
            ui.horizontal(|ui| {
                ui.label("Stepover");
                ui.add(egui::DragValue::new(&mut draft.stepover).speed(0.01).range(0.05..=1.0));
            });
            ui.checkbox(&mut draft.pass_opts.multiple_depths, "Multiple Depths");
            if draft.pass_opts.multiple_depths {
                ui.horizontal(|ui| {
                    ui.label("Max Stepdown");
                    ui.add(egui::DragValue::new(&mut draft.max_stepdown_mm).speed(0.05).range(0.05..=50.0).suffix(" mm"));
                });
            }
        }
        ThreedStrategy::SteepAndShallow => {
            ui.horizontal(|ui| {
                ui.label("Threshold Angle");
                ui.add(egui::DragValue::new(&mut draft.threshold_angle_deg).speed(0.5).range(1.0..=89.0).suffix(" deg"));
            });
            ui.horizontal(|ui| {
                ui.label("Shallow Style");
                egui::ComboBox::from_id_salt("3d_pass_shallow").selected_text(draft.shallow_style.label()).show_ui(ui, |ui| {
                    for s in [ShallowStyle::Parallel, ShallowStyle::Scallop] {
                        if ui.selectable_label(draft.shallow_style == s, s.label()).clicked() {
                            draft.shallow_style = s;
                        }
                    }
                });
            });
            match draft.shallow_style {
                ShallowStyle::Scallop => {
                    ui.horizontal(|ui| {
                        ui.label("Cusp Height");
                        ui.add(egui::DragValue::new(&mut draft.cusp_height_mm).speed(0.01).range(0.01..=5.0).suffix(" mm"));
                    });
                }
                ShallowStyle::Parallel => {
                    ui.horizontal(|ui| {
                        ui.label("Stepover");
                        ui.add(egui::DragValue::new(&mut draft.stepover).speed(0.01).range(0.05..=1.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Pass Direction");
                        ui.add(egui::DragValue::new(&mut draft.pass_direction_deg).speed(1.0).suffix(" deg"));
                    });
                }
            }
            ui.horizontal(|ui| {
                ui.label("Max Stepdown");
                ui.add(egui::DragValue::new(&mut draft.max_stepdown_mm).speed(0.05).range(0.05..=50.0).suffix(" mm"));
            });
        }
        ThreedStrategy::Scallop | ThreedStrategy::MorphedSpiral | ThreedStrategy::Pencil | ThreedStrategy::Corner => {
            ui.horizontal(|ui| {
                ui.label("Cusp Height");
                ui.add(egui::DragValue::new(&mut draft.cusp_height_mm).speed(0.01).range(0.01..=5.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Stepover");
                ui.add(egui::DragValue::new(&mut draft.stepover).speed(0.01).range(0.05..=1.0));
            });
        }
        ThreedStrategy::Parallel | ThreedStrategy::Radial | ThreedStrategy::Spiral | ThreedStrategy::Horizontal => {
            ui.horizontal(|ui| {
                ui.label("Stepover");
                ui.add(egui::DragValue::new(&mut draft.stepover).speed(0.01).range(0.05..=1.0));
            });
            if matches!(draft.strategy, ThreedStrategy::Parallel | ThreedStrategy::Radial) {
                ui.horizontal(|ui| {
                    ui.label("Pass Direction");
                    ui.add(egui::DragValue::new(&mut draft.pass_direction_deg).speed(1.0).suffix(" deg"));
                });
            }
            ui.checkbox(&mut draft.pass_opts.multiple_depths, "Multiple Depths");
            if draft.pass_opts.multiple_depths {
                ui.horizontal(|ui| {
                    ui.label("Max Stepdown");
                    ui.add(egui::DragValue::new(&mut draft.max_stepdown_mm).speed(0.05).range(0.05..=50.0).suffix(" mm"));
                });
            }
        }
        ThreedStrategy::Contour | ThreedStrategy::Ramp | ThreedStrategy::Flat => {
            ui.horizontal(|ui| {
                ui.label("Stepover");
                ui.add(egui::DragValue::new(&mut draft.stepover).speed(0.01).range(0.05..=1.0));
            });
            ui.checkbox(&mut draft.pass_opts.multiple_depths, "Multiple Depths");
            if draft.pass_opts.multiple_depths {
                ui.horizontal(|ui| {
                    ui.label("Max Stepdown");
                    ui.add(egui::DragValue::new(&mut draft.max_stepdown_mm).speed(0.05).range(0.05..=50.0).suffix(" mm"));
                });
                ui.checkbox(&mut draft.pass_opts.use_even_stepdowns, "Use Even Stepdowns");
            }
        }
        ThreedStrategy::Project
        | ThreedStrategy::Blend
        | ThreedStrategy::Morph
        | ThreedStrategy::Flow
        | ThreedStrategy::Geodesic
        | ThreedStrategy::Deburr => {
            ui.horizontal(|ui| {
                ui.label("Stepover");
                ui.add(egui::DragValue::new(&mut draft.stepover).speed(0.01).range(0.05..=1.0));
            });
            ui.horizontal(|ui| {
                ui.label("Cusp Height");
                ui.add(egui::DragValue::new(&mut draft.cusp_height_mm).speed(0.01).range(0.01..=5.0).suffix(" mm"));
            });
        }
    }
}

fn linking_tab(ui: &mut egui::Ui, linking: &mut ContourLinking) {
    ui.heading("Linking");
    egui::ComboBox::from_id_salt("3d_high_feed").selected_text(linking.high_feed_mode.label()).show_ui(ui, |ui| {
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
        egui::ComboBox::from_id_salt("3d_ramp").selected_text(linking.ramp_type.label()).show_ui(ui, |ui| {
            for r in [RampType::Plunge, RampType::ZigZag, RampType::Helix] {
                if ui.selectable_label(linking.ramp_type == r, r.label()).clicked() {
                    linking.ramp_type = r;
                }
            }
        });
    });
}

fn save(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: ThreedOperation) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        return false;
    };
    if op_index >= setup.operations.len() {
        return false;
    }
    setup.operations[op_index] = Operation::Threed(draft);
    match serde_json::to_value(&setup.operations) {
        Ok(ops) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops}})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.threed.saved").to_string(), false);
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
    let Some(state) = &app.manufacture_workspace.threed_dialog else {
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
    let color = egui::Color32::from_rgb(120, 200, 90);
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
