//! New Setup dialog (Setup / Stock / Post Process) and WCS overlays.
//!
//! Recut of #1782 onto `cam.setup`. Bodies, yellow work box, clickable WCS origin on the box.

use serde_json::json;
use solidcraft_engine::doc::cam::{
    BoxAxisRef, BoxPoint, CamSetup, CamStock, SetupOperationType, StockOffsetMode, WcsOrientationMode, WcsOriginMode, WorkCoordinateSystem,
};

use crate::ui_kit::{self, Button};
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
    pub existing_name: Option<String>,
    pub name: String,
    pub draft: CamSetup,
    pub tab: SetupDialogTab,
    pub picking: bool,
}

pub fn open_new(app: &mut SolveApp) {
    crate::manufacture_workspace::enter(app);
    let mut n = app.session.doc.cam.setups.len() + 1;
    let name = loop {
        let candidate = format!("Setup{n}");
        if app.session.doc.cam.setup(&candidate).is_none() {
            break candidate;
        }
        n += 1;
        if n > 64 {
            break "Setup".into();
        }
    };
    let mut draft = CamSetup::new(&name);
    draft.wcs_origin_mode = WcsOriginMode::SelectedPoint;
    app.manufacture_workspace.setup_dialog = Some(SetupDialogState { existing_name: None, name, draft, tab: SetupDialogTab::Setup, picking: true });
    app.ui.pick_bodies = true;
    let _ = app.run("select.clear", json!({}));
}

pub fn dialog_open(app: &SolveApp) -> bool {
    app.manufacture_workspace.setup_dialog.is_some()
}

/// Commit the open SETUP window the same way its OK button does.
pub fn commit(app: &mut SolveApp) {
    let Some(state) = app.manufacture_workspace.setup_dialog.clone() else {
        return;
    };
    if save_setup(app, &state.name, state.draft) {
        app.manufacture_workspace.active_setup = state.name;
        app.ui.pick_bodies = false;
        app.manufacture_workspace.setup_dialog = None;
    }
}

/// Dismiss the open SETUP window the same way its Cancel button does.
pub fn cancel(app: &mut SolveApp) {
    if let Some(state) = app.manufacture_workspace.setup_dialog.take()
        && state.picking
    {
        app.ui.pick_bodies = false;
    }
}

pub fn open_edit(app: &mut SolveApp, name: &str) {
    crate::manufacture_workspace::enter(app);
    let Some(setup) = app.session.doc.cam.setup(name).cloned() else {
        return;
    };
    app.manufacture_workspace.setup_dialog = Some(SetupDialogState {
        existing_name: Some(name.to_string()),
        name: name.to_string(),
        draft: setup,
        tab: SetupDialogTab::Setup,
        picking: false,
    });
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(state) = app.manufacture_workspace.setup_dialog.clone() else {
        return;
    };
    let title_owned = if state.existing_name.is_some() {
        format!("{} : {}", crate::i18n::tr("ui.heading_setup"), state.name.to_uppercase())
    } else {
        crate::i18n::tr("ui.heading_setup").to_string()
    };

    let mut draft = state.draft.clone();
    let mut name = state.name.clone();
    let mut tab = state.tab;
    let mut picking = state.picking;
    let name_editable = state.existing_name.is_none();

    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_setup_dialog"), "settings", &title_owned).fields().cards(1.2).show(ui.ctx(), true, |ui| {
            crate::ui_kit::scope(ui, |ui| {
                let mut tab_idx = match tab {
                    SetupDialogTab::Setup => 0,
                    SetupDialogTab::Stock => 1,
                    SetupDialogTab::PostProcess => 2,
                };
                ui_kit::segmented(ui, &mut tab_idx, &["Setup", "Stock", "Post Process"], Default::default());
                tab = match tab_idx {
                    1 => SetupDialogTab::Stock,
                    2 => SetupDialogTab::PostProcess,
                    _ => SetupDialogTab::Setup,
                };
                ui.separator();
                match tab {
                    SetupDialogTab::Setup => setup_tab(ui, app, &mut name, &mut draft, name_editable, &mut picking),
                    SetupDialogTab::Stock => stock_tab(ui, &mut draft),
                    SetupDialogTab::PostProcess => post_tab(ui, &mut draft),
                }
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
    if commit && save_setup(app, &name, draft.clone()) {
        app.manufacture_workspace.active_setup = name;
        app.ui.pick_bodies = false;
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

fn setup_tab(ui: &mut egui::Ui, app: &mut SolveApp, name: &mut String, draft: &mut CamSetup, name_editable: bool, picking: &mut bool) {
    ui_kit::section(ui, "setup_operation", "Operation", true, |ui| {
        if name_editable {
            ui_kit::text_row(ui, "Name", name, "Setup1");
        } else {
            ui_kit::text(ui, name, ui_kit::Text::Note("setup_name"));
        }
        let types = [SetupOperationType::Milling, SetupOperationType::Turning, SetupOperationType::Cutting, SetupOperationType::Additive];
        let labels: Vec<&str> = types.iter().map(|t| t.label()).collect();
        let mut idx = types.iter().position(|t| *t == draft.operation_type).unwrap_or(0);
        ui_kit::choice_row(ui, "Operation Type", "setup_op_type", &mut idx, &labels);
        draft.operation_type = types[idx];
    });

    ui_kit::section(ui, "setup_wcs", "Work Coordinate System (WCS)", true, |ui| {
        let orients = [
            WcsOrientationMode::ModelOrientation,
            WcsOrientationMode::ZAndX,
            WcsOrientationMode::ZAndY,
            WcsOrientationMode::XAndY,
            WcsOrientationMode::CoordinateSystem,
        ];
        let olabels: Vec<&str> = orients.iter().map(|m| m.label()).collect();
        let mut oidx = orients.iter().position(|m| *m == draft.wcs_orientation).unwrap_or(0);
        ui_kit::choice_row(ui, "Orientation", "setup_wcs_orient", &mut oidx, &olabels);
        draft.wcs_orientation = orients[oidx];
        ui_kit::checkbox(ui, "Flip Z Axis", &mut draft.flip_z);
        ui_kit::checkbox(ui, "Flip X Axis", &mut draft.flip_x);

        let origin_before = draft.wcs_origin_mode;
        let origins = [WcsOriginMode::ModelOrigin, WcsOriginMode::SelectedPoint, WcsOriginMode::StockBoxPoint, WcsOriginMode::ModelBoxPoint];
        let glabels: Vec<&str> = origins.iter().map(|m| m.label()).collect();
        let mut gidx = origins.iter().position(|m| *m == draft.wcs_origin_mode).unwrap_or(0);
        ui_kit::choice_row(ui, "Origin", "setup_wcs_origin", &mut gidx, &glabels);
        draft.wcs_origin_mode = origins[gidx];
        if origin_before != draft.wcs_origin_mode && draft.wcs_origin_mode == WcsOriginMode::ModelOrigin {
            draft.wcs.origin = [0.0, 0.0, 0.0];
        }
        let mut snap_box =
            origin_before != draft.wcs_origin_mode && matches!(draft.wcs_origin_mode, WcsOriginMode::StockBoxPoint | WcsOriginMode::ModelBoxPoint);
        if matches!(draft.wcs_origin_mode, WcsOriginMode::StockBoxPoint | WcsOriginMode::ModelBoxPoint) {
            snap_box |= box_axis_row(ui, "X", &mut draft.box_point.x);
            snap_box |= box_axis_row(ui, "Y", &mut draft.box_point.y);
            snap_box |= box_axis_row(ui, "Z", &mut draft.box_point.z);
        }
        if snap_box {
            apply_box_point_to_wcs(app, draft);
        }

        ui_kit::row(ui, "X", |ui| ui_kit::number_field(ui, &mut draft.wcs.origin[0], -1.0e6..=1.0e6, 0.5, " mm"));
        ui_kit::row(ui, "Y", |ui| ui_kit::number_field(ui, &mut draft.wcs.origin[1], -1.0e6..=1.0e6, 0.5, " mm"));
        ui_kit::row(ui, "Z", |ui| {
            let r = ui_kit::number_field(ui, &mut draft.wcs.origin[2], -1.0e6..=1.0e6, 0.5, " mm");
            if r.changed() {
                draft.wcs_origin_mode = WcsOriginMode::SelectedPoint;
            }
            r
        });
        ui_kit::text(ui, "Click a point on the work box, or type X Y Z. Editing the numbers keeps your point.", ui_kit::Text::Hint("wcs_hint"));
        if ui_kit::button(ui, "Reset axes to model orientation", Button::Ghost).clicked() {
            draft.wcs.x_axis = [1.0, 0.0, 0.0];
            draft.wcs.y_axis = [0.0, 1.0, 0.0];
            draft.wcs_orientation = WcsOrientationMode::ModelOrientation;
            draft.flip_z = false;
            draft.flip_x = false;
        }
    });

    ui_kit::section(ui, "setup_model", "Model", true, |ui| {
        ui_kit::text(
            ui,
            "Select the bodies to machine. That selection is the work area; the WCS origin is relative to it.",
            ui_kit::Text::Hint("model_hint"),
        );
        ui.horizontal(|ui| {
            let pick_label = if *picking { "Selecting bodies" } else { "Select bodies" };
            if ui_kit::button(ui, pick_label, if *picking { Button::Primary } else { Button::Ghost }).clicked() {
                *picking = !*picking;
                app.ui.pick_bodies = *picking;
                if *picking {
                    let _ = app.run("select.clear", json!({}));
                }
            }
            if ui_kit::button(ui, "Clear", Button::Ghost).clicked() {
                draft.model_bodies.clear();
            }
        });
        if draft.model_bodies.is_empty() {
            ui_kit::text(ui, "No bodies selected.", ui_kit::Text::Note("no_bodies"));
        } else {
            let mut remove: Option<usize> = None;
            for (i, body) in draft.model_bodies.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui_kit::text(ui, body, ui_kit::Text::Inline);
                    if ui_kit::button(ui, "Remove", Button::Small).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                draft.model_bodies.remove(i);
            }
        }
    });
}

fn box_axis_row(ui: &mut egui::Ui, label: &str, axis: &mut BoxAxisRef) -> bool {
    let opts = [BoxAxisRef::Negative, BoxAxisRef::Center, BoxAxisRef::Positive];
    let labels = ["−", "Center", "+"];
    let mut idx = opts.iter().position(|v| *v == *axis).unwrap_or(0);
    ui_kit::choice_row(ui, label, &format!("box_axis_{label}"), &mut idx, &labels);
    let changed = opts[idx] != *axis;
    *axis = opts[idx];
    changed
}

fn stock_tab(ui: &mut egui::Ui, draft: &mut CamSetup) {
    ui_kit::section(ui, "setup_stock", "Stock", true, |ui| {
        let modes = [
            "Fixed size box",
            "Relative size box",
            "Fixed size cylinder",
            "Relative size cylinder",
            "Fixed size tube",
            "From preceding setup",
            "From solid",
        ];
        let mut idx = match &draft.stock_spec {
            CamStock::Box { .. } => 0,
            CamStock::RelativeBox { .. } => 1,
            CamStock::Cylinder { .. } => 2,
            CamStock::RelativeCylinder { .. } => 3,
            CamStock::Tube { .. } => 4,
            CamStock::FromPreceding { .. } => 5,
            CamStock::FromSolid { .. } => 6,
        };
        ui_kit::choice_row(ui, "Stock", "setup_stock_mode", &mut idx, &modes);
        draft.stock_spec = match idx {
            0 => match &draft.stock_spec {
                CamStock::Box { .. } => draft.stock_spec.clone(),
                _ => CamStock::Box { width: 100.0, depth: 100.0, height: 25.0 },
            },
            1 => match &draft.stock_spec {
                CamStock::RelativeBox { .. } => draft.stock_spec.clone(),
                _ => CamStock::RelativeBox {
                    mode: StockOffsetMode::SidesAndTopBottom,
                    side_offset: 2.0,
                    top_offset: 1.0,
                    bottom_offset: 0.0,
                    round_up: 0.0,
                    ground_at_model_origin: false,
                },
            },
            2 => match &draft.stock_spec {
                CamStock::Cylinder { .. } => draft.stock_spec.clone(),
                _ => CamStock::Cylinder { diameter: 50.0, height: 80.0 },
            },
            3 => match &draft.stock_spec {
                CamStock::RelativeCylinder { .. } => draft.stock_spec.clone(),
                _ => CamStock::RelativeCylinder { radial_offset: 2.0, front_offset: 1.0, back_offset: 1.0, round_up: 0.0 },
            },
            4 => match &draft.stock_spec {
                CamStock::Tube { .. } => draft.stock_spec.clone(),
                _ => CamStock::Tube { outer_diameter: 50.0, inner_diameter: 30.0, height: 80.0 },
            },
            5 => match &draft.stock_spec {
                CamStock::FromPreceding { .. } => draft.stock_spec.clone(),
                _ => CamStock::FromPreceding { continue_rest: true },
            },
            _ => match &draft.stock_spec {
                CamStock::FromSolid { .. } => draft.stock_spec.clone(),
                _ => CamStock::FromSolid { body: String::new() },
            },
        };
        match &mut draft.stock_spec {
            CamStock::Box { width, depth, height } => {
                ui_kit::row(ui, "Width (X)", |ui| ui_kit::number_field(ui, width, 0.1..=10_000.0, 0.5, " mm"));
                ui_kit::row(ui, "Depth (Y)", |ui| ui_kit::number_field(ui, depth, 0.1..=10_000.0, 0.5, " mm"));
                ui_kit::row(ui, "Height (Z)", |ui| ui_kit::number_field(ui, height, 0.1..=10_000.0, 0.5, " mm"));
            }
            CamStock::RelativeBox { mode, side_offset, top_offset, bottom_offset, round_up, ground_at_model_origin } => {
                let modes = [StockOffsetMode::NoAdditional, StockOffsetMode::SidesAndTopBottom, StockOffsetMode::AllSides];
                let labels: Vec<&str> = modes.iter().map(|m| m.label()).collect();
                let mut midx = modes.iter().position(|m| *m == *mode).unwrap_or(0);
                ui_kit::choice_row(ui, "Offset mode", "rel_box_mode", &mut midx, &labels);
                *mode = modes[midx];
                ui_kit::row(ui, "Side offset", |ui| ui_kit::number_field(ui, side_offset, 0.0..=500.0, 0.1, " mm"));
                ui_kit::row(ui, "Top offset", |ui| ui_kit::number_field(ui, top_offset, 0.0..=500.0, 0.1, " mm"));
                ui_kit::row(ui, "Bottom offset", |ui| ui_kit::number_field(ui, bottom_offset, 0.0..=500.0, 0.1, " mm"));
                ui_kit::row(ui, "Round up to nearest", |ui| ui_kit::number_field(ui, round_up, 0.0..=50.0, 0.1, " mm"));
                ui_kit::checkbox(ui, "Ground stock at model origin", ground_at_model_origin);
            }
            CamStock::Cylinder { diameter, height } => {
                ui_kit::row(ui, "Diameter", |ui| ui_kit::number_field(ui, diameter, 0.1..=10_000.0, 0.5, " mm"));
                ui_kit::row(ui, "Length", |ui| ui_kit::number_field(ui, height, 0.1..=10_000.0, 0.5, " mm"));
            }
            CamStock::RelativeCylinder { radial_offset, front_offset, back_offset, round_up } => {
                ui_kit::row(ui, "Radial stock offset", |ui| ui_kit::number_field(ui, radial_offset, 0.0..=500.0, 0.1, " mm"));
                ui_kit::row(ui, "Frontside offset", |ui| ui_kit::number_field(ui, front_offset, 0.0..=500.0, 0.1, " mm"));
                ui_kit::row(ui, "Backside offset", |ui| ui_kit::number_field(ui, back_offset, 0.0..=500.0, 0.1, " mm"));
                ui_kit::row(ui, "Round up to nearest", |ui| ui_kit::number_field(ui, round_up, 0.0..=50.0, 0.1, " mm"));
            }
            CamStock::Tube { outer_diameter, inner_diameter, height } => {
                ui_kit::row(ui, "Outer diameter", |ui| ui_kit::number_field(ui, outer_diameter, 0.1..=10_000.0, 0.5, " mm"));
                ui_kit::row(ui, "Inner diameter", |ui| ui_kit::number_field(ui, inner_diameter, 0.1..=10_000.0, 0.5, " mm"));
                ui_kit::row(ui, "Length", |ui| ui_kit::number_field(ui, height, 0.1..=10_000.0, 0.5, " mm"));
            }
            CamStock::FromPreceding { continue_rest } => {
                ui_kit::checkbox(ui, "Continue rest machining", continue_rest);
            }
            CamStock::FromSolid { body } => {
                ui_kit::text_row(ui, "Body", body, "Body");
            }
        }
    });
}

fn post_tab(ui: &mut egui::Ui, draft: &mut CamSetup) {
    ui_kit::section(ui, "setup_post", "Post Process", true, |ui| {
        ui_kit::text_row(ui, "Program Name/Number", &mut draft.post_opts.program_name, "1001");
        ui_kit::text_row(ui, "Program Comment", &mut draft.post_opts.program_comment, "");
        ui_kit::row(ui, "WCS Offset", |ui| ui_kit::number_field(ui, &mut draft.post_opts.wcs_offset, 0..=99, 1.0, ""));
        ui_kit::text(ui, wcs_offset_hint(draft.post_opts.wcs_offset), ui_kit::Text::Hint("wcs_off"));
        ui_kit::checkbox(ui, "Multiple WCS Offsets", &mut draft.post_opts.multiple_wcs_offsets);
    });
}

fn wcs_offset_hint(n: u32) -> &'static str {
    match n {
        0 | 1 => "G54",
        2 => "G55",
        3 => "G56",
        4 => "G57",
        5 => "G58",
        6 => "G59",
        _ => "extended offset",
    }
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

fn apply_box_point_to_wcs(app: &SolveApp, draft: &mut CamSetup) {
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

fn stock_box(draft: &CamSetup, model: Option<([f64; 3], [f64; 3])>) -> ([f64; 3], [f64; 3]) {
    if let Some((min, max)) = model {
        return match &draft.stock_spec {
            CamStock::RelativeBox { side_offset, top_offset, bottom_offset, ground_at_model_origin, .. } => {
                let mut lo = [min[0] - side_offset, min[1] - side_offset, min[2] - bottom_offset];
                let mut hi = [max[0] + side_offset, max[1] + side_offset, max[2] + top_offset];
                if *ground_at_model_origin {
                    let height = hi[2] - lo[2];
                    lo[2] = 0.0;
                    hi[2] = height;
                }
                (lo, hi)
            }
            CamStock::RelativeCylinder { radial_offset, front_offset, back_offset, .. } => (
                [min[0] - radial_offset, min[1] - radial_offset, min[2] - back_offset],
                [max[0] + radial_offset, max[1] + radial_offset, max[2] + front_offset],
            ),
            _ => (min, max),
        };
    }
    if let Some(stock) = draft.stock {
        return (stock.min, stock.max);
    }
    let (w, d, h) = match &draft.stock_spec {
        CamStock::Box { width, depth, height } => (*width, *depth, *height),
        CamStock::Cylinder { diameter, height } | CamStock::Tube { outer_diameter: diameter, height, .. } => (*diameter, *diameter, *height),
        _ => (100.0, 100.0, 25.0),
    };
    ([0.0, 0.0, 0.0], [w, d, h])
}

fn save_setup(app: &mut SolveApp, name: &str, mut draft: CamSetup) -> bool {
    let name = name.trim();
    if name.is_empty() {
        app.set_status("Setup name required", true);
        return false;
    }
    if draft.model_bodies.is_empty() {
        app.set_status("Select the bodies for this setup. The setup has no work area until those bodies are chosen.", true);
        return false;
    }
    let model = model_bounds(app, &draft.model_bodies);
    let (min, max) = stock_box(&draft, model);
    draft.stock = Some(solidcraft_engine::doc::cam::StockBox { min, max });
    draft.name = name.to_string();
    draft.work_origin = draft.wcs.origin;
    draft.tool_axis = draft.wcs.z_axis();
    draft.x_axis = Some(draft.wcs.x_axis);
    if (1..=6).contains(&draft.post_opts.wcs_offset) {
        draft.work_offset = draft.post_opts.wcs_offset as u8;
    }
    let Ok(wcs) = serde_json::to_value(&draft.wcs) else {
        return false;
    };
    let Ok(stock_spec) = serde_json::to_value(&draft.stock_spec) else {
        return false;
    };
    let Ok(box_point) = serde_json::to_value(draft.box_point) else {
        return false;
    };
    let Ok(post_opts) = serde_json::to_value(&draft.post_opts) else {
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
    match app.run(
        "cam.setup",
        json!({
            "name": name,
            "work_offset": draft.work_offset,
            "tool_axis": draft.tool_axis,
            "x_axis": draft.x_axis,
            "work_origin": draft.work_origin,
            "stock": {"min": min, "max": max},
            "stock_spec": stock_spec,
            "wcs": wcs,
            "wcs_orientation": wcs_orientation,
            "wcs_origin_mode": wcs_origin_mode,
            "box_point": box_point,
            "flip_z": draft.flip_z,
            "flip_x": draft.flip_x,
            "model_bodies": draft.model_bodies,
            "operation_type": operation_type,
            "post_opts": post_opts,
            "machine": draft.machine,
            "post": draft.post,
        }),
    ) {
        Ok(_) => {
            app.set_status(app.tr("manufacture.setup.saved").to_string(), false);
            true
        }
        Err(e) => {
            app.set_status(e, true);
            false
        }
    }
}

fn triad_len(bounds: ([f64; 3], [f64; 3])) -> f64 {
    let (min, max) = bounds;
    let extent = (0..3).map(|i| (max[i] - min[i]).abs()).fold(0.0_f64, f64::max);
    if extent < 1e-6 { 1.0 } else { extent * 0.28 }
}

/// Draw WCS triad + stock box outline for the setup being edited, or for the setup selected in the browser.
pub fn paint_overlays(app: &SolveApp, painter: &egui::Painter, proj: &viewport::Proj) {
    if let Some(state) = &app.manufacture_workspace.setup_dialog {
        paint_setup(app, painter, proj, &state.draft, true);
        return;
    }
    if !crate::manufacture_workspace::active(app) {
        return;
    }
    let Some(setup) = app.session.doc.cam.setup(&app.manufacture_workspace.active_setup) else {
        return;
    };
    if setup.model_bodies.is_empty() {
        return;
    }
    paint_setup(app, painter, proj, setup, false);
}

fn paint_setup(app: &SolveApp, painter: &egui::Painter, proj: &viewport::Proj, setup: &CamSetup, origin_picks: bool) {
    let wcs = painted_wcs(setup);
    let model = model_bounds(app, &setup.model_bodies);
    let (min, max) = stock_box(setup, model);
    let t = crate::theme::Tokens::get();
    let o = solidcraft_engine::geom::Vec3::new(wcs.origin[0], wcs.origin[1], wcs.origin[2]);
    let len = triad_len(model.unwrap_or((min, max)));
    let axis = |v: [f64; 3]| solidcraft_engine::geom::Vec3::new(v[0] * len, v[1] * len, v[2] * len);
    if let Some(origin) = proj.to_screen(o) {
        for (dir, color) in [(wcs.x_axis, t.axis_x), (wcs.y_axis, t.axis_y), (wcs.z_axis(), t.axis_z)] {
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
    let origin = wcs.origin;
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

fn painted_wcs(setup: &CamSetup) -> WorkCoordinateSystem {
    let mut wcs = setup.wcs.clone();
    if setup.flip_x {
        wcs.x_axis = [-wcs.x_axis[0], -wcs.x_axis[1], -wcs.x_axis[2]];
    }
    if setup.flip_z {
        wcs.y_axis = [-wcs.y_axis[0], -wcs.y_axis[1], -wcs.y_axis[2]];
    }
    wcs
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
    app.set_status(crate::i18n::tr("manufacture.setup.origin_set").to_string(), false);
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
