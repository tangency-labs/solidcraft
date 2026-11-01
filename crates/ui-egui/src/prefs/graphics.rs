//! Graphics preferences use the shared settings form and illustrative option cards.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    ui_kit::settings_heading(ui, app.tr("prefs.graphics"), app.tr("prefs.graphics_note"));
    if let Some(note) = &app.graphics_fallback {
        ui_kit::text(ui, note, ui_kit::Text::Hint("graphics_fallback"));
    }
    if app.services.graphics_path.is_some() {
        ui_kit::settings_section(ui, "graphics_card", app.tr("prefs.graphics_card"), |ui| {
            let adapters = &app.services.graphics_adapters;
            let selected = adapters.iter().position(|(name, _)| *name == app.preferences.graphics_adapter);
            let missing = !app.preferences.graphics_adapter.is_empty() && selected.is_none();
            let current = selected.and_then(|i| adapters.get(i)).map_or(app.tr("prefs.graphics_card_auto"), |(_, label)| label.as_str());
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
            ui_kit::text(ui, app.tr("prefs.graphics_card_restart"), ui_kit::Text::Hint("graphics_card_restart"));
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
        });
    }
    ui.horizontal_wrapped(|ui| {
        let response =
            ui.allocate_ui_with_layout(egui::vec2(t.form_card_width + t.space_24 * 2.0, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                let presets = ["performance", "quality", "custom"];
                let mut selected = presets.iter().position(|preset| *preset == graphics_preset(&app.preferences)).unwrap_or(2);
                let labels = [app.tr("ui.graphics_performance"), app.tr("ui.graphics_quality"), app.tr("ui.graphics_custom")];
                let changed = ui_kit::segmented(ui, &mut selected, &labels, ui_kit::Segments::default());
                if changed && let Some(preset) = presets.get(selected).filter(|preset| **preset != "custom") {
                    let _ = app.run("view.graphics_preset", json!({"preset":preset}));
                }
            });
        let rect = response.response.rect;
        crate::scenario::publish_handle("view:graphics_preset", rect.center());
        for (index, preset) in ["performance", "quality", "custom"].into_iter().enumerate() {
            crate::scenario::publish_handle(
                &format!("view:graphics_preset:{preset}"),
                egui::pos2(rect.left() + rect.width() * (index as f32 + 0.5) / 3.0, rect.center().y),
            );
        }
        response.response.on_hover_text(app.tr("ui.graphics_preset_help"));
    });
    ui_kit::text(ui, app.tr("prefs.graphics_presets_note"), ui_kit::Text::Hint("preset"));
    ui_kit::settings_section(ui, "quality", app.tr("prefs.curved_surfaces"), |ui| {
        use solidcraft_engine::doc::display_quality::DisplayQuality;
        ui_kit::text(ui, app.tr("ui.display_quality"), ui_kit::Text::Heading("display_quality"));
        let current = DisplayQuality::parse(&app.preferences.display_quality).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            for ((q, facets), note) in DisplayQuality::ALL.into_iter().zip([5, 8, 14, 64]).zip([
                "prefs.quality_fastest",
                "prefs.quality_default",
                "prefs.quality_smoother",
                "prefs.quality_slowest",
            ]) {
                if ui_kit::option_card(
                    ui,
                    crate::i18n::tr(&format!("ui.display_quality_{}", q.id())),
                    Some(app.tr(note)),
                    current == q,
                    true,
                    ui_kit::PreferenceFigure::Quality(facets),
                )
                .clicked()
                {
                    app.preferences.display_quality = q.id().into();
                }
            }
        });
        ui_kit::text(ui, app.tr("settings.display_quality_note"), ui_kit::Text::Hint("quality"));
    });
    ui_kit::settings_section(ui, "edges", app.tr("prefs.edges"), |ui| {
        ui.horizontal(|ui| {
            ui_kit::text(ui, app.tr("ui.anti_aliasing"), ui_kit::Text::Heading("antialiasing"));
            ui_kit::settings_badge(ui, app.tr("prefs.applies_on_restart"), true);
        });
        ui.horizontal_wrapped(|ui| {
            for m in MSAA {
                let enabled = app.msaa_supported.is_empty() || app.msaa_supported.contains(&m);
                let label = if m == 1 { app.tr("ui.colour_vision_off").to_owned() } else { format!("{m}×") };
                let response = ui_kit::option_card(
                    ui,
                    &label,
                    if enabled { None } else { Some(app.tr("prefs.not_on_gpu")) },
                    app.preferences.msaa == m,
                    enabled,
                    ui_kit::PreferenceFigure::Antialiasing(m),
                )
                .on_hover_text(app.tr("settings.msaa_applies_on_restart"));
                crate::scenario::publish_target(&format!("prefs:msaa:{m}"), response.rect);
                if enabled && response.clicked() {
                    app.preferences.msaa = m;
                }
            }
        });
        ui_kit::text(ui, app.tr("prefs.anti_aliasing_note"), ui_kit::Text::Hint("msaa"));
        if app.msaa_compatibility {
            ui_kit::text(ui, app.tr("prefs.anti_aliasing_amd_dx12"), ui_kit::Text::Hint("msaa_compatibility"));
        }
        ui.add_space(t.space_6);
        ui_kit::text(ui, app.tr("ui.supersampling"), ui_kit::Text::Heading("supersampling"));
        ui.horizontal_wrapped(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(t.form_card_width - t.space_24, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                let mut selected = SUPERSAMPLING.iter().position(|f| *f == app.preferences.supersampling).unwrap_or(0);
                if ui_kit::segmented(ui, &mut selected, &[app.tr("ui.colour_vision_off"), "1.5×", "2×"], ui_kit::Segments::default())
                    && let Some(factor) = SUPERSAMPLING.get(selected)
                {
                    app.preferences.supersampling = *factor;
                }
            });
            ui.add_enabled_ui(app.preferences.supersampling > 1.0, |ui| {
                setting::supersampling_adaptive(app, ui);
            });
        });
        ui_kit::text(ui, app.tr("prefs.supersampling_brief"), ui_kit::Text::Hint("supersampling"))
            .on_hover_text(app.tr("settings.supersampling_note"));
        ui.add_space(t.space_6);
        ui_kit::text(ui, app.tr_label("Ambient occlusion"), ui_kit::Text::Heading("contact_shading"));
        ui.horizontal_wrapped(|ui| {
            for (on, label) in [(false, "ui.colour_vision_off"), (true, "prefs.contact_shading")] {
                let response = ui_kit::option_card(
                    ui,
                    app.tr(label),
                    None,
                    app.preferences.ambient_occlusion == on,
                    true,
                    ui_kit::PreferenceFigure::ContactShading(on),
                );
                if on != app.preferences.ambient_occlusion {
                    crate::scenario::publish_handle("view:ambient_occlusion", response.rect.center());
                }
                if response.clicked() {
                    let _ = app.run("view.ambient_occlusion", json!({"enabled":on}));
                }
            }
        });
        ui_kit::text(ui, app.tr("prefs.contact_shading_note"), ui_kit::Text::Hint("ao"));
        let response = setting::dark_parts_shading(app, ui).on_hover_text(app.tr("prefs.dark_parts_shading_help"));
        crate::scenario::publish_handle("prefs:dark_parts_shading", response.rect.center());
    });
    ui_kit::settings_section(ui, "viewport", app.tr("prefs.viewport"), |ui| {
        let analysis_policy = setting::single_surface_analysis(app, ui).on_hover_text(app.tr("prefs.single_surface_analysis_help"));
        crate::scenario::publish_target("prefs:single_surface_analysis", analysis_policy.rect);
        if analysis_policy.changed() {
            sync_surface_analysis(app);
        }

        let response = ui_kit::checkbox(ui, app.tr("ui.cosmetic_thread_markings"), &mut app.preferences.cosmetic_threads)
            .on_hover_text(app.tr("ui.cosmetic_thread_markings_help"));
        crate::scenario::publish_handle("prefs:cosmetic_threads", response.rect.center());
        ui.columns(2, |columns| {
            if let Some(left) = columns.get_mut(0) {
                ui_kit::checkbox(left, crate::i18n::tr_label("Grid"), &mut app.ui.show_grid);
                ui_kit::checkbox(left, crate::i18n::tr_label("Perspective view"), &mut app.ui.perspective);
                left.add_enabled_ui(app.ui.perspective || app.ui.auto_ortho_faces, |ui| field_of_view_row(app, ui));
                setting::show_frame_rate(app, left).on_hover_text(app.tr("settings.frame_rate_note"));
                left.add_enabled_ui(app.preferences.show_frame_rate, |ui| {
                    ui_kit::checkbox(ui, app.tr("ui.frame_rate_details"), &mut app.preferences.frame_rate_details)
                        .on_hover_text(app.tr("settings.frame_rate_details_note"));
                });
                crate::prefs::view_angles_checkbox(app, left);
                setting::reference_geometry_through_bodies(app, left).on_hover_text(app.tr("settings.reference_geometry_through_bodies_help"));
                setting::sketch_halo(app, left);
                setting::vertex_markers(app, left);
            }
            if let Some(right) = columns.get_mut(1) {
                ui_kit::checkbox(right, crate::i18n::tr_label("Ground shadow"), &mut app.ui.ground_shadow);
                right.add_enabled_ui(app.ui.show_grid, |ui| {
                    setting::grid_follows_view(app, ui);
                });
            }
        });
        ui_kit::settings_form(ui, t.space_24 * 6.0 + t.space_6, |ui| {
            ui_kit::row(ui, "", |ui| {
                let check = setting::quick_measure(app, ui).on_hover_text(app.tr("prefs.quick_measure_note"));
                crate::scenario::publish_target("prefs:quick_measure", check.rect);
            });
            ui_kit::row(ui, "", |ui| {
                let check = setting::viewport_analysis_status(app, ui).on_hover_text(app.tr("prefs.viewport_analysis_status_help"));
                crate::scenario::publish_target("prefs:viewport_analysis_status", check.rect);
            });
            ui_kit::row(ui, "", |ui| {
                let check = setting::viewport_section_status(app, ui).on_hover_text(app.tr("prefs.viewport_section_status_help"));
                crate::scenario::publish_target("prefs:viewport_section_status", check.rect);
            });
            ui_kit::row(ui, "", |ui| {
                let check = setting::viewport_status_details(app, ui).on_hover_text(app.tr("prefs.viewport_status_details_help"));
                crate::scenario::publish_target("prefs:viewport_status_details", check.rect);
            });
            ui_kit::row(ui, "", |ui| {
                let check = setting::viewport_status_summary(app, ui).on_hover_text(app.tr("prefs.viewport_status_summary_help"));
                crate::scenario::publish_target("prefs:viewport_status_summary", check.rect);
            });
            ui_kit::row(ui, app.tr_label("Visual style"), |ui| {
                let mut selected = usize::from(app.ui.visual_style.min(3));
                ui_kit::dropdown(ui, "pref_style", &mut selected, &["Shaded with edges", "Shaded", "Wireframe", "Shaded with hidden edges"]);
                app.ui.visual_style = selected as u8;
            });
            ui_kit::row(ui, app.tr_label("Joint symbols"), |ui| {
                let mut selected = usize::from(app.preferences.joint_symbols.min(crate::dialogs_assembly::RING_AND_AXIS));
                ui_kit::dropdown(ui, "pref_joint_symbols", &mut selected, &["White on dark", "Solid accent", "Solid amber", "Ring and axis"]);
                app.preferences.joint_symbols = selected as u8;
            });
        });
        let response = setting::highlight_moving_joints(app, ui).on_hover_text(app.tr("prefs.highlight_moving_joints_help"));
        crate::scenario::publish_handle("prefs:highlight_moving_joints", response.rect.center());
    });
    #[cfg(target_os = "linux")]
    ui_kit::settings_section(ui, "window_system", app.tr("ui.window_system"), |ui| window_system_row(app, ui));
    setting::render_previous_geometry_budget(app, ui).on_hover_text(app.tr("prefs.render_previous_geometry_budget_help"));
    ui.horizontal(|ui| {
        ui_kit::text(ui, app.tr_label("Renderer"), ui_kit::Text::Inline);
        let renderer = match (&app.viewport.gpu, &app.help.gpu) {
            (Some(_), Some(g)) => {
                ui_kit::settings_badge(ui, "GPU", false);
                g.to_owned()
            }
            (Some(_), None) => "GPU".into(),
            _ => crate::i18n::tr("prefs.cpu").into(),
        };
        ui_kit::text(ui, &renderer, ui_kit::Text::Inline);
    });
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
