//! Render shell composition; all hosting and chrome belong to the shared kit.
use crate::{
    SolveApp,
    theme::Tokens,
    ui_kit::{self, Container, Edge},
};
use egui::{Rect, pos2, vec2};

pub(crate) fn reset_shell(app: &mut SolveApp) {
    for id in ["render:inspector", "render:browser", "render:shots_strip", "render:hud", "render:shots:show"] {
        crate::shell::register_panel(app, id, None, None);
    }
}

pub(crate) fn canvas(app: &SolveApp, viewport: Rect) -> Rect {
    crate::shell::content_rect(app, viewport)
}

pub(crate) fn overlays(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(viewport) = app.viewport.rect else { return };
    let free = canvas(app, viewport);
    if !free.is_positive() {
        return;
    }
    let ctx = ui.ctx().clone();
    let hud = Container::navigation_card(&ctx, egui::Id::new("sc_render_hud"), pos2(free.center().x, free.top()), egui::Align2::CENTER_TOP, |ui| {
        ui.set_max_width((free.width() - Tokens::get().space_24).max(0.0));
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(false, |ui| {
                ui_kit::text(ui, crate::i18n::tr_label("Pick"), ui_kit::Text::Inline);
                ui_kit::dropdown_menu(ui, "render_pick", crate::i18n::tr_label("Body"), |ui| {
                    for label in ["Body", "Face", "Component"] {
                        ui_kit::text(ui, crate::i18n::tr_label(label), ui_kit::Text::Inline);
                    }
                });
            })
            .response
            .on_disabled_hover_text(crate::i18n::tr_label("Product picking is not supported yet."))
            .on_hover_text(crate::i18n::tr_label("Product picking is not supported yet."));
            ui_kit::text(ui, crate::i18n::tr_label("Integrated preview"), ui_kit::Text::Inline)
                .on_hover_text(crate::i18n::tr_label("The live preview shows direct light only and no shadows."));
        });
    });
    crate::shell::register_panel(app, "render:hud", Some(hud.response.rect), None);
    crate::scenario::publish_rect("render:hud", hud.response.rect);
    crate::viewport::nav_bar(app, ui, viewport);
    shots_pill(app, ui, free);
}

fn shots_pill(app: &mut SolveApp, ui: &mut egui::Ui, free: Rect) {
    crate::shell::register_panel(app, "render:shots:show", None, None);
    if app.preferences.render_shots_strip {
        return;
    }
    let ctx = ui.ctx().clone();
    let pill = Container::navigation_card(
        &ctx,
        egui::Id::new("sc_render_shots_pill"),
        free.left_bottom() + vec2(Tokens::get().space_8, -Tokens::get().space_8),
        egui::Align2::LEFT_BOTTOM,
        |ui| {
            let response =
                ui_kit::button(ui, crate::i18n::tr_label("Shots"), ui_kit::Button::Normal).on_hover_text(crate::i18n::tr_label("Show Shots strip"));
            crate::scenario::publish_target("render:shots:show", response.rect);
            if response.clicked() {
                let _ = app.run("view.shots_strip", serde_json::json!({"visible":true}));
            }
        },
    );
    crate::shell::register_panel(app, "render:shots:show", Some(pill.response.rect), None);
}

/// Existing saved-shot and job actions inside the shared floating bottom-strip host.
pub(crate) fn shots_strip(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::shell::register_panel(app, "render:shots_strip", None, None);
    if !app.preferences.render_shots_strip {
        return;
    }
    let available = ui.available_rect_before_wrap();
    let content = ui_kit::Panel::bottom_strip(ui, "sc_render_shots_strip", available, 48.0, |ui| {
        ui.horizontal_centered(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let hide = ui_kit::button(ui, crate::i18n::tr_label("Hide"), ui_kit::Button::Normal)
                    .on_hover_text(crate::i18n::tr_label("Hide Shots strip"));
                crate::scenario::publish_target("render:shots:hide", hide.rect);
                if hide.clicked() {
                    let _ = app.run("view.shots_strip", serde_json::json!({"visible":false}));
                }
                let ready = crate::render_workspace::job_ready(app, "render.start");
                let render = ui_kit::button(ui, app.tr("render.image_start"), ui_kit::Button::Enabled(ready.is_ok()));
                let render = if let Err(reason) = ready { render.on_disabled_hover_text(reason) } else { render };
                crate::scenario::publish_target("render:shots:render", render.rect);
                if render.clicked() {
                    app.start("render.start");
                }
                let names: Vec<_> = app.session.doc.render.setups.keys().cloned().collect();
                egui::ScrollArea::horizontal()
                    .id_salt("render_shots_names")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui_kit::text(ui, crate::i18n::tr_label("Shots"), ui_kit::Text::Inline);
                            for name in names {
                                let shot = ui_kit::choice_chip(ui, &name, &name, name == app.render_workspace.view.name);
                                crate::scenario::publish_target(&format!("render:shots:setup:{name}"), shot.rect);
                                if shot.clicked() {
                                    crate::render_workspace::switch_shot(app, name);
                                }
                            }
                        });
                    });
            });
        });
    });
    let rect = content.map(|c| c.response.rect);
    crate::shell::register_panel(app, "render:shots_strip", rect, Some(Edge::Bottom));
    if let Some(rect) = rect {
        crate::scenario::publish_rect("render:shots_strip", rect);
    }
}
