//! One floating editor for the selected Render item.
use super::{RenderSelection, environment, inspector, library, light, setups, shot, wear};
use crate::{SolveApp, ui_kit};
use inspector::selection_appearance;
use serde_json::json;

pub(crate) fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    // Reviews belong to the workspace, including when their originating editor closes.
    inspector::confirmation(app, ui.ctx());
    library::review::dialog(app, ui.ctx());
    library::review::confirmation(app, ui.ctx());
    if app.render_workspace.view.inspector.hidden || app.render_workspace.view.selection != RenderSelection::Environment {
        environment::stage_confirmation(app, ui.ctx());
    }
    if app.render_workspace.view.inspector.hidden {
        crate::shell::register_panel(app, "render:inspector", None, None);
        return;
    }
    setups::refresh_dirty(app);
    setups::sync_revision(app);
    let selection = app.render_workspace.view.selection.clone();
    let kind = kind(&selection);
    let appearance = matches!(selection, RenderSelection::Object(_));
    let (title, icon) = title(app, &selection);
    let scope = selection_appearance::scope_label(app).to_owned();
    let bounds = crate::shell::panel_bounds(app, ui.available_rect_before_wrap());
    let mut side = app.preferences.render_inspector_side.clone();
    let mut width = app.preferences.render_inspector_width;
    let action = std::cell::Cell::new(None);
    let content = ui_kit::Panel::workspace_with_header(
        ui,
        ui_kit::WorkspaceOptions {
            id: "render:inspector",
            title: &title,
            icon,
            bounds,
            fill_height: true,
            compact: crate::render_ui::compact(app, bounds.width()),
            editor: appearance,
            enabled: true,
            header_actions: None,
        },
        &mut side,
        &mut width,
        false,
        |ui, options| {
            if appearance {
                action.set(ui_kit::inspector_header(ui, options.icon, options.title, &scope));
            } else {
                ui_kit::panel::header(ui, options.icon, options.title, || action.set(Some(ui_kit::InspectorAction::Close)));
            }
        },
        |ui| {
            if appearance {
                ui_kit::Panel::editor_footer(ui, "render_appearance_footer", |ui| selection_appearance::footer(app, ui));
            } else if selection == RenderSelection::Output {
                ui_kit::Panel::pinned_footer(ui, "render_output_footer", 1, |ui| {
                    output_actions(app, ui);
                    crate::scenario::publish_rect("render:output-footer", ui.min_rect());
                });
            }
            egui::ScrollArea::vertical()
                .max_width(ui.available_width())
                .id_salt(("render_inspector", kind, app.docs.active))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let before = setups::draft(app).ok();
                    let rollback = app.render_workspace.view.clone();
                    let response = ui.scope(|ui| match &selection {
                        RenderSelection::Nothing => inspector::scene_controls(app, ui),
                        RenderSelection::Object(_) => {
                            selection_appearance::controls(app, ui);
                        }
                        RenderSelection::Wear => wear::controls(app, ui),
                        RenderSelection::Environment => environment::editor(app, ui),
                        RenderSelection::Lighting => light::lighting(app, ui),
                        RenderSelection::Light(id) => light::editor(app, ui, id),
                        RenderSelection::Shot(name) => shot::editor(app, ui, name),
                        RenderSelection::Output => inspector::output_editor(app, ui),
                        RenderSelection::Sampling => inspector::sampling_editor(app, ui),
                        RenderSelection::Effects => super::effects::output_controls(app, ui),
                        RenderSelection::Check => inspector::check_editor(app, ui),
                    });
                    if matches!(selection, RenderSelection::Output | RenderSelection::Sampling | RenderSelection::Effects | RenderSelection::Wear)
                        && let Some(before) = before
                        && let Err(error) = setups::persist_editor(app, &before)
                    {
                        // Refuse the numerical edit without undoing transient editor navigation.
                        let inspector = std::mem::take(&mut app.render_workspace.view.inspector);
                        app.render_workspace.view = rollback;
                        app.render_workspace.view.inspector = inspector;
                        app.set_status(error, true);
                    }
                    crate::scenario::publish_rect(&format!("render:editor:{kind}"), response.response.rect);
                });
        },
    );
    match action.get() {
        Some(ui_kit::InspectorAction::Close) => {
            let _ = app.run("view.render_inspector", json!({"visible":false}));
        }
        Some(ui_kit::InspectorAction::Remove) => selection_appearance::remove(app),
        None => {}
    }
    app.preferences.render_inspector_width = width;
    let requested = content
        .as_ref()
        .and_then(|c| c.inner.1)
        .map(str::to_string)
        .or_else(|| (app.preferences.render_inspector_side != side).then(|| side.clone()));
    if let Some(side) = requested {
        let _ = app.run("view.render_dock", json!({"panel":"inspector","side":side}));
    }
    let rect = content.map(|content| content.response.rect);
    crate::shell::register_panel(app, "render:inspector", rect, Some(if side == "left" { ui_kit::Edge::Left } else { ui_kit::Edge::Right }));
    if let Some(rect) = rect {
        crate::scenario::publish_rect("render:inspector", rect);
    }
}

pub(super) fn output_actions(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        for (id, label, target) in [
            ("render.start", app.tr("cmd.render_start").to_owned(), "render:output:render"),
            (
                "render.batch",
                crate::i18n::format("render.batch_all_shots", &[("count", &app.session.doc.render.setups.len().to_string())]),
                "render:output:all-shots",
            ),
        ] {
            let ready = super::ribbon::action_ready(app, id);
            let response = ui_kit::icon_label_action(ui, &label, Some("play"), ready.is_ok(), id == "render.start");
            let response = if let Err(reason) = ready { response.on_disabled_hover_text(reason) } else { response };
            crate::scenario::publish_target(target, response.rect);
            if response.clicked() {
                app.start(id);
            }
        }
    });
}

fn kind(selection: &RenderSelection) -> &'static str {
    match selection {
        RenderSelection::Nothing => "scene",
        RenderSelection::Object(_) => "appearance",
        RenderSelection::Wear => "wear",
        RenderSelection::Environment => "environment",
        RenderSelection::Lighting => "lighting",
        RenderSelection::Light(_) => "light",
        RenderSelection::Shot(_) => "shot",
        RenderSelection::Output => "output",
        RenderSelection::Sampling => "sampling",
        RenderSelection::Effects => "effects",
        RenderSelection::Check => "check",
    }
}

fn title(app: &SolveApp, selection: &RenderSelection) -> (String, &'static str) {
    let (key, icon) = match selection {
        RenderSelection::Nothing => ("render.page_scene", "render_scene"),
        RenderSelection::Object(items) => {
            let name = items
                .last()
                .and_then(|item| match item {
                    solidcraft_engine::Sel::Body { name } | solidcraft_engine::Sel::Face { body: name, .. } => Some(name.clone()),
                    solidcraft_engine::Sel::Occurrence { path } => app.session.occurrence_placement(path).ok().map(|p| p.name),
                    _ => None,
                })
                .unwrap_or_else(|| app.tr("render.page_appearance").to_owned());
            return (name, "body");
        }
        RenderSelection::Shot(name) => return (name.clone(), "camera"),
        RenderSelection::Light(id) => {
            let name = light::rows(app)
                .ok()
                .and_then(|rows| rows.into_iter().find(|row| &row.id == id))
                .map(|row| light::name(app, &row))
                .unwrap_or_else(|| app.tr("render.tree_lights").to_owned());
            return (name, "render_scene");
        }
        RenderSelection::Wear => ("render.wear_title", "appearance"),
        RenderSelection::Environment => ("render.page_environment", "environment_map"),
        RenderSelection::Lighting => ("render.page_lighting", "render_scene"),
        RenderSelection::Output => ("render.page_output", "export"),
        RenderSelection::Sampling => ("render.children.sampling", "settings"),
        RenderSelection::Effects => ("render.page_effects", "camera"),
        RenderSelection::Check => ("render.children.check", "inspect"),
    };
    (app.tr(key).to_owned(), icon)
}
