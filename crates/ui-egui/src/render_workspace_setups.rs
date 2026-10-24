//! Saved-job adapters. HDR payloads are captured by the engine; paths and body identities stay local.
use solidcraft_engine::doc::render_resources::{CpuHdrEnvironment, GpuHdrEnvironment};
use solidcraft_engine::doc::render_settings::{CpuJob, GpuJob, RenderJob, RenderSetup, SkyEnvironment};
use solidcraft_engine::render::job::{AreaLights, RaytraceEnvironmentOptions, RaytraceOptions, StudioLighting};

use super::{ImageMethod, cpu, hdr, materials, studio};
use crate::SolveApp;

/// Unsaved files and temporary bindings use the runtime request route. Embedded sources
/// use the typed job route unless a binding also needs the runtime schema.
pub(super) fn has_runtime_inputs(app: &SolveApp) -> bool {
    has_unportable_bindings(app) || hdr::active(app).is_some_and(|target| !hdr::input(app, target).embedded())
}

fn has_unportable_bindings(app: &SolveApp) -> bool {
    let view = &app.render_workspace.view;
    if !view.materials.bindings.is_empty() {
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
        }
    }
}

/// Pure construction: saving intent must work with no CPU/GPU host and stale geometry.
pub(super) fn snapshot(app: &SolveApp) -> Result<RenderSetup, String> {
    cpu::validate_progressive_image(app)?;
    if has_unportable_bindings(app) {
        return Err(app.tr("render.setup_runtime_unsupported").to_string());
    }
    let setup = draft(app)?;
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
fn draft(app: &SolveApp) -> Result<RenderSetup, String> {
    let view = &app.render_workspace.view;
    let mut setup = RenderSetup { camera: app.cam, ..view.draft.clone() };
    setup.job = Some(match view.method {
        ImageMethod::Raster => RenderJob::Raster { transparent_background: view.transparent },
        ImageMethod::CpuRaytrace => {
            if view.transparent {
                return Err(app.tr("render.cpu_opaque_required").to_string());
            }
            let options = &view.raytrace;
            RenderJob::CpuRaytrace {
                options: CpuJob {
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
                        cpu::Environment::Hdr => None,
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
                },
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

/// A setup switch/reload replaces its numeric editor draft, never a completed image or camera.
pub(super) fn load(app: &mut SolveApp) {
    let setup = app.session.doc.render.setups.get(&app.render_workspace.view.name).cloned().unwrap_or_default();
    let view = &mut app.render_workspace.view;
    view.method = ImageMethod::Raster;
    view.transparent = false;
    view.raytrace = RaytraceOptions::default();
    view.cpu = cpu::View::default();
    view.samples = 64;
    view.max_depth = 8;
    view.seed = 0;
    view.studio = studio::View::default();
    view.materials = materials::View::default();
    match &setup.job {
        Some(RenderJob::Raster { transparent_background }) => view.transparent = *transparent_background,
        Some(RenderJob::CpuRaytrace { options }) => {
            view.method = ImageMethod::CpuRaytrace;
            view.raytrace = RaytraceOptions {
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
    view.dirty = false;
    app.render_workspace.revision = app.session.revision;
    // Keep the existing explicit Restore Saved Camera action: loading options does not navigate.
    app.render_workspace.view.clean_setup = draft(app).ok();
}

pub(super) fn refresh_dirty(app: &mut SolveApp) {
    if let Some(clean) = &app.render_workspace.view.clean_setup
        && (has_runtime_inputs(app) || draft(app).as_ref() != Ok(clean))
    {
        app.render_workspace.view.dirty = true;
    }
}

pub(super) fn save(app: &mut SolveApp) -> Result<(), String> {
    let mut setup = snapshot(app)?;
    let name = app.render_workspace.view.name.clone();
    let capture = hdr::active(app).and_then(|target| match hdr::input(app, target) {
        hdr::Input::FileCaptured { path, metadata } => Some((target, path.clone(), metadata.sha256)),
        _ => None,
    });
    if let Some((_, path, digest)) = &capture {
        let mut params = serde_json::json!({"name":name,"path":path,"expected_sha256":digest});
        match &mut setup.job {
            Some(RenderJob::CpuRaytrace { options }) => {
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
        params["set"] = serde_json::json!(setup);
        // The expected digest rejects an externally changed file before any resource/setup edit.
        app.run("render.settings.capture_hdr", params)?;
    } else {
        app.run("render.settings.set", serde_json::json!({"name":name,"set":setup}))?;
    }
    let setup = app.session.doc.render.setups.get(&name).cloned().ok_or_else(|| app.tr("render.setup_save_missing").to_string())?;
    if let Some((target, _, resource)) = capture {
        match target {
            hdr::Target::Cpu => app.render_workspace.view.cpu.hdr = hdr::Input::EmbeddedResource { resource },
            hdr::Target::Gpu => app.render_workspace.view.studio.hdr = hdr::Input::EmbeddedResource { resource },
        }
    }
    app.render_workspace.view.draft = setup.clone();
    app.render_workspace.view.clean_setup = Some(setup);
    app.render_workspace.view.dirty = false;
    app.render_workspace.revision = app.session.revision;
    Ok(())
}
