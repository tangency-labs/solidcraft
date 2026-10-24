//! Interface settings composition, using shared controls and previews.
use super::*;

/// Origin plane names: each choice shows the three names it gives (`XY / XZ / YZ`), with a note
/// that only the names shown change.
fn origin_plane_names(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_section(ui, "origin_plane_names", app.tr("prefs.origin_plane_names"), |ui| {
        let choices: Vec<String> =
            OriginPlaneNames::ALL.iter().map(|names| OriginPlaneNames::PLANES.map(|plane| names.label(plane)).join(" / ")).collect();
        let options: Vec<&str> = choices.iter().map(String::as_str).collect();
        let mut selected = OriginPlaneNames::ALL.iter().position(|names| *names == app.preferences.origin_plane_names).unwrap_or(0);
        if ui_kit::settings_segments(ui, app.tr("prefs.origin_plane_names"), &mut selected, &options)
            && let Some(names) = OriginPlaneNames::ALL.get(selected)
        {
            app.preferences.origin_plane_names = *names;
        }
        // Handles that don't depend on the language, by the choice's saved value.
        for (choice, key) in choices.iter().zip(["xyz", "top_front_right"]) {
            if let Some(at) = crate::scenario::handle_at(&format!("dialog:{}:{choice}", app.tr("prefs.origin_plane_names"))) {
                crate::scenario::publish_handle(&format!("prefs:origin_plane_names:{key}"), at);
            }
        }
        let top = OriginPlaneNames::TopFrontRight;
        let note =
            crate::i18n::format("prefs.origin_plane_names_note", &[("top", top.label("XY")), ("front", top.label("XZ")), ("right", top.label("YZ"))]);
        ui_kit::text(ui, &note, ui_kit::Text::Hint("origin_plane_names"));
    });
}

pub(super) fn interface(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_heading(ui, app.tr("prefs.interface"), app.tr("prefs.interface_note"));
    ui_kit::settings_section(ui, "theme", app.tr("ui.theme"), |ui| {
        theme_row(app, ui);
        ui_kit::tab_accent_strip(ui);
        // The 3D view in the theme's colours, or the Light or Dark theme's (#4146).
        ui_kit::settings_form(ui, t.form_label_width + t.space_16 + t.space_6, |ui| {
            let label = app.tr("prefs.viewport");
            ui_kit::row(ui, label, |ui| {
                let views = crate::theme::ViewportTheme::ALL;
                let labels: Vec<_> = views.iter().map(|v| crate::i18n::tr(v.label_id())).collect();
                let mut selected = views.iter().position(|v| *v == app.preferences.viewport_theme).unwrap_or(0);
                if ui_kit::settings_segments(ui, label, &mut selected, &labels)
                    && let Some(view) = views.get(selected)
                {
                    app.preferences.viewport_theme = *view;
                }
            });
            // The logo in the theme's own logo colours or in its accent.
            let label = app.tr("prefs.logo");
            ui_kit::row(ui, label, |ui| {
                let labels = [crate::i18n::tr("prefs.logo_theme"), crate::i18n::tr("prefs.logo_accent")];
                let mut selected = usize::from(app.preferences.logo_accent);
                if ui_kit::settings_segments(ui, label, &mut selected, &labels) {
                    app.preferences.logo_accent = selected == 1;
                }
            });
            // A theme file another tool writes, reloaded when it changes (#7573).
            if FOLLOW_AVAILABLE {
                follow_row(app, ui);
            }
        });
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
                    // The SKETCH tab outside a sketch too (#7904).
                    ui_kit::row(ui, "", |ui| {
                        let check = setting::sketch_tab_always(app, ui).on_hover_text(app.tr("prefs.sketch_tab_always_note"));
                        crate::scenario::publish_target("prefs:sketch_tab_always", check.rect);
                    });
                    // Where every feature dialog puts its OK and Cancel (#6708).
                    ui_kit::row(ui, app.tr("prefs.dialog_buttons"), |ui| {
                        let placements = [crate::ui_kit::DialogButtons::Bottom, crate::ui_kit::DialogButtons::Top];
                        let mut selected = placements.iter().position(|p| *p == app.preferences.dialog_buttons).unwrap_or(0);
                        if ui_kit::segmented(ui, &mut selected, &[app.tr("prefs.bottom"), app.tr("prefs.top")], Default::default())
                            && let Some(placement) = placements.get(selected)
                        {
                            app.preferences.dialog_buttons = *placement;
                        }
                    });
                    // Handles that don't depend on the language, by the saved value.
                    let row = app.tr("prefs.dialog_buttons");
                    for (label, key) in [("prefs.bottom", "bottom"), ("prefs.top", "top")] {
                        if let Some(at) = crate::scenario::handle_at(&format!("dialog:{row}:{}", app.tr(label))) {
                            crate::scenario::publish_handle(&format!("prefs:dialog_buttons:{key}"), at);
                        }
                    }
                    ui_kit::row(ui, app.tr("prefs.ribbon"), |ui| {
                        let mut selected = usize::from(app.preferences.ribbon_compact);
                        if ui_kit::segmented(
                            ui,
                            &mut selected,
                            &[app.tr("prefs.ribbon_standard"), app.tr("prefs.ribbon_compact")],
                            Default::default(),
                        ) {
                            app.preferences.ribbon_compact = selected == 1;
                        }
                    });
                    ui_kit::row(ui, app.tr("prefs.tooltips"), |ui| {
                        ui_kit::checkbox(ui, crate::i18n::tr("prefs.tooltip_descriptions"), &mut app.preferences.ribbon_help);
                    });
                    ui_kit::row(ui, "", |ui| {
                        let customize = ui_kit::button(ui, app.tr("ui.customizeToolbar"), ui_kit::Button::Normal);
                        crate::scenario::publish_target("prefs:customize_toolbar", customize.rect);
                        if customize.clicked() {
                            app.toolbar_custom.customizing = true;
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
                    // Only the History at the left can list a sketch under its feature (#6489).
                    ui_kit::row(ui, app.tr("prefs.history_sketches"), |ui| {
                        ui.add_enabled_ui(app.preferences.timeline_left, |ui| {
                            let mut selected = usize::from(app.preferences.history_sketches == HistorySketches::UnderFeature);
                            if ui_kit::segmented(
                                ui,
                                &mut selected,
                                &[app.tr("prefs.history_sketches_in_order"), app.tr("prefs.history_sketches_under_feature")],
                                ui_kit::Segments { proportional: true, compact: true, ..Default::default() },
                            ) {
                                app.preferences.history_sketches =
                                    if selected == 1 { HistorySketches::UnderFeature } else { HistorySketches::InOrder };
                            }
                        });
                    });
                    // Only the history on the left or under the browser has room for these rows.
                    ui_kit::row(ui, "", |ui| {
                        ui.add_enabled_ui(app.preferences.timeline_left, |ui| {
                            let check = ui_kit::checkbox(ui, app.tr("prefs.history_origin_bodies"), &mut app.preferences.timeline_origin_bodies)
                                .on_hover_text(app.tr("prefs.history_origin_bodies_note"))
                                .on_disabled_hover_text(app.tr("prefs.history_origin_bodies_note"));
                            crate::scenario::publish_target("prefs:history_origin_bodies", check.rect);
                        });
                    });
                    ui_kit::row(ui, app.tr("prefs.browser"), |ui| {
                        ui.add_enabled_ui(app.ui.layout == crate::shell::LayoutMode::Floating, |ui| {
                            let mut selected = usize::from(app.preferences.browser_float);
                            if ui_kit::segmented(ui, &mut selected, &[app.tr("prefs.opaque"), app.tr("prefs.translucent")], Default::default()) {
                                app.preferences.browser_float = selected == 1;
                            }
                        });
                    });
                    // Where command dialogs open (#6707).
                    ui_kit::row(ui, app.tr("prefs.dialogs"), |ui| {
                        use crate::dialog_column::DialogPlace;
                        let mut selected = usize::from(app.preferences.dialog_place == DialogPlace::LeftColumn);
                        let options = [app.tr("prefs.dialogs_edge"), app.tr("prefs.dialogs_left_column")];
                        let segments = ui.scope(|ui| ui_kit::segmented(ui, &mut selected, &options, Default::default()));
                        segments.response.on_hover_text(app.tr("prefs.dialogs_note"));
                        if segments.inner {
                            app.preferences.dialog_place = if selected == 1 { DialogPlace::LeftColumn } else { DialogPlace::Edge };
                        }
                    });
                    // Where a dialog's and a sketch's OK and Cancel also sit (#6709).
                    ui_kit::row(ui, "", |ui| {
                        let check = setting::view_confirm_corner(app, ui).on_hover_text(app.tr("prefs.view_confirm_corner_note"));
                        crate::scenario::publish_target("prefs:view_confirm_corner", check.rect);
                    });
                    ui_kit::row(ui, app.tr("prefs.bodies_folders"), |ui| {
                        let modes = BodiesFolders::ALL;
                        let labels: Vec<_> = modes.iter().map(|m| app.tr(m.label_id())).collect();
                        let mut selected = modes.iter().position(|m| *m == app.preferences.browser_bodies_folders).unwrap_or(0);
                        if ui_kit::segmented(ui, &mut selected, &labels, ui_kit::Segments { proportional: true, compact: true, ..Default::default() })
                            && let Some(mode) = modes.get(selected)
                        {
                            app.preferences.browser_bodies_folders = *mode;
                        }
                    });
                    // What is being edited, the sketch's state and the units, along the bottom (#7243).
                    ui_kit::row(ui, "", |ui| {
                        let check = setting::status_bar(app, ui).on_hover_text(app.tr("prefs.status_bar_note"));
                        crate::scenario::publish_target("prefs:status_bar", check.rect);
                    });
                    ui_kit::text(ui, app.tr("prefs.layout_note"), ui_kit::Text::Hint("layout"));
                    ui_kit::text(ui, app.tr("prefs.history_sketches_note"), ui_kit::Text::Hint("history_sketches"));
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
    origin_plane_names(app, ui);
    super::navbar::show(app, ui);
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
        // What the modes recolour, in the mode being chosen (#7152): the status chips above keep
        // their colours in every mode.
        ui_kit::colour_vision_sample(ui, &crate::theme::Tokens::get_in(app.preferences.color_vision));
        if app.preferences.color_vision != mode {
            ui.ctx().request_repaint();
        }
        ui_kit::text(ui, app.tr("ui.colour_vision_note"), ui_kit::Text::Hint("colour_vision"));
    });
}
