//! Selective import of editable sketches from a FreeCAD project.

use std::collections::BTreeSet;

use serde_json::json;
use solidcraft_engine::io::{FreeCadNode, FreeCadNodeKind};

use crate::{SolveApp, ui_kit};

pub(crate) struct Dialog {
    path: String,
    project: String,
    nodes: Vec<FreeCadNode>,
    selected: BTreeSet<String>,
    error: Option<String>,
}

impl Dialog {
    fn new(path: String, project: String, nodes: Vec<FreeCadNode>) -> Self {
        let mut selected = BTreeSet::new();
        for node in &nodes {
            collect_sketches(node, &mut selected);
        }
        Self { path, project, nodes, selected, error: None }
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
                    Ok(nodes) => app.freecad_import = Some(Dialog::new(path, project, nodes)),
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

fn draw_node(ui: &mut egui::Ui, node: &FreeCadNode, selected: &mut BTreeSet<String>) {
    let total = sketch_count(node);
    let picked = selected_count(node, selected);
    let mut on = picked > 0;
    let response = ui_kit::checkbox_indeterminate(ui, &node_label(node), &mut on, picked > 0 && picked < total);
    if response.changed() {
        set_selected(node, selected, on);
    }
    if let Some(info) = &node.sketch {
        for warning in &info.warnings {
            ui_kit::text(ui, warning, ui_kit::Text::Report("freecad_warning", true));
        }
    }
    if !node.children.is_empty() {
        ui.indent(("freecad_children", &node.source_name), |ui| {
            for child in &node.children {
                draw_node(ui, child, selected);
            }
        });
    }
}

fn import(app: &mut SolveApp, dialog: &mut Dialog) -> bool {
    let sketches: Vec<&str> = dialog.selected.iter().map(String::as_str).collect();
    match app.run("file.import_freecad_sketches", json!({"path": &dialog.path, "sketches": sketches})) {
        Ok(value) => {
            let count = value["imported"].as_array().map_or(0, Vec::len);
            let warning_count = value["warnings"].as_array().map_or(0, Vec::len);
            let suffix = if warning_count == 0 { String::new() } else { format!("; {warning_count} warnings") };
            app.set_status(format!("Imported {count} FreeCAD sketches{suffix}"), false);
            true
        }
        Err(error) => {
            dialog.error = Some(error);
            false
        }
    }
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.freecad_import.take() else { return };
    let valid = !dialog.selected.is_empty();
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_freecad_import"), "import", "Import FreeCAD Sketches")
        .actions("Import", "Cancel", None)
        .cards(2.0)
        .show(ctx, valid, |ui| {
            ui_kit::text(ui, &dialog.project, ui_kit::Text::Heading("project"));
            ui_kit::text(ui, &dialog.path, ui_kit::Text::Note("source"));
            ui_kit::text(
                ui,
                "Choose sketches to add to this design. Their local geometry is retained on the XY origin plane; you can reassign each sketch plane after import.",
                ui_kit::Text::Note("help"),
            );
            for node in &dialog.nodes {
                draw_node(ui, node, &mut dialog.selected);
            }
            if dialog.nodes.is_empty() {
                ui_kit::text(ui, "This project contains no Sketcher sketches.", ui_kit::Text::Report("empty", true));
            }
            if let Some(error) = &dialog.error {
                ui_kit::text(ui, error, ui_kit::Text::Report("error", true));
            }
        });
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
