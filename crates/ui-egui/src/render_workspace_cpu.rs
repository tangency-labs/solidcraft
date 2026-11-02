//! Transient CPU image controls; engine commands own file capture and admission.
use serde_json::{Value, json};
use solidcraft_engine::render::job::{CpuBudget, RaytraceEnvironmentOptions, ReflectionBody, ReflectionMode};

use super::hdr;
use crate::{SolveApp, ui_kit};

pub(super) fn validate_progressive_image(app: &SolveApp) -> Result<(), String> {
    let view = &app.render_workspace.view;
    if view.method == super::ImageMethod::GpuRaytrace {
        return Ok(());
    }
    // A size refusal is shown translated, naming the profile that can render it (#8941). Both CPU
    // methods stop here, before the English refusals of the render and document crates.
    let (width, height) = (view.draft.width, view.draft.height);
    let size = (u64::from(width), u64::from(height));
    if width > 0 && height > 0 && image_limits(app).fit(size.0, size.1, None) != Some(size) {
        return Err(super::inspector::size_limit_reason(app, width, height));
    }
    if view.method == super::ImageMethod::CpuRaytrace {
        view.raytrace.budget.validate_image(width, height, view.raytrace.progressive).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(super) fn image_limits(app: &SolveApp) -> solidcraft_engine::render::RenderLimits {
    if app.render_workspace.view.method == super::ImageMethod::GpuRaytrace {
        return solidcraft_engine::render::job::GPU_PATH_LIMITS;
    }
    if app.render_workspace.view.method != super::ImageMethod::CpuRaytrace {
        return Default::default();
    }
    let options = &app.render_workspace.view.raytrace;
    let mut limits = options.budget.image_limits(options.progressive.is_some());
    if let Some(progressive) = options.progressive {
        limits.max_pixels = limits.max_pixels.min(options.budget.limits().max_camera_samples / u64::from(progressive.samples_per_pixel.max(1)));
    }
    limits
}

/// For a size a CPU method refuses, the smallest CPU ray tracing profile that can render it, to
/// name in a hint. `None` for GPU path tracing, for sizes that fit, and past every profile. CPU
/// raster has no profiles of its own, so its hint also names CPU ray tracing. Neither the method
/// nor the profile is switched here: they and the resolution change only when chosen.
pub(super) fn profile_for_size(app: &SolveApp, width: u64, height: u64) -> Option<CpuBudget> {
    if app.render_workspace.view.method == super::ImageMethod::GpuRaytrace || image_limits(app).fit(width, height, None) == Some((width, height)) {
        return None;
    }
    CpuBudget::smallest_admitting(u32::try_from(width).ok()?, u32::try_from(height).ok()?, app.render_workspace.view.raytrace.progressive)
}

/// The translated name of a profile, as the profile selector shows it.
pub(super) fn budget_name(app: &SolveApp, budget: CpuBudget) -> &str {
    app.tr(budget_key(budget))
}

fn budget_key(budget: CpuBudget) -> &'static str {
    match budget {
        CpuBudget::Compact => "render.children.compact",
        CpuBudget::Studio => "render.children.studio",
        CpuBudget::Workstation => "render.children.workstation",
    }
}

fn budget_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let budgets = CpuBudget::ALL;
    let mut selected = budgets.iter().position(|b| *b == app.render_workspace.view.raytrace.budget).unwrap_or(0);
    let labels = budgets.map(|b| app.tr(budget_key(b)).to_string());
    let response = ui.scope(|ui| {
        ui_kit::segmented(
            ui,
            &mut selected,
            &labels.iter().map(String::as_str).collect::<Vec<_>>(),
            ui_kit::Segments {
                compact: true,
                targets: &["render.cpu_budget_compact", "render.cpu_budget_studio", "render.cpu_budget_workstation"],
                ..Default::default()
            },
        )
    });
    crate::scenario::publish_rect("render:cpu-budget", response.response.rect);
    if response.inner
        && let Some(budget) = budgets.get(selected)
    {
        app.render_workspace.view.raytrace.budget = *budget;
        app.render_workspace.view.dirty = true;
    }
    let limits = app.render_workspace.view.raytrace.budget.limits();
    let note = format!(
        "{}: {:.2} GiB · {}: {} · {}: {} · {}: {}. {}",
        app.tr("render.cpu_budget_memory"),
        limits.max_job_bytes as f64 / 1_073_741_824.0,
        app.tr("render.cpu_budget_triangles"),
        limits.max_triangles,
        app.tr("render.cpu_budget_bodies"),
        limits.max_bodies,
        app.tr("render.cpu_progressive_spp"),
        limits.max_samples_per_pixel,
        app.tr("render.cpu_budget_not_guarantee")
    );
    ui_kit::text(ui, &note, ui_kit::Text::Note("render.cpu_budget_help"));
}
fn set_progressive(app: &mut SolveApp, enabled: bool) {
    let options = &mut app.render_workspace.view.raytrace;
    if enabled {
        options.progressive.get_or_insert_default();
        options.primary_samples = 1;
    } else {
        options.progressive = None;
    }
    app.render_workspace.view.dirty = true;
}

pub(super) fn progressive_controls(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    budget_controls(app, ui);
    let mut selected = usize::from(app.render_workspace.view.raytrace.progressive.is_some());
    let labels = [app.tr("render.children.single_image").to_string(), app.tr("render.cpu_progressive").to_string()];
    let response = ui.scope(|ui| {
        ui_kit::row(ui, app.tr("render.children.sampling"), |ui| {
            ui_kit::segmented(
                ui,
                &mut selected,
                &labels.iter().map(String::as_str).collect::<Vec<_>>(),
                ui_kit::Segments { compact: true, targets: &["render:cpu-fixed", "render:cpu-progressive"], ..Default::default() },
            )
        })
    });
    crate::scenario::publish_target("render:cpu-sampling", response.response.rect);
    if response.inner {
        set_progressive(app, selected == 1);
    }
    let enabled = app.render_workspace.view.raytrace.progressive.is_some();
    let mut options = app.render_workspace.view.raytrace.progressive.unwrap_or_default();
    if !enabled {
        ui_kit::text(ui, app.tr("render.children.progressive_reason"), ui_kit::Text::Note("render.children.progressive_reason"));
    }
    ui.add_enabled_ui(enabled, |ui| {
        let response =
            ui_kit::row(ui, app.tr("render.cpu_progressive_spp"), |ui| ui_kit::number_field(ui, &mut options.samples_per_pixel, 1..=4096, 1.0, ""));
        app.render_workspace.view.dirty |= response.changed();
        crate::scenario::publish_target("render:cpu-spp", response.rect);
        let response =
            ui_kit::row(ui, app.tr("render.cpu_progressive_seed"), |ui| ui_kit::number_field(ui, &mut options.seed, 0..=u32::MAX, 1.0, ""));
        app.render_workspace.view.dirty |= response.changed();
        crate::scenario::publish_target("render:cpu-seed", response.rect);
    });
    if enabled {
        app.render_workspace.view.raytrace.progressive = Some(options);
    }
    ui_kit::text(ui, app.tr("render.cpu_progressive_help"), ui_kit::Text::Note("render.cpu_progressive_help"));
    enabled
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Environment {
    #[default]
    None,
    Constant,
    Hdr,
}

#[derive(Clone, Debug)]
pub(super) struct View {
    #[cfg(test)]
    pub selected_body: Option<String>,
    #[cfg(test)]
    pub body_filter: String,
    pub environment: Environment,
    pub hdr: hdr::Input,
    pub rotation_rad: f64,
    pub intensity: f64,
    pub samples: u32,
    pub color: [f64; 3],
    pub checked: Option<Checked>,
    bindings: std::collections::BTreeMap<String, Binding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    ordinal: usize,
    feature: u64,
    component: u64,
}

#[derive(Clone, Debug)]
struct Binding {
    target: Target,
    revision: u64,
    document: std::sync::Weak<solidcraft_engine::doc::Document>,
}

#[derive(Clone, Debug)]
pub(super) struct Checked {
    pub request: Value,
    pub revision: u64,
    pub environment: Value,
    document: std::sync::Weak<solidcraft_engine::doc::Document>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            #[cfg(test)]
            selected_body: None,
            #[cfg(test)]
            body_filter: String::new(),
            environment: Environment::None,
            hdr: hdr::Input::default(),
            rotation_rad: 0.0,
            intensity: 1.0,
            samples: 16,
            color: [1.0; 3],
            checked: None,
            bindings: Default::default(),
        }
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 128 && !name.chars().any(char::is_control)
}

fn target(app: &SolveApp, name: &str) -> Option<Target> {
    if !valid_name(name) || app.session.model.is_stale() {
        return None;
    }
    let world = app.session.world_state();
    let mut matches = world.bodies.iter().filter(|body| app.session.body_visible(&body.name)).enumerate().filter(|(_, body)| body.name == name);
    let (ordinal, body) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(Target { ordinal, feature: body.feature, component: body.component })
}

fn unique_visible_body(app: &SolveApp, name: &str) -> bool {
    target(app, name).is_some()
}

fn binding_current(app: &SolveApp, name: &str) -> bool {
    app.render_workspace.view.cpu.bindings.get(name).is_some_and(|binding| {
        binding.revision == app.session.revision
            && std::sync::Weak::ptr_eq(&binding.document, &std::sync::Arc::downgrade(&app.session.doc))
            && target(app, name).as_ref() == Some(&binding.target)
    })
}

pub(super) fn revalidate(app: &mut SolveApp, name: &str) -> Result<(), String> {
    let current = target(app, name).ok_or_else(|| app.tr("render.cpu_body_missing").to_string())?;
    if app.render_workspace.view.cpu.bindings.get(name).is_none_or(|binding| binding.target != current) {
        return Err(app.tr("render.cpu_body_missing").to_string());
    }
    let revision = app.session.revision;
    let document = std::sync::Arc::downgrade(&app.session.doc);
    if let Some(binding) = app.render_workspace.view.cpu.bindings.get_mut(name) {
        binding.revision = revision;
        binding.document = document;
    }
    Ok(())
}

pub(super) fn refresh_render_revision(app: &mut SolveApp) {
    let mut sources = super::setups::SourceCache::default();
    let eligible: Vec<String> = app
        .render_workspace
        .view
        .cpu
        .bindings
        .iter()
        .filter(|(name, binding)| sources.same_model_source(app, &binding.document) && target(app, name).as_ref() == Some(&binding.target))
        .map(|(name, _)| name.clone())
        .collect();
    for name in eligible {
        if let Some(binding) = app.render_workspace.view.cpu.bindings.get_mut(&name) {
            binding.revision = app.session.revision;
            binding.document = std::sync::Arc::downgrade(&app.session.doc);
        }
    }
}

/// A removed, hidden or ambiguous body never silently becomes a different target.
pub(super) fn set_reflection(app: &mut SolveApp, name: &str, mode: Option<ReflectionMode>) -> Result<(), String> {
    let selected = if mode.is_some() { Some(target(app, name).ok_or_else(|| app.tr("render.cpu_body_missing").to_string())?) } else { None };
    // Changing the mode of an existing row is not a new body selection. Only an
    // explicit confirmation of the same target, or Off followed by Add, may
    // establish a fresh binding after the document or body identity changes.
    if mode.is_some()
        && app.render_workspace.view.raytrace.reflections.as_ref().is_some_and(|r| r.bodies.iter().any(|entry| entry.body == name))
        && !binding_current(app, name)
    {
        return Err(app.tr("render.cpu_binding_stale").to_string());
    }
    let message = app.tr("render.cpu_reflection_limit").to_string();
    let options = &mut app.render_workspace.view.raytrace;
    if let Some(mode) = mode {
        let mut reflections = options.reflections.clone().unwrap_or_default();
        if let Some(entry) = reflections.bodies.iter_mut().find(|entry| entry.body == name) {
            entry.mode = mode;
        } else {
            if reflections.bodies.len() >= 64 {
                return Err(message);
            }
            reflections.bodies.try_reserve_exact(1).map_err(|_| message.clone())?;
            if reflections.bodies.capacity() > 64 {
                return Err(message);
            }
            reflections.bodies.push(ReflectionBody { body: name.to_string(), mode });
        }
        options.reflections = Some(reflections);
    } else if let Some(reflections) = &mut options.reflections {
        reflections.bodies.retain(|entry| entry.body != name);
        if reflections.bodies.is_empty() {
            options.reflections = None;
        }
    }
    if let Some(target) = selected {
        app.render_workspace
            .view
            .cpu
            .bindings
            .insert(name.to_string(), Binding { target, revision: app.session.revision, document: std::sync::Arc::downgrade(&app.session.doc) });
    } else {
        app.render_workspace.view.cpu.bindings.remove(name);
    }
    Ok(())
}

/// Shared by the parent Check and Start builder. This does not read an HDR file.
pub(super) fn params(app: &SolveApp, params: &mut Value) -> Result<(), String> {
    let base = &app.render_workspace.view.raytrace;
    if base.anisotropic_faces.iter().any(Option::is_some)
        || !base.dielectrics.is_empty()
        || base.environment.is_some()
        || base.surface_effects.is_some()
        || base.emissions.is_some()
    {
        return Err(app.tr("render.cpu_options_pending").to_string());
    }
    if base.reflections.as_ref().is_some_and(|r| r.bodies.iter().any(|entry| !unique_visible_body(app, &entry.body))) {
        return Err(app.tr("render.cpu_body_missing").to_string());
    }
    if base.reflections.as_ref().is_some_and(|r| r.bodies.iter().any(|entry| !binding_current(app, &entry.body))) {
        return Err(app.tr("render.cpu_binding_stale").to_string());
    }
    let view = &app.render_workspace.view.cpu;
    let mut options = base.clone();
    if options.area_samples.is_some_and(|n| !matches!(n, 1 | 4 | 16 | 64)) {
        return Err(crate::i18n::tr("render.error_area_samples").into());
    }
    if options.area_lights.as_ref().is_none_or(Vec::is_empty) {
        options.area_samples = None;
        options.seed = None;
    }
    options.exposure_ev = app.render_workspace.view.draft.exposure_ev;
    // Runtime CPU requests keep the same single saved EV authority as typed jobs.
    params["settings"]["exposure_ev"] = json!(0.0);
    if view.environment != Environment::None {
        if !view.intensity.is_finite() || !(0.0..=64.0).contains(&view.intensity) {
            return Err(app.tr("render.cpu_environment_bounds").to_string());
        }
        match view.environment {
            Environment::Constant => options.environment = Some(RaytraceEnvironmentOptions { color: view.color, intensity: view.intensity }),
            Environment::Hdr => {
                if !view.rotation_rad.is_finite() || view.rotation_rad.abs() > 1e6 || !matches!(view.samples, 1 | 4 | 16 | 64) {
                    return Err(app.tr("render.cpu_environment_bounds").to_string());
                }
                let mut environment = hdr::request(app, hdr::Target::Cpu)?;
                environment["rotation_radians"] = json!(view.rotation_rad);
                environment["intensity"] = json!(view.intensity);
                environment["samples"] = json!(view.samples);
                params["environment"] = environment;
            }
            Environment::None => {}
        }
    }
    options.validate().map_err(|error| error.to_string())?;
    params["integrator"] = json!("raytrace");
    params["raytrace"] = json!(options);
    Ok(())
}

#[cfg(test)]
pub(super) fn controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    for (id, key, section) in [
        ("render_cpu_reflections", "render.cpu_reflections", 0),
        ("render_cpu_environment", "render.cpu_environment", 1),
        ("render_cpu_lights", "render.cpu_lights", 2),
    ] {
        let title = app.tr(key).to_string();
        ui_kit::section(ui, id, &title, false, |ui| match section {
            0 => reflection_controls(app, ui),
            1 => environment_controls(app, ui),
            _ => light_controls(app, ui),
        });
    }
    ui.small(app.tr("render.cpu_runtime_help"));
}

#[cfg(test)]
pub(super) fn reflection_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.small(app.tr("render.cpu_reflection_help"));
    let entries = app.render_workspace.view.raytrace.reflections.as_ref().map(|r| r.bodies.clone()).unwrap_or_default();
    for entry in entries {
        ui.label(&entry.body);
        if !binding_current(app, &entry.body) {
            ui.small(app.tr("render.cpu_binding_stale"));
            if ui_kit::button(ui, app.tr("render.cpu_binding_confirm"), ui_kit::Button::Normal).clicked()
                && let Err(error) = revalidate(app, &entry.body)
            {
                app.set_status(error, true);
            }
        }
        let selected = match entry.mode {
            ReflectionMode::Mirror => "render.cpu_reflection_mirror",
            ReflectionMode::RoughSpecular => "render.cpu_reflection_rough",
        };
        let label = app.tr(selected).to_string();
        let mode_control = ui_kit::dropdown_menu(ui, ("cpu_reflection_mode", &entry.body), &label, |ui| {
            for (mode, key) in [
                (None, "render.cpu_reflection_off"),
                (Some(ReflectionMode::Mirror), "render.cpu_reflection_mirror"),
                (Some(ReflectionMode::RoughSpecular), "render.cpu_reflection_rough"),
            ] {
                let enabled = mode.is_none() || binding_current(app, &entry.body);
                if ui.add_enabled_ui(enabled, |ui| ui_kit::list_picker(ui, key, app.tr(key), mode == Some(entry.mode))).inner.clicked() {
                    if let Err(error) = set_reflection(app, &entry.body, mode) {
                        app.set_status(error, true);
                    }
                    ui.close();
                }
            }
        });
        crate::scenario::publish_handle(&format!("render:cpu-reflection-mode:{}", entry.body), mode_control.rect.center());
    }
    let hint = app.tr("render.cpu_body_filter").to_string();
    ui_kit::bounded_cell_editor(ui, ui.id().with("cpu_body_filter"), &mut app.render_workspace.view.cpu.body_filter, &hint, false, 128);
    let selected = app.render_workspace.view.cpu.selected_body.clone();
    let label = selected.clone().unwrap_or_else(|| app.tr("render.cpu_body_choose").to_string());
    let world = app.session.world_state();
    let filter = app.render_workspace.view.cpu.body_filter.to_lowercase();
    let candidates: Vec<_> = world
        .bodies
        .iter()
        .filter(|body| app.session.body_visible(&body.name) && body.name.to_lowercase().contains(&filter))
        .take(257)
        .map(|body| body.name.as_str())
        .collect();
    let dropdown = ui_kit::dropdown_menu(ui, "cpu_reflection_body", &label, |ui| {
        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
            for (shown, name) in candidates.iter().enumerate() {
                if shown == 256 {
                    ui.small(app.tr("render.cpu_body_filter_more"));
                    break;
                }
                let response =
                    ui.add_enabled_ui(valid_name(name), |ui| ui_kit::list_picker(ui, name, name, selected.as_deref() == Some(*name))).inner;
                if response.clicked() {
                    app.render_workspace.view.cpu.selected_body = Some((*name).to_string());
                    ui.close();
                }
            }
        });
    });
    crate::scenario::publish_handle("render:cpu-reflection-body", dropdown.rect.center());
    let can_add = selected.as_ref().is_some_and(|name| unique_visible_body(app, name))
        && app.render_workspace.view.raytrace.reflections.as_ref().is_none_or(|r| r.bodies.len() < 64);
    let add = ui_kit::button(ui, app.tr("render.cpu_reflection_add"), ui_kit::Button::Enabled(can_add));
    crate::scenario::publish_handle("render:cpu-reflection-add", add.rect.center());
    if add.clicked()
        && let Some(name) = selected
        && let Err(error) = set_reflection(app, &name, Some(ReflectionMode::RoughSpecular))
    {
        app.set_status(error, true);
    }
    reflection_sampling_controls(app, ui);
}

/// Existing global reflection budget, shared during the Sampling-editor migration.
pub(super) fn reflection_sampling_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let depth = app.tr("render.cpu_reflection_depth").to_string();
    let samples = app.tr("render.cpu_reflection_samples").to_string();
    if let Some(reflections) = &mut app.render_workspace.view.raytrace.reflections {
        ui_kit::row(ui, &depth, |ui| ui_kit::number_field(ui, &mut reflections.max_bounces, 1..=8, 1.0, ""));
        ui_kit::row(ui, &samples, |ui| ui_kit::number_field(ui, &mut reflections.samples, 1..=64, 1.0, ""));
    } else {
        ui.small(app.tr("render.cpu_reflection_empty"));
    }
}

#[cfg(test)]
pub(super) fn environment_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let selected = app.render_workspace.view.cpu.environment;
    let key = match selected {
        Environment::None => "render.cpu_environment_none",
        Environment::Constant => "render.cpu_environment_constant",
        Environment::Hdr => "render.studio_hdr",
    };
    let label = app.tr(key).to_string();
    ui_kit::dropdown_menu(ui, "cpu_environment_kind", &label, |ui| {
        for (value, key) in [
            (Environment::None, "render.cpu_environment_none"),
            (Environment::Constant, "render.cpu_environment_constant"),
            (Environment::Hdr, "render.studio_hdr"),
        ] {
            if ui_kit::list_picker(ui, key, app.tr(key), value == selected).clicked() {
                app.render_workspace.view.cpu.environment = value;
                ui.close();
            }
        }
    });
    if app.render_workspace.view.cpu.environment == Environment::None {
        return;
    }
    if app.render_workspace.view.cpu.environment == Environment::Constant {
        let color = app.tr("render.cpu_linear_color").to_string();
        vector(ui, &color, &mut app.render_workspace.view.cpu.color, 0.0..=1.0, 0.01, "");
        ui.small(app.tr("render.cpu_constant_help"));
    } else {
        hdr::controls(app, ui, hdr::Target::Cpu);
        ui.small(app.tr("render.cpu_hdr_help"));
        ui.label(app.tr("render.studio_rotation"));
        let mut degrees = app.render_workspace.view.cpu.rotation_rad.to_degrees();
        if ui_kit::number_field(ui, &mut degrees, -1e6_f64.to_degrees()..=1e6_f64.to_degrees(), 1.0, "°").changed() {
            app.render_workspace.view.cpu.rotation_rad = degrees.to_radians();
        }
        ui.label(app.tr("render.cpu_environment_samples"));
        let samples = app.render_workspace.view.cpu.samples;
        ui_kit::dropdown_menu(ui, "cpu_hdr_samples", &samples.to_string(), |ui| {
            for value in [1, 4, 16, 64] {
                if ui_kit::list_picker(ui, &value.to_string(), &value.to_string(), value == samples).clicked() {
                    app.render_workspace.view.cpu.samples = value;
                    ui.close();
                }
            }
        });
        ui.small(app.tr("render.cpu_hdr_reflection_help"));
    }
    ui.label(app.tr("render.studio_intensity"));
    ui_kit::number_field(ui, &mut app.render_workspace.view.cpu.intensity, 0.0..=64.0, 0.1, "");
    ui.small(app.tr("render.cpu_environment_background"));
}

#[cfg(test)]
fn vector(ui: &mut egui::Ui, label: &str, value: &mut [f64; 3], range: std::ops::RangeInclusive<f64>, speed: f64, suffix: &str) {
    ui.label(label);
    ui.horizontal(|ui| {
        for component in value {
            ui_kit::number_field(ui, component, range.clone(), speed, suffix);
        }
    });
}

#[cfg(test)]
pub(super) fn light_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    super::light::lighting(app, ui);
}

pub(super) fn record_checked(app: &mut SolveApp, params: &Value, reply: &Value) {
    if params["integrator"] == "raytrace" || params["settings"]["job"]["method"] == "cpu_raytrace" {
        app.render_workspace.view.cpu.checked = Some(Checked {
            request: params.clone(),
            revision: app.session.revision,
            environment: reply["environment"].clone(),
            document: std::sync::Arc::downgrade(&app.session.doc),
        });
    }
}

fn metadata(app: &SolveApp, ui: &mut egui::Ui, info: &Value) {
    let Some(hash) = info["source_sha256"].as_array().filter(|hash| hash.len() == 32) else { return };
    let digest: Option<Vec<_>> = hash.iter().map(|byte| byte.as_u64().and_then(|n| u8::try_from(n).ok())).collect();
    let Some(digest) = digest else { return };
    let digest: String = digest.into_iter().map(|byte| format!("{byte:02x}")).collect();
    let width = info["header"]["width"].as_u64().unwrap_or(0);
    let height = info["header"]["height"].as_u64().unwrap_or(0);
    ui.label(format!("{width} × {height} px"));
    ui.label(format!("{}: {}", app.tr("render.cpu_environment_samples"), info["samples"].as_u64().unwrap_or(0)));
    ui.label(format!("{}: {:.2}", app.tr("render.studio_intensity"), info["intensity"].as_f64().unwrap_or(0.0)));
    ui.label(format!("{}: {:.1}°", app.tr("render.studio_rotation"), info["rotation_radians"].as_f64().unwrap_or(0.0).to_degrees()));
    ui.label(format!("SHA-256: {digest}"));
}

pub(super) fn checked_status(app: &SolveApp, ui: &mut egui::Ui, params: Option<&Value>) {
    if app.render_workspace.view.method != super::ImageMethod::CpuRaytrace {
        return;
    }
    if let Some(checked) = &app.render_workspace.view.cpu.checked {
        let title = app.tr("render.cpu_last_check").to_string();
        ui_kit::section(ui, "cpu_last_check", &title, false, |ui| {
            if params != Some(&checked.request)
                || checked.revision != app.session.revision
                || !std::sync::Weak::ptr_eq(&checked.document, &std::sync::Arc::downgrade(&app.session.doc))
            {
                ui.small(app.tr("render.cpu_check_changed"));
            }
            metadata(app, ui, &checked.environment);
            ui.small(app.tr("render.cpu_check_capture_help"));
        });
    }
}

pub(super) fn captured_status(app: &SolveApp, ui: &mut egui::Ui, status: &Value) {
    if status["request"]["integrator"] == "raytrace" && status["environment"]["source_sha256"].is_array() {
        let title = app.tr("render.cpu_captured_hdr").to_string();
        ui_kit::section(ui, "cpu_captured_hdr", &title, false, |ui| metadata(app, ui, &status["environment"]));
    }
}
