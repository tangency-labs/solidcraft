//! MODIFY dialogs: Trim, Delete Passes, Leads and Links, Replace Tool, Move Entry.
//! Document writes go through manufacture.modifications.set; not companion HTTP or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{
    DeletePassesModification, DeletePassesSelection, EntrySketchLine, HighFeedMode, LeadsLinksModification, LeadsLinksScope, MoveEntryModification,
    ReplaceToolModification, Tool, ToolpathModification, TrimKeep, TrimModification, TrimType,
};

use crate::SolveApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifyDialogKind {
    Trim,
    DeletePasses,
    LeadsAndLinks,
    ReplaceTool,
    MoveEntryPositions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LeadsTab {
    #[default]
    Leads,
    Linking,
    Heights,
}

#[derive(Clone, Debug)]
pub struct ModifyDialogState {
    pub kind: ModifyDialogKind,
    pub draft: ToolpathModification,
    pub leads_tab: LeadsTab,
}

impl ModifyDialogState {
    pub fn new(kind: ModifyDialogKind, setup: &str, op_index: usize) -> Self {
        let mut draft = match kind {
            ModifyDialogKind::Trim => ToolpathModification::Trim(TrimModification::default()),
            ModifyDialogKind::DeletePasses => ToolpathModification::DeletePasses(DeletePassesModification::default()),
            ModifyDialogKind::LeadsAndLinks => ToolpathModification::LeadsAndLinks(LeadsLinksModification::default()),
            ModifyDialogKind::ReplaceTool => ToolpathModification::ReplaceTool(ReplaceToolModification::default()),
            ModifyDialogKind::MoveEntryPositions => ToolpathModification::MoveEntryPositions(MoveEntryModification::default()),
        };
        {
            let h = draft.header_mut();
            h.setup = setup.to_string();
            h.operation_index = op_index;
        }
        Self { kind, draft, leads_tab: LeadsTab::Leads }
    }
}

pub fn open(app: &mut SolveApp, kind: ModifyDialogKind) {
    crate::manufacture_workspace::ensure_setup(app);
    let setup = app.manufacture_workspace.active_setup.clone();
    let op_index = app.manufacture_workspace.selected_op.unwrap_or(0);
    clear_other(app);
    app.manufacture_workspace.modify_dialog = Some(ModifyDialogState::new(kind, &setup, op_index));
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Modify;
}

/// Return false when `id` is a modify command this panel handles.
pub fn open_from_command(app: &mut SolveApp, id: &str) -> bool {
    let kind = match id {
        "manufacture.modify.trim" => ModifyDialogKind::Trim,
        "manufacture.modify.delete_passes" => ModifyDialogKind::DeletePasses,
        "manufacture.modify.leads_links" => ModifyDialogKind::LeadsAndLinks,
        "manufacture.modify.replace_tool" => ModifyDialogKind::ReplaceTool,
        "manufacture.modify.move_entry" => ModifyDialogKind::MoveEntryPositions,
        _ => return true,
    };
    open(app, kind);
    false
}

fn clear_other(app: &mut SolveApp) {
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
    app.manufacture_workspace.part_handling_dialog = None;
    app.manufacture_workspace.inspection_dialog = None;
    app.manufacture_workspace.manual_inspection_dialog = None;
    app.manufacture_workspace.fabrication_dialog = None;
    app.manufacture_workspace.nest_dialog = None;
    app.manufacture_workspace.additive_dialog = None;
    app.manufacture_workspace.from_gcode_dialog = None;
    app.manufacture_workspace.modify_dialog = None;
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = ui;
    let Some(state) = app.manufacture_workspace.modify_dialog.clone() else {
        return;
    };

    let setup_name = state.draft.header().setup.clone();
    let tools: Vec<Tool> = app.session.doc.manufacture.setups.get(&setup_name).map(|s| s.tools.clone()).unwrap_or_default();
    let op_count = app.session.doc.manufacture.setups.get(&setup_name).map(|s| s.operations.len()).unwrap_or(0);
    let setup_names: Vec<String> = app.session.doc.manufacture.setups.keys().cloned().collect();

    let mut draft = state.draft.clone();
    let mut leads_tab = state.leads_tab;
    let title = format!("{} : TOOLPATH", state.kind_title());

    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_modify_dialog"), "settings", &title).fields().cards(1.5).show(ui.ctx(), true, |ui| {
            target_header(ui, &mut draft, &setup_names, op_count);
            ui.separator();
            egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| match &mut draft {
                ToolpathModification::Trim(m) => trim_body(ui, m),
                ToolpathModification::DeletePasses(m) => delete_passes_body(ui, m),
                ToolpathModification::LeadsAndLinks(m) => {
                    leads_links_body(ui, m, &mut leads_tab);
                }
                ToolpathModification::ReplaceTool(m) => replace_tool_body(ui, m, &tools),
                ToolpathModification::MoveEntryPositions(m) => move_entry_body(ui, m),
            });
            ui.separator();
            ui.label("Notes");
            ui.text_edit_multiline(&mut draft.header_mut().notes);
        });
    let commit = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;
    if cancel {
        app.manufacture_workspace.modify_dialog = None;
        return;
    }
    if commit && save(app, draft.clone()) {
        app.manufacture_workspace.modify_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.modify_dialog {
        d.draft = draft;
        d.leads_tab = leads_tab;
    }
}

impl ModifyDialogKind {
    fn kind_title(self) -> &'static str {
        match self {
            ModifyDialogKind::Trim => "TRIM",
            ModifyDialogKind::DeletePasses => "DELETE PASSES",
            ModifyDialogKind::LeadsAndLinks => "LEADS AND LINKS",
            ModifyDialogKind::ReplaceTool => "REPLACE TOOL",
            ModifyDialogKind::MoveEntryPositions => "MOVE ENTRY POSITIONS",
        }
    }
}

impl ModifyDialogState {
    fn kind_title(&self) -> &'static str {
        self.kind.kind_title()
    }
}

fn target_header(ui: &mut egui::Ui, draft: &mut ToolpathModification, setups: &[String], op_count: usize) {
    let h = draft.header_mut();
    ui.heading("Target");
    ui.horizontal(|ui| {
        ui.label("Setup");
        let label = if h.setup.is_empty() { "(none)".into() } else { h.setup.clone() };
        egui::ComboBox::from_id_salt("mod_setup").selected_text(label).show_ui(ui, |ui| {
            for s in setups {
                if ui.selectable_label(&h.setup == s, s).clicked() {
                    h.setup = s.clone();
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Operation index");
        let max = op_count.saturating_sub(1);
        ui.add(egui::DragValue::new(&mut h.operation_index).speed(1).range(0..=max));
        ui.label(RichText::new(format!("of {op_count}")).weak());
    });
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut h.name);
    });
}

fn trim_body(ui: &mut egui::Ui, m: &mut TrimModification) {
    ui.heading("Trim");
    ui.horizontal(|ui| {
        ui.label("Trim Type");
        egui::ComboBox::from_id_salt("mod_trim_type").selected_text(m.trim_type.label()).show_ui(ui, |ui| {
            for t in [TrimType::Polygon, TrimType::Plane, TrimType::Boundary] {
                if ui.selectable_label(m.trim_type == t, t.label()).clicked() {
                    m.trim_type = t;
                    if t == TrimType::Plane && matches!(m.keep, TrimKeep::Outside | TrimKeep::Inside | TrimKeep::Both) {
                        m.keep = TrimKeep::Side1;
                    }
                    if t != TrimType::Plane && matches!(m.keep, TrimKeep::Side1 | TrimKeep::Side2) {
                        m.keep = TrimKeep::Outside;
                    }
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label("Keep");
        let options = if m.trim_type == TrimType::Plane {
            [TrimKeep::Side1, TrimKeep::Side2, TrimKeep::Both].as_slice()
        } else {
            [TrimKeep::Outside, TrimKeep::Inside, TrimKeep::Both].as_slice()
        };
        egui::ComboBox::from_id_salt("mod_trim_keep").selected_text(m.keep.label()).show_ui(ui, |ui| {
            for k in options {
                if ui.selectable_label(m.keep == *k, k.label()).clicked() {
                    m.keep = *k;
                }
            }
        });
    });
    ui.checkbox(&mut m.visible_only, "Visible Only");
    ui.label(RichText::new("Use orthographic view when placing a polygon.").weak());

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Add point").clicked() && m.points.len() < solidcraft_manufacture::MAX_SAMPLE_POINTS_PER_OP {
            m.points.push([0.0, 0.0, 0.0]);
        }
        if ui.button("Clear").clicked() {
            m.points.clear();
        }
    });
    let mut remove = None;
    for (i, pt) in m.points.iter_mut().enumerate() {
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
        m.points.remove(i);
    }
}

fn delete_passes_body(ui: &mut egui::Ui, m: &mut DeletePassesModification) {
    ui.heading("Delete Passes");
    ui.horizontal(|ui| {
        ui.label("Selection Type");
        egui::ComboBox::from_id_salt("mod_del_sel").selected_text(m.selection_type.label()).show_ui(ui, |ui| {
            for s in [DeletePassesSelection::Passes, DeletePassesSelection::Segments] {
                if ui.selectable_label(m.selection_type == s, s.label()).clicked() {
                    m.selection_type = s;
                }
            }
        });
    });
    ui.checkbox(&mut m.invert_selection, "Invert Selection");
    ui.label(RichText::new("Select passes/segments in the canvas (orthographic view recommended).").weak());

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Add index").clicked() {
            let next = m.indices.last().copied().unwrap_or(0).saturating_add(1);
            m.indices.push(next);
        }
        if ui.button("Clear").clicked() {
            m.indices.clear();
        }
    });
    let mut remove = None;
    for (i, idx) in m.indices.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("#{i}"));
            ui.add(egui::DragValue::new(idx).speed(1).range(0..=10_000));
            if ui.small_button("×").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        m.indices.remove(i);
    }
}

fn leads_links_body(ui: &mut egui::Ui, m: &mut LeadsLinksModification, tab: &mut LeadsTab) {
    ui.horizontal(|ui| {
        for (v, l) in [(LeadsTab::Leads, "Leads"), (LeadsTab::Linking, "Linking"), (LeadsTab::Heights, "Heights")] {
            let sel = *tab == v;
            if ui.selectable_label(sel, if sel { RichText::new(l).strong() } else { RichText::new(l) }).clicked() {
                *tab = v;
            }
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Apply to");
        egui::ComboBox::from_id_salt("mod_ll_scope").selected_text(m.scope.label()).show_ui(ui, |ui| {
            for s in [LeadsLinksScope::AllMoves, LeadsLinksScope::SelectedMoves] {
                if ui.selectable_label(m.scope == s, s.label()).clicked() {
                    m.scope = s;
                }
            }
        });
    });

    match *tab {
        LeadsTab::Leads => {
            ui.checkbox(&mut m.trim_segments_to_fit, "Trim segments to fit leads");
            ui.horizontal(|ui| {
                ui.label("Overlap");
                ui.add(egui::DragValue::new(&mut m.overlap_mm).speed(0.05).range(0.0..=50.0).suffix(" mm"));
            });
            ui.checkbox(&mut m.lead_in, "Lead-In");
            if m.lead_in {
                ui.horizontal(|ui| {
                    ui.label("Lead-In Radius");
                    ui.add(egui::DragValue::new(&mut m.lead_in_radius_mm).speed(0.05).range(0.0..=50.0).suffix(" mm"));
                });
            }
            ui.checkbox(&mut m.lead_out, "Lead-Out");
            if m.lead_out {
                ui.horizontal(|ui| {
                    ui.label("Lead-Out Radius");
                    ui.add(egui::DragValue::new(&mut m.lead_out_radius_mm).speed(0.05).range(0.0..=50.0).suffix(" mm"));
                });
            }
        }
        LeadsTab::Linking => {
            ui.checkbox(&mut m.keep_tool_down, "Keep Tool Down");
            ui.horizontal(|ui| {
                ui.label("Safe Distance");
                ui.add(egui::DragValue::new(&mut m.safe_distance_mm).speed(0.05).range(0.0..=100.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("High Feedrate Mode");
                egui::ComboBox::from_id_salt("mod_ll_hf").selected_text(m.high_feed_mode.label()).show_ui(ui, |ui| {
                    for mode in [HighFeedMode::PreserveRapid, HighFeedMode::AlwaysUseHighFeed, HighFeedMode::NeverUseHighFeed] {
                        if ui.selectable_label(m.high_feed_mode == mode, mode.label()).clicked() {
                            m.high_feed_mode = mode;
                        }
                    }
                });
            });
        }
        LeadsTab::Heights => {
            ui.horizontal(|ui| {
                ui.label("Clearance offset");
                ui.add(egui::DragValue::new(&mut m.clearance_offset_mm).speed(0.1).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Retract offset");
                ui.add(egui::DragValue::new(&mut m.retract_offset_mm).speed(0.1).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Feed offset");
                ui.add(egui::DragValue::new(&mut m.feed_offset_mm).speed(0.1).suffix(" mm"));
            });
        }
    }
}

fn replace_tool_body(ui: &mut egui::Ui, m: &mut ReplaceToolModification, tools: &[Tool]) {
    ui.heading("Replace Tool");
    ui.label(RichText::new("Tip geometry should match the original; length/holder may differ.").weak());
    let tnum = m.new_tool_number;
    let label = tools.iter().find(|t| t.number == tnum).map(|t| format!("#{} ø{:.2}", t.number, t.diameter)).unwrap_or_else(|| format!("#{tnum}"));
    egui::ComboBox::from_id_salt("mod_repl_tool").selected_text(label).show_ui(ui, |ui| {
        for t in tools {
            if ui.selectable_label(tnum == t.number, format!("#{} ø{:.2}", t.number, t.diameter)).clicked() {
                m.new_tool_number = t.number;
            }
        }
    });
    ui.checkbox(&mut m.override_feeds, "Override Feed & Speed");
    if m.override_feeds {
        ui.horizontal(|ui| {
            ui.label("Spindle");
            let mut rpm = m.feeds.spindle_rpm;
            if ui.add(egui::DragValue::new(&mut rpm).speed(100).range(1..=60_000).suffix(" rpm")).changed() {
                m.feeds.spindle_rpm = rpm;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Cutting Feedrate");
            ui.add(egui::DragValue::new(&mut m.feeds.feedrate).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
        });
        ui.horizontal(|ui| {
            ui.label("Plunge Feedrate");
            ui.add(egui::DragValue::new(&mut m.feeds.plunge).speed(10.0).range(1.0..=50_000.0).suffix(" mm/min"));
        });
    }
}

fn move_entry_body(ui: &mut egui::Ui, m: &mut MoveEntryModification) {
    ui.heading("Move Entry Positions");
    ui.label(RichText::new("Sketch lines that intersect closed passes; entry moves to the intersection.").weak());
    ui.horizontal(|ui| {
        if ui.button("Add sketch line").clicked() {
            m.sketch_lines.push(EntrySketchLine::default());
        }
        if ui.button("Clear").clicked() {
            m.sketch_lines.clear();
        }
    });
    let mut remove = None;
    for (i, line) in m.sketch_lines.iter_mut().enumerate() {
        ui.group(|ui| {
            ui.label(format!("Line {i}"));
            ui.horizontal(|ui| {
                ui.label("A");
                ui.add(egui::DragValue::new(&mut line.a[0]).speed(0.1).prefix("X "));
                ui.add(egui::DragValue::new(&mut line.a[1]).speed(0.1).prefix("Y "));
                ui.add(egui::DragValue::new(&mut line.a[2]).speed(0.1).prefix("Z "));
            });
            ui.horizontal(|ui| {
                ui.label("B");
                ui.add(egui::DragValue::new(&mut line.b[0]).speed(0.1).prefix("X "));
                ui.add(egui::DragValue::new(&mut line.b[1]).speed(0.1).prefix("Y "));
                ui.add(egui::DragValue::new(&mut line.b[2]).speed(0.1).prefix("Z "));
            });
            if ui.small_button("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        m.sketch_lines.remove(i);
    }
}

fn save(app: &mut SolveApp, draft: ToolpathModification) -> bool {
    match serde_json::to_value(&draft) {
        Ok(v) => match app.run("manufacture.modifications.set", json!({"add": v})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                app.set_status(app.tr("manufacture.modify.saved").to_string(), false);
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
