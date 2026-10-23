//! Hole Recognition dialog (Hole Groups / Tool Libraries / Options).
//! Document writes go through manufacture.settings.set; not companion HTTP or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{HoleAction, HoleGroup, HoleOrganizeMode, HoleRecognitionOperation, Operation};

use crate::{SolveApp, viewport};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HoleRecTab {
    #[default]
    HoleGroups,
    ToolLibraries,
    Options,
}

#[derive(Clone, Debug)]
pub struct HoleRecDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: HoleRecognitionOperation,
    pub tab: HoleRecTab,
    pub selected_group: Option<usize>,
}

pub fn open(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::HoleRecognition(op)) = setup.operations.get(op_index).cloned() else {
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
    app.manufacture_workspace.hole_rec_dialog =
        Some(HoleRecDialogState { setup_name: setup_name.to_string(), op_index, draft: op, tab: HoleRecTab::HoleGroups, selected_group: None });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::HoleRecognition;
}

/// Create a default Hole Recognition op on the active setup and open its inspector.
pub fn open_new(app: &mut SolveApp) {
    crate::manufacture_workspace::ensure_setup(app);
    let setup_name = app.manufacture_workspace.active_setup.clone();
    if setup_name.is_empty() {
        return;
    }
    let Some(mut setup) = app.session.doc.manufacture.setups.get(&setup_name).cloned() else {
        return;
    };
    setup.operations.push(Operation::HoleRecognition(HoleRecognitionOperation::default()));
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
    let Some(state) = app.manufacture_workspace.hole_rec_dialog.clone() else {
        return;
    };

    let mut draft = state.draft.clone();
    let mut tab = state.tab;
    let mut selected_group = state.selected_group;
    let title = if draft.name.is_empty() { "HOLE RECOGNITION".to_string() } else { format!("HOLE RECOGNITION : {}", draft.name.to_uppercase()) };

    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_hole_rec_dialog"), "settings", &title).fields().cards(1.6).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                for (v, l) in
                    [(HoleRecTab::HoleGroups, "Hole Groups"), (HoleRecTab::ToolLibraries, "Tool Libraries"), (HoleRecTab::Options, "Options")]
                {
                    let sel = tab == v;
                    if ui.selectable_label(sel, if sel { RichText::new(l).strong() } else { RichText::new(l) }).clicked() {
                        tab = v;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| match tab {
                HoleRecTab::HoleGroups => groups_tab(ui, &mut draft, &mut selected_group),
                HoleRecTab::ToolLibraries => libraries_tab(ui, &mut draft),
                HoleRecTab::Options => options_tab(ui, &mut draft),
            });
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;

    if cancel {
        app.manufacture_workspace.hole_rec_dialog = None;
        return;
    }
    if commit && save(app, &state.setup_name, state.op_index, draft.clone()) {
        app.manufacture_workspace.hole_rec_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.hole_rec_dialog {
        d.draft = draft;
        d.tab = tab;
        d.selected_group = selected_group;
    }
}

fn groups_tab(ui: &mut egui::Ui, draft: &mut HoleRecognitionOperation, selected: &mut Option<usize>) {
    ui.heading("Hole Groups");
    ui.label("Holes sorted by diameter. Choose an Action for each size.");

    ui.horizontal(|ui| {
        if ui.button("Detect sample holes").clicked() {
            seed_sample_groups(draft);
        }
        if ui.button("Add group").clicked() {
            draft.groups.push(HoleGroup::default());
            *selected = Some(draft.groups.len().saturating_sub(1));
        }
        if ui.button("Clear").clicked() {
            draft.groups.clear();
            *selected = None;
        }
    });

    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Modifiers").weak());
        let can_mod = selected.and_then(|i| draft.groups.get(i)).is_some();
        if ui.add_enabled(can_mod, egui::Button::new("Delete top segment")).clicked() {
            // L0: no multi-segment topology yet — no-op placeholder for the control.
        }
        ui.add_enabled(can_mod, egui::Button::new("Delete bottom segment")).clicked();
        if ui.add_enabled(can_mod, egui::Button::new("Split signature")).clicked()
            && let Some(i) = *selected
            && let Some(g) = draft.groups.get(i).cloned()
        {
            let mut a = g.clone();
            let mut b = g;
            a.depth_mm = (a.depth_mm / 2.0).max(0.1);
            b.depth_mm = a.depth_mm;
            b.count = a.count;
            draft.groups[i] = a;
            draft.groups.insert(i + 1, b);
        }
        if ui.add_enabled(can_mod, egui::Button::new("Flip hole")).clicked()
            && let Some(i) = *selected
            && let Some(g) = draft.groups.get_mut(i)
        {
            for c in &mut g.centres {
                c[2] = -c[2];
            }
        }
    });

    ui.separator();
    let mut remove = None;
    for (i, g) in draft.groups.iter_mut().enumerate() {
        let row_sel = *selected == Some(i);
        ui.group(|ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(row_sel, format!("#{i}")).clicked() {
                    *selected = Some(i);
                }
                ui.label("Ø");
                ui.add(egui::DragValue::new(&mut g.diameter_mm).speed(0.05).range(0.1..=200.0).suffix(" mm"));
                ui.label("Depth");
                ui.add(egui::DragValue::new(&mut g.depth_mm).speed(0.1).range(0.1..=500.0).suffix(" mm"));
                ui.label("Count");
                ui.add(egui::DragValue::new(&mut g.count).speed(1).range(1..=999));
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Action");
                egui::ComboBox::from_id_salt(format!("hr_action_{i}")).selected_text(g.action.label()).show_ui(ui, |ui| {
                    for a in [
                        HoleAction::SpotDrill,
                        HoleAction::Drill,
                        HoleAction::SpotAndDrill,
                        HoleAction::CounterBore,
                        HoleAction::Bore,
                        HoleAction::Tap,
                        HoleAction::Ream,
                        HoleAction::Ignore,
                    ] {
                        if ui.selectable_label(g.action == a, a.label()).clicked() {
                            g.action = a;
                        }
                    }
                });
                if g.action.needs_thread() {
                    ui.label("Thread");
                    ui.text_edit_singleline(&mut g.thread_type);
                }
            });
            ui.label(format!("{} centre(s)", g.centres.len()));
        });
    }
    if let Some(i) = remove {
        draft.groups.remove(i);
        if *selected == Some(i) {
            *selected = None;
        } else if let Some(s) = *selected
            && s > i
        {
            *selected = Some(s - 1);
        }
    }

    if draft.groups.is_empty() {
        ui.label(RichText::new("No hole groups yet — Detect sample holes or Add group.").weak());
    }
}

fn seed_sample_groups(draft: &mut HoleRecognitionOperation) {
    draft.groups = vec![
        HoleGroup {
            diameter_mm: 3.3,
            depth_mm: 12.0,
            count: 4,
            action: HoleAction::SpotAndDrill,
            thread_type: String::new(),
            centres: vec![[10.0, 10.0, 0.0], [30.0, 10.0, 0.0], [10.0, 30.0, 0.0], [30.0, 30.0, 0.0]],
        },
        HoleGroup {
            diameter_mm: 6.0,
            depth_mm: 15.0,
            count: 2,
            action: HoleAction::Tap,
            thread_type: "M6x1".into(),
            centres: vec![[50.0, 20.0, 0.0], [70.0, 20.0, 0.0]],
        },
        HoleGroup {
            diameter_mm: 8.0,
            depth_mm: 8.0,
            count: 1,
            action: HoleAction::CounterBore,
            thread_type: String::new(),
            centres: vec![[40.0, 40.0, 0.0]],
        },
    ];
}

fn libraries_tab(ui: &mut egui::Ui, draft: &mut HoleRecognitionOperation) {
    ui.heading("Tool Libraries");
    ui.label("Tools are selected from the libraries you enable. Prefer shop libraries that match available tools.");
    ui.checkbox(&mut draft.use_document_tools, "Document / setup tool library");
    ui.checkbox(&mut draft.use_metric_sample_library, "Sample Metric library");
    ui.checkbox(&mut draft.use_inch_sample_library, "Sample Inch library");
    ui.separator();
    ui.heading("Template Library Folder");
    ui.label("Leave empty to consider all hole templates.");
    ui.text_edit_singleline(&mut draft.template_folder);
}

fn options_tab(ui: &mut egui::Ui, draft: &mut HoleRecognitionOperation) {
    ui.heading("Options");
    ui.checkbox(&mut draft.multi_axis_machining, "Multi-Axis Machining");
    ui.label(RichText::new("Include holes whose axis is not parallel to the setup Z.").weak());
    ui.checkbox(&mut draft.include_partial_holes, "Include Partial Holes");

    ui.separator();
    ui.checkbox(&mut draft.find_by_angle, "Find by Angle");
    if draft.find_by_angle {
        ui.horizontal(|ui| {
            ui.label("Minimum Angle");
            ui.add(egui::DragValue::new(&mut draft.min_angle_deg).speed(0.5).range(0.0..=180.0).suffix(" deg"));
        });
        ui.horizontal(|ui| {
            ui.label("Maximum Angle");
            ui.add(egui::DragValue::new(&mut draft.max_angle_deg).speed(0.5).range(0.0..=180.0).suffix(" deg"));
        });
    }

    ui.checkbox(&mut draft.find_by_diameter, "Find by Diameter");
    if draft.find_by_diameter {
        ui.horizontal(|ui| {
            ui.label("Maximum Diameter");
            ui.add(egui::DragValue::new(&mut draft.max_diameter_mm).speed(0.1).range(0.1..=500.0).suffix(" mm"));
        });
    }

    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Organize Operations");
        egui::ComboBox::from_id_salt("hr_organize").selected_text(draft.organize.label()).show_ui(ui, |ui| {
            for m in [HoleOrganizeMode::MinimizeToolChanges, HoleOrganizeMode::GroupBySize] {
                if ui.selectable_label(draft.organize == m, m.label()).clicked() {
                    draft.organize = m;
                }
            }
        });
    });
    ui.checkbox(&mut draft.fewest_spot_drills, "Use Fewest Spot Drills Possible");
}

fn save(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: HoleRecognitionOperation) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        return false;
    };
    if op_index >= setup.operations.len() {
        return false;
    }
    setup.operations[op_index] = Operation::HoleRecognition(draft);
    match serde_json::to_value(&setup.operations) {
        Ok(ops) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops}})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.hole_recognition.saved").to_string(), false);
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
    let Some(state) = &app.manufacture_workspace.hole_rec_dialog else {
        return;
    };
    let color = egui::Color32::from_rgb(255, 170, 60);
    let sel = state.selected_group;
    for (gi, g) in state.draft.groups.iter().enumerate() {
        if g.action == HoleAction::Ignore {
            continue;
        }
        let c = if sel == Some(gi) { egui::Color32::from_rgb(80, 180, 255) } else { color };
        for pt in &g.centres {
            if let Some(s) = proj.to_screen(solidcraft_engine::geom::Vec3::new(pt[0], pt[1], pt[2])) {
                painter.circle_stroke(s, 5.0, egui::Stroke::new(1.5, c));
                painter.circle_filled(s, 2.0, c);
            }
        }
    }
}
