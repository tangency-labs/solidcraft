//! Shared quality controls. Edits retain the selected output size and saved shot intent.
use crate::ui_kit::BackingTooltipExt as _;
use crate::{SolveApp, ui_kit};
use solidcraft_engine::render::job::{DenoiseGuideClass, MAX_CPU_THREADS};

/// New selections are bounded without rewriting larger legacy jobs on paint or load.
pub(super) const MAX_UI_SAMPLES: u32 = solidcraft_engine::render::job::MAX_UI_SAMPLES;

pub(super) fn sample_presets(ui: &mut egui::Ui, samples: &mut u32, range: std::ops::RangeInclusive<u32>, prefix: &str) -> bool {
    let mut changed = false;
    ui_kit::catalog_scope(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for count in [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384] {
                let target = format!("{prefix}:{count}");
                // A child scope advances the row without wrapping. Allocate
                // the chip itself in the wrapping parent instead.
                let response = ui
                    .add_enabled(range.contains(&count), |ui: &mut egui::Ui| ui_kit::choice_chip(ui, &target, &count.to_string(), *samples == count));
                crate::scenario::publish_target(&target, response.rect);
                if response.clicked() {
                    *samples = count;
                    changed = true;
                }
            }
        });
    });
    changed
}

pub(super) fn legacy_samples_note(_app: &SolveApp, ui: &mut egui::Ui, samples: u32) {
    if samples > MAX_UI_SAMPLES {
        let message = solidcraft_ui_text::format(
            "render.quality_legacy_samples",
            &[("samples", &samples.to_string()), ("maximum", &MAX_UI_SAMPLES.to_string())],
        );
        ui_kit::text(ui, &message, ui_kit::Text::Note("render.quality_legacy_samples"));
    }
}

pub(super) fn supersampling_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let available = app.render_workspace.view.method != super::ImageMethod::Raster;
    let label = app.tr("render.supersampling").to_string();
    let labels = [app.tr("render.supersampling_off").to_string(), app.tr("render.supersampling_x2").to_string()];
    let before = app.render_workspace.view.studio.supersampling;
    let mut selected = usize::from(before == 2);
    ui.add_enabled_ui(available, |ui| {
        let response = ui_kit::row(ui, &label, |ui| {
            let response = ui.scope(|ui| {
                if ui_kit::segmented(
                    ui,
                    &mut selected,
                    &[&labels[0], &labels[1]],
                    ui_kit::Segments { compact: true, targets: &["render:supersampling:off", "render:supersampling:x2"], ..Default::default() },
                ) {
                    app.render_workspace.view.studio.supersampling = if selected == 0 { 1 } else { 2 };
                }
            });
            response.response
        });
        crate::scenario::publish_target("render:supersampling", response.rect);
    });
    app.render_workspace.view.dirty |= before != app.render_workspace.view.studio.supersampling;
    let key = if available { "render.supersampling_help" } else { "render.output_raytrace_only" };
    ui_kit::text(ui, app.tr(key), ui_kit::Text::Note(key));
    if available && app.render_workspace.view.studio.supersampling == 2 {
        let draft = &app.render_workspace.view.draft;
        let message = solidcraft_ui_text::format(
            "render.supersampling_dimensions",
            &[
                ("width", &u64::from(draft.width).saturating_mul(2).to_string()),
                ("height", &u64::from(draft.height).saturating_mul(2).to_string()),
                ("output_width", &draft.width.to_string()),
                ("output_height", &draft.height.to_string()),
            ],
        );
        ui_kit::text(ui, &message, ui_kit::Text::Note("render.supersampling_dimensions"));
    }
}

pub(super) fn cpu_threads_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let before = app.render_workspace.view.raytrace.threads;
    let mut mode = match before {
        0 => 0,
        1 => 1,
        _ => 2,
    };
    let labels = [
        app.tr("render.cpu_threads_auto").to_string(),
        app.tr("render.cpu_threads_single").to_string(),
        app.tr("render.cpu_threads_custom").to_string(),
    ];
    let label = app.tr("render.cpu_threads").to_string();
    ui.add_enabled_ui(!cfg!(target_arch = "wasm32"), |ui| {
        let response = ui_kit::row(ui, &label, |ui| {
            let response = ui.scope(|ui| {
                if ui_kit::segmented(
                    ui,
                    &mut mode,
                    &[&labels[0], &labels[1], &labels[2]],
                    ui_kit::Segments {
                        compact: true,
                        targets: &["render:cpu-threads:auto", "render:cpu-threads:single", "render:cpu-threads:custom"],
                        ..Default::default()
                    },
                ) {
                    app.render_workspace.view.raytrace.threads = match mode {
                        0 => 0,
                        1 => 1,
                        _ => before.max(2),
                    };
                }
            });
            response.response
        });
        crate::scenario::publish_target("render:cpu-threads", response.rect);
        if mode == 2 {
            ui_kit::row(ui, &label, |ui| {
                let field = ui_kit::number_field_preserving(ui, &mut app.render_workspace.view.raytrace.threads, 1..=MAX_CPU_THREADS, 1.0, "");
                crate::scenario::publish_target("render:cpu-threads-field", field.rect);
                field
            });
        }
    });
    app.render_workspace.view.dirty |= before != app.render_workspace.view.raytrace.threads;
    let key = if cfg!(target_arch = "wasm32") { "render.cpu_threads_web" } else { "render.cpu_threads_help" };
    ui_kit::text(ui, app.tr(key), ui_kit::Text::Note(key));
}

pub(super) fn denoise_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    // CPU and GPU sections can both stay expanded. Publish shared handles only
    // from the active section, so a disabled copy cannot shadow the real control.
    let publish_controls = ui.is_enabled();
    let publish_target = |id: &str, rect| {
        if publish_controls {
            crate::scenario::publish_target(id, rect);
        }
    };
    let before = (
        app.render_workspace.view.studio.denoise,
        app.render_workspace.view.studio.denoise_settings,
        app.render_workspace.view.studio.denoise_diagnostics,
    );
    let label = app.tr("render.studio_denoise").to_string();
    let response = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.denoise);
    publish_target("render:denoise-enable", response.rect);
    publish_target("render:gpu-denoise", response.rect);
    let enabled = app.render_workspace.view.studio.denoise;
    ui.add_enabled_ui(enabled, |ui| {
        let label = app.tr("render.denoise_passes").to_string();
        ui_kit::row(ui, &label, |ui| {
            let response = ui_kit::number_field_preserving(ui, &mut app.render_workspace.view.studio.denoise_settings.passes, 1..=3, 1.0, "");
            publish_target("render:denoise-passes", response.rect);
            response
        });
        let label = app.tr("render.denoise_strength").to_string();
        ui_kit::row(ui, &label, |ui| {
            let response = ui_kit::slider(
                ui,
                &mut app.render_workspace.view.studio.denoise_settings.strength,
                0.0..=1.0,
                ui_kit::Slider { step: Some(0.05), format: |value| format!("{:.0}%", value * 100.0), ..Default::default() },
            );
            publish_target("render:denoise-strength", response.rect);
            response
        });
        let label = app.tr("render.denoise_firefly").to_string();
        let response = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.denoise_settings.firefly_suppression);
        publish_target("render:denoise-firefly", response.rect);
        let label = app.tr("render.denoise_optical_filter").to_string();
        let response = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.denoise_settings.optical_filter);
        publish_target("render:denoise-optical-filter", response.rect);
        response.backing_on_hover_text(app.tr("render.denoise_optical_filter_help"));
    });
    ui_kit::text(ui, app.tr("render.denoise_settings_help"), ui_kit::Text::Note("render.denoise_settings_help"));
    let label = app.tr("render.denoise_diagnostics_enable").to_string();
    let response = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.denoise_diagnostics);
    publish_target("render:denoise-diagnostics", response.rect);
    if app.render_workspace.view.studio.denoise_diagnostics {
        ui_kit::text(ui, app.tr("render.denoise_diagnostics_help"), ui_kit::Text::Note("render.denoise_diagnostics_help"));
    }
    let after = &app.render_workspace.view.studio;
    app.render_workspace.view.dirty |= before != (after.denoise, after.denoise_settings, after.denoise_diagnostics);
}

pub(super) fn diagnostic_class_label(class: DenoiseGuideClass) -> &'static str {
    match class {
        DenoiseGuideClass::Unavailable => "render.denoise_class_unavailable",
        DenoiseGuideClass::Surface => "render.denoise_class_surface",
        DenoiseGuideClass::Optical => "render.denoise_class_optical",
        DenoiseGuideClass::Background => "render.denoise_class_background",
        DenoiseGuideClass::Emitter => "render.denoise_class_emitter",
        DenoiseGuideClass::UnsupportedMaterial => "render.denoise_class_material",
        DenoiseGuideClass::NestedMedium => "render.denoise_class_nested",
        DenoiseGuideClass::DepthOfField => "render.denoise_class_lens",
        DenoiseGuideClass::Volume => "render.denoise_class_volume",
        DenoiseGuideClass::PathDepth => "render.denoise_class_depth",
        DenoiseGuideClass::DegeneratePath => "render.denoise_class_degenerate",
        DenoiseGuideClass::IncompatibleExit => "render.denoise_class_exit",
        DenoiseGuideClass::TotalInternalReflection => "render.denoise_class_tir",
        DenoiseGuideClass::FurtherTransmission => "render.denoise_class_transmission",
        DenoiseGuideClass::ReflectiveEndpoint => "render.denoise_class_reflection",
        DenoiseGuideClass::IdentityLimit => "render.denoise_class_identity",
        DenoiseGuideClass::PartialCoverage => "render.denoise_class_coverage",
    }
}

pub(super) fn mask_active(app: &SolveApp) -> bool {
    app.render_workspace.view.denoise_mask_job.is_some() && app.render_workspace.view.denoise_mask_job == app.render_workspace.view.completed
}

pub(super) fn mask_available(app: &SolveApp) -> bool {
    app.render_workspace
        .view
        .completed
        .and_then(|job| app.session.render_job_handle(job).ok())
        .and_then(|handle| handle.frame().ok().flatten())
        .is_some_and(|frame| frame.denoise_mask_png().is_some())
}

pub(super) fn mask_view_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let available = mask_available(app);
    let mut selected = usize::from(mask_active(app));
    let labels = [app.tr("render.denoise_view_image").to_string(), app.tr("render.denoise_view_mask").to_string()];
    let reason = app.tr("render.denoise_mask_unavailable").to_string();
    // The comparison strip also has unlabelled segments in this result panel.
    // egui's sibling layout UIs share their stable scope, so isolate this group
    // before the kit derives its per-segment persistent interaction IDs.
    let changed = ui
        .scope_builder(egui::UiBuilder::new().id_salt("render_denoise_mask_view"), |ui| {
            ui_kit::segmented(
                ui,
                &mut selected,
                &[&labels[0], &labels[1]],
                ui_kit::Segments {
                    compact: true,
                    intrinsic: true,
                    disabled_reasons: &[None, (!available).then_some(reason.as_str())],
                    targets: &["render:result:image", "render:result:denoise-mask"],
                    ..Default::default()
                },
            )
        })
        .inner;
    if changed {
        let view = if selected == 0 { "image" } else { "denoise_mask" };
        if let Err(error) = app.run("render.result.view", serde_json::json!({"view":view})) {
            app.set_status(error, true);
        }
    }
}

pub(super) fn diagnostics(app: &mut SolveApp, ui: &mut egui::Ui) {
    let report = app
        .render_workspace
        .view
        .completed
        .and_then(|job| app.session.render_job_handle(job).ok())
        .and_then(|handle| handle.status().ok())
        .and_then(|status| status.denoise_diagnostics);
    let Some(report) = report else {
        ui_kit::text(ui, app.tr("render.denoise_mask_unavailable"), ui_kit::Text::Note("render.denoise_mask_unavailable"));
        return;
    };
    let grid = solidcraft_ui_text::format(
        "render.denoise_diagnostics_grid",
        &[("width", &report.width.to_string()), ("height", &report.height.to_string())],
    );
    ui_kit::text(ui, &grid, ui_kit::Text::Note("render.denoise_diagnostics_grid"));
    let eligible: u64 = DenoiseGuideClass::ALL.iter().filter(|class| class.eligible()).map(|&class| report.pixels(class)).sum();
    let total: u64 = report.counts.iter().sum();
    let summary = solidcraft_ui_text::format(
        "render.denoise_diagnostics_counts",
        &[("eligible", &eligible.to_string()), ("skipped", &total.saturating_sub(eligible).to_string())],
    );
    ui_kit::text(ui, &summary, ui_kit::Text::Inline);
    let t = solidcraft_theme::Tokens::get();
    for &class in DenoiseGuideClass::ALL {
        let count = report.pixels(class);
        if count == 0 {
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(t.space_12, t.space_12), egui::Sense::hover());
            let [r, g, b, a] = class.color();
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(r, g, b, a));
            ui.label(app.tr(diagnostic_class_label(class)));
            ui.label(count.to_string());
        });
    }
    mask_view_controls(app, ui);
    let export = ui_kit::button(ui, app.tr("render.denoise_export_mask"), ui_kit::Button::Enabled(mask_available(app)));
    crate::scenario::publish_target("render:denoise-export-mask", export.rect);
    if export.clicked() {
        app.render_workspace.view.studio.export_denoise_mask = true;
        app.render_workspace.view.studio.export_exr = false;
        app.start("render.export");
    }
}
