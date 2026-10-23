//! 2D milling dialogs (Tool / Geometry / Heights / Passes / Linking)
//! for Face, Adaptive, Pocket, Slot, Trace, Thread, Bore, Circular, Engrave, Chamfer.
//! Document writes go through manufacture.settings.set; not companion HTTP or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{
    AdaptiveOperation, BoreOperation, ChamferOperation, CircularOperation, ContourHeights, ContourLinking, ContourPasses, Coolant, EngraveOperation,
    FaceOperation, HeightFrom, HeightPlane, HighFeedMode, Operation, PocketOperation, RampType, SidewaysCompensation, SlotOperation, ThreadOperation,
    Tool, TraceOperation,
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
pub enum TwodTab {
    #[default]
    Tool,
    Geometry,
    Heights,
    Passes,
    Linking,
}

#[derive(Clone, Debug)]
pub enum TwodDraft {
    Face(FaceOperation),
    Adaptive(AdaptiveOperation),
    Pocket(PocketOperation),
    Slot(SlotOperation),
    Trace(TraceOperation),
    Thread(ThreadOperation),
    Bore(BoreOperation),
    Circular(CircularOperation),
    Engrave(EngraveOperation),
    Chamfer(ChamferOperation),
}

impl TwodDraft {
    fn from_op(op: &Operation) -> Option<Self> {
        match op {
            Operation::Face(o) => Some(Self::Face(o.clone())),
            Operation::Adaptive(o) => Some(Self::Adaptive(o.clone())),
            Operation::Pocket(o) => Some(Self::Pocket(o.clone())),
            Operation::Slot(o) => Some(Self::Slot(o.clone())),
            Operation::Trace(o) => Some(Self::Trace(o.clone())),
            Operation::Thread(o) => Some(Self::Thread(o.clone())),
            Operation::Bore(o) => Some(Self::Bore(o.clone())),
            Operation::Circular(o) => Some(Self::Circular(o.clone())),
            Operation::Engrave(o) => Some(Self::Engrave(o.clone())),
            Operation::Chamfer(o) => Some(Self::Chamfer(o.clone())),
            _ => None,
        }
    }

    fn into_op(self) -> Operation {
        match self {
            Self::Face(o) => Operation::Face(o),
            Self::Adaptive(o) => Operation::Adaptive(o),
            Self::Pocket(o) => Operation::Pocket(o),
            Self::Slot(o) => Operation::Slot(o),
            Self::Trace(o) => Operation::Trace(o),
            Self::Thread(o) => Operation::Thread(o),
            Self::Bore(o) => Operation::Bore(o),
            Self::Circular(o) => Operation::Circular(o),
            Self::Engrave(o) => Operation::Engrave(o),
            Self::Chamfer(o) => Operation::Chamfer(o),
        }
    }

    fn title(&self) -> String {
        let name = match self {
            Self::Face(o) => o.name.as_str(),
            Self::Adaptive(o) => o.name.as_str(),
            Self::Pocket(o) => o.name.as_str(),
            Self::Slot(o) => o.name.as_str(),
            Self::Trace(o) => o.name.as_str(),
            Self::Thread(o) => o.name.as_str(),
            Self::Bore(o) => o.name.as_str(),
            Self::Circular(o) => o.name.as_str(),
            Self::Engrave(o) => o.name.as_str(),
            Self::Chamfer(o) => o.name.as_str(),
        };
        let kind = match self {
            Self::Face(_) => "FACE",
            Self::Adaptive(_) => "2D ADAPTIVE ROUGHING",
            Self::Pocket(_) => "2D POCKET",
            Self::Slot(_) => "SLOT",
            Self::Trace(_) => "TRACE",
            Self::Thread(_) => "THREAD",
            Self::Bore(_) => "BORE",
            Self::Circular(_) => "CIRCULAR",
            Self::Engrave(_) => "ENGRAVE",
            Self::Chamfer(_) => "2D CHAMFER",
        };
        if name.is_empty() { kind.into() } else { format!("{kind} : {}", name.to_uppercase()) }
    }

    fn tool_number(&self) -> u32 {
        match self {
            Self::Face(o) => o.tool_number,
            Self::Adaptive(o) => o.tool_number,
            Self::Pocket(o) => o.tool_number,
            Self::Slot(o) => o.tool_number,
            Self::Trace(o) => o.tool_number,
            Self::Thread(o) => o.tool_number,
            Self::Bore(o) => o.tool_number,
            Self::Circular(o) => o.tool_number,
            Self::Engrave(o) => o.tool_number,
            Self::Chamfer(o) => o.tool_number,
        }
    }

    fn set_tool_number(&mut self, n: u32) {
        match self {
            Self::Face(o) => o.tool_number = n,
            Self::Adaptive(o) => o.tool_number = n,
            Self::Pocket(o) => o.tool_number = n,
            Self::Slot(o) => o.tool_number = n,
            Self::Trace(o) => o.tool_number = n,
            Self::Thread(o) => o.tool_number = n,
            Self::Bore(o) => o.tool_number = n,
            Self::Circular(o) => o.tool_number = n,
            Self::Engrave(o) => o.tool_number = n,
            Self::Chamfer(o) => o.tool_number = n,
        }
    }

    fn feeds_mut(&mut self) -> &mut solidcraft_manufacture::Feeds {
        match self {
            Self::Face(o) => &mut o.feeds,
            Self::Adaptive(o) => &mut o.feeds,
            Self::Pocket(o) => &mut o.feeds,
            Self::Slot(o) => &mut o.feeds,
            Self::Trace(o) => &mut o.feeds,
            Self::Thread(o) => &mut o.feeds,
            Self::Bore(o) => &mut o.feeds,
            Self::Circular(o) => &mut o.feeds,
            Self::Engrave(o) => &mut o.feeds,
            Self::Chamfer(o) => &mut o.feeds,
        }
    }

    fn coolant_mut(&mut self) -> &mut Coolant {
        match self {
            Self::Face(o) => &mut o.coolant,
            Self::Adaptive(o) => &mut o.coolant,
            Self::Pocket(o) => &mut o.coolant,
            Self::Slot(o) => &mut o.coolant,
            Self::Trace(o) => &mut o.coolant,
            Self::Thread(o) => &mut o.coolant,
            Self::Bore(o) => &mut o.coolant,
            Self::Circular(o) => &mut o.coolant,
            Self::Engrave(o) => &mut o.coolant,
            Self::Chamfer(o) => &mut o.coolant,
        }
    }

    fn depth_mut(&mut self) -> &mut f64 {
        match self {
            Self::Face(o) => &mut o.depth,
            Self::Adaptive(o) => &mut o.depth,
            Self::Pocket(o) => &mut o.depth,
            Self::Slot(o) => &mut o.depth,
            Self::Trace(o) => &mut o.depth,
            Self::Thread(o) => &mut o.depth,
            Self::Bore(o) => &mut o.depth,
            Self::Circular(o) => &mut o.depth,
            Self::Engrave(o) => &mut o.depth,
            Self::Chamfer(o) => &mut o.depth,
        }
    }

    fn heights_mut(&mut self) -> &mut ContourHeights {
        match self {
            Self::Face(o) => &mut o.heights,
            Self::Adaptive(o) => &mut o.heights,
            Self::Pocket(o) => &mut o.heights,
            Self::Slot(o) => &mut o.heights,
            Self::Trace(o) => &mut o.heights,
            Self::Thread(o) => &mut o.heights,
            Self::Bore(o) => &mut o.heights,
            Self::Circular(o) => &mut o.heights,
            Self::Engrave(o) => &mut o.heights,
            Self::Chamfer(o) => &mut o.heights,
        }
    }

    fn linking_mut(&mut self) -> &mut ContourLinking {
        match self {
            Self::Face(o) => &mut o.linking,
            Self::Adaptive(o) => &mut o.linking,
            Self::Pocket(o) => &mut o.linking,
            Self::Slot(o) => &mut o.linking,
            Self::Trace(o) => &mut o.linking,
            Self::Thread(o) => &mut o.linking,
            Self::Bore(o) => &mut o.linking,
            Self::Circular(o) => &mut o.linking,
            Self::Engrave(o) => &mut o.linking,
            Self::Chamfer(o) => &mut o.linking,
        }
    }

    fn targets_mut(&mut self) -> &mut solidcraft_manufacture::GeometricTargets {
        match self {
            Self::Face(o) => &mut o.targets,
            Self::Adaptive(o) => &mut o.targets,
            Self::Pocket(o) => &mut o.targets,
            Self::Slot(o) => &mut o.targets,
            Self::Trace(o) => &mut o.targets,
            Self::Thread(o) => &mut o.targets,
            Self::Bore(o) => &mut o.targets,
            Self::Circular(o) => &mut o.targets,
            Self::Engrave(o) => &mut o.targets,
            Self::Chamfer(o) => &mut o.targets,
        }
    }

    fn pass_opts_mut(&mut self) -> Option<&mut ContourPasses> {
        match self {
            Self::Face(o) => Some(&mut o.pass_opts),
            Self::Adaptive(o) => Some(&mut o.pass_opts),
            Self::Pocket(o) => Some(&mut o.pass_opts),
            Self::Slot(o) => Some(&mut o.pass_opts),
            Self::Bore(o) => Some(&mut o.pass_opts),
            Self::Circular(o) => Some(&mut o.pass_opts),
            _ => None,
        }
    }

    fn sample_points(&self) -> &[[f64; 3]] {
        let t = match self {
            Self::Face(o) => &o.targets,
            Self::Adaptive(o) => &o.targets,
            Self::Pocket(o) => &o.targets,
            Self::Slot(o) => &o.targets,
            Self::Trace(o) => &o.targets,
            Self::Thread(o) => &o.targets,
            Self::Bore(o) => &o.targets,
            Self::Circular(o) => &o.targets,
            Self::Engrave(o) => &o.targets,
            Self::Chamfer(o) => &o.targets,
        };
        if t.face_sample_points.is_empty() { &t.drill_points } else { &t.face_sample_points }
    }

    fn depth(&self) -> f64 {
        match self {
            Self::Face(o) => o.depth,
            Self::Adaptive(o) => o.depth,
            Self::Pocket(o) => o.depth,
            Self::Slot(o) => o.depth,
            Self::Trace(o) => o.depth,
            Self::Thread(o) => o.depth,
            Self::Bore(o) => o.depth,
            Self::Circular(o) => o.depth,
            Self::Engrave(o) => o.depth,
            Self::Chamfer(o) => o.depth,
        }
    }

    fn heights(&self) -> &ContourHeights {
        match self {
            Self::Face(o) => &o.heights,
            Self::Adaptive(o) => &o.heights,
            Self::Pocket(o) => &o.heights,
            Self::Slot(o) => &o.heights,
            Self::Trace(o) => &o.heights,
            Self::Thread(o) => &o.heights,
            Self::Bore(o) => &o.heights,
            Self::Circular(o) => &o.heights,
            Self::Engrave(o) => &o.heights,
            Self::Chamfer(o) => &o.heights,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TwodDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: TwodDraft,
    pub tab: TwodTab,
}

pub fn open(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(op) = setup.operations.get(op_index) else {
        return;
    };
    let Some(draft) = TwodDraft::from_op(op) else {
        return;
    };
    app.manufacture_workspace.selected_op = Some(op_index);
    app.manufacture_workspace.tool_library_dialog = None;
    app.manufacture_workspace.edit_tool_dialog = None;
    app.manufacture_workspace.manage_stub_dialog = None;
    app.manufacture_workspace.setup_dialog = None;
    app.manufacture_workspace.contour_dialog = None;
    app.manufacture_workspace.drill_dialog = None;
    app.manufacture_workspace.hole_rec_dialog = None;
    app.manufacture_workspace.threed_dialog = None;
    app.manufacture_workspace.multiaxis_dialog = None;
    app.manufacture_workspace.turning_dialog = None;
    app.manufacture_workspace.part_handling_dialog = None;
    app.manufacture_workspace.inspection_dialog = None;
    app.manufacture_workspace.manual_inspection_dialog = None;
    app.manufacture_workspace.fabrication_dialog = None;
    app.manufacture_workspace.nest_dialog = None;
    app.manufacture_workspace.twod_dialog = Some(TwodDialogState { setup_name: setup_name.to_string(), op_index, draft, tab: TwodTab::Tool });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::TwoD;
}

/// Create a default 2D op on the active setup and open its inspector.
pub fn open_new(app: &mut SolveApp, draft: TwodDraft) {
    crate::manufacture_workspace::ensure_setup(app);
    let setup_name = app.manufacture_workspace.active_setup.clone();
    if setup_name.is_empty() {
        return;
    }
    let Some(mut setup) = app.session.doc.manufacture.setups.get(&setup_name).cloned() else {
        return;
    };
    setup.operations.push(draft.into_op());
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

/// Return false when `id` is a 2D milling command this panel handles.
pub fn open_from_command(app: &mut SolveApp, id: &str) -> bool {
    let draft = match id {
        "manufacture.2d.face" => TwodDraft::Face(Default::default()),
        "manufacture.2d.adaptive" => TwodDraft::Adaptive(Default::default()),
        "manufacture.2d.pocket" => TwodDraft::Pocket(Default::default()),
        "manufacture.2d.slot" => TwodDraft::Slot(Default::default()),
        "manufacture.2d.trace" => TwodDraft::Trace(Default::default()),
        "manufacture.2d.thread" => TwodDraft::Thread(Default::default()),
        "manufacture.2d.bore" => TwodDraft::Bore(Default::default()),
        "manufacture.2d.circular" => TwodDraft::Circular(Default::default()),
        "manufacture.2d.engrave" => TwodDraft::Engrave(Default::default()),
        "manufacture.2d.chamfer" => TwodDraft::Chamfer(Default::default()),
        _ => return true,
    };
    open_new(app, draft);
    false
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.twod_dialog.clone() else {
        return;
    };
    let tools: Vec<Tool> = app.session.doc.manufacture.setups.get(&state.setup_name).map(|s| s.tools.clone()).unwrap_or_default();

    let mut draft = state.draft.clone();
    let mut tab = state.tab;
    let title = draft.title();

    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_2d_dialog"), "settings", &title).fields().cards(1.4).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                for (v, l) in [
                    (TwodTab::Tool, "Tool"),
                    (TwodTab::Geometry, "Geometry"),
                    (TwodTab::Heights, "Heights"),
                    (TwodTab::Passes, "Passes"),
                    (TwodTab::Linking, "Linking"),
                ] {
                    let sel = tab == v;
                    if ui.selectable_label(sel, if sel { RichText::new(l).strong() } else { RichText::new(l) }).clicked() {
                        tab = v;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| match tab {
                TwodTab::Tool => tool_tab(ui, &mut draft, &tools),
                TwodTab::Geometry => geometry_tab(ui, &mut draft),
                TwodTab::Heights => {
                    let mut depth = draft.depth();
                    heights_tab(ui, draft.heights_mut(), &mut depth);
                    *draft.depth_mut() = depth;
                }
                TwodTab::Passes => passes_tab(ui, &mut draft),
                TwodTab::Linking => linking_tab(ui, draft.linking_mut()),
            });
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;

    if cancel {
        app.manufacture_workspace.twod_dialog = None;
        return;
    }
    if commit && save(app, &state.setup_name, state.op_index, draft.clone()) {
        app.manufacture_workspace.twod_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.twod_dialog {
        d.draft = draft;
        d.tab = tab;
    }
}

fn tool_tab(ui: &mut egui::Ui, draft: &mut TwodDraft, tools: &[Tool]) {
    ui.heading("Tool");
    let tnum = draft.tool_number();
    let label = tools.iter().find(|t| t.number == tnum).map(|t| format!("#{} ø{:.2}", t.number, t.diameter)).unwrap_or_else(|| format!("#{tnum}"));
    egui::ComboBox::from_id_salt("twod_tool").selected_text(label).show_ui(ui, |ui| {
        for t in tools {
            if ui.selectable_label(tnum == t.number, format!("#{} ø{:.2}", t.number, t.diameter)).clicked() {
                draft.set_tool_number(t.number);
            }
        }
    });
    ui.separator();
    ui.heading("Feed & Speed");
    let feeds = draft.feeds_mut();
    ui.horizontal(|ui| {
        ui.label("Spindle");
        let mut rpm = feeds.spindle_rpm;
        if ui.add(egui::DragValue::new(&mut rpm).speed(100).range(1..=60_000).suffix(" rpm")).changed() {
            feeds.spindle_rpm = rpm;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Cutting Feedrate");
        ui.add(egui::DragValue::new(&mut feeds.feedrate).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
    });
    ui.horizontal(|ui| {
        ui.label("Plunge Feedrate");
        ui.add(egui::DragValue::new(&mut feeds.plunge).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
    });
    let coolant = *draft.coolant_mut();
    ui.horizontal(|ui| {
        ui.label("Coolant");
        egui::ComboBox::from_id_salt("twod_coolant").selected_text(coolant.label()).show_ui(ui, |ui| {
            for c in [Coolant::None, Coolant::Flood, Coolant::Mist, Coolant::Air, Coolant::ThroughSpindle] {
                if ui.selectable_label(coolant == c, c.label()).clicked() {
                    *draft.coolant_mut() = c;
                }
            }
        });
    });
}

fn geometry_tab(ui: &mut egui::Ui, draft: &mut TwodDraft) {
    ui.heading("Geometry");
    let n = draft.sample_points().len();
    ui.label(format!("{n} selection(s)"));

    match draft {
        TwodDraft::Face(o) => {
            ui.horizontal(|ui| {
                ui.label("Stepover");
                ui.add(egui::DragValue::new(&mut o.stepover).speed(0.01).range(0.05..=1.0));
            });
        }
        TwodDraft::Adaptive(o) => {
            ui.checkbox(&mut o.stock_contours, "Stock Contours");
            ui.horizontal(|ui| {
                ui.label("Optimal Load");
                ui.add(egui::DragValue::new(&mut o.optimal_load).speed(0.01).range(0.05..=1.0));
            });
            ui.horizontal(|ui| {
                ui.label("Stock to Leave");
                ui.add(egui::DragValue::new(&mut o.finish_allowance).speed(0.01).range(0.0..=5.0).suffix(" mm"));
            });
        }
        TwodDraft::Pocket(o) => {
            ui.checkbox(&mut o.stock_contours, "Stock Contours");
            ui.horizontal(|ui| {
                ui.label("Stepover");
                ui.add(egui::DragValue::new(&mut o.stepover).speed(0.01).range(0.05..=1.0));
            });
            ui.horizontal(|ui| {
                ui.label("Stock to Leave");
                ui.add(egui::DragValue::new(&mut o.finish_allowance).speed(0.01).range(0.0..=5.0).suffix(" mm"));
            });
        }
        TwodDraft::Slot(o) => {
            ui.horizontal(|ui| {
                ui.label("Backoff Distance");
                ui.add(egui::DragValue::new(&mut o.backoff_mm).speed(0.05).range(0.0..=20.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Stock to Leave");
                ui.add(egui::DragValue::new(&mut o.finish_allowance).speed(0.01).range(0.0..=5.0).suffix(" mm"));
            });
        }
        TwodDraft::Trace(o) => {
            ui.horizontal(|ui| {
                ui.label("Axial Offset");
                ui.add(egui::DragValue::new(&mut o.axial_offset).speed(0.05).suffix(" mm"));
            });
        }
        TwodDraft::Thread(o) => {
            ui.checkbox(&mut o.internal, "Internal thread");
            ui.horizontal(|ui| {
                ui.label("Pitch");
                ui.add(egui::DragValue::new(&mut o.pitch_mm).speed(0.05).range(0.1..=10.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Major Ø");
                ui.add(egui::DragValue::new(&mut o.major_diameter).speed(0.1).range(0.5..=200.0).suffix(" mm"));
            });
            ui.checkbox(&mut o.do_multiple_threads, "Do Multiple Threads");
            if o.do_multiple_threads {
                ui.horizontal(|ui| {
                    ui.label("Count");
                    ui.add(egui::DragValue::new(&mut o.thread_count).speed(1).range(1..=16));
                });
            }
        }
        TwodDraft::Bore(o) => {
            ui.horizontal(|ui| {
                ui.label("Diameter");
                ui.add(egui::DragValue::new(&mut o.diameter).speed(0.1).range(0.5..=500.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Helix Pitch");
                ui.add(egui::DragValue::new(&mut o.pitch_mm).speed(0.1).range(0.1..=50.0).suffix(" mm"));
            });
        }
        TwodDraft::Circular(o) => {
            ui.checkbox(&mut o.outside, "Outside (boss)");
            ui.horizontal(|ui| {
                ui.label("Diameter");
                ui.add(egui::DragValue::new(&mut o.diameter).speed(0.1).range(0.5..=500.0).suffix(" mm"));
            });
        }
        TwodDraft::Engrave(o) => {
            ui.horizontal(|ui| {
                ui.label("Tip Angle");
                ui.add(egui::DragValue::new(&mut o.tip_angle_deg).speed(1.0).range(1.0..=179.0).suffix(" deg"));
            });
        }
        TwodDraft::Chamfer(o) => {
            ui.horizontal(|ui| {
                ui.label("Chamfer Width");
                ui.add(egui::DragValue::new(&mut o.chamfer_width).speed(0.05).range(0.05..=20.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Tip Offset");
                ui.add(egui::DragValue::new(&mut o.tip_offset).speed(0.05).range(0.0..=5.0).suffix(" mm"));
            });
        }
    }

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Add sample point").clicked() {
            let t = draft.targets_mut();
            if t.face_sample_points.len() < solidcraft_manufacture::MAX_SAMPLE_POINTS_PER_OP {
                t.face_sample_points.push([0.0, 0.0, 0.0]);
            }
        }
        if ui.button("Clear").clicked() {
            let t = draft.targets_mut();
            t.face_sample_points.clear();
            t.drill_points.clear();
        }
    });
    let mut remove = None;
    for (i, pt) in draft.targets_mut().face_sample_points.iter_mut().enumerate() {
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
        draft.targets_mut().face_sample_points.remove(i);
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
            egui::ComboBox::from_id_salt(format!("2dh_{label}")).selected_text(plane.from.label()).show_ui(ui, |ui| {
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

fn passes_tab(ui: &mut egui::Ui, draft: &mut TwodDraft) {
    ui.heading("Passes");
    if let Some(p) = draft.pass_opts_mut() {
        ui.horizontal(|ui| {
            ui.label("Tolerance");
            ui.add(egui::DragValue::new(&mut p.tolerance_mm).speed(0.001).range(0.001..=1.0).suffix(" mm"));
        });
        ui.horizontal(|ui| {
            ui.label("Sideways Compensation");
            egui::ComboBox::from_id_salt("2d_sideways").selected_text(p.sideways.label()).show_ui(ui, |ui| {
                for s in [SidewaysCompensation::Left, SidewaysCompensation::Right, SidewaysCompensation::Center] {
                    if ui.selectable_label(p.sideways == s, s.label()).clicked() {
                        p.sideways = s;
                    }
                }
            });
        });
        ui.checkbox(&mut p.multiple_depths, "Multiple Depths");
        if p.multiple_depths {
            ui.horizontal(|ui| {
                ui.label("Max Stepdown");
                ui.add(egui::DragValue::new(&mut p.max_stepdown_mm).speed(0.05).range(0.05..=50.0).suffix(" mm"));
            });
            ui.checkbox(&mut p.use_even_stepdowns, "Use Even Stepdowns");
        }
    } else {
        ui.label("Pass options: use Geometry / Heights for this strategy.");
        ui.horizontal(|ui| {
            ui.label("Depth");
            ui.add(egui::DragValue::new(draft.depth_mut()).speed(0.1).range(0.01..=500.0).suffix(" mm"));
        });
    }
}

fn linking_tab(ui: &mut egui::Ui, linking: &mut ContourLinking) {
    ui.heading("Linking");
    egui::ComboBox::from_id_salt("2d_high_feed").selected_text(linking.high_feed_mode.label()).show_ui(ui, |ui| {
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
        egui::ComboBox::from_id_salt("2d_ramp").selected_text(linking.ramp_type.label()).show_ui(ui, |ui| {
            for r in [RampType::Plunge, RampType::ZigZag, RampType::Helix] {
                if ui.selectable_label(linking.ramp_type == r, r.label()).clicked() {
                    linking.ramp_type = r;
                }
            }
        });
    });
}

fn save(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: TwodDraft) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        return false;
    };
    if op_index >= setup.operations.len() {
        return false;
    }
    setup.operations[op_index] = draft.into_op();
    match serde_json::to_value(&setup.operations) {
        Ok(ops) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops}})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.twod.saved").to_string(), false);
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
    let Some(state) = &app.manufacture_workspace.twod_dialog else {
        return;
    };
    let pts = state.draft.sample_points();
    if pts.is_empty() {
        return;
    }
    let zs = solidcraft_manufacture::resolve_height_zs(state.draft.heights(), 0.0, state.draft.depth());
    let color = egui::Color32::from_rgb(80, 160, 255);
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
