//! Shared numerical editors for bounded camera sampling and GPU transport extensions.
use crate::{SolveApp, ui_kit};

/// Pilot allocation and conditional transmission sampling share portable intent.
#[cfg(test)]
pub(super) fn sampling_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    adaptive_controls(app, ui);
    glass_controls(app, ui);
}
pub(super) fn adaptive_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
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
        for key in
            ["render.adaptive_pilots", "render.adaptive_minimum", "render.adaptive_relative", "render.adaptive_absolute", "render.adaptive_coverage"]
        {
            let label = match key {
                "render.adaptive_pilots" => "render.children.pilots",
                "render.adaptive_minimum" => "render.children.minimum",
                "render.adaptive_relative" => "render.children.relative_error",
                "render.adaptive_absolute" => "render.children.absolute_floor",
                _ => "render.children.coverage_error",
            };
            let response = ui_kit::row(ui, app.tr(label), |ui| match key {
                "render.adaptive_pilots" => ui_kit::number_field(ui, &mut options.pilot_samples, 16..=256, 1.0, ""),
                "render.adaptive_minimum" => ui_kit::number_field(ui, &mut options.min_samples, 16..=4096, 1.0, ""),
                "render.adaptive_relative" => ui_kit::number_field(ui, &mut options.relative_error, 0.0..=1.0, 0.005, ""),
                "render.adaptive_absolute" => ui_kit::number_field(ui, &mut options.absolute_error, 0.000001..=1e6, 0.001, ""),
                _ => ui_kit::number_field(ui, &mut options.coverage_error, 0.001..=0.25, 0.001, ""),
            });
            crate::scenario::publish_target(key, response.rect);
        }
    });
    if enabled {
        app.render_workspace.view.studio.adaptive = Some(options);
    }
    ui_kit::text(ui, app.tr("render.adaptive_help"), ui_kit::Text::Note("render.adaptive_help"));
    app.render_workspace.view.dirty |= before != app.render_workspace.view.studio.adaptive;
}
pub(super) fn glass_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    caustics_controls(app, ui);
    if app.render_workspace.view.method == super::ImageMethod::CpuRaytrace {
        cpu_glass_controls(app, ui);
        return;
    }
    let before = (app.render_workspace.view.studio.nested_media, app.render_workspace.view.studio.transmission_samples);
    let mut strata = match before.1 {
        2 => 1,
        4 => 2,
        _ => 0,
    };
    ui_kit::row(ui, app.tr("render.children.fresnel"), |ui| {
        if ui_kit::segmented(ui, &mut strata, &["1", "2", "4"], Default::default()) {
            app.render_workspace.view.studio.transmission_samples = match strata {
                1 => 2,
                2 => 4,
                _ => 1,
            };
        }
    });
    let label = app.tr("render.children.nested").to_string();
    let response = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.nested_media);
    crate::scenario::publish_target("render:nested-enable", response.rect);
    ui_kit::text(ui, app.tr("render.nested_help"), ui_kit::Text::Note("render.nested_help"));
    app.render_workspace.view.dirty |=
        before != (app.render_workspace.view.studio.nested_media, app.render_workspace.view.studio.transmission_samples);
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
        let photons = crate::i18n::tr("render.caustics_photons");
        ui_kit::row(ui, photons, |ui| {
            let field = ui_kit::number_field_preserving(ui, &mut options.photons, 1..=65536, 64.0, "");
            crate::scenario::publish_target("render:caustics:photons", field.rect);
            field
        });
        let radius = crate::i18n::tr("render.caustics_radius");
        ui_kit::row(ui, radius, |ui| {
            let field = ui_kit::number_field_preserving(ui, &mut options.radius_mm, 1e-6..=1e6, 0.1, " mm");
            crate::scenario::publish_target("render:caustics:radius", field.rect);
            field
        });
        let seed = crate::i18n::tr("render.children.seed");
        ui_kit::row(ui, seed, |ui| {
            let field = ui_kit::number_field_preserving(ui, &mut options.seed, 0..=u32::MAX, 1.0, "");
            crate::scenario::publish_target("render:caustics:seed", field.rect);
            field
        });
        let depth = crate::i18n::tr("render.caustics_depth");
        ui_kit::row(ui, depth, |ui| {
            let field = ui_kit::number_field_preserving(ui, &mut options.max_depth, 1..=16, 1.0, "");
            crate::scenario::publish_target("render:caustics:depth", field.rect);
            field
        });
    }
    ui_kit::text(ui, app.tr("render.caustics_help"), ui_kit::Text::Note("render.caustics_help"));
    app.render_workspace.view.dirty |= before != app.render_workspace.view.studio.caustics;
}

/// CPU has one progressive Fresnel event per film ordinal. GPU strata and its
/// nested-media flag are deliberately absent from this method's controls.
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
    if let Some(path) = app.render_workspace.view.raytrace.path {
        ui_kit::row(ui, app.tr("render.cpu_path_depth"), |ui| ui_kit::text(ui, &path.max_depth.to_string(), ui_kit::Text::Inline));
    } else {
        let reason = super::cpu::path_reason(app);
        let button = ui_kit::button(ui, app.tr("render.cpu_glass_enable_path"), ui_kit::Button::Enabled(reason.is_none()));
        crate::scenario::publish_target("render:cpu-glass-enable-path", button.rect);
        if button.clicked() {
            app.render_workspace.view.raytrace.path = Some(Default::default());
            app.render_workspace.view.dirty = true;
        }
        if let Some(reason) = reason {
            ui_kit::text(ui, &reason, ui_kit::Text::Note("render.cpu_glass_path_reason"));
        }
    }
    ui_kit::text(ui, app.tr("render.cpu_path_help"), ui_kit::Text::Note("render.cpu_glass_path_help"));
}
