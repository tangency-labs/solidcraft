//! Appearance and Layout settings composition, using shared controls and previews.
use super::*;

pub(super) fn appearance(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading_with_tag(
            ui,
            tr!(app, "prefs.structure.appearance"),
            tr!(app, "prefs.structure.appearance_note"),
            Some(crate::settings_profiles::tag),
        );
        let count = tr!("prefs.appearance.theme_count", count = &theme_choices(app).len().to_string());
        ui_kit::settings_section_count(ui, "theme", tr!(app, "ui.theme"), Some(&count), |ui| {
            theme_row(app, ui);
            theme_options_row(app, ui);
        });
        ui_kit::settings_section(ui, "viewport_logo", tr!(app, "prefs.appearance.viewport_logo"), |ui| {
            // The 3D view in the theme's colours, or the Light or Dark theme's (#4146).
            ui_kit::settings_form(ui, t.space_24 * 6.0 + t.space_6, |ui| {
                // The view's background: the theme's gradient, or one colour of the user's (#7688).
                let label = tr!(app, "prefs.viewport_background");
                let labels = [tr!("prefs.viewport_background_theme"), tr!("prefs.viewport_background_colour")];
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
                            let response = ui_kit::cell_editor(ui, key.with("edit"), &mut hex, tr!(app, "color.hex_hint"), false);
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
                let label = tr!(app, "prefs.appearance.viewport_colours");
                ui_kit::row(ui, label, |ui| {
                    let views = crate::theme::ViewportTheme::ALL;
                    let labels = [tr!(app, "prefs.appearance.theme_own"), tr!(app, "ui.light"), tr!(app, "ui.dark")];
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
                            crate::scenario::publish_handle(&format!("dialog:{}:{}", tr!(app, "prefs.viewport"), app.tr_by_id(view.label_id())), at);
                        }
                    }
                });
                // A picture of the user's behind the model.
                let label = tr!(app, "prefs.background_image");
                ui_kit::row(ui, label, |ui| {
                    if ui_kit::button(ui, tr!(app, "prefs.background_choose"), ui_kit::Button::Normal).clicked() {
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
                let label = tr!(app, "prefs.logo");
                ui_kit::row(ui, label, |ui| {
                    let labels = [tr!("prefs.logo_theme"), tr!("prefs.logo_accent")];
                    let mut selected = usize::from(app.preferences.logo_accent);
                    if ui_kit::settings_segments(ui, label, &mut selected, &labels) {
                        app.preferences.logo_accent = selected == 1;
                    }
                });
                // A theme file another tool writes, reloaded when it changes (#7573).
                follow_row(app, ui);
            });
        });
        ui_kit::settings_section(ui, "scale", tr!(app, "prefs.scale_text"), |ui| {
            ui_kit::settings_pair(
                ui,
                |ui| {
                    ui_kit::settings_form(ui, 0.0, |ui| {
                        ui_kit::row(ui, tr!(app, "prefs.appearance.interface_scale"), |ui| {
                            let response = ui_kit::named_control(ui, tr!(app, "prefs.appearance.interface_scale"), |ui| {
                                ui_kit::slider(
                                    ui,
                                    &mut app.preferences.ui_scale,
                                    0.5..=2.5,
                                    ui_kit::Slider { step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
                                )
                            })
                            .backing_on_hover_text(tr!(app, "prefs.scale_note"));
                            crate::scenario::publish_target("prefs:scale", response.rect);
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        let locale = solidcraft_ui_text::Locale::for_language(&app.preferences.language);
                        for value in [0.75, 1.0, 1.1, 1.25, 1.5, 2.0] {
                            let percent = format!("{:.0}", value * 100.0);
                            let chip = ui_kit::button(
                                ui,
                                tr!(locale, "prefs.structure.scale_percent", percent = &percent),
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
                ui_kit::settings_font_row(ui, tr!("prefs.font"), |ui, part| {
                    if part == 0 {
                        ui_font_row(app, ui);
                        return;
                    }
                    if ui_kit::button(ui, tr!(app, "prefs.choose_font"), ui_kit::Button::Normal).clicked() {
                        app.pick_open_for(&["ttf", "otf", "ttc"], |app, path| {
                            if let Some(path) = path {
                                if crate::fonts::ui_font(&path).is_some() {
                                    app.preferences.ui_font = path;
                                } else {
                                    app.set_status(tr!("prefs.font_unreadable"), true);
                                }
                            }
                        });
                    }
                    if ui_kit::button(ui, app.tr_label("Reset"), ui_kit::Button::Ghost).clicked() {
                        app.preferences.ui_font.clear();
                    }
                });
                ui.scope(|ui| ui_kit::font_specimen(ui, tr!(app, "prefs.font_sample")))
                    .response
                    .backing_on_hover_text(tr!(app, "prefs.font_fallback"));
            }
        });
        ui_kit::settings_section(ui, "colour_vision", tr!(app, "ui.colour_vision"), |ui| {
            let mode = app.preferences.color_vision;
            ui_kit::settings_form(ui, t.form_label_width, |ui| {
                ui_kit::settings_row_help(ui, tr!(app, "ui.colour_vision_mode"), tr!(app, "prefs.appearance.colour_vision_help"), |ui| {
                    let modes = solidcraft_theme::ColorVision::ALL;
                    let labels: Vec<_> = modes.iter().map(|m| solidcraft_ui_text::tr_by_id(m.label_id())).collect();
                    let mut selected = modes.iter().position(|m| *m == mode).unwrap_or(0);
                    if ui_kit::settings_segments(ui, tr!(app, "ui.colour_vision_mode"), &mut selected, &labels)
                        && let Some(mode) = modes.get(selected)
                    {
                        app.preferences.color_vision = *mode;
                    }
                    if let Some(rect) = crate::scenario::rect_of(&format!("dialog:segments:{}", tr!(app, "ui.colour_vision_mode"))) {
                        crate::scenario::publish_target("prefs:color_vision", rect);
                    }
                    for mode in solidcraft_theme::ColorVision::ALL {
                        if let Some(at) =
                            crate::scenario::handle_at(&format!("dialog:{}:{}", tr!(app, "ui.colour_vision_mode"), app.tr_by_id(mode.label_id())))
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

pub(super) fn layout(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading_with_tag(
            ui,
            tr!(app, "prefs.structure.layout"),
            tr!(app, "prefs.structure.layout_note"),
            Some(crate::settings_profiles::tag),
        );
        super::panel_layout::show(app, ui);
        // Retain the section key for existing reveal/open callers.
        ui_kit::settings_section(ui, "layout", tr!(app, "prefs.layout_page.window"), |ui| {
            ui_kit::settings_pair(
                ui,
                |ui| {
                    ui_kit::settings_form(ui, t.form_label_width + t.space_16, |ui| {
                        ui_kit::row(ui, tr!(app, "ui.panel_layout.panel_style"), |ui| {
                            let mut selected = usize::from(app.ui.layout == crate::ui_kit::LayoutMode::Docked);
                            if ui_kit::segmented(
                                ui,
                                &mut selected,
                                &[tr!("ui.layout_floating"), tr!("ui.layout_compact")],
                                ui_kit::Segments { intrinsic: true, ..Default::default() },
                            ) {
                                let _ = app.run("view.layout", json!({"mode": if selected == 1 { "compact" } else { "floating" }}));
                            }
                        });
                        ui_kit::row(ui, tr!(app, "prefs.toolbar"), |ui| {
                            let mut selected = usize::from(!app.preferences.ribbon_labels);
                            if ui_kit::segmented(
                                ui,
                                &mut selected,
                                &[tr!(app, "prefs.labels"), tr!(app, "prefs.icons_only")],
                                ui_kit::Segments { intrinsic: true, ..Default::default() },
                            ) {
                                app.preferences.ribbon_labels = selected == 0;
                            }
                        });
                        let response = setting::legacy_surface_toolbar(app, ui).backing_on_hover_text(tr!(app, "prefs.legacy_surface_toolbar_hint"));
                        crate::scenario::publish_target("prefs:legacy_surface_toolbar", response.rect);
                        ui_kit::row(ui, tr!(app, "prefs.menus"), |ui| {
                            let mut selected = usize::from(app.preferences.ribbon_icon_grid);
                            if ui_kit::segmented(
                                ui,
                                &mut selected,
                                &[tr!(app, "prefs.menu_list"), tr!(app, "prefs.menu_grid")],
                                ui_kit::Segments { intrinsic: true, ..Default::default() },
                            ) {
                                app.preferences.ribbon_icon_grid = selected == 1;
                            }
                        });
                        let response = setting::construct_menu_groups(app, ui);
                        crate::scenario::publish_target("prefs:construct_menu_groups", response.rect);
                        ui_kit::row(ui, tr!(app, "prefs.toast_place"), |ui| {
                            let places = [crate::viewport::ToastPlace::Corner, crate::viewport::ToastPlace::Centre];
                            let mut selected = places.iter().position(|p| *p == app.preferences.toast_place).unwrap_or(0);
                            if ui_kit::segmented(
                                ui,
                                &mut selected,
                                &[tr!(app, "prefs.bottom_right"), tr!(app, "prefs.bottom_centre")],
                                ui_kit::Segments { intrinsic: true, ..Default::default() },
                            ) && let Some(place) = places.get(selected)
                            {
                                app.preferences.toast_place = *place;
                            }
                        });
                        let row = tr!(app, "prefs.toast_place");
                        for (label, key) in [("prefs.bottom_right", "corner"), ("prefs.bottom_centre", "centre")] {
                            if let Some(at) = crate::scenario::handle_at(&format!("dialog:{row}:{}", app.tr_by_id(label))) {
                                crate::scenario::publish_handle(&format!("prefs:toast_place:{key}"), at);
                            }
                        }
                        layout_check(
                            ui,
                            tr!(app, "prefs.status_bar"),
                            tr!(app, "prefs.status_bar_note"),
                            &mut app.preferences.status_bar,
                            "status_bar",
                        );
                        // The operations under way, with their Cancel, in the view's corner.
                        layout_check(
                            ui,
                            tr!(app, "prefs.operations_panel"),
                            tr!(app, "prefs.operations_panel_note"),
                            &mut app.preferences.operations_panel,
                            "operations_panel",
                        );
                        // The design waits for a file being opened or inserted, as before.
                        layout_check(
                            ui,
                            tr!(app, "prefs.file_operations_wait"),
                            tr!(app, "prefs.file_operations_wait_note"),
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
                |ui, state| ui_kit::layout_preview(ui, state, tr!("prefs.layout_note")),
            );
            layout_check(
                ui,
                tr!(app, "prefs.keep_work_on_tab_switch"),
                tr!(app, "prefs.keep_work_on_tab_switch_note"),
                &mut app.preferences.keep_work_on_tab_switch,
                "keep_work_on_tab_switch",
            );
            layout_check(
                ui,
                tr!(app, "prefs.remember_toolbar_tab"),
                tr!(app, "prefs.remember_toolbar_tab_note"),
                &mut app.preferences.remember_toolbar_tab,
                "remember_toolbar_tab",
            );
            layout_check(
                ui,
                tr!(app, "prefs.tab_double_click_renames"),
                tr!(app, "prefs.tab_double_click_renames_note"),
                &mut app.preferences.tab_double_click_renames,
                "tab_double_click_renames",
            );
            windows_smooth_window_drag_row(app, ui, WINDOWS_SMOOTH_WINDOW_DRAG_AVAILABLE);
            layout_check(
                ui,
                tr!(app, "window_ribbon.ribbon"),
                tr!(app, "window_ribbon.preference_help"),
                &mut app.preferences.pop_out_window_defaults.ribbon,
                "pop_out_ribbon",
            );
            ui.add_enabled_ui(app.preferences.pop_out_window_defaults.ribbon, |ui| {
                layout_check(
                    ui,
                    tr!(app, "window_ribbon.auto_hide"),
                    tr!(app, "window_ribbon.auto_hide_help"),
                    &mut app.preferences.pop_out_window_defaults.ribbon_auto_hide,
                    "pop_out_ribbon_auto_hide",
                );
            });
            layout_check(
                ui,
                tr!(app, "window_panels.other_panels"),
                tr!(app, "window_panels.workspace_panels"),
                &mut app.preferences.pop_out_window_defaults.panels,
                "pop_out_other_panels",
            );
            ui_kit::settings_form(ui, t.form_label_width + t.space_24 * 3.0, |ui| {
                use solidcraft_views::{CardHeaders, TopBar};
                let labels = [tr!(app, "window_options.always"), tr!(app, "window_topbar.edge")];
                ui_kit::settings_row_help(ui, tr!(app, "window_topbar.top_bar"), tr!(app, "window_topbar.preference_help"), |ui| {
                    let options = &mut app.preferences.pop_out_window_defaults;
                    let mut selection = usize::from(options.top_bar == TopBar::Edge);
                    if ui_kit::segmented(ui, &mut selection, &labels, ui_kit::Segments { intrinsic: true, ..Default::default() })
                        && let Some(state) = TopBar::ALL.get(selection)
                    {
                        options.top_bar = *state;
                    }
                });
                let labels = [tr!(app, "window_options.always"), tr!(app, "window_options.on_hover"), tr!(app, "window_options.hidden")];
                ui_kit::settings_row_help(ui, tr!(app, "window_options.card_headers"), tr!(app, "window_options.card_headers_help"), |ui| {
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
                ui_kit::settings_row_help(ui, tr!(app, "window_size.interface_size"), tr!(app, "window_size.default_help"), |ui| {
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
                tr!(app, "window_options.keep_on_top"),
                tr!(app, "window_options.keep_on_top_help"),
                &mut app.preferences.pop_out_window_defaults.keep_on_top,
                "pop_out_keep_on_top",
            );
        });
        if let Some(rect) = crate::scenario::rect_of(&format!("dialog:section:{}", tr!(app, "prefs.layout_page.window"))) {
            crate::scenario::publish_target(&format!("dialog:section:{}", tr!(app, "prefs.layout")), rect);
        }
        ui_kit::settings_section(ui, "design_tabs", tr!(app, "prefs.layout_page.design_tabs"), |ui| {
            ui_kit::settings_form(ui, t.form_label_width + t.space_24 * 3.0, |ui| {
                ui_kit::settings_row_help(ui, tr!(app, "prefs.design_views_as_tabs"), tr!(app, "prefs.design_views_as_tabs_note"), |ui| {
                    let mut selected = usize::from(!app.preferences.design_views_as_tabs);
                    if ui_kit::segmented(
                        ui,
                        &mut selected,
                        &[tr!(app, "prefs.design_views_tabs"), tr!(app, "prefs.design_views_switcher")],
                        ui_kit::Segments { intrinsic: true, ..Default::default() },
                    ) {
                        app.preferences.design_views_as_tabs = selected == 0;
                    }
                });
            });
            layout_check(
                ui,
                tr!(app, "prefs.show_design_view_switcher"),
                tr!(app, "prefs.show_design_view_switcher_note"),
                &mut app.preferences.show_design_view_switcher,
                "show_design_view_switcher",
            );
            let response = setting::workspace_follows_view(app, ui);
            crate::scenario::publish_target("prefs:workspace_follows_view", response.rect);
            layout_check(
                ui,
                tr!(app, "prefs.clear_view_tabs"),
                tr!(app, "prefs.clear_view_tabs.help"),
                &mut app.preferences.clear_view_tabs,
                "clear_view_tabs",
            );
            layout_check(
                ui,
                tr!(app, "prefs.design_sub_tab_sheets"),
                tr!(app, "prefs.design_sub_tab_sheets_note"),
                &mut app.preferences.design_sub_tab_sheets,
                "design_sub_tab_sheets",
            );
            layout_check(
                ui,
                tr!(app, "prefs.design_sub_tab_render"),
                tr!(app, "prefs.design_sub_tab_render_note"),
                &mut app.preferences.design_sub_tab_render,
                "design_sub_tab_render",
            );
            layout_check(
                ui,
                tr!(app, "prefs.design_reuse_views"),
                tr!(app, "prefs.design_reuse_views_note"),
                &mut app.preferences.design_reuse_views,
                "design_reuse_views",
            );
            layout_check(
                ui,
                tr!(app, "prefs.design_reopen_views"),
                tr!(app, "prefs.design_reopen_views_note"),
                &mut app.preferences.design_reopen_views,
                "design_reopen_views",
            );
        });
        ui_kit::settings_section(ui, "panels", tr!(app, "prefs.layout_page.panels"), |ui| {
            layout_check(
                ui,
                tr!(app, "prefs.live_panel_drag_preview"),
                tr!(app, "prefs.live_panel_drag_preview_help"),
                &mut app.preferences.live_panel_drag_preview,
                "live_panel_drag_preview",
            );
            layout_check(
                ui,
                tr!(app, "prefs.history_in_browser"),
                tr!(app, "prefs.history_in_browser_note"),
                &mut app.preferences.history_in_browser,
                "history_in_browser",
            );
            layout_check(
                ui,
                tr!(app, "library.show"),
                tr!(app, "library.show_help"),
                &mut app.preferences.show_document_browser,
                "show_document_browser",
            );
            let sizes = crate::library::ThumbnailSize::ALL;
            let options = [
                ui_kit::PictureChoice {
                    key: "small",
                    label: app.tr_by_id(sizes[0].label()),
                    figure: |p, rect| document_browser_preview(p, rect, 0.2),
                },
                ui_kit::PictureChoice {
                    key: "medium",
                    label: app.tr_by_id(sizes[1].label()),
                    figure: |p, rect| document_browser_preview(p, rect, 0.25),
                },
                ui_kit::PictureChoice {
                    key: "large",
                    label: app.tr_by_id(sizes[2].label()),
                    figure: |p, rect| document_browser_preview(p, rect, 0.3),
                },
            ];
            let selected = sizes.iter().position(|size| *size == app.preferences.library_thumbnail_size).unwrap_or(2);
            if let Some(selected) =
                ui_kit::settings_picture_choices(ui, "document_browser", tr!(app, "library.thumbnail_section"), &options, selected, true)
                && let Some(size) = sizes.get(selected)
            {
                app.preferences.library_thumbnail_size = *size;
            }
            ui_kit::text(ui, tr!(app, "library.thumbnail_help"), ui_kit::Text::Note("document_browser_size"));
            // The Browser tree panel can be closed from its own host menu; this control brings it
            // back (#16058).
            let browser_label = tr!(app, "prefs.layout_page.browser_tree");
            let browser_help = tr!(app, "prefs.layout_page.browser_tree_note");
            browser_tree_check(ui, app, browser_label, browser_help);
            ui_kit::settings_form(ui, t.form_label_width + t.space_24 * 3.0, |ui| {
                ui_kit::settings_row_help(ui, tr!(app, "prefs.history_sketches"), tr!(app, "prefs.history_sketches_note"), |ui| {
                    let mut selected = usize::from(app.preferences.history_sketches == HistorySketches::UnderFeature);
                    if ui_kit::segmented(
                        ui,
                        &mut selected,
                        &[tr!(app, "prefs.history_sketches_in_order"), tr!(app, "prefs.history_sketches_under_feature")],
                        ui_kit::Segments { intrinsic: true, ..Default::default() },
                    ) {
                        app.preferences.history_sketches = if selected == 1 { HistorySketches::UnderFeature } else { HistorySketches::InOrder };
                    }
                });
                for (key, label) in [("in_order", "prefs.history_sketches_in_order"), ("under_feature", "prefs.history_sketches_under_feature")] {
                    if let Some(rect) = crate::scenario::rect_of(&format!("dialog:{}:{}", tr!(app, "prefs.history_sketches"), app.tr_by_id(label))) {
                        crate::scenario::publish_target(&format!("prefs:history_sketches:{key}"), rect);
                        crate::scenario::publish_target(&format!("dialog:Sketches:{}", app.tr_by_id(label)), rect);
                    }
                }
                ui_kit::row(ui, tr!(app, "prefs.bodies_folders"), |ui| {
                    let modes = BodiesFolders::ALL;
                    let labels: Vec<_> = modes.iter().map(|m| app.tr_by_id(m.label_id())).collect();
                    let mut selected = modes.iter().position(|m| *m == app.preferences.browser_bodies_folders).unwrap_or(0);
                    if ui_kit::segmented(ui, &mut selected, &labels, ui_kit::Segments { intrinsic: true, ..Default::default() })
                        && let Some(mode) = modes.get(selected)
                    {
                        app.preferences.browser_bodies_folders = *mode;
                    }
                    if let Some(rect) =
                        crate::scenario::rect_of(&format!("dialog:{}:{}", tr!(app, "prefs.bodies_folders"), tr!(app, "prefs.bodies_folders_several")))
                    {
                        crate::scenario::publish_target(&format!("dialog:{}:Several bodies only", tr!(app, "prefs.bodies_folders")), rect);
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_16, t.space_6);
                layout_check(
                    ui,
                    tr!(app, "prefs.layout_page.play_history"),
                    tr!(app, "prefs.history_play_button_note"),
                    &mut app.preferences.history_play_button,
                    "history_play_button",
                );
                // A design's joints as one button in the History, from the second joint (#8884).
                layout_check(
                    ui,
                    tr!(app, "prefs.history_group_joints"),
                    tr!(app, "prefs.history_group_joints_note"),
                    &mut app.preferences.history_group_joints,
                    "history_group_joints",
                );
                // Folds in the History bar, and a folder for a STEP insert's items (#8885).
                layout_check(
                    ui,
                    tr!(app, "prefs.history_bar_groups"),
                    tr!(app, "prefs.history_bar_groups_note"),
                    &mut app.preferences.history_bar_groups,
                    "history_bar_groups",
                );
                layout_check(
                    ui,
                    tr!(app, "prefs.layout_page.origin_bodies"),
                    tr!(app, "prefs.history_origin_bodies_note"),
                    &mut app.preferences.timeline_origin_bodies,
                    "history_origin_bodies",
                );
                layout_check(
                    ui,
                    tr!(app, "prefs.browser_components_open"),
                    tr!(app, "prefs.browser_components_open_note"),
                    &mut app.preferences.browser_components_open,
                    "browser_components_open",
                );
                // Folders and placements start closed and open only on request (#8768).
                layout_check(
                    ui,
                    tr!(app, "prefs.browser_rows_start_closed"),
                    tr!(app, "prefs.browser_rows_start_closed_note"),
                    &mut app.preferences.browser_rows_start_closed,
                    "browser_rows_start_closed",
                );
                layout_check(
                    ui,
                    tr!(app, "prefs.browser_eye_left"),
                    tr!(app, "prefs.browser_eye_left_note"),
                    &mut app.preferences.browser_eye_left,
                    "browser_eye_left",
                );
                layout_check(
                    ui,
                    tr!(app, "prefs.browser_body_features"),
                    tr!(app, "prefs.browser_body_features_note"),
                    &mut app.preferences.browser_body_features,
                    "browser_body_features",
                );
            });
            // A folder's icon from its features (#12514) and the History controls' steps (#11430), in
            // a row of their own: one more box in the row above widens it past the page at narrow
            // windows.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_16, t.space_6);
                layout_check(
                    ui,
                    tr!(app, "prefs.history_folder_kind_icon"),
                    tr!(app, "prefs.history_folder_kind_icon_note"),
                    &mut app.preferences.history_folder_kind_icon,
                    "history_folder_kind_icon",
                );
                layout_check(
                    ui,
                    tr!(app, "prefs.history_steps_component"),
                    tr!(app, "prefs.history_steps_component_note"),
                    &mut app.preferences.history_steps_component,
                    "history_steps_component",
                );
                layout_check(
                    ui,
                    tr!(app, "prefs.history_last_stops_at_component"),
                    tr!(app, "prefs.history_last_stops_at_component_note"),
                    &mut app.preferences.history_last_stops_at_component,
                    "history_last_stops_at_component",
                );
            });
            // What a feature's menu does with several selected features, in a row of its own for
            // the same reason.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_16, t.space_6);
                layout_check(
                    ui,
                    tr!(app, "prefs.feature_menus_act_on_selection"),
                    tr!(app, "prefs.feature_menus_act_on_selection_note"),
                    &mut app.preferences.feature_menus_act_on_selection,
                    "feature_menus_act_on_selection",
                );
            });
        });
        ui_kit::settings_section(ui, "dialogs", tr!(app, "prefs.dialogs"), |ui| {
            ui_kit::settings_form(ui, t.form_label_width + t.space_24, |ui| {
                ui_kit::row(ui, tr!(app, "prefs.layout_page.ok_cancel"), |ui| {
                    let placements = [crate::ui_kit::DialogButtons::Bottom, crate::ui_kit::DialogButtons::Top];
                    let mut selected = placements.iter().position(|p| *p == app.preferences.dialog_buttons).unwrap_or(0);
                    if ui_kit::segmented(
                        ui,
                        &mut selected,
                        &[tr!(app, "prefs.bottom"), tr!(app, "prefs.top")],
                        ui_kit::Segments { intrinsic: true, ..Default::default() },
                    ) && let Some(placement) = placements.get(selected)
                    {
                        app.preferences.dialog_buttons = *placement;
                    }
                });
                // Handles that don't depend on the language, by the saved value.
                let row = tr!(app, "prefs.layout_page.ok_cancel");
                for (label, key) in [("prefs.bottom", "bottom"), ("prefs.top", "top")] {
                    if let Some(at) = crate::scenario::handle_at(&format!("dialog:{row}:{}", app.tr_by_id(label))) {
                        crate::scenario::publish_handle(&format!("prefs:dialog_buttons:{key}"), at);
                        crate::scenario::publish_handle(&format!("dialog:{}:{}", tr!(app, "prefs.dialog_buttons"), app.tr_by_id(label)), at);
                    }
                }
            });
            layout_check(
                ui,
                tr!(app, "prefs.layout_page.view_corner"),
                tr!(app, "prefs.view_confirm_corner_note"),
                &mut app.preferences.view_confirm_corner,
                "view_confirm_corner",
            );
        });
        ui_kit::settings_section(ui, "toolbar_tabs", tr!(app, "prefs.toolbar_tabs"), |ui| {
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
                tr!(app, "prefs.sketch_tab_always"),
                tr!(app, "prefs.layout_page.active_commands"),
                tr!(app, "prefs.ribbon_reveal"),
                tr!(app, "prefs.ribbon_tabs_only"),
                tr!(app, "prefs.layout_page.pinned_menus"),
                tr!(app, "prefs.layout_page.hover_panel_menus"),
                tr!(app, "prefs.pinned_constraints_in_grid"),
            ];
            let widest =
                labels.iter().map(|label| ui_kit::ContentWidth::label(ui, label)).fold(0.0, f32::max) + t.space_14 + t.space_6 * 2.0 + t.space_16;
            let columns = if ui.available_width() >= widest * 2.0 + t.space_16 { 2 } else { 1 };
            let width = (ui.available_width() - t.space_16 * (columns - 1) as f32) / columns as f32;
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = t.space_6;
                for row in (0..labels.len()).step_by(columns) {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = t.space_16;
                        for index in row..(row + columns).min(labels.len()) {
                            ui.allocate_ui_with_layout(
                                egui::vec2(width.max(0.0), t.control_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| match index {
                                    0 => {
                                        layout_check(
                                            ui,
                                            tr!(app, "prefs.sketch_tab_always"),
                                            tr!(app, "prefs.sketch_tab_always_note"),
                                            &mut app.preferences.sketch_tab_always,
                                            "sketch_tab_always",
                                        );
                                    }
                                    1 => {
                                        layout_check(
                                            ui,
                                            tr!(app, "prefs.layout_page.active_commands"),
                                            tr!(app, "prefs.persistent_ribbon_pressed_help"),
                                            &mut app.preferences.persistent_ribbon_pressed,
                                            "persistent_ribbon_pressed",
                                        );
                                    }
                                    2 => {
                                        layout_check(
                                            ui,
                                            tr!(app, "prefs.ribbon_reveal"),
                                            tr!(app, "prefs.ribbon_reveal_note"),
                                            &mut app.preferences.ribbon_reveal,
                                            "ribbon_reveal",
                                        );
                                    }
                                    3 => {
                                        layout_check(
                                            ui,
                                            tr!(app, "prefs.ribbon_tabs_only"),
                                            tr!(app, "prefs.ribbon_tabs_only_note"),
                                            &mut app.preferences.ribbon_tabs_only,
                                            "ribbon_tabs_only",
                                        );
                                    }
                                    4 => {
                                        layout_check(
                                            ui,
                                            tr!(app, "prefs.layout_page.pinned_menus"),
                                            tr!(app, "ui.ribbon_menus_skip_pinned"),
                                            &mut app.preferences.ribbon_menus_skip_pinned,
                                            "ribbon_menus_skip_pinned",
                                        );
                                    }
                                    5 => {
                                        layout_check(
                                            ui,
                                            tr!(app, "prefs.layout_page.hover_panel_menus"),
                                            tr!(app, "prefs.layout_page.hover_panel_menus_help"),
                                            &mut app.preferences.ribbon_menus_follow_hover,
                                            "ribbon_menus_follow_hover",
                                        );
                                    }
                                    // Pinned constraints as small cells in the Constrain grid.
                                    6 => {
                                        layout_check(
                                            ui,
                                            tr!(app, "prefs.pinned_constraints_in_grid"),
                                            tr!(app, "prefs.pinned_constraints_in_grid_note"),
                                            &mut app.preferences.pinned_constraints_in_grid,
                                            "pinned_constraints_in_grid",
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

/// Windows: nothing is drawn while the window is dragged by its title bar, so it moves smoothly
/// (`windows_smooth_window_drag`); drawn only where the choice exists (`available`:
/// [`WINDOWS_SMOOTH_WINDOW_DRAG_AVAILABLE`]). It applies at once: no restart badge.
pub(super) fn windows_smooth_window_drag_row(app: &mut SolveApp, ui: &mut egui::Ui, available: bool) {
    if !available {
        return;
    }
    layout_check(
        ui,
        tr!(app, "prefs.windows_smooth_window_drag"),
        tr!(app, "prefs.windows_smooth_window_drag_help"),
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
        ui_kit::settings_form(ui, 0.0, |ui| {
            ui_kit::row(ui, "", |ui| {
                ui.spacing_mut().item_spacing.x = t.space_6;
                let check_width = (ui.available_width() - t.space_16 - t.space_6).max(0.0);
                let response = ui_kit::settings_control_width(ui, check_width, |ui| ui_kit::checkbox(ui, label, value).backing_on_hover_text(help));
                crate::scenario::publish_target(&format!("prefs:{key}"), response.rect);
                ui_kit::help_icon(ui, help);
            })
        });
    });
}

/// A small window specimen using the same themed card as History and Browser.
fn document_browser_preview(p: &egui::Painter, rect: egui::Rect, panel_fraction: f32) {
    let t = Tokens::get();
    let area = rect.shrink(3.0);
    let at = |x: f32, y: f32| egui::pos2(area.left() + area.width() * x, area.top() + area.height() * y);
    let block = |x: f32, y: f32, w: f32, h: f32| egui::Rect::from_min_max(at(x, y), at(x + w, y + h));
    let stroke = egui::Stroke::new(1.0, t.border);
    p.rect(area, 2.0, t.field, stroke, egui::StrokeKind::Inside);
    p.line_segment([at(0.0, 0.14), at(1.0, 0.14)], stroke);
    for x in [0.04, 0.16, 0.28] {
        p.line_segment([at(x, 0.07), at(x + 0.08, 0.07)], egui::Stroke::new(1.5, t.text_dim));
    }
    let panel = block(0.02, 0.18, panel_fraction, 0.78);
    p.rect(panel, 2.0, t.panel, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    let tile = (panel_fraction - 0.06) / 2.0;
    for row in 0..3 {
        for column in 0..2 {
            let card = block(0.04 + column as f32 * (tile + 0.02), 0.23 + row as f32 * (tile + 0.1), tile, tile + 0.04);
            p.rect(card, 1.0, t.field, stroke, egui::StrokeKind::Inside);
            p.line_segment([card.left_bottom() + egui::vec2(2.0, -3.0), card.right_bottom() + egui::vec2(-2.0, -3.0)], stroke);
        }
    }
    let view = block(panel_fraction + 0.05, 0.18, 0.93 - panel_fraction, 0.78);
    p.rect(view, 2.0, t.panel, stroke, egui::StrokeKind::Inside);
    crate::icons::paint_ink(
        p,
        egui::Rect::from_center_size(view.center(), egui::vec2(area.height() * 0.22, area.height() * 0.22)),
        "box",
        t.text_dim,
    );
}
