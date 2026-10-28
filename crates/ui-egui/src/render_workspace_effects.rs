//! Shared-kit layered weathering and photographic presentation editors.
//! Output presentation edits patch the saved job; legacy master controls retain their draft.
use super::{ImageMethod, materials, studio};
use crate::{SolveApp, ui_kit};
use solidcraft_engine::render::job::looks::{LookKind, LookLayer};

#[derive(Clone, Debug, Default)]
pub(super) struct View {
    pub selected_aging: usize,
    pub selected_look: usize,
    pub(super) targets: Vec<materials::Target>,
    pub(super) search: String,
}
pub(super) fn note(app: &SolveApp, ui: &mut egui::Ui, key: &'static str) {
    ui_kit::text(ui, app.tr(key), ui_kit::Text::Note(key));
}
pub(super) fn number(app: &SolveApp, ui: &mut egui::Ui, key: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, speed: f64, suffix: &str) {
    let response = ui_kit::row(ui, app.tr(key), |ui| ui_kit::number_field(ui, value, range, speed, suffix));
    crate::scenario::publish_target(key, response.rect);
}
fn slider(app: &SolveApp, ui: &mut egui::Ui, key: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, speed: f64, suffix: &str) {
    let label = match key {
        "render.flare_strength" => "render.children.flare",
        "render.flare_threshold" | "render.bloom_threshold" => "render.children.threshold",
        "render.flare_halo" => "render.children.halo",
        "render.flare_streak" => "render.children.streak",
        "render.chromatic_offset" => "render.children.chromatic",
        "render.radial_distortion" => "render.children.distortion",
        "render.soft_focus" => "render.children.soft_focus",
        "render.sharpening" => "render.children.sharpening",
        "render.film_grain" => "render.children.grain",
        "render.post_contrast" => "render.children.contrast",
        "render.post_saturation" => "render.children.saturation",
        "render.post_vignette" => "render.children.vignette",
        "render.bloom_strength" => "render.children.strength",
        _ => key,
    };
    let mut edited = if value.is_finite() { value.clamp(*range.start(), *range.end()) } else { *range.start() };
    let response = ui_kit::row(ui, app.tr(label), |ui| {
        if key == "render.chromatic_offset" {
            ui_kit::number_field(ui, &mut edited, range, speed, suffix)
        } else {
            ui_kit::slider(ui, &mut edited, range, Default::default())
        }
    });
    if response.changed() {
        *value = edited;
    }
    crate::scenario::publish_target(key, response.rect);
}
pub(super) fn failure(app: &mut SolveApp, result: Result<(), String>) {
    if let Err(error) = result {
        app.set_status(error, true);
    }
}

pub(super) fn layer_actions(app: &SolveApp, ui: &mut egui::Ui, prefix: &str, selected: usize, length: usize) -> (i32, bool) {
    let mut movement = 0;
    let mut remove = false;
    ui.horizontal_wrapped(|ui| {
        let up = ui_kit::button(ui, app.tr("render.layer_up"), ui_kit::Button::Enabled(selected > 0));
        crate::scenario::publish_target(&format!("render:{prefix}-up"), up.rect);
        if up.clicked() {
            movement = -1;
        }
        let down = ui_kit::button(ui, app.tr("render.layer_down"), ui_kit::Button::Enabled(selected + 1 < length));
        crate::scenario::publish_target(&format!("render:{prefix}-down"), down.rect);
        if down.clicked() {
            movement = 1;
        }
        let delete = ui_kit::button(ui, app.tr("render.layer_remove"), ui_kit::Button::Enabled(length > 0));
        crate::scenario::publish_target(&format!("render:{prefix}-remove"), delete.rect);
        remove = delete.clicked();
    });
    (movement, remove)
}

pub(super) fn presentation_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::banner(ui, ui_kit::AssistantTone::Neutral, |ui| note(app, ui, "render.children.effects_banner"));
    let enabled = app.render_workspace.view.method == ImageMethod::GpuRaytrace;
    ui.add_enabled_ui(enabled, |ui| presentation_editor(app, ui));
}
fn presentation_editor(app: &mut SolveApp, ui: &mut egui::Ui) {
    let presentation_before = super::ribbon::presentation(app);
    let title = app.tr("render.children.tone").to_string();
    ui_kit::section(ui, "render_effects_tone", &title, true, |ui| {
        let mut selected = usize::from(app.render_workspace.view.studio.tone_map == solidcraft_engine::render::job::ToneMap::AcesFitted);
        let response = ui.scope(|ui| {
            ui_kit::row(ui, app.tr("render.studio_tone_map"), |ui| {
                if ui_kit::segmented(
                    ui,
                    &mut selected,
                    &["Reinhard", "ACES fitted"],
                    ui_kit::Segments { compact: true, targets: &["render:tone:reinhard", "render:tone:aces"], ..Default::default() },
                ) {
                    app.render_workspace.view.studio.tone_map = if selected == 0 {
                        solidcraft_engine::render::job::ToneMap::Reinhard
                    } else {
                        solidcraft_engine::render::job::ToneMap::AcesFitted
                    };
                }
            });
        });
        crate::scenario::publish_target("render:tone-map", response.response.rect);
    });
    let mut editor = std::mem::take(&mut app.render_workspace.view.inspector.effects);
    let before = app.render_workspace.view.studio.finishing;
    let mut finish = before;
    let title = app.tr("render.children.looks").to_string();
    ui_kit::section(ui, "render_creative_looks", &title, true, |ui| {
        let add = ui_kit::dropdown_menu(ui, "render_look_add", app.tr("render.look_add"), |ui| {
            for (index, kind) in LookKind::ALL.into_iter().enumerate() {
                if ui_kit::list_picker(ui, &format!("look-add-{index}"), app.tr(kind.key()), false).clicked() {
                    match finish.looks.push(LookLayer { kind, ..Default::default() }) {
                        Ok(()) => editor.selected_look = finish.looks.len() - 1,
                        Err(error) => app.set_status(error.to_string(), true),
                    }
                    ui.close();
                }
            }
        });
        crate::scenario::publish_target("render:look-add", add.rect);
        editor.selected_look = editor.selected_look.min(finish.looks.len().saturating_sub(1));
        let (movement, remove) = layer_actions(app, ui, "look", editor.selected_look, finish.looks.len());
        if !finish.looks.is_empty() {
            if remove {
                failure(app, finish.looks.remove(editor.selected_look).map_err(|e| e.to_string()));
                editor.selected_look = editor.selected_look.min(finish.looks.len().saturating_sub(1));
            } else if movement != 0 {
                let next = if movement < 0 { editor.selected_look.saturating_sub(1) } else { editor.selected_look + 1 };
                failure(app, finish.looks.move_layer(editor.selected_look, next).map_err(|e| e.to_string()));
                editor.selected_look = next;
            }
        }
        for index in 0..finish.looks.len() {
            if let Some(layer) = finish.looks.get(index) {
                let label = format!("{} · {}", index + 1, app.tr(layer.kind.key()));
                let response = ui_kit::list_picker(ui, &format!("look-layer-{index}"), &label, index == editor.selected_look);
                crate::scenario::publish_target(&format!("render:look-layer:{index}"), response.rect);
                if response.clicked() {
                    editor.selected_look = index;
                }
            }
        }
        if finish.looks.is_empty() {
            note(app, ui, "render.children.add_look_reason");
            ui.add_enabled_ui(false, |ui| {
                let mut enabled = true;
                let mut strength = 1.0;
                ui_kit::checkbox(ui, app.tr("render.layer_enabled"), &mut enabled);
                slider(app, ui, "render.look_strength", &mut strength, 0.0..=1.0, 0.01, "");
            });
        }
        if let Some(layer) = finish.looks.get_mut(editor.selected_look) {
            let response = ui_kit::checkbox(ui, app.tr("render.layer_enabled"), &mut layer.enabled);
            crate::scenario::publish_target("render:look-enabled", response.rect);
            slider(app, ui, "render.look_strength", &mut layer.strength, 0.0..=1.0, 0.01, "");
        }
    });
    let title = app.tr("render.children.lens_effects").to_string();
    ui_kit::section(ui, "render_photographic_effects", &title, true, |ui| {
        let photo = &mut finish.photographic;
        for key in [
            "render.flare_strength",
            "render.flare_threshold",
            "render.flare_halo",
            "render.flare_streak",
            "render.chromatic_offset",
            "render.radial_distortion",
            "render.soft_focus",
            "render.sharpening",
            "render.film_grain",
        ] {
            let (field, range, speed, suffix) = match key {
                "render.flare_strength" => (&mut photo.flare.strength, 0.0..=2.0, 0.01, ""),
                "render.flare_threshold" => (&mut photo.flare.threshold, 0.001..=1e6, 0.05, ""),
                "render.flare_halo" => (&mut photo.flare.halo, 0.0..=2.0, 0.01, ""),
                "render.flare_streak" => (&mut photo.flare.streak, 0.0..=2.0, 0.01, ""),
                "render.chromatic_offset" => (&mut photo.chromatic_pixels, 0.0..=12.0, 0.1, " px"),
                "render.radial_distortion" => (&mut photo.radial_distortion, -0.3..=0.3, 0.005, ""),
                "render.soft_focus" => (&mut photo.soft_focus, 0.0..=1.0, 0.01, ""),
                "render.sharpening" => (&mut photo.sharpening, 0.0..=1.0, 0.01, ""),
                _ => (&mut photo.grain, 0.0..=0.15, 0.001, ""),
            };
            slider(app, ui, key, field, range, speed, suffix);
            if key == "render.flare_streak" {
                let response =
                    ui_kit::row(ui, app.tr("render.children.ghosts"), |ui| ui_kit::number_field(ui, &mut photo.flare.ghosts, 0..=4, 1.0, ""));
                crate::scenario::publish_target("render:flare-ghosts", response.rect);
            }
        }
        let response = ui_kit::row(ui, app.tr("render.effects_seed"), |ui| ui_kit::number_field(ui, &mut photo.seed, 0..=u32::MAX, 1.0, ""));
        crate::scenario::publish_target("render:photo-seed", response.rect);
        note(app, ui, "render.lens_effects_help");
        if ui_kit::button(ui, app.tr("render.effects_reset_lens"), ui_kit::Button::Normal).clicked() {
            *photo = Default::default();
        }
    });
    let title = app.tr("render.children.finishing").to_string();
    ui_kit::section(ui, "render_effects_finishing", &title, true, |ui| {
        studio::vector(ui, app.tr("render.children.rgb_gain"), &mut finish.rgb_gain, 0.1..=8.0, 0.01);
        slider(app, ui, "render.post_contrast", &mut finish.contrast, 0.25..=4.0, 0.01, "");
        slider(app, ui, "render.post_saturation", &mut finish.saturation, 0.0..=2.0, 0.01, "");
        slider(app, ui, "render.post_vignette", &mut finish.vignette, 0.0..=1.0, 0.01, "");
        let mut bloom = finish.bloom.unwrap_or(solidcraft_engine::render::job::bloom::Bloom { strength: 0.15, ..Default::default() });
        let mut enabled = finish.bloom.is_some();
        let response = ui_kit::checkbox(ui, app.tr("render.children.bloom"), &mut enabled);
        crate::scenario::publish_target("render:bloom-enable", response.rect);
        if !enabled {
            note(app, ui, "render.children.enable_bloom");
        }
        ui.add_enabled_ui(enabled, |ui| {
            slider(app, ui, "render.bloom_strength", &mut bloom.strength, 0.0..=2.0, 0.01, "");
            slider(app, ui, "render.bloom_threshold", &mut bloom.threshold, 0.0..=1e6, 0.05, "");
            let response = ui_kit::row(ui, app.tr("render.children.radius"), |ui| ui_kit::number_field(ui, &mut bloom.radius, 1..=64, 1.0, " px"));
            crate::scenario::publish_target("render.bloom_radius", response.rect);
        });
        finish.bloom = enabled.then_some(bloom);
        if app.render_workspace.view.transparent && enabled {
            note(app, ui, "render.bloom_opaque");
        }
        let response = ui_kit::button(ui, app.tr("render.children.reset_finishing"), ui_kit::Button::Normal);
        crate::scenario::publish_target("render:post-reset", response.rect);
        if response.clicked() {
            finish.rgb_gain = [1.0; 3];
            finish.contrast = 1.0;
            finish.saturation = 1.0;
            finish.vignette = 0.0;
            finish.bloom = None;
        }
    });
    if before != finish {
        app.render_workspace.view.studio.finishing = finish;
        app.render_workspace.view.dirty = true;
    }
    app.render_workspace.view.inspector.effects = editor;
    let title = app.tr("render.children.regrade").to_string();
    ui_kit::section(ui, "render_effects_regrade", &title, true, |ui| studio::regrade_action(app, ui));
    app.render_workspace.view.dirty |= presentation_before != super::ribbon::presentation(app);
}
