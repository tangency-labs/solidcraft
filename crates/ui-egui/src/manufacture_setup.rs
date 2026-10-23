//! New Setup dialog (Setup / Stock / Post Process) and SETUP menu helpers.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{
    BoxAxisRef, BoxPoint, ManualNc, ManualNcKind, ManufacturingModel, MfgSetup, NcProgram, OpFolder, OpPattern, PatternKind, SetupOperationType,
    SetupPostOpts, Stock, StockOffsetMode, WcsOrientationMode, WcsOriginMode,
};

use crate::{SolveApp, viewport};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SetupDialogTab {
    #[default]
    Setup,
    Stock,
    PostProcess,
}

#[derive(Clone, Debug)]
pub struct SetupDialogState {
    /// Empty when creating; set when editing an existing setup.
    pub existing_name: Option<String>,
    pub name: String,
    pub draft: MfgSetup,
    pub tab: SetupDialogTab,
    /// Viewport clicks add bodies to the setup model.
    pub picking: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupMenuDialog {
    NcProgram,
    Folder,
    Pattern,
    ManualNc,
    ProbeWcs,
    InsertData,
    MfgModel,
}

#[derive(Clone, Debug)]
pub struct MenuDialogState {
    pub kind: SetupMenuDialog,
    pub nc: NcProgram,
    pub folder: OpFolder,
    pub pattern: OpPattern,
    pub manual: ManualNc,
    pub mfg_model: ManufacturingModel,
    pub probe_mode_stock: bool,
    pub probe_offset: u32,
    pub insert_note: String,
}

impl MenuDialogState {
    pub fn new(kind: SetupMenuDialog, active_setup: &str) -> Self {
        let mut nc = NcProgram::default();
        if !active_setup.is_empty() {
            nc.setup_names = vec![active_setup.to_string()];
        }
        Self {
            kind,
            nc,
            folder: OpFolder { setup: active_setup.to_string(), ..Default::default() },
            pattern: OpPattern { setup: active_setup.to_string(), ..Default::default() },
            manual: ManualNc { setup: active_setup.to_string(), ..Default::default() },
            mfg_model: ManufacturingModel::default(),
            probe_mode_stock: true,
            probe_offset: 1,
            insert_note: String::new(),
        }
    }
}

/// Open New Setup (create) or edit the active setup.
pub fn open_new(app: &mut SolveApp) {
    let n = app.session.doc.manufacture.setups.len() + 1;
    let mut draft = MfgSetup::default();
    match app.manufacture_workspace.cam_mode {
        crate::manufacture_workspace::CamMode::Additive => {
            draft.operation_type = SetupOperationType::Additive;
            draft.operations.push(solidcraft_manufacture::Operation::Additive(solidcraft_manufacture::AdditiveOperation::with_kind(
                solidcraft_manufacture::AdditiveKind::GeneratedToolpath,
            )));
        }
        crate::manufacture_workspace::CamMode::Turning => {
            draft.operation_type = SetupOperationType::Turning;
        }
        crate::manufacture_workspace::CamMode::Fabrication => {
            draft.operation_type = SetupOperationType::Cutting;
        }
        _ => {}
    }
    app.manufacture_workspace.setup_dialog =
        Some(SetupDialogState { existing_name: None, name: format!("Setup{n}"), draft, tab: SetupDialogTab::Setup, picking: true });
    app.ui.pick_bodies = true;
    let _ = app.run("select.clear", json!({}));
    app.manufacture_workspace.menu_dialog = None;
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Setup;
}

pub fn open_edit(app: &mut SolveApp, name: &str) {
    let Some(setup) = app.session.doc.manufacture.setups.get(name).cloned() else {
        return;
    };
    app.manufacture_workspace.setup_dialog = Some(SetupDialogState {
        existing_name: Some(name.to_string()),
        name: name.to_string(),
        draft: setup,
        tab: SetupDialogTab::Setup,
        picking: false,
    });
    app.manufacture_workspace.menu_dialog = None;
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Setup;
}

pub fn open_menu(app: &mut SolveApp, kind: SetupMenuDialog) {
    let active = app.manufacture_workspace.active_setup.clone();
    app.manufacture_workspace.setup_dialog = None;
    app.manufacture_workspace.menu_dialog = Some(MenuDialogState::new(kind, &active));
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Setup;
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    show_setup_dialog(app, ui);
    show_menu_dialog(app, ui);
}

fn show_setup_dialog(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(state) = app.manufacture_workspace.setup_dialog.clone() else {
        return;
    };

    let machines: Vec<(String, String)> = Vec::new();
    let posts: Vec<(String, String)> = Vec::new();

    let title = if state.existing_name.is_some() { format!("SETUP : {}", state.name.to_uppercase()) } else { "SETUP".to_string() };
    let title_owned = title.clone();

    let mut draft = state.draft.clone();
    let mut name = state.name.clone();
    let mut tab = state.tab;
    let mut picking = state.picking;

    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_setup_dialog"), "settings", &title_owned).fields().cards(1.2).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                tab_btn(ui, &mut tab, SetupDialogTab::Setup, "Setup");
                tab_btn(ui, &mut tab, SetupDialogTab::Stock, "Stock");
                tab_btn(ui, &mut tab, SetupDialogTab::PostProcess, "Post Process");
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| match tab {
                SetupDialogTab::Setup => setup_tab(ui, app, &mut name, &mut draft, &machines, state.existing_name.is_none(), &mut picking),
                SetupDialogTab::Stock => stock_tab(ui, &mut draft),
                SetupDialogTab::PostProcess => post_tab(ui, &mut draft, &posts),
            });
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;

    if picking {
        for body in selected_body_names(app) {
            if !draft.model_bodies.iter().any(|n| n == &body) {
                draft.model_bodies.push(body);
            }
        }
    }
    if draft.model_bodies != state.draft.model_bodies && matches!(draft.wcs_origin_mode, WcsOriginMode::StockBoxPoint | WcsOriginMode::ModelBoxPoint)
    {
        apply_box_point_to_wcs(app, &mut draft);
    }
    if cancel {
        if state.picking {
            app.ui.pick_bodies = false;
        }
        app.manufacture_workspace.setup_dialog = None;
        return;
    }
    if commit && save_setup(app, state.existing_name.as_deref(), &name, draft.clone()) {
        app.manufacture_workspace.active_setup = name;
        app.manufacture_workspace.setup_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.setup_dialog {
        d.draft = draft;
        d.name = name;
        d.tab = tab;
        d.picking = picking;
    }
}

fn tab_btn(ui: &mut egui::Ui, tab: &mut SetupDialogTab, value: SetupDialogTab, label: &str) {
    let selected = *tab == value;
    let text = if selected { RichText::new(label).strong() } else { RichText::new(label) };
    if ui.selectable_label(selected, text).clicked() {
        *tab = value;
    }
}

fn setup_tab(
    ui: &mut egui::Ui,
    app: &mut SolveApp,
    name: &mut String,
    draft: &mut MfgSetup,
    machines: &[(String, String)],
    name_editable: bool,
    picking: &mut bool,
) {
    ui.heading("Operation");
    if name_editable {
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(name);
        });
    } else {
        ui.label(format!("Name: {name}"));
    }

    ui.horizontal(|ui| {
        ui.label("Operation Type");
        egui::ComboBox::from_id_salt("setup_op_type").selected_text(draft.operation_type.label()).show_ui(ui, |ui| {
            for t in [SetupOperationType::Milling, SetupOperationType::Turning, SetupOperationType::Cutting, SetupOperationType::Additive] {
                if ui.selectable_label(draft.operation_type == t, t.label()).clicked() {
                    draft.operation_type = t;
                }
            }
        });
    });

    ui.horizontal(|ui| {
        ui.label("Machine");
        let cur = draft.machine.clone().unwrap_or_default();
        let label = if cur.is_empty() {
            "(none)".to_string()
        } else {
            machines.iter().find(|(id, _)| *id == cur).map(|(_, n)| n.clone()).unwrap_or(cur.clone())
        };
        egui::ComboBox::from_id_salt("setup_machine").selected_text(label).show_ui(ui, |ui| {
            if ui.selectable_label(cur.is_empty(), "(none)").clicked() {
                draft.machine = None;
            }
            for (id, n) in machines {
                if ui.selectable_label(cur == *id, n).clicked() {
                    draft.machine = Some(id.clone());
                }
            }
        });
    });

    ui.separator();
    ui.heading("Work Coordinate System (WCS)");
    ui.horizontal(|ui| {
        ui.label("Orientation");
        egui::ComboBox::from_id_salt("setup_wcs_orient").selected_text(draft.wcs_orientation.label()).show_ui(ui, |ui| {
            for m in [
                WcsOrientationMode::ModelOrientation,
                WcsOrientationMode::ZAndX,
                WcsOrientationMode::ZAndY,
                WcsOrientationMode::XAndY,
                WcsOrientationMode::CoordinateSystem,
            ] {
                if ui.selectable_label(draft.wcs_orientation == m, m.label()).clicked() {
                    draft.wcs_orientation = m;
                }
            }
        });
    });
    ui.checkbox(&mut draft.flip_z, axis_text("Flip Z Axis", 'Z'));
    ui.checkbox(&mut draft.flip_x, axis_text("Flip X Axis", 'X'));

    let origin_before = draft.wcs_origin_mode;
    ui.horizontal(|ui| {
        ui.label("Origin");
        egui::ComboBox::from_id_salt("setup_wcs_origin").selected_text(draft.wcs_origin_mode.label()).show_ui(ui, |ui| {
            for m in [WcsOriginMode::ModelOrigin, WcsOriginMode::SelectedPoint, WcsOriginMode::StockBoxPoint, WcsOriginMode::ModelBoxPoint] {
                if ui.selectable_label(draft.wcs_origin_mode == m, m.label()).clicked() {
                    draft.wcs_origin_mode = m;
                }
            }
        });
    });

    if origin_before != draft.wcs_origin_mode && draft.wcs_origin_mode == WcsOriginMode::ModelOrigin {
        draft.wcs.origin = [0.0, 0.0, 0.0];
    }
    let mut snap_box =
        origin_before != draft.wcs_origin_mode && matches!(draft.wcs_origin_mode, WcsOriginMode::StockBoxPoint | WcsOriginMode::ModelBoxPoint);
    if matches!(draft.wcs_origin_mode, WcsOriginMode::StockBoxPoint | WcsOriginMode::ModelBoxPoint) {
        ui.label(RichText::new("Box Point").strong());
        snap_box |= box_axis_row(ui, "X", &mut draft.box_point.x);
        snap_box |= box_axis_row(ui, "Y", &mut draft.box_point.y);
        snap_box |= box_axis_row(ui, "Z", &mut draft.box_point.z);
    }
    if snap_box {
        apply_box_point_to_wcs(app, draft);
    }

    ui.separator();
    ui.label(RichText::new("WCS Origin (mm)").strong());
    ui.horizontal(|ui| {
        ui.label(axis_text("X", 'X'));
        let x = ui.add(egui::DragValue::new(&mut draft.wcs.origin[0]).speed(0.5)).changed();
        ui.label(axis_text("Y", 'Y'));
        let y = ui.add(egui::DragValue::new(&mut draft.wcs.origin[1]).speed(0.5)).changed();
        ui.label(axis_text("Z", 'Z'));
        let z = ui.add(egui::DragValue::new(&mut draft.wcs.origin[2]).speed(0.5)).changed();
        if x || y || z {
            draft.wcs_origin_mode = WcsOriginMode::SelectedPoint;
        }
    });
    ui.small("Click a point on the work box, or type X Y Z. Editing the numbers keeps your point.");
    if ui.small_button("Reset axes to model orientation").clicked() {
        draft.wcs.x_axis = [1.0, 0.0, 0.0];
        draft.wcs.y_axis = [0.0, 1.0, 0.0];
        draft.wcs_orientation = WcsOrientationMode::ModelOrientation;
        draft.flip_z = false;
        draft.flip_x = false;
    }

    ui.separator();
    ui.heading("Model");
    ui.label("Select the bodies to machine. That selection is the work area; the WCS origin is relative to it.");
    ui.horizontal(|ui| {
        if ui.selectable_label(*picking, "Select bodies").clicked() {
            *picking = !*picking;
            app.ui.pick_bodies = *picking;
            if *picking {
                let _ = app.run("select.clear", json!({}));
            }
        }
        if ui.button("Clear").clicked() {
            draft.model_bodies.clear();
        }
    });
    if draft.model_bodies.is_empty() {
        ui.colored_label(egui::Color32::from_rgb(220, 160, 60), "No bodies selected.");
    } else {
        let mut remove: Option<usize> = None;
        for (i, body) in draft.model_bodies.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(body);
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            draft.model_bodies.remove(i);
        }
        if let Some((min, max)) = model_bounds(app, &draft.model_bodies) {
            ui.horizontal(|ui| {
                ui.small("Work box");
                ui.small(axis_text(format!("X {:.1}..{:.1}", min[0], max[0]), 'X'));
                ui.small(axis_text(format!("Y {:.1}..{:.1}", min[1], max[1]), 'Y'));
                ui.small(axis_text(format!("Z {:.1}..{:.1}", min[2], max[2]), 'Z'));
                ui.small("mm");
            });
        }
    }
}

/// Arrow length is a fraction of the selected model's longest side, so a small part does not grow a huge triad.
fn triad_len(bounds: ([f64; 3], [f64; 3])) -> f64 {
    let (min, max) = bounds;
    let extent = (0..3).map(|i| (max[i] - min[i]).abs()).fold(0.0_f64, f64::max);
    if extent < 1e-6 { 1.0 } else { extent * 0.28 }
}

/// X red, Y green, Z blue — the same colors as the viewport origin and the WCS triad.
fn axis_text(label: impl Into<String>, axis: char) -> RichText {
    let t = crate::theme::Tokens::get();
    let color = match axis {
        'X' => t.axis_x,
        'Y' => t.axis_y,
        _ => t.axis_z,
    };
    RichText::new(label).color(color)
}

fn box_axis_row(ui: &mut egui::Ui, label: &str, axis: &mut BoxAxisRef) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(axis_text(label, label.chars().next().unwrap_or('X')));
        for v in [BoxAxisRef::Negative, BoxAxisRef::Center, BoxAxisRef::Positive] {
            if ui.selectable_label(*axis == v, v.label()).clicked() {
                *axis = v;
                changed = true;
            }
        }
    });
    changed
}

fn selected_body_names(app: &SolveApp) -> Vec<String> {
    let mut names = Vec::new();
    for sel in &app.session.selection {
        let name = match sel {
            solidcraft_engine::Sel::Body { name } => Some(name.clone()),
            solidcraft_engine::Sel::Face { body, .. } | solidcraft_engine::Sel::Edge { body, .. } | solidcraft_engine::Sel::Vertex { body, .. } => {
                Some(body.clone())
            }
            _ => None,
        };
        if let Some(name) = name
            && !names.iter().any(|n| n == &name)
        {
            names.push(name);
        }
    }
    names
}

fn model_bounds(app: &SolveApp, names: &[String]) -> Option<([f64; 3], [f64; 3])> {
    let st = app.session.world_state();
    let mut box3 = solidcraft_engine::geom::Aabb3::EMPTY;
    for name in names {
        if let Some(body) = st.body(name) {
            box3 = box3.union(&body.mesh().bounds());
        }
    }
    if box3.is_empty() { None } else { Some(([box3.min.x, box3.min.y, box3.min.z], [box3.max.x, box3.max.y, box3.max.z])) }
}

fn apply_box_point_to_wcs(app: &SolveApp, draft: &mut MfgSetup) {
    let model = model_bounds(app, &draft.model_bodies);
    let (min, max) = if matches!(draft.wcs_origin_mode, WcsOriginMode::ModelBoxPoint) {
        match model {
            Some(b) => b,
            None => return,
        }
    } else {
        stock_box(draft, model)
    };
    let map = |r: BoxAxisRef, neg: f64, pos: f64| match r {
        BoxAxisRef::Negative => neg,
        BoxAxisRef::Center => (neg + pos) * 0.5,
        BoxAxisRef::Positive => pos,
    };
    draft.wcs.origin = [map(draft.box_point.x, min[0], max[0]), map(draft.box_point.y, min[1], max[1]), map(draft.box_point.z, min[2], max[2])];
}

fn stock_box(draft: &MfgSetup, model: Option<([f64; 3], [f64; 3])>) -> ([f64; 3], [f64; 3]) {
    if let Some((min, max)) = model {
        return match &draft.stock {
            Stock::RelativeBox { side_offset, top_offset, bottom_offset, ground_at_model_origin, .. } => {
                let mut lo = [min[0] - side_offset, min[1] - side_offset, min[2] - bottom_offset];
                let mut hi = [max[0] + side_offset, max[1] + side_offset, max[2] + top_offset];
                if *ground_at_model_origin {
                    let height = hi[2] - lo[2];
                    lo[2] = 0.0;
                    hi[2] = height;
                }
                (lo, hi)
            }
            Stock::RelativeCylinder { radial_offset, front_offset, back_offset, .. } => (
                [min[0] - radial_offset, min[1] - radial_offset, min[2] - back_offset],
                [max[0] + radial_offset, max[1] + radial_offset, max[2] + front_offset],
            ),
            _ => (min, max),
        };
    }
    let (w, d, h) = match &draft.stock {
        Stock::Box { width, depth, height } => (*width, *depth, *height),
        Stock::Cylinder { diameter, height } | Stock::Tube { outer_diameter: diameter, height, .. } => (*diameter, *diameter, *height),
        _ => (100.0, 100.0, 25.0),
    };
    ([0.0, 0.0, 0.0], [w, d, h])
}

fn stock_tab(ui: &mut egui::Ui, draft: &mut MfgSetup) {
    ui.heading("Stock");
    let mode_label = draft.stock.mode_label();
    egui::ComboBox::from_id_salt("setup_stock_mode").selected_text(mode_label).show_ui(ui, |ui| {
        if ui.selectable_label(matches!(draft.stock, Stock::Box { .. }), "Fixed size box").clicked() {
            draft.stock = Stock::Box { width: 100.0, depth: 100.0, height: 25.0 };
        }
        if ui.selectable_label(matches!(draft.stock, Stock::RelativeBox { .. }), "Relative size box").clicked() {
            draft.stock = Stock::RelativeBox {
                mode: StockOffsetMode::SidesAndTopBottom,
                side_offset: 2.0,
                top_offset: 1.0,
                bottom_offset: 0.0,
                round_up: 0.0,
                ground_at_model_origin: false,
            };
        }
        if ui.selectable_label(matches!(draft.stock, Stock::Cylinder { .. }), "Fixed size cylinder").clicked() {
            draft.stock = Stock::Cylinder { diameter: 50.0, height: 80.0 };
        }
        if ui.selectable_label(matches!(draft.stock, Stock::RelativeCylinder { .. }), "Relative size cylinder").clicked() {
            draft.stock = Stock::RelativeCylinder { radial_offset: 2.0, front_offset: 1.0, back_offset: 1.0, round_up: 0.0 };
        }
        if ui.selectable_label(matches!(draft.stock, Stock::Tube { .. }), "Fixed size tube").clicked() {
            draft.stock = Stock::Tube { outer_diameter: 50.0, inner_diameter: 30.0, height: 80.0 };
        }
        if ui.selectable_label(matches!(draft.stock, Stock::FromPreceding { .. }), "From preceding setup").clicked() {
            draft.stock = Stock::FromPreceding { continue_rest: true };
        }
        if ui.selectable_label(matches!(draft.stock, Stock::FromSolid { .. }), "From solid").clicked() {
            draft.stock = Stock::FromSolid { body: String::new() };
        }
    });

    ui.separator();
    match &mut draft.stock {
        Stock::Box { width, depth, height } => {
            ui.horizontal(|ui| {
                ui.label("Width (X)");
                ui.add(egui::DragValue::new(width).speed(0.5).range(0.1..=10_000.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Depth (Y)");
                ui.add(egui::DragValue::new(depth).speed(0.5).range(0.1..=10_000.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Height (Z)");
                ui.add(egui::DragValue::new(height).speed(0.5).range(0.1..=10_000.0).suffix(" mm"));
            });
        }
        Stock::RelativeBox { mode, side_offset, top_offset, bottom_offset, round_up, ground_at_model_origin } => {
            egui::ComboBox::from_id_salt("rel_box_mode").selected_text(mode.label()).show_ui(ui, |ui| {
                for m in [StockOffsetMode::NoAdditional, StockOffsetMode::SidesAndTopBottom, StockOffsetMode::AllSides] {
                    if ui.selectable_label(*mode == m, m.label()).clicked() {
                        *mode = m;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Side offset");
                ui.add(egui::DragValue::new(side_offset).speed(0.1).range(0.0..=500.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Top offset");
                ui.add(egui::DragValue::new(top_offset).speed(0.1).range(0.0..=500.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Bottom offset");
                ui.add(egui::DragValue::new(bottom_offset).speed(0.1).range(0.0..=500.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Round up to nearest");
                ui.add(egui::DragValue::new(round_up).speed(0.1).range(0.0..=50.0).suffix(" mm"));
            });
            ui.checkbox(ground_at_model_origin, "Ground stock at model origin");
        }
        Stock::Cylinder { diameter, height } => {
            ui.horizontal(|ui| {
                ui.label("Diameter");
                ui.add(egui::DragValue::new(diameter).speed(0.5).range(0.1..=10_000.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Length");
                ui.add(egui::DragValue::new(height).speed(0.5).range(0.1..=10_000.0).suffix(" mm"));
            });
        }
        Stock::RelativeCylinder { radial_offset, front_offset, back_offset, round_up } => {
            ui.horizontal(|ui| {
                ui.label("Radial stock offset");
                ui.add(egui::DragValue::new(radial_offset).speed(0.1).range(0.0..=500.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Frontside offset");
                ui.add(egui::DragValue::new(front_offset).speed(0.1).range(0.0..=500.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Backside offset");
                ui.add(egui::DragValue::new(back_offset).speed(0.1).range(0.0..=500.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Round up to nearest");
                ui.add(egui::DragValue::new(round_up).speed(0.1).range(0.0..=50.0).suffix(" mm"));
            });
        }
        Stock::Tube { outer_diameter, inner_diameter, height } => {
            ui.horizontal(|ui| {
                ui.label("Outer diameter");
                ui.add(egui::DragValue::new(outer_diameter).speed(0.5).range(0.1..=10_000.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Inner diameter");
                ui.add(egui::DragValue::new(inner_diameter).speed(0.5).range(0.1..=10_000.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Length");
                ui.add(egui::DragValue::new(height).speed(0.5).range(0.1..=10_000.0).suffix(" mm"));
            });
        }
        Stock::FromPreceding { continue_rest } => {
            ui.checkbox(continue_rest, "Continue rest machining");
            ui.small("Uses stock remaining from the previous setup.");
        }
        Stock::FromSolid { body } => {
            ui.horizontal(|ui| {
                ui.label("Body");
                ui.text_edit_singleline(body);
            });
            ui.small("Stock is the named solid in the design.");
        }
    }
}

fn post_tab(ui: &mut egui::Ui, draft: &mut MfgSetup, posts: &[(String, String)]) {
    ui.heading("Post Process");
    ui.horizontal(|ui| {
        ui.label("Post");
        let cur = draft.post.clone().unwrap_or_default();
        let label = if cur.is_empty() {
            "(none)".to_string()
        } else {
            posts.iter().find(|(id, _)| *id == cur).map(|(_, n)| n.clone()).unwrap_or(cur.clone())
        };
        egui::ComboBox::from_id_salt("setup_post").selected_text(label).show_ui(ui, |ui| {
            if ui.selectable_label(cur.is_empty(), "(none)").clicked() {
                draft.post = None;
            }
            for (id, n) in posts {
                if ui.selectable_label(cur == *id, n).clicked() {
                    draft.post = Some(id.clone());
                }
            }
        });
    });

    let opts: &mut SetupPostOpts = &mut draft.post_opts;
    ui.horizontal(|ui| {
        ui.label("Program Name/Number");
        ui.text_edit_singleline(&mut opts.program_name);
    });
    ui.horizontal(|ui| {
        ui.label("Program Comment");
        ui.text_edit_singleline(&mut opts.program_comment);
    });
    ui.horizontal(|ui| {
        ui.label("WCS Offset");
        ui.add(egui::DragValue::new(&mut opts.wcs_offset).speed(1).range(0..=99));
        ui.small(wcs_offset_hint(opts.wcs_offset));
    });
    ui.checkbox(&mut opts.multiple_wcs_offsets, "Multiple WCS Offsets");
}

fn wcs_offset_hint(n: u32) -> &'static str {
    match n {
        0 | 1 => "→ G54",
        2 => "→ G55",
        3 => "→ G56",
        4 => "→ G57",
        5 => "→ G58",
        6 => "→ G59",
        _ => "→ extended offset",
    }
}

fn save_setup(app: &mut SolveApp, _existing: Option<&str>, name: &str, draft: MfgSetup) -> bool {
    let name = name.trim();
    if name.is_empty() {
        app.set_status("Setup name required", true);
        return false;
    }
    if draft.model_bodies.is_empty() {
        app.set_status("Select the bodies for this setup. The setup has no work area until those bodies are chosen.", true);
        return false;
    }

    let Ok(stock) = serde_json::to_value(&draft.stock) else {
        return false;
    };
    let Ok(wcs) = serde_json::to_value(&draft.wcs) else {
        return false;
    };
    let Ok(tools) = serde_json::to_value(&draft.tools) else {
        return false;
    };
    let Ok(operations) = serde_json::to_value(&draft.operations) else {
        return false;
    };
    let Ok(operation_type) = serde_json::to_value(draft.operation_type) else {
        return false;
    };
    let Ok(wcs_orientation) = serde_json::to_value(draft.wcs_orientation) else {
        return false;
    };
    let Ok(wcs_origin_mode) = serde_json::to_value(draft.wcs_origin_mode) else {
        return false;
    };
    let Ok(box_point) = serde_json::to_value(draft.box_point) else {
        return false;
    };
    let Ok(post_opts) = serde_json::to_value(&draft.post_opts) else {
        return false;
    };

    let mut set = json!({
        "stock": stock,
        "wcs": wcs,
        "tools": tools,
        "operations": operations,
        "operation_type": operation_type,
        "wcs_orientation": wcs_orientation,
        "wcs_origin_mode": wcs_origin_mode,
        "box_point": box_point,
        "flip_z": draft.flip_z,
        "flip_x": draft.flip_x,
        "model_bodies": draft.model_bodies,
        "post_opts": post_opts,
    });
    if let Some(obj) = set.as_object_mut() {
        obj.insert("machine".into(), draft.machine.clone().map(serde_json::Value::String).unwrap_or(serde_json::Value::Null));
        obj.insert("post".into(), draft.post.clone().map(serde_json::Value::String).unwrap_or(serde_json::Value::Null));
    }

    match app.run("manufacture.settings.set", json!({"name": name, "set": set})) {
        Ok(_) => {
            app.manufacture_workspace.revision = u64::MAX;
            app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
            app.set_status(app.tr("manufacture.setup.saved").to_string(), false);
            true
        }
        Err(e) => {
            app.set_status(e, true);
            false
        }
    }
}

fn show_menu_dialog(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(state) = app.manufacture_workspace.menu_dialog.clone() else {
        return;
    };
    let title = match state.kind {
        SetupMenuDialog::NcProgram => "CREATE NC PROGRAM",
        SetupMenuDialog::Folder => "NEW FOLDER",
        SetupMenuDialog::Pattern => "NEW PATTERN",
        SetupMenuDialog::ManualNc => "MANUAL NC",
        SetupMenuDialog::ProbeWcs => "PROBE WCS",
        SetupMenuDialog::InsertData => "INSERT MANUFACTURING DATA",
        SetupMenuDialog::MfgModel => "CREATE MANUFACTURING MODEL",
    };

    let mut st = state.clone();
    let setup_names: Vec<String> = app.session.doc.manufacture.setups.keys().cloned().collect();
    let posts: Vec<(String, String)> = Vec::new();
    let title_owned = title.to_string();

    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new(("mfg_setup_menu", title)), "settings", &title_owned).fields().cards(1.1).show(
        ui.ctx(),
        true,
        |ui| {
            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| match st.kind {
                SetupMenuDialog::NcProgram => {
                    ui.horizontal(|ui| {
                        ui.label("Name/Number");
                        ui.text_edit_singleline(&mut st.nc.name);
                    });
                    ui.horizontal(|ui| {
                        ui.label("File Name");
                        ui.text_edit_singleline(&mut st.nc.file_name);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Comment");
                        ui.text_edit_singleline(&mut st.nc.comment);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Post");
                        let cur = st.nc.post.clone().unwrap_or_default();
                        let label = if cur.is_empty() { "(setup default)".into() } else { cur.clone() };
                        egui::ComboBox::from_id_salt("nc_post").selected_text(label).show_ui(ui, |ui| {
                            if ui.selectable_label(cur.is_empty(), "(setup default)").clicked() {
                                st.nc.post = None;
                            }
                            for (id, n) in &posts {
                                if ui.selectable_label(cur == *id, n).clicked() {
                                    st.nc.post = Some(id.clone());
                                }
                            }
                        });
                    });
                    ui.label("Setups included:");
                    for sn in &setup_names {
                        let mut on = st.nc.setup_names.iter().any(|s| s == sn);
                        if ui.checkbox(&mut on, sn).changed() {
                            if on {
                                st.nc.setup_names.push(sn.clone());
                            } else {
                                st.nc.setup_names.retain(|s| s != sn);
                            }
                        }
                    }
                }
                SetupMenuDialog::Folder => {
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut st.folder.name);
                    });
                    setup_pick(ui, "Setup", &mut st.folder.setup, &setup_names);
                }
                SetupMenuDialog::Pattern => {
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut st.pattern.name);
                    });
                    setup_pick(ui, "Setup", &mut st.pattern.setup, &setup_names);
                    ui.horizontal(|ui| {
                        ui.label("Type");
                        egui::ComboBox::from_id_salt("pat_kind").selected_text(st.pattern.kind.label()).show_ui(ui, |ui| {
                            for k in [PatternKind::Linear, PatternKind::Circular, PatternKind::Mirror, PatternKind::Duplication] {
                                if ui.selectable_label(st.pattern.kind == k, k.label()).clicked() {
                                    st.pattern.kind = k;
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Count");
                        ui.add(egui::DragValue::new(&mut st.pattern.count).speed(1).range(1..=64));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Spacing");
                        ui.add(egui::DragValue::new(&mut st.pattern.spacing_mm).speed(0.5).range(0.0..=10_000.0).suffix(" mm"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Angle");
                        ui.add(egui::DragValue::new(&mut st.pattern.angle_deg).speed(1.0).range(0.0..=360.0).suffix(" deg"));
                    });
                }
                SetupMenuDialog::ManualNc => {
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut st.manual.name);
                    });
                    setup_pick(ui, "Setup", &mut st.manual.setup, &setup_names);
                    ui.horizontal(|ui| {
                        ui.label("Type");
                        egui::ComboBox::from_id_salt("manual_kind").selected_text(st.manual.kind.label()).show_ui(ui, |ui| {
                            for k in
                                [ManualNcKind::Comment, ManualNcKind::Stop, ManualNcKind::OptionalStop, ManualNcKind::ToolBreak, ManualNcKind::Custom]
                            {
                                if ui.selectable_label(st.manual.kind == k, k.label()).clicked() {
                                    st.manual.kind = k;
                                }
                            }
                        });
                    });
                    ui.label("Code / comment");
                    ui.text_edit_multiline(&mut st.manual.code);
                }
                SetupMenuDialog::ProbeWcs => {
                    ui.label("Probe WCS updates the setup work offset from a probe cycle.");
                    ui.checkbox(&mut st.probe_mode_stock, "Probe stock surfaces (else model)");
                    ui.horizontal(|ui| {
                        ui.label("WCS Offset");
                        ui.add(egui::DragValue::new(&mut st.probe_offset).speed(1).range(0..=99));
                    });
                    ui.small("Uses the setup probe tool when present. Probe G-code snippets land in the probe PR.");
                }
                SetupMenuDialog::InsertData => {
                    ui.label("Import manufacturing data (tools, fixtures, templates) into this document.");
                    ui.text_edit_multiline(&mut st.insert_note);
                    ui.small("Paste a note or reference path; full import pipelines land in a follow-up PR.");
                }
                SetupMenuDialog::MfgModel => {
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut st.mfg_model.name);
                    });
                    ui.label("Note");
                    ui.text_edit_multiline(&mut st.mfg_model.note);
                    ui.small("Manufacturing models isolate stock/fixture edits from the design model.");
                }
            });
        },
    );

    if closed || action == crate::ui_kit::Action::Cancel {
        app.manufacture_workspace.menu_dialog = None;
        return;
    }
    if action == crate::ui_kit::Action::Ok && save_menu(app, &st) {
        app.manufacture_workspace.menu_dialog = None;
        return;
    }
    app.manufacture_workspace.menu_dialog = Some(st);
}

fn setup_pick(ui: &mut egui::Ui, label: &str, current: &mut String, names: &[String]) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(("setup_pick", label)).selected_text(current.as_str()).show_ui(ui, |ui| {
            for n in names {
                if ui.selectable_label(current == n, n).clicked() {
                    *current = n.clone();
                }
            }
        });
    });
}

fn save_menu(app: &mut SolveApp, st: &MenuDialogState) -> bool {
    let payload = match st.kind {
        SetupMenuDialog::NcProgram => json!({"nc_program": st.nc}),
        SetupMenuDialog::Folder => json!({"folder": st.folder}),
        SetupMenuDialog::Pattern => json!({"pattern": st.pattern}),
        SetupMenuDialog::ManualNc => json!({"manual_nc": st.manual}),
        SetupMenuDialog::MfgModel => json!({"manufacturing_model": st.mfg_model}),
        SetupMenuDialog::ProbeWcs => {
            app.set_status("Probe WCS snippets land in the probe PR (#2626).", true);
            return false;
        }
        SetupMenuDialog::InsertData => {
            app.set_status(
                if st.insert_note.is_empty() {
                    "Insert Manufacturing Data: add a note or path, then OK.".into()
                } else {
                    format!("Manufacturing data noted: {}", st.insert_note.chars().take(80).collect::<String>())
                },
                false,
            );
            return true;
        }
    };
    let _ = payload;
    app.set_status("NC program, folder, pattern and manufacturing-model containers land in follow-up Manufacture PRs.", true);
    false
}

/// Draw WCS triad + stock box outline for the setup being edited, or for the setup selected in the browser.
/// Origin pick points appear only while the setup dialog is open.
pub fn paint_overlays(app: &SolveApp, painter: &egui::Painter, proj: &viewport::Proj) {
    if let Some(state) = &app.manufacture_workspace.setup_dialog {
        paint_setup(app, painter, proj, &state.draft, true);
        return;
    }
    if !crate::manufacture_workspace::active(app) {
        return;
    }
    let Some(setup) = app.session.doc.manufacture.setups.get(&app.manufacture_workspace.active_setup) else {
        return;
    };
    if setup.model_bodies.is_empty() {
        return;
    }
    paint_setup(app, painter, proj, setup, false);
}

fn paint_setup(app: &SolveApp, painter: &egui::Painter, proj: &viewport::Proj, setup: &MfgSetup, origin_picks: bool) {
    let wcs = &setup.wcs;
    let model = model_bounds(app, &setup.model_bodies);
    let (min, max) = stock_box(setup, model);
    let t = crate::theme::Tokens::get();
    let o = solidcraft_engine::geom::Vec3::new(wcs.origin[0], wcs.origin[1], wcs.origin[2]);
    let len = triad_len(model.unwrap_or((min, max)));
    let axis = |v: [f64; 3]| solidcraft_engine::geom::Vec3::new(v[0] * len, v[1] * len, v[2] * len);
    if let Some(origin) = proj.to_screen(o) {
        for (dir, color) in [
            (wcs.x_axis, t.axis_x),
            (wcs.y_axis, t.axis_y),
            (
                [
                    wcs.x_axis[1] * wcs.y_axis[2] - wcs.x_axis[2] * wcs.y_axis[1],
                    wcs.x_axis[2] * wcs.y_axis[0] - wcs.x_axis[0] * wcs.y_axis[2],
                    wcs.x_axis[0] * wcs.y_axis[1] - wcs.x_axis[1] * wcs.y_axis[0],
                ],
                t.axis_z,
            ),
        ] {
            if let Some(tip) = proj.to_screen(o + axis(dir)) {
                painter.arrow(origin, tip - origin, egui::Stroke::new(1.5, color));
            }
        }
    }

    let corners = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ];
    let edges = [[0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7]];
    let stock_color = egui::Color32::from_rgba_unmultiplied(255, 180, 60, 180);
    for e in edges {
        let a = corners[e[0]];
        let b = corners[e[1]];
        let wa = solidcraft_engine::geom::Vec3::new(a[0], a[1], a[2]);
        let wb = solidcraft_engine::geom::Vec3::new(b[0], b[1], b[2]);
        if let (Some(sa), Some(sb)) = (proj.to_screen(wa), proj.to_screen(wb)) {
            painter.line_segment([sa, sb], egui::Stroke::new(1.5, stock_color));
        }
    }
    if !origin_picks {
        return;
    }
    let hover = painter.ctx().input(|i| i.pointer.hover_pos());
    let origin = setup.wcs.origin;
    for (_bp, world) in box_points(min, max) {
        let p = solidcraft_engine::geom::Vec3::new(world[0], world[1], world[2]);
        let Some(sp) = proj.to_screen(p) else { continue };
        let active = (origin[0] - world[0]).abs() < 1e-3 && (origin[1] - world[1]).abs() < 1e-3 && (origin[2] - world[2]).abs() < 1e-3;
        let hot = hover.is_some_and(|h| h.distance(sp) <= 14.0);
        let color = if active {
            egui::Color32::from_rgb(80, 160, 255)
        } else if hot {
            egui::Color32::from_rgb(255, 255, 255)
        } else {
            egui::Color32::from_rgb(255, 210, 80)
        };
        painter.circle_filled(sp, if active || hot { 6.0 } else { 4.0 }, color);
        painter.circle_stroke(sp, if active || hot { 6.0 } else { 4.0 }, egui::Stroke::new(1.0, egui::Color32::BLACK));
    }
}

/// Click a work-box point (corner, edge midpoint, face center, or box center) to set the WCS origin.
pub fn try_pick_origin(app: &mut SolveApp, proj: &viewport::Proj, screen: egui::Pos2) -> bool {
    let Some(state) = app.manufacture_workspace.setup_dialog.as_ref() else {
        return false;
    };
    let model = model_bounds(app, &state.draft.model_bodies);
    let (min, max) = stock_box(&state.draft, model);
    let Some((bp, world)) = nearest_box_point(proj, screen, min, max, 14.0) else {
        return false;
    };
    let Some(state) = app.manufacture_workspace.setup_dialog.as_mut() else {
        return false;
    };
    state.draft.box_point = bp;
    state.draft.wcs.origin = world;
    state.draft.wcs_origin_mode = WcsOriginMode::StockBoxPoint;
    app.set_status(format!("WCS origin {:.2}, {:.2}, {:.2} mm", world[0], world[1], world[2]), false);
    true
}

fn box_points(min: [f64; 3], max: [f64; 3]) -> Vec<(BoxPoint, [f64; 3])> {
    let axis = |neg: f64, pos: f64| [(BoxAxisRef::Negative, neg), (BoxAxisRef::Center, (neg + pos) * 0.5), (BoxAxisRef::Positive, pos)];
    let xs = axis(min[0], max[0]);
    let ys = axis(min[1], max[1]);
    let zs = axis(min[2], max[2]);
    let mut out = Vec::with_capacity(27);
    for (x, xv) in xs {
        for (y, yv) in ys {
            for (z, zv) in zs {
                out.push((BoxPoint { x, y, z }, [xv, yv, zv]));
            }
        }
    }
    out
}

fn nearest_box_point(proj: &viewport::Proj, screen: egui::Pos2, min: [f64; 3], max: [f64; 3], radius: f32) -> Option<(BoxPoint, [f64; 3])> {
    let mut best: Option<(f32, BoxPoint, [f64; 3])> = None;
    for (bp, world) in box_points(min, max) {
        let p = solidcraft_engine::geom::Vec3::new(world[0], world[1], world[2]);
        let Some(sp) = proj.to_screen(p) else { continue };
        let d = sp.distance(screen);
        if d <= radius && best.is_none_or(|(bd, _, _)| d < bd) {
            best = Some((d, bp, world));
        }
    }
    best.map(|(_, bp, world)| (bp, world))
}
