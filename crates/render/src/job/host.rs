//! Caller-owned image execution host. One worker, two bounded queued inputs; browser pumping
//! uses the same incremental task without threads. Shared ownership never includes CAD state.

use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Condvar;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    Admission, AlphaConvention, CancelToken, ColourEncoding, ErrorCode, FrameInfo, ImageExecutor, ImageRequest, ImageTask, ImageWork, Integrator,
    JobError, JobId, JobOptions, Phase, Progress, Result, TaskPoll,
};
use crate::{
    Availability, ExecutorDescriptor, ExecutorId, ProductScene, RenderFormat, RenderLimits, RenderMode, RenderOperation, RenderProfile, ScratchLayout,
};

#[cfg(test)]
use super::admit;

const MEMORY_LIMIT: u64 = 512 * 1024 * 1024;
const QUEUE_LIMIT: usize = 2;

#[cfg(test)]
struct CpuExecutor;
#[cfg(test)]
impl ImageExecutor for CpuExecutor {
    fn descriptor(&self) -> Result<ExecutorDescriptor> {
        Ok(ExecutorDescriptor {
            id: ExecutorId::new("cpu-product").map_err(|e| JobError::new(ErrorCode::Internal, e.to_string()))?,
            availability: Availability::Available,
            profiles: vec![RenderProfile {
                mode: RenderMode::Preview,
                operation: RenderOperation::Image,
                formats: vec![RenderFormat::Png],
                features: vec![crate::RenderFeature::Pbr],
            }],
            limits: RenderLimits::default(),
            scratch: ScratchLayout { sample_scale: 2, sample_bytes_per_pixel: 8, output_bytes_per_pixel: 8, encoding_bytes_per_pixel: 8 },
        })
    }

    fn admit(&self, scene: &ProductScene, request: &ImageRequest, options: JobOptions) -> Result<Admission> {
        if options != JobOptions::default() {
            return Err(JobError::new(ErrorCode::InvalidRequest, "CPU product does not support sampling or exposure controls"));
        }
        admit(scene, request)
    }
    fn begin(&self, scene: ProductScene, request: ImageRequest, _options: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
        Ok(Box::new(ImageTask::new(scene, request, cancel)?))
    }
}

// Completion notification precedes destruction of the worker's temporary handle.
// Lease tests join their owned, idle worker before checking exact retained bytes.
#[cfg(all(test, feature = "raytrace-cpu", not(target_arch = "wasm32")))]
impl ImageHost {
    pub(super) fn join_worker_for_test(&mut self) -> Result<()> {
        self.0.shared.stop.store(true, Ordering::Release);
        self.0.shared.wake.notify_all();
        let inner = Arc::get_mut(&mut self.0).ok_or_else(|| JobError::new(ErrorCode::Internal, "test must own its host"))?;
        if let Some(worker) = inner._worker.take() {
            worker.join().map_err(|_| JobError::new(ErrorCode::Internal, "test worker panicked"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "host_regrade_tests.rs"]
mod regrade_tests;
#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    NativeWorker,
    Cooperative,
}

struct Budget {
    used: AtomicU64,
    limit: u64,
}
pub(crate) struct Reservation {
    budget: Arc<Budget>,
    bytes: u64,
}

impl Budget {
    fn reserve(self: &Arc<Self>, bytes: u64) -> Result<Reservation> {
        let mut used = self.used.load(Ordering::Acquire);
        loop {
            let next = used.checked_add(bytes).filter(|n| *n <= self.limit).ok_or_else(|| {
                JobError::new(ErrorCode::LimitExceeded, format!("image host admission pool is full ({} MiB)", self.limit / (1024 * 1024)))
            })?;
            match self.used.compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(current) => used = current,
            }
        }
        Ok(Reservation { budget: self.clone(), bytes })
    }
}
impl Reservation {
    /// Transfer an already admitted portion without changing the pool's total.
    /// CPU progressive admission prepays at most two reusable checkpoint slots.
    fn split(&mut self, bytes: u64) -> Result<Self> {
        if bytes > self.bytes {
            return Err(JobError::new(ErrorCode::LimitExceeded, "preview exceeds its prepaid reservation"));
        }
        self.bytes -= bytes;
        Ok(Self { budget: self.budget.clone(), bytes })
    }

    /// Return a lease to the same prepaid pool without reserving memory again.
    fn merge_from(&mut self, other: &mut Self) -> Result<()> {
        if !Arc::ptr_eq(&self.budget, &other.budget) {
            return Err(JobError::new(ErrorCode::Internal, "checkpoint lease belongs to another pool"));
        }
        self.bytes = self.bytes.checked_add(other.bytes).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "checkpoint lease overflow"))?;
        other.bytes = 0;
        Ok(())
    }

    pub(crate) fn shrink(&mut self, bytes: u64) -> Result<()> {
        if bytes > self.bytes {
            return Err(JobError::new(ErrorCode::LimitExceeded, "completed PNG exceeds its reservation"));
        }
        self.budget.used.fetch_sub(self.bytes - bytes, Ordering::AcqRel);
        self.bytes = bytes;
        Ok(())
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

/// Retained runtime input capacities survive work/PNG eviction through status handles.
fn retained_request_bytes(request: &ImageRequest) -> Result<u64> {
    request.raytrace.as_ref().map_or(Ok(0), |options| options.retained_bytes())
}

#[cfg(all(test, feature = "raytrace-cpu", not(target_arch = "wasm32")))]
mod light_union_lease_tests {
    use super::super::{DirectionalLightOptions, PunctualLightOptions, RaytraceOptions, led_strip::LedStripOptions};
    use super::*;
    fn request() -> Result<ImageRequest> {
        let mut options = RaytraceOptions {
            primary_samples: 16,
            lights: Some(vec![DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }; 8]),
            punctual_lights: Some(vec![
                PunctualLightOptions {
                    position_mm: [0.0, 0.0, 10.0],
                    color: [1.0; 3],
                    intensity: 1.0,
                    reference_distance_mm: 10.0,
                    minimum_distance_mm: 0.001,
                    cone: None
                };
                8
            ]),
            ..Default::default()
        };
        let strip = LedStripOptions {
            center_mm: [0.0, 0.0, 10.0],
            length_mm: 8.0,
            width_mm: 0.5,
            length_axis: [1.0, 0.0, 0.0],
            width_axis: [0.0, -1.0, 0.0],
            color: [1.0; 3],
            intensity: 2.0,
        };
        for _ in 0..4 {
            strip.append_to(&mut options, 64)?;
        }
        Ok(ImageRequest {
            studio_weights: None,
            environment: None,
            textures: None,
            integrator: Integrator::Raytrace,
            raytrace: Some(options),
            camera: Default::default(),
            width: 8,
            height: 8,
            background: [0; 3],
            transparent_background: false,
        })
    }
    fn scene() -> ProductScene {
        ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY }
    }
    #[test]
    fn normalized_union_uses_one_pool_and_keeps_record_lease_after_frame_eviction() {
        let mut host = CpuHost::for_platform().unwrap();
        let budget = host.0.shared.budget.clone();
        let request = request().unwrap();
        let bytes = retained_request_bytes(&request).unwrap() + request.retained_resource_metadata_bytes();
        let capacities = request
            .raytrace
            .as_ref()
            .map(|o| {
                (o.lights.as_ref().unwrap().capacity(), o.punctual_lights.as_ref().unwrap().capacity(), o.area_lights.as_ref().unwrap().capacity())
            })
            .unwrap();
        let job = host.submit(JobId { session: 1724, serial: 1, generation: 1 }, 1, scene(), request).unwrap();
        assert_eq!(job.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
        // Synchronize this test's completed worker before asserting final Arc release;
        // publication wakes waiters just before Running drops its temporary handle.
        host.0.shared.stop.store(true, Ordering::Release);
        host.0.shared.wake.notify_all();
        Arc::get_mut(&mut host.0).unwrap()._worker.take().unwrap().join().unwrap();
        let retained = job.0.request.raytrace.as_ref().unwrap();
        assert_eq!(
            capacities,
            (
                retained.lights.as_ref().unwrap().capacity(),
                retained.punctual_lights.as_ref().unwrap().capacity(),
                retained.area_lights.as_ref().unwrap().capacity()
            )
        );
        assert!(Arc::ptr_eq(&job.0._request_reservation.as_ref().unwrap().budget, &budget));
        assert_eq!(job.0._request_reservation.as_ref().unwrap().bytes, bytes);
        let frame = job.frame().unwrap().unwrap();
        job.0.data.lock().unwrap().frame = None;
        assert!(host.admitted_bytes() > bytes);
        drop(frame);
        assert_eq!(host.admitted_bytes(), bytes);
        let held = job.clone();
        drop(job);
        assert_eq!(host.admitted_bytes(), bytes);
        drop(held);
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
    }
    #[test]
    fn failed_union_metadata_reservation_releases_work_and_does_not_enqueue() {
        let host = CpuHost::for_platform().unwrap();
        let request = request().unwrap();
        let work = admit(&scene(), &request).unwrap().estimated_total_bytes;
        let fill = host.0.shared.budget.reserve(host.pool_limit_bytes() - work).unwrap();
        let before = host.admitted_bytes();
        let result = host.submit(JobId { session: 1724, serial: 2, generation: 1 }, 1, scene(), request);
        assert_eq!(result.err().unwrap().code, ErrorCode::LimitExceeded);
        assert_eq!(host.admitted_bytes(), before);
        assert!(host.0.shared.queue.lock().unwrap().is_empty());
        drop(fill);
        assert_eq!(host.admitted_bytes(), 0);
    }
}

fn admit_request_sum(work_bytes: u64, request_bytes: u64) -> Result<()> {
    if work_bytes.checked_add(request_bytes).is_none_or(|sum| sum > super::image::MAX_JOB_BYTES) {
        return Err(JobError::new(ErrorCode::LimitExceeded, "render work and retained request exceed 384 MiB per-job admission"));
    }
    Ok(())
}

/// Borrow encoded bytes through the frame, so retaining the data also retains its admission lease.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinearColourEncoding {
    SceneLinearSrgbD65,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinearAlphaConvention {
    CoveragePremultiplied,
}
#[derive(Clone, Debug, Serialize)]
pub struct LinearMasterInfo {
    pub encoding: LinearColourEncoding,
    pub alpha: LinearAlphaConvention,
    pub bytes: u64,
    pub sha256: String,
}
struct RetainedPresentation {
    info: super::presentation::MasterPresentation,
    _reservation: Reservation,
}

pub struct RenderFrame {
    // Derivatives point directly at the original, never form a retention chain.
    source_master: Option<Arc<RenderFrame>>,
    presentation: Option<Arc<RetainedPresentation>>,
    pub info: FrameInfo,
    png: Vec<u8>,
    exr: Option<(Vec<u8>, LinearMasterInfo)>,
    _reservation: Reservation,
}
impl RenderFrame {
    pub fn master_source(self: &Arc<Self>) -> Result<Arc<Self>> {
        if let Some(source) = &self.source_master {
            return Ok(source.clone());
        }
        if self.exr.is_some() {
            return Ok(self.clone());
        }
        Err(JobError::new(ErrorCode::InvalidRequest, "completed image has no retained raw linear master"))
    }
    pub fn presentation(&self) -> Option<&super::presentation::MasterPresentation> {
        self.presentation.as_deref().map(|retained| &retained.info)
    }
    pub fn linear_exr(&self) -> Option<&[u8]> {
        self.exr.as_ref().map(|(bytes, _)| bytes.as_slice()).or_else(|| self.source_master.as_ref().and_then(|source| source.linear_exr()))
    }
    pub fn linear_master_info(&self) -> Option<&LinearMasterInfo> {
        self.exr.as_ref().map(|(_, info)| info).or_else(|| self.source_master.as_ref().and_then(|source| source.linear_master_info()))
    }
    pub fn png(&self) -> &[u8] {
        &self.png
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PreviewInfo {
    pub frame_sequence: u32,
    /// Uniform SPP when adaptive is absent; otherwise absolute completed iteration.
    pub samples_per_pixel: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<super::adaptive::AdaptivePreview>,
    pub frame: FrameInfo,
}

/// Immutable checkpoint bytes retain their own transferred host lease. Keeping
/// an older preview alive never makes its memory disappear from pool accounting.
pub struct RenderPreview {
    pub info: PreviewInfo,
    png: Vec<u8>,
    _reservation: CheckpointLease,
}

/// Two reusable, immutable complete-pass slots. No consumer acknowledgement or
/// queue: externally pinned slots are diagnosed and never stall final work.
struct CheckpointCredits {
    state: Mutex<CheckpointCreditState>,
    slot_bytes: u64,
    capacity: u64,
}
struct CheckpointCreditState {
    free: Reservation,
    closed: bool,
}
impl CheckpointCredits {
    fn new(reservation: &mut Reservation, slot_bytes: u64, slots: u32) -> Result<Arc<Self>> {
        if !(1..=2).contains(&slots) || slot_bytes == 0 {
            return Err(JobError::new(ErrorCode::InvalidRequest, "checkpoint credits require one or two finite slots"));
        }
        let capacity = slot_bytes
            .checked_mul(u64::from(slots))
            .and_then(|n| n.checked_add(FRAME_METADATA_BYTES))
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "checkpoint credit capacity overflow"))?;
        let free = reservation.split(capacity)?;
        Ok(Arc::new(Self { state: Mutex::new(CheckpointCreditState { free, closed: false }), slot_bytes, capacity }))
    }
    fn acquire(self: &Arc<Self>) -> Result<Option<CheckpointLease>> {
        let mut state = self.state.lock().map_err(|_| JobError::new(ErrorCode::Internal, "checkpoint credit lock poisoned"))?;
        if state.closed {
            return Err(JobError::new(ErrorCode::Cancelled, "checkpoint credits are closed"));
        }
        let needed = self
            .slot_bytes
            .checked_add(FRAME_METADATA_BYTES)
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "checkpoint credit bound overflow"))?;
        if state.free.bytes < needed {
            return Ok(None);
        }
        let reservation = state.free.split(self.slot_bytes)?;
        Ok(Some(CheckpointLease { reservation, credits: self.clone() }))
    }
    fn close(&self) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(error) => error.into_inner(),
        };
        state.closed = true;
        let _ = state.free.shrink(0);
    }
}
struct CheckpointLease {
    reservation: Reservation,
    credits: Arc<CheckpointCredits>,
}
impl std::ops::Deref for CheckpointLease {
    type Target = Reservation;
    fn deref(&self) -> &Reservation {
        &self.reservation
    }
}
impl Drop for CheckpointLease {
    fn drop(&mut self) {
        let mut state = match self.credits.state.lock() {
            Ok(state) => state,
            Err(error) => {
                let mut state = error.into_inner();
                state.closed = true;
                let _ = state.free.shrink(0);
                state
            }
        };
        if !state.closed {
            if self.reservation.bytes <= self.credits.slot_bytes
                && state.free.bytes.checked_add(self.reservation.bytes).is_some_and(|n| n <= self.credits.capacity)
                && state.free.merge_from(&mut self.reservation).is_ok()
            {
                return;
            }
            state.closed = true;
            let _ = state.free.shrink(0);
        }
    }
}

impl RenderPreview {
    pub fn png(&self) -> &[u8] {
        &self.png
    }
}

/// Metadata for either runtime environment contract; wire objects retain their original shape.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum JobEnvironmentInfo {
    Cpu(crate::lighting::environment_input::EnvironmentInfo),
    Gpu(crate::lighting::environment::EnvironmentInfo),
}

#[derive(Clone, Debug, Serialize)]
pub struct JobStatus {
    /// Complete passes omitted only when both prepaid snapshots are externally retained.
    /// Final rendering continues; this is not a dropped sample or a stalled job.
    pub preview_retention_skips: u32,
    /// This request still renders final output, but has no intermediate-image route.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_unavailable_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub punctual_diagnostics: Option<super::PunctualDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<super::presentation::MasterPresentation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<super::adaptive::SamplingReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linear_master: Option<LinearMasterInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewInfo>,
    pub textures: Option<super::textures::TextureSummary>,
    pub options: JobOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<JobEnvironmentInfo>,
    pub job: JobId,
    pub scene_generation: u64,
    pub request: ImageRequest,
    pub resolved: crate::ResolvedRequest,
    pub progress: Progress,
    pub error: Option<JobError>,
    pub frame: Option<FrameInfo>,
}

struct Data {
    preview_retention_skips: u32,
    completed_duration: Option<std::time::Duration>,
    punctual_diagnostics: Option<super::PunctualDiagnostics>,
    sampling: Option<super::adaptive::SamplingReport>,
    progress: Progress,
    error: Option<JobError>,
    frame: Option<Arc<RenderFrame>>,
    preview: Option<Arc<RenderPreview>>,
}
struct Record {
    work_released: AtomicBool,
    preview_unavailable_reason: Option<&'static str>,
    started: web_time::Instant,
    presentation: Option<Arc<RetainedPresentation>>,
    textures: Option<super::textures::TextureSummary>,
    options: Option<RetainedOptions>,
    environment: Option<JobEnvironmentInfo>,
    _request_reservation: Option<Reservation>,
    id: JobId,
    scene_generation: u64,
    request: ImageRequest,
    resolved: crate::ResolvedRequest,
    cancel: CancelToken,
    data: Mutex<Data>,
    #[cfg(not(target_arch = "wasm32"))]
    done: Condvar,
}

impl Record {
    fn options(&self) -> Result<JobOptions> {
        match &self.options {
            None => Ok(JobOptions::default()),
            Some(slot) => slot.0.first().copied().ok_or_else(|| JobError::new(ErrorCode::Internal, "missing retained render options")),
        }
    }
    fn notify(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.done.notify_all();
    }
}

#[derive(Clone)]
pub struct JobHandle(Arc<Record>);

impl JobHandle {
    pub fn id(&self) -> JobId {
        self.0.id
    }
    /// True only after queued/running captures, tasks and their work reservations
    /// have been dropped. A terminal phase alone does not prove resource release.
    /// Retained final/preview images and status-input leases remain owned by their
    /// observers; callers retiring a job must drop those handles after this barrier.
    pub fn work_released(&self) -> bool {
        self.0.work_released.load(Ordering::Acquire)
    }
    /// Submission-to-publication time, captured once by the host, independent of UI polling.
    pub fn completed_duration(&self) -> Result<Option<std::time::Duration>> {
        self.0.data.lock().map(|data| data.completed_duration).map_err(|_| JobError::new(ErrorCode::Internal, "render duration lock poisoned"))
    }
    pub fn status(&self) -> Result<JobStatus> {
        let data = self.0.data.lock().map_err(|_| JobError::new(ErrorCode::Internal, "render status lock poisoned"))?;
        Ok(JobStatus {
            preview_retention_skips: data.preview_retention_skips,
            preview_unavailable_reason: self.0.preview_unavailable_reason,
            punctual_diagnostics: data.punctual_diagnostics,
            presentation: self.0.presentation.as_deref().map(|retained| retained.info.clone()),
            sampling: data.sampling.clone(),
            linear_master: data.frame.as_ref().and_then(|frame| frame.linear_master_info().cloned()),
            preview: data.preview.as_ref().map(|preview| preview.info.clone()),
            textures: self.0.textures.clone(),
            options: self.0.options()?,
            environment: self.0.environment.clone(),
            job: self.0.id,
            scene_generation: self.0.scene_generation,
            request: self.0.request.clone(),
            resolved: self.0.resolved.clone(),
            progress: data.progress,
            error: data.error.clone(),
            frame: data.frame.as_ref().map(|f| f.info.clone()),
        })
    }
    pub fn cancel(&self) -> Result<JobStatus> {
        {
            let mut data = self.0.data.lock().map_err(|_| JobError::new(ErrorCode::Internal, "render status lock poisoned"))?;
            if !data.progress.phase.terminal() {
                self.0.cancel.cancel();
                data.preview = None;
                data.progress.phase = if data.progress.phase == Phase::Queued { Phase::Cancelled } else { Phase::Cancelling };
            }
        }
        self.0.notify();
        self.status()
    }

    /// Native application helper; engine status commands remain non-blocking.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_timeout(&self, timeout: std::time::Duration) -> Result<JobStatus> {
        let data = self.0.data.lock().map_err(|_| JobError::new(ErrorCode::Internal, "render status lock poisoned"))?;
        let (data, timed) = self
            .0
            .done
            .wait_timeout_while(data, timeout, |d| !d.progress.phase.terminal())
            .map_err(|_| JobError::new(ErrorCode::Internal, "render completion lock poisoned"))?;
        if timed.timed_out() && !data.progress.phase.terminal() {
            return Err(JobError::new(ErrorCode::TimedOut, "render wait timed out"));
        }
        drop(data);
        self.status()
    }
    pub fn frame(&self) -> Result<Option<Arc<RenderFrame>>> {
        let data = self.0.data.lock().map_err(|_| JobError::new(ErrorCode::Internal, "render result lock poisoned"))?;
        let frame = data.frame.clone();
        if frame.as_ref().is_some_and(|f| f.info.job != self.0.id || f.info.scene_generation != self.0.scene_generation) {
            return Err(JobError::new(ErrorCode::WrongOwner, "render frame identity does not match its job"));
        }
        Ok(frame)
    }

    /// A provisional complete-pass image, never the final/export frame slot.
    pub fn preview(&self) -> Result<Option<Arc<RenderPreview>>> {
        let data = self.0.data.lock().map_err(|_| JobError::new(ErrorCode::Internal, "render preview lock poisoned"))?;
        let preview = data.preview.clone();
        if preview.as_ref().is_some_and(|p| p.info.frame.job != self.0.id || p.info.frame.scene_generation != self.0.scene_generation) {
            return Err(JobError::new(ErrorCode::WrongOwner, "render preview identity does not match its job"));
        }
        Ok(preview)
    }
}

/// The last field of Work and Running. Rust drops earlier fields first, so
/// observers cannot see this barrier until captured inputs, executor tasks,
/// pending PNGs, reservations and checkpoint credits have actually been released.
struct WorkRelease(Weak<Record>);
impl Drop for WorkRelease {
    fn drop(&mut self) {
        if let Some(record) = self.0.upgrade() {
            record.work_released.store(true, Ordering::Release);
            record.notify();
        }
    }
}

/// Independently prepaid captured metadata, retained beyond any one Work/frame.
/// The owner must place its backing before this field so backing drops first.
pub struct RetainedMetadata(Reservation);
impl RetainedMetadata {
    pub fn bytes(&self) -> u64 {
        self.0.bytes
    }
}

/// The explicit execution envelope for NEW common map capture. Registered
/// executors use their actual host limit and never a synthetic CPU profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureProfile {
    CpuRaytrace(super::CpuBudget),
    Registered,
}
impl CaptureProfile {
    fn integrator(self) -> Integrator {
        match self {
            Self::CpuRaytrace(_) => Integrator::Raytrace,
            Self::Registered => Integrator::Raster,
        }
    }
    fn budget(self) -> super::CpuBudget {
        match self {
            Self::CpuRaytrace(budget) => budget,
            Self::Registered => super::CpuBudget::Compact,
        }
    }
}

// Serialize metadata directly into the existing hash state: no captured source,
// descriptor JSON Value or retained descriptor Vec/String clone is created.
fn capture_fingerprint(value: &impl Serialize) -> Result<[u8; 32]> {
    struct HashWriter(Sha256);
    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, value).map_err(|e| JobError::new(ErrorCode::Internal, format!("capture descriptor: {e}")))?;
    Ok(writer.0.finalize().into())
}

/// Move-only same-pool high-water credit acquired BEFORE common captured map
/// backing. Its enclosing caller must drop that backing before this lease.
/// Submission consumes it into the ordinary PreparedSubmission reservations.
/// Any high-water excess over the final layout remains billed as preparation
/// backing until Work releases; Record retains only its normal summary lease.
pub struct CaptureLease<'a> {
    host: ImageHost,
    profile: CaptureProfile,
    descriptor: [u8; 32],
    execution: [u8; 32],
    retained: Option<&'a RetainedMetadata>,
    cancel: CancelToken,
    reservation: Reservation,
    consumed: bool,
}
impl CaptureLease<'_> {
    pub fn bytes(&self) -> u64 {
        self.reservation.bytes
    }
    fn check(&self, cancel: &CancelToken) -> Result<()> {
        if self.consumed {
            return Err(JobError::new(ErrorCode::InvalidRequest, "capture credit was already consumed"));
        }
        if !Arc::ptr_eq(&self.cancel.0, &cancel.0) {
            return Err(JobError::new(ErrorCode::WrongOwner, "capture cancellation owner changed"));
        }
        self.cancel.check()?;
        cancel.check()?;
        let descriptor = self.host.capture_descriptor(self.profile)?;
        if capture_fingerprint(&descriptor)? != self.descriptor {
            return Err(JobError::new(ErrorCode::HostUnavailable, "capture executor metadata changed"));
        }
        Ok(())
    }
    /// Add only the difference between the next live peak and existing credit.
    /// Retired duplicate/pending buffers reuse this high-water lease; no release
    /// or reacquisition is needed between capture phases.
    pub fn try_extend(&mut self, bytes: u64, cancel: &CancelToken) -> Result<()> {
        self.check(cancel)?;
        let total = self.bytes().checked_add(bytes).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "capture credit overflow"))?;
        self.host.check_request_sum(total, self.retained.map_or(0, RetainedMetadata::bytes), self.profile.budget())?;
        let mut extra = self.host.0.shared.budget.reserve(bytes)?;
        self.reservation.merge_from(&mut extra)
    }
    fn check_request(&self, host: &ImageHost, request: &ImageRequest, cancel: &CancelToken, retained: Option<&RetainedMetadata>) -> Result<()> {
        if !self.host.same_host(host) {
            return Err(JobError::new(ErrorCode::WrongOwner, "capture credit belongs to a different installed host"));
        }
        self.check(cancel)?;
        let selected = if host.0.executor.is_some() { CaptureProfile::Registered } else { CaptureProfile::CpuRaytrace(request.cpu_budget()) };
        if self.profile != selected || request.integrator != self.profile.integrator() {
            return Err(JobError::new(ErrorCode::WrongOwner, "capture execution profile changed"));
        }
        host.capture_retained_context(retained)?;
        let same_retained = match (self.retained, retained) {
            (None, None) => true,
            (Some(before), Some(after)) => std::ptr::eq(before, after),
            _ => false,
        };
        if !same_retained {
            return Err(JobError::new(ErrorCode::WrongOwner, "capture retained-metadata context changed"));
        }
        let bytes = request
            .textures
            .as_ref()
            .ok_or_else(|| JobError::new(ErrorCode::InvalidRequest, "capture credit requires common texture backing"))?
            .captured_bytes()?;
        if bytes > self.bytes() {
            return Err(JobError::new(ErrorCode::LimitExceeded, "captured texture backing exceeds its prepaid credit"));
        }
        Ok(())
    }
}

/// Captured, completely validated input under the existing host budget. This
/// has no job identity, document or camera mutation, and cannot be cloned.
pub struct PreparedSubmission {
    scene: ProductScene,
    public_request: ImageRequest,
    work_request: Option<ImageRequest>,
    options: Option<RetainedOptions>,
    environment: Option<crate::lighting::environment_input::EnvironmentInput>,
    environment_info: Option<JobEnvironmentInfo>,
    textures: Option<super::textures::TextureSummary>,
    resolved: crate::ResolvedRequest,
    admission: Admission,
    cancel: CancelToken,
    #[cfg(feature = "raytrace-cpu")]
    proof: Vec<crate::raytrace::image::CertifiedOptics>,
    request_reservation: Option<Reservation>,
    reservation: Reservation,
    host: ImageHost,
}
impl PreparedSubmission {
    /// Check a simultaneously retained metadata owner under the same unchanged
    /// per-job/profile cap. It is not funded by a Work/frame lease.
    pub fn check_retained_metadata(&self, bytes: u64) -> Result<()> {
        let retained = self
            .request_reservation
            .as_ref()
            .map_or(0, |r| r.bytes)
            .checked_add(bytes)
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "retained metadata byte overflow"))?;
        self.host.check_request_sum(self.reservation.bytes, retained, self.request().cpu_budget())
    }
    pub fn reserve_retained_metadata(&self, bytes: u64) -> Result<RetainedMetadata> {
        self.check_retained_metadata(bytes)?;
        self.host.0.shared.budget.reserve(bytes).map(RetainedMetadata)
    }
    pub fn request(&self) -> &ImageRequest {
        self.work_request.as_ref().unwrap_or(&self.public_request)
    }
    pub fn admission(&self) -> Admission {
        self.admission
    }
    pub fn check_cancel(&self) -> Result<()> {
        self.cancel.check()
    }
    pub fn executor(&self) -> &crate::ExecutorId {
        &self.resolved.executor
    }
    pub fn host(&self) -> &ImageHost {
        &self.host
    }
    pub fn options(&self) -> Result<JobOptions> {
        match &self.options {
            None => Ok(JobOptions::default()),
            Some(slot) => slot.0.first().copied().ok_or_else(|| JobError::new(ErrorCode::Internal, "missing prepared render options")),
        }
    }
}

struct Work {
    #[cfg(feature = "raytrace-cpu")]
    proof: Vec<crate::raytrace::image::CertifiedOptics>,
    regrade: Option<(Arc<RenderFrame>, super::presentation::Presentation)>,
    request: Option<ImageRequest>,
    executor: Option<Arc<dyn ImageExecutor>>,
    environment: Option<crate::lighting::environment_input::EnvironmentInput>,
    scene: ProductScene,
    handle: JobHandle,
    reservation: Reservation,
    // Must remain last: see WorkRelease.
    _work_release: WorkRelease,
}
// A queued request moves into the task at begin; no task allocation exists yet.
// Its fixed queue metadata must fit the same already prepaid task allowance,
// independently of the separate retained-Record header reservation.
const _: [(); 1] = [(); (std::mem::size_of::<Work>() <= 4096) as usize];
// One host advance hashes at most four 16 KiB chunks, independently of the
// integrator's quantum. The unchanged work reservation covers the sole PNG and
// this fixed state until publication; it is not another image task or pool.
const PNG_HASH_CHUNK_BYTES: usize = 16_384;
const PNG_HASH_CHUNKS_PER_ADVANCE: usize = 4;
pub(crate) const FRAME_METADATA_BYTES: u64 = 1024;
#[cfg(feature = "raytrace-cpu")]
const PREPARED_PROOF_HEADER_BYTES: usize = std::mem::size_of::<Vec<crate::raytrace::image::CertifiedOptics>>();
#[cfg(not(feature = "raytrace-cpu"))]
const PREPARED_PROOF_HEADER_BYTES: usize = 0;
// Preparation/enqueue only moves fixed headers. All source/resource capacities
// and optical proof backing have independent admission; none survives without
// the owning work/request lease. Keep the existing preparation allowance.
const _: [(); 1] =
    [(); (std::mem::size_of::<PreparedSubmission>() + std::mem::size_of::<Work>() + std::mem::size_of::<Record>() + std::mem::size_of::<Running>()
        <= super::image::PREPARATION_METADATA_BYTES as usize) as usize];

struct PendingPng {
    png: Vec<u8>,
    digest: Sha256,
    png_digest: Option<sha2::digest::Output<Sha256>>,
    hashed: usize,
    preview_samples: Option<u32>,
    exr: Option<Vec<u8>>,
    exr_hashed: usize,
}
// Keep the new fixed completion state within the existing metadata allowance
// even if the hashing dependency's representation changes on another target.
const _: [(); 1] = [(); (std::mem::size_of::<PendingPng>() <= 512) as usize];
const _: [(); 1] = [(); (std::mem::size_of::<Running>()
    + std::mem::size_of::<RenderFrame>()
    + std::mem::size_of::<RenderPreview>()
    + PREPARED_PROOF_HEADER_BYTES
    + 64
    + 2 * crate::MAX_RENDER_ID_BYTES
    + 32
    <= FRAME_METADATA_BYTES as usize) as usize];

impl PendingPng {
    fn new(png: Vec<u8>) -> Self {
        Self { png, digest: Sha256::new(), png_digest: None, hashed: 0, preview_samples: None, exr: None, exr_hashed: 0 }
    }

    fn preview(png: Vec<u8>, samples: u32) -> Self {
        Self { preview_samples: Some(samples), ..Self::new(png) }
    }

    fn step(&mut self, units: usize, cancel: &CancelToken) -> Result<bool> {
        cancel.check()?;
        for _ in 0..units.min(PNG_HASH_CHUNKS_PER_ADVANCE) {
            self.finish_png_digest();
            let (bytes, hashed) = if self.hashed < self.png.len() {
                (&self.png, &mut self.hashed)
            } else if let Some(exr) = self.exr.as_ref().filter(|exr| self.exr_hashed < exr.len()) {
                (exr, &mut self.exr_hashed)
            } else {
                break;
            };
            let end = hashed.saturating_add(PNG_HASH_CHUNK_BYTES).min(bytes.len());
            self.digest.update(bytes.get(*hashed..end).ok_or_else(|| JobError::new(ErrorCode::Internal, "missing completed image digest chunk"))?);
            *hashed = end;
            cancel.check()?;
        }
        self.finish_png_digest();
        Ok(self.hashed == self.png.len() && self.exr.as_ref().is_none_or(|exr| self.exr_hashed == exr.len()))
    }
    fn finish_png_digest(&mut self) {
        if self.hashed == self.png.len() && self.png_digest.is_none() {
            // PNG and EXR are scanned sequentially. Keep only its fixed digest
            // while resetting the same SHA state for the optional raw companion.
            self.png_digest = Some(self.digest.finalize_reset());
        }
    }
    fn progress(&self) -> Progress {
        Progress {
            phase: Phase::Encoding,
            done: self.hashed as u64 + self.exr_hashed as u64,
            total: self.png.len() as u64 + self.exr.as_ref().map_or(0, |bytes| bytes.len() as u64),
        }
    }
}

struct Running {
    source_master: Option<Arc<RenderFrame>>,
    task: Option<WorkTask>,
    pending_png: Option<PendingPng>,
    handle: JobHandle,
    reservation: Option<Reservation>,
    next_preview_sequence: u32,
    next_checkpoint_pass: u32,
    checkpoint_target: u32,
    checkpoints: Option<Arc<CheckpointCredits>>,
    // Must remain last: see WorkRelease.
    _work_release: WorkRelease,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(credits) = &self.checkpoints {
            credits.close();
        }
    }
}

/// One independently prepaid options value. Exact defaults have no slot; authored
/// option/effect presence is structural and is never normalized into that case.
/// The containing Record is smaller than its previous inline JobOptions field.
/// This lease survives work release and frame eviction through retained handles.
struct RetainedOptions(Vec<JobOptions>);
impl RetainedOptions {
    fn new(options: JobOptions) -> Result<Self> {
        let _ = options.retained_bytes()?;
        let mut values = Vec::new();
        values.try_reserve_exact(1).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "could not reserve retained render options"))?;
        if values.capacity() != 1 {
            return Err(JobError::new(ErrorCode::LimitExceeded, "retained render options exceed their prepaid capacity"));
        }
        values.push(options);
        Ok(Self(values))
    }
}
const _: [(); 1] = [(); (std::mem::size_of::<Option<RetainedOptions>>() <= std::mem::size_of::<JobOptions>()) as usize];

/// One fallibly allocated task, keeping the dispatch enum small in either build.
/// Its bounded metadata allocation fits the existing per-job task allowance.
struct TaskSlot<T>(Vec<T>);
impl<T> TaskSlot<T> {
    fn new(task: T) -> Result<Self> {
        if std::mem::size_of::<T>() > 4096 {
            return Err(JobError::new(ErrorCode::LimitExceeded, "render task metadata exceeds its admission allowance"));
        }
        let mut tasks = Vec::new();
        tasks.try_reserve_exact(1).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "could not reserve render task slot"))?;
        if tasks.capacity() != 1 {
            return Err(JobError::new(ErrorCode::LimitExceeded, "render task slot exceeds its bounded capacity"));
        }
        tasks.push(task);
        Ok(Self(tasks))
    }
    fn get_mut(&mut self) -> Result<&mut T> {
        self.0.first_mut().ok_or_else(|| JobError::new(ErrorCode::Internal, "missing render task slot"))
    }
}
enum WorkTask {
    Regrade(TaskSlot<super::regrade::RegradeTask>),
    Registered(Box<dyn ImageWork>),
    Raster(TaskSlot<ImageTask>),
    #[cfg(feature = "raytrace-cpu")]
    Raytrace(TaskSlot<crate::raytrace::image::ImageTask>),
}

enum WorkPoll {
    Existing(TaskPoll),
    Preview { png: Vec<u8>, samples_per_pixel: u32 },
}

impl WorkTask {
    fn punctual_diagnostics(&self) -> Option<super::PunctualDiagnostics> {
        match self {
            #[cfg(feature = "raytrace-cpu")]
            Self::Raytrace(slot) => slot.0.first().and_then(|task| task.punctual_diagnostics()),
            Self::Registered(task) => task.punctual_diagnostics(),
            _ => None,
        }
    }

    fn source_master(&self) -> Result<Option<Arc<RenderFrame>>> {
        match self {
            Self::Regrade(slot) => {
                slot.0.first().map(|task| Some(task.source())).ok_or_else(|| JobError::new(ErrorCode::Internal, "missing master task slot"))
            }
            _ => Ok(None),
        }
    }
    fn sampling_report(&self) -> Option<super::adaptive::SamplingReport> {
        match self {
            Self::Registered(task) => task.sampling_report(),
            #[cfg(feature = "raytrace-cpu")]
            Self::Raytrace(slot) => slot.0.first().and_then(|task| task.sampling_report()),
            _ => None,
        }
    }
    fn take_linear_exr(&mut self) -> Option<Vec<u8>> {
        match self {
            Self::Registered(task) => task.take_linear_exr(),
            #[cfg(feature = "raytrace-cpu")]
            Self::Raytrace(task) => task.0.first_mut().and_then(|task| task.take_linear_exr()),
            _ => None,
        }
    }
    fn new(
        scene: ProductScene,
        request: ImageRequest,
        cancel: CancelToken,
        environment: Option<crate::lighting::environment_input::EnvironmentInput>,
        #[cfg(feature = "raytrace-cpu")] proof: Option<crate::raytrace::image::CertifiedOptics>,
    ) -> Result<Self> {
        let _ = &environment;
        match request.integrator {
            Integrator::Raster => ImageTask::new(scene, request, cancel).and_then(TaskSlot::new).map(Self::Raster),
            Integrator::Raytrace => {
                #[cfg(feature = "raytrace-cpu")]
                {
                    let options = request.raytrace.clone().unwrap_or_default();
                    crate::raytrace::image::ImageTask::new_prepared(scene, request, cancel, options, environment, proof)
                        .and_then(TaskSlot::new)
                        .map(Self::Raytrace)
                }
                #[cfg(not(feature = "raytrace-cpu"))]
                {
                    Err(JobError::new(ErrorCode::HostUnavailable, "CPU raytracing is not compiled in; enable raytrace-cpu"))
                }
            }
        }
    }

    fn step(&mut self, units: usize) -> Result<WorkPoll> {
        match self {
            Self::Regrade(task) => task.get_mut()?.step(units).map(WorkPoll::Existing),
            Self::Registered(task) => {
                let poll = task.step(units)?;
                if let Some(preview) = task.take_preview() {
                    if !matches!(poll, TaskPoll::Pending(_)) {
                        return Err(JobError::new(ErrorCode::Internal, "executor published provisional and final output together"));
                    }
                    Ok(WorkPoll::Preview { png: preview.png, samples_per_pixel: preview.samples_per_pixel })
                } else {
                    Ok(WorkPoll::Existing(poll))
                }
            }
            Self::Raster(task) => task.get_mut()?.step(units).map(WorkPoll::Existing),
            #[cfg(feature = "raytrace-cpu")]
            Self::Raytrace(task) => Ok(match task.get_mut()?.step_with_previews(units)? {
                crate::raytrace::image::ImagePoll::Pending(progress) => WorkPoll::Existing(TaskPoll::Pending(progress)),
                crate::raytrace::image::ImagePoll::Complete(png) => WorkPoll::Existing(TaskPoll::Complete(png)),
                crate::raytrace::image::ImagePoll::Preview { png, samples_per_pixel } => WorkPoll::Preview { png, samples_per_pixel },
            }),
        }
    }
}

fn terminal(handle: &JobHandle, error: JobError) {
    if let Ok(mut data) = handle.0.data.lock() {
        if data.progress.phase == Phase::Completed {
            return;
        }
        if handle.0.cancel.is_cancelled() || error.code == ErrorCode::Cancelled {
            data.progress.phase = Phase::Cancelled;
            data.error = None;
        } else {
            data.progress.phase = Phase::Failed;
            data.error = Some(error);
        }
        data.frame = None;
        data.preview = None;
    }
    handle.0.notify();
}

impl Running {
    fn begin(mut work: Work) -> Option<Self> {
        let options = match work.handle.0.options() {
            Ok(options) => options,
            Err(error) => {
                terminal(&work.handle, error);
                return None;
            }
        };
        let checkpoint_target = if let Some(executor) = &work.executor {
            executor.preview_samples(options).unwrap_or(0)
        } else if work.handle.0.request.integrator == Integrator::Raytrace {
            work.handle.0.request.raytrace.as_ref().map_or(Ok(0), super::RaytraceOptions::preview_target).unwrap_or(0)
        } else {
            0
        };
        let checkpoints = (|| {
            let slots = checkpoint_target.saturating_sub(1).min(2);
            if slots == 0 {
                return Ok(None);
            }
            let slot_bytes = u64::from(work.handle.0.request.width)
                .checked_mul(u64::from(work.handle.0.request.height))
                .and_then(|pixels| pixels.checked_mul(8))
                .and_then(|bytes| bytes.checked_add(65_536 + FRAME_METADATA_BYTES))
                .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "checkpoint slot overflow"))?;
            CheckpointCredits::new(&mut work.reservation, slot_bytes, slots).map(Some)
        })();
        let checkpoints = match checkpoints {
            Ok(credits) => credits,
            Err(error) => {
                terminal(&work.handle, error);
                return None;
            }
        };
        if work.handle.0.cancel.is_cancelled() {
            terminal(&work.handle, JobError::new(ErrorCode::Cancelled, "render cancelled"));
            return None;
        }
        if let Ok(mut data) = work.handle.0.data.lock()
            && !work.handle.0.cancel.is_cancelled()
        {
            data.progress.phase = Phase::Preparing;
        }
        let task = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some((source, presentation)) = work.regrade {
                super::regrade::RegradeTask::new(source, presentation, work.handle.0.cancel.clone()).and_then(TaskSlot::new).map(WorkTask::Regrade)
            } else if let Some(executor) = work.executor {
                let request = work.request.ok_or_else(|| JobError::new(ErrorCode::Internal, "missing registered executor request"))?;
                executor.begin(work.scene, request, options, work.handle.0.cancel.clone()).map(WorkTask::Registered)
            } else {
                // CPU textures are work-owned just like registered resources. The
                // public/status request has already had those Arcs removed.
                let request = work.request.unwrap_or_else(|| work.handle.0.request.clone());
                WorkTask::new(
                    work.scene,
                    request,
                    work.handle.0.cancel.clone(),
                    work.environment,
                    #[cfg(feature = "raytrace-cpu")]
                    work.proof.pop(),
                )
            }
        }))
        .unwrap_or_else(|_| Err(JobError::new(ErrorCode::Internal, "image executor failed while preparing the job")));
        match task {
            Ok(task) => Some(Self {
                source_master: None,
                task: Some(task),
                pending_png: None,
                handle: work.handle,
                reservation: Some(work.reservation),
                next_preview_sequence: 1,
                next_checkpoint_pass: 1,
                checkpoint_target,
                checkpoints,
                _work_release: work._work_release,
            }),
            Err(e) => {
                terminal(&work.handle, e);
                None
            }
        }
    }

    fn advance(&mut self, units: usize) -> Result<bool> {
        self.handle.0.cancel.check()?;
        if self.pending_png.is_some() {
            return self.advance_png(units);
        }
        let result = self.task.as_mut().ok_or_else(|| JobError::new(ErrorCode::Internal, "missing render task"))?.step(units)?;
        let mut data = self.handle.0.data.lock().map_err(|_| JobError::new(ErrorCode::Internal, "render status lock poisoned"))?;
        data.punctual_diagnostics = self.task.as_ref().and_then(WorkTask::punctual_diagnostics);
        // The final publication check is under the same lock as cancellation.
        if self.handle.0.cancel.is_cancelled() {
            data.progress.phase = Phase::Cancelled;
            data.frame = None;
            data.preview = None;
            self.handle.0.notify();
            return Ok(false);
        }
        match result {
            WorkPoll::Existing(TaskPoll::Pending(progress)) => {
                data.progress = progress;
                Ok(true)
            }
            WorkPoll::Existing(TaskPoll::Complete(png)) => {
                // Drop every integrator-owned capture/scratch buffer first, but
                // keep the original admission lease while hashing the sole PNG.
                self.source_master = self.task.as_ref().map(WorkTask::source_master).transpose()?.flatten();
                data.sampling = self.task.as_ref().and_then(WorkTask::sampling_report);
                let exr = self.task.as_mut().and_then(WorkTask::take_linear_exr);
                let retain_exr = self
                    .handle
                    .0
                    .request
                    .raytrace
                    .as_ref()
                    .and_then(|options| options.output.as_deref())
                    .map_or(self.handle.0.options()?.linear_exr, |output| output.linear_exr);
                if exr.is_some() != retain_exr {
                    return Err(JobError::new(ErrorCode::Internal, "linear master availability differs from the admitted request"));
                }
                if let Some(bytes) = &exr {
                    let limit = super::linear_exr::encoded_limit(self.handle.0.request.width, self.handle.0.request.height)?;
                    if bytes.len() < 8 || bytes.capacity() as u64 > limit || bytes.get(..8) != Some(&[0x76, 0x2f, 0x31, 0x01, 2, 0, 0, 0]) {
                        return Err(JobError::new(ErrorCode::LimitExceeded, "linear master exceeds its bound or has an invalid signature"));
                    }
                }
                self.task = None;
                let mut pending = PendingPng::new(png);
                pending.exr = exr;
                data.progress = pending.progress();
                self.pending_png = Some(pending);
                Ok(true)
            }
            WorkPoll::Preview { png, samples_per_pixel } => {
                // Retain the integrator/film and the entire remaining work lease.
                // Hash the checkpoint before making immutable bytes observable.
                let pending = PendingPng::preview(png, samples_per_pixel);
                data.progress = pending.progress();
                self.pending_png = Some(pending);
                Ok(true)
            }
        }
    }

    fn advance_png(&mut self, units: usize) -> Result<bool> {
        // No status/data lock is held while scanning the encoded image.
        let pending = self.pending_png.as_mut().ok_or_else(|| JobError::new(ErrorCode::Internal, "missing pending PNG"))?;
        if !pending.step(units, &self.handle.0.cancel)? {
            let progress = pending.progress();
            let mut data = self.handle.0.data.lock().map_err(|_| JobError::new(ErrorCode::Internal, "render status lock poisoned"))?;
            self.handle.0.cancel.check()?;
            data.progress = progress;
            return Ok(true);
        }
        let PendingPng { png, digest, png_digest, preview_samples, exr, .. } =
            self.pending_png.take().ok_or_else(|| JobError::new(ErrorCode::Internal, "missing completed PNG digest"))?;
        let info = FrameInfo {
            job: self.handle.0.id,
            scene_generation: self.handle.0.scene_generation,
            backend: self.handle.0.resolved.backend.clone(),
            executor: self.handle.0.resolved.executor.clone(),
            width: self.handle.0.request.width,
            height: self.handle.0.request.height,
            encoding: ColourEncoding::Srgb,
            alpha: AlphaConvention::Straight,
            png_bytes: png.len() as u64,
            png_sha256: format!("{:x}", png_digest.ok_or_else(|| JobError::new(ErrorCode::Internal, "missing completed PNG digest"))?),
        };
        // Include the working digest/Vec metadata, completed frame layout,
        // actual hex allocation, bounded copied IDs and Arc/header allowance.
        // The PNG allocation itself is retained once and billed by capacity below.
        let frame_layout = if preview_samples.is_some() { std::mem::size_of::<RenderPreview>() } else { std::mem::size_of::<RenderFrame>() };
        // A retained checkpoint keeps its shared credit ledger alive even after
        // the running job closes it. Bill that fixed Arc payload/header through
        // the checkpoint's own metadata allowance as well as its PNG capacity.
        let checkpoint_metadata = if preview_samples.is_some() { std::mem::size_of::<CheckpointCredits>() + 32 } else { 0 };
        let fixed_metadata = std::mem::size_of::<Running>() + frame_layout + checkpoint_metadata + 2 * crate::MAX_RENDER_ID_BYTES + 32;
        let linear_digest_bytes = if exr.is_some() { 64 } else { 0 };
        if fixed_metadata
            .checked_add(info.png_sha256.capacity())
            .and_then(|n| n.checked_add(linear_digest_bytes))
            .is_none_or(|bytes| bytes > FRAME_METADATA_BYTES as usize)
        {
            return Err(JobError::new(ErrorCode::LimitExceeded, "PNG digest metadata exceeds its existing admission allowance"));
        }
        let mut data = self.handle.0.data.lock().map_err(|_| JobError::new(ErrorCode::Internal, "render status lock poisoned"))?;
        // Cancellation shares this lock; a late cancel must win before frame
        // installation. The digest is complete; checkpoints retain task scratch,
        // while the final-only path has already released it.
        self.handle.0.cancel.check()?;
        if let Some(samples_per_pixel) = preview_samples {
            let target_samples = self.checkpoint_target;
            let frame_sequence = self.next_preview_sequence;
            let expected = Some(self.next_checkpoint_pass);
            if expected != Some(samples_per_pixel) || samples_per_pixel >= target_samples || self.task.is_none() {
                return Err(JobError::new(ErrorCode::Internal, "invalid progressive checkpoint sequence"));
            }
            let next_sequence = frame_sequence.checked_add(1).ok_or_else(|| JobError::new(ErrorCode::Internal, "preview sequence exhausted"))?;
            let credits = self.checkpoints.as_ref().ok_or_else(|| JobError::new(ErrorCode::Internal, "missing checkpoint credits"))?;
            if (png.capacity() as u64).checked_add(FRAME_METADATA_BYTES).is_none_or(|bytes| bytes > credits.slot_bytes) {
                return Err(JobError::new(ErrorCode::LimitExceeded, "checkpoint PNG exceeds its admitted slot"));
            }
            let mut reservation = credits.acquire()?;
            if reservation.is_none() {
                // A headless client need not acknowledge snapshots: release our old
                // slot before deciding that both images are retained by consumers.
                let released = data.preview.take();
                drop(data);
                drop(released);
                reservation = credits.acquire()?;
                data = self.handle.0.data.lock().map_err(|_| JobError::new(ErrorCode::Internal, "render preview publication lock poisoned"))?;
                self.handle.0.cancel.check()?;
            }
            self.next_checkpoint_pass = self
                .next_checkpoint_pass
                .checked_add(1)
                .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "checkpoint pass ordinal exhausted"))?;
            let Some(reservation) = reservation else {
                data.preview_retention_skips = data
                    .preview_retention_skips
                    .checked_add(1)
                    .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "checkpoint retention diagnostic overflow"))?;
                return Ok(true);
            };
            data.preview = Some(Arc::new(RenderPreview {
                info: PreviewInfo {
                    frame_sequence,
                    samples_per_pixel,
                    adaptive: match self.handle.0.request.raytrace.as_ref().filter(|_| self.handle.0.request.integrator == Integrator::Raytrace) {
                        Some(options) => options
                            .adaptive
                            .as_deref()
                            .map(|adaptive| {
                                let maximum = options
                                    .progressive
                                    .ok_or_else(|| JobError::new(ErrorCode::Internal, "adaptive CPU preview has no captured film"))?
                                    .samples_per_pixel;
                                super::adaptive::AdaptivePreview::captured(*adaptive, maximum, samples_per_pixel)
                            })
                            .transpose()?,
                        None => super::adaptive::AdaptivePreview::at(self.handle.0.options()?, samples_per_pixel)?,
                    },
                    frame: info,
                },
                png,
                _reservation: reservation,
            }));
            self.next_preview_sequence = next_sequence;
            let pixels = u64::from(self.handle.0.request.width) * u64::from(self.handle.0.request.height);
            data.progress =
                Progress { phase: Phase::Running, done: pixels * u64::from(samples_per_pixel), total: pixels * u64::from(target_samples) };
            self.handle.0.notify();
            return Ok(true);
        }
        let exr = exr.map(|bytes| {
            let info = LinearMasterInfo {
                bytes: bytes.len() as u64,
                sha256: format!("{:x}", digest.finalize()),
                encoding: LinearColourEncoding::SceneLinearSrgbD65,
                alpha: LinearAlphaConvention::CoveragePremultiplied,
            };
            (bytes, info)
        });
        let retained = png.capacity() as u64 + exr.as_ref().map_or(0, |(bytes, _)| bytes.capacity() as u64) + FRAME_METADATA_BYTES;
        if let Some(credits) = &self.checkpoints {
            credits.close();
        }
        let mut reservation = self.reservation.take().ok_or_else(|| JobError::new(ErrorCode::Internal, "missing render reservation"))?;
        reservation.shrink(retained)?;
        data.frame = Some(Arc::new(RenderFrame {
            info,
            png,
            exr,
            source_master: self.source_master.take(),
            presentation: self.handle.0.presentation.clone(),
            _reservation: reservation,
        }));
        data.preview = None;
        data.progress = Progress { phase: Phase::Completed, done: 1, total: 1 };
        data.completed_duration = Some(self.handle.0.started.elapsed());
        self.handle.0.notify();
        Ok(false)
    }

    fn guarded_step(&mut self, units: usize) -> bool {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.advance(units)));
        match result {
            Ok(Ok(more)) => more,
            Ok(Err(e)) => {
                terminal(&self.handle, e);
                false
            }
            Err(_) => {
                terminal(&self.handle, JobError::new(ErrorCode::Internal, "image host encountered an internal error"));
                false
            }
        }
    }
}

struct Shared {
    queue: Mutex<VecDeque<Work>>,
    active: Mutex<Option<Weak<Record>>>,
    stop: AtomicBool,
    budget: Arc<Budget>,
    #[cfg(not(target_arch = "wasm32"))]
    wake: Condvar,
}

struct Owner {
    executor: Option<Arc<dyn ImageExecutor>>,
    shared: Arc<Shared>,
    kind: HostKind,
    running: Mutex<Option<Running>>,
    #[cfg(not(target_arch = "wasm32"))]
    _worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Ok(active) = self.shared.active.lock()
            && let Some(record) = active.as_ref().and_then(Weak::upgrade)
        {
            let _ = JobHandle(record).cancel();
        }
        if let Ok(mut queue) = self.shared.queue.lock() {
            for work in queue.iter() {
                let _ = work.handle.cancel();
            }
            queue.clear();
        }
        if let Ok(mut running) = self.running.lock()
            && let Some(work) = running.take()
        {
            let _ = work.handle.cancel();
            terminal(&work.handle, JobError::new(ErrorCode::Cancelled, "render host closed"));
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.shared.wake.notify_all();
        // Detaching cannot stall the UI; the bounded worker observes stop/cancel at its next unit.
    }
}

#[derive(Clone)]
pub struct ImageHost(Arc<Owner>);

/// Builtin CPU constructors share the registered image host lifetime machinery.
pub type CpuHost = ImageHost;

impl ImageHost {
    fn shared(limit: u64) -> Arc<Shared> {
        Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            active: Mutex::new(None),
            stop: AtomicBool::new(false),
            budget: Arc::new(Budget { used: AtomicU64::new(0), limit }),
            #[cfg(not(target_arch = "wasm32"))]
            wake: Condvar::new(),
        })
    }

    pub fn cooperative() -> Self {
        Self(Arc::new(Owner {
            executor: None,
            shared: Self::shared(MEMORY_LIMIT),
            kind: HostKind::Cooperative,
            running: Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            _worker: None,
        }))
    }

    /// A cooperative host over `executor` (pumped by its caller).
    #[cfg(test)]
    fn cooperative_with(executor: Arc<dyn ImageExecutor>) -> Self {
        Self(Arc::new(Owner {
            executor: Some(executor),
            shared: Self::shared(MEMORY_LIMIT),
            kind: HostKind::Cooperative,
            running: Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            _worker: None,
        }))
    }

    pub fn for_platform() -> Result<Self> {
        #[cfg(target_arch = "wasm32")]
        {
            Ok(Self::cooperative())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pool = if usize::BITS >= 64 { super::NATIVE_CPU_POOL_BYTES } else { MEMORY_LIMIT };
            let shared = Self::shared(pool);
            let worker_shared = shared.clone();
            let worker = std::thread::Builder::new()
                .name("solidcraft-render".into())
                .spawn(move || native_worker(worker_shared))
                .map_err(|e| JobError::new(ErrorCode::HostUnavailable, format!("could not start CPU image worker: {e}")))?;
            Ok(Self(Arc::new(Owner { executor: None, shared, kind: HostKind::NativeWorker, running: Mutex::new(None), _worker: Some(worker) })))
        }
    }

    /// Start an isolated bounded native worker. GPU discovery occurs before registration.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_executor(executor: Arc<dyn ImageExecutor>) -> Result<Self> {
        let descriptor = executor.descriptor()?;
        if descriptor.profiles.len() != 1
            || descriptor
                .profiles
                .iter()
                .any(|p| p.mode != RenderMode::Preview || p.operation != RenderOperation::Image || p.formats != [RenderFormat::Png])
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "image worker currently accepts one Preview/Image/PNG profile"));
        }
        let limit = executor.memory_limit_bytes();
        let ceiling = if usize::BITS >= 64 { super::NATIVE_CPU_POOL_BYTES } else { MEMORY_LIMIT };
        if limit == 0 || limit > ceiling {
            return Err(JobError::new(ErrorCode::InvalidRequest, "registered image pool exceeds supported address space"));
        }
        let shared = Self::shared(limit);
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new()
            .name("solidcraft-render".into())
            .spawn(move || native_worker(worker_shared))
            .map_err(|e| JobError::new(ErrorCode::HostUnavailable, format!("could not start image worker: {e}")))?;
        Ok(Self(Arc::new(Owner { executor: Some(executor), shared, kind: HostKind::NativeWorker, running: Mutex::new(None), _worker: Some(worker) })))
    }

    pub fn kind(&self) -> HostKind {
        self.0.kind
    }

    pub fn same_host(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn is_available(&self) -> bool {
        !self.0.shared.stop.load(Ordering::Acquire)
    }
    pub fn admitted_bytes(&self) -> u64 {
        self.0.shared.budget.used.load(Ordering::Acquire)
    }

    /// Internal completed-film adapter: the existing host budget, no new pool.
    pub(crate) fn reserve_film_snapshot(&self, bytes: u64) -> Result<Reservation> {
        if self.0.executor.is_some() {
            return Err(JobError::new(ErrorCode::InvalidRequest, "CPU film snapshots require the builtin CPU host"));
        }
        if !self.is_available() {
            return Err(JobError::new(ErrorCode::HostUnavailable, "CPU image host has stopped"));
        }
        if bytes > super::image::MAX_JOB_BYTES {
            return Err(JobError::new(ErrorCode::LimitExceeded, "film snapshot exceeds per-job admission"));
        }
        self.0.shared.budget.reserve(bytes)
    }

    pub fn descriptor(&self) -> Result<ExecutorDescriptor> {
        self.descriptor_for(Integrator::Raster)
    }

    pub fn job_limit_bytes(&self) -> u64 {
        if self.0.executor.is_some() { self.0.shared.budget.limit } else { super::image::MAX_JOB_BYTES }
    }

    pub fn pool_limit_bytes(&self) -> u64 {
        self.0.shared.budget.limit
    }

    fn capture_retained_context(&self, retained: Option<&RetainedMetadata>) -> Result<Option<u64>> {
        if retained.is_some_and(|lease| !Arc::ptr_eq(&lease.0.budget, &self.0.shared.budget)) {
            return Err(JobError::new(ErrorCode::WrongOwner, "retained metadata belongs to a different host pool"));
        }
        Ok(retained.map(RetainedMetadata::bytes))
    }
    fn capture_descriptor(&self, profile: CaptureProfile) -> Result<ExecutorDescriptor> {
        if matches!(profile, CaptureProfile::Registered) != self.0.executor.is_some() {
            return Err(JobError::new(ErrorCode::WrongOwner, "capture profile does not match the installed host"));
        }
        self.validate_integrator(profile.integrator())?;
        if let CaptureProfile::CpuRaytrace(budget) = profile {
            budget.validate_execution()?;
        }
        let descriptor = self.descriptor_for(profile.integrator())?;
        if descriptor.availability != Availability::Available || descriptor.profiles.len() != 1 {
            return Err(JobError::new(ErrorCode::HostUnavailable, "capture executor is unavailable"));
        }
        Ok(descriptor)
    }
    /// Fund the next capture peak without a scene/request, fake job or renderer
    /// allocation. The independent Plan lease is counted, never transferred.
    pub fn reserve_capture<'a>(
        &self,
        bytes: u64,
        profile: CaptureProfile,
        cancel: &CancelToken,
        retained: Option<&'a RetainedMetadata>,
    ) -> Result<CaptureLease<'a>> {
        cancel.check()?;
        let mut descriptor = self.capture_descriptor(profile)?;
        let context = self.capture_retained_context(retained)?;
        self.check_request_sum(bytes, context.unwrap_or(0), profile.budget())?;
        let descriptor_hash = capture_fingerprint(&descriptor)?;
        // Resolution canonicalizes these enum lists. Pin its same value order
        // without cloning/allocating another profile or altering the raw hash.
        for profile in &mut descriptor.profiles {
            profile.formats.sort_unstable();
            profile.features.sort_unstable();
        }
        let execution = capture_fingerprint(&(&descriptor.id, descriptor.profiles.first()))?;
        let reservation = self.0.shared.budget.reserve(bytes)?;
        Ok(CaptureLease {
            host: self.clone(),
            profile,
            descriptor: descriptor_hash,
            execution,
            retained,
            cancel: cancel.clone(),
            reservation,
            consumed: false,
        })
    }

    /// Strict blocking API. Native UI callers prepare beside the window. The
    /// temporary lease exists before any new box proof backing is allocated.
    pub fn admit(&self, scene: &ProductScene, request: &ImageRequest, options: JobOptions) -> Result<Admission> {
        let admission = self.admit_layout(scene, request, options)?;
        #[cfg(feature = "raytrace-cpu")]
        if self.0.executor.is_none() && request.integrator == Integrator::Raytrace {
            let defaults = super::RaytraceOptions::default();
            let raytrace = request.raytrace.as_ref().unwrap_or(&defaults);
            let bytes = crate::raytrace::image::certification_bytes(scene, raytrace)?;
            if bytes != 0 {
                let _validation = self.0.shared.budget.reserve(bytes)?;
                crate::raytrace::image::certify_for_request(scene, request, raytrace, &CancelToken::default())?;
            }
        }
        Ok(admission)
    }

    fn admit_layout(&self, scene: &ProductScene, request: &ImageRequest, options: JobOptions) -> Result<Admission> {
        let scatter = |t: Option<crate::transmission::Transmission>| t.and_then(|v| v.subsurface()).is_some_and(|s| s.active());
        if scene.meshes.iter().any(|b| scatter(b.transmission) || b.faces.iter().any(|f| scatter(f.transmission)))
            && !self.0.executor.as_ref().is_some_and(|executor| executor.supports_subsurface())
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "installed image producer does not implement interior subsurface random walks"));
        }

        self.validate_integrator(request.integrator)?;
        options.validate()?;
        if let Some(executor) = &self.0.executor {
            if request.raytrace.is_some() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "registered executors do not accept CPU raytrace options"));
            }
            if options.aging.active() && !executor.supports_aging() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "this executor cannot evaluate ordered surface aging"));
            }
            if options.finishing.photographic.active() && !executor.supports_photographic() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "this executor cannot apply photographic lens effects"));
            }
            if options.nested_media && !executor.supports_nested_media() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "registered executor does not implement certified nested media"));
            }
            if options.volume.is_some() && !executor.supports_volume() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "registered executor does not implement bounded homogeneous volume transport"));
            }
            if options.adaptive.is_some() && !executor.supports_adaptive_sampling() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "registered executor does not implement independent-pilot adaptive sampling"));
            }
            if options.transmission_samples != 1 && !executor.supports_transmission_sampling() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "registered executor does not implement transmission strata"));
            }
            if options.linear_exr && !executor.supports_linear_exr() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "this executor cannot retain a raw linear EXR"));
            }
            if options.finishing.bloom.is_some_and(|b| b.active()) && !executor.supports_bloom() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "registered executor does not implement display bloom"));
            }
            if (!options.finishing.is_identity() || options.backdrop.is_some()) && !executor.supports_finishing() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "this executor cannot apply the requested finishing/backdrop"));
            }
            let mut admission = executor.admit(scene, request, options)?;
            if let Some(samples) = executor.preview_samples(options) {
                if !(1..=options.preview_target()?).contains(&samples) {
                    return Err(JobError::new(ErrorCode::InvalidRequest, "executor preview target exceeds captured sampling passes"));
                }
                let slots = samples.saturating_sub(1).min(2);
                if slots != 0 {
                    let previews = admission
                        .encoded_limit
                        .checked_add(FRAME_METADATA_BYTES)
                        .and_then(|bytes| bytes.checked_mul(u64::from(slots)))
                        .and_then(|bytes| bytes.checked_add(FRAME_METADATA_BYTES))
                        .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "executor preview admission overflow"))?;
                    admission.estimated_total_bytes = admission
                        .estimated_total_bytes
                        .checked_add(previews)
                        .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "executor preview admission overflow"))?;
                }
            }
            if admission.estimated_total_bytes > self.job_limit_bytes() {
                return Err(JobError::new(ErrorCode::LimitExceeded, "registered image job exceeds its admitted host pool"));
            }
            Ok(admission)
        } else {
            request.cpu_budget().validate_execution()?;
            if options != JobOptions::default() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "CPU images use raytrace options, not progressive sampling controls"));
            }
            super::image::admit_layout(scene, request)
        }
    }

    fn check_request_sum(&self, work: u64, request: u64, budget: super::CpuBudget) -> Result<()> {
        if self.0.executor.is_none() {
            if budget == super::CpuBudget::Compact {
                return admit_request_sum(work, request);
            }
            if work.checked_add(request).is_none_or(|n| n > budget.limits().max_job_bytes) {
                return Err(JobError::new(ErrorCode::LimitExceeded, "CPU work and retained request exceed the selected profile"));
            }
            return Ok(());
        }
        if work.checked_add(request).is_none_or(|n| n > self.job_limit_bytes()) {
            return Err(JobError::new(ErrorCode::LimitExceeded, "registered image job exceeds its admitted host pool"));
        }
        Ok(())
    }

    /// Check the execution choice without capturing geometry or reserving memory.
    pub fn validate_integrator(&self, integrator: Integrator) -> Result<()> {
        if self.0.executor.is_some() && integrator != Integrator::Raster {
            return Err(JobError::new(ErrorCode::InvalidRequest, "registered image executors do not accept CPU raytrace requests"));
        }
        if !self.is_available() {
            return Err(JobError::new(ErrorCode::HostUnavailable, "image host has stopped"));
        }
        if integrator == Integrator::Raytrace && !cfg!(feature = "raytrace-cpu") {
            return Err(JobError::new(ErrorCode::HostUnavailable, "CPU raytracing is not compiled in; enable raytrace-cpu"));
        }
        if integrator == Integrator::Raytrace && self.kind() != HostKind::NativeWorker {
            return Err(JobError::new(ErrorCode::HostUnavailable, "CPU raytracing requires the native worker host"));
        }
        Ok(())
    }

    /// Select exactly one executor for this request. Never rely on registry order
    /// to choose between raster and raytracing implementations.
    pub fn descriptor_for(&self, integrator: Integrator) -> Result<ExecutorDescriptor> {
        if let Some(executor) = &self.0.executor {
            if integrator != Integrator::Raster {
                return Err(JobError::new(ErrorCode::InvalidRequest, "registered image executors do not accept CPU raytrace requests"));
            }
            let mut descriptor = executor.descriptor()?;
            if !self.is_available() {
                descriptor.availability = Availability::Unavailable { reason: "image host has stopped".into() };
            }
            return Ok(descriptor);
        }
        if integrator == Integrator::Raytrace && !cfg!(feature = "raytrace-cpu") {
            return Err(JobError::new(ErrorCode::HostUnavailable, "CPU raytracing is not compiled in; enable raytrace-cpu"));
        }
        Ok(ExecutorDescriptor {
            id: ExecutorId::new(match integrator {
                Integrator::Raster => "cpu-product",
                Integrator::Raytrace => "cpu-raytrace",
            })
            .map_err(|e| JobError::new(ErrorCode::Internal, e.to_string()))?,
            availability: if self.0.shared.stop.load(Ordering::Acquire) {
                Availability::Unavailable { reason: "image host has stopped".into() }
            } else if integrator == Integrator::Raytrace && self.kind() != HostKind::NativeWorker {
                Availability::Unavailable { reason: "CPU raytracing requires the native worker host".into() }
            } else {
                Availability::Available
            },
            profiles: vec![RenderProfile {
                mode: RenderMode::Preview,
                operation: RenderOperation::Image,
                formats: vec![RenderFormat::Png],
                features: if integrator == Integrator::Raytrace && cfg!(feature = "raytrace-cpu") && self.kind() == HostKind::NativeWorker {
                    // Refraction is the certified thin-sheet/axis-aligned-box CPU preview
                    // contract. Admission still refuses unsupported geometry and combinations.
                    vec![crate::RenderFeature::Pbr, crate::RenderFeature::Hdri, crate::RenderFeature::Refraction]
                } else {
                    vec![crate::RenderFeature::Pbr]
                },
            }],
            limits: if integrator == Integrator::Raytrace && self.kind() == HostKind::NativeWorker && usize::BITS >= 64 {
                super::CpuBudget::Workstation.image_limits(false)
            } else {
                RenderLimits::default()
            },
            scratch: ScratchLayout { sample_scale: 2, sample_bytes_per_pixel: 8, output_bytes_per_pixel: 8, encoding_bytes_per_pixel: 8 },
        })
    }

    /// Resolve a CPU request against its explicit envelope, never the largest advertised one.
    pub fn descriptor_for_request(&self, request: &ImageRequest) -> Result<ExecutorDescriptor> {
        let mut descriptor = self.descriptor_for(request.integrator)?;
        if self.0.executor.is_none() && request.integrator == Integrator::Raytrace {
            let budget = request.cpu_budget();
            budget.validate_execution()?;
            budget.validate_image(request.width, request.height, request.raytrace.as_ref().and_then(|o| o.progressive))?;
            descriptor.limits = budget.image_limits(request.raytrace.as_ref().is_some_and(|o| o.progressive.is_some()));
        }
        Ok(descriptor)
    }

    pub fn submit(&self, id: JobId, scene_generation: u64, scene: ProductScene, request: ImageRequest) -> Result<JobHandle> {
        self.submit_with_environment(id, scene_generation, scene, request, None)
    }

    /// Captured HDR ownership stays in work, never serialized/cloned into status.
    pub fn submit_with_environment(
        &self,
        id: JobId,
        scene_generation: u64,
        scene: ProductScene,
        request: ImageRequest,
        environment: Option<crate::lighting::environment_input::EnvironmentInput>,
    ) -> Result<JobHandle> {
        self.submit_with_environment_and_options(id, scene_generation, scene, request, JobOptions::default(), environment)
    }

    pub fn submit_with_options(
        &self,
        id: JobId,
        scene_generation: u64,
        scene: ProductScene,
        request: ImageRequest,
        options: JobOptions,
    ) -> Result<JobHandle> {
        self.submit_with_environment_and_options(id, scene_generation, scene, request, options, None)
    }

    pub fn submit_with_environment_and_options(
        &self,
        id: JobId,
        scene_generation: u64,
        scene: ProductScene,
        request: ImageRequest,
        options: JobOptions,
        environment: Option<crate::lighting::environment_input::EnvironmentInput>,
    ) -> Result<JobHandle> {
        if id.session == 0 || id.serial == 0 || id.generation == 0 {
            return Err(JobError::new(ErrorCode::InvalidRequest, "invalid render job identity"));
        }
        let prepared = self.prepare_submission(scene, request, options, environment, CancelToken::default())?;
        self.submit_prepared(id, scene_generation, prepared)
    }

    /// Strict captured preparation. All new certificate backing is reserved
    /// before allocation; the opaque capsule owns resources and leases to enqueue.
    pub fn prepare_submission(
        &self,
        scene: ProductScene,
        request: ImageRequest,
        options: JobOptions,
        environment: Option<crate::lighting::environment_input::EnvironmentInput>,
        cancel: CancelToken,
    ) -> Result<PreparedSubmission> {
        self.prepare_submission_with_metadata(scene, request, options, environment, cancel, None)
    }
    /// The external owner already holds this independent same-pool lease. Count
    /// its simultaneous backing before certificate/renderer allocation, without
    /// reserving it twice or transferring its lifetime into Work.
    pub fn prepare_submission_with_metadata(
        &self,
        scene: ProductScene,
        request: ImageRequest,
        options: JobOptions,
        environment: Option<crate::lighting::environment_input::EnvironmentInput>,
        cancel: CancelToken,
        retained: Option<&RetainedMetadata>,
    ) -> Result<PreparedSubmission> {
        self.prepare_submission_inner(scene, request, options, environment, cancel, retained, None)
    }
    /// Consume already-owned capture credit. On any inner refusal/unwind, its
    /// typed inputs drop before this outer lease. After PreparedSubmission owns
    /// those inputs, merge credit and use the original Work/request split.
    pub fn prepare_submission_with_capture(
        &self,
        scene: ProductScene,
        request: ImageRequest,
        options: JobOptions,
        environment: Option<crate::lighting::environment_input::EnvironmentInput>,
        cancel: CancelToken,
        retained: Option<&RetainedMetadata>,
        mut capture: CaptureLease<'_>,
    ) -> Result<PreparedSubmission> {
        self.prepare_submission_inner(scene, request, options, environment, cancel, retained, Some(&mut capture))
    }
    fn prepare_submission_inner(
        &self,
        scene: ProductScene,
        request: ImageRequest,
        options: JobOptions,
        environment: Option<crate::lighting::environment_input::EnvironmentInput>,
        cancel: CancelToken,
        retained: Option<&RetainedMetadata>,
        capture: Option<&mut CaptureLease<'_>>,
    ) -> Result<PreparedSubmission> {
        if let Some(credit) = capture.as_deref() {
            credit.check_request(self, &request, &cancel, retained)?;
        }
        cancel.check()?;
        if retained.is_some_and(|lease| !Arc::ptr_eq(&lease.0.budget, &self.0.shared.budget)) {
            return Err(JobError::new(ErrorCode::WrongOwner, "retained metadata belongs to a different host pool"));
        }
        if !self.is_available() {
            return Err(JobError::new(ErrorCode::HostUnavailable, "image host has stopped"));
        }
        if self.0.executor.is_some() && environment.is_some() {
            return Err(JobError::new(ErrorCode::InvalidRequest, "registered image executors do not accept CPU environments"));
        }
        if environment.is_some() && request.integrator != Integrator::Raytrace {
            return Err(JobError::new(ErrorCode::InvalidRequest, "environment lighting requires CPU raytrace"));
        }
        self.validate_integrator(request.integrator)?;
        if environment.is_some() {
            request.raytrace.as_ref().unwrap_or(&super::RaytraceOptions::default()).validate_environment_reflections(&scene)?;
        }
        let environment_info = environment
            .as_ref()
            .map(|e| JobEnvironmentInfo::Cpu(e.metadata()))
            .or_else(|| request.environment.as_ref().map(|source| JobEnvironmentInfo::Gpu(source.info().clone())));
        let textures = request.textures.as_ref().map(|source| source.summary());
        let metadata_bytes =
            if environment.is_some() { std::mem::size_of::<crate::lighting::environment_input::EnvironmentInfo>() as u64 } else { 0 }
                + request.retained_resource_metadata_bytes();
        let request_bytes = retained_request_bytes(&request)?
            .checked_add(options.retained_bytes()?)
            .and_then(|bytes| bytes.checked_add(metadata_bytes))
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "environment metadata overflow"))?;
        let mut features = scene.required_features();
        if environment.is_some() || request.environment.is_some() {
            features.push(crate::RenderFeature::Hdri);
        }
        if request.integrator == Integrator::Raytrace
            && request.raytrace.as_ref().is_some_and(|options| !options.dielectrics.is_empty())
            && !features.contains(&crate::RenderFeature::Refraction)
        {
            features.push(crate::RenderFeature::Refraction);
        }
        let mut admission = self.admit_layout(&scene, &request, options)?;
        let estimated_total_bytes = admission.estimated_total_bytes;
        let environment_bytes =
            environment.as_ref().map_or(Ok(0), |e| e.required_bytes()).map_err(|e| JobError::new(ErrorCode::InvalidRequest, e.to_string()))?;
        let estimated_total_bytes = estimated_total_bytes
            .checked_add(environment_bytes)
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "environment admission overflow"))?;
        let simultaneous = request_bytes
            .checked_add(retained.map_or(0, RetainedMetadata::bytes))
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "retained preparation byte overflow"))?;
        self.check_request_sum(estimated_total_bytes, simultaneous, request.cpu_budget())?;
        let mut backend = crate::builtin_backend();
        let descriptor = self.descriptor_for_request(&request)?;
        if let Some(credit) = capture.as_deref()
            && credit.profile == CaptureProfile::Registered
            && capture_fingerprint(&descriptor)? != credit.descriptor
        {
            return Err(JobError::new(ErrorCode::HostUnavailable, "resolved capture executor metadata changed"));
        }
        let expected_executor = descriptor.id.clone();
        backend.executors = vec![descriptor];
        let resolved = crate::resolve_request(
            &[backend],
            &crate::RenderRequest {
                backend: None,
                mode: RenderMode::Preview,
                operation: RenderOperation::Image,
                width: u64::from(request.width),
                height: u64::from(request.height),
                format: Some(RenderFormat::Png),
                features,
                policy: Default::default(),
            },
        )
        .map_err(|e| JobError::new(ErrorCode::HostUnavailable, e.to_string()))?;
        if resolved.executor != expected_executor {
            return Err(JobError::new(ErrorCode::HostUnavailable, "CPU product executor is unavailable"));
        }
        // Reap cancelled queued captures on the preparation caller, before a
        // replacement competes for their still-held credit. Never hold this
        // lock while geometry is certified.
        {
            let mut queue = self.0.shared.queue.lock().map_err(|_| JobError::new(ErrorCode::HostUnavailable, "render queue lock poisoned"))?;
            queue.retain(|work| !work.handle.0.cancel.is_cancelled());
        }
        let combined_bytes = estimated_total_bytes
            .checked_add(request_bytes)
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "prepared work/request byte overflow"))?;
        if let Some(credit) = capture.as_deref()
            && capture_fingerprint(&(&resolved.executor, Some(&resolved.profile)))? != credit.execution
        {
            return Err(JobError::new(ErrorCode::WrongOwner, "resolved capture executor/profile changed"));
        }
        let already_held = capture.as_deref().map_or(0, CaptureLease::bytes);
        // A retired pending duplicate/validator can make capture's high-water
        // peak larger than the final renderer layout. Keep that conservative
        // excess in Work until release, and report it as preparation backing.
        // The independent Plan is still counted once under the same cap.
        if already_held > combined_bytes {
            self.check_request_sum(already_held, retained.map_or(0, RetainedMetadata::bytes), request.cpu_budget())?;
            let extra = already_held - combined_bytes;
            admission.preparation_bytes = admission
                .preparation_bytes
                .checked_add(extra)
                .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "capture preparation admission overflow"))?;
            admission.estimated_total_bytes = admission
                .estimated_total_bytes
                .checked_add(extra)
                .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "capture work admission overflow"))?;
        }
        let mut reservation = self.0.shared.budget.reserve(combined_bytes.saturating_sub(already_held))?;
        // The legacy None branch keeps its original debit and split. Captured
        // inputs first move under PreparedSubmission's drop order, while the
        // outer caller still owns already_held, before transfer/split can fail.
        let request_reservation = if capture.is_some() || request_bytes == 0 { None } else { Some(reservation.split(request_bytes)?) };
        // Preserve the original CPU option capacities in the retained request and its lease.
        // Registered executors and textured CPU jobs move resource Arcs into work
        // and keep a small public copy under the retained metadata lease.
        let (public_request, work_request) = if self.0.executor.is_some() || request.textures.is_some() {
            (ImageRequest { environment: None, textures: None, ..request.clone() }, Some(request))
        } else {
            (request, None)
        };

        // Move all captured ownership under fields that drop before the leases
        // before any remaining fallible certificate/options allocation.
        let mut prepared = PreparedSubmission {
            scene,
            public_request,
            work_request,
            options: None,
            environment,
            environment_info,
            textures,
            resolved,
            admission,
            cancel,
            #[cfg(feature = "raytrace-cpu")]
            proof: Vec::new(),
            request_reservation,
            reservation,
            host: self.clone(),
        };
        if let Some(credit) = capture {
            prepared.reservation.merge_from(&mut credit.reservation)?;
            credit.consumed = true;
            prepared.request_reservation = if request_bytes == 0 { None } else { Some(prepared.reservation.split(request_bytes)?) };
        }
        #[cfg(feature = "raytrace-cpu")]
        if self.0.executor.is_none() && prepared.request().integrator == Integrator::Raytrace {
            let captured = prepared.request();
            let defaults = super::RaytraceOptions::default();
            let raytrace = captured.raytrace.as_ref().unwrap_or(&defaults);
            if let Some(certified) = crate::raytrace::image::certify_for_request(&prepared.scene, captured, raytrace, &prepared.cancel)? {
                prepared
                    .proof
                    .try_reserve_exact(1)
                    .map_err(|_| JobError::new(ErrorCode::AllocationFailed, "could not retain certified optical result"))?;
                if prepared.proof.capacity() != 1 {
                    return Err(JobError::new(ErrorCode::LimitExceeded, "optical proof capacity exceeds admission"));
                }
                prepared.proof.push(certified);
            }
        }
        prepared.cancel.check()?;
        prepared.options = if options.is_structural_default() { None } else { Some(RetainedOptions::new(options)?) };
        Ok(prepared)
    }

    /// Bounded publication into the existing queue. The real owning Session
    /// supplies identity only after source/host revalidation; no proof is redrained.
    pub fn submit_prepared(&self, id: JobId, scene_generation: u64, prepared: PreparedSubmission) -> Result<JobHandle> {
        self.try_submit_prepared(id, scene_generation, &mut Some(prepared))
    }

    /// Non-consuming failure path for native completion. On refusal the caller
    /// retains the complete capsule and its leases for worker-side retirement.
    /// Success takes it once, after all fallible queue/identity checks.
    pub fn try_submit_prepared(&self, id: JobId, scene_generation: u64, slot: &mut Option<PreparedSubmission>) -> Result<JobHandle> {
        let prepared = slot.as_ref().ok_or_else(|| JobError::new(ErrorCode::InvalidRequest, "prepared render was already consumed"))?;
        if id.session == 0 || id.serial == 0 || id.generation == 0 {
            return Err(JobError::new(ErrorCode::InvalidRequest, "invalid render job identity"));
        }
        if !self.same_host(&prepared.host) {
            return Err(JobError::new(ErrorCode::WrongOwner, "prepared render belongs to a different installed host"));
        }
        prepared.cancel.check()?;
        let options = prepared.options()?;
        let mut queue = self.0.shared.queue.lock().map_err(|_| JobError::new(ErrorCode::HostUnavailable, "render queue lock poisoned"))?;
        if self.0.shared.stop.load(Ordering::Acquire) {
            return Err(JobError::new(ErrorCode::HostUnavailable, "image host has stopped"));
        }
        // Background preparation/worker owns cancelled capture retirement.
        // A racing cancellation may keep this bounded queue full until drain;
        // completion must not destroy large request/source backing on the UI.
        if queue.len() >= QUEUE_LIMIT {
            return Err(JobError::new(ErrorCode::QueueFull, "image queue is full"));
        }
        let prepared = slot.take().ok_or_else(|| JobError::new(ErrorCode::Internal, "prepared render vanished before enqueue"))?;
        let PreparedSubmission {
            scene,
            public_request,
            work_request,
            options: retained_options,
            environment,
            environment_info,
            textures,
            resolved,
            admission: _,
            cancel,
            #[cfg(feature = "raytrace-cpu")]
            proof,
            request_reservation,
            reservation,
            host: _,
        } = prepared;
        let handle = JobHandle(Arc::new(Record {
            work_released: AtomicBool::new(false),
            preview_unavailable_reason: self.0.executor.as_ref().and_then(|executor| executor.preview_unavailable_reason(options)),
            started: web_time::Instant::now(),
            presentation: None,
            textures,
            options: retained_options,
            environment: environment_info,
            _request_reservation: request_reservation,
            id,
            scene_generation,
            request: public_request,
            resolved,
            cancel,
            data: Mutex::new(Data {
                preview_retention_skips: 0,
                completed_duration: None,
                punctual_diagnostics: None,
                sampling: None,
                progress: Progress { phase: Phase::Queued, done: 0, total: 1 },
                error: None,
                frame: None,
                preview: None,
            }),
            #[cfg(not(target_arch = "wasm32"))]
            done: Condvar::new(),
        }));
        queue.push_back(Work {
            regrade: None,
            #[cfg(feature = "raytrace-cpu")]
            proof,
            request: work_request,
            executor: self.0.executor.clone(),
            scene,
            handle: handle.clone(),
            reservation,
            environment,
            _work_release: WorkRelease(Arc::downgrade(&handle.0)),
        });
        drop(queue);
        #[cfg(not(target_arch = "wasm32"))]
        self.0.shared.wake.notify_one();
        Ok(handle)
    }

    /// Regrade the exact retained native master. No geometry, material, light or
    /// device work is submitted. The original frame and its lease remain shared.
    pub fn submit_regrade(&self, id: JobId, source: &JobHandle, presentation: super::presentation::Presentation) -> Result<JobHandle> {
        if id.session == 0 || id.serial == 0 || id.generation == 0 || id.session != source.id().session || id == source.id() {
            return Err(JobError::new(ErrorCode::WrongOwner, "regrade requires a new identity in the source document session"));
        }
        let frame = source.frame()?.ok_or_else(|| JobError::new(ErrorCode::NotCompleted, "regrade requires a completed frame"))?;
        let master = frame.master_source()?;
        let bytes = super::regrade::required_bytes(&master, presentation)?;
        let raw = master.linear_master_info().ok_or_else(|| JobError::new(ErrorCode::InvalidRequest, "raw master metadata is missing"))?;
        let origin = super::presentation::MasterPresentation {
            source_job: source.id(),
            master_job: master.info.job,
            master_sha256: raw.sha256.clone(),
            presentation,
            denoise_applied: false,
        };
        let options = JobOptions {
            finishing: presentation.finishing,
            backdrop: presentation.backdrop,
            exposure_ev: presentation.exposure_ev,
            tone_map: presentation.tone_map,
            ..Default::default()
        };
        let option_bytes = options.retained_bytes()?;
        let request_bytes = option_bytes
            .checked_add(ImageRequest::fixed_cpu_metadata_bytes())
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "master request metadata overflow"))?;
        let metadata_bytes = (std::mem::size_of::<RetainedPresentation>() + origin.master_sha256.capacity() + 32) as u64;
        if bytes.checked_add(metadata_bytes).and_then(|n| n.checked_add(request_bytes)).is_none_or(|n| n > self.job_limit_bytes()) {
            return Err(JobError::new(ErrorCode::LimitExceeded, "master presentation exceeds the installed host's resource budget"));
        }
        let limits = RenderLimits { max_scratch_bytes: self.job_limit_bytes(), ..super::GPU_PATH_LIMITS };
        let descriptor = ExecutorDescriptor {
            id: ExecutorId::new("linear-master").map_err(|e| JobError::new(ErrorCode::Internal, e.to_string()))?,
            availability: Availability::Available,
            profiles: vec![RenderProfile {
                mode: RenderMode::Preview,
                operation: RenderOperation::Image,
                formats: vec![RenderFormat::Png],
                features: Vec::new(),
            }],
            limits,
            scratch: ScratchLayout { sample_scale: 1, sample_bytes_per_pixel: 32, output_bytes_per_pixel: 8, encoding_bytes_per_pixel: 8 },
        };
        let mut backend = crate::builtin_backend();
        backend.executors = vec![descriptor];
        let resolved = crate::resolve_request(
            &[backend],
            &crate::RenderRequest {
                backend: None,
                mode: RenderMode::Preview,
                operation: RenderOperation::Image,
                width: u64::from(frame.info.width),
                height: u64::from(frame.info.height),
                format: Some(RenderFormat::Png),
                features: Vec::new(),
                policy: Default::default(),
            },
        )
        .map_err(|e| JobError::new(ErrorCode::InvalidRequest, e.to_string()))?;
        let request = ImageRequest {
            studio_weights: None,
            width: frame.info.width,
            height: frame.info.height,
            camera: source.0.request.camera,
            background: presentation.background,
            transparent_background: presentation.transparent_background,
            environment: None,
            textures: None,
            integrator: Integrator::Raster,
            raytrace: None,
        };
        let mut queue = self.0.shared.queue.lock().map_err(|_| JobError::new(ErrorCode::HostUnavailable, "render queue lock poisoned"))?;
        if self.0.shared.stop.load(Ordering::Acquire) {
            return Err(JobError::new(ErrorCode::HostUnavailable, "image host has stopped"));
        }
        queue.retain(|work| !work.handle.0.cancel.is_cancelled());
        if queue.len() >= QUEUE_LIMIT {
            return Err(JobError::new(ErrorCode::QueueFull, "image queue is full"));
        }
        let reservation = self.0.shared.budget.reserve(bytes)?;
        let metadata = self.0.shared.budget.reserve(metadata_bytes)?;
        let option_reservation = if request_bytes == 0 { None } else { Some(self.0.shared.budget.reserve(request_bytes)?) };
        let origin = Arc::new(RetainedPresentation { info: origin, _reservation: metadata });
        let handle = JobHandle(Arc::new(Record {
            work_released: AtomicBool::new(false),
            preview_unavailable_reason: None,
            started: web_time::Instant::now(),
            presentation: Some(origin),
            textures: None,
            options: if options.is_structural_default() { None } else { Some(RetainedOptions::new(options)?) },
            environment: None,
            _request_reservation: option_reservation,
            id,
            scene_generation: frame.info.scene_generation,
            request,
            resolved,
            cancel: CancelToken::default(),
            data: Mutex::new(Data {
                preview_retention_skips: 0,
                completed_duration: None,
                punctual_diagnostics: None,
                sampling: None,
                progress: Progress { phase: Phase::Queued, done: 0, total: 1 },
                error: None,
                frame: None,
                preview: None,
            }),
            #[cfg(not(target_arch = "wasm32"))]
            done: Condvar::new(),
        }));
        queue.push_back(Work {
            #[cfg(feature = "raytrace-cpu")]
            proof: Vec::new(),
            regrade: Some((master, presentation)),
            request: None,
            executor: None,
            environment: None,
            scene: ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY },
            handle: handle.clone(),
            reservation,
            _work_release: WorkRelease(Arc::downgrade(&handle.0)),
        });
        drop(queue);
        #[cfg(not(target_arch = "wasm32"))]
        self.0.shared.wake.notify_one();
        Ok(handle)
    }

    /// Cooperative callers pump a small quantum independently of polling a job's status.
    /// Native jobs run even when the UI is idle or a headless caller is not polling.
    pub fn pump(&self, units: usize) -> Result<bool> {
        if self.0.kind != HostKind::Cooperative {
            return Ok(false);
        }
        let mut running = self.0.running.lock().map_err(|_| JobError::new(ErrorCode::HostUnavailable, "cooperative host lock poisoned"))?;
        if running.is_none() {
            let work = self.0.shared.queue.lock().map_err(|_| JobError::new(ErrorCode::HostUnavailable, "render queue lock poisoned"))?.pop_front();
            if let Some(work) = work {
                *running = Running::begin(work);
            }
        }
        if let Some(work) = running.as_mut() {
            if let Ok(mut active) = self.0.shared.active.lock() {
                *active = Some(Arc::downgrade(&work.handle.0));
            }
            if !work.guarded_step(units) {
                *running = None;
                if let Ok(mut active) = self.0.shared.active.lock() {
                    *active = None;
                }
            }
        }
        Ok(running.is_some()
            || !self.0.shared.queue.lock().map_err(|_| JobError::new(ErrorCode::HostUnavailable, "render queue lock poisoned"))?.is_empty())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_worker(shared: Arc<Shared>) {
    loop {
        let mut queue = match shared.queue.lock() {
            Ok(q) => q,
            Err(_) => {
                shared.stop.store(true, Ordering::Release);
                return;
            }
        };
        while queue.is_empty() && !shared.stop.load(Ordering::Acquire) {
            queue = match shared.wake.wait(queue) {
                Ok(q) => q,
                Err(_) => {
                    shared.stop.store(true, Ordering::Release);
                    return;
                }
            };
        }
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        let work = queue.pop_front();
        drop(queue);
        let Some(mut running) = work.and_then(Running::begin) else {
            continue;
        };
        if let Ok(mut active) = shared.active.lock() {
            *active = Some(Arc::downgrade(&running.handle.0));
        }
        loop {
            if shared.stop.load(Ordering::Acquire) {
                let _ = running.handle.cancel();
            }
            if !running.guarded_step(4096) {
                break;
            }
        }
        if let Ok(mut active) = shared.active.lock() {
            *active = None;
        }
    }
}
#[cfg(test)]
mod retained_request_tests {
    use super::*;

    #[test]
    fn retained_native_options_charge_actual_single_capacity_only_for_structural_nondefaults() {
        assert_eq!(JobOptions::default().retained_bytes().unwrap(), 0);
        let explicit = JobOptions { lighting: Some(super::super::StudioLighting::default()), ..Default::default() };
        let expected = std::mem::size_of::<JobOptions>() as u64;
        assert_eq!(explicit.retained_bytes().unwrap(), expected);
        let slot = RetainedOptions::new(explicit).unwrap();
        assert_eq!(slot.0.capacity(), 1);
        assert_eq!(slot.0.len(), 1);
        assert_eq!(slot.0[0], explicit);
        assert_ne!(slot.0[0], JobOptions::default(), "Some(black studio) must not become an absent default studio");
        assert!(std::mem::size_of::<Option<RetainedOptions>>() <= std::mem::size_of::<JobOptions>());
        let mut vignette = JobOptions::default();
        vignette.finishing.vignette = -0.0;
        let mut grain = JobOptions::default();
        grain.finishing.photographic.grain = -0.0;
        for options in [JobOptions { exposure_ev: -0.0, ..Default::default() }, vignette, grain] {
            assert!(!options.is_structural_default());
            assert_eq!(options.retained_bytes().unwrap(), expected);
            assert_eq!(RetainedOptions::new(options).unwrap().0[0].exposure_ev.to_bits(), options.exposure_ev.to_bits());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn retained_gpu_options_lease_survives_work_release_frame_eviction_and_cloned_handles() {
        struct OptionsExecutor;
        impl ImageExecutor for OptionsExecutor {
            fn descriptor(&self) -> Result<ExecutorDescriptor> {
                CpuExecutor.descriptor()
            }
            fn admit(&self, scene: &ProductScene, request: &ImageRequest, options: JobOptions) -> Result<Admission> {
                options.validate()?;
                admit(scene, request)
            }
            fn begin(&self, scene: ProductScene, request: ImageRequest, options: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
                options.validate()?;
                CpuExecutor.begin(scene, request, options, cancel)
            }
        }
        for options in [
            JobOptions::default(),
            JobOptions { exposure_ev: -0.0, ..Default::default() },
            JobOptions { lighting: Some(super::super::StudioLighting::default()), ..Default::default() },
        ] {
            let mut host = ImageHost::with_executor(Arc::new(OptionsExecutor)).unwrap();
            let mut image = request();
            image.integrator = Integrator::Raster;
            image.raytrace = None;
            let expected = options.retained_bytes().unwrap();
            let job = host
                .submit_with_options(
                    JobId { session: 901, serial: 1, generation: 1 },
                    1,
                    ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY },
                    image,
                    options,
                )
                .unwrap();
            assert_eq!(job.status().unwrap().options, options);
            assert_eq!(job.status().unwrap().options.exposure_ev.to_bits(), options.exposure_ev.to_bits());
            assert_eq!(job.0.options.is_some(), expected != 0);
            if let Some(slot) = &job.0.options {
                assert_eq!(slot.0.capacity() as u64 * std::mem::size_of::<JobOptions>() as u64, expected);
            }
            assert_eq!(job.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
            host.0.shared.stop.store(true, Ordering::Release);
            host.0.shared.wake.notify_all();
            Arc::get_mut(&mut host.0).unwrap()._worker.take().unwrap().join().unwrap();
            assert!(job.work_released());
            let frame = job.frame().unwrap().unwrap();
            let held = job.clone();
            assert!(Arc::ptr_eq(&held.0, &job.0));
            job.0.data.lock().unwrap().frame = None;
            drop(frame);
            assert_eq!(host.admitted_bytes(), expected + ImageRequest::fixed_cpu_metadata_bytes());
            assert_eq!(held.status().unwrap().options, options);
            drop(job);
            assert_eq!(host.admitted_bytes(), expected + ImageRequest::fixed_cpu_metadata_bytes());
            drop(held);
            assert_eq!(host.admitted_bytes(), 0);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cancelled_gpu_options_are_retained_once_without_work_or_frame_credit() {
        struct Pending(CancelToken);
        impl ImageWork for Pending {
            fn step(&mut self, _: usize) -> Result<TaskPoll> {
                self.0.check()?;
                Ok(TaskPoll::Pending(Progress { phase: Phase::Running, done: 0, total: 1 }))
            }
        }
        struct Executor(std::sync::mpsc::SyncSender<()>);
        impl ImageExecutor for Executor {
            fn descriptor(&self) -> Result<ExecutorDescriptor> {
                CpuExecutor.descriptor()
            }
            fn admit(&self, scene: &ProductScene, request: &ImageRequest, options: JobOptions) -> Result<Admission> {
                options.validate()?;
                admit(scene, request)
            }
            fn begin(&self, _: ProductScene, _: ImageRequest, _: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
                self.0.send(()).unwrap();
                Ok(Box::new(Pending(cancel)))
            }
        }
        let light = super::super::DirectionalLightOptions { direction: [0.0, 0.0, 2.0], color: [0.25, 0.5, 1.0], intensity: 0.0 };
        let options = JobOptions {
            lighting: Some(super::super::StudioLighting {
                directional_lights: super::super::DirectionalLights::new(&[light; 8]).unwrap(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (began, received) = std::sync::mpsc::sync_channel(1);
        let mut host = ImageHost::with_executor(Arc::new(Executor(began))).unwrap();
        let mut image = request();
        image.integrator = Integrator::Raster;
        image.raytrace = None;
        let job = host
            .submit_with_options(
                JobId { session: 902, serial: 1, generation: 1 },
                1,
                ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY },
                image,
                options,
            )
            .unwrap();
        // Raw worker-stop below does not drain Shared::queue (Owner::drop does).
        // Establish actual begin before cancellation so join proves this active
        // work has dropped, independently of native worker scheduling.
        received.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
        job.cancel().unwrap();
        assert_eq!(job.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, Phase::Cancelled);
        host.0.shared.stop.store(true, Ordering::Release);
        host.0.shared.wake.notify_all();
        Arc::get_mut(&mut host.0).unwrap()._worker.take().unwrap().join().unwrap();
        assert!(job.work_released());
        assert!(job.frame().unwrap().is_none());
        assert_eq!(job.status().unwrap().options, options);
        assert_eq!(host.admitted_bytes(), options.retained_bytes().unwrap() + ImageRequest::fixed_cpu_metadata_bytes());
        let held = job.clone();
        drop(job);
        assert_eq!(held.status().unwrap().options, options);
        drop(held);
        assert_eq!(host.admitted_bytes(), 0);
    }

    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn cpu_master_regrade_retains_exact_options_and_raw_bytes_under_separate_metadata_leases() {
        let mut host = CpuHost::for_platform().unwrap();
        let mut image = request();
        image.raytrace.as_mut().unwrap().output = Some(Box::new(super::super::OutputOptions { linear_exr: true, ..Default::default() }));
        let original = host
            .submit(
                JobId { session: 903, serial: 1, generation: 1 },
                1,
                ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY },
                image,
            )
            .unwrap();
        assert_eq!(original.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
        assert!(original.0.options.is_none(), "CPU transport options retain their existing request lease, not a second native options slot");
        let original_frame = original.frame().unwrap().unwrap();
        let presentation = super::super::presentation::Presentation { exposure_ev: 1.0, ..Default::default() };
        let derivative = host.submit_regrade(JobId { session: 903, serial: 2, generation: 2 }, &original, presentation).unwrap();
        assert_eq!(derivative.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
        host.0.shared.stop.store(true, Ordering::Release);
        host.0.shared.wake.notify_all();
        Arc::get_mut(&mut host.0).unwrap()._worker.take().unwrap().join().unwrap();
        let frame = derivative.frame().unwrap().unwrap();
        assert_eq!(frame.linear_exr().unwrap(), original_frame.linear_exr().unwrap());
        assert_eq!(derivative.status().unwrap().options.exposure_ev, 1.0);
        let option_bytes = derivative.0.options().unwrap().retained_bytes().unwrap();
        let request_bytes = option_bytes + ImageRequest::fixed_cpu_metadata_bytes();
        assert_eq!(derivative.0._request_reservation.as_ref().unwrap().bytes, request_bytes);
        let presentation_bytes = derivative.0.presentation.as_ref().unwrap()._reservation.bytes;
        derivative.0.data.lock().unwrap().frame = None;
        original.0.data.lock().unwrap().frame = None;
        drop(frame);
        drop(original_frame);
        drop(original);
        assert_eq!(host.admitted_bytes(), request_bytes + presentation_bytes);
        let held = derivative.clone();
        drop(derivative);
        assert_eq!(held.status().unwrap().options.exposure_ev, 1.0);
        drop(held);
        assert_eq!(host.admitted_bytes(), 0);
    }

    #[test]
    fn retained_emitters_bill_vector_and_original_name_capacities_in_same_pool() {
        let mut request = request();
        let mut name = String::with_capacity(256);
        name.push_str("fixture");
        let mut emissions = Vec::with_capacity(32);
        emissions.push(super::super::SurfaceEmission {
            body_index: 0,
            body: name,
            color: [1.0; 3],
            strength: 1.0,
            sides: super::super::EmissionSides::Front,
        });
        let options = request.raytrace.as_mut().unwrap();
        options.emissions = Some(emissions);
        let expected = 8 * std::mem::size_of::<super::super::DirectionalLightOptions>() as u64
            + 32 * std::mem::size_of::<super::super::SurfaceEmission>() as u64
            + 256;
        assert_eq!(retained_request_bytes(&request).unwrap(), expected);
        let budget = Arc::new(Budget { used: AtomicU64::new(0), limit: MEMORY_LIMIT });
        let lease = budget.reserve(expected).unwrap();
        assert_eq!(budget.used.load(Ordering::Acquire), expected);
        assert!(admit_request_sum(super::super::image::MAX_JOB_BYTES - expected, expected).is_ok());
        assert!(admit_request_sum(super::super::image::MAX_JOB_BYTES - expected + 1, expected).is_err());
        drop(lease);
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn emitter_request_lease_survives_real_completion_frame_eviction_and_last_handle() {
        let mut host = CpuHost::for_platform().unwrap();
        let mut request = request();
        let mut name = String::with_capacity(256);
        name.push_str("emitter");
        let mut values = Vec::with_capacity(32);
        values.push(super::super::SurfaceEmission {
            body_index: 0,
            body: name,
            color: [1.0; 3],
            strength: 1.0,
            sides: super::super::EmissionSides::Both,
        });
        request.raytrace.as_mut().unwrap().emissions = Some(values);
        let expected = retained_request_bytes(&request).unwrap() + request.retained_resource_metadata_bytes();
        let mesh = solidcraft_geom::Mesh {
            positions: vec![solidcore_record::Vec3::ZERO, solidcore_record::Vec3::X, solidcore_record::Vec3::Y],
            normals: vec![solidcore_record::Vec3::Z; 3],
            triangles: vec![[0, 1, 2]],
            tri_face: vec![0],
            ..Default::default()
        };
        let bounds = mesh.bounds();
        let scene = ProductScene {
            meshes: vec![crate::ProductMesh {
                transmission: None,
                name: "emitter".into(),
                mesh: Arc::new(mesh),
                color: [255; 3],
                opacity: 1.0,
                surface: None,
                faces: Vec::new(),
            }],
            bounds,
        };
        let job = host.submit(JobId { session: 1695, serial: 1, generation: 1 }, 1, scene, request).unwrap();
        assert_eq!(job.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
        // Completion publication precedes the worker's last temporary handle release.
        // Join this test's worker before asserting exact lease ownership, as in the union test.
        host.0.shared.stop.store(true, Ordering::Release);
        host.0.shared.wake.notify_all();
        Arc::get_mut(&mut host.0).unwrap()._worker.take().unwrap().join().unwrap();
        let frame = job.frame().unwrap().unwrap();
        let held = job.clone();
        job.0.data.lock().unwrap().frame = None;
        drop(job);
        assert!(host.admitted_bytes() > expected);
        drop(frame);
        assert_eq!(host.admitted_bytes(), expected);
        drop(held);
        assert_eq!(host.admitted_bytes(), 0);
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    fn cpu_map_capture_fixture(mapped: bool, corrupt: bool) -> (ProductScene, ImageRequest, std::sync::Weak<Vec<u8>>) {
        use super::super::textures::{BaseColorMode, PlanarFrame, RuntimeTextures, TextureBinding, TextureMap};
        use image::ImageEncoder;
        use solidcore_record::Vec3;
        use solidcraft_geom::Mesh;
        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded).write_image(&[128, 64, 255, 255], 1, 1, image::ExtendedColorType::Rgba8).unwrap();
        if corrupt {
            let idat = encoded.windows(4).position(|w| w == b"IDAT").unwrap();
            encoded[idat + 4] ^= 0xff;
        }
        let map = TextureMap::new(encoded, crate::texture_input::MapRole::BaseColor).unwrap();
        let weak = map.source().encoded_weak();
        let surface = if mapped {
            Vec3::new(0.5, 0.5, 0.5)
        } else {
            let v = [128, 64, 255].map(|v| crate::surface::srgb8_to_linear(v) as f32 as f64);
            Vec3::new(v[0], v[1], v[2])
        };
        let mesh = Arc::new(Mesh {
            positions: vec![Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(-10.0, 10.0, 0.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0; 2],
            ..Default::default()
        });
        let scene = ProductScene {
            bounds: mesh.bounds(),
            meshes: vec![crate::ProductMesh {
                name: "mapped".into(),
                mesh,
                color: [255; 3],
                opacity: 1.0,
                transmission: None,
                surface: Some(crate::surface::OpaqueSurface::new(surface, 0.0, 1.0, 1.0).unwrap()),
                faces: vec![],
            }],
        };
        let options = super::super::RaytraceOptions {
            lights: Some(vec![super::super::DirectionalLightOptions {
                direction: [0.0, 0.0, 1.0],
                color: [1.0; 3],
                intensity: std::f64::consts::PI,
            }]),
            progressive: Some(super::super::CpuProgressiveOptions { samples_per_pixel: 3, seed: 9 }),
            output: Some(Box::new(super::super::OutputOptions { linear_exr: true, ..Default::default() })),
            ..Default::default()
        };
        let request = ImageRequest {
            width: 1,
            height: 1,
            camera: crate::Camera { distance: 1.0, fov: 0.0, yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, ..Default::default() },
            background: [0; 3],
            transparent_background: false,
            integrator: Integrator::Raytrace,
            raytrace: Some(options),
            environment: None,
            studio_weights: None,
            textures: if mapped {
                Some(Arc::new(
                    RuntimeTextures::new(vec![TextureBinding {
                        body_index: 0,
                        body_name: "mapped".into(),
                        frame: PlanarFrame { origin: [0.0; 3], u: [100.0, 0.0, 0.0], v: [0.0, 100.0, 0.0] },
                        base_color: Some(map),
                        normal: None,
                        roughness: None,
                        metallic: None,
                        normal_strength: 1.0,
                        base_color_mode: BaseColorMode::Replace,
                        flip_v: false,
                    }])
                    .unwrap(),
                ))
            } else {
                None
            },
        };
        (scene, request, weak)
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    fn cpu_map_manual_native_queue() -> ImageHost {
        // Deterministic queued/cancel/error boundary test using the real builtin
        // CPU factory and work leases; no substitute registered executor or render.
        ImageHost(Arc::new(Owner {
            executor: None,
            shared: ImageHost::shared(super::super::NATIVE_CPU_POOL_BYTES),
            kind: HostKind::NativeWorker,
            running: Mutex::new(None),
            _worker: None,
        }))
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn cpu_map_native_factory_consumes_full_work_capture_and_releases_sources_before_retained_frames() {
        let (scene, request, weak) = cpu_map_capture_fixture(true, false);
        let mut host = ImageHost::for_platform().unwrap();
        let job = host.submit(JobId { session: 3301, serial: 1, generation: 1 }, 1, scene, request).unwrap();
        assert!(job.0.request.textures.is_none(), "Record must never retain a work-only texture Arc");
        let completed = job.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(completed.progress.phase, Phase::Completed);
        assert_eq!(completed.textures.unwrap().bodies, 1);
        let (scene, request, _) = cpu_map_capture_fixture(false, false);
        let reference = host.submit(JobId { session: 3301, serial: 2, generation: 2 }, 1, scene, request).unwrap();
        assert_eq!(reference.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
        host.join_worker_for_test().unwrap();
        assert!(job.work_released());
        assert!(weak.upgrade().is_none(), "all original encoded Arcs drop even while job/frame handles survive");
        let frame = job.frame().unwrap().unwrap();
        let expected = reference.frame().unwrap().unwrap();
        assert_eq!(frame.png(), expected.png(), "builtin CPU factory must consume full Work.request, not stripped public metadata");
        assert_eq!(frame.linear_exr(), expected.linear_exr());
        assert!(frame.linear_exr().is_some());
        let color = [128, 64, 255].map(|v| {
            let x = crate::surface::srgb8_to_linear(v) as f32 as f64;
            solidcraft_geom::srgb::encode8(x / (1.0 + x))
        });
        assert_eq!(image::load_from_memory(frame.png()).unwrap().to_rgba8().get_pixel(0, 0).0, [color[0], color[1], color[2], 255]);
        let held = job.clone();
        job.0.data.lock().unwrap().frame = None;
        reference.0.data.lock().unwrap().frame = None;
        drop(frame);
        drop(expected);
        drop(job);
        assert!(weak.upgrade().is_none());
        drop(held);
        drop(reference);
        assert_eq!(host.admitted_bytes(), 0);
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn cpu_map_queued_cancel_failed_decode_and_late_cancel_keep_full_work_barrier_and_no_publication() {
        for mode in ["queued_cancel", "failed_decode", "late_cancel"] {
            let host = cpu_map_manual_native_queue();
            let (scene, request, weak) = cpu_map_capture_fixture(true, mode == "failed_decode");
            let retained = retained_request_bytes(&request).unwrap() + request.retained_resource_metadata_bytes();
            let job = host.submit(JobId { session: 3302, serial: 1, generation: 1 }, 1, scene, request).unwrap();
            assert!(job.0.request.textures.is_none());
            assert!(weak.upgrade().is_some());
            let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
            assert!(work.request.as_ref().unwrap().textures.is_some());
            if mode == "queued_cancel" {
                job.cancel().unwrap();
                assert!(!job.work_released());
                assert!(Running::begin(work).is_none());
            } else {
                let mut running = Running::begin(work).unwrap();
                let mut stopped = false;
                for _ in 0..200_000 {
                    if mode == "late_cancel" && running.pending_png.as_ref().is_some_and(|pending| pending.preview_samples.is_none()) {
                        job.cancel().unwrap();
                        assert!(!running.guarded_step(1));
                        stopped = true;
                        break;
                    }
                    if !running.guarded_step(1) {
                        stopped = true;
                        break;
                    }
                }
                assert!(stopped);
                assert!(!job.work_released(), "terminal phase cannot release an undropped work lease");
                drop(running);
            }
            assert!(job.work_released());
            assert!(weak.upgrade().is_none());
            let status = job.status().unwrap();
            assert_eq!(status.progress.phase, if mode == "failed_decode" { Phase::Failed } else { Phase::Cancelled });
            assert!(job.frame().unwrap().is_none());
            assert!(job.preview().unwrap().is_none());
            assert_eq!(host.admitted_bytes(), retained, "only exact small public request/summary ownership survives work release");
            let held = job.clone();
            drop(job);
            assert_eq!(host.admitted_bytes(), retained);
            drop(held);
            assert_eq!(host.admitted_bytes(), 0);
        }
    }

    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn cpu_map_source_and_peak_decode_prepaid_pool_refusal_releases_all_captures_before_queueing() {
        let (scene, request, weak) = cpu_map_capture_fixture(true, false);
        let admission = super::super::admit(&scene, &request).unwrap();
        let total = admission.estimated_total_bytes + retained_request_bytes(&request).unwrap() + request.retained_resource_metadata_bytes();
        let mut host = cpu_map_manual_native_queue();
        Arc::get_mut(&mut host.0).unwrap().shared = ImageHost::shared(total - 1);
        assert_eq!(host.submit(JobId { session: 3303, serial: 1, generation: 1 }, 1, scene, request).err().unwrap().code, ErrorCode::LimitExceeded);
        assert_eq!(host.admitted_bytes(), 0);
        assert!(host.0.shared.queue.lock().unwrap().is_empty());
        assert!(weak.upgrade().is_none(), "a failed prepaid capture cannot retain hidden encoded resources or queue a decoder");
    }

    fn request() -> ImageRequest {
        let mut lights = Vec::with_capacity(8);
        lights.push(super::super::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 });
        ImageRequest {
            studio_weights: None,
            environment: None,
            textures: None,
            integrator: Integrator::Raytrace,
            raytrace: Some(super::super::RaytraceOptions { lights: Some(lights), ..Default::default() }),
            camera: Default::default(),
            width: 8,
            height: 8,
            background: [0; 3],
            transparent_background: false,
        }
    }
    #[test]
    fn retained_lights_are_billed_by_original_capacity_and_sum_is_checked() {
        let request = request();
        let bytes = retained_request_bytes(&request).unwrap();
        assert_eq!(bytes, 8 * std::mem::size_of::<super::super::DirectionalLightOptions>() as u64);
        let cap = super::super::image::MAX_JOB_BYTES;
        assert!(admit_request_sum(cap - bytes, bytes).is_ok());
        assert_eq!(admit_request_sum(cap - bytes + 1, bytes).unwrap_err().code, ErrorCode::LimitExceeded);
        assert_eq!(admit_request_sum(u64::MAX, 1).unwrap_err().code, ErrorCode::LimitExceeded);
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn retained_lights_keep_the_same_pool_lease_after_png_eviction() {
        let host = CpuHost::for_platform().unwrap();
        let request = request();
        let expected = retained_request_bytes(&request).unwrap() + request.retained_resource_metadata_bytes();
        let job = host
            .submit(
                JobId { session: 99, serial: 1, generation: 1 },
                1,
                ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY },
                request,
            )
            .unwrap();
        assert_eq!(job.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
        let frame = job.frame().unwrap().unwrap();
        let held_bytes = host.admitted_bytes();
        assert!(held_bytes > expected);
        job.0.data.lock().unwrap().frame = None;
        assert_eq!(host.admitted_bytes(), held_bytes);
        drop(frame);
        assert_eq!(host.admitted_bytes(), expected);
        let held = job.clone();
        drop(job);
        assert_eq!(host.admitted_bytes(), expected);
        drop(held);
    }
}

#[cfg(all(test, feature = "raytrace-cpu", not(target_arch = "wasm32")))]
mod environment_metadata_lease_tests {
    use super::*;
    #[test]
    fn captured_environment_metadata_survives_png_eviction_without_source_bytes() {
        use crate::lighting::{
            HdrSource,
            environment_input::{EnvironmentInfo, EnvironmentInput},
        };
        let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
        for _ in 0..8 {
            bytes.extend_from_slice(&[128, 64, 32, 129]);
        }
        let input = EnvironmentInput::new(HdrSource::new(bytes).unwrap(), 0.5, 2.0, 16).unwrap();
        let metadata = input.metadata();
        let expected = std::mem::size_of::<EnvironmentInfo>() as u64 + ImageRequest::fixed_cpu_metadata_bytes();
        let mut host = CpuHost::for_platform().unwrap();
        let budget = host.0.shared.budget.clone();
        let request = ImageRequest {
            studio_weights: None,
            environment: None,
            textures: None,
            integrator: Integrator::Raytrace,
            raytrace: None,
            camera: Default::default(),
            width: 8,
            height: 8,
            background: [0; 3],
            transparent_background: false,
        };
        let job = host
            .submit_with_environment(
                JobId { session: 1693, serial: 7, generation: 1 },
                1,
                ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY },
                request,
                Some(input),
            )
            .unwrap();
        let status = job.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(status.progress.phase, Phase::Completed);
        assert!(matches!(status.environment, Some(JobEnvironmentInfo::Cpu(info)) if info.source_sha256 == metadata.source_sha256));
        host.join_worker_for_test().unwrap();
        let frame = job.frame().unwrap().unwrap();
        job.0.data.lock().unwrap().frame = None;
        assert_eq!(host.admitted_bytes(), frame._reservation.bytes + expected);
        drop(frame);
        assert_eq!(host.admitted_bytes(), expected);
        drop(host);
        drop(job);
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
    }
}

#[cfg(all(test, feature = "raytrace-cpu", not(target_arch = "wasm32")))]
mod surface_request_metadata_tests {
    use super::*;
    use solidcraft_geom::{Aabb3, Mesh};
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn surface_request_lease_survives_completion_frame_eviction_and_held_handles() {
        use crate::{ProductMesh, surface_patterns::SurfaceEffect};
        use std::time::{Duration, Instant};
        fn settle(host: &CpuHost, expected: u64) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while host.admitted_bytes() != expected && Instant::now() < deadline {
                std::thread::yield_now();
            }
            assert_eq!(host.admitted_bytes(), expected);
        }
        let mut host = CpuHost::for_platform().unwrap();
        let mut meshes = Vec::new();
        let mut effects = Vec::with_capacity(32);
        for index in 0..32 {
            let mut name = String::with_capacity(256);
            name.push_str(&format!("{index:03}"));
            name.push_str(&"x".repeat(253));
            assert_eq!(name.capacity(), 256);
            meshes.push(ProductMesh {
                transmission: None,
                name: name.clone(),
                mesh: Arc::new(Mesh::default()),
                color: [100; 3],
                opacity: 1.0,
                surface: None,
                faces: Vec::new(),
            });
            effects.push(SurfaceEffect {
                body: name,
                frame: Default::default(),
                repeat_mm: [20.0; 3],
                roughness_amplitude: 0.0,
                bump_amplitude_mm: 0.0,
            });
        }
        let scene = ProductScene { meshes, bounds: Aabb3 { min: solidcore_record::Vec3::ZERO, max: solidcore_record::Vec3::ZERO } };
        let mut request = request();
        request.integrator = Integrator::Raytrace;
        request.raytrace = Some(super::super::RaytraceOptions { surface_effects: Some(effects), ..Default::default() });
        let metadata = retained_request_bytes(&request).unwrap() + request.retained_resource_metadata_bytes();
        assert!(metadata >= 32 * 256);
        let mut oversized = request.clone();
        let mut overallocated = Vec::with_capacity(64);
        overallocated.extend(oversized.raytrace.as_mut().unwrap().surface_effects.take().unwrap());
        oversized.raytrace.as_mut().unwrap().surface_effects = Some(overallocated);
        assert!(host.submit(id(40, 1), 1, scene.clone(), oversized).is_err());
        assert_eq!(host.admitted_bytes(), 0);

        let job = host.submit(id(40, 2), 1, scene, request).unwrap();
        assert_eq!(job.wait_timeout(Duration::from_secs(10)).unwrap().progress.phase, Phase::Completed);
        // Wait for this host's last temporary worker handle before exact lease checks.
        host.0.shared.stop.store(true, Ordering::Release);
        host.0.shared.wake.notify_all();
        Arc::get_mut(&mut host.0).unwrap()._worker.take().unwrap().join().unwrap();
        let frame = job.frame().unwrap().unwrap();
        let png_bytes = frame._reservation.bytes;
        settle(&host, metadata + png_bytes);
        // Evict the record's frame under the same lock used by publication, while
        // an external frame and a cloned job handle retain their independent leases.
        job.0.data.lock().unwrap().frame = None;
        let held = job.clone();
        drop(job);
        assert_eq!(host.admitted_bytes(), metadata + png_bytes);
        drop(frame);
        settle(&host, metadata);
        assert_eq!(held.status().unwrap().request.raytrace.unwrap().surface_effects.unwrap().len(), 32);
        drop(held);
        settle(&host, 0);
    }

    fn request() -> ImageRequest {
        ImageRequest {
            studio_weights: None,
            environment: None,
            textures: None,
            integrator: Default::default(),
            raytrace: None,
            camera: Default::default(),
            width: 16,
            height: 16,
            background: [40, 80, 120],
            transparent_background: false,
        }
    }

    fn id(owner: u64, serial: u64) -> JobId {
        JobId { session: owner, serial, generation: serial }
    }
}
