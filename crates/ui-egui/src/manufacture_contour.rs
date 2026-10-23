//! 2D Contour dialog (Tool / Geometry / Heights / Passes / Linking).
//! Document writes go through manufacture.settings.set; not companion HTTP or GPL CAM.

use egui::{Color32, RichText, Stroke, Vec2};
use serde_json::json;
use solidcraft_manufacture::{
    CompensationType, Coolant, HeightFrom, HeightPlane, HighFeedMode, Operation, ProfileOperation, ProfileSide, RampType, SidewaysCompensation,
    TabShape, Tool,
};

use crate::theme::Tokens;
use crate::{SolveApp, viewport};

const HEIGHT_FROM_CHOICES: &[HeightFrom] = &[
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
];

/// Which inspector tab is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ContourDialogTab {
    #[default]
    Tool,
    Geometry,
    Heights,
    Passes,
    Linking,
}

/// In-progress edit of a 2D Contour / Profile operation.
#[derive(Clone, Debug)]
pub struct ContourDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: ProfileOperation,
    pub tab: ContourDialogTab,
}

/// Open the contour inspector for `operations[op_index]` when it is a Profile.
pub fn open(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::Profile(op)) = setup.operations.get(op_index).cloned() else {
        return;
    };
    app.manufacture_workspace.selected_op = Some(op_index);
    app.manufacture_workspace.tool_library_dialog = None;
    app.manufacture_workspace.edit_tool_dialog = None;
    app.manufacture_workspace.manage_stub_dialog = None;
    app.manufacture_workspace.setup_dialog = None;
    app.manufacture_workspace.twod_dialog = None;
    app.manufacture_workspace.contour_dialog =
        Some(ContourDialogState { setup_name: setup_name.to_string(), op_index, draft: op, tab: ContourDialogTab::Tool });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Contour;
}

/// Create a default 2D Contour op on the active setup and open its inspector.
pub fn open_new(app: &mut SolveApp) {
    crate::manufacture_workspace::ensure_setup(app);
    let setup_name = app.manufacture_workspace.active_setup.clone();
    if setup_name.is_empty() {
        return;
    }
    let Some(mut setup) = app.session.doc.manufacture.setups.get(&setup_name).cloned() else {
        return;
    };
    setup.operations.push(Operation::Profile(ProfileOperation::default()));
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
    let Some(state) = app.manufacture_workspace.contour_dialog.clone() else {
        return;
    };

    let tools: Vec<Tool> = app.session.doc.manufacture.setups.get(&state.setup_name).map(|s| s.tools.clone()).unwrap_or_default();

    let title = if state.draft.name.is_empty() { "2D CONTOUR".to_string() } else { format!("2D CONTOUR : {}", state.draft.name.to_uppercase()) };

    let mut draft = state.draft.clone();
    let mut tab = state.tab;

    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_contour_dialog"), "settings", &title).fields().cards(1.4).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                tab_btn(ui, &mut tab, ContourDialogTab::Tool, "Tool");
                tab_btn(ui, &mut tab, ContourDialogTab::Geometry, "Geometry");
                tab_btn(ui, &mut tab, ContourDialogTab::Heights, "Heights");
                tab_btn(ui, &mut tab, ContourDialogTab::Passes, "Passes");
                tab_btn(ui, &mut tab, ContourDialogTab::Linking, "Linking");
            });
            ui.separator();

            egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| match tab {
                ContourDialogTab::Tool => tool_tab(ui, &mut draft, &tools),
                ContourDialogTab::Geometry => geometry_tab(ui, &mut draft),
                ContourDialogTab::Heights => heights_tab(ui, &mut draft),
                ContourDialogTab::Passes => passes_tab(ui, &mut draft),
                ContourDialogTab::Linking => linking_tab(ui, &mut draft),
            });
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;

    if cancel {
        app.manufacture_workspace.contour_dialog = None;
        return;
    }

    if commit {
        if save_contour(app, &state.setup_name, state.op_index, draft.clone()) {
            app.manufacture_workspace.contour_dialog = None;
            return;
        }
    }

    if let Some(d) = &mut app.manufacture_workspace.contour_dialog {
        d.draft = draft;
        d.tab = tab;
    }
}

fn tab_btn(ui: &mut egui::Ui, tab: &mut ContourDialogTab, value: ContourDialogTab, label: &str) {
    let selected = *tab == value;
    let text = if selected { RichText::new(label).strong() } else { RichText::new(label) };
    if ui.selectable_label(selected, text).clicked() {
        *tab = value;
    }
}

fn tool_tab(ui: &mut egui::Ui, draft: &mut ProfileOperation, tools: &[Tool]) {
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
        egui::ComboBox::from_id_salt("contour_tool_pick").selected_text(label).show_ui(ui, |ui| {
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
    let flutes = tools.iter().find(|t| t.number == draft.tool_number).map(|t| t.flutes.max(1)).unwrap_or(2);
    let feed_per_tooth =
        if draft.feeds.spindle_rpm > 0 { draft.feeds.feedrate / (f64::from(draft.feeds.spindle_rpm) * f64::from(flutes)) } else { 0.0 };

    ui.horizontal(|ui| {
        ui.label("Spindle Speed");
        let mut rpm = draft.feeds.spindle_rpm;
        if ui.add(egui::DragValue::new(&mut rpm).speed(100).range(1..=60_000).suffix(" rpm")).changed() {
            draft.feeds.spindle_rpm = rpm;
        }
    });
    ui.label(format!("Surface Speed  {surface:.3} m/min"));

    ui.horizontal(|ui| {
        ui.label("Cutting Feedrate");
        ui.add(egui::DragValue::new(&mut draft.feeds.feedrate).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
    });
    ui.label(format!("Feed per Tooth  {feed_per_tooth:.3} mm"));

    ui.horizontal(|ui| {
        ui.label("Lead-In Feedrate");
        ui.add(egui::DragValue::new(&mut draft.lead_in_feedrate).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
    });
    ui.horizontal(|ui| {
        ui.label("Lead-Out Feedrate");
        ui.add(egui::DragValue::new(&mut draft.lead_out_feedrate).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
    });
    ui.horizontal(|ui| {
        ui.label("Plunge Feedrate");
        ui.add(egui::DragValue::new(&mut draft.feeds.plunge).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
    });

    ui.horizontal(|ui| {
        ui.label("Coolant");
        egui::ComboBox::from_id_salt("contour_coolant").selected_text(draft.coolant.label()).show_ui(ui, |ui| {
            for c in [Coolant::None, Coolant::Flood, Coolant::Mist, Coolant::Air, Coolant::ThroughSpindle] {
                if ui.selectable_label(draft.coolant == c, c.label()).clicked() {
                    draft.coolant = c;
                }
            }
        });
    });
}

fn geometry_tab(ui: &mut egui::Ui, draft: &mut ProfileOperation) {
    ui.heading("Contour Selection");
    let n = draft.contour_points().len();
    ui.label(format!("{n} selection(s)"));

    ui.horizontal(|ui| {
        ui.label("Side");
        egui::ComboBox::from_id_salt("contour_side").selected_text(draft.side.label()).show_ui(ui, |ui| {
            for s in [ProfileSide::Outside, ProfileSide::Inside, ProfileSide::On] {
                if ui.selectable_label(draft.side == s, s.label()).clicked() {
                    draft.side = s;
                }
            }
        });
    });

    ui.checkbox(&mut draft.geometry.stock_contours, "Stock Contours");
    ui.checkbox(&mut draft.geometry.rest_machining, "Rest Machining");

    ui.separator();
    ui.checkbox(&mut draft.geometry.tabs.enabled, "Tabs");
    if draft.geometry.tabs.enabled {
        ui.horizontal(|ui| {
            ui.label("Tab Shape");
            egui::ComboBox::from_id_salt("contour_tab_shape").selected_text(draft.geometry.tabs.shape.label()).show_ui(ui, |ui| {
                for s in [TabShape::Rectangular, TabShape::Triangular] {
                    if ui.selectable_label(draft.geometry.tabs.shape == s, s.label()).clicked() {
                        draft.geometry.tabs.shape = s;
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Tab Width");
            ui.add(egui::DragValue::new(&mut draft.geometry.tabs.width_mm).speed(0.1).range(0.1..=50.0).suffix(" mm"));
        });
        ui.horizontal(|ui| {
            ui.label("Tab Height");
            ui.add(egui::DragValue::new(&mut draft.geometry.tabs.height_mm).speed(0.05).range(0.0..=20.0).suffix(" mm"));
        });
        ui.checkbox(&mut draft.geometry.tabs.by_distance, "By distance");
        if draft.geometry.tabs.by_distance {
            ui.horizontal(|ui| {
                ui.label("Tab Spacing");
                ui.add(egui::DragValue::new(&mut draft.geometry.tabs.spacing_mm).speed(1.0).range(1.0..=500.0).suffix(" mm"));
            });
        } else {
            ui.horizontal(|ui| {
                ui.label("Tab Count");
                let mut c = draft.geometry.tabs.count;
                if ui.add(egui::DragValue::new(&mut c).speed(1).range(1..=64)).changed() {
                    draft.geometry.tabs.count = c;
                }
            });
        }
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

    let mut remove: Option<usize> = None;
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

fn heights_tab(ui: &mut egui::Ui, draft: &mut ProfileOperation) {
    ui.heading("Heights");
    height_row(ui, "Clearance Height", &mut draft.heights.clearance);
    height_row(ui, "Retract Height", &mut draft.heights.retract);
    height_row(ui, "Feed Height", &mut draft.heights.feed);
    height_row(ui, "Top Height", &mut draft.heights.top);
    height_row(ui, "Bottom Height", &mut draft.heights.bottom);

    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Cut depth (fallback)");
        ui.add(egui::DragValue::new(&mut draft.depth).speed(0.1).range(0.01..=500.0).suffix(" mm"));
    });
}

fn height_row(ui: &mut egui::Ui, label: &str, plane: &mut HeightPlane) {
    ui.group(|ui| {
        ui.label(RichText::new(label).strong());
        ui.horizontal(|ui| {
            ui.label("From");
            egui::ComboBox::from_id_salt(format!("chf_{label}")).selected_text(plane.from.label()).show_ui(ui, |ui| {
                for f in HEIGHT_FROM_CHOICES {
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

fn passes_tab(ui: &mut egui::Ui, draft: &mut ProfileOperation) {
    ui.heading("Passes");
    ui.horizontal(|ui| {
        ui.label("Tolerance");
        ui.add(egui::DragValue::new(&mut draft.pass_opts.tolerance_mm).speed(0.001).range(0.001..=1.0).suffix(" mm"));
    });

    ui.horizontal(|ui| {
        ui.label("Sideways Compensation");
        egui::ComboBox::from_id_salt("contour_sideways").selected_text(draft.pass_opts.sideways.label()).show_ui(ui, |ui| {
            for s in [SidewaysCompensation::Left, SidewaysCompensation::Right, SidewaysCompensation::Center] {
                if ui.selectable_label(draft.pass_opts.sideways == s, s.label()).clicked() {
                    draft.pass_opts.sideways = s;
                }
            }
        });
    });

    ui.horizontal(|ui| {
        ui.label("Compensation Type");
        egui::ComboBox::from_id_salt("contour_comp_type").selected_text(draft.pass_opts.compensation_type.label()).show_ui(ui, |ui| {
            for c in [CompensationType::InComputer, CompensationType::InControl, CompensationType::Off] {
                if ui.selectable_label(draft.pass_opts.compensation_type == c, c.label()).clicked() {
                    draft.pass_opts.compensation_type = c;
                }
            }
        });
    });

    ui.horizontal(|ui| {
        ui.label("Stock to Leave");
        ui.add(egui::DragValue::new(&mut draft.finish_allowance).speed(0.01).range(0.0..=10.0).suffix(" mm"));
    });

    ui.horizontal(|ui| {
        ui.label("Radial passes");
        let mut p = draft.passes;
        if ui.add(egui::DragValue::new(&mut p).speed(1).range(1..=64)).changed() {
            draft.passes = p;
        }
    });

    ui.separator();
    ui.checkbox(&mut draft.pass_opts.roughing_passes, "Roughing Passes");
    ui.checkbox(&mut draft.pass_opts.multiple_depths, "Multiple Depths");
    if draft.pass_opts.multiple_depths {
        ui.horizontal(|ui| {
            ui.label("Maximum Roughing Stepdown");
            ui.add(egui::DragValue::new(&mut draft.pass_opts.max_stepdown_mm).speed(0.05).range(0.05..=50.0).suffix(" mm"));
        });
        ui.checkbox(&mut draft.pass_opts.use_even_stepdowns, "Use Even Stepdowns");
        ui.horizontal(|ui| {
            ui.label("Finishing Stepdowns");
            let mut n = draft.pass_opts.finishing_stepdowns;
            if ui.add(egui::DragValue::new(&mut n).speed(1).range(0..=32)).changed() {
                draft.pass_opts.finishing_stepdowns = n;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Finishing Stepdown");
            ui.add(egui::DragValue::new(&mut draft.pass_opts.finishing_stepdown_mm).speed(0.05).range(0.01..=10.0).suffix(" mm"));
        });
    }
}

fn linking_tab(ui: &mut egui::Ui, draft: &mut ProfileOperation) {
    ui.heading("Linking");
    ui.horizontal(|ui| {
        ui.label("High Feedrate Mode");
        egui::ComboBox::from_id_salt("contour_high_feed").selected_text(draft.linking.high_feed_mode.label()).show_ui(ui, |ui| {
            for m in [HighFeedMode::PreserveRapid, HighFeedMode::AlwaysUseHighFeed, HighFeedMode::NeverUseHighFeed] {
                if ui.selectable_label(draft.linking.high_feed_mode == m, m.label()).clicked() {
                    draft.linking.high_feed_mode = m;
                }
            }
        });
    });
    ui.checkbox(&mut draft.linking.allow_rapid_retract, "Allow Rapid Retract");
    ui.horizontal(|ui| {
        ui.label("Safe Distance");
        ui.add(egui::DragValue::new(&mut draft.linking.safe_distance_mm).speed(0.01).range(0.0..=100.0).suffix(" mm"));
    });
    ui.checkbox(&mut draft.linking.keep_tool_down, "Keep Tool Down");
    ui.horizontal(|ui| {
        ui.label("Maximum Stay-Down Distance");
        ui.add(egui::DragValue::new(&mut draft.linking.max_stay_down_mm).speed(1.0).range(0.0..=500.0).suffix(" mm"));
    });
    ui.horizontal(|ui| {
        ui.label("Lift Height");
        ui.add(egui::DragValue::new(&mut draft.linking.lift_height_mm).speed(0.1).range(0.0..=50.0).suffix(" mm"));
    });

    ui.separator();
    ui.heading("Leads & Transitions");
    ui.checkbox(&mut draft.linking.lead_in, "Lead-In (Entry)");
    if draft.linking.lead_in {
        ui.horizontal(|ui| {
            ui.label("Horizontal Lead-In Radius");
            ui.add(egui::DragValue::new(&mut draft.linking.lead_in_radius_mm).speed(0.05).range(0.0..=50.0).suffix(" mm"));
        });
        ui.horizontal(|ui| {
            ui.label("Lead-In Sweep Angle");
            ui.add(egui::DragValue::new(&mut draft.linking.lead_in_sweep_deg).speed(1.0).range(0.0..=180.0).suffix(" deg"));
        });
        ui.horizontal(|ui| {
            ui.label("Linear Lead-In Distance");
            ui.add(egui::DragValue::new(&mut draft.linking.lead_in_linear_mm).speed(0.05).range(0.0..=50.0).suffix(" mm"));
        });
    }
    ui.checkbox(&mut draft.linking.lead_out, "Lead-Out (Exit)");

    ui.separator();
    ui.heading("Ramp");
    ui.horizontal(|ui| {
        ui.label("Ramp Type");
        egui::ComboBox::from_id_salt("contour_ramp_type").selected_text(draft.linking.ramp_type.label()).show_ui(ui, |ui| {
            for r in [RampType::Plunge, RampType::ZigZag, RampType::Helix] {
                if ui.selectable_label(draft.linking.ramp_type == r, r.label()).clicked() {
                    draft.linking.ramp_type = r;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Ramping Angle");
        ui.add(egui::DragValue::new(&mut draft.linking.ramp_angle_deg).speed(0.5).range(0.1..=45.0).suffix(" deg"));
    });
    ui.horizontal(|ui| {
        ui.label("Maximum Ramp Stepdown");
        ui.add(egui::DragValue::new(&mut draft.linking.max_ramp_stepdown_mm).speed(0.1).range(0.05..=50.0).suffix(" mm"));
    });
    ui.horizontal(|ui| {
        ui.label("Ramp Clearance Height");
        ui.add(egui::DragValue::new(&mut draft.linking.ramp_clearance_mm).speed(0.05).range(0.0..=20.0).suffix(" mm"));
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

fn save_contour(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: ProfileOperation) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        app.set_status("Setup not found", true);
        return false;
    };
    if op_index >= setup.operations.len() {
        app.set_status("Operation index out of range", true);
        return false;
    }
    setup.operations[op_index] = Operation::Profile(draft);
    match serde_json::to_value(&setup.operations) {
        Ok(ops_json) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops_json}})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.contour.saved").to_string(), false);
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

/// Active contour draft or selected profile for viewport indicators.
pub fn active_contour(app: &SolveApp) -> Option<&ProfileOperation> {
    if let Some(d) = &app.manufacture_workspace.contour_dialog {
        return Some(&d.draft);
    }
    let setup_name = &app.manufacture_workspace.active_setup;
    let idx = app.manufacture_workspace.selected_op?;
    let setup = app.session.doc.manufacture.setups.get(setup_name)?;
    match setup.operations.get(idx)? {
        Operation::Profile(op) => Some(op),
        _ => None,
    }
}

/// Draw contour samples, multi-depth hints, height planes, and lead-in markers.
pub fn paint_overlays(app: &SolveApp, painter: &egui::Painter, proj: &viewport::Proj) {
    let Some(op) = active_contour(app) else {
        return;
    };
    let pts = op.contour_points();
    if pts.is_empty() {
        return;
    }

    let t = Tokens::get();

    let zs = op.height_zs(0.0);
    let cut_color = Color32::from_rgb(70, 140, 255);
    let link_color = Color32::from_rgba_unmultiplied(255, 160, 60, 200);
    let height_color = Color32::from_rgba_unmultiplied(80, 180, 255, 160);
    let entry_color = Color32::from_rgb(60, 200, 80);

    // Depth passes: stacked contour polylines
    let mut depths = Vec::new();
    if op.pass_opts.multiple_depths && op.pass_opts.max_stepdown_mm > 0.0 {
        let total = (zs.top - zs.bottom).abs().max(op.depth);
        let mut z = zs.top;
        let step = if op.pass_opts.use_even_stepdowns {
            let n = (total / op.pass_opts.max_stepdown_mm).ceil().max(1.0);
            total / n
        } else {
            op.pass_opts.max_stepdown_mm
        };
        while z > zs.bottom + 1e-6 {
            z = (z - step).max(zs.bottom);
            depths.push(z);
            if depths.len() > 64 {
                break;
            }
        }
        if depths.last().copied() != Some(zs.bottom) {
            depths.push(zs.bottom);
        }
    } else {
        depths.push(zs.bottom);
    }

    for &z in &depths {
        let mut screen: Vec<egui::Pos2> = Vec::new();
        for pt in pts {
            if let Some(s) = proj.to_screen(solidcraft_engine::geom::Vec3::new(pt[0], pt[1], z)) {
                screen.push(s);
            }
        }
        if screen.len() >= 2 {
            for w in screen.windows(2) {
                painter.line_segment([w[0], w[1]], Stroke::new(1.5, cut_color));
            }
            if screen.len() >= 3 {
                painter.line_segment([screen[screen.len() - 1], screen[0]], Stroke::new(1.5, cut_color));
            }
        }
        for s in &screen {
            painter.circle_filled(*s, 2.5, cut_color);
        }
    }

    // Linking between first and subsequent contour centroids at retract
    if pts.len() >= 2 {
        let mut prev: Option<egui::Pos2> = None;
        for pt in pts {
            let w = solidcraft_engine::geom::Vec3::new(pt[0], pt[1], zs.retract);
            if let Some(s) = proj.to_screen(w) {
                if let Some(p) = prev {
                    painter.line_segment([p, s], Stroke::new(1.0, link_color));
                }
                prev = Some(s);
            }
        }
    }

    // Lead-in arrow at first sample
    if let Some(pt) = pts.first() {
        let entry = solidcraft_engine::geom::Vec3::new(pt[0], pt[1], zs.feed);
        let tip = solidcraft_engine::geom::Vec3::new(pt[0], pt[1], zs.bottom);
        if let (Some(a), Some(b)) = (proj.to_screen(entry), proj.to_screen(tip)) {
            painter.line_segment([a, b], Stroke::new(2.0, entry_color));
            painter.circle_filled(a, 4.0, entry_color);
            painter.arrow(a, b - a, Stroke::new(1.5, entry_color));
        }

        // Height ticks
        let x = pt[0] + 3.0;
        let y = pt[1];
        for (z, label) in [(zs.clearance, "C"), (zs.retract, "R"), (zs.feed, "F"), (zs.top, "T"), (zs.bottom, "B")] {
            let a = solidcraft_engine::geom::Vec3::new(x, y, z);
            let b = solidcraft_engine::geom::Vec3::new(x + 2.0, y, z);
            if let (Some(sa), Some(sb)) = (proj.to_screen(a), proj.to_screen(b)) {
                painter.line_segment([sa, sb], Stroke::new(1.0, height_color));
                painter.text(sb + Vec2::new(4.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(10.0), height_color);
            }
        }
    }

    let _ = t;
}
