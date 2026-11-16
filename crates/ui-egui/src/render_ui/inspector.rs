//! Inspector content for the selected Render item.
use super::{RenderSelection, environment, inspector, library, light, setups, shot, wear};
use crate::ui_kit::BackingTooltipExt as _;
use crate::{SolveApp, ui_kit};
use inspector::selection_appearance;

/// Render owns reviews even when an originating panel is hidden.
pub(crate) fn owner_overlays(app: &mut SolveApp, ctx: &egui::Context) {
    inspector::confirmation(app, ctx);
    library::review::dialog(app, ctx);
    library::review::confirmation(app, ctx);
    environment::stage_confirmation(app, ctx);
}

pub(crate) fn shared_title(app: &SolveApp) -> (String, &'static str) {
    let (mut title, icon) = title(app, &app.render_workspace.view.selection);
    title.truncate(title.floor_char_boundary(256));
    (title, icon)
}

pub(crate) fn shared_status(app: &SolveApp) -> Option<String> {
    matches!(app.render_workspace.view.selection, RenderSelection::Object(_)).then(|| selection_appearance::scope_label(app).to_owned())
}

pub(crate) fn shared_remove_available(app: &SolveApp) -> bool {
    matches!(app.render_workspace.view.selection, RenderSelection::Object(_)) && selection_appearance::remove_is_current(app)
}

pub(crate) fn shared_remove(app: &mut SolveApp) {
    if shared_remove_available(app) {
        selection_appearance::remove(app);
    }
}

pub(crate) fn shared_body(app: &mut SolveApp, ui: &mut egui::Ui) {
    setups::refresh_dirty(app);
    setups::sync_revision(app);
    let selection = app.render_workspace.view.selection.clone();
    body(app, ui, &selection);
}

fn body(app: &mut SolveApp, ui: &mut egui::Ui, selection: &RenderSelection) {
    let top = ui.cursor().top();
    let available_height = ui.available_height();
    if matches!(selection, RenderSelection::Object(_)) {
        let stacked = selection_appearance::footer_stacked(app, ui);
        let band = ui_kit::Panel::pinned_footer(ui, "render_appearance_footer", if stacked { 2 } else { 1 }, |ui| {
            selection_appearance::footer(app, ui, stacked);
        });
        crate::scenario::publish_rect("render:appearance-footer", band.response.rect);
    } else if *selection == RenderSelection::Output {
        let labels = [
            app.tr("cmd.render_start").to_owned(),
            solidcraft_ui_text::format("render.batch_all_shots", &[("count", &app.session.doc.render.setups.len().to_string())]),
        ];
        let stacked = ui_kit::icon_label_width(ui, &labels[0], Some("play"), true)
            + ui.spacing().item_spacing.x
            + ui_kit::icon_label_width(ui, &labels[1], Some("play"), false)
            > ui.available_width();
        ui_kit::Panel::pinned_footer(ui, "render_output_footer", if stacked { 2 } else { 1 }, |ui| {
            output_actions(app, ui, labels, stacked);
            crate::scenario::publish_rect("render:output-footer", ui.min_rect());
        });
    }
    let footer = (available_height - ui.available_height()).max(0.0);
    let kind = kind(selection);
    let scroll = egui::ScrollArea::vertical()
        .min_scrolled_height(0.0)
        .max_width(ui.available_width())
        .id_salt(("render_inspector", kind, app.docs.active))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let before = setups::draft(app).ok();
            let rollback = app.render_workspace.view.clone();
            let response = ui.scope(|ui| match selection {
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
    crate::panel_runtime::report_content_extent(ui, top, scroll.content_size.y + footer, false);
}

fn output_actions(app: &mut SolveApp, ui: &mut egui::Ui, labels: [String; 2], stacked: bool) {
    let draw = |ui: &mut egui::Ui| {
        for ((id, target), label) in [("render.start", "render:output:render"), ("render.batch", "render:output:all-shots")].into_iter().zip(labels) {
            let ready = super::ribbon::action_ready(app, id);
            let primary = id == "render.start";
            let response = if ui_kit::icon_label_width(ui, &label, Some("play"), primary) > ui.available_width() {
                ui_kit::icon_action(ui, &label, "play", ready.is_ok(), false)
            } else {
                ui_kit::icon_label_action(ui, &label, Some("play"), ready.is_ok(), primary)
            };
            let response = if let Err(reason) = ready { response.backing_on_disabled_hover_text(reason) } else { response };
            crate::scenario::publish_target(target, response.rect);
            if response.clicked() {
                app.start(id);
            }
        }
    };
    if stacked {
        ui.vertical(draw);
    } else {
        ui.horizontal_wrapped(draw);
    }
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
