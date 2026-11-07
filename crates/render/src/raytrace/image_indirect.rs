//! One prepaid opaque path continuation. No ray queue, recursion or image storage.
use super::*;
use crate::raytrace::indirect::{self, Rectangle, multiply, power_weight};
#[path = "image_glass_model.rs"]
mod advanced;
#[path = "image_glass_lighting.rs"]
mod glass_lighting;
#[path = "image_scattering.rs"]
mod scattering;
#[path = "image_volume.rs"]
mod volume;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    New,
    Origin,
    Medium,
    MediumOrigin,
    MediumScatter,
    MediumSurface,
    Volume,
    VolumeSurface,
    VolumeShadow,
    Trace,
    Emitters,
    Resolve,
    Glass,
    GlassSheet,
    GlassLighting,
    ShadowBeer,
    ShadowGlass,
    Effects,
    Maps,
    Aging,
    Emission,
    Caustics,
    Directional,
    Punctual,
    Area,
    Environment,
    Sun,
    Shadow,
    ShadowEmitters,
    Add,
    Scatter,
    Commit,
}
pub(super) struct State {
    stage: Stage,
    optical: Option<GlassPath>,
    volume: Option<volume::Context>,
    origins: Vec<super::super::origin::Task>,
    ray: Option<Ray>,
    cursor: Option<TraceCursor>,
    hit: Option<super::super::Hit>,
    emitter: Option<(usize, f64, Vec3)>,
    emitter_cursor: usize,
    effect_cursor: usize,
    effect: Option<usize>,
    emission_cursor: usize,
    light: usize,
    sample: u32,
    depth: u8,
    throughput: Vec3,
    radiance: Vec3,
    previous_pdf: f64,
    previous_position: Vec3,
    texture_position: Vec3,
    texture_distance: f64,
    contribution: Vec3,
    after_shadow: Stage,
    shadow_ray: Option<Ray>,
    shadow_emitter: usize,
    shadow_excluded: Option<usize>,
    caustic_gather: super::caustics::Gather,
    glass_lighting: Option<glass_lighting::Context>,
    sampled_continuous: bool,
    smooth_caustic_chain: Option<u8>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            stage: Stage::New,
            optical: None,
            volume: None,
            origins: Vec::new(),
            ray: None,
            cursor: None,
            hit: None,
            emitter: None,
            emitter_cursor: 0,
            effect_cursor: 0,
            effect: None,
            emission_cursor: 0,
            light: 0,
            sample: 0,
            depth: 0,
            throughput: Vec3::new(1.0, 1.0, 1.0),
            radiance: Vec3::ZERO,
            previous_pdf: 0.0,
            previous_position: Vec3::ZERO,
            texture_position: Vec3::ZERO,
            texture_distance: 0.0,
            contribution: Vec3::ZERO,
            after_shadow: Stage::Directional,
            shadow_ray: None,
            shadow_emitter: 0,
            shadow_excluded: None,
            caustic_gather: Default::default(),
            glass_lighting: None,
            sampled_continuous: false,
            smooth_caustic_chain: None,
        }
    }
}
#[cfg(test)]
pub(super) struct ShadowProbe {
    pub ray_origin: Option<Vec3>,
    pub contribution: Vec3,
    pub probability: f64,
    pub from: Vec3,
    pub endpoint: Option<Vec3>,
    pub mis: Option<[f64; 2]>,
    pub radiance: Vec3,
}
#[cfg(test)]
pub(super) struct VolumeProbe {
    pub source: Vec3,
    pub incoming: Option<Vec3>,
    pub ray: Option<Ray>,
    pub shadow: Option<Ray>,
    pub mis: Option<[f64; 2]>,
    pub survival: f64,
    pub contribution: Vec3,
    pub radiance: Vec3,
    pub throughput: Vec3,
    pub depth: u8,
}
#[cfg(test)]
pub(super) struct MediumProbe {
    pub ray_origin: Option<Vec3>,
    pub depth: u8,
    pub delta: bool,
    pub pdf: f64,
    pub throughput: Vec3,
    pub from: Vec3,
    pub emitter: Option<Vec3>,
}
#[cfg(test)]
impl State {
    pub(super) fn hard_light_probe(&self) -> Option<(u8, u8, Vec3, Vec3)> {
        self.glass_lighting.as_ref()?.pending_probe().map(|(kind, pending)| (self.depth, kind, pending, self.radiance))
    }
    pub(super) fn continuous_sampled(&self) -> bool {
        self.sampled_continuous
    }
    pub(super) fn volume_probe(&self) -> Option<VolumeProbe> {
        self.volume.as_ref().map(|v| VolumeProbe {
            source: v.source,
            incoming: v.event,
            ray: self.ray,
            shadow: self.shadow_ray,
            mis: v.shadow_mis,
            survival: v.shadow_probability,
            contribution: self.contribution,
            radiance: self.radiance,
            throughput: self.throughput,
            depth: self.depth,
        })
    }
    pub(super) fn surface_source(&self) -> Option<(u8, Ray, super::super::Hit)> {
        Some((self.depth, self.ray?, self.hit?))
    }
    pub(super) fn shadow_probe(&self) -> Option<ShadowProbe> {
        self.optical.as_ref().map(|o| ShadowProbe {
            ray_origin: o.shadow_current.map(Ray::origin),
            contribution: self.contribution,
            probability: o.shadow_probability,
            from: o.shadow_start,
            endpoint: o.shadow_endpoint,
            mis: o.shadow_mis,
            radiance: self.radiance,
        })
    }
    pub(super) fn medium_probe(&self) -> Option<MediumProbe> {
        self.optical.as_ref().map(|o| MediumProbe {
            ray_origin: self.ray.map(Ray::origin),
            depth: self.depth,
            delta: o.delta,
            pdf: self.previous_pdf,
            throughput: self.throughput,
            from: o.segment_start,
            emitter: self.emitter.map(|(_, _, p)| p),
        })
    }
    pub(super) fn origin_slot_info(&self) -> (usize, usize, bool) {
        (self.origins.len(), self.origins.capacity(), self.origins.first().is_some_and(|o| o.ready()))
    }
    pub(super) fn initial_medium(&self) -> Option<([usize; job::MAX_NESTED_MEDIA], usize, Vec3)> {
        self.optical.as_ref().map(|o| (o.media, o.len, o.segment_start))
    }
    pub(super) fn stage_name(&self) -> &'static str {
        match self.stage {
            Stage::New => "new",
            Stage::Origin => "origin",
            Stage::Medium => "medium",
            Stage::MediumOrigin => "medium_origin",
            Stage::MediumScatter => "medium_scatter",
            Stage::MediumSurface => "medium_surface",
            Stage::Volume => "volume",
            Stage::VolumeSurface => "volume_surface",
            Stage::VolumeShadow => "volume_shadow",
            Stage::Trace => "trace",
            Stage::Emitters => "emitters",
            Stage::Resolve => "resolve",
            Stage::Glass => "glass",
            Stage::GlassSheet => "glass_sheet",
            Stage::GlassLighting => "glass_lighting",
            Stage::ShadowBeer => "shadow_beer",
            Stage::ShadowGlass => "shadow_glass",
            Stage::Effects => "effects",
            Stage::Maps => "maps",
            Stage::Aging => "aging",
            Stage::Emission => "emission",
            Stage::Caustics => "caustics",
            Stage::Directional => "directional",
            Stage::Punctual => "punctual",
            Stage::Area => "area",
            Stage::Environment => "environment",
            Stage::Sun => "sun",
            Stage::Shadow => "shadow",
            Stage::ShadowEmitters => "shadow_emitters",
            Stage::Add => "add",
            Stage::Scatter => "scatter",
            Stage::Commit => "commit",
        }
    }
}
fn variates(task: &ImageTask, state: &State, domain: u64) -> job::Result<[f64; 2]> {
    let p = task.progressive.as_ref().ok_or_else(|| invalid("CPU path needs progressive film"))?;
    p.variates(task.pixel, (0x5041_5448_0000_u64 + u64::from(state.depth) * 0x10000).wrapping_add(domain))
}
fn remaining_depth(task: &ImageTask, state: &State) -> job::Result<u8> {
    Ok(task.options.path.ok_or_else(|| invalid("CPU path settings disappeared"))?.max_depth.saturating_sub(state.depth))
}
fn continuing(task: &ImageTask, state: &State) -> job::Result<bool> {
    Ok(remaining_depth(task, state)? > 1)
}
fn owned_caustic_tail(task: &ImageTask, state: &State) -> Option<job::CausticOptions> {
    let events = state.smooth_caustic_chain?;
    task.options.caustics.filter(|caustics| events > 0 && events < caustics.max_depth)
}
pub(super) fn environment_sample(task: &ImageTask, u: [f64; 2]) -> job::Result<(Vec3, Vec3, f64)> {
    if let Some(environment) = &task.environment {
        let env = environment.view().map_err(environment_error)?;
        env.require_linear_srgb().map_err(environment_error)?;
        let sample = env.sample(u).map_err(environment_error)?;
        return Ok((sample.direction, sample.radiance_source_rgb, sample.pdf_solid_angle));
    }
    let direction = indirect::uniform_environment(u);
    let radiance = if let Some(sky) = task.options.sky.as_deref() {
        sky.radiance(direction)?
    } else {
        let environment = task.options.environment.ok_or_else(|| invalid("CPU path environment missing"))?;
        Vec3::new(environment.color[0], environment.color[1], environment.color[2]) * environment.intensity
    };
    Ok((direction, radiance, indirect::ENVIRONMENT_PDF))
}
pub(super) fn advanced_normals(task: &ImageTask) -> bool {
    // An HG collision adds phase transport inside the medium. Its closure does
    // not use a boundary normal, so HG alone keeps the smooth interface policy.
    task.options.caustics.is_some()
        || task.options.dielectrics.iter().any(|input| input.model.is_some_and(|model| model.roughness > 0.0 || model.spectral()))
}
fn vertex_lighting(task: &ImageTask, state: &State) -> bool {
    task.options.caustics.is_none() || !state.sampled_continuous
}
fn shared<'a>(task: &'a ImageTask, state: &State) -> job::Result<super::super::prepared::SharedHit<'a>> {
    let mut hit = unaged(task, state)?;
    if let Some(aging) = task.aging.first() {
        let changed = aging.surface()?;
        hit.surface = changed.surface;
        hit.shading_normal = changed.shading_normal;
    }
    Ok(hit)
}
fn unaged<'a>(task: &'a ImageTask, state: &State) -> job::Result<super::super::prepared::SharedHit<'a>> {
    let mut hit = unmapped(task, state)?;
    if let Some(maps) = task.maps.first() {
        let mapped = maps.surface()?;
        hit.surface = mapped.surface;
        hit.shading_normal = mapped.shading_normal;
    }
    Ok(hit)
}
fn unmapped<'a>(task: &'a ImageTask, state: &State) -> job::Result<super::super::prepared::SharedHit<'a>> {
    let bvh = task.bvh.as_ref().ok_or_else(|| invalid("CPU path BVH missing"))?;
    let scene = PreparedScene { bvh, primitives: &task.looks, source: &task.scene };
    let mut hit = scene
        .resolve_hit(state.ray.ok_or_else(|| invalid("CPU path ray missing"))?, state.hit.ok_or_else(|| invalid("CPU path hit missing"))?)
        .map_err(error)?;
    if let Some(index) = state.effect {
        let (_, effect) = task.effects.get(index).ok_or_else(|| invalid("CPU path effect missing"))?;
        let changed = effect.apply(hit.surface, hit.shading_normal, hit.oriented_geometric_normal(), hit.position)?;
        hit.surface = changed.surface;
        hit.shading_normal = changed.shading_normal;
    }
    Ok(hit)
}
fn response(task: &ImageTask, state: &State, direction: Vec3) -> job::Result<Vec3> {
    if state.volume.is_some()
        && let Some(phase) = volume::response(task, state, direction)?
    {
        return Ok(Vec3::new(phase, phase, phase));
    }
    let hit = shared(task, state)?;
    let view = -state.ray.ok_or_else(|| invalid("CPU path ray missing"))?.direction();
    if hit.oriented_geometric_normal().dot(direction) <= 0.0 || hit.oriented_geometric_normal().dot(view) <= 0.0 {
        return Ok(Vec3::ZERO);
    }
    hit.surface
        .brdf(hit.shading_normal, view, direction)
        .map(|f| f * hit.shading_normal.dot(direction).max(0.0))
        .map_err(|_| invalid("CPU path direct response"))
}
fn light_weight(task: &ImageTask, state: &State, direction: Vec3, pdf: f64, samples: u32) -> job::Result<f64> {
    if !continuing(task, state)? || state.optical.is_some() || state.volume.is_some() {
        return Ok(1.0);
    }
    let hit = shared(task, state)?;
    let view = -state.ray.ok_or_else(|| invalid("CPU path ray missing"))?.direction();
    let bsdf = indirect::surface_pdf(hit.surface, hit.shading_normal, view, direction).map_err(error)?;
    power_weight(f64::from(samples) * pdf, bsdf).map_err(error)
}
fn queue(task: &ImageTask, state: &mut State, value: Vec3, ray: Option<Ray>, next: Stage) -> job::Result<()> {
    state.contribution = multiply(state.throughput, value);
    super::super::lighting::validate_radiance(state.contribution).map_err(error)?;
    state.after_shadow = next;
    state.shadow_excluded = None;
    let shadow_start = if state.optical.is_some() { Some(unmapped(task, state)?.position) } else { None };
    if let Some(optical) = &mut state.optical {
        optical.shadow_start = shadow_start.ok_or_else(|| invalid("CPU glass shadow origin disappeared"))?;
        optical.shadow_mis = None;
        optical.shadow_probability = 1.0;
        optical.shadow_interfaces = 0;
        optical.shadow_endpoint = None;
        optical.shadow_inclusive = true;
        optical.shadow_exact = false;
        optical.shadow_current = ray;
        optical.shadow_hit = None;
    }
    if let Some(v) = &mut state.volume {
        v.shadow_mis = None;
        v.shadow_probability = 1.0;
    }
    if task.options.shadows
        && value != Vec3::ZERO
        && let Some(ray) = ray
    {
        state.shadow_ray = Some(ray);
        state.shadow_emitter = 0;
        state.cursor = Some(if state.volume.is_some() { volume::cursor(task, state, ray)? } else { TraceCursor::new(ray, state.optical.is_none()) });
        state.stage = Stage::Shadow;
    } else {
        state.stage = Stage::Add;
    }
    Ok(())
}
fn bounded_shadow(hit: super::super::prepared::SharedHit<'_>, point: Vec3, direction: Vec3) -> job::Result<Option<Ray>> {
    let origin = hit.spawn(direction).map_err(error)?.origin();
    let delta = point - origin;
    let distance = delta.len();
    if delta.dot(direction) <= 0.0 || !distance.is_finite() || distance.next_down() <= 0.0_f64.next_up() {
        return Ok(None);
    }
    // Geometry wins equal-distance model/emitter ties in Stage::Emitters. Keep
    // the sampled endpoint in model visibility too; only the analytic source
    // itself is excluded separately from the emitter-occluder scan.
    Ray::new(origin, delta, 0.0_f64.next_up(), distance).map(Some).map_err(error)
}

pub(super) fn step(task: &mut ImageTask) -> job::Result<()> {
    // Taking this fixed slot avoids aliasing the source scene with the mutable worker.
    // Its exact capacity was reserved before transport; returning it cannot allocate.
    let mut state = task.path.pop().ok_or_else(|| invalid("CPU path continuation missing"))?;
    if state.stage == Stage::Commit {
        task.finish_sample(state.radiance)?;
        let mut origins = std::mem::take(&mut state.origins);
        origins.clear();
        task.path.push(State { origins, ..Default::default() });
        return Ok(());
    }
    let result = advance(task, &mut state);
    task.path.push(state);
    result
}
fn advance(task: &mut ImageTask, state: &mut State) -> job::Result<()> {
    if state.stage == Stage::GlassLighting {
        return glass_lighting::step(task, state);
    }
    if matches!(state.stage, Stage::Origin | Stage::MediumOrigin) {
        let origin = state.origins.first_mut().ok_or_else(|| invalid("CPU primary-origin certificate missing"))?;
        if origin.ready() {
            task.work.tick(&task.cancel).map_err(error)?;
            let (media, len, start) = origin.result()?;
            let optical = state.optical.as_mut().ok_or_else(|| invalid("CPU initial medium state missing"))?;
            if state.stage == Stage::MediumOrigin {
                if len != optical.len || media[..len] != optical.media[..len] {
                    return Err(invalid("CPU subsurface collision disagrees with the certified current medium"));
                }
                state.stage = Stage::MediumScatter;
                return Ok(());
            }
            optical.media = media;
            optical.len = len;
            optical.segment_start = start;
            optical.delta = true;
            state.cursor = Some(TraceCursor::new(state.ray.ok_or_else(|| invalid("CPU classified primary ray missing"))?, false));
            state.stage = Stage::Trace;
        } else {
            let certificate = task.glass.first().ok_or_else(|| invalid("CPU closed geometry certificate missing"))?;
            let bvh = task.bvh.as_ref().ok_or_else(|| invalid("CPU origin classification BVH missing"))?;
            origin.step(
                certificate,
                &task.options.dielectrics,
                PreparedScene { bvh, primitives: &task.looks, source: &task.scene },
                &mut task.work,
                &task.cancel,
            )?;
        }
        return Ok(());
    }

    if state.stage == Stage::Shadow && scattering::needs_shadow_setup(state) {
        task.work.tick(&task.cancel).map_err(error)?;
        scattering::shadow_setup(task, state)?;
        return Ok(());
    }
    if state.stage == Stage::Trace || state.stage == Stage::Shadow {
        let bvh = task.bvh.as_ref().ok_or_else(|| invalid("CPU path BVH missing"))?;
        let cursor = state.cursor.as_mut().ok_or_else(|| invalid("CPU path traversal cursor missing"))?;
        let complete = if state.optical.is_some() {
            let looks = &task.looks;
            let inputs = &task.options.dielectrics;
            cursor.step_ordered(bvh, &mut task.work, &task.cancel, |new, old| {
                let optical = |hit: super::super::Hit| {
                    looks.get(hit.primitive_index).is_some_and(|look| inputs.iter().any(|input| input.body_index == look.body_index))
                };
                // Opaque contact keeps its visible material. At a glass resting
                // on a plane, shading reflects into the incident medium; crossing
                // and then suppressing the coplanar plane would lose that surface.
                match (optical(new), optical(old)) {
                    (false, true) => true,
                    (true, false) => false,
                    _ => new.primitive_index < old.primitive_index,
                }
            })
        } else {
            cursor.step(bvh, &mut task.work, &task.cancel)
        }
        .map_err(error)?;
        let Some(hit) = complete else {
            return Ok(());
        };
        state.cursor = None;
        if state.stage == Stage::Shadow {
            if let Some(optical) = &mut state.optical {
                optical.shadow_hit = hit;
                state.stage = Stage::ShadowBeer;
            } else if hit.is_some() {
                state.contribution = Vec3::ZERO;
                state.stage = Stage::Add;
            } else {
                state.stage = if state.volume.is_some() { Stage::VolumeShadow } else { Stage::ShadowEmitters };
            }
        } else {
            state.hit = hit;
            volume::reset_hit(state);
            if let Some(optical) = &mut state.optical {
                optical.surface_range = None;
            }
            if let Some(maps) = task.maps.first_mut() {
                maps.reset_hit();
            }
            if let Some(aging) = task.aging.first_mut() {
                aging.reset();
            }
            state.emitter = None;
            state.emitter_cursor = 0;
            state.stage = Stage::Emitters;
        }
        return Ok(());
    }
    task.work.tick(&task.cancel).map_err(error)?;
    let track_texture_distance = task.options.texture_filtering && !task.maps.is_empty();
    match state.stage {
        Stage::New => {
            let p = task.progressive.as_ref().ok_or_else(|| invalid("CPU path progressive film missing"))?;
            if task.options.lens.is_some_and(|l| l.aperture_radius_mm > 0.0) && task.pupil_sample.is_none() {
                task.pupil_sample = Some(p.variates(task.pixel, 0x4c454e53)?);
                return Ok(());
            }
            let [dx, dy] = p.variates(task.pixel, 0)?;
            let x = (task.pixel % task.request.width as usize) as f64 + dx;
            let y = (task.pixel / task.request.width as usize) as f64 + dy;
            let mut physical_origin = None;
            let ray = if let Some(lens) = task.options.lens.filter(|l| l.aperture_radius_mm > 0.0) {
                let pupil = task.pupil_sample.take().ok_or_else(|| invalid("CPU path pupil missing"))?;
                let ray = lens_primary_ray(&task.request, x, y, task.clip, lens, pupil).map_err(error)?;
                if !task.glass.is_empty() || volume::enabled(task) || track_texture_distance {
                    let (right, up, _) = task.request.camera.basis();
                    let radius = lens.aperture_radius_mm * pupil[0].sqrt();
                    let phi = std::f64::consts::TAU * pupil[1];
                    physical_origin = Some(task.request.camera.eye() + (right * phi.cos() + up * phi.sin()) * radius);
                }
                ray
            } else {
                let ray = primary_ray(&task.request, x, y, task.clip).map_err(error)?;
                if (!task.glass.is_empty() || volume::enabled(task) || track_texture_distance) && task.request.camera.fov > 0.0 {
                    physical_origin = Some(task.request.camera.eye());
                }
                ray
            };
            let ray = if volume::enabled(task) { volume::primary(task, ray, physical_origin)? } else { ray };
            if volume::enabled(task) {
                volume::begin(task, state, ray)?;
            }
            if track_texture_distance {
                state.texture_position = physical_origin.unwrap_or(ray.origin());
                state.texture_distance = 0.0;
            }
            state.ray = Some(ray);
            state.cursor = Some(TraceCursor::new(ray, false));
            state.stage = Stage::Trace;
            if let Some(certificate) = task.glass.first() {
                state.optical = Some(GlassPath { segment_start: ray.origin(), ..Default::default() });
                let origin = physical_origin.unwrap_or(ray.origin());
                if super::super::origin::Task::required(certificate, &task.options.dielectrics, origin, ray.origin())? {
                    if !state.origins.is_empty() {
                        return Err(invalid("CPU prior origin certificate was not drained"));
                    }
                    if state.origins.capacity() == 0 {
                        state.origins.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
                    }
                    if state.origins.capacity() != 1 {
                        return Err(error(TraceError::MemoryLimit));
                    }
                    state.origins.push(super::super::origin::Task::new(origin, ray.origin())?);
                    state.cursor = None;
                    state.stage = Stage::Origin;
                }
            }
        }

        Stage::Emitters => {
            if let Some(input) = task.options.area_lights.as_ref().and_then(|v| v.get(state.emitter_cursor)) {
                let light = Rectangle::new(input).map_err(error)?;
                if let Some((distance, position)) = light.intersect(state.ray.ok_or_else(|| invalid("CPU path ray missing"))?).map_err(error)?
                    && state.hit.is_none_or(|hit| distance < hit.geometry.distance)
                    && state.emitter.is_none_or(|(_, prior, _)| distance < prior)
                {
                    state.emitter = Some((state.emitter_cursor, distance, position));
                }
                state.emitter_cursor += 1;
            } else {
                state.stage = if state.volume.is_some() {
                    Stage::Volume
                } else if scattering::active(task, state)? {
                    Stage::Medium
                } else {
                    Stage::Resolve
                };
            }
        }
        Stage::Medium | Stage::MediumScatter | Stage::MediumSurface => {
            if state.stage == Stage::MediumScatter {
                state.smooth_caustic_chain = None;
            }
            scattering::step(task, state)?;
        }
        Stage::Volume | Stage::VolumeSurface | Stage::VolumeShadow => volume::step(task, state)?,
        Stage::Resolve => {
            if state.optical.is_some() {
                if let Some((_, _, position)) = state.emitter {
                    beer(task, state, position, false)?;
                } else if state.hit.is_some() {
                    let position = unmapped(task, state)?.position;
                    beer(task, state, position, false)?;
                } else if state.optical.as_ref().is_some_and(|optical| optical.len != 0) {
                    return Err(invalid("CPU ray escaped a certified closed glass medium"));
                }
            }
            if let Some((index, _, position)) = state.emitter {
                // The photon prepass owns the reverse of an opaque continuation
                // followed exclusively by one or more smooth closed events.
                if owned_caustic_tail(task, state).is_some_and(|caustics| caustics.area_lights) {
                    state.stage = Stage::Commit;
                    return Ok(());
                }
                let input = task.options.area_lights.as_ref().and_then(|v| v.get(index)).ok_or_else(|| invalid("CPU path emitter disappeared"))?;
                let light = Rectangle::new(input).map_err(error)?;
                let weight = if state.depth == 0 || state.optical.as_ref().is_some_and(|optical| optical.delta) {
                    1.0
                } else {
                    power_weight(
                        state.previous_pdf,
                        f64::from(task.options.area_samples.unwrap_or(16)) * light.pdf(state.previous_position, position).map_err(error)?,
                    )
                    .map_err(error)?
                };
                state.radiance += multiply(state.throughput, light.radiance) * weight;
                if state.depth == 0 {
                    task.primary_coverage = 1;
                }
                state.stage = Stage::Commit;
            } else if state.hit.is_some() {
                if track_texture_distance {
                    let position = unmapped(task, state)?.position;
                    state.texture_distance += (position - state.texture_position).len();
                    state.texture_position = position;
                }
                if state.depth == 0 {
                    task.primary_coverage = 1;
                }
                state.effect_cursor = 0;
                state.effect = None;
                state.emission_cursor = 0;
                if let Some(optical) = &mut state.optical {
                    optical.selector = 0;
                    optical.selected = None;
                    state.stage = Stage::Glass;
                } else if state.volume.is_some() {
                    volume::surface(task, state)?;
                } else {
                    state.stage = Stage::Effects;
                }
            } else {
                // A camera miss remains the separate unexposed display background.
                if state.depth > 0 {
                    let direction = state.ray.ok_or_else(|| invalid("CPU escaped path ray missing"))?.direction();
                    if !owned_caustic_tail(task, state).is_some_and(|caustics| caustics.environment) {
                        let (radiance, pdf, n) = if let Some(environment) = &task.environment {
                            let env = environment.view().map_err(environment_error)?;
                            (
                                env.radiance_linear_srgb(direction).map_err(environment_error)?,
                                env.pdf_solid_angle(direction).map_err(environment_error)?,
                                environment.samples,
                            )
                        } else if let Some(sky) = task.options.sky.as_deref() {
                            (sky.radiance(direction)?, indirect::ENVIRONMENT_PDF, 1)
                        } else if let Some(environment) = task.options.environment {
                            (
                                Vec3::new(environment.color[0], environment.color[1], environment.color[2]) * environment.intensity,
                                indirect::ENVIRONMENT_PDF,
                                1,
                            )
                        } else {
                            (Vec3::ZERO, 0.0, 0)
                        };
                        let weight = if state.optical.as_ref().is_some_and(|optical| optical.delta) {
                            1.0
                        } else {
                            power_weight(state.previous_pdf, f64::from(n) * pdf).map_err(error)?
                        };
                        state.radiance += multiply(state.throughput, radiance) * weight;
                    }
                    if let Some(sun) = task.options.sun.as_deref().copied() {
                        let pdf = super::super::celestial::pdf(sun, direction).map_err(error)?;
                        let weight = if state.optical.as_ref().is_some_and(|o| o.delta) {
                            1.0
                        } else {
                            power_weight(state.previous_pdf, pdf).map_err(error)?
                        };
                        state.radiance += multiply(state.throughput, super::super::celestial::radiance(sun, direction).map_err(error)?) * weight;
                    }
                }
                state.stage = Stage::Commit;
            }
            super::super::lighting::validate_radiance(state.radiance).map_err(error)?;
        }
        Stage::Glass => {
            glass_one(task, state)?;
        }
        Stage::GlassSheet => advanced::sheet_step(task, state)?,
        Stage::ShadowBeer => {
            shadow_beer(task, state)?;
        }
        Stage::ShadowGlass => {
            shadow_glass(task, state)?;
        }
        Stage::Effects => {
            let hit = unmapped(task, state)?;
            if let Some((body, _)) = task.effects.get(state.effect_cursor) {
                if *body == hit.body_index {
                    state.effect = Some(state.effect_cursor);
                    state.effect_cursor = task.effects.len();
                } else {
                    state.effect_cursor += 1;
                }
            } else {
                state.stage = if !task.maps.is_empty() {
                    Stage::Maps
                } else if !task.aging.is_empty() {
                    Stage::Aging
                } else {
                    Stage::Emission
                };
            }
        }
        Stage::Maps => {
            let input = {
                let hit = unmapped(task, state)?;
                let has_face_override = task.looks.get(hit.primitive_index).is_none_or(|look| look.has_face_override);
                let ray = state.ray.ok_or_else(|| invalid("CPU mapped path ray missing"))?;
                // Exact primary differentials and an optional secondary ray cone
                // use the same receiver-plane footprint representation.
                let footprint = if state.depth == 0 {
                    primary_footprint(
                        &task.request,
                        &task.options,
                        task.progressive.as_ref(),
                        task.pixel,
                        task.spatial,
                        task.clip,
                        hit.position,
                        hit.oriented_geometric_normal(),
                    )?
                } else if track_texture_distance {
                    crate::job::textures::secondary_footprint(
                        &task.request.camera,
                        task.request.height,
                        state.texture_distance,
                        ray.direction(),
                        hit.oriented_geometric_normal(),
                    )?
                } else {
                    None
                };
                (
                    crate::surface_patterns::ShadingSurface { surface: hit.surface, shading_normal: hit.shading_normal },
                    (!has_face_override).then_some(hit.body_index),
                    crate::job::textures::cpu_preparation::MapPoint {
                        position: hit.position,
                        geometric_normal: hit.oriented_geometric_normal(),
                        view_direction: -ray.direction(),
                    },
                    footprint,
                )
            };
            let maps = task.maps.first_mut().ok_or_else(|| invalid("CPU path map cache disappeared"))?;
            if !maps.hit_started() {
                maps.begin_hit(input.0, input.1, input.2, input.3)?;
            }
            if maps.hit_ready() || maps.advance_hit()? {
                state.stage = if task.aging.is_empty() { Stage::Emission } else { Stage::Aging };
            }
        }
        Stage::Aging => {
            let input = {
                let hit = unaged(task, state)?;
                (
                    crate::surface_patterns::ShadingSurface { surface: hit.surface, shading_normal: hit.shading_normal },
                    hit.position,
                    hit.geometric_normal,
                    hit.body_index,
                    super::super::aging::eligible(&task.options, hit.body_index, hit.body_name),
                )
            };
            let aging = task.aging.first_mut().ok_or_else(|| invalid("CPU path aging task disappeared"))?;
            if !aging.started() {
                aging.begin(input.0, input.1, input.2, input.3, input.4)?;
            }
            if aging.ready() || aging.advance(task.options.aging.as_deref().ok_or_else(|| invalid("CPU path aging stack disappeared"))?)? {
                state.stage = Stage::Emission;
            }
        }
        Stage::Emission => {
            if let Some((body, emission)) = task.emissions.get(state.emission_cursor) {
                let hit = shared(task, state)?;
                if *body == hit.body_index {
                    state.radiance += multiply(state.throughput, emission.radiance(hit.front_face));
                }
                state.emission_cursor += 1;
                super::super::lighting::validate_radiance(state.radiance).map_err(error)?;
            } else {
                state.light = 0;
                state.caustic_gather = Default::default();
                state.stage = if task.caustics.is_empty() { Stage::Directional } else { Stage::Caustics };
            }
        }
        Stage::Caustics => {
            let hit = shared(task, state)?;
            let view = -state.ray.ok_or_else(|| invalid("CPU caustic receiver ray missing"))?.direction();
            let remaining = remaining_depth(task, state)?;
            if let Some(contribution) = super::caustics::gather(task, &mut state.caustic_gather, hit, view, remaining)? {
                state.radiance += multiply(state.throughput, contribution);
                super::super::lighting::validate_radiance(state.radiance).map_err(error)?;
            } else {
                state.stage = Stage::Directional;
            }
        }
        Stage::Directional => {
            if let Some(light) = task.lights.lights().get(state.light) {
                let value = multiply(response(task, state, light.direction())?, light.color()) * light.intensity();
                let ray =
                    volume::ray(task, state, light.direction(), None, true)?.ok_or_else(|| invalid("CPU directional source interval collapsed"))?;
                state.light += 1;
                queue(task, state, value, Some(ray), Stage::Directional)?;
            } else {
                state.light = 0;
                state.stage = Stage::Punctual;
            }
        }
        Stage::Punctual => {
            if let Some(input) = task.options.punctual_lights.as_ref().and_then(|v| v.get(state.light)) {
                let sample = input.light().map_err(error)?.sample(volume::source(task, state)?).map_err(error)?;
                state.light += 1;
                if let Some(why) = sample.omitted {
                    match why {
                        super::super::punctual::OmittedSample::NearSource => {
                            task.punctual_diagnostics.near_source = task.punctual_diagnostics.near_source.saturating_add(1)
                        }
                        super::super::punctual::OmittedSample::RadianceLimit => {
                            task.punctual_diagnostics.radiance_limit = task.punctual_diagnostics.radiance_limit.saturating_add(1)
                        }
                    }
                } else {
                    let value = multiply(response(task, state, sample.direction)?, sample.incident_linear_srgb);
                    if super::super::lighting::validate_radiance(value).is_err() {
                        task.punctual_diagnostics.radiance_limit = task.punctual_diagnostics.radiance_limit.saturating_add(1);
                    } else if let Some(ray) = if state.volume.is_some() {
                        volume::ray(
                            task,
                            state,
                            sample.direction,
                            Some(Vec3::new(input.position_mm[0], input.position_mm[1], input.position_mm[2])),
                            false,
                        )?
                    } else {
                        sample.shadow_ray(shared(task, state)?).map_err(error)?
                    } {
                        let endpoint = Vec3::new(input.position_mm[0], input.position_mm[1], input.position_mm[2]);
                        queue(task, state, value, Some(ray), Stage::Punctual)?;
                        if let Some(optical) = &mut state.optical {
                            optical.shadow_endpoint = Some(endpoint);
                            optical.shadow_inclusive = false;
                        }
                    } else {
                        task.punctual_diagnostics.shadow_offset_crossed = task.punctual_diagnostics.shadow_offset_crossed.saturating_add(1);
                    }
                }
            } else {
                state.light = 0;
                state.sample = 0;
                state.stage = Stage::Area;
            }
        }
        Stage::Area => {
            if let Some(input) = task.options.area_lights.as_ref().and_then(|v| v.get(state.light)) {
                let count = task.options.area_samples.unwrap_or(16);
                if state.sample == count {
                    state.light += 1;
                    state.sample = 0;
                    return Ok(());
                }
                let side = match count {
                    1 => 1,
                    4 => 2,
                    16 => 4,
                    _ => 8,
                };
                let [dx, dy] = variates(
                    task,
                    state,
                    (0x1000 + state.light as u64 * 128 + u64::from(state.sample)) ^ (u64::from(task.options.seed.unwrap_or(0)) << 32),
                )?;
                let u = [
                    indirect::open_stratum(dx, state.sample / side, side).map_err(error)?,
                    indirect::open_stratum(dy, state.sample % side, side).map_err(error)?,
                ];
                let light = Rectangle::new(input).map_err(error)?;
                state.sample += 1;
                if let Some(sample) = light.sample(volume::source(task, state)?, u).map_err(error)? {
                    let weight = light_weight(task, state, sample.direction, sample.pdf, count)?;
                    let value = multiply(response(task, state, sample.direction)?, light.radiance) * (weight / (f64::from(count) * sample.pdf));
                    if let Some(ray) = volume::ray(task, state, sample.direction, Some(sample.position), true)? {
                        queue(task, state, value, Some(ray), Stage::Area)?;
                        state.shadow_excluded = Some(state.light);
                        defer_mis(task, state, sample.direction, sample.pdf, count)?;
                        if let Some(optical) = &mut state.optical {
                            optical.shadow_endpoint = Some(sample.position);
                        }
                    }
                }
            } else {
                state.sample = 0;
                state.stage = Stage::Environment;
            }
        }
        Stage::Environment => {
            let count =
                task.environment.as_ref().map_or_else(|| u32::from(task.options.environment.is_some() || task.options.sky.is_some()), |e| e.samples);
            if state.sample == count {
                state.stage = if task.options.sun.is_some() { Stage::Sun } else { Stage::Scatter };
                return Ok(());
            }
            let side = match count {
                1 => 1,
                4 => 2,
                16 => 4,
                _ => 8,
            };
            let [dx, dy] = variates(task, state, 0x2000 + u64::from(state.sample))?;
            let u = [
                indirect::open_stratum(dx, state.sample / side, side).map_err(error)?,
                indirect::open_stratum(dy, state.sample % side, side).map_err(error)?,
            ];
            let (direction, emitted, pdf) = environment_sample(task, u)?;
            let weight = light_weight(task, state, direction, pdf, count)?;
            let value = multiply(response(task, state, direction)?, emitted) * (weight / (f64::from(count) * pdf));
            let ray = volume::ray(task, state, direction, None, true)?.ok_or_else(|| invalid("CPU environment source interval collapsed"))?;
            state.sample += 1;
            queue(task, state, value, Some(ray), Stage::Environment)?;
            defer_mis(task, state, direction, pdf, count)?;
        }
        Stage::Sun => {
            let sun = task.options.sun.as_deref().copied().ok_or_else(|| invalid("CPU finite Sun disappeared"))?;
            //0x5000/0x5001 are reserved for the independent SSS free-flight/HG arc.
            let u = variates(task, state, 0x7000)?;
            let sample = super::super::celestial::sample(sun, u).map_err(error)?;
            let weight = light_weight(task, state, sample.direction, sample.pdf, 1)?;
            let value = multiply(response(task, state, sample.direction)?, sample.radiance) * (weight / sample.pdf);
            let ray = volume::ray(task, state, sample.direction, None, true)?.ok_or_else(|| invalid("CPU finite Sun source interval collapsed"))?;
            queue(task, state, value, Some(ray), Stage::Scatter)?;
            defer_mis(task, state, sample.direction, sample.pdf, 1)?;
        }
        Stage::ShadowEmitters => {
            if let Some(input) = task.options.area_lights.as_ref().and_then(|v| v.get(state.shadow_emitter)) {
                let ray = state.shadow_ray.ok_or_else(|| invalid("CPU path finite shadow ray missing"))?;
                if state.shadow_excluded != Some(state.shadow_emitter)
                    && Rectangle::new(input).map_err(error)?.intersect(ray).map_err(error)?.is_some()
                {
                    state.contribution = Vec3::ZERO;
                    state.stage = Stage::Add;
                } else {
                    state.shadow_emitter += 1;
                }
            } else {
                state.stage = Stage::Add;
            }
        }
        Stage::Add => {
            let volume_mis = state.volume.as_mut().and_then(|v| v.shadow_mis.take().map(|mis| (mis, v.shadow_probability)));
            if let Some(([light, bsdf], survival)) = volume_mis {
                let weight = if !continuing(task, state)? { 1.0 } else { power_weight(light, bsdf * survival).map_err(error)? };
                state.contribution = state.contribution * weight;
            }
            if let Some(optical) = &mut state.optical
                && let Some([light, bsdf]) = optical.shadow_mis.take()
            {
                let max = task.options.path.ok_or_else(|| invalid("CPU glass path settings disappeared"))?.max_depth;
                let weight = if u16::from(state.depth) + 1 + u16::from(optical.shadow_interfaces) >= u16::from(max) {
                    1.0
                } else {
                    power_weight(light, bsdf * optical.shadow_probability).map_err(error)?
                };
                state.contribution = state.contribution * weight;
            }
            state.radiance += state.contribution;
            super::super::lighting::validate_radiance(state.radiance).map_err(error)?;
            state.stage = state.after_shadow;
        }
        Stage::Scatter => {
            if state.volume.as_ref().is_some_and(|v| v.event.is_some()) {
                volume::scatter(task, state)?;
                return Ok(());
            }
            if !continuing(task, state)? {
                state.stage = Stage::Commit;
                return Ok(());
            }
            let hit = shared(task, state)?;
            let [component, x] = variates(task, state, 0x3000)?;
            let [y, _] = variates(task, state, 0x3001)?;
            let sample = indirect::sample(
                hit.surface,
                hit.shading_normal,
                hit.oriented_geometric_normal(),
                -state.ray.ok_or_else(|| invalid("CPU path ray missing"))?.direction(),
                [component, x, y],
            )
            .map_err(error)?;
            if let Some(sample) = sample {
                state.throughput = multiply(state.throughput, sample.weight);
                super::super::lighting::validate_radiance(state.throughput).map_err(error)?;
                if state.throughput != Vec3::ZERO {
                    // Match the GPU opaque continuation policy. Keep this draw
                    // independent of the BSDF and preserve the expected weight.
                    if state.depth >= 3 {
                        let survive = state.throughput.x.max(state.throughput.y).max(state.throughput.z).clamp(0.05, 0.95);
                        let [draw, _] = variates(task, state, 0x3002)?;
                        if draw >= survive {
                            state.stage = Stage::Commit;
                            return Ok(());
                        }
                        state.throughput = state.throughput / survive;
                        super::super::lighting::validate_radiance(state.throughput).map_err(error)?;
                    }
                    state.sampled_continuous = true;
                    state.smooth_caustic_chain = task.options.caustics.filter(|caustics| caustics.area_lights || caustics.environment).map(|_| 0);
                    state.previous_pdf = sample.pdf;
                    state.previous_position = hit.position;
                    if let Some(optical) = &mut state.optical {
                        optical.delta = false;
                        optical.segment_start = hit.position;
                        optical.source_plane = Some(hit.vertices);
                    }
                    let (ray, cursor) = if state.volume.is_some() {
                        let ray =
                            volume::ray(task, state, sample.direction, None, true)?.ok_or_else(|| invalid("CPU volume surface launch collapsed"))?;
                        (ray, volume::cursor(task, state, ray)?)
                    } else if state.optical.as_ref().is_some_and(|o| o.surface_range.is_some()) {
                        scattering::surface_cursor(state, hit, sample.direction, super::super::MAX_DISTANCE_MM)?
                    } else {
                        let ray = hit.spawn(sample.direction).map_err(error)?;
                        (ray, TraceCursor::new(ray, false))
                    };
                    state.ray = Some(ray);
                    state.cursor = Some(cursor);
                    state.hit = None;
                    state.depth += 1;
                    state.stage = Stage::Trace;
                    return Ok(());
                }
            }
            state.stage = Stage::Commit;
        }
        Stage::Trace | Stage::Shadow | Stage::Commit | Stage::Origin | Stage::MediumOrigin | Stage::GlassLighting => {
            return Err(invalid("CPU path phase bypassed its continuation"));
        }
    }
    Ok(())
}

/// Conditional state on the existing single path slot: no ray tree or new film.
/// Interface events consume the same finite depth domain as opaque surfaces.
#[derive(Clone, Copy, Default)]
struct GlassPath {
    media: [usize; job::MAX_NESTED_MEDIA],
    len: usize,
    segment_start: Vec3,
    source_plane: Option<[Vec3; 3]>,
    collision: Option<scattering::Collision>,
    spectral_channel: Option<u8>,
    sheet: Option<advanced::SheetWalk>,
    surface_range: Option<[usize; 2]>,
    surface_forward: bool,
    shadow_exact: bool,
    delta: bool,
    selector: usize,
    selected: Option<usize>,
    shadow_start: Vec3,
    shadow_current: Option<Ray>,
    shadow_hit: Option<super::super::Hit>,
    shadow_endpoint: Option<Vec3>,
    shadow_inclusive: bool,
    shadow_probability: f64,
    shadow_interfaces: u8,
    shadow_mis: Option<[f64; 2]>,
}
fn current(optical: &GlassPath) -> job::Result<Option<usize>> {
    if optical.len == 0 { Ok(None) } else { optical.media.get(optical.len - 1).copied().map(Some).ok_or_else(|| invalid("CPU medium stack index")) }
}
fn beer(task: &ImageTask, state: &mut State, to: Vec3, shadow: bool) -> job::Result<()> {
    let optical = state.optical.as_ref().ok_or_else(|| invalid("CPU glass segment state missing"))?;
    let from = if shadow { optical.shadow_start } else { optical.segment_start };
    if let Some(index) = current(optical)? {
        let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU medium disappeared"))?;
        let factor = dielectric::attenuation(input.absorption_per_mm, from.dist(to)).map_err(error)?;
        if shadow {
            state.contribution = multiply(state.contribution, factor);
            if let Some(scattering) = input.subsurface.filter(|s| s.active()) {
                let survival = scattering.survival(from.dist(to)).map_err(|e| JobError::new(ErrorCode::InvalidScene, e.to_string()))?;
                state.contribution = state.contribution * survival;
                state.optical.as_mut().ok_or_else(|| invalid("CPU subsurface visibility state missing"))?.shadow_probability *= survival;
            }
        } else {
            state.throughput = multiply(state.throughput, factor);
        }
    }
    let optical = state.optical.as_mut().ok_or_else(|| invalid("CPU glass segment state missing"))?;
    if shadow {
        optical.shadow_start = to;
    } else {
        optical.segment_start = to;
    }
    Ok(())
}
fn thin_probability(r: Vec3, t: Vec3) -> job::Result<f64> {
    let reflected = (r.x + r.y + r.z) / 3.0;
    let transmitted = (t.x + t.y + t.z) / 3.0;
    if !reflected.is_finite() || !transmitted.is_finite() || reflected < 0.0 || transmitted < 0.0 {
        return Err(invalid("CPU glass sheet has no finite scattering mass"));
    }
    Ok(if reflected + transmitted == 0.0 { 0.0 } else { reflected / (reflected + transmitted) })
}
fn interface_cursor(
    task: &ImageTask,
    index: usize,
    hit: SharedHit<'_>,
    direction: Vec3,
    endpoint: Option<Vec3>,
    inclusive: bool,
) -> job::Result<(Ray, TraceCursor)> {
    let region = task.glass.first().ok_or_else(|| invalid("CPU glass geometry certificate missing"))?.region(index)?;
    let (direction, far) = if let Some(endpoint) = endpoint {
        let delta = endpoint - hit.position;
        let distance = delta.len();
        if !distance.is_finite() || distance <= 0.0 || delta.dot(direction) <= 0.0 {
            return Err(invalid("CPU glass finite connection crosses its endpoint"));
        }
        let far = if inclusive { distance } else { distance.next_down() };
        if far <= 0.0_f64.next_up() {
            return Err(invalid("CPU glass finite light interval is empty"));
        }
        (delta, far)
    } else {
        (direction, super::super::MAX_DISTANCE_MM)
    };
    // Physical boundary coordinates are retained. Exact source-plane exclusion
    // avoids consuming a short interior segment with a geometric offset.
    let ray = Ray::new(hit.position, direction, 0.0_f64.next_up(), far).map_err(error)?;
    let cursor = TraceCursor::from_plane(ray, region.primitive_begin..region.primitive_end, hit.vertices);
    Ok((ray, cursor))
}
fn glass_one(task: &mut ImageTask, state: &mut State) -> job::Result<()> {
    let hit = unmapped(task, state)?;
    let optical = state.optical.as_mut().ok_or_else(|| invalid("CPU glass selector state missing"))?;
    if let Some(input) = task.options.dielectrics.get(optical.selector) {
        if input.body_index == hit.body_index {
            if input.body_name != hit.body_name {
                return Err(invalid("CPU glass captured identity changed"));
            }
            optical.selected = Some(optical.selector);
            optical.selector = task.options.dielectrics.len();
        } else {
            optical.selector += 1;
        }
        return Ok(());
    }
    let Some(index) = optical.selected else {
        if scattering::enabled(task) {
            optical.surface_range =
                Some([hit.primitive_index, hit.primitive_index.checked_add(1).ok_or_else(|| invalid("CPU source primitive overflow"))?]);
            optical.surface_forward = false;
            state.stage = Stage::MediumSurface;
        } else {
            state.stage = Stage::Effects;
        }
        return Ok(());
    };
    let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU glass interface disappeared"))?;
    // A selected interface only reaches another endpoint after continuation.
    if let Some(events) = state.smooth_caustic_chain {
        state.smooth_caustic_chain = super::caustics::smooth_interface(input).then_some(events + 1);
    }
    if advanced::required(task, state, index)? {
        return advanced::surface(task, state, index);
    }
    if !continuing(task, state)? {
        state.stage = Stage::Commit;
        return Ok(());
    }
    let direction = state.ray.ok_or_else(|| invalid("CPU glass incident ray missing"))?.direction();
    let normal = hit.oriented_geometric_normal();
    let [draw, _] = variates(task, state, 0x4000)?;
    let mut preserve_pdf = false;
    let (outgoing, weight, entering) = if input.boundary == job::DielectricBoundary::ThinSheet {
        let normal = if advanced_normals(task) {
            normal
        } else {
            dielectric::shaded_interface(direction, normal, hit.shading_normal, 1.0, input.ior).map_err(error)?.1
        };
        let (interface, r, t) = dielectric::modeled_sheet(direction, normal, input).map_err(error)?;
        let p = thin_probability(r, t)?;
        if draw < p {
            (interface.reflected, r / p, None)
        } else {
            let probability = 1.0 - p;
            if probability <= 0.0 {
                return Err(invalid("CPU thin transmission has zero sampling mass"));
            }
            preserve_pdf = input.shadow_transmission;
            if preserve_pdf {
                state.previous_pdf *= probability;
            }
            (direction, t / probability, None)
        }
    } else {
        let optical = state.optical.as_ref().ok_or_else(|| invalid("CPU glass stack missing"))?;
        let current = current(optical)?;
        let region = task.glass.first().ok_or_else(|| invalid("CPU glass certificate missing"))?.region(index)?;
        let parent = region.parent;
        let next = if hit.front_face {
            if current != parent {
                return Err(invalid("CPU glass entered outside its certified medium parent"));
            }
            Some(index)
        } else {
            if current != Some(index) {
                return Err(invalid("CPU glass exit does not match the active medium"));
            }
            parent
        };
        let ior = |i: Option<usize>| -> job::Result<f64> {
            i.map(|i| task.options.dielectrics.get(i).map(|v| v.ior).ok_or_else(|| invalid("CPU glass IOR index"))).unwrap_or(Ok(1.0))
        };
        let ni = ior(current)?;
        let nt = ior(next)?;
        let interface = if advanced_normals(task) {
            dielectric::interface(direction, normal, ni, nt).map_err(error)?
        } else {
            dielectric::shaded_interface(direction, normal, hit.shading_normal, ni, nt).map_err(error)?.0
        };
        if draw < interface.reflectance || interface.transmitted.is_none() {
            (interface.reflected, Vec3::new(1.0, 1.0, 1.0), None)
        } else {
            (
                interface.transmitted.ok_or_else(|| invalid("CPU Snell transmission missing"))?,
                Vec3::new(1.0, 1.0, 1.0) * (ni / nt).powi(2),
                Some(hit.front_face),
            )
        }
    };
    let optical = state.optical.as_mut().ok_or_else(|| invalid("CPU glass stack missing"))?;
    if let Some(enter) = entering {
        if enter {
            let slot = optical.media.get_mut(optical.len).ok_or_else(|| invalid("CPU glass medium stack exceeds 16"))?;
            *slot = index;
            optical.len += 1;
        } else {
            optical.len = optical.len.checked_sub(1).ok_or_else(|| invalid("CPU glass stack underflow"))?;
        }
    }
    if !preserve_pdf {
        optical.delta = true;
        state.previous_pdf = 0.0;
        state.previous_position = hit.position;
    }
    optical.segment_start = hit.position;
    optical.source_plane = Some(hit.vertices);
    state.throughput = multiply(state.throughput, weight);
    super::super::lighting::validate_radiance(state.throughput).map_err(error)?;
    if state.throughput == Vec3::ZERO {
        state.stage = Stage::Commit;
        return Ok(());
    }
    let (ray, cursor) = interface_cursor(task, index, hit, outgoing, None, true)?;
    state.ray = Some(ray);
    state.cursor = Some(cursor);
    state.hit = None;
    state.depth += 1;
    state.stage = Stage::Trace;
    Ok(())
}
fn defer_mis(task: &ImageTask, state: &mut State, direction: Vec3, pdf: f64, count: u32) -> job::Result<()> {
    if state.volume.is_some() {
        if continuing(task, state)? {
            let bsdf = if let Some(phase) = volume::response(task, state, direction)? {
                phase
            } else {
                let hit = shared(task, state)?;
                indirect::surface_pdf(
                    hit.surface,
                    hit.shading_normal,
                    -state.ray.ok_or_else(|| invalid("CPU volume incident ray missing"))?.direction(),
                    direction,
                )
                .map_err(error)?
            };
            state.volume.as_mut().ok_or_else(|| invalid("CPU volume MIS state missing"))?.shadow_mis = Some([f64::from(count) * pdf, bsdf]);
        }
        return Ok(());
    }
    if state.optical.is_none() || !continuing(task, state)? {
        return Ok(());
    }
    let hit = shared(task, state)?;
    let view = -state.ray.ok_or_else(|| invalid("CPU glass NEE incident ray missing"))?.direction();
    let bsdf = indirect::surface_pdf(hit.surface, hit.shading_normal, view, direction).map_err(error)?;
    state.optical.as_mut().ok_or_else(|| invalid("CPU glass NEE state missing"))?.shadow_mis = Some([f64::from(count) * pdf, bsdf]);
    Ok(())
}
fn shadow_beer(task: &ImageTask, state: &mut State) -> job::Result<()> {
    let optical = state.optical.as_ref().ok_or_else(|| invalid("CPU glass visibility state missing"))?;
    if let Some(hit) = optical.shadow_hit {
        let ray = optical.shadow_current.ok_or_else(|| invalid("CPU glass visibility ray missing"))?;
        let bvh = task.bvh.as_ref().ok_or_else(|| invalid("CPU glass visibility BVH missing"))?;
        let shared = PreparedScene { bvh, primitives: &task.looks, source: &task.scene }.resolve_hit(ray, hit).map_err(error)?;
        beer(task, state, shared.position, true)?;
        let optical = state.optical.as_mut().ok_or_else(|| invalid("CPU glass visibility state missing"))?;
        optical.selector = 0;
        optical.selected = None;
        state.stage = Stage::ShadowGlass;
    } else {
        if let Some(endpoint) = optical.shadow_endpoint {
            beer(task, state, endpoint, true)?;
        } else if optical.len != 0 {
            return Err(invalid("CPU visibility escaped a certified medium"));
        }
        state.stage = Stage::ShadowEmitters;
    }
    Ok(())
}
fn shadow_glass(task: &ImageTask, state: &mut State) -> job::Result<()> {
    let optical = state.optical.as_ref().ok_or_else(|| invalid("CPU glass visibility state missing"))?;
    let ray = optical.shadow_current.ok_or_else(|| invalid("CPU glass visibility ray missing"))?;
    let hit = optical.shadow_hit.ok_or_else(|| invalid("CPU glass visibility hit missing"))?;
    let bvh = task.bvh.as_ref().ok_or_else(|| invalid("CPU glass visibility BVH missing"))?;
    let hit = PreparedScene { bvh, primitives: &task.looks, source: &task.scene }.resolve_hit(ray, hit).map_err(error)?;
    let optical = state.optical.as_mut().ok_or_else(|| invalid("CPU glass visibility state missing"))?;
    if let Some(input) = task.options.dielectrics.get(optical.selector) {
        if input.body_index == hit.body_index && input.body_name == hit.body_name {
            optical.selected = Some(optical.selector);
            optical.selector = task.options.dielectrics.len();
        } else {
            optical.selector += 1;
        }
        return Ok(());
    }
    let Some(index) = optical.selected else {
        state.contribution = Vec3::ZERO;
        state.stage = Stage::Add;
        return Ok(());
    };
    let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU glass visibility material disappeared"))?;
    if !input.shadow_transmission {
        state.contribution = Vec3::ZERO;
        state.stage = Stage::Add;
        return Ok(());
    }
    if input.boundary != job::DielectricBoundary::ThinSheet {
        return Err(invalid("CPU refractive closed-medium light connections require an unsupported caustic solver"));
    }
    let normal = if advanced_normals(task) {
        hit.oriented_geometric_normal()
    } else {
        dielectric::shaded_interface(ray.direction(), hit.oriented_geometric_normal(), hit.shading_normal, 1.0, input.ior).map_err(error)?.1
    };
    let (_, r, t) = dielectric::modeled_sheet(ray.direction(), normal, input).map_err(error)?;
    optical.shadow_probability *= 1.0 - thin_probability(r, t)?;
    optical.shadow_interfaces = optical
        .shadow_interfaces
        .checked_add(1)
        .filter(|n| *n <= 16)
        .ok_or_else(|| invalid("CPU thin light visibility exceeds sixteen interfaces"))?;
    state.contribution = multiply(state.contribution, t);
    if state.contribution == Vec3::ZERO {
        state.stage = Stage::Add;
        return Ok(());
    }
    let (ray, cursor) = interface_cursor(task, index, hit, ray.direction(), optical.shadow_endpoint, optical.shadow_inclusive)?;
    optical.shadow_current = Some(ray);
    optical.shadow_hit = None;
    state.cursor = Some(cursor);
    state.stage = Stage::Shadow;
    Ok(())
}
