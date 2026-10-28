//! Saved-job adapters. HDR payloads are captured by the engine; paths and body identities stay local.
use solidcraft_engine::doc::render_resources::{CpuHdrEnvironment, GpuHdrEnvironment};
use solidcraft_engine::doc::render_settings::{CpuJob, GpuJob, RenderJob, RenderSetup, SkyEnvironment};
use solidcraft_engine::render::job::{AreaLights, RaytraceEnvironmentOptions, RaytraceOptions, StudioLighting};

use super::{ImageMethod, cpu, hdr, materials, studio};
use crate::SolveApp;

/// Unsaved inputs protect local shot edits and independent navigation. Portable
/// Glass and coat drafts remain unsaved while using the typed capture route.
pub(super) fn has_runtime_inputs(app: &SolveApp) -> bool {
    requires_runtime_request(app)
        || app.render_workspace.view.materials.bindings.iter().any(|binding| binding.has_unsaved_coat_intent() || binding.has_unsaved_optics_intent())
}

/// File sources and ordinal Glass/maps use the runtime request schema. Stable
/// Glass and coat drafts overlay targets into typed captured settings instead.
pub(super) fn requires_runtime_request(app: &SolveApp) -> bool {
    has_runtime_bindings(app) || hdr::active(app).is_some_and(|target| !hdr::input(app, target).embedded())
}

pub(super) fn has_unportable_bindings(app: &SolveApp) -> bool {
    !app.render_workspace.view.materials.bindings.is_empty() || has_runtime_bindings(app)
}

fn has_runtime_bindings(app: &SolveApp) -> bool {
    let view = &app.render_workspace.view;
    if view.materials.bindings.iter().any(super::materials::Binding::has_runtime_intent) {
        return true;
    }
    match view.method {
        ImageMethod::Raster | ImageMethod::GpuRaytrace => false,
        ImageMethod::CpuRaytrace => {
            let options = &view.raytrace;
            options.reflections.is_some()
                || options.anisotropic_faces.iter().any(Option::is_some)
                || !options.dielectrics.is_empty()
                || options.surface_effects.is_some()
                || options.emissions.is_some()
                || options.aging.is_some()
                || options.sky.is_some()
                || (options.sun.is_some() && !view.studio.sun_enabled)
                || (options.volume.is_some() && view.studio.volume.is_none())
        }
    }
}

/// Pure construction: saving intent must work with no CPU/GPU host and stale geometry.
pub(super) fn snapshot(app: &SolveApp) -> Result<RenderSetup, String> {
    snapshot_with_lens_override(app, None)
}

/// Validate a prospective lens edit without changing the live draft first. In particular,
/// removing an authored lens remains possible after navigating to orthographic.
pub(super) fn snapshot_with_lens(app: &SolveApp, lens: Option<solidcraft_engine::render::job::ThinLens>) -> Result<RenderSetup, String> {
    snapshot_with_lens_override(app, Some(lens))
}

fn snapshot_with_lens_override(app: &SolveApp, lens: Option<Option<solidcraft_engine::render::job::ThinLens>>) -> Result<RenderSetup, String> {
    cpu::validate_progressive_image(app)?;
    if has_unportable_bindings(app) {
        return Err(app.tr("render.setup_runtime_unsupported").to_string());
    }
    let mut setup = draft(app)?;
    if let Some(lens) = lens {
        match &mut setup.job {
            Some(RenderJob::CpuRaytrace { options, .. }) => options.lens = lens,
            Some(RenderJob::GpuPath { options, .. }) => options.lens = lens,
            _ => return Err(app.tr("render.shot_lens_raytrace_reason").into()),
        }
    }
    validate_snapshot(app, setup)
}

/// Stable saved optical edits stay transient while every ordinary image capture sees
/// the prospective values. Generic Save still requires the explicit optical review.
pub(super) fn image_snapshot(app: &SolveApp) -> Result<RenderSetup, String> {
    cpu::validate_progressive_image(app)?;
    if has_runtime_bindings(app) {
        return Err(app.tr("render.setup_runtime_unsupported").into());
    }
    let mut setup = draft(app)?;
    materials::overlay_saved(app, &mut setup)?;
    validate_snapshot(app, setup)
}

fn validate_snapshot(app: &SolveApp, setup: RenderSetup) -> Result<RenderSetup, String> {
    // Preserve unsupported legacy intent visibly instead of erasing it while upgrading a setup.
    if setup.environment_rotation != 0.0 || setup.environment_intensity != 1.0 {
        return Err(app.tr("render.setup_legacy_environment").to_string());
    }
    setup.validate().map_err(|error| error.to_string())?;
    if hdr::active(app).is_some_and(|target| hdr::input(app, target).embedded()) {
        app.session.doc.render.validate_resources(&setup).map_err(|error| error.to_string())?;
    }
    Ok(setup)
}

// Compare editable numerical intent before validation, so an incompatible legacy value
// or a lens temporarily paired with an orthographic camera cannot disable dirty tracking.
pub(super) fn draft(app: &SolveApp) -> Result<RenderSetup, String> {
    let view = &app.render_workspace.view;
    let mut setup = RenderSetup { camera: app.cam, ..view.draft.clone() };
    setup.job = Some(match view.method {
        ImageMethod::Raster => RenderJob::Raster { transparent_background: view.transparent },
        ImageMethod::CpuRaytrace => {
            let options = &view.raytrace;
            RenderJob::CpuRaytrace {
                transparent_background: view.transparent,
                options: Box::new(CpuJob {
                    volume: view.studio.volume,
                    sun: view.studio.sun_enabled.then_some(view.studio.sun),
                    sky: (view.cpu.environment == cpu::Environment::Sky).then_some(SkyEnvironment::Sky {
                        gradient: view.cpu.gradient,
                        rotation_rad: view.cpu.rotation_rad,
                        intensity: view.cpu.intensity,
                    }),
                    aging: view.studio.aging.clone(),
                    adaptive: view.studio.adaptive,
                    denoise: view.studio.denoise,
                    path: options.path,
                    lens: view.studio.lens_enabled.then_some(view.studio.lens),
                    output: studio::cpu_output(view),
                    budget: options.budget,
                    primary_samples: options.primary_samples,
                    progressive: options.progressive,
                    preset: options.preset,
                    lights: options.lights.clone(),
                    punctual_lights: options.punctual_lights.clone(),
                    area_lights: options.area_lights.clone(),
                    area_samples: options.area_samples,
                    seed: options.seed,
                    shadows: options.shadows,
                    environment: match view.cpu.environment {
                        cpu::Environment::Constant => Some(RaytraceEnvironmentOptions { color: view.cpu.color, intensity: view.cpu.intensity }),
                        cpu::Environment::Hdr | cpu::Environment::Sky => None,
                        cpu::Environment::None => options.environment,
                    },
                    hdr: if view.cpu.environment == cpu::Environment::Hdr {
                        Some(CpuHdrEnvironment {
                            resource: view.cpu.hdr.resource().ok_or_else(|| app.tr("render.hdr_inspect_required").to_string())?,
                            rotation_rad: view.cpu.rotation_rad,
                            intensity: view.cpu.intensity,
                            samples: view.cpu.samples,
                        })
                    } else {
                        None
                    },
                }),
            }
        }
        ImageMethod::GpuRaytrace => {
            let studio = &view.studio;
            RenderJob::GpuPath {
                transparent_background: view.transparent,
                options: Box::new(GpuJob {
                    samples: view.samples,
                    max_depth: view.max_depth,
                    seed: view.seed,
                    aging: studio.aging.clone(),
                    nested_media: studio.nested_media,
                    volume: studio.volume,
                    adaptive: studio.adaptive,
                    transmission_samples: studio.transmission_samples,
                    finishing: studio.finishing,
                    backdrop: studio.backdrop_enabled.then_some(studio.backdrop),
                    linear_exr: studio.linear_exr,
                    tone_map: studio.tone_map,
                    denoise: studio.denoise,
                    lens: studio.lens_enabled.then_some(studio.lens),
                    lighting: if studio.lighting_enabled {
                        Some(StudioLighting {
                            punctual_lights: studio.punctual_lights,
                            directional_lights: studio.directional_lights,
                            environment: studio.ambient,
                            sun: studio.sun_enabled.then_some(studio.sun),
                            area_lights: AreaLights::new(&studio.area_lights).map_err(|error| error.to_string())?,
                        })
                    } else {
                        None
                    },
                    environment: if studio.environment == studio::Environment::Sky {
                        Some(SkyEnvironment::Sky { gradient: studio.gradient, rotation_rad: studio.rotation_rad, intensity: studio.intensity })
                    } else {
                        None
                    },
                    hdr: if studio.environment == studio::Environment::Hdr {
                        Some(GpuHdrEnvironment {
                            resource: studio.hdr.resource().ok_or_else(|| app.tr("render.hdr_inspect_required").to_string())?,
                            rotation_rad: studio.rotation_rad,
                            intensity: studio.intensity,
                            assume_linear_srgb: studio.assume_linear_srgb,
                        })
                    } else {
                        None
                    },
                }),
            }
        }
    });
    Ok(setup)
}

/// Save only fields a numerical editor changed. Navigation keeps its explicit
/// Update shot/Revert flow, and temporary optical bindings remain local.
pub(super) fn persist_editor(app: &mut SolveApp, before: &RenderSetup) -> Result<(), String> {
    let after = draft(app)?;
    // A corrective edit (for example removing an invalid wear layer) must not
    // require the hostile previous draft to be serialisable. The new value and
    // the saved command are still validated normally.
    let old = serde_json::to_value(before).ok();
    let new = serde_json::to_value(&after).map_err(|error| error.to_string())?;
    let saved = app.session.doc.render.setups.get(&app.render_workspace.view.name);
    let creating = saved.is_none();
    let saved = saved.and_then(|setup| serde_json::to_value(setup).ok());
    let mut patch = serde_json::Map::new();
    if let Some(fields) = new.as_object() {
        for (key, value) in fields {
            if key != "camera"
                && old.as_ref().and_then(|old| old.get(key)) != Some(value)
                && saved.as_ref().and_then(|saved| saved.get(key)) != Some(value)
            {
                patch.insert(key.clone(), value.clone());
            }
        }
    }
    if !patch.is_empty() {
        // The engine creates missing setups from its legacy defaults. On the first real
        // numerical edit, preserve the whole visible numerical baseline, including image
        // size and method; a job-only patch can be inadmissible at the engine's default size.
        // Navigation keeps its explicit Update shot policy. Merely revealing an editor is
        // still read-only because an actual edit must have populated the patch first.
        if creating && let Some(fields) = new.as_object() {
            patch = fields.iter().filter(|(key, _)| key.as_str() != "camera").map(|(key, value)| (key.clone(), value.clone())).collect();
        }
        after.validate().map_err(|error| error.to_string())?;
        let name = app.render_workspace.view.name.clone();
        app.run("render.settings.set", serde_json::json!({"name":name,"set":patch}))?;
        app.render_workspace.revision = app.session.revision;
        let camera = app.render_workspace.view.clean_setup.as_ref().map_or(after.camera, |clean| clean.camera);
        app.render_workspace.view.clean_setup = Some(RenderSetup { camera, ..after.clone() });
        app.render_workspace.view.clean_persisted = true;
        app.render_workspace.view.clean_name = app.render_workspace.view.name.clone();
        materials::refresh_render_revision(app);
        cpu::refresh_render_revision(app);
    }
    if let Some(saved) = app.session.doc.render.setups.get(&app.render_workspace.view.name) {
        if let Some(RenderJob::CpuRaytrace { options, .. }) = &saved.job {
            app.render_workspace.view.raytrace.output = options.output.clone();
            app.render_workspace.view.raytrace.denoise = options.denoise;
            app.render_workspace.view.raytrace.lens = options.lens;
            app.render_workspace.view.raytrace.path = options.path;
            app.render_workspace.view.raytrace.volume = options.volume;
        }
        let mut saved = saved.clone();
        saved.camera = after.camera;
        if saved == after {
            let camera = app.render_workspace.view.clean_setup.as_ref().map_or(after.camera, |clean| clean.camera);
            app.render_workspace.view.clean_setup = Some(RenderSetup { camera, ..after });
            app.render_workspace.view.clean_persisted = true;
            app.render_workspace.view.clean_name = app.render_workspace.view.name.clone();
            app.render_workspace.revision = app.session.revision;
        }
    }
    refresh_dirty(app);
    Ok(())
}

/// A missing setup uses the same transient baseline for initial loading and later model edits.
/// Existing entries, including a legacy job-less setup, retain their saved interpretation.
fn saved_or_initial(app: &SolveApp) -> RenderSetup {
    app.session.doc.render.setups.get(&app.render_workspace.view.name).cloned().unwrap_or_else(|| {
        let mut setup = RenderSetup::default();
        // Only a new, unsaved setup adopts the useful native image route. Discovery is read-only;
        // saved Raster, fixed CPU and legacy job-less setups keep their exact interpretation.
        if !app.preferences.render_legacy_image_defaults && super::image_host(app, ImageMethod::CpuRaytrace).is_ok() {
            setup.job = Some(RenderJob::CpuRaytrace {
                transparent_background: false,
                options: Box::new(CpuJob { progressive: Some(Default::default()), ..Default::default() }),
            });
        }
        setup
    })
}

/// A setup switch/reload replaces its numeric editor draft, never a completed image or camera.
pub(super) fn load(app: &mut SolveApp) {
    let persisted = app.session.doc.render.setups.contains_key(&app.render_workspace.view.name);
    let setup = saved_or_initial(app);
    load_values(&mut app.render_workspace.view, setup);
    app.render_workspace.view.dirty = false;
    app.render_workspace.revision = app.session.revision;
    // Keep the explicit Restore Saved Camera action: loading options does not navigate.
    app.render_workspace.view.clean_setup = draft(app).ok();
    app.render_workspace.view.clean_persisted = persisted;
    app.render_workspace.view.clean_name = app.render_workspace.view.name.clone();
    super::light::loaded(app);
}

fn load_values(view: &mut super::ViewState, setup: RenderSetup) {
    view.method = ImageMethod::Raster;
    view.transparent = false;
    view.raytrace = RaytraceOptions::default();
    view.cpu_sampling_initialized = false;
    view.cpu = cpu::View::default();
    view.samples = 64;
    view.max_depth = 8;
    view.seed = 0;
    view.studio = studio::View::default();
    view.materials = materials::View::default();
    match &setup.job {
        Some(RenderJob::Raster { transparent_background }) => view.transparent = *transparent_background,
        Some(RenderJob::CpuRaytrace { options, transparent_background }) => {
            view.method = ImageMethod::CpuRaytrace;
            view.transparent = *transparent_background;
            view.cpu_sampling_initialized = true;
            view.studio.volume = options.volume;
            view.studio.denoise = options.denoise;
            view.studio.aging = options.aging.clone();
            view.studio.adaptive = options.adaptive;
            view.studio.sun_enabled = options.sun.is_some();
            if let Some(sun) = options.sun {
                view.studio.sun = sun;
                view.studio.lighting_enabled = true;
            }
            view.raytrace = RaytraceOptions {
                sun: options.sun.map(Box::new),
                volume: options.volume,
                adaptive: options.adaptive.map(Box::new),
                denoise: options.denoise,
                path: options.path,
                lens: options.lens,
                output: options.output.clone(),
                budget: options.budget,
                primary_samples: options.primary_samples,
                progressive: options.progressive,
                preset: options.preset,
                lights: options.lights.clone(),
                punctual_lights: options.punctual_lights.clone(),
                area_lights: options.area_lights.clone(),
                area_samples: options.area_samples,
                seed: options.seed,
                shadows: options.shadows,
                ..Default::default()
            };
            if let Some(lens) = options.lens {
                view.studio.lens_enabled = true;
                view.studio.lens = lens;
            }
            if let Some(output) = &options.output {
                view.studio.tone_map = output.tone_map;
                view.studio.finishing = output.finishing;
                view.studio.backdrop_enabled = output.backdrop.is_some();
                view.studio.backdrop = output.backdrop.unwrap_or_default();
                view.studio.linear_exr = output.linear_exr;
            }
            if let Some(SkyEnvironment::Sky { gradient, rotation_rad, intensity }) = options.sky {
                view.cpu.environment = cpu::Environment::Sky;
                view.cpu.gradient = gradient;
                view.cpu.rotation_rad = rotation_rad;
                view.cpu.intensity = intensity;
            }
            if let Some(environment) = options.environment {
                view.cpu.environment = cpu::Environment::Constant;
                view.cpu.color = environment.color;
                view.cpu.intensity = environment.intensity;
            }
            if let Some(environment) = options.hdr {
                view.cpu.environment = cpu::Environment::Hdr;
                view.cpu.hdr = hdr::Input::EmbeddedResource { resource: environment.resource };
                view.cpu.rotation_rad = environment.rotation_rad;
                view.cpu.intensity = environment.intensity;
                view.cpu.samples = environment.samples;
            }
        }
        Some(RenderJob::GpuPath { transparent_background, options }) => {
            view.method = ImageMethod::GpuRaytrace;
            view.transparent = *transparent_background;
            view.samples = options.samples;
            view.max_depth = options.max_depth;
            view.seed = options.seed;
            view.studio.aging = options.aging.clone();
            view.studio.nested_media = options.nested_media;
            view.studio.volume = options.volume;
            view.studio.adaptive = options.adaptive;
            view.studio.transmission_samples = options.transmission_samples;
            view.studio.finishing = options.finishing;
            view.studio.backdrop_enabled = options.backdrop.is_some();
            view.studio.backdrop = options.backdrop.unwrap_or_default();
            view.studio.linear_exr = options.linear_exr;
            view.studio.tone_map = options.tone_map;
            view.studio.denoise = options.denoise;
            if let Some(lens) = options.lens {
                view.studio.lens_enabled = true;
                view.studio.lens = lens;
            }
            if let Some(lighting) = options.lighting {
                view.studio.lighting_enabled = true;
                view.studio.ambient = lighting.environment;
                view.studio.sun_enabled = lighting.sun.is_some();
                if let Some(sun) = lighting.sun {
                    view.studio.sun = sun;
                }
                view.studio.area_lights = lighting.area_lights.iter().copied().collect();
                view.studio.directional_lights = lighting.directional_lights;
                view.studio.punctual_lights = lighting.punctual_lights;
            }
            if let Some(SkyEnvironment::Sky { gradient, rotation_rad, intensity }) = &options.environment {
                view.studio.environment = studio::Environment::Sky;
                view.studio.gradient = *gradient;
                view.studio.rotation_rad = *rotation_rad;
                view.studio.intensity = *intensity;
            }
            if let Some(environment) = options.hdr {
                view.studio.environment = studio::Environment::Hdr;
                view.studio.hdr = hdr::Input::EmbeddedResource { resource: environment.resource };
                view.studio.rotation_rad = environment.rotation_rad;
                view.studio.intensity = environment.intensity;
                view.studio.assume_linear_srgb = environment.assume_linear_srgb;
            }
        }
        None => {}
    }
    view.draft = setup;
}

/// Compare the complete authoring source, excluding only saved Render settings.
/// Stable body labels alone do not prove that geometry or its material is unchanged.
pub(super) fn same_model_source(app: &SolveApp, document: &std::sync::Weak<solidcraft_engine::doc::Document>) -> bool {
    if app.session.model.is_stale() {
        return false;
    }
    let Some(source) = document.upgrade() else {
        return false;
    };
    // Exhaustive by design: adding a Document field must update this comparison.
    // Match ergonomics bind references, including vectors and point-cloud data.
    // Section Analyses cut only the view, never what is rendered.
    macro_rules! compare_authoring_fields {
        ($($field:ident),* $(,)?) => {{
            let solidcraft_engine::doc::Document { render: _, section_analyses: _, $($field),* } = source.as_ref();
            true $(&& $field == &app.session.doc.$field)*
        }};
    }
    compare_authoring_fields!(
        newer_file,
        format,
        format_pending,
        name,
        units,
        metadata,
        params,
        text_params,
        features,
        marker,
        next_id,
        reserved_body_names,
        components,
        next_component_color_index,
        occurrences,
        assembly,
        sheet,
        plastic,
        parts,
        links,
        configs,
        favorites,
        canvases,
        point_clouds,
        param_comments,
        materials,
        print_faces,
        user_materials,
        appearances,
        body_components,
        body_offsets,
        browser_groups,
        timeline_groups,
        browser_order,
        named_views,
        visibility,
    )
}

/// A single refresh's source comparisons. Targets are still checked per binding.
#[derive(Default)]
pub(super) struct SourceCache(Vec<(std::sync::Weak<solidcraft_engine::doc::Document>, bool)>);

impl SourceCache {
    pub(super) fn same_model_source(&mut self, app: &SolveApp, document: &std::sync::Weak<solidcraft_engine::doc::Document>) -> bool {
        if let Some((_, same)) = self.0.iter().find(|(source, _)| std::sync::Weak::ptr_eq(source, document)) {
            return *same && !app.session.model.is_stale() && document.upgrade().is_some();
        }
        let same = same_model_source(app, document);
        self.0.push((document.clone(), same));
        same
    }
}

/// Synchronize saved edits/Undo without replacing local camera or runtime inputs.
pub(super) fn sync_revision(app: &mut SolveApp) {
    if app.render_workspace.revision == app.session.revision {
        return;
    }
    // A new shot name can intentionally carry the unsaved draft of its source.
    // Its missing saved entry is not an Undo of that source's numerical fields.
    if app.render_workspace.view.clean_name != app.render_workspace.view.name {
        if !app.render_workspace.view.dirty {
            load(app);
        }
        return;
    }
    let Some(clean) = app.render_workspace.view.clean_setup.clone() else {
        if !app.render_workspace.view.dirty {
            load(app);
        }
        return;
    };
    let persisted = app.session.doc.render.setups.contains_key(&app.render_workspace.view.name);
    let saved = app.session.doc.render.setups.get(&app.render_workspace.view.name).cloned().unwrap_or_else(|| {
        // An active unsaved setup keeps the baseline captured when it was opened. A missing
        // entry after Undo of a saved setup, however, returns to the current initial defaults.
        if app.render_workspace.view.clean_persisted { saved_or_initial(app) } else { clean.clone() }
    });
    let mut previous = super::ViewState::default();
    load_values(&mut previous, clean.clone());
    let mut updated = super::ViewState::default();
    load_values(&mut updated, saved);
    // Construct the new clean numerical baseline independently of local drafts.
    let current = std::mem::replace(&mut app.render_workspace.view, updated);
    let mut next_clean = draft(app).ok();
    if let Some(next) = &mut next_clean {
        next.camera = clean.camera;
    }
    let updated = std::mem::replace(&mut app.render_workspace.view, current);
    let view = &mut app.render_workspace.view;
    macro_rules! sync {
        ($($head:ident $(.$tail:ident)*),* $(,)?) => {$(
            if previous.$head$(.$tail)* != updated.$head$(.$tail)* && view.$head$(.$tail)* == previous.$head$(.$tail)* {
                view.$head$(.$tail)* = updated.$head$(.$tail)*;
            }
        )*};
    }
    // Only persisted fields participate. Reflections, optical bindings, file selectors,
    // editor selection, results and host/job state stay in their existing owners.
    sync!(
        method,
        cpu_sampling_initialized,
        transparent,
        samples,
        max_depth,
        seed,
        draft.backend,
        draft.mode,
        draft.width,
        draft.height,
        draft.format,
        draft.exposure_ev,
        draft.environment_rotation,
        draft.environment_intensity,
        draft.background,
        draft.job,
        draft.surfaces,
        draft.optics,
        draft.coats,
        draft.stage,
        raytrace.budget,
        raytrace.denoise,
        raytrace.output,
        raytrace.lens,
        raytrace.path,
        raytrace.sun,
        raytrace.sky,
        raytrace.volume,
        raytrace.primary_samples,
        raytrace.progressive,
        raytrace.adaptive,
        raytrace.preset,
        raytrace.lights,
        raytrace.punctual_lights,
        raytrace.area_lights,
        raytrace.area_samples,
        raytrace.seed,
        raytrace.shadows,
        cpu.environment,
        cpu.color,
        cpu.gradient,
        cpu.intensity,
        cpu.rotation_rad,
        cpu.samples,
        studio.aging,
        studio.nested_media,
        studio.volume,
        studio.adaptive,
        studio.transmission_samples,
        studio.finishing,
        studio.backdrop_enabled,
        studio.backdrop,
        studio.linear_exr,
        studio.tone_map,
        studio.denoise,
        studio.lens_enabled,
        studio.lens,
        studio.lighting_enabled,
        studio.ambient,
        studio.sun_enabled,
        studio.sun,
        studio.area_lights,
        studio.directional_lights,
        studio.punctual_lights,
        studio.environment,
        studio.gradient,
        studio.rotation_rad,
        studio.intensity,
        studio.assume_linear_srgb
    );
    // File HDR selectors are runtime inputs; embedded selectors follow saved Undo.
    if view.cpu.hdr == previous.cpu.hdr {
        view.cpu.hdr = updated.cpu.hdr;
    }
    if view.studio.hdr == previous.studio.hdr {
        view.studio.hdr = updated.studio.hdr;
    }
    view.clean_setup = next_clean;
    view.clean_persisted = persisted;
    app.render_workspace.revision = app.session.revision;
    materials::refresh_render_revision(app);
    cpu::refresh_render_revision(app);
    super::light::loaded(app);
    refresh_dirty(app);
}

pub(super) fn refresh_dirty(app: &mut SolveApp) {
    if let Some(clean) = &app.render_workspace.view.clean_setup {
        app.render_workspace.view.dirty = has_runtime_inputs(app) || draft(app).as_ref() != Ok(clean);
    }
}

pub(super) fn save(app: &mut SolveApp) -> Result<(), String> {
    save_with_camera(app, true)
}

/// Numerical source edits capture bytes/intent atomically without updating the shot camera.
pub(super) fn save_numerical(app: &mut SolveApp) -> Result<(), String> {
    save_with_camera(app, false)
}

fn setup_patch(setup: RenderSetup, save_camera: bool) -> Result<serde_json::Value, String> {
    let mut value = serde_json::to_value(setup).map_err(|error| error.to_string())?;
    if !save_camera && let Some(fields) = value.as_object_mut() {
        fields.remove("camera");
    }
    Ok(value)
}

/// Numeric reloads keep both the live camera and its previous dirty baseline.
pub(super) fn reload_numerical(app: &mut SolveApp) {
    let camera = app.render_workspace.view.clean_setup.as_ref().map(|clean| clean.camera);
    load(app);
    if let Some(camera) = camera
        && let Some(clean) = &mut app.render_workspace.view.clean_setup
    {
        clean.camera = camera;
    }
    refresh_dirty(app);
}

fn save_with_camera(app: &mut SolveApp, save_camera: bool) -> Result<(), String> {
    let clean_camera = app.render_workspace.view.clean_setup.as_ref().map_or(app.cam, |clean| clean.camera);
    let mut setup = snapshot(app)?;
    let name = app.render_workspace.view.name.clone();
    let capture = hdr::active(app).and_then(|target| match hdr::input(app, target) {
        hdr::Input::FileCaptured { path, metadata } => Some((target, path.clone(), metadata.sha256)),
        _ => None,
    });
    if let Some((_, path, digest)) = &capture {
        let mut params = serde_json::json!({"name":name,"path":path,"expected_sha256":digest});
        match &mut setup.job {
            Some(RenderJob::CpuRaytrace { options, .. }) => {
                let environment = options.hdr.take().ok_or_else(|| app.tr("render.hdr_inspect_required").to_string())?;
                params["rotation_rad"] = serde_json::json!(environment.rotation_rad);
                params["intensity"] = serde_json::json!(environment.intensity);
                params["samples"] = serde_json::json!(environment.samples);
            }
            Some(RenderJob::GpuPath { options, .. }) => {
                let environment = options.hdr.take().ok_or_else(|| app.tr("render.hdr_inspect_required").to_string())?;
                params["rotation_rad"] = serde_json::json!(environment.rotation_rad);
                params["intensity"] = serde_json::json!(environment.intensity);
                params["assume_linear_srgb"] = serde_json::json!(environment.assume_linear_srgb);
            }
            _ => return Err(app.tr("render.hdr_inspect_required").to_string()),
        }
        params["set"] = setup_patch(setup, save_camera)?;
        // The expected digest rejects an externally changed file before any resource/setup edit.
        app.run("render.settings.capture_hdr", params)?;
    } else {
        app.run("render.settings.set", serde_json::json!({"name":name,"set":setup_patch(setup, save_camera)?}))?;
    }
    let setup = app.session.doc.render.setups.get(&name).cloned().ok_or_else(|| app.tr("render.setup_save_missing").to_string())?;
    if let Some((target, _, resource)) = capture {
        match target {
            hdr::Target::Cpu => app.render_workspace.view.cpu.hdr = hdr::Input::EmbeddedResource { resource },
            hdr::Target::Gpu => app.render_workspace.view.studio.hdr = hdr::Input::EmbeddedResource { resource },
        }
    }
    if let Some(RenderJob::CpuRaytrace { options, .. }) = &setup.job {
        app.render_workspace.view.raytrace.output = options.output.clone();
        app.render_workspace.view.raytrace.denoise = options.denoise;
        app.render_workspace.view.raytrace.lens = options.lens;
        app.render_workspace.view.raytrace.path = options.path;
        app.render_workspace.view.raytrace.volume = options.volume;
    }
    app.render_workspace.view.draft = setup.clone();
    app.render_workspace.view.clean_setup = Some(if save_camera { setup } else { RenderSetup { camera: clean_camera, ..setup } });
    app.render_workspace.view.clean_persisted = true;
    app.render_workspace.view.clean_name = app.render_workspace.view.name.clone();
    app.render_workspace.view.dirty = false;
    app.render_workspace.revision = app.session.revision;
    if !save_camera {
        refresh_dirty(app);
    }
    Ok(())
}
