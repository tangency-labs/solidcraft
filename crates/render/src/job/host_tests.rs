use super::*;
use solidcraft_geom::{Aabb3, Mesh};

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
mod progressive_checkpoints {
    use super::*;
    use crate::job::{CpuProgressiveOptions, RaytraceOptions};

    // The production native admission/Running path, manually advanced so tests
    // can stop exactly between hashing and the cancellation/publication lock.
    fn fixture(samples: u32) -> (CpuHost, JobHandle, Running) {
        let host = ImageHost(Arc::new(Owner {
            executor: None,
            shared: CpuHost::shared(MEMORY_LIMIT),
            kind: HostKind::NativeWorker,
            running: Mutex::new(None),
            _worker: None,
        }));
        let request = ImageRequest {
            integrator: Integrator::Raytrace,
            raytrace: Some(RaytraceOptions {
                progressive: Some(CpuProgressiveOptions { samples_per_pixel: samples, seed: 123 }),
                lights: Some(Vec::new()),
                ..Default::default()
            }),
            width: 4,
            height: 4,
            ..request()
        };
        let expected = admit(&empty(), &request).unwrap().estimated_total_bytes;
        let job = host.submit(id(7300, 1), 19, empty(), request).unwrap();
        assert_eq!(host.admitted_bytes(), expected);
        let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
        let running = Running::begin(work).unwrap();
        (host, job, running)
    }

    fn before_preview(running: &mut Running) {
        for _ in 0..100_000 {
            if running.pending_png.as_ref().is_some_and(|p| p.preview_samples.is_some()) {
                return;
            }
            assert!(running.advance(1).unwrap());
        }
        panic!("small CPU job did not reach its checkpoint");
    }

    #[test]
    fn checkpoints_transfer_png_allocations_and_keep_every_external_lease() {
        let (host, job, mut running) = fixture(3);
        let initial = host.admitted_bytes();
        let mut held = Vec::new();
        for (sequence, samples) in [(1, 1), (2, 2)] {
            before_preview(&mut running);
            let pointer = running.pending_png.as_ref().unwrap().png.as_ptr();
            assert!(running.task.is_some(), "a preview must retain film, BVH and task state");
            assert!(running.advance(1).unwrap());
            let preview = job.preview().unwrap().unwrap();
            assert_eq!(preview.png().as_ptr(), pointer, "PNG ownership moves without cloning its allocation");
            assert_eq!(preview.info.frame_sequence, sequence);
            assert_eq!(preview.info.samples_per_pixel, samples);
            assert_eq!(preview.info.frame.job, job.id());
            assert_eq!(preview.info.frame.scene_generation, 19);
            assert_eq!(preview.info.frame.png_sha256, format!("{:x}", Sha256::digest(preview.png())));
            assert!(job.frame().unwrap().is_none());
            assert!(job.status().unwrap().frame.is_none());
            assert_eq!(host.admitted_bytes(), initial, "splitting a prepaid lease does not change pool usage");
            held.push(preview);
        }
        let mut complete = false;
        for _ in 0..100_000 {
            if !running.advance(1).unwrap() {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
        assert!(job.preview().unwrap().is_none());
        let frame = job.frame().unwrap().unwrap();
        assert_eq!(frame.info.job, job.id());
        let retained: u64 = held.iter().map(|p| p._reservation.bytes).sum();
        assert_eq!(host.admitted_bytes(), retained + frame._reservation.bytes);
        drop(running);
        drop(job);
        drop(frame);
        assert_eq!(host.admitted_bytes(), retained);
        while let Some(preview) = held.pop() {
            let before = host.admitted_bytes();
            let bytes = preview._reservation.bytes;
            drop(preview);
            assert_eq!(host.admitted_bytes(), before - bytes);
        }
        assert_eq!(host.admitted_bytes(), 0);
    }

    #[test]
    fn cancellation_before_or_after_digest_cannot_publish_a_late_preview() {
        for finish_digest in [false, true] {
            let (host, job, mut running) = fixture(3);
            before_preview(&mut running);
            assert!(running.advance(1).unwrap());
            let held = job.preview().unwrap().unwrap();
            before_preview(&mut running);
            if finish_digest {
                let pending = running.pending_png.as_mut().unwrap();
                while !pending.step(1, &job.0.cancel).unwrap() {}
                assert_eq!(pending.hashed, pending.png.len());
            }
            job.cancel().unwrap();
            assert!(job.preview().unwrap().is_none(), "cancel removes the provisional slot immediately");
            assert!(!running.guarded_step(1));
            assert_eq!(job.status().unwrap().progress.phase, Phase::Cancelled);
            assert!(job.frame().unwrap().is_none());
            drop(running);
            drop(job);
            assert_eq!(host.admitted_bytes(), held._reservation.bytes);
            assert_eq!(held.info.samples_per_pixel, 1);
            drop(held);
            assert_eq!(host.admitted_bytes(), 0);
        }
    }

    #[test]
    fn an_invalid_checkpoint_sequence_fails_without_replacing_the_old_snapshot() {
        let (host, job, mut running) = fixture(4);
        before_preview(&mut running);
        assert!(running.advance(1).unwrap());
        let held = job.preview().unwrap().unwrap();
        before_preview(&mut running);
        running.pending_png.as_mut().unwrap().preview_samples = Some(3);
        assert!(!running.guarded_step(1));
        assert_eq!(job.status().unwrap().progress.phase, Phase::Failed);
        assert!(job.preview().unwrap().is_none());
        assert!(job.frame().unwrap().is_none());
        assert_eq!(held.info.samples_per_pixel, 1);
        drop(running);
        drop(job);
        assert_eq!(host.admitted_bytes(), held._reservation.bytes);
        drop(held);
        assert_eq!(host.admitted_bytes(), 0);
    }
}

fn request() -> ImageRequest {
    ImageRequest {
        studio_weights: None,
        integrator: Default::default(),
        raytrace: None,
        textures: None,
        environment: None,
        camera: Default::default(),
        width: 16,
        height: 16,
        background: [40, 80, 120],
        transparent_background: false,
    }
}

#[test]
fn legacy_image_requests_default_to_raster_and_reject_unknown_integrators() {
    let mut value = serde_json::to_value(request()).unwrap();
    value.as_object_mut().unwrap().remove("integrator");
    let decoded: ImageRequest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.integrator, Integrator::Raster);
    value["integrator"] = serde_json::json!("gpu_pathtrace");
    assert!(serde_json::from_value::<ImageRequest>(value).is_err());
}

#[cfg(feature = "raytrace-cpu")]
#[test]
fn cooperative_raytrace_descriptor_matches_native_only_validation() {
    let host = CpuHost::cooperative();
    assert!(host.validate_integrator(Integrator::Raytrace).is_err());
    assert!(matches!(host.descriptor_for(Integrator::Raytrace).unwrap().availability, Availability::Unavailable { .. }));
    assert!(host.validate_integrator(Integrator::Raster).is_ok());
    assert!(!host.descriptor_for(Integrator::Raytrace).unwrap().profiles.iter().any(|p| p.features.contains(&crate::RenderFeature::Refraction)));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn native_raytrace_refraction_capability_resolves_only_preview_images() {
    let host = CpuHost::for_platform().unwrap();
    let descriptor = host.descriptor_for(Integrator::Raytrace).unwrap();
    assert_eq!(descriptor.availability, Availability::Available);
    assert!(descriptor.profiles.iter().any(|profile| profile.features.contains(&crate::RenderFeature::Refraction)));
    let mut backend = crate::builtin_backend();
    backend.executors = vec![descriptor];
    let mut request = crate::RenderRequest {
        backend: None,
        mode: RenderMode::Preview,
        operation: RenderOperation::Image,
        width: 1,
        height: 1,
        format: Some(RenderFormat::Png),
        features: vec![crate::RenderFeature::Refraction],
        policy: Default::default(),
    };
    let resolved = crate::resolve_request(std::slice::from_ref(&backend), &request).unwrap();
    assert_eq!(resolved.executor.as_str(), "cpu-raytrace");
    request.mode = RenderMode::Final;
    assert!(crate::resolve_request(std::slice::from_ref(&backend), &request).is_err());
    request.mode = RenderMode::Preview;
    backend.executors = vec![host.descriptor_for(Integrator::Raster).unwrap()];
    assert!(crate::resolve_request(&[backend], &request).is_err(), "CPU raster must not advertise physical refraction");
    assert_eq!(host.admitted_bytes(), 0, "capability discovery never starts a render");
}

#[cfg(not(feature = "raytrace-cpu"))]
#[test]
fn compiled_out_raytracing_never_enters_the_host_queue() {
    let host = CpuHost::cooperative();
    let mut request = request();
    request.integrator = Integrator::Raytrace;
    assert!(host.validate_integrator(Integrator::Raytrace).unwrap_err().message.contains("not compiled in"));
    assert!(host.descriptor_for(Integrator::Raytrace).is_err());
    let error = host.submit(JobId { session: 1, serial: 1, generation: 1 }, 0, empty(), request).err().unwrap();
    assert_eq!(error.code, ErrorCode::HostUnavailable);
    assert_eq!(host.admitted_bytes(), 0);
    assert!(!host.pump(1).unwrap());
}
fn empty() -> ProductScene {
    ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY }
}
fn id(owner: u64, serial: u64) -> JobId {
    JobId { session: owner, serial, generation: serial }
}
fn drain(host: &CpuHost) {
    for _ in 0..10_000 {
        if !host.pump(4096).unwrap() {
            return;
        }
    }
    panic!("small cooperative jobs did not drain");
}

#[test]
fn queue_is_bounded_and_cancelled_entries_release_their_reservations() {
    let host = CpuHost::cooperative();
    let a = host.submit(id(10, 1), 100, empty(), request()).unwrap();
    let b = host.submit(id(20, 1), 200, empty(), request()).unwrap();
    let before = host.admitted_bytes();
    assert_eq!(host.submit(id(30, 1), 300, empty(), request()).err().unwrap().code, ErrorCode::QueueFull);
    assert_eq!(host.admitted_bytes(), before);
    assert_eq!(a.cancel().unwrap().progress.phase, Phase::Cancelled);
    let c = host.submit(id(30, 1), 300, empty(), request()).unwrap();
    drain(&host);
    assert_eq!(a.status().unwrap().progress.phase, Phase::Cancelled);
    for (handle, owner, generation) in [(&b, 20, 200), (&c, 30, 300)] {
        let frame = handle.frame().unwrap().unwrap();
        assert_eq!(frame.info.job, id(owner, 1));
        assert_eq!(frame.info.scene_generation, generation);
        assert_eq!(frame.info.executor.as_str(), "cpu-product");
        assert_eq!(image::load_from_memory(frame.png()).unwrap().width(), 16);
        assert_eq!(frame.info.png_bytes, frame.png().len() as u64);
        assert_eq!(frame.info.png_sha256, format!("{:x}", Sha256::digest(frame.png())));
        assert_eq!(handle.status().unwrap().frame.unwrap().png_sha256, frame.info.png_sha256);
    }
    assert!(host.admitted_bytes() > 0 && host.admitted_bytes() < before);
    let frame = b.frame().unwrap().unwrap();
    drop(b);
    drop(c);
    assert!(host.admitted_bytes() > 0, "an externally retained frame retains its lease");
    drop(frame);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn cancellation_while_encoding_cannot_publish_a_late_frame() {
    let host = CpuHost::cooperative();
    let job = host.submit(id(1, 1), 8, empty(), request()).unwrap();
    for _ in 0..10_000 {
        host.pump(64).unwrap();
        let status = job.status().unwrap();
        if status.progress.phase == Phase::Encoding && status.progress.done > 0 {
            break;
        }
    }
    assert_eq!(job.status().unwrap().progress.phase, Phase::Encoding);
    assert_eq!(job.cancel().unwrap().progress.phase, Phase::Cancelling);
    drain(&host);
    assert_eq!(job.status().unwrap().progress.phase, Phase::Cancelled);
    assert!(job.frame().unwrap().is_none());
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn malformed_geometry_is_rejected_without_stopping_the_host() {
    let host = CpuHost::cooperative();
    let scene = ProductScene {
        bounds: Aabb3::EMPTY,
        meshes: vec![crate::ProductMesh {
            name: "Broken".into(),
            mesh: Arc::new(Mesh { triangles: vec![[99, 99, 99]], ..Default::default() }),
            color: [0; 3],
            opacity: 1.0,
            surface: None,
            transmission: None,
            faces: Vec::new(),
        }],
    };
    let error = host.submit(id(1, 1), 1, scene, request()).err().unwrap();
    assert_eq!(error.code, ErrorCode::InvalidScene);
    assert_eq!(host.admitted_bytes(), 0);
    assert!(host.is_available());
    let healthy = host.submit(id(1, 2), 1, empty(), request()).unwrap();
    drain(&host);
    assert_eq!(healthy.status().unwrap().progress.phase, Phase::Completed);
}

#[test]
fn closing_the_host_cancels_active_and_queued_work() {
    let host = CpuHost::cooperative();
    let budget = host.0.shared.budget.clone();
    let active = host.submit(id(1, 1), 1, empty(), request()).unwrap();
    let queued = host.submit(id(2, 1), 1, empty(), request()).unwrap();
    host.pump(1).unwrap();
    drop(host);
    assert_eq!(active.status().unwrap().progress.phase, Phase::Cancelled);
    assert_eq!(queued.status().unwrap().progress.phase, Phase::Cancelled);
    assert_eq!(budget.used.load(Ordering::Acquire), 0);
}

#[test]
fn a_foreign_frame_is_rejected_even_if_a_broken_host_puts_it_in_the_result_slot() {
    let host = CpuHost::cooperative();
    let job = host.submit(id(1, 1), 1, empty(), request()).unwrap();
    drain(&host);
    {
        let mut data = job.0.data.lock().unwrap();
        let frame = Arc::get_mut(data.frame.as_mut().unwrap()).unwrap();
        frame.info.job.session = 2;
    }
    assert_eq!(job.frame().err().unwrap().code, ErrorCode::WrongOwner);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_job_finishes_without_any_poll_or_ui_pump() {
    let host = CpuHost::for_platform().unwrap();
    assert_eq!(host.kind(), HostKind::NativeWorker);
    let job = host.submit(id(1, 1), 12, empty(), request()).unwrap();
    // A generous deadlock watchdog, not a performance assertion.
    let status = job.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed);
    assert_eq!(job.frame().unwrap().unwrap().info.scene_generation, 12);
    let frame = job.frame().unwrap().unwrap();
    assert_eq!(frame.info.png_bytes, frame.png().len() as u64);
    assert_eq!(frame.info.png_sha256, format!("{:x}", Sha256::digest(frame.png())));
    assert_eq!(status.frame.unwrap().png_sha256, frame.info.png_sha256);
}

/// Isolate the existing integrator-to-host completion boundary with a real PNG
/// from the production encoder. This does not pretend to render the noisy pixels.
fn pending_digest_fixture() -> (CpuHost, JobHandle, Running, usize) {
    let host = CpuHost::cooperative();
    let mut request = request();
    request.width = 256;
    request.height = 128;
    let mut rgba = vec![255; request.width as usize * request.height as usize * 4];
    let mut seed = 0x6a09_e667u32;
    for pixel in rgba.as_chunks_mut::<4>().0 {
        for channel in pixel.iter_mut().take(3) {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *channel = seed as u8;
        }
    }
    let mut encoder = super::super::encoding::PngEncoding::new(
        request.width,
        request.height,
        u64::from(request.width) * u64::from(request.height) * 8 + 65_536,
        CancelToken::default(),
    )
    .unwrap();
    let png = loop {
        if let Some(png) = encoder.step(&rgba).unwrap() {
            break png;
        }
    };
    assert!(png.len() > PNG_HASH_CHUNK_BYTES * PNG_HASH_CHUNKS_PER_ADVANCE);
    let length = png.len();
    let job = host.submit(id(61, 1), 99, empty(), request).unwrap();
    let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
    let mut running = Running::begin(work).unwrap();
    // Exactly the same state transition used when WorkTask returns Complete.
    running.task = None;
    running.pending_png = Some(PendingPng::new(png));
    (host, job, running, length)
}

#[test]
fn png_digest_is_incremental_and_keeps_the_original_allocation_and_lease_until_completion() {
    let (host, job, mut running, length) = pending_digest_fixture();
    let before = host.admitted_bytes();
    let pointer = running.pending_png.as_ref().unwrap().png.as_ptr();
    assert!(running.advance(0).unwrap());
    assert_eq!(running.pending_png.as_ref().unwrap().hashed, 0, "zero units cannot hash a chunk");
    assert_eq!(host.admitted_bytes(), before);
    assert!(job.frame().unwrap().is_none());
    assert!(running.advance(usize::MAX).unwrap());
    assert_eq!(running.pending_png.as_ref().unwrap().hashed, PNG_HASH_CHUNK_BYTES * PNG_HASH_CHUNKS_PER_ADVANCE);
    assert_eq!(host.admitted_bytes(), before, "hashing must retain the full original work lease");
    assert!(job.frame().unwrap().is_none());
    let mut previous = job.status().unwrap().progress.done;
    while running.advance(1).unwrap() {
        let progress = job.status().unwrap().progress;
        assert_eq!(progress.phase, Phase::Encoding);
        assert_eq!(progress.total, length as u64);
        assert_eq!(progress.done - previous, PNG_HASH_CHUNK_BYTES as u64);
        assert_eq!(host.admitted_bytes(), before);
        assert!(job.frame().unwrap().is_none());
        previous = progress.done;
    }
    assert!(running.task.is_none() && running.pending_png.is_none() && running.reservation.is_none());
    let status = job.status().unwrap();
    assert_eq!(status.progress.phase, Phase::Completed);
    let frame = job.frame().unwrap().unwrap();
    assert_eq!(frame.png().as_ptr(), pointer, "the original Vec moves into the retained frame without cloning");
    assert_eq!(frame.info.png_bytes, length as u64);
    assert_eq!(frame.info.png_sha256.len(), 64);
    assert!(frame.info.png_sha256.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
    assert_eq!(frame.info.png_sha256, format!("{:x}", Sha256::digest(frame.png())));
    assert_eq!(status.frame.unwrap().png_sha256, frame.info.png_sha256);
    let decoded = image::load_from_memory(frame.png()).unwrap().to_rgba8();
    assert_eq!(decoded.dimensions(), (256, 128));
    assert!(decoded.pixels().all(|p| p.0[3] == 255));
    assert!(decoded.pixels().any(|p| p.0[..3] != decoded.get_pixel(0, 0).0[..3]));
    let actual_metadata =
        std::mem::size_of::<Running>() + std::mem::size_of::<RenderFrame>() + frame.info.png_sha256.capacity() + 2 * crate::MAX_RENDER_ID_BYTES + 32;
    assert!(actual_metadata <= FRAME_METADATA_BYTES as usize, "digest Vec/offset/state and actual hex allocation must be admitted");
    assert_eq!(host.admitted_bytes(), frame.png.capacity() as u64 + FRAME_METADATA_BYTES);
    for _ in 0..3 {
        let current = job.status().unwrap();
        assert_eq!(current.frame.unwrap().png_sha256, frame.info.png_sha256);
        assert_eq!(frame.png().as_ptr(), pointer);
    }
    drop(running);
    drop(job);
    assert!(host.admitted_bytes() > 0);
    drop(frame);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn cancellation_during_png_hashing_and_host_disposal_cannot_publish_a_frame() {
    for close_host in [false, true] {
        let (host, job, running, _) = pending_digest_fixture();
        let budget = host.0.shared.budget.clone();
        *host.0.running.lock().unwrap() = Some(running);
        assert!(host.pump(1).unwrap());
        assert!(host.0.running.lock().unwrap().as_ref().unwrap().pending_png.as_ref().unwrap().hashed > 0);
        assert_eq!(job.status().unwrap().progress.phase, Phase::Encoding);
        assert!(job.frame().unwrap().is_none());
        if close_host {
            drop(host);
        } else {
            assert_eq!(job.cancel().unwrap().progress.phase, Phase::Cancelling);
            assert!(!host.pump(1).unwrap());
            drop(host);
        }
        assert_eq!(job.status().unwrap().progress.phase, Phase::Cancelled);
        assert!(job.frame().unwrap().is_none());
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
    }
}

#[test]
fn a_cancel_after_the_last_digest_chunk_still_wins_the_publication_lock() {
    let (host, job, mut running, _) = pending_digest_fixture();
    let pending = running.pending_png.as_mut().unwrap();
    while !pending.step(1, &job.0.cancel).unwrap() {}
    assert_eq!(pending.hashed, pending.png.len());
    job.cancel().unwrap();
    assert!(!running.guarded_step(1));
    assert_eq!(job.status().unwrap().progress.phase, Phase::Cancelled);
    assert!(job.frame().unwrap().is_none());
    drop(running);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn png_digest_metadata_fits_the_existing_allowance_without_changing_admission() {
    // Fixed digest/offset state and the completed frame (including its bounded
    // hex string and backend/executor IDs) fit the existing 1024-byte allowance.
    assert!(std::mem::size_of::<PendingPng>() <= 512);
    assert!(
        std::mem::size_of::<Running>() + std::mem::size_of::<RenderFrame>() + 64 + 2 * crate::MAX_RENDER_ID_BYTES + 32
            <= FRAME_METADATA_BYTES as usize
    );
    let host = CpuHost::cooperative();
    let expected = admit(&empty(), &request()).unwrap().estimated_total_bytes;
    let job = host.submit(id(70, 1), 1, empty(), request()).unwrap();
    assert_eq!(host.admitted_bytes(), expected);
    assert!(job.status().unwrap().frame.is_none());
    job.cancel().unwrap();
    drain(&host);
    assert_eq!(host.admitted_bytes(), 0);
}

/// An executor whose task reports, as it is dropped, how much of the pool is admitted, and
/// completes after `steps` steps (or never, cancelled on its last one with `cancel_last`).
struct DropProbe {
    admitted_at_drop: Arc<AtomicU64>,
    budget: Arc<std::sync::OnceLock<Arc<Budget>>>,
    estimated: u64,
}
struct ProbeTask {
    admitted_at_drop: Arc<AtomicU64>,
    budget: Arc<std::sync::OnceLock<Arc<Budget>>>,
    steps: u32,
}
impl Drop for ProbeTask {
    fn drop(&mut self) {
        let used = self.budget.get().map_or(u64::MAX, |b| b.used.load(Ordering::Acquire));
        self.admitted_at_drop.store(used, Ordering::Release);
    }
}
impl ImageWork for ProbeTask {
    fn step(&mut self, _units: usize) -> Result<TaskPoll> {
        self.steps += 1;
        if self.steps < 3 {
            return Ok(TaskPoll::Pending(Progress { phase: Phase::Running, done: u64::from(self.steps), total: 3 }));
        }
        Ok(TaskPoll::Complete(vec![0u8; 100]))
    }
}
impl ImageExecutor for DropProbe {
    fn descriptor(&self) -> Result<ExecutorDescriptor> {
        CpuExecutor.descriptor()
    }
    fn admit(&self, _: &ProductScene, _: &ImageRequest, _: JobOptions) -> Result<Admission> {
        Ok(Admission {
            triangles: 0,
            face_appearances: 0,
            projected_pixel_tests: None,
            geometry_bytes: 0,
            pixel_scratch_bytes: self.estimated,
            preparation_bytes: 0,
            estimated_total_bytes: self.estimated,
            encoded_limit: 4096,
        })
    }
    fn begin(&self, _: ProductScene, _: ImageRequest, _: JobOptions, _: CancelToken) -> Result<Box<dyn ImageWork>> {
        Ok(Box::new(ProbeTask { admitted_at_drop: self.admitted_at_drop.clone(), budget: self.budget.clone(), steps: 0 }))
    }
}

fn probe_host(estimated: u64) -> (CpuHost, Arc<AtomicU64>) {
    let admitted_at_drop = Arc::new(AtomicU64::new(0));
    let budget = Arc::new(std::sync::OnceLock::new());
    let host = CpuHost::cooperative_with(Arc::new(DropProbe { admitted_at_drop: admitted_at_drop.clone(), budget: budget.clone(), estimated }));
    let _ = budget.set(host.0.shared.budget.clone());
    (host, admitted_at_drop)
}

#[test]
fn a_completed_task_is_dropped_while_its_full_reservation_still_pays_for_it() {
    let estimated = 1 << 20;
    let (host, admitted_at_drop) = probe_host(estimated);
    let job = host.submit(id(1, 1), 1, empty(), request()).unwrap();
    drain(&host);
    assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
    // While the task (and its scratch) was being dropped, its whole charge was still held.
    assert_eq!(admitted_at_drop.load(Ordering::Acquire), estimated);
    // Afterwards only the PNG's lease remains, and the frame keeps it.
    let frame = job.frame().unwrap().unwrap();
    assert!(host.admitted_bytes() > 0 && host.admitted_bytes() < estimated, "{}", host.admitted_bytes());
    drop(job);
    drop(frame);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn a_cancel_on_the_final_step_publishes_no_frame_and_drops_the_task() {
    let estimated = 1 << 20;
    let (host, admitted_at_drop) = probe_host(estimated);
    let job = host.submit(id(1, 1), 1, empty(), request()).unwrap();
    // Two pending steps, then cancel before the step that would complete.
    host.pump(1).unwrap();
    host.pump(1).unwrap();
    job.cancel().unwrap();
    drain(&host);
    assert_eq!(job.status().unwrap().progress.phase, Phase::Cancelled);
    assert!(job.frame().unwrap().is_none());
    assert_eq!(admitted_at_drop.load(Ordering::Acquire), estimated, "the task went before its charge");
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn cpu_and_registered_hosts_keep_separate_job_limits() {
    let cpu = CpuHost::cooperative();
    let (registered, _) = probe_host(400 * 1024 * 1024);
    assert_eq!(cpu.job_limit_bytes(), 384 * 1024 * 1024);
    assert_eq!(registered.job_limit_bytes(), 512 * 1024 * 1024);
    assert_eq!(cpu.pool_limit_bytes(), registered.pool_limit_bytes());
    assert_eq!(registered.pool_limit_bytes(), 512 * 1024 * 1024);
    assert_eq!(registered.admit(&empty(), &request(), JobOptions::default()).unwrap().estimated_total_bytes, 400 * 1024 * 1024);
    assert_eq!(admit_request_sum(400 * 1024 * 1024, 0).unwrap_err().code, ErrorCode::LimitExceeded);
    let (over_budget, _) = probe_host(512 * 1024 * 1024 + 1);
    assert_eq!(over_budget.admit(&empty(), &request(), JobOptions::default()).unwrap_err().code, ErrorCode::LimitExceeded);
    assert_eq!(registered.admitted_bytes(), 0);
}

#[test]
fn registered_hosts_refuse_cpu_integrators_and_options_before_reservation() {
    let (registered, _) = probe_host(1024 * 1024);
    let mut raytrace = request();
    raytrace.integrator = Integrator::Raytrace;
    assert_eq!(registered.validate_integrator(Integrator::Raytrace).unwrap_err().code, ErrorCode::InvalidRequest);
    assert_eq!(registered.descriptor_for(Integrator::Raytrace).unwrap_err().code, ErrorCode::InvalidRequest);
    assert_eq!(registered.admit(&empty(), &raytrace, JobOptions::default()).unwrap_err().code, ErrorCode::InvalidRequest);
    assert_eq!(registered.submit(id(250, 1), 1, empty(), raytrace).err().unwrap().code, ErrorCode::InvalidRequest);
    let mut options = request();
    options.raytrace = Some(super::super::RaytraceOptions::default());
    assert_eq!(registered.admit(&empty(), &options, JobOptions::default()).unwrap_err().code, ErrorCode::InvalidRequest);
    assert_eq!(registered.admitted_bytes(), 0);
    assert!(!registered.pump(1).unwrap());
}

#[test]
fn environment_payload_is_owned_by_admitted_work_not_retained_status() {
    struct ResourceExecutor;
    impl ImageExecutor for ResourceExecutor {
        fn descriptor(&self) -> Result<ExecutorDescriptor> {
            let mut descriptor = CpuExecutor.descriptor()?;
            descriptor.profiles[0].features.push(crate::RenderFeature::Hdri);
            Ok(descriptor)
        }
        fn admit(&self, scene: &ProductScene, request: &ImageRequest, _: JobOptions) -> Result<Admission> {
            let mut admission = admit(scene, &ImageRequest { environment: None, textures: None, ..request.clone() })?;
            admission.estimated_total_bytes += request.environment.as_ref().map_or(0, |source| source.admitted_bytes());
            Ok(admission)
        }
        fn begin(&self, scene: ProductScene, mut request: ImageRequest, _: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
            request.environment = None;
            Ok(Box::new(ImageTask::new(scene, request, cancel)?))
        }
    }
    for cancelled in [false, true] {
        let host = ImageHost(Arc::new(Owner {
            executor: Some(Arc::new(ResourceExecutor)),
            shared: ImageHost::shared(MEMORY_LIMIT),
            kind: HostKind::Cooperative,
            running: Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            _worker: None,
        }));
        let source = Arc::new(crate::lighting::environment::EnvironmentSource::sky(Default::default(), 0.0, 1.0).unwrap());
        let weak = Arc::downgrade(&source);
        let mut input = request();
        input.environment = Some(source);
        let job = host.submit(id(1, 1), 1, empty(), input).unwrap();
        let status = job.status().unwrap();
        assert!(status.request.environment.is_none());
        assert!(matches!(status.environment.as_ref(), Some(JobEnvironmentInfo::Gpu(info)) if info.kind == "original_gradient_sky"));
        assert!(weak.upgrade().is_some(), "queuedwork owns source");
        if cancelled {
            job.cancel().unwrap();
        }
        drain(&host);
        assert!(weak.upgrade().is_none(), "completed/cancelled work releases captured source despite retained status/job");
        assert_eq!(job.status().unwrap().progress.phase, if cancelled { Phase::Cancelled } else { Phase::Completed });
    }
}

#[test]
fn texture_payload_is_owned_by_admitted_work_not_retained_status() {
    struct ResourceExecutor;
    impl ImageExecutor for ResourceExecutor {
        fn descriptor(&self) -> Result<ExecutorDescriptor> {
            let mut descriptor = CpuExecutor.descriptor()?;
            descriptor.profiles[0].features.push(crate::RenderFeature::Hdri);
            Ok(descriptor)
        }
        fn admit(&self, scene: &ProductScene, request: &ImageRequest, _: JobOptions) -> Result<Admission> {
            let mut admission = admit(scene, &ImageRequest { environment: None, textures: None, ..request.clone() })?;
            admission.estimated_total_bytes += request.textures.as_ref().map_or(0, |source| source.working_bytes());
            Ok(admission)
        }
        fn begin(&self, scene: ProductScene, mut request: ImageRequest, _: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
            request.textures = None;
            Ok(Box::new(ImageTask::new(scene, request, cancel)?))
        }
    }
    for cancelled in [false, true] {
        let host = ImageHost(Arc::new(Owner {
            executor: Some(Arc::new(ResourceExecutor)),
            shared: ImageHost::shared(MEMORY_LIMIT),
            kind: HostKind::Cooperative,
            running: Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            _worker: None,
        }));
        use crate::job::textures::{PlanarFrame, RuntimeTextures, TextureBinding, TextureMap};
        use image::ImageEncoder;
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes).write_image(&[128, 128, 255, 255], 1, 1, image::ExtendedColorType::Rgba8).unwrap();
        let source = Arc::new(
            RuntimeTextures::new(vec![TextureBinding {
                body_index: 0,
                body_name: "unused test resource".into(),
                frame: PlanarFrame { origin: [0.; 3], u: [1., 0., 0.], v: [0., 1., 0.] },
                normal_strength: 1.,
                base_color: Some(TextureMap::new(bytes, crate::texture_input::MapRole::BaseColor).unwrap()),
                normal: None,
                roughness: None,
                metallic: None,
                base_color_mode: Default::default(),
                flip_v: false,
            }])
            .unwrap(),
        );
        let weak = Arc::downgrade(&source);
        let mut input = request();
        input.textures = Some(source);
        let job = host.submit(id(1, 1), 1, empty(), input).unwrap();
        let status = job.status().unwrap();
        assert!(status.request.textures.is_none());
        assert_eq!(status.textures.as_ref().unwrap().bodies, 1);
        assert!(weak.upgrade().is_some(), "queuedwork owns source");
        if cancelled {
            job.cancel().unwrap();
        }
        drain(&host);
        assert!(weak.upgrade().is_none(), "completed/cancelled work releases captured source despite retained status/job");
        assert_eq!(job.status().unwrap().progress.phase, if cancelled { Phase::Cancelled } else { Phase::Completed });
    }
}

#[test]
fn cpu_film_snapshots_refuse_registered_executors_without_acquiring_a_lease() {
    let (host, _) = probe_host(1 << 20);
    let failure = host.reserve_film_snapshot(1024).err().unwrap();
    assert_eq!(failure.code, ErrorCode::InvalidRequest);
    assert!(failure.message.contains("builtin CPU host"));
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32"), target_pointer_width = "64"))]
#[test]
fn native_cpu_envelopes_do_not_expand_registered_or_cooperative_pools() {
    use super::super::{CpuBudget, NATIVE_CPU_POOL_BYTES, RaytraceOptions};
    let cpu = CpuHost::for_platform().unwrap();
    let cooperative = CpuHost::cooperative();
    let (registered, _) = probe_host(1024);
    assert_eq!(cpu.pool_limit_bytes(), NATIVE_CPU_POOL_BYTES);
    assert_eq!(cooperative.pool_limit_bytes(), 512 * 1024 * 1024);
    assert_eq!(registered.pool_limit_bytes(), 512 * 1024 * 1024);
    assert!(cpu.check_request_sum(400 * 1024 * 1024, 0, CpuBudget::Compact).is_err());
    assert!(cpu.check_request_sum(400 * 1024 * 1024, 0, CpuBudget::Studio).is_ok());
    assert!(cpu.check_request_sum(u64::MAX, 1, CpuBudget::Workstation).is_err());
    let mut r = request();
    r.integrator = Integrator::Raytrace;
    r.width = 3840;
    r.height = 2160;
    r.raytrace = Some(RaytraceOptions { budget: CpuBudget::Studio, ..Default::default() });
    assert_eq!(cpu.descriptor_for_request(&r).unwrap().limits, CpuBudget::Studio.image_limits(false));
    r.raytrace.as_mut().unwrap().budget = CpuBudget::Compact;
    assert!(cpu.descriptor_for_request(&r).is_err());
    assert_eq!(cpu.admitted_bytes(), 0);
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
#[test]
fn registered_large_pool_is_explicit_finite_and_accounted() {
    struct Envelope(u64);
    impl ImageExecutor for Envelope {
        fn memory_limit_bytes(&self) -> u64 {
            self.0
        }
        fn descriptor(&self) -> Result<ExecutorDescriptor> {
            CpuExecutor.descriptor()
        }
        fn admit(&self, _: &ProductScene, _: &ImageRequest, _: JobOptions) -> Result<Admission> {
            Ok(Admission {
                triangles: 0,
                face_appearances: 0,
                projected_pixel_tests: None,
                geometry_bytes: 0,
                pixel_scratch_bytes: 0,
                preparation_bytes: 0,
                estimated_total_bytes: 600 * 1024 * 1024,
                encoded_limit: 4096,
            })
        }
        fn begin(&self, _: ProductScene, _: ImageRequest, _: JobOptions, _: CancelToken) -> Result<Box<dyn ImageWork>> {
            Err(JobError::new(ErrorCode::InvalidRequest, "admission-only fixture"))
        }
    }
    for limit in [0, super::super::NATIVE_CPU_POOL_BYTES + 1] {
        assert!(ImageHost::with_executor(Arc::new(Envelope(limit))).is_err());
    }
    let host = ImageHost::with_executor(Arc::new(Envelope(1024 * 1024 * 1024))).unwrap();
    assert_eq!(host.pool_limit_bytes(), 1024 * 1024 * 1024);
    assert!(host.admit(&empty(), &request(), JobOptions::default()).is_ok());
    assert_eq!(host.admitted_bytes(), 0, "preflight does not allocate or reserve");
    let lease = host.0.shared.budget.reserve(600 * 1024 * 1024).unwrap();
    assert!(host.0.shared.budget.reserve(600 * 1024 * 1024).is_err());
    drop(lease);
    assert_eq!(host.admitted_bytes(), 0);
}
