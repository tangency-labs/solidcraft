//! One canonical direct-light continuation shared by the serial owner and
//! native camera workers. Geometry, lights and optical certificates are borrowed;
//! each worker owns only its continuation. Publication remains in camera order.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Pixel {
    pub pixel: usize,
    pub radiance: Vec3,
    pub covered: u32,
    pub attempts: u32,
    pub rgba: Option<[u8; 4]>,
}

struct Optical<'a> {
    certificates: &'a [Option<Certificate>; MAX_OVERRIDES],
    pending: &'a mut Option<DielectricPixel>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) struct State {
    pending: PendingPixelSlot,
    optical: Option<DielectricPixel>,
}
#[cfg(not(target_arch = "wasm32"))]
impl State {
    pub(super) fn required_bytes() -> u64 {
        std::mem::size_of::<Self>() as u64 + PendingPixelSlot::required_bytes()
    }
    pub(super) fn new(cancel: &CancelToken) -> job::Result<Self> {
        Ok(Self { pending: PendingPixelSlot::new(cancel)?, optical: None })
    }
}

pub(super) struct Context<'a> {
    common: TransportContext<'a>,
    pending: &'a mut PendingPixelSlot,
    optical: Option<Optical<'a>>,
    anisotropic_targets: &'a [Option<PlanarTarget>; 2],
    sample: u8,
    spatial_radiance: Vec3,
    finish_spatial: bool,
    emit_spatial_pixel: bool,
    pixel_radiance: Vec3,
    preserves_display: bool,
    pixel_output: Option<Pixel>,
}

impl<'a> Context<'a> {
    fn image(task: &'a mut ImageTask) -> Self {
        let preserves_display = task.preserves_display();
        Self {
            common: TransportContext {
                maps: &mut task.maps,
                aging: &mut task.aging,
                caustics: &task.caustics,
                punctual_diagnostics: &mut task.punctual_diagnostics,
                progressive: task.progressive.as_ref(),
                glass: &task.glass,
                environment: task.environment.as_ref(),
                scene: &task.scene,
                request: &task.request,
                options: &task.options,
                cancel: &task.cancel,
                lights: &task.lights,
                emissions: &task.emissions,
                effects: &task.effects,
                work: &mut task.work,
                clip: task.clip,
                pupil_sample: task.pupil_sample,
                looks: &task.looks,
                bvh: task.bvh.as_ref(),
                pixel: task.pixel,
                spatial: task.spatial,
                primary_coverage: task.primary_coverage,
                completed: None,
            },
            pending: &mut task.pending,
            optical: task.optical.first_mut().map(|optical| Optical { certificates: &optical.certificates, pending: &mut optical.pending }),
            anisotropic_targets: &task.anisotropic_targets,
            sample: task.sample,
            spatial_radiance: task.spatial_radiance,
            finish_spatial: task.finish_spatial,
            emit_spatial_pixel: task.emit_spatial_pixel,
            pixel_radiance: task.pixel_radiance,
            preserves_display,
            pixel_output: None,
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn worker(
        common: TransportContext<'a>,
        state: &'a mut State,
        certificates: Option<&'a [Option<Certificate>; MAX_OVERRIDES]>,
        anisotropic_targets: &'a [Option<PlanarTarget>; 2],
        preserves_display: bool,
    ) -> job::Result<Self> {
        if common.options.path.is_some()
            || common.options.dielectrics.is_empty() != certificates.is_none()
            || !state.pending.is_none()
            || state.optical.is_some()
        {
            return Err(invalid("CPU direct worker requires a drained certified continuation"));
        }
        Ok(Self {
            common,
            pending: &mut state.pending,
            optical: certificates.map(|certificates| Optical { certificates, pending: &mut state.optical }),
            anisotropic_targets,
            sample: 0,
            spatial_radiance: Vec3::ZERO,
            finish_spatial: false,
            emit_spatial_pixel: false,
            pixel_radiance: Vec3::ZERO,
            preserves_display,
            pixel_output: None,
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn completed(&self) -> Option<Pixel> {
        self.pixel_output
    }
    /// The worker only drains its private continuation. The image owner still
    /// validates and publishes this result in ordinal order after the batch.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn finish_worker(self) -> job::Result<Pixel> {
        let pixel = self.pixel_output.ok_or_else(|| invalid("CPU direct worker result missing"))?;
        if self.optical.as_ref().is_some_and(|optical| optical.pending.is_some()) {
            return Err(invalid("CPU direct worker completed before draining optical paths"));
        }
        self.pending.clear();
        Ok(pixel)
    }
    fn append_linear(&mut self, radiance: Vec3, covered: u32, attempts: u32) -> job::Result<()> {
        self.common.cancel.check()?;
        if self.pixel_output.is_some()
            || attempts == 0
            || covered > attempts
            || !bounded_radiance(radiance)
            || (covered == 0 && radiance != Vec3::ZERO)
        {
            return Err(invalid("CPU direct worker produced invalid radiance or coverage"));
        }
        self.pixel_output = Some(Pixel { pixel: self.common.pixel, radiance, covered, attempts, rgba: None });
        Ok(())
    }
    fn preserves_display(&self) -> bool {
        self.preserves_display
    }
    fn store_rgba(&mut self, rgba: [u8; 4]) -> job::Result<()> {
        let output = self.pixel_output.as_mut().ok_or_else(|| invalid("CPU direct display lacks a linear result"))?;
        if output.rgba.is_some() {
            return Err(invalid("CPU direct display was already produced"));
        }
        output.rgba = Some(rgba);
        Ok(())
    }
    fn sample_count(&self) -> u8 {
        self.common.options.reflections.as_ref().filter(|r| !r.bodies.is_empty()).map_or(1, |r| r.samples)
    }
    fn finish_sample(&mut self, radiance: Vec3) -> job::Result<()> {
        if self.common.progressive.is_some() {
            if self.common.primary_coverage > 1 {
                return Err(invalid("progressive camera sample has duplicate primary coverage"));
            }
            // Film publication may still fail its work debit or cancellation
            // check. Keep the continuation and coverage intact until it succeeds.
            self.append_linear(radiance, self.common.primary_coverage, 1)?;
            return Ok(());
        }
        if self.common.options.primary_samples != 1 {
            return self.finish_spatial_sample(radiance);
        }
        // Only the current fixed-centre, nonselected primary path is identical
        // across reflection samples: directional inputs and the environment's
        // fixed u-grid do not depend on self.sample, and this path never bounces.
        // Secondary nonselected hits and selected null samples still require R.
        let repeats =
            if self.sample == 0 && self.pending.as_ref().is_some_and(|p| p.repeatable_primary && p.bounce == 0) { self.sample_count() } else { 1 };
        // Reuse the shading result, but retain the original addition order,
        // per-add bound and final divisor exactly. At most64 scalar additions,
        // no repeated BVH/BSDF/environment work or new allocation/work budget.
        for _ in 0..repeats {
            self.pixel_radiance += radiance;
            if !self.pixel_radiance.is_finite()
                || [self.pixel_radiance.x, self.pixel_radiance.y, self.pixel_radiance.z].into_iter().any(|v| !(0.0..=1e12).contains(&v))
            {
                return Err(invalid("reflection radiance exceeds finite transport bound"));
            }
            self.sample += 1;
        }
        self.pending.clear();
        if self.sample == self.sample_count() {
            let radiance = self.pixel_radiance / self.sample as f64;
            self.append_linear(radiance, 1, 1)?;
            if self.preserves_display() {
                let display = map_to_display(radiance, self.common.options.exposure_ev).map_err(error)?;
                self.store_rgba([encode8(display.x), encode8(display.y), encode8(display.z), 255])?;
            }
        }
        Ok(())
    }
    fn finish_spatial_sample(&mut self, radiance: Vec3) -> job::Result<()> {
        // Same per-R-add finite bound and denominator, including zero/null events.
        self.pixel_radiance += radiance;
        if !bounded_radiance(self.pixel_radiance) {
            return Err(invalid("reflection radiance exceeds finite transport bound"));
        }
        self.sample = self.sample.checked_add(1).ok_or_else(|| invalid("reflection sample overflow"))?;
        if self.sample > self.sample_count() {
            return Err(invalid("reflection sample count exceeded"));
        }
        self.finish_spatial = self.sample == self.sample_count();
        let retain = !self.finish_spatial && self.pending.as_ref().is_some_and(|p| p.repeatable_primary && p.bounce == 0 && p.hit.is_some());
        if retain {
            // A completed primary, never a traversal/shading intermediate. The
            // existing light cursor has no other len+1 state (at most 8 lights).
            let completed = self.common.lights.lights().len() + 1;
            self.pending.as_mut().ok_or_else(|| invalid("missing reusable primary"))?.light = completed;
        } else {
            self.pending.clear();
        }
        Ok(())
    }
    fn spatial_transition(&mut self) -> job::Result<bool> {
        if self.finish_spatial {
            self.common.work.tick(self.common.cancel).map_err(error)?;
            self.spatial_radiance += self.pixel_radiance / f64::from(self.sample_count()) / f64::from(self.common.options.primary_samples);
            if !bounded_radiance(self.spatial_radiance) {
                return Err(invalid("spatial radiance exceeds finite transport bound"));
            }
            self.pixel_radiance = Vec3::ZERO;
            self.sample = 0;
            self.common.spatial = self.common.spatial.checked_add(1).ok_or_else(|| invalid("spatial sample overflow"))?;
            self.finish_spatial = false;
            self.emit_spatial_pixel = self.common.spatial == self.common.options.primary_samples;
            return Ok(true);
        }
        if self.emit_spatial_pixel {
            self.common.work.tick(self.common.cancel).map_err(error)?;
            let attempts = u32::from(self.common.options.primary_samples)
                .checked_mul(u32::from(self.sample_count()))
                .filter(|n| *n <= 1024)
                .ok_or_else(|| invalid("spatial coverage bound"))?;
            self.append_linear(self.spatial_radiance, self.common.primary_coverage, attempts)?;
            if self.preserves_display() {
                let rgba = composite_spatial_alpha(
                    self.spatial_radiance,
                    self.common.primary_coverage,
                    attempts,
                    self.common.request.background,
                    self.common.options.exposure_ev,
                    self.common.request.transparent_background,
                )?;
                self.common.cancel.check()?;
                self.store_rgba(rgba)?;
            }
            return Ok(true);
        }
        Ok(false)
    }
    pub(super) fn pixel_one(&mut self) -> job::Result<()> {
        if self.common.options.primary_samples != 1 && self.pending.as_ref().is_some_and(|p| p.light > self.common.lights.lights().len()) {
            let pending = self.pending.as_ref().ok_or_else(|| invalid("missing reusable primary"))?;
            if pending.light != self.common.lights.lights().len() + 1
                || !pending.repeatable_primary
                || pending.bounce != 0
                || pending.hit.is_none()
                || self.sample == 0
                || self.sample >= self.sample_count()
                || self.common.spatial >= self.common.options.primary_samples
                || self.finish_spatial
                || self.emit_spatial_pixel
            {
                return Err(invalid("invalid reusable primary state"));
            }
            if !pending.surface_ready
                || pending.surface_cursor != self.common.effects.len()
                || pending.emission_cursor != self.common.emissions.len()
                || pending.punctual_index != self.common.options.punctual_lights.as_ref().map_or(0, Vec::len)
                || pending.area_light != self.common.options.area_lights.as_ref().map_or(0, Vec::len)
                || pending.punctual_shadow
                || pending.punctual_ready
                || pending.area_shadow
                || pending.area_ready
                || pending.environment_cursor.is_some()
                || pending.environment_sample != self.common.environment.as_ref().map_or(0, |e| e.samples)
            {
                return Err(invalid("incomplete reusable primary shading"));
            }
            let radiance = pending.radiance;
            // One actual scalar continuation per logical R; no bulk R loop or
            // fabricated traversal debit. Check cancel/limit before mutations.
            self.common.work.tick(self.common.cancel).map_err(error)?;
            let bvh = self.common.bvh.as_ref().ok_or_else(|| invalid("missing reusable primary BVH"))?;
            let shared = resolved_surface_hit(
                &PreparedScene { bvh, primitives: self.common.looks, source: self.common.scene },
                pending,
                self.common.effects,
                self.common.maps.first(),
                self.common.aging.first(),
            )?;
            if self.optical.as_ref().is_some_and(|o| o.pending.is_some())
                || self.common.options.dielectrics.iter().any(|input| input.body_index == shared.body_index)
            {
                return Err(invalid("optical primary cannot reuse shading"));
            }
            if self.common.options.reflections.as_ref().is_some_and(|r| r.bodies.iter().any(|entry| entry.body == shared.body_name)) {
                return Err(invalid("selected primary cannot reuse shading"));
            }
            self.common.primary_coverage =
                self.common.primary_coverage.checked_add(1).filter(|n| *n <= 1024).ok_or_else(|| invalid("spatial coverage overflow"))?;
            return self.finish_spatial_sample(radiance);
        }
        if self.common.options.primary_samples != 1 && self.spatial_transition()? {
            return Ok(());
        }
        let bvh = self.common.bvh.as_ref().ok_or_else(|| invalid("missing image BVH"))?;
        if self.pending.is_none() {
            if self.common.options.lens.is_some_and(|lens| lens.aperture_radius_mm > 0.0) && self.common.pupil_sample.is_none() {
                self.common.work.tick(self.common.cancel).map_err(error)?;
                let progressive = self.common.progressive.ok_or_else(|| invalid("physical pupil requires progressive camera sampling"))?;
                self.common.pupil_sample = Some(progressive.variates(self.common.pixel, 0x4c454e53)?);
                return Ok(());
            }
            self.common.work.tick(self.common.cancel).map_err(error)?;
            let width = self.common.request.width as usize;
            // This proof belongs to the generator for a fixed (pixel, spatial),
            // not the camera/request globally. Future per-R jitter must revoke it.
            let (ray, repeatable_primary) = if let Some(progressive) = &self.common.progressive {
                let [dx, dy] = progressive.variates(self.common.pixel, 0)?;
                let x = (self.common.pixel % width) as f64 + dx;
                let y = (self.common.pixel / width) as f64 + dy;
                let ray = if let Some(lens) = self.common.options.lens.filter(|lens| lens.aperture_radius_mm > 0.0) {
                    let pupil = self.common.pupil_sample.take().ok_or_else(|| invalid("missing physical pupil sample"))?;
                    lens_primary_ray(self.common.request, x, y, self.common.clip, lens, pupil).map_err(error)?
                } else {
                    primary_ray(self.common.request, x, y, self.common.clip).map_err(error)?
                };
                (ray, false)
            } else if self.common.options.primary_samples == 1 {
                // Preserve the exact legacy centre path, including its reuse proof.
                (
                    primary_ray(
                        self.common.request,
                        (self.common.pixel % width) as f64 + 0.5,
                        (self.common.pixel / width) as f64 + 0.5,
                        self.common.clip,
                    )
                    .map_err(error)?,
                    true,
                )
            } else {
                let [dx, dy] = primary_offset(self.common.options.primary_samples, self.common.spatial)?;
                (
                    primary_ray(
                        self.common.request,
                        (self.common.pixel % width) as f64 + dx,
                        (self.common.pixel / width) as f64 + dy,
                        self.common.clip,
                    )
                    .map_err(error)?,
                    true,
                )
            };
            if let Some(optical) = self.optical.as_mut() {
                if optical.pending.is_some() {
                    return Err(invalid("previous optical sample was not drained"));
                }
                *optical.pending = Some(DielectricPixel::new(ray));
            }
            self.pending.insert(PendingPixel {
                surface_cursor: 0,
                surface_index: None,
                surface_ready: false,
                emission_cursor: 0,
                punctual_index: 0,
                punctual_shadow: false,
                punctual_ready: false,
                punctual_value: Vec3::ZERO,
                area_light: 0,
                area_index: 0,
                area_shadow: false,
                area_ready: false,
                area_value: Vec3::ZERO,

                repeatable_primary,
                environment_sample: 0,
                environment_sum: Vec3::ZERO,
                environment_cursor: None,
                environment_value: Vec3::ZERO,
                ray,
                cursor: TraceCursor::new(ray, false),
                hit: None,
                visibility: [true; 8],
                light: 0,
                bounce: 0,
                throughput: Vec3::new(1.0, 1.0, 1.0),
                radiance: Vec3::ZERO,
            })?;
            return Ok(());
        }
        if let Some(optical) = self.optical.as_mut() {
            let state = optical.pending.as_mut().ok_or_else(|| invalid("missing optical primary state"))?;
            if state.primary_check < self.common.options.dielectrics.len() {
                self.common.work.tick(self.common.cancel).map_err(error)?;
                if let Some(Certificate::ClosedBox(bounds)) = optical.certificates.get(state.primary_check).copied().flatten()
                    && (dielectric::contains(bounds, state.path.ray.origin()) || dielectric::contains(bounds, self.common.request.camera.eye()))
                {
                    return Err(invalid("camera or near plane inside a dielectric volume is unsupported"));
                }
                state.primary_check += 1;
                return Ok(());
            }
            if state.primary_check != self.common.options.dielectrics.len() {
                return Err(invalid("invalid primary certificate cursor"));
            }
        }
        let scene = PreparedScene { bvh, primitives: self.common.looks, source: self.common.scene };
        let pending = self.pending.as_mut().ok_or_else(|| invalid("missing pixel state"))?;
        let primary_segment =
            pending.bounce == 0 && self.optical.as_ref().and_then(|o| o.pending.as_ref()).is_none_or(|state| state.path.boundaries == 0);
        if pending.hit.is_none() {
            let Some(hit) = pending.cursor.step(bvh, self.common.work, self.common.cancel).map_err(error)? else {
                return Ok(());
            };
            let Some(hit) = hit else {
                if self.optical.as_ref().and_then(|o| o.pending.as_ref()).is_some_and(|state| state.path.medium.is_some()) {
                    return Err(invalid("dielectric path escaped without its certified exit"));
                }
                if primary_segment {
                    if let Some(optical) = self.optical.as_mut() {
                        *optical.pending = None;
                    }
                    if self.common.options.primary_samples != 1 || self.common.progressive.is_some() {
                        // A miss is one of S*R attempts, never the completed pixel.
                        return self.finish_sample(Vec3::ZERO);
                    }
                    self.append_linear(Vec3::ZERO, 0, 1)?;
                    if self.preserves_display() {
                        let rgba = if self.common.request.transparent_background {
                            [0; 4]
                        } else {
                            let [r, g, b] = self.common.request.background;
                            [r, g, b, 255]
                        };
                        self.store_rgba(rgba)?;
                    }
                    return Ok(());
                }
                if let Some(environment) = &self.common.environment {
                    let emitted =
                        environment.view().map_err(environment_error)?.radiance_linear_srgb(pending.ray.direction()).map_err(environment_error)?;
                    pending.radiance += multiply(pending.throughput, emitted);
                }
                if self.common.environment.is_none()
                    && let Some(environment) = self.common.options.environment
                {
                    let [r, g, b] = environment.color;
                    pending.radiance += multiply(pending.throughput, Vec3::new(r, g, b) * environment.intensity);
                }
                let radiance = pending.radiance;
                if !bounded_radiance(radiance) {
                    return Err(invalid("secondary environment radiance exceeds finite bound"));
                }
                return self.finish_transport_path(radiance);
            };
            if primary_segment && (self.common.options.primary_samples != 1 || self.common.progressive.is_some()) {
                self.common.primary_coverage =
                    self.common.primary_coverage.checked_add(1).filter(|n| *n <= 1024).ok_or_else(|| invalid("spatial coverage overflow"))?;
            }
            pending.hit = Some(hit);
            if let Some(maps) = self.common.maps.first_mut() {
                maps.reset_hit();
            }
            if let Some(aging) = self.common.aging.first_mut() {
                aging.reset();
            }
            pending.light = 0;
            let shared = scene.resolve_hit(pending.ray, hit).map_err(error)?;
            let mode = self
                .common
                .options
                .reflections
                .as_ref()
                .and_then(|r| r.bodies.iter().find(|entry| entry.body == shared.body_name).map(|entry| entry.mode));
            pending.repeatable_primary &= primary_segment && mode.is_none();
            if let Some(optical) = self.optical.as_mut() {
                let state = optical.pending.as_mut().ok_or_else(|| invalid("missing optical hit state"))?;
                state.selector = 0;
                state.selected = None;
                state.ready = false;
                // Traversal consumed this continuation; selector work begins next call.
                return Ok(());
            }
            pending.surface_ready = self.common.effects.is_empty() && self.common.maps.is_empty() && self.common.aging.is_empty();
            if !pending.surface_ready {
                // Traversal already consumed this continuation's unit. Scan one
                // effect per following call, before beginning any lighting stage.
                return Ok(());
            }
            if self.common.options.shadows && !self.common.lights.lights().is_empty() && mode != Some(job::ReflectionMode::Mirror) {
                let light = self.common.lights.lights().first().ok_or_else(|| invalid("missing first light"))?;
                pending.cursor = TraceCursor::new(shared.spawn(light.direction()).map_err(error)?, true);
                return Ok(());
            }
            pending.light = self.common.lights.lights().len();
            return Ok(());
        } else if self.optical.as_ref().and_then(|o| o.pending.as_ref()).is_some_and(|state| !state.ready) {
            return self.dielectric_hit_one();
        } else if !pending.surface_ready {
            self.common.work.tick(self.common.cancel).map_err(error)?;
            if let Some((body_index, _)) = self.common.effects.get(pending.surface_cursor) {
                let hit = pending.hit.ok_or_else(|| invalid("missing surface effect hit"))?;
                let look = self.common.looks.get(hit.primitive_index).ok_or_else(|| invalid("missing surface effect appearance"))?;
                let index = pending.surface_cursor;
                pending.surface_cursor += 1;
                if *body_index == look.body_index {
                    pending.surface_index = Some(index);
                    pending.surface_cursor = self.common.effects.len();
                }
                return Ok(());
            }
            if pending.surface_cursor != self.common.effects.len() {
                return Err(invalid("invalid surface effect scan state"));
            }
            if let Some(maps) = self.common.maps.first_mut() {
                if !maps.hit_started() {
                    let shared = unmapped_surface_hit(&scene, pending, self.common.effects)?;
                    let has_face_override = self.common.looks.get(shared.primitive_index).is_none_or(|look| look.has_face_override);
                    let footprint = primary_footprint(
                        self.common.request,
                        self.common.options,
                        self.common.progressive,
                        self.common.pixel,
                        self.common.spatial,
                        self.common.clip,
                        shared.position,
                        shared.oriented_geometric_normal(),
                    )?;
                    maps.begin_hit(
                        crate::surface_patterns::ShadingSurface { surface: shared.surface, shading_normal: shared.shading_normal },
                        (!has_face_override).then_some(shared.body_index),
                        job::textures::cpu_preparation::MapPoint {
                            position: shared.position,
                            geometric_normal: shared.oriented_geometric_normal(),
                            view_direction: -pending.ray.direction(),
                        },
                        footprint,
                    )?;
                }
                if !maps.hit_ready() {
                    let ready = maps.advance_hit()?;
                    if !ready || !self.common.aging.is_empty() {
                        return Ok(());
                    }
                }
            }
            if let Some(aging) = self.common.aging.first_mut() {
                if !aging.started() {
                    let shared = unaged_surface_hit(&scene, pending, self.common.effects, self.common.maps.first())?;
                    aging.begin(
                        crate::surface_patterns::ShadingSurface { surface: shared.surface, shading_normal: shared.shading_normal },
                        shared.position,
                        shared.geometric_normal,
                        shared.body_index,
                        super::super::aging::eligible(self.common.options, shared.body_index, shared.body_name),
                    )?;
                }
                if !aging.ready() {
                    let stack = self.common.options.aging.as_deref().ok_or_else(|| invalid("CPU aging stack disappeared"))?;
                    if !aging.advance(stack)? {
                        return Ok(());
                    }
                }
            }
            pending.surface_ready = true;
            let shared = resolved_surface_hit(&scene, pending, self.common.effects, self.common.maps.first(), self.common.aging.first())?;
            let mode = self
                .common
                .options
                .reflections
                .as_ref()
                .and_then(|r| r.bodies.iter().find(|entry| entry.body == shared.body_name).map(|entry| entry.mode));
            if self.common.options.shadows && !self.common.lights.lights().is_empty() && mode != Some(job::ReflectionMode::Mirror) {
                let light = self.common.lights.lights().first().ok_or_else(|| invalid("missing first surface light"))?;
                pending.cursor = TraceCursor::new(shared.spawn(light.direction()).map_err(error)?, true);
            } else {
                pending.light = self.common.lights.lights().len();
            }
            return Ok(());
        } else if pending.light < self.common.lights.lights().len() {
            let Some(shadow) = pending.cursor.step(bvh, self.common.work, self.common.cancel).map_err(error)? else {
                return Ok(());
            };
            *pending.visibility.get_mut(pending.light).ok_or_else(|| invalid("light visibility index"))? = shadow.is_none();
            pending.light += 1;
            if let Some(light) = self.common.lights.lights().get(pending.light) {
                let shared = resolved_surface_hit(&scene, pending, self.common.effects, self.common.maps.first(), self.common.aging.first())?;
                pending.cursor = TraceCursor::new(shared.spawn(light.direction()).map_err(error)?, true);
                return Ok(());
            }
            return Ok(());
        }
        if let Some((body_index, emission)) = self.common.emissions.get(pending.emission_cursor) {
            self.common.work.tick(self.common.cancel).map_err(error)?;
            let hit = pending.hit.ok_or_else(|| invalid("missing emission hit"))?;
            let look = self.common.looks.get(hit.primitive_index).ok_or_else(|| invalid("missing emission appearance"))?;
            pending.emission_cursor += 1;
            if *body_index == look.body_index {
                let emitted = emission.radiance(hit.geometry.geometric_normal.dot(-pending.ray.direction()) > 0.0);
                pending.radiance += multiply(pending.throughput, emitted);
                if !bounded_radiance(pending.radiance) {
                    return Err(invalid("emission path radiance exceeds finite bound"));
                }
                pending.emission_cursor = self.common.emissions.len();
            }
            return Ok(());
        }
        if pending.punctual_index < self.common.options.punctual_lights.as_ref().map_or(0, Vec::len) {
            let shared = resolved_surface_hit(&scene, pending, self.common.effects, self.common.maps.first(), self.common.aging.first())?;
            let mode = self
                .common
                .options
                .reflections
                .as_ref()
                .and_then(|r| r.bodies.iter().find(|entry| entry.body == shared.body_name).map(|entry| entry.mode));
            let face = self.common.looks.get(shared.primitive_index).ok_or_else(|| invalid("punctual face missing"))?.face;
            let target = anisotropic_faces::target_index(self.common.options, shared.body_index, face)
                .map(|i| self.anisotropic_targets.get(i).copied().flatten().ok_or_else(|| invalid("punctual anisotropic target missing")))
                .transpose()?;
            punctual_one(
                pending,
                self.common.options,
                bvh,
                (shared, target, mode),
                self.common.work,
                self.common.cancel,
                self.common.punctual_diagnostics,
            )?;
            return Ok(());
        }
        if pending.area_light < self.common.options.area_lights.as_ref().map_or(0, Vec::len) {
            let shared = resolved_surface_hit(&scene, pending, self.common.effects, self.common.maps.first(), self.common.aging.first())?;
            let mode = self
                .common
                .options
                .reflections
                .as_ref()
                .and_then(|r| r.bodies.iter().find(|entry| entry.body == shared.body_name).map(|entry| entry.mode));
            let face = self.common.looks.get(shared.primitive_index).ok_or_else(|| invalid("area face missing"))?.face;
            let target = anisotropic_faces::target_index(self.common.options, shared.body_index, face)
                .map(|i| self.anisotropic_targets.get(i).copied().flatten().ok_or_else(|| invalid("area anisotropic target missing")))
                .transpose()?;
            let ordinal = self.common.progressive.map(|p| p.identity(self.common.pixel).map(|i| i.ordinal)).transpose()?.unwrap_or(0);
            let key = area_sample_key(self.common.pixel as u64, self.common.spatial, self.common.options.primary_samples, ordinal)?;
            let key = self.common.progressive.map_or(Ok(key), |p| p.area_key(self.common.pixel, key))?;
            area_one(pending, self.common.options, bvh, (shared, target, mode), self.common.work, self.common.cancel, key)?;
            return Ok(());
        }
        if let Some(cursor) = &mut pending.environment_cursor {
            let Some(blocker) = cursor.step(bvh, self.common.work, self.common.cancel).map_err(error)? else {
                return Ok(());
            };
            if blocker.is_none() {
                pending.environment_sum += pending.environment_value;
            }
            pending.environment_cursor = None;
            pending.environment_sample += 1;
            return Ok(());
        }
        if pending.light == self.common.lights.lights().len() {
            self.common.work.tick(self.common.cancel).map_err(error)?;
            let shared = resolved_surface_hit(&scene, pending, self.common.effects, self.common.maps.first(), self.common.aging.first())?;
            let mode = self
                .common
                .options
                .reflections
                .as_ref()
                .and_then(|r| r.bodies.iter().find(|entry| entry.body == shared.body_name).map(|entry| entry.mode));
            let view = -pending.ray.direction();
            let geometric = shared.oriented_geometric_normal();
            // Disjoint estimators: selected reflectors use BSDF transport to the environment;
            // only nonselected opaque hits use environment next-event estimates.
            if mode.is_none()
                && let Some(environment) = &self.common.environment
            {
                if pending.environment_sample < environment.samples {
                    let side = match environment.samples {
                        1 => 1,
                        4 => 2,
                        16 => 4,
                        _ => 8,
                    };
                    let i = pending.environment_sample;
                    let u = if let Some(progressive) = &self.common.progressive {
                        let [dx, dy] = progressive.variates(self.common.pixel, 0x1000 + u64::from(pending.bounce) * 128 + u64::from(i))?;
                        [(f64::from(i / side) + dx) / f64::from(side), (f64::from(i % side) + dy) / f64::from(side)]
                    } else if side == 1 {
                        [0.25, 0.375]
                    } else {
                        [(f64::from(i / side) + 0.5) / f64::from(side), (f64::from(i % side) + 0.5) / f64::from(side)]
                    };
                    let sample = environment
                        .view()
                        .map_err(environment_error)?
                        .direct_sample_linear_srgb(u, shared.surface, shared.shading_normal, geometric, view)
                        .map_err(environment_error)?;
                    pending.environment_value = sample.contribution_linear_srgb;
                    if self.common.options.shadows && sample.contribution_linear_srgb.len2() > 0.0 {
                        pending.environment_cursor = Some(TraceCursor::new(shared.spawn(sample.direction).map_err(error)?, true));
                    } else {
                        pending.environment_sum += pending.environment_value;
                        pending.environment_sample += 1;
                    }
                    return Ok(());
                }
                pending.radiance += multiply(pending.throughput, pending.environment_sum / f64::from(environment.samples));
            }
            let face = self.common.looks.get(shared.primitive_index).ok_or_else(|| invalid("missing captured face"))?.face;
            let target_index = anisotropic_faces::target_index(self.common.options, shared.body_index, face);
            if mode != Some(job::ReflectionMode::Mirror) {
                let visible = |index: usize, light: Vec3| {
                    Ok(geometric.dot(light) > 0.0 && *pending.visibility.get(index).ok_or(TraceError::Internal("light visibility"))?)
                };
                let direct = if let Some(index) = target_index {
                    self.anisotropic_targets
                        .get(index)
                        .copied()
                        .flatten()
                        .ok_or_else(|| invalid("missing certified anisotropic face"))?
                        .shade(self.common.lights, shared.surface, geometric, view, visible)
                        .map_err(error)?
                } else {
                    self.common.lights.shade_linear(shared.surface, shared.shading_normal, view, visible).map_err(error)?
                };
                pending.radiance += multiply(pending.throughput, direct);
            }
            let max_bounces = self.common.options.reflections.as_ref().map_or(0, |r| r.max_bounces);
            if let Some(mode) = mode.filter(|_| pending.bounce < max_bounces) {
                let samples = self.common.options.reflections.as_ref().map_or(1, |r| u64::from(r.samples));
                let u = if let Some(progressive) = &self.common.progressive {
                    progressive.variates(self.common.pixel, 0x100 + u64::from(pending.bounce))?
                } else if self.common.options.primary_samples == 1 {
                    reflection::variates(self.common.pixel as u64, self.sample as u64, pending.bounce as u64, samples).map_err(error)?
                } else {
                    let key = spatial_key(self.common.pixel as u64, self.common.spatial, self.common.options.primary_samples)?;
                    reflection::variates(key, self.sample as u64, pending.bounce as u64, samples).map_err(error)?
                };
                let sampled = match (mode, target_index) {
                    (job::ReflectionMode::Mirror, _) => reflection::mirror(shared.surface, shared.shading_normal, view),
                    (job::ReflectionMode::RoughSpecular, Some(index)) => self
                        .anisotropic_targets
                        .get(index)
                        .copied()
                        .flatten()
                        .ok_or_else(|| invalid("missing anisotropic reflection target"))?
                        .sample(shared.surface, geometric, view, u),
                    (job::ReflectionMode::RoughSpecular, None) => reflection::rough_specular(shared.surface, shared.shading_normal, view, u),
                }
                .map_err(error)?;
                if let Some(sample) = sampled.filter(|s| s.direction.dot(geometric) > 0.0) {
                    pending.throughput = multiply(pending.throughput, sample.weight);
                    if !pending.throughput.is_finite() {
                        return Err(invalid("nonfinite reflection throughput"));
                    }
                    if pending.throughput.len2() > 0.0 {
                        pending.ray = shared.spawn(sample.direction).map_err(error)?;
                        pending.cursor = TraceCursor::new(pending.ray, false);
                        pending.hit = None;
                        pending.surface_cursor = 0;
                        pending.surface_index = None;
                        pending.surface_ready = false;
                        pending.emission_cursor = 0;
                        pending.light = 0;
                        pending.visibility = [true; 8];
                        pending.punctual_index = 0;
                        pending.punctual_shadow = false;
                        pending.punctual_ready = false;
                        pending.punctual_value = Vec3::ZERO;
                        pending.area_light = 0;
                        pending.area_index = 0;
                        pending.area_shadow = false;
                        pending.area_ready = false;
                        pending.area_value = Vec3::ZERO;

                        pending.environment_sample = 0;
                        pending.environment_sum = Vec3::ZERO;
                        pending.environment_cursor = None;
                        pending.environment_value = Vec3::ZERO;
                        pending.bounce += 1;
                        if let Some(optical) = self.optical.as_mut() {
                            let state = optical.pending.as_mut().ok_or_else(|| invalid("missing reflected optical path"))?;
                            state.path.ray = pending.ray;
                            state.path.throughput = pending.throughput;
                            state.path.segment_start = shared.position;
                            state.path.bounce = pending.bounce;
                            state.selector = 0;
                            state.selected = None;
                            state.ready = false;
                        }
                        return Ok(());
                    }
                }
            }
            let radiance = pending.radiance;
            return self.finish_transport_path(radiance);
        }
        Ok(())
    }
    /// Complete a transport branch. Only a drained optical stack completes one R sample.
    fn finish_transport_path(&mut self, radiance: Vec3) -> job::Result<()> {
        if let Some(optical) = self.optical.as_mut() {
            let state = optical.pending.as_mut().ok_or_else(|| invalid("missing optical sample"))?;
            if state.advance()? {
                let pending = self.pending.as_mut().ok_or_else(|| invalid("missing optical continuation"))?;
                resume_optical_path(pending, state.path);
                return Ok(());
            }
            *optical.pending = None;
        }
        self.finish_sample(radiance)
    }

    /// Each selector and each interface is a separate charged continuation. Traversal has
    /// already yielded with the captured hit, and opaque hits rejoin the shared light stages.
    fn dielectric_hit_one(&mut self) -> job::Result<()> {
        self.common.work.tick(self.common.cancel).map_err(error)?;
        let optical = self.optical.as_mut().ok_or_else(|| invalid("missing optical task"))?;
        let state = optical.pending.as_mut().ok_or_else(|| invalid("missing optical sample"))?;
        let pending = self.pending.as_mut().ok_or_else(|| invalid("missing optical hit"))?;
        if state.ready {
            return Err(invalid("optical hit already consumed"));
        }
        let hit = pending.hit.ok_or_else(|| invalid("missing captured optical hit"))?;
        let look = self.common.looks.get(hit.primitive_index).ok_or_else(|| invalid("missing optical appearance"))?;
        if let Some(input) = self.common.options.dielectrics.get(state.selector) {
            let index = state.selector;
            state.selector += 1;
            if input.body_index == look.body_index {
                state.selected = Some(index);
                state.selector = self.common.options.dielectrics.len();
            }
            return Ok(());
        }
        if state.selector != self.common.options.dielectrics.len() {
            return Err(invalid("invalid optical selector state"));
        }
        let bvh = self.common.bvh.as_ref().ok_or_else(|| invalid("missing optical BVH"))?;
        let scene = PreparedScene { bvh, primitives: self.common.looks, source: self.common.scene };
        // Optics always uses the certified geometric boundary, before optional opaque
        // shading-normal effects. Distance starts before the numerical spawn offset.
        let shared = scene.resolve_hit(pending.ray, hit).map_err(error)?;
        if let Some(index) = state.path.medium {
            let medium = self.common.options.dielectrics.get(index).ok_or_else(|| invalid("active dielectric medium"))?;
            let distance = shared.position.dist(state.path.segment_start);
            pending.throughput = multiply(pending.throughput, dielectric::attenuation(medium.absorption_per_mm, distance).map_err(error)?);
            if !bounded_radiance(pending.throughput) {
                return Err(invalid("dielectric throughput exceeds finite bound"));
            }
        }
        state.ready = true;
        let Some(index) = state.selected else {
            pending.surface_ready = self.common.effects.is_empty() && self.common.maps.is_empty() && self.common.aging.is_empty();
            if !pending.surface_ready {
                return Ok(());
            }
            let mode = self
                .common
                .options
                .reflections
                .as_ref()
                .and_then(|r| r.bodies.iter().find(|entry| entry.body == shared.body_name).map(|entry| entry.mode));
            if self.common.options.shadows && !self.common.lights.lights().is_empty() && mode != Some(job::ReflectionMode::Mirror) {
                let light = self.common.lights.lights().first().ok_or_else(|| invalid("missing optical opaque light"))?;
                pending.cursor = TraceCursor::new(shared.spawn(light.direction()).map_err(error)?, true);
            } else {
                pending.light = self.common.lights.lights().len();
            }
            return Ok(());
        };
        let medium = self.common.options.dielectrics.get(index).ok_or_else(|| invalid("selected dielectric medium"))?;
        if medium.body_index != shared.body_index || medium.body_name != shared.body_name {
            return Err(invalid("optical captured identity changed"));
        }
        pending.repeatable_primary = false;
        state.path.ray = pending.ray;
        state.path.throughput = pending.throughput;
        state.path.bounce = pending.bounce;
        if state.path.boundaries == MAX_BOUNDARIES {
            // Explicit finite-depth preview: unresolved continuation is black, while
            // already accumulated terms and the other bounded branches remain intact.
            let radiance = pending.radiance;
            return self.finish_transport_path(radiance);
        }
        if state.path.boundaries > MAX_BOUNDARIES {
            return Err(invalid("invalid dielectric boundary depth"));
        }
        let next_depth = state.path.boundaries + 1;
        let normal = shared.oriented_geometric_normal();
        let direction = state.path.ray.direction();
        let certificate = optical.certificates.get(index).copied().flatten().ok_or_else(|| invalid("missing dielectric certificate"))?;
        let (optics, reflected_weight, transmitted_weight, next_medium) = match certificate {
            Certificate::ThinSheet => {
                if state.path.medium.is_some() {
                    return Err(invalid("thin sheet inside dielectric medium"));
                }
                let (optics, reflected, transmitted) = dielectric::thin_sheet(direction, normal, medium).map_err(error)?;
                (optics, reflected, transmitted, None)
            }
            Certificate::ClosedBox(bounds) => {
                let boundary_axes = (0..3)
                    .filter(|axis| {
                        let p = super::super::component(shared.position, *axis);
                        p == super::super::component(bounds.min(), *axis) || p == super::super::component(bounds.max(), *axis)
                    })
                    .count();
                if boundary_axes > 1 {
                    return Err(invalid("dielectric box edge or corner has an ambiguous interface normal"));
                }
                let (incident, transmitted, next_medium) = if shared.front_face {
                    if state.path.medium.is_some() {
                        return Err(invalid("nested dielectric entry"));
                    }
                    (1.0, medium.ior, Some(index))
                } else {
                    if state.path.medium != Some(index) {
                        return Err(invalid("dielectric exit without matching entry"));
                    }
                    (medium.ior, 1.0, None)
                };
                let optics = dielectric::interface(direction, normal, incident, transmitted).map_err(error)?;
                let f = optics.reflectance;
                let t = (1.0 - f) * (incident / transmitted).powi(2);
                (optics, Vec3::new(f, f, f), Vec3::new(t, t, t), next_medium)
            }
        };
        state.push(OpticalPath {
            ray: shared.spawn(optics.reflected).map_err(error)?,
            throughput: dielectric::multiply(state.path.throughput, reflected_weight),
            medium: state.path.medium,
            segment_start: shared.position,
            boundaries: next_depth,
            bounce: pending.bounce,
        })?;
        if let Some(direction) = optics.transmitted {
            state.push(OpticalPath {
                ray: shared.spawn(direction).map_err(error)?,
                throughput: dielectric::multiply(state.path.throughput, transmitted_weight),
                medium: next_medium,
                segment_start: shared.position,
                boundaries: next_depth,
                bounce: pending.bounce,
            })?;
        }
        let radiance = pending.radiance;
        self.finish_transport_path(radiance)
    }
}

pub(super) fn publish(task: &mut ImageTask, pixel: Pixel) -> job::Result<()> {
    if let Some(progressive) = &mut task.progressive {
        if pixel.attempts != 1 || pixel.covered > 1 || pixel.rgba.is_some() {
            return Err(invalid("CPU direct progressive sample has invalid output metadata"));
        }
        let ordinal = progressive.film.samples();
        progressive.film.add(
            pixel.pixel,
            ordinal,
            [pixel.radiance.x, pixel.radiance.y, pixel.radiance.z],
            pixel.covered == 1,
            &task.cancel,
            &mut || task.work.tick(&task.cancel).map_err(error),
        )?;
    } else {
        task.append_linear(pixel.radiance, pixel.covered, pixel.attempts)?;
        if task.preserves_display() != pixel.rgba.is_some() {
            return Err(invalid("CPU direct display differs from the admitted output"));
        }
        if let Some(rgba) = pixel.rgba {
            task.pixels.extend_from_slice(&rgba);
        }
    }
    Ok(())
}

pub(super) fn step(task: &mut ImageTask) -> job::Result<()> {
    advance(task, None)
}

/// Retain the owner's private sample API while sharing the exact direct
/// per-add radiance bounds and progressive publication path.
pub(super) fn finish_sample(task: &mut ImageTask, radiance: Vec3) -> job::Result<()> {
    advance(task, Some(radiance))
}

fn advance(task: &mut ImageTask, radiance: Option<Vec3>) -> job::Result<()> {
    let (outcome, output, pixel, sample, spatial, coverage, pupil, spatial_radiance, finish_spatial, emit_spatial_pixel, pixel_radiance) = {
        let mut context = Context::image(task);
        let outcome = match radiance {
            Some(radiance) => context.finish_sample(radiance),
            None => context.pixel_one(),
        };
        (
            outcome,
            context.pixel_output,
            context.common.pixel,
            context.sample,
            context.common.spatial,
            context.common.primary_coverage,
            context.common.pupil_sample,
            context.spatial_radiance,
            context.finish_spatial,
            context.emit_spatial_pixel,
            context.pixel_radiance,
        )
    };
    task.pixel = pixel;
    task.sample = sample;
    task.spatial = spatial;
    task.primary_coverage = coverage;
    task.pupil_sample = pupil;
    task.spatial_radiance = spatial_radiance;
    task.finish_spatial = finish_spatial;
    task.emit_spatial_pixel = emit_spatial_pixel;
    task.pixel_radiance = pixel_radiance;
    outcome?;
    if let Some(output) = output {
        // No pixel is complete until the owner has successfully charged and
        // stored its film/linear output. On error the pre-publication cursors,
        // coverage and progressive PendingPixel remain available for diagnosis.
        publish(task, output)?;
        task.pixel += 1;
        if task.progressive.is_some() {
            task.pending.clear();
            task.primary_coverage = 0;
        } else if task.options.primary_samples != 1 {
            task.spatial = 0;
            task.primary_coverage = 0;
            task.spatial_radiance = Vec3::ZERO;
            task.emit_spatial_pixel = false;
        } else {
            task.sample = 0;
            task.pixel_radiance = Vec3::ZERO;
            task.pending.clear();
        }
    }
    Ok(())
}
