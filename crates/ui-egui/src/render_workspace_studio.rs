//! GPU studio editors. Numeric intent is portable; HDR resources cross the engine capture boundary.
use serde_json::{Value, json};
use solidcraft_engine::render::job::{AreaLight, AreaLights, StudioLighting, Sun, ThinLens, ToneMap};
use solidcraft_engine::render::lighting::environment::SkyGradient;

use super::hdr;
use crate::{SolveApp, ui_kit};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Environment {
    #[default]
    None,
    Sky,
    Hdr,
}

#[derive(Clone, Debug)]
pub(super) struct View {
    pub aging: solidcraft_engine::doc::render_aging::SavedAgingStack,
    pub nested_media: bool,
    pub volume: Option<solidcraft_engine::render::job::volume::HomogeneousVolume>,
    pub adaptive: Option<solidcraft_engine::render::job::adaptive::AdaptiveSampling>,
    pub transmission_samples: u32,
    pub finishing: solidcraft_engine::render::job::finishing::Finishing,
    pub backdrop_enabled: bool,
    pub backdrop: solidcraft_engine::render::job::finishing::Backdrop,
    pub linear_exr: bool,
    pub export_exr: bool,
    pub tone_map: ToneMap,
    pub denoise: bool,
    pub lens_enabled: bool,
    pub lens: ThinLens,
    pub environment: Environment,
    pub hdr: hdr::Input,
    pub assume_linear_srgb: bool,
    pub rotation_rad: f64,
    pub gradient: SkyGradient,
    pub intensity: f64,
    pub lighting_enabled: bool,
    pub ambient: [f64; 3],
    pub sun_enabled: bool,
    pub sun: Sun,
    pub area_lights: Vec<AreaLight>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            aging: Default::default(),
            nested_media: false,
            volume: None,
            adaptive: None,
            transmission_samples: 1,
            finishing: Default::default(),
            backdrop_enabled: false,
            backdrop: Default::default(),
            linear_exr: false,
            export_exr: false,
            tone_map: ToneMap::Reinhard,
            denoise: false,
            lens_enabled: false,
            lens: ThinLens { aperture_radius_mm: 1.0, focus_distance_mm: 100.0 },
            environment: Environment::None,
            hdr: hdr::Input::default(),
            assume_linear_srgb: false,
            rotation_rad: 0.0,
            gradient: SkyGradient::default(),
            intensity: 1.0,
            lighting_enabled: false,
            ambient: [0.5; 3],
            sun_enabled: false,
            sun: Sun { direction: [-0.4, -0.6, 0.7], angular_radius_rad: 0.02, radiance: [1000.0, 950.0, 850.0] },
            area_lights: Vec::new(),
        }
    }
}

pub(super) fn params(app: &SolveApp, params: &mut Value) -> Result<(), String> {
    let view = &app.render_workspace.view.studio;
    params["tone_map"] = json!(view.tone_map);
    params["denoise"] = json!(view.denoise);
    params["finishing"] = json!(view.finishing);
    params["backdrop"] = json!(view.backdrop_enabled.then_some(view.backdrop));
    params["linear_exr"] = json!(view.linear_exr);
    params["aging"] = json!(view.aging);
    params["nested_media"] = json!(view.nested_media);
    params["volume"] = json!(view.volume);
    params["adaptive"] = json!(view.adaptive);
    params["transmission_samples"] = json!(view.transmission_samples);
    if view.lens_enabled {
        view.lens.validate().map_err(|error| error.to_string())?;
        if view.lens.aperture_radius_mm > 0.0 && app.cam.fov == 0.0 {
            return Err(app.tr("render.studio_lens_perspective").to_string());
        }
        params["lens"] = json!(view.lens);
    }
    if view.lighting_enabled {
        let lighting = StudioLighting {
            environment: view.ambient,
            sun: view.sun_enabled.then_some(view.sun),
            area_lights: AreaLights::new(&view.area_lights).map_err(|error| error.to_string())?,
        };
        lighting.validate().map_err(|error| error.to_string())?;
        params["lighting"] = json!(lighting);
    }
    if view.environment != Environment::None {
        if !view.intensity.is_finite() || !(0.0..=100.0).contains(&view.intensity) || !view.rotation_rad.is_finite() || view.rotation_rad.abs() > 1e6
        {
            return Err(app.tr("render.studio_environment_bounds").to_string());
        }
        let mut environment = json!({"kind":"sky","rotation_rad":view.rotation_rad,"intensity":view.intensity});
        if view.environment == Environment::Hdr {
            environment = hdr::request(app, hdr::Target::Gpu)?;
            environment["rotation_rad"] = json!(view.rotation_rad);
            environment["intensity"] = json!(view.intensity);
            environment["assume_linear_srgb"] = json!(view.assume_linear_srgb);
        }
        if view.environment == Environment::Sky {
            environment["gradient"] = json!(view.gradient);
        }
        params["environment"] = environment;
    }
    Ok(())
}

pub(super) fn vector(ui: &mut egui::Ui, label: &str, value: &mut [f64; 3], range: std::ops::RangeInclusive<f64>, speed: f64) {
    ui.label(label);
    ui.horizontal(|ui| {
        for component in value {
            ui_kit::number_field(ui, component, range.clone(), speed, "");
        }
    });
}

pub(super) fn finishing_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    super::transport::sampling_controls(app, ui);
    post_controls(app, ui);
    ui.label(app.tr("render.studio_tone_map"));
    let selected = app.render_workspace.view.studio.tone_map;
    let label = match selected {
        ToneMap::Reinhard => "Reinhard",
        ToneMap::AcesFitted => "ACES fitted",
    };
    ui_kit::dropdown_menu(ui, "render_tone_map", label, |ui| {
        for (map, label) in [(ToneMap::Reinhard, "Reinhard"), (ToneMap::AcesFitted, "ACES fitted")] {
            if ui_kit::list_picker(ui, label, label, map == selected).clicked() {
                app.render_workspace.view.studio.tone_map = map;
                ui.close();
            }
        }
    });
    let denoise = app.tr("render.studio_denoise").to_string();
    ui_kit::checkbox(ui, &denoise, &mut app.render_workspace.view.studio.denoise);
    if app.render_workspace.view.studio.denoise {
        ui.small(app.tr("render.studio_denoise_help"));
    }
}

pub(super) fn lens_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let lens = app.tr("render.studio_lens").to_string();
    ui_kit::checkbox(ui, &lens, &mut app.render_workspace.view.studio.lens_enabled);
    if app.render_workspace.view.studio.lens_enabled {
        ui.label(app.tr("render.studio_aperture"));
        ui_kit::number_field(ui, &mut app.render_workspace.view.studio.lens.aperture_radius_mm, 0.0..=1e7, 0.1, " mm");
        ui.label(app.tr("render.studio_focus"));
        ui_kit::number_field(ui, &mut app.render_workspace.view.studio.lens.focus_distance_mm, 1e-6..=1e9, 1.0, " mm");
    }
}

pub(super) fn controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    finishing_controls(app, ui);
    lens_controls(app, ui);
    let title = app.tr("render.studio_environment").to_string();
    ui_kit::section(ui, "render_environment", &title, false, |ui| environment_controls(app, ui));
    let title = app.tr("render.studio_lighting").to_string();
    ui_kit::section(ui, "render_studio_lighting", &title, false, |ui| lighting_controls(app, ui));
    ui.small(app.tr("render.studio_runtime_help"));
}

pub(super) fn environment_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    super::transport::volume_controls(app, ui);
    let variants =
        [(Environment::None, "render.studio_environment_none"), (Environment::Sky, "render.studio_sky"), (Environment::Hdr, "render.studio_hdr")];
    let selected = app.render_workspace.view.studio.environment;
    let key = match selected {
        Environment::None => "render.studio_environment_none",
        Environment::Sky => "render.studio_sky",
        Environment::Hdr => "render.studio_hdr",
    };
    let label = app.tr(key).to_string();
    ui_kit::dropdown_menu(ui, "render_environment_kind", &label, |ui| {
        for (value, key) in variants {
            if ui_kit::list_picker(ui, key, app.tr(key), value == selected).clicked() {
                app.render_workspace.view.studio.environment = value;
                ui.close();
            }
        }
    });
    if app.render_workspace.view.studio.environment == Environment::None {
        return;
    }
    if app.render_workspace.view.studio.environment == Environment::Hdr {
        hdr::controls(app, ui, hdr::Target::Gpu);
        let assume = app.tr("render.studio_hdr_srgb").to_string();
        ui_kit::checkbox(ui, &assume, &mut app.render_workspace.view.studio.assume_linear_srgb);
        ui.small(app.tr("render.studio_hdr_help"));
    } else {
        ui.small(app.tr("render.studio_sky_help"));
        for (index, key) in [(0, "render.sky_zenith"), (1, "render.sky_horizon"), (2, "render.sky_ground")] {
            let label = app.tr(key).to_string();
            let gradient = &mut app.render_workspace.view.studio.gradient;
            let field = match index {
                0 => &mut gradient.zenith,
                1 => &mut gradient.horizon,
                _ => &mut gradient.ground,
            };
            let mut editable = field.map(f64::from);
            let before = editable;
            vector(ui, &label, &mut editable, 0.0..=1e6, 0.01);
            if editable != before {
                *field = editable.map(|value| value as f32);
            }
        }
    }
    ui.label(app.tr("render.studio_intensity"));
    ui_kit::number_field(ui, &mut app.render_workspace.view.studio.intensity, 0.0..=100.0, 0.1, "");
    ui.label(app.tr("render.studio_rotation"));
    let mut degrees = app.render_workspace.view.studio.rotation_rad.to_degrees();
    if ui_kit::number_field(ui, &mut degrees, -1e6_f64.to_degrees()..=1e6_f64.to_degrees(), 1.0, "°").changed() {
        app.render_workspace.view.studio.rotation_rad = degrees.to_radians();
    }
}

pub(super) fn lighting_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let label = app.tr("render.studio_lighting_enable").to_string();
    ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.lighting_enabled);
    if !app.render_workspace.view.studio.lighting_enabled {
        return;
    }
    let ambient = app.tr("render.studio_ambient_rgb").to_string();
    vector(ui, &ambient, &mut app.render_workspace.view.studio.ambient, 0.0..=1e6, 0.1);
    let label = app.tr("render.studio_sun").to_string();
    ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.sun_enabled);
    if app.render_workspace.view.studio.sun_enabled {
        let direction = app.tr("render.studio_direction").to_string();
        vector(ui, &direction, &mut app.render_workspace.view.studio.sun.direction, -1e8..=1e8, 0.01);
        let radiance = app.tr("render.studio_radiance").to_string();
        vector(ui, &radiance, &mut app.render_workspace.view.studio.sun.radiance, 0.0..=1e8, 1.0);
        ui.label(app.tr("render.studio_sun_radius"));
        let mut radius = app.render_workspace.view.studio.sun.angular_radius_rad.to_degrees();
        if ui_kit::number_field(ui, &mut radius, 0.001_f64.to_degrees()..=90.0, 0.05, "°").changed() {
            app.render_workspace.view.studio.sun.angular_radius_rad = radius.to_radians();
        }
    }
    let title = app.tr("render.studio_area").to_string();
    let mut remove = None;
    for index in 0..app.render_workspace.view.studio.area_lights.len() {
        ui_kit::section(ui, ("render_area", index), &format!("{title} {}", index + 1), false, |ui| {
            for (key, field) in [("render.studio_center", 0), ("render.studio_half_u", 1), ("render.studio_half_v", 2), ("render.studio_radiance", 3)]
            {
                let label = app.tr(key).to_string();
                let Some(light) = app.render_workspace.view.studio.area_lights.get_mut(index) else { continue };
                let value = match field {
                    0 => &mut light.center,
                    1 => &mut light.u,
                    2 => &mut light.v,
                    _ => &mut light.radiance,
                };
                vector(
                    ui,
                    &label,
                    value,
                    if field == 3 {
                        0.0..=1e6
                    } else if field == 0 {
                        -1e12..=1e12
                    } else {
                        -1e6..=1e6
                    },
                    1.0,
                );
            }
            if ui_kit::button(ui, app.tr("render.studio_remove_area"), ui_kit::Button::Normal).clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        app.render_workspace.view.studio.area_lights.remove(index);
    }
    if ui_kit::button(ui, app.tr("render.studio_add_area"), ui_kit::Button::Enabled(app.render_workspace.view.studio.area_lights.len() < 4)).clicked()
    {
        let target = app.cam.target;
        let size = (app.cam.distance * 0.25).clamp(1.0, 1e5);
        app.render_workspace.view.studio.area_lights.push(AreaLight {
            center: [target.x, target.y, target.z + size * 2.0],
            u: [size, 0.0, 0.0],
            v: [0.0, -size, 0.0],
            radiance: [8.0; 3],
        });
    }
    ui.small(app.tr("render.studio_area_help"));
}

/// Presentation is opt-in; raw radiance and engineering appearance stay untouched.
pub(super) fn post_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let title = app.tr("render.post_title").to_string();
    ui_kit::section(ui, "render_post_filters", &title, false, |ui| {
        ui_kit::text(ui, app.tr("render.post_help"), ui_kit::Text::Note("render.post_help"));
        let before = app.render_workspace.view.studio.finishing;
        for (channel, key) in ["render.post_gain_r", "render.post_gain_g", "render.post_gain_b"].into_iter().enumerate() {
            let label = app.tr(key).to_string();
            let Some(value) = app.render_workspace.view.studio.finishing.rgb_gain.get_mut(channel) else {
                continue;
            };
            let r = ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, value, 0.1..=8.0, 0.01, ""));
            crate::scenario::publish_target(key, r.rect);
        }
        for key in ["render.post_contrast", "render.post_saturation", "render.post_vignette"] {
            let label = app.tr(key).to_string();
            let finish = &mut app.render_workspace.view.studio.finishing;
            let (value, range) = match key {
                "render.post_contrast" => (&mut finish.contrast, 0.25..=4.0),
                "render.post_saturation" => (&mut finish.saturation, 0.0..=2.0),
                _ => (&mut finish.vignette, 0.0..=1.0),
            };
            let r = ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, value, range, 0.01, ""));
            crate::scenario::publish_target(key, r.rect);
        }
        let mut enabled = app.render_workspace.view.studio.finishing.bloom.is_some();
        let label = app.tr("render.bloom_enable").to_string();
        let toggle = ui_kit::checkbox(ui, &label, &mut enabled);
        crate::scenario::publish_target("render:bloom-enable", toggle.rect);
        if toggle.changed() {
            app.render_workspace.view.studio.finishing.bloom =
                enabled.then_some(solidcraft_engine::render::job::bloom::Bloom { strength: 0.15, ..Default::default() });
        }
        if enabled {
            for key in ["render.bloom_strength", "render.bloom_threshold", "render.bloom_radius"] {
                let label = app.tr(key).to_string();
                if let Some(bloom) = &mut app.render_workspace.view.studio.finishing.bloom {
                    let response = ui_kit::row(ui, &label, |ui| match key {
                        "render.bloom_strength" => ui_kit::number_field(ui, &mut bloom.strength, 0.0..=2.0, 0.01, ""),
                        "render.bloom_threshold" => ui_kit::number_field(ui, &mut bloom.threshold, 0.0..=1e6, 0.05, ""),
                        _ => ui_kit::number_field(ui, &mut bloom.radius, 1..=64, 1.0, " px"),
                    });
                    crate::scenario::publish_target(key, response.rect);
                }
            }
            ui_kit::text(ui, app.tr("render.bloom_help"), ui_kit::Text::Note("render.bloom_help"));
            if app.render_workspace.view.transparent {
                ui_kit::text(ui, app.tr("render.bloom_opaque"), ui_kit::Text::Error("render.bloom_opaque"));
            }
        }
        let reset = ui_kit::button(ui, app.tr("render.post_reset"), ui_kit::Button::Normal);
        crate::scenario::publish_target("render:post-reset", reset.rect);
        if reset.clicked() {
            app.render_workspace.view.studio.finishing = Default::default();
        }
        if before != app.render_workspace.view.studio.finishing {
            app.render_workspace.view.dirty = true;
        }
    });
    let regrade = ui_kit::button(
        ui,
        app.tr("render.regrade_action"),
        ui_kit::Button::Enabled(super::ribbon::action_ready(app, "render.result.regrade").is_ok()),
    );
    crate::scenario::publish_target("render:regrade", regrade.rect);
    if regrade.clicked() {
        if let Err(error) = super::ribbon::perform_job(app, "render.result.regrade") {
            app.set_status(error, true);
        }
        ui.ctx().request_repaint();
    }
    ui_kit::text(ui, app.tr("render.regrade_help"), ui_kit::Text::Note("render.regrade_help"));
    let label = app.tr("render.linear_retain").to_string();
    let r = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.linear_exr);
    crate::scenario::publish_target("render:retain-exr", r.rect);
    app.render_workspace.view.dirty |= r.changed();
    if app.render_workspace.view.studio.linear_exr {
        ui_kit::text(ui, app.tr("render.linear_help"), ui_kit::Text::Note("render.linear_help"));
    }
}

pub(super) fn backdrop_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    use solidcraft_engine::render::job::finishing::BackdropMode;
    let label = app.tr("render.backdrop_enable").to_string();
    let r = ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.backdrop_enabled);
    crate::scenario::publish_target("render:backdrop-enabled", r.rect);
    app.render_workspace.view.dirty |= r.changed();
    if !app.render_workspace.view.studio.backdrop_enabled {
        return;
    }
    ui_kit::text(ui, app.tr("render.backdrop_help"), ui_kit::Text::Note("render.backdrop_help"));
    let before = app.render_workspace.view.studio.backdrop;
    let modes = [
        (BackdropMode::Vertical, "render.backdrop_vertical"),
        (BackdropMode::Horizontal, "render.backdrop_horizontal"),
        (BackdropMode::Radial, "render.backdrop_radial"),
    ];
    let current = app.render_workspace.view.studio.backdrop.mode;
    let key = modes.iter().find(|(m, _)| *m == current).map_or("render.backdrop_vertical", |(_, key)| *key);
    let label = app.tr(key).to_string();
    let r = ui_kit::dropdown_menu(ui, "render_backdrop_mode", &label, |ui| {
        for (mode, key) in modes {
            if ui_kit::list_picker(ui, key, app.tr(key), mode == current).clicked() {
                app.render_workspace.view.studio.backdrop.mode = mode;
                ui.close();
            }
        }
    });
    crate::scenario::publish_target("render:backdrop-mode", r.rect);
    for (first, key) in [(true, "render.backdrop_first"), (false, "render.backdrop_second")] {
        let label = app.tr(key).to_string();
        ui.label(label);
        let backdrop = &mut app.render_workspace.view.studio.backdrop;
        let color = if first { &mut backdrop.first_srgb } else { &mut backdrop.second_srgb };
        let r = ui_kit::color_field(ui, color);
        crate::scenario::publish_target(key, r.rect);
    }
    if current == BackdropMode::Radial {
        for (axis, key) in [(0, "render.backdrop_x"), (1, "render.backdrop_y")] {
            let label = app.tr(key).to_string();
            let Some(value) = app.render_workspace.view.studio.backdrop.center.get_mut(axis) else {
                continue;
            };
            ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, value, 0.0..=1.0, 0.01, ""));
        }
        let label = app.tr("render.backdrop_radius").to_string();
        ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, &mut app.render_workspace.view.studio.backdrop.radius, 0.05..=4.0, 0.01, ""));
    }
    if app.render_workspace.view.transparent {
        ui_kit::text(ui, app.tr("render.backdrop_alpha_conflict"), ui_kit::Text::Error("render.backdrop_alpha_conflict"));
    }
    if before != app.render_workspace.view.studio.backdrop {
        app.render_workspace.view.dirty = true;
    }
}

pub(super) fn export_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let exr = app.render_workspace.view.studio.export_exr;
    let key = if exr { "render.export_linear" } else { "render.export_display" };
    let label = app.tr(key).to_string();
    let r = ui_kit::dropdown_menu(ui, "render_export_format", &label, |ui| {
        for (value, key) in [(false, "render.export_display"), (true, "render.export_linear")] {
            if ui_kit::list_picker(ui, key, app.tr(key), value == exr).clicked() {
                app.render_workspace.view.studio.export_exr = value;
                ui.close();
            }
        }
    });
    crate::scenario::publish_target("render:export-format", r.rect);
    if app.render_workspace.view.studio.export_exr && !linear_export_available(app) {
        ui_kit::text(ui, app.tr("render.linear_unavailable"), ui_kit::Text::Note("render.linear_unavailable"));
    }
}
pub(super) fn linear_export_available(app: &SolveApp) -> bool {
    app.render_workspace
        .view
        .completed
        .and_then(|job| app.session.render_job_handle(job).ok())
        .and_then(|handle| handle.frame().ok().flatten())
        .is_some_and(|frame| frame.linear_exr().is_some())
}
