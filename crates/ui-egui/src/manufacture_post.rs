//! Post Process / NC Program dialog and Setup Sheet / Machining Time helpers.
//! Document writes go through manufacture.nc_programs.set and manufacture.settings.set;
//! not companion HTTP or GPL CAM.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{
    LineEnding, NcOpRef, NcProgram, NcUnit, Operation, PathSegment, PostProfile, Tool, build_setup_sheet_html, estimate_machining_time_s,
    format_cycle_time,
};

use crate::SolveApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PostDialogTab {
    #[default]
    Settings,
    Operations,
}

#[derive(Clone, Debug)]
pub struct PostDialogState {
    pub draft: NcProgram,
    pub tab: PostDialogTab,
    /// When editing an existing NC program in the document.
    pub existing_index: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct MachiningTimeState {
    pub feed_scale: f64,
    pub rapid_mmpm: f64,
    pub tool_change_s: f64,
    pub estimated_s: Option<f64>,
}

impl Default for MachiningTimeState {
    fn default() -> Self {
        Self { feed_scale: 1.0, rapid_mmpm: 5_000.0, tool_change_s: 8.0, estimated_s: None }
    }
}

#[derive(Clone, Debug)]
pub struct SetupSheetState {
    pub html: String,
    pub title: String,
}

pub fn open_post_process(app: &mut SolveApp) {
    crate::manufacture_workspace::ensure_setup(app);
    clear_other(app);
    let active = app.manufacture_workspace.active_setup.clone();
    let mut draft = NcProgram::default();
    let n = app.session.doc.manufacture.nc_programs.len() + 1;
    draft.name = format!("NCProgram{n}");
    if !active.is_empty() {
        draft.setup_names = vec![active.clone()];
        if let Some(setup) = app.session.doc.manufacture.setups.get(&active) {
            draft.program_name = setup.post_opts.program_name.clone();
            draft.file_name = setup.post_opts.program_name.clone();
            draft.comment = setup.post_opts.program_comment.clone();
            if let Some(post) = &setup.post {
                draft.post = Some(post.clone());
            }
            // Select all operations in the active setup by default.
            draft.operation_refs = setup.operations.iter().enumerate().map(|(i, _)| NcOpRef { setup: active.clone(), operation_index: i }).collect();
        }
    }
    app.manufacture_workspace.post_dialog = Some(PostDialogState { draft, tab: PostDialogTab::Settings, existing_index: None });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Post;
}

pub fn open_machining_time(app: &mut SolveApp) {
    clear_other(app);
    let mut st = MachiningTimeState::default();
    if let Some(setup_name) = non_empty(&app.manufacture_workspace.active_setup) {
        if let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) {
            if let Some(mname) = &setup.machine {
                if let Some(m) = app.session.doc.manufacture.machines.get(mname) {
                    if m.rapid_feedrate_mmpm > 0.0 {
                        st.rapid_mmpm = m.rapid_feedrate_mmpm;
                    }
                    if m.tool_changer.change_time_s > 0.0 {
                        st.tool_change_s = m.tool_changer.change_time_s;
                    }
                }
            }
        }
    }
    st.estimated_s = Some(compute_cycle_time(app, &st));
    app.manufacture_workspace.machining_time_dialog = Some(st);
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Post;
}

pub fn open_setup_sheet(app: &mut SolveApp) {
    let existing = app.manufacture_workspace.post_dialog.as_ref().map(|d| d.draft.clone());
    clear_other(app);
    let program = existing.unwrap_or_else(|| {
        let mut p = NcProgram::default();
        let active = app.manufacture_workspace.active_setup.clone();
        if !active.is_empty() {
            p.setup_names = vec![active];
        }
        p
    });
    let rows = collect_op_rows(app, &program);
    let mt = MachiningTimeState::default();
    let secs = compute_cycle_time(app, &mt);
    let title = if app.session.doc.name.trim().is_empty() { "SolidCraft".into() } else { app.session.doc.name.clone() };
    let html = build_setup_sheet_html(&title, &program, &rows, &format_cycle_time(secs));
    app.manufacture_workspace.setup_sheet_dialog = Some(SetupSheetState { html, title });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Post;
}

fn non_empty(s: &str) -> Option<&str> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t) }
}

/// Return false when `id` is a post command this panel handles.
pub fn open_from_command(app: &mut SolveApp, id: &str) -> bool {
    match id {
        "manufacture.post.process" => open_post_process(app),
        "manufacture.post.time" => open_machining_time(app),
        "manufacture.post.sheet" => open_setup_sheet(app),
        _ => return true,
    }
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
    app.manufacture_workspace.post_dialog = None;
    app.manufacture_workspace.machining_time_dialog = None;
    app.manufacture_workspace.setup_sheet_dialog = None;
}

fn compute_cycle_time(app: &SolveApp, st: &MachiningTimeState) -> f64 {
    let segs = &app.manufacture_workspace.toolpath_segments;
    let tools = non_empty(&app.manufacture_workspace.active_setup)
        .and_then(|n| app.session.doc.manufacture.setups.get(n))
        .map(|s| s.operations.len().saturating_sub(1) as u32)
        .unwrap_or(0);
    estimate_machining_time_s(segs, st.rapid_mmpm, st.feed_scale, st.tool_change_s, tools)
}

fn collect_op_rows(app: &SolveApp, program: &NcProgram) -> Vec<(String, String, String, String)> {
    let mut rows = Vec::new();
    let refs = resolved_op_refs(app, program);
    for r in refs {
        let Some(setup) = app.session.doc.manufacture.setups.get(&r.setup) else {
            continue;
        };
        let Some(op) = setup.operations.get(r.operation_index) else {
            continue;
        };
        let tool = tool_label(setup.tools.as_slice(), op);
        let wcs = format!("#{}", setup.post_opts.wcs_offset);
        rows.push((op.display_name().to_string(), r.setup.clone(), tool, wcs));
    }
    rows
}

fn resolved_op_refs(app: &SolveApp, program: &NcProgram) -> Vec<NcOpRef> {
    if !program.operation_refs.is_empty() {
        return program.operation_refs.clone();
    }
    let mut out = Vec::new();
    for sn in &program.setup_names {
        if let Some(setup) = app.session.doc.manufacture.setups.get(sn) {
            for i in 0..setup.operations.len() {
                out.push(NcOpRef { setup: sn.clone(), operation_index: i });
            }
        }
    }
    if out.is_empty() {
        let active = &app.manufacture_workspace.active_setup;
        if let Some(setup) = app.session.doc.manufacture.setups.get(active) {
            for i in 0..setup.operations.len() {
                out.push(NcOpRef { setup: active.clone(), operation_index: i });
            }
        }
    }
    out
}

fn tool_label(tools: &[Tool], op: &Operation) -> String {
    let n = op.tool_number();
    if n == 0 {
        return "—".into();
    }
    match tools.iter().find(|t| t.number == n) {
        Some(t) => format!("#{n} - ø{:.2}mm", t.diameter),
        None => format!("#{n}"),
    }
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    show_post_dialog(app, ui);
    show_machining_time(app, ui);
    show_setup_sheet(app, ui);
}

fn show_post_dialog(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(state) = app.manufacture_workspace.post_dialog.clone() else {
        return;
    };

    let posts: Vec<(String, String)> = {
        let mut v: Vec<_> = app.session.doc.manufacture.posts.iter().map(|(k, p)| (k.clone(), p.name.clone())).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };
    let setup_names: Vec<String> = app.session.doc.manufacture.setups.keys().cloned().collect();

    let mut draft = state.draft.clone();
    let mut tab = state.tab;
    let title = format!("NC PROGRAM : {}", draft.name.to_uppercase());
    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_post_dialog"), "settings", &title).fields().cards(1.5).show(ui.ctx(), true, |ui| {
            ui.horizontal(|ui| {
                for (v, l) in [(PostDialogTab::Settings, "Settings"), (PostDialogTab::Operations, "Operations")] {
                    let sel = tab == v;
                    if ui.selectable_label(sel, if sel { RichText::new(l).strong() } else { RichText::new(l) }).clicked() {
                        tab = v;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| match tab {
                PostDialogTab::Settings => settings_tab(ui, &mut draft, &posts),
                PostDialogTab::Operations => operations_tab(ui, app, &mut draft, &setup_names),
            });
        });
    let do_post = action == crate::ui_kit::Action::Ok;
    let cancel = closed || action == crate::ui_kit::Action::Cancel;
    if cancel {
        app.manufacture_workspace.post_dialog = None;
        return;
    }
    if do_post && run_post(app, &draft) {
        app.manufacture_workspace.post_dialog = None;
        return;
    }
    if let Some(d) = &mut app.manufacture_workspace.post_dialog {
        d.draft = draft;
        d.tab = tab;
    }
}

fn settings_tab(ui: &mut egui::Ui, draft: &mut NcProgram, posts: &[(String, String)]) {
    ui.heading("Machine and post");
    ui.checkbox(&mut draft.use_machine_configuration, "Use machine configuration");
    ui.horizontal(|ui| {
        ui.label("Post");
        let cur = draft.post.clone().unwrap_or_default();
        let label = if cur.is_empty() {
            "(select post)".into()
        } else {
            posts.iter().find(|(id, _)| id == &cur).map(|(_, n)| n.clone()).unwrap_or(cur.clone())
        };
        egui::ComboBox::from_id_salt("post_dlg_post").selected_text(label).show_ui(ui, |ui| {
            if ui.selectable_label(cur.is_empty(), "(select post)").clicked() {
                draft.post = None;
            }
            for (id, name) in posts {
                if ui.selectable_label(cur == *id, name).clicked() {
                    draft.post = Some(id.clone());
                    // Seed a few generic post properties when first selected.
                    if draft.post_properties.is_empty() {
                        draft.post_properties.insert("writeMachine".into(), "true".into());
                        draft.post_properties.insert("showSequenceNumbers".into(), "false".into());
                        draft.post_properties.insert("optionalStop".into(), "false".into());
                    }
                }
            }
        });
    });
    ui.checkbox(&mut draft.use_cascading_post, "Use cascading post");

    ui.separator();
    ui.heading("Program");
    ui.horizontal(|ui| {
        ui.label("Name/number");
        ui.text_edit_singleline(&mut draft.program_name);
    });
    ui.horizontal(|ui| {
        ui.label("File name");
        ui.text_edit_singleline(&mut draft.file_name);
    });
    ui.horizontal(|ui| {
        ui.label("Comment");
        ui.text_edit_singleline(&mut draft.comment);
    });
    ui.horizontal(|ui| {
        ui.label("Output folder");
        ui.text_edit_singleline(&mut draft.output_folder);
    });
    ui.checkbox(&mut draft.post_to_hub, "Post to hub");
    ui.horizontal(|ui| {
        ui.label("NC extension");
        ui.text_edit_singleline(&mut draft.nc_extension);
    });
    ui.horizontal(|ui| {
        ui.label("Unit");
        egui::ComboBox::from_id_salt("post_unit").selected_text(draft.unit.label()).show_ui(ui, |ui| {
            for u in [NcUnit::Millimeters, NcUnit::Inches] {
                if ui.selectable_label(draft.unit == u, u.label()).clicked() {
                    draft.unit = u;
                }
            }
        });
    });
    ui.checkbox(&mut draft.open_nc_in_editor, "Open NC file in editor");

    ui.separator();
    ui.heading("Save in the document");
    ui.checkbox(&mut draft.create_in_document, "Create NC program");
    if draft.create_in_document {
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut draft.name);
        });
    }

    ui.separator();
    ui.heading("Post properties");
    if draft.post_properties.is_empty() {
        ui.label(RichText::new("Select a post to load properties.").weak());
    } else {
        let keys: Vec<String> = draft.post_properties.keys().cloned().collect();
        for k in keys {
            if let Some(v) = draft.post_properties.get_mut(&k) {
                ui.horizontal(|ui| {
                    ui.label(&k);
                    ui.text_edit_singleline(v);
                });
            }
        }
    }
}

fn operations_tab(ui: &mut egui::Ui, app: &SolveApp, draft: &mut NcProgram, setup_names: &[String]) {
    ui.heading("Operations");
    ui.checkbox(&mut draft.reorder_minimize_tool_changes, "Reorder to Minimize Tool Changes");
    ui.label(RichText::new("Include setups / operations to post.").weak());
    ui.separator();

    // Left: tree of setups with checkboxes for ops
    for sn in setup_names {
        let Some(setup) = app.session.doc.manufacture.setups.get(sn) else {
            continue;
        };
        let all_selected = !setup.operations.is_empty()
            && (0..setup.operations.len()).all(|i| draft.operation_refs.iter().any(|r| r.setup == *sn && r.operation_index == i));
        let mut setup_on = all_selected || draft.setup_names.iter().any(|s| s == sn);
        ui.horizontal(|ui| {
            if ui.checkbox(&mut setup_on, sn).changed() {
                if setup_on {
                    if !draft.setup_names.iter().any(|s| s == sn) {
                        draft.setup_names.push(sn.clone());
                    }
                    for i in 0..setup.operations.len() {
                        let r = NcOpRef { setup: sn.clone(), operation_index: i };
                        if !draft.operation_refs.iter().any(|x| x == &r) {
                            draft.operation_refs.push(r);
                        }
                    }
                } else {
                    draft.setup_names.retain(|s| s != sn);
                    draft.operation_refs.retain(|r| r.setup != *sn);
                }
            }
        });
        for (i, op) in setup.operations.iter().enumerate() {
            let r = NcOpRef { setup: sn.clone(), operation_index: i };
            let mut on = draft.operation_refs.iter().any(|x| x == &r);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                let label = format!("{} — {}", op.display_name(), tool_label(&setup.tools, op));
                if ui.checkbox(&mut on, label).changed() {
                    if on {
                        draft.operation_refs.push(r);
                        if !draft.setup_names.iter().any(|s| s == sn) {
                            draft.setup_names.push(sn.clone());
                        }
                    } else {
                        draft.operation_refs.retain(|x| !(x.setup == *sn && x.operation_index == i));
                    }
                }
            });
        }
    }

    ui.separator();
    ui.heading("Selected");
    ui.horizontal(|ui| {
        ui.label(RichText::new("Operation").strong());
        ui.label(RichText::new("Setup").strong());
        ui.label(RichText::new("Tool").strong());
        ui.label(RichText::new("WCS").strong());
    });
    for (op, setup, tool, wcs) in collect_op_rows(app, draft) {
        ui.horizontal(|ui| {
            ui.label(&op);
            ui.label(&setup);
            ui.label(&tool);
            ui.label(&wcs);
        });
    }
}

fn run_post(app: &mut SolveApp, draft: &NcProgram) -> bool {
    if draft.post.is_none() && !draft.use_machine_configuration {
        app.set_status(app.tr("manufacture.post.need_post").to_string(), true);
        return false;
    }

    // Persist NC program in document when requested.
    if draft.create_in_document {
        match serde_json::to_value(draft) {
            Ok(v) => {
                if let Err(e) = app.run("manufacture.nc_programs.set", json!({"add": v})) {
                    app.set_status(e, true);
                    return false;
                }
            }
            Err(e) => {
                app.set_status(e.to_string(), true);
                return false;
            }
        }
    }

    let active = app.manufacture_workspace.active_setup.clone();
    if !active.is_empty() {
        if let Some(post) = &draft.post {
            let _ = app.run("manufacture.settings.set", json!({"name": active, "set": {"post": post}}));
        }
    }

    let Some(code) = local_post_gcode(app, draft) else {
        app.set_status(app.tr("manufacture.post.need_toolpath").to_string(), true);
        return false;
    };

    if code.is_empty() {
        app.set_status(app.tr("manufacture.post.no_output").to_string(), true);
        return false;
    }

    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Post;

    let file_name = draft.output_file_name();
    let folder = draft.output_folder.trim().to_string();
    if !folder.is_empty() {
        let path = std::path::Path::new(&folder).join(&file_name);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&path, code.as_bytes()) {
            app.set_status(format!("{}: {e}", app.tr("manufacture.post.save_error")), true);
            return false;
        }
        app.set_status(format!("{} → {}", app.tr("manufacture.post.posted"), path.display()), false);
        return true;
    }

    // No output folder: ask where to save the NC file.
    let code_save = code.clone();
    let suggested = file_name;
    app.pick_save(&suggested, &["nc", "gcode", "txt", "cnc"], move |app, path| {
        let Some(path) = path else { return };
        if let Err(e) = std::fs::write(&path, code_save.as_bytes()) {
            app.set_status(format!("{}: {e}", app.tr("manufacture.post.save_error")), true);
        } else {
            app.set_status(format!("{} → {path}", app.tr("manufacture.post.posted")), false);
        }
    });
    true
}

/// Build G-code from in-memory toolpath + setup post profile (no companion).
fn local_post_gcode(app: &SolveApp, draft: &NcProgram) -> Option<String> {
    let segs = &app.manufacture_workspace.toolpath_segments;
    if segs.is_empty() {
        return None;
    }
    let post_id = draft.post.as_deref().or_else(|| {
        let active = app.manufacture_workspace.active_setup.as_str();
        app.session.doc.manufacture.setups.get(active).and_then(|s| s.post.as_deref())
    });
    let profile = post_id.and_then(|id| app.session.doc.manufacture.posts.get(id)).cloned().unwrap_or_default();
    Some(emit_nc(segs, &profile, draft))
}

fn emit_nc(segs: &[PathSegment], profile: &PostProfile, draft: &NcProgram) -> String {
    let eol = match profile.line_ending {
        LineEnding::Lf => "\n",
        LineEnding::CrLf => "\r\n",
    };
    let mut out = String::new();
    if profile.preamble.is_empty() {
        out.push_str("G21");
        out.push_str(eol);
        out.push_str("G90");
        out.push_str(eol);
    } else {
        for line in profile.preamble.lines() {
            out.push_str(line);
            out.push_str(eol);
        }
    }
    let comment = draft.program_name.trim();
    if !comment.is_empty() {
        out.push_str(&format!("( {comment} )"));
        out.push_str(eol);
    }
    let mut pos = [0.0_f64; 3];
    for seg in segs {
        match seg {
            PathSegment::Rapid { to } => {
                out.push_str(&format!("G0 X{:.4} Y{:.4} Z{:.4}", to[0], to[1], to[2]));
                out.push_str(eol);
                pos = *to;
            }
            PathSegment::Linear { to, feed } => {
                out.push_str(&format!("G1 X{:.4} Y{:.4} Z{:.4} F{:.1}", to[0], to[1], to[2], feed));
                out.push_str(eol);
                pos = *to;
            }
            PathSegment::Arc { center, to, feed, clockwise } => {
                let code = if *clockwise { "G2" } else { "G3" };
                let i = center[0] - pos[0];
                let j = center[1] - pos[1];
                out.push_str(&format!("{code} X{:.4} Y{:.4} Z{:.4} I{:.4} J{:.4} F{:.1}", to[0], to[1], to[2], i, j, feed));
                out.push_str(eol);
                pos = *to;
            }
        }
    }
    if profile.postamble.is_empty() {
        out.push_str("M30");
        out.push_str(eol);
    } else {
        for line in profile.postamble.lines() {
            out.push_str(line);
            out.push_str(eol);
        }
    }
    out
}

fn show_machining_time(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut st) = app.manufacture_workspace.machining_time_dialog.clone() else {
        return;
    };
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("mfg_machining_time"), "settings", "Machining Time").fields().cards(1.5).show(
        ui.ctx(),
        true,
        |ui| {
            ui.heading("Estimate");
            ui.horizontal(|ui| {
                ui.label("Feed Scale");
                ui.add(egui::DragValue::new(&mut st.feed_scale).speed(0.01).range(0.1..=5.0));
            });
            ui.horizontal(|ui| {
                ui.label("Rapid Feed");
                ui.add(egui::DragValue::new(&mut st.rapid_mmpm).speed(50.0).range(100.0..=50_000.0).suffix(" mm/min"));
            });
            ui.horizontal(|ui| {
                ui.label("Tool Change Time");
                ui.add(egui::DragValue::new(&mut st.tool_change_s).speed(0.5).range(0.0..=300.0).suffix(" s"));
            });
            if ui.button("Calculate").clicked() {
                st.estimated_s = Some(compute_cycle_time(app, &st));
            }
            ui.separator();
            let secs = st.estimated_s.unwrap_or_else(|| compute_cycle_time(app, &st));
            ui.label(format!("Estimated cycle time: {}", format_cycle_time(secs)));
            if app.manufacture_workspace.toolpath_segments.is_empty() {
                ui.label(RichText::new("Generate a toolpath for a more accurate estimate.").weak());
            }
        },
    );
    if closed || action == crate::ui_kit::Action::Cancel || action == crate::ui_kit::Action::Ok {
        app.manufacture_workspace.machining_time_dialog = None;
    } else {
        app.manufacture_workspace.machining_time_dialog = Some(st);
    }
}

fn show_setup_sheet(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(st) = app.manufacture_workspace.setup_sheet_dialog.clone() else {
        return;
    };
    let (closed, action) =
        crate::ui_kit::Window::new(egui::Id::new("mfg_setup_sheet"), "settings", "Setup Sheet").fields().cards(1.5).show(ui.ctx(), true, |ui| {
            ui.label(RichText::new(&st.title).strong());
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                ui.code(&st.html);
            });
            if ui.button("Save HTML").clicked() {
                let html = st.html.clone();
                app.pick_save("setup_sheet.html", &["html", "htm"], move |app, path| {
                    let Some(path) = path else { return };
                    if let Err(e) = std::fs::write(&path, html.as_bytes()) {
                        app.set_status(format!("{}: {e}", app.tr("manufacture.post.save_error")), true);
                    } else {
                        app.set_status(app.tr("manufacture.setup_sheet.saved").to_string(), false);
                    }
                });
            }
        });
    if closed || action == crate::ui_kit::Action::Cancel || action == crate::ui_kit::Action::Ok {
        app.manufacture_workspace.setup_sheet_dialog = None;
    }
}
