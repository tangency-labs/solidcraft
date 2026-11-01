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
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading_with_tag(
            ui,
            app.tr("prefs.structure.layout"),
            app.tr("prefs.structure.layout_note"),
            Some(crate::settings_profiles::tag),
        );
        // Retain the section key for existing reveal/open callers.
        ui_kit::settings_section(ui, "layout", app.tr("prefs.layout_page.window"), |ui| {
            ui_kit::settings_pair(
                ui,
                |ui| {
                    ui_kit::settings_form(ui, t.form_label_width + t.space_16, |ui| {
                        ui_kit::row(ui, app.tr("prefs.workspace"), |ui| {
                            let mut selected = usize::from(app.ui.layout == crate::shell::LayoutMode::Docked);
                            if ui_kit::segmented(
                                ui,
                                &mut selected,
                                &[crate::i18n::tr("ui.layout_floating"), crate::i18n::tr("ui.layout_compact")],
                                ui_kit::Segments { intrinsic: true, ..Default::default() },
                            ) {
                                let _ = app.run("view.layout", json!({"mode": if selected == 1 { "compact" } else { "floating" }}));
                            }
                        });
                        ui_kit::row(ui, app.tr("prefs.toolbar"), |ui| {
                            let mut selected = usize::from(!app.preferences.ribbon_labels);
                            if ui_kit::segmented(
                                ui,
                                &mut selected,
                                &[app.tr("prefs.labels"), app.tr("prefs.icons_only")],
                                ui_kit::Segments { intrinsic: true, ..Default::default() },
                            ) {
                                app.preferences.ribbon_labels = selected == 0;
                            }
                        });
                        ui_kit::row(ui, app.tr("prefs.menus"), |ui| {
                            let mut selected = usize::from(app.preferences.ribbon_icon_grid);
                            if ui_kit::segmented(
                                ui,
                                &mut selected,
                                &[app.tr("prefs.menu_list"), app.tr("prefs.menu_grid")],
                                ui_kit::Segments { intrinsic: true, ..Default::default() },
                            ) {
                                app.preferences.ribbon_icon_grid = selected == 1;
                            }
                        });
                        ui_kit::row(ui, app.tr("prefs.toast_place"), |ui| {
                            let places = [crate::viewport::ToastPlace::Corner, crate::viewport::ToastPlace::Centre];
                            let mut selected = places.iter().position(|p| *p == app.preferences.toast_place).unwrap_or(0);
                            if ui_kit::segmented(
                                ui,
                                &mut selected,
                                &[app.tr("prefs.bottom_right"), app.tr("prefs.bottom_centre")],
                                ui_kit::Segments { intrinsic: true, ..Default::default() },
                            ) && let Some(place) = places.get(selected)
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
                        layout_check(ui, app.tr("prefs.status_bar"), app.tr("prefs.status_bar_note"), &mut app.preferences.status_bar, "status_bar");
                    });
                    ui_kit::LayoutPreview {
                        compact: app.ui.layout == crate::shell::LayoutMode::Docked,
                        labels: app.preferences.ribbon_labels,
                        history: history_choice(app),
                        translucent: app.preferences.browser_float,
                        dialog_left: app.preferences.dialog_place == crate::dialog_column::DialogPlace::LeftColumn,
                    }
                },
                ui_kit::layout_preview,
            );
            layout_check(
                ui,
                app.tr("prefs.keep_work_on_tab_switch"),
                app.tr("prefs.keep_work_on_tab_switch_note"),
                &mut app.preferences.keep_work_on_tab_switch,
                "keep_work_on_tab_switch",
            );
        });
        if let Some(rect) = crate::scenario::rect_of(&format!("dialog:section:{}", app.tr("prefs.layout_page.window"))) {
            crate::scenario::publish_target(&format!("dialog:section:{}", app.tr("prefs.layout")), rect);
        }
        ui_kit::settings_section(ui, "panels", app.tr("prefs.layout_page.panels"), |ui| {
            let options = [
                ui_kit::PictureChoice { key: "bottom", label: app.tr("prefs.bottom"), figure: ui_kit::paint_history_bottom },
                ui_kit::PictureChoice { key: "left", label: app.tr("prefs.left"), figure: ui_kit::paint_history_left },
                ui_kit::PictureChoice { key: "under_browser", label: app.tr("prefs.under_browser"), figure: ui_kit::paint_history_under_browser },
                ui_kit::PictureChoice { key: "browser", label: app.tr("prefs.in_browser"), figure: ui_kit::paint_history_in_browser },
            ];
            if let Some(selected) = ui_kit::settings_picture_choices(ui, "history", app.tr("prefs.history"), &options, history_choice(app), true)
                && let Some(option) = options.get(selected)
            {
                let _ = app.run("view.history_layout", json!({"dock":option.key}));
            }
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
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = t.space_16;
                let cards_width = (t.space_24 * 10.0 + t.space_2).min(ui.max_rect().width());
                ui.allocate_ui_with_layout(egui::vec2(cards_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    let options = [
                        ui_kit::PictureChoice { key: "column", label: app.tr("prefs.layout_page.column"), figure: ui_kit::paint_browser_column },
                        ui_kit::PictureChoice { key: "card", label: app.tr("prefs.layout_page.card"), figure: ui_kit::paint_browser_card },
                    ];
                    if let Some(selected) = ui_kit::settings_picture_choices(
                        ui,
                        "browser",
                        app.tr("prefs.browser"),
                        &options,
                        usize::from(app.preferences.browser_float),
                        app.ui.layout == crate::shell::LayoutMode::Floating,
                    ) {
                        app.preferences.browser_float = selected == 1;
                    }
                    for (key, legacy) in [("column", "prefs.opaque"), ("card", "prefs.translucent")] {
                        if let Some(at) = crate::scenario::handle_at(&format!("prefs:browser:{key}")) {
                            crate::scenario::publish_handle(&format!("dialog:{}:{}", app.tr("prefs.browser"), app.tr(legacy)), at);
                        }
                    }
                });
                let labels = [app.tr("prefs.browser_shown"), app.tr("prefs.layout_page.on_hover")];
                let width = labels.iter().map(|label| ui_kit::ContentWidth::name(ui, label)).fold(0.0, f32::max) * 2.0 + t.space_16 * 3.0 + t.space_6;
                ui.allocate_ui_with_layout(
                    egui::vec2(width.min(ui.max_rect().width()), t.control_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        browser_visibility(app, ui);
                    },
                );
            });
            ui_kit::settings_form(ui, t.form_label_width + t.space_24 * 3.0, |ui| {
                ui_kit::settings_row_help(ui, app.tr("prefs.history_sketches"), app.tr("prefs.history_sketches_note"), |ui| {
                    ui.add_enabled_ui(app.preferences.timeline_left || app.preferences.history_in_browser, |ui| {
                        let mut selected = usize::from(app.preferences.history_sketches == HistorySketches::UnderFeature);
                        if ui_kit::segmented(
                            ui,
                            &mut selected,
                            &[app.tr("prefs.history_sketches_in_order"), app.tr("prefs.history_sketches_under_feature")],
                            ui_kit::Segments { intrinsic: true, ..Default::default() },
                        ) {
                            app.preferences.history_sketches = if selected == 1 { HistorySketches::UnderFeature } else { HistorySketches::InOrder };
                        }
                    });
                });
                for (key, label) in [("in_order", "prefs.history_sketches_in_order"), ("under_feature", "prefs.history_sketches_under_feature")] {
                    if let Some(rect) = crate::scenario::rect_of(&format!("dialog:{}:{}", app.tr("prefs.history_sketches"), app.tr(label))) {
                        crate::scenario::publish_target(&format!("prefs:history_sketches:{key}"), rect);
                        crate::scenario::publish_target(&format!("dialog:Sketches:{}", app.tr(label)), rect);
                    }
                }
                ui_kit::row(ui, app.tr("prefs.bodies_folders"), |ui| {
                    let modes = BodiesFolders::ALL;
                    let labels: Vec<_> = modes.iter().map(|m| app.tr(m.label_id())).collect();
                    let mut selected = modes.iter().position(|m| *m == app.preferences.browser_bodies_folders).unwrap_or(0);
                    if ui_kit::segmented(ui, &mut selected, &labels, ui_kit::Segments { intrinsic: true, ..Default::default() })
                        && let Some(mode) = modes.get(selected)
                    {
                        app.preferences.browser_bodies_folders = *mode;
                    }
                    if let Some(rect) =
                        crate::scenario::rect_of(&format!("dialog:{}:{}", app.tr("prefs.bodies_folders"), app.tr("prefs.bodies_folders_several")))
                    {
                        crate::scenario::publish_target(&format!("dialog:{}:Several bodies only", app.tr("prefs.bodies_folders")), rect);
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_16, t.space_6);
                layout_check(
                    ui,
                    app.tr("prefs.layout_page.play_history"),
                    app.tr("prefs.history_play_button_note"),
                    &mut app.preferences.history_play_button,
                    "history_play_button",
                );
                ui.add_enabled_ui(app.preferences.timeline_left, |ui| {
                    layout_check(
                        ui,
                        app.tr("prefs.layout_page.origin_bodies"),
                        app.tr("prefs.history_origin_bodies_note"),
                        &mut app.preferences.timeline_origin_bodies,
                        "history_origin_bodies",
                    );
                });
                layout_check(
                    ui,
                    app.tr("prefs.browser_eye_left"),
                    app.tr("prefs.browser_eye_left_note"),
                    &mut app.preferences.browser_eye_left,
                    "browser_eye_left",
                );
            });
        });
        ui_kit::settings_section(ui, "dialogs", app.tr("prefs.dialogs"), |ui| {
            ui_kit::settings_form(ui, t.form_label_width + t.space_24, |ui| {
                ui_kit::settings_row_help(ui, app.tr("prefs.layout_page.open"), app.tr("prefs.dialogs_note"), |ui| {
                    use crate::dialog_column::DialogPlace;
                    let mut selected = usize::from(app.preferences.dialog_place == DialogPlace::LeftColumn);
                    let options = [app.tr("prefs.dialogs_edge"), app.tr("prefs.dialogs_left_column")];
                    let segments =
                        ui.scope(|ui| ui_kit::segmented(ui, &mut selected, &options, ui_kit::Segments { intrinsic: true, ..Default::default() }));
                    segments.response.on_hover_text(app.tr("prefs.dialogs_note"));
                    if segments.inner {
                        app.preferences.dialog_place = if selected == 1 { DialogPlace::LeftColumn } else { DialogPlace::Edge };
                    }
                });
                for option in ["prefs.dialogs_edge", "prefs.dialogs_left_column"] {
                    if let Some(at) = crate::scenario::handle_at(&format!("dialog:{}:{}", app.tr("prefs.layout_page.open"), app.tr(option))) {
                        crate::scenario::publish_handle(&format!("dialog:{}:{}", app.tr("prefs.dialogs"), app.tr(option)), at);
                    }
                }
                ui_kit::row(ui, app.tr("prefs.layout_page.ok_cancel"), |ui| {
                    let placements = [crate::ui_kit::DialogButtons::Bottom, crate::ui_kit::DialogButtons::Top];
                    let mut selected = placements.iter().position(|p| *p == app.preferences.dialog_buttons).unwrap_or(0);
                    if ui_kit::segmented(
                        ui,
                        &mut selected,
                        &[app.tr("prefs.bottom"), app.tr("prefs.top")],
                        ui_kit::Segments { intrinsic: true, ..Default::default() },
                    ) && let Some(placement) = placements.get(selected)
                    {
                        app.preferences.dialog_buttons = *placement;
                    }
                });
                // Handles that don't depend on the language, by the saved value.
                let row = app.tr("prefs.layout_page.ok_cancel");
                for (label, key) in [("prefs.bottom", "bottom"), ("prefs.top", "top")] {
                    if let Some(at) = crate::scenario::handle_at(&format!("dialog:{row}:{}", app.tr(label))) {
                        crate::scenario::publish_handle(&format!("prefs:dialog_buttons:{key}"), at);
                        crate::scenario::publish_handle(&format!("dialog:{}:{}", app.tr("prefs.dialog_buttons"), app.tr(label)), at);
                    }
                }
            });
            layout_check(
                ui,
                app.tr("prefs.layout_page.view_corner"),
                app.tr("prefs.view_confirm_corner_note"),
                &mut app.preferences.view_confirm_corner,
                "view_confirm_corner",
            );
        });
        ui_kit::settings_section(ui, "toolbar_tabs", app.tr("prefs.toolbar_tabs"), |ui| {
            ui.horizontal_wrapped(|ui| {
                for tab in crate::workspace::choosable_tabs() {
                    let label = crate::toolbar::ribbon_label(app, tab);
                    let on = crate::workspace::tab_turned_on(app, tab);
                    if ui_kit::settings_tab_chip(ui, tab, &label, crate::workspace::tab_icon(tab), on) {
                        crate::workspace::set_tab_shown(app, tab, !on);
                    }
                }
            });
            let labels = [
                app.tr("prefs.sketch_tab_always"),
                app.tr("prefs.layout_page.active_commands"),
                app.tr("prefs.ribbon_reveal"),
                app.tr("prefs.layout_page.pinned_menus"),
            ];
            let widest =
                labels.iter().map(|label| ui_kit::ContentWidth::label(ui, label)).fold(0.0, f32::max) + t.space_14 + t.space_6 * 2.0 + t.space_16;
            let columns = if ui.available_width() >= widest * 2.0 + t.space_16 { 2 } else { 1 };
            let width = (ui.available_width() - t.space_16 * (columns - 1) as f32) / columns as f32;
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = t.space_6;
                for row in (0..4).step_by(columns) {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = t.space_16;
                        for index in row..(row + columns).min(4) {
                            ui.allocate_ui_with_layout(
                                egui::vec2(width.max(0.0), t.control_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| match index {
                                    0 => {
                                        layout_check(
                                            ui,
                                            app.tr("prefs.sketch_tab_always"),
                                            app.tr("prefs.sketch_tab_always_note"),
                                            &mut app.preferences.sketch_tab_always,
                                            "sketch_tab_always",
                                        );
                                    }
                                    1 => {
                                        layout_check(
                                            ui,
                                            app.tr("prefs.layout_page.active_commands"),
                                            app.tr("prefs.persistent_ribbon_pressed_help"),
                                            &mut app.preferences.persistent_ribbon_pressed,
                                            "persistent_ribbon_pressed",
                                        );
                                    }
                                    2 => {
                                        layout_check(
                                            ui,
                                            app.tr("prefs.ribbon_reveal"),
                                            app.tr("prefs.ribbon_reveal_note"),
                                            &mut app.preferences.ribbon_reveal,
                                            "ribbon_reveal",
                                        );
                                    }
                                    3 => {
                                        layout_check(
                                            ui,
                                            app.tr("prefs.layout_page.pinned_menus"),
                                            app.tr("ui.ribbon_menus_skip_pinned"),
                                            &mut app.preferences.ribbon_menus_skip_pinned,
                                            "ribbon_menus_skip_pinned",
                                        );
                                    }
                                    _ => {}
                                },
                            );
                        }
                    });
                }
            });
        });
        super::navbar::show(app, ui);
    });
}

fn history_choice(app: &SolveApp) -> usize {
    if app.preferences.history_in_browser {
        3
    } else if app.preferences.timeline_left {
        if app.preferences.timeline_under_browser { 2 } else { 1 }
    } else {
        0
    }
}

/// Keep native checkbox responses and one real search target; guidance stays on hover.
fn layout_check(ui: &mut egui::Ui, label: &str, help: &str, value: &mut bool, key: &str) {
    let t = Tokens::get();
    let width = (ui_kit::ContentWidth::label(ui, label) + t.space_14 + t.space_6 * 2.0 + t.space_16).min(ui.max_rect().width());
    ui.allocate_ui_with_layout(egui::vec2(width.max(0.0), t.control_height), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui_kit::settings_form(ui, 0.0, |ui| {
            ui_kit::row(ui, "", |ui| {
                ui.spacing_mut().item_spacing.x = t.space_6;
                let check_width = (ui.available_width() - t.space_16 - t.space_6).max(0.0);
                let response = ui_kit::settings_control_width(ui, check_width, |ui| {
                    ui_kit::checkbox(ui, label, value).on_hover_text(help).on_disabled_hover_text(help)
                });
                crate::scenario::publish_target(&format!("prefs:{key}"), response.rect);
                ui_kit::help_icon(ui, help);
            })
        });
    });
}

fn browser_visibility(app: &mut SolveApp, ui: &mut egui::Ui) {
    use crate::browser::BrowserShow;
    ui_kit::settings_form(ui, 0.0, |ui| {
        ui_kit::row(ui, app.tr("prefs.browser_shown"), |ui| {
            ui.spacing_mut().item_spacing.x = Tokens::get().space_6;
            crate::prefs_search::note_current(ui, app.tr("prefs.layout_page.on_hover"));
            ui.add_enabled_ui(!crate::timeline_left::stacked(app), |ui| {
                let mut selected = usize::from(app.preferences.browser_show == BrowserShow::Hover);
                if ui_kit::settings_segments(
                    ui,
                    app.tr("prefs.browser_shown"),
                    &mut selected,
                    &[app.tr("prefs.browser_shown"), app.tr("prefs.layout_page.on_hover")],
                ) {
                    app.preferences.browser_show = if selected == 1 { BrowserShow::Hover } else { BrowserShow::Shown };
                }
                if let Some(rect) = crate::scenario::rect_of(&format!("dialog:segments:{}", app.tr("prefs.browser_shown"))) {
                    crate::scenario::publish_target("prefs:browser_show", rect);
                }
                ui_kit::help_icon(ui, app.tr("prefs.browser_show_note"));
            });
        });
    });
}
