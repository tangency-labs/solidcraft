//! Interface settings composition, using shared controls and previews.
use super::*;
pub(super) fn interface(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_heading(ui, app.tr("prefs.interface"), app.tr("prefs.interface_note"));
    ui_kit::settings_section(ui, "theme", app.tr("ui.theme"), |ui| {
        theme_row(app, ui);
        ui_kit::tab_accent_strip(ui);
    });
    ui_kit::settings_section(ui, "layout", app.tr("prefs.layout"), |ui| {
        ui_kit::settings_pair(
            ui,
            |ui| {
                ui_kit::settings_form(ui, t.form_label_width + t.space_16 + t.space_6, |ui| {
                    ui_kit::row(ui, app.tr("prefs.workspace"), |ui| {
                        let mut selected = usize::from(app.ui.layout == crate::shell::LayoutMode::Docked);
                        if ui_kit::segmented(
                            ui,
                            &mut selected,
                            &[crate::i18n::tr("ui.layout_floating"), crate::i18n::tr("ui.layout_compact")],
                            Default::default(),
                        ) {
                            let _ = app.run("view.layout", json!({"mode": if selected == 1 { "compact" } else { "floating" }}));
                        }
                    });
                    ui_kit::row(ui, app.tr("prefs.toolbar"), |ui| {
                        let mut selected = usize::from(!app.preferences.ribbon_labels);
                        if ui_kit::segmented(ui, &mut selected, &[app.tr("prefs.labels"), app.tr("prefs.icons_only")], Default::default()) {
                            app.preferences.ribbon_labels = selected == 0;
                        }
                    });
                    ui_kit::row(ui, app.tr("prefs.history"), |ui| {
                        let mut selected = if app.preferences.timeline_left { if app.preferences.timeline_under_browser { 2 } else { 1 } } else { 0 };
                        if ui_kit::segmented(
                            ui,
                            &mut selected,
                            &[crate::i18n::tr("prefs.bottom"), crate::i18n::tr("prefs.left"), crate::i18n::tr("prefs.under_browser")],
                            ui_kit::Segments { proportional: true, compact: true, ..Default::default() },
                        ) {
                            let _ = app.run("view.history_layout", json!({"dock": match selected { 1=>"left",2=>"under_browser",_=>"bottom" }}));
                        }
                    });
                    for (short, legacy) in [
                        ("prefs.bottom", "ui.timeline_bottom"),
                        ("prefs.left", "ui.timeline_left_sidebar"),
                        ("prefs.under_browser", "ui.history_below_browser"),
                    ] {
                        let row = app.tr("prefs.history");
                        if let Some(at) = crate::scenario::handle_at(&format!("dialog:{row}:{}", app.tr(short))) {
                            crate::scenario::publish_handle(&format!("dialog:{row}:{}", app.tr(legacy)), at);
                        }
                    }
                    ui_kit::row(ui, app.tr("prefs.browser"), |ui| {
                        ui.add_enabled_ui(app.ui.layout == crate::shell::LayoutMode::Floating, |ui| {
                            let mut selected = usize::from(app.preferences.browser_float);
                            if ui_kit::segmented(ui, &mut selected, &[app.tr("prefs.opaque"), app.tr("prefs.translucent")], Default::default()) {
                                app.preferences.browser_float = selected == 1;
                            }
                        });
                    });
                    ui_kit::text(ui, app.tr("prefs.layout_note"), ui_kit::Text::Hint("layout"));
                    ui_kit::LayoutPreview {
                        compact: app.ui.layout == crate::shell::LayoutMode::Docked,
                        labels: app.preferences.ribbon_labels,
                        history: if app.preferences.timeline_left { if app.preferences.timeline_under_browser { 2 } else { 1 } } else { 0 },
                        translucent: app.preferences.browser_float,
                    }
                })
            },
            ui_kit::layout_preview,
        );
    });
    ui_kit::settings_section(ui, "scale", app.tr("prefs.scale_text"), |ui| {
        ui_kit::settings_pair(
            ui,
            |ui| {
                let response = ui_kit::slider(
                    ui,
                    &mut app.preferences.ui_scale,
                    0.5..=2.5,
                    ui_kit::Slider { step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
                );
                crate::scenario::publish_target("prefs:scale", response.rect);
                ui.horizontal_wrapped(|ui| {
                    for value in [0.75, 1.0, 1.1, 1.25, 1.5, 2.0] {
                        if ui_kit::button(
                            ui,
                            format!("{:.0}%", value * 100.0),
                            if (app.preferences.ui_scale - value).abs() < 0.001 { ui_kit::Button::SmallSelected } else { ui_kit::Button::Small },
                        )
                        .clicked()
                        {
                            app.preferences.ui_scale = value;
                        }
                    }
                });
                ui_kit::text(ui, app.tr("prefs.scale_note"), ui_kit::Text::Hint("scale"));
                app.preferences.ui_scale
            },
            ui_kit::scale_specimen,
        );
        ui_kit::divider(ui, "scale_font");
        if !cfg!(target_arch = "wasm32") {
            ui_kit::settings_font_row(ui, crate::i18n::tr("prefs.font"), |ui, part| {
                if part == 0 {
                    ui_font_row(app, ui);
                    return;
                }
                if ui_kit::button(ui, app.tr("prefs.choose_font"), ui_kit::Button::Normal).clicked() {
                    app.pick_open_for(&["ttf", "otf", "ttc"], |app, path| {
                        if let Some(path) = path {
                            if crate::fonts::ui_font(&path).is_some() {
                                app.preferences.ui_font = path;
                            } else {
                                app.set_status(crate::i18n::tr("prefs.font_unreadable"), true);
                            }
                        }
                    });
                }
                if ui_kit::button(ui, app.tr_label("Reset"), ui_kit::Button::Ghost).clicked() {
                    app.preferences.ui_font.clear();
                }
            });
            ui_kit::font_specimen(ui, app.tr("prefs.font_sample"));
            ui_kit::text(ui, app.tr("prefs.font_fallback"), ui_kit::Text::Hint("font_fallback"));
        }
    });
    ui_kit::settings_section(ui, "colour_vision", app.tr("ui.colour_vision"), |ui| {
        let mode = app.preferences.color_vision;
        ui.horizontal_wrapped(|ui| {
            let modes = crate::theme::ColorVision::ALL;
            let labels: Vec<_> = modes.iter().map(|m| crate::i18n::tr(m.label_id())).collect();
            let mut selected = modes.iter().position(|m| *m == mode).unwrap_or(0);
            if ui_kit::settings_segments(ui, app.tr("ui.colour_vision_mode"), &mut selected, &labels)
                && let Some(mode) = modes.get(selected)
            {
                app.preferences.color_vision = *mode;
            }
            if let Some(rect) = crate::scenario::rect_of(&format!("dialog:segments:{}", app.tr("ui.colour_vision_mode"))) {
                crate::scenario::publish_target("prefs:color_vision", rect);
            }
            ui_kit::status_palette_sample(ui, app.preferences.color_vision);
        });
        if app.preferences.color_vision != mode {
            ui.ctx().request_repaint();
        }
        ui_kit::text(ui, app.tr("ui.colour_vision_note"), ui_kit::Text::Hint("colour_vision"));
    });
}
