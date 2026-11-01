//! Appearance and Layout settings composition, using shared controls and previews.
use super::*;

pub(super) fn appearance(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading_with_tag(
            ui,
            app.tr("prefs.structure.appearance"),
            app.tr("prefs.structure.appearance_note"),
            Some(crate::settings_profiles::tag),
        );
        let count = crate::i18n::format("prefs.appearance.theme_count", &[("count", &theme_choices(app).len().to_string())]);
        ui_kit::settings_section_count(ui, "theme", app.tr("ui.theme"), Some(&count), |ui| {
            theme_row(app, ui);
            theme_options_row(app, ui);
        });
        ui_kit::settings_section(ui, "viewport_logo", app.tr("prefs.appearance.viewport_logo"), |ui| {
            // The 3D view in the theme's colours, or the Light or Dark theme's (#4146).
            ui_kit::settings_form(ui, t.space_24 * 6.0 + t.space_6, |ui| {
                // The view's background: the theme's gradient, or one colour of the user's (#7688).
                let label = app.tr("prefs.viewport_background");
                let labels = [crate::i18n::tr("prefs.viewport_background_theme"), crate::i18n::tr("prefs.viewport_background_colour")];
                let segments_width = (labels.iter().map(|label| ui_kit::ContentWidth::name(ui, label)).fold(0.0, f32::max) + t.space_16) * 2.0;
                let label_width = (t.space_24 * 6.0 + t.space_6).min(ui.available_width() * 0.6);
                let available = (ui.available_width() - label_width - t.space_14).max(0.0);
                let narrow = available < segments_width + t.control_height + t.space_6 + t.space_24 * 4.0 + ui.spacing().item_spacing.x * 2.0;
                let colour_controls = |ui: &mut egui::Ui, app: &mut SolveApp| {
                    let enabled = app.preferences.viewport_background.is_some();
                    let mut rgb = app.preferences.viewport_background.unwrap_or([255, 255, 255]);
                    ui.add_enabled_ui(enabled, |ui| {
                        let response = ui_kit::color_field(ui, &mut rgb);
                        crate::scenario::publish_target("prefs:viewport_background_picker", response.rect);
                        if response.changed() {
                            app.preferences.viewport_background = Some(rgb);
                        }
                    });
                    let saved = app.preferences.viewport_background;
                    let key = ui.make_persistent_id("appearance_background_hex");
                    let (last, mut hex) = ui
                        .data(|data| data.get_temp::<(Option<[u8; 3]>, String)>(key))
                        .unwrap_or_else(|| (saved, crate::dialogs_appearance::hex(saved.unwrap_or([255, 255, 255]))));
                    if last != saved {
                        hex = crate::dialogs_appearance::hex(saved.unwrap_or([255, 255, 255]));
                    }
                    ui.add_enabled_ui(saved.is_some(), |ui| {
                        ui_kit::settings_control_width(ui, t.space_24 * 4.0, |ui| {
                            let response = ui_kit::cell_editor(ui, key.with("edit"), &mut hex, app.tr("color.hex_hint"), false);
                            crate::scenario::publish_target("prefs:viewport_background_hex", response.rect);
                            if response.changed()
                                && let Some(colour) = crate::dialogs_appearance::parse_color(&json!(hex))
                            {
                                app.preferences.viewport_background = Some(colour);
                            }
                        });
                    });
                    ui.data_mut(|data| data.insert_temp(key, (app.preferences.viewport_background, hex)));
                };
                let shown = ui_kit::row(ui, label, |ui| {
                    let mut selected = usize::from(app.preferences.viewport_background.is_some());
                    if ui_kit::settings_segments(ui, label, &mut selected, &labels) {
                        app.preferences.viewport_background = (selected == 1).then_some([255, 255, 255]);
                    }
                    if !narrow {
                        colour_controls(ui, app);
                    }
                    ui.is_visible()
                });
                if narrow && shown {
                    ui_kit::below_row_control(ui, label, ui.available_width(), |ui| {
                        ui.horizontal_wrapped(|ui| colour_controls(ui, app));
                    });
                }
                let label = app.tr("prefs.appearance.viewport_colours");
                ui_kit::row(ui, label, |ui| {
                    let views = crate::theme::ViewportTheme::ALL;
                    let labels = [app.tr("prefs.appearance.theme_own"), app.tr("ui.light"), app.tr("ui.dark")];
                    let mut selected = views.iter().position(|v| *v == app.preferences.viewport_theme).unwrap_or(0);
                    if ui_kit::settings_segments(ui, label, &mut selected, &labels)
                        && let Some(view) = views.get(selected)
                    {
                        app.preferences.viewport_theme = *view;
                    }
                    for (view, caption) in views.into_iter().zip(labels) {
                        if let Some(at) = crate::scenario::handle_at(&format!("dialog:{label}:{caption}")) {
                            let id = view.id();
                            crate::scenario::publish_handle(&format!("prefs:viewport_theme:{id}"), at);
                            crate::scenario::publish_handle(&format!("dialog:{}:{}", app.tr("prefs.viewport"), app.tr(view.label_id())), at);
                        }
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
                follow_row(app, ui);
            });
        });
        ui_kit::settings_section(ui, "scale", app.tr("prefs.scale_text"), |ui| {
            ui_kit::settings_pair(
                ui,
                |ui| {
                    ui_kit::settings_form(ui, 0.0, |ui| {
                        ui_kit::row(ui, app.tr("prefs.appearance.interface_scale"), |ui| {
                            let response = ui_kit::named_control(ui, app.tr("prefs.appearance.interface_scale"), |ui| {
                                ui_kit::slider(
                                    ui,
                                    &mut app.preferences.ui_scale,
                                    0.5..=2.5,
                                    ui_kit::Slider { step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
                                )
                            })
                            .on_hover_text(app.tr("prefs.scale_note"));
                            crate::scenario::publish_target("prefs:scale", response.rect);
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        let locale = crate::i18n::Locale::for_language(&app.preferences.language);
                        for value in [0.75, 1.0, 1.1, 1.25, 1.5, 2.0] {
                            let percent = format!("{:.0}", value * 100.0);
                            let chip = ui_kit::button(
                                ui,
                                locale.format("prefs.structure.scale_percent", &[("percent", &percent)]),
                                if (app.preferences.ui_scale - value).abs() < 0.001 { ui_kit::Button::SmallSelected } else { ui_kit::Button::Small },
                            );
                            crate::scenario::publish_target(&format!("prefs:scale_preset:{percent}"), chip.rect);
                            if chip.clicked() {
                                app.preferences.ui_scale = value;
                            }
                        }
                    });
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
                ui.scope(|ui| ui_kit::font_specimen(ui, app.tr("prefs.font_sample"))).response.on_hover_text(app.tr("prefs.font_fallback"));
            }
        });
        ui_kit::settings_section(ui, "colour_vision", app.tr("ui.colour_vision"), |ui| {
            let mode = app.preferences.color_vision;
            ui_kit::settings_form(ui, t.form_label_width, |ui| {
                ui_kit::settings_row_help(ui, app.tr("ui.colour_vision_mode"), app.tr("prefs.appearance.colour_vision_help"), |ui| {
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
                    for mode in crate::theme::ColorVision::ALL {
                        if let Some(at) =
                            crate::scenario::handle_at(&format!("dialog:{}:{}", app.tr("ui.colour_vision_mode"), app.tr(mode.label_id())))
                        {
                            crate::scenario::publish_handle(&format!("prefs:color_vision:{}", mode.id()), at);
                        }
                    }
                });
            });
            ui_kit::status_palette_sample(ui, app.preferences.color_vision);
            // What the modes recolour, in the mode being chosen (#7152): the status chips above keep
            // their colours in every mode.
            ui_kit::colour_vision_sample(ui, &crate::theme::Tokens::get_in(app.preferences.color_vision));
            if app.preferences.color_vision != mode {
                ui.ctx().request_repaint();
            }
        });
    });
}

pub(super) fn layout(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_heading_with_tag(
        ui,
        app.tr("prefs.structure.layout"),
        app.tr("prefs.structure.layout_note"),
        Some(crate::settings_profiles::tag),
    );
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
                    ui_kit::row(ui, "", |ui| {
                        let check = setting::persistent_ribbon_pressed(app, ui).on_hover_text(app.tr("prefs.persistent_ribbon_pressed_help"));
                        crate::scenario::publish_target("prefs:persistent_ribbon_pressed", check.rect);
                    });
                    // The SKETCH tab outside a sketch too (#7904).
                    ui_kit::row(ui, "", |ui| {
                        let check = setting::sketch_tab_always(app, ui).on_hover_text(app.tr("prefs.sketch_tab_always_note"));
                        crate::scenario::publish_target("prefs:sketch_tab_always", check.rect);
                    });
                    // Each design tab keeps its dialog, tool and toolbar tab across a switch (#9360).
                    ui_kit::row(ui, "", |ui| {
                        let check = setting::keep_work_on_tab_switch(app, ui).on_hover_text(app.tr("prefs.keep_work_on_tab_switch_note"));
                        crate::scenario::publish_target("prefs:keep_work_on_tab_switch", check.rect);
                    });
                    // Search results point out their place in the ribbon (#7287).
                    ui_kit::row(ui, "", |ui| {
                        let check = setting::ribbon_reveal(app, ui).on_hover_text(app.tr("prefs.ribbon_reveal_note"));
                        crate::scenario::publish_target("prefs:ribbon_reveal", check.rect);
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
                    // Where notices and offers stack (#7893).
                    ui_kit::row(ui, app.tr("prefs.toast_place"), |ui| {
                        let places = [crate::viewport::ToastPlace::Corner, crate::viewport::ToastPlace::Centre];
                        let mut selected = places.iter().position(|p| *p == app.preferences.toast_place).unwrap_or(0);
                        if ui_kit::segmented(ui, &mut selected, &[app.tr("prefs.bottom_right"), app.tr("prefs.bottom_centre")], Default::default())
                            && let Some(place) = places.get(selected)
                        {
                            app.preferences.toast_place = *place;
                        }
                    });
                    let row = app.tr("prefs.toast_place");
                    for (label, key) in [("prefs.bottom_right", "corner"), ("prefs.bottom_centre", "centre")] {
                        if let Some(at) = crate::scenario::handle_at(&format!("dialog:{row}:{}", app.tr(label))) {
                            crate::scenario::publish_handle(&format!("prefs:toast_place:{key}"), at);
                        }
                    }
                    // The ribbon's panel drop-downs: a list, or a grid of large icons (#4739).
                    ui_kit::row(ui, app.tr("prefs.menus"), |ui| {
                        let mut selected = usize::from(app.preferences.ribbon_icon_grid);
                        if ui_kit::segmented(ui, &mut selected, &[app.tr("prefs.menu_list"), app.tr("prefs.menu_grid")], Default::default()) {
                            app.preferences.ribbon_icon_grid = selected == 1;
                        }
                    });
                    setting::ribbon_menus_skip_pinned(app, ui);
                    ui_kit::row(ui, app.tr("prefs.history"), |ui| {
                        let mut selected = if app.preferences.history_in_browser {
                            3
                        } else if app.preferences.timeline_left {
                            if app.preferences.timeline_under_browser { 2 } else { 1 }
                        } else {
                            0
                        };
                        if ui_kit::segmented(
                            ui,
                            &mut selected,
                            &[
                                crate::i18n::tr("prefs.bottom"),
                                crate::i18n::tr("prefs.left"),
                                crate::i18n::tr("prefs.under_browser"),
                                crate::i18n::tr("prefs.in_browser"),
                            ],
                            ui_kit::Segments { proportional: true, compact: true, ..Default::default() },
                        ) {
                            let dock = match selected {
                                1 => "left",
                                2 => "under_browser",
                                3 => "browser",
                                _ => "bottom",
                            };
                            let _ = app.run("view.history_layout", json!({ "dock": dock }));
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
                    // The History at the left and the one in the browser can list a sketch under its
                    // feature (#6489); the timeline at the bottom can't.
                    ui_kit::row(ui, app.tr("prefs.history_sketches"), |ui| {
                        ui.add_enabled_ui(app.preferences.timeline_left || app.preferences.history_in_browser, |ui| {
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
                    // Play History between Previous and Next, for people who want it there (#3688).
                    ui_kit::row(ui, "", |ui| {
                        let check = setting::history_play_button(app, ui).on_hover_text(app.tr("prefs.history_play_button_note"));
                        crate::scenario::publish_target("prefs:history_play_button", check.rect);
                    });
                    ui_kit::row(ui, app.tr("prefs.browser"), |ui| {
                        ui.add_enabled_ui(app.ui.layout == crate::shell::LayoutMode::Floating, |ui| {
                            let mut selected = usize::from(app.preferences.browser_float);
                            if ui_kit::segmented(ui, &mut selected, &[app.tr("prefs.opaque"), app.tr("prefs.translucent")], Default::default()) {
                                app.preferences.browser_float = selected == 1;
                            }
                        });
                    });
                    // Out of sight until the pointer comes to it (#8692); the browser sharing its
                    // column with History keeps its place.
                    ui_kit::row(ui, "", |ui| {
                        use crate::browser::BrowserShow;
                        ui.add_enabled_ui(!crate::timeline_left::stacked(app), |ui| {
                            let mut selected = usize::from(app.preferences.browser_show == BrowserShow::Hover);
                            let options = [app.tr("prefs.browser_shown"), app.tr("prefs.browser_reveal_on_hover")];
                            let segments = ui.scope(|ui| ui_kit::segmented(ui, &mut selected, &options, Default::default()));
                            segments.response.on_hover_text(app.tr("prefs.browser_show_note"));
                            if segments.inner {
                                app.preferences.browser_show = if selected == 1 { BrowserShow::Hover } else { BrowserShow::Shown };
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
                    // The eye beside the fold arrow, where it was before #8718.
                    ui_kit::row(ui, "", |ui| {
                        let check = setting::browser_eye_left(app, ui).on_hover_text(app.tr("prefs.browser_eye_left_note"));
                        crate::scenario::publish_target("prefs:browser_eye_left", check.rect);
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
                        history: if app.preferences.history_in_browser {
                            3
                        } else if app.preferences.timeline_left {
                            if app.preferences.timeline_under_browser { 2 } else { 1 }
                        } else {
                            0
                        },
                        translucent: app.preferences.browser_float,
                    }
                })
            },
            ui_kit::layout_preview,
        );
    });
    super::navbar::show(app, ui);
    // The same choice as the toolbar's own menu (the plus after the tabs, or right-click a tab).
    ui_kit::settings_section(ui, "toolbar_tabs", app.tr("prefs.toolbar_tabs"), |ui| {
        for tab in crate::workspace::choosable_tabs() {
            let mut on = crate::workspace::tab_turned_on(app, tab);
            let response = ui_kit::checkbox(ui, &crate::toolbar::ribbon_label(app, tab), &mut on);
            crate::scenario::publish_target(&format!("prefs:toolbar_tab:{tab}"), response.rect);
            if response.changed() {
                crate::workspace::set_tab_shown(app, tab, on);
            }
        }
    });
}
