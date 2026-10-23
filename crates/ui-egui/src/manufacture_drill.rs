//! Drill operation dialog (Tool / Geometry / Heights / Cycle / Linking).
//! Document writes go through manufacture.settings.set; not companion HTTP or GPL CAM.

use egui::{Color32, RichText, Stroke, Vec2};
use serde_json::json;
use solidcraft_manufacture::{
    Coolant, DrillCycle, DrillOperation, DrillOrder, HeightFrom, HeightPlane, HighFeedMode, Operation, RetractionPolicy, Tool,
};

use crate::theme::Tokens;
use crate::{SolveApp, viewport};

/// Which inspector tab is active in the drill dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DrillDialogTab {
    #[default]
    Tool,
    Geometry,
    Heights,
    Cycle,
    Linking,
}

/// In-progress edit of a drill operation (OK commits via `manufacture.settings.set`).
#[derive(Clone, Debug)]
pub struct DrillDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: DrillOperation,
    pub tab: DrillDialogTab,
}

/// Open the drill inspector for `operations[op_index]` when it is a Drill.
pub fn open(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::Drill(op)) = setup.operations.get(op_index).cloned() else {
        return;
    };
    app.manufacture_workspace.selected_op = Some(op_index);
    app.manufacture_workspace.tool_library_dialog = None;
    app.manufacture_workspace.edit_tool_dialog = None;
    app.manufacture_workspace.manage_stub_dialog = None;
    app.manufacture_workspace.setup_dialog = None;
    app.manufacture_workspace.contour_dialog = None;
    app.manufacture_workspace.twod_dialog = None;
    app.manufacture_workspace.hole_rec_dialog = None;
    app.manufacture_workspace.threed_dialog = None;
    app.manufacture_workspace.multiaxis_dialog = None;
    app.manufacture_workspace.turning_dialog = None;
    app.manufacture_workspace.part_handling_dialog = None;
    app.manufacture_workspace.inspection_dialog = None;
    app.manufacture_workspace.manual_inspection_dialog = None;
    app.manufacture_workspace.fabrication_dialog = None;
    app.manufacture_workspace.nest_dialog = None;
    app.manufacture_workspace.drill_dialog =
        Some(DrillDialogState { setup_name: setup_name.to_string(), op_index, draft: op, tab: DrillDialogTab::Tool });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Drill;
}

/// Create a default Drill op on the active setup and open its inspector.
pub fn open_new(app: &mut SolveApp) {
    crate::manufacture_workspace::ensure_setup(app);
    let setup_name = app.manufacture_workspace.active_setup.clone();
    if setup_name.is_empty() {
        return;
    }
    let Some(mut setup) = app.session.doc.manufacture.setups.get(&setup_name).cloned() else {
        return;
    };
    setup.operations.push(Operation::Drill(DrillOperation::default()));
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

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.drill_dialog.clone() else {
        return;
    };

    let tools: Vec<Tool> = app.session.doc.manufacture.setups.get(&state.setup_name).map(|s| s.tools.clone()).unwrap_or_default();

    let title = if state.draft.name.is_empty() { "DRILL".to_string() } else { format!("DRILL : {}", state.draft.name.to_uppercase()) };

    let mut draft = state.draft.clone();
    let mut tab = state.tab;

    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_drill_dialog"), "settings", &title).fields().cards(1.4).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                tab_btn(ui, &mut tab, DrillDialogTab::Tool, "Tool");
                tab_btn(ui, &mut tab, DrillDialogTab::Geometry, "Geometry");
                tab_btn(ui, &mut tab, DrillDialogTab::Heights, "Heights");
                tab_btn(ui, &mut tab, DrillDialogTab::Cycle, "Cycle");
                tab_btn(ui, &mut tab, DrillDialogTab::Linking, "Linking");
            });
            ui.separator();

            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| match tab {
                DrillDialogTab::Tool => tool_tab(ui, &mut draft, &tools),
                DrillDialogTab::Geometry => geometry_tab(ui, &mut draft),
                DrillDialogTab::Heights => heights_tab(ui, &mut draft),
                DrillDialogTab::Cycle => cycle_tab(ui, &mut draft),
                DrillDialogTab::Linking => linking_tab(ui, &mut draft),
            });
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;

    if cancel {
        app.manufacture_workspace.drill_dialog = None;
        return;
    }

    if commit {
        if save_drill(app, &state.setup_name, state.op_index, draft.clone()) {
            app.manufacture_workspace.drill_dialog = None;
            return;
        }
    }

    if let Some(d) = &mut app.manufacture_workspace.drill_dialog {
        d.draft = draft;
        d.tab = tab;
    }
}

fn tab_btn(ui: &mut egui::Ui, tab: &mut DrillDialogTab, value: DrillDialogTab, label: &str) {
    let selected = *tab == value;
    let text = if selected { RichText::new(label).strong() } else { RichText::new(label) };
    if ui.selectable_label(selected, text).clicked() {
        *tab = value;
    }
}

fn tool_tab(ui: &mut egui::Ui, draft: &mut DrillOperation, tools: &[Tool]) {
    ui.heading("Tool");
    ui.horizontal(|ui| {
        ui.label("Tool");
        let current = tools.iter().find(|t| t.number == draft.tool_number);
        let label = match current {
            Some(t) => format!(
                "#{} - ø{:.2}mm {}",
                t.number,
                t.diameter,
                if t.description.is_empty() { tool_kind_short(t.kind) } else { t.description.as_str() }
            ),
            None => format!("#{} (missing)", draft.tool_number),
        };
        egui::ComboBox::from_id_salt("drill_tool_pick").selected_text(label).show_ui(ui, |ui| {
            for t in tools {
                let text = format!("#{} ø{:.2} {}", t.number, t.diameter, tool_kind_short(t.kind));
                if ui.selectable_label(draft.tool_number == t.number, text).clicked() {
                    draft.tool_number = t.number;
                }
            }
        });
    });

    ui.separator();
    ui.heading("Feed & Speed");
    ui.label(RichText::new("Preset").weak());
    ui.label("Carbon Fiber (custom)");

    let diameter = tools.iter().find(|t| t.number == draft.tool_number).map(|t| t.diameter).unwrap_or(1.7);
    let surface = std::f64::consts::PI * diameter * f64::from(draft.feeds.spindle_rpm) / 1_000.0;
    let plunge_per_rev = if draft.feeds.spindle_rpm > 0 { draft.feeds.plunge / f64::from(draft.feeds.spindle_rpm) } else { 0.0 };

    ui.horizontal(|ui| {
        ui.label("Spindle Speed");
        let mut rpm = draft.feeds.spindle_rpm;
        if ui.add(egui::DragValue::new(&mut rpm).speed(100).range(1..=60_000).suffix(" rpm")).changed() {
            draft.feeds.spindle_rpm = rpm;
        }
    });
    ui.label(format!("Surface Speed  {surface:.3} m/min"));

    ui.horizontal(|ui| {
        ui.label("Plunge Feedrate");
        ui.add(egui::DragValue::new(&mut draft.feeds.plunge).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
    });
    ui.label(format!("Plunge Feed/Rev  {plunge_per_rev:.3} mm"));

    ui.horizontal(|ui| {
        ui.label("Cutting Feedrate");
        ui.add(egui::DragValue::new(&mut draft.feeds.feedrate).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
    });

    ui.horizontal(|ui| {
        ui.label("Coolant");
        egui::ComboBox::from_id_salt("drill_coolant").selected_text(draft.coolant.label()).show_ui(ui, |ui| {
            for c in [Coolant::None, Coolant::Flood, Coolant::Mist, Coolant::Air, Coolant::ThroughSpindle] {
                if ui.selectable_label(draft.coolant == c, c.label()).clicked() {
                    draft.coolant = c;
                }
            }
        });
    });
}

fn geometry_tab(ui: &mut egui::Ui, draft: &mut DrillOperation) {
    ui.heading("Hole Faces");
    let n = draft.hole_centres().len();
    ui.label(format!("{n} hole(s) selected"));

    ui.checkbox(&mut draft.geometry.select_same_diameter, "Select same diameter");
    ui.checkbox(&mut draft.geometry.only_same_hole_depth, "Only same hole depth");
    ui.checkbox(&mut draft.geometry.only_same_z_top, "Only same Z top height");

    ui.separator();
    ui.heading("Order");
    egui::ComboBox::from_id_salt("drill_order").selected_text(draft.geometry.order.label()).show_ui(ui, |ui| {
        for o in [DrillOrder::ShortestDistance, DrillOrder::XThenY, DrillOrder::YThenX, DrillOrder::OrderOfSelection] {
            if ui.selectable_label(draft.geometry.order == o, o.label()).clicked() {
                draft.geometry.order = o;
            }
        }
    });
    ui.checkbox(&mut draft.geometry.reverse_order, "Reverse order");

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Add hole at origin").clicked() && draft.targets.drill_points.len() < solidcraft_manufacture::MAX_DRILL_POINTS_PER_OP {
            draft.targets.drill_points.push([0.0, 0.0, 0.0]);
        }
        if ui.button("Clear holes").clicked() {
            draft.targets.drill_points.clear();
            draft.targets.face_sample_points.clear();
        }
    });

    let mut remove: Option<usize> = None;
    for (i, pt) in draft.targets.drill_points.iter_mut().enumerate() {
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
        draft.targets.drill_points.remove(i);
    }
}

fn heights_tab(ui: &mut egui::Ui, draft: &mut DrillOperation) {
    ui.heading("Heights");
    height_row(ui, "Clearance Height", &mut draft.heights.clearance, true);
    height_row(ui, "Retract Height", &mut draft.heights.retract, true);
    height_row(ui, "Feed Height", &mut draft.heights.feed, true);
    height_row(ui, "Top Height", &mut draft.heights.top, true);
    height_row(ui, "Bottom Height", &mut draft.heights.bottom, true);
    ui.checkbox(&mut draft.heights.tip_through_bottom, "Drill tip through bottom");

    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Hole depth (fallback)");
        ui.add(egui::DragValue::new(&mut draft.depth).speed(0.1).range(0.01..=500.0).suffix(" mm"));
    });
}

fn height_row(ui: &mut egui::Ui, label: &str, plane: &mut HeightPlane, with_from: bool) {
    ui.group(|ui| {
        ui.label(RichText::new(label).strong());
        if with_from {
            ui.horizontal(|ui| {
                ui.label("From");
                egui::ComboBox::from_id_salt(format!("hf_{label}")).selected_text(plane.from.label()).show_ui(ui, |ui| {
                    for f in [
                        HeightFrom::ClearanceHeight,
                        HeightFrom::RetractHeight,
                        HeightFrom::FeedHeight,
                        HeightFrom::StockTop,
                        HeightFrom::TopHeight,
                        HeightFrom::HoleTop,
                        HeightFrom::HoleBottom,
                        HeightFrom::ModelTop,
                        HeightFrom::ModelBottom,
                        HeightFrom::SelectedContours,
                    ] {
                        if ui.selectable_label(plane.from == f, f.label()).clicked() {
                            plane.from = f;
                        }
                    }
                });
            });
        }
        ui.horizontal(|ui| {
            ui.label("Offset");
            ui.add(egui::DragValue::new(&mut plane.offset_mm).speed(0.1).suffix(" mm"));
        });
    });
}

fn cycle_tab(ui: &mut egui::Ui, draft: &mut DrillOperation) {
    ui.heading("Cycle");
    egui::ComboBox::from_id_salt("drill_cycle").selected_text(draft.cycle.label()).show_ui(ui, |ui| {
        for c in [DrillCycle::Simple, DrillCycle::Peck, DrillCycle::ChipBreak, DrillCycle::Dwell] {
            if ui.selectable_label(draft.cycle == c, c.label()).clicked() {
                draft.cycle = c;
            }
        }
    });

    match draft.cycle {
        DrillCycle::Peck | DrillCycle::ChipBreak => {
            ui.horizontal(|ui| {
                ui.label("Peck depth");
                ui.add(egui::DragValue::new(&mut draft.peck_depth).speed(0.1).range(0.01..=100.0).suffix(" mm"));
            });
        }
        DrillCycle::Dwell => {
            ui.horizontal(|ui| {
                ui.label("Dwell");
                ui.add(egui::DragValue::new(&mut draft.dwell_s).speed(0.1).range(0.0..=60.0).suffix(" s"));
            });
        }
        DrillCycle::Simple => {}
    }
}

fn linking_tab(ui: &mut egui::Ui, draft: &mut DrillOperation) {
    ui.heading("Linking");
    ui.checkbox(&mut draft.linking.ignore_gouges, "Ignore gouges");

    ui.horizontal(|ui| {
        ui.label("Retraction Policy");
        egui::ComboBox::from_id_salt("drill_retract_pol").selected_text(draft.linking.retraction_policy.label()).show_ui(ui, |ui| {
            for p in [RetractionPolicy::AvoidCollisions, RetractionPolicy::FullRetract, RetractionPolicy::MinimumRetract] {
                if ui.selectable_label(draft.linking.retraction_policy == p, p.label()).clicked() {
                    draft.linking.retraction_policy = p;
                }
            }
        });
    });

    ui.horizontal(|ui| {
        ui.label("High Feedrate Mode");
        egui::ComboBox::from_id_salt("drill_high_feed").selected_text(draft.linking.high_feed_mode.label()).show_ui(ui, |ui| {
            for m in [HighFeedMode::PreserveRapid, HighFeedMode::AlwaysUseHighFeed, HighFeedMode::NeverUseHighFeed] {
                if ui.selectable_label(draft.linking.high_feed_mode == m, m.label()).clicked() {
                    draft.linking.high_feed_mode = m;
                }
            }
        });
    });

    ui.horizontal(|ui| {
        ui.label("Safe Distance");
        ui.add(egui::DragValue::new(&mut draft.linking.safe_distance_mm).speed(0.1).range(0.0..=100.0).suffix(" mm"));
    });
}

fn tool_kind_short(kind: solidcraft_manufacture::ToolKind) -> &'static str {
    match kind {
        solidcraft_manufacture::ToolKind::FlatEndmill => "flat",
        solidcraft_manufacture::ToolKind::BallEndmill => "ball",
        solidcraft_manufacture::ToolKind::BullnoseEndmill => "bullnose",
        solidcraft_manufacture::ToolKind::Drill => "drill",
        solidcraft_manufacture::ToolKind::Chamfer => "chamfer",
        solidcraft_manufacture::ToolKind::Facemill => "facemill",
        solidcraft_manufacture::ToolKind::Tap => "tap",
        solidcraft_manufacture::ToolKind::Probe => "probe",
        solidcraft_manufacture::ToolKind::Waterjet => "waterjet",
        solidcraft_manufacture::ToolKind::Laser => "laser",
        solidcraft_manufacture::ToolKind::Plasma => "plasma",
    }
}

fn save_drill(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: DrillOperation) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        app.set_status("Setup not found", true);
        return false;
    };
    if op_index >= setup.operations.len() {
        app.set_status("Operation index out of range", true);
        return false;
    }
    setup.operations[op_index] = Operation::Drill(draft);
    match serde_json::to_value(&setup.operations) {
        Ok(ops_json) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops_json}})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.drill.saved").to_string(), false);
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

/// Active drill draft or selected drill op for viewport indicators.
pub fn active_drill(app: &SolveApp) -> Option<&DrillOperation> {
    if let Some(d) = &app.manufacture_workspace.drill_dialog {
        return Some(&d.draft);
    }
    let setup_name = &app.manufacture_workspace.active_setup;
    let idx = app.manufacture_workspace.selected_op?;
    let setup = app.session.doc.manufacture.setups.get(setup_name)?;
    match setup.operations.get(idx)? {
        Operation::Drill(op) => Some(op),
        _ => None,
    }
}

/// Draw hole markers, linking paths, and height-plane ticks for the active drill.
pub fn paint_overlays(app: &SolveApp, painter: &egui::Painter, proj: &viewport::Proj) {
    let Some(op) = active_drill(app) else {
        return;
    };
    let holes = op.hole_centres();
    if holes.is_empty() {
        return;
    }

    let t = Tokens::get();

    let stock_top = 0.0;
    let zs = op.height_zs(stock_top);
    let link_z = match op.linking.retraction_policy {
        RetractionPolicy::FullRetract => zs.clearance,
        RetractionPolicy::MinimumRetract => zs.retract,
        RetractionPolicy::AvoidCollisions => zs.clearance,
    };

    let hole_color = Color32::from_rgb(255, 140, 40);
    let link_color = Color32::from_rgba_unmultiplied(255, 160, 60, 200);
    let height_color = Color32::from_rgba_unmultiplied(80, 180, 255, 160);
    let tool_color = Color32::from_rgba_unmultiplied(100, 160, 255, 90);

    let diameter = app
        .session
        .doc
        .manufacture
        .setups
        .get(&app.manufacture_workspace.active_setup)
        .and_then(|s| s.tools.iter().find(|tool| tool.number == op.tool_number))
        .map(|tool| tool.diameter)
        .unwrap_or(1.7);
    let radius_world = (diameter * 0.5).max(0.2);

    // Linking polyline at retract/clearance
    let mut prev: Option<egui::Pos2> = None;
    for pt in holes {
        let w = solidcraft_engine::geom::Vec3::new(pt[0], pt[1], link_z);
        if let Some(s) = proj.to_screen(w) {
            if let Some(p) = prev {
                painter.line_segment([p, s], Stroke::new(1.25, link_color));
            }
            prev = Some(s);
        }
    }

    // Hole markers + vertical plunge hints
    for pt in holes {
        let top = solidcraft_engine::geom::Vec3::new(pt[0], pt[1], zs.feed);
        let bot = solidcraft_engine::geom::Vec3::new(pt[0], pt[1], zs.bottom);
        if let (Some(a), Some(b)) = (proj.to_screen(top), proj.to_screen(bot)) {
            painter.line_segment([a, b], Stroke::new(1.0, t.accent));
        }
        if let Some(c) = proj.to_screen(solidcraft_engine::geom::Vec3::new(pt[0], pt[1], zs.top)) {
            let r = screen_radius(&proj, pt[0], pt[1], zs.top, radius_world).unwrap_or(4.0);
            painter.circle_stroke(c, r, Stroke::new(1.5, hole_color));
            painter.circle_filled(c, 2.0, hole_color);
        }
    }

    // Height plane ticks beside the first hole
    if let Some(pt) = holes.first() {
        let x = pt[0] + radius_world * 3.0;
        let y = pt[1];
        for (z, label) in [(zs.clearance, "C"), (zs.retract, "R"), (zs.feed, "F"), (zs.top, "T"), (zs.bottom, "B")] {
            let a = solidcraft_engine::geom::Vec3::new(x, y, z);
            let b = solidcraft_engine::geom::Vec3::new(x + radius_world * 2.0, y, z);
            if let (Some(sa), Some(sb)) = (proj.to_screen(a), proj.to_screen(b)) {
                painter.line_segment([sa, sb], Stroke::new(1.0, height_color));
                painter.text(sb + Vec2::new(4.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(10.0), height_color);
            }
        }

        // Soft tool body disc at feed height on first hole
        if let Some(c) = proj.to_screen(solidcraft_engine::geom::Vec3::new(pt[0], pt[1], zs.feed)) {
            let r = screen_radius(&proj, pt[0], pt[1], zs.feed, radius_world).unwrap_or(6.0);
            painter.circle_filled(c, r, tool_color);
            painter.circle_stroke(c, r, Stroke::new(1.0, Color32::from_rgb(120, 180, 255)));
        }
    }
}

fn screen_radius(proj: &viewport::Proj, x: f64, y: f64, z: f64, radius_mm: f64) -> Option<f32> {
    let c = proj.to_screen(solidcraft_engine::geom::Vec3::new(x, y, z))?;
    let e = proj.to_screen(solidcraft_engine::geom::Vec3::new(x + radius_mm, y, z))?;
    Some(c.distance(e).max(3.0))
}
