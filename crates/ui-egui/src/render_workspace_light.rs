//! Selected-light editor and transient visibility. Saved settings are edited only by commands.
use super::{ImageMethod, RenderSelection, cpu, setups, studio};
use crate::{SolveApp, ui_kit};
use serde_json::{Value, json};
use solidcraft_engine::doc::render_settings::{CpuJob, GpuJob, RenderJob, RenderSetup, SkyEnvironment};
use solidcraft_engine::render::job::{AreaLights, StudioLighting};
use solidcraft_engine::schema::render_lights::{Geometry, Light, Visibility};

#[derive(Clone, Debug, Default)]
pub(super) struct View {
    pub visibility: Visibility,
    scope: String,
    aim: Option<[f64; 3]>,
    draft: Option<Draft>,
    review: Option<Review>,
}

#[derive(Clone, Debug)]
struct Review {
    name: String,
    source: RenderSetup,
    setup: RenderSetup,
    revision: u64,
    document: std::sync::Weak<solidcraft_engine::doc::Document>,
    consent: bool,
}

#[derive(Clone, Debug)]
struct Draft {
    id: String,
    revision: u64,
    ev: f64,
    temperature: f64,
    softness: f64,
    azimuth: f64,
    elevation: f64,
    distance: f64,
    size: f64,
    height: f64,
    position: [f64; 3],
    direction: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
    radiance: [f64; 3],
    reference: f64,
    minimum: f64,
    aim: [f64; 3],
}

/// Lighting-only read snapshot also works with unsaved HDR paths and temporary bindings.
fn settings(app: &SolveApp) -> Result<RenderSetup, String> {
    let view = &app.render_workspace.view;
    let job = match view.method {
        ImageMethod::Raster => RenderJob::Raster { transparent_background: false },
        ImageMethod::CpuRaytrace => RenderJob::CpuRaytrace {
            options: CpuJob {
                lights: view.raytrace.lights.clone(),
                preset: view.raytrace.preset,
                punctual_lights: view.raytrace.punctual_lights.clone(),
                area_lights: view.raytrace.area_lights.clone(),
                ..Default::default()
            },
        },
        ImageMethod::GpuRaytrace => RenderJob::GpuPath {
            transparent_background: false,
            options: Box::new(GpuJob {
                lighting: if view.studio.lighting_enabled {
                    Some(StudioLighting {
                        sun: view.studio.sun_enabled.then_some(view.studio.sun),
                        environment: view.studio.ambient,
                        area_lights: AreaLights::new(&view.studio.area_lights).map_err(|e| e.to_string())?,
                    })
                } else {
                    None
                },
                // Environment illumination suppresses the legacy analytic studio; its bytes are not needed here.
                environment: (view.studio.environment != studio::Environment::None).then_some(SkyEnvironment::Sky {
                    gradient: Default::default(),
                    rotation_rad: 0.0,
                    intensity: 1.0,
                }),
                ..Default::default()
            }),
        },
    };
    Ok(RenderSetup { camera: app.cam, job: Some(job), ..Default::default() })
}

pub(super) fn rows(app: &SolveApp) -> Result<Vec<Light>, String> {
    solidcraft_engine::render_lights::inspect(&settings(app)?)
}

pub(super) fn name(app: &SolveApp, light: &Light) -> String {
    if light.number <= 3 { app.tr(&light.name_key).into() } else { crate::i18n::format(&light.name_key, &[("number", &light.number.to_string())]) }
}

pub(crate) fn validate_visibility(app: &SolveApp, params: &Value) -> Result<(), String> {
    if let Some(id) = params.get("id").and_then(Value::as_str)
        && !rows(app)?.iter().any(|light| light.id == id)
    {
        return Err(app.tr("render.light_missing").into());
    }
    Ok(())
}

pub(crate) fn apply_visibility(app: &mut SolveApp, command: &str, reply: &Value) {
    let visibility = &mut app.render_workspace.view.lights.visibility;
    let id = reply.get("id").and_then(Value::as_str);
    if command == "render.lights.solo" {
        visibility.solo = if reply.get("solo").and_then(Value::as_bool) == Some(true) { id.map(str::to_owned) } else { None };
    } else if let Some(id) = id {
        if reply.get("mute").and_then(Value::as_bool) == Some(true) {
            visibility.muted.insert(id.into());
        } else {
            visibility.muted.remove(id);
        }
    }
}

pub(super) fn loaded(app: &mut SolveApp) {
    if app.render_workspace.view.lights.scope != app.render_workspace.view.name {
        app.render_workspace.view.lights = View { scope: app.render_workspace.view.name.clone(), ..Default::default() };
    }
    if let Ok(lights) = rows(app) {
        let view = &mut app.render_workspace.view.lights;
        view.visibility.muted.retain(|id| lights.iter().any(|light| &light.id == id));
        if view.visibility.solo.as_ref().is_some_and(|id| !lights.iter().any(|light| &light.id == id)) {
            view.visibility.solo = None;
        }
    }
}

/// Runs after successful command dispatch, including programmatic control-channel edits.
pub(crate) fn edited(app: &mut SolveApp, params: &Value, reply: &Value) {
    let Some(name) = reply.get("name").and_then(Value::as_str) else { return };
    if name != app.render_workspace.view.name {
        return;
    }
    let visibility = app.render_workspace.view.lights.visibility.clone();
    setups::load(app);
    app.render_workspace.view.lights.visibility = visibility;
    loaded(app);
    if let Some(aim) = params.get("set").and_then(|set| set.get("aim_at")) {
        app.render_workspace.view.lights.aim = serde_json::from_value(aim.clone()).ok();
    }
    app.render_workspace.view.selection =
        reply.get("id").and_then(Value::as_str).map_or(RenderSelection::Nothing, |id| RenderSelection::Light(id.into()));
}

fn product_centre(app: &SolveApp) -> [f64; 3] {
    let bounds = solidcraft_engine::view::bounds(&app.session).unwrap_or(solidcraft_engine::geom::Aabb3::EMPTY);
    let point = if bounds.is_empty() { app.cam.target } else { bounds.center() };
    [point.x, point.y, point.z]
}

pub(super) fn edit(app: &mut SolveApp, command: &str, id: Option<&str>, mut set: Option<Value>) -> Result<(), String> {
    let snapshot = setups::snapshot(app)?;
    let mut request = json!({"name":app.render_workspace.view.name,"settings":snapshot,"expected_revision":app.session.revision});
    if let Some(id) = id {
        request["id"] = json!(id);
    }
    if let Some(patch) = set.as_mut().and_then(Value::as_object_mut) {
        let kind = patch.get("kind").and_then(Value::as_str).or_else(|| id.and_then(|id| id.split_once(':').map(|(kind, _)| kind))).unwrap_or("area");
        if kind != "directional"
            && ["kind", "azimuth_deg", "elevation_deg", "distance_mm"].iter().any(|key| patch.contains_key(*key))
            && !patch.contains_key("aim_at")
        {
            patch.insert("aim_at".into(), json!(app.render_workspace.view.lights.aim.unwrap_or_else(|| product_centre(app))));
        }
    }
    if let Some(set) = set {
        request["set"] = set;
    }
    app.run(command, request).map(|_| ())
}

/// Applied to copies at the request boundary, including engine.execute through SolveApp::run.
/// The draft and saved RenderSetup remain unfiltered, including while saving or undoing.
pub(crate) fn filter_request(app: &SolveApp, mut request: Value) -> Result<Value, String> {
    let visibility = &app.render_workspace.view.lights.visibility;
    if !visibility.active() {
        return Ok(request);
    }
    if request.is_null() {
        request = json!({});
    }
    if !request.is_object() {
        return Err("render request must be an object".into());
    }
    let mut setup = if let Some(inline) = request.get("settings").filter(|value| !value.is_null()) {
        serde_json::from_value::<RenderSetup>(inline.clone()).map_err(|e| e.to_string())?
    } else if let Some(name) = request.get("setup").and_then(Value::as_str) {
        app.session.doc.render.setups.get(name).cloned().ok_or_else(|| app.tr("render.light_missing").to_string())?
    } else {
        RenderSetup::default()
    };
    if setup.job.is_some() {
        if let Some(weights) = solidcraft_engine::render_lights::filter(&mut setup, visibility)? {
            request["studio_weights"] = json!(weights);
        }
        if let Some(object) = request.as_object_mut() {
            object.remove("setup");
        }
        request["settings"] = json!(setup);
    } else if request.get("progressive").and_then(Value::as_bool) == Some(true) {
        let gpu_lighting = request
            .get("lighting")
            .filter(|value| !value.is_null())
            .map(|value| serde_json::from_value(value.clone()).map_err(|e| e.to_string()))
            .transpose()?;
        let environment = request.get("environment").filter(|value| !value.is_null()).map(|_| SkyEnvironment::Sky {
            gradient: Default::default(),
            rotation_rad: 0.0,
            intensity: 1.0,
        });
        let mut lighting = RenderSetup {
            job: Some(RenderJob::GpuPath {
                transparent_background: false,
                options: Box::new(GpuJob { lighting: gpu_lighting, environment, ..Default::default() }),
            }),
            ..Default::default()
        };
        if let Some(weights) = solidcraft_engine::render_lights::filter(&mut lighting, visibility)? {
            request["studio_weights"] = json!(weights);
        }
        if let Some(RenderJob::GpuPath { options, .. }) = lighting.job {
            request["lighting"] = json!(options.lighting);
        }
    } else if request.get("integrator").and_then(Value::as_str) == Some("raytrace") {
        let options: solidcraft_engine::render::job::RaytraceOptions =
            serde_json::from_value(request.get("raytrace").filter(|value| !value.is_null()).cloned().unwrap_or_else(|| json!({})))
                .map_err(|e| e.to_string())?;
        let mut lighting = RenderSetup {
            job: Some(RenderJob::CpuRaytrace {
                options: CpuJob {
                    lights: options.lights.clone(),
                    preset: options.preset,
                    punctual_lights: options.punctual_lights.clone(),
                    area_lights: options.area_lights.clone(),
                    ..Default::default()
                },
            }),
            ..Default::default()
        };
        solidcraft_engine::render_lights::filter(&mut lighting, visibility)?;
        if let Some(RenderJob::CpuRaytrace { options: filtered }) = lighting.job {
            let mut options = options;
            options.lights = filtered.lights;
            options.preset = filtered.preset;
            options.punctual_lights = filtered.punctual_lights;
            options.area_lights = filtered.area_lights;
            request["raytrace"] = json!(options);
        }
    } else if let Some(weights) = solidcraft_engine::render_lights::filter(&mut setup, visibility)? {
        request["studio_weights"] = json!(weights);
    }
    Ok(request)
}

/// Unshadowed analytic preview: area sources use their centre and projected area.
/// Final rendering retains the full emitter and transport contracts.
pub(super) fn preview(app: &SolveApp) -> Result<Vec<[f32; 16]>, String> {
    let mut output = Vec::new();
    for light in rows(app)? {
        if !app.render_workspace.view.lights.visibility.visible(&light.id) {
            continue;
        }
        let mut value = [0.0_f32; 16];
        let (position, kind) = match light.geometry {
            Geometry::Directional { direction, .. } => (direction, 0.0),
            Geometry::Point { position_mm, .. } => (position_mm, 1.0),
            Geometry::Spot { position_mm, .. } => (position_mm, 2.0),
            Geometry::Area { position_mm, .. } => (position_mm, 3.0),
        };
        for (slot, v) in value.iter_mut().take(3).zip(position) {
            *slot = v as f32;
        }
        if let Some(slot) = value.get_mut(3) {
            *slot = kind;
        }
        for (slot, v) in value.iter_mut().skip(4).take(3).zip(light.colour) {
            *slot = (v * light.intensity) as f32;
        }
        let (axis, inner, outer, scale, minimum) = match light.geometry {
            Geometry::Point { reference_distance_mm, minimum_distance_mm, .. } => {
                ([0.0; 3], 0.0, 0.0, reference_distance_mm.powi(2), minimum_distance_mm)
            }
            Geometry::Spot {
                emitted_axis, inner_half_angle_radians, outer_half_angle_radians, reference_distance_mm, minimum_distance_mm, ..
            } => (emitted_axis, inner_half_angle_radians.cos(), outer_half_angle_radians.cos(), reference_distance_mm.powi(2), minimum_distance_mm),
            Geometry::Area { edge_u_mm: [ux, uy, uz], edge_v_mm: [vx, vy, vz], .. } => {
                let normal = solidcraft_engine::geom::Vec3::new(ux, uy, uz).cross(solidcraft_engine::geom::Vec3::new(vx, vy, vz));
                let area = normal.len();
                let n = solidcraft_engine::render::surface::unit_direction(normal).unwrap_or(-solidcraft_engine::geom::Vec3::Z);
                ([n.x, n.y, n.z], 0.0, 0.0, area, 0.001)
            }
            _ => ([0.0; 3], 0.0, 0.0, 1.0, 0.001),
        };
        for (slot, v) in value.iter_mut().skip(8).take(3).zip(axis) {
            *slot = v as f32;
        }
        if let Some(slot) = value.get_mut(7) {
            *slot = outer as f32;
        }
        if let Some(slot) = value.get_mut(11) {
            *slot = inner as f32;
        }
        if let Some(slot) = value.get_mut(12) {
            *slot = scale as f32;
        }
        if let Some(slot) = value.get_mut(13) {
            *slot = minimum as f32;
        }
        output.push(value);
    }
    let view = &app.render_workspace.view;
    let ambient = match view.method {
        ImageMethod::GpuRaytrace if view.studio.lighting_enabled => view.studio.ambient,
        ImageMethod::CpuRaytrace if view.cpu.environment == cpu::Environment::Constant => view.cpu.color.map(|c| c * view.cpu.intensity),
        _ => [0.0; 3],
    };
    if ambient.iter().any(|c| *c > 0.0) {
        let mut value = [0.0; 16];
        if let Some(slot) = value.get_mut(3) {
            *slot = 4.0;
        }
        for (slot, c) in value.iter_mut().skip(4).take(3).zip(ambient) {
            *slot = c as f32;
        }
        output.push(value);
    }
    Ok(output)
}

/// The group row and legacy Lighting page share this one editor.
pub(super) fn lighting(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::scenario::publish_target("render:lighting:editor", ui.available_rect_before_wrap());
    let method = app.render_workspace.view.method;
    let cpu = method == ImageMethod::CpuRaytrace;
    let gpu = method == ImageMethod::GpuRaytrace;
    let raster = method == ImageMethod::Raster;
    let lights = rows(app).unwrap_or_default();
    let legacy_gpu = gpu && lights.iter().filter(|l| l.geometry.kind() == "directional").count() > 1;
    if raster {
        note(app, ui, "render.method_limit_raster");
    }
    ui_kit::scope(ui, |ui| {
        let title = app.tr("render.lighting_source_section").to_string();
        ui_kit::section(ui, "render:lighting:source", &title, true, |ui| {
            let custom = if gpu { app.render_workspace.view.studio.lighting_enabled } else { app.render_workspace.view.raytrace.lights.is_some() };
            let mut chosen = usize::from(custom);
            let labels = [app.tr("render.lighting_preset").to_string(), app.tr("render.lighting_custom").to_string()];
            let reason = app.tr("render.lighting_preset_cpu").to_string();
            let legacy_reason = app.tr("render.light_backend_limit").to_string();
            let disabled = [(!cpu).then_some(reason.as_str()), legacy_gpu.then_some(legacy_reason.as_str())];
            ui.add_enabled_ui(!raster, |ui| {
                let label = app.tr("render.lighting_source").to_string();
                ui_kit::row(ui, &label, |ui| {
                    if ui_kit::segmented(
                        ui,
                        &mut chosen,
                        &labels.iter().map(String::as_str).collect::<Vec<_>>(),
                        ui_kit::Segments {
                            compact: true,
                            disabled: &disabled,
                            targets: &["render:lighting:preset", "render:lighting:custom"],
                            ..Default::default()
                        },
                    ) && chosen != usize::from(custom)
                    {
                        if chosen == 1 {
                            let result = (|| {
                                let mut snapshot = setups::snapshot(app)?;
                                solidcraft_engine::render_lights::write(&mut snapshot, &lights)?;
                                save_lighting(app, snapshot)
                            })();
                            if let Err(error) = result {
                                app.set_status(error, true);
                            }
                        } else {
                            review_preset(
                                app,
                                app.render_workspace.view.raytrace.preset.unwrap_or(solidcraft_engine::render::job::LightingPreset::Neutral),
                            );
                        }
                    }
                });
            });
            let presets = [
                (solidcraft_engine::render::job::LightingPreset::Neutral, "render.cpu_neutral"),
                (solidcraft_engine::render::job::LightingPreset::KeyRim, "render.cpu_key_rim"),
                (solidcraft_engine::render::job::LightingPreset::WarmCool, "render.cpu_warm_cool"),
            ];
            let selected = app.render_workspace.view.raytrace.preset.unwrap_or(solidcraft_engine::render::job::LightingPreset::Neutral);
            let selected_label = presets.iter().find(|(p, _)| *p == selected).map(|(_, key)| *key).unwrap_or("render.cpu_neutral");
            ui.add_enabled_ui(cpu && !custom, |ui| {
                let label = app.tr("render.lighting_preset_row").to_string();
                ui_kit::row(ui, &label, |ui| {
                    let selected_label = app.tr(selected_label).to_string();
                    ui_kit::dropdown_menu(ui, "render:lighting:presets", &selected_label, |ui| {
                        for (preset, key) in presets {
                            if ui_kit::list_picker(ui, key, app.tr(key), preset == selected).clicked() {
                                review_preset(app, preset);
                                ui.close();
                            }
                        }
                    });
                });
            });
            note(app, ui, "render.lighting_custom_help");
            if !cpu {
                note(app, ui, "render.lighting_preset_cpu");
            }
            if legacy_gpu {
                note(app, ui, "render.light_backend_limit");
            }
            // RenderSetup has no saved rig identity. Do not invent a Rig row.
        });
        let title = app.tr("render.lighting_shadows_section").to_string();
        ui_kit::section(ui, "render:lighting:shadows", &title, true, |ui| {
            let mut shadows = if cpu { app.render_workspace.view.raytrace.shadows } else { true };
            ui.add_enabled_ui(cpu, |ui| {
                if ui_kit::checkbox(ui, app.tr("render.lighting_shadows"), &mut shadows).changed() {
                    update_cpu(app, |options| options.shadows = shadows);
                }
            });
            if !cpu {
                note(app, ui, "render.lighting_shadow_cpu");
            }
            if cpu {
                let enabled = app.render_workspace.view.raytrace.area_lights.as_ref().is_some_and(|l| !l.is_empty());
                let mut samples = app.render_workspace.view.raytrace.area_samples.unwrap_or(16);
                ui.add_enabled_ui(enabled, |ui| {
                    let label = app.tr("render.lighting_area_samples").to_string();
                    ui_kit::row(ui, &label, |ui| {
                        let response = ui_kit::number_field(ui, &mut samples, 1..=64, 1.0, "");
                        crate::scenario::publish_target("render:lighting:area-samples", response.rect);
                        if accepted(&response) {
                            if [1, 4, 16, 64].contains(&samples) {
                                update_cpu(app, |options| {
                                    options.area_samples = Some(samples);
                                });
                            } else {
                                app.set_status(app.tr("render.lighting_samples_reason").to_string(), true);
                            }
                        }
                    });
                });
                note(app, ui, if enabled { "render.lighting_samples_reason" } else { "render.lighting_samples_need_area" });
            } else {
                note(app, ui, "render.lighting_samples_cpu");
            }
        });
        let title = app.tr("render.lighting_budget").to_string();
        ui_kit::section(ui, "render:lighting:budget", &title, true, |ui| {
            for (kind, key, limit) in [
                ("directional", if gpu { "render.light_type_sun" } else { "render.tree_directional" }, if gpu { 1 } else { 8 }),
                ("area", "render.light_type_area", 4),
                ("point", "render.light_type_point", 8),
            ] {
                let count = lights
                    .iter()
                    .filter(|l| if kind == "point" { matches!(l.geometry.kind(), "point" | "spot") } else { l.geometry.kind() == kind })
                    .count();
                let unsupported = raster || (gpu && kind == "point");
                let label = app.tr(key).to_string();
                ui.add_enabled_ui(!unsupported, |ui| {
                    ui_kit::row(ui, &label, |ui| {
                        ui.columns(2, |columns| {
                            for (index, column) in columns.iter_mut().enumerate() {
                                if index == 0 {
                                    let mut used = count.min(limit) as u32;
                                    column.add_enabled_ui(false, |ui| {
                                        ui_kit::slider(
                                            ui,
                                            &mut used,
                                            0..=limit as u32,
                                            ui_kit::Slider { integer: true, show_value: false, ..Default::default() },
                                        );
                                    });
                                } else {
                                    let readout = if unsupported {
                                        app.tr("render.lighting_unsupported").to_string()
                                    } else {
                                        crate::i18n::format("render.lighting_used", &[("used", &count.to_string()), ("limit", &limit.to_string())])
                                    };
                                    column.add_enabled_ui(!unsupported, |ui| ui_kit::text(ui, &readout, ui_kit::Text::Inline));
                                }
                            }
                        });
                    })
                });
                if gpu && kind == "point" {
                    note(app, ui, "render.light_gpu_punctual");
                }
            }
            note(app, ui, "render.lighting_budget_help");
            add_menu(app, ui);
        });
    });
}

pub(super) fn add_menu(app: &mut SolveApp, ui: &mut egui::Ui) {
    let gpu = app.render_workspace.view.method == ImageMethod::GpuRaytrace;
    let raster = app.render_workspace.view.method == ImageMethod::Raster;
    let lights = rows(app).unwrap_or_default();
    let legacy = gpu && lights.iter().filter(|l| l.geometry.kind() == "directional").count() > 1;
    let label = app.tr("render.tree_add_light").to_string();
    ui.add_enabled_ui(!raster && !legacy, |ui| {
        ui_kit::dropdown_menu(ui, "render:lighting:add", &label, |ui| {
            for (kind, key, cap) in [
                ("area", "render.light_type_area", 4),
                ("point", "render.light_type_point", 8),
                ("directional", if gpu { "render.light_type_sun" } else { "render.tree_directional" }, if gpu { 1 } else { 8 }),
            ] {
                let count = lights
                    .iter()
                    .filter(|l| if kind == "point" { matches!(l.geometry.kind(), "point" | "spot") } else { l.geometry.kind() == kind })
                    .count();
                let enabled = count < cap && !(gpu && kind == "point");
                let response = ui.add_enabled_ui(enabled, |ui| ui_kit::list_picker(ui, kind, app.tr(key), false)).inner;
                if response
                    .on_hover_text(app.tr(if gpu && kind == "point" { "render.light_gpu_punctual" } else { "render.lighting_budget_help" }))
                    .clicked()
                {
                    dispatch(app, "render.lights.add", None, Some(json!({"kind":kind})));
                    ui.close();
                }
            }
        });
    });
    if legacy {
        note(app, ui, "render.light_backend_limit");
    }
}

fn review_preset(app: &mut SolveApp, preset: solidcraft_engine::render::job::LightingPreset) {
    match setups::snapshot(app) {
        Ok(mut setup) => {
            let source = setup.clone();
            if let Some(RenderJob::CpuRaytrace { options }) = &mut setup.job {
                options.lights = None;
                options.preset = Some(preset);
                app.render_workspace.view.lights.review = Some(Review {
                    name: app.render_workspace.view.name.clone(),
                    source,
                    setup,
                    revision: app.session.revision,
                    document: std::sync::Arc::downgrade(&app.session.doc),
                    consent: false,
                });
            }
        }
        Err(error) => app.set_status(error, true),
    }
}

fn save_lighting(app: &mut SolveApp, setup: RenderSetup) -> Result<(), String> {
    let selection = app.render_workspace.view.selection.clone();
    let visibility = app.render_workspace.view.lights.visibility.clone();
    app.run("render.settings.set", json!({"name":app.render_workspace.view.name,"set":setup}))?;
    setups::load(app);
    app.render_workspace.view.selection = selection;
    app.render_workspace.view.lights.visibility = visibility;
    loaded(app);
    Ok(())
}
fn update_cpu(app: &mut SolveApp, update: impl FnOnce(&mut CpuJob)) {
    let result = (|| {
        let mut setup = setups::snapshot(app)?;
        if let Some(RenderJob::CpuRaytrace { options }) = &mut setup.job {
            update(options);
        }
        save_lighting(app, setup)
    })();
    if let Err(error) = result {
        app.set_status(error, true);
    }
}

pub(super) fn confirmation(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut review) = app.render_workspace.view.lights.review.take() else { return };
    if review.revision != app.session.revision
        || review.name != app.render_workspace.view.name
        || setups::snapshot(app).ok().as_ref() != Some(&review.source)
        || !std::sync::Weak::ptr_eq(&review.document, &std::sync::Arc::downgrade(&app.session.doc))
    {
        app.set_status(app.tr("render.review_stale").to_string(), true);
        return;
    }
    let title = app.tr("render.lighting_review_title").to_string();
    let apply = app.tr("render.lighting_apply").to_string();
    let cancel = app.tr("render.keep_editing").to_string();
    let (closed, action) = ui_kit::Confirm::new(egui::Id::new("render_lighting_review"), "render_scene", &title)
        .actions(&apply, &cancel, None)
        .selected(ui_kit::Action::Cancel)
        .show(ctx, review.consent, |ui| {
            note(app, ui, "render.lighting_review_help");
            ui_kit::checkbox(ui, app.tr("render.lighting_consent"), &mut review.consent);
        });
    match action {
        ui_kit::Action::Ok if review.consent => {
            if let Err(error) = save_lighting(app, review.setup) {
                app.set_status(error, true);
            }
        }
        ui_kit::Action::Cancel => {}
        _ if closed => {}
        _ => app.render_workspace.view.lights.review = Some(review),
    }
}
pub(super) fn editor(app: &mut SolveApp, ui: &mut egui::Ui, id: &str) {
    let lights = match rows(app) {
        Ok(lights) => lights,
        Err(error) => {
            ui_kit::text(ui, &error, ui_kit::Text::Error("render.light_invalid"));
            return;
        }
    };
    let Some(light) = lights.iter().find(|light| light.id == id) else {
        note(app, ui, "render.light_missing");
        return;
    };
    crate::scenario::publish_target("render:light:editor", ui.available_rect_before_wrap());
    let kind = light.geometry.kind();
    let gpu = app.render_workspace.view.method == ImageMethod::GpuRaytrace;
    let blocked = app.render_workspace.view.method == ImageMethod::Raster
        || (gpu && lights.iter().filter(|light| light.geometry.kind() == "directional").count() > 1);
    let maximum_ev = if gpu { if kind == "area" { 1e6_f64.log2() } else { 1e8_f64.log2() } } else { 6.0 };
    let type_key = match kind {
        "area" => "render.light_type_area",
        "spot" => "render.light_type_spot",
        "point" => "render.light_type_point",
        _ => "render.light_type_sun",
    };
    let aim = app.render_workspace.view.lights.aim.unwrap_or_else(|| product_centre(app));
    let subject_size = subject_size(app);
    let (azimuth, elevation, distance) = solidcraft_engine::render_lights::position(light, aim, app.cam.distance);
    let (position, direction, u, v, reference, minimum) = match light.geometry {
        Geometry::Directional { direction, .. } => ([0.0; 3], direction, [0.0; 3], [0.0; 3], 1.0, 0.001),
        Geometry::Area { position_mm, edge_u_mm, edge_v_mm } => (position_mm, [0.0; 3], edge_u_mm, edge_v_mm, 1.0, 0.001),
        Geometry::Point { position_mm, reference_distance_mm, minimum_distance_mm }
        | Geometry::Spot { position_mm, reference_distance_mm, minimum_distance_mm, .. } => {
            (position_mm, [0.0; 3], [0.0; 3], [0.0; 3], reference_distance_mm, minimum_distance_mm)
        }
    };
    let width = length(u);
    let softness = match light.geometry {
        Geometry::Directional { angular_radius_rad: Some(radius), .. } => radius.to_degrees(),
        Geometry::Area { .. } => (width - 0.0002) / 1000.0,
        _ => 0.0,
    };
    let mut draft =
        app.render_workspace.view.lights.draft.take().filter(|draft| draft.id == id && draft.revision == app.session.revision).unwrap_or(Draft {
            id: id.into(),
            revision: app.session.revision,
            ev: bounded(light.intensity.max(2.0_f64.powi(-20)).log2(), -20.0, maximum_ev),
            temperature: solidcraft_engine::render_lights::colour_temperature(light.colour),
            softness: bounded(softness, 0.0, if kind == "directional" { 90.0 } else { 1.0 }),
            azimuth,
            elevation,
            distance: bounded(distance / subject_size, 0.001 / subject_size, 1e6 / subject_size),
            size: width,
            height: length(v),
            position,
            direction,
            u,
            v,
            radiance: light.colour.map(|c| c * light.intensity),
            reference,
            minimum,
            aim,
        });
    ui_kit::scope(ui, |ui| {
        ui_kit::row(ui, &name(app, light), |ui| {
            ui.add_enabled_ui(false, |ui| ui_kit::choice_chip(ui, "render:light:badge", app.tr(type_key), true));
        });
        if blocked {
            note(app, ui, "render.light_backend_limit");
        }
        ui.add_enabled_ui(!blocked, |ui| {
            let labels = [
                app.tr("render.light_type_area").to_string(),
                app.tr("render.light_type_point").to_string(),
                app.tr("render.light_type_sun").to_string(),
            ];
            let kinds = ["area", "point", "directional"];
            let mut selected = kinds.iter().position(|value| *value == kind).unwrap_or(1);
            let reason = app.tr("render.light_gpu_punctual").to_string();
            let budget_reason = app.tr("render.lighting_budget_help").to_string();
            let areas = lights.iter().filter(|l| l.geometry.kind() == "area").count();
            let points = lights.iter().filter(|l| matches!(l.geometry.kind(), "point" | "spot")).count();
            let suns = lights.iter().filter(|l| l.geometry.kind() == "directional").count();
            let disabled = [
                (kind != "area" && areas >= 4).then_some(budget_reason.as_str()),
                if gpu { Some(reason.as_str()) } else { (kind != "point" && kind != "spot" && points >= 8).then_some(budget_reason.as_str()) },
                (kind != "directional" && suns >= if gpu { 1 } else { 8 }).then_some(budget_reason.as_str()),
            ];
            if ui_kit::segmented(
                ui,
                &mut selected,
                &labels.iter().map(String::as_str).collect::<Vec<_>>(),
                ui_kit::Segments {
                    compact: true,
                    disabled: &disabled,
                    targets: &["render:light:type:area", "render:light:type:point", "render:light:type:directional"],
                    ..Default::default()
                },
            ) && let Some(chosen) = kinds.get(selected)
                && *chosen != kind
            {
                dispatch(app, "render.lights.set", Some(id), Some(json!({"kind":chosen})));
            }
            if gpu {
                note(app, ui, "render.light_gpu_punctual");
            }
            if kind == "spot" {
                note(app, ui, "render.lighting_spot_preserved");
            }
            slider_field(app, ui, id, "render.light_intensity", "intensity_ev", &mut draft.ev, -20.0..=maximum_ev, |v| format!("{v:+.1} EV"), true);
            slider_field(
                app,
                ui,
                id,
                "render.lighting_colour",
                "temperature_k",
                &mut draft.temperature,
                1000.0..=40000.0,
                |v| format!("{v:.0} K"),
                true,
            );
            note(app, ui, "render.light_temperature_help");
            let soft = kind == "area" || (kind == "directional" && gpu);
            let sun = kind == "directional";
            // A sun displays its actual angular radius; areas display the size adapter.
            slider_field(
                app,
                ui,
                id,
                "render.light_softness",
                if sun { "angular_radius_deg" } else { "softness" },
                &mut draft.softness,
                if sun { 0.001_f64.to_degrees()..=90.0 } else { 0.0..=1.0 },
                if sun { |v| format!("{v:.2}°") } else { |v| format!("{v:.2}") },
                soft,
            );
            if !soft {
                note(app, ui, "render.light_hard_shadow");
            }
            let title = app.tr("render.light_position").to_string();
            ui_kit::section(ui, "render:light:position", &title, true, |ui| {
                field(app, ui, id, "render.light_azimuth", "azimuth_deg", &mut draft.azimuth, -360.0..=360.0, 1.0, "°", true);
                field(app, ui, id, "render.light_elevation", "elevation_deg", &mut draft.elevation, -90.0..=90.0, 1.0, "°", true);
                let distance_unit = app.tr("render.lighting_distance_unit").to_string();
                field(
                    app,
                    ui,
                    id,
                    "render.light_distance",
                    "distance_size",
                    &mut draft.distance,
                    0.001 / subject_size..=1e6 / subject_size,
                    0.1,
                    &distance_unit,
                    !sun,
                );
                if sun {
                    note(app, ui, "render.lighting_sun_distance");
                }
                ui.add_enabled_ui(!sun, |ui| {
                    let label = app.tr("render.light_aim").to_string();
                    ui_kit::row(ui, &label, |ui| {
                        let label =
                            app.tr(if aim == product_centre(app) { "render.light_product_centre" } else { "render.lighting_point" }).to_string();
                        ui_kit::dropdown_menu(ui, "render:light:aim", &label, |ui| {
                            let bounds = solidcraft_engine::view::bounds(&app.session).unwrap_or(solidcraft_engine::geom::Aabb3::EMPTY);
                            let mut targets = vec![
                                (app.tr("render.light_product_centre").to_string(), product_centre(app)),
                                (app.tr("render.light_world_origin").to_string(), [0.0; 3]),
                            ];
                            if !bounds.is_empty() {
                                for body in &crate::open_meshing::session_world(&app.session).bodies {
                                    let bounds = body.mesh_bounds();
                                    let c = bounds.center();
                                    targets.push((body.name.clone(), [c.x, c.y, c.z]));
                                }
                            }
                            for (label, target) in targets {
                                if ui_kit::list_picker(ui, &label, &label, aim == target).clicked() {
                                    dispatch(app, "render.lights.set", Some(id), Some(json!({"aim_at":target})));
                                    ui.close();
                                }
                            }
                        });
                    });
                    vector_field(app, ui, id, "render.lighting_aim_point", "aim_at", &mut draft.aim, -1e9..=1e9, " mm");
                });
                let response = ui_kit::button(ui, app.tr("render.light_place_highlight"), ui_kit::Button::Enabled(false));
                crate::scenario::publish_target("render:light:place-highlight", response.rect);
                response.on_hover_text(app.tr("render.light_place_pending"));
                note(app, ui, "render.light_place_pending");
                if sun {
                    note(app, ui, "render.lighting_direction_help");
                    vector_field(app, ui, id, "render.lighting_direction", "direction", &mut draft.direction, -1e6..=1e6, "");
                } else {
                    vector_field(
                        app,
                        ui,
                        id,
                        "render.lighting_world_position",
                        "position_mm",
                        &mut draft.position,
                        if gpu || matches!(kind, "point" | "spot") { -1e9..=1e9 } else { -1e12..=1e12 },
                        " mm",
                    );
                }
            });
            if kind == "area" {
                let title = app.tr("render.lighting_shape").to_string();
                ui_kit::section(ui, "render:light:shape", &title, true, |ui| {
                    field(
                        app,
                        ui,
                        id,
                        "render.lighting_width",
                        "width_mm",
                        &mut draft.size,
                        if gpu { 0.0002..=2e6 } else { 0.000002..=1e6 },
                        1.0,
                        " mm",
                        true,
                    );
                    field(
                        app,
                        ui,
                        id,
                        "render.lighting_height",
                        "height_mm",
                        &mut draft.height,
                        if gpu { 0.0002..=2e6 } else { 0.000002..=1e6 },
                        1.0,
                        " mm",
                        true,
                    );
                    vector_field(app, ui, id, "render.lighting_radiance", "radiance", &mut draft.radiance, 0.0..=if gpu { 1e6 } else { 64.0 }, "");
                    vector_field(app, ui, id, "render.lighting_edge_u", "edge_u_mm", &mut draft.u, if gpu { -2e6..=2e6 } else { -1e6..=1e6 }, " mm");
                    vector_field(app, ui, id, "render.lighting_edge_v", "edge_v_mm", &mut draft.v, if gpu { -2e6..=2e6 } else { -1e6..=1e6 }, " mm");
                });
            }
            if matches!(kind, "point" | "spot") {
                let title = app.tr("render.lighting_falloff").to_string();
                ui_kit::section(ui, "render:light:falloff", &title, true, |ui| {
                    field(
                        app,
                        ui,
                        id,
                        "render.lighting_reference",
                        "reference_distance_mm",
                        &mut draft.reference,
                        draft.minimum..=1e6,
                        1.0,
                        " mm",
                        true,
                    );
                    field(
                        app,
                        ui,
                        id,
                        "render.lighting_minimum",
                        "minimum_distance_mm",
                        &mut draft.minimum,
                        0.001..=draft.reference,
                        1.0,
                        " mm",
                        true,
                    );
                    note(app, ui, "render.lighting_falloff_help");
                });
            }
        });
        app.render_workspace.view.lights.draft = Some(draft);
        let title = app.tr("render.light_behaviour").to_string();
        ui_kit::section(ui, "render:light:behaviour", &title, true, |ui| {
            let mut solo = app.render_workspace.view.lights.visibility.solo.as_deref() == Some(id);
            if ui_kit::checkbox(ui, app.tr("render.lighting_solo"), &mut solo).changed() {
                let _ = app.run("render.lights.solo", json!({"id":id,"solo":solo}));
            }
            let mut muted = app.render_workspace.view.lights.visibility.muted.contains(id);
            if ui_kit::checkbox(ui, app.tr("render.lighting_mute"), &mut muted).changed() {
                let _ = app.run("render.lights.mute", json!({"id":id,"mute":muted}));
            }
            ui.horizontal(|ui| {
                let cap = if kind == "area" {
                    4
                } else if gpu {
                    1
                } else {
                    8
                };
                let count = lights
                    .iter()
                    .filter(
                        |l| if matches!(kind, "point" | "spot") { matches!(l.geometry.kind(), "point" | "spot") } else { l.geometry.kind() == kind },
                    )
                    .count();
                if ui_kit::button(ui, app.tr("render.light_duplicate"), ui_kit::Button::Enabled(!blocked && count < cap))
                    .on_hover_text(app.tr("render.lighting_budget_help"))
                    .clicked()
                {
                    dispatch(app, "render.lights.duplicate", Some(id), None);
                }
                if ui_kit::button(ui, app.tr("render.light_delete"), ui_kit::Button::Enabled(!blocked)).clicked() {
                    dispatch(app, "render.lights.remove", Some(id), None);
                }
            });
            note(app, ui, "render.light_view_only");
        });
    });
}

fn bounded(value: f64, low: f64, high: f64) -> f64 {
    if value.is_finite() { value.clamp(low, high) } else { low }
}
fn length([x, y, z]: [f64; 3]) -> f64 {
    x.hypot(y).hypot(z)
}
fn subject_size(app: &SolveApp) -> f64 {
    bounded(solidcraft_engine::view::bounds(&app.session).unwrap_or(solidcraft_engine::geom::Aabb3::EMPTY).diagonal(), 1.0, 1e6)
}
fn note(app: &SolveApp, ui: &mut egui::Ui, key: &'static str) {
    ui_kit::text(ui, app.tr(key), ui_kit::Text::Note(key));
}

#[allow(clippy::too_many_arguments)]
fn slider_field(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    key: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    format: fn(f64) -> String,
    enabled: bool,
) {
    let label = app.tr(label).to_string();
    ui.add_enabled_ui(enabled, |ui| {
        ui_kit::row(ui, &label, |ui| {
            let response = ui_kit::slider(ui, value, range.clone(), ui_kit::Slider { format, ..Default::default() });
            crate::scenario::publish_target(&format!("render:light:{key}"), response.rect);
            if accepted(&response) && value.is_finite() && range.contains(value) {
                let patch = if key == "angular_radius_deg" {
                    json!({"softness":(value.to_radians() - 0.001) / (std::f64::consts::FRAC_PI_2 - 0.001)})
                } else {
                    json!({(key):*value})
                };
                dispatch(app, "render.lights.set", Some(id), Some(patch));
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn vector_field(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    key: &str,
    value: &mut [f64; 3],
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) {
    let label = app.tr(label).to_string();
    ui_kit::row(ui, &label, |ui| {
        let mut changed = false;
        ui.horizontal(|ui| {
            for (index, component) in value.iter_mut().enumerate() {
                let response = ui_kit::number_field(ui, component, range.clone(), 0.1, suffix);
                crate::scenario::publish_target(&format!("render:light:{key}:{index}"), response.rect);
                changed |= accepted(&response);
            }
        });
        if changed && value.iter().all(|v| v.is_finite() && range.contains(v)) {
            dispatch(app, "render.lights.set", Some(id), Some(json!({(key):*value})));
        }
    });
}
fn accepted(response: &egui::Response) -> bool {
    response.drag_stopped() || (response.changed() && (response.has_focus() || response.lost_focus()))
}
fn dispatch(app: &mut SolveApp, command: &str, id: Option<&str>, set: Option<Value>) {
    if let Err(error) = edit(app, command, id, set) {
        app.set_status(error, true);
    }
}

#[allow(clippy::too_many_arguments)]
fn field(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    key: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
    suffix: &str,
    enabled: bool,
) {
    let label = app.tr(label).to_string();
    ui.add_enabled_ui(enabled, |ui| {
        ui_kit::row(ui, &label, |ui| {
            let response = ui_kit::number_field(ui, value, range.clone(), speed, suffix);
            crate::scenario::publish_target(&format!("render:light:{key}"), response.rect);
            if accepted(&response) && value.is_finite() && range.contains(value) {
                let patch = if key == "distance_size" { json!({"distance_mm":*value * subject_size(app)}) } else { json!({(key):*value}) };
                dispatch(app, "render.lights.set", Some(id), Some(patch));
            }
        })
    });
}
