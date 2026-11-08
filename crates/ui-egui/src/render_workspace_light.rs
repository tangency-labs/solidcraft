//! Selected-light editor and transient visibility. Saved settings are edited only by commands.
use super::{ImageMethod, RenderSelection, cpu, setups, studio};
use crate::ui_kit::BackingTooltipExt as _;
use crate::{SolveApp, ui_kit};
use serde_json::{Value, json};
use solidcraft_engine::doc::render_settings::{CpuJob, GpuJob, RenderJob, RenderSetup, SkyEnvironment};
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::render::job::{AreaLights, StudioLighting};
use solidcraft_engine::schema::render_lights::{Geometry, Light, LightIdRemap, Visibility};

#[derive(Clone, Debug, Default)]
pub(super) struct View {
    pub visibility: Visibility,
    scope: String,
    aim: Option<[f64; 3]>,
    draft: Option<Draft>,
    review: Option<Review>,
    source: Option<Source>,
    numeric_history: Option<NumericHistory>,
}

#[derive(Clone, Debug)]
struct Source {
    settings: std::sync::Weak<solidcraft_engine::doc::render_settings::RenderSettings>,
    method: ImageMethod,
}
#[derive(Clone, Debug)]
struct NumericHistory {
    before: std::sync::Weak<solidcraft_engine::doc::render_settings::RenderSettings>,
    after: std::sync::Weak<solidcraft_engine::doc::render_settings::RenderSettings>,
}
impl NumericHistory {
    fn connects(&self, a: &Source, b: &Source) -> bool {
        a.method == b.method
            && ((self.before.ptr_eq(&a.settings) && self.after.ptr_eq(&b.settings))
                || (self.after.ptr_eq(&a.settings) && self.before.ptr_eq(&b.settings)))
    }
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
    emitted_axis: [f64; 3],
    inner_angle: f64,
    outer_angle: f64,
}

/// Lighting-only read snapshot also works with unsaved HDR paths and temporary bindings.
fn settings(app: &SolveApp) -> Result<RenderSetup, String> {
    let view = &app.render_workspace.view;
    let job = match view.method {
        ImageMethod::Raster => RenderJob::Raster { transparent_background: false },
        ImageMethod::CpuRaytrace => RenderJob::CpuRaytrace {
            transparent_background: false,
            options: Box::new(CpuJob {
                budget: view.raytrace.budget,
                sun: view.studio.sun_enabled.then_some(view.studio.sun),
                path: view.raytrace.path,
                progressive: view.raytrace.progressive,
                primary_samples: view.raytrace.primary_samples,
                shadows: view.raytrace.shadows,
                lights: view.raytrace.lights.clone(),
                preset: view.raytrace.preset,
                punctual_lights: view.raytrace.punctual_lights.clone(),
                area_lights: view.raytrace.area_lights.clone(),
                ..Default::default()
            }),
        },
        ImageMethod::GpuRaytrace => RenderJob::GpuPath {
            transparent_background: false,
            options: Box::new(GpuJob {
                lighting: if view.studio.lighting_enabled {
                    Some(StudioLighting {
                        punctual_lights: view.studio.punctual_lights,
                        directional_lights: view.studio.directional_lights,
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
    Ok(RenderSetup { camera: app.cam, width: 1, height: 1, job: Some(job), ..Default::default() })
}

/// Copy the common analytic rig without inventing a backend's implicit studio.
/// This is a pure prospective conversion; the caller publishes only after full validation.
pub(super) fn celestial_method(app: &SolveApp, next: &mut super::ViewState, method: ImageMethod) -> Result<(), String> {
    let sources = rows(app)?;
    let mut setup = RenderSetup {
        camera: app.cam,
        width: 1,
        height: 1,
        job: Some(match method {
            ImageMethod::CpuRaytrace => RenderJob::CpuRaytrace {
                transparent_background: false,
                options: Box::new(CpuJob {
                    budget: next.raytrace.budget,
                    sun: next.studio.sun_enabled.then_some(next.studio.sun),
                    path: next.raytrace.path,
                    progressive: next.raytrace.progressive,
                    primary_samples: next.raytrace.primary_samples,
                    shadows: next.raytrace.shadows,
                    ..Default::default()
                }),
            },
            ImageMethod::GpuRaytrace => RenderJob::GpuPath {
                transparent_background: false,
                options: Box::new(GpuJob {
                    lighting: Some(StudioLighting { sun: next.studio.sun_enabled.then_some(next.studio.sun), ..Default::default() }),
                    ..Default::default()
                }),
            },
            ImageMethod::Raster => return Ok(()),
        }),
        ..Default::default()
    };
    solidcraft_engine::render_lights::write(&mut setup, &sources)?;
    let converted = solidcraft_engine::render_lights::inspect(&setup)?;
    if sources.len() != converted.len()
        || sources.iter().any(|source| {
            !converted.iter().any(|target| {
                source.id == target.id
                    && source.geometry.kind() == target.geometry.kind()
                    && matches!(source.geometry, Geometry::Directional { angular_radius_rad: Some(_), .. })
                        == matches!(target.geometry, Geometry::Directional { angular_radius_rad: Some(_), .. })
            })
        })
    {
        return Err(app.tr("render.cpu_celestial_switch").into());
    }
    match setup.job {
        Some(RenderJob::CpuRaytrace { options, .. }) => {
            next.raytrace.sun = options.sun.map(Box::new);
            next.studio.sun_enabled = options.sun.is_some();
            if let Some(sun) = options.sun {
                next.studio.sun = sun;
            }
            next.raytrace.preset = options.preset;
            next.raytrace.lights = options.lights;
            next.raytrace.punctual_lights = options.punctual_lights;
            next.raytrace.area_lights = options.area_lights;
        }
        Some(RenderJob::GpuPath { options, .. }) => {
            if let Some(lighting) = options.lighting {
                next.studio.lighting_enabled = true;
                next.studio.directional_lights = lighting.directional_lights;
                next.studio.punctual_lights = lighting.punctual_lights;
                next.studio.area_lights = lighting.area_lights.iter().copied().collect();
                next.studio.sun_enabled = lighting.sun.is_some();
                if let Some(sun) = lighting.sun {
                    next.studio.sun = sun;
                }
            }
        }
        _ => return Err(app.tr("render.cpu_celestial_switch").into()),
    }
    Ok(())
}

/// The caller has just executed celestial_method's address-preserving conversion.
/// Preserve transient visibility only when its prior Arc/method source is proven;
/// equal row values or counts alone never establish identity.
pub(super) fn celestial_method_applied(
    app: &mut SolveApp,
    before: &std::sync::Weak<solidcraft_engine::doc::render_settings::RenderSettings>,
    method: ImageMethod,
) {
    let valid = app.render_workspace.view.lights.scope == app.render_workspace.view.name
        && app.render_workspace.view.lights.source.as_ref().is_some_and(|source| source.method == method && source.settings.ptr_eq(before));
    let source = Source { settings: std::sync::Arc::downgrade(&app.session.doc.render), method: app.render_workspace.view.method };
    let scope = app.render_workspace.view.name.clone();
    let view = &mut app.render_workspace.view.lights;
    view.scope = scope;
    view.source = Some(source);
    view.numeric_history = None;
    view.draft = None;
    if !valid {
        view.visibility = Visibility::default();
        if matches!(app.render_workspace.view.selection, RenderSelection::Light(_)) {
            app.render_workspace.view.selection = RenderSelection::Nothing;
        }
    }
}

pub(super) fn rows(app: &SolveApp) -> Result<Vec<Light>, String> {
    solidcraft_engine::render_lights::inspect(&settings(app)?)
}

pub(super) fn name(app: &SolveApp, light: &Light) -> String {
    if light.number <= 3 {
        app.tr(&light.name_key).into()
    } else {
        solidcraft_ui_text::format(&light.name_key, &[("number", &light.number.to_string())])
    }
}

/// A known volume-only/transport adoption preserves the analytic rig's addresses.
/// Equal values alone are insufficient: prove both the cached pre-command Arc
/// and its saved rows, then retain only this one exact Undo/Redo edge.
pub(super) fn volume_edited(
    app: &mut SolveApp,
    before: &std::sync::Weak<solidcraft_engine::doc::render_settings::RenderSettings>,
    method: ImageMethod,
    draft: &RenderSetup,
    rig: &[Light],
) {
    let valid = app.render_workspace.view.method == method
        && app.render_workspace.view.lights.scope == app.render_workspace.view.name
        && app.render_workspace.view.lights.source.as_ref().is_some_and(|source| source.method == method && source.settings.ptr_eq(before))
        && before
            .upgrade()
            .and_then(|settings| {
                // A first numerical edit creates the setup from this exact
                // controller draft; no unknown source replacement is inferred.
                let setup = settings.setups.get(&app.render_workspace.view.name).unwrap_or(draft);
                solidcraft_engine::render_lights::inspect(setup).ok()
            })
            .as_deref()
            == Some(rig)
        && rows(app).ok().as_deref() == Some(rig);
    if !valid {
        loaded(app);
        return;
    }
    let after = std::sync::Arc::downgrade(&app.session.doc.render);
    let view = &mut app.render_workspace.view.lights;
    if !before.ptr_eq(&after) {
        view.numeric_history = Some(NumericHistory { before: before.clone(), after: after.clone() });
    }
    view.source = Some(Source { settings: after, method });
    view.draft = None;
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
    let scope_changed = app.render_workspace.view.lights.scope != app.render_workspace.view.name;
    if scope_changed {
        app.render_workspace.view.lights = View { scope: app.render_workspace.view.name.clone(), ..Default::default() };
    }
    let source = Source { settings: std::sync::Arc::downgrade(&app.session.doc.render), method: app.render_workspace.view.method };
    let view = &mut app.render_workspace.view.lights;
    let unknown = scope_changed
        || view.source.as_ref().is_some_and(|previous| {
            !(previous.method == source.method && previous.settings.ptr_eq(&source.settings))
                && !view.numeric_history.as_ref().is_some_and(|proof| proof.connects(previous, &source))
        });
    view.source = Some(source);
    if unknown {
        // Undo/Redo, checkpoint and arbitrary replacements do not carry an edit
        // identity map. Never infer identity from equal kind/count/value lists.
        view.visibility = Visibility::default();
        view.draft = None;
        if matches!(app.render_workspace.view.selection, RenderSelection::Light(_)) {
            app.render_workspace.view.selection = RenderSelection::Nothing;
        }
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
pub(crate) fn edited(app: &mut SolveApp, command: &str, params: &Value, reply: &Value) {
    let Some(name) = reply.get("name").and_then(Value::as_str) else { return };
    if name != app.render_workspace.view.name {
        return;
    }
    let before = rows(app).unwrap_or_default();
    let remap = reply
        .get("id_remap")
        .filter(|value| value.as_array().is_some_and(|array| array.len() <= 20))
        .and_then(|value| serde_json::from_value::<Vec<LightIdRemap>>(value.clone()).ok())
        .unwrap_or_default();
    let remap_id = |id: &str| {
        remap.iter().find(|entry| entry.before.id == id && before.iter().any(|light| light == &entry.before)).map(|entry| entry.after_id.clone())
    };
    // One bounded provenance proof keeps the latest verified numerical edit's
    // Undo/Redo on the same source. It is not an independent history journal.
    // The saved source must match the actual pre-edit draft, not merely its shape.
    let history = (|| {
        if command != "render.lights.set" || remap.len() != before.len() {
            return None;
        }
        let previous = app.session.undo.last()?;
        if previous.label != solidcraft_engine::find_command(command)?.label {
            return None;
        }
        let saved = previous.doc.render.setups.get(name)?;
        if solidcraft_engine::render_lights::inspect(saved).ok()? != before {
            return None;
        }
        let cached = app.render_workspace.view.lights.source.as_ref()?;
        let source = std::sync::Arc::downgrade(&previous.doc.render);
        if !cached.settings.ptr_eq(&source) {
            return None;
        }
        let after = solidcraft_engine::render_lights::inspect(app.session.doc.render.setups.get(name)?).ok()?;
        let kind = |light: &Light| (light.geometry.kind(), matches!(light.geometry, Geometry::Directional { angular_radius_rad: Some(_), .. }));
        if before.iter().any(|light| {
            !remap.iter().any(|entry| {
                entry.before == *light && entry.after_id == light.id && after.iter().any(|new| new.id == entry.after_id && kind(new) == kind(light))
            })
        }) {
            return None;
        }
        Some(NumericHistory { before: source, after: std::sync::Arc::downgrade(&app.session.doc.render) })
    })();
    if let Some(history) = history {
        app.render_workspace.view.lights.numeric_history = Some(history);
    }
    let previous = &app.render_workspace.view.lights.visibility;
    let visibility =
        Visibility { solo: previous.solo.as_deref().and_then(remap_id), muted: previous.muted.iter().filter_map(|id| remap_id(id)).collect() };
    let selection = match &app.render_workspace.view.selection {
        RenderSelection::Light(id) => remap_id(id).map_or(RenderSelection::Nothing, RenderSelection::Light),
        other => other.clone(),
    };
    setups::load(app);
    app.render_workspace.view.lights.visibility = visibility;
    // Parameter drafts are keyed by old address/revision. Never hand one to a
    // different source that now occupies that address after canonicalization.
    app.render_workspace.view.lights.draft = None;
    loaded(app);
    if let Some(aim) = params.get("set").and_then(|set| set.get("aim_at")) {
        app.render_workspace.view.lights.aim = serde_json::from_value(aim.clone()).ok();
    }
    // Add/set/duplicate select the command's actual result. Removing another
    // source preserves an unrelated selected source through its explicit map.
    app.render_workspace.view.selection = reply.get("id").and_then(Value::as_str).map_or(selection, |id| RenderSelection::Light(id.into()));
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
                transparent_background: false,
                options: Box::new(CpuJob {
                    sun: options.sun.as_deref().copied(),
                    path: options.path,
                    progressive: options.progressive,
                    primary_samples: options.primary_samples,
                    shadows: options.shadows,
                    lights: options.lights.clone(),
                    preset: options.preset,
                    punctual_lights: options.punctual_lights.clone(),
                    area_lights: options.area_lights.clone(),
                    ..Default::default()
                }),
            }),
            ..Default::default()
        };
        solidcraft_engine::render_lights::filter(&mut lighting, visibility)?;
        if let Some(RenderJob::CpuRaytrace { options: filtered, .. }) = lighting.job {
            let mut options = options;
            options.sun = filtered.sun.map(Box::new);
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
                let normal = solidcore_record::Vec3::new(ux, uy, uz).cross(solidcore_record::Vec3::new(vx, vy, vz));
                let area = normal.len();
                let n = solidcraft_engine::render::surface::unit_direction(normal).unwrap_or(-solidcore_record::Vec3::Z);
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
            let disabled = [(!cpu).then_some(reason.as_str()), None];
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
            for (kind, key, limit) in
                [("directional", "render.tree_directional", 8), ("area", "render.light_type_area", 4), ("point", "render.light_type_point", 8)]
            {
                let count = lights
                    .iter()
                    .filter(|l| if kind == "point" { matches!(l.geometry.kind(), "point" | "spot") } else { l.geometry.kind() == kind })
                    .count();
                let unsupported = raster;
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
                                        solidcraft_ui_text::format(
                                            "render.lighting_used",
                                            &[("used", &count.to_string()), ("limit", &limit.to_string())],
                                        )
                                    };
                                    column.add_enabled_ui(!unsupported, |ui| ui_kit::text(ui, &readout, ui_kit::Text::Inline));
                                }
                            }
                        });
                    })
                });
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
    let label = app.tr("render.tree_add_light").to_string();
    ui.add_enabled_ui(!raster, |ui| {
        ui_kit::dropdown_menu(ui, "render:lighting:add", &label, |ui| {
            for (kind, key, cap) in [
                ("area", "render.light_type_area", 4),
                ("point", "render.light_type_point", 8),
                ("spot", "render.light_type_spot", 8),
                ("directional", "render.tree_directional", 8),
            ] {
                let count = lights
                    .iter()
                    .filter(
                        |l| if matches!(kind, "point" | "spot") { matches!(l.geometry.kind(), "point" | "spot") } else { l.geometry.kind() == kind },
                    )
                    .count();
                let enabled = count < cap;
                let response = ui.add_enabled_ui(enabled, |ui| ui_kit::list_picker(ui, kind, app.tr(key), false)).inner;
                crate::scenario::publish_target(&format!("render:lighting:add:{kind}"), response.rect);
                if response.on_hover_text(app.tr("render.lighting_budget_help")).clicked() {
                    dispatch(app, "render.lights.add", None, Some(json!({"kind":kind})));
                    ui.close();
                }
            }
            {
                let compatible = gpu || app.render_workspace.view.raytrace.path.is_some();
                let directional = lights.iter().filter(|l| l.geometry.kind() == "directional").count();
                let finite = lights.iter().any(|l| matches!(l.geometry, Geometry::Directional { angular_radius_rad: Some(_), .. }));
                let response = ui
                    .add_enabled_ui(compatible && !finite && directional < 8, |ui| {
                        ui_kit::list_picker(ui, "sun", app.tr("render.light_type_sun"), false)
                    })
                    .inner;
                crate::scenario::publish_target("render:lighting:add:sun", response.rect);
                if response
                    .backing_on_hover_text(app.tr(if compatible { "render.directional_sun_limit" } else { "render.cpu_celestial_path_required" }))
                    .clicked()
                {
                    let softness = (0.02 - 0.001) / (std::f64::consts::FRAC_PI_2 - 0.001);
                    dispatch(app, "render.lights.add", None, Some(json!({"kind":"directional","softness":softness})));
                    ui.close();
                }
            }
        });
    });
}

fn review_preset(app: &mut SolveApp, preset: solidcraft_engine::render::job::LightingPreset) {
    match setups::snapshot(app) {
        Ok(mut setup) => {
            let source = setup.clone();
            if let Some(RenderJob::CpuRaytrace { options, .. }) = &mut setup.job {
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
    app.run("render.settings.set", json!({"name":app.render_workspace.view.name,"set":setup}))?;
    // Full settings replacement has no source map. Respect loaded()'s proven-
    // provenance policy instead of restoring visibility by reused ordinal IDs.
    setups::load(app);
    Ok(())
}
fn update_cpu(app: &mut SolveApp, update: impl FnOnce(&mut CpuJob)) {
    let result = (|| {
        let mut setup = setups::snapshot(app)?;
        if let Some(RenderJob::CpuRaytrace { options, .. }) = &mut setup.job {
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
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("render_lighting_review"), "render_scene", &title)
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
    let blocked = app.render_workspace.view.method == ImageMethod::Raster;
    let finite_sun = matches!(light.geometry, Geometry::Directional { angular_radius_rad: Some(_), .. });
    let maximum_ev = if gpu && kind == "area" {
        1e6_f64.log2()
    } else if finite_sun {
        1e8_f64.log2()
    } else {
        6.0
    };
    let type_key = match kind {
        "area" => "render.light_type_area",
        "spot" => "render.light_type_spot",
        "point" => "render.light_type_point",
        _ => {
            if finite_sun {
                "render.light_type_sun"
            } else {
                "render.tree_directional"
            }
        }
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
    let (emitted_axis, inner_angle, outer_angle) = match light.geometry {
        Geometry::Spot { emitted_axis, inner_half_angle_radians, outer_half_angle_radians, .. } => {
            (emitted_axis, inner_half_angle_radians.to_degrees(), outer_half_angle_radians.to_degrees())
        }
        _ => ([0.0, 0.0, -1.0], 0.2_f64.to_degrees(), 0.5_f64.to_degrees()),
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
            emitted_axis,
            inner_angle,
            outer_angle,
        });
    ui_kit::scope(ui, |ui| {
        ui_kit::row(ui, &name(app, light), |ui| {
            ui.add_enabled_ui(false, |ui| ui_kit::choice_chip(ui, "render:light:badge", app.tr(type_key), true));
        });
        if blocked {
            note(app, ui, "render.method_limit_raster");
        }
        ui.add_enabled_ui(!blocked, |ui| {
            let labels = [
                app.tr("render.light_type_area").to_string(),
                app.tr("render.light_type_point").to_string(),
                app.tr("render.light_type_spot").to_string(),
                app.tr(if finite_sun { "render.light_type_sun" } else { "render.tree_directional" }).to_string(),
            ];
            let kinds = ["area", "point", "spot", "directional"];
            let mut selected = kinds.iter().position(|value| *value == kind).unwrap_or(1);
            let budget_reason = app.tr("render.lighting_budget_help").to_string();
            let areas = lights.iter().filter(|l| l.geometry.kind() == "area").count();
            let points = lights.iter().filter(|l| matches!(l.geometry.kind(), "point" | "spot")).count();
            let suns = lights.iter().filter(|l| l.geometry.kind() == "directional").count();
            let disabled = [
                (kind != "area" && areas >= 4).then_some(budget_reason.as_str()),
                (kind != "point" && kind != "spot" && points >= 8).then_some(budget_reason.as_str()),
                (kind != "point" && kind != "spot" && points >= 8).then_some(budget_reason.as_str()),
                (kind != "directional" && suns >= 8).then_some(budget_reason.as_str()),
            ];
            if ui_kit::segmented(
                ui,
                &mut selected,
                &labels.iter().map(String::as_str).collect::<Vec<_>>(),
                ui_kit::Segments {
                    compact: true,
                    disabled: &disabled,
                    targets: &["render:light:type:area", "render:light:type:point", "render:light:type:spot", "render:light:type:directional"],
                    ..Default::default()
                },
            ) && let Some(chosen) = kinds.get(selected)
                && *chosen != kind
            {
                dispatch(app, "render.lights.set", Some(id), Some(json!({"kind":chosen})));
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
            let soft = kind == "area" || finite_sun;
            let sun = kind == "directional";
            // A sun displays its actual angular radius; areas display the size adapter.
            slider_field(
                app,
                ui,
                id,
                "render.light_softness",
                if sun { "angular_radius_deg" } else { "softness" },
                &mut draft.softness,
                if finite_sun {
                    0.001_f64.to_degrees()..=90.0
                } else if sun {
                    0.0..=90.0
                } else {
                    0.0..=1.0
                },
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
                response.backing_on_hover_text(app.tr("render.light_place_pending"));
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
            if kind == "spot" {
                let title = app.tr("render.spot_cone").to_string();
                ui_kit::section(ui, "render:light:cone", &title, true, |ui| {
                    vector_field(app, ui, id, "render.spot_axis", "emitted_axis", &mut draft.emitted_axis, -1e9..=1e9, "");
                    field(app, ui, id, "render.spot_inner", "inner_half_angle_radians", &mut draft.inner_angle, 0.0..=90.0, 0.1, "°", true);
                    field(app, ui, id, "render.spot_outer", "outer_half_angle_radians", &mut draft.outer_angle, 0.0..=90.0, 0.1, "°", true);
                    note(app, ui, if gpu { "render.spot_gpu_help" } else { "render.spot_help" });
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
                let cap = if kind == "area" { 4 } else { 8 };
                let count = lights
                    .iter()
                    .filter(
                        |l| if matches!(kind, "point" | "spot") { matches!(l.geometry.kind(), "point" | "spot") } else { l.geometry.kind() == kind },
                    )
                    .count();
                if ui_kit::button(ui, app.tr("render.light_duplicate"), ui_kit::Button::Enabled(!blocked && !finite_sun && count < cap))
                    .backing_on_hover_text(app.tr(if finite_sun { "render.directional_sun_limit" } else { "render.lighting_budget_help" }))
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
    // Each coordinate needs the full control column: three content-sized
    // DragValues can extend beyond a narrow inspector as their text grows.
    // Separate axis rows retain the standard parser, focus and accessible label.
    let mut changed = false;
    for ((index, component), axis) in value.iter_mut().enumerate().zip(["X", "Y", "Z"]) {
        ui_kit::row(ui, &format!("{label} {axis}"), |ui| {
            let response = ui_kit::number_field(ui, component, range.clone(), 0.1, suffix);
            crate::scenario::publish_target(&format!("render:light:{key}:{index}"), response.rect);
            changed |= accepted(&response);
        });
    }
    if changed && value.iter().all(|v| v.is_finite() && range.contains(v)) {
        dispatch(app, "render.lights.set", Some(id), Some(json!({(key):*value})));
    }
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
                let patch = if key == "distance_size" {
                    json!({"distance_mm":*value * subject_size(app)})
                } else if matches!(key, "inner_half_angle_radians" | "outer_half_angle_radians") {
                    json!({(key):value.to_radians()})
                } else {
                    json!({(key):*value})
                };
                dispatch(app, "render.lights.set", Some(id), Some(patch));
            }
        })
    });
}

#[cfg(test)]
pub(super) fn editor_draft(app: &SolveApp) -> Option<(String, u64, f64, f64)> {
    app.render_workspace.view.lights.draft.as_ref().map(|draft| (draft.id.clone(), draft.revision, draft.ev, draft.softness))
}
