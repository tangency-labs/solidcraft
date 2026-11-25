//! Appearance and Layout settings composition, using shared controls and previews.
use super::*;

/// The accent presets, as `#rrggbb`: blue, green, teal, purple, pink, red, orange and amber.
const ACCENTS: [&str; 8] = ["#3b82f6", "#22c55e", "#14b8a6", "#8b5cf6", "#ec4899", "#ef4444", "#f97316", "#eab308"];

/// Accent: the theme's own, a preset, or any colour; a theme that keeps its own shows the row disabled.
fn accent_row(app: &mut SolveApp, ui: &mut egui::Ui) {
    let accepted = accent_accepted(active_theme_file(app));
    ui.add_enabled_ui(accepted, |ui| {
        ui_kit::row(ui, app.tr("prefs.accent"), |ui| {
            let own = app.preferences.accent.is_empty();
            let chip = ui_kit::choice_chip(ui, "accent_theme", app.tr("prefs.accent_theme"), own);
            crate::scenario::publish_target("prefs:accent:theme", chip.rect);
            if chip.clicked() {
                app.preferences.accent.clear();
            }
            // Any other colour: the shared picker, starting from the accent on now.
            let on = crate::theme::parse_accent(&app.preferences.accent).unwrap_or(Tokens::get().accent);
            let mut rgb = [on.r(), on.g(), on.b()];
            if ui_kit::color_field(ui, &mut rgb).changed() {
                app.preferences.accent = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
            }
        });
        // The presets wrap on their own lines: a row clips its controls to one line.
        ui_kit::swatch_grid(ui, &ACCENTS, |ui, hex| {
            let Some(color) = crate::theme::parse_accent(hex) else { return };
            let chosen = app.preferences.accent.eq_ignore_ascii_case(hex);
            // Search draws the page unseen first (#15902): that copy takes an id of its own place.
            let id = if ui.is_visible() { egui::Id::unique(("sc_accent", hex)) } else { ui.scope_id().with(("sc_accent", hex)) };
            let swatch = ui_kit::color_swatch(ui, id, color, chosen, hex, ());
            crate::scenario::publish_target(&format!("prefs:accent:{hex}"), swatch.rect);
            if swatch.clicked() {
                app.preferences.accent = (*hex).to_string();
            }
        });
    });
    if !accepted {
        ui.label(egui::RichText::new(app.tr("prefs.accent_locked")).weak());
    }
}

/// Renders the Appearance preferences page, configuring theme, viewport background, and visual styling.
pub(super) fn appearance(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading_with_tag(
            ui,
            app.tr("prefs.structure.appearance"),
            app.tr("prefs.structure.appearance_note"),
            Some(crate::settings_profiles::tag),
        );
        let count = solidcraft_ui_text::format("prefs.appearance.theme_count", &[("count", &theme_choices(app).len().to_string())]);
        ui_kit::settings_section_count(ui, "theme", app.tr("ui.theme"), Some(&count), |ui| {
            theme_row(app, ui);
            theme_options_row(app, ui);
        });
        ui_kit::settings_section(ui, "viewport_logo", app.tr("prefs.appearance.viewport_logo"), |ui| {
            // The 3D view in the theme's colours, or the Light or Dark theme's (#4146).
            ui_kit::settings_form(ui, t.space_24 * 6.0 + t.space_6, |ui| {
                // The view's background: the theme's gradient, or one colour of the user's (#7688).
                let label = app.tr("prefs.viewport_background");
                let labels = [solidcraft_ui_text::tr("prefs.viewport_background_theme"), solidcraft_ui_text::tr("prefs.viewport_background_colour")];
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
                // A picture of the user's behind the model.
                let label = app.tr("prefs.background_image");
                ui_kit::row(ui, label, |ui| {
                    if ui_kit::button(ui, app.tr("prefs.background_choose"), ui_kit::Button::Normal).clicked() {
                        app.pick_open_for(&["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"], |app, path| {
                            if let Some(path) = path {
                                app.preferences.background_image = path;
                            }
                        });
                    }
                    if !app.preferences.background_image.is_empty() && ui_kit::button(ui, app.tr_label("Reset"), ui_kit::Button::Ghost).clicked() {
                        app.preferences.background_image.clear();
                    }
                });
                // The logo in the theme's own logo colours or in its accent.
                let label = app.tr("prefs.logo");
                ui_kit::row(ui, label, |ui| {
                    let labels = [solidcraft_ui_text::tr("prefs.logo_theme"), solidcraft_ui_text::tr("prefs.logo_accent")];
                    let mut selected = usize::from(app.preferences.logo_accent);
                    if ui_kit::settings_segments(ui, label, &mut selected, &labels) {
                        app.preferences.logo_accent = selected == 1;
                    }
                });
                accent_row(app, ui);
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
                            .backing_on_hover_text(app.tr("prefs.scale_note"));
                            crate::scenario::publish_target("prefs:scale", response.rect);
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        let locale = solidcraft_ui_text::Locale::for_language(&app.preferences.language);
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
                super::figures::scale_specimen,
            );
            ui_kit::divider(ui, "scale_font");
            if !cfg!(target_arch = "wasm32") {
                ui_kit::settings_font_row(ui, solidcraft_ui_text::tr("prefs.font"), |ui, part| {
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
                                    app.set_status(solidcraft_ui_text::tr("prefs.font_unreadable"), true);
                                }
                            }
                        });
                    }
                    if ui_kit::button(ui, app.tr_label("Reset"), ui_kit::Button::Ghost).clicked() {
                        app.preferences.ui_font.clear();
                    }
                });
                ui.scope(|ui| ui_kit::font_specimen(ui, app.tr("prefs.font_sample"))).response.backing_on_hover_text(app.tr("prefs.font_fallback"));
            }
        });
        ui_kit::settings_section(ui, "colour_vision", app.tr("ui.colour_vision"), |ui| {
            let mode = app.preferences.color_vision;
            ui_kit::settings_form(ui, t.form_label_width, |ui| {
                ui_kit::settings_row_help(ui, app.tr("ui.colour_vision_mode"), app.tr("prefs.appearance.colour_vision_help"), |ui| {
                    let modes = solidcraft_theme::ColorVision::ALL;
                    let labels: Vec<_> = modes.iter().map(|m| solidcraft_ui_text::tr(m.label_id())).collect();
                    let mut selected = modes.iter().position(|m| *m == mode).unwrap_or(0);
                    if ui_kit::settings_segments(ui, app.tr("ui.colour_vision_mode"), &mut selected, &labels)
                        && let Some(mode) = modes.get(selected)
                    {
                        app.preferences.color_vision = *mode;
                    }
                    if let Some(rect) = crate::scenario::rect_of(&format!("dialog:segments:{}", app.tr("ui.colour_vision_mode"))) {
                        crate::scenario::publish_target("prefs:color_vision", rect);
                    }
                    for mode in solidcraft_theme::ColorVision::ALL {
                        if let Some(at) =
                            crate::scenario::handle_at(&format!("dialog:{}:{}", app.tr("ui.colour_vision_mode"), app.tr(mode.label_id())))
                        {
                            crate::scenario::publish_handle(&format!("prefs:color_vision:{}", mode.id()), at);
                        }
                    }
                });
            });
            super::figures::status_palette_sample(ui, app.preferences.color_vision);
            // What the modes recolour, in the mode being chosen (#7152): the status chips above keep
            // their colours in every mode.
            super::figures::colour_vision_sample(ui, &solidcraft_theme::Tokens::get_in(app.preferences.color_vision));
            if app.preferences.color_vision != mode {
                ui.ctx().request_repaint();
            }
        });
    });
}

/// Renders the Layout & Panels preferences page, configuring interface density, window layout, dock placement, and timeline display style.
pub(super) fn layout(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading_with_tag(
            ui,
            app.tr("prefs.structure.layout"),
            app.tr("prefs.structure.layout_note"),
            Some(crate::settings_profiles::tag),
        );
        super::panel_layout::show(app, ui);
        // Retain the section key for existing reveal/open callers.
        ui_kit::settings_section(ui, "layout", app.tr("prefs.layout_page.window"), |ui| {
            ui_kit::settings_pair(
                ui,
                |ui| {
                    ui_kit::settings_form(ui, t.form_label_width + t.space_16, |ui| {
                        ui_kit::row(ui, app.tr("ui.panel_layout.panel_style"), |ui| {
                            let mut selected = usize::from(app.ui.layout == crate::ui_kit::LayoutMode::Docked);
                            if ui_kit::segmented(
                                ui,
                                &mut selected,
                                &[solidcraft_ui_text::tr("ui.layout_floating"), solidcraft_ui_text::tr("ui.layout_compact")],
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
                        let response = setting::legacy_surface_toolbar(app, ui).backing_on_hover_text(app.tr("prefs.legacy_surface_toolbar_hint"));
                        crate::scenario::publish_target("prefs:legacy_surface_toolbar", response.rect);
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
                        let response = setting::construct_menu_groups(app, ui);
                        crate::scenario::publish_target("prefs:construct_menu_groups", response.rect);
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
                        // The operations under way, with their Cancel, in the view's corner.
                        layout_check(
                            ui,
                            app.tr("prefs.operations_panel"),
                            app.tr("prefs.operations_panel_note"),
                            &mut app.preferences.operations_panel,
                            "operations_panel",
                        );
                        // The design waits for a file being opened or inserted, as before.
                        layout_check(
                            ui,
                            app.tr("prefs.file_operations_wait"),
                            app.tr("prefs.file_operations_wait_note"),
                            &mut app.preferences.file_operations_wait,
                            "file_operations_wait",
                        );
                    });
                    ui_kit::LayoutPreview {
                        compact: app.ui.layout == crate::ui_kit::LayoutMode::Docked,
                        labels: app.preferences.ribbon_labels,
                        history_in_browser: app.preferences.history_in_browser,
                    }
                },
                |ui, state| ui_kit::layout_preview(ui, state, solidcraft_ui_text::tr("prefs.layout_note")),
            );
            layout_check(
                ui,
                app.tr("prefs.keep_work_on_tab_switch"),
                app.tr("prefs.keep_work_on_tab_switch_note"),
                &mut app.preferences.keep_work_on_tab_switch,
                "keep_work_on_tab_switch",
            );
            layout_check(
                ui,
                app.tr("prefs.remember_toolbar_tab"),
                app.tr("prefs.remember_toolbar_tab_note"),
                &mut app.preferences.remember_toolbar_tab,
                "remember_toolbar_tab",
            );
            layout_check(
                ui,
                app.tr("prefs.tab_double_click_renames"),
                app.tr("prefs.tab_double_click_renames_note"),
                &mut app.preferences.tab_double_click_renames,
                "tab_double_click_renames",
            );
            windows_smooth_window_drag_row(app, ui, WINDOWS_SMOOTH_WINDOW_DRAG_AVAILABLE);
            layout_check(
                ui,
                app.tr("prefs.show_window_controls"),
                app.tr("prefs.show_window_controls_help"),
                &mut app.preferences.show_window_controls,
                "show_window_controls",
            );
            layout_check(
                ui,
                app.tr("window_ribbon.ribbon"),
                app.tr("window_ribbon.preference_help"),
                &mut app.preferences.pop_out_window_defaults.ribbon,
                "pop_out_ribbon",
            );
            ui.add_enabled_ui(app.preferences.pop_out_window_defaults.ribbon, |ui| {
                layout_check(
                    ui,
                    app.tr("window_ribbon.auto_hide"),
                    app.tr("window_ribbon.auto_hide_help"),
                    &mut app.preferences.pop_out_window_defaults.ribbon_auto_hide,
                    "pop_out_ribbon_auto_hide",
                );
            });
            layout_check(
                ui,
                app.tr("window_panels.other_panels"),
                app.tr("window_panels.workspace_panels"),
                &mut app.preferences.pop_out_window_defaults.panels,
                "pop_out_other_panels",
            );
            ui_kit::settings_form(ui, t.form_label_width + t.space_24 * 3.0, |ui| {
                use solidcraft_views::{CardHeaders, TopBar};
                let labels = [app.tr("window_options.always"), app.tr("window_topbar.edge")];
                ui_kit::settings_row_help(ui, app.tr("window_topbar.top_bar"), app.tr("window_topbar.preference_help"), |ui| {
                    let options = &mut app.preferences.pop_out_window_defaults;
                    let mut selection = usize::from(options.top_bar == TopBar::Edge);
                    if ui_kit::segmented(ui, &mut selection, &labels, ui_kit::Segments { intrinsic: true, ..Default::default() })
                        && let Some(state) = TopBar::ALL.get(selection)
                    {
                        options.top_bar = *state;
                    }
                });
                let labels = [app.tr("window_options.always"), app.tr("window_options.on_hover"), app.tr("window_options.hidden")];
                ui_kit::settings_row_help(ui, app.tr("window_options.card_headers"), app.tr("window_options.card_headers_help"), |ui| {
                    let options = &mut app.preferences.pop_out_window_defaults;
                    let mut selection = CardHeaders::ALL.iter().position(|state| *state == options.card_headers).unwrap_or_default();
                    if ui_kit::segmented(ui, &mut selection, &labels, ui_kit::Segments { intrinsic: true, ..Default::default() })
                        && let Some(state) = CardHeaders::ALL.get(selection)
                    {
                        options.card_headers = *state;
                    }
                });
            });
            ui_kit::settings_form(ui, t.form_label_width + t.space_24 * 3.0, |ui| {
                ui_kit::settings_row_help(ui, app.tr("window_size.interface_size"), app.tr("window_size.default_help"), |ui| {
                    use solidcraft_views::InterfaceSize;
                    let options = &mut app.preferences.pop_out_window_defaults;
                    let labels = InterfaceSize::ALL.map(|size| format!("{}%", u16::from(size)));
                    let labels = labels.each_ref().map(String::as_str);
                    let mut selection = InterfaceSize::ALL.iter().position(|size| *size == options.interface_size).unwrap_or_default();
                    if ui_kit::segmented(ui, &mut selection, &labels, ui_kit::Segments { intrinsic: true, ..Default::default() })
                        && let Some(size) = InterfaceSize::ALL.get(selection)
                    {
                        options.interface_size = *size;
                    }
                });
            });
            layout_check(
                ui,
                app.tr("window_options.keep_on_top"),
                app.tr("window_options.keep_on_top_help"),
                &mut app.preferences.pop_out_window_defaults.keep_on_top,
                "pop_out_keep_on_top",
            );
        });
        if let Some(rect) = crate::scenario::rect_of(&format!("dialog:section:{}", app.tr("prefs.layout_page.window"))) {
            crate::scenario::publish_target(&format!("dialog:section:{}", app.tr("prefs.layout")), rect);
        }
        ui_kit::settings_section(ui, "design_tabs", app.tr("prefs.layout_page.design_tabs"), |ui| {
            ui_kit::settings_form(ui, t.form_label_width + t.space_24 * 3.0, |ui| {
                ui_kit::settings_row_help(ui, app.tr("prefs.design_views_as_tabs"), app.tr("prefs.design_views_as_tabs_note"), |ui| {
                    let mut selected = usize::from(!app.preferences.design_views_as_tabs);
                    if ui_kit::segmented(
                        ui,
                        &mut selected,
                        &[app.tr("prefs.design_views_tabs"), app.tr("prefs.design_views_switcher")],
                        ui_kit::Segments { intrinsic: true, ..Default::default() },
                    ) {
                        app.preferences.design_views_as_tabs = selected == 0;
                    }
                });
            });
            layout_check(
                ui,
                app.tr("prefs.show_design_view_switcher"),
                app.tr("prefs.show_design_view_switcher_note"),
                &mut app.preferences.show_design_view_switcher,
                "show_design_view_switcher",
            );
            let response = setting::workspace_follows_view(app, ui);
            crate::scenario::publish_target("prefs:workspace_follows_view", response.rect);
            layout_check(
                ui,
                app.tr("prefs.clear_view_tabs"),
                app.tr("prefs.clear_view_tabs.help"),
                &mut app.preferences.clear_view_tabs,
                "clear_view_tabs",
            );
            layout_check(
                ui,
                app.tr("prefs.design_sub_tab_sheets"),
                app.tr("prefs.design_sub_tab_sheets_note"),
                &mut app.preferences.design_sub_tab_sheets,
                "design_sub_tab_sheets",
            );
            layout_check(
                ui,
                app.tr("prefs.design_sub_tab_render"),
                app.tr("prefs.design_sub_tab_render_note"),
                &mut app.preferences.design_sub_tab_render,
                "design_sub_tab_render",
            );
            layout_check(
                ui,
                app.tr("prefs.design_reuse_views"),
                app.tr("prefs.design_reuse_views_note"),
                &mut app.preferences.design_reuse_views,
                "design_reuse_views",
            );
            layout_check(
                ui,
                app.tr("prefs.design_reopen_views"),
                app.tr("prefs.design_reopen_views_note"),
                &mut app.preferences.design_reopen_views,
                "design_reopen_views",
            );
        });
        ui_kit::settings_section(ui, "panels", app.tr("prefs.layout_page.panels"), |ui| {
            layout_check(
                ui,
                app.tr("prefs.live_panel_drag_preview"),
                app.tr("prefs.live_panel_drag_preview_help"),
                &mut app.preferences.live_panel_drag_preview,
                "live_panel_drag_preview",
            );
            layout_check(
                ui,
                app.tr("prefs.history_in_browser"),
                app.tr("prefs.history_in_browser_note"),
                &mut app.preferences.history_in_browser,
                "history_in_browser",
            );
            layout_check(ui, app.tr("parts_library.show"), app.tr("parts_library.show_help"), &mut app.preferences.library_open, "library_open");
            // The Browser tree panel can be closed from its own host menu; this control brings it
            // back (#16058).
            let browser_label = app.tr("prefs.layout_page.browser_tree");
            let browser_help = app.tr("prefs.layout_page.browser_tree_note");
            browser_tree_check(ui, app, browser_label, browser_help);
            ui_kit::settings_form(ui, t.form_label_width + t.space_24 * 3.0, |ui| {
                ui_kit::settings_row_help(ui, app.tr("prefs.history_sketches"), app.tr("prefs.history_sketches_note"), |ui| {
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
            // The History's and the Browser's checkboxes, each group in even columns (#16542).
            check_columns(
                ui,
                app,
                Some("ui.history_title"),
                &[
                    Check {
                        label: "prefs.layout_page.play_history",
                        help: "prefs.history_play_button_note",
                        key: "history_play_button",
                        value: |p| &mut p.history_play_button,
                    },
                    // A design's joints as one button in the History, from the second joint (#8884).
                    Check {
                        label: "prefs.history_group_joints",
                        help: "prefs.history_group_joints_note",
                        key: "history_group_joints",
                        value: |p| &mut p.history_group_joints,
                    },
                    // Folds in the History bar, and a folder for a STEP insert's items (#8885).
                    Check {
                        label: "prefs.history_bar_groups",
                        help: "prefs.history_bar_groups_note",
                        key: "history_bar_groups",
                        value: |p| &mut p.history_bar_groups,
                    },
                    Check {
                        label: "prefs.layout_page.origin_bodies",
                        help: "prefs.history_origin_bodies_note",
                        key: "history_origin_bodies",
                        value: |p| &mut p.timeline_origin_bodies,
                    },
                    // A folder's icon from its features (#12514).
                    Check {
                        label: "prefs.history_folder_kind_icon",
                        help: "prefs.history_folder_kind_icon_note",
                        key: "history_folder_kind_icon",
                        value: |p| &mut p.history_folder_kind_icon,
                    },
                    // The History controls' steps (#11430).
                    Check {
                        label: "prefs.history_steps_component",
                        help: "prefs.history_steps_component_note",
                        key: "history_steps_component",
                        value: |p| &mut p.history_steps_component,
                    },
                    Check {
                        label: "prefs.history_last_stops_at_component",
                        help: "prefs.history_last_stops_at_component_note",
                        key: "history_last_stops_at_component",
                        value: |p| &mut p.history_last_stops_at_component,
                    },
                    // What a feature's menu does with several selected features.
                    Check {
                        label: "prefs.feature_menus_act_on_selection",
                        help: "prefs.feature_menus_act_on_selection_note",
                        key: "feature_menus_act_on_selection",
                        value: |p| &mut p.feature_menus_act_on_selection,
                    },
                ],
            );
            check_columns(
                ui,
                app,
                Some("ui.browser_title"),
                &[
                    Check {
                        label: "prefs.browser_components_open",
                        help: "prefs.browser_components_open_note",
                        key: "browser_components_open",
                        value: |p| &mut p.browser_components_open,
                    },
                    // Folders and placements start closed and open only on request (#8768).
                    Check {
                        label: "prefs.browser_rows_start_closed",
                        help: "prefs.browser_rows_start_closed_note",
                        key: "browser_rows_start_closed",
                        value: |p| &mut p.browser_rows_start_closed,
                    },
                    Check {
                        label: "prefs.browser_eye_left",
                        help: "prefs.browser_eye_left_note",
                        key: "browser_eye_left",
                        value: |p| &mut p.browser_eye_left,
                    },
                    Check {
                        label: "prefs.browser_body_features",
                        help: "prefs.browser_body_features_note",
                        key: "browser_body_features",
                        value: |p| &mut p.browser_body_features,
                    },
                    // Whether a component's further placements list its contents again (#21458).
                    Check {
                        label: "prefs.browser_instances_list_contents",
                        help: "prefs.browser_instances_list_contents_note",
                        key: "browser_instances_list_contents",
                        value: |p| &mut p.browser_instances_list_contents,
                    },
                ],
            );
        });
        ui_kit::settings_section(ui, "dialogs", app.tr("prefs.dialogs"), |ui| {
            ui_kit::settings_form(ui, t.form_label_width + t.space_24, |ui| {
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
            check_columns(
                ui,
                app,
                None,
                &[
                    Check {
                        label: "prefs.sketch_tab_always",
                        help: "prefs.sketch_tab_always_note",
                        key: "sketch_tab_always",
                        value: |p| &mut p.sketch_tab_always,
                    },
                    Check {
                        label: "prefs.layout_page.active_commands",
                        help: "prefs.persistent_ribbon_pressed_help",
                        key: "persistent_ribbon_pressed",
                        value: |p| &mut p.persistent_ribbon_pressed,
                    },
                    Check { label: "prefs.ribbon_reveal", help: "prefs.ribbon_reveal_note", key: "ribbon_reveal", value: |p| &mut p.ribbon_reveal },
                    Check {
                        label: "prefs.ribbon_tabs_only",
                        help: "prefs.ribbon_tabs_only_note",
                        key: "ribbon_tabs_only",
                        value: |p| &mut p.ribbon_tabs_only,
                    },
                    Check {
                        label: "prefs.layout_page.pinned_menus",
                        help: "ui.ribbon_menus_skip_pinned",
                        key: "ribbon_menus_skip_pinned",
                        value: |p| &mut p.ribbon_menus_skip_pinned,
                    },
                    Check {
                        label: "prefs.layout_page.hover_panel_menus",
                        help: "prefs.layout_page.hover_panel_menus_help",
                        key: "ribbon_menus_follow_hover",
                        value: |p| &mut p.ribbon_menus_follow_hover,
                    },
                    // Pinned constraints as small cells in the Constrain grid.
                    Check {
                        label: "prefs.pinned_constraints_in_grid",
                        help: "prefs.pinned_constraints_in_grid_note",
                        key: "pinned_constraints_in_grid",
                        value: |p| &mut p.pinned_constraints_in_grid,
                    },
                    Check {
                        label: "prefs.command_search_centred",
                        help: "prefs.command_search_centred_note",
                        key: "command_search_centred",
                        value: |p| &mut p.command_search_centred,
                    },
                ],
            );
        });
        super::navbar::show(app, ui);
    });
}

/// One box of a [`check_columns`] group: its label and hover help as message ids, its handle
/// (`prefs:<key>`), and the preference it turns on and off.
struct Check {
    label: &'static str,
    help: &'static str,
    key: &'static str,
    value: fn(&mut Prefs) -> &mut bool,
}

/// Checkboxes in even columns, so they line up instead of wrapping like words (#16542): two
/// columns when the widest label fits twice, one in a narrow window, every cell as wide as its
/// column. A dim caption (a message id) names the group.
fn check_columns(ui: &mut egui::Ui, app: &mut SolveApp, caption: Option<&'static str>, checks: &[Check]) {
    let t = Tokens::get();
    if let Some(caption) = caption {
        ui_kit::settings_inline_note(ui, app.tr(caption));
    }
    let widest = checks.iter().map(|check| ui_kit::ContentWidth::label(ui, app.tr(check.label))).fold(0.0, f32::max)
        + t.space_14
        + t.space_6 * 2.0
        + t.space_16;
    let columns: usize = if ui.available_width() >= widest * 2.0 + t.space_16 { 2 } else { 1 };
    let width = ((ui.available_width() - t.space_16 * (columns - 1) as f32) / columns as f32).max(0.0);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = t.space_6;
        for row in checks.chunks(columns) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = t.space_16;
                for check in row {
                    ui.allocate_ui_with_layout(egui::vec2(width, t.control_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                        // A cell shrinks to its box unless told otherwise; the full width keeps the columns even.
                        ui.set_min_width(width);
                        let (label, help) = (app.tr(check.label), app.tr(check.help));
                        // The box gets the whole cell: sized to its label alone, a label a fraction wider
                        // at a fractional display scale would end in an ellipsis.
                        check_row(ui, label, help, (check.value)(&mut app.preferences), check.key);
                    });
                }
            });
        }
    });
}

/// Windows: nothing is drawn while the window is dragged by its title bar, so it moves smoothly
/// (`windows_smooth_window_drag`); drawn only where the choice exists (`available`:
/// [`WINDOWS_SMOOTH_WINDOW_DRAG_AVAILABLE`]). It applies at once: no restart badge.
pub(super) fn windows_smooth_window_drag_row(app: &mut SolveApp, ui: &mut egui::Ui, available: bool) {
    if !available {
        return;
    }
    layout_check(
        ui,
        app.tr("prefs.windows_smooth_window_drag"),
        app.tr("prefs.windows_smooth_window_drag_help"),
        &mut app.preferences.windows_smooth_window_drag,
        "windows_smooth_window_drag",
    );
}

/// Keep the Browser tree panel reachable from Layout settings. The panel closes from its own host
/// menu, and the window has no other way back (#16058).
fn browser_tree_check(ui: &mut egui::Ui, app: &mut SolveApp, label: &str, help: &str) {
    let t = Tokens::get();
    let mut visible = crate::browser::panel_open(app);
    let width = (ui_kit::ContentWidth::label(ui, label) + t.space_14 + t.space_6 * 2.0 + t.space_16).min(ui.max_rect().width());
    ui.allocate_ui_with_layout(egui::vec2(width.max(0.0), t.control_height), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui_kit::settings_form(ui, 0.0, |ui| {
            ui_kit::row(ui, "", |ui| {
                ui.spacing_mut().item_spacing.x = t.space_6;
                let check_width = (ui.available_width() - t.space_16 - t.space_6).max(0.0);
                let response =
                    ui_kit::settings_control_width(ui, check_width, |ui| ui_kit::checkbox(ui, label, &mut visible).backing_on_hover_text(help));
                crate::scenario::publish_target("prefs:browser_tree", response.rect);
                ui_kit::help_icon(ui, help);
                if response.changed() {
                    let _ = app.run("view.browser", json!({"visible": visible}));
                }
            })
        });
    });
}

/// Keep native checkbox responses and one real search target; guidance stays on hover.
fn layout_check(ui: &mut egui::Ui, label: &str, help: &str, value: &mut bool, key: &str) {
    let t = Tokens::get();
    let width = (ui_kit::ContentWidth::label(ui, label) + t.space_14 + t.space_6 * 2.0 + t.space_16).min(ui.max_rect().width());
    ui.allocate_ui_with_layout(egui::vec2(width.max(0.0), t.control_height), egui::Layout::top_down(egui::Align::Min), |ui| {
        check_row(ui, label, help, value, key);
    });
}

/// The box, its label and its help icon, in the width `ui` gives them.
fn check_row(ui: &mut egui::Ui, label: &str, help: &str, value: &mut bool, key: &str) {
    let t = Tokens::get();
    ui_kit::settings_form(ui, 0.0, |ui| {
        ui_kit::row(ui, "", |ui| {
            ui.spacing_mut().item_spacing.x = t.space_6;
            let check_width = (ui.available_width() - t.space_16 - t.space_6).max(0.0);
            let response = ui_kit::settings_control_width(ui, check_width, |ui| ui_kit::checkbox(ui, label, value).backing_on_hover_text(help));
            crate::scenario::publish_target(&format!("prefs:{key}"), response.rect);
            ui_kit::help_icon(ui, help);
        })
    });
}
