//! Opt-in, bounded photon density estimate for delta and finite-density sources. Every trace, event,
//! merge-sort copy, grid lookup and gathered photon cooperates with the job ledger.
use super::super::{
    Bounds,
    indirect::{PhotonCone, PhotonGuide},
};
use super::*;
use crate::glass_model::WAVELENGTHS_NM;
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Default)]
struct Photon {
    cell: [i64; 3],
    position: Vec3,
    normal: Vec3,
    incoming: Vec3,
    flux: Vec3,
    body: usize,
    kind: ReceiverKind,
    side: i8,
    source: u32,
    smooth_chain_events: u8,
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ReceiverKind {
    #[default]
    Opaque,
    Rough,
    Volume,
}
#[derive(Clone, Copy)]
struct Sheet {
    index: usize,
    normal: Vec3,
    geometric: Vec3,
    direction: Vec3,
    ray: Ray,
    hit: super::super::Hit,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum SmoothSource {
    Rectangle(usize),
    Environment,
}
struct Path {
    ray: Ray,
    cursor: Option<TraceCursor>,
    hit: Option<super::super::Hit>,
    emitter: Option<(usize, f64, Vec3)>,
    emitter_cursor: usize,
    flux: Vec3,
    from: Vec3,
    media: [usize; job::MAX_NESTED_MEDIA],
    len: usize,
    depth: u8,
    optical: bool,
    channel: Option<u8>,
    source: Option<(Vec3, f64)>,
    source_index: u32,
    smooth_source: Option<SmoothSource>,
    source_plane: Option<[Vec3; 3]>,
    sheet: Option<Sheet>,
    origin: Option<super::super::origin::Task>,
    verify_origin: bool,
    reservoir: Option<Photon>,
    candidates: u8,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    GuideBounds,
    SourcePowers,
    Emit,
    Origin,
    Trace,
    Emitters,
    Resolve,
    Sheet,
    Sort,
    Ready,
}
pub(super) struct Task {
    photons: Vec<Photon>,
    scratch: Vec<Photon>,
    options: job::CausticOptions,
    guide_bounds: Bounds,
    guide_region: usize,
    guide: Option<PhotonGuide>,
    source_powers: [f64; job::photon_sources::MAX_PHOTON_SOURCES],
    source_cursor: usize,
    sky_row: u32,
    sky_integral: f64,
    sources: Option<job::photon_sources::PhotonSourceDistribution>,
    pass: u32,
    ordinal: u32,
    path: Option<Path>,
    stage: Stage,
    width: usize,
    begin: usize,
    left: usize,
    right: usize,
    end: usize,
    out: usize,
}
fn model_error(error: impl std::fmt::Display) -> JobError {
    JobError::new(ErrorCode::InvalidScene, error.to_string())
}
fn finite_flux(flux: Vec3) -> job::Result<()> {
    if !flux.is_finite() || flux.x < 0.0 || flux.y < 0.0 || flux.z < 0.0 {
        return Err(invalid("CPU caustic photon power is nonfinite or negative"));
    }
    Ok(())
}
fn cell(position: Vec3, radius: f64) -> job::Result<[i64; 3]> {
    let values = [position.x, position.y, position.z].map(|value| (value / radius).floor());
    if values.iter().any(|value| !value.is_finite() || value.abs() > 4e18) {
        return Err(invalid("CPU caustic spatial cell exceeds its bounded integer domain"));
    }
    Ok(values.map(|value| value as i64))
}
fn reserve<T>(count: usize) -> job::Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| error(TraceError::AllocationFailed))?;
    if values.capacity() != count {
        return Err(error(TraceError::MemoryLimit));
    }
    Ok(values)
}
impl Task {
    pub(super) fn required_bytes(options: job::CausticOptions) -> job::Result<u64> {
        options.validate()?;
        u64::from(options.photons)
            .checked_mul(2 * std::mem::size_of::<Photon>() as u64)
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>() as u64))
            .ok_or_else(|| error(TraceError::MemoryLimit))
    }
    pub(super) fn new(options: job::CausticOptions) -> job::Result<Self> {
        Self::with_capacity(options, options.photons as usize)
    }
    fn with_capacity(options: job::CausticOptions, capacity: usize) -> job::Result<Self> {
        Self::required_bytes(options)?;
        Ok(Self {
            photons: reserve(capacity)?,
            scratch: reserve(capacity)?,
            options,
            guide_bounds: Bounds::empty(),
            guide_region: 0,
            guide: None,
            source_powers: [0.0; job::photon_sources::MAX_PHOTON_SOURCES],
            source_cursor: 0,
            sky_row: 0,
            sky_integral: 0.0,
            sources: None,
            pass: 0,
            ordinal: 0,
            path: None,
            stage: if options.guided_emission {
                Stage::GuideBounds
            } else if options.power_weighted_sources {
                Stage::SourcePowers
            } else {
                Stage::Emit
            },
            width: 1,
            begin: 0,
            left: 0,
            right: 0,
            end: 0,
            out: 0,
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn worker(options: job::CausticOptions, capacity: usize) -> job::Result<Self> {
        Self::with_capacity(options, capacity)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn worker_bytes(capacity: usize) -> job::Result<u64> {
        (capacity as u64)
            .checked_mul(2 * std::mem::size_of::<Photon>() as u64)
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>() as u64))
            .ok_or_else(|| error(TraceError::MemoryLimit))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn parallel_ready(&self) -> bool {
        self.stage == Stage::Emit && self.path.is_none() && self.ordinal < self.options.photons
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn parallel_sources(&self, task: &ImageTask) -> bool {
        !task.lights.lights().is_empty()
            || task.options.punctual_lights.as_ref().is_some_and(|lights| !lights.is_empty())
            || (self.options.area_lights && task.options.area_lights.as_ref().is_some_and(|lights| !lights.is_empty()))
            || (self.options.environment && (task.environment.is_some() || task.options.sky.is_some() || task.options.environment.is_some()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn batch_bounds(&self) -> job::Result<(u32, u32)> {
        if !self.parallel_ready() {
            return Err(invalid("CPU photon batch is not at an emission boundary"));
        }
        Ok((self.ordinal, self.options.photons))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn begin_batch(&mut self, source: &Self, begin: u32) -> job::Result<()> {
        if !source.parallel_ready() || begin > source.options.photons || self.path.is_some() {
            return Err(invalid("CPU photon batch has invalid source or ordinal"));
        }
        self.photons.clear();
        self.scratch.clear();
        self.options = source.options;
        self.guide_bounds = source.guide_bounds;
        self.guide_region = source.guide_region;
        self.guide = source.guide;
        self.source_powers = source.source_powers;
        self.source_cursor = source.source_cursor;
        self.sky_row = source.sky_row;
        self.sky_integral = source.sky_integral;
        self.sources = source.sources;
        self.pass = source.pass;
        self.ordinal = begin;
        self.stage = Stage::Emit;
        self.begin = begin as usize;
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn trace_batch(&mut self, task: &mut TransportContext<'_>, end: u32) -> job::Result<()> {
        if end < self.ordinal || end > self.options.photons || end as usize - self.begin > self.photons.capacity() {
            return Err(invalid("CPU photon batch exceeds its admitted range"));
        }
        while self.ordinal < end {
            task.cancel.check()?;
            advance(self, task)?;
            if self.stage == Stage::Sort {
                // A source-free or empty scene has no deposits. Preserve the
                // bounded batch boundary without entering its local sort.
                self.ordinal = end;
            }
        }
        if self.path.is_some() {
            return Err(invalid("CPU photon batch ended inside a path"));
        }
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn merge_batch(&mut self, worker: &mut Self) -> job::Result<()> {
        if worker.begin != self.ordinal as usize || worker.pass != self.pass || worker.path.is_some() {
            return Err(invalid("CPU photon batches must merge in ordinal order"));
        }
        for photon in worker.photons.drain(..) {
            if self.photons.len() == self.photons.capacity() || self.scratch.len() == self.scratch.capacity() {
                return Err(error(TraceError::MemoryLimit));
            }
            self.photons.push(photon);
            self.scratch.push(Photon::default());
        }
        worker.scratch.clear();
        self.ordinal = worker.ordinal;
        Ok(())
    }
    pub(super) fn ready(&self) -> bool {
        self.stage == Stage::Ready
    }
    pub(super) fn initial_preparation(&self) -> bool {
        self.pass == 0 && !self.ready()
    }
    pub(super) fn refresh_due(&self, pass: u32) -> bool {
        self.options.refresh_per_pass && pass != self.pass
    }
    fn refresh(&mut self, pass: u32, work: &mut WorkBudget, cancel: &CancelToken) -> job::Result<()> {
        if !self.ready() || self.path.is_some() || self.pass.checked_add(1) != Some(pass) {
            return Err(invalid("CPU caustic refresh requires the next complete camera pass"));
        }
        work.tick(cancel).map_err(error)?;
        self.pass = pass;
        self.photons.clear();
        self.scratch.clear();
        self.ordinal = 0;
        self.stage = Stage::Emit;
        self.width = 1;
        self.begin = 0;
        self.left = 0;
        self.right = 0;
        self.end = 0;
        self.out = 0;
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn receiver_counts(&self) -> [usize; 3] {
        let mut counts = [0; 3];
        for photon in &self.photons {
            counts[match photon.kind {
                ReceiverKind::Opaque => 0,
                ReceiverKind::Rough => 1,
                ReceiverKind::Volume => 2,
            }] += 1;
        }
        counts
    }
    #[cfg(test)]
    pub(super) fn test_epoch(&self) -> u32 {
        self.pass
    }
    #[cfg(test)]
    pub(super) fn test_buffers(&self) -> [usize; 2] {
        let mut buffers = [self.photons.as_ptr() as usize, self.scratch.as_ptr() as usize];
        buffers.sort_unstable();
        buffers
    }
    #[cfg(test)]
    pub(super) fn test_deposits(&self) -> Vec<[Vec3; 3]> {
        self.photons.iter().map(|photon| [photon.position, photon.incoming, photon.flux]).collect()
    }
    pub(super) fn progress(&self) -> (u64, u64) {
        (u64::from(self.ordinal), u64::from(self.options.photons))
    }
    fn random(&self, depth: u8, domain: u64) -> [f64; 2] {
        super::super::progressive::variates(self.options.seed, u64::from(self.pass), self.ordinal, 0x4341_5553_0000 + u64::from(depth) * 256 + domain)
    }
    fn finish_path(&mut self) -> job::Result<()> {
        if let Some(path) = self.path.take()
            && let Some(mut photon) = path.reservoir
        {
            photon.flux = photon.flux * f64::from(path.candidates);
            finite_flux(photon.flux)?;
            if self.photons.len() == self.photons.capacity() || self.scratch.len() == self.scratch.capacity() {
                return Err(error(TraceError::MemoryLimit));
            }
            self.photons.push(photon);
            self.scratch.push(Photon::default());
        }
        self.ordinal += 1;
        self.stage = Stage::Emit;
        Ok(())
    }
}
fn candidate(
    map: &Task,
    position: Vec3,
    normal: Vec3,
    incoming: Vec3,
    body: usize,
    kind: ReceiverKind,
    side: i8,
    path: &mut Path,
) -> job::Result<()> {
    if !path.optical || path.flux == Vec3::ZERO || path.source.is_some_and(|(origin, minimum)| origin.dist(position) < minimum) {
        return Ok(());
    }
    path.candidates = path
        .candidates
        .checked_add(1)
        .filter(|count| *count <= map.options.max_depth)
        .ok_or_else(|| invalid("CPU caustic receiver reservoir exceeds its path depth"))?;
    // One independent counter draw selects a uniform visit, without changing
    // any interface, wavelength, or free-flight variate on this light path.
    if map.random(path.depth, 13)[0] < 1.0 / f64::from(path.candidates) {
        path.reservoir = Some(Photon {
            cell: cell(position, map.options.radius_mm)?,
            position,
            normal,
            incoming,
            flux: path.flux,
            body,
            kind,
            side,
            source: path.source_index,
            smooth_chain_events: if path.smooth_source.is_some() { path.depth } else { 0 },
        });
    }
    Ok(())
}
fn prepared<'a>(task: &'a TransportContext<'_>) -> job::Result<PreparedScene<'a>> {
    Ok(PreparedScene { bvh: task.bvh.as_ref().ok_or_else(|| invalid("CPU caustic BVH missing"))?, primitives: task.looks, source: task.scene })
}
fn normal_basis(n: Vec3, x: f64, y: f64, z: f64) -> job::Result<Vec3> {
    let tangent =
        super::super::unit((if n.z.abs() > 0.9 { Vec3::Y } else { Vec3::Z }).cross(n)).ok_or_else(|| invalid("CPU caustic source basis"))?;
    super::super::unit(tangent * x + n.cross(tangent) * y + n * z).ok_or_else(|| invalid("CPU caustic source direction"))
}
pub(super) fn smooth_interface(input: &job::DielectricOptions) -> bool {
    input.boundary != job::DielectricBoundary::ThinSheet && input.model.unwrap_or_default().roughness == 0.0
}
fn prepare_guide(map: &mut Task, task: &TransportContext<'_>) -> job::Result<()> {
    if map.guide_region < task.options.dielectrics.len() {
        let certificate = task.glass.first().ok_or_else(|| invalid("CPU caustic optical bounds certificate missing"))?;
        map.guide_bounds = map.guide_bounds.include(certificate.region(map.guide_region)?.bounds);
        map.guide_region += 1;
        return Ok(());
    }
    let bounds = map.guide_bounds;
    let center = bounds.min * 0.5 + bounds.max * 0.5;
    let radius = (bounds.max - bounds.min).len() * 0.5;
    // Round outward to cover the captured corners and the projected basis.
    let padded_radius = radius + 32.0 * f64::EPSILON * (center.len() + radius);
    if center.is_finite() && padded_radius.is_finite() && radius > 0.0 {
        map.guide = Some(PhotonGuide { center, radius: padded_radius });
    }
    map.stage = if map.options.power_weighted_sources { Stage::SourcePowers } else { Stage::Emit };
    Ok(())
}
fn bounding_disk(task: &TransportContext<'_>, guide: Option<PhotonGuide>, toward: Vec3, [u, v]: [f64; 2]) -> job::Result<Option<(Vec3, f64)>> {
    let bounds = task.scene.bounds;
    let center = bounds.min * 0.5 + bounds.max * 0.5;
    let radius = (bounds.max - bounds.min).len() * 0.5;
    if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return Ok(None);
    }
    let mut launch_center = center + toward * (radius + 1.0);
    let mut disk_radius = radius;
    if let Some(guide) = guide.filter(|guide| guide.radius < radius) {
        // Keep the plane upstream of the full scene, including opaque blockers.
        let launch = radius + 1.0 + (center - guide.center).dot(toward);
        let guided_center = guide.center + toward * launch;
        let extent = guided_center.x.abs().max(guided_center.y.abs()).max(guided_center.z.abs()) + guide.radius;
        // Decide once for the entire disk, independently of the drawn point.
        if extent <= super::super::MAX_COORDINATE_MM {
            launch_center = guided_center;
            disk_radius = guide.radius;
        }
    }
    let r = disk_radius * u.sqrt();
    let phi = TAU * v;
    let disk = normal_basis(toward, phi.cos(), phi.sin(), 0.0)?;
    Ok(Some((launch_center + disk * r, disk_radius)))
}

// One source (or one sky row) per charged preparation step; immutable proposal
// storage is retained across map refreshes and never rescanned per photon.
fn prepare_source_powers(map: &mut Task, task: &TransportContext<'_>) -> job::Result<()> {
    use job::photon_sources as power;
    let directional = task.lights.lights().len();
    let punctual = task.options.punctual_lights.as_ref().map_or(0, Vec::len);
    let area = if map.options.area_lights { task.options.area_lights.as_deref().unwrap_or(&[]) } else { &[] };
    let local = directional + punctual + area.len();
    let environment = map.options.environment && (task.environment.is_some() || task.options.sky.is_some() || task.options.environment.is_some());
    let count = local + usize::from(environment);
    if count > map.source_powers.len() {
        return Err(invalid("CPU photon source count exceeds proposal capacity"));
    }
    if map.source_cursor == count {
        if count > 0 {
            map.sources = Some(power::PhotonSourceDistribution::new(&map.source_powers[..count])?);
        }
        map.stage = Stage::Emit;
        return Ok(());
    }
    let index = map.source_cursor;
    let value = if let Some(light) = task.lights.lights().get(index) {
        let radius = bounding_disk(task, map.guide, light.direction(), [0.5, 0.5])?.map_or(0.0, |(_, radius)| radius);
        let c = light.color() * light.intensity();
        power::disk_power(radius, power::luminance([c.x, c.y, c.z])?)?
    } else if index < directional + punctual {
        let light = task
            .options
            .punctual_lights
            .as_ref()
            .and_then(|v| v.get(index - directional))
            .ok_or_else(|| invalid("CPU photon power light missing"))?;
        power::punctual_power(light)?
    } else if index < local {
        let light = area.get(index - directional - punctual).ok_or_else(|| invalid("CPU photon power rectangle missing"))?;
        let u = Vec3::new(light.edge_u_mm[0], light.edge_u_mm[1], light.edge_u_mm[2]);
        let v = Vec3::new(light.edge_v_mm[0], light.edge_v_mm[1], light.edge_v_mm[2]);
        power::rectangle_power(u.cross(v).len(), light.color.map(|c| c * light.intensity))?
    } else {
        let integral = if let Some(environment) = &task.environment {
            environment.luminance_integral().map_err(environment_error)?
        } else if let Some(sky) = task.options.sky.as_deref() {
            if map.sky_row < 256 {
                let theta = PI * (f64::from(map.sky_row) + 0.5) / 256.0;
                let c = sky.radiance(Vec3::new(theta.sin(), 0.0, theta.cos()))?;
                let width = 4.0 * PI * theta.sin() * (PI / 512.0).sin();
                map.sky_integral += power::luminance([c.x, c.y, c.z])? * width;
                map.sky_row += 1;
                return Ok(());
            }
            map.sky_integral
        } else {
            let environment = task.options.environment.ok_or_else(|| invalid("CPU photon power environment missing"))?;
            4.0 * PI * power::luminance(environment.color.map(|c| c * environment.intensity))?
        };
        // The guide disk is direction-independent in admitted ordinary scenes.
        // Coordinate-envelope fallbacks can make this a power proxy; full source
        // support and reciprocal selection probability preserve transport either way.
        let radius = bounding_disk(task, map.guide, Vec3::Z, [0.5, 0.5])?.map_or(0.0, |(_, radius)| radius);
        power::disk_power(radius, integral)?
    };
    *map.source_powers.get_mut(index).ok_or_else(|| invalid("CPU photon source power index"))? = value;
    map.source_cursor += 1;
    Ok(())
}

fn spot_feather(cosine: f64, outer: f64, inner: f64) -> f64 {
    let feather = ((cosine - outer) / (inner - outer)).clamp(0.0, 1.0);
    feather * feather * (3.0 - 2.0 * feather)
}

fn emit(map: &mut Task, task: &TransportContext<'_>) -> job::Result<()> {
    let directional = task.lights.lights().len();
    let punctual = task.options.punctual_lights.as_ref().map_or(0, Vec::len);
    let area = if map.options.area_lights { task.options.area_lights.as_deref().unwrap_or(&[]) } else { &[] };
    let delta_count = directional + punctual;
    let local_count = delta_count + area.len();
    let environment = map.options.environment && (task.environment.is_some() || task.options.sky.is_some() || task.options.environment.is_some());
    let count = local_count + usize::from(environment);
    if map.ordinal == map.options.photons || count == 0 || task.scene.meshes.is_empty() {
        map.stage = Stage::Sort;
        return Ok(());
    }
    let choice = map.random(0, 0)[0];
    let (source, inverse_probability) = if let Some(distribution) = &map.sources {
        let (source, probability) = distribution.select(choice)?;
        (source, 1.0 / probability)
    } else {
        ((choice * count as f64) as usize, count as f64)
    };
    let [u, v] = map.random(0, 1);
    let normalization = inverse_probability / f64::from(map.options.photons);
    let (origin, direction, flux, point_source) = if let Some(light) = task.lights.lights().get(source) {
        let Some((origin, radius)) = bounding_disk(task, map.guide, light.direction(), [u, v])? else {
            map.finish_path()?;
            return Ok(());
        };
        (origin, -light.direction(), light.color() * (light.intensity() * PI * radius * radius * normalization), None)
    } else if source < delta_count {
        let light = task
            .options
            .punctual_lights
            .as_ref()
            .and_then(|lights| lights.get(source - directional))
            .ok_or_else(|| invalid("CPU caustic point source index"))?;
        let origin = Vec3::new(light.position_mm[0], light.position_mm[1], light.position_mm[2]);
        let spot = if let Some(spot) = light.cone {
            let axis = super::super::unit(Vec3::new(spot.emitted_axis[0], spot.emitted_axis[1], spot.emitted_axis[2]))
                .ok_or_else(|| invalid("CPU caustic cone axis"))?;
            Some((axis, spot.outer_half_angle_radians.cos(), spot.inner_half_angle_radians.cos()))
        } else {
            None
        };
        let (mut direction, mut density, mut feather) = if let Some((axis, outer, inner)) = spot {
            let cosine = 1.0 - u * (1.0 - outer);
            let sine = (1.0 - cosine * cosine).max(0.0).sqrt();
            let direction = normal_basis(axis, sine * (TAU * v).cos(), sine * (TAU * v).sin(), cosine)?;
            (direction, 1.0 / (TAU * (1.0 - outer)), spot_feather(cosine, outer, inner))
        } else {
            let z = 1.0 - 2.0 * u;
            let sine = (1.0 - z * z).max(0.0).sqrt();
            (Vec3::new(sine * (TAU * v).cos(), sine * (TAU * v).sin(), z), 1.0 / (4.0 * PI), 1.0)
        };
        if let Some(guide) = map.guide.and_then(|guide| guide.cone(origin, spot.map_or(2.0, |(_, outer, _)| 1.0 - outer))) {
            let sampled_cone = map.random(0, 17)[0] < PhotonCone::MIX;
            if sampled_cone {
                direction = guide.sample([u, v]).map_err(error)?;
                if let Some((axis, outer, inner)) = spot {
                    feather = spot_feather(axis.dot(direction), outer, inner);
                }
            }
            if feather == 0.0 {
                map.finish_path()?;
                return Ok(());
            }
            // Positive spot emission proves membership in the baseline cone.
            density = guide.mixture_pdf(direction, density, sampled_cone);
        }
        let color = Vec3::new(light.color[0], light.color[1], light.color[2]);
        (
            origin,
            direction,
            color * (light.intensity * light.reference_distance_mm.powi(2) * feather * normalization / density),
            Some((origin, light.minimum_distance_mm)),
        )
    } else if source < local_count {
        let light = area.get(source - delta_count).ok_or_else(|| invalid("CPU caustic area source index"))?;
        let light = super::super::indirect::Rectangle::new(light).map_err(error)?;
        let guided = map.guide.map(|guide| (guide, map.random(0, 17)[0]));
        let (origin, direction, power) = light.emit_photon(map.random(0, 16), [u, v], guided).map_err(error)?;
        (origin, direction, power * normalization, None)
    } else {
        let [x, y] = map.random(0, 16);
        let x = super::super::indirect::open_stratum(x, 0, 1).map_err(error)?;
        let y = super::super::indirect::open_stratum(y, 0, 1).map_err(error)?;
        let (toward, radiance, pdf) = super::indirect_image::environment_sample(task, [x, y])?;
        let Some((origin, radius)) = bounding_disk(task, map.guide, toward, [u, v])? else {
            map.finish_path()?;
            return Ok(());
        };
        // The disk is perpendicular to the sampled incident direction, so
        // its projected area is constant without an extra cosine factor.
        (origin, -toward, radiance * (PI * radius * radius * normalization / pdf), None)
    };
    finite_flux(flux)?;
    if flux == Vec3::ZERO {
        map.finish_path()?;
        return Ok(());
    }
    let ray = Ray::new(origin, direction, 0.0_f64.next_up(), super::super::MAX_DISTANCE_MM).map_err(error)?;
    let origin_task = if let Some(certificate) = task.glass.first() {
        super::super::origin::Task::required(certificate, &task.options.dielectrics, origin, origin)?
            .then(|| super::super::origin::Task::new(origin, origin))
            .transpose()?
    } else {
        None
    };
    map.stage = if origin_task.is_some() { Stage::Origin } else { Stage::Trace };
    map.path = Some(Path {
        ray,
        cursor: Some(TraceCursor::new(ray, false)),
        hit: None,
        emitter: None,
        emitter_cursor: 0,
        flux,
        from: origin,
        media: [0; job::MAX_NESTED_MEDIA],
        len: 0,
        depth: 0,
        optical: false,
        channel: None,
        source: point_source,
        source_index: if (directional..delta_count).contains(&source) {
            u32::try_from(source - directional).map_err(|_| invalid("CPU caustic source index exceeds its bounded record"))?
        } else {
            u32::MAX
        },
        smooth_source: if source >= local_count {
            Some(SmoothSource::Environment)
        } else {
            source.checked_sub(delta_count).map(SmoothSource::Rectangle)
        },
        source_plane: None,
        sheet: None,
        origin: origin_task,
        verify_origin: false,
        reservoir: None,
        candidates: 0,
    });
    Ok(())
}
fn current(path: &Path) -> job::Result<Option<usize>> {
    if path.len == 0 { Ok(None) } else { path.media.get(path.len - 1).copied().map(Some).ok_or_else(|| invalid("CPU caustic medium stack index")) }
}
fn wavelength(path: &Path) -> job::Result<f64> {
    WAVELENGTHS_NM.get(usize::from(path.channel.unwrap_or(1))).copied().ok_or_else(|| invalid("CPU caustic wavelength channel"))
}
fn ior(task: &TransportContext<'_>, path: &Path, index: Option<usize>) -> job::Result<f64> {
    let Some(index) = index else { return Ok(1.0) };
    let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU caustic medium disappeared"))?;
    input.model.unwrap_or_default().ior(input.ior, wavelength(path)?).map_err(model_error)
}
fn select_spectrum(map: &Task, task: &TransportContext<'_>, path: &mut Path, index: usize) -> job::Result<()> {
    if path.channel.is_some() {
        return Ok(());
    }
    let spectral = |index: usize| task.options.dielectrics.get(index).and_then(|input| input.model).is_some_and(|model| model.spectral());
    if !spectral(index) && !current(path)?.is_some_and(spectral) {
        return Ok(());
    }
    let channel = (map.random(path.depth, 7)[0] * 3.0) as u8;
    path.flux = multiply(
        path.flux,
        match channel {
            0 => Vec3::new(3.0, 0.0, 0.0),
            1 => Vec3::new(0.0, 3.0, 0.0),
            2 => Vec3::new(0.0, 0.0, 3.0),
            _ => return Err(invalid("CPU caustic wavelength draw")),
        },
    );
    path.channel = Some(channel);
    Ok(())
}
fn launch(task: &TransportContext<'_>, path: &mut Path, index: usize, hit: SharedHit<'_>, direction: Vec3) -> job::Result<()> {
    let region = task.glass.first().ok_or_else(|| invalid("CPU caustic glass certificate missing"))?.region(index)?;
    let ray = Ray::new(hit.position, direction, 0.0_f64.next_up(), super::super::MAX_DISTANCE_MM).map_err(error)?;
    path.ray = ray;
    path.cursor = Some(TraceCursor::from_plane(ray, region.primitive_begin..region.primitive_end, hit.vertices));
    path.hit = None;
    path.emitter = None;
    path.emitter_cursor = 0;
    path.from = hit.position;
    path.source_plane = Some(hit.vertices);
    path.depth += 1;
    Ok(())
}
fn sample_uniforms(map: &Task, path: &Path) -> [f64; 3] {
    let [branch, x] = map.random(path.depth, 2);
    let [y, _] = map.random(path.depth, 3);
    [x, y, branch]
}
fn smooth_sheet(input: &job::DielectricOptions, path: &Path, normal: Vec3) -> job::Result<(Vec3, Vec3)> {
    let (_, r, t) = dielectric::modeled_sheet(path.ray.direction(), normal, input).map_err(error)?;
    Ok((r, t))
}

fn resolve(map: &mut Task, task: &TransportContext<'_>, path: &mut Path) -> job::Result<bool> {
    let geometry = path.hit.map(|hit| prepared(task)?.resolve_hit(path.ray, hit).map_err(error)).transpose()?;
    let emitter = path.emitter.filter(|(_, distance, _)| geometry.as_ref().is_none_or(|hit| *distance < hit.distance_mm));
    let endpoint = if let Some((_, _, position)) = emitter {
        position
    } else if let Some(hit) = &geometry {
        hit.position
    } else {
        if path.len != 0 {
            return Err(invalid("CPU caustic photon escaped its certified closed medium"));
        }
        return Ok(true);
    };
    let distance = path.from.dist(endpoint);
    if let Some(index) = current(path)? {
        let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU caustic absorbing medium missing"))?;
        if let Some(scattering) = input.subsurface.filter(|scattering| scattering.active()) {
            let travel = scattering.free_flight(map.random(path.depth, 8)[0]).map_err(model_error)?;
            if travel < distance {
                if path.smooth_source.is_some() {
                    return Ok(true);
                }
                let position = path.from + path.ray.direction() * travel;
                if position == path.from || position == endpoint {
                    return Err(invalid("CPU caustic scattering collision is not representable"));
                }
                for plane in [path.source_plane, geometry.as_ref().filter(|_| emitter.is_none()).map(|hit| hit.vertices)].into_iter().flatten() {
                    if super::super::glass_mesh::origin_on_plane(plane, position)? {
                        return Err(invalid("CPU caustic scattering collision rounds onto an interface"));
                    }
                }
                if let Some((index, _, _)) = emitter {
                    let light = task
                        .options
                        .area_lights
                        .as_ref()
                        .and_then(|lights| lights.get(index))
                        .ok_or_else(|| invalid("CPU caustic analytic emitter missing"))?;
                    if super::super::indirect::Rectangle::new(light).map_err(error)?.on_plane(position) {
                        return Err(invalid("CPU caustic collision rounds onto an analytic emitter plane"));
                    }
                }
                path.flux = multiply(path.flux, dielectric::attenuation(input.absorption_per_mm, travel).map_err(error)?);
                candidate(map, position, Vec3::ZERO, -path.ray.direction(), input.body_index, ReceiverKind::Volume, 0, path)?;
                if path.depth + 1 >= map.options.max_depth {
                    return Ok(true);
                }
                let direction = scattering.sample_direction(path.ray.direction(), map.random(path.depth, 9)).map_err(model_error)?;
                path.ray = Ray::new(position, direction, 0.0_f64.next_up(), super::super::MAX_DISTANCE_MM).map_err(error)?;
                path.from = position;
                path.cursor = Some(TraceCursor::new(path.ray, false));
                path.hit = None;
                path.emitter = None;
                path.emitter_cursor = 0;
                path.source_plane = None;
                path.depth += 1;
                path.optical = true;
                path.origin = Some(super::super::origin::Task::new(position, position)?);
                path.verify_origin = true;
                map.stage = Stage::Origin;
                return Ok(false);
            }
        }
        path.flux = multiply(path.flux, dielectric::attenuation(input.absorption_per_mm, distance).map_err(error)?);
    }
    if emitter.is_some() {
        return Ok(true);
    }
    let hit = geometry.ok_or_else(|| invalid("CPU caustic geometric endpoint disappeared"))?;
    let raw = path.hit.ok_or_else(|| invalid("CPU caustic raw endpoint disappeared"))?;
    let selected = task.options.dielectrics.iter().position(|input| input.body_index == hit.body_index && input.body_name == hit.body_name);
    let Some(index) = selected else {
        candidate(map, hit.position, hit.oriented_geometric_normal(), -path.ray.direction(), hit.body_index, ReceiverKind::Opaque, 0, path)?;
        return Ok(true);
    };
    let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU caustic optical material missing"))?;
    if path.smooth_source.is_some() && !smooth_interface(input) {
        return Ok(true);
    }
    let model = input.model.unwrap_or_default();
    let thin = input.boundary == job::DielectricBoundary::ThinSheet;
    if model.roughness > 0.0 {
        let side = if thin { if hit.front_face { 1 } else { -1 } } else { 0 };
        candidate(map, hit.position, hit.geometric_normal, -path.ray.direction(), hit.body_index, ReceiverKind::Rough, side, path)?;
    }
    if path.depth + 1 >= map.options.max_depth {
        return Ok(true);
    }
    if !thin || model.roughness > 0.0 {
        select_spectrum(map, task, path, index)?;
    }
    let medium = current(path)?;
    let region = task.glass.first().ok_or_else(|| invalid("CPU caustic glass certificate missing"))?.region(index)?;
    let next = if thin {
        if medium.is_some() {
            return Err(invalid("CPU caustic sheet lies inside a closed medium"));
        }
        Some(index)
    } else if hit.front_face {
        if medium != region.parent {
            return Err(invalid("CPU caustic entry disagrees with its certified parent"));
        }
        Some(index)
    } else {
        if medium != Some(index) {
            return Err(invalid("CPU caustic exit disagrees with the current medium"));
        }
        region.parent
    };
    let ni = ior(task, path, medium)?;
    let nt = ior(task, path, next)?;
    let geometric = hit.oriented_geometric_normal();
    // The opt-in map uses the same reciprocal physical facet normal as its
    // camera-side closures, including plain interfaces elsewhere in the job.
    let normal = geometric;
    if thin && model.roughness == 0.0 {
        let (r, t) = smooth_sheet(input, path, normal)?;
        let reflected = (r.x + r.y + r.z) / 3.0;
        let transmitted = (t.x + t.y + t.z) / 3.0;
        if reflected + transmitted == 0.0 {
            return Ok(true);
        }
        let probability = reflected / (reflected + transmitted);
        let reflection = map.random(path.depth, 2)[0] < probability;
        let direction = if reflection {
            path.flux = multiply(path.flux, r / probability);
            dielectric::interface(path.ray.direction(), normal, ni, nt).map_err(error)?.reflected
        } else {
            path.flux = multiply(path.flux, t / (1.0 - probability));
            path.ray.direction()
        };
        path.optical |= reflection || !input.shadow_transmission;
        launch(task, path, index, hit, direction)?;
        map.stage = Stage::Trace;
        return Ok(false);
    }
    let Some(sample) = model.sample(path.ray.direction(), normal, ni, nt, wavelength(path)?, sample_uniforms(map, path)).map_err(model_error)? else {
        return Ok(true);
    };
    if (sample.crosses && sample.direction.dot(geometric) >= 0.0) || (!sample.crosses && sample.direction.dot(geometric) <= 0.0) {
        return Ok(true);
    }
    // Shared samples transport radiance; photons transport adjoint power.
    let correction =
        crate::glass_model::importance_normal_correction(path.ray.direction(), sample.direction, normal, geometric).map_err(model_error)?;
    path.flux = path.flux * (sample.weight * correction * if sample.crosses { (nt / ni).powi(2) } else { 1.0 });
    finite_flux(path.flux)?;
    path.optical = true;
    if thin && sample.crosses {
        path.sheet = Some(Sheet { index, normal, geometric, direction: sample.direction, ray: path.ray, hit: raw });
        path.depth += 1;
        map.stage = Stage::Sheet;
        return Ok(false);
    }
    if !thin && sample.crosses {
        if hit.front_face {
            *path.media.get_mut(path.len).ok_or_else(|| invalid("CPU caustic stack exceeds 16"))? = index;
            path.len += 1;
        } else {
            path.len = path.len.checked_sub(1).ok_or_else(|| invalid("CPU caustic stack underflow"))?;
        }
    }
    launch(task, path, index, hit, sample.direction)?;
    map.stage = Stage::Trace;
    Ok(false)
}
fn sheet(map: &mut Task, task: &TransportContext<'_>, path: &mut Path) -> job::Result<bool> {
    let sheet = path.sheet.ok_or_else(|| invalid("CPU caustic rough sheet continuation missing"))?;
    let input = task.options.dielectrics.get(sheet.index).ok_or_else(|| invalid("CPU caustic rough sheet material missing"))?;
    let model = input.model.unwrap_or_default();
    let cosine = sheet.direction.dot(sheet.normal).abs();
    let distance = if input.thickness_mm == 0.0 {
        0.0
    } else if cosine > 0.0 {
        input.thickness_mm / cosine
    } else {
        return Err(invalid("CPU caustic rough sheet has no finite grazing distance"));
    };
    path.flux = multiply(path.flux, dielectric::attenuation(input.absorption_per_mm, distance).map_err(error)?);
    let normal = if sheet.direction.dot(sheet.normal) < 0.0 { sheet.normal } else { -sheet.normal };
    let geometric = if sheet.direction.dot(sheet.normal) < 0.0 { sheet.geometric } else { -sheet.geometric };
    let hit = prepared(task)?.resolve_hit(sheet.ray, sheet.hit).map_err(error)?;
    let side = if (-geometric).dot(hit.geometric_normal) > 0.0 { 1 } else { -1 };
    candidate(map, hit.position, hit.geometric_normal, -sheet.direction, hit.body_index, ReceiverKind::Rough, side, path)?;
    if path.depth + 1 >= map.options.max_depth {
        return Ok(true);
    }
    let ni = ior(task, path, Some(sheet.index))?;
    let Some(sample) = model.sample(sheet.direction, normal, ni, 1.0, wavelength(path)?, sample_uniforms(map, path)).map_err(model_error)? else {
        return Ok(true);
    };
    if (sample.crosses && sample.direction.dot(geometric) >= 0.0) || (!sample.crosses && sample.direction.dot(geometric) <= 0.0) {
        return Ok(true);
    }
    let correction = crate::glass_model::importance_normal_correction(sheet.direction, sample.direction, normal, geometric).map_err(model_error)?;
    path.flux = path.flux * (sample.weight * correction * if sample.crosses { ni * ni } else { 1.0 });
    finite_flux(path.flux)?;
    if sample.crosses {
        if sample.direction.dot(sheet.normal) * sample.direction.dot(sheet.geometric) <= 0.0 {
            return Ok(true);
        }
        path.sheet = None;
        launch(task, path, sheet.index, hit, sample.direction)?;
        map.stage = Stage::Trace;
    } else {
        path.sheet = Some(Sheet { direction: sample.direction, ..sheet });
        path.depth += 1;
    }
    Ok(false)
}
fn sort(map: &mut Task) -> job::Result<()> {
    if map.width >= map.photons.len() {
        if !map.options.refresh_per_pass {
            map.scratch = Vec::new();
        }
        map.stage = Stage::Ready;
        return Ok(());
    }
    if map.out == map.begin {
        map.left = map.begin;
        map.right = (map.begin + map.width).min(map.photons.len());
        map.end = (map.right + map.width).min(map.photons.len());
    }
    let middle = (map.begin + map.width).min(map.photons.len());
    let left = map.left < middle && (map.right >= map.end || map.photons.get(map.left).map(|p| p.cell) <= map.photons.get(map.right).map(|p| p.cell));
    let index = if left {
        let index = map.left;
        map.left += 1;
        index
    } else {
        let index = map.right;
        map.right += 1;
        index
    };
    *map.scratch.get_mut(map.out).ok_or_else(|| invalid("CPU caustic merge output"))? =
        *map.photons.get(index).ok_or_else(|| invalid("CPU caustic merge input"))?;
    map.out += 1;
    if map.out == map.end {
        map.begin = map.end;
        if map.begin == map.photons.len() {
            std::mem::swap(&mut map.photons, &mut map.scratch);
            map.width = map.width.checked_mul(2).ok_or_else(|| error(TraceError::MemoryLimit))?;
            map.begin = 0;
            map.out = 0;
        }
    }
    Ok(())
}
pub(super) fn step(task: &mut ImageTask) -> job::Result<()> {
    let mut maps = std::mem::take(&mut task.caustics);
    let pass = task.progressive.as_ref().map_or(0, |p| p.film.samples());
    let result = {
        let mut context = TransportContext::from_image(task);
        maps.last_mut()
            .ok_or_else(|| invalid("CPU caustic task missing"))
            .and_then(|map| if map.refresh_due(pass) { map.refresh(pass, context.work, context.cancel) } else { advance(map, &mut context) })
    };
    task.caustics = maps;
    result
}

fn advance(map: &mut Task, task: &mut TransportContext<'_>) -> job::Result<()> {
    if map.stage == Stage::Trace {
        let path = map.path.as_mut().ok_or_else(|| invalid("CPU caustic path missing"))?;
        let complete = path
            .cursor
            .as_mut()
            .ok_or_else(|| invalid("CPU caustic trace cursor missing"))?
            .step_ordered(task.bvh.as_ref().ok_or_else(|| invalid("CPU caustic BVH missing"))?, task.work, task.cancel, |new, old| {
                let optical = |hit: super::super::Hit| {
                    task.looks
                        .get(hit.primitive_index)
                        .is_some_and(|look| task.options.dielectrics.iter().any(|input| input.body_index == look.body_index))
                };
                match (optical(new), optical(old)) {
                    (false, true) => true,
                    (true, false) => false,
                    _ => new.primitive_index < old.primitive_index,
                }
            })
            .map_err(error)?;
        if let Some(hit) = complete {
            path.hit = hit;
            path.cursor = None;
            path.emitter = None;
            path.emitter_cursor = 0;
            map.stage = Stage::Emitters;
        }
        return Ok(());
    }
    if map.stage == Stage::Origin && map.path.as_ref().and_then(|path| path.origin.as_ref()).is_some_and(|origin| !origin.ready()) {
        let scene =
            PreparedScene { bvh: task.bvh.as_ref().ok_or_else(|| invalid("CPU caustic BVH missing"))?, primitives: task.looks, source: task.scene };
        map.path.as_mut().and_then(|path| path.origin.as_mut()).ok_or_else(|| invalid("CPU caustic origin classifier missing"))?.step(
            task.glass.first().ok_or_else(|| invalid("CPU caustic glass certificate missing"))?,
            &task.options.dielectrics,
            scene,
            task.work,
            task.cancel,
        )?;
        return Ok(());
    }
    task.work.tick(task.cancel).map_err(error)?;
    match map.stage {
        Stage::GuideBounds => prepare_guide(map, task),
        Stage::SourcePowers => prepare_source_powers(map, task),
        Stage::Emit => emit(map, task),
        Stage::Emitters => {
            let path = map.path.as_mut().ok_or_else(|| invalid("CPU caustic emitter path missing"))?;
            if let Some(input) = task.options.area_lights.as_ref().and_then(|lights| lights.get(path.emitter_cursor)) {
                let source_launch = path.depth == 0 && path.smooth_source == Some(SmoothSource::Rectangle(path.emitter_cursor));
                if !source_launch {
                    let light = super::super::indirect::Rectangle::new(input).map_err(error)?;
                    let intersection = if path.smooth_source.is_some() {
                        // Analytic geometry beyond the environment launch disk
                        // can block the ray even before its first mesh trace.
                        let before_origin = path.depth == 0 && path.smooth_source == Some(SmoothSource::Environment);
                        light.intersect_reverse(path.ray, before_origin)
                    } else {
                        light.intersect(path.ray)
                    };
                    if let Some((distance, position)) = intersection.map_err(error)?
                        && path.hit.is_none_or(|hit| distance < hit.geometry.distance)
                        && path.emitter.is_none_or(|(_, prior, _)| distance < prior)
                    {
                        path.emitter = Some((path.emitter_cursor, distance, position));
                    }
                }
                path.emitter_cursor += 1;
            } else {
                map.stage = Stage::Resolve;
            }
            Ok(())
        }
        Stage::Origin => {
            let path = map.path.as_mut().ok_or_else(|| invalid("CPU caustic path missing"))?;
            let (media, len, _) = path.origin.take().ok_or_else(|| invalid("CPU caustic origin missing"))?.result()?;
            if path.verify_origin && (len != path.len || media.get(..len) != path.media.get(..path.len)) {
                return Err(invalid("CPU caustic collision left its certified medium"));
            }
            path.media = media;
            path.len = len;
            path.verify_origin = false;
            map.stage = Stage::Trace;
            Ok(())
        }
        Stage::Resolve | Stage::Sheet => {
            let mut path = map.path.take().ok_or_else(|| invalid("CPU caustic path missing"))?;
            let done = if map.stage == Stage::Sheet { sheet(map, task, &mut path)? } else { resolve(map, task, &mut path)? };
            let done = done || path.flux == Vec3::ZERO || path.depth >= map.options.max_depth;
            map.path = Some(path);
            if done {
                map.finish_path()?;
            }
            Ok(())
        }
        Stage::Sort => sort(map),
        Stage::Trace | Stage::Ready => Err(invalid("CPU caustic stage bypassed its continuation")),
    }
}

#[derive(Default)]
pub(super) struct Gather {
    cell: Option<[i64; 3]>,
    neighbor: u8,
    index: usize,
    end: usize,
}
fn next(task: &TransportContext<'_>, cursor: &mut Gather, position: Vec3) -> job::Result<Option<Option<(Photon, f64)>>> {
    let map = task.caustics.first().filter(|map| map.ready()).ok_or_else(|| invalid("CPU caustic map is incomplete"))?;
    let center = if let Some(center) = cursor.cell {
        center
    } else {
        let center = cell(position, map.options.radius_mm)?;
        cursor.cell = Some(center);
        center
    };
    if cursor.index < cursor.end {
        let photon = *map.photons.get(cursor.index).ok_or_else(|| invalid("CPU caustic gather index"))?;
        cursor.index += 1;
        return Ok(Some(Some((photon, map.options.radius_mm))));
    }
    if cursor.neighbor == 27 {
        return Ok(None);
    }
    let n = i64::from(cursor.neighbor);
    let key = [center[0] + n % 3 - 1, center[1] + (n / 3) % 3 - 1, center[2] + n / 9 - 1];
    cursor.neighbor += 1;
    cursor.index = map.photons.partition_point(|photon| photon.cell < key);
    cursor.end = map.photons.partition_point(|photon| photon.cell <= key);
    Ok(Some(None))
}
fn source_allows(task: &TransportContext<'_>, photon: Photon, position: Vec3) -> job::Result<bool> {
    if photon.source == u32::MAX {
        return Ok(true);
    }
    let input = task
        .options
        .punctual_lights
        .as_ref()
        .and_then(|lights| lights.get(photon.source as usize))
        .ok_or_else(|| invalid("CPU caustic receiver source identity disappeared"))?;
    Ok(position.dist(Vec3::new(input.position_mm[0], input.position_mm[1], input.position_mm[2])) >= input.minimum_distance_mm)
}
pub(super) fn gather(
    task: &TransportContext<'_>,
    cursor: &mut Gather,
    hit: SharedHit<'_>,
    view: Vec3,
    camera_remaining: u8,
) -> job::Result<Option<Vec3>> {
    let Some(visit) = next(task, cursor, hit.position)? else { return Ok(None) };
    let Some((photon, radius)) = visit else { return Ok(Some(Vec3::ZERO)) };
    let geometric = hit.oriented_geometric_normal();
    if photon.kind != ReceiverKind::Opaque
        || photon.body != hit.body_index
        // The camera needs a receiver, every glass event, and the emitter endpoint.
        || (photon.smooth_chain_events != 0 && photon.smooth_chain_events + 1 >= camera_remaining)
        || !source_allows(task, photon, hit.position)?
        || photon.position.dist(hit.position) > radius
        || (photon.position - hit.position).dot(geometric).abs() > radius * 0.1
        || photon.normal.dot(geometric) <= 0.0
        || geometric.dot(photon.incoming) <= 0.0
        || hit.shading_normal.dot(photon.incoming) <= 0.0
    {
        return Ok(Some(Vec3::ZERO));
    }
    let brdf = hit.surface.brdf(hit.shading_normal, view, photon.incoming).map_err(|_| invalid("CPU caustic receiver BRDF is invalid"))?;
    let measure = crate::glass_model::surface_normal_measure(photon.incoming, hit.shading_normal, geometric).map_err(model_error)?;
    Ok(Some(multiply(brdf, photon.flux) * (measure / (PI * radius.powi(2)))))
}

#[derive(Clone, Copy)]
pub(super) struct RoughReceiver {
    pub position: Vec3,
    pub canonical: Vec3,
    pub geometric: Vec3,
    pub normal: Vec3,
    pub incident: Vec3,
    pub body: usize,
    pub side: i8,
    pub model: crate::glass_model::GlassModel,
    pub ni: f64,
    pub nt: f64,
    pub wavelength_nm: f64,
}
pub(super) fn gather_rough(task: &TransportContext<'_>, cursor: &mut Gather, receiver: RoughReceiver) -> job::Result<Option<Vec3>> {
    let Some(visit) = next(task, cursor, receiver.position)? else { return Ok(None) };
    let Some((photon, radius)) = visit else { return Ok(Some(Vec3::ZERO)) };
    if photon.kind != ReceiverKind::Rough
        || photon.body != receiver.body
        || photon.side != receiver.side
        || !source_allows(task, photon, receiver.position)?
        || photon.position.dist(receiver.position) > radius
        || (photon.position - receiver.position).dot(receiver.canonical).abs() > radius * 0.1
        || photon.normal.dot(receiver.canonical) <= 0.0
        || photon.incoming.dot(receiver.normal) * photon.incoming.dot(receiver.geometric) <= 0.0
    {
        return Ok(Some(Vec3::ZERO));
    }
    let value = receiver
        .model
        .evaluate(receiver.incident, photon.incoming, receiver.normal, receiver.ni, receiver.nt, receiver.wavelength_nm)
        .map_err(model_error)?
        .value;
    let measure = crate::glass_model::surface_normal_measure(photon.incoming, receiver.normal, receiver.geometric).map_err(model_error)?;
    Ok(Some(photon.flux * (value * measure / (PI * radius.powi(2)))))
}
pub(super) fn gather_volume(
    task: &TransportContext<'_>,
    cursor: &mut Gather,
    position: Vec3,
    body: usize,
    incident: Vec3,
    scattering: crate::transmission::subsurface::Subsurface,
) -> job::Result<Option<Vec3>> {
    let Some(visit) = next(task, cursor, position)? else { return Ok(None) };
    let Some((photon, radius)) = visit else { return Ok(Some(Vec3::ZERO)) };
    if photon.kind != ReceiverKind::Volume
        || photon.body != body
        || photon.position.dist(position) > radius
        || !source_allows(task, photon, position)?
    {
        return Ok(Some(Vec3::ZERO));
    }
    let phase = scattering.phase(incident.dot(photon.incoming).clamp(-1.0, 1.0)).map_err(model_error)?;
    let kernel = crate::glass_model::volume_photon_kernel(radius, scattering.scattering_per_mm).map_err(model_error)?;
    Ok(Some(photon.flux * (phase * kernel)))
}
