//! Deterministic first-hit CPU guides, generated once without touching the sampling stream.
use super::bvh::TraceCursor;
use super::image::{error, invalid, primary_ray};
use super::prepared::PreparedScene;
use super::{Hit, Ray, WorkBudget};
use crate::job::{CancelToken, ImageRequest, RaytraceOptions, Result, denoise::Guide};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

pub(super) struct Guides {
    pixels: Vec<Guide>,
    count: usize,
    cursor: Option<TraceCursor>,
    ray: Option<Ray>,
    hit: Option<Hit>,
    emitter_cursor: usize,
    analytic_emitter: bool,
    effect_cursor: usize,
    effect: Option<usize>,
    pending: Option<Guide>,
}
impl Guides {
    pub(super) fn required_bytes(width: u32, height: u32) -> Result<u64> {
        (u64::from(width) * u64::from(height))
            .checked_mul(crate::job::denoise::GUIDE_BYTES)
            .and_then(|v| v.checked_add(std::mem::size_of::<Self>() as u64))
            .ok_or_else(|| invalid("CPU denoising guide size overflow"))
    }
    pub(super) fn new(width: u32, height: u32) -> Result<Self> {
        let count = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| invalid("CPU guide dimensions overflow"))?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(count).map_err(|_| error(super::TraceError::AllocationFailed))?;
        if count == 0 || pixels.capacity() != count {
            return Err(invalid("CPU guide capacity exceeds admission"));
        }
        Ok(Self {
            pixels,
            count,
            cursor: None,
            ray: None,
            hit: None,
            emitter_cursor: 0,
            analytic_emitter: false,
            effect_cursor: 0,
            effect: None,
            pending: None,
        })
    }
    #[cfg(test)]
    pub(super) fn ready_to_store(&self, options: &RaytraceOptions) -> bool {
        let scanned_emitters = options.path.is_none() || self.emitter_cursor >= options.area_lights.as_ref().map_or(0, Vec::len);
        self.pending.is_some() || (self.ray.is_some() && self.cursor.is_none() && scanned_emitters && (self.hit.is_none() || self.analytic_emitter))
    }
    #[cfg(test)]
    pub(super) fn emitter_scans(&self) -> usize {
        self.emitter_cursor
    }
    pub(super) fn completed_pixels(&self) -> usize {
        self.pixels.len()
    }
    pub(super) fn ready(&self) -> bool {
        self.pixels.len() == self.count
    }
    pub(super) fn pixels(&self) -> Result<&[Guide]> {
        if !self.ready() {
            return Err(invalid("CPU denoising guides are incomplete"));
        }
        Ok(&self.pixels)
    }
    fn append(&mut self, guide: Guide, work: &mut WorkBudget, cancel: &CancelToken) -> Result<()> {
        work.tick(cancel).map_err(error)?;
        guide.validate()?;
        if self.ready() {
            return Err(invalid("CPU denoising guide cursor exceeds admission"));
        }
        self.pixels.push(guide);
        self.ray = None;
        self.cursor = None;
        self.hit = None;
        self.emitter_cursor = 0;
        self.analytic_emitter = false;
        self.effect_cursor = 0;
        self.effect = None;
        self.pending = None;
        Ok(())
    }
    /// A continuation creates one ray, visits one BVH node/triangle, scans one
    /// bounded emitter/effect row, or resolves/stores one guide. The normal image budget is shared.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn step(
        &mut self,
        scene: PreparedScene<'_>,
        request: &ImageRequest,
        options: &RaytraceOptions,
        clip: (f64, f64),
        effects: &[(usize, crate::surface_patterns::PreparedEffect)],
        mut maps: Option<&mut crate::job::textures::cpu_cache::CpuTextureCache>,
        mut aging: Option<&mut super::aging::Task>,
        work: &mut WorkBudget,
        cancel: &CancelToken,
    ) -> Result<()> {
        cancel.check()?;
        if self.ready() {
            return Err(invalid("CPU denoising guides are already complete"));
        }
        if let Some(guide) = self.pending {
            return self.append(guide, work, cancel);
        }
        if self.ray.is_none() {
            work.tick(cancel).map_err(error)?;
            let pixel = self.pixels.len();
            let width = request.width as usize;
            let ray = primary_ray(request, (pixel % width) as f64 + 0.5, (pixel / width) as f64 + 0.5, clip).map_err(error)?;
            if let Some(maps) = &mut maps {
                maps.reset_hit();
            }
            if let Some(aging) = &mut aging {
                aging.reset();
            }
            self.ray = Some(ray);
            self.cursor = Some(TraceCursor::new(ray, false));
            return Ok(());
        }
        if let Some(cursor) = &mut self.cursor {
            if let Some(hit) = cursor.step(scene.bvh, work, cancel).map_err(error)? {
                self.cursor = None;
                self.hit = hit;
            }
            return Ok(());
        }
        let ray = self.ray.ok_or_else(|| invalid("CPU guide ray disappeared"))?;
        if options.path.is_some()
            && let Some(input) = options.area_lights.as_ref().and_then(|lights| lights.get(self.emitter_cursor))
        {
            // The path producer makes analytic rectangles visible. A guide for the
            // body behind a closer panel would incorrectly filter its emitted pixels.
            // One source scan owns one debit; equal-depth geometry still wins.
            work.tick(cancel).map_err(error)?;
            let light = super::indirect::Rectangle::new(input).map_err(error)?;
            if let Some((distance, _)) = light.intersect(ray).map_err(error)?
                && self.hit.is_none_or(|hit| distance < hit.geometry.distance)
            {
                self.analytic_emitter = true;
            }
            self.emitter_cursor += 1;
            return Ok(());
        }
        if self.analytic_emitter {
            return self.append(Guide::default(), work, cancel);
        }
        let Some(hit) = self.hit else {
            return self.append(Guide::default(), work, cancel);
        };
        work.tick(cancel).map_err(error)?;
        let mut shared = scene.resolve_hit(ray, hit).map_err(error)?;
        if let Some((body, _)) = effects.get(self.effect_cursor) {
            if *body == shared.body_index {
                self.effect = Some(self.effect_cursor);
                self.effect_cursor = effects.len();
            } else {
                self.effect_cursor += 1;
            }
            return Ok(());
        }
        if let Some(index) = self.effect {
            let (_, effect) = effects.get(index).ok_or_else(|| invalid("CPU guide effect disappeared"))?;
            let changed = effect.apply(shared.surface, shared.shading_normal, shared.oriented_geometric_normal(), shared.position)?;
            shared.surface = changed.surface;
            shared.shading_normal = changed.shading_normal;
        }
        let has_aging = aging.is_some();
        if let Some(maps) = maps {
            if !maps.hit_started() {
                maps.begin_hit(
                    crate::surface_patterns::ShadingSurface { surface: shared.surface, shading_normal: shared.shading_normal },
                    shared.body_index,
                    crate::job::textures::cpu_preparation::MapPoint {
                        position: shared.position,
                        geometric_normal: shared.oriented_geometric_normal(),
                        view_direction: -ray.direction(),
                    },
                )?;
            }
            if !maps.hit_ready() {
                let ready = maps.advance_hit()?;
                // One map-role transition owns this debit. Aging starts on a
                // subsequent continuation, after the complete mapped closure.
                if !ready || has_aging {
                    return Ok(());
                }
            }
            let mapped = maps.surface()?;
            shared.surface = mapped.surface;
            shared.shading_normal = mapped.shading_normal;
        }
        if let Some(aging) = aging {
            if !aging.started() {
                aging.begin(
                    crate::surface_patterns::ShadingSurface { surface: shared.surface, shading_normal: shared.shading_normal },
                    shared.position,
                    shared.geometric_normal,
                    shared.body_index,
                    super::aging::eligible(options, shared.body_index, shared.body_name),
                )?;
            }
            if !aging.ready() {
                let stack = options.aging.as_deref().ok_or_else(|| invalid("CPU guide aging stack disappeared"))?;
                if !aging.advance(stack)? {
                    return Ok(());
                }
            }
            let changed = aging.surface()?;
            shared.surface = changed.surface;
            shared.shading_normal = changed.shading_normal;
        }
        // Match the GPU's first-hit policy. The explicit CPU Mirror mode is the
        // corresponding sharp-reflection override even on a dielectric base colour.
        let glass = options.dielectrics.iter().any(|value| value.body_index == shared.body_index);
        let mirror = options.reflections.as_ref().is_some_and(|settings| {
            settings.bodies.iter().any(|value| value.body == shared.body_name && value.mode == crate::job::ReflectionMode::Mirror)
        });
        let emitter = options
            .emissions
            .iter()
            .flatten()
            .any(|value| value.body_index == shared.body_index && value.strength > 0.0 && value.color.iter().any(|v| *v > 0.0));
        let smooth_metal = shared.surface.metallic() > 0.5 && shared.surface.roughness() < 0.25;
        let sharp_coat = shared.surface.clear_coat().is_some_and(|coat| coat.weight() > 0.0 && coat.roughness() < 0.05);
        if glass || mirror || emitter || smooth_metal || sharp_coat {
            self.pending = Some(Guide::default());
            return Ok(());
        }
        let geometric = shared.oriented_geometric_normal();
        let normal = if shared.shading_normal.dot(-ray.direction()) > 0.0 { shared.shading_normal } else { geometric };
        let camera = &request.camera;
        let relative = shared.position - camera.eye();
        let depth = relative.dot(-camera.back());
        let mut footprint = 2.0 * camera.half_height() / f64::from(request.height);
        if camera.fov != 0.0 {
            footprint *= depth.max(0.0) / camera.distance;
        }
        let array = |v: Vec3| [v.x as f32, v.y as f32, v.z as f32];
        let body = u32::try_from(shared.body_index)
            .ok()
            .and_then(|body| body.checked_add(1))
            .ok_or_else(|| invalid("CPU denoising body identity overflow"))?;
        let guide = Guide {
            albedo: array(shared.surface.base_color()),
            body,
            normal: array(normal),
            depth: depth as f32,
            position: array(relative),
            footprint: footprint.max(0.000001) as f32,
        };
        guide.validate()?;
        self.pending = Some(guide);
        Ok(())
    }
}
