//! Selective import of editable sketches from a FreeCAD project.

use std::collections::BTreeSet;

use serde_json::json;
use solidcraft_engine::io::{FreeCadNode, FreeCadNodeKind, FreeCadOperation, FreeCadSketchPreview, FreeCadWarning, FreeCadWarningSeverity};

use crate::{SolveApp, ui_kit};

pub(crate) struct Dialog {
    path: String,
    project: String,
    nodes: Vec<FreeCadNode>,
    selected: BTreeSet<String>,
    organization: usize,
    naming: usize,
    detail: Option<FreeCadWarning>,
    preserve_planes: bool,
    operations: Vec<FreeCadOperation>,
    selected_operations: BTreeSet<String>,
    import_operations: bool,
    report: Option<(String, Vec<FreeCadWarning>)>,
    report_message: Option<(String, bool)>,
    previews: Vec<FreeCadSketchPreview>,
    report_images: bool,
    error: Option<String>,
}

impl Dialog {
    fn new(path: String, project: String, nodes: Vec<FreeCadNode>) -> Self {
        let mut selected = BTreeSet::new();
        for node in &nodes {
            collect_sketches(node, &mut selected);
        }
        Self {
            path,
            project,
            nodes,
            selected,
            organization: 0,
            naming: 0,
            detail: None,
            preserve_planes: true,
            operations: Vec::new(),
            selected_operations: BTreeSet::new(),
            import_operations: true,
            report: None,
            report_message: None,
            previews: Vec::new(),
            report_images: true,
            error: None,
        }
    }
}

/// Ask for an FCStd project and open its sketch hierarchy without changing the design.
pub(crate) fn start(app: &mut SolveApp) {
    app.pick_open_for(&["fcstd"], |app, path| {
        let Some(path) = path else { return };
        match app.run("file.inspect_freecad_sketches", json!({"path": &path})) {
            Ok(value) => {
                let project = value["project"].as_str().unwrap_or("FreeCAD project").to_string();
                match serde_json::from_value::<Vec<FreeCadNode>>(value["nodes"].clone()) {
                    Ok(nodes) => match serde_json::from_value::<Vec<FreeCadOperation>>(value["operations"].clone()) {
                        Ok(operations) => {
                            let mut dialog = Dialog::new(path, project, nodes);
                            dialog.selected_operations = operations.iter().map(|op| op.source_name.clone()).collect();
                            dialog.operations = operations;
                            app.freecad_import = Some(dialog);
                        }
                        Err(error) => app.set_status(format!("Could not read the FreeCAD operation list: {error}"), true),
                    },
                    Err(error) => app.set_status(format!("Could not read the FreeCAD sketch list: {error}"), true),
                }
            }
            Err(error) => app.set_status(error, true),
        }
    });
}

fn collect_sketches(node: &FreeCadNode, selected: &mut BTreeSet<String>) {
    if node.kind == FreeCadNodeKind::Sketch {
        selected.insert(node.source_name.clone());
    }
    for child in &node.children {
        collect_sketches(child, selected);
    }
}

fn sketch_count(node: &FreeCadNode) -> usize {
    usize::from(node.kind == FreeCadNodeKind::Sketch) + node.children.iter().map(sketch_count).sum::<usize>()
}

fn selected_count(node: &FreeCadNode, selected: &BTreeSet<String>) -> usize {
    usize::from(node.kind == FreeCadNodeKind::Sketch && selected.contains(&node.source_name))
        + node.children.iter().map(|child| selected_count(child, selected)).sum::<usize>()
}

fn set_selected(node: &FreeCadNode, selected: &mut BTreeSet<String>, on: bool) {
    if node.kind == FreeCadNodeKind::Sketch {
        if on {
            selected.insert(node.source_name.clone());
        } else {
            selected.remove(&node.source_name);
        }
    }
    for child in &node.children {
        set_selected(child, selected, on);
    }
}

fn node_label(node: &FreeCadNode) -> String {
    match (&node.kind, &node.sketch) {
        (FreeCadNodeKind::Part, _) => format!("{} (Part)", node.label),
        (FreeCadNodeKind::Body, _) => format!("{} (Body)", node.label),
        (FreeCadNodeKind::Sketch, Some(info)) => {
            format!("{} (Sketch: {} geometries, {} constraints)", node.label, info.geometry, info.constraints)
        }
        (FreeCadNodeKind::Sketch, None) => format!("{} (Sketch)", node.label),
    }
}

fn draw_node(ui: &mut egui::Ui, node: &FreeCadNode, selected: &mut BTreeSet<String>, detail: &mut Option<FreeCadWarning>, operations: bool) {
    let total = sketch_count(node);
    let picked = selected_count(node, selected);
    let mut on = picked > 0;
    let response = ui_kit::checkbox_indeterminate(ui, &node_label(node), &mut on, picked > 0 && picked < total);
    if response.changed() {
        set_selected(node, selected, on);
    }
    if let Some(info) = &node.sketch {
        if !info.plane.is_empty() {
            let plane = if operations && info.attachment_unresolved && info.saved_plane_available {
                "Saved plane (attachment needs repair)"
            } else {
                &info.plane
            };
            ui_kit::text(ui, plane, ui_kit::Text::Note("plane"));
        }
        for warning in &info.warnings {
            let mut warning = warning.clone();
            if operations && info.saved_plane_available && warning.summary.contains("the sketch will use XY") {
                warning.summary = "The saved sketch plane is retained; its source attachment needs repair.".into();
            }
            draw_warning(ui, &warning, detail);
        }
    }
    if !node.children.is_empty() {
        ui.indent(("freecad_children", &node.source_name), |ui| {
            for child in &node.children {
                draw_node(ui, child, selected, detail, operations);
            }
        });
    }
}

fn draw_warning(ui: &mut egui::Ui, warning: &FreeCadWarning, detail: &mut Option<FreeCadWarning>) {
    ui.horizontal(|ui| {
        if ui_kit::button(ui, "More…", ui_kit::Button::Small).clicked() {
            *detail = Some(warning.clone());
        }
        ui_kit::text(ui, &warning.summary, ui_kit::Text::Report("freecad_warning", warning.severity == FreeCadWarningSeverity::Issue));
    });
}

fn import(app: &mut SolveApp, dialog: &mut Dialog) -> bool {
    let sketches: Vec<&str> = dialog.selected.iter().map(String::as_str).collect();
    let organization = if dialog.organization == 0 { "hierarchy" } else { "flat" };
    let naming = ["original", "body", "path"].get(dialog.naming).copied().unwrap_or("original");
    let mut params = json!({"path": &dialog.path, "sketches": sketches, "organization": organization, "preserve_planes": dialog.preserve_planes,
        "import_operations": dialog.import_operations, "operations": dialog.selected_operations});
    if organization == "flat" {
        params["naming"] = json!(naming);
    }
    match app.run("file.import_freecad_sketches", params) {
        Ok(value) => {
            let count = value["imported"].as_array().map_or(0, Vec::len);
            let warning_count = value["warnings"].as_array().map_or(0, Vec::len);
            let suffix = if warning_count == 0 { String::new() } else { format!("; {warning_count} warnings") };
            let operations = value["operations"].as_array().cloned().unwrap_or_default();
            let summary = if dialog.import_operations {
                let built = operations.iter().filter(|op| op["status"] == "imported").count();
                let repair = operations.iter().filter(|op| op["status"] == "needs_repair").count();
                let skipped = operations.iter().filter(|op| op["status"] == "skipped").count();
                format!("Imported {count} sketches; {built} operations rebuilt, {repair} suppressed for repair, {skipped} skipped.")
            } else {
                format!("Imported {count} FreeCAD sketches{suffix}.")
            };
            let mut warnings = Vec::new();
            dialog.previews.clear();
            for item in value["imported"].as_array().into_iter().flatten().chain(operations.iter()) {
                let name = item["name"].as_str().unwrap_or("FreeCAD item");
                for warning in item["warnings"].as_array().into_iter().flatten() {
                    if let Ok(mut warning) = serde_json::from_value::<FreeCadWarning>(warning.clone()) {
                        warning.summary = format!("{name}: {}", warning.summary);
                        warnings.push(warning);
                    }
                }
                if item["warnings"].as_array().is_some_and(|warnings| !warnings.is_empty())
                    && let Ok(preview) = serde_json::from_value::<FreeCadSketchPreview>(item["preview"].clone())
                {
                    dialog.previews.push(preview);
                }
            }
            for issue in value["body_warnings"].as_array().into_iter().flatten() {
                if let Ok(warning) = serde_json::from_value::<FreeCadWarning>(issue.clone()) {
                    warnings.push(warning);
                }
            }
            app.set_status(&summary, false);
            dialog.report = Some((summary, warnings));
            false
        }
        Err(error) => {
            dialog.error = Some(error);
            false
        }
    }
}

fn show_detail(detail: &mut Option<FreeCadWarning>, ctx: &egui::Context) {
    let Some(warning) = detail.clone() else { return };
    let all = format!("{}\n\n{}", warning.summary, warning.detail);
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_freecad_import_warning"), "warning", "FreeCAD Import Details")
        .actions("", "Close", None)
        .show(ctx, true, |ui| {
            if ui_kit::button(ui, "Copy", ui_kit::Button::Normal).clicked() {
                ui.ctx().copy_text(all.clone());
            }
            let heading = ui.add(egui::Label::new(egui::RichText::new(&warning.summary).strong()).wrap().selectable(true));
            let detail = ui.add(egui::Label::new(&warning.detail).wrap().selectable(true));
            for response in [&heading, &detail] {
                ui_kit::Menu::context(response, |ui| {
                    if crate::context_menu::button(ui, "Copy all", "copy").clicked() {
                        ui.ctx().copy_text(all.clone());
                        ui.close();
                    }
                });
            }
        });
    if closed || action == ui_kit::Action::Cancel {
        *detail = None;
    }
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if app.freecad_operation_detail.is_some()
        && let Some(dialog) = app.freecad_import.as_mut()
    {
        dialog.detail = None;
    }
    show_detail(&mut app.freecad_operation_detail, ctx);
    let Some(mut dialog) = app.freecad_import.take() else { return };
    if let Some((summary, warnings)) = dialog.report.clone() {
        let can_save = app.services.pick_save.is_some() && !app.file_picker_busy();
        let mut copy = false;
        let mut save = false;
        let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_freecad_import_report"), "import", "FreeCAD Import Report")
            .actions("", "Close", None).cards(2.0).show(ctx, true, |ui| {
                ui_kit::text(ui, &dialog.project, ui_kit::Text::Heading("project"));
                ui_kit::text(ui, &dialog.path, ui_kit::Text::Note("source"));
                ui_kit::text(ui, &summary, ui_kit::Text::Heading("result"));
                ui.horizontal(|ui| {
                    copy = ui_kit::button(ui, "Copy", ui_kit::Button::Normal).clicked();
                    save = ui_kit::button(ui, "Save As…", ui_kit::Button::Enabled(can_save)).clicked();
                });
                ui_kit::text(ui, "Copy and Save As include every issue and warning with its full More… explanation.", ui_kit::Text::Note("export_help"));
                if !dialog.previews.is_empty() {
                    ui_kit::checkbox(ui, "Include sketch issue images when saving", &mut dialog.report_images);
                }
                ui_kit::text(ui, "Save As writes Markdown with available PNG images in an adjacent folder. Keep the report and folder together. Copy includes Markdown text only.", ui_kit::Text::Note("image_help"));
                ui_kit::text(ui, "Repair the indicated sketch planes, profiles or targets, then enable suppressed features in dependency order. Skipped operations appear as [Not imported] placeholders; double-click one for its explanation. Rebuild skipped operations manually.", ui_kit::Text::Note("repair_help"));
                let mut issues = 0;
                let mut notices = 0;
                for warning in &warnings {
                    let mut numbered = warning.clone();
                    numbered.summary = if warning.severity == FreeCadWarningSeverity::Warning {
                        notices += 1;
                        format!("Warning {notices}: {}", warning.summary)
                    } else {
                        issues += 1;
                        format!("Issue {issues}: {}", warning.summary)
                    };
                    draw_warning(ui, &numbered, &mut dialog.detail);
                }
                if let Some((message, error)) = &dialog.report_message {
                    ui_kit::text(ui, message, ui_kit::Text::Report("report_message", *error));
                }
            });
        let params = if copy || save {
            json!({"report": {"project": &dialog.project, "source": &dialog.path, "summary": &summary, "warnings": &warnings, "previews": &dialog.previews}, "images":dialog.report_images})
        } else {
            serde_json::Value::Null
        };
        if copy {
            match app.run("file.freecad_import_report", params.clone()) {
                Ok(report) => {
                    if let Some(text) = report["text"].as_str() {
                        ctx.copy_text(text.to_string());
                        dialog.report_message = Some(("Complete report copied to the clipboard.".into(), false));
                    }
                }
                Err(error) => dialog.report_message = Some((error, true)),
            }
        }
        if save {
            let stem = std::path::Path::new(&dialog.path).file_stem().and_then(|stem| stem.to_str()).unwrap_or("FreeCAD");
            let name = format!("{stem}-import-report.md");
            app.pick_save(&name, &["md"], move |app, path| {
                let Some(path) = path else { return };
                let mut params = params;
                params["path"] = json!(&path);
                let message = match app.run("file.freecad_import_report", params) {
                    Ok(value) => {
                        let images = value["images"].as_array().map_or(0, Vec::len);
                        let errors = value["image_errors"].as_array().map_or(0, Vec::len);
                        (format!("Report saved to {path} with {images} sketch images; {errors} images unavailable."), errors > 0)
                    }
                    Err(error) => (error, true),
                };
                app.set_status(&message.0, message.1);
                if let Some(dialog) = app.freecad_import.as_mut() {
                    dialog.report_message = Some(message);
                }
            });
        }
        show_detail(&mut dialog.detail, ctx);
        if !closed && action != ui_kit::Action::Cancel {
            app.freecad_import = Some(dialog);
        }
        return;
    }
    let valid = !dialog.selected.is_empty() || (dialog.import_operations && !dialog.selected_operations.is_empty());
    if dialog.import_operations {
        dialog.preserve_planes = true;
    }
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_freecad_import"), "import", "Import FreeCAD")
        .actions("Import", "Cancel", None)
        .cards(2.0)
        .show(ctx, valid, |ui| {
            ui_kit::text(ui, &dialog.project, ui_kit::Text::Heading("project"));
            ui_kit::text(ui, &dialog.path, ui_kit::Text::Note("source"));
            ui_kit::text(
                ui,
                "Choose sketches to add to this design. Origin and datum plane placements are preserved where possible. Attachment exceptions have a More… explanation.",
                ui_kit::Text::Note("help"),
            );
            ui.add_enabled_ui(!dialog.import_operations, |ui| { ui_kit::checkbox(ui, "Preserve non-face sketch planes", &mut dialog.preserve_planes); });
            if !dialog.preserve_planes {
                ui_kit::text(ui, "All selected sketches will use the XY origin plane.", ui_kit::Text::Note("plane_override"));
            }
            ui_kit::row(ui, "Organization", |ui| {
                ui_kit::dropdown(
                    ui,
                    "freecad_organization",
                    &mut dialog.organization,
                    &["Preserve FreeCAD hierarchy", "Flat in active component"],
                )
            });
            if dialog.organization == 0 {
                ui_kit::text(
                    ui,
                    "FreeCAD Parts and Bodies become nested components. Each Body component contains its own Bodies, Sketches and Features folders. Missing solids are explained in the import report.",
                    ui_kit::Text::Note("organization_help"),
                );
            } else {
                ui_kit::row(ui, "Sketch names", |ui| {
                    ui_kit::dropdown(
                        ui,
                        "freecad_naming",
                        &mut dialog.naming,
                        &["Original name", "Body — Sketch", "Part — Body — Sketch"],
                    )
                });
            }
            ui_kit::prominent_section(ui, "freecad_sketches", "Sketches", true, |ui| {
                for node in &dialog.nodes {
                    draw_node(ui, node, &mut dialog.selected, &mut dialog.detail, dialog.import_operations);
                }
                if dialog.nodes.is_empty() {
                    ui_kit::text(ui, "This project contains no Sketcher sketches.", ui_kit::Text::Report("empty", true));
                }
            });
            ui_kit::prominent_section(ui, "freecad_operations", "Operations", true, |ui| {
                if dialog.operations.is_empty() {
                    ui_kit::text(ui, "This project contains no recognized operations.", ui_kit::Text::Note("no_operations"));
                } else {
                    ui_kit::checkbox(ui, "Import supported operations", &mut dialog.import_operations);
                    if dialog.import_operations {
                        dialog.preserve_planes = true;
                        ui_kit::text(ui, "Required sketches and preceding operations are included automatically. Unresolved face attachments keep their saved plane; affected operations are suppressed for repair.", ui_kit::Text::Note("operations_help"));
                        for operation in &dialog.operations {
                            let mut on = dialog.selected_operations.contains(&operation.source_name);
                            let kind = operation.type_name.rsplit("::").next().unwrap_or(&operation.type_name);
                            let label = match &operation.body {
                                Some((_, body)) => format!("{body} — {} ({kind})", operation.label),
                                None => format!("{} ({kind})", operation.label),
                            };
                            if ui_kit::checkbox(ui, &label, &mut on).changed() {
                                if on { dialog.selected_operations.insert(operation.source_name.clone()); } else { dialog.selected_operations.remove(&operation.source_name); }
                            }
                            for warning in operation.warnings.iter().chain(&operation.blockers) { draw_warning(ui, warning, &mut dialog.detail); }
                        }
                    }
                }
            });
            if let Some(error) = &dialog.error {
                ui_kit::text(ui, error, ui_kit::Text::Report("error", true));
            }
        });
    show_detail(&mut dialog.detail, ctx);
    if closed || action == ui_kit::Action::Cancel || (action == ui_kit::Action::Ok && import(app, &mut dialog)) {
        return;
    }
    app.freecad_import = Some(dialog);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sketch(name: &str) -> FreeCadNode {
        FreeCadNode { source_name: name.into(), label: name.into(), kind: FreeCadNodeKind::Sketch, children: Vec::new(), sketch: None }
    }

    #[test]
    fn selecting_a_group_controls_all_descendant_sketches() {
        let group = FreeCadNode {
            source_name: "Body".into(),
            label: "Body".into(),
            kind: FreeCadNodeKind::Body,
            children: vec![sketch("A"), sketch("B")],
            sketch: None,
        };
        let mut selected = BTreeSet::new();
        set_selected(&group, &mut selected, true);
        assert_eq!(selected, BTreeSet::from(["A".into(), "B".into()]));
        set_selected(&group.children[0], &mut selected, false);
        assert_eq!((sketch_count(&group), selected_count(&group, &selected)), (2, 1));
        set_selected(&group, &mut selected, false);
        assert!(selected.is_empty());
    }
}
