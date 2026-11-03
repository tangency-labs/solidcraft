//! Graphics preferences compose shared controls and canonical figures, preserving transactions.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let help_label_width = ["prefs.graphics_page.supersampling", "prefs.graphics_card", "ui.window_system"]
        .into_iter()
        .map(|id| ui_kit::ContentWidth::label(ui, app.tr(id)))
        .fold(0.0, f32::max)
        + t.space_16
        + t.space_4;
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading(ui, app.tr("prefs.graphics"), app.tr("prefs.graphics_page.subtitle"));
        ui_kit::settings_section(ui, "preset", app.tr("navbar.preset"), |ui| {
            let width = ((ui.available_width() - t.space_10 * 2.0) / 3.0).max(t.space_24 * 8.0 + t.space_12).min(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_10, t.space_10);
                for (id, label, word) in [
                    ("performance", "ui.graphics_performance", "prefs.graphics_page.fastest"),
                    ("quality", "ui.graphics_quality", "prefs.graphics_page.sharpest"),
                    ("custom", "ui.graphics_custom", "prefs.graphics_page.own_mix"),
                ] {
                    let response = ui_kit::preset_card(
                        ui,
                        app.tr(label),
                        app.tr(word),
                        graphics_preset(&app.preferences) == id,
                        true,
                        &|p, rect, ink| paint_preset_glyph(p, rect, id, ink),
                        width,
                    )
                    .on_hover_text(app.tr("ui.graphics_preset_help"));
                    crate::scenario::publish_target(&format!("view:graphics_preset:{id}"), response.rect);
                    if id == "performance" {
                        crate::scenario::publish_target("view:graphics_preset", response.rect);
                    }
                    if response.clicked() && id != "custom" {
                        let _ = app.run("view.graphics_preset", json!({"preset":id}));
                    }
                }
            });
        });
        ui_kit::settings_section(ui, "quality", app.tr("prefs.curved_surfaces"), |ui| {
            use solidcraft_engine::doc::display_quality::DisplayQuality;
            let current = DisplayQuality::parse(&app.preferences.display_quality).unwrap_or_default();
            let figures: [fn(&egui::Painter, egui::Rect); 4] =
                [ui_kit::paint_tess_coarse, ui_kit::paint_tess_normal, ui_kit::paint_tess_fine, ui_kit::paint_tess_ultra];
            let notes = ["prefs.quality_fastest", "prefs.quality_default", "prefs.quality_smoother", "prefs.quality_slowest"];
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_10, t.space_10);
                for ((q, figure), note) in DisplayQuality::ALL.into_iter().zip(figures).zip(notes) {
                    let label_id = format!("ui.display_quality_{}", q.id());
                    let label = crate::i18n::tr(&label_id);
                    let width = (t.space_24 * 5.0 + t.space_10).max(ui_kit::ContentWidth::name(ui, label) + t.space_12).min(ui.max_rect().width());
                    let response = ui_kit::picture_card(
                        ui,
                        &ui_kit::Card {
                            label,
                            sub: Some(app.tr(note)),
                            selected: current == q,
                            enabled: true,
                            options: None,
                            check: current == q,
                            aspect: 120.0 / 64.0,
                            figure: &figure,
                        },
                        width,
                    )
                    .on_hover_text(app.tr("settings.display_quality_note"));
                    crate::scenario::publish_target(&format!("prefs:display_quality:{}", q.id()), response.rect);
                    crate::scenario::publish_target(&format!("prefs:option:{label}"), response.rect);
                    if response.clicked() {
                        app.preferences.display_quality = q.id().into();
                    }
                }
            });
        });
        ui_kit::settings_section(ui, "edges", app.tr("prefs.edges"), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui_kit::text(ui, app.tr("ui.anti_aliasing"), ui_kit::Text::Heading("antialiasing"));
                ui_kit::settings_badge(ui, app.tr("prefs.graphics_page.restart"), true);
                ui_kit::help_icon(ui, app.tr("settings.msaa_applies_on_restart"));
            });
            let figures: [fn(&egui::Painter, egui::Rect); 4] = [ui_kit::paint_aa_off, ui_kit::paint_aa_2x, ui_kit::paint_aa_4x, ui_kit::paint_aa_8x];
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_10, t.space_10);
                for (m, figure) in MSAA.into_iter().zip(figures) {
                    let enabled = app.msaa_supported.is_empty() || app.msaa_supported.contains(&m);
                    let label = if m == 1 { app.tr("ui.colour_vision_off").to_owned() } else { format!("{m}×") };
                    let response = ui_kit::picture_card(
                        ui,
                        &ui_kit::Card {
                            label: &label,
                            sub: Some(if enabled { "" } else { app.tr("prefs.not_on_gpu") }),
                            selected: app.preferences.msaa == m,
                            enabled,
                            options: None,
                            check: app.preferences.msaa == m,
                            aspect: 120.0 / 64.0,
                            figure: &figure,
                        },
                        (t.space_24 * 5.0 + t.space_10).min(ui.max_rect().width()),
                    )
                    .on_hover_text(app.tr("settings.msaa_applies_on_restart"))
                    .on_disabled_hover_text(app.tr("settings.msaa_applies_on_restart"));
                    crate::scenario::publish_target(&format!("prefs:msaa:{m}"), response.rect);
                    crate::scenario::publish_target(&format!("prefs:option:{label}"), response.rect);
                    if enabled && response.clicked() {
                        app.preferences.msaa = m;
                    }
                }
            });
            if app.msaa_compatibility {
                ui_kit::text(ui, app.tr("prefs.anti_aliasing_amd_dx12"), ui_kit::Text::Hint("msaa_compatibility"));
            }
            ui_kit::settings_form(ui, help_label_width, |ui| {
                ui_kit::settings_row_help(ui, app.tr("prefs.graphics_page.supersampling"), app.tr("settings.supersampling_note"), |ui| {
                    let mut selected = SUPERSAMPLING.iter().position(|f| *f == app.preferences.supersampling).unwrap_or(0);
                    if ui_kit::segmented(
                        ui,
                        &mut selected,
                        &[app.tr("ui.colour_vision_off"), "1.5×", "2×"],
                        ui_kit::Segments {
                            targets: &["prefs:supersampling:1", "prefs:supersampling:1.5", "prefs:supersampling:2"],
                            ..Default::default()
                        },
                    ) && let Some(factor) = SUPERSAMPLING.get(selected)
                    {
                        app.preferences.supersampling = *factor;
                    }
                });
                let label = app.tr("prefs.graphics_page.supersampling");
                if let Some(rect) = crate::scenario::rect_of(&format!("dialog:{label}")) {
                    crate::scenario::publish_target("dialog:Supersampling (SSAA)", rect);
                }
                for (option, legacy) in [(app.tr("ui.colour_vision_off"), "Off"), ("1.5×", "1.5×"), ("2×", "2×")] {
                    if let Some(rect) = crate::scenario::rect_of(&format!("dialog:{label}:{option}")) {
                        crate::scenario::publish_target(&format!("dialog:Supersampling (SSAA):{legacy}"), rect);
                    }
                }
            });
            ui.add_enabled_ui(app.preferences.supersampling > 1.0, |ui| {
                let response = setting::supersampling_adaptive(app, ui);
                crate::scenario::publish_target("prefs:supersampling_adaptive", response.rect);
                crate::scenario::publish_target("dialog:Only when the view is still", response.rect);
            });
            let response = setting::dark_parts_shading(app, ui).on_hover_text(app.tr("prefs.dark_parts_shading_help"));
            crate::scenario::publish_target("prefs:dark_parts_shading", response.rect);
        });
        ui_kit::settings_section(ui, "current_view", app.tr("prefs.graphics_page.current_view"), |ui| {
            ui_kit::settings_badge(ui, app.tr("prefs.graphics_page.view_bar"), false);
            let width = ((ui.available_width() - t.space_10 * 3.0) / 4.0).max(t.space_24 * 5.0 + t.space_10).min(ui.available_width());
            let tiles: [(_, _, _, fn(&egui::Painter, egui::Rect)); 4] = [
                ("grid", "navbar.grid", app.ui.show_grid, ui_kit::paint_view_grid),
                ("ground_shadow", "ui.ground_shadow", app.ui.ground_shadow, ui_kit::paint_view_shadow),
                ("perspective", "navbar.perspective", app.ui.perspective, ui_kit::paint_view_perspective),
                ("ambient_occlusion", "ui.ambient_occlusion", app.preferences.ambient_occlusion, ui_kit::paint_ao_on),
            ];
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_10, t.space_10);
                for (id, label, selected, figure) in tiles {
                    let response = ui_kit::toggle_tile(
                        ui,
                        &ui_kit::Card {
                            label: app.tr(label),
                            sub: None,
                            selected,
                            enabled: true,
                            options: None,
                            check: false,
                            aspect: 120.0 / 64.0,
                            figure: &figure,
                        },
                        width,
                    );
                    crate::scenario::publish_target(&format!("prefs:{id}"), response.rect);
                    crate::scenario::publish_target(&format!("view:{id}"), response.rect);
                    if response.clicked() {
                        match id {
                            "grid" => app.ui.show_grid = !selected,
                            "ground_shadow" => app.ui.ground_shadow = !selected,
                            "perspective" => app.ui.perspective = !selected,
                            _ => {
                                let _ = app.run("view.ambient_occlusion", json!({"enabled":!selected}));
                            }
                        }
                    }
                }
            });
            ui_kit::settings_form(ui, t.form_label_width + t.space_24, |ui| {
                ui_kit::row(ui, app.tr_label("Visual style"), |ui| {
                    let options = crate::navigation_bar::VISUAL_STYLES.map(|(_, label)| crate::i18n::tr_label(label));
                    let choices: Vec<_> =
                        options.into_iter().zip(["style_shaded_edges", "style_shaded", "style_wireframe", "style_hidden_edges"]).collect();
                    let mut selected = usize::from(app.ui.visual_style.min(3));
                    let before = selected;
                    let response = ui
                        .allocate_ui_with_layout(
                            egui::vec2((t.space_24 * 10.0).min(ui.available_width()), t.control_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| ui_kit::dropdown_icons(ui, "pref_style", &mut selected, &choices),
                        )
                        .inner;
                    crate::scenario::publish_target("prefs:visual_style", response.rect);
                    if selected != before
                        && let Some((style, _)) = crate::navigation_bar::VISUAL_STYLES.get(selected)
                    {
                        let _ = app.run("view.visual_style", json!({"style":style}));
                    }
                });
                ui.add_enabled_ui(app.ui.perspective || app.ui.auto_ortho_faces, |ui| field_of_view_row(app, ui));
            });
        });
        ui_kit::settings_section(ui, "in_view", app.tr("prefs.graphics_page.in_view"), |ui| {
            let width = ((ui.available_width() - t.space_10 * 3.0) / 4.0).max(t.space_24 * 5.0 + t.space_10).min(ui.available_width());
            let tiles: [(_, _, _, _, fn(&egui::Painter, egui::Rect)); 4] = [
                ("show_view_angles", "ui.view_angles", "ui.view_angles_help", app.preferences.show_view_angles, ui_kit::paint_view_angles),
                (
                    "vertex_markers",
                    "Vertex markers",
                    "Show a small dot at every vertex wherever edges are drawn",
                    app.preferences.vertex_markers,
                    ui_kit::paint_vertex_markers,
                ),
                (
                    "quick_measure",
                    "prefs.graphics_page.quick_measure",
                    "prefs.quick_measure_note",
                    app.preferences.quick_measure,
                    ui_kit::paint_quick_measure,
                ),
                ("show_frame_rate", "ui.frame_rate", "settings.frame_rate_note", app.preferences.show_frame_rate, ui_kit::paint_frame_rate),
            ];
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.space_10, t.space_10);
                for (id, label, help, selected, figure) in tiles {
                    let label = if id == "vertex_markers" { app.tr_label(label) } else { app.tr(label) };
                    let help = if id == "vertex_markers" { app.tr_label(help) } else { app.tr(help) };
                    let response = ui_kit::toggle_tile(
                        ui,
                        &ui_kit::Card {
                            label,
                            sub: None,
                            selected,
                            enabled: true,
                            options: None,
                            check: false,
                            aspect: 120.0 / 64.0,
                            figure: &figure,
                        },
                        width,
                    )
                    .on_hover_text(help);
                    crate::scenario::publish_target(&format!("prefs:{id}"), response.rect);
                    crate::scenario::publish_target(&format!("dialog:{label}"), response.rect);
                    if id == "quick_measure" {
                        crate::scenario::publish_target(&format!("dialog:{}", app.tr("prefs.quick_measure")), response.rect);
                    }
                    if id == "show_view_angles" {
                        crate::scenario::publish_target("view:view_angles", response.rect);
                    }
                    if response.clicked() {
                        match id {
                            "show_view_angles" => {
                                let _ = app.run("view.angle_readout", json!({"enabled":!selected}));
                            }
                            "vertex_markers" => app.preferences.vertex_markers = !selected,
                            "quick_measure" => app.preferences.quick_measure = !selected,
                            _ => app.preferences.show_frame_rate = !selected,
                        }
                    }
                }
            });
            ui.add_enabled_ui(app.preferences.show_frame_rate, |ui| {
                let response = ui_kit::checkbox(ui, app.tr("ui.frame_rate_details"), &mut app.preferences.frame_rate_details)
                    .on_hover_text(app.tr("settings.frame_rate_details_note"));
                crate::scenario::publish_target("prefs:frame_rate_details", response.rect);
            });
            setting::profile_operations(app, ui).on_hover_text(app.tr("prefs.profile_operations_note"));
            ui.horizontal_wrapped(|ui| {
                ui_kit::settings_inline_note(ui, app.tr("prefs.graphics_page.status_chips"));
                for (id, label, help) in [
                    ("viewport_analysis_status", "ui.analysis", "prefs.viewport_analysis_status_help"),
                    ("viewport_section_status", "Section", "prefs.viewport_section_status_help"),
                    ("viewport_status_details", "prefs.graphics_page.live_details", "prefs.viewport_status_details_help"),
                    ("viewport_status_summary", "prefs.graphics_page.fold_items", "prefs.viewport_status_summary_help"),
                ] {
                    let value = match id {
                        "viewport_analysis_status" => &mut app.preferences.viewport_analysis_status,
                        "viewport_section_status" => &mut app.preferences.viewport_section_status,
                        "viewport_status_details" => &mut app.preferences.viewport_status_details,
                        _ => &mut app.preferences.viewport_status_summary,
                    };
                    let response = ui_kit::choice_chip(
                        ui,
                        id,
                        if id == "viewport_section_status" { crate::i18n::tr_label(label) } else { crate::i18n::tr(label) },
                        *value,
                    )
                    .on_hover_text(crate::i18n::tr(help));
                    crate::scenario::publish_target(&format!("prefs:{id}"), response.rect);
                    if response.clicked() {
                        *value = !*value;
                    }
                }
            });
        });
        ui_kit::settings_section(ui, "surfaces_edges", app.tr("prefs.graphics_page.surfaces_edges"), |ui| {
            let response = ui_kit::checkbox(ui, app.tr("ui.cosmetic_thread_markings"), &mut app.preferences.cosmetic_threads)
                .on_hover_text(app.tr("ui.cosmetic_thread_markings_help"));
            crate::scenario::publish_target("prefs:cosmetic_threads", response.rect);
            setting::sketch_halo(app, ui);
            ui_kit::row(ui, app.tr("prefs.section_hatch"), |ui| {
                let styles = [crate::prefs::SectionHatch::One, crate::prefs::SectionHatch::ByPart];
                let mut selected = styles.iter().position(|s| *s == app.preferences.section_hatch).unwrap_or(1);
                if ui_kit::segmented(
                    ui,
                    &mut selected,
                    &[app.tr("prefs.section_hatch_one"), app.tr("prefs.section_hatch_by_part")],
                    Default::default(),
                ) && let Some(style) = styles.get(selected)
                {
                    app.preferences.section_hatch = *style;
                }
            });
            let tint_new = setting::preview_tint_new_faces(app, ui).on_hover_text(app.tr("prefs.preview_tint_new_faces_help"));
            crate::scenario::publish_target("prefs:preview_tint_new_faces", tint_new.rect);
            let cut_tint = setting::cut_preview_tint(app, ui).on_hover_text(app.tr("prefs.cut_preview_tint_help"));
            crate::scenario::publish_target("prefs:cut_preview_tint", cut_tint.rect);
            let reference =
                setting::reference_geometry_through_bodies(app, ui).on_hover_text(app.tr("settings.reference_geometry_through_bodies_help"));
            crate::scenario::publish_target("dialog:Show reference geometry through bodies", reference.rect);
            ui.add_enabled_ui(app.ui.show_grid, |ui| {
                setting::grid_follows_view(app, ui).on_hover_text(app.tr("ui.grid_follows_view_hint"));
            });
            let response = setting::single_surface_analysis(app, ui).on_hover_text(app.tr("prefs.single_surface_analysis_help"));
            crate::scenario::publish_target("prefs:single_surface_analysis", response.rect);
            crate::scenario::publish_target("dialog:Use a single surface analysis", response.rect);
            if response.changed() {
                sync_surface_analysis(app);
            }
        });
        ui_kit::settings_section(ui, "computer", app.tr("prefs.general.this_computer"), |ui| {
            computer_card(ui, |ui| {
                computer_header(app, ui);
                if let Some(note) = &app.graphics_fallback {
                    ui_kit::text(ui, note, ui_kit::Text::Hint("graphics_fallback"));
                }
                if app.services.graphics_path.is_some() {
                    graphics_card_row(app, ui, help_label_width);
                }
                #[cfg(target_os = "linux")]
                window_system_row(app, ui, help_label_width);
                setting::render_previous_geometry_budget(app, ui).on_hover_text(app.tr("prefs.render_previous_geometry_budget_help"));
            });
        });
    });
}

/// One stroked glyph of a preset card on the design's 24 x 24 grid: a bolt, a gem or three sliders.
fn paint_preset_glyph(p: &egui::Painter, rect: egui::Rect, preset: &str, ink: egui::Color32) {
    let s = rect.width().min(rect.height()) / 24.0;
    let at = |x: f32, y: f32| rect.center() + egui::vec2(x - 12.0, y - 12.0) * s;
    let stroke = egui::Stroke::new(1.9 * s, ink);
    let line = |points: &[(f32, f32)], closed: bool| {
        let points: Vec<egui::Pos2> = points.iter().map(|&(x, y)| at(x, y)).collect();
        p.add(if closed { egui::epaint::PathShape::closed_line(points, stroke) } else { egui::epaint::PathShape::line(points, stroke) });
    };
    match preset {
        "performance" => line(&[(13.0, 3.0), (5.0, 13.0), (11.0, 13.0), (10.0, 21.0), (18.0, 11.0), (12.0, 11.0)], true),
        "quality" => {
            line(&[(6.0, 4.0), (18.0, 4.0), (21.0, 9.0), (12.0, 20.0), (3.0, 9.0)], true);
            line(&[(3.0, 9.0), (21.0, 9.0)], false);
            line(&[(9.0, 4.0), (12.0, 20.0), (15.0, 4.0)], false);
        }
        _ => {
            for (y, knob) in [(6.0, 9.0), (12.0, 15.0), (18.0, 8.0)] {
                line(&[(5.0, y), (19.0, y)], false);
                p.circle_filled(at(knob, y), 2.0 * s, ink);
            }
        }
    }
}

/// The monitor on the device tile, on the same grid.
fn paint_monitor(p: &egui::Painter, rect: egui::Rect, ink: egui::Color32) {
    let s = rect.width().min(rect.height()) / 24.0;
    let at = |x: f32, y: f32| rect.center() + egui::vec2(x - 12.0, y - 12.0) * s;
    let stroke = egui::Stroke::new(1.8 * s, ink);
    p.rect_stroke(egui::Rect::from_two_pos(at(3.0, 4.0), at(21.0, 16.0)), 2.0 * s, stroke, egui::StrokeKind::Middle);
    p.line_segment([at(8.0, 20.0), at(16.0, 20.0)], stroke);
    p.line_segment([at(12.0, 16.0), at(12.0, 20.0)], stroke);
}

/// The section's card: one grey surface holding the device, its pickers and its checkbox. A search
/// shows the rows without it, like every other card.
fn computer_card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let t = Tokens::get();
    if crate::prefs_search::suppress_chrome(ui) {
        add(ui);
        return;
    }
    egui::Frame::NONE
        .fill(t.pref_well)
        .stroke(egui::Stroke::new(1.0, t.dialog_border))
        .corner_radius(f32::from(t.radius_lg) + t.space_2)
        .inner_margin(egui::Margin::same(t.space_14 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = t.space_12;
            add(ui);
        });
}

/// The device line: a tile with a monitor, the adapter's name, its backend and renderer, and a badge.
fn computer_header(app: &SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let adapter = app.viewport.adapter.as_str();
    let (backend, name) = adapter.split_once(" · ").unwrap_or(("", adapter));
    let renderer_kind = if app.viewport.gpu.is_some() { "GPU" } else { app.tr("prefs.cpu") };
    let renderer_details = match (&app.viewport.gpu, &app.help.gpu) {
        (Some(_), Some(g)) => g.as_str(),
        _ => renderer_kind,
    };
    let renderer = format!("{renderer_kind} {}", app.tr_label("Renderer").to_lowercase());
    // Without an adapter name (a software renderer reports none) the title is the renderer itself.
    let (name, line) = if name.is_empty() {
        (app.tr_label("Renderer"), renderer_kind.to_owned())
    } else if backend.is_empty() {
        (name, renderer)
    } else {
        (name, format!("{backend} · {renderer}"))
    };
    let tile = t.space_24 + t.space_12;
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), tile), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = t.space_12;
        ui_kit::settings_badge(ui, renderer_kind, app.viewport.gpu.is_none());
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(tile, tile), egui::Sense::hover());
            ui.painter().rect_filled(rect, f32::from(t.radius_lg) + t.space_2 * 0.5, t.selected_bg);
            paint_monitor(ui.painter(), egui::Rect::from_center_size(rect.center(), rect.size() * 0.56), t.form_primary);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui_kit::text(ui, name, ui_kit::Text::NameCell(true)).on_hover_text(adapter);
                ui.label(egui::RichText::new(line).monospace().size(t.font_small).color(t.form_dim)).on_hover_text(renderer_details);
            });
        });
    });
}

/// The saved adapter picker retains validation, persistence and all fallback notices.
fn graphics_card_row(app: &mut SolveApp, ui: &mut egui::Ui, label_width: f32) {
    let t = Tokens::get();
    let selected = app.services.graphics_adapters.iter().position(|(name, _)| *name == app.preferences.graphics_adapter);
    let missing = !app.preferences.graphics_adapter.is_empty() && selected.is_none();
    let row_width = (label_width + t.space_14 + t.space_24 * 10.0).min(ui.available_width());
    ui.horizontal_wrapped(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(row_width, t.control_height), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui_kit::settings_form(ui, label_width, |ui| {
                ui_kit::settings_row_help(ui, app.tr("prefs.graphics_card"), app.tr("prefs.graphics_card_restart"), |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2((t.space_24 * 10.0).min(ui.available_width()), t.control_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            let adapters = &app.services.graphics_adapters;
                            let selected = adapters.iter().position(|(name, _)| *name == app.preferences.graphics_adapter);
                            let current =
                                selected.and_then(|i| adapters.get(i)).map_or(app.tr("prefs.graphics_card_auto"), |(_, label)| label.as_str());
                            let mut requested = None;
                            let response = ui_kit::dropdown_menu(ui, "graphics_card", current, |ui| {
                                if ui_kit::list_picker(ui, "graphics_card:auto", app.tr("prefs.graphics_card_auto"), selected.is_none()).clicked() {
                                    requested = Some(String::new());
                                }
                                for (index, (name, label)) in adapters.iter().enumerate() {
                                    if ui_kit::list_picker(ui, &format!("graphics_card:{index}"), label, selected == Some(index)).clicked() {
                                        requested = Some(name.clone());
                                    }
                                }
                            });
                            crate::scenario::publish_target("prefs:graphics_card", response.rect);
                            if let Some(name) = requested {
                                let _ = super::set(app, &json!({"graphics_adapter": name}));
                            }
                        },
                    );
                });
            });
        });
        ui_kit::settings_badge(ui, app.tr("prefs.graphics_page.restart"), true);
    });
    if missing {
        ui_kit::text(ui, app.tr("prefs.graphics_card_missing"), ui_kit::Text::Hint("graphics_card_missing"));
    } else if !app.preferences.graphics_adapter.is_empty() {
        // Status / Help show the adapter actually in use; the dropdown shows the saved
        // preference. When they differ the saved card was not applied (#8341).
        let active = app.viewport.adapter.rsplit(" · ").next().unwrap_or("");
        if !active.is_empty() && !active.eq_ignore_ascii_case(&app.preferences.graphics_adapter) {
            ui_kit::text(ui, app.tr("prefs.graphics_card_unused"), ui_kit::Text::Hint("graphics_card_unused"));
        }
    }
}
/// Perspective view's Field of view (#5364): the kit slider in degrees, typed or dragged, kept to
/// whole degrees and written back in radians. No step on the slider itself: it would round the
/// default 0.6 (34.4°) just by being drawn, so an untouched angle stays exactly as saved.
fn field_of_view_row(app: &mut SolveApp, ui: &mut egui::Ui) {
    let mut degrees = perspective_fov(&app.preferences).to_degrees();
    ui_kit::row(ui, app.tr("prefs.perspective_fov"), |ui| {
        let response = ui_kit::slider(ui, &mut degrees, 15.0..=90.0, ui_kit::Slider { format: |v| format!("{v:.0}°"), ..Default::default() });
        if response.changed() {
            app.preferences.perspective_fov = degrees.round().to_radians();
        }
        response
    });
}
