//! Deterministic first-hit CPU guides, generated once without touching the sampling stream.
use super::bvh::TraceCursor;
use super::image::{error, invalid, primary_ray};
use super::prepared::{PreparedScene, SharedHit};
use super::{Hit, Ray, WorkBudget, dielectric, glass_mesh, reflection};
use crate::job::{
    CancelToken, DenoiseGuideClass as GuideClass, DielectricBoundary, DielectricOptions, ImageRequest, RaytraceOptions, ReflectionMode, Result,
    denoise::{Guide, SurfaceGuide},
};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

#[derive(Clone, Copy)]
pub(super) enum GlassCertification<'a> {
    Path(&'a glass_mesh::Task),
    Legacy(&'a [Option<dielectric::Certificate>]),
}
#[derive(Clone, Copy)]
enum CertifiedBoundary {
    Path(glass_mesh::Region),
    Legacy(dielectric::Certificate),
}
impl GlassCertification<'_> {
    fn boundary(self, index: usize) -> Option<CertifiedBoundary> {
        match self {
            Self::Path(task) => task.region(index).ok().map(CertifiedBoundary::Path),
            Self::Legacy(certificates) => certificates.get(index).copied().flatten().map(CertifiedBoundary::Legacy),
        }
    }
    fn outside(self, options: &RaytraceOptions, request: &ImageRequest, ray: Ray) -> bool {
        let eye = request.camera.eye();
        match self {
            Self::Path(task) => {
                let origin = if request.camera.fov > 0.0 { eye } else { ray.origin() };
                super::origin::Task::required(task, &options.dielectrics, origin, ray.origin()).is_ok_and(|required| !required)
            }
            Self::Legacy(certificates) => certificates.iter().all(|certificate| {
                !matches!(certificate, Some(dielectric::Certificate::ClosedBox(bounds))
                    if dielectric::contains(*bounds, eye) || dielectric::contains(*bounds, ray.origin()))
            }),
        }
    }
}
impl CertifiedBoundary {
    fn launch(self, hit: SharedHit<'_>, direction: Vec3) -> Option<Launch> {
        match self {
            Self::Path(region) => {
                let ray = Ray::new(hit.position, direction, 0.0_f64.next_up(), super::MAX_DISTANCE_MM).ok()?;
                Some(Launch { ray, plane: Some((region.primitive_begin, region.primitive_end, hit.vertices)) })
            }
            Self::Legacy(_) => Some(Launch::ordinary(hit.spawn(direction).ok()?)),
        }
    }
    fn unambiguous(self, hit: SharedHit<'_>) -> bool {
        match self {
            Self::Path(region) => region.parent.is_none(),
            Self::Legacy(dielectric::Certificate::ThinSheet) => true,
            Self::Legacy(dielectric::Certificate::ClosedBox(bounds)) => {
                (0..3)
                    .filter(|&axis| {
                        let value = super::component(hit.position, axis);
                        value == super::component(bounds.min(), axis) || value == super::component(bounds.max(), axis)
                    })
                    .count()
                    <= 1
            }
        }
    }
}
#[derive(Clone, Copy)]
struct Launch {
    ray: Ray,
    plane: Option<(usize, usize, [Vec3; 3])>,
}
impl Launch {
    fn ordinary(ray: Ray) -> Self {
        Self { ray, plane: None }
    }
}
#[derive(Clone, Copy)]
enum Segment {
    Reflected,
    GlassExit(usize),
    Transmitted,
}
#[derive(Clone, Copy)]
struct Forward {
    launch: Launch,
    segment: Segment,
}
#[derive(Clone, Copy)]
struct OpticalWalk {
    primary: u32,
    reflected: SurfaceGuide,
    footprint: f64,
    segment: Segment,
    forward: Option<Forward>,
}

fn body_id(hit: SharedHit<'_>) -> Result<u32> {
    u32::try_from(hit.body_index).ok().and_then(|body| body.checked_add(1)).ok_or_else(|| invalid("CPU denoising body identity overflow"))
}
fn guide_geometry(request: &ImageRequest, position: Vec3) -> (Vec3, f64, f64) {
    let camera = &request.camera;
    let relative = position - camera.eye();
    let depth = relative.dot(-camera.back());
    let width = 2.0 * camera.half_height() / f64::from(request.height);
    let footprint = if camera.fov == 0.0 { width } else { width * depth.max(0.0) / camera.distance };
    (relative, depth, footprint)
}
fn is_emitter(options: &RaytraceOptions, hit: SharedHit<'_>) -> bool {
    options.emissions.iter().flatten().any(|value| value.body_index == hit.body_index && value.strength > 0.0 && value.color.iter().any(|v| *v > 0.0))
}
fn lens_active(options: &RaytraceOptions) -> bool {
    options.lens.is_some_and(|lens| lens.aperture_radius_mm > 0.0)
}
fn glass_interface(options: &RaytraceOptions, ray: Ray, hit: SharedHit<'_>, ni: f64, nt: f64) -> Option<(dielectric::Interface, Vec3)> {
    let direction = ray.direction();
    let geometric = hit.oriented_geometric_normal();
    if options.path.is_none()
        || options.caustics.is_some()
        || options.dielectrics.iter().any(|input| input.model.is_some_and(|model| !model.is_identity()))
    {
        Some((dielectric::interface(direction, geometric, ni, nt).ok()?, geometric))
    } else {
        dielectric::shaded_interface(direction, geometric, hit.shading_normal, ni, nt).ok()
    }
}
fn glass_branches(
    options: &RaytraceOptions,
    index: usize,
    input: &DielectricOptions,
    boundary: CertifiedBoundary,
    ray: Ray,
    hit: SharedHit<'_>,
) -> std::result::Result<(Option<Launch>, Option<Forward>), GuideClass> {
    let thin = input.boundary == DielectricBoundary::ThinSheet;
    let needed = if thin { 2 } else { 3 };
    if lens_active(options) {
        return Err(GuideClass::DepthOfField);
    }
    if options.path.is_some_and(|path| path.max_depth < needed) {
        return Err(GuideClass::PathDepth);
    }
    if input.model.is_some_and(|model| !model.is_identity()) {
        return Err(GuideClass::UnsupportedMaterial);
    }
    if input.subsurface.is_some_and(|medium| medium.active()) {
        return Err(GuideClass::Volume);
    }
    if !boundary.unambiguous(hit) {
        return Err(match boundary {
            CertifiedBoundary::Path(_) => GuideClass::NestedMedium,
            CertifiedBoundary::Legacy(_) => GuideClass::DegeneratePath,
        });
    }
    if !thin && !hit.front_face {
        return Err(GuideClass::NestedMedium);
    }
    let (interface, normal) = glass_interface(options, ray, hit, 1.0, input.ior).ok_or(GuideClass::DegeneratePath)?;
    let (interface, reflects, transmits) = if thin {
        let (sheet, reflected, transmitted) = dielectric::thin_sheet(ray.direction(), normal, input).map_err(|_| GuideClass::DegeneratePath)?;
        (sheet, reflected != Vec3::ZERO, transmitted != Vec3::ZERO)
    } else {
        (interface, interface.reflectance > 0.0, interface.reflectance < 1.0)
    };
    let geometric = hit.oriented_geometric_normal();
    let reflected = if reflects {
        if interface.reflected.dot(geometric) <= 0.0 {
            return Err(GuideClass::DegeneratePath);
        }
        Some(boundary.launch(hit, interface.reflected).ok_or(GuideClass::DegeneratePath)?)
    } else {
        None
    };
    let forward = if transmits {
        let direction = interface.transmitted.ok_or(GuideClass::TotalInternalReflection)?;
        if direction.dot(geometric) >= 0.0 {
            return Err(GuideClass::DegeneratePath);
        }
        let segment = if thin { Segment::Transmitted } else { Segment::GlassExit(index) };
        Some(Forward { launch: boundary.launch(hit, direction).ok_or(GuideClass::DegeneratePath)?, segment })
    } else {
        None
    };
    Ok((reflected, forward))
}
fn glass_exit(
    options: &RaytraceOptions,
    index: usize,
    certification: Option<GlassCertification<'_>>,
    ray: Ray,
    hit: SharedHit<'_>,
) -> std::result::Result<Launch, GuideClass> {
    let input = options.dielectrics.get(index).ok_or(GuideClass::Unavailable)?;
    let boundary = certification.and_then(|value| value.boundary(index)).ok_or(GuideClass::Unavailable)?;
    if hit.body_index != input.body_index || hit.front_face || !boundary.unambiguous(hit) {
        return Err(GuideClass::IncompatibleExit);
    }
    let (interface, _) = glass_interface(options, ray, hit, input.ior, 1.0).ok_or(GuideClass::DegeneratePath)?;
    let direction = interface.transmitted.ok_or(GuideClass::TotalInternalReflection)?;
    if direction.dot(hit.oriented_geometric_normal()) >= 0.0
        || dielectric::attenuation(input.absorption_per_mm, ray.origin().dist(hit.position)).map_err(|_| GuideClass::DegeneratePath)? == Vec3::ZERO
    {
        return Err(GuideClass::DegeneratePath);
    }
    boundary.launch(hit, direction).ok_or(GuideClass::DegeneratePath)
}

pub(super) struct Guides {
    pixels: Vec<Guide>,
    classes: Option<Vec<GuideClass>>,
    count: usize,
    cursor: Option<TraceCursor>,
    ray: Option<Ray>,
    hit: Option<Hit>,
    emitter_cursor: usize,
    analytic_emitter: bool,
    classified: bool,
    effect_cursor: usize,
    effect: Option<usize>,
    pending: Option<(Guide, GuideClass)>,
    optical: Option<OpticalWalk>,
}
impl Guides {
    pub(super) fn required_bytes(width: u32, height: u32) -> Result<u64> {
        Self::required_bytes_with_diagnostics(width, height, false)
    }
    pub(super) fn required_bytes_with_diagnostics(width: u32, height: u32, diagnostics: bool) -> Result<u64> {
        let class_bytes = if diagnostics { std::mem::size_of::<GuideClass>() as u64 } else { 0 };
        (u64::from(width) * u64::from(height))
            .checked_mul(crate::job::denoise::GUIDE_BYTES + class_bytes)
            .and_then(|v| v.checked_add(std::mem::size_of::<Self>() as u64))
            .ok_or_else(|| invalid("CPU denoising guide size overflow"))
    }
    pub(super) fn new(width: u32, height: u32) -> Result<Self> {
        Self::new_with_diagnostics(width, height, false)
    }
    pub(super) fn new_with_diagnostics(width: u32, height: u32, diagnostics: bool) -> Result<Self> {
        let count = usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| invalid("CPU guide dimensions overflow"))?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(count).map_err(|_| error(super::TraceError::AllocationFailed))?;
        if count == 0 || pixels.capacity() != count {
            return Err(invalid("CPU guide capacity exceeds admission"));
        }
        let classes = if diagnostics {
            let mut classes = Vec::new();
            classes.try_reserve_exact(count).map_err(|_| error(super::TraceError::AllocationFailed))?;
            if classes.capacity() != count {
                return Err(invalid("CPU guide diagnostic capacity exceeds admission"));
            }
            Some(classes)
        } else {
            None
        };
        Ok(Self {
            pixels,
            classes,
            count,
            cursor: None,
            ray: None,
            hit: None,
            emitter_cursor: 0,
            analytic_emitter: false,
            classified: false,
            effect_cursor: 0,
            effect: None,
            pending: None,
            optical: None,
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
    pub(super) fn classes(&self) -> Result<&[GuideClass]> {
        let classes = self.classes.as_deref().ok_or_else(|| invalid("CPU guide diagnostics were not requested"))?;
        if !self.ready() || classes.len() != self.count {
            return Err(invalid("CPU guide diagnostics are incomplete"));
        }
        Ok(classes)
    }
    fn bypass(&mut self, reason: GuideClass) {
        self.pending = Some((Guide::default(), reason));
    }
    fn append(&mut self, guide: Guide, class: GuideClass, work: &mut WorkBudget, cancel: &CancelToken) -> Result<()> {
        work.tick(cancel).map_err(error)?;
        guide.validate()?;
        if self.ready() || class.eligible() != (guide.body != 0) {
            return Err(invalid("CPU denoising guide cursor or classification is invalid"));
        }
        if let Some(classes) = &mut self.classes {
            if classes.len() != self.pixels.len() || classes.len() >= classes.capacity() {
                return Err(invalid("CPU guide diagnostic cursor exceeds admission"));
            }
            classes.push(class);
        }
        self.pixels.push(guide);
        self.ray = None;
        self.cursor = None;
        self.hit = None;
        self.emitter_cursor = 0;
        self.analytic_emitter = false;
        self.classified = false;
        self.effect_cursor = 0;
        self.effect = None;
        self.pending = None;
        self.optical = None;
        Ok(())
    }
    fn launch(&mut self, launch: Launch) {
        let ray = launch.ray;
        self.ray = Some(ray);
        self.cursor = Some(match launch.plane {
            Some((begin, end, vertices)) => TraceCursor::from_plane(ray, begin..end, vertices),
            None => TraceCursor::new(ray, false),
        });
        self.hit = None;
        self.emitter_cursor = 0;
        self.analytic_emitter = false;
        self.classified = false;
        self.effect_cursor = 0;
        self.effect = None;
    }
    fn begin_optical(&mut self, primary: u32, footprint: f64, reflected: Option<Launch>, forward: Option<Forward>) {
        if let Some(launch) = reflected {
            self.optical = Some(OpticalWalk { primary, reflected: SurfaceGuide::default(), footprint, segment: Segment::Reflected, forward });
            self.launch(launch);
        } else if let Some(forward) = forward {
            self.optical = Some(OpticalWalk { primary, reflected: SurfaceGuide::default(), footprint, segment: forward.segment, forward: None });
            self.launch(forward.launch);
        } else {
            self.bypass(GuideClass::DegeneratePath);
        }
    }
    fn endpoint(&mut self, mut guide: Guide) {
        if let Some(mut optical) = self.optical {
            let body = guide.body;
            let transmitted = if matches!(optical.segment, Segment::Reflected) {
                optical.reflected = guide.surface();
                if let Some(forward) = optical.forward.take() {
                    optical.segment = forward.segment;
                    self.optical = Some(optical);
                    self.launch(forward.launch);
                    return;
                }
                0
            } else {
                body
            };
            let Some(identity) = Guide::optical_body(optical.primary, optical.reflected.body, transmitted) else {
                self.bypass(GuideClass::IdentityLimit);
                return;
            };
            if transmitted != 0 {
                guide.reflected = optical.reflected;
            }
            guide.body = identity;
        }
        let class = if self.optical.is_some() { GuideClass::Optical } else { GuideClass::Surface };
        self.pending = Some((guide, class));
    }
    /// A continuation creates one ray, visits one BVH node/triangle, scans one
    /// bounded emitter/effect row, or resolves/stores one guide. The normal image budget is shared.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn step(
        &mut self,
        scene: PreparedScene<'_>,
        request: &ImageRequest,
        options: &RaytraceOptions,
        glass: Option<GlassCertification<'_>>,
        clip: (f64, f64),
        effects: &[(usize, crate::surface_patterns::PreparedEffect)],
        maps: Option<&mut crate::job::textures::cpu_cache::CpuTextureCache>,
        aging: Option<&mut super::aging::Task>,
        work: &mut WorkBudget,
        cancel: &CancelToken,
    ) -> Result<()> {
        cancel.check()?;
        if self.ready() {
            return Err(invalid("CPU denoising guides are already complete"));
        }
        if let Some((guide, class)) = self.pending {
            return self.append(guide, class, work, cancel);
        }
        if self.ray.is_none() {
            work.tick(cancel).map_err(error)?;
            let pixel = self.pixels.len();
            let width = request.width as usize;
            let ray = primary_ray(request, (pixel % width) as f64 + 0.5, (pixel / width) as f64 + 0.5, clip).map_err(error)?;
            self.launch(Launch::ordinary(ray));
            return Ok(());
        }
        if let Some(cursor) = &mut self.cursor {
            let complete = if matches!(glass, Some(GlassCertification::Path(_))) {
                cursor.step_ordered(scene.bvh, work, cancel, |new, old| {
                    let is_glass = |hit: Hit| {
                        scene
                            .primitives
                            .get(hit.primitive_index)
                            .is_some_and(|look| options.dielectrics.iter().any(|input| input.body_index == look.body_index))
                    };
                    match (is_glass(new), is_glass(old)) {
                        (false, true) => true,
                        (true, false) => false,
                        _ => new.primitive_index < old.primitive_index,
                    }
                })
            } else {
                cursor.step(scene.bvh, work, cancel)
            }
            .map_err(error)?;
            if let Some(hit) = complete {
                self.cursor = None;
                self.hit = hit;
                if let Some(maps) = maps {
                    maps.reset_hit();
                }
                if let Some(aging) = aging {
                    aging.reset();
                }
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
            let class = if self.optical.is_some() { GuideClass::Unavailable } else { GuideClass::Emitter };
            return self.append(Guide::default(), class, work, cancel);
        }
        let Some(hit) = self.hit else {
            let class = if self.optical.is_some() { GuideClass::Unavailable } else { GuideClass::Background };
            return self.append(Guide::default(), class, work, cancel);
        };
        work.tick(cancel).map_err(error)?;
        let mut shared = scene.resolve_hit(ray, hit).map_err(error)?;
        if !self.classified {
            self.classified = true;
            if is_emitter(options, shared) {
                self.bypass(if self.optical.is_some() { GuideClass::Unavailable } else { GuideClass::Emitter });
                return Ok(());
            }
            if let Some(mut walk) = self.optical
                && let Segment::GlassExit(index) = walk.segment
            {
                match glass_exit(options, index, glass, ray, shared) {
                    Ok(launch) => {
                        walk.segment = Segment::Transmitted;
                        self.optical = Some(walk);
                        self.launch(launch);
                    }
                    Err(reason) => self.bypass(reason),
                }
                return Ok(());
            }
            if let Some((index, input)) = options.dielectrics.iter().enumerate().find(|(_, input)| input.body_index == shared.body_index) {
                let branches = if self.optical.is_some() {
                    Err(GuideClass::FurtherTransmission)
                } else if let Some(glass) = glass {
                    if glass.outside(options, request, ray) {
                        glass
                            .boundary(index)
                            .ok_or(GuideClass::Unavailable)
                            .and_then(|boundary| glass_branches(options, index, input, boundary, ray, shared))
                    } else {
                        Err(GuideClass::NestedMedium)
                    }
                } else {
                    Err(GuideClass::Unavailable)
                };
                match branches {
                    Ok((reflected, forward)) => {
                        let (_, _, footprint) = guide_geometry(request, shared.position);
                        self.begin_optical(body_id(shared)?, footprint, reflected, forward);
                    }
                    Err(reason) => self.bypass(reason),
                }
                return Ok(());
            }
        }
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
                let has_face_override = scene.primitives.get(shared.primitive_index).is_none_or(|look| look.has_face_override);
                let pixel = self.pixels.len();
                let width = request.width as usize;
                let footprint = if self.optical.is_none() {
                    super::image::camera_footprint(
                        request,
                        (pixel % width) as f64 + 0.5,
                        (pixel / width) as f64 + 0.5,
                        1.0,
                        clip,
                        shared.position,
                        shared.oriented_geometric_normal(),
                    )?
                } else {
                    None
                };
                maps.begin_hit(
                    crate::surface_patterns::ShadingSurface { surface: shared.surface, shading_normal: shared.shading_normal },
                    (!has_face_override).then_some(shared.body_index),
                    crate::job::textures::cpu_preparation::MapPoint {
                        position: shared.position,
                        geometric_normal: shared.oriented_geometric_normal(),
                        view_direction: -ray.direction(),
                    },
                    footprint,
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
        let reflection = options
            .reflections
            .as_ref()
            .filter(|settings| settings.max_bounces > 0)
            .and_then(|settings| settings.bodies.iter().find(|value| value.body == shared.body_name).map(|value| value.mode));
        let mirror = reflection == Some(ReflectionMode::Mirror);
        let near_perfect = shared.surface.metallic() >= 0.99 && shared.surface.roughness() <= 0.05;
        let smooth_metal = shared.surface.metallic() > 0.5 && shared.surface.roughness() < 0.25;
        let active_coat = shared.surface.clear_coat().is_some_and(|coat| coat.weight() > 0.0);
        let sharp_coat = shared.surface.clear_coat().is_some_and(|coat| coat.weight() > 0.0 && coat.roughness() < 0.05);
        let geometric = shared.oriented_geometric_normal();
        let (relative, depth, surface_footprint) = guide_geometry(request, shared.position);
        let body = body_id(shared)?;
        if self.optical.is_none() && (mirror || (near_perfect && !active_coat && (options.path.is_some() || reflection.is_some()))) {
            if lens_active(options) {
                self.bypass(GuideClass::DepthOfField);
                return Ok(());
            }
            if options.path.is_some_and(|path| path.max_depth < 2) {
                self.bypass(GuideClass::PathDepth);
                return Ok(());
            }
            if glass.is_some_and(|glass| !glass.outside(options, request, ray)) {
                self.bypass(GuideClass::NestedMedium);
                return Ok(());
            }
            let reflected = reflection::mirror(shared.surface, shared.shading_normal, -ray.direction())
                .ok()
                .flatten()
                .filter(|sample| sample.weight != Vec3::ZERO && sample.direction.dot(geometric) > 0.0)
                .and_then(|sample| shared.spawn(sample.direction).ok())
                .map(Launch::ordinary);
            self.begin_optical(body, surface_footprint, reflected, None);
            return Ok(());
        }
        if mirror || smooth_metal || sharp_coat {
            self.bypass(if self.optical.is_some() { GuideClass::ReflectiveEndpoint } else { GuideClass::UnsupportedMaterial });
            return Ok(());
        }
        let normal = if shared.shading_normal.dot(-ray.direction()) > 0.0 { shared.shading_normal } else { geometric };
        let array = |v: Vec3| [v.x as f32, v.y as f32, v.z as f32];
        let guide = Guide {
            albedo: array(shared.surface.base_color()),
            body,
            normal: array(normal),
            depth: depth as f32,
            position: array(relative),
            footprint: surface_footprint.max(self.optical.map_or(0.0, |walk| walk.footprint)).max(0.000001) as f32,
            reflected: SurfaceGuide::default(),
        };
        guide.validate()?;
        self.endpoint(guide);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit() -> SharedHit<'static> {
        SharedHit {
            body_index: 0,
            body_name: "glass",
            primitive_index: 0,
            distance_mm: 1.0,
            position: Vec3::ZERO,
            geometric_normal: Vec3::new(0.0, 0.0, 1.0),
            front_face: true,
            shading_normal: Vec3::new(0.0, 0.0, 1.0),
            surface: crate::surface::OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0.0, 0.5, 1.5).unwrap(),
            vertices: [Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, -1.0, 0.0), Vec3::new(0.0, 2.0, 0.0)],
            barycentric: [1.0 / 3.0; 3],
        }
    }

    #[test]
    fn optical_branch_diagnostics_explain_actual_policy_bypasses() {
        let mut options = RaytraceOptions { path: Some(crate::job::CpuPathOptions { max_depth: 3 }), ..Default::default() };
        let mut input = dielectric::tests::input(DielectricBoundary::ThinSheet);
        let boundary = CertifiedBoundary::Legacy(dielectric::Certificate::ThinSheet);
        let ray = Ray::new(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, -1.0), 0.0, 10.0).unwrap();
        assert!(glass_branches(&options, 0, &input, boundary, ray, hit()).is_ok());
        options.lens = Some(crate::job::ThinLens { aperture_radius_mm: 0.1, focus_distance_mm: 1.0 });
        assert_eq!(glass_branches(&options, 0, &input, boundary, ray, hit()).err(), Some(GuideClass::DepthOfField));
        options.lens = None;
        options.path.as_mut().unwrap().max_depth = 1;
        assert_eq!(glass_branches(&options, 0, &input, boundary, ray, hit()).err(), Some(GuideClass::PathDepth));
        options.path.as_mut().unwrap().max_depth = 3;
        input.model = Some(crate::glass_model::GlassModel { roughness: 0.2, ..Default::default() });
        assert_eq!(glass_branches(&options, 0, &input, boundary, ray, hit()).err(), Some(GuideClass::UnsupportedMaterial));
        input.model = None;
        assert!(glass_branches(&options, 0, &input, boundary, ray, hit()).is_ok());
    }

    #[test]
    fn optical_endpoint_misses_and_emitters_do_not_classify_the_primary_as_background() {
        let cancel = CancelToken::default();
        let bvh = super::super::bvh::BvhBuilder::new(Vec::new(), Default::default(), &cancel).unwrap().finish().unwrap();
        let source = crate::ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY };
        let request = ImageRequest {
            studio_weights: None,
            integrator: Default::default(),
            raytrace: None,
            camera: crate::Camera::default(),
            width: 1,
            height: 1,
            background: [0; 3],
            transparent_background: false,
            environment: None,
            textures: None,
        };
        let ray = Ray::new(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, -1.0), 0.0, 10.0).unwrap();
        for optical in [false, true] {
            for emitter in [false, true] {
                let mut options = RaytraceOptions { path: Some(crate::job::CpuPathOptions { max_depth: 3 }), ..Default::default() };
                if emitter {
                    options.area_lights = Some(vec![crate::job::RectangleLightOptions {
                        center_mm: [0.0; 3],
                        edge_u_mm: [2.0, 0.0, 0.0],
                        edge_v_mm: [0.0, 2.0, 0.0],
                        color: [1.0; 3],
                        intensity: 1.0,
                    }]);
                }
                let mut guides = Guides::new_with_diagnostics(1, 1, true).unwrap();
                if optical {
                    guides.begin_optical(1, 1.0, Some(Launch::ordinary(ray)), None);
                } else {
                    guides.launch(Launch::ordinary(ray));
                }
                let mut work = WorkBudget::new(100).unwrap();
                for _ in 0..16 {
                    if guides.ready() {
                        break;
                    }
                    guides
                        .step(
                            PreparedScene { bvh: &bvh, primitives: &[], source: &source },
                            &request,
                            &options,
                            None,
                            (0.0, 10.0),
                            &[],
                            None,
                            None,
                            &mut work,
                            &cancel,
                        )
                        .unwrap();
                }
                assert!(guides.ready(), "optical={optical} emitter={emitter}");
                let expected = if optical {
                    GuideClass::Unavailable
                } else if emitter {
                    GuideClass::Emitter
                } else {
                    GuideClass::Background
                };
                assert_eq!(guides.classes().unwrap(), &[expected], "optical={optical} emitter={emitter}");
                assert_eq!(guides.pixels().unwrap()[0].body, 0);
            }
        }
    }

    #[test]
    fn diagnostic_plane_is_optional_bounded_and_cancellation_safe() {
        assert_eq!(std::mem::size_of::<Guide>(), 96);
        assert_eq!(Guides::required_bytes_with_diagnostics(3, 1, true).unwrap() - Guides::required_bytes(3, 1).unwrap(), 3);
        let mut disabled = Guides::new(1, 1).unwrap();
        assert!(disabled.classes.is_none());
        let cancel = CancelToken::default();
        let mut work = WorkBudget::new(100).unwrap();
        disabled.append(Guide::default(), GuideClass::Background, &mut work, &cancel).unwrap();
        assert!(disabled.ready());
        assert!(disabled.classes().is_err());
        let mut guides = Guides::new_with_diagnostics(3, 1, true).unwrap();
        assert!(guides.classes().is_err());
        for class in [GuideClass::Background, GuideClass::Emitter, GuideClass::NestedMedium] {
            guides.append(Guide::default(), class, &mut work, &cancel).unwrap();
        }
        assert_eq!(guides.classes().unwrap(), &[GuideClass::Background, GuideClass::Emitter, GuideClass::NestedMedium]);
        assert_eq!(guides.classes.as_ref().unwrap().capacity(), 3);
        let mut interrupted = Guides::new_with_diagnostics(1, 1, true).unwrap();
        cancel.cancel();
        assert!(interrupted.append(Guide::default(), GuideClass::Emitter, &mut work, &cancel).is_err());
        assert!(interrupted.pixels.is_empty());
        assert!(interrupted.classes.as_ref().unwrap().is_empty());
    }
}
