//! Shared numerical editors for bounded camera sampling and optical transport.
use crate::{SolveApp, ui_kit};
use solidcraft_engine::render::job::adaptive::MAX_PILOT_SAMPLES;

/// Pilot allocation and conditional transmission sampling share portable intent.
#[cfg(test)]
pub(super) fn sampling_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    adaptive_controls(app, ui);
    glass_controls(app, ui);
}
pub(super) fn adaptive_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    sampler_controls(app, ui);
    let label = app.tr("render.texture_filtering").to_string();
    let response = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.texture_filtering);
    crate::scenario::publish_target("render:texture-filtering", response.rect);
    app.render_workspace.view.dirty |= response.changed();
    ui_kit::text(ui, app.tr("render.texture_filtering_help"), ui_kit::Text::Note("render.texture_filtering_help"));
    let label = app.tr("render.metal_energy_compensation").to_string();
    let response = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.metal_energy_compensation);
    crate::scenario::publish_target("render:metal-energy-compensation", response.rect);
    app.render_workspace.view.dirty |= response.changed();
    ui_kit::text(ui, app.tr("render.metal_energy_compensation_help"), ui_kit::Text::Note("render.metal_energy_compensation_help"));
    let before = app.render_workspace.view.studio.adaptive;
    let mut enabled = before.is_some();
    let response = ui_kit::checkbox(ui, app.tr("render.adaptive_enable"), &mut enabled);
    crate::scenario::publish_target("render:adaptive-enable", response.rect);
    if response.changed() {
        app.render_workspace.view.studio.adaptive = enabled.then(|| {
            let mut options = solidcraft_engine::render::job::adaptive::AdaptiveSampling::default();
            if app.render_workspace.view.method == super::ImageMethod::CpuRaytrace {
                // Explicitly enabling the option does not silently raise the saved
                // production cap, change the image size or choose a larger profile.
                options.pilot_samples = 16;
                options.min_samples = 16;
            }
            options
        });
    }
    let mut options = app.render_workspace.view.studio.adaptive.unwrap_or_default();
    if !enabled {
        ui_kit::text(ui, app.tr("render.children.enable_adaptive"), ui_kit::Text::Note("render.children.enable_adaptive"));
    }
    ui.add_enabled_ui(enabled, |ui| {
        for key in [
            "render.adaptive_pilots",
            "render.adaptive_minimum",
            "render.convergence.optical_minimum",
            "render.adaptive_relative",
            "render.adaptive_absolute",
            "render.adaptive_coverage",
        ] {
            let label = match key {
                "render.adaptive_pilots" => "render.children.pilots",
                "render.adaptive_minimum" => "render.children.minimum",
                "render.convergence.optical_minimum" => "render.convergence.optical_minimum",
                "render.adaptive_relative" => "render.children.relative_error",
                "render.adaptive_absolute" => "render.children.absolute_floor",
                _ => "render.children.coverage_error",
            };
            let response = ui_kit::row(ui, app.tr(label), |ui| match key {
                "render.adaptive_pilots" => ui_kit::number_field_preserving(ui, &mut options.pilot_samples, 16..=MAX_PILOT_SAMPLES, 1.0, ""),
                "render.adaptive_minimum" => {
                    ui_kit::number_field_preserving(ui, &mut options.min_samples, 16..=super::quality::MAX_UI_SAMPLES, 1.0, "")
                }
                "render.convergence.optical_minimum" => {
                    ui_kit::number_field_preserving(ui, &mut options.optical_min_samples, 0..=super::quality::MAX_UI_SAMPLES, 16.0, "")
                }
                "render.adaptive_relative" => ui_kit::number_field(ui, &mut options.relative_error, 0.0..=1.0, 0.005, ""),
                "render.adaptive_absolute" => ui_kit::number_field(ui, &mut options.absolute_error, 0.000001..=1e6, 0.001, ""),
                _ => ui_kit::number_field(ui, &mut options.coverage_error, 0.001..=0.25, 0.001, ""),
            });
            crate::scenario::publish_target(key, response.rect);
        }
        pilot_evidence_controls(app, ui, &mut options);
    });
    if enabled {
        app.render_workspace.view.studio.adaptive = Some(options);
    }
    ui_kit::text(ui, app.tr("render.adaptive_help"), ui_kit::Text::Note("render.adaptive_help"));
    if enabled {
        ui_kit::text(ui, app.tr("render.convergence.optical_minimum_help"), ui_kit::Text::Note("render.convergence.optical_minimum_help"));
    }
    app.render_workspace.view.dirty |= before != app.render_workspace.view.studio.adaptive;
}
fn pilot_evidence_controls(app: &SolveApp, ui: &mut egui::Ui, options: &mut solidcraft_engine::render::job::adaptive::AdaptiveSampling) {
    let can_trace = app.render_workspace.view.method == super::ImageMethod::GpuRaytrace || app.render_workspace.view.raytrace.path.is_some();
    ui.add_enabled_ui(can_trace || options.component_uncertainty, |ui| {
        let response = ui_kit::checkbox(ui, app.tr("render.convergence.component_pilots"), &mut options.component_uncertainty);
        crate::scenario::publish_target("render:convergence:component-pilots", response.rect);
    });
    ui_kit::text(ui, app.tr("render.convergence.component_pilots_help"), ui_kit::Text::Note("render.convergence.component_pilots_help"));
    let can_assess = can_trace && app.render_workspace.view.studio.caustics.is_some();
    let selected = options.map_validation_batches;
    let label = if selected == 0 { app.tr("render.convergence.map_assessment_off").to_string() } else { selected.to_string() };
    ui_kit::row(ui, app.tr("render.convergence.map_assessment"), |ui| {
        ui.add_enabled_ui(can_assess || selected > 0, |ui| {
            let response = ui_kit::dropdown_menu(ui, "render_map_assessment", &label, |ui| {
                for batches in [0, 2, 4, 8] {
                    let allowed =
                        batches == 0 || (can_assess && options.pilot_samples.is_multiple_of(batches) && options.pilot_samples / batches >= 2);
                    let label = if batches == 0 { app.tr("render.convergence.map_assessment_off").to_string() } else { batches.to_string() };
                    let target = format!("render:convergence:map-batches:{batches}");
                    let response = ui.add_enabled_ui(allowed, |ui| ui_kit::list_picker(ui, &target, &label, selected == batches));
                    crate::scenario::publish_target(&target, response.inner.rect);
                    if response.inner.clicked() {
                        options.map_validation_batches = batches;
                        ui.close();
                    }
                }
            });
            crate::scenario::publish_target("render:convergence:map-assessment", response.rect);
        });
    });
    ui_kit::text(ui, app.tr("render.convergence.map_assessment_help"), ui_kit::Text::Note("render.convergence.map_assessment_help"));
}

fn sampler_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    use solidcraft_engine::render::sampling::Sampler;
    let before = (app.render_workspace.view.studio.sampler, app.render_workspace.view.studio.radiance_components);
    let can_sample = app.render_workspace.view.method == super::ImageMethod::GpuRaytrace || app.render_workspace.view.raytrace.progressive.is_some();
    let choices = [(Sampler::Independent, "render.convergence.independent"), (Sampler::ScrambledSobol, "render.convergence.sobol")];
    let selected = app.render_workspace.view.studio.sampler;
    let selected_key = if selected.is_independent() { choices[0].1 } else { choices[1].1 };
    let label = app.tr(selected_key).to_string();
    ui_kit::row(ui, app.tr("render.convergence.sampler"), |ui| {
        ui.add_enabled_ui(can_sample || !selected.is_independent(), |ui| {
            let response = ui_kit::dropdown_menu(ui, "render_sampling_pattern", &label, |ui| {
                for (sampler, key) in choices {
                    let response = ui_kit::list_picker(ui, key, app.tr(key), sampler == selected);
                    crate::scenario::publish_target(key, response.rect);
                    if response.clicked() {
                        app.render_workspace.view.studio.sampler = sampler;
                        ui.close();
                    }
                }
            });
            crate::scenario::publish_target("render:convergence:sampler", response.rect);
        });
    });
    ui_kit::text(ui, app.tr("render.convergence.sampler_help"), ui_kit::Text::Note("render.convergence.sampler_help"));
    let can_record =
        app.render_workspace.view.method == super::ImageMethod::GpuRaytrace || (can_sample && app.render_workspace.view.raytrace.path.is_some());
    let label = app.tr("render.convergence.components").to_string();
    let components_required = app.render_workspace.view.studio.denoise_settings.component_filtering;
    ui.add_enabled_ui((can_record || app.render_workspace.view.studio.radiance_components) && !components_required, |ui| {
        let response = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.radiance_components);
        crate::scenario::publish_target("render:convergence:components", response.rect);
    });
    ui_kit::text(ui, app.tr("render.convergence.components_help"), ui_kit::Text::Note("render.convergence.components_help"));
    let after = &app.render_workspace.view.studio;
    app.render_workspace.view.dirty |= before != (after.sampler, after.radiance_components);
}

pub(super) fn glass_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    caustics_controls(app, ui);
    if app.render_workspace.view.method == super::ImageMethod::CpuRaytrace {
        cpu_glass_controls(app, ui);
        return;
    }
    let before = (
        app.render_workspace.view.studio.nested_media,
        app.render_workspace.view.studio.transmission_samples,
        app.render_workspace.view.studio.split_first_interface,
    );
    let mut strata = match before.1 {
        2 => 1,
        4 => 2,
        _ => 0,
    };
    ui.add_enabled_ui(!before.2, |ui| {
        ui_kit::row(ui, app.tr("render.children.fresnel"), |ui| {
            if ui_kit::segmented(
                ui,
                &mut strata,
                &["1", "2", "4"],
                ui_kit::Segments { targets: &["render:gpu-fresnel:1", "render:gpu-fresnel:2", "render:gpu-fresnel:4"], ..Default::default() },
            ) {
                app.render_workspace.view.studio.transmission_samples = match strata {
                    1 => 2,
                    2 => 4,
                    _ => 1,
                };
            }
        });
    });
    let studio = &mut app.render_workspace.view.studio;
    first_split_controls(ui, &mut studio.split_first_interface, studio.transmission_samples);
    let label = app.tr("render.children.nested").to_string();
    let response = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.nested_media);
    crate::scenario::publish_target("render:nested-enable", response.rect);
    ui_kit::text(ui, app.tr("render.nested_help"), ui_kit::Text::Note("render.nested_help"));
    let studio = &app.render_workspace.view.studio;
    app.render_workspace.view.dirty |= before != (studio.nested_media, studio.transmission_samples, studio.split_first_interface);
}

/// Splitting replaces the first Fresnel decision; it cannot be combined with
/// conditional strata. An authored incompatible value can always be disabled.
fn first_split_controls(ui: &mut egui::Ui, split: &mut bool, strata: u32) {
    let response = ui
        .add_enabled_ui(strata == 1 || *split, |ui| ui_kit::checkbox(ui, solidcraft_ui_text::tr("render.convergence.split_first_interface"), split));
    crate::scenario::publish_target("render:convergence:split-first-interface", response.inner.rect);
    ui_kit::text(
        ui,
        solidcraft_ui_text::tr("render.convergence.split_first_interface_help"),
        ui_kit::Text::Note("render.convergence.split_first_interface_help"),
    );
}

fn caustics_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let before = app.render_workspace.view.studio.caustics;
    let mut enabled = before.is_some();
    let can_enable = app.render_workspace.view.method == super::ImageMethod::GpuRaytrace
        || (app.render_workspace.view.method == super::ImageMethod::CpuRaytrace && app.render_workspace.view.raytrace.path.is_some());
    let response = ui.add_enabled_ui(enabled || can_enable, |ui| ui_kit::checkbox(ui, app.tr("render.caustics_enable"), &mut enabled));
    crate::scenario::publish_target("render:caustics:enable", response.inner.rect);
    if response.inner.changed() {
        app.render_workspace.view.studio.caustics = enabled.then(solidcraft_engine::render::job::CausticOptions::default);
    }
    if let Some(options) = &mut app.render_workspace.view.studio.caustics {
        let area_lights = solidcraft_ui_text::tr("render.caustics_area_lights");
        let response = ui_kit::checkbox(ui, area_lights, &mut options.area_lights);
        crate::scenario::publish_target("render:caustics:area-lights", response.rect);
        let environment = solidcraft_ui_text::tr("render.caustics_environment");
        let response = ui_kit::checkbox(ui, environment, &mut options.environment);
        crate::scenario::publish_target("render:caustics:environment", response.rect);
        let mut glossy = options.glossy_continuation_max_roughness.is_some();
        let response = ui.add_enabled_ui(glossy || options.area_lights || options.environment, |ui| {
            ui_kit::checkbox(ui, solidcraft_ui_text::tr("render.convergence.glossy_continuation"), &mut glossy)
        });
        crate::scenario::publish_target("render:caustics:glossy-continuation", response.inner.rect);
        if response.inner.changed() {
            options.glossy_continuation_max_roughness = glossy.then_some(0.35);
        }
        if let Some(roughness) = &mut options.glossy_continuation_max_roughness {
            ui_kit::row(ui, solidcraft_ui_text::tr("render.convergence.glossy_roughness"), |ui| {
                let field = ui_kit::number_field_preserving(ui, roughness, 0.0..=1.0, 0.01, "");
                crate::scenario::publish_target("render:caustics:glossy-roughness", field.rect);
                field
            });
        }
        ui_kit::text(
            ui,
            solidcraft_ui_text::tr("render.convergence.glossy_continuation_help"),
            ui_kit::Text::Note("render.convergence.glossy_continuation_help"),
        );
        let refresh_label = solidcraft_ui_text::tr("render.convergence.refresh_photons");
        let response = ui.add_enabled_ui(!options.progressive_radius, |ui| ui_kit::checkbox(ui, refresh_label, &mut options.refresh_per_pass));
        crate::scenario::publish_target("render:caustics:refresh", response.inner.rect);
        let progressive_label = solidcraft_ui_text::tr("render.convergence.progressive_radius");
        let response = ui.add_enabled_ui(options.refresh_per_pass || options.progressive_radius, |ui| {
            ui_kit::checkbox(ui, progressive_label, &mut options.progressive_radius)
        });
        crate::scenario::publish_target("render:caustics:progressive-radius", response.inner.rect);
        ui_kit::text(ui, solidcraft_ui_text::tr("render.convergence.progressive_help"), ui_kit::Text::Note("render.convergence.progressive_help"));
        let photons = solidcraft_ui_text::tr("render.caustics_photons");
        ui_kit::row(ui, photons, |ui| {
            let field = ui_kit::number_field_preserving(ui, &mut options.photons, 1..=solidcraft_engine::render::job::MAX_CAUSTIC_PHOTONS, 64.0, "");
            crate::scenario::publish_target("render:caustics:photons", field.rect);
            field
        });
        let radius = solidcraft_ui_text::tr("render.caustics_radius");
        ui_kit::row(ui, radius, |ui| {
            let field = ui_kit::number_field_preserving(ui, &mut options.radius_mm, 1e-6..=1e6, 0.1, " mm");
            crate::scenario::publish_target("render:caustics:radius", field.rect);
            field
        });
        let seed = solidcraft_ui_text::tr("render.children.seed");
        ui_kit::row(ui, seed, |ui| {
            let field = ui_kit::number_field_preserving(ui, &mut options.seed, 0..=u32::MAX, 1.0, "");
            crate::scenario::publish_target("render:caustics:seed", field.rect);
            field
        });
        let depth = solidcraft_ui_text::tr("render.caustics_depth");
        ui_kit::row(ui, depth, |ui| {
            let field = ui_kit::number_field_preserving(ui, &mut options.max_depth, 1..=solidcraft_engine::render::job::MAX_PATH_DEPTH, 1.0, "");
            crate::scenario::publish_target("render:caustics:depth", field.rect);
            field
        });
    }
    ui_kit::text(ui, app.tr("render.caustics_help"), ui_kit::Text::Note("render.caustics_help"));
    app.render_workspace.view.dirty |= before != app.render_workspace.view.studio.caustics;
}

/// CPU strata stay with path settings; the legacy direct producer has no lanes.
/// Every group contributes one camera observation to adaptive and film moments.
fn cpu_glass_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::text(ui, app.tr("render.material_cpu_optics_limits"), ui_kit::Text::Note("render.cpu_glass_limits"));
    if app.render_workspace.view.raytrace.progressive.is_none() {
        let reason = super::cpu::progressive_adoption_reason(app);
        let button = ui_kit::button(ui, app.tr("render.cpu_progressive_adopt"), ui_kit::Button::Enabled(reason.is_none()));
        crate::scenario::publish_target("render:cpu-glass-adopt-progressive", button.rect);
        if button.clicked()
            && let Err(error) = super::cpu::adopt_progressive(app)
        {
            app.set_status(error, true);
        }
        if let Some(reason) = reason {
            ui_kit::text(ui, &reason, ui_kit::Text::Note("render.cpu_glass_adoption_reason"));
        }
    }
    if let Some(mut path) = app.render_workspace.view.raytrace.path {
        let before = path;
        ui_kit::row(ui, app.tr("render.cpu_path_depth"), |ui| ui_kit::text(ui, &path.max_depth.to_string(), ui_kit::Text::Inline));
        let mut strata = match path.transmission_samples {
            2 => 1,
            4 => 2,
            _ => 0,
        };
        ui.add_enabled_ui(!path.split_first_interface, |ui| {
            ui_kit::row(ui, app.tr("render.children.fresnel"), |ui| {
                if ui_kit::segmented(
                    ui,
                    &mut strata,
                    &["1", "2", "4"],
                    ui_kit::Segments { targets: &["render:cpu-fresnel:1", "render:cpu-fresnel:2", "render:cpu-fresnel:4"], ..Default::default() },
                ) {
                    path.transmission_samples = match strata {
                        1 => 2,
                        2 => 4,
                        _ => 1,
                    };
                }
            });
        });
        ui_kit::text(ui, app.tr("render.convergence.fresnel_help"), ui_kit::Text::Note("render.convergence.fresnel_help"));
        first_split_controls(ui, &mut path.split_first_interface, path.transmission_samples);
        let response = ui_kit::checkbox(ui, app.tr("render.convergence.roulette"), &mut path.russian_roulette);
        crate::scenario::publish_target("render:cpu-roulette", response.rect);
        ui_kit::text(ui, app.tr("render.convergence.roulette_help"), ui_kit::Text::Note("render.convergence.roulette_help"));
        if before != path {
            app.render_workspace.view.raytrace.path = Some(path);
            app.render_workspace.view.studio.transmission_samples = path.transmission_samples;
            app.render_workspace.view.studio.split_first_interface = path.split_first_interface;
            app.render_workspace.view.dirty = true;
        }
    } else {
        let reason = super::cpu::path_reason(app);
        let button = ui_kit::button(ui, app.tr("render.cpu_glass_enable_path"), ui_kit::Button::Enabled(reason.is_none()));
        crate::scenario::publish_target("render:cpu-glass-enable-path", button.rect);
        if button.clicked() {
            app.render_workspace.view.raytrace.path = Some(solidcraft_engine::render::job::CpuPathOptions {
                transmission_samples: app.render_workspace.view.studio.transmission_samples,
                split_first_interface: app.render_workspace.view.studio.split_first_interface,
                ..Default::default()
            });
            app.render_workspace.view.dirty = true;
        }
        if let Some(reason) = reason {
            ui_kit::text(ui, &reason, ui_kit::Text::Note("render.cpu_glass_path_reason"));
        }
    }
    ui_kit::text(ui, app.tr("render.cpu_path_help"), ui_kit::Text::Note("render.cpu_glass_path_help"));
}
