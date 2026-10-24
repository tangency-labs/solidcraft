//! The Versions panel: the design's branches and frozen versions, drawn as a graph.

use egui::{Pos2, Rect, Stroke, Vec2, pos2};

use solidcraft_engine::doc::versions::{GraphKind, GraphPoint};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::ui_kit::{Container, Surface};

/// Draw the graph and the buttons that create a version, branch, switch or restore.
pub(super) fn rows(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    name: &mut String,
    selected: &mut Option<String>,
    notice: &mut Option<String>,
    error: &mut bool,
    take_theirs: &mut Vec<String>,
    compared: &mut Option<String>,
) -> bool {
    let t = Tokens::get();
    let viewing = app.session.version_view.clone();
    if let Some(view) = &viewing {
        crate::ui_kit::text(ui, &crate::i18n::format("ui.versions_viewing", &[("name", &view.name)]), crate::ui_kit::Text::Hint("versions"));
        if button(ui, "Back to workspace").clicked() {
            act(app, notice, error, "version.return", serde_json::json!({}));
        }
        ui.add_space(t.space_4);
    }
    ui_kit_hint(ui);
    let (enter, _) = crate::ui_kit::text_field(ui, name, crate::i18n::tr("ui.versions_name"));
    ui.add_space(t.space_4);
    let point = selected.as_deref().and_then(|id| app.session.doc.versions.layout().into_iter().find(|point| point.id == id));
    let version = point.as_ref().is_some_and(|point| point.kind == GraphKind::Version);
    let other_workspace = point.as_ref().is_some_and(|point| point.kind == GraphKind::Workspace && !point.current);
    let from_here = !version;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_4;
        let editing = viewing.is_none();
        if button_enabled(ui, "Create version", editing).clicked() || (enter && editing) {
            act(app, notice, error, "version.create", serde_json::json!({ "name": name.trim() }));
            if notice.is_none() {
                name.clear();
            }
        }
        if button_enabled(ui, "View version", version).clicked()
            && let Some(id) = selected.clone()
        {
            act(app, notice, error, "version.view", serde_json::json!({ "version": id }));
        }
        if button_enabled(ui, "New branch", editing).clicked() {
            let previous = app
                .session
                .doc
                .versions
                .workspaces
                .iter()
                .find(|workspace| workspace.id == app.session.doc.versions.current)
                .map(|workspace| workspace.name.clone())
                .unwrap_or_else(|| "Main".into());
            let version_name = point.as_ref().filter(|_| version).map(|item| item.name.clone()).unwrap_or_else(|| previous.clone());
            let chosen = fresh_branch_name(app, name);
            let params = if version && let Some(id) = selected.clone() {
                serde_json::json!({ "name": chosen, "version": id })
            } else {
                serde_json::json!({ "name": chosen })
            };
            if act(app, notice, error, "version.branch", params).is_some() {
                *selected = Some(app.session.doc.versions.current.clone());
                let message = if from_here {
                    crate::i18n::format("ui.versions_branched_here", &[("branch", chosen.as_str()), ("previous", previous.as_str())])
                } else {
                    crate::i18n::format(
                        "ui.versions_branched",
                        &[("branch", chosen.as_str()), ("version", version_name.as_str()), ("previous", previous.as_str())],
                    )
                };
                *notice = Some(message);
                name.clear();
            }
        }
        if button_enabled(ui, "Open workspace", other_workspace && editing).clicked()
            && let Some(id) = selected.clone()
        {
            act(app, notice, error, "version.switch", serde_json::json!({ "workspace": id }));
        }
        if button_enabled(ui, "Restore this version", version && editing).clicked()
            && let Some(id) = selected.clone()
        {
            act(app, notice, error, "version.restore", serde_json::json!({ "version": id }));
        }
    });
    ui.add_space(t.space_4);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_4;
        let editing = viewing.is_none();
        let selected_id = selected.clone();
        let can_rename = editing && selected_id.is_some() && !name.trim().is_empty();
        let can_delete = editing
            && point
                .as_ref()
                .is_some_and(|item| item.kind == GraphKind::Version || (item.kind == GraphKind::Workspace && !item.current && item.id != "main"));
        if button_enabled(ui, "Rename", can_rename).clicked()
            && let Some(id) = selected_id.clone()
        {
            let params = if version {
                serde_json::json!({ "name": name.trim(), "version": id })
            } else {
                serde_json::json!({ "name": name.trim(), "workspace": id })
            };
            if act(app, notice, error, "version.rename", params).is_some() {
                *notice = Some(crate::i18n::format("ui.versions_renamed", &[("name", name.trim())]));
                name.clear();
            }
        }
        if button_enabled(ui, "Delete", can_delete).clicked()
            && let Some(id) = selected.clone()
        {
            let label = point.as_ref().map(|item| item.name.clone()).unwrap_or_default();
            let params = if version { serde_json::json!({ "version": id }) } else { serde_json::json!({ "workspace": id }) };
            if act(app, notice, error, "version.delete", params).is_some() {
                *selected = None;
                *notice = Some(crate::i18n::format("ui.versions_deleted", &[("name", label.as_str())]));
            }
        }
        if button_enabled(ui, "Merge into here", other_workspace && editing).clicked()
            && let Some(id) = selected.clone()
        {
            let label = point.as_ref().map(|item| item.name.clone()).unwrap_or_default();
            let current = app
                .session
                .doc
                .versions
                .workspaces
                .iter()
                .find(|workspace| workspace.id == app.session.doc.versions.current)
                .map(|workspace| workspace.name.clone())
                .unwrap_or_else(|| "Main".into());
            let changes = app.session.doc.changes_for_workspace(&id).unwrap_or_default();
            let take: Vec<&str> = take_theirs.iter().map(String::as_str).filter(|path| changes.iter().any(|change| change.path == *path)).collect();
            let keep: Vec<&str> = changes
                .iter()
                .filter(|change| {
                    change.kind == solidcraft_engine::doc::versions::ChangeKind::Conflict && !take.iter().any(|path| *path == change.path)
                })
                .map(|change| change.path.as_str())
                .collect();
            if let Some(value) = act(app, notice, error, "version.merge", serde_json::json!({ "workspace": id, "take": take, "keep": keep })) {
                let key = match value.get("result").and_then(|item| item.as_str()) {
                    Some("brought") => "ui.versions_brought",
                    Some("already") => "ui.versions_merge_none",
                    _ => "ui.versions_merged",
                };
                *notice = Some(crate::i18n::format(key, &[("name", label.as_str()), ("current", current.as_str())]));
            }
        }
    });
    if compared.as_deref() != selected.as_deref() {
        *compared = selected.clone();
        take_theirs.clear();
    }
    let preview = match selected.as_deref() {
        Some(id) if version => app.session.doc.changes_for_version(id).map_err(|err| err.to_string()),
        Some(id) if other_workspace => app.session.doc.changes_for_workspace(id).map_err(|err| err.to_string()),
        _ => Ok(Vec::new()),
    };
    match preview {
        Ok(changes) if !changes.is_empty() || version || other_workspace => {
            egui::ScrollArea::vertical().id_salt("versions-diff").max_height(140.0).show(ui, |ui| {
                if changes.is_empty() {
                    ui.label(crate::i18n::tr("ui.versions_same"));
                }
                for change in &changes {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = t.space_4;
                        let key = match change.kind {
                            solidcraft_engine::doc::versions::ChangeKind::Added => "ui.versions_diff_added",
                            solidcraft_engine::doc::versions::ChangeKind::Removed => "ui.versions_diff_removed",
                            solidcraft_engine::doc::versions::ChangeKind::Changed => "ui.versions_diff_changed",
                            solidcraft_engine::doc::versions::ChangeKind::Conflict => "ui.versions_diff_conflict",
                        };
                        ui.label(crate::i18n::format(key, &[("name", change.name.as_str())]));
                        if change.kind == solidcraft_engine::doc::versions::ChangeKind::Conflict {
                            let taking = take_theirs.iter().any(|path| path == &change.path);
                            if ui.selectable_label(!taking, crate::i18n::tr("ui.versions_keep")).clicked() {
                                take_theirs.retain(|path| path != &change.path);
                            }
                            if ui.selectable_label(taking, crate::i18n::tr("ui.versions_take")).clicked() && !taking {
                                take_theirs.push(change.path.clone());
                            }
                        }
                    });
                }
            });
        }
        Err(message) => {
            crate::ui_kit::text(ui, &message, crate::ui_kit::Text::Hint("versions"));
        }
        _ => {}
    }
    if let Some(text) = notice {
        let style = if *error { crate::ui_kit::Text::Error("versions") } else { crate::ui_kit::Text::Hint("versions") };
        crate::ui_kit::text(ui, text, style);
    } else {
        crate::ui_kit::text(ui, crate::i18n::tr("ui.versions_restore_note"), crate::ui_kit::Text::Hint("versions"));
    }
    ui.add_space(t.space_6);
    let viewed = app.session.version_view.as_ref().map(|view| view.version.clone());
    egui::ScrollArea::both().id_salt("versions-graph").max_height(260.0).show(ui, |ui| {
        graph(ui, &app.session.doc.versions.layout(), selected, viewed.as_deref());
    });
    false
}

fn ui_kit_hint(ui: &mut egui::Ui) {
    crate::ui_kit::text(ui, crate::i18n::tr("ui.versions_hint"), crate::ui_kit::Text::Hint("versions"));
}

fn button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    button_enabled(ui, label, true)
}

fn button_enabled(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    crate::ui_kit::button(
        ui,
        crate::i18n::tr_label(label),
        if enabled { crate::ui_kit::Button::Normal } else { crate::ui_kit::Button::Enabled(false) },
    )
}

fn act(app: &mut SolveApp, notice: &mut Option<String>, error: &mut bool, command: &str, params: serde_json::Value) -> Option<serde_json::Value> {
    match app.session.execute(command, &params) {
        Ok(value) => {
            *notice = None;
            *error = false;
            Some(value)
        }
        Err(message) => {
            *notice = Some(message.to_string());
            *error = true;
            None
        }
    }
}

/// An empty name becomes Branch 2, Branch 3, and so on. A name already in use is returned as
/// typed, so the command can say it is taken.
fn fresh_branch_name(app: &SolveApp, typed: &str) -> String {
    let versions = &app.session.doc.versions;
    let taken = |candidate: &str| {
        versions.versions.iter().any(|item| item.name == candidate) || versions.workspaces.iter().any(|item| item.name == candidate)
    };
    let typed = typed.trim();
    if !typed.is_empty() {
        return typed.to_string();
    }
    let mut number = 2u32;
    loop {
        let candidate = format!("Branch {number}");
        if !taken(&candidate) {
            return candidate;
        }
        number += 1;
    }
}

fn graph(ui: &mut egui::Ui, points: &[GraphPoint], selected: &mut Option<String>, viewed: Option<&str>) {
    let t = Tokens::get();
    let (columns, rows) = points.iter().fold((0u32, 0u32), |(c, r), point| (c.max(point.column), r.max(point.row)));
    let cell = Vec2::new(176.0, 76.0);
    let size = Vec2::new(cell.x * (columns + 1) as f32 + 16.0, cell.y * (rows + 1) as f32 + 12.0);
    let (response, painter) = ui.allocate_painter(size, egui::Sense::click());
    let origin = response.rect.min + Vec2::new(8.0, 8.0);
    let at = |point: &GraphPoint| origin + Vec2::new(point.column as f32 * cell.x, point.row as f32 * cell.y);
    let node = |point: &GraphPoint| Rect::from_min_size(at(point) + Vec2::new(8.0, 16.0), Vec2::new(152.0, 44.0));
    for point in points {
        let Some(parent) = point.parent.as_deref().and_then(|id| points.iter().find(|other| other.id == id)) else { continue };
        let from = node(parent).right_center();
        let to = node(point).left_center();
        let mid = pos2((from.x + to.x) * 0.5, from.y);
        let down = pos2(mid.x, to.y);
        let stroke = Stroke::new(1.5, t.dialog_rule);
        if (from.y - to.y).abs() < 2.0 {
            painter.line_segment([from, to], stroke);
        } else {
            painter.line_segment([from, mid], stroke);
            painter.line_segment([mid, down], stroke);
            painter.line_segment([down, to], stroke);
        }
    }
    for point in points {
        let rect = node(point);
        let chosen = selected.as_deref() == Some(point.id.as_str());
        let on_screen = viewed == Some(point.id.as_str());
        let stroke = if chosen || point.current || on_screen { Stroke::new(2.0, t.form_primary) } else { Stroke::new(1.0, t.dialog_rule) };
        let fill = if on_screen {
            t.form_primary.gamma_multiply(0.35)
        } else if point.current {
            t.selected_bg
        } else {
            t.control_fill
        };
        painter.rect(rect, 8.0, fill, stroke, egui::StrokeKind::Inside);
        let kind = match point.kind {
            GraphKind::Version => "version",
            GraphKind::Workspace if point.current => "editing",
            GraphKind::Workspace => "workspace",
        };
        let name = fit_line(&painter, &point.name, t.font_body, t.text, rect.width() - 16.0);
        painter.galley(pos2(rect.left() + 8.0, rect.center().y - name.size().y + 1.0), name, t.text);
        let caption = fit_line(&painter, kind, t.font_caption, t.text_dim, rect.width() - 16.0);
        painter.galley(pos2(rect.left() + 8.0, rect.center().y + 1.0), caption, t.text_dim);
        crate::scenario::publish_handle(&format!("versions:{}", point.id), rect.center());
        if response.clicked() && rect.contains(response.interact_pointer_pos().unwrap_or(Pos2::ZERO)) {
            *selected = Some(point.id.clone());
        }
    }
}

fn fit_line(painter: &egui::Painter, text: &str, size: f32, color: egui::Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let font = egui::FontId::proportional(size);
    let mut shown: String = text.chars().collect();
    while painter.layout_no_wrap(shown.clone(), font.clone(), color).size().x > width && shown.chars().count() > 1 {
        shown.pop();
    }
    if shown.chars().count() < text.chars().count() {
        shown.pop();
        shown.push('…');
    }
    painter.layout_no_wrap(shown, font, color)
}

/// Fade a panel of editing controls while a version is only being viewed.
pub(crate) fn soften_while_viewing(app: &SolveApp, ui: &mut egui::Ui) {
    if app.session.version_view.is_none() {
        return;
    }
    ui.visuals_mut().disabled_alpha = 1.0;
    ui.set_opacity(0.38);
    ui.disable();
}

/// Shown over the model whenever a version is open, including when the Versions panel is closed.
pub(crate) fn banner(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(view) = app.session.version_view.clone() else { return };
    let t = Tokens::get();
    Container::new(ctx, Surface::Toast)
        .area(egui::Id::new("sc_version_banner"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, t.space_24 * 4.0))
        .show(ctx, |ui| {
            Container::popup_frame(ui.style()).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.space_8;
                    ui.label(crate::i18n::format("ui.versions_viewing", &[("name", view.name.as_str())]));
                    if button(ui, "Back to workspace").clicked() {
                        let _ = app.session.execute("version.return", &serde_json::json!({}));
                    }
                });
            });
            crate::scenario::publish_handle("versions:back", ui.min_rect().center());
        });
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn the_versions_panel_opens_from_manage() {
        let mut h = crate::scenario::Harness::new();
        h.step(&json!({"click":{"handle":"tab:MANAGE"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"button:version.graph"}})).unwrap();
        h.frames(2);
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(super::super::Kind::Versions { .. })));
    }
}
