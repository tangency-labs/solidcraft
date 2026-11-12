//! Native bounded camera tiles and photon batches. The pool is owned by one
//! image task and reused across passes; immutable prepared inputs are borrowed.
//! Completed work is merged in ordinal order, independently of worker scheduling.
use super::*;
use rayon::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const TILE_PIXELS: usize = 64;
const WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;
const POOL_OVERHEAD_BYTES: u64 = 64 * 1024;

pub(super) fn worker_count(options: &RaytraceOptions) -> u16 {
    match options.threads {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()).min(usize::from(job::MAX_CPU_THREADS)) as u16,
        count => count,
    }
}

pub(super) fn admitted_workers(admission: &Admission) -> job::Result<usize> {
    admission
        .cpu_workers
        .filter(|count| (1..=job::MAX_CPU_THREADS).contains(count))
        .map(usize::from)
        .ok_or_else(|| invalid("CPU worker admission is missing or invalid"))
}

#[derive(Clone, Copy)]
struct CameraResult {
    pixel: direct::Pixel,
    skipped: bool,
}

struct Worker {
    paths: Vec<indirect_image::State>,
    direct: Vec<direct::State>,
    photons: Vec<caustics::Task>,
    maps: Vec<job::textures::cpu_cache::CpuTextureCache>,
    aging: Vec<super::super::aging::Task>,
    results: Vec<CameraResult>,
    diagnostics: job::PunctualDiagnostics,
    work: WorkBudget,
    failure: Option<JobError>,
}

fn reserve<T>(length: usize) -> job::Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(|_| error(TraceError::AllocationFailed))?;
    if values.capacity() != length {
        return Err(error(TraceError::MemoryLimit));
    }
    Ok(values)
}

#[derive(Clone, Copy)]
struct Source<'a> {
    caustics: &'a [caustics::Task],
    progressive: Option<&'a Progressive>,
    glass: &'a [super::super::glass_mesh::Task],
    environment: Option<&'a crate::lighting::environment_input::EnvironmentState>,
    scene: &'a ProductScene,
    request: &'a ImageRequest,
    options: &'a RaytraceOptions,
    cancel: &'a CancelToken,
    lights: &'a Lighting,
    emissions: &'a [(usize, super::super::emission::Emission)],
    effects: &'a [(usize, crate::surface_patterns::PreparedEffect)],
    clip: (f64, f64),
    looks: &'a [Look],
    bvh: Option<&'a Bvh>,
    certificates: Option<&'a [Option<Certificate>; MAX_OVERRIDES]>,
    anisotropic_targets: &'a [Option<PlanarTarget>; 2],
    preserves_display: bool,
}
impl<'a> Source<'a> {
    fn new(task: &'a ImageTask) -> Self {
        Self {
            caustics: &task.caustics,
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
            clip: task.clip,
            looks: &task.looks,
            bvh: task.bvh.as_ref(),
            certificates: task.optical.first().map(|optical| &optical.certificates),
            anisotropic_targets: &task.anisotropic_targets,
            preserves_display: task.preserves_display(),
        }
    }
    fn context<'b>(&'b self, worker: &'b mut Worker, pixel: usize) -> TransportContext<'b> {
        TransportContext {
            maps: &mut worker.maps,
            aging: &mut worker.aging,
            caustics: self.caustics,
            punctual_diagnostics: &mut worker.diagnostics,
            progressive: self.progressive,
            glass: self.glass,
            environment: self.environment,
            scene: self.scene,
            request: self.request,
            options: self.options,
            cancel: self.cancel,
            lights: self.lights,
            emissions: self.emissions,
            effects: self.effects,
            work: &mut worker.work,
            clip: self.clip,
            pupil_sample: None,
            looks: self.looks,
            bvh: self.bvh,
            pixel,
            spatial: 0,
            primary_coverage: 0,
            completed: None,
        }
    }
}

impl Worker {
    fn new(task: &ImageTask) -> job::Result<Self> {
        task.cancel.check()?;
        let mut paths = reserve(usize::from(task.options.path.is_some()))?;
        if task.options.path.is_some() {
            paths.push(indirect_image::State::default());
        }
        let mut direct = reserve(usize::from(task.options.path.is_none()))?;
        if task.options.path.is_none() {
            direct.push(direct::State::new(&task.cancel)?);
        }
        let mut photons = reserve(usize::from(task.options.caustics.is_some()))?;
        if let Some(options) = task.options.caustics {
            photons.push(caustics::Task::worker(options, TILE_PIXELS)?);
        }
        let mut maps = reserve(task.maps.len())?;
        for cache in &task.maps {
            maps.push(cache.fork_ready()?);
        }
        let mut aging = reserve(task.aging.len())?;
        for _ in &task.aging {
            aging.push(super::super::aging::Task::default());
        }
        Ok(Self {
            paths,
            direct,
            photons,
            maps,
            aging,
            results: reserve(TILE_PIXELS)?,
            diagnostics: Default::default(),
            work: WorkBudget::new(job::MAX_CPU_WORK).map_err(error)?,
            failure: None,
        })
    }
    fn reset(&mut self, remaining: u64, ledger: &Arc<AtomicU64>) {
        self.work.parallel_reset(remaining, ledger);
        self.results.clear();
        self.diagnostics = Default::default();
        self.failure = None;
    }
    fn camera(&mut self, source: &Source<'_>, begin: usize, end: usize) -> job::Result<()> {
        let mut paths = std::mem::take(&mut self.paths);
        let mut direct = std::mem::take(&mut self.direct);
        let result = (|| {
            for pixel in begin..end {
                source.cancel.check()?;
                let skipped = match source.progressive {
                    Some(progressive) => progressive.film.identity(pixel, progressive.options.seed)?.is_none(),
                    None => false,
                };
                let output = if skipped {
                    direct::Pixel { pixel, radiance: Vec3::ZERO, covered: 0, attempts: 1, rgba: None }
                } else if source.options.path.is_some() {
                    let state = paths.first_mut().ok_or_else(|| invalid("CPU worker path state missing"))?;
                    let mut context = source.context(self, pixel);
                    while context.completed.is_none() {
                        context.cancel.check()?;
                        indirect_image::advance_or_commit(&mut context, state)?;
                    }
                    direct::Pixel {
                        pixel,
                        radiance: context.completed.ok_or_else(|| invalid("CPU worker camera result missing"))?,
                        covered: context.primary_coverage,
                        attempts: 1,
                        rgba: None,
                    }
                } else {
                    let state = direct.first_mut().ok_or_else(|| invalid("CPU worker direct state missing"))?;
                    let common = source.context(self, pixel);
                    let mut context =
                        direct::Context::worker(common, state, source.certificates, source.anisotropic_targets, source.preserves_display)?;
                    while context.completed().is_none() {
                        source.cancel.check()?;
                        context.pixel_one()?;
                    }
                    context.finish_worker()?
                };
                if self.results.len() == self.results.capacity() {
                    return Err(error(TraceError::MemoryLimit));
                }
                self.results.push(CameraResult { pixel: output, skipped });
            }
            Ok(())
        })();
        self.paths = paths;
        self.direct = direct;
        result
    }
    fn photons(&mut self, source: &Source<'_>, map: &caustics::Task, begin: u32, end: u32) -> job::Result<()> {
        let mut slots = std::mem::take(&mut self.photons);
        let result = (|| {
            let worker = slots.first_mut().ok_or_else(|| invalid("CPU worker photon state missing"))?;
            worker.begin_batch(map, begin)?;
            let mut context = source.context(self, 0);
            worker.trace_batch(&mut context, end)
        })();
        self.photons = slots;
        result
    }
}

pub(super) struct Pool {
    pool: rayon::ThreadPool,
    workers: Vec<Worker>,
    ledger: Arc<AtomicU64>,
}

impl Pool {
    pub(super) fn required_bytes(options: &RaytraceOptions, textured: bool, count: u16) -> job::Result<u64> {
        if !(1..=job::MAX_CPU_THREADS).contains(&count) {
            return Err(invalid("CPU worker count exceeds its admission range"));
        }
        if count <= 1 {
            return Ok(0);
        }
        let photon = if options.caustics.is_some() { caustics::Task::worker_bytes(TILE_PIXELS)? } else { 0 };
        let continuation = if options.path.is_some() {
            std::mem::size_of::<indirect_image::State>() as u64 + super::super::origin::Task::required_bytes()
        } else {
            direct::State::required_bytes()
        };
        let per_worker = WORKER_STACK_BYTES as u64
            + POOL_OVERHEAD_BYTES
            + std::mem::size_of::<Worker>() as u64
            + continuation
            + photon
            + if textured { std::mem::size_of::<job::textures::cpu_cache::CpuTextureCache>() as u64 } else { 0 }
            + super::super::aging::Task::required_bytes()
            + TILE_PIXELS as u64 * std::mem::size_of::<CameraResult>() as u64;
        per_worker
            .checked_mul(count as u64)
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>() as u64 + 64))
            .ok_or_else(|| error(TraceError::MemoryLimit))
    }
    fn new(task: &ImageTask) -> job::Result<Self> {
        task.cancel.check()?;
        let count = admitted_workers(&task.admission)?;
        if count <= 1 || task.bvh.is_none() {
            return Err(invalid("CPU pool requires prepared parallel transport"));
        }
        let mut workers = reserve(count)?;
        for _ in 0..count {
            workers.push(Worker::new(task)?);
        }
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(count)
            .stack_size(WORKER_STACK_BYTES)
            .thread_name(|index| format!("solidcraft-ray-{index}"))
            .build()
            .map_err(|error| JobError::new(ErrorCode::AllocationFailed, format!("could not start CPU ray workers: {error}")))?;
        task.cancel.check()?;
        Ok(Self { pool, workers, ledger: Arc::new(AtomicU64::new(0)) })
    }
    #[cfg(test)]
    pub(super) fn test_worker_counts(&self) -> (usize, usize) {
        (self.workers.len(), self.pool.current_num_threads())
    }

    fn prepare(&mut self, task: &ImageTask) -> job::Result<u64> {
        task.cancel.check()?;
        let remaining = task.work.remaining();
        if remaining == 0 {
            return Err(error(TraceError::WorkLimit));
        }
        self.ledger.store(remaining, Ordering::Relaxed);
        for worker in &mut self.workers {
            worker.reset(remaining, &self.ledger);
        }
        Ok(remaining)
    }
    fn debit(&mut self, task: &mut ImageTask, remaining: u64) -> job::Result<()> {
        let consumed = remaining.checked_sub(self.ledger.load(Ordering::Relaxed)).ok_or_else(|| invalid("CPU worker ledger increased"))?;
        task.work.charge(consumed, &task.cancel).map_err(error)?;
        for worker in &mut self.workers {
            if let Some(error) = worker.failure.take() {
                return Err(error);
            }
        }
        task.cancel.check()
    }
    fn camera(&mut self, task: &mut ImageTask) -> job::Result<()> {
        let remaining = self.prepare(task)?;
        let count = task.request.width as usize * task.request.height as usize;
        let begin = task.pixel;
        let source = Source::new(task);
        self.pool.install(|| {
            self.workers.par_iter_mut().enumerate().for_each(|(index, worker)| {
                let start = begin.saturating_add(index * TILE_PIXELS).min(count);
                let end = start.saturating_add(TILE_PIXELS).min(count);
                worker.failure = worker.camera(&source, start, end).err();
            })
        });
        self.debit(task, remaining)?;
        for worker in &mut self.workers {
            task.punctual_diagnostics.near_source = task.punctual_diagnostics.near_source.saturating_add(worker.diagnostics.near_source);
            task.punctual_diagnostics.radiance_limit = task.punctual_diagnostics.radiance_limit.saturating_add(worker.diagnostics.radiance_limit);
            task.punctual_diagnostics.shadow_offset_crossed =
                task.punctual_diagnostics.shadow_offset_crossed.saturating_add(worker.diagnostics.shadow_offset_crossed);
            for result in &worker.results {
                if task.pixel != result.pixel.pixel {
                    return Err(invalid("CPU worker results are not in camera order"));
                }
                if result.skipped {
                    task.progressive.as_mut().ok_or_else(|| invalid("CPU adaptive worker film missing"))?.film.skip(
                        task.pixel,
                        &task.cancel,
                        &mut || task.work.tick(&task.cancel).map_err(error),
                    )?;
                    task.pixel += 1;
                } else if task.progressive.is_some() {
                    if result.pixel.attempts != 1 || result.pixel.rgba.is_some() {
                        return Err(invalid("CPU progressive worker returned a fixed-sample pixel"));
                    }
                    task.primary_coverage = result.pixel.covered;
                    task.finish_sample(result.pixel.radiance)?;
                } else {
                    direct::publish(task, result.pixel)?;
                    task.pixel += 1;
                }
            }
        }
        Ok(())
    }
    fn photons(&mut self, task: &mut ImageTask) -> job::Result<()> {
        let remaining = self.prepare(task)?;
        let mut maps = std::mem::take(&mut task.caustics);
        let result = (|| {
            let map = maps.first_mut().ok_or_else(|| invalid("CPU parallel photon map missing"))?;
            let (begin, limit) = map.batch_bounds()?;
            let source = Source::new(task);
            self.pool.install(|| {
                self.workers.par_iter_mut().enumerate().for_each(|(index, worker)| {
                    let start = begin.saturating_add((index * TILE_PIXELS) as u32).min(limit);
                    let end = start.saturating_add(TILE_PIXELS as u32).min(limit);
                    worker.failure = worker.photons(&source, map, start, end).err();
                })
            });
            self.debit(task, remaining)?;
            for worker in &mut self.workers {
                map.merge_batch(worker.photons.first_mut().ok_or_else(|| invalid("CPU completed photon batch missing"))?)?;
            }
            Ok(())
        })();
        task.caustics = maps;
        result
    }
}

fn with_pool(task: &mut ImageTask, run: impl FnOnce(&mut Pool, &mut ImageTask) -> job::Result<()>) -> job::Result<()> {
    let mut slots = std::mem::take(&mut task.parallel);
    let result = (|| {
        if slots.is_empty() {
            slots.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
            if slots.capacity() != 1 {
                return Err(error(TraceError::MemoryLimit));
            }
            slots.push(Pool::new(task)?);
        }
        run(slots.first_mut().ok_or_else(|| invalid("CPU worker pool disappeared"))?, task)
    })();
    task.parallel = slots;
    result
}

pub(super) fn camera(task: &mut ImageTask) -> job::Result<()> {
    with_pool(task, Pool::camera)
}

pub(super) fn photons(task: &mut ImageTask) -> job::Result<()> {
    with_pool(task, Pool::photons)
}
